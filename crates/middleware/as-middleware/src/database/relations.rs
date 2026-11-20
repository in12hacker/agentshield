//! Business relationship persistence; source matching stays in middleware activity, never Main.
use super::{BusinessLog, StoreError, StoredReceipt, chat, event, parts, row, source};
use crate::{
    activity::{activity_view, exchange_relation, protocol_in_scope, question_completion},
    context::*,
};
use as_main::messages::protocol::{OperationResult, Up};
use as_types::{flow::*, process_tree::EventKey};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn add(conn: &Connection, relation: &EvidenceRelation) -> Result<(), StoreError> {
    if relation.evidence.is_empty() {
        return Err(StoreError::NotProvided);
    }
    conn.execute(
        "INSERT OR IGNORE INTO relations(kind,from_ref,to_ref) VALUES(?1,?2,?3)",
        params![
            row::string(&relation.kind)?,
            super::key(&relation.from)?,
            super::key(&relation.to)?
        ],
    )?;
    let id = conn.query_row(
        "SELECT id FROM relations WHERE kind=?1 AND from_ref=?2 AND to_ref=?3",
        params![
            row::string(&relation.kind)?,
            super::key(&relation.from)?,
            super::key(&relation.to)?
        ],
        |r| r.get::<_, i64>(0),
    )?;
    for (n, reference) in relation.evidence.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO relation_evidence VALUES(?1,?2,?3)",
            params![id, n as i64, super::key(reference)?],
        )?;
    }
    Ok(())
}
pub(super) fn gap(conn: &Connection, value: &ContextGap) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO context_gaps(at_ref,reason) VALUES(?1,?2)",
        params![super::key(&value.at)?, row::string(&value.reason)?],
    )?;
    Ok(())
}
pub(super) fn read(conn: &Connection, id: i64) -> Result<EvidenceRelation, StoreError> {
    let (kind, from, to) = conn.query_row(
        "SELECT kind,from_ref,to_ref FROM relations WHERE id=?1",
        [id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut stmt =
        conn.prepare("SELECT reference FROM relation_evidence WHERE owner=?1 ORDER BY ordinal")?;
    let evidence = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .map(|v| Ok(serde_json::from_str(&v?)?))
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(EvidenceRelation {
        kind: row::named(&kind)?,
        from: serde_json::from_str(&from)?,
        to: serde_json::from_str(&to)?,
        evidence,
    })
}
pub(super) fn derive_completion(
    conn: &Connection,
    result: &OperationResult,
) -> Result<(), StoreError> {
    let Some(decision) = result.decision_id else {
        return Ok(());
    };
    let Some(id) = conn
        .query_row(
            "SELECT id FROM questions WHERE decision_id=?1",
            [decision.0.as_slice()],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(());
    };
    let question = event::read_question(conn, id)?;
    let Up::Ask {
        decision_id, event, ..
    } = question.as_ref()
    else {
        return Err(StoreError::Invalid);
    };
    match question_completion(*decision_id, event, result) {
        Ok(Some(relation)) => add(conn, &relation),
        Ok(None) => Ok(()),
        Err(error) => gap(conn, &error),
    }
}
pub(super) fn derive_question(
    conn: &Connection,
    decision: as_types::DecisionId,
) -> Result<(), StoreError> {
    let mut stmt = conn.prepare("SELECT id FROM operation_reports WHERE decision_id=?1")?;
    let ids = stmt
        .query_map([decision.0.as_slice()], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        derive_completion(conn, event::read_operation(conn, id)?.as_ref())?;
    }
    Ok(())
}
fn protocol_proofs(
    conn: &Connection,
    message: &ChatMessage,
    persist_exchange: bool,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<i64>,
) -> Result<(), StoreError> {
    let natives = message
        .observed_parts
        .iter()
        .filter_map(|p| match p {
            ChatPart::ToolCall {
                native_id: Some(n), ..
            }
            | ChatPart::ToolResult {
                native_id: Some(n), ..
            } => Some(n),
            _ => None,
        })
        .collect::<Vec<_>>();
    if natives.is_empty() && !persist_exchange {
        return Ok(());
    }
    let mut metadata = conn.prepare(
        "SELECT id,source FROM protocol_records
         WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3",
    )?;
    for reference in &message.record_refs {
        let Some((id, source_id)) = metadata
            .query_row(
                params![
                    reference.exchange.flow_instance.as_slice(),
                    reference.exchange.sequence.to_be_bytes().as_slice(),
                    reference.sequence.to_be_bytes().as_slice(),
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
        else {
            continue;
        };
        let original_source = source::read(conn, source_id)?;
        let scoped = if persist_exchange {
            match exchange_relation(message, *reference, &original_source) {
                Ok(relation) => {
                    add(conn, &relation)?;
                    true
                }
                Err(error) => {
                    gap(conn, &error)?;
                    false
                }
            }
        } else {
            protocol_in_scope(message, *reference, &original_source)
        };
        if !scoped || natives.is_empty() || seen.contains(&id) {
            continue;
        }
        let original = parts::read_native(conn, "protocol_native_ids", id)?;
        if natives.iter().any(|n| original.contains(n)) {
            seen.insert(id);
            records.push(SourceRecord::Protocol(Arc::new(chat::read_protocol(
                conn, id,
            )?)));
        }
    }
    Ok(())
}
pub(super) fn tool_part_candidates(
    conn: &Connection,
    message: &ChatMessage,
) -> Result<BTreeSet<i64>, StoreError> {
    let mut candidates = BTreeSet::new();
    for part in &message.observed_parts {
        let (native, kind, namespace) = match part {
            ChatPart::ToolResult {
                native_id: Some(native),
                ..
            } => (native, "tool_call", "tool_call"),
            ChatPart::ToolCall {
                native_id: Some(native),
                ..
            } => (native, "tool_result", "tool_result_reference"),
            _ => continue,
        };
        let (value_kind, value) = match &native.value {
            NativeIdValue::JsonString(v) => ("json_string", v),
            NativeIdValue::JsonNumber(v) => ("json_number", v),
        };
        let mut stmt=conn.prepare("SELECT DISTINCT m.id,r.source FROM part_native_ids n JOIN parts p ON p.id=n.owner JOIN message_revisions r ON r.parts=p.set_id JOIN messages m ON m.id=r.message WHERE p.type=?1 AND n.namespace=?2 AND n.value_kind=?3 AND n.value=?4 AND r.revision=(SELECT max(revision) FROM message_revisions WHERE message=m.id)")?;
        for candidate in stmt.query_map(params![kind, namespace, value_kind, value], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (id, source_id) = candidate?;
            let candidate_source = source::read(conn, source_id)?;
            if candidate_source == *message.source
                || crate::activity::tool_source_domain(&candidate_source, &message.source)
            {
                candidates.insert(id);
            }
        }
    }
    Ok(candidates)
}

/// Locate original model Parts only by the already captured native tool ID and process lifetime.
pub(super) fn native_tool_candidates(
    conn: &Connection,
    records: &[SourceRecord],
) -> Result<BTreeSet<i64>, StoreError> {
    let mut candidates = BTreeSet::new();
    let mut statement = conn.prepare("SELECT DISTINCT m.id FROM part_native_ids n JOIN parts p ON p.id=n.owner JOIN message_revisions r ON r.parts=p.set_id JOIN messages m ON m.id=r.message JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor JOIN process_keys k ON k.id=a.process WHERE n.namespace='tool_call' AND n.value_kind='json_string' AND n.value=?1 AND p.type='tool_call' AND m.origin='response_output' AND s.side='upstream_response' AND k.boot_id=?2 AND k.host_tgid=?3 AND k.birth_boottime_ns=?4 AND r.revision=(SELECT max(revision) FROM message_revisions WHERE message=m.id)")?;
    let mut response = conn.prepare("SELECT DISTINCT m.id FROM protocol_native_ids n JOIN protocol_records p ON p.id=n.owner JOIN message_record_refs q ON q.flow_instance=p.flow_instance AND q.exchange_sequence=p.exchange_sequence AND q.sequence=p.sequence JOIN message_revisions r ON r.id=q.owner JOIN messages m ON m.id=r.message JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor JOIN process_keys k ON k.id=a.process WHERE n.namespace='response' AND n.value_kind='json_string' AND n.value=?1 AND m.origin='response_output' AND s.side='upstream_response' AND k.boot_id=?2 AND k.host_tgid=?3 AND k.birth_boottime_ns=?4 AND r.revision=(SELECT max(revision) FROM message_revisions WHERE message=m.id) AND EXISTS(SELECT 1 FROM parts x WHERE x.set_id=r.parts AND x.type='tool_call')")?;
    for record in records {
        let SourceRecord::RequestHandoff(fact) = record else {
            continue;
        };
        let as_types::runtime_observation::NativeRequestPhase::Submission {
            analysis: Some(analysis),
            ..
        } = &fact.phase
        else {
            continue;
        };
        for candidate in &analysis.candidates {
            let context = &candidate.context;
            for owner in statement.query_map(
                params![
                    context.tool_call_id,
                    context.process.boot_id.as_slice(),
                    context.process.host_tgid,
                    context.process.birth_boottime_ns.to_be_bytes().as_slice()
                ],
                |row| row.get::<_, i64>(0),
            )? {
                candidates.insert(owner?);
            }
            if let Some(id) = &context.response_id {
                for owner in response.query_map(
                    params![
                        id,
                        context.process.boot_id.as_slice(),
                        context.process.host_tgid,
                        context.process.birth_boottime_ns.to_be_bytes().as_slice()
                    ],
                    |row| row.get::<_, i64>(0),
                )? {
                    candidates.insert(owner?);
                }
            }
        }
    }
    let mut recent = conn.prepare(
        "SELECT m.id FROM message_revisions r
         JOIN messages m ON m.id=r.message JOIN protocol_sources s ON s.id=r.source
         JOIN actors a ON a.id=s.actor JOIN process_keys k ON k.id=a.process
         WHERE k.boot_id=?1 AND k.host_tgid=?2 AND k.birth_boottime_ns=?3
           AND (?4 IS NULL OR r.last_seen_boottime_ns<=?4)
           AND r.revision=(SELECT max(revision) FROM message_revisions WHERE message=m.id)
           AND m.origin='response_output' AND s.side='upstream_response'
           AND EXISTS(SELECT 1 FROM parts p WHERE p.set_id=r.parts AND p.type='tool_call')
         ORDER BY r.last_seen_boottime_ns DESC,m.id DESC LIMIT 20",
    )?;
    for (process, _, before, _) in crate::activity::tool_candidate_scopes(records) {
        for owner in recent.query_map(
            params![
                process.boot_id.as_slice(),
                process.host_tgid,
                process.birth_boottime_ns.to_be_bytes().as_slice(),
                before
                    .map(u64::to_be_bytes)
                    .as_ref()
                    .map(|value| value.as_slice())
            ],
            |row| row.get::<_, i64>(0),
        )? {
            candidates.insert(owner?);
        }
    }
    let mut container = conn.prepare(
        "SELECT m.id FROM message_revisions r JOIN messages m ON m.id=r.message
         JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor
         JOIN process_keys k ON k.id=a.process JOIN process_info info ON info.id=a.info
         WHERE k.boot_id=?1 AND info.workload_kind='container' AND info.workload_runtime=?2
           AND info.workload_namespace IS ?3 AND info.workload_id=?4 AND r.last_seen_boottime_ns<=?5
           AND r.revision=(SELECT max(revision) FROM message_revisions WHERE message=m.id)
           AND m.origin='response_output' AND s.side='upstream_response'
           AND EXISTS(SELECT 1 FROM parts p WHERE p.set_id=r.parts AND p.type='tool_call')
         ORDER BY r.last_seen_boottime_ns DESC,m.id DESC LIMIT 20",
    )?;
    for (instance, boot, _, before) in crate::activity::tool_candidate_containers(records) {
        for owner in container.query_map(
            params![
                boot.as_slice(),
                instance.runtime,
                instance.namespace,
                instance.id,
                before.to_be_bytes().as_slice()
            ],
            |row| row.get::<_, i64>(0),
        )? {
            candidates.insert(owner?);
        }
    }
    Ok(candidates)
}

fn derive_message(conn: &Connection, owner: i64) -> Result<(), StoreError> {
    let message = Arc::new(chat::read_message(conn, owner)?);
    let mut records = vec![SourceRecord::Message(message.clone())];
    let mut seen = BTreeSet::new();
    protocol_proofs(conn, &message, true, &mut records, &mut seen)?;
    let candidates = tool_part_candidates(conn, &message)?;
    for candidate in candidates {
        if candidate == owner {
            continue;
        }
        let value = Arc::new(chat::read_message(conn, candidate)?);
        protocol_proofs(conn, &value, false, &mut records, &mut seen)?;
        records.push(SourceRecord::Message(value));
    }
    let anchor = FactReference::Message(message.id);
    let view = activity_view(anchor.clone(), records, vec![]);
    for relation in view
        .relations
        .into_iter()
        .filter(|relation| relation.kind != RelationKind::SameExchange || relation.to != anchor)
    {
        add(conn, &relation)?;
    }
    for value in view.gaps {
        gap(conn, &value)?;
    }
    Ok(())
}
pub(super) fn derive_chat(conn: &Connection, event: &ChatEvent) -> Result<(), StoreError> {
    match event {
        ChatEvent::Started(m) | ChatEvent::Finished(m) => {
            if let Some(id) = chat::identity(
                conn,
                "messages",
                &m.id.exchange.flow_instance,
                m.id.exchange.sequence,
                m.id.sequence,
            )? {
                derive_message(conn, id)?;
            }
        }
        ChatEvent::Observation(p) => {
            let mut stmt=conn.prepare("SELECT DISTINCT r.message FROM message_record_refs q JOIN message_revisions r ON r.id=q.owner WHERE q.flow_instance=?1 AND q.exchange_sequence=?2 AND q.sequence=?3")?;
            let ids = stmt
                .query_map(
                    params![
                        p.id.exchange.flow_instance.as_slice(),
                        p.id.exchange.sequence.to_be_bytes().as_slice(),
                        p.id.sequence.to_be_bytes().as_slice()
                    ],
                    |r| r.get::<_, i64>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            for id in ids {
                derive_message(conn, id)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn feedback(
    conn: &Connection,
    delivery: i64,
    fact: &RuleFeedbackFact,
) -> Result<i64, StoreError> {
    let denial = super::key(&FactReference::Event(fact.denial))?;
    let call = super::key(&FactReference::Part(fact.call))?;
    let bound = conn
        .query_row(
            "SELECT denial_ref,call_ref FROM feedback_bindings WHERE denial_ref=?1 OR call_ref=?2",
            params![denial, call],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    if bound.is_some_and(|(d, c)| d != denial || c != call) {
        return Err(StoreError::Conflict);
    }
    let (state, original, evidence, failure) = match &fact.state {
        RuleFeedbackState::Prepared => ("prepared", None, None, None),
        RuleFeedbackState::Applied {
            original,
            modified_body,
        } => (
            "applied",
            Some(*original),
            Some(parts::write_evidence(conn, modified_body)?),
            None,
        ),
        RuleFeedbackState::Submitted { original } => ("submitted", Some(*original), None, None),
        RuleFeedbackState::Failed { original, code } => {
            ("failed", *original, None, Some(row::string(code)?))
        }
    };
    conn.execute("INSERT INTO feedback(delivery,denial_ref,call_ref,state,original_flow,original_exchange,original_sequence,evidence,failure_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![delivery,denial,call,state,original.as_ref().map(|v|v.exchange.flow_instance.as_slice()),original.map(|v|v.exchange.sequence.to_be_bytes().to_vec()),original.map(|v|v.sequence.to_be_bytes().to_vec()),evidence,failure])?;
    let id = conn.last_insert_rowid();
    let old = conn
        .query_row(
            "SELECT last_fact FROM feedback_bindings WHERE denial_ref=?1",
            [&denial],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();
    let advance = if let Some(old) = old {
        let previous = read_feedback(conn, old)?;
        let rank = |s: &RuleFeedbackState| match s {
            RuleFeedbackState::Prepared => 0,
            RuleFeedbackState::Applied { .. } => 1,
            _ => 2,
        };
        rank(&fact.state) >= rank(&previous.state)
    } else {
        true
    };
    conn.execute("INSERT INTO feedback_bindings(denial_ref,call_ref,last_fact) VALUES(?1,?2,?3) ON CONFLICT(denial_ref) DO UPDATE SET last_fact=CASE WHEN ?4 THEN excluded.last_fact ELSE last_fact END",params![denial,call,id,advance])?;
    Ok(id)
}
pub(super) fn read_feedback(conn: &Connection, id: i64) -> Result<RuleFeedbackFact, StoreError> {
    let(d,c,state,f,e,s,body,code)=conn.query_row("SELECT denial_ref,call_ref,state,original_flow,original_exchange,original_sequence,evidence,failure_code FROM feedback WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<Vec<u8>>>(3)?,r.get::<_,Option<Vec<u8>>>(4)?,r.get::<_,Option<Vec<u8>>>(5)?,r.get::<_,Option<i64>>(6)?,r.get::<_,Option<String>>(7)?)))?;
    let FactReference::Event(denial) = serde_json::from_str(&d)? else {
        return Err(StoreError::Invalid);
    };
    let FactReference::Part(call) = serde_json::from_str(&c)? else {
        return Err(StoreError::Invalid);
    };
    let original = match (f, e, s) {
        (Some(f), Some(e), Some(s)) => Some(ProtocolRecordId {
            exchange: ExchangeKey {
                flow_instance: f.try_into().map_err(|_| StoreError::Invalid)?,
                sequence: parts::u64_blob(e)?,
            },
            sequence: parts::u64_blob(s)?,
        }),
        (None, None, None) => None,
        _ => return Err(StoreError::Invalid),
    };
    let state = match state.as_str() {
        "prepared" => RuleFeedbackState::Prepared,
        "applied" => RuleFeedbackState::Applied {
            original: original.ok_or(StoreError::Invalid)?,
            modified_body: parts::read_evidence(conn, body.ok_or(StoreError::Invalid)?)?.into(),
        },
        "submitted" => RuleFeedbackState::Submitted {
            original: original.ok_or(StoreError::Invalid)?,
        },
        "failed" => RuleFeedbackState::Failed {
            original,
            code: row::named(&code.ok_or(StoreError::Invalid)?)?,
        },
        _ => return Err(StoreError::Invalid),
    };
    Ok(RuleFeedbackFact {
        denial,
        call,
        state,
    })
}

impl BusinessLog {
    /// Persist a middleware-verified protocol binding for existing native execution facts.
    pub async fn bind_tool_execution(
        &self,
        call: ChatPartRef,
        event: EventKey,
    ) -> Result<StoredReceipt, StoreError> {
        let failure_owner = self.clone();
        self.job(
            move |conn| {
                let from = FactReference::Part(call);
                let to = FactReference::Event(event);
                let reference = super::key(&to)?;
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM native_tool_executions WHERE event_ref=?1)",
                    [reference],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(StoreError::NotProvided);
                }
                let result = (|| {
                    let tx = conn.transaction()?;
                    super::parts::ensure_reference(&tx, &from, "message")?;
                    super::parts::ensure_reference(&tx, &to, "event")?;
                    let relation = EvidenceRelation {
                        kind: RelationKind::ToolExecution,
                        evidence: vec![from.clone(), to.clone()],
                        from,
                        to,
                    };
                    add(&tx, &relation)?;
                    let view = super::query::activity(&tx, FactReference::Event(event))?;
                    // Check the candidate's original facts before deriving competition between bindings.
                    let candidate = ContextView {
                        anchor: view.anchor,
                        source_records: view.source_records,
                        relations: Vec::new(),
                        gaps: Vec::new(),
                    };
                    crate::activity::validate_execution_link(&candidate, &relation).map_err(
                        |gap| match gap.reason {
                            ContextGapReason::ConflictingEvidence => StoreError::Conflict,
                            _ => StoreError::NotProvided,
                        },
                    )?;
                    let record = super::record(&tx, "tool_execution")?;
                    tx.commit()?;
                    Ok(StoredReceipt { record })
                })();
                match result {
                    Err(StoreError::Conflict | StoreError::NotProvided) => result,
                    other => failure_owner.write_result(other),
                }
            },
            true,
            false,
        )?
        .await
    }
}
