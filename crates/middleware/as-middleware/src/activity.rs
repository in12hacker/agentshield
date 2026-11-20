//! Explicit references and separately named evidence patterns from retained original facts.
use crate::context::*;
use as_main::messages::protocol::{OperationResult, Up};
use as_types::{
    DecisionId,
    dispatch_event::{EventData, EventPayload},
    driver::{NativeRequestOperation, NativeSyncThread},
    flow::*,
    process_tree::EventKey,
    runtime_observation::{NativeRequestHandoff, NativeRequestPhase},
};
use std::collections::{HashMap, HashSet};

fn push(view: &mut ContextView, relation: EvidenceRelation) {
    if !view
        .relations
        .iter()
        .any(|r| r.kind == relation.kind && r.from == relation.from && r.to == relation.to)
    {
        view.relations.push(relation);
    }
}
fn fail(view: &mut ContextView, at: FactReference, reason: ContextGapReason) {
    let gap = ContextGap { at, reason };
    if !view.gaps.contains(&gap) {
        view.gaps.push(gap);
    }
}

pub(crate) fn protocol_in_scope(
    message: &ChatMessage,
    record: ProtocolRecordId,
    source: &ProtocolSource,
) -> bool {
    record.exchange == message.id.exchange && source == message.source.as_ref()
}

/// Shared real collection domain across independent HTTP exchanges; not session identity.
pub(crate) fn tool_source_domain(a: &ProtocolSource, b: &ProtocolSource) -> bool {
    a.actor.process.is_some()
        && a.actor.process == b.actor.process
        && a.actual_auth_scope.is_some()
        && a.actual_auth_scope == b.actual_auth_scope
        && a.profile == b.profile
        && a.origin == b.origin
        && a.route_label == b.route_label
        && a.config_version == b.config_version
}

/// Both TLS observers are created for one admitted exchange and credential use.
pub(crate) fn request_for_response(
    request: &ChatMessage,
    response: &ChatMessage,
) -> Result<(), ContextGapReason> {
    let request_source = &request.source;
    let response_source = &response.source;
    if request.id.exchange != response.id.exchange
        || request_source.side != ProtocolSide::ClientRequest
        || response_source.side != ProtocolSide::UpstreamResponse
        || request_source.actor != response_source.actor
        || request_source.profile != response_source.profile
        || request_source.origin != response_source.origin
        || request_source.route_label != response_source.route_label
        || request_source.config_version != response_source.config_version
    {
        return Err(ContextGapReason::ConflictingEvidence);
    }
    let (
        Some(request_channel),
        Some(response_channel),
        Some(usage),
        Some(response_usage),
        Some(scope),
        Some(response_scope),
    ) = (
        &request_source.channel,
        &response_source.channel,
        request_source.credential_use,
        response_source.credential_use,
        &request_source.actual_auth_scope,
        &response_source.actual_auth_scope,
    )
    else {
        return Err(ContextGapReason::NotProvided);
    };
    if request_channel != response_channel
        || usage != response_usage
        || usage.exchange != request.id.exchange
        || scope != response_scope
    {
        return Err(ContextGapReason::ConflictingEvidence);
    }
    Ok(())
}

pub(crate) fn exchange_relation(
    message: &ChatMessage,
    record: ProtocolRecordId,
    source: &ProtocolSource,
) -> Result<EvidenceRelation, ContextGap> {
    let to = FactReference::Message(message.id);
    if !protocol_in_scope(message, record, source) {
        return Err(conflict(&to));
    }
    let from = FactReference::Protocol(record);
    Ok(EvidenceRelation {
        kind: RelationKind::SameExchange,
        evidence: vec![from.clone(), to.clone()],
        from,
        to,
    })
}

pub(crate) fn question_completion(
    decision_id: DecisionId,
    event: &EventData,
    completion: &OperationResult,
) -> Result<Option<EvidenceRelation>, ContextGap> {
    if completion.decision_id != Some(decision_id) {
        return Ok(None);
    }
    let from = FactReference::Question(decision_id);
    if completion.event.as_ref() != event {
        return Err(conflict(&from));
    }
    let to = FactReference::Completion(EventKey::from(event));
    Ok(Some(EvidenceRelation {
        kind: RelationKind::QuestionCompletion,
        evidence: vec![from.clone(), to.clone()],
        from,
        to,
    }))
}
/// Build business relationships from original facts and trusted explicit execution-source links.
/// `execution_links` is an in-process source input, never a deserialized client assertion.
pub fn activity_view(
    anchor: FactReference,
    records: Vec<SourceRecord>,
    execution_links: Vec<EvidenceRelation>,
) -> ContextView {
    let mut view = ContextView {
        anchor,
        source_records: records,
        relations: Vec::new(),
        gaps: Vec::new(),
    };
    let mut declared = HashMap::new();
    let mut competing = HashSet::new();
    for link in &execution_links {
        if let (FactReference::Event(event), FactReference::Part(call)) = (&link.to, &link.from)
            && link.kind == RelationKind::ToolExecution
            && declared
                .insert(*event, *call)
                .is_some_and(|original| original != *call)
        {
            competing.insert(*event);
        }
    }
    for event in &competing {
        fail(
            &mut view,
            FactReference::Event(*event),
            ContextGapReason::ConflictingEvidence,
        );
    }
    let mut gaps = Vec::new();
    for link in execution_links {
        if matches!(&link.to, FactReference::Event(event) if competing.contains(event)) {
            continue;
        }
        match validate_execution_link(&view, &link) {
            Ok(_) => push(&mut view, link),
            Err(error) => {
                if matches!(link.to, FactReference::Event(_)) && error.at != link.to {
                    gaps.push(ContextGap {
                        at: link.to,
                        reason: error.reason,
                    });
                }
                gaps.push(error);
            }
        }
    }
    for error in gaps {
        fail(&mut view, error.at, error.reason);
    }
    let records = view.source_records.clone(); // Arc/reference metadata only; source payloads stay shared.
    for record in &records {
        if let SourceRecord::Question(question) = record {
            let Up::Ask {
                decision_id, event, ..
            } = question.as_ref()
            else {
                let anchor = view.anchor.clone();
                fail(&mut view, anchor, ContextGapReason::ConflictingEvidence);
                continue;
            };
            for completion in records.iter().filter_map(|r| match r {
                SourceRecord::OperationResult(c) => Some(c),
                _ => None,
            }) {
                match question_completion(*decision_id, event, completion) {
                    Ok(Some(relation)) => push(&mut view, relation),
                    Ok(None) => {}
                    Err(error) => fail(&mut view, error.at, error.reason),
                }
            }
        }
        let SourceRecord::Message(message) = record else {
            continue;
        };
        for reference in &message.record_refs {
            if let Some(original) = records.iter().find_map(|r| match r {
                SourceRecord::Protocol(p) if p.id == *reference => Some(p),
                _ => None,
            }) {
                match exchange_relation(message, original.id, &original.source) {
                    Ok(relation) => push(&mut view, relation),
                    Err(error) => fail(&mut view, error.at, error.reason),
                }
            }
        }
        for part in &message.observed_parts {
            let ChatPart::ToolResult {
                native_id: Some(id),
                ..
            } = part
            else {
                continue;
            };
            let result = ChatPartRef {
                message: message.id,
                part_id: part_id(part),
            };
            if id.namespace != NativeIdNamespace::ToolResultReference {
                fail(
                    &mut view,
                    FactReference::Part(result),
                    ContextGapReason::ConflictingEvidence,
                );
                continue;
            }
            let proven = records.iter().any(|record| {
                matches!(record, SourceRecord::Protocol(original)
                    if message.record_refs.contains(&original.id)
                        && protocol_in_scope(message, original.id, &original.source)
                        && original.native_ids.contains(id))
            });
            if !proven {
                fail(
                    &mut view,
                    FactReference::Part(result),
                    ContextGapReason::NotProvided,
                );
                continue;
            }
            let mut calls = Vec::new();
            for candidate in records.iter().filter_map(|r| match r {
                SourceRecord::Message(m)
                    if m.id.exchange == message.id.exchange && m.source == message.source =>
                {
                    Some(m)
                }
                _ => None,
            }) {
                for p in &candidate.observed_parts {
                    if matches!(p,ChatPart::ToolCall{native_id:Some(native),..}if native.namespace==NativeIdNamespace::ToolCall&&native.value==id.value)
                    {
                        let call = ChatPartRef {
                            message: candidate.id,
                            part_id: part_id(p),
                        };
                        if !calls.contains(&call) {
                            calls.push(call);
                        }
                    }
                }
            }
            if calls.len() > 1 {
                fail(
                    &mut view,
                    FactReference::Part(result),
                    ContextGapReason::ConflictingEvidence,
                );
                continue;
            }
            if let Some(call) = calls.first() {
                if let Err(e) = validate_call(&view, call) {
                    if !view.gaps.contains(&e) {
                        view.gaps.push(e)
                    };
                    continue;
                }
                push(
                    &mut view,
                    EvidenceRelation {
                        kind: RelationKind::NativeToolReference,
                        from: FactReference::Part(*call),
                        to: FactReference::Part(result),
                        evidence: vec![FactReference::Part(*call), FactReference::Part(result)],
                    },
                );
            }
        }
    }
    native_relations(&mut view);
    tool_candidate_relations(&mut view);
    view
}

fn tool_candidate_relations(view: &mut ContextView) {
    let records = view.source_records.clone();
    let scopes = tool_candidate_scopes(&records);
    let containers = tool_candidate_containers(&records);
    for record in &records {
        let SourceRecord::Message(call_message) = record else {
            continue;
        };
        if call_message.origin != ChatOrigin::ResponseOutput {
            continue;
        }
        for part in &call_message.observed_parts {
            let ChatPart::ToolCall {
                id,
                native_id,
                name,
                ..
            } = part
            else {
                continue;
            };
            let from = FactReference::Part(ChatPartRef {
                message: call_message.id,
                part_id: *id,
            });
            for (instance, boot, event, before) in &containers {
                if call_message
                    .source
                    .actor
                    .process
                    .is_some_and(|process| process.boot_id == *boot)
                    && matches!(&call_message.source.actor.info.workload, Some(as_types::workload::Workload::Container(actual)) if actual == *instance)
                    && call_message.source.side == ProtocolSide::UpstreamResponse
                    && call_message.last_seen_boottime_ns <= *before
                {
                    let to = FactReference::Event(*event);
                    push(
                        view,
                        EvidenceRelation {
                            kind: RelationKind::ToolContainerCandidate,
                            evidence: vec![from.clone(), to.clone()],
                            from: from.clone(),
                            to,
                        },
                    );
                }
            }
            let Some(native) = native_id else {
                continue;
            };
            if native.namespace != NativeIdNamespace::ToolCall {
                continue;
            }
            for (process, event, before, basis) in &scopes {
                if call_message.source.actor.process == Some(*process)
                    && before.is_none_or(|time| call_message.last_seen_boottime_ns <= time)
                {
                    let to = FactReference::Event(*event);
                    let mut evidence = vec![from.clone(), to.clone()];
                    evidence.extend(basis.iter().cloned());
                    push(
                        view,
                        EvidenceRelation {
                            kind: RelationKind::ToolReferenceCandidate,
                            from: from.clone(),
                            to,
                            evidence,
                        },
                    );
                }
            }
            for result in &records {
                let SourceRecord::Message(result_message) = result else {
                    continue;
                };
                if result_message.origin != ChatOrigin::RequestContext
                    || result_message.source.side != ProtocolSide::ClientRequest
                    || !tool_source_domain(&call_message.source, &result_message.source)
                {
                    continue;
                }
                for part in &result_message.observed_parts {
                    let ChatPart::ToolResult {
                        id,
                        native_id: Some(result_id),
                        ..
                    } = part
                    else {
                        continue;
                    };
                    if result_id.namespace != NativeIdNamespace::ToolResultReference
                        || result_id.value != native.value
                    {
                        continue;
                    }
                    let to = FactReference::Part(ChatPartRef {
                        message: result_message.id,
                        part_id: *id,
                    });
                    let mut evidence = vec![from.clone(), to.clone()];
                    evidence.extend(
                        call_message
                            .record_refs
                            .iter()
                            .chain(&result_message.record_refs)
                            .copied()
                            .map(FactReference::Protocol),
                    );
                    push(
                        view,
                        EvidenceRelation {
                            kind: RelationKind::ToolReferenceCandidate,
                            from: from.clone(),
                            to,
                            evidence,
                        },
                    );
                }
            }
            for record in &records {
                let SourceRecord::RequestHandoff(bound) = record else {
                    continue;
                };
                let NativeRequestPhase::EventBound {
                    event,
                    enclosing_call,
                    ..
                } = &bound.phase
                else {
                    continue;
                };
                for opening in [bound.request, *enclosing_call].into_iter().flatten() {
                    for source in &records {
                        let SourceRecord::RequestHandoff(source) = source else {
                            continue;
                        };
                        if source.observation != opening {
                            continue;
                        }
                        let NativeRequestPhase::Submission {
                            analysis: Some(analysis),
                            ..
                        } = &source.phase
                        else {
                            continue;
                        };
                        if !analysis.candidates.iter().any(|candidate| {
                            let same_id = matches!(&native.value, NativeIdValue::JsonString(id) if id == &candidate.context.tool_call_id);
                            let same_response = candidate.context.response_id.as_ref().is_some_and(|response| records.iter().any(|record| matches!(record, SourceRecord::Protocol(original)
                                if call_message.record_refs.contains(&original.id)
                                    && protocol_in_scope(call_message, original.id, &original.source)
                                    && original.native_ids.iter().any(|id| id.namespace == NativeIdNamespace::Response && matches!(&id.value, NativeIdValue::JsonString(value) if value == response)))));
                            call_message.source.actor.process == Some(candidate.context.process)
                                && name == &candidate.context.tool_name
                                && (same_id || same_response)
                        }) {
                            continue;
                        }
                        let to = FactReference::Event(*event);
                        push(
                            view,
                            EvidenceRelation {
                                kind: RelationKind::ToolReferenceCandidate,
                                from: from.clone(),
                                to: to.clone(),
                                evidence: vec![
                                    from.clone(),
                                    to,
                                    FactReference::NativeObservation(source.observation),
                                    FactReference::NativeObservation(bound.observation),
                                ],
                            },
                        );
                    }
                }
            }
        }
    }
}

/// Existing strong lifetimes bound the candidate read scope; none establishes tool execution.
pub(crate) fn tool_candidate_scopes(
    records: &[SourceRecord],
) -> Vec<(
    as_types::process_tree::ProcessKey,
    EventKey,
    Option<u64>,
    Vec<FactReference>,
)> {
    let mut scopes = Vec::new();
    for record in records {
        let event = original_event(record);
        if let Some(event) = event
            && let Some(boot_id) = event.host_boot_id()
        {
            let header = event.header();
            let key = EventKey::from(event);
            for (pid, birth) in [
                (header.actor_tgid, header.actor_birth_boottime_ns),
                (
                    event.process.parent_pid,
                    event.process.parent_birth_boottime_ns,
                ),
            ] {
                if let (Some(host_tgid), Some(birth_boottime_ns)) = (pid, birth) {
                    scopes.push((
                        as_types::process_tree::ProcessKey {
                            boot_id,
                            host_tgid,
                            birth_boottime_ns,
                        },
                        key,
                        header.source_boottime_ns,
                        vec![FactReference::Event(key)],
                    ));
                }
            }
        }
        let SourceRecord::RequestHandoff(bound) = record else {
            continue;
        };
        let NativeRequestPhase::EventBound {
            event,
            enclosing_call,
            ..
        } = &bound.phase
        else {
            continue;
        };
        for opening in [bound.request, *enclosing_call].into_iter().flatten() {
            for source in records {
                if let SourceRecord::RequestHandoff(source) = source
                    && source.observation == opening
                    && let NativeRequestPhase::Submission { site, .. } = &source.phase
                {
                    let before = event_records(records, *event)
                        .find_map(|event| event.header().source_boottime_ns)
                        .or(Some(bound.observed_boottime_ns));
                    scopes.push((
                        site.process,
                        *event,
                        before,
                        vec![
                            FactReference::NativeObservation(source.observation),
                            FactReference::NativeObservation(bound.observation),
                        ],
                    ));
                }
            }
        }
    }
    scopes
}

/// Original event values only; native observation IDs never substitute for an LSM event.
pub(crate) fn event_records(
    records: &[SourceRecord],
    event: EventKey,
) -> impl Iterator<Item = &EventData> {
    records
        .iter()
        .filter_map(original_event)
        .filter(move |original| EventKey::from(*original) == event)
}

fn original_event(record: &SourceRecord) -> Option<&EventData> {
    match record {
        SourceRecord::Question(question) => match question.as_ref() {
            Up::Ask { event, .. } => Some(event.as_ref()),
            _ => None,
        },
        SourceRecord::OperationResult(result) => Some(result.event.as_ref()),
        _ => None,
    }
}

/// Borrow only original container facts; no PID walk, cgroup-name inference, or new cache.
pub(crate) fn tool_candidate_containers(
    records: &[SourceRecord],
) -> Vec<(
    &as_types::workload::ContainerInstance,
    [u8; 16],
    EventKey,
    u64,
)> {
    let mut seen = HashSet::new();
    records
        .iter()
        .filter_map(original_event)
        .filter_map(|event| {
            let Some(as_types::workload::Workload::Container(instance)) = &event.process.workload
            else {
                return None;
            };
            Some((
                instance,
                event.host_boot_id()?,
                EventKey::from(event),
                event.header().source_boottime_ns?,
            ))
        })
        .filter(|(instance, boot, event, time)| seen.insert((*instance, *boot, *event, *time)))
        .collect()
}

fn native_unique<'a, T: PartialEq + 'a>(
    mut originals: impl Iterator<Item = &'a T>,
    at: &FactReference,
) -> Result<&'a T, ContextGap> {
    let first = originals.next().ok_or_else(|| ContextGap {
        at: at.clone(),
        reason: ContextGapReason::NotProvided,
    })?;
    if originals.any(|original| original != first) {
        return Err(conflict(at));
    }
    Ok(first)
}

fn observation(
    records: &[SourceRecord],
    key: EventKey,
) -> Result<&NativeRequestHandoff, ContextGap> {
    native_unique(
        records.iter().filter_map(|record| match record {
            SourceRecord::RequestHandoff(fact) if fact.observation == key => Some(fact.as_ref()),
            _ => None,
        }),
        &FactReference::NativeObservation(key),
    )
}

fn native_relations(view: &mut ContextView) {
    let records = view.source_records.clone(); // Only Arc ownership is shared within this read.
    let raw = records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::RequestHandoff(fact) => Some(fact.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for fact in &raw {
        let from = FactReference::NativeObservation(fact.observation);
        if let Err(error) = observation(&records, fact.observation) {
            fail(view, error.at, error.reason);
        }
        if let NativeRequestPhase::EventBound {
            enclosing_call: Some(call),
            event,
            ..
        } = &fact.phase
        {
            let target = FactReference::NativeObservation(*call);
            match observation(&records, *call) {
                Ok(submission)
                    if matches!(submission.phase, NativeRequestPhase::Submission { .. }) =>
                {
                    push(
                        view,
                        EvidenceRelation {
                            kind: RelationKind::NativeCallScope,
                            from: FactReference::Event(*event),
                            to: target,
                            evidence: vec![from.clone()],
                        },
                    )
                }
                Ok(_) => fail(view, target, ContextGapReason::ConflictingEvidence),
                Err(error) => fail(view, error.at, error.reason),
            }
        }
        let Some(request) = fact.request else {
            continue;
        };
        let to = FactReference::NativeObservation(request);
        push(
            view,
            EvidenceRelation {
                kind: RelationKind::NativeRequestReference,
                from: from.clone(),
                to: to.clone(),
                evidence: vec![from.clone()],
            },
        );
        match observation(&records, request) {
            Ok(open)
                if matches!(open.phase, NativeRequestPhase::Submission { .. })
                    && open.request.is_none()
                    && fact.observation.source_instance == request.source_instance => {}
            Ok(_) => fail(view, to, ContextGapReason::ConflictingEvidence),
            Err(error) => fail(view, error.at, error.reason),
        }
    }
    let mut event_ids = HashSet::new();
    let events = records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::RequestReturn(returned) => Some(returned.event),
            _ => None,
        })
        .filter(|key| event_ids.insert(*key))
        .collect::<Vec<_>>();
    for key in events {
        let at = FactReference::Event(key);
        let returned = match native_unique(
            records.iter().filter_map(|record| match record {
                SourceRecord::RequestReturn(value) if value.event == key => Some(value.as_ref()),
                _ => None,
            }),
            &at,
        ) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if returned.operation != NativeRequestOperation::FileOpen {
            continue;
        }
        let event = match native_unique(event_records(&records, key), &at) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !matches!(event.payload, EventPayload::File(_))
            || event.header().operation != as_types::event_operation::FileOperation::Open as u32
        {
            continue;
        }
        let (Some(tgid), Some(tid), Some(birth), Some(event_time)) = (
            event.header().actor_tgid,
            event.header().actor_tid,
            event.header().actor_thread_birth_boottime_ns,
            event.header().source_boottime_ns,
        ) else {
            continue;
        };
        if event_time > returned.returned_boottime_ns {
            continue;
        }
        let actor = NativeSyncThread {
            tgid,
            tid,
            birth_boottime_ns: birth,
        };
        let candidates = raw
            .iter()
            .copied()
            .filter(|fact| {
                fact.observation.source_instance == key.source_instance
                    && fact.actor == actor
                    && fact.observed_boottime_ns > returned.returned_boottime_ns
                    && matches!(fact.phase, NativeRequestPhase::WorkLeave { .. })
            })
            .collect::<Vec<_>>();
        let Some(first) = candidates
            .iter()
            .map(|fact| fact.observed_boottime_ns)
            .min()
        else {
            continue;
        };
        for leave in candidates
            .into_iter()
            .filter(|fact| fact.observed_boottime_ns == first)
        {
            let target = FactReference::NativeObservation(leave.observation);
            if observation(&records, leave.observation).is_err() {
                continue;
            }
            let (Some(request), Some(NativeRequestOperation::FileOpen)) =
                (leave.request, leave.operation)
            else {
                if leave.request.is_none() || leave.operation.is_none() {
                    fail(view, target, ContextGapReason::NotProvided);
                }
                continue;
            };
            if request.source_instance != key.source_instance {
                fail(view, target, ContextGapReason::ConflictingEvidence);
                continue;
            }
            let NativeRequestPhase::WorkLeave { work_address } = leave.phase else {
                continue;
            };
            let done = raw.iter().copied().filter(|fact| {
                fact.request == Some(request) && fact.observation.source_instance == key.source_instance
                    && matches!(fact.phase, NativeRequestPhase::Done { work_address: work, .. } if work == work_address)
            }).collect::<Vec<_>>();
            let mut evidence = vec![at.clone(), target.clone()];
            let mut conflict_found = false;
            for fact in &done {
                let reference = FactReference::NativeObservation(fact.observation);
                if observation(&records, fact.observation).is_err() {
                    conflict_found = true;
                    continue;
                }
                let NativeRequestPhase::Done { result, .. } = fact.phase else {
                    continue;
                };
                if fact.operation.is_none() || result.is_none() {
                    fail(view, reference, ContextGapReason::NotProvided);
                    continue;
                }
                if fact.operation != Some(NativeRequestOperation::FileOpen)
                    || fact.observed_boottime_ns < leave.observed_boottime_ns
                    || result != Some(i64::from(returned.return_value))
                {
                    fail(view, reference, ContextGapReason::ConflictingEvidence);
                    conflict_found = true;
                } else if !evidence.contains(&reference) {
                    evidence.push(reference);
                }
            }
            if done.is_empty() {
                fail(view, target.clone(), ContextGapReason::NotProvided);
            }
            if conflict_found {
                fail(view, target, ContextGapReason::ConflictingEvidence);
            } else if evidence.len() > 2 {
                push(
                    view,
                    EvidenceRelation {
                        kind: RelationKind::SameThreadReturnSequence,
                        from: at.clone(),
                        to: target,
                        evidence,
                    },
                );
            }
        }
    }
}

/// Resolve a supplied execution relationship against its original tool and system facts.
pub(crate) fn execution_call(
    view: &ContextView,
    event: EventKey,
) -> Result<ChatPartRef, ContextGap> {
    let target = FactReference::Event(event);
    if view
        .gaps
        .iter()
        .any(|gap| gap.at == target && gap.reason == ContextGapReason::ConflictingEvidence)
    {
        return Err(conflict(&target));
    }
    let mut selected = None;
    for link in view
        .relations
        .iter()
        .filter(|link| link.kind == RelationKind::ToolExecution && link.to == target)
    {
        if selected.is_some_and(|call| link.from != FactReference::Part(call)) {
            return Err(conflict(&target));
        }
        selected = Some(validate_execution_link(view, link)?);
    }
    selected.ok_or_else(|| gap(view, &target, ContextGapReason::NoExplicitRelation))
}

pub(crate) fn validate_execution_link(
    view: &ContextView,
    link: &EvidenceRelation,
) -> Result<ChatPartRef, ContextGap> {
    let (FactReference::Part(call), FactReference::Event(_)) = (&link.from, &link.to) else {
        return Err(conflict(&link.to));
    };
    if link.kind != RelationKind::ToolExecution {
        return Err(conflict(&link.to));
    }
    if link.evidence.is_empty() {
        return Err(gap(view, &link.to, ContextGapReason::NotProvided));
    }
    for reference in &link.evidence {
        require_reference(view, reference)?;
    }
    require_reference(view, &link.to)?;
    let original = message(view, call.message)?;
    let ChatPart::ToolCall {
        native_id: Some(native),
        name,
        arguments,
        ..
    } = validate_call(view, call)?
    else {
        return Err(conflict(&link.from));
    };
    let mut native_context = None;
    for record in &view.source_records {
        if let SourceRecord::ToolExecution(facts) = record
            && FactReference::Event(facts.event) == link.to
        {
            if native_context.is_some_and(|context| context != &facts.context) {
                return Err(conflict(&link.to));
            }
            native_context = Some(&facts.context);
            let Some(process) = original.source.actor.process else {
                return Err(gap(view, &link.from, ContextGapReason::NotProvided));
            };
            if process != facts.context.process
                || name != &facts.context.tool_name
                || matches!((arguments, &facts.context.arguments), (Some(a), Some(b)) if a != b)
            {
                return Err(conflict(&link.from));
            }
            let NativeIdValue::JsonString(call_id) = &native.value else {
                return Err(conflict(&link.from));
            };
            if facts.context.tool_call_id != *call_id {
                let Some(item_id) = facts
                    .context
                    .tool_call_id
                    .strip_prefix(call_id)
                    .and_then(|suffix| suffix.strip_prefix('|'))
                    .filter(|_| original.source.profile == ProviderProfile::OpenAiResponses)
                else {
                    return Err(conflict(&link.from));
                };
                let Some(parent) = native.field_path.strip_suffix("/call_id") else {
                    return Err(conflict(&link.from));
                };
                let item_path = format!("{parent}/id");
                let mut found = false;
                for source in &view.source_records {
                    if let SourceRecord::Protocol(protocol) = source
                        && original.record_refs.contains(&protocol.id)
                        && protocol_in_scope(original, protocol.id, &protocol.source)
                        && protocol.native_ids.contains(native)
                    {
                        for id in &protocol.native_ids {
                            if id.namespace == NativeIdNamespace::ProviderEvent
                                && id.field_path == item_path
                            {
                                if !matches!(&id.value, NativeIdValue::JsonString(value) if value == item_id)
                                {
                                    return Err(conflict(&link.from));
                                }
                                found = true;
                            }
                        }
                    }
                }
                if !found {
                    return Err(gap(view, &link.from, ContextGapReason::NotProvided));
                }
            }
            let Some(response_id) = &facts.context.response_id else {
                continue;
            };
            let response_namespace = match original.source.profile {
                ProviderProfile::AnthropicMessages => NativeIdNamespace::Message,
                ProviderProfile::OpenAiResponses | ProviderProfile::OpenAiChatCompletions => {
                    NativeIdNamespace::Response
                }
            };
            let mut found = false;
            for source in &view.source_records {
                if let SourceRecord::Protocol(protocol) = source
                    && original.record_refs.contains(&protocol.id)
                    && protocol_in_scope(original, protocol.id, &protocol.source)
                {
                    for native in &protocol.native_ids {
                        if native.namespace == response_namespace {
                            if !matches!(&native.value, NativeIdValue::JsonString(value) if value == response_id)
                            {
                                return Err(conflict(&link.to));
                            }
                            found = true;
                        }
                    }
                }
            }
            if !found {
                return Err(gap(view, &link.to, ContextGapReason::NotProvided));
            }
        }
    }
    if original.origin != ChatOrigin::ResponseOutput {
        return Err(conflict(&link.from));
    }
    if original.state != ChatState::Complete {
        return Err(gap(view, &link.from, ContextGapReason::NotProvided));
    }
    Ok(*call)
}

pub(crate) fn original_completion(
    view: &ContextView,
    event: EventKey,
) -> Result<&OperationResult, ContextGap> {
    unique(
        view.source_records
            .iter()
            .filter_map(|record| match record {
                SourceRecord::OperationResult(value)
                    if EventKey::from(value.event.as_ref()) == event =>
                {
                    Some(value.as_ref())
                }
                _ => None,
            }),
        view,
        &FactReference::Completion(event),
    )
}

fn validate_call<'a>(
    context: &'a ContextView,
    call: &ChatPartRef,
) -> Result<&'a ChatPart, ContextGap> {
    let reference = FactReference::Part(*call);
    let message = message(context, call.message)?;
    let part = unique(
        message
            .observed_parts
            .iter()
            .filter(|part| part_id(part) == call.part_id),
        context,
        &reference,
    )?;
    let ChatPart::ToolCall { native_id, .. } = part else {
        return Err(conflict(&reference));
    };
    let Some(native) = native_id else {
        return Err(gap(context, &reference, ContextGapReason::NotProvided));
    };
    if native.namespace != NativeIdNamespace::ToolCall {
        return Err(conflict(&reference));
    }
    if message.record_refs.is_empty() {
        return Err(gap(context, &reference, ContextGapReason::NotProvided));
    }
    let mut missing = None;
    for id in &message.record_refs {
        if id.exchange != call.message.exchange {
            return Err(conflict(&reference));
        }
        let source = FactReference::Protocol(*id);
        let records = context
            .source_records
            .iter()
            .filter_map(|record| match record {
                SourceRecord::Protocol(value) if value.id == *id => Some(value.as_ref()),
                _ => None,
            });
        let record = match unique(records, context, &source) {
            Ok(record) => record,
            Err(error) if error.reason == ContextGapReason::ConflictingEvidence => {
                return Err(error);
            }
            Err(error) => {
                missing.get_or_insert(error);
                continue;
            }
        };
        if !protocol_in_scope(message, record.id, &record.source) {
            return Err(conflict(&source));
        }
        if record.native_ids.contains(native) {
            return Ok(part);
        }
    }
    Err(missing.unwrap_or_else(|| conflict(&reference)))
}

/// Read a unique original message with its existing evidence gaps applied.
pub(crate) fn message(
    context: &ContextView,
    id: as_types::flow::ChatMessageId,
) -> Result<&ChatMessage, ContextGap> {
    unique(
        context
            .source_records
            .iter()
            .filter_map(|record| match record {
                SourceRecord::Message(value) if value.id == id => Some(value.as_ref()),
                _ => None,
            }),
        context,
        &FactReference::Message(id),
    )
}

fn require_reference(context: &ContextView, reference: &FactReference) -> Result<(), ContextGap> {
    match reference {
        FactReference::NativeObservation(key) => {
            observation(&context.source_records, *key).map(|_| ())
        }
        FactReference::Event(key) => unique(
            context.source_records.iter().filter_map(|r| match r {
                SourceRecord::OperationResult(v) if EventKey::from(v.event.as_ref()) == *key => {
                    Some(v.event.as_ref())
                }
                SourceRecord::Question(v) => match v.as_ref() {
                    as_main::messages::protocol::Up::Ask { event, .. }
                        if EventKey::from(event.as_ref()) == *key =>
                    {
                        Some(event.as_ref())
                    }
                    _ => None,
                },
                _ => None,
            }),
            context,
            reference,
        )
        .map(|_| ()),
        FactReference::Question(id) => {
            let mut values = context.source_records.iter().filter_map(|r| match r {
                SourceRecord::Question(q) => match q.as_ref() {
                    as_main::messages::protocol::Up::Ask {
                        decision_id, event, ..
                    } if decision_id == id => Some(event.as_ref()),
                    _ => None,
                },
                _ => None,
            });
            unique(&mut values, context, reference).map(|_| ())
        }
        FactReference::Completion(key) => original_completion(context, *key).map(|_| ()),
        FactReference::Protocol(id) => unique(
            context
                .source_records
                .iter()
                .filter_map(|record| match record {
                    SourceRecord::Protocol(value) if value.id == *id => Some(value.as_ref()),
                    _ => None,
                }),
            context,
            reference,
        )
        .map(|_| ()),
        FactReference::Message(id) => message(context, *id).map(|_| ()),
        FactReference::Part(call) => {
            let message = message(context, call.message)?;
            unique(
                message
                    .observed_parts
                    .iter()
                    .filter(|part| part_id(part) == call.part_id),
                context,
                reference,
            )
            .map(|_| ())
        }
        FactReference::Process(_) => Err(gap(context, reference, ContextGapReason::NotProvided)),
    }
}

fn unique<'a, T: PartialEq + 'a>(
    mut values: impl Iterator<Item = &'a T>,
    context: &ContextView,
    reference: &FactReference,
) -> Result<&'a T, ContextGap> {
    if let Some(gap) = context.gaps.iter().find(|gap| {
        &gap.at == reference
            && !matches!(
                gap.reason,
                ContextGapReason::AgentBindingUnavailable
                    | ContextGapReason::CommandMappingUnavailable
                    | ContextGapReason::CommandInputUnavailable
            )
    }) {
        return Err(gap.clone());
    }
    let first = values
        .next()
        .ok_or_else(|| gap(context, reference, ContextGapReason::NotProvided))?;
    if values.any(|value| value != first) {
        return Err(conflict(reference));
    }
    Ok(first)
}

fn gap(context: &ContextView, at: &FactReference, reason: ContextGapReason) -> ContextGap {
    context
        .gaps
        .iter()
        .find(|gap| &gap.at == at)
        .cloned()
        .unwrap_or_else(|| ContextGap {
            at: at.clone(),
            reason,
        })
}

fn conflict(at: &FactReference) -> ContextGap {
    ContextGap {
        at: at.clone(),
        reason: ContextGapReason::ConflictingEvidence,
    }
}

pub(crate) fn part_id(part: &ChatPart) -> u32 {
    match part {
        ChatPart::Text { id, .. }
        | ChatPart::ToolCall { id, .. }
        | ChatPart::ToolResult { id, .. }
        | ChatPart::Opaque { id, .. }
        | ChatPart::MediaRef { id, .. } => *id,
    }
}
