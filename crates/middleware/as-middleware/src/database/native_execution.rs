//! Native execution facts remain useful before a model-protocol binding exists.
use super::{BusinessLog, StoreError, StoredReceipt, parts, row, schema};
use crate::context::FactReference;
use as_types::{
    driver::{NativeFutexSelection, NativeRequestReturn},
    main_ports::ToolExecutionFacts,
    process_tree::EventKey,
};
use rusqlite::Connection;
#[cfg(test)]
use rusqlite::params;
use serde_json::{Map, Value, json};

fn write_observation(
    conn: &mut Connection,
    event: EventKey,
    kind: &str,
    table: &str,
    columns: row::Columns,
    mut fields: Map<String, Value>,
) -> Result<StoredReceipt, StoreError> {
    let tx = conn.transaction()?;
    let event = FactReference::Event(event);
    fields.insert("event_ref".into(), Value::String(super::key(&event)?));
    let delivery = super::record(&tx, kind)?;
    fields.insert("delivery".into(), row::integer(delivery));
    let id = row::put(&tx, table, columns, fields)?;
    parts::reference(&tx, &event, kind, id)?;
    tx.commit()?;
    Ok(StoredReceipt { record: delivery })
}

pub(super) fn read(conn: &Connection, id: i64) -> Result<ToolExecutionFacts, StoreError> {
    let mut fields = row::get(conn, "native_tool_executions", schema::NATIVE_TOOL, id)?;
    let reference: FactReference = row::decode(serde_json::from_str(
        row::take(&mut fields, "event_ref")?
            .as_str()
            .ok_or(StoreError::Invalid)?,
    )?)?;
    let FactReference::Event(event) = reference else {
        return Err(StoreError::Invalid);
    };
    row::take(&mut fields, "delivery")?;
    let origin = row::take(&mut fields, "origin_handoff")?;
    let candidate = row::take(&mut fields, "origin_candidate")?;
    let origin = origin.as_i64().ok_or(StoreError::Invalid)?;
    let candidate = candidate
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(StoreError::Invalid)?;
    let source = super::native_handoff::read_at(conn, origin)?;
    let as_types::runtime_observation::NativeRequestPhase::Submission {
        analysis: Some(analysis),
        ..
    } = source.phase
    else {
        return Err(StoreError::Invalid);
    };
    let context = analysis
        .candidates
        .into_iter()
        .nth(candidate)
        .ok_or(StoreError::Invalid)?
        .context;
    Ok(ToolExecutionFacts { event, context })
}

impl BusinessLog {
    pub(crate) async fn record_request_return(
        &self,
        fact: NativeRequestReturn,
    ) -> Result<StoredReceipt, StoreError> {
        self.write_call(move |conn| {
            let mut fields = row::object(&fact)?;
            row::take(&mut fields, "event")?;
            write_observation(
                conn,
                fact.event,
                "native_request_return",
                "native_request_returns",
                schema::REQUEST_RETURN,
                fields,
            )
        })?
        .await
    }

    pub(crate) async fn record_futex_selection(
        &self,
        fact: NativeFutexSelection,
    ) -> Result<StoredReceipt, StoreError> {
        self.write_call(move |conn| {
            let fields = row::object(&json!({
                "selected_boottime_ns": fact.selected_boottime_ns.to_string(),
                "waker_tgid": fact.waker.tgid,
                "waker_tid": fact.waker.tid,
                "waker_birth_ns": fact.waker.birth_boottime_ns.to_string(),
                "waiter_tgid": fact.waiter.tgid,
                "waiter_tid": fact.waiter.tid,
                "waiter_birth_ns": fact.waiter.birth_boottime_ns.to_string(),
                "wait_id": fact.wait.as_ref().map(|wait| wait.id.to_string()),
                "wait_boottime_ns": fact.wait.as_ref().map(|wait| wait.observed_boottime_ns.to_string()),
            }))?;
            write_observation(
                conn,
                fact.event,
                "native_futex_selection",
                "native_futex_selections",
                schema::FUTEX_SELECTION,
                fields,
            )
        })?
        .await
    }
}

pub(super) fn read_request_return(
    conn: &Connection,
    id: i64,
) -> Result<NativeRequestReturn, StoreError> {
    let mut fields = row::get(conn, "native_request_returns", schema::REQUEST_RETURN, id)?;
    let event = observation_event(&mut fields)?;
    fields.insert("event".into(), serde_json::to_value(event)?);
    row::decode(Value::Object(fields))
}

fn observation_event(fields: &mut Map<String, Value>) -> Result<EventKey, StoreError> {
    let reference: FactReference = serde_json::from_str(
        row::take(fields, "event_ref")?
            .as_str()
            .ok_or(StoreError::Invalid)?,
    )?;
    let FactReference::Event(event) = reference else {
        return Err(StoreError::Invalid);
    };
    row::take(fields, "delivery")?;
    Ok(event)
}

pub(super) fn read_futex_selection(
    conn: &Connection,
    id: i64,
) -> Result<NativeFutexSelection, StoreError> {
    let mut fields = row::get(conn, "native_futex_selections", schema::FUTEX_SELECTION, id)?;
    let event = observation_event(&mut fields)?;
    let wait_id = row::take(&mut fields, "wait_id")?;
    let wait_ns = row::take(&mut fields, "wait_boottime_ns")?;
    let wait = match (wait_id.is_null(), wait_ns.is_null()) {
        (true, true) => Value::Null,
        (false, false) => json!({"id":wait_id,"observed_boottime_ns":wait_ns}),
        _ => return Err(StoreError::Invalid),
    };
    row::decode(json!({
        "event":event,
        "selected_boottime_ns":row::take(&mut fields, "selected_boottime_ns")?,
        "waker":{"tgid":row::take(&mut fields,"waker_tgid")?, "tid":row::take(&mut fields,"waker_tid")?,
                 "birth_boottime_ns":row::take(&mut fields,"waker_birth_ns")?},
        "waiter":{"tgid":row::take(&mut fields,"waiter_tgid")?, "tid":row::take(&mut fields,"waiter_tid")?,
                  "birth_boottime_ns":row::take(&mut fields,"waiter_birth_ns")?},
        "wait":wait,
    }))
}

/// Software coordinator fixture: explicitly asserted execution authority, never candidate inference.
/// The production current-tool publisher is not implemented.
#[cfg(test)]
pub(super) fn link_capture(
    conn: &Connection,
    event: EventKey,
    origin: i64,
    candidate: usize,
    context: &as_types::main_ports::NativeToolContext,
) -> Result<(), StoreError> {
    let reference = super::key(&FactReference::Event(event))?;
    let candidate = i64::try_from(candidate).map_err(|_| StoreError::Invalid)?;
    let existing: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM native_tool_executions WHERE event_ref=?1 AND origin_handoff=?2 AND origin_candidate=?3)",params![reference,origin,candidate],|r|r.get(0))?;
    if existing {
        return Ok(());
    }
    let ids = {
        let mut statement =
            conn.prepare("SELECT id FROM native_tool_executions WHERE event_ref=?1")?;
        statement
            .query_map([&reference], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for id in ids {
        if &read(conn, id)?.context != context {
            conn.execute(
                "INSERT OR IGNORE INTO context_gaps(at_ref,reason) VALUES(?1,?2)",
                params![
                    reference,
                    row::string(&crate::context::ContextGapReason::ConflictingEvidence)?
                ],
            )?;
            break;
        }
    }
    let delivery = super::record(conn, "native_tool")?;
    let mut fields = Map::new();
    fields.insert("event_ref".into(), Value::String(reference));
    fields.insert("delivery".into(), row::integer(delivery));
    fields.insert("origin_handoff".into(), row::integer(origin));
    fields.insert("origin_candidate".into(), row::integer(candidate));
    let id = row::put(conn, "native_tool_executions", schema::NATIVE_TOOL, fields)?;
    parts::reference(conn, &FactReference::Event(event), "native_tool", id)?;
    Ok(())
}
