//! Structured original source metadata; collections retain presence independently of rows.
use super::{
    StoreError,
    row::{self, Object},
    schema,
};
use as_types::{
    dispatch_event::ProcessInfo,
    flow::{ActorFacts, ProtocolSource},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

pub(super) fn id_fields(value: &impl serde::Serialize) -> Result<Object, StoreError> {
    let mut v = row::object(value)?;
    let mut exchange = row::child(&mut v, "exchange")?;
    v.insert(
        "flow_instance".into(),
        row::take(&mut exchange, "flow_instance")?,
    );
    v.insert(
        "exchange_sequence".into(),
        row::take(&mut exchange, "sequence")?,
    );
    if !exchange.is_empty() {
        return Err(StoreError::Invalid);
    }
    Ok(v)
}
pub(super) fn id_value(mut v: Object) -> Result<Value, StoreError> {
    let flow = row::take(&mut v, "flow_instance")?;
    let seq = row::take(&mut v, "exchange_sequence")?;
    v.insert(
        "exchange".into(),
        json!({"flow_instance":flow,"sequence":seq}),
    );
    Ok(Value::Object(v))
}
pub(super) fn write_info(conn: &Connection, info: &ProcessInfo) -> Result<i64, StoreError> {
    write_info_value(conn, row::object(info)?)
}
fn write_info_value(conn: &Connection, mut v: Object) -> Result<i64, StoreError> {
    let mut lists = Vec::new();
    for name in ["argv", "parent_argv", "namespace_tgids"] {
        let value = row::take(&mut v, name)?;
        v.insert(format!("{name}_present"), Value::Bool(!value.is_null()));
        lists.push((name, value));
    }
    let workload = row::take(&mut v, "workload")?;
    let (mut kind, mut runtime, mut namespace, mut id) =
        (Value::Null, Value::Null, Value::Null, Value::Null);
    if let Value::Object(mut workload) = workload {
        kind = row::take(&mut workload, "kind")?;
        if kind == "container" {
            let mut original = row::child(&mut workload, "instance")?;
            runtime = row::take(&mut original, "runtime")?;
            namespace = row::take(&mut original, "namespace")?;
            id = row::take(&mut original, "id")?;
            if !original.is_empty() {
                return Err(StoreError::Invalid);
            }
        }
        if !workload.is_empty() {
            return Err(StoreError::Invalid);
        }
    } else if !workload.is_null() {
        return Err(StoreError::Invalid);
    }
    for (name, value) in [
        ("workload_kind", kind),
        ("workload_runtime", runtime),
        ("workload_namespace", namespace),
        ("workload_id", id),
    ] {
        v.insert(name.into(), value);
    }
    let id = row::put(conn, "process_info", schema::INFO, v)?;
    for (name, value) in lists {
        if let Value::Array(items) = value {
            for (n, item) in items.iter().enumerate() {
                if name == "namespace_tgids" {
                    conn.execute(
                        "INSERT INTO namespace_tgids VALUES(?1,?2,?3)",
                        params![
                            id,
                            n as i64,
                            item.as_u64()
                                .and_then(|v| u32::try_from(v).ok())
                                .ok_or(StoreError::Invalid)?
                        ],
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO argv VALUES(?1,?2,?3,?4)",
                        params![id, name, n as i64, row::blob(item)?],
                    )?;
                }
            }
        } else if !value.is_null() {
            return Err(StoreError::Invalid);
        }
    }
    Ok(id)
}
pub(super) fn argv(
    conn: &Connection,
    owner: &str,
    id: i64,
    present: bool,
) -> Result<Value, StoreError> {
    if !present {
        return Ok(Value::Null);
    }
    let mut stmt =
        conn.prepare("SELECT bytes FROM argv WHERE owner=?1 AND info=?2 ORDER BY ordinal")?;
    let values = stmt
        .query_map(params![owner, id], |r| r.get::<_, Vec<u8>>(0))?
        .map(|r| r.map(|b| row::bytes(&b)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(values))
}
pub(super) fn read_info(conn: &Connection, id: i64) -> Result<ProcessInfo, StoreError> {
    let mut v = row::get(conn, "process_info", schema::INFO, id)?;
    for name in ["argv", "parent_argv"] {
        let present = row::take(&mut v, &format!("{name}_present"))?
            .as_bool()
            .ok_or(StoreError::Invalid)?;
        v.insert(name.into(), argv(conn, name, id, present)?);
    }
    let present = row::take(&mut v, "namespace_tgids_present")?
        .as_bool()
        .ok_or(StoreError::Invalid)?;
    let namespaces = if present {
        let mut stmt =
            conn.prepare("SELECT value FROM namespace_tgids WHERE info=?1 ORDER BY ordinal")?;
        Value::Array(
            stmt.query_map([id], |r| r.get::<_, u32>(0))?
                .map(|r| r.map(|v| json!(v)))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        Value::Null
    };
    v.insert("namespace_tgids".into(), namespaces);
    let kind = row::take(&mut v, "workload_kind")?;
    let runtime = row::take(&mut v, "workload_runtime")?;
    let namespace = row::take(&mut v, "workload_namespace")?;
    let original = row::take(&mut v, "workload_id")?;
    v.insert(
        "workload".into(),
        if kind.is_null() {
            Value::Null
        } else if kind == "host" {
            json!({"kind":"host"})
        } else {
            json!({"kind":kind,"instance":{"runtime":runtime,"namespace":namespace,"id":original}})
        },
    );
    row::decode(Value::Object(v))
}
pub(super) fn write_actor(conn: &Connection, actor: &ActorFacts) -> Result<i64, StoreError> {
    let mut value = row::object(actor)?;
    let _ = row::take(&mut value, "info")?;
    value.insert("info".into(), row::integer(write_info(conn, &actor.info)?));
    let process = row::take(&mut value, "process")?;
    value.insert(
        "process".into(),
        if process.is_null() {
            Value::Null
        } else {
            row::integer(row::put(
                conn,
                "process_keys",
                schema::PROCESS_KEY,
                row::object(&process)?,
            )?)
        },
    );
    row::put(conn, "actors", schema::ACTOR, value)
}
pub(super) fn read_actor(conn: &Connection, id: i64) -> Result<ActorFacts, StoreError> {
    let mut v = row::get(conn, "actors", schema::ACTOR, id)?;
    let info = row::take(&mut v, "info")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    v.insert("info".into(), serde_json::to_value(read_info(conn, info)?)?);
    let process = row::take(&mut v, "process")?;
    v.insert(
        "process".into(),
        if process.is_null() {
            Value::Null
        } else {
            Value::Object(row::get(
                conn,
                "process_keys",
                schema::PROCESS_KEY,
                process.as_i64().ok_or(StoreError::Invalid)?,
            )?)
        },
    );
    row::decode(Value::Object(v))
}
pub(super) fn write(conn: &Connection, source: &ProtocolSource) -> Result<i64, StoreError> {
    let mut value = row::object(source)?;
    let _ = row::take(&mut value, "actor")?;
    value.insert(
        "actor".into(),
        row::integer(write_actor(conn, &source.actor)?),
    );
    let channel = row::take(&mut value, "channel")?;
    value.insert(
        "channel".into(),
        if channel.is_null() {
            Value::Null
        } else {
            row::integer(row::put(
                conn,
                "channels",
                schema::CHANNEL,
                row::object(&channel)?,
            )?)
        },
    );
    let credential = row::take(&mut value, "credential_use")?;
    value.insert(
        "credential_use".into(),
        if credential.is_null() {
            Value::Null
        } else {
            row::integer(row::put(
                conn,
                "credential_ids",
                schema::CREDENTIAL_ID,
                id_fields(&credential)?,
            )?)
        },
    );
    row::put(conn, "protocol_sources", schema::SOURCE, value)
}
pub(super) fn read(conn: &Connection, id: i64) -> Result<ProtocolSource, StoreError> {
    let mut value = row::get(conn, "protocol_sources", schema::SOURCE, id)?;
    let actor = row::take(&mut value, "actor")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    value.insert(
        "actor".into(),
        serde_json::to_value(read_actor(conn, actor)?)?,
    );
    let channel = row::take(&mut value, "channel")?;
    value.insert(
        "channel".into(),
        if channel.is_null() {
            Value::Null
        } else {
            Value::Object(row::get(
                conn,
                "channels",
                schema::CHANNEL,
                channel.as_i64().ok_or(StoreError::Invalid)?,
            )?)
        },
    );
    let credential = row::take(&mut value, "credential_use")?;
    value.insert(
        "credential_use".into(),
        if credential.is_null() {
            Value::Null
        } else {
            id_value(row::get(
                conn,
                "credential_ids",
                schema::CREDENTIAL_ID,
                credential.as_i64().ok_or(StoreError::Invalid)?,
            )?)?
        },
    );
    row::decode(Value::Object(value))
}
