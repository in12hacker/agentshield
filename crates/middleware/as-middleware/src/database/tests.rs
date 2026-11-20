//! Actual temporary SQLite regressions; no native execution or remote authentication claim.
use super::*;
use crate::{context::*, execution_feedback::tests::Fixture};
use as_main::messages::protocol::Up;
use as_types::{
    DecisionId,
    flow::*,
    main_ports::{ChoiceReason, Decision, DispatchOutcome, MainNotice, RuleChoice, Termination},
    operation::{ExecutionOutcome, OperationReceipt, TerminationReport},
    process_tree::EventKey,
};
use std::sync::Arc;

async fn chat(log: &BusinessLog, sequence: u64, event: ChatEvent) -> Result<(), StoreError> {
    let envelope = Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [9; 16],
            sequence,
        },
        event: Arc::new(event),
    });
    let original = Arc::downgrade(&envelope);
    log.record_chat(envelope).await?;
    assert!(
        original.upgrade().is_none(),
        "dequeued/committed source envelope is not a history cache"
    );
    Ok(())
}

#[tokio::test]
async fn deferred_protocol_original_is_read_in_exact_byte_pages_without_expanding_the_graph()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("original-pages.sqlite"))?;
    let mut original = Fixture::new().protocol;
    let bytes = (0..=255u8).cycle().take(513).collect::<Vec<_>>();
    original.body = EvidenceBody::from_bytes(bytes.clone());
    original.body.original_length = None;
    original.body.segments.insert(
        0,
        EvidenceSegment::Missing {
            offset: 0,
            length: Some(0),
            reason: String::new(),
        },
    );
    original.body.segments.push(EvidenceSegment::Missing {
        offset: 513,
        length: None,
        reason: "unobtained tail".into(),
    });
    let reference = FactReference::Protocol(original.id);
    chat(&log, 1, ChatEvent::Observation(Arc::new(original.clone()))).await?;
    let mut view = log.activity(reference.clone()).await?;
    view.defer_originals();
    assert!(
        view.source_records
            .iter()
            .any(|record| matches!(record, SourceRecord::Deferred(value) if value == &reference))
    );
    assert!(
        !view
            .source_records
            .iter()
            .any(|record| matches!(record, SourceRecord::Protocol(_)))
    );
    let mut offset = 0;
    let mut obtained = Vec::new();
    let mut missing = Vec::new();
    loop {
        let page = log.original_record(reference.clone(), offset, 127).await?;
        assert!(page.relations.is_empty());
        assert_eq!(page.source_records.len(), 1);
        let SourceRecord::ProtocolPage {
            observation,
            next_offset,
        } = &page.source_records[0]
        else {
            panic!("original page");
        };
        assert_eq!(observation.id, original.id);
        assert_eq!(observation.source, original.source);
        assert_eq!(
            observation.body.original_length,
            original.body.original_length
        );
        for segment in &observation.body.segments {
            let EvidenceSegment::Bytes { offset, data } = segment else {
                missing.push(segment.clone());
                continue;
            };
            assert_eq!(*offset, obtained.len() as u64);
            obtained.extend_from_slice(data);
        }
        let Some(next) = next_offset else {
            break;
        };
        offset = *next;
    }
    assert_eq!(obtained, bytes);
    assert_eq!(
        missing,
        original
            .body
            .segments
            .iter()
            .filter(|segment| matches!(segment, EvidenceSegment::Missing { .. }))
            .cloned()
            .collect::<Vec<_>>()
    );
    let view = log.activity(reference).await?;
    assert!(
        view.source_records.iter().any(
            |record| matches!(record, SourceRecord::Protocol(value) if value.as_ref() == &original)
        ),
        "query projection does not delete or rewrite the stored original"
    );
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn dns_send_snapshots_round_trip_actual_bytes_and_analysis_without_relations()
-> Result<(), StoreError> {
    use as_types::dispatch_event::{DnsQueryAnalysis, DnsSendSnapshot, NetworkData};
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("dns.sqlite"))?;
    let cases = [
        None,
        Some(DnsSendSnapshot {
            requested_bytes: 0,
            attempted_bytes: 0,
            send_flags: 0,
            data: vec![],
            analysis: Some(DnsQueryAnalysis::NotDns),
        }),
        Some(DnsSendSnapshot {
            requested_bytes: u64::MAX,
            attempted_bytes: 4,
            send_flags: u32::MAX,
            data: vec![0, 255, 0, 1],
            analysis: Some(DnsQueryAnalysis::CaptureLimited),
        }),
        Some(DnsSendSnapshot {
            requested_bytes: 20,
            attempted_bytes: 20,
            send_flags: 0,
            data: vec![0, 255],
            analysis: Some(DnsQueryAnalysis::ShortRead),
        }),
        Some(DnsSendSnapshot {
            requested_bytes: 12,
            attempted_bytes: 12,
            send_flags: 0,
            data: vec![0; 12],
            analysis: None,
        }),
        Some(DnsSendSnapshot {
            requested_bytes: 19,
            attempted_bytes: 19,
            send_flags: 0,
            data: b"\0\x01\x01\0\0\x01\0\0\0\0\0\0\x01a\0\0\x01\0\x01".to_vec(),
            analysis: Some(DnsQueryAnalysis::Question {
                qtype: 1,
                qclass: 1,
            }),
        }),
        Some(DnsSendSnapshot {
            requested_bytes: 12,
            attempted_bytes: 12,
            send_flags: 0,
            data: vec![255; 12],
            analysis: Some(DnsQueryAnalysis::Malformed {
                reason: "actual parser error".into(),
            }),
        }),
    ];
    for (index, snapshot) in cases.into_iter().enumerate() {
        let mut result = Fixture::new().completion;
        let mut value = serde_json::to_value(result.event.as_ref())?;
        value["header"]["kind"] = serde_json::json!("network");
        value["header"]["operation"] = serde_json::json!(2);
        value["header"]["sequence"] = row::number(index as u64 + 100);
        let target_domain = snapshot.as_ref().and_then(|s| {
            matches!(s.analysis, Some(DnsQueryAnalysis::Question { .. })).then(|| b"a".to_vec())
        });
        value["payload"] = serde_json::to_value(NetworkData {
            remote_port: Some(53),
            protocol: Some(17),
            dns_snapshot: snapshot,
            target_domain,
            ..NetworkData::default()
        })?;
        result.event = Arc::new(serde_json::from_value(value)?);
        let key = EventKey::from(result.event.as_ref());
        let result = Arc::new(result);
        log.record_up(Arc::new(Up::OperationResult(result.clone())))?
            .await?;
        let view = log.activity(FactReference::Completion(key)).await?;
        let actual = view
            .source_records
            .iter()
            .find_map(|record| match record {
                SourceRecord::OperationResult(value) => Some(value),
                _ => None,
            })
            .ok_or(StoreError::NotProvided)?;
        assert_eq!(actual.as_ref(), result.as_ref());
        assert!(view.relations.is_empty());
    }
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn mount_effect_preserves_unknown_and_missing_without_reclassifying() -> Result<(), StoreError>
{
    use as_types::dispatch_event::MountEffect;
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("mount.sqlite"))?;
    for (index, effect) in [
        None,
        Some(MountEffect::IndependentView),
        Some(MountEffect::ProtectedTree),
        Some(MountEffect::Unknown),
    ]
    .into_iter()
    .enumerate()
    {
        let mut result = Fixture::new().completion;
        let mut value = serde_json::to_value(result.event.as_ref())?;
        value["header"]["operation"] = serde_json::json!(14);
        value["header"]["sequence"] = row::number(index as u64 + 200);
        value["payload"]["mount_effect"] = serde_json::to_value(effect)?;
        result.event = Arc::new(serde_json::from_value(value)?);
        let key = EventKey::from(result.event.as_ref());
        let result = Arc::new(result);
        log.record_up(Arc::new(Up::OperationResult(result.clone())))?
            .await?;
        let view = log.activity(FactReference::Completion(key)).await?;
        let actual = view
            .source_records
            .iter()
            .find_map(|record| match record {
                SourceRecord::OperationResult(value) => Some(value),
                _ => None,
            })
            .ok_or(StoreError::NotProvided)?;
        assert_eq!(actual.as_ref(), result.as_ref());
        assert!(view.relations.is_empty());
    }
    log.close().await?;
    Ok(())
}

/// Explicit software source for business tests, not a native execution assertion.
/// The coordinator explicitly declares current execution; a unique public candidate does not.
async fn record_tool_fixture(
    log: &BusinessLog,
    facts: as_types::main_ports::ToolExecutionFacts,
) -> Result<(), StoreError> {
    use as_types::{
        driver::{NativeRequestOperation, NativeSyncThread},
        runtime_observation::{
            CaptureAnalysis, NativeApiSite, NativeCaptureStatus, NativeEventBinding,
            NativeLibraryCall, NativeRequestHandoff, NativeRequestPhase, SharedMessageCandidate,
            SharedMessageProof,
        },
    };
    let declared_context = facts.context.clone();
    let request = EventKey {
        source_instance: facts.event.source_instance,
        sequence_domain: 2,
        sequence: facts
            .event
            .sequence
            .checked_mul(2)
            .ok_or(StoreError::Invalid)?,
    };
    let actor = NativeSyncThread {
        tgid: facts.context.process.host_tgid,
        tid: facts.context.process.host_tgid,
        birth_boottime_ns: facts.context.process.birth_boottime_ns,
    };
    let submission = NativeRequestHandoff {
        observation: request,
        request: None,
        operation: Some(NativeRequestOperation::FileOpen),
        actor: actor.clone(),
        observed_boottime_ns: 0,
        phase: NativeRequestPhase::Submission {
            site: NativeApiSite {
                process: facts.context.process,
                exec_cookie: 0,
                instruction_pointer: 0,
                stack_pointer: 0,
                link_register: None,
                frame_pointer: None,
            },
            call: NativeLibraryCall::FileOpen {
                request_address: 0,
                loop_address: 0,
                path_address: 0,
                path: bytes::Bytes::new(),
                path_status: NativeCaptureStatus::Complete,
                flags: 0,
                mode: 0,
                callback_address: 0,
            },
            projection: Some(bytes::Bytes::new()),
            analysis: Some(CaptureAnalysis {
                status: 0,
                candidates: vec![SharedMessageCandidate {
                    context: facts.context,
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
    };
    let bound = NativeRequestHandoff {
        observation: EventKey {
            sequence: request.sequence.checked_add(1).ok_or(StoreError::Invalid)?,
            ..request
        },
        request: Some(request),
        operation: Some(NativeRequestOperation::FileOpen),
        actor,
        observed_boottime_ns: 0,
        phase: NativeRequestPhase::EventBound {
            event: facts.event,
            binding: NativeEventBinding::DirectCheck,
            enclosing_call: None,
        },
    };
    let origin_receipt = log.record_request_handoff(submission).await?;
    log.record_request_handoff(bound).await?;
    // This is an explicit controlled-source assertion by the test, not inference
    // from the captured pair, its count, actor, or native operation.
    log.call(move |conn| {
        let tx = conn.transaction()?;
        let origin: i64 = tx.query_row(
            "SELECT id FROM native_request_handoffs WHERE delivery=?1",
            [origin_receipt.record],
            |row| row.get(0),
        )?;
        super::native_execution::link_capture(&tx, facts.event, origin, 0, &declared_context)?;
        tx.commit()?;
        Ok(())
    })?
    .await?;
    Ok(())
}

#[tokio::test]
async fn activity_context_attaches_original_request_text_for_a_confirmed_call()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let mut fixture = Fixture::new();
    let call = fixture.call;
    let exchange = call.message.exchange;
    let source = Arc::make_mut(&mut fixture.protocol.source);
    source.channel = Some(ChannelRef {
        kind: ChannelKind::Socket,
        source_instance: [8; 16],
        object_id: 5,
        generation: 1,
    });
    source.actual_auth_scope = Some([2; 32]);
    source.credential_use = Some(CredentialUseId {
        exchange,
        sequence: 0,
    });
    source.observed_at_utc_ms = Some(20);
    fixture.message.source = fixture.protocol.source.clone();
    let mut request_source = fixture.message.source.as_ref().clone();
    request_source.side = ProtocolSide::ClientRequest;
    request_source.observed_at_utc_ms = Some(10);
    let request_source = Arc::new(request_source);
    let request_id = ProtocolRecordId {
        exchange,
        sequence: 0,
    };
    let original_text = "  Check the settings.\n";
    let request = ProtocolObservation {
        id: request_id,
        source: request_source.clone(),
        unit: ProtocolUnit::HttpJsonBody,
        event_name: None,
        body: EvidenceBody::from_bytes(
            br#"{"input":[{"role":"user","content":"  Check the settings.\n"}]}"#.to_vec(),
        ),
        native_ids: vec![],
        quality: vec![],
        request_session: None,
    };
    let mut user = fixture.message.clone();
    user.id.sequence = 0;
    user.source = request_source;
    user.record_refs = vec![request_id];
    user.origin = ChatOrigin::RequestContext;
    user.role = ChatRole::User;
    user.observed_parts = vec![ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: original_text.into(),
    }];
    let mut foreign = user.clone();
    foreign.id.exchange.sequence += 1;
    let mut later_revision = user.clone();
    later_revision.revision += 1;
    later_revision.observed_parts[0] = ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: "A later revision.".into(),
    };
    let event = EventKey::from(fixture.completion.event.as_ref());
    let native = native_tool_facts(&fixture);
    let completion = Arc::new(Up::OperationResult(Arc::new(fixture.completion)));
    let anchor = FactReference::Event(event);
    let input = |view: &ContextView| {
        view.source_records
            .iter()
            .filter_map(|record| match record {
                SourceRecord::Message(message) if message.role == ChatRole::User => {
                    Some(message.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    for (name, failure) in [
        ("valid", None),
        ("missing", Some(false)),
        ("conflict", Some(true)),
    ] {
        let log = BusinessLog::open_path(&dir.path().join(format!("{name}.sqlite")))?;
        record_tool_fixture(&log, native.clone()).await?;
        log.record_up(completion.clone())?.await?;
        chat(&log, 1, ChatEvent::Observation(Arc::new(request.clone()))).await?;
        chat(&log, 2, ChatEvent::Finished(Arc::new(user.clone()))).await?;
        chat(&log, 3, ChatEvent::Finished(Arc::new(foreign.clone()))).await?;
        let expected_gap = if let Some(conflict) = failure {
            let mut newer = user.clone();
            newer.id.sequence = 1;
            let gap = if conflict {
                Arc::make_mut(&mut newer.source).credential_use = Some(CredentialUseId {
                    exchange,
                    sequence: 1,
                });
                ContextGap {
                    at: FactReference::Message(newer.id),
                    reason: ContextGapReason::ConflictingEvidence,
                }
            } else {
                newer.record_refs[0].sequence = 55;
                ContextGap {
                    at: FactReference::Protocol(newer.record_refs[0]),
                    reason: ContextGapReason::SourceUnavailable,
                }
            };
            chat(&log, 4, ChatEvent::Finished(Arc::new(newer))).await?;
            Some(gap)
        } else {
            None
        };
        chat(
            &log,
            5,
            ChatEvent::Observation(Arc::new(fixture.protocol.clone())),
        )
        .await?;
        chat(
            &log,
            6,
            ChatEvent::Finished(Arc::new(fixture.message.clone())),
        )
        .await?;
        chat(
            &log,
            7,
            ChatEvent::Finished(Arc::new(later_revision.clone())),
        )
        .await?;

        let (before, _) = log.activity_context(anchor.clone(), None).await?;
        assert!(input(&before).is_empty());
        log.bind_tool_execution(call, event).await?;
        let (after, _) = log.activity_context(anchor.clone(), None).await?;
        assert_eq!(crate::activity::execution_call(&after, event), Ok(call));
        if let Some(gap) = expected_gap {
            assert!(input(&after).is_empty());
            assert!(after.gaps.contains(&gap));
        } else {
            assert_eq!(input(&after), vec![Arc::new(user.clone())]);
            let mut conflict = native.clone();
            conflict.context.response_id = Some("different-original-response".into());
            record_tool_fixture(&log, conflict).await?;
            let (conflicted, _) = log.activity_context(anchor.clone(), None).await?;
            assert_eq!(
                crate::activity::execution_call(&conflicted, event)
                    .unwrap_err()
                    .reason,
                ContextGapReason::ConflictingEvidence,
            );
            assert!(input(&conflicted).is_empty());
        }
        log.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn group_receipts_follow_original_request_positions_and_page_order() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("group.sqlite"))?;
    let mut observation = Fixture::new().protocol;
    let source = Arc::make_mut(&mut observation.source);
    source.side = ProtocolSide::ClientRequest;
    source.profile = ProviderProfile::OpenAiResponses;
    let actor = as_types::process_tree::ProcessKey {
        boot_id: [1; 16],
        host_tgid: 77,
        birth_boottime_ns: 12,
    };
    source.actor.process = Some(actor);
    observation.body = EvidenceBody::from_bytes(
        br#"{"input":[
            {"type":"function_call","name":"sessions_spawn","call_id":"call-A"},
            {"type":"function_call_output","call_id":"call-A","output":"{\"status\":\"accepted\",\"runId\":\"run-B\",\"childSessionKey\":\"session-C\"}"}
        ]}"#.to_vec(),
    );
    observation.native_ids = vec![
        NativeProtocolId {
            namespace: NativeIdNamespace::ToolCall,
            field_path: "/input/0/call_id".into(),
            value: NativeIdValue::JsonString("call-A".into()),
        },
        NativeProtocolId {
            namespace: NativeIdNamespace::ToolResultReference,
            field_path: "/input/1/call_id".into(),
            value: NativeIdValue::JsonString("call-A".into()),
        },
    ];
    let first = observation.id;
    chat(&log, 1, ChatEvent::Observation(observation.clone().into())).await?;
    observation.id.exchange.sequence += 1;
    chat(&log, 2, ChatEvent::Observation(observation.clone().into())).await?;
    observation.id.exchange.sequence += 1;
    observation
        .native_ids
        .push(observation.native_ids[1].clone());
    chat(&log, 3, ChatEvent::Observation(observation.into())).await?;
    let latest = log.group_delegations(None, 1).await?;
    let page = serde_json::to_value(&latest)?;
    let entry = &page["records"][0];
    assert_eq!(entry["kind"], "openclaw_tool_receipt");
    assert_eq!(entry["actor"], serde_json::to_value(actor)?);
    assert_eq!(entry["tool_call_id"], "call-A");
    assert_eq!(entry["child_run_id"], "run-B");
    assert_eq!(entry["child_session_key"], "session-C");
    assert_eq!(entry["call_index"], 0);
    assert_eq!(entry["output_index"], 1);
    assert_eq!(
        entry["gaps"],
        serde_json::json!([
            "gateway_receipt_unobserved",
            "child_terminal_unobserved",
            "parent_publication_unobserved"
        ])
    );
    assert_eq!(latest.next_before, Some(2));
    let older = log.group_delegations(latest.next_before, 1).await?;
    let page = serde_json::to_value(&older)?;
    assert_eq!(
        page["records"][0]["receipt"],
        serde_json::to_value(FactReference::Protocol(first))?
    );
    assert_eq!(older.next_before, None);
    Ok(())
}

#[test]
fn traceparent_and_response_must_share_the_original_exchange() -> Result<(), StoreError> {
    let db = Connection::open_in_memory()?;
    schema::initialize(&db)?;
    let fixture = Fixture::new();
    let process = as_types::process_tree::ProcessKey {
        boot_id: [1; 16],
        host_tgid: 123,
        birth_boottime_ns: 45,
    };
    let mut source = fixture.message.source.as_ref().clone();
    source.actor.process = Some(process);
    source.channel = Some(ChannelRef {
        kind: ChannelKind::Socket,
        source_instance: [3; 16],
        object_id: 2,
        generation: 1,
    });
    source.actual_auth_scope = Some([5; 32]);
    let trace = [
        "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
        "00-0123456789abcdef0123456789abcdef-1123456789abcdef-01",
    ];
    for (i, traceparent) in trace.iter().enumerate() {
        let exchange = ExchangeKey {
            flow_instance: [7; 16],
            sequence: i as u64 + 1,
        };
        let mut request = source.clone();
        request.side = ProtocolSide::ClientRequest;
        request.request_traceparent = Some((*traceparent).to_owned());
        chat::write_protocol(
            &db,
            &ProtocolObservation {
                id: ProtocolRecordId {
                    exchange,
                    sequence: 1,
                },
                source: Arc::new(request),
                unit: ProtocolUnit::HttpJsonBody,
                event_name: None,
                body: EvidenceBody::from_bytes(b"{}".to_vec()),
                native_ids: Vec::new(),
                quality: Vec::new(),
                request_session: None,
            },
        )?;
        chat::write_protocol(
            &db,
            &ProtocolObservation {
                id: ProtocolRecordId {
                    exchange,
                    sequence: 2,
                },
                source: Arc::new(source.clone()),
                unit: ProtocolUnit::HttpJsonBody,
                event_name: None,
                body: EvidenceBody::from_bytes(b"{}".to_vec()),
                native_ids: vec![NativeProtocolId {
                    namespace: NativeIdNamespace::Response,
                    field_path: "/id".into(),
                    value: NativeIdValue::JsonString(format!("response-{i}")),
                }],
                quality: Vec::new(),
                request_session: None,
            },
        )?;
    }
    let query = |traceparent: &str, response: &str| {
        let (trace, span) = traceparent_ids(traceparent).expect("original tracing header");
        query::traced_response_references(&db, process, &source.origin, trace, span, response)
    };
    assert_eq!(query(trace[0], "response-0")?[0].exchange.sequence, 1);
    assert_eq!(query(trace[1], "response-1")?[0].exchange.sequence, 2);
    assert!(query(trace[0], "response-1")?.is_empty());
    assert!(query(trace[1], "response-0")?.is_empty());
    Ok(())
}

#[tokio::test]
async fn tool_attribution_keeps_parallel_calls_and_checks_the_retained_scope()
-> Result<(), StoreError> {
    use crate::attribution::{
        ActorScope, AttributionPattern, AttributionRequest, TargetComparison, TargetPattern,
    };
    use as_types::process_tree::ProcessKey;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("attribution.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let f = Fixture::new();
    let mut original = serde_json::to_value(f.completion.event.as_ref())?;
    original["header"]["stage"] = serde_json::json!("controlled_request");
    original["header"]["source_boottime_ns"] = row::number(2_000);
    original["header"]["actor_tgid"] = serde_json::json!(321);
    original["header"]["actor_tid"] = serde_json::json!(323);
    original["header"]["actor_birth_boottime_ns"] = row::number(70);
    original["header"]["actor_thread_birth_boottime_ns"] = row::number(80);
    original["payload"]["path"] = serde_json::json!(b"/audit/file");
    let mut event: as_types::dispatch_event::EventData = serde_json::from_value(original)?;
    assert!(event.bind_host_boot([10; 16]));
    assert!(!event.bind_host_boot([11; 16]));
    let event = Arc::new(event);
    let event_key = EventKey::from(event.as_ref());
    log.record_up(Arc::new(Up::Ask {
        decision_id: DecisionId([7; 16]),
        event: event.clone(),
        hit: f.completion.hit.clone().ok_or(StoreError::Invalid)?,
        timeout: Decision::Allow,
        remaining_ms: 100,
        choices: vec![Decision::Allow],
    }))?
    .await?;

    for (sequence, boot, time, thread_birth, target) in [
        (1, 10, 1_900, 80, "/audit/file"),
        (2, 10, 1_901, 80, "/audit/file"),
        (3, 11, 1_901, 80, "/audit/file"),
        (4, 10, 1_899, 80, "/audit/file"),
        (5, 10, 1_901, 81, "/audit/file"),
        (6, 10, 1_901, 80, "/audit/other"),
        (7, 10, 2_001, 80, "/audit/file"),
    ] {
        let mut message = f.message.clone();
        message.id.exchange.sequence = sequence;
        message.first_seen_boottime_ns = time;
        message.last_seen_boottime_ns = time;
        message.record_refs.clear();
        let actor = &mut Arc::make_mut(&mut message.source).actor;
        actor.process = Some(ProcessKey {
            boot_id: [boot; 16],
            host_tgid: 321,
            birth_boottime_ns: 70,
        });
        actor.tid = Some(323);
        actor.thread_birth_boottime_ns = Some(thread_birth);
        let ChatPart::ToolCall { arguments, .. } = &mut message.observed_parts[0] else {
            return Err(StoreError::Invalid);
        };
        *arguments = Some(ChatContent::Json(serde_json::json!({"path":target})));
        chat(&log, sequence, ChatEvent::Finished(Arc::new(message))).await?;
    }
    log.close().await?;
    let log = BusinessLog::open_path(&path)?;
    let pattern = || AttributionPattern {
        actor: ActorScope::SameThread,
        lookback_ns: 100,
        target: TargetPattern::FilePath {
            pointer: "/path".into(),
        },
    };
    let first = log.tool_attribution(event_key, pattern(), None, 1).await?;
    assert_eq!(first.candidates.len(), 1);
    assert_eq!(first.candidates[0].call.message.exchange.sequence, 1);
    assert_eq!(
        first.candidates[0].target_comparison,
        TargetComparison::Equal
    );
    assert_eq!(first.next_after, Some(first.candidates[0].call.message));
    let rest = log
        .tool_attribution(event_key, pattern(), first.next_after, 64)
        .await?;
    assert_eq!(
        rest.candidates
            .iter()
            .map(|c| c.call.message.exchange.sequence)
            .collect::<Vec<_>>(),
        vec![2, 6]
    );
    assert_eq!(
        rest.candidates[0].target_comparison,
        TargetComparison::Equal
    );
    assert_eq!(
        rest.candidates[1].target_comparison,
        TargetComparison::Different
    );
    assert!(rest.next_after.is_none());
    let ask = log
        .read_ask_context(
            DecisionId([7; 16]),
            Some(AttributionRequest {
                pattern: pattern(),
                after: None,
                limit: 64,
            }),
        )
        .await?;
    assert!(ask.call.is_err());
    assert!(ask.window.is_none());
    let context = ask.attribution.ok_or(StoreError::NotProvided)??;
    assert_eq!(context.matches.candidates.len(), 3);
    assert_eq!(context.windows.len(), 3);
    for (index, (window, messages)) in context.windows.iter().enumerate() {
        assert_eq!(window.exchange.sequence, [1, 2, 6][index]);
        assert_eq!(
            window.message_refs,
            messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].revision,
            context.matches.candidates[index].message_revision
        );
        assert_eq!(
            messages[0].observed_parts,
            log.model_messages(messages[0].id.exchange, None, 1)
                .await?
                .0[0]
                .observed_parts
        );
    }
    let mut absent_thread = serde_json::to_value(event.as_ref())?;
    absent_thread["header"]["stage"] = serde_json::json!("controlled_request");
    absent_thread["header"]["sequence"] = row::number(300);
    absent_thread["header"]["actor_thread_birth_boottime_ns"] = serde_json::Value::Null;
    let absent_thread = Arc::new(serde_json::from_value(absent_thread)?);
    log.record_up(Arc::new(Up::Ask {
        decision_id: DecisionId([8; 16]),
        event: absent_thread,
        hit: f.completion.hit.ok_or(StoreError::Invalid)?,
        timeout: Decision::Allow,
        remaining_ms: 100,
        choices: vec![Decision::Allow],
    }))?
    .await?;
    let missing = log
        .read_ask_context(
            DecisionId([8; 16]),
            Some(AttributionRequest {
                pattern: pattern(),
                after: None,
                limit: 64,
            }),
        )
        .await?;
    assert!(matches!(
        missing.attribution,
        Some(Err(StoreError::NotProvided))
    ));
    assert!(
        matches!(missing.ask.as_ref(), Up::Ask { decision_id, .. } if *decision_id == DecisionId([8; 16]))
    );
    let view = log.activity(FactReference::Event(event_key)).await?;
    assert!(
        view.relations
            .iter()
            .all(|r| r.kind != RelationKind::ToolExecution)
    );
    Ok(())
}

#[tokio::test]
async fn chat_history_pages_follow_commits_and_open_original_tool_evidence()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("history.sqlite"))?;
    let f = Fixture::new();
    let first = f.message.id.exchange;
    let second = ExchangeKey {
        flow_instance: [42; 16],
        sequence: 0,
    };
    let mut same_exchange = f.message.clone();
    same_exchange.id.sequence = 0;
    let mut result_record = f.protocol.clone();
    result_record.id.sequence = 2;
    result_record.native_ids[0].namespace = NativeIdNamespace::ToolResultReference;
    let result_body =
        br#"{"output":[{"type":"function_call_output","call_id":"original-call","output":"ok"}]}"#;
    result_record.body = EvidenceBody {
        original_length: Some(result_body.len() as u64),
        segments: vec![EvidenceSegment::Bytes {
            offset: 0,
            data: result_body.to_vec(),
        }],
    };
    same_exchange.record_refs = vec![result_record.id];
    same_exchange.observed_parts = vec![ChatPart::ToolResult {
        id: 0,
        native_id: Some(result_record.native_ids[0].clone()),
        content: ChatContent::Text("ok".into()),
    }];
    let mut foreign_record = result_record.clone();
    foreign_record.id.exchange.sequence += 1;
    let mut second_message = f.message.clone();
    second_message.id.exchange = second;
    let inputs = [
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
        ChatEvent::Finished(Arc::new(f.message.clone())),
        ChatEvent::Observation(Arc::new(result_record)),
        ChatEvent::Finished(Arc::new(same_exchange.clone())),
        ChatEvent::Finished(Arc::new(second_message)),
    ];
    for (index, event) in inputs.into_iter().enumerate() {
        log.record_chat(Arc::new(ChatEnvelope {
            cursor: ChatCursor {
                service_instance: [4; 16],
                sequence: index as u64,
            },
            event: Arc::new(event),
        }))
        .await?;
    }
    assert_eq!(
        log.chat_exchanges(None, None, 1).await?,
        (vec![second], Some(second))
    );
    assert_eq!(
        log.chat_exchanges(None, Some(second), 1).await?,
        (vec![first], None)
    );
    let (page, next) = log.model_messages(first, None, 1).await?;
    assert_eq!(page[0].id, f.message.id);
    assert_eq!(next, Some(f.message.id));
    let (page, next) = log.model_messages(first, next, 1).await?;
    assert_eq!(page[0].id, same_exchange.id);
    assert_eq!(next, None);
    assert!(matches!(
        log.model_messages(second, Some(f.message.id), 1).await,
        Err(StoreError::Invalid)
    ));
    assert!(matches!(
        log.chat_exchanges(None, None, 0).await,
        Err(StoreError::Invalid)
    ));
    let tool = FactReference::Part(f.call);
    let result = FactReference::Part(ChatPartRef {
        message: same_exchange.id,
        part_id: 0,
    });
    let view = log.activity(tool.clone()).await?;
    assert!(
        view.relations
            .iter()
            .any(|link| link.kind == RelationKind::SameExchange
                && link.from == FactReference::Protocol(f.protocol.id)
                && link.to == FactReference::Message(f.message.id))
    );
    assert!(
        view.relations
            .iter()
            .any(|link| link.kind == RelationKind::NativeToolReference
                && link.from == tool
                && link.to == result)
    );
    assert!(view.gaps.contains(&ContextGap {
        at: tool,
        reason: ContextGapReason::NoExplicitRelation
    }));
    let mut foreign_result = same_exchange.clone();
    foreign_result.id.sequence = 98;
    foreign_result.record_refs = vec![foreign_record.id];
    let foreign_message = FactReference::Message(foreign_result.id);
    let foreign_part = FactReference::Part(ChatPartRef {
        message: foreign_result.id,
        part_id: 0,
    });
    chat(&log, 0, ChatEvent::Observation(Arc::new(foreign_record))).await?;
    chat(&log, 1, ChatEvent::Finished(Arc::new(foreign_result))).await?;
    let view = log.activity(foreign_part.clone()).await?;
    assert!(view.gaps.contains(&ContextGap {
        at: foreign_message,
        reason: ContextGapReason::ConflictingEvidence,
    }));
    assert!(
        view.relations.iter().all(|link| {
            link.kind != RelationKind::NativeToolReference || link.to != foreign_part
        }),
        "a protocol record in another HTTP exchange cannot prove this tool result"
    );
    let persisted = log.call(move |conn| {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM relations WHERE kind='native_tool_reference' AND to_ref=?1)",
            [super::key(&foreign_part)?],
            |row| row.get::<_, bool>(0),
        ).map_err(StoreError::from)
    })?.await?;
    assert!(
        !persisted,
        "foreign-exchange proof must not produce a stored edge"
    );
    let mut competing_call = f.message.clone();
    competing_call.id.sequence = 99;
    log.record_chat(Arc::new(ChatEnvelope {
        cursor: ChatCursor {
            service_instance: [4; 16],
            sequence: 5,
        },
        event: Arc::new(ChatEvent::Finished(Arc::new(competing_call))),
    }))
    .await?;
    let view = log.activity(result.clone()).await?;
    assert!(view.gaps.contains(&ContextGap {
        at: result,
        reason: ContextGapReason::ConflictingEvidence
    }));
    assert!(
        view.relations
            .iter()
            .all(|link| link.kind != RelationKind::NativeToolReference)
    );
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn sql_chat_inputs_preserve_all_variants_exact_revisions_and_commit_order()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("stream.sqlite");
    let log = BusinessLog::open_path(&path)?;
    assert_eq!(log.chat_tail().await?, None);
    let f = Fixture::new();
    let mut started = f.message.clone();
    started.revision = 1;
    started.state = ChatState::Streaming;
    started.model = Some(String::new());
    started.observed_parts.push(ChatPart::Text {
        id: 33,
        kind: TextKind::Plain,
        text: String::new(),
    });
    let mut finished = started.clone();
    finished.revision = 3;
    finished.state = ChatState::Complete;
    if let Some(part) = finished.observed_parts.last_mut() {
        *part = ChatPart::Text {
            id: 33,
            kind: TextKind::Plain,
            text: "terminal".into(),
        };
    }
    let denial = EventKey::from(f.completion.event.as_ref());
    let original = f.protocol.id;
    let fact = |state| RuleFeedbackFact {
        denial,
        call: f.call,
        state,
    };
    let values = vec![
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
        ChatEvent::Started(Arc::new(started)),
        ChatEvent::Delta {
            id: f.message.id,
            base_revision: 1,
            revision: 2,
            changes: vec![
                ChatDelta::AppendText {
                    part_id: 33,
                    text: "delta".into(),
                },
                ChatDelta::PutPart {
                    part: ChatPart::Text {
                        id: 34,
                        kind: TextKind::Refusal,
                        text: String::new(),
                    },
                },
            ],
        },
        ChatEvent::Finished(Arc::new(finished)),
        ChatEvent::CredentialUse(Arc::new(CredentialUseFact {
            id: CredentialUseId {
                exchange: f.message.id.exchange,
                sequence: u64::MAX,
            },
            revision: u64::MAX,
            actor: f.message.source.actor.clone(),
            binding_ref: None,
            destination_origin: String::new(),
            state: CredentialUseState::Prepared,
            auth_field: Some(String::new()),
            reason: None,
            observed_boottime_ns: 0,
        })),
        ChatEvent::RuleFeedback(fact(RuleFeedbackState::Prepared)),
        ChatEvent::RuleFeedback(fact(RuleFeedbackState::Applied {
            original,
            modified_body: f.protocol.body.clone().into(),
        })),
        ChatEvent::RuleFeedback(fact(RuleFeedbackState::Submitted { original })),
        ChatEvent::ExchangeFailed {
            exchange: f.message.id.exchange,
            http_status: Some(403),
            reason: "upstream".into(),
        },
        ChatEvent::Gap(ObservationGap {
            kind: String::new(),
            exchange: None,
            cursor_range: Some((
                ChatCursor {
                    service_instance: [5; 16],
                    sequence: 0,
                },
                ChatCursor {
                    service_instance: [5; 16],
                    sequence: u64::MAX,
                },
            )),
            reason: String::new(),
        }),
        ChatEvent::Status(FlowStatus {
            intake_state: FlowIntakeState::SourceClosed,
            mode: FlowMode::MediateTls,
            decoder_profiles: vec![
                ProviderProfile::OpenAiResponses,
                ProviderProfile::OpenAiResponses,
            ],
            active_exchanges: 0,
            lost_deliveries: u64::MAX,
            parse_failures: 0,
        }),
    ];
    let expected = values
        .into_iter()
        .enumerate()
        .map(|(n, event)| {
            Arc::new(ChatEnvelope {
                cursor: ChatCursor {
                    service_instance: [9; 16],
                    sequence: u64::MAX - n as u64,
                },
                event: Arc::new(event),
            })
        })
        .collect::<Vec<_>>();
    let changed = log.chat_changed();
    for value in &expected {
        log.record_chat(value.clone()).await?;
    }
    tokio::time::timeout(std::time::Duration::from_secs(1), changed)
        .await
        .expect("commit is observed even before the waiter is polled");
    assert_eq!(
        log.chat_tail().await?,
        expected.last().map(|e| e.cursor),
        "tail follows committed input order, not greatest source number"
    );
    let mut after = None;
    for original in &expected {
        let actual = log
            .read_next_chat(after)
            .await?
            .expect("one committed original input");
        assert_eq!(
            serde_json::to_value(actual.as_ref())?,
            serde_json::to_value(original.as_ref())?
        );
        log.validate_chat_cursor(actual.cursor).await?;
        after = Some(actual.cursor);
    }
    assert!(log.read_next_chat(after).await?.is_none());
    let unknown = ChatCursor {
        service_instance: [250; 16],
        sequence: 0,
    };
    assert!(matches!(
        log.validate_chat_cursor(unknown).await,
        Err(StoreError::NotProvided)
    ));
    assert!(matches!(
        log.read_next_chat(Some(unknown)).await,
        Err(StoreError::NotProvided)
    ));
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    assert_eq!(
        serde_json::to_value(
            reopened
                .read_next_chat(None)
                .await?
                .expect("durable input")
                .as_ref()
        )?,
        serde_json::to_value(expected[0].as_ref())?
    );
    reopened.close().await?;
    let old = rusqlite::Connection::open(&path)?;
    old.execute("DROP TABLE chat_inputs", [])?;
    drop(old);
    assert!(
        matches!(BusinessLog::open_path(&path), Err(StoreError::Invalid)),
        "no inference or empty-index migration for an old database"
    );
    Ok(())
}
fn question(f: &Fixture) -> Arc<Up> {
    Arc::new(Up::Ask {
        decision_id: DecisionId([7; 16]),
        event: f.completion.event.clone(),
        hit: f.completion.hit.clone().expect("hit"),
        choices: vec![
            Decision::Allow,
            Decision::Deny {
                terminate: Termination::None,
            },
        ],
        timeout: Decision::Deny {
            terminate: Termination::ActorAndParent,
        },
        remaining_ms: 30000,
    })
}

#[tokio::test]
async fn ask_timeout_round_trips_and_a_conflicting_replay_cannot_replace_it()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("ask-timeout.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let original = question(&Fixture::new());
    let Up::Ask {
        decision_id,
        event,
        hit,
        choices,
        remaining_ms,
        ..
    } = original.as_ref()
    else {
        return Err(StoreError::Invalid);
    };
    let id = *decision_id;
    log.record_up(original.clone())?.await?;
    log.record_up(original.clone())?.await?;
    let conflict = Arc::new(Up::Ask {
        decision_id: id,
        event: event.clone(),
        hit: hit.clone(),
        choices: choices.clone(),
        remaining_ms: *remaining_ms,
        timeout: Decision::Allow,
    });
    assert!(matches!(
        log.record_up(conflict)?.await,
        Err(StoreError::Conflict)
    ));
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    assert_eq!(
        serde_json::to_value(reopened.question(id).await?.as_ref())?,
        serde_json::to_value(original.as_ref())?
    );
    reopened.close().await?;
    let old = Connection::open(&path)?;
    old.pragma_update(None, "user_version", 1)?;
    drop(old);
    assert!(matches!(
        BusinessLog::open_path(&path),
        Err(StoreError::Invalid)
    ));
    Ok(())
}
#[tokio::test]
async fn file_hook_flags_and_mode_round_trip_without_null_or_zero_inference()
-> Result<(), StoreError> {
    use as_types::{
        dispatch_event::{EventData, FileData},
        event_operation::FileOperation,
    };
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("file-facts");
    let log = BusinessLog::open_path(&path)?;
    let fixture = Fixture::new();
    let mut originals = Vec::new();
    for (n, operation, rename_flags, creation_mode) in [
        (1u8, FileOperation::Rename, Some(0), None),
        (2, FileOperation::Rename, Some(2), None),
        (3, FileOperation::CreateDirectory, None, Some(0)),
        (4, FileOperation::CreateDirectory, None, Some(0o1755)),
        (5, FileOperation::Open, None, None),
        (6, FileOperation::MapSharedWritable, None, None),
        (7, FileOperation::TruncateOpenFile, None, None),
    ] {
        let mapped = operation == FileOperation::MapSharedWritable;
        let truncated = operation == FileOperation::TruncateOpenFile;
        let data = FileData {
            path: Some(b"/workspace/original".to_vec()),
            new_path: (operation == FileOperation::Rename)
                .then(|| b"/workspace/destination".to_vec()),
            device: Some((0, 0)),
            inode: (operation != FileOperation::CreateDirectory).then_some(u64::MAX),
            rename_flags,
            creation_mode,
            map_requested_prot: mapped.then_some(3),
            map_effective_prot: mapped.then_some(3),
            map_flags: mapped.then_some((1u64 << 63) | 1),
            truncate_size: truncated.then_some(0),
            truncate_attributes: truncated.then_some(0x2008),
            ..Default::default()
        };
        let mut value = serde_json::to_value(fixture.completion.event.as_ref())?;
        value["header"]["kind"] = serde_json::json!("file");
        value["header"]["operation"] = serde_json::json!(operation as u32);
        value["header"]["sequence"] = serde_json::json!(u64::from(n).to_string());
        if operation == FileOperation::Open {
            value["header"]["actor_cgroup_id"] = serde_json::json!(u64::MAX.to_string());
            value["header"]["actor_cgroup_lineage"] =
                serde_json::json!(["1", u64::MAX.to_string()]);
        }
        value["payload"] = serde_json::to_value(data)?;
        let event: EventData = serde_json::from_value(value)?;
        let original = Arc::new(Up::Ask {
            decision_id: DecisionId([n; 16]),
            event: Arc::new(event),
            hit: fixture.completion.hit.clone().ok_or(StoreError::Invalid)?,
            choices: vec![
                Decision::Allow,
                Decision::Deny {
                    terminate: Termination::None,
                },
            ],
            timeout: Decision::Allow,
            remaining_ms: 1000,
        });
        log.record_up(original.clone())?.await?;
        originals.push((DecisionId([n; 16]), original));
    }
    log.close().await?;
    let log = BusinessLog::open_path(&path)?;
    for (id, original) in originals {
        assert_eq!(
            serde_json::to_value(log.question(id).await?.as_ref())?,
            serde_json::to_value(original.as_ref())?
        );
    }
    log.close().await?;
    let old = rusqlite::Connection::open(&path)?;
    old.execute("ALTER TABLE file_payloads DROP COLUMN creation_mode", [])?;
    drop(old);
    assert!(matches!(
        BusinessLog::open_path(&path),
        Err(StoreError::Invalid)
    ));
    Ok(())
}

#[tokio::test]
async fn execution_snapshots_survive_question_and_completion_reopen() -> Result<(), StoreError> {
    use as_types::dispatch_event::{
        EventData, KERNEL_STACK_LIMIT, StackSnapshot, SyscallSnapshot, UserStackSnapshot,
    };

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("execution-facts");
    let log = BusinessLog::open_path(&path)?;
    let fixture = Fixture::new();
    let large = (1u64 << 53) + 1;
    let syscall = SyscallSnapshot {
        audit_arch: 0xc00000b7,
        number: -1,
        instruction_pointer: large,
        stack_pointer: u64::MAX,
        arguments: vec![0, large, u64::MAX, 7, 0, large],
    };
    let empty_stack = StackSnapshot {
        instruction_pointers: Vec::new(),
        buffer_full: false,
    };
    let pointers = [large, 0, large, u64::MAX];
    let full_stack = StackSnapshot {
        instruction_pointers: pointers
            .into_iter()
            .cycle()
            .take(KERNEL_STACK_LIMIT)
            .collect(),
        buffer_full: true,
    };
    let empty_user_stack = UserStackSnapshot {
        result: 0,
        snapshot: empty_stack.clone(),
    };
    let failed_user_stack = UserStackSnapshot {
        result: -14,
        snapshot: empty_stack.clone(),
    };
    let captured_user_stack = UserStackSnapshot {
        result: (pointers.len() * size_of::<u64>()) as i32,
        snapshot: StackSnapshot {
            instruction_pointers: pointers.to_vec(),
            buffer_full: false,
        },
    };
    let mut originals = Vec::new();
    for (sequence, syscall, kernel_stack, user_stack) in [
        (1u8, None, None, None),
        (2, Some(syscall), Some(empty_stack), Some(empty_user_stack)),
        (3, None, Some(full_stack), Some(failed_user_stack)),
        (4, None, None, Some(captured_user_stack)),
    ] {
        let mut value = serde_json::to_value(fixture.completion.event.as_ref())?;
        value["header"]["stage"] = serde_json::json!("controlled_request");
        value["header"]["sequence"] = row::number(u64::from(sequence));
        value["header"]["actor_tgid"] = serde_json::json!(180);
        value["header"]["actor_tid"] = serde_json::json!(4196);
        value["process"]["namespace_tgids"] = serde_json::json!([180, 42, 1]);
        let namespace_tids = (sequence == 2).then_some([4196u32, 900, 1]);
        value["header"]["actor_namespace_tids"] = serde_json::json!(namespace_tids);
        value["syscall"] = serde_json::to_value(syscall)?;
        value["kernel_stack"] = serde_json::to_value(kernel_stack)?;
        value["user_stack"] = serde_json::to_value(&user_stack)?;
        let event = Arc::new(serde_json::from_value::<EventData>(value)?);
        assert_eq!(event.user_stack, user_stack);
        assert_eq!(
            event.header().actor_namespace_tids.as_deref(),
            namespace_tids.as_ref().map(|tids| tids.as_slice())
        );
        let decision_id = DecisionId([sequence; 16]);
        log.record_up(Arc::new(Up::Ask {
            decision_id,
            event: event.clone(),
            hit: fixture.completion.hit.clone().ok_or(StoreError::Invalid)?,
            choices: vec![Decision::Allow],
            timeout: Decision::Allow,
            remaining_ms: 1000,
        }))?
        .await?;

        let mut completion = fixture.completion.clone();
        completion.decision_id = Some(decision_id);
        completion.event = event.clone();
        log.record_up(Arc::new(Up::OperationResult(Arc::new(completion))))?
            .await?;
        originals.push((decision_id, event));
    }
    log.close().await?;

    let log = BusinessLog::open_path(&path)?;
    for (decision_id, original) in originals {
        let ask = log.question(decision_id).await?;
        let Up::Ask { event, .. } = ask.as_ref() else {
            return Err(StoreError::Invalid);
        };
        assert_eq!(event, &original);
        let view = log
            .activity(FactReference::Completion(EventKey::from(original.as_ref())))
            .await?;
        assert!(view.source_records.iter().any(|record| matches!(
            record,
            SourceRecord::OperationResult(result) if result.event == original
        )));
    }
    log.close().await?;

    let old = rusqlite::Connection::open(&path)?;
    old.execute("DROP TABLE event_actor_namespace_tids", [])?;
    drop(old);
    assert!(matches!(
        BusinessLog::open_path(&path),
        Err(StoreError::Invalid)
    ));
    Ok(())
}

#[tokio::test]
async fn sqlite_reopen_preserves_unsigned_native_scalar_and_empty_values() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let mut f = Fixture::new();
    f.protocol.id.sequence = u64::MAX;
    f.protocol.id.exchange.sequence = u64::MAX;
    f.message.id.exchange = f.protocol.id.exchange;
    f.message.id.sequence = u64::MAX;
    f.call.message = f.message.id;
    f.message.revision = u64::MAX;
    f.message.model = Some(String::new());
    f.message.candidate_index = Some(0);
    f.message.usage = Some(vec![]);
    f.message.provider_status = Some(String::new());
    f.message.finish_reason = Some(String::new());
    f.message.first_seen_boottime_ns = 0;
    f.message.last_seen_boottime_ns = u64::MAX;
    let source = Arc::make_mut(&mut f.message.source);
    source.actor.observed_boottime_ns = u64::MAX;
    source.actor.info.name = Some(vec![]);
    source.actor.info.argv = Some(vec![vec![], b"x".to_vec()]);
    source.actor.info.parent_argv = Some(vec![]);
    source.actor.info.namespace_tgids = Some(vec![]);
    source.actor.info.uid = Some(0);
    source.actual_auth_scope = Some([0; 32]);
    let original = "Authorization: Bearer sk-fixture\\raw\"quoted\"\n密码=literal\0end";
    source.origin = "https://fixture.invalid/path?api_key=sk-fixture\\raw".into();
    source.route_label = original.into();
    f.message.role = ChatRole::Other(original.into());
    f.protocol.source = f.message.source.clone();
    f.protocol.event_name = Some(original.into());
    f.protocol.native_ids[0].value = NativeIdValue::JsonNumber(u64::MAX.to_string());
    let raw_native_id = NativeProtocolId {
        namespace: NativeIdNamespace::ToolCall,
        field_path: "/output/1/call_id".into(),
        value: NativeIdValue::JsonString(original.into()),
    };
    f.protocol.native_ids.push(raw_native_id.clone());
    f.message.record_refs = vec![f.protocol.id];
    f.message.observed_parts = vec![
        ChatPart::ToolCall {
            id: 0,
            native_id: Some(f.protocol.native_ids[0].clone()),
            name: String::new(),
            arguments: Some(ChatContent::Json(serde_json::Value::Null)),
        },
        ChatPart::ToolCall {
            id: 1,
            native_id: None,
            name: String::new(),
            arguments: None,
        },
        ChatPart::ToolResult {
            id: 2,
            native_id: None,
            content: ChatContent::Json(serde_json::json!([])),
        },
        ChatPart::Text {
            id: 3,
            kind: TextKind::Plain,
            text: String::new(),
        },
        ChatPart::Text {
            id: 4,
            kind: TextKind::Plain,
            text: original.into(),
        },
        ChatPart::ToolCall {
            id: 5,
            native_id: Some(raw_native_id),
            name: original.into(),
            arguments: Some(ChatContent::Json(serde_json::json!({
                "api_key": original,
                "literal_backslashes": r#"\u005c\n\""#,
                "empty": "", "array": [null, "", []], "zero": 0, "false": false,
                "large_integer": u64::MAX,
                "literal_number_object": {"$serde_json::private::Number": "1.0"},
                "literal_raw_object": {"$serde_json::private::RawValue": "false"},
            }))),
        },
        ChatPart::ToolResult {
            id: 6,
            native_id: None,
            content: ChatContent::Text(original.into()),
        },
        ChatPart::Opaque {
            id: 7,
            provider_type: original.into(),
            reason: original.into(),
        },
        ChatPart::MediaRef {
            id: 8,
            kind: original.into(),
            safe_label: original.into(),
        },
    ];
    f.protocol.body = EvidenceBody {
        original_length: Some(u64::MAX),
        segments: vec![
            EvidenceSegment::Bytes {
                offset: 0,
                data: vec![],
            },
            EvidenceSegment::Bytes {
                offset: 7,
                data: b"Authorization: Bearer sk-fixture\\raw\n\0\xff".to_vec(),
            },
            EvidenceSegment::Missing {
                offset: u64::MAX,
                length: None,
                reason: String::new(),
            },
        ],
    };
    chat(
        &log,
        0,
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
    )
    .await?;
    chat(
        &log,
        u64::MAX,
        ChatEvent::Finished(Arc::new(f.message.clone())),
    )
    .await?;
    log.close().await?;
    let log = BusinessLog::open_path(&path)?;
    let view = log.activity(FactReference::Message(f.message.id)).await?;
    assert!(
        view.source_records
            .iter()
            .any(|r| matches!(r,SourceRecord::Message(m)if **m==f.message))
    );
    assert!(
        view.source_records
            .iter()
            .any(|r| matches!(r,SourceRecord::Protocol(p)if **p==f.protocol))
    );
    let counts = log.counts().await?;
    assert_eq!(counts.messages, 1);
    assert_eq!(counts.protocol_records, 1);
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_transaction_rolls_back_and_reports_separately_from_delivery()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let f = Fixture::new();
    let mut changed = f.message.clone();
    changed.model = Some("conflicting same revision".into());
    chat(&log, 1, ChatEvent::Finished(Arc::new(f.message.clone()))).await?;
    assert!(matches!(
        chat(&log, 2, ChatEvent::Finished(Arc::new(changed))).await,
        Err(StoreError::Conflict)
    ));
    let counts = log.counts().await?;
    assert_eq!(counts.messages, 1);
    assert_eq!(counts.protocol_records, 0);
    assert_eq!(
        log.chat_tail().await?.map(|cursor| cursor.sequence),
        Some(1)
    );
    assert_eq!(log.write_failure_count(), 1);
    tokio::time::timeout(std::time::Duration::from_secs(1), log.write_failed())
        .await
        .expect("actual failure notification");
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn read_only_sql_failure_never_returns_a_stored_receipt() -> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let f = Fixture::new();
    log.call(|db| {
        db.execute_batch("PRAGMA query_only=ON")?;
        Ok(())
    })?
    .await?;
    let error = log
        .record_chat(Arc::new(ChatEnvelope {
            cursor: ChatCursor {
                service_instance: [4; 16],
                sequence: 0,
            },
            event: Arc::new(ChatEvent::Observation(Arc::new(f.protocol))),
        }))
        .await
        .expect_err("the actual read-only SQLite write must fail");
    let StoreError::Sql(ref source) = error else {
        panic!("actual SQLite failure must retain its source");
    };
    let native = source
        .sqlite_error()
        .expect("original SQLite numeric error");
    assert_eq!(native.extended_code & 0xff, rusqlite::ffi::SQLITE_READONLY);
    let diagnostic = format!("{error:?}");
    assert!(diagnostic.contains(&source.to_string()));
    assert!(diagnostic.contains(&format!("extended_code: {}", native.extended_code)));
    assert_eq!(error.to_string(), "business database operation failed");
    assert_eq!(log.counts().await?.protocol_records, 0);
    assert_eq!(log.write_failure_count(), 1);
    log.close().await?;
    Ok(())
}

fn native_tool_facts(f: &Fixture) -> as_types::main_ports::ToolExecutionFacts {
    use as_types::main_ports::{NativeToolContext, ToolExecutionFacts};
    let ChatPart::ToolCall {
        native_id: Some(id),
        name,
        arguments,
        ..
    } = &f.message.observed_parts[0]
    else {
        panic!("software fixture declares its original tool call");
    };
    let NativeIdValue::JsonString(id) = &id.value else {
        panic!("software context contains a string identifier");
    };
    ToolExecutionFacts {
        event: EventKey::from(f.completion.event.as_ref()),
        context: NativeToolContext {
            process: f.message.source.actor.process.expect("software process"),
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            response_id: None,
            run_id: Some("original-run".into()),
            session_id: None,
            session_key: Some(String::new()),
            agent_id: None,
            provider: Some("fixture".into()),
            api: Some("openai-responses".into()),
            model: None,
            arguments: arguments.clone(),
        },
    }
}

#[tokio::test]
async fn execution_binding_rejects_a_different_tool_in_the_same_response() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("tool-binding.sqlite"))?;
    let mut f = Fixture::new();
    let mut facts = native_tool_facts(&f);
    facts.context.tool_call_id = "original-call".into();
    facts.context.tool_name = "read_file".into();
    facts.context.arguments = Some(ChatContent::Json(serde_json::json!({})));
    facts.context.response_id = Some("same-response".into());
    Arc::make_mut(&mut f.message.source).actor.process = Some(facts.context.process);
    f.protocol.source = f.message.source.clone();
    if let ChatPart::ToolCall { arguments, .. } = &mut f.message.observed_parts[0] {
        *arguments = facts.context.arguments.clone();
    }
    let second = NativeProtocolId {
        namespace: NativeIdNamespace::ToolCall,
        field_path: "/output/1/call_id".into(),
        value: NativeIdValue::JsonString("second-call".into()),
    };
    f.message.observed_parts.push(ChatPart::ToolCall {
        id: 1,
        native_id: Some(second.clone()),
        name: "read_file".into(),
        arguments: Some(ChatContent::Json(serde_json::json!({}))),
    });
    f.protocol.native_ids.extend([
        second,
        NativeProtocolId {
            namespace: NativeIdNamespace::Response,
            field_path: "/id".into(),
            value: NativeIdValue::JsonString("same-response".into()),
        },
    ]);
    f.protocol.body = EvidenceBody::from_bytes(serde_json::to_vec(&serde_json::json!({
        "id": "same-response", "output": [
            {"type":"function_call","call_id":"original-call","name":"read_file","arguments":"{}"},
            {"type":"function_call","call_id":"second-call","name":"read_file","arguments":"{}"}
        ]
    }))?);
    let event = facts.event;
    record_tool_fixture(&log, facts).await?;
    log.record_up(Arc::new(Up::OperationResult(Arc::new(f.completion))))?
        .await?;
    chat(&log, 1, ChatEvent::Observation(f.protocol.into())).await?;
    chat(&log, 2, ChatEvent::Finished(f.message.into())).await?;
    let second_call = ChatPartRef {
        part_id: 1,
        ..f.call
    };
    let rejected = log.bind_tool_execution(second_call, event).await;
    assert!(
        matches!(rejected, Err(StoreError::Conflict)),
        "{rejected:?}"
    );
    log.bind_tool_execution(f.call, event).await?;
    assert_eq!(
        crate::activity::execution_call(&log.activity(FactReference::Event(event)).await?, event),
        Ok(f.call)
    );
    log.close().await?;
    Ok(())
}

#[test]
fn execution_binding_checks_original_context_and_complete_compound_id() {
    let mut f = Fixture::new();
    let mut facts = native_tool_facts(&f);
    facts.context.tool_call_id.push_str("|item-0");
    f.protocol.native_ids.push(NativeProtocolId {
        namespace: NativeIdNamespace::ProviderEvent,
        field_path: "/output/0/id".into(),
        value: NativeIdValue::JsonString("item-0".into()),
    });
    f.protocol.body = EvidenceBody::from_bytes(
        br#"{"output":[{"type":"function_call","id":"item-0","call_id":"original-call","name":"read_file","arguments":"{}"}]}"#.to_vec(),
    );
    let link = EvidenceRelation {
        kind: RelationKind::ToolExecution,
        from: FactReference::Part(f.call),
        to: FactReference::Event(facts.event),
        evidence: vec![FactReference::Protocol(f.protocol.id)],
    };
    let view = ContextView {
        anchor: link.to.clone(),
        source_records: vec![
            SourceRecord::Protocol(Arc::new(f.protocol)),
            SourceRecord::Message(Arc::new(f.message)),
            SourceRecord::OperationResult(Arc::new(f.completion)),
            SourceRecord::ToolExecution(Arc::new(facts)),
        ],
        relations: vec![],
        gaps: vec![],
    };
    assert_eq!(
        crate::activity::validate_execution_link(&view, &link),
        Ok(f.call)
    );
    for field in [
        "process",
        "name",
        "arguments",
        "argument_type",
        "compound_item",
    ] {
        let mut changed = view.clone();
        let SourceRecord::ToolExecution(value) = &mut changed.source_records[3] else {
            panic!("fixture source");
        };
        let context = &mut Arc::make_mut(value).context;
        match field {
            "process" => context.process.birth_boottime_ns += 1,
            "name" => context.tool_name.push_str("-other"),
            "arguments" => context.arguments = Some(ChatContent::Json(serde_json::json!(null))),
            "argument_type" => context.arguments = Some(ChatContent::Text("{}".into())),
            "compound_item" => context.tool_call_id = "original-call|other-item".into(),
            _ => unreachable!(),
        }
        assert_eq!(
            crate::activity::validate_execution_link(&changed, &link)
                .expect_err(field)
                .reason,
            ContextGapReason::ConflictingEvidence
        );
    }
    for field in [
        "missing_process",
        "missing_item",
        "other_item_path",
        "numeric_item",
    ] {
        let mut changed = view.clone();
        let SourceRecord::Protocol(value) = &mut changed.source_records[0] else {
            panic!("fixture protocol");
        };
        let protocol = Arc::make_mut(value);
        let reason = match field {
            "missing_process" => {
                Arc::make_mut(&mut protocol.source).actor.process = None;
                let source = protocol.source.clone();
                let SourceRecord::Message(message) = &mut changed.source_records[1] else {
                    panic!("fixture message");
                };
                Arc::make_mut(message).source = source;
                ContextGapReason::NotProvided
            }
            "missing_item" => {
                protocol.native_ids.pop();
                ContextGapReason::NotProvided
            }
            "other_item_path" => {
                protocol.native_ids[1].field_path = "/output/1/id".into();
                ContextGapReason::NotProvided
            }
            "numeric_item" => {
                protocol.native_ids[1].value = NativeIdValue::JsonNumber("0".into());
                ContextGapReason::ConflictingEvidence
            }
            _ => unreachable!(),
        };
        assert_eq!(
            crate::activity::validate_execution_link(&changed, &link)
                .expect_err(field)
                .reason,
            reason
        );
    }
    let mut missing_arguments = view;
    let SourceRecord::ToolExecution(value) = &mut missing_arguments.source_records[3] else {
        panic!("fixture source");
    };
    Arc::make_mut(value).context.arguments = None;
    assert_eq!(
        crate::activity::validate_execution_link(&missing_arguments, &link),
        Ok(f.call)
    );
}

#[tokio::test]
async fn rule_feedback_uses_original_denial_without_a_frontend_or_native_return()
-> Result<(), StoreError> {
    use crate::execution_feedback::{FeedbackExecution, execute_feedback};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct PreparedOnly {
        calls: AtomicUsize,
        request: RuleFeedbackRequest,
        cancelled: Mutex<Option<tokio_util::sync::CancellationToken>>,
    }
    #[async_trait::async_trait]
    impl FlowFeedbackPort for PreparedOnly {
        async fn prepare(
            &self,
            input: RuleFeedbackPrepareInput,
        ) -> Result<RuleFeedbackFact, FlowError> {
            assert_eq!(
                EventKey::from(input.notice.event.as_ref()),
                self.request.denial
            );
            assert_eq!(input.call, self.request.call);
            assert_eq!(input.message.id, self.request.call.message);
            assert_eq!(input.observations.len(), 1);
            assert!(!input.cancelled.is_cancelled());
            *self.cancelled.lock().expect("source token") = Some(input.cancelled);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(RuleFeedbackFact {
                denial: self.request.denial,
                call: input.call,
                state: RuleFeedbackState::Prepared,
            })
        }
    }
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("feedback.sqlite"))?;
    let f = Fixture::new();
    let event = EventKey::from(f.completion.event.as_ref());
    let executor = Arc::new(PreparedOnly {
        calls: AtomicUsize::new(0),
        request: RuleFeedbackRequest {
            denial: event,
            call: f.call,
        },
        cancelled: Mutex::new(None),
    });
    let notice = Arc::new(MainNotice {
        decision_id: None,
        event: f.completion.event.clone(),
        choice: Some(RuleChoice {
            decision: Decision::Deny {
                terminate: Termination::None,
            },
            hit: f.completion.hit.clone(),
            reason: ChoiceReason::Direct,
        }),
        result: Ok(DispatchOutcome::Driver {
            outcome: ExecutionOutcome::Denied,
            termination: TerminationReport::NONE,
        }),
    });
    let Some(Up::OperationResult(completion)) =
        as_main::messages::protocol::operation_result(&notice)
    else {
        panic!("original completion");
    };
    assert!(matches!(
        log.record_completion(&notice, completion.clone()).await?,
        RecordOutcome::Stored(_)
    ));
    record_tool_fixture(&log, native_tool_facts(&f)).await?;
    chat(&log, 1, ChatEvent::Observation(Arc::new(f.protocol))).await?;
    chat(&log, 2, ChatEvent::Finished(Arc::new(f.message))).await?;
    let cancelled = tokio_util::sync::CancellationToken::new();
    assert!(matches!(
        execute_feedback(
            executor.as_ref(),
            &log,
            notice.clone(),
            &completion,
            cancelled.clone()
        )
        .await?,
        FeedbackExecution::Gap(_)
    ));
    assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
    log.bind_tool_execution(f.call, event).await?;
    use as_types::main_ports::{MainMessage, MessageModule};
    use futures::FutureExt;
    let media = crate::media::Media::new(log.clone());
    let (source, input) = ChatSender::channel(4096);
    let mut consumer = Box::pin(media.consume_chat(input, Some(executor.clone())));
    assert!(consumer.as_mut().now_or_never().is_none());
    for _ in 0..2 {
        let (send, input) = tokio::sync::mpsc::channel(1);
        send.send(MainMessage::Completed(Box::new(notice.as_ref().clone())))
            .await
            .expect("original completed notice");
        drop(send);
        media.run(input).await.expect("formal completion consumer");
    }
    drop(source);
    consumer.await.expect("source retired");
    assert!(
        executor
            .cancelled
            .lock()
            .expect("source token")
            .as_ref()
            .expect("prepared token")
            .is_cancelled()
    );
    assert_eq!(
        executor.calls.load(Ordering::SeqCst),
        1,
        "persistent claim prevents duplicate preparation"
    );
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn native_request_return_reopens_as_an_original_fact_without_a_tool_edge()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let original = Fixture::new();
    let event = EventKey::from(original.completion.event.as_ref());
    let fact = as_types::driver::NativeRequestReturn {
        event,
        operation: as_types::driver::NativeRequestOperation::FileOpen,
        decision_id: DecisionId([7; 16]),
        wait_id: 3,
        returned_boottime_ns: 10,
        return_value: -1,
    };
    log.record_request_return(fact.clone()).await?;
    log.close().await.map_err(|_| StoreError::Unconfirmed)?;
    let reopened = BusinessLog::open_path(&path)?;
    let view = reopened.activity(FactReference::Event(event)).await?;
    assert!(view.source_records.iter().any(
        |record| matches!(record, SourceRecord::RequestReturn(value) if value.as_ref() == &fact)
    ));
    assert!(view.relations.is_empty());
    Ok(())
}

#[tokio::test]
async fn canonical_source_remains_unbound_until_protocol_proof_and_preserves_conflicts()
-> Result<(), StoreError> {
    use super::query::FeedbackClaim;
    use crate::{
        activity::execution_call,
        execution_feedback::{FeedbackPreparation, prepare_feedback},
    };
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let original = Fixture::new();
    let mut second_message = original.message.clone();
    second_message.id.sequence += 1;
    let event = EventKey::from(original.completion.event.as_ref());
    let handoff = native_tool_facts(&original);
    let call = original.call;
    assert!(matches!(
        log.bind_tool_execution(call, event).await,
        Err(StoreError::NotProvided)
    ));
    assert_eq!(log.write_failure_count(), 0);
    record_tool_fixture(&log, handoff.clone()).await?;
    record_tool_fixture(&log, handoff.clone()).await?;
    let counts = log.counts().await?;
    assert_eq!(counts.relations, 0);
    assert_eq!(counts.messages, 0);
    assert_eq!(counts.operation_reports, 0);
    let observed = log.activity(FactReference::Event(event)).await?;
    assert!(observed.source_records.iter().any(|record| matches!(record,
        SourceRecord::ToolExecution(value) if value.as_ref() == &handoff)));
    assert!(
        observed
            .relations
            .iter()
            .all(|relation| relation.kind == RelationKind::NativeRequestReference)
    );

    let question_id = DecisionId([7; 16]);
    let anchor = FactReference::Question(question_id);
    log.record_up(question(&original))?.await?;
    let pending = log.activity(anchor.clone()).await?;
    assert_eq!(
        execution_call(&pending, event).unwrap_err().reason,
        ContextGapReason::NoExplicitRelation
    );
    let completion = Arc::new(original.completion.clone());
    log.record_up(Arc::new(Up::OperationResult(completion.clone())))?
        .await?;
    chat(&log, 1, ChatEvent::Observation(Arc::new(original.protocol))).await?;
    chat(&log, 2, ChatEvent::Finished(Arc::new(original.message))).await?;
    let unbound = log.read_ask_context(question_id, None).await?;
    assert!(unbound.window.is_none());
    assert_eq!(
        unbound.call.unwrap_err().reason,
        ContextGapReason::NoExplicitRelation
    );
    assert!(unbound.view.source_records.iter().any(|r| matches!(
        r, crate::context::SourceRecord::ToolExecution(facts) if facts.as_ref() == &handoff
    )));
    log.bind_tool_execution(call, event)
        .await
        .expect("first proven binding");
    let ready = log.activity(anchor.clone()).await?;
    assert_eq!(execution_call(&ready, event), Ok(call));
    let read = log.read_ask_context(question_id, None).await?;
    assert_eq!(read.call, Ok(call));
    let (window, _) = read.window.expect("original exchange window")?;
    assert_eq!(window.message_refs, vec![call.message]);
    assert_eq!(
        prepare_feedback(&completion, &ready),
        FeedbackPreparation::Request(RuleFeedbackRequest {
            denial: event,
            call,
        })
    );
    let event_anchor = FactReference::Event(event);
    let request = RuleFeedbackRequest {
        denial: event,
        call,
    };
    assert!(matches!(
        log.claim_feedback(request).await?,
        FeedbackClaim::New
    ));
    let (only_claimed, _) = log.activity_context(event_anchor.clone(), None).await?;
    assert!(
        !only_claimed
            .source_records
            .iter()
            .any(|record| matches!(record, SourceRecord::RuleFeedback(_)))
    );
    let feedback = RuleFeedbackFact {
        denial: event,
        call,
        state: RuleFeedbackState::Submitted {
            original: ProtocolRecordId {
                exchange: call.message.exchange,
                sequence: 4,
            },
        },
    };
    chat(&log, 4, ChatEvent::RuleFeedback(feedback.clone())).await?;
    let (with_feedback, _) = log.activity_context(event_anchor.clone(), None).await?;
    assert!(with_feedback.source_records.iter().any(
        |record| matches!(record, SourceRecord::RuleFeedback(value) if value.as_ref() == &feedback)
    ));
    let second_call = ChatPartRef {
        message: second_message.id,
        part_id: call.part_id,
    };
    chat(&log, 3, ChatEvent::Finished(Arc::new(second_message))).await?;
    log.bind_tool_execution(second_call, event)
        .await
        .expect("second individually proven binding");
    let ambiguous = log.activity(FactReference::Event(event)).await?;
    assert_eq!(
        execution_call(&ambiguous, event).unwrap_err().reason,
        ContextGapReason::ConflictingEvidence
    );
    let (ambiguous_feedback, _) = log.activity_context(event_anchor, None).await?;
    assert!(
        !ambiguous_feedback
            .source_records
            .iter()
            .any(|record| matches!(record, SourceRecord::RuleFeedback(_)))
    );

    let mut conflicting_source = handoff.clone();
    conflicting_source.context.response_id = Some("different-original-response".into());
    record_tool_fixture(&log, conflicting_source.clone()).await?;
    assert_eq!(log.write_failure_count(), 0);
    let observed = log.activity(FactReference::Event(event)).await?;
    assert!(observed.source_records.iter().any(|record| matches!(record,
        SourceRecord::RequestHandoff(raw) if matches!(&raw.phase, as_types::runtime_observation::NativeRequestPhase::Submission { analysis: Some(analysis), .. } if analysis.candidates.iter().any(|candidate| candidate.context == conflicting_source.context)))));
    assert_eq!(
        execution_call(&observed, event).unwrap_err().reason,
        ContextGapReason::ConflictingEvidence
    );
    let mut other = call;
    other.part_id += 1;
    assert!(matches!(
        log.bind_tool_execution(other, event).await,
        Err(StoreError::NotProvided)
    ));
    assert_eq!(log.write_failure_count(), 0);
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    let conflict = reopened.activity(anchor).await?;
    assert!(conflict.source_records.iter().any(|record| matches!(
        record, crate::context::SourceRecord::ToolExecution(facts) if facts.as_ref() == &handoff
    )));
    assert_eq!(
        execution_call(&conflict, event).unwrap_err().reason,
        ContextGapReason::ConflictingEvidence
    );
    let read = reopened.read_ask_context(question_id, None).await?;
    assert_eq!(
        read.call.unwrap_err().reason,
        ContextGapReason::ConflictingEvidence
    );
    assert!(read.window.is_none());
    for anchor in [
        FactReference::Part(call),
        FactReference::Message(call.message),
    ] {
        let view = reopened.activity(anchor).await?;
        assert_eq!(
            execution_call(&view, event).unwrap_err().reason,
            ContextGapReason::ConflictingEvidence
        );
        assert!(
            view.relations
                .iter()
                .all(|relation| relation.kind != RelationKind::ToolExecution)
        );
    }
    assert!(matches!(
        prepare_feedback(&completion, &conflict),
        FeedbackPreparation::Gap(ContextGap {
            reason: ContextGapReason::ConflictingEvidence,
            ..
        })
    ));
    assert!(matches!(
        reopened
            .claim_feedback(RuleFeedbackRequest {
                denial: event,
                call
            })
            .await?,
        FeedbackClaim::ConflictingEvidence
    ));
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn attribution_reuses_frozen_exchange_after_sibling_stream_advances() -> Result<(), StoreError>
{
    use crate::attribution::{
        ActorScope, AttributionPattern, AttributionRequest, TargetComparison, TargetPattern,
    };

    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("frozen-attribution.sqlite"))?;
    let mut f = Fixture::new();
    let native = native_tool_facts(&f);
    let mut raw_event = serde_json::to_value(f.completion.event.as_ref())?;
    raw_event["header"]["stage"] = serde_json::json!("controlled_request");
    raw_event["header"]["source_boottime_ns"] = row::number(2_000);
    raw_event["header"]["actor_tgid"] = serde_json::json!(native.context.process.host_tgid);
    raw_event["header"]["actor_birth_boottime_ns"] =
        row::number(native.context.process.birth_boottime_ns);
    raw_event["payload"]["path"] = serde_json::json!(b"/audit/file");
    let mut event: as_types::dispatch_event::EventData = serde_json::from_value(raw_event)?;
    assert!(event.bind_host_boot(native.context.process.boot_id));
    f.completion.event = Arc::new(event);
    Arc::make_mut(&mut f.message.source).actor.process = Some(native.context.process);
    f.protocol.source = f.message.source.clone();
    f.message.first_seen_boottime_ns = 1_900;
    f.message.last_seen_boottime_ns = 1_900;
    let ChatPart::ToolCall { arguments, .. } = &mut f.message.observed_parts[0] else {
        return Err(StoreError::Invalid);
    };
    *arguments = Some(ChatContent::Json(serde_json::json!({"path":"/audit/file"})));
    f.protocol.body = EvidenceBody::from_bytes(serde_json::to_vec(&serde_json::json!({
        "output": [{"type":"function_call", "call_id":"original-call", "name":"read_file",
                    "arguments":"{\"path\":\"/audit/file\"}"}]
    }))?);
    chat(
        &log,
        1,
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
    )
    .await?;
    chat(&log, 2, ChatEvent::Finished(Arc::new(f.message.clone()))).await?;

    let mut sibling = f.message.clone();
    sibling.id.sequence += 1;
    sibling.record_refs.clear();
    sibling.state = ChatState::Streaming;
    sibling.observed_parts = vec![ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: "pending".into(),
    }];
    chat(&log, 3, ChatEvent::Started(Arc::new(sibling.clone()))).await?;
    log.record_up(question(&f))?.await?;
    record_tool_fixture(&log, native_tool_facts(&f)).await?;
    log.bind_tool_execution(f.call, EventKey::from(f.completion.event.as_ref()))
        .await?;
    let question_id = DecisionId([7; 16]);
    let frozen = log.read_ask_context(question_id, None).await?;
    assert_eq!(frozen.call, Ok(f.call));
    let (window, messages) = frozen.window.ok_or(StoreError::NotProvided)??;
    assert!(messages.iter().any(|message| message.as_ref() == &sibling));

    chat(
        &log,
        4,
        ChatEvent::Delta {
            id: sibling.id,
            base_revision: sibling.revision,
            revision: sibling.revision + 1,
            changes: vec![ChatDelta::AppendText {
                part_id: 0,
                text: " advanced".into(),
            }],
        },
    )
    .await?;
    let latest = log.model_messages(sibling.id.exchange, None, 64).await?.0;
    assert!(latest.iter().any(|message| message.id == sibling.id
        && message.revision == sibling.revision + 1
        && matches!(&message.observed_parts[0], ChatPart::Text { text, .. } if text == "pending advanced")));

    let read = log
        .read_ask_context(
            question_id,
            Some(AttributionRequest {
                pattern: AttributionPattern {
                    actor: ActorScope::SameProcess,
                    lookback_ns: 100,
                    target: TargetPattern::FilePath {
                        pointer: "/path".into(),
                    },
                },
                after: None,
                limit: 64,
            }),
        )
        .await?;
    assert_eq!(read.call, Ok(f.call));
    let candidates = read.attribution.ok_or(StoreError::NotProvided)??;
    assert_eq!(candidates.matches.candidates.len(), 1);
    assert_eq!(
        candidates.matches.candidates[0].target_comparison,
        TargetComparison::Equal
    );
    assert!(
        candidates.windows.is_empty(),
        "confirmed exchange must not get a competing latest window"
    );
    let (again, original_messages) = read.window.ok_or(StoreError::NotProvided)??;
    assert_eq!(serde_json::to_value(again)?, serde_json::to_value(window)?);
    assert_eq!(original_messages, messages);
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn ask_twenty_is_explicit_exchange_ordered_and_frozen_in_sql() -> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let mut f = Fixture::new();
    f.message.id.sequence = 1 << 63;
    f.call.message = f.message.id;
    chat(
        &log,
        0,
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
    )
    .await?;
    for n in 0..25 {
        let mut m = f.message.clone();
        m.id.sequence = n;
        m.origin = ChatOrigin::RequestContext;
        m.role = ChatRole::User;
        m.record_refs = vec![];
        m.observed_parts = vec![ChatPart::Text {
            id: 0,
            kind: TextKind::Plain,
            text: "same original content".into(),
        }];
        chat(&log, n + 1, ChatEvent::Finished(Arc::new(m))).await?;
    }
    chat(&log, 26, ChatEvent::Finished(Arc::new(f.message.clone()))).await?;
    let q = question(&f);
    log.record_up(q.clone())?.await?;
    let question_id = DecisionId([7; 16]);
    let read = log.read_ask_context(question_id, None).await?;
    assert_eq!(
        read.call.unwrap_err().reason,
        ContextGapReason::NoExplicitRelation
    );
    assert!(
        read.window.is_none(),
        "a real Ask alone is not a tool bridge"
    );
    assert_eq!(
        log.write_failure_count(),
        0,
        "a missing execution bridge is a read gap"
    );
    record_tool_fixture(&log, native_tool_facts(&f)).await?;
    log.bind_tool_execution(f.call, EventKey::from(f.completion.event.as_ref()))
        .await?;
    log.call(|db| {
        db.execute_batch("PRAGMA query_only=ON")?;
        Ok(())
    })?
    .await?;
    assert!(
        matches!(
            log.read_ask_context(question_id, None).await?.window,
            Some(Err(StoreError::Sql(_)))
        ),
        "actual freeze write must fail on readonly database"
    );
    assert_eq!(
        log.write_failure_count(),
        1,
        "freeze write failure observed in the SQL worker"
    );
    log.call(|db| {
        db.execute_batch("PRAGMA query_only=OFF")?;
        Ok(())
    })?
    .await?;
    let (window, messages) = log
        .read_ask_context(question_id, None)
        .await?
        .window
        .expect("real exchange window")?;
    assert_eq!(window.message_refs.len(), 20);
    assert_eq!(window.earlier_observed_messages, 6);
    assert_eq!(window.message_refs[0].sequence, 6);
    assert_eq!(messages.last().expect("last").id, f.call.message);
    let mut later = f.message.clone();
    later.id.sequence += 1;
    later.observed_parts = vec![ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: "later output".into(),
    }];
    later.record_refs = vec![];
    chat(&log, 27, ChatEvent::Finished(Arc::new(later))).await?;
    let (frozen, again) = log
        .read_ask_context(question_id, None)
        .await?
        .window
        .expect("real exchange window")?;
    assert_eq!(frozen.message_refs, window.message_refs);
    assert_eq!(again.as_slice(), messages.as_slice());
    log.close().await?;
    let reopened = BusinessLog::open_path(&dir.path().join("db"))?;
    let (frozen, _) = reopened
        .read_ask_context(question_id, None)
        .await?
        .window
        .expect("real exchange window")?;
    assert_eq!(frozen.message_refs, window.message_refs);
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn operation_results_keep_original_events_without_questions_or_native_associations()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("failure-first.sqlite"))?;
    let f = Fixture::new();
    for (index, outcome) in [
        Some(ExecutionOutcome::Failed { native_error: -110 }),
        Some(ExecutionOutcome::Cancelled),
        Some(ExecutionOutcome::Unknown),
        None,
        Some(ExecutionOutcome::Allowed),
        Some(ExecutionOutcome::Denied),
    ]
    .into_iter()
    .enumerate()
    {
        for entry in 0..2 {
            let identity = index * 2 + entry;
            let decision_id = DecisionId([40 + identity as u8; 16]);
            let mut event = serde_json::to_value(f.completion.event.as_ref())?;
            event["header"]["sequence"] = row::number(100 + identity as u64);
            let notice = MainNotice {
                decision_id: Some(decision_id),
                event: Arc::new(serde_json::from_value(event)?),
                choice: None,
                result: match outcome {
                    Some(outcome) => Ok(DispatchOutcome::Execution(OperationReceipt {
                        decision_id,
                        outcome,
                        termination: TerminationReport::NONE,
                    })),
                    None => Err(as_types::main_ports::ModuleError::Unavailable),
                },
            };
            let event = EventKey::from(notice.event.as_ref());
            let Some(Up::OperationResult(expected)) =
                as_main::messages::protocol::operation_result(&notice)
            else {
                panic!("controlled completion retains its real decision identity")
            };
            let stored = if entry == 0 {
                log.record_completion(&notice, expected.clone()).await?
            } else {
                log.record_up(Arc::new(Up::OperationResult(expected.clone())))?
                    .await?
            };
            assert!(matches!(stored, RecordOutcome::Stored(_)));
            let (view, _) = log
                .activity_context(FactReference::Event(event), None)
                .await?;
            assert_eq!(view.source_records.len(), 1);
            let SourceRecord::OperationResult(original) = &view.source_records[0] else {
                panic!("result is not an invented question or native association")
            };
            assert_eq!(original, &expected);
            assert!(view.relations.is_empty());
        }
    }
    let counts = log.counts().await?;
    assert_eq!(counts.questions, 0);
    assert_eq!(counts.operation_reports, 12);
    Ok(())
}

#[tokio::test]
async fn completion_identity_does_not_manufacture_a_question_or_execution_relation()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let f = Fixture::new();
    let id = DecisionId([7; 16]);
    let mut notice = MainNotice {
        decision_id: Some(id),
        event: f.completion.event.clone(),
        choice: Some(RuleChoice {
            decision: Decision::Deny {
                terminate: Termination::None,
            },
            hit: f.completion.hit.clone(),
            reason: ChoiceReason::Direct,
        }),
        result: Ok(DispatchOutcome::Execution(OperationReceipt {
            decision_id: id,
            outcome: ExecutionOutcome::Denied,
            termination: TerminationReport::NONE,
        })),
    };
    let mut ordinary = notice.clone();
    ordinary
        .choice
        .as_mut()
        .expect("choice")
        .hit
        .as_mut()
        .expect("hit")
        .deny_message = Some(String::new());
    let Some(Up::OperationResult(completion)) =
        as_main::messages::protocol::operation_result(&ordinary)
    else {
        panic!("original completion");
    };
    assert!(matches!(
        log.record_completion(&ordinary, completion).await?,
        RecordOutcome::Stored(_)
    ));
    // The previously filtered result now exists; a different completion needs its own identities.
    notice = ordinary;
    let timeout_id = DecisionId([2; 16]);
    notice.decision_id = Some(timeout_id);
    let mut event = serde_json::to_value(notice.event.as_ref())?;
    event["header"]["sequence"] = row::number(8);
    notice.event = Arc::new(serde_json::from_value(event)?);
    notice.result = Ok(DispatchOutcome::Execution(OperationReceipt {
        decision_id: timeout_id,
        outcome: ExecutionOutcome::Denied,
        termination: TerminationReport::NONE,
    }));
    notice.choice.as_mut().expect("choice").reason = ChoiceReason::Timeout;
    let Some(Up::OperationResult(completion)) =
        as_main::messages::protocol::operation_result(&notice)
    else {
        panic!("original completion");
    };
    assert!(matches!(
        log.record_completion(&notice, completion).await?,
        RecordOutcome::Stored(_)
    ));
    let counts = log.counts().await?;
    assert_eq!(counts.questions, 0);
    assert_eq!(counts.operation_reports, 2);
    assert!(matches!(
        log.question(id).await,
        Err(StoreError::NotProvided)
    ));
    let mut native_only = native_tool_facts(&f);
    native_only.event.sequence += 100;
    record_tool_fixture(&log, native_only.clone()).await?;
    let mut direct = notice.clone();
    let mut event = serde_json::to_value(&direct.event)?;
    event["header"]["sequence"] = native_only.event.sequence.to_string().into();
    direct.event = serde_json::from_value(event)?;
    direct.choice.as_mut().expect("choice").reason = ChoiceReason::Direct;
    let Some(Up::OperationResult(completion)) =
        as_main::messages::protocol::operation_result(&direct)
    else {
        panic!("original completion");
    };
    assert!(matches!(
        log.record_completion(&direct, completion).await?,
        RecordOutcome::Stored(_)
    ));
    let facts = log
        .activity(FactReference::Event(native_only.event))
        .await?;
    assert!(
        facts
            .source_records
            .iter()
            .any(|r| matches!(r, SourceRecord::OperationResult(_)))
    );
    assert!(
        facts
            .relations
            .iter()
            .all(|r| r.kind != crate::context::RelationKind::ToolExecution)
    );
    for (index, native_error) in [Some(-110), Some(-5), None].into_iter().enumerate() {
        let mut original = notice.clone();
        let decision_id = DecisionId([index as u8 + 8; 16]);
        original.decision_id = Some(decision_id);
        let mut event = serde_json::to_value(&original.event)?;
        event["header"]["sequence"] = (index as u64 + 2).to_string().into();
        original.event = serde_json::from_value(event)?;
        original.result = match native_error {
            Some(native_error) => Ok(DispatchOutcome::Execution(OperationReceipt {
                decision_id,
                outcome: ExecutionOutcome::Failed { native_error },
                termination: TerminationReport::NONE,
            })),
            None => Err(as_types::main_ports::ModuleError::Unavailable),
        };
        let reference = FactReference::Completion(EventKey::from(original.event.as_ref()));
        let Some(Up::OperationResult(expected)) =
            as_main::messages::protocol::operation_result(&original)
        else {
            panic!("original result");
        };
        for _ in 0..2 {
            log.record_completion(&original, expected.clone()).await?;
        }
        let view = log.activity(reference).await?;
        let actual = view
            .source_records
            .iter()
            .find_map(|r| match r {
                SourceRecord::OperationResult(report) => Some(report),
                _ => None,
            })
            .expect("one stored original report");
        assert_eq!(actual.as_ref(), expected.as_ref());
        let json = serde_json::to_value(actual)?;
        if let Some(code) = native_error {
            assert_eq!(json["result"]["type"], "receipt");
            assert_eq!(json["result"]["outcome"]["failed"]["native_error"], code);
        } else {
            assert_eq!(json["result"]["type"], "failed");
        }
    }
    assert_eq!(
        log.counts().await?.operation_reports,
        6,
        "one canonical report per original, not one more failure fact"
    );
    assert_eq!(log.counts().await?.questions, 0);
    log.close().await?;
    // This owned fixture simulates the old columns; no historical evidence database is touched.
    let old = rusqlite::Connection::open(dir.path().join("db"))?;
    old.execute(
        "ALTER TABLE operation_reports DROP COLUMN outcome_native_error",
        [],
    )?;
    drop(old);
    assert!(matches!(
        BusinessLog::open_path(&dir.path().join("db")),
        Err(StoreError::Invalid)
    ));
    Ok(())
}

#[tokio::test]
async fn delta_is_persisted_and_reconstructed_without_a_rust_history_cache()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let mut f = Fixture::new();
    f.message.observed_parts = vec![ChatPart::Text {
        id: 0,
        kind: TextKind::Plain,
        text: String::new(),
    }];
    f.message.record_refs = vec![];
    f.message.state = ChatState::Streaming;
    chat(&log, 0, ChatEvent::Started(Arc::new(f.message.clone()))).await?;
    chat(
        &log,
        1,
        ChatEvent::Delta {
            id: f.message.id,
            base_revision: 1,
            revision: 2,
            changes: vec![ChatDelta::AppendText {
                part_id: 0,
                text: "actual text".into(),
            }],
        },
    )
    .await?;
    log.close().await?;
    let log = BusinessLog::open_path(&dir.path().join("db"))?;
    let view = log.activity(FactReference::Message(f.message.id)).await?;
    let message = view
        .source_records
        .iter()
        .find_map(|r| match r {
            SourceRecord::Message(m) => Some(m),
            _ => None,
        })
        .expect("stored message");
    assert_eq!(message.revision, 2);
    assert!(matches!(&message.observed_parts[0],ChatPart::Text{text,..}if text=="actual text"));
    assert_eq!(log.counts().await?.message_revisions, 1);
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn durable_feedback_claim_is_unconfirmed_after_restart_and_does_not_replay()
-> Result<(), StoreError> {
    use super::query::FeedbackClaim;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let log = BusinessLog::open_path(&path)?;
    let f = Fixture::new();
    let request = RuleFeedbackRequest {
        denial: EventKey::from(f.completion.event.as_ref()),
        call: f.call,
    };
    assert!(matches!(
        log.claim_feedback(request).await?,
        FeedbackClaim::New
    ));
    let mut second = request;
    second.denial.sequence += 1;
    assert!(matches!(
        log.claim_feedback(second).await?,
        FeedbackClaim::AlreadyClaimed
    ));
    assert_eq!(log.write_failure_count(), 0);
    log.close().await?;
    let log = BusinessLog::open_path(&path)?;
    assert!(matches!(
        log.claim_feedback(request).await?,
        FeedbackClaim::Unconfirmed
    ));
    assert!(matches!(
        log.feedback_state(request).await,
        Err(StoreError::NotProvided)
    ));
    let original = ProtocolRecordId {
        exchange: f.call.message.exchange,
        sequence: 4,
    };
    chat(
        &log,
        0,
        ChatEvent::RuleFeedback(RuleFeedbackFact {
            denial: request.denial,
            call: request.call,
            state: RuleFeedbackState::Submitted { original },
        }),
    )
    .await?;
    chat(
        &log,
        1,
        ChatEvent::RuleFeedback(RuleFeedbackFact {
            denial: request.denial,
            call: request.call,
            state: RuleFeedbackState::Prepared,
        }),
    )
    .await?;
    assert!(
        matches!(log.feedback_state(request).await?.state,RuleFeedbackState::Submitted{original:actual}if actual==original)
    );
    assert!(matches!(
        log.claim_feedback(request).await?,
        FeedbackClaim::Existing(_)
    ));
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn ask_write_starts_without_polling_its_waiter_and_close_seals_admission()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("business.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let ask = question(&Fixture::new());
    // A transport can disappear before its completion lane is polled at all.
    drop(log.record_up(ask.clone())?);
    let closing = log.close();
    assert!(matches!(
        log.record_up(ask.clone()),
        Err(StoreError::Closed)
    ));
    tokio::try_join!(closing, log.close())?;
    let reopened = BusinessLog::open_path(&path)?;
    assert_eq!(
        serde_json::to_value(reopened.question(DecisionId([7; 16])).await?.as_ref())?,
        serde_json::to_value(ask.as_ref())?
    );
    assert_eq!(log.write_failure_count(), 0);
    reopened.close().await?;
    Ok(())
}

#[test]
fn ask_write_outside_runtime_returns_unconfirmed_without_starting_sql() -> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("business.sqlite"))?;
    assert!(matches!(
        log.record_up(question(&Fixture::new())),
        Err(StoreError::Unconfirmed)
    ));
    assert_eq!(log.write_failure_count(), 0, "no SQL work was admitted");
    Ok(())
}

#[tokio::test]
async fn cancelled_waiters_preserve_real_write_failure_and_physical_close() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let log = BusinessLog::open_path(&path)?;
    let (started, begin) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let writer = log.clone();
    let caller = tokio::spawn(async move {
        writer
            .write_call(move |db| {
                let _ = started.send(());
                resume.recv().map_err(|_| StoreError::Unconfirmed)?;
                let tx = db.transaction()?;
                tx.execute(
                    "INSERT INTO deliveries(kind) VALUES('actual_started_work')",
                    [],
                )?;
                tx.execute("INSERT INTO missing_cancelled_write_table VALUES(1)", [])?;
                tx.commit()?;
                Ok(())
            })?
            .await
    });
    begin.await.expect("original SQL closure began");
    caller.abort();
    let _ = caller.await;
    let mut closing = Box::pin(log.close());
    assert!(futures::poll!(closing.as_mut()).is_pending());
    drop(closing);
    release.send(()).expect("release original SQL");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if log
                .inner
                .database
                .try_lock()
                .is_ok_and(|owner| owner.is_none())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("database owner must close without another waiting caller");
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    let count = reopened
        .call(|db| {
            Ok(db.query_row(
                "SELECT count(*) FROM deliveries WHERE kind='actual_started_work'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })?
        .await?;
    assert_eq!(count, 0, "failed transaction really rolled back");
    assert_eq!(
        log.write_failure_count(),
        1,
        "the cancelled waiter did not hide the actual error"
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), log.write_failed())
        .await
        .expect("actual failure wakes the host");
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn application_sessions_keep_original_types_and_do_not_merge_scopes_or_ambiguity()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sessions.sqlite");
    let log = BusinessLog::open_path(&path)?;
    let f = Fixture::new();
    let mut refs = Vec::new();
    for i in 0..10u64 {
        let mut p = f.protocol.clone();
        p.id.exchange.sequence = i;
        let source = Arc::make_mut(&mut p.source);
        source.side = ProtocolSide::ClientRequest;
        source.actor.process = Some(as_types::process_tree::ProcessKey {
            boot_id: [1; 16],
            host_tgid: 50,
            birth_boottime_ns: 1,
        });
        source.actual_auth_scope = Some([2; 32]);
        p.request_session = Some(SessionObservation {
            field: SessionField::Header {
                name: "session-id".into(),
            },
            values: vec![SessionValue::Header(b" A ".to_vec())],
            issue: None,
        });
        match i {
            2 => {
                p.request_session.as_mut().expect("session").values =
                    vec![SessionValue::Header(b"B".to_vec())]
            }
            3 => {
                source
                    .actor
                    .process
                    .as_mut()
                    .expect("process")
                    .birth_boottime_ns = 2
            }
            4 => source.actual_auth_scope = Some([3; 32]),
            5 => {
                let session = p.request_session.as_mut().expect("session");
                session.values.push(SessionValue::Header(b"C".to_vec()));
                session.issue = Some(SessionCaptureIssue::MultipleValues);
            }
            6 => source.actual_auth_scope = None,
            7 => {
                p.request_session.as_mut().expect("session").values =
                    vec![SessionValue::Header(vec![])]
            }
            8 => {
                p.request_session = Some(SessionObservation {
                    field: SessionField::JsonPointer {
                        pointer: "/session".into(),
                    },
                    values: vec![SessionValue::Json(NativeIdValue::JsonString("7".into()))],
                    issue: None,
                })
            }
            9 => {
                p.request_session = Some(SessionObservation {
                    field: SessionField::JsonPointer {
                        pointer: "/session".into(),
                    },
                    values: vec![SessionValue::Json(NativeIdValue::JsonNumber("7".into()))],
                    issue: None,
                })
            }
            _ => (),
        }
        refs.push(p.id);
        let mut message = f.message.clone();
        message.id.exchange = p.id.exchange;
        message.source = p.source.clone();
        message.record_refs = vec![p.id];
        chat(&log, i * 2, ChatEvent::Observation(Arc::new(p))).await?;
        chat(&log, i * 2 + 1, ChatEvent::Finished(Arc::new(message))).await?;
    }
    let (sessions, next) = log.chat_sessions(None, 64).await?;
    assert_eq!(sessions.len(), 6);
    assert_eq!(next, None);
    let a = sessions
        .iter()
        .find(|s| {
            s.reference()
                == &crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[0] }
        })
        .expect("first A");
    let crate::business_log::sessions::ChatSession::ModelHttp {
        exchanges, session, ..
    } = a
    else {
        panic!("model session");
    };
    assert_eq!(*exchanges, 2);
    assert_eq!(session.values, vec![SessionValue::Header(b" A ".to_vec())]);
    let (exchanges, _) = log
        .chat_exchanges(
            Some(crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[0] }),
            None,
            64,
        )
        .await?;
    assert_eq!(exchanges, vec![refs[1].exchange, refs[0].exchange]);
    assert!(matches!(
        log.chat_exchanges(
            Some(crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[0] }),
            Some(refs[2].exchange),
            64
        )
        .await,
        Err(StoreError::Invalid)
    ));
    for reference in &refs[5..8] {
        assert!(matches!(
            log.chat_exchanges(
                Some(crate::business_log::sessions::ChatSessionRef::ModelHttp {
                    record: *reference
                }),
                None,
                64
            )
            .await,
            Err(StoreError::NotProvided)
        ));
    }
    let (first, next) = log.chat_sessions(None, 1).await?;
    assert_eq!(
        first[0].reference(),
        &crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[9] }
    );
    assert_eq!(
        next,
        Some(crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[9] })
    );
    assert_eq!(
        log.chat_sessions(next, 1).await?.0[0].reference(),
        &crate::business_log::sessions::ChatSessionRef::ModelHttp { record: refs[8] }
    );
    log.close().await?;
    let reopened = BusinessLog::open_path(&path)?;
    assert_eq!(reopened.chat_sessions(None, 64).await?.0.len(), 6);
    // Ambiguous original values survive reopen even though they cannot name a confirmed session.
    let view = reopened.activity(FactReference::Protocol(refs[5])).await?;
    assert!(view.source_records.iter().any(|r|matches!(r,SourceRecord::Protocol(p) if p.request_session.as_ref().is_some_and(|s|s.values.len()==2))));
    Ok(())
}

#[tokio::test]
async fn ordinary_messages_do_not_displace_recent_process_tool_candidates() -> Result<(), StoreError>
{
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("candidate-limit.sqlite"))?;
    let mut f = Fixture::new();
    let process = as_types::process_tree::ProcessKey {
        boot_id: [8; 16],
        host_tgid: 42,
        birth_boottime_ns: 7,
    };
    Arc::make_mut(&mut f.protocol.source).actor.process = Some(process);
    f.message.source = f.protocol.source.clone();
    chat(
        &log,
        1,
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
    )
    .await?;
    chat(&log, 2, ChatEvent::Finished(Arc::new(f.message.clone()))).await?;
    for index in 1..=21 {
        let mut ordinary = f.message.clone();
        ordinary.id.sequence = 10 + index;
        ordinary.record_refs.clear();
        ordinary.observed_parts = vec![ChatPart::Text {
            id: 0,
            kind: TextKind::Plain,
            text: format!("ordinary response {index}"),
        }];
        ordinary.first_seen_boottime_ns = index;
        ordinary.last_seen_boottime_ns = index;
        chat(&log, 2 + index, ChatEvent::Finished(Arc::new(ordinary))).await?;
    }
    let mut original = serde_json::to_value(f.completion.event.as_ref())?;
    original["header"]["stage"] = serde_json::json!("controlled_request");
    original["header"]["source_boottime_ns"] = row::number(2000);
    original["header"]["actor_tgid"] = serde_json::json!(42);
    original["header"]["actor_birth_boottime_ns"] = row::number(7);
    let mut event: as_types::dispatch_event::EventData = serde_json::from_value(original)?;
    assert!(event.bind_host_boot([8; 16]));
    let event_key = EventKey::from(&event);
    let question = DecisionId([20; 16]);
    log.record_up(Arc::new(Up::Ask {
        decision_id: question,
        event: Arc::new(event),
        hit: f.completion.hit.clone().ok_or(StoreError::Invalid)?,
        timeout: Decision::Allow,
        remaining_ms: 100,
        choices: vec![Decision::Allow],
    }))?
    .await?;
    let read = log.read_ask_context(question, None).await?;
    assert!(read.call.is_err());
    assert!(read.window.is_none());
    assert_eq!(read.candidate_windows.len(), 1);
    assert!(read.view.relations.iter().any(|relation| relation.kind
        == RelationKind::ToolReferenceCandidate
        && relation.from == FactReference::Part(f.call)
        && relation.to == FactReference::Event(event_key)));
    assert!(
        !read
            .view
            .relations
            .iter()
            .any(|relation| relation.kind == RelationKind::ToolExecution)
    );
    Ok(())
}

#[tokio::test]
async fn shell_events_read_same_container_tool_candidates_without_inventing_ancestry()
-> Result<(), StoreError> {
    use as_types::workload::{ContainerInstance, Workload};
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("container-candidates.sqlite"))?;
    let instance = ContainerInstance {
        runtime: "docker".into(),
        namespace: None,
        id: "a".repeat(64),
    };
    let mut expected = Vec::new();
    for index in 1..=4 {
        let mut fixture = Fixture::new();
        fixture.protocol.id.exchange.sequence = index;
        let source = Arc::make_mut(&mut fixture.protocol.source);
        source.actor.process = Some(as_types::process_tree::ProcessKey {
            boot_id: [8; 16],
            host_tgid: 42,
            birth_boottime_ns: 7,
        });
        source.actor.info.workload = Some(Workload::Container(if index == 3 {
            ContainerInstance {
                id: "b".repeat(64),
                ..instance.clone()
            }
        } else {
            instance.clone()
        }));
        fixture.message.source = fixture.protocol.source.clone();
        fixture.message.id.exchange = fixture.protocol.id.exchange;
        fixture.message.record_refs = vec![fixture.protocol.id];
        fixture.message.last_seen_boottime_ns = if index == 4 { 3000 } else { 100 + index };
        if index == 2 {
            for part in &mut fixture.message.observed_parts {
                if let ChatPart::ToolCall { native_id, .. } = part {
                    *native_id = None;
                }
            }
        }
        if index <= 2 {
            expected.push(ChatPartRef {
                message: fixture.message.id,
                part_id: fixture.call.part_id,
            });
        }
        chat(
            &log,
            index * 2,
            ChatEvent::Observation(Arc::new(fixture.protocol)),
        )
        .await?;
        chat(
            &log,
            index * 2 + 1,
            ChatEvent::Finished(Arc::new(fixture.message)),
        )
        .await?;
    }
    let fixture = Fixture::new();
    let mut original = serde_json::to_value(fixture.completion.event.as_ref())?;
    original["header"]["stage"] = serde_json::json!("controlled_request");
    original["header"]["source_boottime_ns"] = row::number(2000);
    original["header"]["actor_tgid"] = serde_json::json!(900);
    original["header"]["actor_birth_boottime_ns"] = row::number(1000);
    let mut event: as_types::dispatch_event::EventData = serde_json::from_value(original)?;
    event.process.parent_pid = Some(901);
    event.process.parent_birth_boottime_ns = Some(900);
    event.process.workload = Some(Workload::Container(instance));
    assert!(event.bind_host_boot([8; 16]));
    let key = EventKey::from(&event);
    let question = DecisionId([23; 16]);
    log.record_up(Arc::new(Up::Ask {
        decision_id: question,
        event: Arc::new(event),
        hit: fixture.completion.hit.ok_or(StoreError::Invalid)?,
        timeout: Decision::Allow,
        remaining_ms: 100,
        choices: vec![Decision::Allow],
    }))?
    .await?;
    let read = log.read_ask_context(question, None).await?;
    let links = read
        .view
        .relations
        .iter()
        .filter(|link| link.kind == RelationKind::ToolContainerCandidate)
        .collect::<Vec<_>>();
    assert_eq!(links.len(), 2);
    for call in expected {
        assert!(
            links
                .iter()
                .any(|link| link.from == FactReference::Part(call)
                    && link.to == FactReference::Event(key))
        );
    }
    assert_eq!(read.candidate_windows.len(), 2);
    assert!(read.call.is_err() && read.window.is_none());
    assert!(
        read.view
            .relations
            .iter()
            .all(|link| link.kind != RelationKind::ToolExecution)
    );
    log.close().await?;
    Ok(())
}

#[tokio::test]
async fn process_scope_candidates_and_late_reported_results_need_no_execution_publisher()
-> Result<(), StoreError> {
    let dir = tempfile::tempdir()?;
    let log = BusinessLog::open_path(&dir.path().join("candidate.sqlite"))?;
    let mut f = Fixture::new();
    let process = as_types::process_tree::ProcessKey {
        boot_id: [8; 16],
        host_tgid: 42,
        birth_boottime_ns: 7,
    };
    let mut source = (*f.protocol.source).clone();
    source.actor.process = Some(process);
    source.actual_auth_scope = Some([3; 32]);
    f.protocol.source = Arc::new(source);
    f.message.source = f.protocol.source.clone();
    chat(
        &log,
        1,
        ChatEvent::Observation(Arc::new(f.protocol.clone())),
    )
    .await?;
    chat(&log, 2, ChatEvent::Finished(Arc::new(f.message.clone()))).await?;
    let mut original = serde_json::to_value(f.completion.event.as_ref())?;
    original["header"]["stage"] = serde_json::json!("controlled_request");
    original["header"]["source_boottime_ns"] = row::number(2000);
    original["header"]["actor_tgid"] = serde_json::json!(42);
    original["header"]["actor_birth_boottime_ns"] = row::number(7);
    let mut event: as_types::dispatch_event::EventData = serde_json::from_value(original)?;
    assert!(event.bind_host_boot([8; 16]));
    let event = Arc::new(event);
    let event_key = EventKey::from(event.as_ref());
    let question_id = DecisionId([19; 16]);
    log.record_up(Arc::new(Up::Ask {
        decision_id: question_id,
        event,
        hit: f.completion.hit.clone().ok_or(StoreError::Invalid)?,
        timeout: Decision::Allow,
        remaining_ms: 100,
        choices: vec![Decision::Allow],
    }))?
    .await?;
    let before = log.read_ask_context(question_id, None).await?;
    assert!(before.call.is_err());
    assert!(before.window.is_none());
    assert_eq!(before.candidate_windows.len(), 1);
    let window = before.candidate_windows[0].0.message_refs.clone();
    assert!(before.view.relations.iter().any(|relation| relation.kind
        == RelationKind::ToolReferenceCandidate
        && relation.from == FactReference::Part(f.call)
        && relation.to == FactReference::Event(event_key)));
    assert!(
        !before
            .view
            .relations
            .iter()
            .any(|relation| relation.kind == RelationKind::ToolExecution)
    );

    let ChatPart::ToolCall {
        native_id: Some(call_id),
        ..
    } = &f.message.observed_parts[0]
    else {
        return Err(StoreError::Invalid);
    };
    let mut result_id = call_id.clone();
    result_id.namespace = NativeIdNamespace::ToolResultReference;
    result_id.field_path = "/input/0/call_id".into();
    let mut result_protocol = f.protocol.clone();
    result_protocol.id.exchange.sequence += 1;
    Arc::make_mut(&mut result_protocol.source).side = ProtocolSide::ClientRequest;
    result_protocol.native_ids = vec![result_id.clone()];
    let content = ChatContent::Json(serde_json::json!({"passed":true,"empty":[],"zero":0}));
    result_protocol.body = EvidenceBody::from_bytes(serde_json::to_vec(
        &serde_json::json!({"input":[{"type":"function_call_output","call_id":"original-call","output":{"passed":true,"empty":[],"zero":0}}]}),
    )?);
    let mut result = f.message.clone();
    result.id.exchange = result_protocol.id.exchange;
    result.id.sequence = 0;
    result.source = result_protocol.source.clone();
    result.origin = ChatOrigin::RequestContext;
    result.role = ChatRole::Tool;
    result.record_refs = vec![result_protocol.id];
    result.observed_parts = vec![ChatPart::ToolResult {
        id: 0,
        native_id: Some(result_id),
        content: content.clone(),
    }];
    let result_ref = ChatPartRef {
        message: result.id,
        part_id: 0,
    };
    chat(&log, 3, ChatEvent::Observation(Arc::new(result_protocol))).await?;
    chat(&log, 4, ChatEvent::Finished(Arc::new(result))).await?;
    let after = log.read_ask_context(question_id, None).await?;
    assert_eq!(after.candidate_windows[0].0.message_refs, window);
    assert!(after.view.relations.iter().any(|relation| relation.kind
        == RelationKind::ToolReferenceCandidate
        && relation.from == FactReference::Part(f.call)
        && relation.to == FactReference::Part(result_ref)));
    assert!(after.view.source_records.iter().any(|record| matches!(record, SourceRecord::Message(message) if message.id == result_ref.message && message.observed_parts.iter().any(|part|matches!(part,ChatPart::ToolResult{content:actual,..}if actual==&content)))));
    assert!(
        !after
            .view
            .relations
            .iter()
            .any(|relation| relation.kind == RelationKind::ToolExecution)
    );
    log.close().await?;
    Ok(())
}
