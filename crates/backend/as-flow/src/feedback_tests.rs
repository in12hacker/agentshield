use super::*;
use as_types::{
    dispatch_event::EventData,
    main_ports::{
        ChoiceReason, Decision, DispatchOutcome, MainNotice, RuleChoice, RuleHit,
        RuleSourceVersion, Termination,
    },
    operation::{ExecutionOutcome, TerminationReport},
    process_tree::EventKey,
};
use serde_json::Value;
use std::num::NonZeroU32;
fn completion(sequence: u64, text: Option<&str>, outcome: ExecutionOutcome) -> Arc<MainNotice> {
    let event:EventData=serde_json::from_value(serde_json::json!({"header":{"kind":"process","stage":"controlled_request","operation":1,"source_instance":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"sequence_domain":"1","sequence":sequence.to_string(),"source_boottime_ns":null,"actor_tgid":123,"actor_tid":123,"actor_birth_boottime_ns":"1","actor_thread_birth_boottime_ns":"1","actor_cgroup_id":null},"process":ProcessInfo::default(),"payload":as_types::dispatch_event::ProcessData::default()})).expect("source event");
    Arc::new(MainNotice {
        decision_id: None,
        event: Arc::new(event),
        choice: Some(RuleChoice {
            decision: Decision::Deny {
                terminate: Termination::None,
            },
            reason: ChoiceReason::Direct,
            hit: Some(RuleHit {
                group_id: "g".into(),
                group_number: NonZeroU32::new(1).expect("number"),
                rule_id: "deny".into(),
                rule_number: NonZeroU32::new(1).expect("number"),
                source: RuleSourceVersion::RulesDocument(as_types::RuleVersion::from_digest(
                    [1; 32],
                )),
                log: false,
                deny_message: text.map(str::to_owned),
            }),
        }),
        result: Ok(DispatchOutcome::Driver {
            outcome,
            termination: TerminationReport::NONE,
        }),
    })
}
fn route() -> ProviderRoute {
    ProviderRoute {
        id: RouteId([7; 16]),
        origin: "https://example.test".into(),
        method: "POST".into(),
        path: "/v1/responses".into(),
        profile: ProviderProfile::OpenAiResponses,
        session: None,
    }
}
fn observer(shared: &Arc<Shared>, side: ProtocolSide, exchange: ExchangeKey) -> observe::Observer {
    observe::Observer::new(
        shared.clone(),
        Arc::new(facts(shared.version, 443)),
        exchange,
        route(),
        side,
        None,
        Some([7; 32]),
        &http::HeaderMap::new(),
        Some(200),
    )
}
async fn original(
    shared: &Arc<Shared>,
    events: &mut ChatReceiver,
) -> (Arc<ChatMessage>, Vec<Arc<ProtocolObservation>>) {
    original_arguments(shared, events, "{}").await
}
async fn original_arguments(
    shared: &Arc<Shared>,
    events: &mut ChatReceiver,
    arguments: &str,
) -> (Arc<ChatMessage>, Vec<Arc<ProtocolObservation>>) {
    let exchange = shared.next_exchange().expect("exchange");
    let mut parser = observer(shared, ProtocolSide::UpstreamResponse, exchange);
    let body = serde_json::json!({"id":"resp_A","conversation":"conv_C","status":"completed","output":[{"type":"function_call","id":"fc","call_id":"call_X","name":"fixture_tool","arguments":arguments}]});
    parser
        .feed(&serde_json::to_vec(&body).expect("body"))
        .await
        .expect("capture");
    parser.finish(true).await.expect("finish");
    let mut message = None;
    let mut records = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event.event.as_ref() {
            ChatEvent::Finished(m) if m.id.exchange == exchange => message = Some(m.clone()),
            ChatEvent::Observation(o) if o.id.exchange == exchange => records.push(o.clone()),
            _ => {}
        }
    }
    (message.expect("original call"), records)
}
fn input(
    message: Arc<ChatMessage>,
    records: Vec<Arc<ProtocolObservation>>,
    notice: Arc<MainNotice>,
    token: CancellationToken,
) -> RuleFeedbackPrepareInput {
    let part_id = message
        .observed_parts
        .iter()
        .find_map(|p| {
            if let ChatPart::ToolCall { id, .. } = p {
                Some(*id)
            } else {
                None
            }
        })
        .expect("part");
    RuleFeedbackPrepareInput {
        notice,
        call: ChatPartRef {
            message: message.id,
            part_id,
        },
        message,
        observations: records,
        cancelled: token,
        retention_guard: None,
    }
}
async fn prepared(
    shared: &Arc<Shared>,
    events: &mut ChatReceiver,
    token: CancellationToken,
    retention_guard: Option<Box<dyn Send + Sync>>,
) -> (EventKey, ChatPartRef) {
    let (message, records) = original(shared, events).await;
    let notice = completion(9, Some("fixed denial"), ExecutionOutcome::Denied);
    let denial = EventKey::from(notice.event.as_ref());
    let mut request = input(message, records, notice, token);
    request.retention_guard = retention_guard;
    let call = request.call;
    assert_eq!(
        feedback::Port(shared.clone())
            .prepare(request)
            .await
            .expect("prepared")
            .state,
        RuleFeedbackState::Prepared
    );
    (denial, call)
}
async fn finish_tasks(shared: &Arc<Shared>) {
    shared.feedback_stop.cancel();
    shared.tasks.close();
    shared.tasks.wait().await;
}
#[tokio::test]
async fn feedback_prepare_cancel_releases_pending_entry_before_reusing_its_key() {
    use futures::poll;

    let (shared, mut events) = protocol_fixture();
    let (message, records) = original(&shared, &mut events).await;
    let notice = completion(1, Some("fixed"), ExecutionOutcome::Denied);
    let old_token = CancellationToken::new();
    let first = input(
        message.clone(),
        records.clone(),
        notice.clone(),
        old_token.clone(),
    );
    let occupied = PreparedChatEvent::new(ChatEvent::RuleFeedback(RuleFeedbackFact {
        denial: EventKey::from(notice.event.as_ref()),
        call: first.call,
        state: RuleFeedbackState::Prepared,
    }))
    .expect("bounded fixture record");
    let (output, mut queued) = ChatSender::channel(occupied.charged_bytes() as u32);
    let occupied = occupied.into_envelope(ChatCursor {
        service_instance: shared.instance,
        sequence: 0,
    });
    output.send(&occupied).await.expect("occupy output");
    *shared.output.lock().expect("output") = Some(output);

    let port = feedback::Port(shared.clone());
    let mut preparing = Box::pin(port.prepare(first));
    assert!(poll!(preparing.as_mut()).is_pending());
    assert_eq!(shared.feedback.lock().expect("pending").active_count(), 1);
    drop(preparing);
    assert_eq!(shared.feedback.lock().expect("cancelled").active_count(), 0);

    // Model an owner removing an old entry before that caller is dropped.
    // Its delayed cleanup must not remove a different Arc with the same key.
    let mut stale = Box::pin(port.prepare(input(
        message.clone(),
        records.clone(),
        notice.clone(),
        old_token.clone(),
    )));
    assert!(poll!(stale.as_mut()).is_pending());
    shared.feedback.lock().expect("old removal").clear();
    let token = CancellationToken::new();
    let mut replacement = Box::pin(port.prepare(input(message, records, notice, token.clone())));
    assert!(poll!(replacement.as_mut()).is_pending());
    drop(stale);
    assert_eq!(
        shared.feedback.lock().expect("replacement").active_count(),
        1
    );
    assert!(Arc::ptr_eq(
        &queued.try_recv().expect("original queued record"),
        occupied.envelope()
    ));
    assert!(
        queued.try_recv().is_err(),
        "no fabricated preparation or terminal"
    );

    replacement
        .await
        .expect("the same denial/call can be prepared after cancellation");
    old_token.cancel();
    tokio::task::yield_now().await;
    assert_eq!(shared.feedback.lock().expect("new entry").active_count(), 1);
    assert!(matches!(
        queued.try_recv().expect("real preparation").event.as_ref(),
        ChatEvent::RuleFeedback(RuleFeedbackFact {
            state: RuleFeedbackState::Prepared,
            ..
        })
    ));
    assert!(
        queued.try_recv().is_err(),
        "old cancellation cannot finish the new entry"
    );
    token.cancel();
    finish_tasks(&shared).await;
    assert_eq!(shared.feedback.lock().expect("finished").active_count(), 0);
}
#[tokio::test]
async fn feedback_keeps_only_unfinished_context_and_releases_sources_and_terminal() {
    let (shared, mut events) = protocol_fixture();
    let (message, records) = original(&shared, &mut events).await;
    let notice = completion(1, Some("frozen"), ExecutionOutcome::Denied);
    let weak_message = Arc::downgrade(&message);
    let weak_record = Arc::downgrade(&records[0]);
    let weak_notice = Arc::downgrade(&notice);
    let token = CancellationToken::new();
    let retained = Arc::new(());
    let weak_retained = Arc::downgrade(&retained);
    let mut request = input(message, records, notice, token.clone());
    request.retention_guard = Some(Box::new(retained));
    feedback::Port(shared.clone())
        .prepare(request)
        .await
        .expect("prepared");
    assert!(weak_message.upgrade().is_none());
    assert!(weak_record.upgrade().is_none());
    assert!(weak_notice.upgrade().is_none(), "no completed-event cache");
    assert_eq!(shared.feedback.lock().expect("pending").active_count(), 1);
    assert!(
        weak_retained.upgrade().is_some(),
        "Prepared retains the original business custody"
    );
    token.cancel();
    finish_tasks(&shared).await;
    assert!(
        weak_retained.upgrade().is_none(),
        "actual cancelled transaction releases custody"
    );
    assert_eq!(shared.feedback.lock().expect("pending").active_count(), 0);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).any(|e| matches!(
            e.event.as_ref(),
            ChatEvent::RuleFeedback(RuleFeedbackFact {
                state: RuleFeedbackState::Failed {
                    code: RuleFeedbackFailure::Cancelled,
                    ..
                },
                ..
            })
        ))
    );
    let (shared, mut events) = protocol_fixture();
    let retained = Arc::new(());
    let weak_retained = Arc::downgrade(&retained);
    prepared(
        &shared,
        &mut events,
        CancellationToken::new(),
        Some(Box::new(retained)),
    )
    .await;
    assert!(weak_retained.upgrade().is_some());
    shared.feedback_stop.cancel();
    shared.tasks.close();
    shared.tasks.wait().await;
    assert!(
        weak_retained.upgrade().is_none(),
        "SourceEnd releases custody"
    );
    assert_eq!(shared.feedback.lock().expect("pending").active_count(), 0);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).any(|e| matches!(
            e.event.as_ref(),
            ChatEvent::RuleFeedback(RuleFeedbackFact {
                state: RuleFeedbackState::Failed {
                    code: RuleFeedbackFailure::SourceUnavailable,
                    ..
                },
                ..
            })
        ))
    );
}
#[tokio::test]
async fn feedback_validates_denial_and_rejects_unanchored_or_conflicting_outputs() {
    for result in [ExecutionOutcome::Unknown, ExecutionOutcome::Allowed] {
        let (shared, mut events) = protocol_fixture();
        let (m, r) = original(&shared, &mut events).await;
        let request = input(
            m,
            r,
            completion(1, Some("fixed"), result),
            CancellationToken::new(),
        );
        assert!(matches!(
            feedback::Port(shared).prepare(request).await,
            Err(FlowError::InvalidInput)
        ));
    }
    let (shared, mut events) = protocol_fixture();
    let token = CancellationToken::new();
    let (denial, _) = prepared(&shared, &mut events, token.clone(), None).await;
    for body in [br#"{"previous_response_id":"resp_B","conversation":"conv_C","input":[{"type":"function_call_output","call_id":"call_X","output":"original"}]}"#.as_slice(),br#"{"conversation":"conv_C","input":[{"type":"function_call_output","call_id":"call_X","output":"original"}]}"#.as_slice()]{
        let exchange=shared.next_exchange().expect("exchange");let mut parser=observer(&shared,ProtocolSide::ClientRequest,exchange);parser.feed(body).await.expect("original");let record=parser.finish_record(true).await.expect("finish").expect("record");
        assert!(shared.modify_feedback(record).await.expect("matching").is_none());
    }
    let exchange = shared.next_exchange().expect("exchange");
    let mut parser = observer(&shared, ProtocolSide::ClientRequest, exchange);
    parser.unmodified_submission(RuleFeedbackFailure::TooLate);
    parser.feed(br#"{"previous_response_id":"resp_A","input":[{"type":"function_call_output","call_id":"call_X","output":"original"}]}"#).await.expect("original request");
    let original = parser
        .finish_record(true)
        .await
        .expect("finish")
        .expect("decoded")
        .observation
        .id;
    finish_tasks(&shared).await;
    assert!(std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
        event.event.as_ref(),
        ChatEvent::RuleFeedback(RuleFeedbackFact {
            denial: event_denial,
            state: RuleFeedbackState::Failed { original: Some(record), code: RuleFeedbackFailure::TooLate },
            ..
        }) if *event_denial == denial && *record == original
    )));
}
#[tokio::test]
async fn feedback_stateless_replay_checks_complete_original_fields_and_unmodified_retirement() {
    let original = serde_json::json!({"input":[
        {"type":"function_call","id":"fc","call_id":"call_X","name":"fixture_tool","arguments":"{}"},
        {"type":"function_call_output","call_id":"call_X","output":"original","is_error":false}
    ]});
    let mut cases = vec![(original.clone(), true)];
    let mut missing_optional = original.clone();
    missing_optional["input"][0]
        .as_object_mut()
        .expect("call")
        .remove("id");
    cases.push((missing_optional, true));
    let mut nullable = original.clone();
    nullable["previous_response_id"] = Value::Null;
    cases.push((nullable, true));
    for (field, value) in [
        ("name", serde_json::json!("other")),
        ("arguments", serde_json::json!("{\"other\":1}")),
        ("id", serde_json::json!("another-item")),
        ("call_id", serde_json::json!(0)),
    ] {
        let mut changed = original.clone();
        changed["input"][0][field] = value;
        cases.push((changed, false));
    }
    for value in [serde_json::json!("resp_other"), serde_json::json!(false)] {
        let mut changed = original.clone();
        changed["previous_response_id"] = value;
        cases.push((changed, false));
    }
    let mut partial = original.clone();
    partial["input"][0]
        .as_object_mut()
        .expect("call")
        .remove("arguments");
    cases.push((partial, false));
    let mut duplicate = original.clone();
    let call = duplicate["input"][0].clone();
    duplicate["input"]
        .as_array_mut()
        .expect("items")
        .insert(0, call);
    cases.push((duplicate, false));
    let mut duplicate = original.clone();
    let output = duplicate["input"][1].clone();
    duplicate["input"]
        .as_array_mut()
        .expect("items")
        .push(output);
    cases.push((duplicate, false));
    for (body, expected) in cases {
        let (shared, mut events) = protocol_fixture();
        prepared(&shared, &mut events, CancellationToken::new(), None).await;
        let exchange = shared.next_exchange().expect("exchange");
        let mut parser = observer(&shared, ProtocolSide::ClientRequest, exchange);
        parser
            .feed(&serde_json::to_vec(&body).expect("body"))
            .await
            .expect("observed");
        let request = parser
            .finish_record(true)
            .await
            .expect("finished")
            .expect("parsed");
        let modified = shared.modify_feedback(request).await.expect("feedback");
        assert_eq!(modified.is_some(), expected, "original request {body}");
        if let Some(mut modified) = modified {
            let received: Value = serde_json::from_slice(&modified.bytes).expect("changed body");
            assert_eq!(received["input"][0], body["input"][0]);
            assert_eq!(received["input"][1]["output"], "original\nfixed denial");
            assert_eq!(received["input"][1]["is_error"], false);
            modified.guard.failed(RuleFeedbackFailure::TransportError); // No transport submission in this unit case.
        }
        finish_tasks(&shared).await;
    }
    let (shared, mut events) = protocol_fixture();
    let (denial, _) = prepared(&shared, &mut events, CancellationToken::new(), None).await;
    let exchange = shared.next_exchange().expect("exchange");
    let mut parser = observer(&shared, ProtocolSide::ClientRequest, exchange);
    parser.unmodified_submission(RuleFeedbackFailure::TooLate);
    parser
        .feed(&serde_json::to_vec(&original).expect("body"))
        .await
        .expect("observed");
    let original_id = parser
        .finish_record(true)
        .await
        .expect("finished")
        .expect("parsed")
        .observation
        .id;
    finish_tasks(&shared).await;
    assert!(std::iter::from_fn(||events.try_recv().ok()).any(|envelope|matches!(envelope.event.as_ref(),ChatEvent::RuleFeedback(RuleFeedbackFact { denial:key,state:RuleFeedbackState::Failed { original:Some(record),code:RuleFeedbackFailure::TooLate },.. }) if *key==denial&&*record==original_id)));
}

#[tokio::test]
async fn feedback_uses_only_profile_fields_and_strict_original_arguments() {
    let bodies = [
        serde_json::json!({"input":[{"type":"function_call","id":"fc","call_id":"call_X","name":"fixture_tool","arguments":"{\"x\":1,\"x\":2}"},{"type":"function_call_output","call_id":"call_X","output":"original"}]}),
        serde_json::json!({"input":[{"type":"function_call","id":"fc","call_id":"call_X","name":"fixture_tool","arguments":"{\"x\":1}"},{"type":"function_call_output","call_id":"call_X","output":"original"}],"messages":[{"type":"function_call","id":"fc","call_id":"call_X","name":"fixture_tool","arguments":"{\"x\":2}"},{"type":"function_call_output","call_id":"call_X","output":"original"}]}),
    ];
    for body in bodies {
        let (shared, mut events) = protocol_fixture();
        let (message, records) = original_arguments(&shared, &mut events, "{\"x\":2}").await;
        feedback::Port(shared.clone())
            .prepare(input(
                message,
                records,
                completion(9, Some("fixed"), ExecutionOutcome::Denied),
                CancellationToken::new(),
            ))
            .await
            .expect("prepared");
        let exchange = shared.next_exchange().expect("exchange");
        let mut parser = observer(&shared, ProtocolSide::ClientRequest, exchange);
        parser
            .feed(&serde_json::to_vec(&body).expect("body"))
            .await
            .expect("observe");
        let request = parser
            .finish_record(true)
            .await
            .expect("finish")
            .expect("parsed");
        assert!(
            shared
                .modify_feedback(request)
                .await
                .expect("match")
                .is_none(),
            "{body}"
        );
        finish_tasks(&shared).await;
    }
}

#[tokio::test]
async fn feedback_real_tls_h1_h2_fixed_text_once_and_original_evidence() {
    for (h2, stateless) in [(false, false), (true, false), (false, true), (true, true)] {
        let (flow, _creds, phantom, tls, hits, upstream, _temp, _source, mut events) =
            setup_profile(h2, true, ProviderProfile::OpenAiResponses).await;
        let origin = flow.shared.settings.provider_routes[0].origin.clone();
        let feedback = flow.feedback_port();
        enum Sender {
            H1(hyper::client::conn::http1::SendRequest<Full<Bytes>>),
            H2(hyper::client::conn::http2::SendRequest<Full<Bytes>>),
        }
        let mut sender = if h2 {
            let (s, c) =
                hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
                    .await
                    .expect("h2");
            tokio::spawn(async move {
                let _ = c.await;
            });
            Sender::H2(s)
        } else {
            let (s, c) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
                .await
                .expect("h1");
            tokio::spawn(async move {
                let _ = c.await;
            });
            Sender::H1(s)
        };
        for turn in 0..3 {
            let body = if turn == 0 {
                br#"{"input":"first"}"#.to_vec()
            } else if stateless {
                br#"{"input":[{"type":"function_call","id":"fc_feedback","call_id":"call_feedback","name":"fixture_tool","arguments":"{}"},{"type":"function_call_output","call_id":"call_feedback","output":"original result","is_error":true}]}"#.to_vec()
            } else {
                br#"{"previous_response_id":"resp_feedback","input":[{"type":"function_call_output","call_id":"call_feedback","output":"original result","is_error":true}]}"#.to_vec()
            };
            let request = Request::builder()
                .method("POST")
                .uri(format!("{origin}/v1/responses"))
                .header("host", origin.strip_prefix("https://").expect("origin"))
                .header(
                    "authorization",
                    format!(
                        "Bearer {}",
                        std::str::from_utf8(phantom.expose()).expect("phantom")
                    ),
                )
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(body.clone())))
                .expect("request");
            let response = match &mut sender {
                Sender::H1(s) => s.send_request(request).await,
                Sender::H2(s) => s.send_request(request).await,
            }
            .expect("response");
            assert_eq!(response.status(), 200);
            let received: serde_json::Value = serde_json::from_slice(
                &response
                    .into_body()
                    .collect()
                    .await
                    .expect("body")
                    .to_bytes(),
            )
            .expect("JSON");
            if turn == 0 {
                let mut message = None;
                let mut observations = Vec::new();
                while let Ok(e) = events.try_recv() {
                    match e.event.as_ref() {
                        ChatEvent::Finished(m) if m.origin == ChatOrigin::ResponseOutput => {
                            message = Some(m.clone())
                        }
                        ChatEvent::Observation(o)
                            if o.source.side == ProtocolSide::UpstreamResponse =>
                        {
                            observations.push(o.clone())
                        }
                        _ => {}
                    }
                }
                let message = message.expect("real call");
                assert!(message.source.actual_auth_scope.is_some());
                let request = input(
                    message,
                    observations,
                    completion(
                        42,
                        Some("Rule forbids this operation."),
                        ExecutionOutcome::Denied,
                    ),
                    CancellationToken::new(),
                );
                feedback
                    .prepare(request)
                    .await
                    .expect("explicit authenticated plan");
            } else {
                assert_eq!(
                    received["received"]["input"][usize::from(stateless)]["output"],
                    if turn == 1 {
                        "original result\nRule forbids this operation."
                    } else {
                        "original result"
                    }
                );
                assert_eq!(
                    received["received"]["input"][usize::from(stateless)]["call_id"],
                    "call_feedback"
                );
                assert_eq!(
                    received["received"]["input"][usize::from(stateless)]["is_error"],
                    true
                );
                if turn == 1 {
                    let mut raw = false;
                    let mut applied = None;
                    let mut submitted = None;
                    while submitted.is_none() {
                        let e = events.recv().await.expect("source");
                        match e.event.as_ref() {
                            ChatEvent::Observation(o)
                                if o.source.side == ProtocolSide::ClientRequest =>
                            {
                                for s in &o.body.segments {
                                    if let EvidenceSegment::Bytes { data, .. } = s {
                                        assert_eq!(data, &body);
                                        raw = true;
                                    }
                                }
                            }
                            ChatEvent::RuleFeedback(f) => match &f.state {
                                RuleFeedbackState::Applied {
                                    original,
                                    modified_body,
                                } => {
                                    applied = Some(*original);
                                    assert_ne!(
                                        modified_body.original_length,
                                        Some(body.len() as u64)
                                    );
                                }
                                RuleFeedbackState::Submitted { original } => {
                                    submitted = Some(*original)
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                    assert!(raw);
                    assert!(applied.is_some());
                    assert_eq!(applied, submitted);
                    assert_eq!(
                        flow.shared.feedback.lock().expect("pending").active_count(),
                        0
                    );
                }
            }
        }
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        drop(sender);
        upstream.abort();
        flow.stop().await.expect("stop");
    }
}

struct Sink {
    bytes: Arc<Mutex<Vec<u8>>>,
    flush: Arc<std::sync::atomic::AtomicBool>,
    maximum: usize,
}
impl tokio::io::AsyncRead for Sink {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
}
impl tokio::io::AsyncWrite for Sink {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut out = self.bytes.lock().expect("sink");
        let n = bytes.len().min(self.maximum.saturating_sub(out.len()));
        if n == 0 {
            return std::task::Poll::Pending;
        }
        out.extend_from_slice(&bytes[..n]);
        std::task::Poll::Ready(Ok(n))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.flush.load(Ordering::Acquire) {
            std::task::Poll::Ready(Ok(()))
        } else {
            std::task::Poll::Pending
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn feedback_actual_flush_and_partial_cancel_release_the_write_transaction() {
    use futures::FutureExt;
    use tokio::io::AsyncWriteExt;
    for partial in [false, true] {
        let (shared, mut events) = protocol_fixture();
        let token = CancellationToken::new();
        let retained = Arc::new(());
        let weak_retained = Arc::downgrade(&retained);
        let (denial, _call) = prepared(
            &shared,
            &mut events,
            token.clone(),
            Some(Box::new(retained)),
        )
        .await;
        let body=br#"{"previous_response_id":"resp_A","input":[{"type":"function_call_output","call_id":"call_X","output":"original"}]}"#;
        let exchange = shared.next_exchange().expect("exchange");
        let mut parser = observer(&shared, ProtocolSide::ClientRequest, exchange);
        parser.feed(body).await.expect("original");
        let record = parser
            .finish_record(true)
            .await
            .expect("finish")
            .expect("record");
        let modified = shared
            .modify_feedback(record)
            .await
            .expect("matching")
            .expect("modified");
        assert!(
            weak_retained.upgrade().is_some(),
            "Applied is still an unfinished write"
        );
        let length = modified.bytes.len();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let flush = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sink = Sink {
            bytes: bytes.clone(),
            flush: flush.clone(),
            maximum: if partial { 20 } else { usize::MAX },
        };
        let mut witness = feedback_write::Witness::new(sink, Some(modified.guard), length, false);
        let mut request =
            format!("POST /v1/responses HTTP/1.1\r\nContent-Length: {length}\r\n\r\n").into_bytes();
        request.extend_from_slice(&modified.bytes);
        let result = witness.write_all(&request).now_or_never();
        if partial {
            assert!(result.is_none());
            assert_eq!(bytes.lock().expect("bytes").len(), 20);
            token.cancel();
            drop(witness);
        } else {
            result.expect("ready").expect("write");
            assert!(witness.flush().now_or_never().is_none());
            assert_eq!(
                shared.feedback.lock().expect("pending").active_count(),
                1,
                "no submission before actual flush"
            );
            flush.store(true, Ordering::Release);
            witness.flush().await.expect("real flush");
            assert_eq!(shared.feedback.lock().expect("pending").active_count(), 0);
            drop(witness);
        }
        finish_tasks(&shared).await;
        assert!(
            weak_retained.upgrade().is_none(),
            "actual Submitted or partial-cancel releases custody"
        );
        assert_eq!(shared.feedback.lock().expect("pending").active_count(), 0);
        let mut terminal = None;
        while let Ok(e) = events.try_recv() {
            if let ChatEvent::RuleFeedback(f) = e.event.as_ref()
                && f.denial == denial
                && matches!(
                    f.state,
                    RuleFeedbackState::Submitted { .. } | RuleFeedbackState::Failed { .. }
                )
            {
                terminal = Some(f.state.clone());
            }
        }
        assert!(if partial {
            matches!(
                terminal,
                Some(RuleFeedbackState::Failed {
                    code: RuleFeedbackFailure::Cancelled,
                    ..
                })
            )
        } else {
            matches!(terminal, Some(RuleFeedbackState::Submitted { .. }))
        });
    }
}
