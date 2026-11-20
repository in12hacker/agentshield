//! One original committed chat input at a time, without a serialized envelope or RAM history.
use super::{BusinessLog, StoreError, chat, parts, relations, row, source};
use crate::context::FactReference;
use as_types::flow::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;

fn cursor(runtime: Vec<u8>, sequence: Vec<u8>) -> Result<ChatCursor, StoreError> {
    Ok(ChatCursor {
        service_instance: runtime.try_into().map_err(|_| StoreError::Invalid)?,
        sequence: parts::u64_blob(sequence)?,
    })
}
fn position(conn: &Connection, after: ChatCursor) -> Result<i64, StoreError> {
    conn.query_row("SELECT i.delivery FROM chat_inputs i JOIN deliveries d ON d.id=i.delivery WHERE d.cursor_runtime=?1 AND d.cursor_sequence=?2", params![after.service_instance.as_slice(),after.sequence.to_be_bytes().as_slice()], |r|r.get(0)).optional()?.ok_or(StoreError::NotProvided)
}
fn event(conn: &Connection, kind: &str, id: i64) -> Result<ChatEvent, StoreError> {
    if id <= 0 {
        return Err(StoreError::Invalid);
    }
    Ok(match kind {
        "observation" => ChatEvent::Observation(Arc::new(chat::read_protocol(conn, id)?)),
        "started" => ChatEvent::Started(Arc::new(chat::read_revision(conn, id)?)),
        "finished" => ChatEvent::Finished(Arc::new(chat::read_revision(conn, id)?)),
        "delta" => {
            let (reference, base, revision) = conn.query_row(
                "SELECT message_ref,base_revision,revision FROM deltas WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )?;
            let FactReference::Message(message) = serde_json::from_str(&reference)? else {
                return Err(StoreError::Invalid);
            };
            ChatEvent::Delta {
                id: message,
                base_revision: parts::u64_blob(base)?,
                revision: parts::u64_blob(revision)?,
                changes: chat::read_changes(conn, id)?,
            }
        }
        "credential_use" => {
            let (flow,exchange,sequence,revision,actor,binding,destination,state,field,reason,time)=conn.query_row("SELECT flow_instance,exchange_sequence,sequence,revision,actor,binding,destination_origin,state,auth_field,reason,observed_boottime_ns FROM credential_facts WHERE id=?1",[id],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,i64>(4)?,r.get::<_,Option<Vec<u8>>>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,Option<String>>(9)?,r.get::<_,Vec<u8>>(10)?)))?;
            ChatEvent::CredentialUse(Arc::new(CredentialUseFact {
                id: CredentialUseId {
                    exchange: ExchangeKey {
                        flow_instance: flow.try_into().map_err(|_| StoreError::Invalid)?,
                        sequence: parts::u64_blob(exchange)?,
                    },
                    sequence: parts::u64_blob(sequence)?,
                },
                revision: parts::u64_blob(revision)?,
                actor: source::read_actor(conn, actor)?,
                binding_ref: binding
                    .map(|b| b.try_into().map(BindingId).map_err(|_| StoreError::Invalid))
                    .transpose()?,
                destination_origin: destination,
                state: row::named(&state)?,
                auth_field: field,
                reason,
                observed_boottime_ns: parts::u64_blob(time)?,
            }))
        }
        "rule_feedback" => ChatEvent::RuleFeedback(relations::read_feedback(conn, id)?),
        "exchange_failed" => {
            let (flow,sequence,http_status,reason)=conn.query_row("SELECT flow_instance,exchange_sequence,http_status,reason FROM exchange_failures WHERE id=?1",[id],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,Option<u16>>(2)?,r.get::<_,String>(3)?)))?;
            ChatEvent::ExchangeFailed {
                exchange: ExchangeKey {
                    flow_instance: flow.try_into().map_err(|_| StoreError::Invalid)?,
                    sequence: parts::u64_blob(sequence)?,
                },
                http_status,
                reason,
            }
        }
        "gap" => ChatEvent::Gap(read_gap(conn, id)?),
        "status" => {
            let (intake,mode,active,lost,failed)=conn.query_row("SELECT intake_state,mode,active_exchanges,lost_deliveries,parse_failures FROM flow_status WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,Vec<u8>>(4)?)))?;
            let mut profiles = conn
                .prepare("SELECT profile FROM status_profiles WHERE owner=?1 ORDER BY ordinal")?;
            let decoder_profiles = profiles
                .query_map([id], |r| r.get::<_, String>(0))?
                .map(|v| row::named(&v?))
                .collect::<Result<_, StoreError>>()?;
            ChatEvent::Status(FlowStatus {
                intake_state: row::named(&intake)?,
                mode: row::named(&mode)?,
                decoder_profiles,
                active_exchanges: parts::u64_blob(active)?,
                lost_deliveries: parts::u64_blob(lost)?,
                parse_failures: parts::u64_blob(failed)?,
            })
        }
        _ => return Err(StoreError::Invalid),
    })
}

pub(super) fn read_gap(conn: &Connection, id: i64) -> Result<ObservationGap, StoreError> {
    let (kind,flow,exchange,sr,ss,er,es,reason)=conn.query_row("SELECT kind,flow_instance,exchange_sequence,start_runtime,start_sequence,end_runtime,end_sequence,reason FROM source_gaps WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<Vec<u8>>>(1)?,r.get::<_,Option<Vec<u8>>>(2)?,r.get::<_,Option<Vec<u8>>>(3)?,r.get::<_,Option<Vec<u8>>>(4)?,r.get::<_,Option<Vec<u8>>>(5)?,r.get::<_,Option<Vec<u8>>>(6)?,r.get::<_,String>(7)?)))?;
    let exchange = match (flow, exchange) {
        (None, None) => None,
        (Some(f), Some(s)) => Some(ExchangeKey {
            flow_instance: f.try_into().map_err(|_| StoreError::Invalid)?,
            sequence: parts::u64_blob(s)?,
        }),
        _ => return Err(StoreError::Invalid),
    };
    let cursor_range = match (sr, ss, er, es) {
        (None, None, None, None) => None,
        (Some(sr), Some(ss), Some(er), Some(es)) => Some((cursor(sr, ss)?, cursor(er, es)?)),
        _ => return Err(StoreError::Invalid),
    };
    Ok(ObservationGap {
        kind,
        exchange,
        cursor_range,
        reason,
    })
}

impl BusinessLog {
    /// Last committed chat input in this database's intake order, not the greatest source sequence.
    pub async fn chat_tail(&self) -> Result<Option<ChatCursor>, StoreError> {
        self.call(|conn|{
            let value=conn.query_row("SELECT d.cursor_runtime,d.cursor_sequence FROM chat_inputs i JOIN deliveries d ON d.id=i.delivery ORDER BY i.delivery DESC LIMIT 1",[],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?))).optional()?;
            value.map(|(runtime,sequence)|cursor(runtime,sequence)).transpose()
        })?.await
    }
    /// Verify an original source cursor exists in this exact fixed database, without reading bodies.
    pub async fn validate_chat_cursor(&self, after: ChatCursor) -> Result<(), StoreError> {
        self.call(move |conn| position(conn, after).map(|_| ()))?
            .await
    }
    /// Rebuild at most one original event after this exact source cursor's committed input position.
    /// None means no next committed input; it does not claim source EOF.
    pub async fn read_next_chat(
        &self,
        after: Option<ChatCursor>,
    ) -> Result<Option<Arc<ChatEnvelope>>, StoreError> {
        self.call(move|conn|{
            let tx=conn.transaction()?;
            let fields="SELECT i.event_kind,i.payload_row,d.cursor_runtime,d.cursor_sequence FROM chat_inputs i JOIN deliveries d ON d.id=i.delivery";
            let read=|r:&rusqlite::Row<'_>|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?));
            let row=match after{
                Some(after)=>tx.query_row(&format!("{fields} WHERE i.delivery>?1 ORDER BY i.delivery LIMIT 1"),[position(&tx,after)?],read).optional()?,
                None=>tx.query_row(&format!("{fields} ORDER BY i.delivery LIMIT 1"),[],read).optional()?,
            };
            let value=row.map(|(kind,payload,runtime,sequence)|Ok::<_,StoreError>(Arc::new(ChatEnvelope{cursor:cursor(runtime,sequence)?,event:Arc::new(event(&tx,&kind,payload)?)}))).transpose()?;
            tx.commit()?;
            Ok(value)
        })?.await
    }
    /// Register a coalesced commit wake now; no event or cursor is cached in this notification.
    pub fn chat_changed(&self) -> impl std::future::Future<Output = ()> + Send + 'static + use<> {
        self.inner.chat_changed.clone().notified_owned()
    }
}
