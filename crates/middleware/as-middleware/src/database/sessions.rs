//! Application sessions are SQL views of original selected fields, not another history owner.
use super::group_gateway;
use super::{BusinessLog, StoreError, row, source};
use crate::business_log::sessions::*;
use crate::gateway::{GatewayConnection, GatewayDocument, GatewayFieldReference, GatewayReference};
use as_types::flow::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;

pub(super) fn write(
    conn: &Connection,
    owner: i64,
    session: Option<&SessionObservation>,
) -> Result<(), StoreError> {
    let Some(session) = session else {
        return Ok(());
    };
    let (kind, field) = match &session.field {
        SessionField::Header { name } => ("header", name),
        SessionField::JsonPointer { pointer } => ("json_pointer", pointer),
    };
    let issue = session.issue.as_ref().map(row::string).transpose()?;
    conn.execute(
        "INSERT INTO protocol_session_fields VALUES(?1,?2,?3,?4)",
        params![owner, kind, field, issue],
    )?;
    for (ordinal, value) in session.values.iter().enumerate() {
        let (kind, bytes) = match value {
            SessionValue::Header(bytes) => ("header", bytes.as_slice()),
            SessionValue::Json(NativeIdValue::JsonString(value)) => {
                ("json_string", value.as_bytes())
            }
            SessionValue::Json(NativeIdValue::JsonNumber(value)) => {
                ("json_number", value.as_bytes())
            }
        };
        conn.execute(
            "INSERT INTO protocol_session_values VALUES(?1,?2,?3,?4)",
            params![owner, ordinal as i64, kind, bytes],
        )?;
    }
    Ok(())
}

pub(super) fn read(
    conn: &Connection,
    owner: i64,
) -> Result<Option<SessionObservation>, StoreError> {
    let field = conn
        .query_row(
            "SELECT kind,field,issue FROM protocol_session_fields WHERE owner=?1",
            [owner],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((kind, field, issue)) = field else {
        return Ok(None);
    };
    let field = match kind.as_str() {
        "header" => SessionField::Header { name: field },
        "json_pointer" => SessionField::JsonPointer { pointer: field },
        _ => return Err(StoreError::Invalid),
    };
    let mut stmt = conn.prepare(
        "SELECT kind,bytes FROM protocol_session_values WHERE owner=?1 ORDER BY ordinal",
    )?;
    let values = stmt
        .query_map([owner], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .map(|r| {
            let (kind, bytes) = r?;
            Ok(match kind.as_str() {
                "header" => SessionValue::Header(bytes),
                "json_string" => SessionValue::Json(NativeIdValue::JsonString(
                    String::from_utf8(bytes).map_err(|_| StoreError::Invalid)?,
                )),
                "json_number" => SessionValue::Json(NativeIdValue::JsonNumber(
                    String::from_utf8(bytes).map_err(|_| StoreError::Invalid)?,
                )),
                _ => return Err(StoreError::Invalid),
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(Some(SessionObservation {
        field,
        values,
        issue: issue.as_deref().map(row::named).transpose()?,
    }))
}

// All domain columns are non-NULL here. Process row IDs are not identities: compare actual lifetimes.
pub(super) const DOMAIN: &str = "boot_id,host_tgid,birth_boottime_ns,origin,profile,route_label,config_version,actual_auth_scope,field_kind,field,value_kind,value_bytes";
pub(super) const VALID: &str = "valid AS (
 SELECT p.id,p.source,p.flow_instance,p.exchange_sequence,p.sequence,
 k.boot_id,k.host_tgid,k.birth_boottime_ns,s.origin,s.profile,s.route_label,s.config_version,s.actual_auth_scope,
 f.kind AS field_kind,f.field,v.kind AS value_kind,v.bytes AS value_bytes
 FROM protocol_session_fields f JOIN protocol_session_values v ON v.owner=f.owner AND v.ordinal=0
 JOIN protocol_records p ON p.id=f.owner JOIN protocol_sources s ON s.id=p.source
 JOIN actors a ON a.id=s.actor JOIN process_keys k ON k.id=a.process
 WHERE f.issue IS NULL AND length(v.bytes)>0 AND s.side='client_request'
 AND p.unit='http_json_body' AND s.actual_auth_scope IS NOT NULL
 AND k.boot_id IS NOT NULL AND k.host_tgid IS NOT NULL AND k.birth_boottime_ns IS NOT NULL
 AND s.origin IS NOT NULL AND s.profile IS NOT NULL AND s.route_label IS NOT NULL AND s.config_version IS NOT NULL
 AND ((f.kind='header' AND v.kind='header') OR (f.kind='json_pointer' AND v.kind IN ('json_string','json_number')))
 AND NOT EXISTS(SELECT 1 FROM protocol_session_values vv WHERE vv.owner=f.owner AND vv.ordinal<>0)
 AND NOT EXISTS(SELECT 1 FROM record_quality q WHERE q.owner=p.id)
)";

pub(super) fn resolve(conn: &Connection, reference: ProtocolRecordId) -> Result<i64, StoreError> {
    conn.query_row(&format!("WITH {VALID} SELECT id FROM valid WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3"),
        params![reference.exchange.flow_instance.as_slice(),reference.exchange.sequence.to_be_bytes().as_slice(),reference.sequence.to_be_bytes().as_slice()], |r| r.get(0))
        .optional()?.ok_or(StoreError::NotProvided)
}

pub(super) fn contains_exchange(
    conn: &Connection,
    session: i64,
    exchange: ExchangeKey,
) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        &format!(
            "WITH {VALID} SELECT EXISTS(SELECT 1 FROM valid v JOIN valid a USING ({DOMAIN})
        WHERE a.id=?1 AND v.flow_instance=?2 AND v.exchange_sequence=?3)"
        ),
        params![
            session,
            exchange.flow_instance.as_slice(),
            exchange.sequence.to_be_bytes().as_slice()
        ],
        |r| r.get(0),
    )?)
}

pub(super) fn reference(conn: &Connection, id: i64) -> Result<ProtocolRecordId, StoreError> {
    conn.query_row(
        "SELECT flow_instance,exchange_sequence,sequence FROM protocol_records WHERE id=?1",
        [id],
        |r| {
            Ok(ProtocolRecordId {
                exchange: ExchangeKey {
                    flow_instance: r.get(0)?,
                    sequence: u64::from_be_bytes(r.get(1)?),
                },
                sequence: u64::from_be_bytes(r.get(2)?),
            })
        },
    )
    .map_err(StoreError::from)
}

impl BusinessLog {
    /// Page source-backed application sessions, newest first by their first original record.
    pub async fn chat_sessions(
        &self,
        before: Option<ChatSessionRef>,
        limit: u16,
    ) -> Result<(Vec<ChatSession>, Option<ChatSessionRef>), StoreError> {
        if !(1..=64).contains(&limit) {
            return Err(StoreError::Invalid);
        }
        self.call(move |conn| {
            let tx=conn.transaction()?;
            let boundary=before.as_ref().map(|r| entry(&tx,r)).transpose()?;
            let mut entries = {
                let mut stmt=tx.prepare("SELECT id FROM chat_session_entries WHERE ?1 IS NULL OR id<?1 ORDER BY id DESC LIMIT ?2")?;
                stmt.query_map(params![boundary,u32::from(limit)+1],|r| r.get::<_,i64>(0))?
                    .collect::<Result<Vec<_>,_>>()?
            };
            let more=entries.len()>usize::from(limit);
            entries.truncate(usize::from(limit));
            let sessions=entries.into_iter().map(|id| read_entry(&tx,id)).collect::<Result<Vec<_>,StoreError>>()?;
            let next=sessions.last().map(|s|s.reference().clone()).filter(|_|more);
            tx.commit()?;
            Ok((sessions,next))
        })?.await
    }
}

/// Register the original selected-field model anchor without copying Session values or content.
pub(super) fn register_model(
    conn: &Connection,
    delivery: i64,
    protocol: i64,
) -> Result<(), StoreError> {
    let anchor: Option<i64> = conn.query_row(
        &format!(
            "WITH {VALID} SELECT min(v.id) FROM valid v JOIN valid a USING ({DOMAIN}) WHERE a.id=?1"
        ),
        [protocol],
        |r| r.get(0),
    )?;
    if let Some(anchor) = anchor {
        conn.execute("INSERT INTO chat_session_entries(delivery,kind,protocol) VALUES(?1,'model_http',?2) ON CONFLICT DO NOTHING",params![delivery,anchor])?;
    }
    Ok(())
}
/// Resolve only the supplied fixed directory anchor, validating a native reference against its raw row.
pub(crate) fn entry(conn: &Connection, reference: &ChatSessionRef) -> Result<i64, StoreError> {
    let id = match reference {
        ChatSessionRef::ModelHttp { record } => {
            let p = resolve(conn, *record)?;
            conn.query_row(
                "SELECT id FROM chat_session_entries WHERE protocol=?1",
                [p],
                |r| r.get(0),
            )
            .optional()?
        }
        ChatSessionRef::NativeGateway { object } => {
            validate_reference(conn, &object.message)?;
            conn.query_row("SELECT e.id FROM chat_session_entries e JOIN gateway_session_observations s ON s.id=e.gateway_session WHERE s.message=?1 AND s.object_pointer=?2",
                params![object.message.id as i64,object.pointer],|r|r.get(0)).optional()?
        }
    };
    id.ok_or(StoreError::NotProvided)
}
/// Check the complete original message reference; a valid row number alone is insufficient.
pub(crate) fn validate_reference(
    conn: &Connection,
    r: &GatewayReference,
) -> Result<(), StoreError> {
    if r.id == 0 || r.id > i64::MAX as u64 {
        return Err(StoreError::Invalid);
    }
    let stored = group_gateway::reference(conn, r.id as i64).map_err(|e| match e {
        StoreError::Sql(rusqlite::Error::QueryReturnedNoRows) => StoreError::NotProvided,
        e => e,
    })?;
    if stored != *r {
        return Err(StoreError::Invalid);
    }
    Ok(())
}
/// Read the fixed source-specific directory anchor without selecting a substitute object.
pub(crate) fn entry_reference(conn: &Connection, id: i64) -> Result<ChatSessionRef, StoreError> {
    let (p, s): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT protocol,gateway_session FROM chat_session_entries WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    match (p, s) {
        (Some(p), None) => Ok(ChatSessionRef::ModelHttp {
            record: reference(conn, p)?,
        }),
        (None, Some(s)) => Ok(ChatSessionRef::NativeGateway {
            object: read_session(conn, s)?.reference,
        }),
        _ => Err(StoreError::Invalid),
    }
}
fn read_entry(conn: &Connection, id: i64) -> Result<ChatSession, StoreError> {
    let reference = entry_reference(conn, id)?;
    match &reference {
        ChatSessionRef::ModelHttp { record } => {
            let p = resolve(conn, *record)?;
            let src = conn.query_row(
                "SELECT source FROM protocol_records WHERE id=?1",
                [p],
                |r| r.get(0),
            )?;
            let exchanges=conn.query_row(&format!("WITH {VALID} SELECT count(DISTINCT hex(v.flow_instance)||hex(v.exchange_sequence)) FROM valid v JOIN valid a USING ({DOMAIN}) WHERE a.id=?1"),[p],|r|r.get::<_,i64>(0))? as u64;
            Ok(ChatSession::ModelHttp {
                reference,
                source: Arc::new(source::read(conn, src)?),
                session: read(conn, p)?.ok_or(StoreError::Invalid)?,
                exchanges,
            })
        }
        ChatSessionRef::NativeGateway { object } => {
            let s = conn.query_row(
                "SELECT gateway_session FROM chat_session_entries WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            let session = read_session(conn, s)?;
            let source = Arc::new(group_gateway::connection(conn, &object.message.capture)?);
            let count=conn.query_row("SELECT count(*) FROM gateway_chat_memberships m WHERE m.session=?1 AND m.delivery=(SELECT max(n.delivery) FROM gateway_chat_memberships n WHERE n.chat=m.chat)",[id],|r|r.get::<_,i64>(0))? as u64;
            let mut gaps = projection_gaps(conn, object.message.id as i64, "")?;
            if session.request.is_some() && !rpc_valid(conn, object.message.id as i64)? {
                gaps.push(gap(SessionGapReason::RpcConflict, &session.reference));
            }
            Ok(ChatSession::NativeGateway {
                reference,
                namespace: GatewaySessionNamespace::from(source.as_ref()),
                source,
                session: Box::new(session),
                chat_observations: count,
                gaps,
            })
        }
    }
}
/// Reconstruct one original session object's indexed fields and original source/request references.
pub(crate) fn read_session(
    conn: &Connection,
    id: i64,
) -> Result<GatewaySessionObservation, StoreError> {
    let (m,path,session_id,session_key,run_id,last_run_id,agent_id,request)=conn.query_row(
        "SELECT message,object_pointer,session_id,session_key,run_id,last_run_id,agent_id,request FROM gateway_session_observations WHERE id=?1",[id],|r|
        Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get::<_,Option<i64>>(7)?)))?;
    Ok(GatewaySessionObservation {
        reference: GatewayFieldReference {
            message: group_gateway::reference(conn, m)?,
            pointer: path,
        },
        session_id,
        session_key,
        run_id,
        last_run_id,
        agent_id,
        request: request
            .map(|r| group_gateway::reference(conn, r))
            .transpose()?,
    })
}
/// Read only the gaps stored for the specified original object, not a later object's gaps.
pub(crate) fn projection_gaps(
    conn: &Connection,
    message: i64,
    path: &str,
) -> Result<Vec<SessionGap>, StoreError> {
    let gaps: Option<String> = conn
        .query_row(
            "SELECT gaps FROM gateway_projection_gaps WHERE message=?1 AND object_pointer=?2",
            params![message, path],
            |r| r.get(0),
        )
        .optional()?;
    gaps.map(|s| serde_json::from_str(&s).map_err(StoreError::from))
        .transpose()
        .map(Option::unwrap_or_default)
}
/// Save supplied content-free business projections and original cursor equality indexes atomically.
pub(crate) fn save_projection(
    conn: &Connection,
    doc: &GatewayDocument<'_>,
    p: &GatewayProjection,
    request: Option<&GatewayDocument<'_>>,
) -> Result<(), StoreError> {
    let m = doc.reference.id as i64;
    for s in &p.sessions {
        conn.execute("INSERT INTO gateway_session_observations(message,object_pointer,session_id,session_key,run_id,last_run_id,agent_id,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT DO NOTHING",
            params![m,s.reference.pointer,s.session_id,s.session_key,s.run_id,s.last_run_id,s.agent_id,s.request.as_ref().map(|r|r.id as i64)])?;
    }
    conn.execute(
        "INSERT INTO gateway_projection_gaps VALUES(?1,'',?2) ON CONFLICT DO NOTHING",
        params![m, serde_json::to_string(&p.gaps)?],
    )?;
    for c in &p.chats {
        let o = c;
        let (nk, nv) = id_columns(&c.native_id);
        let (wk, wv) = id_columns(&c.wrapper_id);
        let (sk, sv) = id_columns(&o.message_sequence);
        let (wsk, wsv) = id_columns(&o.wrapper_sequence);
        let (rk, rv) = id_columns(&o.revision);
        conn.execute("INSERT INTO gateway_chat_observations(message,object_pointer,wrapper_pointer,history_pointer,history_index,full_window,native_id_kind,native_id_value,wrapper_id_kind,wrapper_id_value,sequence_kind,sequence_value,wrapper_sequence_kind,wrapper_sequence_value,revision_kind,revision_value,run_id,message_run_id)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) ON CONFLICT DO NOTHING",
            params![m,o.reference.pointer,o.wrapper.as_ref().map(|r|&r.pointer),o.history.as_ref().map(|r|&r.pointer),c.history_index,c.full_window,nk,nv,wk,wv,sk,sv,wsk,wsv,rk,rv,o.run_id,o.message_run_id])?;
        let chat = conn.query_row(
            "SELECT id FROM gateway_chat_observations WHERE message=?1 AND object_pointer=?2",
            params![m, o.reference.pointer],
            |r| r.get::<_, i64>(0),
        )?;
        for s in &c.session_fields {
            conn.execute("INSERT INTO gateway_chat_session_fields SELECT ?1,id FROM gateway_session_observations WHERE message=?2 AND object_pointer=?3 ON CONFLICT DO NOTHING",params![chat,s.reference.message.id as i64,s.reference.pointer])?;
        }
        conn.execute(
            "INSERT INTO gateway_projection_gaps VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
            params![m, o.reference.pointer, serde_json::to_string(&c.gaps)?],
        )?;
    }
    for w in &p.windows {
        let session = w
            .session
            .as_ref()
            .map(|s| session_row(conn, &s.reference))
            .transpose()?;
        let cursor = w
            .output_cursor
            .as_ref()
            .and_then(|r| doc.value.pointer(&r.pointer));
        let cursor_kind = cursor.map(|v| match v {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "bool",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        });
        let input = w
            .input_cursor
            .as_ref()
            .and_then(|r| request.and_then(|req| req.value.pointer(&r.pointer)));
        conn.execute("INSERT INTO gateway_history_windows VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT DO NOTHING",params![m,w.request.id as i64,row::string(&w.kind)?,session,w.messages.as_ref().map(|r|&r.pointer),w.input_cursor.as_ref().map(|r|&r.pointer),w.output_cursor.as_ref().map(|r|&r.pointer),cursor_kind,cursor.map(serde_json::to_string).transpose()?,input.map(serde_json::to_string).transpose()?])?;
        if let Some(s) = session {
            conn.execute(
                "INSERT INTO gateway_history_sessions VALUES(?1,?2) ON CONFLICT DO NOTHING",
                params![m, s],
            )?;
        }
    }
    Ok(())
}
/// Locate exactly the supplied original session object by its message and JSON pointer.
pub(crate) fn session_row(conn: &Connection, r: &GatewayFieldReference) -> Result<i64, StoreError> {
    Ok(conn.query_row(
        "SELECT id FROM gateway_session_observations WHERE message=?1 AND object_pointer=?2",
        params![r.message.id as i64, r.pointer],
        |r| r.get(0),
    )?)
}
/// Borrow the original scalar kind/value for lossless TEXT indexes, with no SQL numeric conversion.
pub(crate) fn id_columns(id: &Option<NativeIdValue>) -> (Option<&str>, Option<&str>) {
    match id {
        Some(NativeIdValue::JsonString(s)) => (Some("json_string"), Some(s)),
        Some(NativeIdValue::JsonNumber(s)) => (Some("json_number"), Some(s)),
        None => (None, None),
    }
}
/// Restore an original string/number scalar from its explicit kind/value columns, rejecting mismatch.
pub(crate) fn read_id(
    kind: Option<String>,
    value: Option<String>,
) -> Result<Option<NativeIdValue>, StoreError> {
    match (kind.as_deref(), value) {
        (Some("json_string"), Some(v)) => Ok(Some(NativeIdValue::JsonString(v))),
        (Some("json_number"), Some(v)) => Ok(Some(NativeIdValue::JsonNumber(v))),
        (None, None) => Ok(None),
        _ => Err(StoreError::Invalid),
    }
}

/// Persist only supplied original RPC envelope fields, without copying params or payload content.
pub(crate) fn save_rpc(
    conn: &Connection,
    reference: &GatewayReference,
    rpc: &crate::gateway::GatewayRpc<'_>,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO gateway_rpc_observations VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            reference.id as i64,
            reference.capture.as_slice(),
            rpc.kind,
            rpc.id,
            rpc.method,
            rpc.ok
        ],
    )?;
    Ok(())
}
/// Read all same-capture/same-ID RPC rows for this actual RPC observation; events have no peers.
pub(crate) fn rpc_peers(
    conn: &Connection,
    message: i64,
) -> Result<(Vec<i64>, Vec<i64>), StoreError> {
    let rows=conn.prepare("SELECT peer.message,peer.kind FROM gateway_rpc_observations current JOIN gateway_rpc_observations peer ON peer.capture=current.capture AND peer.rpc_id=current.rpc_id
        WHERE current.message=?1 AND current.rpc_id<>'' ORDER BY peer.message")?
        .query_map([message],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
    let mut req = Vec::new();
    let mut res = Vec::new();
    for (m, k) in rows {
        if k == "req" {
            req.push(m);
        } else {
            res.push(m);
        }
    }
    Ok((req, res))
}
/// Check exact original RPC cardinality: one request and response for a nonempty same-capture ID.
pub(crate) fn rpc_valid(conn: &Connection, m: i64) -> Result<bool, StoreError> {
    let pair: Option<(Vec<u8>, Option<String>)> = conn
        .query_row(
            "SELECT capture,rpc_id FROM gateway_rpc_observations WHERE message=?1",
            [m],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((capture, Some(id))) = pair else {
        return Ok(false);
    };
    let (requests,replies):(i64,i64)=conn.query_row("SELECT count(CASE WHEN kind='req' THEN 1 END),count(CASE WHEN kind='res' THEN 1 END) FROM gateway_rpc_observations WHERE capture=?1 AND rpc_id=?2",params![capture,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(!id.is_empty() && requests == 1 && replies == 1)
}

/// Original same-issuer session fields and source-validity facts from the caller's SQL transaction.
pub(crate) struct SessionCandidate {
    /// Positive gateway_session_observations row ID, used only as a foreign key.
    pub id: i64,
    /// Original object's fields and references; no values are filled from another object.
    pub observation: GatewaySessionObservation,
    /// Whether original RPC consistency and explicit identity gaps permit using this evidence.
    pub valid: bool,
    /// Existing fixed directory anchor row, only when this observation originally registered it.
    pub entry: Option<i64>,
}
/// Read original candidates only within the complete actual server lifetime and endpoint issuer.
pub(crate) fn session_candidates(
    conn: &Connection,
    c: &GatewayConnection,
) -> Result<Vec<SessionCandidate>, StoreError> {
    let ids=conn.prepare("SELECT s.id,e.id FROM gateway_session_observations s JOIN group_gateway_messages m ON m.id=s.message JOIN group_gateway_connections c ON c.capture=m.connection JOIN process_keys p ON p.id=c.server LEFT JOIN chat_session_entries e ON e.gateway_session=s.id
        WHERE p.boot_id=?1 AND p.host_tgid=?2 AND p.birth_boottime_ns=?3 AND c.server_port=?4 ORDER BY s.id")?
        .query_map(params![c.server.boot_id.as_slice(),c.server.host_tgid,c.server.birth_boottime_ns.to_be_bytes().as_slice(),c.server_port],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
    ids.into_iter()
        .map(|(id, entry)| {
            let observation = read_session(conn, id)?;
            let valid = observation
                .request
                .as_ref()
                .map(|r| rpc_valid(conn, r.id as i64))
                .transpose()?
                .unwrap_or(true)
                && !identity_disputed(
                    &projection_gaps(conn, observation.reference.message.id as i64, "")?,
                    &observation.reference,
                );
            Ok(SessionCandidate {
                id,
                observation,
                valid,
                entry,
            })
        })
        .collect()
}
/// Register the business-selected original session anchor at this delivery; no Session value is copied.
pub(crate) fn register_native(conn: &Connection, delivery: i64, s: i64) -> Result<i64, StoreError> {
    conn.execute("INSERT INTO chat_session_entries(delivery,kind,gateway_session) VALUES(?1,'native_gateway',?2)",params![delivery,s])?;
    Ok(conn.last_insert_rowid())
}

/// Read affected original index facts and source-validation gaps for the pure membership matcher.
pub(crate) fn chat_candidates(
    conn: &Connection,
    c: &GatewayConnection,
    messages: &[i64],
    runs: &[String],
) -> Result<Vec<MembershipInput>, StoreError> {
    use rusqlite::types::Value as SqlValue;
    let mut parameters = vec![
        SqlValue::Blob(c.server.boot_id.to_vec()),
        SqlValue::Integer(i64::from(c.server.host_tgid)),
        SqlValue::Blob(c.server.birth_boottime_ns.to_be_bytes().to_vec()),
        SqlValue::Integer(i64::from(c.server_port)),
    ];
    let messages = messages
        .iter()
        .map(|m| {
            parameters.push(SqlValue::Integer(*m));
            format!("?{}", parameters.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    let runs = runs
        .iter()
        .map(|r| {
            parameters.push(SqlValue::Text(r.clone()));
            format!("?{}", parameters.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    let ids=conn.prepare(&format!("SELECT o.id,o.message,o.object_pointer,o.run_id,o.message_run_id,o.native_id_kind,o.native_id_value,o.wrapper_id_kind,o.wrapper_id_value FROM gateway_chat_observations o JOIN group_gateway_messages m ON m.id=o.message JOIN group_gateway_connections c ON c.capture=m.connection JOIN process_keys p ON p.id=c.server
        WHERE p.boot_id=?1 AND p.host_tgid=?2 AND p.birth_boottime_ns=?3 AND c.server_port=?4
        AND (o.message IN ({messages}) OR o.run_id IN ({runs}) OR o.message_run_id IN ({runs}) OR o.id IN (
            SELECT f.chat FROM gateway_chat_session_fields f JOIN gateway_session_observations s ON s.id=f.session WHERE s.message IN ({messages}) OR s.request IN ({messages}))) ORDER BY o.id"))?
        .query_map(rusqlite::params_from_iter(parameters),|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)))?.collect::<Result<Vec<_>,_>>()?;
    ids.into_iter().map(|(id,m,path,run_id,message_run_id,nk,nv,wk,wv)|{
        let reference=GatewayFieldReference {message:group_gateway::reference(conn,m)?,pointer:path};
        let fields=conn.prepare("SELECT session FROM gateway_chat_session_fields WHERE chat=?1 ORDER BY session")?.query_map([id],|r|r.get::<_,i64>(0))?
            .map(|r|read_session(conn,r?)).collect::<Result<Vec<_>,StoreError>>()?;
        let handoffs=conn.prepare("SELECT id FROM gateway_session_observations WHERE request=?1 ORDER BY id")?.query_map([m],|r|r.get::<_,i64>(0))?
            .map(|r|read_session(conn,r?)).collect::<Result<Vec<_>,StoreError>>()?;
        let mut rpc=None;
        if reference.message.from_client {
            if !rpc_valid(conn,m)? {
                let counts=conn.query_row("SELECT count(*) FROM gateway_rpc_observations a JOIN gateway_rpc_observations b ON a.capture=b.capture AND a.rpc_id=b.rpc_id WHERE a.message=?1",[m],|r|r.get::<_,i64>(0))?;
                rpc=Some(if counts>2 {SessionGapReason::RpcConflict}else{SessionGapReason::RpcUnpaired});
            } else {
                let ok:Option<bool>=conn.query_row("SELECT b.ok FROM gateway_rpc_observations a JOIN gateway_rpc_observations b ON a.capture=b.capture AND a.rpc_id=b.rpc_id WHERE a.message=?1 AND b.kind='res'",[m],|r|r.get(0))?;
                if ok!=Some(true) || handoffs.is_empty() {rpc=Some(SessionGapReason::RpcFailed);}
            }
        }
        let valid=rpc.is_none();
        let mut gaps=projection_gaps(conn,m,&reference.pointer)?;
        gaps.extend(projection_gaps(conn,m,"")?.into_iter().filter(|g|g.reason!=SessionGapReason::IdentityConflict || fields.iter().any(|s|g.references.contains(&s.reference))));
        let mut verified_fields=Vec::new();
        for s in fields {
            let source_gaps=projection_gaps(conn,s.reference.message.id as i64,"")?;
            let paired=s.request.as_ref().map(|r|rpc_valid(conn,r.id as i64)).transpose()?.unwrap_or(true);
            if !paired {
                gaps.push(gap(SessionGapReason::RpcConflict,&s.reference));
            } else if identity_disputed(&source_gaps,&s.reference) {gaps.extend(source_gaps);}
            else {verified_fields.push(s);}
        }
        for s in &handoffs {gaps.extend(projection_gaps(conn,s.reference.message.id as i64,"")?);}
        Ok(MembershipInput {id,reference:reference.clone(),native_id:read_id(nk,nv)?,wrapper_id:read_id(wk,wv)?,fields:if reference.message.from_client && !valid {Vec::new()}else{verified_fields},run_id,message_run_id,rpc,
            handoffs:if valid {handoffs}else{Vec::new()},gaps})
    }).collect()
}
/// Locate a committed fixed message anchor for the business-proven Session and original scalar ID.
pub(crate) fn message_anchor(
    conn: &Connection,
    session: i64,
    native_id: &NativeIdValue,
) -> Result<Option<i64>, StoreError> {
    let id = Some(native_id.clone());
    let (kind, value) = id_columns(&id);
    Ok(conn.query_row("SELECT m.message_anchor FROM gateway_chat_memberships m JOIN gateway_chat_observations o ON o.id=m.chat
        WHERE m.session=?1 AND m.delivery=(SELECT max(n.delivery) FROM gateway_chat_memberships n WHERE n.chat=m.chat)
        AND ((o.native_id_kind=?2 AND o.native_id_value=?3) OR (o.native_id_kind IS NULL AND o.wrapper_id_kind=?2 AND o.wrapper_id_value=?3))
        ORDER BY m.delivery,m.chat LIMIT 1",params![session,kind,value],|r|r.get(0)).optional()?)
}
/// Append a changed business membership/anchor result at this delivery; prior evidence is retained.
pub(crate) fn save_membership(
    conn: &Connection,
    chat: i64,
    delivery: i64,
    session: Option<i64>,
    handoff: Option<i64>,
    anchor: i64,
    gaps: &[SessionGap],
) -> Result<(), StoreError> {
    let gaps = serde_json::to_string(gaps)?;
    let old:Option<(Option<i64>,Option<i64>,i64,String)>=conn.query_row("SELECT session,handoff,message_anchor,gaps FROM gateway_chat_memberships WHERE chat=?1 ORDER BY delivery DESC LIMIT 1",[chat],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if old != Some((session, handoff, anchor, gaps.clone())) {
        conn.execute(
            "INSERT INTO gateway_chat_memberships VALUES(?1,?2,?3,?4,?5,?6)",
            params![chat, delivery, session, handoff, anchor, gaps],
        )?;
    }
    Ok(())
}

/// Read exact opaque-cursor matches within the verified issuer, retaining original window references.
pub(crate) fn history_cursor_candidates(
    conn: &Connection,
    c: &GatewayConnection,
    message: i64,
    cursor: &serde_json::Value,
) -> Result<Vec<(GatewayHistoryKind, GatewaySessionObservation)>, StoreError> {
    let rows=conn.prepare("SELECT h.session,w.kind,w.request FROM gateway_history_windows w JOIN gateway_history_sessions h ON h.window=w.message JOIN group_gateway_messages m ON m.id=w.message JOIN group_gateway_connections c ON c.capture=m.connection JOIN process_keys s ON s.id=c.server WHERE (w.cursor_value=?1 OR (w.kind='reset' AND w.input_cursor_value=?1)) AND w.message<>?2 AND s.boot_id=?3 AND s.host_tgid=?4 AND s.birth_boottime_ns=?5 AND c.server_port=?6")?
        .query_map(params![serde_json::to_string(cursor)?,message,c.server.boot_id.as_slice(),c.server.host_tgid,c.server.birth_boottime_ns.to_be_bytes().as_slice(),c.server_port],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<Result<Vec<_>,_>>()?;
    rows.into_iter()
        .filter_map(|(s, kind, request)| match rpc_valid(conn, request) {
            Ok(true) => Some(Ok((s, kind))),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        })
        .map(|r| {
            let (s, kind) = r?;
            Ok((row::named(&kind)?, read_session(conn, s)?))
        })
        .collect()
}
/// Persist only the pure projection's inherited original session references, without window content.
pub(crate) fn link_history_sessions(
    conn: &Connection,
    message: i64,
    sessions: &[GatewaySessionObservation],
) -> Result<(), StoreError> {
    for s in sessions {
        let s = session_row(conn, &s.reference)?;
        conn.execute("INSERT INTO gateway_chat_session_fields SELECT id,?1 FROM gateway_chat_observations WHERE message=?2 ON CONFLICT DO NOTHING",params![s,message])?;
        conn.execute(
            "INSERT INTO gateway_history_sessions VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![message, s],
        )?;
    }
    Ok(())
}

/// Read membership and original position indexes visible at the fixed committed delivery cutoff.
pub(crate) fn read_chat_indexes(
    conn: &Connection,
    session: Option<i64>,
    cutoff: i64,
) -> Result<Vec<ChatIndex>, StoreError> {
    let mut stmt=conn.prepare("SELECT o.id,o.message,o.object_pointer,o.native_id_kind,o.native_id_value,o.wrapper_id_kind,o.wrapper_id_value,
        o.sequence_kind,o.sequence_value,o.wrapper_sequence_kind,o.wrapper_sequence_value,o.history_pointer,o.history_index,o.full_window,
        b.session,b.handoff,b.gaps,b.message_anchor FROM gateway_chat_observations o JOIN gateway_chat_memberships b ON b.chat=o.id
        WHERE b.delivery=(SELECT max(n.delivery) FROM gateway_chat_memberships n WHERE n.chat=o.id AND n.delivery<=?1)
        AND ((?2 IS NULL AND b.session IS NULL) OR b.session=?2) ORDER BY o.id")?;
    let mut rows = stmt.query(params![cutoff, session])?;
    let mut result = Vec::new();
    while let Some(r) = rows.next()? {
        let m = r.get::<_, i64>(1)?;
        let reference = GatewayFieldReference {
            message: group_gateway::reference(conn, m)?,
            pointer: r.get(2)?,
        };
        let session = r
            .get::<_, Option<i64>>(14)?
            .map(|s| entry_reference(conn, s))
            .transpose()?;
        let anchor = r.get::<_, i64>(17)?;
        let (anchor_message, anchor_pointer) = conn.query_row(
            "SELECT message,object_pointer FROM gateway_chat_observations WHERE id=?1",
            [anchor],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )?;
        result.push(ChatIndex {
            id: r.get(0)?,
            reference: reference.clone(),
            anchor: GatewayFieldReference {
                message: group_gateway::reference(conn, anchor_message)?,
                pointer: anchor_pointer,
            },
            session,
            native_id: read_id(r.get(3)?, r.get(4)?)?,
            wrapper_id: read_id(r.get(5)?, r.get(6)?)?,
            message_sequence: read_id(r.get(7)?, r.get(8)?)?,
            wrapper_sequence: read_id(r.get(9)?, r.get(10)?)?,
            history: r
                .get::<_, Option<String>>(11)?
                .map(|pointer| GatewayFieldReference {
                    message: reference.message.clone(),
                    pointer,
                }),
            history_index: r.get(12)?,
            full_window: r.get(13)?,
            handoff: r
                .get::<_, Option<i64>>(15)?
                .map(|s| read_session(conn, s))
                .transpose()?,
            gaps: serde_json::from_str(&r.get::<_, String>(16)?)?,
        });
    }
    Ok(result)
}
/// One query's necessary original wire and provenance; released after the caller rebuilds its result.
pub(crate) struct OriginalGatewayMessage {
    /// Actual verified connection owning this original message.
    pub connection: GatewayConnection,
    /// Complete original stored message/frame reference.
    pub reference: GatewayReference,
    /// Original ordered wire frames; no decoded/body copy is stored alongside them.
    pub frames: Vec<crate::agent_graph::websocket::WireFrame>,
    /// Original paired request row, when the stored projection actually names one.
    pub request: Option<i64>,
}
/// Fetch exactly one original message and its explicit request reference, without application parsing.
pub(crate) fn original_message(
    conn: &Connection,
    id: i64,
) -> Result<OriginalGatewayMessage, StoreError> {
    let reference = group_gateway::reference(conn, id)?;
    let request=conn.query_row("SELECT request FROM gateway_session_observations WHERE message=?1 AND request IS NOT NULL ORDER BY id LIMIT 1",[id],|r|r.get(0)).optional()?;
    Ok(OriginalGatewayMessage {
        connection: group_gateway::connection(conn, &reference.capture)?,
        frames: group_gateway::frames(conn, id)?,
        reference,
        request,
    })
}
