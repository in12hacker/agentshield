//! Original runtime observations in the existing business owner, without semantic admission gates.
use super::{BusinessLog, StoreError, StoredReceipt, parts};
use crate::context::{FactReference, SourceRecord};
use as_types::{
    process_tree::EventKey,
    runtime_observation::{NativeRequestHandoff, NativeRequestPhase},
};
use rusqlite::{Connection, params};
use std::{collections::BTreeSet, sync::Arc};

fn phase(phase: &NativeRequestPhase) -> &'static str {
    match phase {
        NativeRequestPhase::Submission { .. } => "submission",
        NativeRequestPhase::Submit { .. } => "submit",
        NativeRequestPhase::WorkEnter { .. } => "work_enter",
        NativeRequestPhase::WorkLeave { .. } => "work_leave",
        NativeRequestPhase::Done { .. } => "done",
        NativeRequestPhase::Cleanup { .. } => "cleanup",
        NativeRequestPhase::EventBound { .. } => "event_bound",
        NativeRequestPhase::ApiReturned { .. } => "api_returned",
        NativeRequestPhase::ChildCreated { .. } => "child_created",
    }
}

/// Read the exact request lifetimes explicitly named by this event's actual EventBound rows.
/// Missing opening/tool/HTTP records never remove the observations that were acquired.
pub(super) fn load_event(
    conn: &Connection,
    event: EventKey,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    let reference = super::key(&FactReference::Event(event))?;
    let mut statement = conn.prepare(
        "WITH bound_requests AS (
             SELECT request FROM native_request_handoffs
             WHERE bound_event=?1 AND request IS NOT NULL
             UNION SELECT enclosing_call FROM native_request_handoffs WHERE bound_event=?1 AND enclosing_call IS NOT NULL
         ) SELECT id,fact,projection FROM native_request_handoffs
         WHERE observation IN (
             SELECT observation FROM native_request_handoffs
             WHERE bound_event=?1 OR request IN (SELECT request FROM bound_requests)
                 OR observation IN (SELECT request FROM bound_requests))
         ORDER BY id",
    )?;
    let mut rows = statement.query([reference])?;
    while let Some(row) = rows.next()? {
        append(row.get(0)?, row.get(1)?, row.get(2)?, records, seen)?;
    }
    Ok(())
}

/// Locate the raw observation directly, then follow only its explicit request lifetime.
pub(super) fn load_observation(
    conn: &Connection,
    observation: EventKey,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    let mut statement = conn.prepare(
        "WITH requests AS (
             SELECT request FROM native_request_handoffs WHERE observation=?1 AND request IS NOT NULL
             UNION SELECT enclosing_call FROM native_request_handoffs WHERE observation=?1 AND enclosing_call IS NOT NULL
             UNION SELECT observation FROM native_request_handoffs WHERE observation=?1 AND phase='submission'
         ) SELECT id,fact,projection FROM native_request_handoffs
         WHERE observation IN (
             SELECT observation FROM native_request_handoffs
             WHERE observation=?1 OR request IN (SELECT * FROM requests)
                 OR observation IN (SELECT * FROM requests)) ORDER BY id",
    )?;
    let mut rows = statement.query([super::key(&observation)?])?;
    while let Some(row) = rows.next()? {
        append(row.get(0)?, row.get(1)?, row.get(2)?, records, seen)?;
    }
    Ok(())
}

fn append(
    id: i64,
    fact: String,
    projection: Option<Vec<u8>>,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    if seen.contains(&("native_request_handoff".to_owned(), id)) {
        return Ok(());
    }
    append_original(id, decode_stored(&fact, projection)?, records, seen);
    Ok(())
}

fn append_original(
    id: i64,
    original: NativeRequestHandoff,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) {
    if seen.insert(("native_request_handoff".to_owned(), id)) {
        let earlier = seen
            .iter()
            .filter(|(kind, original)| kind == "native_request_handoff" && *original < id)
            .count();
        let position = records
            .iter()
            .enumerate()
            .filter(|(_, record)| matches!(record, SourceRecord::RequestHandoff(_)))
            .nth(earlier)
            .map_or(records.len(), |(position, _)| position);
        records.insert(position, SourceRecord::RequestHandoff(Arc::new(original)));
    }
}

/// Read only the first retained same-thread WorkLeave time after an actual native return.
/// The activity owner separately verifies the operation and the result pattern.
pub(super) fn load_return_sequences(
    conn: &Connection,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    let originals = records.clone();
    let mut selected = std::collections::BTreeMap::new();
    for returned in originals.iter().filter_map(|r| match r {
        SourceRecord::RequestReturn(r) => Some(r),
        _ => None,
    }) {
        if returned.operation != as_types::driver::NativeRequestOperation::FileOpen {
            continue;
        }
        for event in crate::activity::event_records(&originals, returned.event) {
            let header = event.header();
            let (Some(tgid), Some(tid), Some(birth)) = (
                header.actor_tgid,
                header.actor_tid,
                header.actor_thread_birth_boottime_ns,
            ) else {
                continue;
            };
            let mut statement = conn.prepare(
                "SELECT id,fact,observed_boottime_ns FROM native_request_handoffs
                 WHERE source=?1 AND phase='work_leave' AND actor_tgid=?2 AND actor_tid=?3
                   AND thread_birth=?4 AND observed_boottime_ns>?5
                 ORDER BY observed_boottime_ns,id",
            )?;
            let mut rows = statement.query(params![
                returned.event.source_instance.as_slice(),
                tgid,
                tid,
                birth.to_be_bytes().as_slice(),
                returned.returned_boottime_ns.to_be_bytes().as_slice()
            ])?;
            let mut first_time = None;
            while let Some(row) = rows.next()? {
                let time: [u8; 8] = row.get(2)?;
                if first_time.is_some_and(|first| first != time) {
                    break;
                }
                first_time = Some(time);
                let fact: String = row.get(1)?;
                let original = decode_stored(&fact, None)?;
                selected
                    .entry(super::key(
                        &original.request.unwrap_or(original.observation),
                    )?)
                    .or_insert(original.observation);
                append_original(row.get(0)?, original, records, seen);
            }
        }
    }
    for observation in selected.into_values() {
        load_observation(conn, observation, records, seen)?;
    }
    load_openings(conn, records, seen)?;
    Ok(())
}

/// Resolve the currently supplied request references in one batch, without traversing bad targets.
fn load_openings(
    conn: &Connection,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    let requests = records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::RequestHandoff(fact) => fact.request,
            _ => None,
        })
        .map(|request| super::key(&request))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if requests.is_empty() {
        return Ok(());
    }
    let mut statement = conn.prepare(
        "SELECT id,fact,projection FROM native_request_handoffs
         WHERE observation IN (SELECT value FROM json_each(?1)) ORDER BY id",
    )?;
    let mut rows = statement.query([serde_json::to_string(&requests)?])?;
    while let Some(row) = rows.next()? {
        append(row.get(0)?, row.get(1)?, row.get(2)?, records, seen)?;
    }
    Ok(())
}

// Projection bytes remain a single SQL BLOB. The small JSON part carries an
// explicit storage reference, never a fake null/empty replacement of acquired data.
fn encode_stored(
    fact: &mut NativeRequestHandoff,
) -> Result<(String, Option<bytes::Bytes>), StoreError> {
    let projection = match &mut fact.phase {
        NativeRequestPhase::Submission { projection, .. } => projection.take(),
        _ => None,
    };
    let mut value = serde_json::to_value(&*fact)?;
    if let NativeRequestPhase::Submission { .. } = fact.phase {
        let phase = value
            .get_mut("phase")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or(StoreError::Invalid)?;
        phase.remove("projection");
        phase.insert("projection_ref".into(), projection.is_some().into());
    }
    Ok((serde_json::to_string(&value)?, projection))
}
fn decode_stored(
    text: &str,
    projection: Option<Vec<u8>>,
) -> Result<NativeRequestHandoff, StoreError> {
    let mut value = as_types::json::parse(text.as_bytes())?;
    let phase = value
        .get_mut("phase")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(StoreError::Invalid)?;
    let submission = phase.get("type").and_then(serde_json::Value::as_str) == Some("submission");
    if submission {
        let stored = phase
            .remove("projection_ref")
            .and_then(|v| v.as_bool())
            .ok_or(StoreError::Invalid)?;
        if stored != projection.is_some() {
            return Err(StoreError::Conflict);
        }
    } else if projection.is_some() {
        return Err(StoreError::Conflict);
    }
    // Only the JSON payload is opaque. Keep its existing ChatContent tag/shape in the typed
    // decoder, and move each payload out by its original candidate ordinal before that decode.
    let mut arguments = Vec::new();
    if submission && let Some(analysis) = phase.get_mut("analysis").filter(|value| !value.is_null())
    {
        let candidates = analysis
            .get_mut("candidates")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or(StoreError::Invalid)?;
        for candidate in candidates {
            let context = candidate
                .get_mut("context")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(StoreError::Invalid)?;
            arguments.push(
                context
                    .get_mut("arguments")
                    .and_then(serde_json::Value::as_object_mut)
                    .filter(|content| {
                        content.get("kind").and_then(serde_json::Value::as_str) == Some("json")
                    })
                    .and_then(|content| content.get_mut("value"))
                    .map(std::mem::take),
            );
        }
    }
    let mut fact: NativeRequestHandoff = serde_json::from_value(value)?;
    if let NativeRequestPhase::Submission {
        projection: target,
        analysis,
        ..
    } = &mut fact.phase
    {
        if let Some(analysis) = analysis {
            if analysis.candidates.len() != arguments.len() {
                return Err(StoreError::Invalid);
            }
            for (candidate, original) in analysis.candidates.iter_mut().zip(arguments) {
                match (candidate.context.arguments.as_mut(), original) {
                    (Some(as_types::flow::ChatContent::Json(target)), Some(original)) => {
                        *target = original;
                    }
                    (Some(as_types::flow::ChatContent::Json(_)), None) | (_, Some(_)) => {
                        return Err(StoreError::Invalid);
                    }
                    (_, None) => {}
                }
            }
        } else if !arguments.is_empty() {
            return Err(StoreError::Invalid);
        }
        *target = projection.map(bytes::Bytes::from);
    }
    Ok(fact)
}

pub(super) fn read_at(conn: &Connection, id: i64) -> Result<NativeRequestHandoff, StoreError> {
    let (fact, projection): (String, Option<Vec<u8>>) = conn.query_row(
        "SELECT fact,projection FROM native_request_handoffs WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    decode_stored(&fact, projection)
}

fn observation_conflicts(conn: &Connection, observation: EventKey) -> Result<bool, StoreError> {
    let mut statement =
        conn.prepare("SELECT id FROM native_request_handoffs WHERE observation=?1 ORDER BY id")?;
    let ids = statement
        .query_map([super::key(&observation)?], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if ids.len() < 2 {
        return Ok(false);
    }
    let mut first = None;
    for id in ids {
        let original = read_at(conn, id)?;
        if first.as_ref().is_some_and(|earlier| earlier != &original) {
            return Ok(true);
        }
        if first.is_none() {
            first = Some(original);
        }
    }
    Ok(false)
}

fn source_gap(
    conn: &Connection,
    event: EventKey,
    reason: crate::context::ContextGapReason,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO context_gaps(at_ref,reason) VALUES(?1,?2)",
        params![
            super::key(&FactReference::Event(event))?,
            super::row::string(&reason)?
        ],
    )?;
    Ok(())
}

// Preserve conflicts across explicit raw references; this never publishes tool execution.
fn record_source_conflicts(
    conn: &Connection,
    fact: &NativeRequestHandoff,
) -> Result<(), StoreError> {
    if observation_conflicts(conn, fact.observation)? {
        // The conflicting duplicate can change request or bound_event. Both
        // old and newly claimed consumers retain the conflicting raw evidence.
        let mut statement = conn.prepare(
            "SELECT DISTINCT bound_event FROM native_request_handoffs
             WHERE bound_event IS NOT NULL AND
               (observation=?1 OR request=?1 OR enclosing_call=?1 OR request IN
                 (SELECT request FROM native_request_handoffs WHERE observation=?1 AND request IS NOT NULL))",
        )?;
        let events = statement
            .query_map([super::key(&fact.observation)?], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for reference in events {
            let FactReference::Event(event) = serde_json::from_str(&reference)? else {
                return Err(StoreError::Invalid);
            };
            source_gap(
                conn,
                event,
                crate::context::ContextGapReason::ConflictingEvidence,
            )?;
        }
        return Ok(());
    }
    Ok(())
}

impl BusinessLog {
    /// Commit every received raw observation, even without a request association or tool semantics.
    pub(crate) async fn record_request_handoff(
        &self,
        mut fact: NativeRequestHandoff,
    ) -> Result<StoredReceipt, StoreError> {
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            let delivery = super::record(&tx, "native_request_handoff")?;
            let bound_event = match &fact.phase {
                NativeRequestPhase::EventBound { event, .. } => Some(FactReference::Event(*event)),
                _ => None,
            };
            let observation = super::key(&fact.observation)?;
            let request = fact.request.as_ref().map(super::key).transpose()?;
            let bound_key = bound_event.as_ref().map(super::key).transpose()?;
            // Serialize the original typed record once; all other columns are identity indexes.
            let (original, projection) = encode_stored(&mut fact)?;
            tx.execute(
                "INSERT INTO native_request_handoffs
                 (delivery,observation,request,source,phase,bound_event,fact,
                  actor_tgid,actor_tid,thread_birth,observed_boottime_ns,projection,enclosing_call)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    delivery,
                    observation,
                    request,
                    fact.observation.source_instance.as_slice(),
                    phase(&fact.phase),
                    bound_key,
                    original,
                    fact.actor.tgid,
                    fact.actor.tid,
                    fact.actor.birth_boottime_ns.to_be_bytes().as_slice(),
                    fact.observed_boottime_ns.to_be_bytes().as_slice(),
                    projection.as_deref(),
                    match &fact.phase {
                        NativeRequestPhase::EventBound { enclosing_call, .. } =>
                            enclosing_call.as_ref().map(super::key).transpose()?,
                        _ => None,
                    }
                ],
            )?;
            if let Some(event) = bound_event {
                parts::reference(
                    &tx,
                    &event,
                    "native_request_handoff",
                    tx.last_insert_rowid(),
                )?;
            }
            record_source_conflicts(&tx, &fact)?;
            tx.commit()?;
            Ok(StoredReceipt { record: delivery })
        })?
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use as_types::{
        driver::{NativeRequestOperation, NativeSyncThread},
        main_ports::MessageModule,
        runtime_observation::NativeCaptureStatus,
    };

    fn key(source: u8, domain: u64, sequence: u64) -> EventKey {
        EventKey {
            source_instance: [source; 16],
            sequence_domain: domain,
            sequence,
        }
    }

    fn raw(
        sequence: u64,
        request: Option<EventKey>,
        phase: NativeRequestPhase,
    ) -> NativeRequestHandoff {
        NativeRequestHandoff {
            observation: key(1, 8, sequence),
            request,
            operation: None,
            actor: NativeSyncThread {
                tgid: 158,
                tid: 167,
                birth_boottime_ns: 76253843216,
            },
            observed_boottime_ns: 0,
            phase,
        }
    }

    async fn intake(log: &BusinessLog, facts: &[NativeRequestHandoff]) -> Result<(), StoreError> {
        let media = crate::media::Media::new(log.clone());
        for fact in facts {
            media
                .consume_native_fact(as_types::main_ports::NativeFact::RequestHandoff(
                    fact.clone(),
                ))
                .await
                .map_err(|_| StoreError::Unconfirmed)?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn public_pairs_and_native_handoffs_remain_raw_without_execution_authority()
    -> Result<(), StoreError> {
        use as_types::{
            main_ports::NativeToolContext,
            process_tree::ProcessKey,
            runtime_observation::{
                CaptureAnalysis, NativeApiSite, NativeEventBinding, NativeLibraryCall,
                NativeStreamTryWriteEntry, NativeStreamWriteEntry, SharedMessageCandidate,
                SharedMessageProof,
            },
        };
        let dir = tempfile::tempdir()?;
        let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
        let process = ProcessKey {
            boot_id: [1; 16],
            host_tgid: 158,
            birth_boottime_ns: 11,
        };
        let context = NativeToolContext {
            process,
            tool_call_id: "call-1".into(),
            tool_name: "execute".into(),
            response_id: Some(String::new()),
            run_id: None,
            session_id: None,
            session_key: None,
            agent_id: None,
            provider: None,
            api: None,
            model: None,
            arguments: Some(as_types::flow::ChatContent::Json(serde_json::json!([
                "",
                [],
                0,
                false
            ]))),
        };
        // This is storage/linkage software evidence, not a V8 capture or live attribution claim.
        let mut submission = raw(
            40,
            None,
            NativeRequestPhase::Submission {
                site: NativeApiSite {
                    process,
                    exec_cookie: 0,
                    instruction_pointer: 0,
                    stack_pointer: 0,
                    link_register: Some(0),
                    frame_pointer: None,
                },
                call: NativeLibraryCall::StreamWrite {
                    entry: NativeStreamWriteEntry::Write,
                    request_address: 0,
                    stream_address: 0,
                    buffers_address: 0,
                    buffer_count: 0,
                    send_stream_address: None,
                    callback_address: 0,
                },
                projection: Some(bytes::Bytes::from_static(&[0, 255, 128, 0])),
                analysis: Some(CaptureAnalysis {
                    status: 1,
                    candidates: vec![SharedMessageCandidate {
                        context: context.clone(),
                        proof: SharedMessageProof {
                            frame: 0,
                            store: 0,
                            tool: 0,
                            assistant: 0,
                            content: 0,
                            content_index: 0,
                            reads_consistent: true,
                        },
                        issues: vec![],
                    }],
                }),
                projection_error: None,
            },
        );
        if let NativeRequestPhase::Submission {
            analysis: Some(analysis),
            ..
        } = &mut submission.phase
        {
            let original = analysis.candidates[0].clone();
            for arguments in [
                Some(as_types::flow::ChatContent::Json(serde_json::json!({
                    "$serde_json::private::Number": "1.0"
                }))),
                Some(as_types::flow::ChatContent::Json(serde_json::json!({
                    "$serde_json::private::RawValue": "false"
                }))),
                Some(as_types::flow::ChatContent::Json(serde_json::json!([
                    {"$serde_json::private::Number": "1e400"},
                    {"$serde_json::private::RawValue": "null"}
                ]))),
                None,
                Some(as_types::flow::ChatContent::Json(serde_json::Value::Null)),
                Some(as_types::flow::ChatContent::Text(String::new())),
            ] {
                let mut candidate = original.clone();
                candidate.context.arguments = arguments;
                analysis.candidates.push(candidate);
            }
        }
        let try_source = submission.clone();
        let scope = submission.observation;
        intake(&log, &[submission.clone()]).await?;
        let original = submission.clone();
        log.call(move |db| {
            let (id, json, blob): (i64, String, Vec<u8>) = db.query_row(
                "SELECT id,fact,projection FROM native_request_handoffs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(blob, [0, 255, 128, 0]);
            let stored = as_types::json::parse(json.as_bytes())?;
            assert!(stored["phase"].get("projection").is_none());
            assert_eq!(stored["phase"]["projection_ref"], true);
            assert!(
                stored["phase"]["analysis"]["candidates"][1]["context"]["arguments"]["value"]
                    .is_object()
            );
            assert_eq!(read_at(db, id)?, original);
            assert_eq!(
                read_at(db, id)?,
                original,
                "repeated read keeps candidate order"
            );
            assert!(decode_stored(&json, None).is_err());
            for (pointer, replacement) in [
                ("/phase/analysis/candidates", serde_json::json!([null])),
                (
                    "/phase/analysis/candidates/0/context/arguments",
                    serde_json::json!({"kind":"text","value":false}),
                ),
                (
                    "/phase/analysis/candidates/0/context/arguments",
                    serde_json::json!({"kind":"json"}),
                ),
                (
                    "/phase/analysis/candidates/0/proof/reads_consistent",
                    serde_json::json!("true"),
                ),
            ] {
                let mut malformed = stored.clone();
                *malformed.pointer_mut(pointer).ok_or(StoreError::Invalid)? = replacement;
                assert!(
                    decode_stored(&serde_json::to_string(&malformed)?, Some(blob.clone())).is_err()
                );
            }
            Ok(())
        })?
        .await?;
        let event = key(2, 1, 40);
        let mut bound = raw(
            41,
            None,
            NativeRequestPhase::EventBound {
                event,
                binding: NativeEventBinding::ControllerWait { wait_id: 1 },
                enclosing_call: Some(scope),
            },
        );
        bound.operation = Some(NativeRequestOperation::Send);
        intake(&log, &[bound.clone()]).await?;
        assert_eq!(
            log.call(|db| Ok(db.query_row(
                "SELECT COUNT(*) FROM native_tool_executions",
                [],
                |r| r.get::<_, u32>(0)
            )?))?
            .await?,
            0
        );
        // An actual request reference still cannot turn a write scope into queue ownership.
        bound.observation.sequence += 1;
        bound.request = Some(scope);
        intake(&log, &[bound]).await?;
        assert_eq!(
            log.call(|db| Ok(db.query_row(
                "SELECT COUNT(*) FROM native_tool_executions",
                [],
                |r| r.get::<_, u32>(0)
            )?))?
            .await?,
            0
        );

        if let NativeRequestPhase::Submission { call, .. } = &mut submission.phase {
            *call = NativeLibraryCall::TcpConnect {
                request_address: 0,
                stream_address: 0,
                address: 0,
                callback_address: 0,
            };
        }
        submission.observation.sequence = 44;
        let mut connect = raw(
            45,
            Some(submission.observation),
            NativeRequestPhase::EventBound {
                event: key(2, 1, 44),
                binding: NativeEventBinding::DirectCheck,
                enclosing_call: None,
            },
        );
        connect.operation = Some(NativeRequestOperation::Connect);
        intake(&log, &[submission.clone(), connect]).await?;
        let connected = log.activity(FactReference::Event(key(2, 1, 44))).await?;
        assert!(
            !connected
                .source_records
                .iter()
                .any(|record| matches!(record, SourceRecord::ToolExecution(_)))
        );
        assert!(
            connected
                .source_records
                .iter()
                .any(|record| matches!(record,
            SourceRecord::RequestHandoff(fact) if matches!(&fact.phase,
                NativeRequestPhase::Submission { analysis: Some(analysis), .. }
                if analysis.candidates.iter().any(|candidate| candidate.context == context))))
        );
        assert!(
            connected
                .source_records
                .iter()
                .any(|record| matches!(record,
            SourceRecord::RequestHandoff(fact) if matches!(fact.phase,
                NativeRequestPhase::EventBound { event, .. } if event == key(2, 1, 44))))
        );
        let mut spawn = submission.clone();
        spawn.observation.sequence = 50;
        if let NativeRequestPhase::Submission { call, .. } = &mut spawn.phase {
            *call = NativeLibraryCall::Spawn {
                loop_address: 0,
                process_address: 0,
                options_address: 0,
            };
        }
        let child = ProcessKey {
            boot_id: process.boot_id,
            host_tgid: 888,
            birth_boottime_ns: 50,
        };
        let mut created = raw(
            51,
            Some(spawn.observation),
            NativeRequestPhase::ChildCreated { child },
        );
        let mut execute = raw(
            52,
            Some(spawn.observation),
            NativeRequestPhase::EventBound {
                event: key(2, 1, 50),
                binding: NativeEventBinding::ControllerWait { wait_id: 9 },
                enclosing_call: None,
            },
        );
        execute.actor = NativeSyncThread {
            tgid: child.host_tgid,
            tid: child.host_tgid,
            birth_boottime_ns: child.birth_boottime_ns,
        };
        execute.operation = Some(NativeRequestOperation::Execute);
        intake(&log, &[spawn, created.clone(), execute]).await?;
        assert_eq!(
            log.call(|db| Ok(db.query_row(
                "SELECT COUNT(*) FROM native_tool_executions",
                [],
                |r| r.get::<_, u32>(0)
            )?))?
            .await?,
            0
        );
        // A duplicate can change both child and request; its old consumer must
        // still receive the conflict instead of retaining the earlier proof.
        created.request = Some(key(1, 8, 999));
        created.phase = NativeRequestPhase::ChildCreated {
            child: ProcessKey {
                host_tgid: 999,
                ..child
            },
        };
        intake(&log, &[created]).await?;
        log.call(|db| {
            let reference = super::super::key(&FactReference::Event(key(2, 1, 50)))?;
            assert!(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM context_gaps WHERE at_ref=?1 AND reason=?2)",
                params![
                    reference,
                    super::super::row::string(
                        &crate::context::ContextGapReason::ConflictingEvidence
                    )?
                ],
                |r| r.get::<_, bool>(0)
            )?);
            Ok(())
        })?
        .await?;
        // Late source conflicts preserve all raw values without publishing execution authority.
        if let NativeRequestPhase::Submission {
            analysis: Some(analysis),
            ..
        } = &mut submission.phase
        {
            analysis.candidates[0].context.tool_call_id = "different-call".into();
        }
        intake(&log, &[submission]).await?;
        log.call(|db| {
            let reference = super::super::key(&FactReference::Event(key(2, 1, 44)))?;
            let conflict: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM context_gaps WHERE at_ref=?1 AND reason=?2)",
                params![
                    reference,
                    super::super::row::string(
                        &crate::context::ContextGapReason::ConflictingEvidence
                    )?
                ],
                |r| r.get(0),
            )?;
            assert!(conflict);
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM native_tool_executions", [], |r| r
                    .get::<_, u32>(0))?,
                0
            );
            Ok(())
        })?
        .await?;
        // Synchronous CAPI ownership is still not current-tool execution authority.
        // Same-thread and different-thread originals both remain acquired facts.
        for (index, entry, send_stream_address) in [
            (0, NativeStreamTryWriteEntry::TryWrite, None),
            (1, NativeStreamTryWriteEntry::TryWrite2, Some(0)),
        ] {
            let mut synchronous = try_source.clone();
            synchronous.observation.sequence = 60 + index * 4;
            synchronous.operation = Some(NativeRequestOperation::Send);
            if let NativeRequestPhase::Submission { call, .. } = &mut synchronous.phase {
                *call = NativeLibraryCall::StreamTryWrite {
                    entry,
                    stream_address: 0,
                    buffers_address: 0,
                    buffer_count: 0,
                    send_stream_address,
                };
            }
            let scope = synchronous.observation;
            let wrong_event = key(2, 1, 60 + index * 4);
            let mut wrong = raw(
                scope.sequence + 1,
                Some(scope),
                NativeRequestPhase::EventBound {
                    event: wrong_event,
                    binding: NativeEventBinding::DirectCheck,
                    enclosing_call: None,
                },
            );
            wrong.operation = Some(NativeRequestOperation::Send);
            wrong.actor.tid += 1;
            intake(&log, &[synchronous.clone(), wrong.clone()]).await?;
            let wrong_view = log.activity(FactReference::Event(wrong_event)).await?;
            assert!(wrong_view.source_records.iter().any(|record| matches!(record, SourceRecord::RequestHandoff(fact) if fact.as_ref() == &wrong)));
            log.call(move |db| {
                assert!(!db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM native_tool_executions WHERE event_ref=?1)",
                    [super::super::key(&FactReference::Event(wrong_event))?],
                    |r| r.get::<_, bool>(0)
                )?);
                Ok(())
            })?
            .await?;
            let actual_event = key(2, 1, 61 + index * 4);
            let mut actual = raw(
                scope.sequence + 2,
                Some(scope),
                NativeRequestPhase::EventBound {
                    event: actual_event,
                    binding: NativeEventBinding::DirectCheck,
                    enclosing_call: None,
                },
            );
            actual.actor = synchronous.actor.clone();
            actual.operation = Some(NativeRequestOperation::Send);
            intake(&log, &[actual.clone()]).await?;
            let view = log.activity(FactReference::Event(actual_event)).await?;
            assert!(
                !view
                    .source_records
                    .iter()
                    .any(|record| matches!(record, SourceRecord::ToolExecution(_)))
            );
            assert!(view.source_records.iter().any(|record| matches!(record,
                SourceRecord::RequestHandoff(fact) if fact.as_ref() == &actual)));
            assert!(view.source_records.iter().any(|record| matches!(record,
                SourceRecord::RequestHandoff(fact) if fact.as_ref() == &synchronous)));
        }
        log.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn raw_lifetime_intake_keeps_missing_fields_and_only_actual_event_bound_joins()
    -> Result<(), StoreError> {
        let dir = tempfile::tempdir()?;
        let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
        let request = key(1, 9, 10);
        let other_request = key(1, 9, 11);
        let event = key(2, 1, 163);
        let other_event = key(2, 1, 164);
        let mut all = vec![
            raw(
                1,
                None,
                NativeRequestPhase::Submission {
                    site: as_types::runtime_observation::NativeApiSite {
                        process: as_types::process_tree::ProcessKey {
                            boot_id: [1; 16],
                            host_tgid: 158,
                            birth_boottime_ns: 0,
                        },
                        exec_cookie: 0,
                        instruction_pointer: 0,
                        stack_pointer: 0,
                        link_register: Some(0),
                        frame_pointer: None,
                    },
                    call: as_types::runtime_observation::NativeLibraryCall::FileOpen {
                        request_address: 0,
                        loop_address: 0,
                        path_address: 4096,
                        path: (vec![255, 128]).into(),
                        path_status: NativeCaptureStatus::ReadFault { native_error: -14 },
                        flags: 0,
                        mode: 0,
                        callback_address: 0,
                    },
                    projection: None,
                    analysis: None,
                    projection_error: Some(
                        as_types::runtime_observation::NativeProjectionError::Allocation,
                    ),
                },
            ),
            raw(
                2,
                Some(request),
                NativeRequestPhase::Submit {
                    loop_address: 0,
                    work_address: 4096,
                    work_callback: 0,
                    done_callback: 0,
                },
            ),
            raw(
                3,
                Some(request),
                NativeRequestPhase::WorkEnter { work_address: 4096 },
            ),
        ];
        intake(&log, &all).await?;
        assert!(matches!(
            log.activity(FactReference::Event(all[0].observation)).await,
            Err(StoreError::NotProvided)
        ));
        assert!(matches!(
            log.activity(FactReference::Event(event)).await,
            Err(StoreError::NotProvided)
        ));
        let raw_view = log
            .activity(FactReference::NativeObservation(all[0].observation))
            .await?;
        assert_eq!(
            raw_view.anchor,
            FactReference::NativeObservation(all[0].observation)
        );
        assert_eq!(raw_view.source_records.len(), 1);
        let first_count = log
            .call(|db| {
                Ok(
                    db.query_row("SELECT COUNT(*) FROM native_request_handoffs", [], |r| {
                        r.get::<_, u32>(0)
                    })?,
                )
            })?
            .await?;
        assert_eq!(first_count, 3);

        let mut bound = raw(
            4,
            Some(request),
            NativeRequestPhase::EventBound {
                event,
                binding: as_types::runtime_observation::NativeEventBinding::ControllerWait {
                    wait_id: 1,
                },
                enclosing_call: None,
            },
        );
        bound.operation = Some(NativeRequestOperation::FileOpen);
        let done = raw(
            5,
            Some(request),
            NativeRequestPhase::Done {
                work_address: 4096,
                result: None,
                result_read_error: Some(-14),
            },
        );
        intake(&log, &[bound.clone(), done.clone()]).await?;
        all.extend([bound.clone(), done.clone()]);
        let view = log
            .activity_context(FactReference::Event(event), None)
            .await?
            .0;
        let observed = view
            .source_records
            .iter()
            .filter_map(|r| match r {
                SourceRecord::RequestHandoff(fact) => Some(fact.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(observed.len(), 4);
        assert!(observed.contains(&&all[1]) && observed.contains(&&all[2]));
        assert!(observed.contains(&&bound) && observed.contains(&&done));
        assert!(
            view.relations
                .iter()
                .all(|relation| relation.kind
                    == crate::context::RelationKind::NativeRequestReference)
        );
        assert!(
            view.source_records
                .iter()
                .all(|r| matches!(r, SourceRecord::RequestHandoff(_)))
        );

        let tail = vec![
            raw(
                6,
                Some(request),
                NativeRequestPhase::Cleanup {
                    request_address: 17,
                },
            ),
            raw(
                7,
                Some(other_request),
                NativeRequestPhase::Submit {
                    loop_address: 0,
                    work_address: 4096,
                    work_callback: 0,
                    done_callback: 0,
                },
            ),
            raw(
                8,
                None,
                NativeRequestPhase::EventBound {
                    event: other_event,
                    binding: as_types::runtime_observation::NativeEventBinding::ControllerWait {
                        wait_id: 2,
                    },
                    enclosing_call: None,
                },
            ),
            raw(
                9,
                None,
                NativeRequestPhase::Done {
                    work_address: 4096,
                    result: Some(0),
                    result_read_error: None,
                },
            ),
        ];
        intake(&log, &tail).await?;
        all.extend(tail);
        let view = log.activity(FactReference::Event(event)).await?;
        assert_eq!(view.source_records.len(), 5);
        assert!(view.source_records.iter().all(
            |r| matches!(r, SourceRecord::RequestHandoff(fact) if fact.request==Some(request))
        ));
        let unknown_request = log.activity(FactReference::Event(other_event)).await?;
        assert_eq!(unknown_request.source_records.len(), 1);
        assert!(
            matches!(&unknown_request.source_records[0], SourceRecord::RequestHandoff(fact) if fact.as_ref()==&all[7])
        );
        let mut open = raw(
            10,
            None,
            NativeRequestPhase::Submission {
                site: as_types::runtime_observation::NativeApiSite {
                    process: as_types::process_tree::ProcessKey {
                        boot_id: [1; 16],
                        host_tgid: 158,
                        birth_boottime_ns: 0,
                    },
                    exec_cookie: 0,
                    instruction_pointer: 0,
                    stack_pointer: 0,
                    link_register: None,
                    frame_pointer: Some(0),
                },
                call: as_types::runtime_observation::NativeLibraryCall::FileOpen {
                    request_address: 20,
                    loop_address: 0,
                    path_address: 4096,
                    path: (vec![]).into(),
                    path_status: NativeCaptureStatus::Complete,
                    flags: 0,
                    mode: 0,
                    callback_address: 0,
                },
                projection: None,
                analysis: None,
                projection_error: Some(
                    as_types::runtime_observation::NativeProjectionError::Allocation,
                ),
            },
        );
        open.operation = Some(NativeRequestOperation::FileOpen);
        let open_request = open.observation;
        let with_open_event = key(2, 1, 165);
        let with_open = vec![
            open,
            raw(
                11,
                Some(open_request),
                NativeRequestPhase::Submit {
                    loop_address: 0,
                    work_address: 4096,
                    work_callback: 0,
                    done_callback: 0,
                },
            ),
            raw(
                12,
                Some(open_request),
                NativeRequestPhase::EventBound {
                    event: with_open_event,
                    binding: as_types::runtime_observation::NativeEventBinding::ControllerWait {
                        wait_id: 3,
                    },
                    enclosing_call: None,
                },
            ),
        ];
        intake(&log, &with_open).await?;
        let view = log.activity(FactReference::Event(with_open_event)).await?;
        let observed = view
            .source_records
            .iter()
            .filter_map(|record| match record {
                SourceRecord::RequestHandoff(fact) => Some(fact.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(observed, with_open.iter().collect::<Vec<_>>());
        assert_eq!(
            observed[0].request, None,
            "Open's original request field is not backfilled"
        );
        assert!(
            view.relations
                .iter()
                .all(|relation| relation.kind
                    == crate::context::RelationKind::NativeRequestReference)
        );
        all.extend(with_open);
        let stored = log
            .call(|db| {
                let mut stmt = db.prepare("SELECT id FROM native_request_handoffs ORDER BY id")?;
                let ids = stmt
                    .query_map([], |r| r.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| read_at(db, id))
                    .collect::<Result<Vec<_>, _>>()
            })?
            .await?;
        assert_eq!(stored, all);
        assert_eq!(log.counts().await?.questions, 0);
        assert_eq!(log.counts().await?.relations, 0);
        log.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn activity_reads_raw_references_and_the_first_retained_return_sequence()
    -> Result<(), StoreError> {
        use crate::context::{ContextGapReason, RelationKind};
        use as_main::messages::protocol::Up;
        use as_types::{DecisionId, driver::NativeRequestReturn, main_ports::Decision};
        let dir = tempfile::tempdir()?;
        let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
        let fixture = crate::execution_feedback::tests::Fixture::new();
        let mut original = serde_json::to_value(fixture.completion.event.as_ref())?;
        original["header"]["stage"] = serde_json::json!("controlled_request");
        original["header"]["source_boottime_ns"] = serde_json::json!("90");
        original["header"]["actor_tgid"] = serde_json::json!(158);
        original["header"]["actor_tid"] = serde_json::json!(167);
        original["header"]["actor_thread_birth_boottime_ns"] = serde_json::json!("76253843216");
        let event: as_types::dispatch_event::EventData = serde_json::from_value(original)?;
        let event_key = EventKey::from(&event);
        log.record_up(Arc::new(Up::Ask {
            decision_id: DecisionId([7; 16]),
            event: Arc::new(event),
            hit: fixture.completion.hit.ok_or(StoreError::Invalid)?,
            timeout: Decision::Allow,
            choices: vec![Decision::Allow],
            remaining_ms: 100,
        }))?
        .await?;
        let request = key(1, 8, 1);
        let mut open = raw(
            1,
            None,
            NativeRequestPhase::Submission {
                site: as_types::runtime_observation::NativeApiSite {
                    process: as_types::process_tree::ProcessKey {
                        boot_id: [1; 16],
                        host_tgid: 158,
                        birth_boottime_ns: 0,
                    },
                    exec_cookie: 0,
                    instruction_pointer: 0,
                    stack_pointer: 0,
                    link_register: None,
                    frame_pointer: None,
                },
                call: as_types::runtime_observation::NativeLibraryCall::FileOpen {
                    request_address: 0,
                    loop_address: 0,
                    path_address: 0,
                    path: (vec![]).into(),
                    path_status: NativeCaptureStatus::Complete,
                    flags: 0,
                    mode: 0,
                    callback_address: 0,
                },
                projection: None,
                analysis: None,
                projection_error: Some(
                    as_types::runtime_observation::NativeProjectionError::Allocation,
                ),
            },
        );
        open.operation = Some(NativeRequestOperation::FileOpen);
        let mut leave = raw(
            2,
            Some(request),
            NativeRequestPhase::WorkLeave { work_address: 0 },
        );
        leave.operation = Some(NativeRequestOperation::FileOpen);
        leave.observed_boottime_ns = 102;
        let mut tied = leave.clone();
        tied.observation.sequence = 3;
        let mut done = raw(
            4,
            Some(request),
            NativeRequestPhase::Done {
                work_address: 0,
                result: Some(0),
                result_read_error: None,
            },
        );
        done.operation = Some(NativeRequestOperation::FileOpen);
        done.observed_boottime_ns = 103;
        intake(
            &log,
            &[
                open.clone(),
                leave.clone(),
                leave.clone(),
                tied.clone(),
                done.clone(),
            ],
        )
        .await?;
        let raw_anchor = FactReference::NativeObservation(leave.observation);
        let (raw_view, attribution) = log.activity_context(raw_anchor.clone(), None).await?;
        assert_eq!(raw_view.anchor, raw_anchor);
        assert!(attribution.is_none());
        assert_eq!(
            raw_view.source_records.len(),
            5,
            "equal duplicate raw rows remain retained"
        );
        assert!(
            raw_view
                .relations
                .iter()
                .all(|r| r.kind == RelationKind::NativeRequestReference)
        );
        let absent_return = log.activity(FactReference::Event(event_key)).await?;
        assert!(
            absent_return.relations.is_empty(),
            "an event alone supplies no native return"
        );
        log.record_request_return(NativeRequestReturn {
            event: event_key,
            operation: NativeRequestOperation::FileOpen,
            decision_id: DecisionId([7; 16]),
            wait_id: 1,
            returned_boottime_ns: 100,
            return_value: 0,
        })
        .await?;
        let view = log
            .activity_context(FactReference::Event(event_key), None)
            .await?
            .0;
        let patterns = view
            .relations
            .iter()
            .filter(|r| r.kind == RelationKind::SameThreadReturnSequence)
            .collect::<Vec<_>>();
        assert_eq!(
            patterns.len(),
            2,
            "all ties at the first retained time are preserved"
        );
        for relation in patterns {
            assert_eq!(relation.from, FactReference::Event(event_key));
            assert!(
                relation
                    .evidence
                    .contains(&FactReference::NativeObservation(done.observation))
            );
        }
        assert!(
            !view
                .relations
                .iter()
                .any(|r| r.kind == RelationKind::ToolExecution)
        );
        assert_eq!(
            view.source_records
                .iter()
                .filter(|r| matches!(r, SourceRecord::RequestHandoff(_)))
                .count(),
            5
        );
        let ids = view
            .source_records
            .iter()
            .filter_map(|r| match r {
                SourceRecord::RequestHandoff(f) => Some(f.observation.sequence),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![1, 2, 2, 3, 4], "raw row intake order is retained");

        let missing = key(1, 8, 99);
        let mut earlier = raw(
            5,
            Some(missing),
            NativeRequestPhase::WorkLeave { work_address: 0 },
        );
        earlier.observed_boottime_ns = 101;
        intake(&log, &[earlier.clone()]).await?;
        let view = log.activity(FactReference::Event(event_key)).await?;
        assert!(
            !view
                .relations
                .iter()
                .any(|r| r.kind == RelationKind::SameThreadReturnSequence),
            "a partial earlier observation is not skipped for a later compatible one"
        );
        assert!(
            view.relations
                .iter()
                .any(|r| r.kind == RelationKind::NativeRequestReference
                    && r.to == FactReference::NativeObservation(missing))
        );
        assert!(
            view.gaps
                .iter()
                .any(|g| g.at == FactReference::NativeObservation(missing)
                    && g.reason == ContextGapReason::NotProvided)
        );
        let mut conflict = done.clone();
        conflict.phase = NativeRequestPhase::Done {
            work_address: 0,
            result: Some(-1),
            result_read_error: None,
        };
        intake(&log, &[conflict.clone()]).await?;
        let view = log
            .activity(FactReference::NativeObservation(done.observation))
            .await?;
        assert!(view.gaps.iter().any(|g| g.at
            == FactReference::NativeObservation(done.observation)
            && g.reason == ContextGapReason::ConflictingEvidence));
        assert!(
            view.source_records
                .iter()
                .any(|r| matches!(r, SourceRecord::RequestHandoff(f) if f.as_ref()==&done))
        );
        assert!(
            view.source_records
                .iter()
                .any(|r| matches!(r, SourceRecord::RequestHandoff(f) if f.as_ref()==&conflict))
        );
        log.close().await?;
        Ok(())
    }
}
