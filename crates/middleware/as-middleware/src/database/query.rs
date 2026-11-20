//! SQL-only temporary activity reads and frozen question windows; no history cache.
use super::{BusinessLog, StoreError, chat, event, relations};
use crate::{
    activity::{
        activity_view, execution_call, message, original_completion, part_id, protocol_in_scope,
        request_for_response,
    },
    audit::{AskMessageWindow, WindowRead},
    context::*,
    execution_feedback::has_fixed_denial,
};
use as_main::messages::protocol::Up;
use as_types::{
    DecisionId,
    flow::*,
    process_tree::{EventKey, ProcessKey},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeSet, sync::Arc};

/// Find original string identifiers in a verified business archive, scoped to a real process.
/// The caller owns the read-only archive connection and transaction.
pub(crate) fn protocol_references(
    conn: &Connection,
    process: ProcessKey,
    side: ProtocolSide,
    namespace: NativeIdNamespace,
    origin: Option<&str>,
    value: &str,
) -> Result<Vec<ProtocolRecordId>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT r.flow_instance, r.exchange_sequence, r.sequence
         FROM protocol_native_ids n JOIN protocol_records r ON r.id=n.owner
         JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor
         JOIN process_keys p ON p.id=a.process
         WHERE p.boot_id=?1 AND p.host_tgid=?2 AND p.birth_boottime_ns=?3
           AND s.side=?4 AND n.namespace=?5 AND (?6 IS NULL OR s.origin=?6) AND n.value_kind='json_string' AND n.value=?7
           AND NOT EXISTS(SELECT 1 FROM record_quality q WHERE q.owner=r.id)
         ORDER BY r.flow_instance, r.exchange_sequence, r.sequence",
    )?;
    let rows = statement.query_map(
        params![
            process.boot_id.as_slice(),
            process.host_tgid,
            process.birth_boottime_ns.to_be_bytes().as_slice(),
            super::row::string(&side)?,
            super::row::string(&namespace)?,
            origin,
            value
        ],
        |r| {
            Ok(ProtocolRecordId {
                exchange: ExchangeKey {
                    flow_instance: r.get(0)?,
                    sequence: u64::from_be_bytes(r.get(1)?),
                },
                sequence: u64::from_be_bytes(r.get(2)?),
            })
        },
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Keep only tool results sent within the same original provider and credential scope as the call.
pub(crate) fn same_tool_scope(
    conn: &Connection,
    call: ProtocolRecordId,
    result: ProtocolRecordId,
) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (
          SELECT 1 FROM protocol_records c JOIN protocol_sources cs ON cs.id=c.source,
                        protocol_records r JOIN protocol_sources rs ON rs.id=r.source
          WHERE c.flow_instance=?1 AND c.exchange_sequence=?2 AND c.sequence=?3
            AND r.flow_instance=?4 AND r.exchange_sequence=?5 AND r.sequence=?6
            AND cs.origin=rs.origin AND cs.profile=rs.profile AND cs.route_label=rs.route_label
            AND cs.config_version=rs.config_version AND cs.actual_auth_scope=rs.actual_auth_scope)",
        params![
            call.exchange.flow_instance.as_slice(),
            call.exchange.sequence.to_be_bytes().as_slice(),
            call.sequence.to_be_bytes().as_slice(),
            result.exchange.flow_instance.as_slice(),
            result.exchange.sequence.to_be_bytes().as_slice(),
            result.sequence.to_be_bytes().as_slice(),
        ],
        |row| row.get(0),
    )?)
}

/// Resolve a response only inside the exchange that received the original tracing header.
pub(crate) fn traced_response_references(
    conn: &Connection,
    process: ProcessKey,
    origin: &str,
    trace_id: &str,
    span_id: &str,
    response_id: &str,
) -> Result<Vec<ProtocolRecordId>, StoreError> {
    let header_start = format!("00-{trace_id}-{span_id}-");
    let mut statement = conn.prepare(
        "SELECT DISTINCT r.flow_instance,r.exchange_sequence,r.sequence
         FROM protocol_native_ids n JOIN protocol_records r ON r.id=n.owner
         JOIN protocol_sources s ON s.id=r.source JOIN actors a ON a.id=s.actor
         JOIN channels c ON c.id=s.channel
         JOIN process_keys p ON p.id=a.process
         WHERE n.namespace='response' AND n.value_kind='json_string' AND n.value=?1
           AND s.side='upstream_response' AND s.origin=?2
           AND p.boot_id=?3 AND p.host_tgid=?4 AND p.birth_boottime_ns=?5
           AND NOT EXISTS(SELECT 1 FROM record_quality q WHERE q.owner=r.id)
           AND EXISTS (
             SELECT 1 FROM protocol_records rr JOIN protocol_sources ss ON ss.id=rr.source
             JOIN actors aa ON aa.id=ss.actor JOIN process_keys pp ON pp.id=aa.process
             JOIN channels cc ON cc.id=ss.channel
             WHERE rr.flow_instance=r.flow_instance AND rr.exchange_sequence=r.exchange_sequence
               AND ss.side='client_request' AND substr(ss.request_traceparent,1,53)=?6
               AND ss.origin=s.origin AND ss.profile=s.profile AND ss.route_label=s.route_label
               AND ss.config_version=s.config_version AND cc.kind=c.kind
               AND cc.source_instance=c.source_instance AND cc.object_id=c.object_id
               AND cc.generation=c.generation
               AND ss.actual_auth_scope=s.actual_auth_scope
               AND pp.boot_id=p.boot_id AND pp.host_tgid=p.host_tgid
               AND pp.birth_boottime_ns=p.birth_boottime_ns
               AND NOT EXISTS(SELECT 1 FROM record_quality q WHERE q.owner=rr.id)
           ) ORDER BY r.flow_instance,r.exchange_sequence,r.sequence",
    )?;
    let rows = statement.query_map(
        params![
            response_id,
            origin,
            process.boot_id.as_slice(),
            process.host_tgid,
            process.birth_boottime_ns.to_be_bytes().as_slice(),
            header_start
        ],
        |r| {
            Ok(ProtocolRecordId {
                exchange: ExchangeKey {
                    flow_instance: r.get(0)?,
                    sequence: u64::from_be_bytes(r.get(1)?),
                },
                sequence: u64::from_be_bytes(r.get(2)?),
            })
        },
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load(
    conn: &Connection,
    reference: &FactReference,
    records: &mut Vec<SourceRecord>,
    seen: &mut BTreeSet<(String, i64)>,
) -> Result<(), StoreError> {
    let mut stmt =
        conn.prepare("SELECT record_kind,record_id FROM fact_records WHERE reference=?1")?;
    for item in stmt.query_map([super::key(reference)?], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (kind, id) = item?;
        if kind == "native_request_handoff" {
            // Load the explicitly bound lifetime together below, in original intake row order.
            continue;
        }
        if !seen.insert((kind.clone(), id)) {
            continue;
        }
        records.push(match kind.as_str() {
            "question" => SourceRecord::Question(event::read_question(conn, id)?),
            "operation" => SourceRecord::OperationResult(event::read_operation(conn, id)?),
            "protocol" => SourceRecord::Protocol(Arc::new(chat::read_protocol(conn, id)?)),
            "message" => SourceRecord::Message(Arc::new(chat::read_message(conn, id)?)),
            "native_tool" => {
                SourceRecord::ToolExecution(Arc::new(super::native_execution::read(conn, id)?))
            }
            "native_request_return" => SourceRecord::RequestReturn(Arc::new(
                super::native_execution::read_request_return(conn, id)?,
            )),
            "native_futex_selection" => SourceRecord::FutexSelection(Arc::new(
                super::native_execution::read_futex_selection(conn, id)?,
            )),
            _ => return Err(StoreError::Invalid),
        });
    }
    if let FactReference::Event(event) = reference {
        super::native_handoff::load_event(conn, *event, records, seen)?;
    } else if let FactReference::NativeObservation(observation) = reference {
        super::native_handoff::load_observation(conn, *observation, records, seen)?;
    }
    Ok(())
}
pub(super) fn activity(
    conn: &Connection,
    anchor: FactReference,
) -> Result<ContextView, StoreError> {
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    load(conn, &anchor, &mut records, &mut seen)?;
    if records.is_empty() {
        return Err(StoreError::NotProvided);
    }
    if let FactReference::Part(part) = &anchor
        && !records.iter().any(|record| matches!(record, SourceRecord::Message(message)
            if message.id == part.message && message.observed_parts.iter().any(|value| part_id(value) == part.part_id)))
    {
        return Err(StoreError::NotProvided);
    }
    let mut references = vec![anchor.clone()];
    let mut tool_calls = Vec::new();
    for record in &records {
        match record {
            SourceRecord::Question(q) => {
                if let Up::Ask { event, .. } = q.as_ref() {
                    references.push(FactReference::Event(EventKey::from(event.as_ref())));
                }
            }
            SourceRecord::OperationResult(c) => {
                references.push(FactReference::Event(EventKey::from(c.event.as_ref())))
            }
            SourceRecord::RequestHandoff(fact) => {
                if let as_types::runtime_observation::NativeRequestPhase::EventBound {
                    event, ..
                } = fact.phase
                {
                    references.push(FactReference::Event(event));
                }
            }
            SourceRecord::Message(message) => {
                for part in &message.observed_parts {
                    if !matches!(
                        part,
                        ChatPart::ToolCall { .. } | ChatPart::ToolResult { .. }
                    ) {
                        continue;
                    }
                    let reference = FactReference::Part(ChatPartRef {
                        message: message.id,
                        part_id: part_id(part),
                    });
                    if matches!(&anchor, FactReference::Part(_)) && reference != anchor {
                        continue;
                    }
                    if matches!(part, ChatPart::ToolCall { .. }) {
                        tool_calls.push(reference.clone());
                    }
                    if reference != anchor {
                        references.push(reference);
                    }
                }
            }
            _ => {}
        }
    }
    let mut edge_ids = BTreeSet::new();
    for reference in references.clone() {
        let key = super::key(&reference)?;
        let mut stmt =
            conn.prepare("SELECT id FROM relations WHERE (from_ref=?1 OR to_ref=?1) ORDER BY id")?;
        for id in stmt.query_map([key], |r| r.get::<_, i64>(0))? {
            edge_ids.insert(id?);
        }
    }
    let mut edges = edge_ids
        .iter()
        .map(|id| relations::read(conn, *id))
        .collect::<Result<Vec<_>, _>>()?;
    let targets = edges
        .iter()
        .filter(|link| link.kind == RelationKind::ToolExecution)
        .map(|link| super::key(&link.to))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut competitors = conn.prepare(
        "SELECT id FROM relations WHERE kind='tool_execution' AND to_ref=?1 ORDER BY id",
    )?;
    for target in targets {
        for id in competitors.query_map([target], |row| row.get::<_, i64>(0))? {
            let id = id?;
            if edge_ids.insert(id) {
                edges.push(relations::read(conn, id)?);
            }
        }
    }
    for edge in &edges {
        references.push(edge.from.clone());
        references.push(edge.to.clone());
        references.extend(edge.evidence.iter().cloned());
    }
    for reference in &references {
        load(conn, reference, &mut records, &mut seen)?;
    }
    for owner in relations::native_tool_candidates(conn, &records)? {
        if seen.insert(("message".to_owned(), owner)) {
            records.push(SourceRecord::Message(Arc::new(chat::read_message(
                conn, owner,
            )?)));
        }
    }
    let messages = records
        .iter()
        .filter_map(|r| match r {
            SourceRecord::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for m in messages {
        for owner in relations::tool_part_candidates(conn, &m)? {
            if seen.insert(("message".to_owned(), owner)) {
                records.push(SourceRecord::Message(Arc::new(chat::read_message(
                    conn, owner,
                )?)));
            }
        }
    }
    let protocol_refs = records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::Message(message) => Some(message.record_refs.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    let mut loaded = records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::Protocol(original) => Some(original.id),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let mut missing = Vec::new();
    for reference in protocol_refs
        .into_iter()
        .filter(|reference| loaded.insert(*reference))
    {
        let before = records.len();
        load(
            conn,
            &FactReference::Protocol(reference),
            &mut records,
            &mut seen,
        )?;
        if records.len() == before {
            missing.push(ContextGap {
                at: FactReference::Protocol(reference),
                reason: ContextGapReason::NotProvided,
            });
        }
    }
    edges.retain(|relation| relation.kind == RelationKind::ToolExecution);
    for at in tool_calls {
        if !edges.iter().any(|link| link.from == at) {
            missing.push(ContextGap {
                at,
                reason: ContextGapReason::NoExplicitRelation,
            });
        }
    }
    super::native_handoff::load_return_sequences(conn, &mut records, &mut seen)?;
    let mut view = activity_view(anchor, records, edges);
    view.gaps.extend(missing);
    for reference in references {
        let mut stmt = conn.prepare("SELECT reason FROM context_gaps WHERE at_ref=?1")?;
        for reason in stmt.query_map([super::key(&reference)?], |r| r.get::<_, String>(0))? {
            let value = ContextGap {
                at: reference.clone(),
                reason: super::row::named(&reason?)?,
            };
            if !view.gaps.contains(&value) {
                view.gaps.push(value);
            }
        }
    }
    Ok(view)
}

fn attach_attribution(
    conn: &Connection,
    view: &mut ContextView,
    attribution: &crate::attribution::ToolAttribution,
) -> Result<(), StoreError> {
    for candidate in &attribution.candidates {
        let related = activity(conn, FactReference::Part(candidate.call))?;
        for record in related.source_records {
            let present = view.source_records.iter().any(|old| match (old, &record) {
                (SourceRecord::Message(a), SourceRecord::Message(b)) => a == b,
                (SourceRecord::Protocol(a), SourceRecord::Protocol(b)) => a == b,
                _ => false,
            });
            if !present && matches!(record, SourceRecord::Message(_) | SourceRecord::Protocol(_)) {
                view.source_records.push(record);
            }
        }
        for relation in related.relations {
            if matches!(
                relation.to,
                FactReference::Part(_) | FactReference::Message(_)
            ) && !view.relations.iter().any(|old| {
                old.kind == relation.kind && old.from == relation.from && old.to == relation.to
            }) {
                view.relations.push(relation);
            }
        }
        let from = FactReference::Part(candidate.call);
        let to = FactReference::Event(attribution.event);
        if !view.relations.iter().any(|old| {
            old.kind == RelationKind::ToolReferenceCandidate && old.from == from && old.to == to
        }) {
            view.relations.push(EvidenceRelation {
                kind: RelationKind::ToolReferenceCandidate,
                from: from.clone(),
                to: to.clone(),
                evidence: vec![from, to],
            });
        }
    }
    Ok(())
}
impl BusinessLog {
    /// Read only the selected immutable original; protocol byte pages retain absolute offsets.
    pub async fn original_record(
        &self,
        reference: FactReference,
        body_offset: u64,
        body_limit: u32,
    ) -> Result<ContextView, StoreError> {
        if body_limit == 0 || body_limit as usize > as_main::messages::protocol::MAX_FRAME_BYTES / 8
        {
            return Err(StoreError::Invalid);
        }
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut records = Vec::new();
            match &reference {
                FactReference::Protocol(_) => {
                    let id = tx.query_row("SELECT record_id FROM fact_records WHERE reference=?1 AND record_kind='protocol'",
                        [super::key(&reference)?], |r| r.get::<_, i64>(0)).optional()?.ok_or(StoreError::NotProvided)?;
                    let end = body_offset.checked_add(u64::from(body_limit)).ok_or(StoreError::Invalid)?;
                    let (observation, next_offset) = chat::read_protocol_range(&tx, id, Some(body_offset..end))?;
                    records.push(SourceRecord::ProtocolPage { observation: Arc::new(observation), next_offset });
                }
                FactReference::NativeObservation(observation) if body_offset == 0 => {
                    let mut statement = tx.prepare("SELECT id FROM native_request_handoffs WHERE observation=?1 ORDER BY id")?;
                    for id in statement.query_map([super::key(observation)?], |r| r.get::<_, i64>(0))? {
                        records.push(SourceRecord::RequestHandoff(Arc::new(super::native_handoff::read_at(&tx, id?)?)));
                    }
                }
                _ => return Err(StoreError::Invalid),
            }
            if records.is_empty() { return Err(StoreError::NotProvided); }
            Ok(ContextView { anchor: reference, source_records: records, relations: vec![], gaps: vec![] })
        })?.await
    }
    /// Page original exchanges with retained messages, newest first by first commit.
    pub async fn chat_exchanges(
        &self,
        session: Option<crate::business_log::sessions::ChatSessionRef>,
        after: Option<ExchangeKey>,
        limit: u16,
    ) -> Result<(Vec<ExchangeKey>, Option<ExchangeKey>), StoreError> {
        let limit = page_limit(limit)?;
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let session = match session {
                Some(crate::business_log::sessions::ChatSessionRef::ModelHttp {record})=>Some(super::sessions::resolve(&tx,record)?),
                Some(reference @ crate::business_log::sessions::ChatSessionRef::NativeGateway {..})=>{
                    super::sessions::entry(&tx,&reference)?;
                    if after.is_some() {return Err(StoreError::Invalid);}
                    return Ok((Vec::new(),None));
                }
                None=>None,
            };
            if let (Some(session), Some(after)) = (session, after)
                && !super::sessions::contains_exchange(&tx, session, after)? {
                return Err(StoreError::Invalid);
            }
            let boundary = after.map(|key| {
                tx.query_row(
                    "SELECT min(first_delivery) FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2",
                    params![key.flow_instance.as_slice(), key.sequence.to_be_bytes().as_slice()],
                    |row| row.get::<_, Option<i64>>(0),
                )?.ok_or(StoreError::NotProvided)
            }).transpose()?;
            let flow = after.map(|key| key.flow_instance.to_vec());
            let sequence = after.map(|key| key.sequence.to_be_bytes().to_vec());
            let mut exchanges = {
                let mut stmt = tx.prepare(
                    &format!("WITH {}, exchange_order AS (
                       SELECT m.flow_instance,m.exchange_sequence,min(m.first_delivery) AS first
                       FROM messages m WHERE ?5 IS NULL OR EXISTS (
                         SELECT 1 FROM valid v JOIN valid a USING ({})
                         WHERE a.id=?5 AND v.flow_instance=m.flow_instance AND v.exchange_sequence=m.exchange_sequence
                       ) GROUP BY m.flow_instance,m.exchange_sequence
                     )
                     SELECT flow_instance,exchange_sequence FROM exchange_order
                     WHERE ?1 IS NULL OR (first,flow_instance,exchange_sequence)<(?1,?2,?3)
                     ORDER BY first DESC,flow_instance DESC,exchange_sequence DESC LIMIT ?4", super::sessions::VALID, super::sessions::DOMAIN),
                )?;
                stmt.query_map(params![boundary, flow, sequence, limit + 1, session], |row| {
                    Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?.map(|row| {
                    let (flow, sequence) = row?;
                    Ok(ExchangeKey {
                        flow_instance: flow.try_into().map_err(|_| StoreError::Invalid)?,
                        sequence: super::parts::u64_blob(sequence)?,
                    })
                }).collect::<Result<Vec<_>, StoreError>>()?
            };
            let has_more = exchanges.len() > usize::from(limit);
            exchanges.truncate(usize::from(limit));
            let next = exchanges.last().copied().filter(|_| has_more);
            tx.commit()?;
            Ok((exchanges, next))
        })?.await
    }

    /// Page the current committed messages of one original exchange in intake order.
    pub(crate) async fn model_messages(
        &self,
        exchange: ExchangeKey,
        after: Option<ChatMessageId>,
        limit: u16,
    ) -> Result<(Vec<Arc<ChatMessage>>, Option<ChatMessageId>), StoreError> {
        let limit = page_limit(limit)?;
        if after.is_some_and(|id| id.exchange != exchange) {
            return Err(StoreError::Invalid);
        }
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let flow = exchange.flow_instance.as_slice();
            let sequence = exchange.sequence.to_be_bytes();
            let boundary = after.map(|id| {
                tx.query_row(
                    "SELECT first_delivery FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3",
                    params![flow, sequence.as_slice(), id.sequence.to_be_bytes().as_slice()],
                    |row| row.get::<_, i64>(0),
                ).optional()?.ok_or(StoreError::NotProvided)
            }).transpose()?;
            let after_sequence = after.map(|id| id.sequence.to_be_bytes().to_vec());
            let mut owners = {
                let mut stmt = tx.prepare(
                    "SELECT id FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2
                       AND (?3 IS NULL OR (first_delivery,sequence)>(?3,?4))
                     ORDER BY first_delivery,sequence LIMIT ?5",
                )?;
                stmt.query_map(params![flow, sequence.as_slice(), boundary, after_sequence, limit + 1],
                    |row| row.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?
            };
            if owners.is_empty() && after.is_none() {
                return Err(StoreError::NotProvided);
            }
            let has_more = owners.len() > usize::from(limit);
            owners.truncate(usize::from(limit));
            let messages = owners.into_iter().map(|owner| chat::read_message(&tx, owner).map(Arc::new))
                .collect::<Result<Vec<_>, _>>()?;
            let next = messages.last().map(|message| message.id).filter(|_| has_more);
            tx.commit()?;
            Ok((messages, next))
        })?.await
    }

    /// Read committed row counts for host diagnostics; no content or in-memory history is retained.
    pub async fn counts(&self) -> Result<BusinessLogCounts, StoreError> {
        self.call(|conn| {
            let count = |table: &str| -> Result<u64, StoreError> {
                let value = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })?;
                u64::try_from(value).map_err(|_| StoreError::Invalid)
            };
            Ok(BusinessLogCounts {
                protocol_records: count("protocol_records")?,
                messages: count("messages")?,
                message_revisions: count("message_revisions")?,
                questions: count("questions")?,
                operation_reports: count("operation_reports")?,
                relations: count("relations")?,
                feedback_facts: count("feedback")?,
            })
        })?
        .await
    }
    /// Read original facts and explicit relationships into a caller-owned, short-lived view.
    pub async fn activity(&self, anchor: FactReference) -> Result<ContextView, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            activity(&tx, anchor)
        })?
        .await
    }
    /// Read the original graph and independent event candidates from one database transaction.
    /// This trusted in-process query neither writes relationships nor authorizes enforcement.
    pub async fn activity_context(
        &self,
        anchor: FactReference,
        attribution: Option<crate::attribution::AttributionRequest>,
    ) -> Result<(ContextView, Option<crate::attribution::ToolAttribution>), StoreError> {
        let attribution = match (attribution, &anchor) {
            (None, _) => None,
            (Some(request), FactReference::Event(event)) => Some((*event, request)),
            (Some(_), _) => return Err(StoreError::Invalid),
        };
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut view = activity(&tx, anchor)?;
            if let FactReference::Event(event) = view.anchor
                && let Ok(call) = execution_call(&view, event)
            {
                match activity_input(&tx, &view, call)? {
                    Ok(Some(input)) => view.source_records.push(SourceRecord::Message(input)),
                    Ok(None) => {}
                    Err(gap) if !view.gaps.contains(&gap) => view.gaps.push(gap),
                    Err(_) => {}
                }
                if original_completion(&view, event).is_ok_and(has_fixed_denial) {
                    match retained_feedback(
                        &tx,
                        RuleFeedbackRequest {
                            denial: event,
                            call,
                        },
                    ) {
                        Ok(Some(fact)) => view
                            .source_records
                            .push(SourceRecord::RuleFeedback(Arc::new(fact))),
                        Ok(None) | Err(StoreError::Conflict) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            let attribution = if let Some((event, request)) = attribution {
                match super::attribution::read_attribution(
                    &tx,
                    event,
                    request.pattern,
                    request.after,
                    request.limit,
                ) {
                    Ok(matches) => Some(matches),
                    Err(StoreError::NotProvided) => {
                        let gap = ContextGap {
                            at: FactReference::Event(event),
                            reason: ContextGapReason::NotProvided,
                        };
                        if !view.gaps.contains(&gap) {
                            view.gaps.push(gap);
                        }
                        None
                    }
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            if let Some(matches) = &attribution {
                attach_attribution(&tx, &mut view, matches)?;
            }
            tx.commit()?;
            Ok((view, attribution))
        })?
        .await
    }
    /// Get only a genuinely stored question, not one inferred from a result's DecisionId.
    pub async fn question(&self, id: DecisionId) -> Result<Arc<Up>, StoreError> {
        self.call(move |conn| event::read_question(conn, question_row(conn, id)?))?
            .await
    }

    /// Read the original Ask, its activity and its optional frozen window in one transaction.
    pub(crate) async fn read_ask_context(
        &self,
        question: DecisionId,
        attribution: Option<crate::attribution::AttributionRequest>,
    ) -> Result<AskContextRead, StoreError> {
        let failure_owner = self.clone();
        self.job(
            move |conn| {
                let tx = conn.transaction()?;
                let mut view = activity(&tx, FactReference::Question(question))?;
                let ask = view
                    .source_records
                    .iter()
                    .find_map(|record| match record {
                        SourceRecord::Question(ask)
                            if matches!(
                                ask.as_ref(),
                                Up::Ask { decision_id, .. } if *decision_id == question
                            ) =>
                        {
                            Some(ask.clone())
                        }
                        _ => None,
                    })
                    .ok_or(StoreError::NotProvided)?;
                let Up::Ask { event, .. } = ask.as_ref() else {
                    return Err(StoreError::Invalid);
                };
                let call = execution_call(&view, EventKey::from(event.as_ref()));
                if event.header().source_boottime_ns.is_none() {
                    view.gaps.push(ContextGap {
                        at: FactReference::Event(EventKey::from(event.as_ref())),
                        reason: ContextGapReason::NotProvided,
                    });
                }
                let cutoff: i64 =
                    tx.query_row("SELECT coalesce(max(id),0) FROM deliveries", [], |row| {
                        row.get(0)
                    })?;
                let mut candidate_windows = Vec::new();
                let mut candidate_exchanges = std::collections::HashSet::new();
                for relation in &view.relations {
                    if relation.kind.is_tool_candidate()
                        && relation.to == FactReference::Event(EventKey::from(event.as_ref()))
                        && let FactReference::Part(candidate) = relation.from
                        && candidate_exchanges.insert(candidate.message.exchange)
                    {
                        match recent_window(
                            &tx,
                            candidate.message.exchange,
                            cutoff,
                            event.header().source_boottime_ns,
                        ) {
                            Ok(window) => candidate_windows.push(window),
                            Err(StoreError::NotProvided) => view.gaps.push(ContextGap {
                                at: relation.from.clone(),
                                reason: ContextGapReason::NotProvided,
                            }),
                            Err(error) => return Err(error),
                        }
                    }
                }
                let attribution = attribution
                    .map(|request| {
                        let matches = super::attribution::read_attribution(
                            &tx,
                            EventKey::from(event.as_ref()),
                            request.pattern,
                            request.after,
                            request.limit,
                        )?;
                        attach_attribution(&tx, &mut view, &matches)?;
                        let mut windows = Vec::new();
                        let mut exchanges = std::collections::HashSet::new();
                        for candidate in &matches.candidates {
                            if !call.as_ref().is_ok_and(|call| {
                                call.message.exchange == candidate.call.message.exchange
                            }) && exchanges.insert(candidate.call.message.exchange)
                            {
                                match recent_window(
                                    &tx,
                                    candidate.call.message.exchange,
                                    cutoff,
                                    event.header().source_boottime_ns,
                                ) {
                                    Ok(window) => windows.push(window),
                                    Err(StoreError::NotProvided) => view.gaps.push(ContextGap {
                                        at: FactReference::Part(candidate.call),
                                        reason: ContextGapReason::NotProvided,
                                    }),
                                    Err(error) => return Err(error),
                                }
                            }
                        }
                        Ok::<_, StoreError>(AttributionRead { matches, windows })
                    })
                    .map(|result| match result {
                        Ok(value) => Ok(Ok(value)),
                        Err(StoreError::NotProvided) => Ok(Err(StoreError::NotProvided)),
                        Err(error) => Err(error),
                    })
                    .transpose()?;
                let window = if let Ok(call) = &call {
                    let mut started_write = false;
                    let result = (|| {
                        let row = question_row(&tx, question)?;
                        let window =
                            freeze_ask_window(&tx, row, call.message.exchange, &mut started_write)?;
                        tx.commit()?;
                        Ok(window)
                    })();
                    Some(if started_write {
                        failure_owner.write_result(result)
                    } else {
                        result
                    })
                } else {
                    tx.commit()?;
                    None
                };
                Ok(AskContextRead {
                    ask,
                    view,
                    call,
                    window,
                    attribution,
                    candidate_windows,
                })
            },
            true,
            false,
        )?
        .await
    }
    /// Read the actual retained terminal/in-progress Flow fact from the business database.
    pub async fn feedback_state(
        &self,
        request: RuleFeedbackRequest,
    ) -> Result<RuleFeedbackFact, StoreError> {
        self.call(move |conn| retained_feedback(conn, request)?.ok_or(StoreError::NotProvided))?
            .await
    }
    pub(crate) async fn claim_feedback(
        &self,
        request: RuleFeedbackRequest,
    ) -> Result<FeedbackClaim, StoreError> {
        self.write_call(move|conn|{
        let tx=conn.transaction()?;let denial=super::key(&FactReference::Event(request.denial))?;let call=super::key(&FactReference::Part(request.call))?;
        let competing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM relations WHERE kind='tool_execution' AND to_ref=?1 AND from_ref<>?2)
             OR EXISTS(SELECT 1 FROM context_gaps WHERE at_ref=?1 AND reason='conflicting_evidence')",
            params![denial,call],|row|row.get(0),
        )?;
        if competing {return Ok(FeedbackClaim::ConflictingEvidence);}
        let old = tx.query_row(
            "SELECT denial_ref,call_ref,last_fact,failure FROM feedback_bindings
             WHERE denial_ref=?1 OR call_ref=?2 ORDER BY denial_ref=?1 DESC LIMIT 1",
            params![denial,call],
            |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<String>>(3)?)),
        ).optional()?;
        if let Some((denial_ref, call_ref, fact, error)) = old {
            if call_ref != call {
                return Ok(FeedbackClaim::ConflictingEvidence);
            }
            if denial_ref != denial {
                return Ok(FeedbackClaim::AlreadyClaimed);
            }
            return if let Some(fact) = fact {
                Ok(FeedbackClaim::Existing(relations::read_feedback(&tx, fact)?))
            } else if let Some(error) = error {
                Ok(FeedbackClaim::Failed(module_error(&error)?))
            } else {
                Ok(FeedbackClaim::Unconfirmed)
            };
        }
        tx.execute("INSERT INTO feedback_bindings(denial_ref,call_ref) VALUES(?1,?2)",params![denial,call])?;tx.commit()?;Ok(FeedbackClaim::New)
    })?.await
    }
    pub(crate) async fn feedback_result(
        &self,
        request: RuleFeedbackRequest,
        result: Result<RuleFeedbackFact, as_types::main_ports::ModuleError>,
    ) -> Result<(), StoreError> {
        self.write_call(move|conn|{
        let tx=conn.transaction()?;
        match result{Ok(fact)=>{if fact.denial!=request.denial||fact.call!=request.call{return Err(StoreError::Conflict);}let delivery=super::record(&tx,"feedback_reply")?;relations::feedback(&tx,delivery,&fact)?;},Err(error)=>{tx.execute("UPDATE feedback_bindings SET failure=?1 WHERE denial_ref=?2 AND call_ref=?3",params![error.to_string(),super::key(&FactReference::Event(request.denial))?,super::key(&FactReference::Part(request.call))?])?;}}
        tx.commit()?;Ok(())
    })?.await
    }
}

fn retained_feedback(
    conn: &Connection,
    request: RuleFeedbackRequest,
) -> Result<Option<RuleFeedbackFact>, StoreError> {
    let stored = conn
        .query_row(
            "SELECT call_ref,last_fact FROM feedback_bindings WHERE denial_ref=?1",
            [super::key(&FactReference::Event(request.denial))?],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?;
    let Some((call, fact)) = stored else {
        return Ok(None);
    };
    if call != super::key(&FactReference::Part(request.call))? {
        return Err(StoreError::Conflict);
    }
    let Some(fact) = fact else { return Ok(None) };
    let original = relations::read_feedback(conn, fact)?;
    if original.denial != request.denial || original.call != request.call {
        return Err(StoreError::Conflict);
    }
    Ok(Some(original))
}

pub(crate) struct AskContextRead {
    pub candidate_windows: Vec<(AskMessageWindow, Vec<Arc<ChatMessage>>)>,
    pub ask: Arc<Up>,
    pub view: ContextView,
    pub call: Result<ChatPartRef, ContextGap>,
    pub window: Option<WindowRead>,
    pub attribution: Option<Result<AttributionRead, StoreError>>,
}
pub(crate) struct AttributionRead {
    pub matches: crate::attribution::ToolAttribution,
    pub windows: Vec<(AskMessageWindow, Vec<Arc<ChatMessage>>)>,
}

fn question_row(conn: &Connection, id: DecisionId) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT id FROM questions WHERE decision_id=?1",
        [id.0.as_slice()],
        |row| row.get(0),
    )
    .optional()?
    .ok_or(StoreError::NotProvided)
}

fn freeze_ask_window(
    conn: &Connection,
    question: i64,
    exchange: ExchangeKey,
    started_write: &mut bool,
) -> WindowRead {
    let flow = exchange.flow_instance.as_slice();
    let sequence = exchange.sequence.to_be_bytes();
    let existing = conn.query_row(
        "SELECT id,flow_instance,exchange_sequence,cutoff,earlier FROM ask_windows WHERE question=?1",
        [question],
        |row| Ok((
            row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?,
            row.get::<_, i64>(3)?, row.get::<_, u32>(4)?,
        )),
    ).optional()?;
    let (cutoff, earlier, messages) = if let Some((
        owner,
        stored_flow,
        stored_sequence,
        cutoff,
        earlier,
    )) = existing
    {
        if stored_flow != flow || stored_sequence != sequence {
            return Err(StoreError::Conflict);
        }
        let mut stmt = conn.prepare(
            "SELECT message,revision FROM ask_window_items WHERE owner=?1 ORDER BY ordinal",
        )?;
        let mut messages = Vec::new();
        for row in stmt.query_map([owner], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })? {
            let (id, revision) = row?;
            let row: i64 = conn.query_row("SELECT id FROM message_revisions WHERE message=?1 AND revision=?2 AND delivery<=?3", params![id,revision,cutoff], |row|row.get(0))?;
            let message = chat::read_revision(conn, row)?;
            messages.push(Arc::new(message));
        }
        (cutoff, earlier, messages)
    } else {
        let cutoff: i64 =
            conn.query_row("SELECT coalesce(max(id),0) FROM deliveries", [], |row| {
                row.get(0)
            })?;
        let ask = event::read_question(conn, question)?;
        let Up::Ask { event, .. } = ask.as_ref() else {
            return Err(StoreError::Invalid);
        };
        let (window, messages) =
            recent_window(conn, exchange, cutoff, event.header().source_boottime_ns)?;
        let earlier = window.earlier_observed_messages;
        *started_write = true;
        conn.execute(
            "INSERT INTO ask_windows(question,flow_instance,exchange_sequence,cutoff,earlier) VALUES(?1,?2,?3,?4,?5)",
            params![question, flow, sequence.as_slice(), cutoff, earlier],
        )?;
        let owner = conn.last_insert_rowid();
        let mut insert = conn.prepare("INSERT INTO ask_window_items VALUES(?1,?2,?3,?4)")?;
        for (ordinal, message) in messages.iter().enumerate() {
            let id = conn.query_row("SELECT id FROM messages WHERE flow_instance=?1 AND exchange_sequence=?2 AND sequence=?3",
                params![message.id.exchange.flow_instance.as_slice(),message.id.exchange.sequence.to_be_bytes().as_slice(),message.id.sequence.to_be_bytes().as_slice()], |row| row.get::<_, i64>(0))?;
            insert.execute(params![
                owner,
                ordinal as i64,
                id,
                message.revision.to_be_bytes().as_slice()
            ])?;
        }
        (cutoff, earlier, messages)
    };
    let window = AskMessageWindow {
        exchange,
        message_refs: messages.iter().map(|message| message.id).collect(),
        earlier_observed_messages: earlier,
        source_gaps: read_gaps(conn, exchange, cutoff)?,
    };
    Ok((window, messages))
}

fn activity_input(
    conn: &Connection,
    view: &ContextView,
    call: ChatPartRef,
) -> Result<Result<Option<Arc<ChatMessage>>, ContextGap>, StoreError> {
    use ContextGapReason::{ConflictingEvidence, NotProvided, SourceUnavailable};

    let response = match message(view, call.message) {
        Ok(value) => value,
        Err(gap) => return Ok(Err(gap)),
    };
    let Some(row) = chat::last_request_user(conn, call.message)? else {
        return Ok(Ok(None));
    };
    let at_message = |reason| ContextGap {
        at: FactReference::Message(row.reference),
        reason,
    };
    let input = match chat::read_message_at(conn, row.owner, row.cutoff) {
        Ok(value) => value,
        Err(StoreError::NotProvided) => return Ok(Err(at_message(NotProvided))),
        Err(StoreError::Conflict) => return Ok(Err(at_message(ConflictingEvidence))),
        Err(error) => return Err(error),
    };
    if input.id != row.reference || input.role != ChatRole::User {
        return Ok(Err(at_message(ConflictingEvidence)));
    }
    if let Err(reason) = request_for_response(&input, response) {
        return Ok(Err(at_message(reason)));
    }
    if !input.quality.is_empty() || input.state != ChatState::Complete {
        return Ok(Err(at_message(SourceUnavailable)));
    }
    let reference = match input.record_refs.as_slice() {
        [] => return Ok(Err(at_message(NotProvided))),
        [reference] if reference.exchange == input.id.exchange => *reference,
        _ => return Ok(Err(at_message(ConflictingEvidence))),
    };
    let Some(source) = chat::request_provenance(conn, reference, row.first_delivery)? else {
        return Ok(Err(ContextGap {
            at: FactReference::Protocol(reference),
            reason: SourceUnavailable,
        }));
    };
    if !protocol_in_scope(&input, reference, &source) {
        return Ok(Err(ContextGap {
            at: FactReference::Protocol(reference),
            reason: ConflictingEvidence,
        }));
    }
    if let Some(existing) = view.source_records.iter().find_map(|record| match record {
        SourceRecord::Message(existing) if existing.id == input.id => Some(existing),
        _ => None,
    }) {
        return Ok(if existing.as_ref() == &input {
            Ok(None)
        } else {
            Err(at_message(ConflictingEvidence))
        });
    }
    Ok(Ok(Some(Arc::new(input))))
}

fn recent_window(
    conn: &Connection,
    exchange: ExchangeKey,
    cutoff: i64,
    before: Option<u64>,
) -> WindowRead {
    let flow = exchange.flow_instance.as_slice();
    let sequence = exchange.sequence.to_be_bytes();
    let count: u32 = conn.query_row(
        "SELECT count(*) FROM messages m WHERE flow_instance=?1 AND exchange_sequence=?2 AND first_delivery<=?3 AND (?4 IS NULL OR EXISTS(SELECT 1 FROM message_revisions r WHERE r.message=m.id AND r.delivery<=?3 AND r.last_seen_boottime_ns<=?4))",
        params![flow, sequence.as_slice(), cutoff, before.map(u64::to_be_bytes).as_ref().map(|value| value.as_slice())], |row| row.get(0),
    )?;
    if count == 0 {
        return Err(StoreError::NotProvided);
    }
    let mut stmt = conn.prepare(
        "SELECT id FROM messages m WHERE flow_instance=?1 AND exchange_sequence=?2 AND first_delivery<=?3 AND (?4 IS NULL OR EXISTS(SELECT 1 FROM message_revisions r WHERE r.message=m.id AND r.delivery<=?3 AND r.last_seen_boottime_ns<=?4))
         ORDER BY CASE origin WHEN 'request_context' THEN 0 ELSE 1 END DESC,first_delivery DESC LIMIT 20",
    )?;
    let ids = stmt
        .query_map(
            params![
                flow,
                sequence.as_slice(),
                cutoff,
                before
                    .map(u64::to_be_bytes)
                    .as_ref()
                    .map(|value| value.as_slice())
            ],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let messages = ids
        .into_iter()
        .rev()
        .map(|id| {
            if let Some(before) = before {
                let revision: i64 = conn.query_row("SELECT id FROM message_revisions WHERE message=?1 AND delivery<=?2 AND last_seen_boottime_ns<=?3 ORDER BY revision DESC LIMIT 1",params![id,cutoff,before.to_be_bytes().as_slice()],|row|row.get(0))?;
                chat::read_revision(conn,revision).map(Arc::new)
            } else { chat::read_message_at(conn, id, cutoff).map(Arc::new) }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let window = AskMessageWindow {
        exchange,
        message_refs: messages.iter().map(|message| message.id).collect(),
        earlier_observed_messages: count.saturating_sub(20),
        source_gaps: read_gaps(conn, exchange, cutoff)?,
    };
    Ok((window, messages))
}

pub(super) fn page_limit(limit: u16) -> Result<u16, StoreError> {
    if (1..=64).contains(&limit) {
        Ok(limit)
    } else {
        Err(StoreError::Invalid)
    }
}
/// Actual committed canonical-row counts, not source coverage or media-delivery guarantees.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BusinessLogCounts {
    /// Original raw protocol records.
    pub protocol_records: u64,
    /// Distinct original logical messages.
    pub messages: u64,
    /// Actual original full message revisions; deltas remain in their own rows.
    pub message_revisions: u64,
    /// Genuinely received questions, never inferred from completions.
    pub questions: u64,
    /// Business-related actual source reports.
    pub operation_reports: u64,
    /// Evidence-backed canonical relationships.
    pub relations: u64,
    /// Actual Flow feedback observations or replies.
    pub feedback_facts: u64,
}
pub(crate) enum FeedbackClaim {
    New,
    ConflictingEvidence,
    AlreadyClaimed,
    Unconfirmed,
    Existing(RuleFeedbackFact),
    Failed(as_types::main_ports::ModuleError),
}
fn module_error(value: &str) -> Result<as_types::main_ports::ModuleError, StoreError> {
    use as_types::main_ports::ModuleError::*;
    [
        Unsupported,
        Unavailable,
        Conflict,
        InvalidInput,
        Forbidden,
        Unauthenticated,
        Busy,
        Stopped,
    ]
    .into_iter()
    .find(|v| v.to_string() == value)
    .ok_or(StoreError::Invalid)
}
fn read_gaps(
    conn: &Connection,
    exchange: ExchangeKey,
    cutoff: i64,
) -> Result<Vec<ObservationGap>, StoreError> {
    let mut stmt=conn.prepare("SELECT id FROM source_gaps WHERE flow_instance=?1 AND exchange_sequence=?2 AND delivery<=?3 ORDER BY id")?;
    let ids = stmt.query_map(
        params![
            exchange.flow_instance.as_slice(),
            exchange.sequence.to_be_bytes().as_slice(),
            cutoff
        ],
        |r| r.get::<_, i64>(0),
    )?;
    ids.map(|id| super::stream::read_gap(conn, id?)).collect()
}
