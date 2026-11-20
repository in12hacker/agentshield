//! Structured chat intake; all source bodies live in SQLite, not a replay/snapshot map.
use super::{BusinessLog, StoreError, StoredReceipt, group, parts, row, schema, source};
use crate::context::FactReference;
use as_types::flow::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn identity(
    conn: &Connection,
    table: &str,
    flow: &[u8],
    exchange: u64,
    sequence: u64,
) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(&format!("SELECT id FROM {table} WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3"),params![flow,exchange.to_be_bytes().as_slice(),sequence.to_be_bytes().as_slice()],|r|r.get(0)).optional()?)
}
pub(super) fn write_protocol(
    conn: &Connection,
    p: &ProtocolObservation,
) -> Result<i64, StoreError> {
    if let Some(id) = identity(
        conn,
        "protocol_records",
        &p.id.exchange.flow_instance,
        p.id.exchange.sequence,
        p.id.sequence,
    )? {
        return if read_protocol(conn, id)? == *p {
            Ok(id)
        } else {
            Err(StoreError::Conflict)
        };
    }
    let mut value = source::id_fields(&p.id)?;
    value.insert(
        "source".into(),
        row::integer(source::write(conn, &p.source)?),
    );
    value.insert(
        "body".into(),
        row::integer(parts::write_evidence(conn, &p.body)?),
    );
    value.insert("unit".into(), json!(p.unit));
    value.insert("event_name".into(), json!(p.event_name));
    let id = row::put(conn, "protocol_records", schema::PROTOCOL, value)?;
    parts::write_native(conn, "protocol_native_ids", id, &p.native_ids)?;
    parts::quality(conn, "record_quality", id, &p.quality)?;
    parts::reference(conn, &FactReference::Protocol(p.id), "protocol", id)?;
    super::sessions::write(conn, id, p.request_session.as_ref())?;
    group::index_openclaw_receipts(conn, id, p)?;
    Ok(id)
}
pub(super) fn read_protocol(conn: &Connection, id: i64) -> Result<ProtocolObservation, StoreError> {
    read_protocol_range(conn, id, None).map(|(observation, _)| observation)
}

pub(super) fn read_protocol_range(
    conn: &Connection,
    id: i64,
    range: Option<std::ops::Range<u64>>,
) -> Result<(ProtocolObservation, Option<u64>), StoreError> {
    let mut value = row::get(conn, "protocol_records", schema::PROTOCOL, id)?;
    let src = row::take(&mut value, "source")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let body = row::take(&mut value, "body")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let unit = row::decode(row::take(&mut value, "unit")?)?;
    let event_name = row::decode(row::take(&mut value, "event_name")?)?;
    let (body, next_offset) = parts::read_evidence_range(conn, body, range)?;
    Ok((
        ProtocolObservation {
            id: row::decode(source::id_value(value)?)?,
            source: Arc::new(source::read(conn, src)?),
            unit,
            event_name,
            body,
            native_ids: parts::read_native(conn, "protocol_native_ids", id)?,
            quality: parts::read_quality(conn, "record_quality", id)?,
            request_session: super::sessions::read(conn, id)?,
        },
        next_offset,
    ))
}
pub(super) fn write_message_revision(
    conn: &Connection,
    m: &ChatMessage,
    delivery: i64,
) -> Result<i64, StoreError> {
    let existing = identity(
        conn,
        "messages",
        &m.id.exchange.flow_instance,
        m.id.exchange.sequence,
        m.id.sequence,
    )?;
    let owner = if let Some(id) = existing {
        let origin: String =
            conn.query_row("SELECT origin FROM messages WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        if origin != row::string(&m.origin)? {
            return Err(StoreError::Conflict);
        }
        id
    } else {
        let mut value = source::id_fields(&m.id)?;
        value.insert("origin".into(), json!(m.origin));
        value.insert("first_delivery".into(), json!(delivery));
        row::put(conn, "messages", schema::MESSAGE, value)?
    };
    let previous = conn
        .query_row(
            "SELECT id FROM message_revisions WHERE message=?1 AND revision=?2",
            params![owner, m.revision.to_be_bytes().as_slice()],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(id) = previous {
        return if read_revision(conn, id)? == *m {
            Ok(id)
        } else {
            Err(StoreError::Conflict)
        };
    }
    let role = row::object(&m.role)?;
    let value = row::object(
        &json!({"message":owner,"revision":m.revision.to_string(),"source":source::write(conn,&m.source)?,"role_kind":role.get("kind").ok_or(StoreError::Invalid)?,"role_value":role.get("value"),"candidate_index":m.candidate_index,"model":m.model,"state":m.state,"provider_status":m.provider_status,"finish_reason":m.finish_reason,"first_seen_boottime_ns":m.first_seen_boottime_ns.to_string(),"last_seen_boottime_ns":m.last_seen_boottime_ns.to_string(),"usage_present":m.usage.is_some(),"parts":parts::write(conn,&m.observed_parts)?,"derived":false,"delivery":delivery}),
    )?;
    let revision = row::put(conn, "message_revisions", schema::REVISION, value)?;
    for (n, reference) in m.record_refs.iter().enumerate() {
        conn.execute(
            "INSERT INTO message_record_refs VALUES(?1,?2,?3,?4,?5)",
            params![
                revision,
                n as i64,
                reference.exchange.flow_instance.as_slice(),
                reference.exchange.sequence.to_be_bytes().as_slice(),
                reference.sequence.to_be_bytes().as_slice()
            ],
        )?;
    }
    parts::quality(conn, "message_quality", revision, &m.quality)?;
    if let Some(usage) = &m.usage {
        for (n, counter) in usage.iter().enumerate() {
            conn.execute(
                "INSERT INTO usage VALUES(?1,?2,?3,?4)",
                params![
                    revision,
                    n as i64,
                    counter.name,
                    counter.value.to_be_bytes().as_slice()
                ],
            )?;
        }
    }
    parts::reference(conn, &FactReference::Message(m.id), "message", owner)?;
    for part in &m.observed_parts {
        parts::reference(
            conn,
            &FactReference::Part(ChatPartRef {
                message: m.id,
                part_id: crate::activity::part_id(part),
            }),
            "message",
            owner,
        )?;
    }
    Ok(revision)
}
fn u64_value(value: Value) -> Result<u64, StoreError> {
    value
        .as_str()
        .ok_or(StoreError::Invalid)?
        .parse()
        .map_err(|_| StoreError::Invalid)
}
pub(super) fn read_revision(conn: &Connection, id: i64) -> Result<ChatMessage, StoreError> {
    let mut v = row::get(conn, "message_revisions", schema::REVISION, id)?;
    let owner = row::take(&mut v, "message")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let mut base = row::get(conn, "messages", schema::MESSAGE, owner)?;
    let origin = row::decode(row::take(&mut base, "origin")?)?;
    let _ = row::take(&mut base, "first_delivery")?;
    let source_id = row::take(&mut v, "source")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let part_set = row::take(&mut v, "parts")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let usage_present = row::take(&mut v, "usage_present")?
        .as_bool()
        .ok_or(StoreError::Invalid)?;
    let mut role = json!({"kind":row::take(&mut v,"role_kind")?});
    let role_value = row::take(&mut v, "role_value")?;
    if !role_value.is_null() {
        role["value"] = role_value;
    }
    let mut stmt=conn.prepare("SELECT flow_instance,exchange_sequence,sequence FROM message_record_refs WHERE owner=?1 ORDER BY ordinal")?;
    let record_refs = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?
        .map(|r| {
            let (f, e, s) = r?;
            Ok(ProtocolRecordId {
                exchange: ExchangeKey {
                    flow_instance: f.try_into().map_err(|_| StoreError::Invalid)?,
                    sequence: parts::u64_blob(e)?,
                },
                sequence: parts::u64_blob(s)?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let usage = if usage_present {
        let mut stmt =
            conn.prepare("SELECT name,value FROM usage WHERE owner=?1 ORDER BY ordinal")?;
        Some(
            stmt.query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .map(|r| {
                let (name, value) = r?;
                Ok(UsageCounter {
                    name,
                    value: parts::u64_blob(value)?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?,
        )
    } else {
        None
    };
    Ok(ChatMessage {
        id: row::decode(source::id_value(base)?)?,
        revision: u64_value(row::take(&mut v, "revision")?)?,
        source: Arc::new(source::read(conn, source_id)?),
        record_refs,
        origin,
        role: row::decode(role)?,
        candidate_index: row::decode(row::take(&mut v, "candidate_index")?)?,
        model: row::decode(row::take(&mut v, "model")?)?,
        observed_parts: parts::read(conn, part_set)?,
        state: row::decode(row::take(&mut v, "state")?)?,
        quality: parts::read_quality(conn, "message_quality", id)?,
        provider_status: row::decode(row::take(&mut v, "provider_status")?)?,
        finish_reason: row::decode(row::take(&mut v, "finish_reason")?)?,
        usage,
        first_seen_boottime_ns: u64_value(row::take(&mut v, "first_seen_boottime_ns")?)?,
        last_seen_boottime_ns: u64_value(row::take(&mut v, "last_seen_boottime_ns")?)?,
    })
}
pub(super) fn read_message(conn: &Connection, owner: i64) -> Result<ChatMessage, StoreError> {
    read_message_at(conn, owner, i64::MAX)
}

/// Locate the last user projection and its original intake position before the tool message.
pub(super) struct RequestUserRow {
    pub owner: i64,
    pub reference: ChatMessageId,
    pub first_delivery: i64,
    pub cutoff: i64,
}

/// Select by the original role and order. The query then verifies the retained revision.
pub(super) fn last_request_user(
    conn: &Connection,
    call: ChatMessageId,
) -> Result<Option<RequestUserRow>, StoreError> {
    let cutoff = conn.query_row(
        "SELECT first_delivery-1 FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3",
        params![call.exchange.flow_instance.as_slice(), call.exchange.sequence.to_be_bytes().as_slice(), call.sequence.to_be_bytes().as_slice()],
        |row| row.get::<_, i64>(0),
    )?;
    let original = conn
        .query_row(
            "SELECT m.id,m.first_delivery,m.sequence FROM messages m
             JOIN message_revisions r ON r.message=m.id AND r.delivery=m.first_delivery
             WHERE m.flow_instance=?1 AND m.exchange_sequence=?2 AND m.origin='request_context'
               AND m.first_delivery<=?3 AND r.role_kind='user'
             ORDER BY m.first_delivery DESC LIMIT 1",
            params![
                call.exchange.flow_instance.as_slice(),
                call.exchange.sequence.to_be_bytes().as_slice(),
                cutoff
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    original
        .map(|(owner, first_delivery, sequence)| {
            Ok(RequestUserRow {
                owner,
                reference: ChatMessageId {
                    exchange: call.exchange,
                    sequence: parts::u64_blob(sequence)?,
                },
                first_delivery,
                cutoff,
            })
        })
        .transpose()
}

/// Read the provenance of a complete JSON observation published before this projection.
pub(super) fn request_provenance(
    conn: &Connection,
    record: ProtocolRecordId,
    before: i64,
) -> Result<Option<ProtocolSource>, StoreError> {
    let Some(id) = identity(
        conn,
        "protocol_records",
        &record.exchange.flow_instance,
        record.exchange.sequence,
        record.sequence,
    )?
    else {
        return Ok(None);
    };
    let source = conn.query_row(
        "SELECT p.source FROM protocol_records p
         JOIN evidence_sets e ON e.id=p.body
         JOIN evidence_segments s ON s.set_id=e.id AND s.ordinal=0
         WHERE p.id=?1 AND p.unit='http_json_body'
           AND s.kind='bytes' AND s.bytes IS NOT NULL AND s.offset=X'0000000000000000'
           AND hex(e.original_length)=printf('%016X',length(s.bytes))
           AND NOT EXISTS (SELECT 1 FROM evidence_segments extra WHERE extra.set_id=e.id AND extra.ordinal<>0)
           AND NOT EXISTS (SELECT 1 FROM record_quality q WHERE q.owner=p.id)
           AND EXISTS (SELECT 1 FROM chat_inputs i
                       WHERE i.event_kind='observation' AND i.payload_row=p.id AND i.delivery<?2)",
        params![id, before],
        |row| row.get::<_, i64>(0),
    ).optional()?;
    source.map(|id| source::read(conn, id)).transpose()
}

pub(super) fn read_message_at(
    conn: &Connection,
    owner: i64,
    cutoff: i64,
) -> Result<ChatMessage, StoreError> {
    let revision=conn.query_row("SELECT id FROM message_revisions WHERE message=?1 AND delivery<=?2 ORDER BY revision DESC LIMIT 1",params![owner,cutoff],|r|r.get::<_,i64>(0))?;
    let mut message = read_revision(conn, revision)?;
    let mut stmt=conn.prepare("SELECT id,base_revision,revision FROM deltas WHERE message_ref=?1 AND delivery<=?2 ORDER BY revision")?;
    let rows = stmt.query_map(
        params![super::key(&FactReference::Message(message.id))?, cutoff],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        },
    )?;
    for entry in rows {
        let (id, base, new) = entry?;
        let (base, new) = (parts::u64_blob(base)?, parts::u64_blob(new)?);
        if new <= message.revision {
            continue;
        }
        if base != message.revision {
            return Err(StoreError::NotProvided);
        }
        for change in read_changes(conn, id)? {
            match change {
                ChatDelta::AppendText { part_id, text } => {
                    let target = message
                        .observed_parts
                        .iter_mut()
                        .find(|p| crate::activity::part_id(p) == part_id)
                        .ok_or(StoreError::NotProvided)?;
                    let ChatPart::Text { text: value, .. } = target else {
                        return Err(StoreError::Conflict);
                    };
                    value.push_str(&text);
                }
                ChatDelta::PutPart { part } => {
                    if let Some(target) = message
                        .observed_parts
                        .iter_mut()
                        .find(|p| crate::activity::part_id(p) == crate::activity::part_id(&part))
                    {
                        *target = part;
                    } else {
                        message.observed_parts.push(part);
                    }
                }
            }
        }
        message.revision = new;
    }
    Ok(message)
}

pub(super) fn read_changes(conn: &Connection, id: i64) -> Result<Vec<ChatDelta>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT kind,part_id,text,parts FROM delta_changes WHERE owner=?1 ORDER BY ordinal",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<u32>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<i64>>(3)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (kind, part, text, set) = row?;
        result.push(match (kind.as_str(), part, text, set) {
            ("append_text", Some(part_id), Some(text), None) => {
                ChatDelta::AppendText { part_id, text }
            }
            ("put_part", None, None, Some(set)) => {
                let mut parts = parts::read(conn, set)?;
                if parts.len() != 1 {
                    return Err(StoreError::Invalid);
                }
                ChatDelta::PutPart {
                    part: parts.remove(0),
                }
            }
            _ => return Err(StoreError::Invalid),
        });
    }
    Ok(result)
}

impl BusinessLog {
    /// Commit one original Flow input and its exact tagged payload reference before waking readers.
    pub async fn record_chat(
        &self,
        envelope: Arc<ChatEnvelope>,
    ) -> Result<StoredReceipt, StoreError> {
        let changed = self.inner.chat_changed.clone();
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            let delivery = super::record(&tx, "chat")?;
            tx.execute("UPDATE deliveries SET cursor_runtime=?1,cursor_sequence=?2 WHERE id=?3",params![envelope.cursor.service_instance.as_slice(),envelope.cursor.sequence.to_be_bytes().as_slice(),delivery])?;
            let (kind, payload) = match envelope.event.as_ref() {
                ChatEvent::Observation(p) => ("observation", write_protocol(&tx, p)?),
                ChatEvent::Started(m) => ("started", write_message_revision(&tx, m, delivery)?),
                ChatEvent::Finished(m) => ("finished", write_message_revision(&tx, m, delivery)?),
                ChatEvent::Delta { id, base_revision, revision, changes } => {
                    tx.execute("INSERT INTO deltas(delivery,message_ref,base_revision,revision) VALUES(?1,?2,?3,?4)",params![delivery,super::key(&FactReference::Message(*id))?,base_revision.to_be_bytes().as_slice(),revision.to_be_bytes().as_slice()])?;
                    let delta = tx.last_insert_rowid();
                    for (n, change) in changes.iter().enumerate() { match change {
                        ChatDelta::AppendText { part_id, text } => {tx.execute("INSERT INTO delta_changes VALUES(?1,?2,'append_text',?3,?4,NULL)",params![delta,n as i64,part_id,text])?;},
                        ChatDelta::PutPart { part } => {let set=parts::write(&tx,std::slice::from_ref(part))?;tx.execute("INSERT INTO delta_changes VALUES(?1,?2,'put_part',NULL,NULL,?3)",params![delta,n as i64,set])?;},
                    }}
                    ("delta", delta)
                }
                ChatEvent::CredentialUse(value) => {
                    tx.execute("INSERT INTO credential_facts(delivery,flow_instance,exchange_sequence,sequence,revision,actor,binding,destination_origin,state,auth_field,reason,observed_boottime_ns) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![delivery,value.id.exchange.flow_instance.as_slice(),value.id.exchange.sequence.to_be_bytes().as_slice(),value.id.sequence.to_be_bytes().as_slice(),value.revision.to_be_bytes().as_slice(),source::write_actor(&tx,&value.actor)?,value.binding_ref.as_ref().map(|v|v.0.as_slice()),value.destination_origin,row::string(&value.state)?,value.auth_field,value.reason,value.observed_boottime_ns.to_be_bytes().as_slice()])?;
                    ("credential_use", tx.last_insert_rowid())
                }
                ChatEvent::RuleFeedback(value) => ("rule_feedback", super::relations::feedback(&tx, delivery, value)?),
                ChatEvent::ExchangeFailed { exchange, http_status, reason } => {
                    tx.execute("INSERT INTO exchange_failures(delivery,flow_instance,exchange_sequence,http_status,reason) VALUES(?1,?2,?3,?4,?5)",params![delivery,exchange.flow_instance.as_slice(),exchange.sequence.to_be_bytes().as_slice(),http_status,reason])?;
                    ("exchange_failed", tx.last_insert_rowid())
                }
                ChatEvent::Gap(value) => ("gap", write_gap(&tx, delivery, value)?),
                ChatEvent::Status(status) => {
                    tx.execute("INSERT INTO flow_status(delivery,intake_state,mode,active_exchanges,lost_deliveries,parse_failures) VALUES(?1,?2,?3,?4,?5,?6)",params![delivery,row::string(&status.intake_state)?,row::string(&status.mode)?,status.active_exchanges.to_be_bytes().as_slice(),status.lost_deliveries.to_be_bytes().as_slice(),status.parse_failures.to_be_bytes().as_slice()])?;
                    let owner = tx.last_insert_rowid();
                    for (n, profile) in status.decoder_profiles.iter().enumerate() {tx.execute("INSERT INTO status_profiles VALUES(?1,?2,?3)",params![owner,n as i64,row::string(profile)?])?;}
                    ("status", owner)
                }
            };
            tx.execute("INSERT INTO chat_inputs VALUES(?1,?2,?3)",params![delivery,kind,payload])?;
            if kind=="observation" { super::sessions::register_model(&tx,delivery,payload)?; }
            super::relations::derive_chat(&tx, envelope.event.as_ref())?;
            tx.commit()?;
            changed.notify_waiters();
            Ok(StoredReceipt { record: delivery })
        })?.await
    }
}
fn write_gap(conn: &Connection, delivery: i64, gap: &ObservationGap) -> Result<i64, StoreError> {
    let start = gap.cursor_range.as_ref().map(|v| v.0);
    let end = gap.cursor_range.as_ref().map(|v| v.1);
    conn.execute("INSERT INTO source_gaps(delivery,kind,flow_instance,exchange_sequence,start_runtime,start_sequence,end_runtime,end_sequence,reason) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![delivery,gap.kind,gap.exchange.as_ref().map(|v|v.flow_instance.as_slice()),gap.exchange.map(|v|v.sequence.to_be_bytes().to_vec()),start.as_ref().map(|v|v.service_instance.as_slice()),start.map(|v|v.sequence.to_be_bytes().to_vec()),end.as_ref().map(|v|v.service_instance.as_slice()),end.map(|v|v.sequence.to_be_bytes().to_vec()),gap.reason])?;
    Ok(conn.last_insert_rowid())
}
