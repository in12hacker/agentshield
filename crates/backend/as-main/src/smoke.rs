//! Main wiring checks with explicit platform/media providers and the real rule factory.
use super::*;
use as_types::{
    DecisionId, RuleVersion,
    dispatch_event::{self, EventData, ProcessInfo},
    hold_budget::HoldWindow,
    main_ports::*,
    operation::*,
};
use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default, Debug)]
struct Smoke {
    trace: Arc<Mutex<Vec<(&'static str, thread::ThreadId)>>>,
    executed: Arc<AtomicUsize>,
    notices: Arc<AtomicUsize>,
    completions: Arc<Mutex<Vec<MainNotice>>>,
    logs: Arc<AtomicUsize>,
    logged_events: Arc<Mutex<Vec<std::sync::Weak<EventData>>>>,
    pending_logs: Arc<AtomicUsize>,
    log_errors: Arc<AtomicUsize>,
    chat_cursors: Arc<Mutex<Vec<as_types::flow::ChatCursor>>>,
    chat_entered: Option<Arc<tokio::sync::Notify>>,
    chat_gate: Option<Arc<tokio::sync::Notify>>,
    started: Option<Arc<tokio::sync::Notify>>,
    gate: Option<Arc<tokio::sync::Semaphore>>,
    expected_name: Option<Vec<u8>>,
    expected_argv: Option<Vec<Vec<u8>>>,
}
impl Smoke {
    fn mark(&self, name: &'static str) {
        self.trace
            .lock()
            .expect("trace")
            .push((name, thread::current().id()));
    }
    fn choice(decision: Decision) -> RuleChoice {
        RuleChoice {
            decision,
            reason: ChoiceReason::Direct,
            hit: Some(RuleHit {
                group_id: "smoke".into(),
                group_number: NonZeroU32::new(1).expect("positive"),
                rule_id: "smoke".into(),
                rule_number: NonZeroU32::new(1).expect("positive"),
                source: RuleSourceVersion::RulesDocument(RuleVersion::from_digest([1; 32])),
                log: false,
                deny_message: None,
            }),
        }
    }
    fn modules(&self) -> MainModules {
        let shared = Arc::new(self.clone());
        MainModules {
            rules: shared.clone(),
            fast_facts: shared.clone(),
            slow_facts: Some(shared.clone()),
            messages: shared,
        }
    }
}
impl RulesFactory for Smoke {
    fn select(&self, _: EventKind) -> Arc<dyn RulesModule> {
        Arc::new(self.clone())
    }
}
#[async_trait::async_trait]
impl RulesModule for Smoke {
    fn can_prompt(&self) -> bool {
        true
    }
    fn fast(&self, event: &EventData) -> Result<FastMatch, ModuleError> {
        self.mark("fast");
        if let Some(expected) = &self.expected_name {
            assert_eq!(event.process.name.as_ref(), Some(expected));
        }
        assert_eq!(
            event.process.parent_name.as_deref(),
            Some(b"parent".as_slice())
        );
        Ok(if event.header().sequence == 1 {
            FastMatch::Matched(Self::choice(Decision::Deny {
                terminate: Termination::None,
            }))
        } else {
            FastMatch::NoMatch
        })
    }
    async fn prompt(&self, request: ReviewRequest<'_>) -> Result<Option<RuleChoice>, ModuleError> {
        self.mark("prompt");
        let expected = self
            .expected_argv
            .clone()
            .unwrap_or_else(|| vec![b"slow".to_vec()]);
        assert_eq!(request.event.process.argv.as_ref(), Some(&expected));
        Ok((request.event.header().sequence == 2).then(|| Self::choice(Decision::Allow)))
    }
}
impl FastFacts for Smoke {
    fn source_gap(&self) {
        self.mark("source_gap");
    }
    fn finish_event(&self, _: as_types::process_tree::EventKey) {}
    fn prepare(&self, _: &EventData) -> Result<ProcessInfo, ModuleError> {
        self.mark("basic");
        Ok(ProcessInfo {
            name: Some(b"actor".to_vec()),
            parent_name: Some(b"parent".to_vec()),
            ..Default::default()
        })
    }
    fn record_hit(&self, _: &EventData, _: &RuleHit) {
        self.mark("hit");
    }
    fn record_direct_hits(&self, _: Vec<as_types::driver::DirectHitCount>) {
        self.mark("direct_hits");
    }
}
#[async_trait::async_trait]
impl SlowFacts for Smoke {
    async fn prepare(&self, _: &EventData) -> Result<ProcessInfo, ModuleError> {
        self.mark("slow");
        Ok(ProcessInfo {
            argv: Some(vec![b"slow".to_vec()]),
            ..Default::default()
        })
    }
}
#[async_trait::async_trait]
impl MessageModule for Smoke {
    async fn consume_native_fact(&self, fact: NativeFact) -> Result<(), ModuleError> {
        self.mark(match fact {
            NativeFact::RequestHandoff(_) => "request_handoff",
            NativeFact::RequestReturn(_) => "request_return",
            NativeFact::FutexSelection(_) => "futex_selection",
        });
        Ok(())
    }
    async fn run(
        &self,
        mut notices: tokio::sync::mpsc::Receiver<MainMessage>,
    ) -> Result<(), ModuleError> {
        while let Some(message) = notices.recv().await {
            match message {
                MainMessage::DriverStatus(_) => self.mark("driver_status"),
                MainMessage::DriverUnavailable => self.mark("driver_unavailable"),
                MainMessage::Completed(notice) => {
                    self.mark("message");
                    self.notices.fetch_add(1, Ordering::Relaxed);
                    self.completions
                        .lock()
                        .expect("completed notices")
                        .push(*notice);
                }
            }
        }
        Ok(())
    }
    async fn consume_chat(
        &self,
        mut events: as_types::flow::ChatReceiver,
        _feedback: Option<Arc<dyn as_types::flow::FlowFeedbackPort>>,
    ) -> Result<(), ModuleError> {
        while let Some(envelope) = events.recv().await {
            self.chat_cursors
                .lock()
                .expect("original chat cursors")
                .push(envelope.cursor);
            self.mark("chat");
            if let Some(entered) = &self.chat_entered {
                entered.notify_one();
            }
            if let Some(gate) = &self.chat_gate {
                gate.notified().await;
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecurityLogSink for Smoke {
    async fn run(
        self: Box<Self>,
        mut logs: tokio::sync::mpsc::Receiver<SecurityLogEntry>,
    ) -> std::io::Result<()> {
        while let Some(record) = logs.recv().await {
            let SecurityLogEntry::Rule(record) = record else {
                self.mark("file_close");
                self.logs.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            assert!(record.hit.log);
            self.logged_events
                .lock()
                .expect("log event references")
                .push(Arc::downgrade(&record.event));
            if record.result.is_err() {
                self.log_errors.fetch_add(1, Ordering::Relaxed);
            }
            if record.decision == RuleLogDecision::Pending {
                self.pending_logs.fetch_add(1, Ordering::Relaxed);
            }
            self.mark("rule_log");
            self.logs.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Original {
    id: DecisionId,
    window: HoldWindow,
    smoke: Smoke,
}
#[async_trait::async_trait]
impl OriginalOperation for Original {
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        None
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        Err(ModuleError::Stopped)
    }

    async fn bind_review(&self, _: as_types::operation::ReviewBinding) -> Result<(), ModuleError> {
        Ok(())
    }
    fn decision_id(&self) -> DecisionId {
        self.id
    }
    fn hold_window(&self) -> HoldWindow {
        self.window
    }
    fn validate(&self, _: &EventData) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn apply(&self, _: &EventData, _: &RuleChoice) -> Result<OperationReceipt, ModuleError> {
        self.smoke.mark("execute");
        if let Some(started) = &self.smoke.started {
            started.notify_one();
        }
        if let Some(gate) = &self.smoke.gate {
            let _permit = gate.acquire().await.map_err(|_| ModuleError::Stopped)?;
        }
        self.smoke.executed.fetch_add(1, Ordering::Relaxed);
        Ok(OperationReceipt {
            decision_id: self.id,
            outcome: ExecutionOutcome::Unknown,
            termination: as_types::operation::TerminationReport::NONE,
        })
    }
    fn cancel(&self) {
        self.smoke.mark("cancel");
    }
}
fn event_bytes(kind: u16, sequence: u64, stage: u16, operation: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 88];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&kind.to_le_bytes());
    bytes[10..12].copy_from_slice(&stage.to_le_bytes());
    bytes[12..16].copy_from_slice(&operation.to_le_bytes());
    bytes[16..32].copy_from_slice(&[1; 16]);
    bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&sequence.to_le_bytes());
    bytes
}
fn input(smoke: &Smoke, kind: u16, sequence: u64) -> DispatchInput {
    let bytes = event_bytes(kind, sequence, 2, 1);
    DispatchInput {
        transfer_hit: None,
        event: Arc::new(dispatch_event::decode(&bytes).expect("current unified frame")),
        operation: Some(Arc::new(Original {
            id: DecisionId([sequence as u8; 16]),
            window: HoldWindow::begin(Duration::from_secs(5)).expect("original lifetime"),
            smoke: smoke.clone(),
        })),
    }
}
#[test]
fn direct_result_keeps_source_facts_without_inventing_a_controlled_identity() {
    use crate::messages::protocol::{OperationReport, OperationResult, Up, operation_result};
    let notice = MainNotice {
        decision_id: None,
        event: Arc::new(dispatch_event::decode(&event_bytes(2, 77, 3, 1)).expect("source frame")),
        choice: Some(Smoke::choice(Decision::Deny {
            terminate: Termination::None,
        })),
        result: Ok(DispatchOutcome::Driver {
            outcome: ExecutionOutcome::Denied,
            termination: TerminationReport::NONE,
        }),
    };
    let Some(Up::OperationResult(original)) = operation_result(&notice) else {
        panic!("actual direct result missing")
    };
    let OperationResult {
        decision_id,
        event,
        hit,
        result,
    } = original.as_ref();
    assert!(decision_id.is_none());
    assert!(Arc::ptr_eq(event, &notice.event));
    assert_eq!(
        serde_json::to_value(hit).expect("hit"),
        serde_json::to_value(notice.choice.as_ref().expect("choice").hit.clone())
            .expect("source hit")
    );
    assert!(matches!(
        result,
        OperationReport::Receipt {
            outcome: ExecutionOutcome::Denied,
            termination: TerminationReport::NONE,
            ..
        }
    ));
}
#[tokio::test]
async fn three_kinds_flow_through_callbacks_main_messages_and_close() {
    let (_workspace, context, queues) = crate::test_driver::environment();
    let main_id = thread::current().id();
    let mut threads = crate::test_driver::start(
        context.clone(),
        queues.clone(),
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("main");
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    for (kind, sequence) in [(1, 1), (2, 2), (3, 3)] {
        let reply = ingress
            .submit(input(&smoke, kind, sequence))
            .unwrap_or_else(|_| panic!("admission"));
        assert!(matches!(
            reply.await.expect("reply").expect("result"),
            DispatchOutcome::Execution(OperationReceipt {
                outcome: ExecutionOutcome::Unknown,
                ..
            })
        ));
    }
    threads
        .shutdown()
        .await
        .expect("actual queue and thread joins");
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 3);
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 3);
    let trace = smoke.trace.lock().expect("trace");
    let review = trace.iter().find(|(s, _)| *s == "slow").expect("complex").1;
    let messages = trace
        .iter()
        .find(|(s, _)| *s == "message")
        .expect("messages")
        .1;
    assert_eq!(main_id, review);
    let callbacks = trace
        .iter()
        .find(|(s, _)| *s == "basic")
        .expect("callbacks")
        .1;
    assert_ne!(callbacks, main_id);
    assert_ne!(callbacks, messages);
    assert_ne!(main_id, messages);
    assert_ne!(review, messages);
    assert!(
        trace
            .iter()
            .filter(|(s, _)| matches!(*s, "basic" | "fast"))
            .all(|(_, id)| *id == callbacks)
    );
    assert!(
        trace
            .iter()
            .filter(|(s, _)| matches!(*s, "slow" | "prompt"))
            .all(|(_, id)| *id == review)
    );
    assert!(matches!(
        ingress.submit(input(&smoke, 1, 4)),
        Err(crate::dispatch::SubmitError {
            reason: ModuleError::Stopped,
            ..
        })
    ));
}
#[tokio::test]
async fn disconnected_reply_keeps_started_execution_until_drain() {
    let (_workspace, context, queues) = crate::test_driver::environment();
    let started = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let smoke = Smoke {
        started: Some(started.clone()),
        gate: Some(gate.clone()),
        ..Default::default()
    };
    let mut threads = crate::test_driver::start(
        context.clone(),
        queues.clone(),
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("main");
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    let reply = ingress
        .submit(input(&smoke, 1, 1))
        .unwrap_or_else(|_| panic!("admission"));
    started.notified().await;
    drop(reply);
    let shutdown = threads.shutdown();
    tokio::pin!(shutdown);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), shutdown.as_mut())
            .await
            .is_err()
    );
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 0);
    gate.add_permits(1);
    shutdown.await.expect("retirement");
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 1);
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn actual_policy_factory_drives_main_and_only_marked_rules_queue_logs_on_message_thread() {
    let (_workspace, context, queues) = crate::test_driver::environment();
    let (interaction, _prompts) = as_policy_engine::interaction::Interaction::channel(
        NonZeroUsize::new(4).expect("capacity"),
    );
    let policy: as_policy_engine::model::PolicyDocument = serde_saphyr::from_str(
        r#"
groups:
- kind: process
  id: process
  number: 1
  enabled: true
  execution: { location: backend, stage: fast }
  rules:
  - id: block
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [execute]
    source: {parent_name: {operator: exact, value: parent}}
    target: {}
    action: {type: deny, terminate: actor}
    log: true
- kind: file
  id: file
  number: 1
  enabled: true
  execution: { location: backend, stage: ask }
  rules:
  - id: ask
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [open]
    target: {}
    action: {type: ask, timeout_ms: 100, timeout: {type: deny, terminate: none}}
    log: true
- kind: network
  id: network
  number: 1
  enabled: true
  execution: { location: backend, stage: fast }
  rules:
  - id: silent
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [connect]
    target: {}
    action: {type: allow}
    log: false
"#,
    )
    .expect("real policy");
    let source = context.paths().rules_file().to_path_buf();
    std::fs::write(
        &source,
        serde_saphyr::to_string(&policy).expect("current YAML"),
    )
    .expect("source");
    let engine = as_policy_engine::PolicyEngine::initialize(&context, interaction.clone())
        .await
        .expect("config-selected rule module");
    let engine = Arc::new(engine);
    let smoke = Smoke::default();
    let mut modules = smoke.modules();
    modules.rules = engine.clone();
    let mut threads = crate::test_driver::start(
        context.clone(),
        queues.clone(),
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("main");
    let (session, remote, connection_thread) = threads
        .dispatch()
        .spawn(ProcessingThread::Messages, async move {
            let (stream, remote) = tokio::net::UnixStream::pair().expect("actual IPC stream");
            let uid = stream.peer_cred().expect("kernel peer UID").uid();
            let session = crate::messages::connections::Connections::for_testing(interaction)
                .connect(
                    stream,
                    &as_types::configuration::MiddlewareAccess {
                        chat_content: false,
                        websocket: None,
                        peer: as_types::configuration::Principal::UnixUid { uid },
                        access: as_types::configuration::Access::ReadOnly,
                    },
                )
                .expect("first connection authenticated in message thread");
            (session, remote, thread::current().id())
        })
        .await
        .expect("message-thread admission");
    drop((session, remote)); // Disconnected before Ask; the rules only observe channel availability.
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules, Box::new(smoke.clone()), stopped)
        .expect("connected real factory");
    for (kind, seq) in [(1, 1), (2, 2), (3, 3)] {
        ingress
            .submit(input(&smoke, kind, seq))
            .unwrap_or_else(|_| panic!("admission"))
            .await
            .expect("reply")
            .expect("actual source result");
    }
    threads.shutdown().await.expect("drained all three threads");
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 3);
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 3);
    assert_eq!(smoke.logs.load(Ordering::Relaxed), 1);
    {
        let logged = smoke.logged_events.lock().expect("log event references");
        let mut completed = smoke.completions.lock().expect("completed notices");
        for event in logged.iter() {
            assert!(
                completed
                    .iter()
                    .any(|notice| event.ptr_eq(&Arc::downgrade(&notice.event)))
            );
        }
        completed.clear();
        assert!(logged.iter().all(|event| event.upgrade().is_none()));
    }
    assert!(
        !smoke
            .trace
            .lock()
            .expect("trace")
            .iter()
            .any(|(name, _)| matches!(*name, "slow" | "prompt")),
        "headless requests only use quick rules"
    );
    assert_eq!(ingress.rejected_notices(), 0);
    let trace = smoke.trace.lock().expect("trace");
    let message_thread = trace
        .iter()
        .find(|(name, _)| *name == "message")
        .expect("messages")
        .1;
    assert!(
        trace
            .iter()
            .filter(|(name, _)| *name == "rule_log")
            .all(|(_, id)| *id == message_thread)
    );
    assert_eq!(connection_thread, message_thread);
    assert_ne!(message_thread, thread::current().id());
}

#[tokio::test]
async fn cancelled_pending_ask_keeps_its_hit_in_the_log_without_inventing_a_decision() {
    let (_workspace, context, queues) = crate::test_driver::environment();
    let source = context.paths().rules_file().to_path_buf();
    std::fs::write(
        &source,
        r#"groups:
- kind: file
  id: file
  number: 1
  enabled: true
  execution: { location: backend, stage: ask }
  rules:
  - id: ask-and-log
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [open]
    target: {}
    action: {type: ask, timeout_ms: 5000, timeout: {type: deny, terminate: none}}
    log: true
"#,
    )
    .expect("policy");
    let (interaction, mut prompts) = as_policy_engine::interaction::Interaction::channel(
        NonZeroUsize::new(2).expect("capacity"),
    );
    let (stream, _remote) = tokio::net::UnixStream::pair().expect("message transport");
    let uid = stream.peer_cred().expect("kernel identity").uid();
    let session = crate::messages::connections::Connections::for_testing(interaction.clone())
        .connect(
            stream,
            &as_types::configuration::MiddlewareAccess {
                chat_content: false,
                websocket: None,
                peer: as_types::configuration::Principal::UnixUid { uid },
                access: as_types::configuration::Access::ManageRules,
            },
        )
        .expect("authenticated channel");
    session
        .set_review_available(true)
        .expect("real review channel ready");
    let engine = as_policy_engine::PolicyEngine::initialize(&context, interaction)
        .await
        .expect("rules");
    let smoke = Smoke::default();
    let mut modules = smoke.modules();
    modules.rules = Arc::new(engine);
    let mut threads = crate::test_driver::start(
        context.clone(),
        queues.clone(),
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("main");
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules, Box::new(smoke.clone()), stopped)
        .expect("connect");
    let reply = ingress
        .submit(input(&smoke, 2, 1))
        .unwrap_or_else(|_| panic!("admission"));
    let prompt = prompts.recv().await.expect("actual matched Ask");
    assert!(prompt.hit.log);
    drop(reply);
    tokio::time::timeout(Duration::from_secs(1), threads.shutdown())
        .await
        .expect("cancelled wait drains")
        .expect("joined");
    assert!(prompt.reply.is_closed());
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 0);
    assert_eq!(smoke.logs.load(Ordering::Relaxed), 1);
    assert_eq!(smoke.pending_logs.load(Ordering::Relaxed), 1);
    assert_eq!(
        smoke
            .trace
            .lock()
            .expect("trace")
            .iter()
            .filter(|(name, _)| *name == "hit")
            .count(),
        1
    );
    assert_eq!(smoke.log_errors.load(Ordering::Relaxed), 1);
}

struct CountedDriver {
    dropped: Arc<AtomicUsize>,
    retired: Arc<tokio::sync::Notify>,
}
struct CountedSession {
    dropped: Arc<AtomicUsize>,
    retired: Arc<tokio::sync::Notify>,
    inner: Arc<dyn as_types::driver::DriverSession>,
}
#[async_trait::async_trait]
impl as_types::driver::DriverAdapter for CountedDriver {
    async fn connect(
        &self,
        input: &as_types::driver::DriverConnectInput,
    ) -> Result<Arc<dyn as_types::driver::DriverSession>, ModuleError> {
        let inner = crate::test_driver::SoftwareDriver.connect(input).await?;
        Ok(Arc::new(CountedSession {
            dropped: self.dropped.clone(),
            retired: self.retired.clone(),
            inner,
        }))
    }
}
#[async_trait::async_trait]
impl as_types::driver_control::DriverControl for CountedSession {
    async fn request(
        &self,
        command: as_types::driver_control::DriverCommand,
    ) -> Result<as_types::driver_control::DriverStatus, ModuleError> {
        self.inner.request(command).await
    }
}
#[async_trait::async_trait]
impl as_types::process_source::ProcessFactsPort for CountedSession {
    async fn inspect(
        &self,
        pids: &[u32],
    ) -> Result<Vec<as_types::process_source::ProcessInspection>, ModuleError> {
        self.inner.inspect(pids).await
    }
    async fn facts_barrier(
        &self,
    ) -> Result<as_types::process_source::ProcessFactsBarrier, ModuleError> {
        self.inner.facts_barrier().await
    }
}
#[async_trait::async_trait]
impl as_types::driver_tools::DriverTools for CountedSession {
    async fn kill_process(
        &self,
        target: as_types::driver_tools::DriverProcessTarget,
    ) -> Result<as_types::driver_tools::KillProcessOutcome, ModuleError> {
        self.inner.kill_process(target).await
    }
}
#[async_trait::async_trait]
impl as_types::driver::DriverSession for CountedSession {
    fn info(&self) -> as_types::driver::DriverInfo {
        self.inner.info()
    }
    fn rejected_callbacks(&self) -> u64 {
        self.inner.rejected_callbacks()
    }
    fn callback_loss(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.callback_loss()
    }
    async fn start_process_tracking(
        &self,
        callbacks: as_types::driver::DriverCallbacks,
    ) -> Result<as_types::process_source::ProcessTrackingStart, ModuleError> {
        self.inner.start_process_tracking(callbacks).await
    }
    async fn initialize(
        &self,
        parts: &as_types::driver_image::DriverInitParts,
    ) -> Result<as_types::driver::AppliedPersistentState, ModuleError> {
        self.inner.initialize(parts).await
    }
    async fn replace_persistent(
        &self,
        parts: &as_types::driver_image::DriverInitParts,
    ) -> Result<as_types::driver::AppliedPersistentState, ModuleError> {
        self.inner.replace_persistent(parts).await
    }
    fn stop_admission(&self) -> Result<(), ModuleError> {
        self.inner.stop_admission()
    }
    async fn shutdown(&self) -> Result<(), ModuleError> {
        self.inner.shutdown().await
    }
}
impl Drop for CountedSession {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
        self.retired.notify_one();
    }
}
#[tokio::test]
async fn cancelling_shutdown_wait_does_not_release_driver_before_original_execution() {
    let (_workspace, context, queues) = crate::test_driver::environment();
    let dropped = Arc::new(AtomicUsize::new(0));
    let retired = Arc::new(tokio::sync::Notify::new());
    let driver = CountedDriver {
        dropped: dropped.clone(),
        retired: retired.clone(),
    };
    let started = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let smoke = Smoke {
        started: Some(started.clone()),
        gate: Some(gate.clone()),
        ..Default::default()
    };
    let mut threads = crate::test_driver::start(context.clone(), queues.clone(), Arc::new(driver))
        .await
        .expect("main");
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    let reply = ingress
        .submit(input(&smoke, 1, 1))
        .unwrap_or_else(|_| panic!("admission"));
    started.notified().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), threads.shutdown())
            .await
            .is_err()
    );
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 0);
    gate.add_permits(1);
    tokio::time::timeout(Duration::from_secs(1), retired.notified())
        .await
        .expect("real retirement");
    assert!(reply.await.expect("reply").is_ok());
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 1);
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
}

#[derive(Debug)]
struct ExpectedOperation {
    inner: Arc<dyn OriginalOperation>,
    decision: Decision,
}
#[async_trait::async_trait]
impl OriginalOperation for ExpectedOperation {
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        self.inner.try_result()
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        self.inner.result().await
    }

    async fn bind_review(&self, _: as_types::operation::ReviewBinding) -> Result<(), ModuleError> {
        Ok(())
    }
    fn decision_id(&self) -> DecisionId {
        self.inner.decision_id()
    }
    fn hold_window(&self) -> HoldWindow {
        self.inner.hold_window()
    }
    fn validate(&self, event: &EventData) -> Result<(), ModuleError> {
        self.inner.validate(event)
    }
    async fn apply(
        &self,
        event: &EventData,
        choice: &RuleChoice,
    ) -> Result<OperationReceipt, ModuleError> {
        assert_eq!(
            choice.decision, self.decision,
            "container decision must not leak across events"
        );
        self.inner.apply(event, choice).await
    }
    fn cancel(&self) {
        self.inner.cancel();
    }
}
#[tokio::test]
async fn one_container_waiting_does_not_block_another_containers_fast_pool() {
    use as_types::workload::{ContainerInstance, Workload};
    let (_workspace, context, queues) = crate::test_driver::environment();
    let source = context.paths().rules_file().to_path_buf();
    std::fs::write(
        &source,
        r#"groups:
- kind: file
  id: fast-B
  number: 1
  enabled: true
  execution: { location: backend, stage: fast }
  rules:
  - id: deny-B
    number: 1
    enabled: true
    scope: {kind: containers, instances: [{runtime: smoke, namespace: null, id: B}]}
    operations: [open]
    target: {}
    action: {type: deny, terminate: none}
    log: false
- kind: file
  id: ask-A
  number: 1
  enabled: true
  execution: { location: backend, stage: ask }
  rules:
  - id: ask-A
    number: 1
    enabled: true
    scope: {kind: containers, instances: [{runtime: smoke, namespace: null, id: A}]}
    operations: [open]
    target: {}
    action: {type: ask, timeout_ms: 5000, timeout: {type: deny, terminate: none}}
    log: false
"#,
    )
    .expect("policy");
    let (interaction, mut prompts) = as_policy_engine::interaction::Interaction::channel(
        NonZeroUsize::new(8).expect("capacity"),
    );
    let (stream, _remote) = tokio::net::UnixStream::pair().expect("actual IPC");
    let uid = stream.peer_cred().expect("kernel UID").uid();
    let session = crate::messages::connections::Connections::for_testing(interaction.clone())
        .connect(
            stream,
            &as_types::configuration::MiddlewareAccess {
                chat_content: false,
                websocket: None,
                peer: as_types::configuration::Principal::UnixUid { uid },
                access: as_types::configuration::Access::ManageRules,
            },
        )
        .expect("auth");
    session.set_review_available(true).expect("review ready");
    let engine = as_policy_engine::PolicyEngine::initialize(&context, interaction)
        .await
        .expect("rules");
    let smoke = Smoke::default();
    let mut modules = smoke.modules();
    modules.rules = Arc::new(engine);
    let mut threads = crate::test_driver::start(
        context.clone(),
        queues.clone(),
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("main");
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules, Box::new(smoke.clone()), stopped)
        .expect("connected");
    let make = |id: &str, seq, decision| {
        let mut input = input(&smoke, 2, seq);
        Arc::make_mut(&mut input.event).process.workload =
            Some(Workload::Container(ContainerInstance {
                runtime: "smoke".into(),
                namespace: None,
                id: id.into(),
            }));
        let inner = input.operation.take().expect("original");
        input.operation = Some(Arc::new(ExpectedOperation { inner, decision }));
        input
    };
    let mut waiting = Vec::new();
    for seq in 1..=2 {
        waiting.push(
            ingress
                .submit(make("A", seq, Decision::Allow))
                .unwrap_or_else(|_| panic!("admission")),
        );
    }
    let first = prompts.recv().await.expect("first A is waiting");
    assert!(prompts.try_recv().is_err(), "second A must stay queued");
    let mut fast = Vec::new();
    for seq in 3..=4 {
        fast.push(
            ingress
                .submit(make(
                    "B",
                    seq,
                    Decision::Deny {
                        terminate: Termination::None,
                    },
                ))
                .unwrap_or_else(|_| panic!("admission")),
        );
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        for reply in fast {
            assert!(reply.await.expect("B reply").is_ok());
        }
    })
    .await
    .expect("B returns while A has no answer");
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 2);
    first.reply.send(Decision::Allow).expect("first A");
    let second = tokio::time::timeout(Duration::from_secs(1), prompts.recv())
        .await
        .expect("next serial question")
        .expect("second A");
    assert!(prompts.try_recv().is_err(), "only one active question");
    second.reply.send(Decision::Allow).expect("second A");
    for reply in waiting {
        assert!(reply.await.expect("A reply").is_ok());
    }
    let slow_calls = smoke
        .trace
        .lock()
        .expect("trace")
        .iter()
        .filter(|(name, _)| *name == "slow")
        .count();
    drop(session);
    let headless = ingress
        .submit(make("A", 5, Decision::Allow))
        .unwrap_or_else(|_| panic!("headless admission"));
    assert!(headless.await.expect("headless reply").is_ok());
    assert!(prompts.try_recv().is_err(), "no frontend means no question");
    assert_eq!(
        slow_calls,
        smoke
            .trace
            .lock()
            .expect("trace")
            .iter()
            .filter(|(name, _)| *name == "slow")
            .count(),
        "no slow queries without a frontend"
    );
    let unknown = ingress
        .submit(input(&smoke, 2, 6))
        .unwrap_or_else(|_| panic!("unknown scope admission"));
    assert!(
        matches!(
            unknown.await.expect("unknown scope reply"),
            Err(ModuleError::Unavailable)
        ),
        "missing scope cannot become a headless allow"
    );
    threads.shutdown().await.expect("drained");
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 5);
}

#[tokio::test]
async fn lifecycle_callback_reaches_fast_facts_without_a_rule_or_original_wait() {
    use as_types::driver_wire::{DriverFrame, FrameClass};
    let (_root, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("threads");
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    let mut body = event_bytes(
        1,
        77,
        1,
        as_types::event_operation::ProcessOperation::Create as u32,
    );
    for (id, value) in [
        (3u16, 222u32.to_le_bytes().to_vec()),
        (6, 333u32.to_le_bytes().to_vec()),
    ] {
        body.extend(id.to_le_bytes());
        body.extend([0, 0]);
        body.extend((value.len() as u32).to_le_bytes());
        body.extend(value);
    }
    let length = body.len() as u32;
    body[4..8].copy_from_slice(&length.to_le_bytes());
    let frame = DriverFrame {
        class: FrameClass::Event,
        message_type: 0x104,
        session: [9; 16],
        request_id: 0,
        total_bytes: body.len() as u64,
        offset: 0,
        body,
    };
    let wire = frame.encode().expect("current envelope");
    let frame = DriverFrame::decode(&wire, 4096, 4096).expect("current decode");
    let event = frame.observation([9; 16]).expect("bound callback");
    let dispatch_event::EventPayload::Process(child) = &event.payload else {
        panic!("process")
    };
    assert_eq!(child.child_pid, Some(222));
    assert_eq!(child.child_parent_pid, Some(333));
    let reply = ingress
        .submit(DispatchInput {
            transfer_hit: None,
            event: Arc::new(event),
            operation: None,
        })
        .unwrap_or_else(|_| panic!("admission"));
    assert!(matches!(
        reply.await.expect("receiving").expect("actual main result"),
        DispatchOutcome::Observed
    ));
    threads.shutdown().await.expect("drained threads");
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 1);
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 0);
    assert_eq!(smoke.logs.load(Ordering::Relaxed), 0);
    {
        let notices = smoke.completions.lock().expect("observation notice");
        let observed = notices
            .iter()
            .find(|n| matches!(n.result, Ok(DispatchOutcome::Observed)))
            .expect("actual observation");
        assert!(observed.decision_id.is_none());
        assert!(crate::messages::protocol::operation_result(observed).is_none());
    }
    let trace = smoke.trace.lock().expect("trace");
    assert!(trace.iter().any(|(name, _)| *name == "basic"));
    assert!(
        trace
            .iter()
            .all(|(name, _)| !["fast", "prompt", "slow", "execute", "hit"].contains(name))
    );
}

#[derive(Debug)]
struct CompletedDuringBinding {
    window: HoldWindow,
    done: std::sync::atomic::AtomicBool,
    apply_calls: AtomicUsize,
    cancel_calls: AtomicUsize,
}
#[async_trait::async_trait]
impl OriginalOperation for CompletedDuringBinding {
    fn decision_id(&self) -> DecisionId {
        DecisionId([55; 16])
    }
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        self.done
            .load(Ordering::SeqCst)
            .then_some(Ok(OperationReceipt {
                decision_id: self.decision_id(),
                outcome: ExecutionOutcome::Allowed,
                termination: as_types::operation::TerminationReport::NONE,
            }))
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        self.try_result().unwrap_or(Err(ModuleError::Unavailable))
    }
    fn hold_window(&self) -> HoldWindow {
        self.window
    }
    fn validate(&self, _: &EventData) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn bind_review(&self, _: ReviewBinding) -> Result<(), ModuleError> {
        // Explicit software source: binding was accepted, its Allow fallback completed,
        // and the binding reply arrived too late. This is not an unbound automatic allow.
        self.done.store(true, Ordering::SeqCst);
        Err(ModuleError::Stopped)
    }
    async fn apply(&self, _: &EventData, _: &RuleChoice) -> Result<OperationReceipt, ModuleError> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        Err(ModuleError::InvalidInput)
    }
    fn cancel(&self) {
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn late_bind_reply_collects_the_same_bound_fallback_without_another_decision() {
    let (_root, context, queues) = crate::test_driver::environment();
    std::fs::write(
        context.paths().rules_file(),
        r#"groups:
- kind: file
  id: late-bind
  number: 1
  enabled: true
  execution: {location: backend, stage: ask}
  rules:
  - id: original-question
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [open]
    target: {}
    action: {type: ask, timeout_ms: 1000, timeout: {type: allow}}
    log: true
"#,
    )
    .expect("policy");
    let (interaction, mut prompts) =
        as_policy_engine::interaction::Interaction::channel(NonZeroUsize::MIN);
    let _ready = interaction.connect().expect("ready channel");
    let policy = Arc::new(
        as_policy_engine::PolicyEngine::initialize(&context, interaction)
            .await
            .expect("actual rule factory"),
    );
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("threads");
    let smoke = Smoke::default();
    let mut modules = smoke.modules();
    modules.rules = policy;
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules, Box::new(smoke.clone()), stopped)
        .expect("connect");
    let original = Arc::new(CompletedDuringBinding {
        window: HoldWindow::begin(Duration::from_secs(5)).expect("window"),
        done: false.into(),
        apply_calls: AtomicUsize::new(0),
        cancel_calls: AtomicUsize::new(0),
    });
    let mut work = input(&smoke, 2, 55);
    work.operation = Some(original.clone());
    let reply = ingress.submit(work).unwrap_or_else(|_| panic!("admission"));
    assert!(matches!(
        reply
            .await
            .expect("receiving")
            .expect("real source completion"),
        DispatchOutcome::Execution(OperationReceipt {
            outcome: ExecutionOutcome::Allowed,
            ..
        })
    ));
    threads.shutdown().await.expect("actual source retirement");
    assert_eq!(original.apply_calls.load(Ordering::SeqCst), 0);
    assert_eq!(original.cancel_calls.load(Ordering::SeqCst), 0);
    assert!(prompts.try_recv().is_err());
    assert_eq!(smoke.logs.load(Ordering::Relaxed), 1);
    assert_eq!(smoke.log_errors.load(Ordering::Relaxed), 0);
    let notices = smoke.completions.lock().expect("completed source notices");
    let notice = notices
        .iter()
        .find(|notice| notice.decision_id == Some(original.decision_id()))
        .expect("actual bound source completion");
    let Some(crate::messages::protocol::Up::OperationResult(result)) =
        crate::messages::protocol::operation_result(notice)
    else {
        panic!("original result")
    };
    assert!(
        matches!(
            &result.result,
            crate::messages::protocol::OperationReport::Receipt {
                proposal: None,
                outcome: ExecutionOutcome::Allowed,
                ..
            }
        ),
        "source fallback must not invent proposal provenance"
    );
}

#[derive(Debug)]
struct FailedOperation {
    window: HoldWindow,
    done: std::sync::atomic::AtomicBool,
    changed: tokio::sync::Notify,
    applied: AtomicUsize,
    cancelled: AtomicUsize,
    deferred: Option<Arc<tokio::sync::Notify>>,
    entered: Option<Arc<tokio::sync::Notify>>,
}
#[async_trait::async_trait]
impl OriginalOperation for FailedOperation {
    fn decision_id(&self) -> DecisionId {
        DecisionId([71; 16])
    }
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        self.done
            .load(Ordering::SeqCst)
            .then_some(Err(ModuleError::Unavailable))
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        if let Some(entered) = &self.entered {
            entered.notify_one();
        }
        if let Some(released) = &self.deferred {
            released.notified().await;
            self.done.store(true, Ordering::SeqCst);
        }
        loop {
            let changed = self.changed.notified();
            if let Some(result) = self.try_result() {
                return result;
            }
            changed.await;
        }
    }
    fn hold_window(&self) -> HoldWindow {
        self.window
    }
    fn validate(&self, _: &EventData) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn bind_review(&self, _: ReviewBinding) -> Result<(), ModuleError> {
        Err(ModuleError::Unsupported)
    }
    async fn apply(&self, _: &EventData, _: &RuleChoice) -> Result<OperationReceipt, ModuleError> {
        self.applied.fetch_add(1, Ordering::SeqCst);
        self.done.store(true, Ordering::SeqCst);
        self.changed.notify_one();
        Err(ModuleError::Unavailable)
    }
    fn cancel(&self) {
        self.cancelled.fetch_add(1, Ordering::SeqCst);
        if self.deferred.is_none() {
            self.done.store(true, Ordering::SeqCst);
        }
        self.changed.notify_one();
    }
}
#[tokio::test]
async fn log_false_failure_has_one_real_completion_and_upstream_failure() {
    use crate::messages::protocol::{
        FailureCode, OperationReport, OperationResult, Up, operation_result,
    };
    let (_root, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("threads");
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    let operation = Arc::new(FailedOperation {
        window: HoldWindow::begin(Duration::from_secs(5)).expect("window"),
        done: false.into(),
        changed: tokio::sync::Notify::new(),
        applied: AtomicUsize::new(0),
        cancelled: AtomicUsize::new(0),
        deferred: None,
        entered: None,
    });
    let mut work = input(&smoke, 2, 1);
    work.operation = Some(operation.clone());
    assert!(matches!(
        ingress
            .submit(work)
            .unwrap_or_else(|_| panic!("admission"))
            .await
            .expect("internal reply"),
        Err(ModuleError::Unavailable)
    ));
    threads.shutdown().await.expect("real owners drained");
    assert_eq!(
        operation.applied.load(Ordering::SeqCst),
        1,
        "failure notification must not execute again"
    );
    assert_eq!(
        operation.cancelled.load(Ordering::SeqCst),
        0,
        "the source already completed its failure"
    );
    assert_eq!(
        smoke.logs.load(Ordering::Relaxed),
        0,
        "log=false remains independent of completion"
    );
    let notices = smoke.completions.lock().expect("completion consumer");
    assert_eq!(notices.len(), 1);
    let notice = &notices[0];
    assert_eq!(notice.decision_id, Some(operation.decision_id()));
    assert!(matches!(notice.result, Err(ModuleError::Unavailable)));
    assert!(
        matches!(operation_result(notice),Some(Up::OperationResult(result)) if matches!(result.as_ref(),OperationResult{decision_id,event,result:OperationReport::Failed{code:FailureCode::Unavailable,..},..} if *decision_id==Some(operation.decision_id()) && Arc::ptr_eq(event, &notice.event)))
    );
}

#[tokio::test]
async fn native_retirement_is_retained_by_processing_drain_after_source_eof() {
    let (_root, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("threads");
    let smoke = Smoke::default();
    let modules = smoke.modules();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules.clone(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let operation = Arc::new(FailedOperation {
        window: HoldWindow::begin(Duration::from_secs(5)).expect("window"),
        done: false.into(),
        changed: tokio::sync::Notify::new(),
        applied: AtomicUsize::new(0),
        cancelled: AtomicUsize::new(0),
        deferred: Some(release.clone()),
        entered: Some(entered.clone()),
    });
    let mut input = input(&smoke, 2, 97);
    input.operation = Some(operation.clone());
    let (events, receiving) = tokio::sync::mpsc::channel(1);
    let dispatch = threads.dispatch();
    let source_port = threads.driver_port();
    let source_ingress = ingress.clone();
    let source = dispatch.spawn(ProcessingThread::Fast, async move {
        crate::native_events::controlled(receiving, source_port, source_ingress, modules, None)
            .await
    });
    events.send(input).await.expect("real retained input");
    entered.notified().await;
    ingress.close();
    drop(events);
    source
        .await
        .expect("source task joined")
        .expect("source stream ended");
    let mut connection = threads.connection.take().expect("main connection");
    {
        let mut processing = Box::pin(connection.drain_processing());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut processing)
                .await
                .is_err(),
            "the same processing budget retains unfinished native retirement"
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), processing)
            .await
            .expect("original retirement completes processing drain")
            .expect("processing drained");
    }
    let messages = connection.tasks[2].take().expect("message consumer");
    tokio::time::timeout(Duration::from_secs(2), messages)
        .await
        .expect("actual completion and EOF consumed")
        .expect("message task joined")
        .expect("message module completed");
    threads
        .shutdown()
        .await
        .expect("all physical runtimes joined");
    assert_eq!(
        operation.applied.load(Ordering::SeqCst),
        0,
        "compensation must not execute a rejected decision"
    );
    let notices = smoke
        .completions
        .lock()
        .expect("completion after source EOF");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].decision_id, Some(operation.decision_id()));
    assert!(matches!(notices[0].result, Err(ModuleError::Unavailable)));
}

#[tokio::test]
async fn process_facts_fill_before_matching_without_replacing_captured_values() {
    let (_root, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("threads");
    let smoke = Smoke {
        expected_name: Some(b"captured-actor".to_vec()),
        expected_argv: Some(vec![b"captured-argument".to_vec()]),
        ..Default::default()
    };
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("connect");
    for sequence in [1, 2] {
        let mut work = input(&smoke, 2, sequence);
        Arc::make_mut(&mut work.event).process.name = smoke.expected_name.clone();
        Arc::make_mut(&mut work.event).process.argv = smoke.expected_argv.clone();
        ingress
            .submit(work)
            .unwrap_or_else(|_| panic!("admission"))
            .await
            .expect("receipt")
            .expect("processing");
    }
    threads.shutdown().await.expect("drained");
    let trace = smoke.trace.lock().expect("trace");
    assert_eq!(trace.iter().filter(|(name, _)| *name == "basic").count(), 2);
    assert_eq!(trace.iter().filter(|(name, _)| *name == "slow").count(), 1);
}

#[tokio::test]
async fn finished_original_and_cancelled_observation_still_feed_process_facts() {
    let (_root, _context, queues) = crate::test_driver::environment();
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(smoke.modules(), Box::new(smoke.clone()), &queues, stopped);
    let original = Arc::new(CompletedDuringBinding {
        window: HoldWindow::begin(Duration::from_secs(5)).expect("window"),
        done: true.into(),
        apply_calls: AtomicUsize::new(0),
        cancel_calls: AtomicUsize::new(0),
    });
    let mut work = input(&smoke, 2, 71);
    work.operation = Some(original.clone());
    let completed = ingress.submit(work).unwrap_or_else(|_| panic!("admission"));
    let observation = DispatchInput {
        event: Arc::new(dispatch_event::decode(&event_bytes(1, 72, 1, 2)).expect("create fact")),
        operation: None,
        transfer_hit: None,
    };
    drop(
        ingress
            .submit(observation)
            .unwrap_or_else(|_| panic!("observation admission")),
    );
    ingress.close();
    let (fast, main, messages) = tokio::join!(workers.fast, workers.main, workers.messages);
    fast.expect("fast drained");
    main.expect("main drained");
    messages.expect("messages drained");
    assert!(matches!(
        completed.await.expect("receipt").expect("actual result"),
        DispatchOutcome::Execution(OperationReceipt {
            outcome: ExecutionOutcome::Allowed,
            ..
        })
    ));
    assert_eq!(original.apply_calls.load(Ordering::SeqCst), 0);
    assert_eq!(original.cancel_calls.load(Ordering::SeqCst), 0);
    let trace = smoke.trace.lock().expect("trace");
    assert_eq!(trace.iter().filter(|(name, _)| *name == "basic").count(), 2);
    assert!(
        trace
            .iter()
            .all(|(name, _)| !["fast", "slow", "prompt", "execute"].contains(name))
    );
}

#[tokio::test]
async fn full_notice_queue_retains_original_completion_until_consumer_accepts_it() {
    let (_workspace, _context, mut queues) = crate::test_driver::environment();
    queues.messages = NonZeroUsize::MIN;
    let started = Arc::new(tokio::sync::Notify::new());
    let smoke = Smoke {
        started: Some(started.clone()),
        ..Smoke::default()
    };
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(smoke.modules(), Box::new(smoke.clone()), &queues, stopped);
    let fast = tokio::spawn(workers.fast);
    let main = tokio::spawn(workers.main);

    // Do not poll the actual message consumer yet: the first real completion fills its one slot.
    ingress
        .submit(input(&smoke, 1, 1))
        .unwrap_or_else(|_| panic!("first admission"))
        .await
        .expect("first reply")
        .expect("first source result");
    started.notified().await;
    let mut second = ingress
        .submit(input(&smoke, 2, 2))
        .unwrap_or_else(|_| panic!("second admission"));
    started.notified().await;
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 2);
    assert!(
        futures::poll!(&mut second).is_pending(),
        "original completed context still owns its queued notice"
    );
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 0);
    assert_eq!(ingress.rejected_notices(), 0);

    // Shutdown processing also waits for these original contexts; the consumer remains live.
    ingress.close();
    let (fast, main, messages) = tokio::join!(fast, main, workers.messages);
    fast.expect("fast task").expect("fast drained");
    main.expect("main task").expect("main drained");
    messages.expect("messages drained");
    second
        .await
        .expect("second reply")
        .expect("second source result");
    let completed = smoke.completions.lock().expect("actual completions");
    assert_eq!(completed.len(), 2);
    for (notice, original) in completed
        .iter()
        .zip([DecisionId([1; 16]), DecisionId([2; 16])])
    {
        assert_eq!(notice.decision_id, Some(original));
        assert!(
            matches!(&notice.result, Ok(DispatchOutcome::Execution(receipt))
            if receipt.decision_id == original && receipt.outcome == ExecutionOutcome::Unknown)
        );
    }
    assert_eq!(ingress.rejected_notices(), 0);
}

#[tokio::test]
async fn direct_results_wait_for_consumption_and_report_a_closed_consumer() {
    let (_workspace, _context, mut queues) = crate::test_driver::environment();
    queues.messages = NonZeroUsize::MIN;
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(smoke.modules(), Box::new(smoke.clone()), &queues, stopped);
    let fast = tokio::spawn(workers.fast);
    let main = tokio::spawn(workers.main);
    let direct = |number: u16| {
        MainMessage::Completed(Box::new(MainNotice {
            decision_id: None,
            event: input(&smoke, number, u64::from(number)).event,
            choice: Some(Smoke::choice(Decision::Deny {
                terminate: Termination::None,
            })),
            result: Ok(DispatchOutcome::Driver {
                outcome: ExecutionOutcome::Denied,
                termination: TerminationReport::NONE,
            }),
        }))
    };
    ingress
        .driver_notice(direct(1))
        .await
        .expect("first accepted");
    let mut second = Box::pin(ingress.driver_notice(direct(2)));
    assert!(
        futures::poll!(&mut second).is_pending(),
        "a full queue retains this actual direct result"
    );
    assert_eq!(ingress.rejected_notices(), 0);
    let messages = tokio::spawn(workers.messages);
    second.await.expect("consumer accepted second result");
    ingress.close();
    let (fast, main, messages) = tokio::join!(fast, main, messages);
    fast.expect("fast joined").expect("fast drained");
    main.expect("main joined").expect("main drained");
    messages
        .expect("messages joined")
        .expect("messages drained");
    {
        let completed = smoke.completions.lock().expect("original results");
        assert_eq!(completed.len(), 2);
        assert!(completed.iter().all(|n| n.decision_id.is_none()
            && matches!(
                n.result,
                Ok(DispatchOutcome::Driver {
                    outcome: ExecutionOutcome::Denied,
                    ..
                })
            )));
    }
    assert!(matches!(
        ingress.driver_notice(direct(3)).await,
        Err(ModuleError::Stopped)
    ));
    assert_eq!(ingress.rejected_notices(), 1);
}

#[tokio::test]
async fn blocked_diagnostic_sink_does_not_stop_the_actual_messages_consumer() {
    use as_types::diagnostics::DiagnosticWriter;
    use tokio::io::AsyncReadExt;
    let (_workspace, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("original runtimes");
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(smoke.modules(), Box::new(smoke.clone()), stopped)
        .expect("actual consumers");
    let (output, mut sink_reader) = tokio::io::duplex(3);
    let writer = Arc::new(DiagnosticWriter::new(output));
    let (entered, blocked) = tokio::sync::oneshot::channel();
    let writing = threads
        .dispatch()
        .spawn(ProcessingThread::Messages, async move {
            let mut pending = Box::pin(writer.line("abcdef".into()));
            assert!(
                futures::poll!(&mut pending).is_pending(),
                "the real byte pipe is full"
            );
            let _ = entered.send(thread::current().id());
            pending.await
        });
    let diagnostic_thread = blocked.await.expect("writer actually blocked");
    ingress
        .driver_notice(MainMessage::Completed(Box::new(MainNotice {
            decision_id: None,
            event: input(&smoke, 1, 92).event,
            choice: Some(Smoke::choice(Decision::Allow)),
            result: Ok(DispatchOutcome::Driver {
                outcome: ExecutionOutcome::Allowed,
                termination: TerminationReport::NONE,
            }),
        })))
        .await
        .expect("original control result admitted");
    while smoke.notices.load(Ordering::Relaxed) != 1 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        smoke
            .trace
            .lock()
            .expect("affinity")
            .iter()
            .find(|(name, _)| *name == "message")
            .expect("actual message consumed")
            .1,
        diagnostic_thread
    );
    let mut bytes = [0; 7];
    sink_reader
        .read_exact(&mut bytes)
        .await
        .expect("release actual sink");
    writing
        .await
        .expect("same runtime task joined")
        .expect("diagnostic actually written");
    assert_eq!(&bytes, b"abcdef\n");
    threads.shutdown().await.expect("real runtime drain");
}

#[tokio::test]
async fn full_original_chat_queue_does_not_take_control_notice_capacity_on_messages_runtime() {
    use as_types::flow::{ChatCursor, ChatEvent, ChatSender, ObservationGap, PreparedChatEvent};
    let (_workspace, context, mut queues) = crate::test_driver::environment();
    queues.messages = NonZeroUsize::MIN;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let smoke = Smoke {
        chat_entered: Some(entered.clone()),
        chat_gate: Some(release.clone()),
        ..Smoke::default()
    };
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("runtimes");
    let modules = smoke.modules();
    let consumer = modules.messages.clone();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules, Box::new(smoke.clone()), stopped)
        .expect("control consumers");
    let event = || {
        PreparedChatEvent::new(ChatEvent::Gap(ObservationGap {
            kind: "fixture".into(),
            exchange: None,
            cursor_range: None,
            reason: "original".into(),
        }))
        .expect("bounded source fact")
    };
    let charge = event().charged_bytes() as u32;
    let (producer, events) = ChatSender::channel(charge);
    let cursors = [
        ChatCursor {
            service_instance: [7; 16],
            sequence: 1,
        },
        ChatCursor {
            service_instance: [7; 16],
            sequence: 2,
        },
    ];
    producer
        .send(&event().into_envelope(cursors[0]))
        .await
        .expect("first original queued");
    let chatting = threads
        .dispatch()
        .spawn(ProcessingThread::Messages, async move {
            consumer.consume_chat(events, None).await
        });
    entered.notified().await;
    producer
        .send(&event().into_envelope(cursors[1]))
        .await
        .expect("original queue fills while its consumer holds current work");
    assert_eq!(producer.queued_bytes(), charge);
    ingress
        .driver_notice(MainMessage::Completed(Box::new(MainNotice {
            decision_id: None,
            event: input(&smoke, 1, 91).event,
            choice: Some(Smoke::choice(Decision::Deny {
                terminate: Termination::None,
            })),
            result: Ok(DispatchOutcome::Driver {
                outcome: ExecutionOutcome::Denied,
                termination: TerminationReport::NONE,
            }),
        })))
        .await
        .expect("control result has independent capacity");
    tokio::time::timeout(Duration::from_secs(1), async {
        while smoke.notices.load(Ordering::Relaxed) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("same Messages runtime continues actual results");
    assert_eq!(
        producer.queued_bytes(),
        charge,
        "control consumption did not steal or drain chat"
    );
    release.notify_one();
    entered.notified().await;
    drop(producer);
    release.notify_one();
    chatting
        .await
        .expect("original chat task join")
        .expect("chat EOF consumed");
    assert_eq!(
        *smoke.chat_cursors.lock().expect("consumed cursors"),
        cursors
    );
    {
        let trace = smoke.trace.lock().expect("thread affinity");
        let messages = trace
            .iter()
            .find(|(name, _)| *name == "message")
            .expect("control consumed")
            .1;
        assert!(
            trace
                .iter()
                .filter(|(name, _)| *name == "chat")
                .all(|(_, thread)| *thread == messages)
        );
    }
    threads.shutdown().await.expect("actual runtime joins");
}

#[tokio::test]
async fn late_direct_hit_batch_does_not_abort_ordered_driver_retirement() {
    use as_types::{driver::DirectHitCount, process_tree::EventKey};
    use std::num::NonZeroU64;

    let (_workspace, _context, queues) = crate::test_driver::environment();
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(smoke.modules(), Box::new(smoke.clone()), &queues, stopped);
    let batch = || {
        let event = EventKey::from(input(&smoke, 1, 1).event.as_ref());
        vec![DirectHitCount {
            host_tgid: NonZeroU32::MIN,
            birth_boottime_ns: 1,
            kind: EventKind::File,
            hit: Smoke::choice(Decision::Allow).hit.expect("real hit shape"),
            count: NonZeroU64::MIN,
            first: event,
            last: event,
        }]
    };
    drop(workers.fast);
    assert!(matches!(
        ingress.native_hits(batch()).await,
        Err(ModuleError::Stopped)
    ));
    ingress.close();
    ingress
        .native_hits(batch())
        .await
        .expect("ordered retirement");
}

#[tokio::test]
async fn native_handoff_uses_auxiliary_consumer_without_taking_control_notice_capacity() {
    use as_types::{
        driver::{DriverAdapter, DriverConnectInput, DriverEvent, NativeSyncThread},
        process_tree::EventKey,
        runtime_observation::{NativeRequestHandoff, NativeRequestPhase},
    };
    let (_workspace, context, mut queues) = crate::test_driver::environment();
    queues.messages = NonZeroUsize::MIN;
    let session = crate::test_driver::SoftwareDriver
        .connect(&DriverConnectInput {
            platform: context.platform(),
            host_boot_id: [1; 16],
            driver_assets_directory: context.paths().driver_assets_directory().to_path_buf(),
        })
        .await
        .expect("explicit software source");
    let smoke = Smoke::default();
    let modules = smoke.modules();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(modules.clone(), Box::new(smoke.clone()), &queues, stopped);
    let (source, events) = tokio::sync::mpsc::channel(2);
    for sequence in 1..=2 {
        source
            .send(DriverEvent::RequestHandoff(NativeRequestHandoff {
                observation: EventKey {
                    source_instance: [3; 16],
                    sequence_domain: 2,
                    sequence,
                },
                request: None,
                operation: None,
                actor: NativeSyncThread {
                    tgid: 7,
                    tid: 8,
                    birth_boottime_ns: 9,
                },
                observed_boottime_ns: sequence,
                phase: NativeRequestPhase::WorkEnter { work_address: 0 },
            }))
            .await
            .expect("original source input");
    }
    drop(source);
    ingress.close();
    crate::native_events::run(
        events,
        crate::messages::driver::DriverPort::bind(session),
        ingress.clone(),
        modules,
        None,
    )
    .await
    .expect("auxiliary originals directly consumed through source EOF");
    assert_eq!(
        ingress.rejected_notices(),
        0,
        "full capacity retains the acquired fact"
    );
    drop(workers.fast);
    drop(workers.main);
    workers
        .messages
        .await
        .expect("control notice queue closes without carrying native facts");
    assert_eq!(
        smoke
            .trace
            .lock()
            .expect("trace")
            .iter()
            .filter(|(name, _)| *name == "request_handoff")
            .count(),
        2
    );
    assert_eq!(ingress.rejected_notices(), 0);
}

#[tokio::test]
async fn saturated_logs_do_not_take_notice_slots_and_both_queues_drain() {
    let (_workspace, _context, mut queues) = crate::test_driver::environment();
    queues.messages = NonZeroUsize::MIN;
    let smoke = Smoke::default();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let (ingress, workers) =
        crate::dispatch::main_dispatch(smoke.modules(), Box::new(smoke.clone()), &queues, stopped);
    let producer = ingress
        .driver_message_producer()
        .expect("actual source producer");
    let log = || {
        let event = input(&smoke, 1, 1).event;
        let mut hit = Smoke::choice(Decision::Allow).hit.expect("fixture hit");
        hit.log = true;
        RuleLogRequest {
            decision_id: None,
            event,
            hit,
            decision: RuleLogDecision::Transferred,
            result: Ok(DispatchOutcome::Observed),
        }
    };
    let close = || as_types::security_log::FileCloseAuditRecord::FileWriteClose {
        reporter_tid: 0,
        path: Some(Vec::new()),
        path_error: None,
        device_major: 0,
        device_minor: 0,
        inode: 0,
    };
    ingress.driver_log(log());
    ingress.driver_log(log()); // Ordinary rule logs keep their rejection accounting.
    let mut pending_close = Box::pin(ingress.driver_file_close(close()));
    assert!(matches!(
        futures::poll!(&mut pending_close),
        std::task::Poll::Pending
    ));
    ingress
        .driver_notice(MainMessage::DriverUnavailable)
        .await
        .expect("consumer accepts actual source failure");
    assert_eq!(ingress.rejected_logs(), 1);
    assert_eq!(ingress.rejected_notices(), 0);
    let crate::dispatch::MainWorkers {
        fast,
        main,
        mut messages,
    } = workers;
    drop(fast);
    drop(main);
    assert!(matches!(
        futures::poll!(&mut messages),
        std::task::Poll::Pending
    ));
    assert_eq!(
        smoke
            .trace
            .lock()
            .expect("trace")
            .iter()
            .map(|(kind, _)| *kind)
            .collect::<Vec<_>>(),
        vec!["driver_unavailable", "rule_log"]
    );
    // The owned close fact reaches the same queue after processing contexts retire.
    pending_close.await;
    drop(producer);
    messages
        .await
        .expect("drain both channels after actual producer ends");
    assert_eq!(smoke.logs.load(Ordering::Relaxed), 2);
    assert_eq!(
        smoke
            .trace
            .lock()
            .expect("trace")
            .last()
            .map(|(kind, _)| *kind),
        Some("file_close")
    );
}

#[derive(Clone)]
struct BlockedAuxiliaryMessages {
    smoke: Smoke,
    blocked: Arc<std::sync::atomic::AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl MessageModule for BlockedAuxiliaryMessages {
    async fn consume_native_fact(&self, fact: NativeFact) -> Result<(), ModuleError> {
        if !self.blocked.swap(true, Ordering::Relaxed) {
            self.smoke.mark("auxiliary_blocked");
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.smoke.consume_native_fact(fact).await
    }
    async fn consume_chat(
        &self,
        events: as_types::flow::ChatReceiver,
        feedback: Option<Arc<dyn as_types::flow::FlowFeedbackPort>>,
    ) -> Result<(), ModuleError> {
        self.smoke.consume_chat(events, feedback).await
    }
    async fn run(
        &self,
        notices: tokio::sync::mpsc::Receiver<MainMessage>,
    ) -> Result<(), ModuleError> {
        self.smoke.run(notices).await
    }
}

#[derive(Debug)]
struct VersionedReviewOriginal {
    inner: Arc<dyn OriginalOperation>,
    version: RuleVersion,
    bound: AtomicUsize,
}
#[async_trait::async_trait]
impl OriginalOperation for VersionedReviewOriginal {
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        self.inner.try_result()
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        self.inner.result().await
    }
    fn decision_id(&self) -> DecisionId {
        self.inner.decision_id()
    }
    fn policy_version(&self) -> Option<RuleVersion> {
        Some(self.version)
    }
    fn hold_window(&self) -> HoldWindow {
        self.inner.hold_window()
    }
    async fn bind_review(&self, binding: ReviewBinding) -> Result<(), ModuleError> {
        assert!(
            matches!(binding.hit.source,RuleSourceVersion::RulesDocument(version) if version==self.version)
        );
        self.inner.bind_review(binding).await?;
        self.bound.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn validate(&self, event: &EventData) -> Result<(), ModuleError> {
        self.inner.validate(event)
    }
    async fn apply(
        &self,
        event: &EventData,
        choice: &RuleChoice,
    ) -> Result<OperationReceipt, ModuleError> {
        self.inner.apply(event, choice).await
    }
    fn cancel(&self) {
        self.inner.cancel();
    }
}

#[tokio::test]
async fn blocked_auxiliary_consumption_does_not_displace_the_original_unified_ask() {
    use as_types::{
        configuration::{ConfigVersion, DriverConfigVersion},
        context::TargetPlatform,
        driver::DriverEvent,
        driver_image::*,
    };
    let (_root, context, mut queues) = crate::test_driver::environment();
    queues.callbacks = NonZeroUsize::MIN;
    queues.messages = NonZeroUsize::MIN;
    std::fs::write(
        context.paths().rules_file(),
        r#"groups:
- kind: process
  id: execute-review
  number: 1
  enabled: true
  execution: {location: backend, stage: ask}
  rules:
  - id: execute-review
    number: 1
    enabled: true
    scope: {kind: all_protected}
    operations: [execute]
    target: {}
    action: {type: ask, timeout_ms: 30000, timeout: {type: deny, terminate: none}}
    log: false
"#,
    )
    .expect("normal process rule");
    let (interaction, mut prompts) =
        as_policy_engine::interaction::Interaction::channel(NonZeroUsize::MIN);
    let _ready = interaction.connect().expect("actual review availability");
    let policy = Arc::new(
        as_policy_engine::PolicyEngine::initialize(&context, interaction)
            .await
            .expect("rule owner"),
    );
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("software runtimes");
    let port = threads.driver_port();
    let (rules, lease) = policy
        .prepare_driver_policy(&port.capabilities().expect("software capabilities"))
        .expect("compiled policy");
    let version = rules.rule_version;
    // Explicit software configuration projection: this case proves queue/thread wiring, not Native enforcement.
    let parts = DriverInitParts {
        expected_persistent_generation: 0,
        expected_lifecycle_epoch: 0,
        configuration: DriverConfigImage {
            config_version: ConfigVersion::from_digest([2; 32]),
            driver_config_version: DriverConfigVersion([3; 32]),
            platform: TargetPlatform::current(),
            encoding_revision: 1,
            immutable_buffer: Arc::from([1u8]),
        },
        rules,
    };
    port.initialize(parts, lease)
        .await
        .expect("declared software adoption");
    let media_entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let applied = Arc::new(tokio::sync::Notify::new());
    let smoke = Smoke {
        started: Some(applied.clone()),
        ..Smoke::default()
    };
    let mut modules = smoke.modules();
    modules.rules = policy;
    modules.slow_facts = None;
    modules.messages = Arc::new(BlockedAuxiliaryMessages {
        blocked: Arc::new(false.into()),
        smoke: smoke.clone(),
        entered: media_entered.clone(),
        release: release.clone(),
    });
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules.clone(), Box::new(smoke.clone()), stopped)
        .expect("existing dispatch");
    let (aux, events) = tokio::sync::mpsc::channel(1);
    let (controlled, originals) = tokio::sync::mpsc::channel(1);
    threads.native_events(originals, events, ingress.clone(), modules);
    let fact = as_types::driver::NativeRequestReturn {
        event: as_types::process_tree::EventKey::from(input(&smoke, 1, 3).event.as_ref()),
        operation: as_types::driver::NativeRequestOperation::Execute,
        decision_id: DecisionId([4; 16]),
        wait_id: 1,
        returned_boottime_ns: 1,
        return_value: 0,
    };
    aux.send(DriverEvent::RequestReturn(fact.clone()))
        .await
        .expect("first auxiliary original");
    media_entered.notified().await;
    aux.send(DriverEvent::RequestReturn(fact))
        .await
        .expect("queued auxiliary original");
    assert_eq!(
        aux.capacity(),
        0,
        "auxiliary source queue is actually full while the original consumer waits"
    );
    let mut work = input(&smoke, 1, 2);
    let operation = Arc::new(VersionedReviewOriginal {
        inner: work.operation.take().expect("same original"),
        version,
        bound: AtomicUsize::new(0),
    });
    let identity = operation.decision_id();
    work.operation = Some(operation.clone());
    controlled
        .send(work)
        .await
        .expect("independent controlled responsibility");
    let prompt = prompts
        .recv()
        .await
        .expect("original Interaction receives the bound question despite auxiliary backpressure");
    assert_eq!(prompt.decision_id, identity);
    assert_eq!(operation.bound.load(Ordering::Relaxed), 1);
    assert_eq!(prompt.hit.rule_id.as_str(), "execute-review");
    prompt
        .reply
        .send(Decision::Allow)
        .expect("same original response");
    applied.notified().await;
    assert_eq!(smoke.executed.load(Ordering::Relaxed), 1);
    {
        let trace = smoke.trace.lock().expect("thread observations");
        let messages = trace
            .iter()
            .find(|(stage, _)| *stage == "auxiliary_blocked")
            .expect("Messages runtime")
            .1;
        let fast = trace
            .iter()
            .find(|(stage, _)| *stage == "basic")
            .expect("Fast runtime")
            .1;
        assert_ne!(messages, fast);
    }
    ingress.close();
    drop(aux);
    drop(controlled);
    release.notify_one();
    threads
        .shutdown()
        .await
        .expect("both source consumers and all original work actually join");
    assert_eq!(smoke.notices.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn completed_controlled_original_needs_no_retired_policy_and_does_not_fail_the_source() {
    let (_root, context, queues) = crate::test_driver::environment();
    let mut threads = crate::test_driver::start(
        context,
        queues,
        Arc::new(crate::test_driver::SoftwareDriver),
    )
    .await
    .expect("software source");
    let smoke = Smoke::default();
    let modules = smoke.modules();
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let ingress = threads
        .connect(modules.clone(), Box::new(smoke.clone()), stopped)
        .expect("dispatch");
    let original = Arc::new(CompletedDuringBinding {
        window: HoldWindow::begin(Duration::from_secs(30)).expect("source window"),
        done: true.into(),
        apply_calls: AtomicUsize::new(0),
        cancel_calls: AtomicUsize::new(0),
    });
    let mut work = input(&smoke, 1, 79);
    work.operation = Some(original.clone());
    let (sending, receiving) = tokio::sync::mpsc::channel(1);
    let port = threads.driver_port();
    let status = port
        .request(as_types::driver_control::DriverCommand::GetStatus)
        .await
        .expect("source status");
    let dispatch = threads.dispatch();
    let source_ingress = ingress.clone();
    let source_port = port.clone();
    let (entered, receiving_started) = tokio::sync::oneshot::channel();
    let task = dispatch.spawn(ProcessingThread::Fast, async move {
        let mut consumer = Box::pin(crate::native_events::controlled(
            receiving,
            source_port,
            source_ingress,
            modules,
            None,
        ));
        assert!(futures::poll!(&mut consumer).is_pending());
        let _ = entered.send(());
        consumer.await
    });
    receiving_started
        .await
        .expect("actual controlled consumer entered");
    sending
        .send(work)
        .await
        .expect("original completed capability");
    ingress.close();
    drop(sending);
    task.await
        .expect("joined controlled consumer")
        .expect("source EOF after admission close");
    assert_eq!(
        port.request(as_types::driver_control::DriverCommand::GetStatus)
            .await
            .expect("same actual source")
            .source_state,
        status.source_state,
        "finished original must not DeclareFailure when its policy is already retired"
    );
    threads
        .shutdown()
        .await
        .expect("all original results consumed");
    assert_eq!(original.apply_calls.load(Ordering::Relaxed), 0);
    assert_eq!(original.cancel_calls.load(Ordering::Relaxed), 0);
    let notices = smoke.completions.lock().expect("completed facts");
    assert_eq!(notices.len(), 1);
    assert!(matches!(
        notices[0].result,
        Ok(DispatchOutcome::Execution(OperationReceipt {
            outcome: ExecutionOutcome::Allowed,
            ..
        }))
    ));
    assert!(notices[0].choice.is_none());
}
