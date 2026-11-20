//! Software evidence routing only; these fixtures do not claim a native execution bridge or TLS delivery.
use super::*;
use crate::codec as protocol;
use crate::context::{EvidenceRelation, RelationKind, SourceRecord};
use as_kernel_common::event_frame::{
    EVENT_FRAME_HEADER_LEN, EVENT_FRAME_VERSION, EventFrameHeader,
};
use as_main::messages::protocol::{FailureCode, OperationProposal, Up};
use as_types::{
    RuleId, RuleVersion,
    configuration::ConfigVersion,
    dispatch_event::{EventKind, EventStage, ProcessInfo},
    flow::{
        ActorFacts, ChatMessage, ChatOrigin, ChatPart, ChatPartRef, ChatRole, ChatState,
        EvidenceBody, EvidenceSegment, ExchangeKey, NativeIdNamespace, NativeIdValue,
        NativeProtocolId, ProtocolRecordId, ProtocolSide, ProtocolSource, ProtocolUnit,
        ProviderProfile,
    },
    main_ports::{ChoiceReason, Decision, RuleHit, RuleSourceVersion, Termination},
    operation::{SignalOutcome, TerminationReport},
};
use std::num::NonZeroU32;
use std::sync::Arc;
use zerocopy::IntoBytes;

pub(crate) struct Fixture {
    pub(crate) completion: OperationResult,
    pub(crate) protocol: as_types::flow::ProtocolObservation,
    pub(crate) message: ChatMessage,
    pub(crate) call: ChatPartRef,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let header = EventFrameHeader {
            version: EVENT_FRAME_VERSION.into(),
            header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
            record_len: (EVENT_FRAME_HEADER_LEN as u32).into(),
            kind: (EventKind::File as u16).into(),
            stage: (EventStage::ExecutionResult as u16).into(),
            operation: (as_types::event_operation::FileOperation::Open as u32).into(),
            source_instance: [1; 16],
            sequence_domain: 1.into(),
            sequence: 7.into(),
            source_boottime_ns: 0.into(),
            actor_tgid: 0.into(),
            actor_tid: 0.into(),
            actor_birth_boottime_ns: 0.into(),
            actor_cgroup_id: 0.into(),
            available: 0.into(),
            common_process_len: 0.into(),
        };
        let completion = OperationResult {
            decision_id: None,
            event: Arc::new(
                as_types::dispatch_event::decode(header.as_bytes()).expect("current event"),
            ),
            hit: Some(RuleHit {
                group_id: "files".into(),
                group_number: NonZeroU32::new(1).expect("nonzero"),
                rule_id: RuleId::from("protected-file"),
                rule_number: NonZeroU32::new(1).expect("nonzero"),
                source: RuleSourceVersion::RulesDocument(RuleVersion::from_digest([3; 32])),
                log: false,
                deny_message: Some("AS stopped this file read.\nKeep the original text.".into()),
            }),
            result: OperationReport::Receipt {
                proposal: None,
                outcome: ExecutionOutcome::Denied,
                termination: TerminationReport::NONE,
            },
        };
        let exchange = ExchangeKey {
            flow_instance: [2; 16],
            sequence: 11,
        };
        let id = ProtocolRecordId {
            exchange,
            sequence: 1,
        };
        let source = Arc::new(ProtocolSource {
            actor: ActorFacts {
                process: Some(as_types::process_tree::ProcessKey {
                    boot_id: [8; 16],
                    host_tgid: 42,
                    birth_boottime_ns: 7,
                }),
                info: ProcessInfo::default(),
                tid: None,
                thread_birth_boottime_ns: None,
                cgroup_id: None,
                observed_boottime_ns: 0,
            },
            channel: None,
            side: ProtocolSide::UpstreamResponse,
            request_traceparent: None,
            profile: ProviderProfile::OpenAiResponses,
            origin: "https://model.invalid".into(),
            route_label: "configured-route".into(),
            config_version: ConfigVersion::from_digest([4; 32]),
            observed_at_utc_ms: None,
            actual_auth_scope: None,
            credential_use: None,
        });
        let native = NativeProtocolId {
            namespace: NativeIdNamespace::ToolCall,
            field_path: "/output/0/call_id".into(),
            value: NativeIdValue::JsonString("original-call".into()),
        };
        let body = br#"{"output":[{"type":"function_call","call_id":"original-call","name":"read_file","arguments":"{}"}]}"#.to_vec();
        let protocol = as_types::flow::ProtocolObservation {
            id,
            source: source.clone(),
            unit: ProtocolUnit::HttpJsonBody,
            event_name: None,
            body: EvidenceBody {
                original_length: Some(body.len() as u64),
                segments: vec![EvidenceSegment::Bytes {
                    offset: 0,
                    data: body,
                }],
            },
            native_ids: vec![native.clone()],
            quality: Vec::new(),
            request_session: None,
        };
        let call = ChatPartRef {
            message: as_types::flow::ChatMessageId {
                exchange,
                sequence: 2,
            },
            part_id: 0,
        };
        let message = ChatMessage {
            id: call.message,
            revision: 1,
            source,
            record_refs: vec![id],
            origin: ChatOrigin::ResponseOutput,
            role: ChatRole::Assistant,
            candidate_index: None,
            model: None,
            observed_parts: vec![ChatPart::ToolCall {
                id: 0,
                native_id: Some(native),
                name: "read_file".into(),
                arguments: Some(as_types::flow::ChatContent::Json(serde_json::json!({}))),
            }],
            state: ChatState::Complete,
            quality: Vec::new(),
            provider_status: None,
            finish_reason: None,
            usage: None,
            first_seen_boottime_ns: 0,
            last_seen_boottime_ns: 0,
        };
        Self {
            completion,
            protocol,
            message,
            call,
        }
    }

    fn route(&self) -> FeedbackPreparation {
        let event = EventKey::from(self.completion.event.as_ref());
        let evidence = [
            FactReference::Completion(event),
            FactReference::Protocol(self.protocol.id),
        ];
        let relations = [EvidenceRelation {
            kind: RelationKind::ToolExecution,
            from: FactReference::Part(self.call),
            to: FactReference::Event(event),
            evidence: evidence.to_vec(),
        }];
        self.route_with(&relations, &[])
    }

    fn route_with(
        &self,
        relations: &[EvidenceRelation],
        gaps: &[ContextGap],
    ) -> FeedbackPreparation {
        let source_records = [
            SourceRecord::OperationResult(Arc::new(self.completion.clone())),
            SourceRecord::Protocol(Arc::new(self.protocol.clone())),
            SourceRecord::Message(Arc::new(self.message.clone())),
        ];
        prepare_feedback(
            &self.completion,
            &ContextView {
                anchor: FactReference::Completion(EventKey::from(self.completion.event.as_ref())),
                source_records: source_records.to_vec(),
                relations: relations.to_vec(),
                gaps: gaps.to_vec(),
            },
        )
    }
}

#[test]
fn original_completion_wire_and_explicit_bridge_produce_only_original_references() {
    let mut fixture = Fixture::new();
    let encoded = protocol::encode_up(&Up::OperationResult(Arc::new(fixture.completion.clone())))
        .expect("current wire");
    fixture.completion =
        protocol::decode_operation_result(&encoded).expect("original completion value");
    let FeedbackPreparation::Request(request) = fixture.route() else {
        panic!("explicit bridge");
    };
    assert_eq!(
        request.denial,
        EventKey::from(fixture.completion.event.as_ref())
    );
    assert_eq!(request.call, fixture.call);
    let wire = serde_json::to_value(request).expect("request");
    assert!(wire.get("text").is_none());
    assert!(wire.get("deny_message").is_none());
    assert_eq!(wire.as_object().expect("object").len(), 2);
    let completion_wire: serde_json::Value =
        serde_json::from_slice(&encoded).expect("completion wire");
    assert_eq!(
        completion_wire["message"]["decision_id"],
        serde_json::Value::Null
    );
    assert_eq!(completion_wire["message"]["hit"]["log"], false);
}

#[test]
fn only_actual_denial_with_nonempty_original_text_applies() {
    let mut fixture = Fixture::new();
    fixture.completion.decision_id = Some(as_types::DecisionId([5; 16]));
    for outcome in [
        ExecutionOutcome::Allowed,
        ExecutionOutcome::Unknown,
        ExecutionOutcome::Cancelled,
    ] {
        fixture.completion.result = OperationReport::Receipt {
            proposal: Some(OperationProposal {
                decision: Decision::Deny {
                    terminate: Termination::None,
                },
                reason: ChoiceReason::User,
            }),
            outcome,
            termination: TerminationReport::NONE,
        };
        assert_eq!(fixture.route(), FeedbackPreparation::NotApplicable);
    }
    fixture.completion.result = OperationReport::Failed {
        proposal: Some(OperationProposal {
            decision: Decision::Deny {
                terminate: Termination::None,
            },
            reason: ChoiceReason::Timeout,
        }),
        code: FailureCode::Failed,
        message: "actual failure".into(),
    };
    assert_eq!(fixture.route(), FeedbackPreparation::NotApplicable);
    let mut fixture = Fixture::new();
    fixture.completion.decision_id = Some(as_types::DecisionId([6; 16]));
    fixture.completion.result = OperationReport::Receipt {
        proposal: Some(OperationProposal {
            decision: Decision::Deny {
                terminate: Termination::Actor,
            },
            reason: ChoiceReason::Timeout,
        }),
        outcome: ExecutionOutcome::Denied,
        termination: TerminationReport {
            requested: Termination::Actor,
            actor: SignalOutcome::Failed { native_error: -1 },
            parent: SignalOutcome::NotRequested,
        },
    };
    assert!(
        matches!(fixture.route(), FeedbackPreparation::Request(_)),
        "termination failure does not erase actual denial"
    );
    for text in [None, Some(String::new())] {
        fixture.completion.hit.as_mut().expect("hit").deny_message = text;
        assert_eq!(fixture.route(), FeedbackPreparation::NotApplicable);
    }
    fixture.completion.hit.as_mut().expect("hit").deny_message = Some(" \n".into());
    assert!(
        matches!(fixture.route(), FeedbackPreparation::Request(_)),
        "fixed whitespace is not rewritten"
    );
    fixture.completion.hit = None;
    assert_eq!(fixture.route(), FeedbackPreparation::NotApplicable);
}

#[test]
fn native_protocol_reference_does_not_create_an_execution_bridge() {
    let fixture = Fixture::new();
    let event = EventKey::from(fixture.completion.event.as_ref());
    assert_eq!(
        fixture.route_with(&[], &[]),
        FeedbackPreparation::Gap(ContextGap {
            at: FactReference::Event(event),
            reason: ContextGapReason::NoExplicitRelation,
        })
    );
    let evidence = [FactReference::Protocol(fixture.protocol.id)];
    let relation = EvidenceRelation {
        kind: RelationKind::NativeToolReference,
        from: FactReference::Part(fixture.call),
        to: FactReference::Event(event),
        evidence: evidence.to_vec(),
    };
    assert!(matches!(
        fixture.route_with(&[relation], &[]),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::NoExplicitRelation,
            ..
        })
    ));
}

#[test]
fn absent_evidence_and_conflicting_original_tool_provenance_never_route() {
    let mut fixture = Fixture::new();
    let event = EventKey::from(fixture.completion.event.as_ref());
    let absent = FactReference::Protocol(ProtocolRecordId {
        exchange: fixture.protocol.id.exchange,
        sequence: 900,
    });
    let evidence = [absent.clone()];
    let relations = [EvidenceRelation {
        kind: RelationKind::ToolExecution,
        from: FactReference::Part(fixture.call),
        to: FactReference::Event(event),
        evidence: evidence.to_vec(),
    }];
    let gap = ContextGap {
        at: absent,
        reason: ContextGapReason::NotRetained,
    };
    assert_eq!(
        fixture.route_with(&relations, std::slice::from_ref(&gap)),
        FeedbackPreparation::Gap(gap)
    );
    fixture.protocol.native_ids[0].field_path = "/output/1/call_id".into();
    assert!(matches!(
        fixture.route(),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::ConflictingEvidence,
            ..
        })
    ));
    fixture.protocol.native_ids[0].field_path = "/output/0/call_id".into();
    Arc::make_mut(&mut fixture.protocol.source).profile = ProviderProfile::AnthropicMessages;
    assert!(matches!(
        fixture.route(),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::ConflictingEvidence,
            ..
        })
    ));
}

#[test]
fn a_tool_result_is_not_the_original_tool_call() {
    let mut fixture = Fixture::new();
    fixture.message.observed_parts[0] = ChatPart::ToolResult {
        id: fixture.call.part_id,
        native_id: Some(fixture.protocol.native_ids[0].clone()),
        content: as_types::flow::ChatContent::Text("tool output".into()),
    };
    assert!(matches!(
        fixture.route(),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::ConflictingEvidence,
            ..
        })
    ));
}

#[test]
fn missing_original_tool_identity_stays_a_gap() {
    let mut fixture = Fixture::new();
    let ChatPart::ToolCall { native_id, .. } = &mut fixture.message.observed_parts[0] else {
        panic!("call fixture");
    };
    *native_id = None;
    assert!(matches!(
        fixture.route(),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::NotProvided,
            ..
        })
    ));
}

#[test]
fn reversed_or_ambiguous_execution_relations_are_not_reinterpreted() {
    let fixture = Fixture::new();
    let event = FactReference::Event(EventKey::from(fixture.completion.event.as_ref()));
    let evidence = [FactReference::Protocol(fixture.protocol.id)];
    let reversed = EvidenceRelation {
        kind: RelationKind::ToolExecution,
        from: event.clone(),
        to: FactReference::Part(fixture.call),
        evidence: evidence.to_vec(),
    };
    assert!(matches!(
        fixture.route_with(&[reversed], &[]),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::NoExplicitRelation,
            ..
        })
    ));
    let mut another_call = fixture.call;
    another_call.part_id += 1;
    let relations = [
        EvidenceRelation {
            kind: RelationKind::ToolExecution,
            from: FactReference::Part(fixture.call),
            to: event.clone(),
            evidence: evidence.to_vec(),
        },
        EvidenceRelation {
            kind: RelationKind::ToolExecution,
            from: FactReference::Part(another_call),
            to: event,
            evidence: evidence.to_vec(),
        },
    ];
    assert!(matches!(
        fixture.route_with(&relations, &[]),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::ConflictingEvidence,
            ..
        })
    ));
}

#[test]
fn same_event_identity_does_not_replace_the_original_completion() {
    let fixture = Fixture::new();
    let mut changed = fixture.completion.clone();
    changed.hit.as_mut().expect("hit").deny_message = Some("another revision's text".into());
    let event = EventKey::from(fixture.completion.event.as_ref());
    let evidence = [FactReference::Completion(event)];
    let relations = [EvidenceRelation {
        kind: RelationKind::ToolExecution,
        from: FactReference::Part(fixture.call),
        to: FactReference::Event(event),
        evidence: evidence.to_vec(),
    }];
    let records = [
        SourceRecord::OperationResult(Arc::new(changed.clone())),
        SourceRecord::Protocol(Arc::new(fixture.protocol.clone())),
        SourceRecord::Message(Arc::new(fixture.message.clone())),
    ];
    let view = ContextView {
        anchor: FactReference::Completion(event),
        source_records: records.to_vec(),
        relations: relations.to_vec(),
        gaps: vec![],
    };
    assert_eq!(
        prepare_feedback(&fixture.completion, &view),
        FeedbackPreparation::Gap(ContextGap {
            at: FactReference::Completion(event),
            reason: ContextGapReason::ConflictingEvidence,
        })
    );
}
