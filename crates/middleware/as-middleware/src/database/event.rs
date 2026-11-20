//! Original questions and operation outcomes in the existing business owner.
use super::{BusinessLog, RecordOutcome, StoreError, StoredReceipt, parts, row, schema, source};
use crate::context::FactReference;
use as_main::messages::protocol::{OperationReport, OperationResult, Up};
use as_types::{
    dispatch_event::{
        EventData, EventPayload, SYSCALL_ARGUMENT_COUNT, StackSnapshot, SyscallSnapshot,
        UserStackSnapshot,
    },
    main_ports::{DispatchOutcome, MainNotice},
    process_tree::EventKey,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn write_event(conn: &Connection, event: &EventData) -> Result<i64, StoreError> {
    let mut header = row::object(event.header())?;
    row::take(&mut header, "actor_cgroup_lineage")?;
    row::take(&mut header, "actor_namespace_tids")?;
    let header = row::put(conn, "event_headers", schema::HEADER, header)?;
    if let Some(lineage) = &event.header().actor_cgroup_lineage {
        for (ordinal, cgroup) in lineage.iter().enumerate() {
            conn.execute(
                "INSERT INTO event_cgroup_lineage VALUES(?1,?2,?3)",
                params![header, ordinal as i64, cgroup.to_be_bytes().as_slice()],
            )?;
        }
    }
    if let Some(tids) = &event.header().actor_namespace_tids {
        let mut insert = conn.prepare(
            "INSERT INTO event_actor_namespace_tids(header,ordinal,tid) VALUES(?1,?2,?3)",
        )?;
        for (ordinal, tid) in tids.iter().enumerate() {
            insert.execute(params![header, ordinal as i64, tid])?;
        }
    }
    let process = source::write_info(conn, &event.process)?;
    let (kind, payload) = match &event.payload {
        EventPayload::File(data) => {
            let mut v = row::object(data)?;
            let mount_effect = row::take(&mut v, "mount_effect")?;
            let device = row::take(&mut v, "device")?;
            let (major, minor) = match device {
                Value::Array(v) if v.len() == 2 => (v[0].clone(), v[1].clone()),
                Value::Null => (Value::Null, Value::Null),
                _ => return Err(StoreError::Invalid),
            };
            v.insert("device_major".into(), major);
            v.insert("device_minor".into(), minor);
            let id = row::put(conn, "file_payloads", schema::FILE, v)?;
            if !mount_effect.is_null() {
                row::put(
                    conn,
                    "file_mount_effects",
                    schema::MOUNT_EFFECT,
                    row::object(&json!({"file_payload":id,"effect":mount_effect}))?,
                )?;
            }
            ("file", id)
        }
        EventPayload::Network(data) => {
            let mut fields = row::object(data)?;
            let snapshot = row::take(&mut fields, "dns_snapshot")?;
            let id = row::put(conn, "network_payloads", schema::NETWORK, fields)?;
            if !snapshot.is_null() {
                write_dns_snapshot(conn, id, snapshot)?;
            }
            ("network", id)
        }
        EventPayload::Process(data) => {
            let mut v = row::object(data)?;
            let argv = row::take(&mut v, "target_argv")?;
            v.insert("target_argv_present".into(), Value::Bool(!argv.is_null()));
            let child = row::take(&mut v, "child_process")?;
            v.insert(
                "child_process".into(),
                if child.is_null() {
                    Value::Null
                } else {
                    let info: as_types::dispatch_event::ProcessInfo = row::decode(child)?;
                    row::integer(source::write_info(conn, &info)?)
                },
            );
            let id = row::put(conn, "process_payloads", schema::PROCESS, v)?;
            if let Value::Array(values) = argv {
                for (n, value) in values.iter().enumerate() {
                    conn.execute(
                        "INSERT INTO argv VALUES(?1,'target_argv',?2,?3)",
                        params![id, n as i64, row::blob(value)?],
                    )?;
                }
            }
            ("process", id)
        }
    };
    conn.execute(
        "INSERT INTO events(header,process,payload_kind,payload) VALUES(?1,?2,?3,?4)",
        params![header, process, kind, payload],
    )?;
    let id = conn.last_insert_rowid();
    if let Some(boot) = event.host_boot_id() {
        conn.execute(
            "INSERT INTO event_boot_scopes(event,boot_id) VALUES(?1,?2)",
            params![id, boot.as_slice()],
        )?;
    }
    write_execution(conn, id, event)?;
    Ok(id)
}

fn write_dns_snapshot(conn: &Connection, owner: i64, snapshot: Value) -> Result<(), StoreError> {
    let Value::Object(mut fields) = snapshot else {
        return Err(StoreError::Invalid);
    };
    let analysis = row::take(&mut fields, "analysis")?;
    fields.insert("network_payload".into(), row::integer(owner));
    for name in [
        "analysis_status",
        "qtype",
        "qclass",
        "opcode",
        "question_count",
        "reason",
    ] {
        fields.insert(name.into(), Value::Null);
    }
    if let Value::Object(mut analysis) = analysis {
        fields.insert(
            "analysis_status".into(),
            row::take(&mut analysis, "status")?,
        );
        for (name, value) in analysis {
            let column = match name.as_str() {
                "qtype" | "qclass" | "opcode" | "reason" => name,
                "count" => "question_count".into(),
                _ => return Err(StoreError::Invalid),
            };
            fields.insert(column, value);
        }
    } else if !analysis.is_null() {
        return Err(StoreError::Invalid);
    }
    row::put(conn, "network_dns_snapshots", schema::DNS_SNAPSHOT, fields)?;
    Ok(())
}

fn read_dns_snapshot(conn: &Connection, owner: i64) -> Result<Value, StoreError> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM network_dns_snapshots WHERE network_payload=?1",
            [owner],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let mut fields = row::get(conn, "network_dns_snapshots", schema::DNS_SNAPSHOT, id)?;
    row::take(&mut fields, "network_payload")?;
    let status = row::take(&mut fields, "analysis_status")?;
    let mut analysis = row::Object::new();
    for (column, name) in [
        ("qtype", "qtype"),
        ("qclass", "qclass"),
        ("opcode", "opcode"),
        ("question_count", "count"),
        ("reason", "reason"),
    ] {
        let value = row::take(&mut fields, column)?;
        if !value.is_null() {
            analysis.insert(name.into(), value);
        }
    }
    let analysis = if status.is_null() {
        if !analysis.is_empty() {
            return Err(StoreError::Invalid);
        }
        Value::Null
    } else {
        analysis.insert("status".into(), status);
        Value::Object(analysis)
    };
    fields.insert("analysis".into(), analysis);
    Ok(Value::Object(fields))
}

fn write_execution(conn: &Connection, id: i64, event: &EventData) -> Result<(), StoreError> {
    if let Some(syscall) = &event.syscall {
        let arguments: [u64; SYSCALL_ARGUMENT_COUNT] = syscall
            .arguments
            .as_slice()
            .try_into()
            .map_err(|_| StoreError::Invalid)?;
        let [arg0, arg1, arg2, arg3, arg4, arg5] = arguments.map(u64::to_be_bytes);
        conn.execute(
            "INSERT INTO event_syscalls(
               event,audit_arch,number,instruction_pointer,stack_pointer,arg0,arg1,arg2,arg3,arg4,arg5
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id,
                syscall.audit_arch,
                syscall.number,
                syscall.instruction_pointer.to_be_bytes().as_slice(),
                syscall.stack_pointer.to_be_bytes().as_slice(),
                arg0.as_slice(),
                arg1.as_slice(),
                arg2.as_slice(),
                arg3.as_slice(),
                arg4.as_slice(),
                arg5.as_slice(),
            ],
        )?;
    }
    if let Some(stack) = &event.kernel_stack {
        write_stack(conn, id, "kernel", stack, None)?;
    }
    if let Some(stack) = &event.user_stack {
        write_stack(conn, id, "user", &stack.snapshot, Some(stack.result))?;
    }
    Ok(())
}

fn write_stack(
    conn: &Connection,
    event: i64,
    kind: &str,
    snapshot: &StackSnapshot,
    result: Option<i32>,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO event_stacks(event,kind,buffer_full,result) VALUES(?1,?2,?3,?4)",
        params![event, kind, snapshot.buffer_full, result],
    )?;
    let stack = conn.last_insert_rowid();
    let mut insert = conn.prepare(
        "INSERT INTO event_stack_frames(stack,ordinal,instruction_pointer) VALUES(?1,?2,?3)",
    )?;
    for (ordinal, pointer) in snapshot.instruction_pointers.iter().enumerate() {
        insert.execute(params![
            stack,
            ordinal as i64,
            pointer.to_be_bytes().as_slice()
        ])?;
    }
    Ok(())
}

fn read_syscall(conn: &Connection, id: i64) -> Result<Option<SyscallSnapshot>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT audit_arch,number,instruction_pointer,stack_pointer,arg0,arg1,arg2,arg3,arg4,arg5
             FROM event_syscalls WHERE event=?1",
            [id],
            |row| {
                let unsigned = |index| row.get::<_, [u8; 8]>(index).map(u64::from_be_bytes);
                Ok(SyscallSnapshot {
                    audit_arch: row.get(0)?,
                    number: row.get(1)?,
                    instruction_pointer: unsigned(2)?,
                    stack_pointer: unsigned(3)?,
                    arguments: (4..4 + SYSCALL_ARGUMENT_COUNT)
                        .map(unsigned)
                        .collect::<rusqlite::Result<_>>()?,
                })
            },
        )
        .optional()?)
}

fn read_stack(
    conn: &Connection,
    event: i64,
    kind: &str,
) -> Result<Option<(StackSnapshot, Option<i32>)>, StoreError> {
    let Some((stack, buffer_full, result)) = conn
        .query_row(
            "SELECT id,buffer_full,result FROM event_stacks WHERE event=?1 AND kind=?2",
            params![event, kind],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?, row.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut statement = conn.prepare(
        "SELECT ordinal,instruction_pointer FROM event_stack_frames WHERE stack=?1 ORDER BY ordinal",
    )?;
    let mut instruction_pointers = Vec::new();
    for row in statement.query_map([stack], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, [u8; 8]>(1)?))
    })? {
        let (ordinal, pointer) = row?;
        if ordinal != instruction_pointers.len() as i64 {
            return Err(StoreError::Invalid);
        }
        instruction_pointers.push(u64::from_be_bytes(pointer));
    }
    Ok(Some((
        StackSnapshot {
            instruction_pointers,
            buffer_full,
        },
        result,
    )))
}
pub(super) fn read_event(conn: &Connection, id: i64) -> Result<EventData, StoreError> {
    let (header, process, kind, payload) = conn.query_row(
        "SELECT header,process,payload_kind,payload FROM events WHERE id=?1",
        [id],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        },
    )?;
    let value = match kind.as_str() {
        "file" => {
            let mut v = row::get(conn, "file_payloads", schema::FILE, payload)?;
            let effect: Option<String> = conn
                .query_row(
                    "SELECT effect FROM file_mount_effects WHERE file_payload=?1",
                    [payload],
                    |r| r.get(0),
                )
                .optional()?;
            v.insert(
                "mount_effect".into(),
                effect.map_or(Value::Null, Value::String),
            );
            let major = row::take(&mut v, "device_major")?;
            let minor = row::take(&mut v, "device_minor")?;
            if major.is_null() != minor.is_null() {
                return Err(StoreError::Invalid);
            }
            v.insert(
                "device".into(),
                if major.is_null() {
                    Value::Null
                } else {
                    json!([major, minor])
                },
            );
            v
        }
        "network" => {
            let mut fields = row::get(conn, "network_payloads", schema::NETWORK, payload)?;
            fields.insert("dns_snapshot".into(), read_dns_snapshot(conn, payload)?);
            fields
        }
        "process" => {
            let mut v = row::get(conn, "process_payloads", schema::PROCESS, payload)?;
            let present = row::take(&mut v, "target_argv_present")?
                .as_bool()
                .ok_or(StoreError::Invalid)?;
            v.insert(
                "target_argv".into(),
                source::argv(conn, "target_argv", payload, present)?,
            );
            let child = row::take(&mut v, "child_process")?;
            v.insert(
                "child_process".into(),
                if child.is_null() {
                    Value::Null
                } else {
                    serde_json::to_value(source::read_info(
                        conn,
                        child.as_i64().ok_or(StoreError::Invalid)?,
                    )?)?
                },
            );
            v
        }
        _ => return Err(StoreError::Invalid),
    };
    let mut h = row::get(conn, "event_headers", schema::HEADER, header)?;
    let mut statement = conn
        .prepare("SELECT cgroup_id FROM event_cgroup_lineage WHERE header=?1 ORDER BY ordinal")?;
    let mut lineage = Vec::new();
    for row in statement.query_map([header], |r| r.get::<_, Vec<u8>>(0))? {
        lineage.push(row::number(u64::from_be_bytes(
            row?.try_into().map_err(|_| StoreError::Invalid)?,
        )));
    }
    h.insert(
        "actor_cgroup_lineage".into(),
        if lineage.is_empty() {
            Value::Null
        } else {
            Value::Array(lineage)
        },
    );
    let mut statement = conn.prepare(
        "SELECT ordinal,tid FROM event_actor_namespace_tids WHERE header=?1 ORDER BY ordinal",
    )?;
    let mut namespace_tids = Vec::new();
    for row in statement.query_map([header], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, u32>(1)?))
    })? {
        let (ordinal, tid) = row?;
        if ordinal != namespace_tids.len() as i64 {
            return Err(StoreError::Invalid);
        }
        namespace_tids.push(tid);
    }
    h.insert(
        "actor_namespace_tids".into(),
        if namespace_tids.is_empty() {
            Value::Null
        } else {
            json!(namespace_tids)
        },
    );
    // The original EventData codec validates the header and payload together.
    let host_boot_id = conn
        .query_row(
            "SELECT boot_id FROM event_boot_scopes WHERE event=?1",
            [id],
            |row| row.get::<_, [u8; 16]>(0),
        )
        .optional()?;
    let kernel_stack = read_stack(conn, id, "kernel")?.map(|(snapshot, _)| snapshot);
    let user_stack = read_stack(conn, id, "user")?
        .map(|(snapshot, result)| {
            result
                .map(|result| UserStackSnapshot { result, snapshot })
                .ok_or(StoreError::Invalid)
        })
        .transpose()?;
    row::decode(json!({
        "header": h,
        "host_boot_id": host_boot_id,
        "process": source::read_info(conn, process)?,
        "payload": value,
        "syscall": read_syscall(conn, id)?,
        "kernel_stack": kernel_stack,
        "user_stack": user_stack,
    }))
}
pub(super) fn write_hit(
    conn: &Connection,
    hit: &as_types::main_ports::RuleHit,
) -> Result<i64, StoreError> {
    let mut v = row::object(hit)?;
    let mut source = row::child(&mut v, "source")?;
    v.insert("source_kind".into(), row::take(&mut source, "kind")?);
    v.insert("source_version".into(), row::take(&mut source, "version")?);
    if !source.is_empty() {
        return Err(StoreError::Invalid);
    }
    row::put(conn, "rule_hits", schema::HIT, v)
}
pub(super) fn read_hit(
    conn: &Connection,
    id: i64,
) -> Result<as_types::main_ports::RuleHit, StoreError> {
    let mut v = row::get(conn, "rule_hits", schema::HIT, id)?;
    let kind = row::take(&mut v, "source_kind")?;
    let version = row::take(&mut v, "source_version")?;
    v.insert("source".into(), json!({"kind":kind,"version":version}));
    row::decode(Value::Object(v))
}
pub(super) fn read_question(conn: &Connection, id: i64) -> Result<Arc<Up>, StoreError> {
    let mut v = row::get(conn, "questions", schema::QUESTION, id)?;
    let decision_id = row::decode(row::take(&mut v, "decision_id")?)?;
    let event = row::take(&mut v, "event")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let hit = row::take(&mut v, "hit")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let timeout = read_question_decision(
        row::take(&mut v, "timeout_type")?,
        row::take(&mut v, "timeout_terminate")?,
    )?;
    let remaining_ms = row::take(&mut v, "remaining_ms")?
        .as_str()
        .ok_or(StoreError::Invalid)?
        .parse()
        .map_err(|_| StoreError::Invalid)?;
    let mut stmt = conn
        .prepare("SELECT type,terminate FROM question_choices WHERE owner=?1 ORDER BY ordinal")?;
    let mut choices = Vec::new();
    for r in stmt.query_map([id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (kind, terminate) = r?;
        choices.push(read_question_decision(json!(kind), json!(terminate))?);
    }
    Ok(Arc::new(Up::Ask {
        decision_id,
        event: Arc::new(read_event(conn, event)?),
        hit: read_hit(conn, hit)?,
        timeout,
        remaining_ms,
        choices,
    }))
}
fn read_question_decision(
    kind: Value,
    terminate: Value,
) -> Result<as_types::main_ports::Decision, StoreError> {
    let mut decision = json!({"type": kind});
    if !terminate.is_null() {
        decision["terminate"] = terminate;
    }
    row::decode(decision)
}
fn same_question(a: &Up, b: &Up) -> bool {
    let (
        Up::Ask {
            decision_id: id,
            event,
            hit,
            choices,
            timeout,
            remaining_ms,
        },
        Up::Ask {
            decision_id: other_id,
            event: other_event,
            hit: other_hit,
            choices: other_choices,
            timeout: other_timeout,
            remaining_ms: other_ms,
        },
    ) = (a, b)
    else {
        return false;
    };
    (id, event, hit, choices, timeout, remaining_ms)
        == (
            other_id,
            other_event,
            other_hit,
            other_choices,
            other_timeout,
            other_ms,
        )
}
pub(super) fn write_question(conn: &Connection, up: &Up) -> Result<i64, StoreError> {
    let Up::Ask {
        decision_id,
        event,
        hit,
        choices,
        timeout,
        remaining_ms,
    } = up
    else {
        return Err(StoreError::Invalid);
    };
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM questions WHERE decision_id=?1",
            [decision_id.0.as_slice()],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return if same_question(read_question(conn, id)?.as_ref(), up) {
            Ok(id)
        } else {
            Err(StoreError::Conflict)
        };
    }
    let timeout = row::object(timeout)?;
    let v = row::object(&json!({
        "decision_id": decision_id,
        "event": write_event(conn, event)?,
        "hit": write_hit(conn, hit)?,
        "timeout_type": timeout["type"],
        "timeout_terminate": timeout.get("terminate"),
        "remaining_ms": remaining_ms.to_string(),
    }))?;
    let id = row::put(conn, "questions", schema::QUESTION, v)?;
    for (n, choice) in choices.iter().enumerate() {
        let v = row::object(choice)?;
        conn.execute(
            "INSERT INTO question_choices VALUES(?1,?2,?3,?4)",
            params![
                id,
                n as i64,
                v.get("type")
                    .and_then(Value::as_str)
                    .ok_or(StoreError::Invalid)?,
                v.get("terminate").and_then(Value::as_str)
            ],
        )?;
    }
    parts::reference(conn, &FactReference::Question(*decision_id), "question", id)?;
    parts::reference(
        conn,
        &FactReference::Event(EventKey::from(event.as_ref())),
        "question",
        id,
    )?;
    super::relations::derive_question(conn, *decision_id)?;
    Ok(id)
}
pub(super) fn write_operation(
    conn: &Connection,
    value: &OperationResult,
    original_kind: &str,
) -> Result<i64, StoreError> {
    let reference = super::key(&FactReference::Completion(EventKey::from(
        value.event.as_ref(),
    )))?;
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM operation_reports WHERE reference=?1",
            [&reference],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return if read_operation(conn, id)?.as_ref() == value {
            Ok(id)
        } else {
            Err(StoreError::Conflict)
        };
    }
    let mut v = row::Object::new();
    for (name, _) in schema::REPORT {
        v.insert((*name).into(), Value::Null);
    }
    v.insert("reference".into(), json!(reference));
    v.insert("event".into(), json!(write_event(conn, &value.event)?));
    v.insert("decision_id".into(), json!(value.decision_id));
    v.insert(
        "hit".into(),
        value
            .hit
            .as_ref()
            .map(|h| write_hit(conn, h))
            .transpose()?
            .map(row::integer)
            .unwrap_or(Value::Null),
    );
    v.insert("original_kind".into(), json!(original_kind));
    let data = row::object(&value.result)?;
    v.insert(
        "report_type".into(),
        data.get("type").cloned().ok_or(StoreError::Invalid)?,
    );
    if let Some(Value::Object(proposal)) = data.get("proposal") {
        let decision = proposal
            .get("decision")
            .and_then(Value::as_object)
            .ok_or(StoreError::Invalid)?;
        v.insert(
            "proposal_type".into(),
            decision.get("type").cloned().ok_or(StoreError::Invalid)?,
        );
        v.insert(
            "proposal_terminate".into(),
            decision.get("terminate").cloned().unwrap_or(Value::Null),
        );
        v.insert(
            "proposal_reason".into(),
            proposal.get("reason").cloned().ok_or(StoreError::Invalid)?,
        );
    }
    match &value.result {
        OperationReport::Receipt { outcome, .. } => {
            match outcome {
                as_types::operation::ExecutionOutcome::Failed { native_error } => {
                    if *native_error >= 0 {
                        return Err(StoreError::Invalid);
                    }
                    v.insert("outcome".into(), json!("failed"));
                    v.insert("outcome_native_error".into(), json!(native_error));
                }
                _ => {
                    v.insert(
                        "outcome".into(),
                        data.get("outcome").cloned().ok_or(StoreError::Invalid)?,
                    );
                }
            }
            let termination = data
                .get("termination")
                .and_then(Value::as_object)
                .ok_or(StoreError::Invalid)?;
            v.insert(
                "termination_requested".into(),
                termination
                    .get("requested")
                    .cloned()
                    .ok_or(StoreError::Invalid)?,
            );
            for role in ["actor", "parent"] {
                let signal = termination
                    .get(role)
                    .and_then(Value::as_object)
                    .ok_or(StoreError::Invalid)?;
                v.insert(
                    format!("{role}_signal"),
                    signal.get("status").cloned().ok_or(StoreError::Invalid)?,
                );
                v.insert(
                    format!("{role}_error"),
                    signal.get("native_error").cloned().unwrap_or(Value::Null),
                );
            }
        }
        OperationReport::Failed { .. } => {
            v.insert(
                "failure_code".into(),
                data.get("code").cloned().ok_or(StoreError::Invalid)?,
            );
            v.insert(
                "failure_message".into(),
                data.get("message").cloned().ok_or(StoreError::Invalid)?,
            );
        }
    }
    let id = row::put(conn, "operation_reports", schema::REPORT, v)?;
    let event = EventKey::from(value.event.as_ref());
    parts::reference(conn, &FactReference::Completion(event), "operation", id)?;
    parts::reference(conn, &FactReference::Event(event), "operation", id)?;
    super::relations::derive_completion(conn, value)?;
    Ok(id)
}
pub(super) fn read_operation(
    conn: &Connection,
    id: i64,
) -> Result<Arc<OperationResult>, StoreError> {
    let mut v = row::get(conn, "operation_reports", schema::REPORT, id)?;
    let event = row::take(&mut v, "event")?
        .as_i64()
        .ok_or(StoreError::Invalid)?;
    let hit = row::take(&mut v, "hit")?;
    let decision_id = row::decode(row::take(&mut v, "decision_id")?)?;
    let proposal_type = row::take(&mut v, "proposal_type")?;
    let proposal = if proposal_type.is_null() {
        Value::Null
    } else {
        let mut decision = json!({"type":proposal_type});
        let terminate = row::take(&mut v, "proposal_terminate")?;
        if !terminate.is_null() {
            decision["terminate"] = terminate;
        }
        json!({"decision":decision,"reason":row::take(&mut v,"proposal_reason")?})
    };
    let report_type = row::take(&mut v, "report_type")?;
    let native_error = row::take(&mut v, "outcome_native_error")?;
    let result = if report_type == "receipt" {
        let outcome = row::take(&mut v, "outcome")?;
        let outcome = if outcome == "failed" {
            let code: i32 = row::decode(native_error)?;
            if code >= 0 {
                return Err(StoreError::Invalid);
            }
            json!({"failed":{"native_error":code}})
        } else {
            if !native_error.is_null() {
                return Err(StoreError::Invalid);
            }
            outcome
        };
        let mut termination = json!({"requested":row::take(&mut v,"termination_requested")?});
        for role in ["actor", "parent"] {
            let status = row::take(&mut v, &format!("{role}_signal"))?;
            let error = row::take(&mut v, &format!("{role}_error"))?;
            let mut signal = json!({"status":status});
            if !error.is_null() {
                signal["native_error"] = error;
            }
            termination[role] = signal;
        }
        json!({"type":"receipt","proposal":proposal,"outcome":outcome,"termination":termination})
    } else {
        if !native_error.is_null() {
            return Err(StoreError::Invalid);
        }
        json!({"type":report_type,"proposal":proposal,"code":row::take(&mut v,"failure_code")?,"message":row::take(&mut v,"failure_message")?})
    };
    Ok(Arc::new(OperationResult {
        decision_id,
        event: Arc::new(read_event(conn, event)?),
        hit: if hit.is_null() {
            None
        } else {
            Some(read_hit(conn, hit.as_i64().ok_or(StoreError::Invalid)?)?)
        },
        result: row::decode(result)?,
    }))
}
impl BusinessLog {
    /// Start storing an actual Ask or operation result before returning its completion future.
    /// Dropping that future does not cancel the database owner's admitted SQL work.
    pub fn record_up(
        &self,
        up: Arc<Up>,
    ) -> Result<
        impl std::future::Future<Output = Result<RecordOutcome, StoreError>> + Send + 'static + use<>,
        StoreError,
    > {
        let owner = self.inner.clone();
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            let kind = match up.as_ref() {
                Up::Ask { .. } => {
                    write_question(&tx, &up)?;
                    "question"
                }
                Up::OperationResult(value) => {
                    write_operation(&tx, value, "up")?;
                    "operation"
                }
                _ => return Ok(RecordOutcome::NotApplicable),
            };
            let record = super::record(&tx, kind)?;
            tx.commit()?;
            let mut live = owner
                .questions
                .lock()
                .map_err(|_| StoreError::Unconfirmed)?;
            match up.as_ref() {
                Up::Ask { decision_id, .. } => {
                    if !conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM operation_reports WHERE decision_id=?1)",
                        [decision_id.0.as_slice()],
                        |r| r.get::<_, bool>(0),
                    )? {
                        live.insert(decision_id.0);
                    }
                }
                Up::OperationResult(value) => {
                    if let Some(id) = value.decision_id {
                        live.remove(&id.0);
                    }
                }
                _ => {}
            }
            Ok(RecordOutcome::Stored(StoredReceipt { record }))
        })
    }
    /// Store the message owner's one completion projection, retaining its actual internal source.
    pub(crate) async fn record_completion(
        &self,
        notice: &MainNotice,
        value: Arc<OperationResult>,
    ) -> Result<RecordOutcome, StoreError> {
        let kind = match &notice.result {
            Ok(DispatchOutcome::Driver { .. }) => "driver",
            Ok(DispatchOutcome::Execution(_)) => "execution",
            Ok(DispatchOutcome::Observed) => return Ok(RecordOutcome::NotApplicable),
            Err(_) => "error",
        };
        let owner = self.inner.clone();
        self.write_call(move |conn| {
            let tx = conn.transaction()?;
            write_operation(&tx, &value, kind)?;
            let record = super::record(&tx, "operation")?;
            tx.commit()?;
            if let Some(id) = value.decision_id {
                owner
                    .questions
                    .lock()
                    .map_err(|_| StoreError::Unconfirmed)?
                    .remove(&id.0);
            }
            Ok(RecordOutcome::Stored(StoredReceipt { record }))
        })?
        .await
    }
}
