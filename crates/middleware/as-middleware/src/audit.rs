//! Business-only causal views and fixed Ask windows over original, authorized source values.
use crate::attribution::{AttributionRequest, ToolAttribution};
use crate::business_log::BusinessLog;
use crate::{activity::part_id, context::*};
use as_main::messages::{ContentAccess, protocol::Up};
use as_types::{
    DecisionId,
    dispatch_event::{EventData, EventPayload},
    flow::*,
    main_ports::ModuleError,
    process_tree::{EventKey, ProcessKey},
};
use serde::Serialize;
use std::sync::Arc;

/// Original message references only; content appears once in the activity view.
#[derive(Debug, Clone, Serialize)]
pub struct AskMessageWindow {
    pub exchange: ExchangeKey,
    pub message_refs: Vec<ChatMessageId>,
    pub earlier_observed_messages: u32,
    pub source_gaps: Vec<ObservationGap>,
}
/// Content equality is independent of a tool-execution relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    EqualBytes,
    DifferentBytes,
    NotComparable,
}
/// One comparison whose field mapping was explicitly supplied by its caller.
#[derive(Debug, Clone, Serialize)]
pub struct CommandCheck {
    pub call: ChatPartRef,
    pub operation: EventKey,
    pub argument_pointer: String,
    pub exec_argument_index: u32,
    pub outcome: CommandOutcome,
    pub evidence: Vec<FactReference>,
}
/// Source-backed context; this value never owns or changes the pending operation or deadline.
#[derive(Debug, Clone, Serialize)]
pub struct AskAuditContext {
    pub decision_id: DecisionId,
    pub event: EventKey,
    pub agent: Option<ProcessKey>,
    pub trigger_call: Option<ChatPartRef>,
    pub window: Option<AskMessageWindow>,
    pub view: ContextView,
    pub command_checks: Vec<CommandCheck>,
    pub attribution: Option<ToolAttribution>,
    pub candidate_windows: Vec<AskMessageWindow>,
}
fn gap(at: FactReference, reason: ContextGapReason) -> ContextGap {
    ContextGap { at, reason }
}
fn add_gap(view: &mut ContextView, value: ContextGap) {
    if !view.gaps.contains(&value) {
        view.gaps.push(value);
    }
}

fn add_candidate_windows(
    result: &mut AskAuditContext,
    windows: Vec<(AskMessageWindow, Vec<Arc<ChatMessage>>)>,
) {
    for (window, messages) in windows {
        let mut conflict = false;
        for message in messages {
            let existing = result
                .view
                .source_records
                .iter()
                .find_map(|record| match record {
                    SourceRecord::Message(existing) if existing.id == message.id => {
                        Some(existing.revision)
                    }
                    _ => None,
                });
            match existing {
                Some(revision) if revision != message.revision => {
                    conflict = true;
                    add_gap(
                        &mut result.view,
                        gap(
                            FactReference::Message(message.id),
                            ContextGapReason::ConflictingEvidence,
                        ),
                    );
                }
                Some(_) => {}
                None => result
                    .view
                    .source_records
                    .push(SourceRecord::Message(message)),
            }
        }
        if !conflict {
            result.candidate_windows.push(window);
        }
    }
}

/// Compose from a genuinely stored Ask and the original live content grant.
/// SQL reads create one temporary view; they never query or cache Flow history.
pub async fn explain_ask(
    reader: &ContentAccess,
    log: &BusinessLog,
    decision_id: DecisionId,
    attribution: Option<AttributionRequest>,
) -> Result<AskAuditContext, ModuleError> {
    reader.authorize()?;
    let read = log
        .read_ask_context(decision_id, attribution)
        .await
        .map_err(store_error)?;
    let graph_messages = read
        .view
        .source_records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::Message(message) => Some(message.id),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let mut result = compose(read.ask, read.view, read.call, read.window)?;
    add_candidate_windows(&mut result, read.candidate_windows);
    let attribution = match read.attribution {
        Some(Ok(value)) => Some(value),
        Some(Err(crate::business_log::StoreError::NotProvided)) => {
            let reference = FactReference::Event(result.event);
            add_gap(
                &mut result.view,
                gap(reference, ContextGapReason::NotProvided),
            );
            None
        }
        Some(Err(error)) => return Err(store_error(error)),
        None => None,
    };
    if let Some(attribution) = attribution {
        for candidate in &attribution.matches.candidates {
            if result.window.as_ref().is_some_and(|window| window.exchange == candidate.call.message.exchange)
                && !result.view.source_records.iter().any(|record| matches!(record,
                    SourceRecord::Message(message) if message.id == candidate.call.message && message.revision == candidate.message_revision)) {
                add_gap(&mut result.view, gap(FactReference::Part(candidate.call), ContextGapReason::NotProvided));
            }
        }
        result.attribution = Some(attribution.matches);
        add_candidate_windows(&mut result, attribution.windows);
    }
    limit_candidate_windows(&mut result, &graph_messages);
    result.view.defer_originals();
    reader.authorize()?;
    Ok(result)
}
fn limit_candidate_windows(
    result: &mut AskAuditContext,
    graph_messages: &std::collections::HashSet<ChatMessageId>,
) {
    let confirmed: std::collections::HashSet<ChatMessageId> = result
        .window
        .as_ref()
        .map(|window| window.message_refs.iter().copied().collect())
        .unwrap_or_default();
    let candidates = result
        .candidate_windows
        .iter()
        .flat_map(|window| window.message_refs.iter().copied())
        .collect::<std::collections::HashSet<_>>();
    let mut messages = result
        .view
        .source_records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::Message(message)
                if candidates.contains(&message.id) && !confirmed.contains(&message.id) =>
            {
                Some(message)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    messages.sort_by_key(|message| std::cmp::Reverse(message.last_seen_boottime_ns));
    let selected = messages
        .into_iter()
        .take(20usize.saturating_sub(confirmed.len()))
        .map(|message| message.id)
        .chain(confirmed.iter().copied())
        .collect::<std::collections::HashSet<_>>();
    for window in &mut result.candidate_windows {
        let before = window.message_refs.len();
        window
            .message_refs
            .retain(|reference| selected.contains(reference));
        window.earlier_observed_messages = window
            .earlier_observed_messages
            .saturating_add((before - window.message_refs.len()) as u32);
    }
    result
        .candidate_windows
        .retain(|window| !window.message_refs.is_empty());
    result.view.source_records.retain(|record| match record {
        SourceRecord::Message(message) => {
            graph_messages.contains(&message.id) || selected.contains(&message.id)
        }
        _ => true,
    });
}
fn store_error(error: crate::business_log::StoreError) -> ModuleError {
    match error {
        crate::business_log::StoreError::Conflict => ModuleError::Conflict,
        crate::business_log::StoreError::Invalid => ModuleError::InvalidInput,
        _ => ModuleError::Unavailable,
    }
}
pub(crate) type WindowRead =
    Result<(AskMessageWindow, Vec<Arc<ChatMessage>>), crate::business_log::StoreError>;
fn compose(
    ask: Arc<Up>,
    mut view: ContextView,
    call: Result<ChatPartRef, ContextGap>,
    window: Option<WindowRead>,
) -> Result<AskAuditContext, ModuleError> {
    let Up::Ask {
        decision_id, event, ..
    } = ask.as_ref()
    else {
        return Err(ModuleError::InvalidInput);
    };
    let event = EventKey::from(event.as_ref());
    view.anchor = FactReference::Question(*decision_id);
    add_gap(
        &mut view,
        gap(
            FactReference::Event(event),
            ContextGapReason::AgentBindingUnavailable,
        ),
    );
    let mut result = AskAuditContext {
        decision_id: *decision_id,
        event,
        agent: None,
        trigger_call: None,
        window: None,
        view,
        command_checks: Vec::new(),
        attribution: None,
        candidate_windows: Vec::new(),
    };
    let call = match call {
        Ok(call) => call,
        Err(error) => {
            add_gap(&mut result.view, error);
            return Ok(result);
        }
    };
    result.trigger_call = Some(call);
    add_gap(
        &mut result.view,
        gap(
            FactReference::Part(call),
            ContextGapReason::CommandMappingUnavailable,
        ),
    );
    let (window, messages) = match window {
        Some(Ok(value)) => value,
        Some(Err(error)) => {
            add_gap(
                &mut result.view,
                gap(
                    FactReference::Message(call.message),
                    match error {
                        crate::business_log::StoreError::NotProvided => {
                            ContextGapReason::NotProvided
                        }
                        crate::business_log::StoreError::Conflict => {
                            ContextGapReason::ConflictingEvidence
                        }
                        _ => ContextGapReason::SourceUnavailable,
                    },
                ),
            );
            return Ok(result);
        }
        None => {
            add_gap(
                &mut result.view,
                gap(
                    FactReference::Message(call.message),
                    ContextGapReason::NotProvided,
                ),
            );
            return Ok(result);
        }
    };
    if window.exchange != call.message.exchange
        || window.message_refs != messages.iter().map(|m| m.id).collect::<Vec<_>>()
    {
        add_gap(
            &mut result.view,
            gap(
                FactReference::Part(call),
                ContextGapReason::ConflictingEvidence,
            ),
        );
        return Ok(result);
    }
    for message in messages {
        result
            .view
            .source_records
            .retain(|r| !matches!(r,SourceRecord::Message(m)if m.id==message.id));
        result
            .view
            .source_records
            .push(SourceRecord::Message(message));
    }
    result.window = Some(window);
    Ok(result)
}

/// Compare explicitly mapped data. The output has no causal relation or enforcement capability.
pub fn compare_command(
    call: ChatPartRef,
    message: &ChatMessage,
    event: &EventData,
    pointer: &str,
    index: u32,
) -> Result<(CommandCheck, Vec<ContextGap>), ContextGap> {
    if message.id != call.message {
        return Err(gap(
            FactReference::Part(call),
            ContextGapReason::ConflictingEvidence,
        ));
    }
    let part = message
        .observed_parts
        .iter()
        .find(|p| part_id(p) == call.part_id);
    let requested = match part {
        Some(ChatPart::ToolCall {
            arguments: Some(ChatContent::Json(v)),
            ..
        }) => v.pointer(pointer).and_then(|v| v.as_str()),
        Some(ChatPart::ToolCall {
            arguments: Some(ChatContent::Text(v)),
            ..
        }) if pointer.is_empty() => Some(v.as_str()),
        Some(ChatPart::ToolCall { .. }) => None,
        _ => {
            return Err(gap(
                FactReference::Part(call),
                ContextGapReason::ConflictingEvidence,
            ));
        }
    };
    let actual = match &event.payload {
        EventPayload::Process(p) => p.target_argv.as_ref().and_then(|v| v.get(index as usize)),
        _ => None,
    };
    let outcome = match (requested, actual) {
        (Some(a), Some(b)) if a.as_bytes() == b => CommandOutcome::EqualBytes,
        (Some(_), Some(_)) => CommandOutcome::DifferentBytes,
        _ => CommandOutcome::NotComparable,
    };
    let gaps = if outcome == CommandOutcome::NotComparable {
        vec![gap(
            FactReference::Part(call),
            ContextGapReason::CommandInputUnavailable,
        )]
    } else {
        vec![]
    };
    Ok((
        CommandCheck {
            call,
            operation: EventKey::from(event),
            argument_pointer: pointer.into(),
            exec_argument_index: index,
            outcome,
            evidence: vec![
                FactReference::Part(call),
                FactReference::Event(EventKey::from(event)),
            ],
        },
        gaps,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::execution_call;
    use crate::{activity::activity_view, execution_feedback::tests::Fixture};
    #[test]
    fn candidate_exchanges_share_one_twenty_message_window_budget() {
        let fixture = Fixture::new();
        let ask = question(&fixture);
        let original_view = view(&fixture, ask.clone(), false);
        let mut result = compose(
            ask,
            original_view,
            Err(ContextGap {
                at: FactReference::Part(fixture.call),
                reason: ContextGapReason::NoExplicitRelation,
            }),
            None,
        )
        .expect("original question");
        let mut windows = Vec::new();
        for exchange in 101..=102 {
            let mut messages = Vec::new();
            for sequence in 1..=20 {
                let mut message = fixture.message.clone();
                message.id.exchange.sequence = exchange;
                message.id.sequence = sequence;
                message.last_seen_boottime_ns = exchange * 100 + sequence;
                messages.push(Arc::new(message));
            }
            windows.push((
                AskMessageWindow {
                    exchange: messages[0].id.exchange,
                    message_refs: messages.iter().map(|message| message.id).collect(),
                    earlier_observed_messages: 0,
                    source_gaps: vec![],
                },
                messages,
            ));
        }
        add_candidate_windows(&mut result, windows);
        limit_candidate_windows(&mut result, &Default::default());
        assert_eq!(
            result
                .candidate_windows
                .iter()
                .map(|window| window.message_refs.len())
                .sum::<usize>(),
            20
        );
        assert_eq!(result.candidate_windows[0].exchange.sequence, 102);
        assert_eq!(
            result
                .view
                .source_records
                .iter()
                .filter(|record| matches!(record, SourceRecord::Message(_)))
                .count(),
            20
        );
        assert!(result.trigger_call.is_none());
    }
    fn question(f: &Fixture) -> Arc<Up> {
        Arc::new(Up::Ask {
            decision_id: DecisionId([7; 16]),
            hit: f.completion.hit.clone().unwrap(),
            event: f.completion.event.clone(),
            choices: vec![as_types::main_ports::Decision::Allow],
            timeout: as_types::main_ports::Decision::Allow,
            remaining_ms: 30000,
        })
    }
    fn view(f: &Fixture, q: Arc<Up>, bound: bool) -> ContextView {
        let event = EventKey::from(f.completion.event.as_ref());
        let records = vec![
            SourceRecord::Question(q),
            SourceRecord::Protocol(Arc::new(f.protocol.clone())),
            SourceRecord::Message(Arc::new(f.message.clone())),
        ];
        let links = if bound {
            vec![EvidenceRelation {
                kind: RelationKind::ToolExecution,
                from: FactReference::Part(f.call),
                to: FactReference::Event(event),
                evidence: vec![
                    FactReference::Part(f.call),
                    FactReference::Protocol(f.protocol.id),
                    FactReference::Question(DecisionId([7; 16])),
                ],
            }]
        } else {
            vec![]
        };
        activity_view(FactReference::Question(DecisionId([7; 16])), records, links)
    }
    #[test]
    fn request_and_execution_content_equality_does_not_create_a_causal_edge() {
        let mut f = Fixture::new();
        let mut event = (*f.completion.event).clone();
        event.payload = EventPayload::Process(as_types::dispatch_event::ProcessData {
            target_argv: Some(vec![b"shell".to_vec(), b"cat /same".to_vec()]),
            ..Default::default()
        });
        if let ChatPart::ToolCall { arguments, .. } = &mut f.message.observed_parts[0] {
            *arguments = Some(ChatContent::Json(
                serde_json::json!({"command":"cat /same"}),
            ));
        }
        let check = compare_command(f.call, &f.message, &event, "/command", 1).unwrap();
        assert_eq!(check.0.outcome, CommandOutcome::EqualBytes);
        let ask = question(&f);
        let v = view(&f, ask, false);
        assert!(
            !v.relations
                .iter()
                .any(|r| r.kind == RelationKind::ToolExecution)
        );
        assert!(execution_call(&v, EventKey::from(f.completion.event.as_ref())).is_err());
        assert_eq!(
            compare_command(f.call, &f.message, &event, "/absent", 1)
                .unwrap()
                .0
                .outcome,
            CommandOutcome::NotComparable
        );
    }
    #[test]
    fn actual_question_and_receipt_share_identity_without_claiming_tool_execution() {
        let f = Fixture::new();
        let ask = question(&f);
        let mut receipt = f.completion.clone();
        receipt.decision_id = Some(DecisionId([7; 16]));
        let view = activity_view(
            FactReference::Question(DecisionId([7; 16])),
            vec![
                SourceRecord::Question(ask),
                SourceRecord::OperationResult(Arc::new(receipt)),
            ],
            vec![],
        );
        assert_eq!(view.relations.len(), 1);
        assert_eq!(view.relations[0].kind, RelationKind::QuestionCompletion);
    }

    #[test]
    fn execution_relation_requires_the_original_event_even_with_valid_protocol_evidence() {
        let fixture = Fixture::new();
        let event = EventKey::from(fixture.completion.event.as_ref());
        let mut view = view(&fixture, question(&fixture), true);
        view.relations[0].evidence = vec![FactReference::Protocol(fixture.protocol.id)];
        assert_eq!(execution_call(&view, event), Ok(fixture.call));

        view.source_records
            .retain(|record| !matches!(record, SourceRecord::Question(_)));
        assert_eq!(
            execution_call(&view, event),
            Err(gap(
                FactReference::Event(event),
                ContextGapReason::NotProvided
            ))
        );
    }

    #[test]
    fn execution_relation_only_accepts_a_complete_provider_response_call() {
        let mut fixture = Fixture::new();
        let event = EventKey::from(fixture.completion.event.as_ref());
        fixture.message.origin = ChatOrigin::RequestContext;
        let history = view(&fixture, question(&fixture), true);
        assert_eq!(
            execution_call(&history, event).unwrap_err().reason,
            ContextGapReason::ConflictingEvidence
        );
        fixture.message.origin = ChatOrigin::ResponseOutput;
        fixture.message.state = ChatState::Streaming;
        let partial = view(&fixture, question(&fixture), true);
        assert_eq!(
            execution_call(&partial, event).unwrap_err().reason,
            ContextGapReason::NotProvided
        );
    }
}
