//! Real MainAssembly/Native smoke in a disposable guest; media only observe queue delivery.
#![forbid(unsafe_code)]
#[path = "tls_pipeline_smoke.rs"]
#[allow(dead_code)]
// The module entry point is only used by the separately packaged client executable.
mod tls_pipeline_smoke;
use as_driver_adapter::LinuxDriverAdapter;
use as_main::{
    assembly::{MainAssembly, MainComponents},
    messages::{MainMessages, connections::Connection, protocol::*},
};
use as_types::{
    configuration::ConfigDocument,
    dispatch_event::{EventData, EventPayload},
    driver_control::{DriverCommand, DriverStatus, FeatureState, SourceState},
    main_ports::*,
    security_log::FileCloseAuditRecord,
};
use serde_json::{Value, json};
use std::{
    error::Error,
    io,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{Notify, mpsc},
    time::timeout,
};
type Result<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
fn require(value: bool, message: &str) -> Result {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
fn report(value: Value) {
    println!("{value}");
}
struct BoundaryDiagnostics;
struct Fields(serde_json::Map<String, Value>);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), Value::String(format!("{value:?}")));
    }
}
impl tracing::Subscriber for BoundaryDiagnostics {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(serde_json::Map::new());
        event.record(&mut fields);
        report(
            json!({"event":"native_boundary_diagnostic","level":event.metadata().level().as_str(),"target":event.metadata().target(),"fields":fields.0}),
        );
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
fn path(event: &EventData) -> Option<String> {
    match &event.payload {
        EventPayload::File(file) => file
            .path
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).into_owned()),
        _ => None,
    }
}
#[derive(Clone)]
struct Completed {
    pid: u32,
    kind: EventKind,
    operation: u32,
    rule_id: Option<String>,
    reason: Option<ChoiceReason>,
    outcome: as_types::operation::ExecutionOutcome,
    termination: as_types::operation::TerminationReport,
    target_argv: Option<Vec<Vec<u8>>>,
}
#[derive(Default)]
struct Captures {
    completed: Mutex<Vec<Completed>>,
    changed: Notify,
    observed: Mutex<Vec<Arc<EventData>>>,
}
impl Captures {
    async fn observation(
        &self,
        pid: u32,
        operation: u32,
        cookie: Option<u64>,
    ) -> Result<Arc<EventData>> {
        timeout(Duration::from_secs(10),async{loop {
            let changed=self.changed.notified();
            let found=self.observed.lock().map_err(|_|io::Error::other("observation capture poisoned"))?.iter().find(|event|{
                event.kind()==EventKind::Process && event.header().operation==operation &&
                if operation==2 {matches!(&event.payload,EventPayload::Process(data) if data.child_pid==Some(pid))}
                else{event.header().actor_tgid==Some(pid)&&cookie.is_none_or(|cookie|event.process.exec_cookie==Some(cookie))}
            }).cloned();
            if let Some(event)=found{return Ok(event)}changed.await;
        }}).await?
    }
    async fn completion(&self, pid: u32, kind: EventKind, operation: u32) -> Result<Completed> {
        self.completion_for(pid, kind, operation, None).await
    }
    async fn completion_for(
        &self,
        pid: u32,
        kind: EventKind,
        operation: u32,
        rule: Option<&str>,
    ) -> Result<Completed> {
        self.completion_matching(Some(pid), kind, operation, rule)
            .await
    }
    async fn completion_matching(
        &self,
        pid: Option<u32>,
        kind: EventKind,
        operation: u32,
        rule: Option<&str>,
    ) -> Result<Completed> {
        timeout(Duration::from_secs(10), async {
            loop {
                let changed = self.changed.notified();
                if let Some(value) = self
                    .completed
                    .lock()
                    .map_err(|_| io::Error::other("completion observer poisoned"))?
                    .iter()
                    .find(|v| {
                        pid.is_none_or(|pid| v.pid == pid)
                            && v.kind == kind
                            && v.operation == operation
                            && rule.is_none_or(|rule| v.rule_id.as_deref() == Some(rule))
                    })
                    .cloned()
                {
                    return Ok(value);
                }
                changed.await;
            }
        })
        .await?
    }
}
#[async_trait::async_trait]
impl MessageModule for Captures {
    async fn consume_native_fact(&self, fact: NativeFact) -> std::result::Result<(), ModuleError> {
        match fact {
            NativeFact::RequestHandoff(fact) => {
                report(json!({"event":"native_request_handoff","fact":fact}))
            }
            NativeFact::RequestReturn(fact) => {
                report(json!({"event":"native_request_return","fact":fact}))
            }
            NativeFact::FutexSelection(fact) => {
                report(json!({"event":"native_futex_selection","fact":fact}))
            }
        }
        Ok(())
    }
    async fn run(
        &self,
        mut messages: mpsc::Receiver<MainMessage>,
    ) -> std::result::Result<(), ModuleError> {
        while let Some(message) = messages.recv().await {
            match message {
                MainMessage::Completed(notice) => {
                    if matches!(notice.result, Ok(DispatchOutcome::Observed)) {
                        self.observed
                            .lock()
                            .map_err(|_| ModuleError::Unavailable)?
                            .push(notice.event.clone());
                        self.changed.notify_waiters();
                    }
                    if let Some(message) = operation_result(&notice) {
                        report(
                            json!({"event":"operation_result_media","message":serde_json::to_value(message).map_err(|_|ModuleError::InvalidInput)?}),
                        );
                    }
                    let outcome = match &notice.result {
                        Ok(DispatchOutcome::Execution(receipt)) => {
                            Some((receipt.outcome, receipt.termination))
                        }
                        Ok(DispatchOutcome::Driver {
                            outcome,
                            termination,
                        }) => Some((*outcome, *termination)),
                        Ok(DispatchOutcome::Observed) | Err(_) => None,
                    };
                    if let (Some((outcome, termination)), Some(pid)) =
                        (outcome, notice.event.header().actor_tgid)
                    {
                        let reason = notice.choice.as_ref().map(|v| v.reason);
                        report(
                            json!({"event":"main_completed","pid":pid,"kind":format!("{:?}",notice.event.kind()),"operation":notice.event.header().operation,"path":path(&notice.event),"reason":reason,"outcome":outcome,"termination":termination}),
                        );
                        self.completed
                            .lock()
                            .map_err(|_| ModuleError::Unavailable)?
                            .push(Completed {
                                pid,
                                kind: notice.event.kind(),
                                operation: notice.event.header().operation,
                                rule_id: notice
                                    .choice
                                    .as_ref()
                                    .and_then(|v| v.hit.as_ref())
                                    .map(|hit| hit.rule_id.to_string()),
                                reason,
                                outcome,
                                termination,
                                target_argv: match &notice.event.payload {
                                    EventPayload::Process(process) => process.target_argv.clone(),
                                    _ => None,
                                },
                            });
                        self.changed.notify_waiters();
                    }
                }
                MainMessage::DriverStatus(status) => {
                    report(json!({"event":"main_driver_status","message":driver_status(status)}))
                }
                MainMessage::DriverUnavailable => {
                    report(json!({"event":"main_driver_unavailable"}))
                }
            }
        }
        Ok(())
    }
    async fn consume_chat(
        &self,
        mut events: as_types::flow::ChatReceiver,
        _feedback: Option<std::sync::Arc<dyn as_types::flow::FlowFeedbackPort>>,
    ) -> std::result::Result<(), ModuleError> {
        while let Some(envelope) = events.recv().await {
            report(json!({"event":"original_chat_media","envelope":envelope}));
        }
        Ok(())
    }
}

async fn response(
    messages: &Arc<MainMessages>,
    connection: &Connection,
    down: Down,
) -> Result<CommandResult> {
    let reply = messages
        .submit(connection, down)?
        .ok_or_else(|| io::Error::other("management reply missing"))?
        .await?
        .message;
    report(json!({"event":"management_reply","reply":serde_json::to_value(&reply)?}));
    match reply {
        Up::Reply { result, .. } => Ok(result),
        _ => Err(io::Error::other("management reply type").into()),
    }
}
async fn request(
    messages: &Arc<MainMessages>,
    connection: &Connection,
    down: Down,
) -> Result<ReplyData> {
    match response(messages, connection, down).await? {
        CommandResult::Completed { data } => Ok(data),
        _ => Err(io::Error::other("management effect not completed").into()),
    }
}
async fn status(messages: &Arc<MainMessages>, connection: &Connection) -> Result<DriverStatus> {
    match request(
        messages,
        connection,
        Down::Control {
            id: CommandId([1; 16]),
            command: ControlCommand::GetStatus,
        },
    )
    .await?
    {
        ReplyData::DriverStatus { snapshot } => Ok(snapshot),
        _ => Err(io::Error::other("status reply type").into()),
    }
}
fn subject(name: &str) -> Result<Child> {
    Ok(Command::new("/suite/subject")
        .arg("open")
        .arg(format!("/suite/files/{name}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?)
}
async fn output(child: Child, name: &str, expected_errno: i64) -> Result<u32> {
    let output = timeout(Duration::from_secs(10), child.wait_with_output()).await??;
    require(output.status.success(), "independent helper failed")?;
    original_output(&output.stdout, name, expected_errno)
}
fn original_output(bytes: &[u8], name: &str, expected_errno: i64) -> Result<u32> {
    let value: Value = serde_json::from_slice(bytes)?;
    report(json!({"case":name,"subject":value}));
    require(
        value["errno"].as_i64() == Some(expected_errno),
        "independent original errno differs from expected result",
    )?;
    require(
        value["result"].as_i64() == Some(if expected_errno == 0 { 0 } else { -1 }),
        "independent open result mismatch",
    )?;
    value["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or_else(|| io::Error::other("subject PID missing").into())
}
async fn live_prompt(
    prompts: &mut mpsc::Receiver<as_policy_engine::interaction::Prompt>,
    child: &mut Child,
    messages: &Arc<MainMessages>,
    connection: &Connection,
) -> Result<as_types::DecisionId> {
    let prompt = timeout(Duration::from_secs(10), prompts.recv())
        .await?
        .ok_or_else(|| io::Error::other("prompt channel stopped"))?;
    require(
        prompt.event.header().actor_tgid == child.id(),
        "question did not identify the independent original actor",
    )?;
    require(
        path(&prompt.event).as_deref() == Some("/suite/files/ask"),
        "question target differed from the held original",
    )?;
    require(
        child.try_wait()?.is_none(),
        "original completed before presenting its actual bound question",
    )?;
    let id = prompt.decision_id;
    let ask = messages.question(connection, prompt)?;
    require(
        matches!(ask,Up::Ask{decision_id,..} if decision_id==id),
        "question identity changed",
    )?;
    report(json!({"event":"authenticated_question","message":serde_json::to_value(ask)?}));
    Ok(id)
}
async fn strict_recovery_policy(
    assembly: &MainAssembly,
    messages: &Arc<MainMessages>,
    connection: &Connection,
) -> Result {
    use as_policy_engine::model::{Action, RuleGroup};
    let old = assembly.policy().snapshot();
    let mut document = (*old.document).clone();
    let group = document
        .groups
        .iter_mut()
        .find_map(|group| match group {
            RuleGroup::File(group) if group.id == "driver-file" => Some(group),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("driver fixture file group missing"))?;
    let insertion = group
        .rules
        .iter()
        .map(|rule| rule.number.get())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| io::Error::other("fixture rule order overflow"))?;
    let fallback = group
        .rules
        .iter_mut()
        .find(|rule| rule.id == "driver-other-open")
        .ok_or_else(|| io::Error::other("fixture fallback missing"))?;
    fallback.number =
        std::num::NonZeroU32::new(insertion + 2).ok_or_else(|| io::Error::other("rule order"))?;
    fallback.action = Action::ToUser;
    for (offset,mut value) in [
        json!({"id":"driver-shell-subject","number":4,"enabled":true,"scope":{"kind":"all_protected"},"operations":["open"],"source":{"executable":{"operator":"exact","value":"/bin/busybox"}},"target":{"path":{"operator":"exact","value":"/suite/subject"}},"action":{"type":"allow"},"log":false}),
        json!({"id":"driver-shell-control-executable","number":6,"enabled":true,"scope":{"kind":"all_protected"},"operations":["open"],"source":{"executable":{"operator":"exact","value":"/bin/busybox"}},"target":{"path":{"operator":"exact","value":"/bin/busybox"}},"action":{"type":"allow"},"log":false}),
    ].into_iter().enumerate() {
        value["number"]=json!(insertion+offset as u32);
        group.rules.push(serde_json::from_value(value)?);
    }
    group.rules.sort_by_key(|rule| rule.number);
    match request(
        messages,
        connection,
        Down::Rules {
            id: CommandId([3; 16]),
            command: RuleCommand::Update {
                expected_version: old.version,
                document,
            },
        },
    )
    .await?
    {
        ReplyData::RulesSaved {
            applied: Some(status),
            ..
        } if status.source_state == SourceState::Running
            && status.self_protection == FeatureState::Enabled => {}
        _ => return Err(io::Error::other("strict recovery policy was not adopted").into()),
    }
    report(
        json!({"case":"strict_cold_recovery_policy","default_file_open":"to_user","test_only_narrow_allows":["busybox opens subject executable","busybox opens its own control executable"]}),
    );
    Ok(())
}
async fn verify_process_tree(threads: &as_main::MainThreads, with_child: bool) -> Result {
    use as_types::{
        process_source::{ProcessInspection, ProcessKind},
        process_tree::{Lifecycle, PageRequest, ProcessFilter, ProcessQuery},
    };
    let tree = threads
        .process_tree()
        .ok_or_else(|| io::Error::other("real process tree missing"))?;
    let source = threads.driver_port().process_facts()?;
    let status = tree.status()?;
    require(
        status.phase == as_process_tree::TreePhase::Ready,
        "process tree did not complete real bootstrap",
    )?;
    let key = tree
        .resolve_current_pid(std::process::id())?
        .ok_or_else(|| io::Error::other("service missing from initial process image"))?;
    let binding = source.inspect(&[std::process::id()]).await?;
    require(
        matches!(&binding[..],[ProcessInspection::Bound{binding}] if binding.birth_boottime_ns==key.birth_boottime_ns),
        "tree used proc ticks or another process as kernel identity",
    )?;
    let kernel = tree
        .query(ProcessQuery::List {
            filter: ProcessFilter {
                kind: Some(ProcessKind::Kernel),
                ..Default::default()
            },
            page: PageRequest {
                limit: std::num::NonZeroUsize::MIN,
                cursor: None,
            },
        })
        .await?;
    require(
        matches!(kernel,as_process_tree::QueryReply::Page(ref page) if !page.records.is_empty()),
        "full-system image omitted kernel processes",
    )?;
    if let as_process_tree::QueryReply::Page(page) = &kernel {
        let selected = page
            .records
            .first()
            .ok_or_else(|| io::Error::other("kernel query empty"))?;
        let actual = std::fs::read_to_string(format!("/proc/{}/status", selected.key.host_tgid))?;
        require(
            actual.lines().any(|line| {
                line.strip_prefix("Kthread:")
                    .is_some_and(|value| value.trim() == "1")
            }),
            "tree kernel kind disagreed with independent procfs Kthread flag",
        )?;
    }
    report(
        json!({"case":"real_process_tree_bootstrap","status":status,"service_key":key,"kernel_process_present":true}),
    );
    if !with_child {
        return Ok(());
    }
    let mut child = Command::new("/suite/subject")
        .args(["open", "/suite/files/allow", "--wait"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("child pid missing"))?;
    let mut child_output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child output absent"))?,
    );
    let mut ready = String::new();
    timeout(Duration::from_secs(10), child_output.read_line(&mut ready)).await??;
    let ready: Value = serde_json::from_str(&ready)?;
    require(
        ready["phase"].as_str() == Some("ready") && ready["pid"].as_u64() == Some(u64::from(pid)),
        "actual child did not reach its executable readiness point",
    )?;
    // The actual child waits on stdin. Inspect is an identity query, not a synthetic birth.
    let binding = source.inspect(&[pid]).await?;
    let binding = match binding.into_iter().next() {
        Some(ProcessInspection::Bound { binding }) => binding,
        _ => return Err(io::Error::other("held live child not found by native source").into()),
    };
    source.facts_barrier().await?;
    let child_key = as_types::process_tree::ProcessKey {
        boot_id: key.boot_id,
        host_tgid: pid,
        birth_boottime_ns: binding.birth_boottime_ns,
    };
    let before = tree.get_cached_process(child_key)?;
    require(
        before.record.lifecycle == Lifecycle::Present,
        "child lifecycle was not maintained before exit",
    )?;
    require(
        before.record.creator == Some(key),
        "creation callback did not preserve actual creator",
    )?;
    require(
        before.record.latest.process.executable.as_deref() == Some(b"/suite/subject".as_slice()),
        "exec-success callback did not update image",
    )?;
    let initial = before.record.initial.clone();
    // Explicit software lookup input with a real native-bound process. This proves cache
    // supplementation, not another kernel operation or a protection receipt.
    let sample = source.inspect(&[pid]).await?;
    let time = match sample.as_slice() {
        [ProcessInspection::Bound { binding }] => binding.window.end_boottime_ns,
        _ => return Err(io::Error::other("live child identity changed").into()),
    };
    let mut bytes = vec![0u8; 88];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&88u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&2u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
    bytes[16..32].copy_from_slice(&[44; 16]);
    bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
    bytes[48..56].copy_from_slice(&time.to_le_bytes());
    bytes[56..60].copy_from_slice(&pid.to_le_bytes());
    bytes[60..64].copy_from_slice(&pid.to_le_bytes());
    bytes[64..72].copy_from_slice(&binding.birth_boottime_ns.to_le_bytes());
    bytes[80..84].copy_from_slice(&15u32.to_le_bytes());
    let event = as_types::dispatch_event::decode(&bytes)?;
    let supplement = FastFacts::prepare(tree.as_ref(), &event)?;
    require(
        supplement.executable.as_deref() == Some(b"/suite/subject".as_slice()),
        "quick cache lookup did not fill missing executable",
    )?;
    require(
        supplement.parent_pid == Some(key.host_tgid)
            && supplement.parent_birth_boottime_ns == Some(key.birth_boottime_ns),
        "quick cache lookup lost actual parent identity",
    )?;
    tree.finish_event((&event).into());
    report(
        json!({"case":"tree_fast_supplement","input":"explicit_software_event_real_native_identity","executable":String::from_utf8_lossy(supplement.executable.as_deref().unwrap_or_default())}),
    );
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("child input absent"))?;
    input.write_all(b"G").await?;
    input.shutdown().await?;
    drop(input);
    let mut result = Vec::new();
    timeout(
        Duration::from_secs(10),
        child_output.read_to_end(&mut result),
    )
    .await??;
    require(
        timeout(Duration::from_secs(10), child.wait())
            .await??
            .success(),
        "tree lifecycle helper failed",
    )?;
    original_output(&result, "tree_child_original_open", 0)?;
    source.facts_barrier().await?;
    let after = tree.get_cached_process(child_key)?;
    require(
        after.record.lifecycle == Lifecycle::ExitObserved,
        "group-exit callback did not retire original process",
    )?;
    require(
        Arc::ptr_eq(&initial, &after.record.initial),
        "volatile initial data was rewritten",
    )?;
    require(
        tree.resolve_current_pid(pid)?.is_none(),
        "exited process remained current PID hint",
    )?;
    report(
        json!({"case":"real_process_tree_lifecycle","key":child_key,"lifecycle":after.record.lifecycle,"initial_unchanged":true}),
    );
    Ok(())
}
async fn verify_driver_tools(threads: &as_main::MainThreads) -> Result {
    use as_types::{
        driver_tools::{DriverProcessTarget, KillProcessOutcome},
        process_source::ProcessInspection,
    };
    use std::os::unix::process::ExitStatusExt;
    let port = threads.driver_port();
    let source = port.process_facts()?;
    let instance = port.request(DriverCommand::GetStatus).await?.instance;
    let mut child = Command::new("/suite/subject")
        .args(["open", "/suite/files/allow", "--wait"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("kill target PID missing"))?;
    let mut output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("kill target output missing"))?,
    );
    let mut line = String::new();
    timeout(Duration::from_secs(10), output.read_line(&mut line)).await??;
    let ready: Value = serde_json::from_str(&line)?;
    require(
        ready["phase"] == "ready" && ready["pid"].as_u64() == Some(u64::from(pid)),
        "kill target not ready",
    )?;
    let binding = match source.inspect(&[pid]).await?.into_iter().next() {
        Some(ProcessInspection::Bound { binding }) => binding,
        _ => return Err(io::Error::other("kill target not inspected").into()),
    };
    let target = DriverProcessTarget::from_binding(instance, &binding);
    let mut stale = target;
    stale.birth_boottime_ns ^= 1;
    require(
        matches!(port.kill_process(stale).await, Err(ModuleError::Conflict)),
        "wrong birth was not rejected",
    )?;
    let mut other_instance = target;
    other_instance.driver_instance[0] ^= 1;
    require(
        matches!(
            port.kill_process(other_instance).await,
            Err(ModuleError::Conflict)
        ),
        "wrong driver instance was not rejected",
    )?;
    for number in [0, u32::MAX] {
        let mut invalid = target;
        invalid.host_tgid = number;
        require(
            matches!(
                port.kill_process(invalid).await,
                Err(ModuleError::InvalidInput)
            ),
            "invalid PID was not rejected",
        )?;
    }
    for number in [1, std::process::id(), 2] {
        if let Some(ProcessInspection::Bound { binding }) =
            source.inspect(&[number]).await?.into_iter().next()
        {
            require(
                matches!(
                    port.kill_process(DriverProcessTarget::from_binding(instance, &binding))
                        .await,
                    Err(ModuleError::Forbidden)
                ),
                "protected process was not rejected",
            )?;
        } else {
            return Err(io::Error::other("protected native process missing").into());
        }
    }
    require(
        child.try_wait()?.is_none(),
        "rejected kill changed live target",
    )?;
    require(
        port.kill_process(target).await? == KillProcessOutcome::SignalSent,
        "native kill did not report accepted signal",
    )?;
    let exited = timeout(Duration::from_secs(10), child.wait()).await??;
    require(
        exited.signal() == Some(9),
        "target did not actually exit via SIGKILL",
    )?;
    require(
        port.kill_process(target).await? == KillProcessOutcome::NotRunning,
        "dead target was not explicit",
    )?;
    report(
        json!({"case":"driver_tools_exact_process_kill","pid":pid,"birth":binding.birth_boottime_ns,"stale_birth_rejected":true,"wrong_instance_rejected":true,"invalid_pid_rejected":true,"init_controller_kthread_protected":true,"actual_exit_signal":exited.signal(),"after_exit":"not_running"}),
    );
    Ok(())
}

struct FamilyProbe {
    child: Child,
    reader: BufReader<tokio::process::ChildStdout>,
    actor: u32,
}
impl FamilyProbe {
    async fn start(
        operation: &str,
        target: &str,
        witness: &str,
        scope: Termination,
    ) -> Result<Self> {
        let mut result = Self::prepare(operation, target, witness, scope).await?;
        result.release().await?;
        Ok(result)
    }
    async fn prepare(
        operation: &str,
        target: &str,
        witness: &str,
        scope: Termination,
    ) -> Result<Self> {
        let mut child = Command::new("/suite/rule_kill_probe")
            .args([
                operation,
                target,
                witness,
                if scope == Termination::ActorAndParent {
                    "hold-parent"
                } else {
                    "release-parent"
                },
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let mut reader = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("family output missing"))?,
        );
        let mut ready = String::new();
        timeout(Duration::from_secs(10), reader.read_line(&mut ready)).await??;
        let ready: Value = serde_json::from_str(&ready)?;
        let actor = ready["actor"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| io::Error::other("family actor missing"))?;
        Ok(Self {
            child,
            reader,
            actor,
        })
    }
    async fn release(&mut self) -> Result {
        self.child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("family gate missing"))?
            .write_all(b"g")
            .await?;
        Ok(())
    }
    async fn finish(mut self) -> Result<Value> {
        let mut output = Vec::new();
        timeout(
            Duration::from_secs(10),
            self.reader.read_to_end(&mut output),
        )
        .await??;
        let status = timeout(Duration::from_secs(10), self.child.wait()).await??;
        if !status.success() {
            return Err(io::Error::other(format!("family observer failed: {status}")).into());
        }
        Ok(serde_json::from_slice(&output)?)
    }
}
fn verify_family_decision(actual: &Value, completion: &Completed, decision: Decision) -> Result {
    use as_types::operation::{ExecutionOutcome, SignalOutcome};
    let (allowed, scope) = match decision {
        Decision::Allow => (true, Termination::None),
        Decision::Deny { terminate } => (false, terminate),
    };
    let killed = scope != Termination::None;
    require(
        actual["actor_signal"].as_i64() == Some(if killed { 9 } else { 0 }),
        "actor exit differs from selected scope",
    )?;
    require(
        actual["parent_signal"].as_i64()
            == Some(if scope == Termination::ActorAndParent {
                9
            } else {
                0
            }),
        "parent exit differs from selected scope",
    )?;
    if !killed {
        require(
            actual["actor_exit"].as_i64() == Some(if allowed { 0 } else { 1 }),
            "syscall did not return expected success/EPERM",
        )?;
    }
    require(
        completion.outcome
            == if allowed {
                ExecutionOutcome::Allowed
            } else {
                ExecutionOutcome::Denied
            },
        "native original result differs from actual rule",
    )?;
    require(
        completion.termination.requested == scope,
        "native termination scope differs from rule",
    )?;
    require(
        completion.termination.actor
            == if killed {
                SignalOutcome::SignalSent
            } else {
                SignalOutcome::NotRequested
            },
        "actor signal report differs",
    )?;
    require(
        completion.termination.parent
            == if scope == Termination::ActorAndParent {
                SignalOutcome::SignalSent
            } else {
                SignalOutcome::NotRequested
            },
        "parent signal report differs",
    )?;
    Ok(())
}

async fn verify_rule_termination(
    threads: &as_main::MainThreads,
    captures: &Captures,
    messages: &Arc<MainMessages>,
    connection: &Connection,
    prompts: &mut mpsc::Receiver<as_policy_engine::interaction::Prompt>,
) -> Result {
    use as_types::operation::{ExecutionOutcome, SignalOutcome};
    use std::os::unix::process::ExitStatusExt;
    for (name, scope) in [
        ("driver-none", Termination::None),
        ("driver-actor", Termination::Actor),
        ("driver-parent", Termination::ActorAndParent),
        ("remembered", Termination::Actor),
        ("fast", Termination::ActorAndParent),
        ("answer", Termination::None),
        ("timeout", Termination::ActorAndParent),
        ("disconnect", Termination::ActorAndParent),
    ] {
        let target = format!("/suite/files/kill-{name}");
        let before = std::fs::read(&target)?;
        let ask = matches!(name, "answer" | "timeout" | "disconnect");
        if ask {
            messages.submit(connection, Down::ReviewAvailability { available: true })?;
        }
        let probe = FamilyProbe::start("file", &target, &target, scope).await?;
        let actor = probe.actor;
        if ask {
            let prompt = timeout(Duration::from_secs(10), prompts.recv())
                .await?
                .ok_or_else(|| io::Error::other("kill prompt missing"))?;
            require(
                prompt.event.header().actor_tgid == Some(actor)
                    && path(&prompt.event).as_deref() == Some(target.as_str()),
                "kill prompt identity mismatch",
            )?;
            let id = prompt.decision_id;
            messages.question(connection, prompt)?;
            if name == "answer" {
                messages.submit(
                    connection,
                    Down::Answer {
                        decision_id: id,
                        decision: Decision::Deny { terminate: scope },
                    },
                )?;
            }
            if name == "disconnect" {
                messages.submit(connection, Down::ReviewAvailability { available: false })?;
            }
        }
        let actual = probe.finish().await?;
        require(
            std::fs::read(&target)? == before,
            "denied operation changed protected target",
        )?;
        let completion = captures.completion(actor, EventKind::File, 1).await?;
        verify_family_decision(&actual, &completion, Decision::Deny { terminate: scope })?;
        let reason = if name == "answer" {
            ChoiceReason::User
        } else if ask {
            ChoiceReason::Timeout
        } else {
            ChoiceReason::Direct
        };
        require(
            completion.reason == Some(reason),
            "termination reason was not actually selected",
        )?;
        report(
            json!({"case":"rule_termination","stage":name,"actual_wait":actual,"termination":completion.termination,"original_unchanged":true,"actor_uid":65534,"parent_uid":0}),
        );
    }
    messages.submit(connection, Down::ReviewAvailability { available: false })?;
    let mut child = subject("kill-driver-protected")?;
    let actor = child
        .id()
        .ok_or_else(|| io::Error::other("protected-parent actor missing"))?;
    require(
        timeout(Duration::from_secs(10), child.wait())
            .await??
            .signal()
            == Some(9),
        "protected-parent actor survived",
    )?;
    let completion = captures.completion(actor, EventKind::File, 1).await?;
    require(
        completion.outcome == ExecutionOutcome::Denied
            && completion.termination.actor == SignalOutcome::SignalSent
            && completion.termination.parent == SignalOutcome::Failed { native_error: -1 },
        "protected parent failure did not preserve deny and actor result",
    )?;
    threads
        .driver_port()
        .request(DriverCommand::GetStatus)
        .await?;
    report(
        json!({"case":"rule_termination_protected_parent","termination":completion.termination,"controller_alive":true}),
    );
    Ok(())
}

async fn verify_process_network_rules(captures: &Captures) -> Result {
    use as_types::operation::ExecutionOutcome;
    for (operation, kind, code, base) in [
        ("exec", EventKind::Process, 1, 0u16),
        ("connect", EventKind::Network, 1, 18100),
        ("send", EventKind::Network, 2, 18200),
        ("sendto", EventKind::Network, 2, 18300),
    ] {
        for (stage, offset) in [("driver", 0u16), ("fast", 16)] {
            for (index, (label, decision)) in [
                ("allow", Decision::Allow),
                (
                    "none",
                    Decision::Deny {
                        terminate: Termination::None,
                    },
                ),
                (
                    "actor",
                    Decision::Deny {
                        terminate: Termination::Actor,
                    },
                ),
                (
                    "parent",
                    Decision::Deny {
                        terminate: Termination::ActorAndParent,
                    },
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let scope = match decision {
                    Decision::Allow => Termination::None,
                    Decision::Deny { terminate } => terminate,
                };
                let allowed = decision == Decision::Allow;
                let name = format!("{operation}-{stage}-{label}");
                let witness = format!("/suite/files/{name}");
                let port = base + offset + index as u16;
                let target = if operation == "exec" {
                    format!("/suite/{name}")
                } else {
                    port.to_string()
                };
                let tcp = if matches!(operation, "connect" | "send") {
                    Some(tokio::net::TcpListener::bind(("127.0.0.1", port)).await?)
                } else {
                    None
                };
                let udp = if operation == "sendto" {
                    Some(tokio::net::UdpSocket::bind(("127.0.0.1", port)).await?)
                } else {
                    None
                };
                let probe = FamilyProbe::start(operation, &target, &witness, scope).await?;
                let actor = probe.actor;
                let actual = probe.finish().await?;
                let completion = captures.completion(actor, kind, code).await?;
                verify_family_decision(&actual, &completion, decision)?;
                require(
                    completion.rule_id.as_deref() == Some(name.as_str()),
                    "completion came from a different rule",
                )?;
                let mut witness_bytes = Vec::new();
                let mut connected = false;
                if operation == "exec" {
                    witness_bytes = std::fs::read(&witness)?;
                    require(
                        witness_bytes
                            == if allowed {
                                b"changed".as_slice()
                            } else {
                                b"original".as_slice()
                            },
                        "exec image witness differs from actual allow/deny",
                    )?;
                } else if let Some(tcp) = tcp {
                    if operation == "connect" && !allowed {
                        require(
                            timeout(Duration::from_millis(100), tcp.accept())
                                .await
                                .is_err(),
                            "denied Connect reached listener",
                        )?;
                    } else {
                        let (mut peer, _) = timeout(Duration::from_secs(5), tcp.accept()).await??;
                        connected = true;
                        match timeout(Duration::from_secs(5), peer.read_to_end(&mut witness_bytes))
                            .await?
                        {
                            Ok(_) => {}
                            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
                            Err(error) => return Err(error.into()),
                        }
                        require(
                            witness_bytes
                                == if operation == "send" && allowed {
                                    b"changed".as_slice()
                                } else {
                                    b"".as_slice()
                                },
                            "TCP delivery differs from selected operation rule",
                        )?;
                    }
                } else if let Some(udp) = udp {
                    let mut packet = [0u8; 32];
                    if allowed {
                        let (size, _) =
                            timeout(Duration::from_secs(5), udp.recv_from(&mut packet)).await??;
                        witness_bytes.extend_from_slice(&packet[..size]);
                        require(
                            witness_bytes == b"changed",
                            "UDP allow did not actually deliver bytes",
                        )?;
                    } else {
                        require(
                            timeout(Duration::from_millis(100), udp.recv_from(&mut packet))
                                .await
                                .is_err(),
                            "denied UDP Send delivered bytes",
                        )?;
                    }
                }
                require(
                    completion.outcome
                        == if allowed {
                            ExecutionOutcome::Allowed
                        } else {
                            ExecutionOutcome::Denied
                        },
                    "wrong original result",
                )?;
                report(
                    json!({"case":"process_network_rule","operation":operation,"stage":stage,"action":label,"rule":completion.rule_id,"actual_wait":actual,"termination":completion.termination,"connected":connected,"witness":String::from_utf8_lossy(&witness_bytes),"actor_uid":65534,"parent_uid":0}),
                );
            }
        }
    }
    Ok(())
}

async fn verify_named_creation(
    captures: &Captures,
    messages: &Arc<MainMessages>,
    connection: &Connection,
    prompts: &mut mpsc::Receiver<as_policy_engine::interaction::Prompt>,
) -> Result {
    use as_types::event_operation::FileOperation;

    let denied = Decision::Deny {
        terminate: Termination::None,
    };
    let code = FileOperation::CreateFile as u32;
    messages.submit(connection, Down::ReviewAvailability { available: true })?;
    for (probe, leaf, decision) in [
        ("create-open", "open-allow", Decision::Allow),
        ("create-mknod", "mknod-deny", denied),
    ] {
        let alias = format!("/suite/create-view/{leaf}");
        let backing = format!("/suite/create-real/{leaf}");
        let mut original = FamilyProbe::start(probe, &alias, &alias, Termination::None).await?;
        let prompt = match timeout(Duration::from_secs(10), prompts.recv()).await {
            Ok(Some(prompt)) => prompt,
            _ if original.child.try_wait()?.is_some() => {
                let actual = original.finish().await?;
                return Err(io::Error::other(format!(
                    "named creation returned without Ask: {actual}"
                ))
                .into());
            }
            _ => return Err(io::Error::other("named creation prompt missing").into()),
        };
        let actor = original.actor;
        require(
            original.child.try_wait()?.is_none()
                && prompt.event.header().actor_tgid == Some(actor)
                && prompt.event.header().operation == code
                && path(&prompt.event).as_deref() == Some(alias.as_str())
                && matches!(&prompt.event.payload, EventPayload::File(data)
                    if data.inode.is_none() && data.creation_mode.is_some_and(|mode| mode & 0o777 == 0o600)),
            "create Ask lost the live operation, original bind mount or mode",
        )?;
        let decision_id = prompt.decision_id;
        messages.question(connection, prompt)?;
        messages.submit(
            connection,
            Down::Answer {
                decision_id,
                decision,
            },
        )?;
        let actual = original.finish().await?;
        let completion = captures.completion(actor, EventKind::File, code).await?;
        verify_family_decision(&actual, &completion, decision)?;
        require(
            completion.reason == Some(ChoiceReason::User),
            "create lost its user answer",
        )?;
        if decision == Decision::Allow {
            require(
                std::fs::read(&backing)? == b"created",
                "allowed create did not write the original mount",
            )?;
        } else {
            require(
                matches!(std::fs::metadata(&backing), Err(error) if error.kind() == io::ErrorKind::NotFound),
                "denied create left an object in the original mount",
            )?;
        }
        report(
            json!({"case":"named_create_ask","probe":probe,"path":alias,"answer":decision,"actual_wait":actual}),
        );
    }
    let existing = "/suite/create-view/open-allow";
    let reopened = FamilyProbe::start("create-open", existing, existing, Termination::None)
        .await?
        .finish()
        .await?;
    require(
        reopened["actor_exit"].as_i64() == Some(0) && prompts.try_recv().is_err(),
        "O_CREAT on an existing file produced a creation Ask",
    )?;
    messages.submit(connection, Down::ReviewAvailability { available: false })?;
    Ok(())
}

async fn verify_tls_ingress_rules(
    assembly: &MainAssembly,
    threads: &as_main::MainThreads,
    captures: &Captures,
) -> Result {
    use as_types::{
        driver::TlsIngressInput, process_source::ProcessInspection, process_tree::ProcessKey,
    };
    let tree = threads
        .process_tree()
        .ok_or_else(|| io::Error::other("tree missing"))?;
    let service = tree
        .resolve_current_pid(std::process::id())?
        .ok_or_else(|| io::Error::other("service identity missing"))?;
    for (stage, base) in [("driver", 18200u16), ("fast", 18216u16)] {
        for (offset, label, decision) in [
            (0u16, "allow", Decision::Allow),
            (
                1u16,
                "none",
                Decision::Deny {
                    terminate: Termination::None,
                },
            ),
        ] {
            let port = base + offset;
            let mut probe = FamilyProbe::prepare(
                "sendhold",
                &port.to_string(),
                "/suite/files/send-driver-allow",
                Termination::None,
            )
            .await?;
            let actor = probe.actor;
            let inspection = threads
                .driver_port()
                .process_facts()?
                .inspect(&[actor])
                .await?;
            let birth = match inspection.as_slice() {
                [ProcessInspection::Bound { binding }] => binding.birth_boottime_ns,
                _ => return Err(io::Error::other("held actor identity missing").into()),
            };
            let key = ProcessKey {
                boot_id: service.boot_id,
                host_tgid: actor,
                birth_boottime_ns: birth,
            };
            let (sent, mut admitted) = mpsc::channel(2);
            let ingress = threads
                .driver_port()
                .start_tls_ingress(
                    TlsIngressInput {
                        boot_id: service.boot_id,
                        scope: as_types::flow::FlowScope::Processes(vec![key]),
                        destinations: vec![(std::net::Ipv4Addr::LOCALHOST, port).into()],
                        config_version: assembly.configuration().snapshot().version(),
                        max_connections: 8,
                    },
                    sent,
                )
                .await?;
            probe.release().await?;
            let mut connection = timeout(Duration::from_secs(10), admitted.recv())
                .await?
                .ok_or_else(|| io::Error::other("real redirected actor not admitted"))?;
            require(
                connection.facts.connection_initiator.process == Some(key)
                    && connection.facts.original_remote_port == port
                    && connection.facts.original_remote_ip
                        == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                    && connection.facts.channel.object_id != 0,
                "ingress source/target mismatch",
            )?;
            let completion = captures.completion(actor, EventKind::Network, 2).await?;
            require(
                completion.rule_id.as_deref() == Some(format!("send-{stage}-{label}").as_str()),
                "redirected Send selected wrong original-target rule",
            )?;
            let mut bytes = [0u8; 7];
            if decision == Decision::Allow {
                timeout(
                    Duration::from_secs(5),
                    connection.stream.read_exact(&mut bytes),
                )
                .await??;
                require(bytes == *b"changed", "redirected Send allow bytes missing")?;
            } else {
                require(
                    timeout(
                        Duration::from_millis(100),
                        connection.stream.read(&mut bytes),
                    )
                    .await
                    .is_err(),
                    "denied original-target Send delivered data through redirect",
                )?;
            }
            probe.release().await?;
            let actual = probe.finish().await?;
            verify_family_decision(&actual, &completion, decision)?;
            let stopped = ingress.stop_accepting().await?;
            require(
                !stopped.accepting && stopped.admitted == 1,
                "ingress did not stop/join actual helper",
            )?;
            report(
                json!({"case":"tls_ingress_network_rule","stage":stage,"action":label,"original_remote_port":port,"actual_process":key,"origin_cookie":connection.facts.channel.object_id,"actual_wait":actual,"rule":completion.rule_id,"allowed_bytes":if decision==Decision::Allow{7}else{0},"stopped":!stopped.accepting}),
            );
        }
    }
    Ok(())
}

// Independent procfs oracle. Expected values come from OS files, not the tree DTO.
fn process_oracle(pid: u32) -> Result<as_types::dispatch_event::ProcessInfo> {
    use as_types::{dispatch_event::ProcessInfo, process_source::ProcessKind};
    use std::os::unix::ffi::OsStrExt;
    let root = format!("/proc/{pid}");
    let status = std::fs::read_to_string(format!("{root}/status"))?;
    let values = |key: &str| -> Result<Vec<u32>> {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .ok_or_else(|| io::Error::other(format!("oracle {key} absent")))?
            .split_ascii_whitespace()
            .map(|word| word.parse().map_err(Into::into))
            .collect()
    };
    let ids = |key: &str| -> Result<[u32; 4]> {
        values(key)?
            .try_into()
            .map_err(|_| io::Error::other(format!("oracle {key} width")).into())
    };
    let ns = |name: &str| -> Result<u64> {
        let target = std::fs::read_link(format!("{root}/ns/{name}"))?;
        let target = target
            .to_str()
            .ok_or_else(|| io::Error::other("namespace encoding"))?;
        target
            .split_once('[')
            .and_then(|(_, v)| v.strip_suffix(']'))
            .ok_or_else(|| io::Error::other("namespace shape"))?
            .parse()
            .map_err(Into::into)
    };
    let uid = ids("Uid:")?;
    let gid = ids("Gid:")?;
    let mut name = std::fs::read(format!("{root}/comm"))?;
    if name.last() == Some(&b'\n') {
        name.pop();
    }
    let raw = std::fs::read(format!("{root}/cmdline"))?;
    let argv = if raw.is_empty() {
        Vec::new()
    } else {
        require(
            raw.last() == Some(&0),
            "oracle cmdline lacked final separator",
        )?;
        raw[..raw.len() - 1]
            .split(|b| *b == 0)
            .map(<[u8]>::to_vec)
            .collect()
    };
    let kernel = values("Kthread:")?;
    require(
        matches!(kernel.as_slice(), [0] | [1]),
        "oracle process kind shape",
    )?;
    Ok(ProcessInfo {
        kind: Some(if kernel[0] == 0 {
            ProcessKind::User
        } else {
            ProcessKind::Kernel
        }),
        pid_namespace_inode: Some(ns("pid")?),
        net_namespace_inode: Some(ns("net")?),
        user_namespace_inode: Some(ns("user")?),
        mount_namespace_inode: Some(ns("mnt")?),
        namespace_tgids: Some(values("NStgid:")?),
        name: Some(name),
        executable: Some(
            std::fs::read_link(format!("{root}/exe"))?
                .as_os_str()
                .as_bytes()
                .to_vec(),
        ),
        argv: Some(argv),
        cwd: Some(
            std::fs::read_link(format!("{root}/cwd"))?
                .as_os_str()
                .as_bytes()
                .to_vec(),
        ),
        uid: Some(uid[0]),
        euid: Some(uid[1]),
        suid: Some(uid[2]),
        fsuid: Some(uid[3]),
        gid: Some(gid[0]),
        egid: Some(gid[1]),
        sgid: Some(gid[2]),
        fsgid: Some(gid[3]),
        ..Default::default()
    })
}
fn compare_process_fields(
    actual: &as_types::dispatch_event::ProcessInfo,
    expected: &as_types::dispatch_event::ProcessInfo,
    phase: &str,
    compare_argv: bool,
) -> Result {
    macro_rules! same {
        ($field:ident) => {
            require(
                actual.$field == expected.$field,
                &format!("{phase}: {} differs from procfs oracle", stringify!($field)),
            )?;
        };
    }
    same!(kind);
    same!(pid_namespace_inode);
    same!(net_namespace_inode);
    same!(user_namespace_inode);
    // Lifecycle callbacks have no BPF hook facts; the same tree may already
    // have received this field from an original controlled operation.
    if actual.mount_namespace_inode.is_some() {
        same!(mount_namespace_inode);
    }
    same!(namespace_tgids);
    same!(uid);
    same!(euid);
    same!(suid);
    same!(fsuid);
    same!(gid);
    same!(egid);
    same!(sgid);
    same!(fsgid);
    same!(name);
    same!(executable);
    same!(cwd);
    if compare_argv {
        same!(argv);
    }
    Ok(())
}
fn callback_field_child(args: &[String]) -> Result {
    use std::{
        io::{Read, Write},
        os::unix::process::CommandExt,
    };
    require(
        std::fs::read_to_string("/proc/cmdline")?
            .split_ascii_whitespace()
            .any(|value| value == "as_native_main_smoke=1"),
        "callback child requires disposable guest marker",
    )?;
    require(
        args.len() == 5
            && args[1] == "--callback-fields"
            && matches!(args[2].as_str(), "allow" | "deny"),
        "callback child argv shape",
    )?;
    let stage = &args[3];
    let witness = &args[4];
    std::fs::write(witness, format!("{stage}-executed"))?;
    println!("{}", json!({"phase":stage,"pid":std::process::id()}));
    std::io::stdout().flush()?;
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte)?;
    if stage == "before" {
        require(byte == [b'E'], "callback child exec signal")?;
        let error = std::process::Command::new("/suite/callback_probe")
            .args(["--callback-fields", "allow", "after", witness])
            .current_dir("/suite/files")
            .exec();
        return Err(error.into());
    }
    require(
        stage == "after" && byte == [b'X'],
        "callback child exit signal",
    )?;
    std::process::exit(23);
}
async fn callback_phase(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    pid: u32,
    phase: &str,
) -> Result {
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let value: Value = serde_json::from_str(&line)?;
    report(json!({"case":"callback_child_phase","pid":pid,"stdout":line.as_bytes(),"value":value}));
    require(
        value["pid"].as_u64() == Some(u64::from(pid)) && value["phase"].as_str() == Some(phase),
        "actual callback child phase mismatch",
    )
}
fn callback_subject(label: &str, witness: &str) -> Command {
    let mut command = Command::new("/bin/busybox");
    command
        .args([
            "sh",
            "-c",
            "exec \"$@\"",
            "as-callback",
            "/suite/callback_probe",
            "--callback-fields",
            label,
            "before",
            witness,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
async fn verify_callback_fields(
    assembly: &MainAssembly,
    threads: &as_main::MainThreads,
    captures: &Captures,
    messages: &Arc<MainMessages>,
    connection: &Connection,
) -> Result {
    use as_policy_engine::model::{Rule, RuleGroup};
    use as_types::{
        process_source::ProcessInspection,
        process_tree::{Lifecycle, ProcessKey},
    };
    use std::os::unix::process::ExitStatusExt;
    let old = assembly.policy().snapshot();
    let mut document = (*old.document).clone();
    let group = document
        .groups
        .iter_mut()
        .find_map(|group| match group {
            RuleGroup::Process(group) if group.id == "driver-process" => Some(group),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("process pool absent"))?;
    let mut added = Vec::new();
    for (number, label, action) in [
        (1, "deny", json!({"type":"deny","terminate":"none"})),
        (2, "allow", json!({"type":"allow"})),
    ] {
        let rule: Rule<
            as_policy_engine::model::ProcessTarget,
            as_types::event_operation::ProcessOperation,
        > = serde_json::from_value(
            json!({"id":format!("callback-argv-{label}"),"number":number,"enabled":true,"scope":{"kind":"all_protected"},"operations":["execute"],"source":{},"target":{"executable":{"operator":"exact","value":"/suite/callback_probe"},"arguments":[{"index":2,"pattern":{"operator":"exact","value":label}}]},"action":action,"log":true}),
        )?;
        added.push(rule);
    }
    added.append(&mut group.rules);
    for (number, rule) in added.iter_mut().enumerate() {
        rule.number = std::num::NonZeroU32::new(number as u32 + 1)
            .ok_or_else(|| io::Error::other("rule number"))?;
    }
    group.rules = added;
    require(
        matches!(
            request(
                messages,
                connection,
                Down::Rules {
                    id: CommandId([12; 16]),
                    command: RuleCommand::Update {
                        expected_version: old.version,
                        document
                    }
                }
            )
            .await?,
            ReplyData::RulesSaved {
                applied: Some(_),
                ..
            }
        ),
        "argv rules were not actually adopted",
    )?;
    let witness = "/suite/files/callback-allow";
    let denied_witness = "/suite/files/callback-deny";
    std::fs::write(witness, b"original")?;
    std::fs::write(denied_witness, b"original")?;
    let denied = callback_subject("deny", denied_witness)
        .stdin(Stdio::null())
        .spawn()?;
    let denied_pid = denied
        .id()
        .ok_or_else(|| io::Error::other("denied child pid"))?;
    let denied_output = denied.wait_with_output().await?;
    let denied_bytes = std::fs::read(denied_witness)?;
    report(
        json!({"case":"callback_argv_deny_process","pid":denied_pid,"status":denied_output.status.into_raw(),
        "stdout":denied_output.stdout,"stderr":denied_output.stderr,"witness":denied_bytes}),
    );
    require(
        !denied_output.status.success(),
        "target argv Deny did not reject the real exec",
    )?;
    require(
        denied_bytes == b"original",
        "denied target executed witness code",
    )?;
    let deny = captures
        .completion_for(
            denied_pid,
            EventKind::Process,
            1,
            Some("callback-argv-deny"),
        )
        .await?;
    require(
        deny.outcome == as_types::operation::ExecutionOutcome::Denied
            && deny
                .target_argv
                .as_ref()
                .and_then(|argv| argv.get(2))
                .map(Vec::as_slice)
                == Some(b"deny"),
        "Deny was not matched on this child's real target argv",
    )?;
    let denied_creation = captures.observation(deny.pid, 2, None).await?;
    require(
        denied_creation.header().actor_tgid == Some(std::process::id())
            && matches!(&denied_creation.payload,EventPayload::Process(data) if data.child_pid==Some(deny.pid)),
        "Deny receipt did not identify this actual forked exec child",
    )?;
    let parent = process_oracle(std::process::id())?;
    let source = threads.driver_port().process_facts()?;
    let tree = threads
        .process_tree()
        .ok_or_else(|| io::Error::other("tree absent"))?;
    let self_key = tree
        .resolve_current_pid(std::process::id())?
        .ok_or_else(|| io::Error::other("service key absent"))?;
    let mut child = callback_subject("allow", witness)
        .stderr(Stdio::inherit())
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("callback child pid"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("callback child input"))?;
    let mut output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("callback child output"))?,
    );
    callback_phase(&mut output, pid, "before").await?;
    source.facts_barrier().await?;
    let binding = match source.inspect(&[pid]).await?.pop() {
        Some(ProcessInspection::Bound { binding }) => binding,
        _ => return Err(io::Error::other("live callback child binding").into()),
    };
    let key = ProcessKey {
        boot_id: self_key.boot_id,
        host_tgid: pid,
        birth_boottime_ns: binding.birth_boottime_ns,
    };
    let first = tree.get_cached_process(key)?;
    let created = captures.observation(pid, 2, None).await?;
    let raw_child = match &created.payload {
        EventPayload::Process(data) => data
            .child_process
            .as_ref()
            .ok_or_else(|| io::Error::other("fork callback lacked child facts"))?,
        _ => return Err(io::Error::other("fork payload kind").into()),
    };
    report(json!({
        "case":"callback_initial_argv_origin",
        "pid":pid,
        "initial_argv":first.record.initial.process.argv,
        "initial_argv_stamp":first.record.initial.stamps.get(&as_process_tree::Field::Argv),
        "raw_create_key":as_types::process_tree::EventKey::from(created.as_ref()),
        "raw_child_argv":raw_child.argv,
        "first_observed":first.record.first_observed,
    }));
    let oracle = process_oracle(pid)?;
    let mut fork_oracle = parent.clone();
    fork_oracle.namespace_tgids = oracle.namespace_tgids.clone();
    compare_process_fields(
        &first.record.initial.process,
        &fork_oracle,
        "fork initial",
        false,
    )?;
    require(
        raw_child.argv.is_none(),
        "fork callback fabricated foreign argv",
    )?;
    // Initial is the first accepted view; a child's own pre-exec callback can arrive first.
    require(
        first.record.initial.process.argv.is_none()
            || first.record.initial.process.argv == parent.argv,
        "first accepted child argv disagreed with actual inherited parent argv",
    )?;
    compare_process_fields(&first.record.latest.process, &oracle, "first exec", true)?;
    let fork_cookie = raw_child
        .exec_cookie
        .ok_or_else(|| io::Error::other("fork cookie absent"))?;
    let busybox_cookie = fork_cookie
        .checked_add(1)
        .ok_or("busybox cookie overflow")?;
    let busybox_exec = captures.observation(pid, 1, Some(busybox_cookie)).await?;
    require(
        busybox_exec.process.executable.as_deref() == Some(b"/bin/busybox")
            && busybox_exec.process.exec_cookie == Some(busybox_cookie)
            && first.record.latest.process.exec_cookie == Some(binding.exec_cookie)
            && busybox_cookie.checked_add(1) == Some(binding.exec_cookie),
        "actual fork -> BusyBox -> callback exec lifecycle/cookies differ",
    )?;
    require(
        std::fs::read(witness)? == b"before-executed",
        "allowed target did not execute",
    )?;
    compare_process_fields(raw_child, &fork_oracle, "raw fork callback", false)?;
    require(
        raw_child.exec_cookie == first.record.initial.process.exec_cookie,
        "raw fork cookie was not preserved in initial view",
    )?;
    let raw_exec = captures
        .observation(pid, 1, Some(binding.exec_cookie))
        .await?;
    compare_process_fields(&raw_exec.process, &oracle, "first exec callback", true)?;
    require(
        raw_exec.process.argv == oracle.argv && raw_exec.process.cwd == oracle.cwd,
        "first successful exec did not carry actual argv/cwd",
    )?;
    let initial = first.record.initial.clone();
    let allow = captures
        .completion_for(pid, EventKind::Process, 1, Some("callback-argv-allow"))
        .await?;
    require(
        allow.outcome == as_types::operation::ExecutionOutcome::Allowed
            && allow
                .target_argv
                .as_ref()
                .and_then(|argv| argv.get(2))
                .map(Vec::as_slice)
                == Some(b"allow"),
        "Allow did not match actual target arguments",
    )?;
    input.write_all(b"E").await?;
    callback_phase(&mut output, pid, "after").await?;
    source.facts_barrier().await?;
    let after_binding = match source.inspect(&[pid]).await?.pop() {
        Some(ProcessInspection::Bound { binding }) => binding,
        _ => return Err(io::Error::other("re-exec child binding").into()),
    };
    let second = tree.get_cached_process(key)?;
    let oracle = process_oracle(pid)?;
    compare_process_fields(&second.record.latest.process, &oracle, "second exec", true)?;
    require(
        after_binding.exec_cookie == binding.exec_cookie + 1
            && second.record.latest.process.exec_cookie == Some(after_binding.exec_cookie),
        "same-PID successful exec did not advance cookie",
    )?;
    require(
        oracle.cwd.as_deref() == Some(b"/suite/files")
            && oracle
                .argv
                .as_ref()
                .and_then(|v| v.get(3))
                .map(Vec::as_slice)
                == Some(b"after"),
        "independent post-exec cwd/argv did not change",
    )?;
    let raw_exec = captures
        .observation(pid, 1, Some(after_binding.exec_cookie))
        .await?;
    compare_process_fields(&raw_exec.process, &oracle, "second exec callback", true)?;
    require(
        raw_exec.process.argv == oracle.argv && raw_exec.process.cwd == oracle.cwd,
        "second successful exec did not carry new argv/cwd",
    )?;
    require(
        Arc::ptr_eq(&initial, &second.record.initial),
        "exec rewrote fork initial data",
    )?;
    let refreshed = tree.get_process(key).await?;
    let refreshed_oracle = process_oracle(pid)?;
    require(
        refreshed.record.latest.process.mount_namespace_inode
            == refreshed_oracle.mount_namespace_inode
            && refreshed_oracle.mount_namespace_inode.is_some(),
        "detail read did not obtain actual mount namespace",
    )?;
    let cached_again = tree.get_process(key).await?;
    require(
        Arc::ptr_eq(&refreshed.record.latest, &cached_again.record.latest),
        "repeated detail read repeated process I/O",
    )?;
    require(
        Arc::ptr_eq(&initial, &refreshed.record.initial),
        "mount refresh rewrote raw fork initial snapshot",
    )?;
    input.write_all(b"X").await?;
    input.shutdown().await?;
    drop(input);
    let exit = child.wait().await?;
    require(
        exit.code() == Some(23),
        "callback child did not reach actual exit status 23",
    )?;
    let raw = exit.into_raw();
    source.facts_barrier().await?;
    let exited = tree.get_cached_process(key)?;
    let exit_event = captures.observation(pid, 3, None).await?;
    require(
        matches!(&exit_event.payload,EventPayload::Process(data) if data.exit_code==Some(raw)),
        "group exit callback differed from independent wait status",
    )?;
    require(
        exited.record.lifecycle == Lifecycle::ExitObserved && exited.record.exit_code == Some(raw),
        "group exit did not retain actual wait status",
    )?;
    require(
        std::fs::read(witness)? == b"after-executed",
        "second exec did not execute witness code",
    )?;
    report(
        json!({"case":"callback_process_fields","kind_namespaces_creds":"compared_each_field_with_procfs","native_lifecycle_namespaces":3,"native_lifecycle_mount_missing":raw_exec.process.mount_namespace_inode.is_none(),"resolved_mount_matches_proc":true,"fork_initial_retained":true,"fork_to_callback_exec_cookie_delta":binding.exec_cookie-fork_cookie,"busybox_exec_cookie":busybox_cookie,"callback_reexec_cookie_delta":after_binding.exec_cookie-binding.exec_cookie,"new_argv_cwd":true,"exit_code_raw":raw,"target_argv_allow_deny":true,"denied_exec_actor":deny.pid}),
    );
    Ok(())
}
const CLOSE_ROOT: &str = "/suite/audit-fs/watch";
const CLOSE_MOVED: &str = "/suite/audit-fs/watch-moved";

fn close_oracle(path: &Path, file: &std::fs::File) -> Result<Value> {
    let stat = rustix::fs::fstat(file)?;
    Ok(serde_json::to_value(
        FileCloseAuditRecord::FileWriteClose {
            reporter_tid: rustix::thread::gettid().as_raw_pid(),
            path: Some(path.as_os_str().as_bytes().to_vec()),
            path_error: None,
            device_major: rustix::fs::major(stat.st_dev),
            device_minor: rustix::fs::minor(stat.st_dev),
            inode: stat.st_ino,
        },
    )?)
}
async fn close_records(
    messages: &Arc<MainMessages>,
    connection: &Connection,
) -> Result<Vec<Value>> {
    let mut before = None;
    let mut records = Vec::new();
    loop {
        match response(
            messages,
            connection,
            Down::SecurityLog {
                id: CommandId([0x61; 16]),
                command: as_security_log::SecurityLogQuery { before, limit: 64 },
            },
        )
        .await?
        {
            CommandResult::Completed {
                data: ReplyData::SecurityLog { page },
            } => {
                for row in page.records {
                    if let SecurityLogEntry::FileClose(record) = row.record {
                        records.push(serde_json::to_value(record)?);
                    }
                }
                match page.next_before {
                    Some(next) => before = Some(next),
                    None => return Ok(records),
                }
            }
            CommandResult::Failed {
                code: FailureCode::Busy,
                ..
            } => tokio::task::yield_now().await,
            _ => return Err(io::Error::other("security log query did not complete").into()),
        }
    }
}
async fn close_committed(
    messages: &Arc<MainMessages>,
    connection: &Connection,
    path: &Value,
) -> Result<Vec<Value>> {
    loop {
        let records = close_records(messages, connection).await?;
        require(
            records
                .iter()
                .all(|record| record["path"].is_array() && record["path_error"].is_null()),
            "normal write-close path acquisition failed; see original log reply",
        )?;
        if records.iter().any(|record| record["path"] == *path) {
            return Ok(records);
        }
        require(
            status(messages, connection).await?.source_state == SourceState::Running,
            "write-close source ended before expected committed record",
        )?;
        tokio::task::yield_now().await;
    }
}
fn check_close_records(actual: &[Value], expected: &[Value]) -> Result {
    report(json!({"case":"write_close_records","expected":expected,"actual":actual}));
    require(
        actual.len() == expected.len()
            && expected
                .iter()
                .all(|record| actual.iter().filter(|value| *value == record).count() == 1),
        "write-close records differ from original fstat/gettid and released file references",
    )
}
async fn verify_write_close(
    assembly: &MainAssembly,
    messages: &Arc<MainMessages>,
    connection: &Connection,
) -> Result<Vec<Value>> {
    let directory = Path::new(CLOSE_ROOT);
    let readonly = directory.join("readonly");
    drop(std::fs::File::open(&readonly)?);
    report(json!({"case":"write_close_readonly_input","path":readonly.as_os_str().as_bytes()}));
    let mut expected = Vec::new();
    for name in [
        std::ffi::OsString::from("unmodified"),
        std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()),
    ] {
        let path = directory.join(name);
        let file = std::fs::OpenOptions::new().write(true).open(&path)?;
        expected.push(close_oracle(&path, &file)?);
        drop(file);
        require(
            std::fs::read(&path)? == b"original",
            "write-open without content change modified its input",
        )?;
    }
    let duplicate_path = directory.join("duplicate");
    let original = std::fs::OpenOptions::new()
        .write(true)
        .open(&duplicate_path)?;
    let duplicate = original.try_clone()?;
    drop(original);
    let barrier = directory.join("barrier");
    let file = std::fs::OpenOptions::new().write(true).open(&barrier)?;
    expected.push(close_oracle(&barrier, &file)?);
    drop(file);
    // This real committed record follows prior close notifications in the same source/log FIFO.
    let records = close_committed(
        messages,
        connection,
        &expected.last().ok_or("barrier oracle")?["path"],
    )
    .await?;
    check_close_records(&records, &expected)?;
    expected.push(close_oracle(&duplicate_path, &duplicate)?);
    drop(duplicate);
    let records = close_committed(
        messages,
        connection,
        &expected.last().ok_or("duplicate oracle")?["path"],
    )
    .await?;
    check_close_records(&records, &expected)?;

    std::fs::rename(CLOSE_ROOT, CLOSE_MOVED)?;
    std::fs::create_dir(CLOSE_ROOT)?;
    std::fs::write(directory.join("decoy"), b"original")?;
    let version = assembly.policy().snapshot().version;
    require(
        matches!(
            request(
                messages,
                connection,
                Down::Rules {
                    id: CommandId([0x62; 16]),
                    command: RuleCommand::Reload {
                        expected_version: version
                    },
                }
            )
            .await?,
            ReplyData::RulesReloaded { .. }
        ),
        "ordinary rule reload lacked actual native adoption",
    )?;
    let path = Path::new(CLOSE_MOVED).join("after-reload");
    let file = std::fs::OpenOptions::new().write(true).open(&path)?;
    expected.push(close_oracle(&path, &file)?);
    drop(file);
    let records = close_committed(
        messages,
        connection,
        &expected.last().ok_or("renamed oracle")?["path"],
    )
    .await?;
    check_close_records(&records, &expected)?;
    std::fs::remove_file(directory.join("decoy"))?;
    std::fs::remove_dir(CLOSE_ROOT)?;
    std::fs::rename(CLOSE_MOVED, CLOSE_ROOT)?;

    loop {
        let output = Command::new("/suite/umount_probe")
            .arg("/suite/audit-fs")
            .output()
            .await?;
        report(
            json!({"case":"write_close_umount_output","exit":output.status.code(),"stdout":output.stdout,"stderr":output.stderr}),
        );
        require(
            output.status.success(),
            "owned umount helper did not execute",
        )?;
        let value: Value = serde_json::from_slice(&output.stdout)?;
        if value["result"] == 0 && value["errno"] == 0 {
            break;
        }
        require(
            value["result"] == -1
                && value["errno"].as_i64()
                    == Some(i64::from(rustix::io::Errno::BUSY.raw_os_error())),
            "normal audit tmpfs unmount returned a non-busy failure",
        )?;
        tokio::task::yield_now().await;
    }
    report(
        json!({"case":"write_close_directory_reload_and_unmount","old_name_replacement_not_bound":true,"original_name_restored":true,"normal_umount":true}),
    );
    Ok(expected)
}
async fn run(mode: &str) -> Result {
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"marker_read_begin"}));
    require(
        std::fs::read_to_string("/proc/cmdline")?
            .split_ascii_whitespace()
            .any(|s| s == "as_native_main_smoke=1"),
        "formal native smoke requires disposable guest marker",
    )?;
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"marker_read_complete"}));
    if mode == "--guest-main-smoke" {
        report(
            json!({"scope":"formal_main_assembly_native_self","mode":mode,"kernel":std::fs::read_to_string("/proc/sys/kernel/osrelease")?.trim(),"sources":serde_json::from_slice::<Value>(&std::fs::read("/suite/source-main-sha256.json")?)?,"fast_facts":"real_process_tree","logs":"backend_security_sqlite"}),
        );
    } else {
        report(json!({"scope":"formal_main_assembly_native_self","mode":mode}));
    }
    if mode == "--guest-main-smoke" {
        for name in [
            std::ffi::OsString::from("readonly"),
            "unmodified".into(),
            "duplicate".into(),
            "barrier".into(),
            "after-reload".into(),
            std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()),
        ] {
            std::fs::write(Path::new(CLOSE_ROOT).join(name), b"original")?;
        }
    }
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"initialize_begin"}));
    let mut assembly = MainAssembly::initialize().await?;
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"initialize_complete"}));
    let mut probe = if mode == "--guest-main-smoke" {
        let mut child = Command::new("/suite/self_probe")
            .args(["/as/data/self-probe", "/suite/self-probe-alias"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Self probe input missing"))?;
        let mut reader = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Self probe output missing"))?,
        );
        let mut ready = String::new();
        timeout(Duration::from_secs(10), reader.read_line(&mut ready)).await??;
        let ready: Value = serde_json::from_str(&ready)?;
        report(json!({"case":"self_probe_prepared_before_activation","probe":ready}));
        require(
            ready["phase"].as_str() == Some("ready"),
            "preexisting Self probe resources were not ready",
        )?;
        Some((child, input, reader))
    } else {
        None
    };
    let mut prompts = assembly
        .take_prompts()
        .ok_or_else(|| io::Error::other("prompt receiver missing"))?;
    let captures = Arc::new(Captures::default());
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"start_begin"}));
    let threads = assembly
        .start(
            Arc::new(LinuxDriverAdapter),
            MainComponents {
                messages: captures.clone(),
            },
            stopped,
        )
        .await?;
    report(json!({"event":"formal_startup_phase","mode":mode,"phase":"start_complete"}));
    let actual = threads
        .driver_port()
        .request(DriverCommand::GetStatus)
        .await?;
    require(
        actual.source_state == SourceState::Running
            && actual.self_protection == FeatureState::Enabled,
        "formal startup did not physically enable self protection",
    )?;
    report(json!({"case":"formal_main_initialized","status":actual}));
    if mode == "--guest-tls-only" {
        tls_pipeline_smoke::verify(&assembly, &threads).await?;
        threads.shutdown().await?;
        return Ok(());
    }
    verify_process_tree(&threads, mode == "--guest-main-smoke").await?;
    if let Some((mut child, mut input, mut reader)) = probe.take() {
        input.write_all(b"G").await?;
        input.shutdown().await?;
        drop(input);
        let mut output = Vec::new();
        timeout(Duration::from_secs(10), reader.read_to_end(&mut output)).await??;
        print!("{}", String::from_utf8(output)?);
        require(
            timeout(Duration::from_secs(10), child.wait())
                .await??
                .success(),
            "actual Self protection side-effect probe failed",
        )?;
    }
    let messages = assembly.messages(&threads);
    let (stream, _peer) = tokio::net::UnixStream::pair()?;
    let connection = messages.connect(stream)?;
    require(
        connection.uid() == Some(0),
        "Unix peer credential was not root",
    )?;
    let own_key = threads
        .process_tree()
        .ok_or_else(|| io::Error::other("tree missing"))?
        .resolve_current_pid(std::process::id())?
        .ok_or_else(|| io::Error::other("service key missing"))?;
    for query in [
        as_types::process_tree::ProcessQuery::Status,
        as_types::process_tree::ProcessQuery::Get { key: own_key },
    ] {
        let data = request(
            &messages,
            &connection,
            Down::Processes {
                id: CommandId([8; 16]),
                command: ProcessCommand::Query {
                    query: Box::new(query),
                },
            },
        )
        .await?;
        require(
            matches!(data, ReplyData::Processes { .. }),
            "authenticated process query did not reach tree",
        )?;
    }
    report(json!({"case":"authenticated_process_queries","status_and_detail":true}));
    if mode == "--guest-create-only" {
        verify_named_creation(&captures, &messages, &connection, &mut prompts).await?;
        messages.close()?;
        threads.shutdown().await?;
        println!("AS_MAIN_NAMED_CREATE_PASS");
        return Ok(());
    }
    if mode == "--guest-main-smoke" {
        verify_callback_fields(&assembly, &threads, &captures, &messages, &connection).await?;
        verify_driver_tools(&threads).await?;
        verify_rule_termination(&threads, &captures, &messages, &connection, &mut prompts).await?;
        verify_named_creation(&captures, &messages, &connection, &mut prompts).await?;
        verify_process_network_rules(&captures).await?;
        verify_tls_ingress_rules(&assembly, &threads, &captures).await?;
        tls_pipeline_smoke::verify(&assembly, &threads).await?;
    }
    if mode == "--guest-arm-cold-recovery" {
        strict_recovery_policy(&assembly, &messages, &connection).await?;
        println!("AS_MAIN_COLD_RECOVERY_ARMED");
        // Deliberately exit the actual controller without MainThreads::shutdown.
        std::process::exit(77);
    }
    if mode == "--guest-cold-recovery" {
        let recovered = status(&messages, &connection).await?;
        require(
            recovered.self_protection == FeatureState::Enabled,
            "cold recovery lost actual self protection",
        )?;
        messages.close()?;
        threads.shutdown().await?;
        println!("AS_MAIN_COLD_RECOVERY_PASS");
        return Ok(());
    }
    require(mode == "--guest-main-smoke", "unknown smoke entry")?;
    let pid = output(subject("allow")?, "formal_direct_allow", 0).await?;
    require(
        captures.completion(pid, EventKind::File, 1).await?.outcome
            == as_types::operation::ExecutionOutcome::Allowed,
        "main did not observe actual direct Allow",
    )?;
    let pid = output(subject("deny")?, "formal_direct_deny", 1).await?;
    require(
        captures.completion(pid, EventKind::File, 1).await?.outcome
            == as_types::operation::ExecutionOutcome::Denied,
        "main did not observe actual direct Deny",
    )?;
    messages.submit(&connection, Down::ReviewAvailability { available: true })?;
    let mut child = subject("ask")?;
    let decision_id = live_prompt(&mut prompts, &mut child, &messages, &connection).await?;
    messages.submit(
        &connection,
        Down::Answer {
            decision_id,
            decision: Decision::Allow,
        },
    )?;
    let pid = output(child, "formal_answer_allow", 0).await?;
    let completed = captures.completion(pid, EventKind::File, 1).await?;
    require(
        completed.reason == Some(ChoiceReason::User)
            && completed.outcome == as_types::operation::ExecutionOutcome::Allowed,
        "answer was not the source-confirmed original user choice",
    )?;
    let mut child = subject("ask")?;
    live_prompt(&mut prompts, &mut child, &messages, &connection).await?;
    let pid = output(child, "formal_bound_timeout", 1).await?;
    let completed = captures.completion(pid, EventKind::File, 1).await?;
    require(
        completed.reason == Some(ChoiceReason::Timeout)
            && completed.outcome == as_types::operation::ExecutionOutcome::Denied,
        "bound fallback was not source-confirmed",
    )?;
    messages.submit(&connection, Down::ReviewAvailability { available: false })?;
    let pid = output(subject("ask")?, "formal_no_frontend_skips_ask", 0).await?;
    let completed = captures.completion(pid, EventKind::File, 1).await?;
    require(
        completed.reason == Some(ChoiceReason::NoInteraction)
            && completed.outcome == as_types::operation::ExecutionOutcome::Allowed,
        "offline flow entered Ask or changed quick-only behavior",
    )?;
    require(
        matches!(prompts.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
        "offline flow produced a question",
    )?;
    let headless_pid = pid;
    let write = Command::new("/suite/write_probe")
        .arg(assembly.configuration().shared_paths().configuration_file())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .await?;
    require(write.status.success(), "write probe failed")?;
    let value: Value = serde_json::from_slice(&write.stdout)?;
    report(json!({"case":"independent_self_write_denied","subject":value}));
    require(
        value["errno"].as_i64() == Some(1) && value["open_result"].as_i64() == Some(-1),
        "self protection allowed independent configuration write-open",
    )?;
    // Run the original ordinary protection workflow before the close-audit lifecycle boundary.
    let close_expected = verify_write_close(&assembly, &messages, &connection).await?;
    let before = assembly.configuration().snapshot();
    let mut document = (**before.document()).clone();
    document.driver.startup_protection.network = false;
    let saved = request(
        &messages,
        &connection,
        Down::Configuration {
            id: CommandId([2; 16]),
            command: ConfigCommand::Update {
                expected_version: before.version(),
                document: Box::new(document.clone()),
            },
        },
    )
    .await?;
    match saved {
        ReplyData::ConfigurationSaved {
            applied: Some(applied),
            ..
        } => {
            require(
                applied.self_protection == FeatureState::Enabled
                    && applied.persistent_generation > actual.persistent_generation,
                "owner save was not adopted with self protection",
            )?;
            require(
                applied.network.effective_state == FeatureState::Disabled,
                "updated network default not physically reported",
            )?;
        }
        _ => return Err(io::Error::other("configuration save lacked native adoption").into()),
    }
    let disk: ConfigDocument = serde_saphyr::from_str(&std::fs::read_to_string(
        assembly.configuration().shared_paths().configuration_file(),
    )?)?;
    require(
        disk == document,
        "sole configuration owner did not persist the actual document",
    )?;
    report(
        json!({"case":"authenticated_owner_configuration_update","saved_on_disk":true,"native_adopted":true}),
    );
    // A dead selected mount must not attach its same-named underlying directory on ordinary replacement.
    drop(
        std::fs::OpenOptions::new()
            .write(true)
            .open(Path::new(CLOSE_ROOT).join("underlying"))?,
    );
    messages.close()?;
    threads.shutdown().await?;
    let log = rusqlite::Connection::open_with_flags(
        assembly.context().paths().security_log_file(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut rows = log.prepare("SELECT record FROM rule_logs ORDER BY id")?;
    let mut rows = rows.query([])?;
    let mut observed_ask = false;
    let mut close_actual = Vec::new();
    while let Some(row) = rows.next()? {
        let entry: SecurityLogEntry = serde_json::from_str(&row.get::<_, String>(0)?)?;
        let SecurityLogEntry::Rule(log) = entry else {
            if let SecurityLogEntry::FileClose(record) = entry {
                close_actual.push(serde_json::to_value(record)?);
            }
            continue;
        };
        if log.hit.rule_id.as_str() == "backend-ask" {
            observed_ask = true;
            require(
                log.event.header().actor_tgid != Some(headless_pid),
                "offline flow invented an Ask hit log",
            )?;
        }
    }
    require(observed_ask, "no completed Ask hit was persisted")?;
    check_close_records(&close_actual, &close_expected)?;
    report(
        json!({"case":"write_close_native_sqlite_complete","query_and_sqlite_agree":true,"same_named_unmounted_directory_not_rebound":true}),
    );
    println!("AS_MAIN_NATIVE_SMOKE_PASS");
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = tracing::subscriber::set_global_default(BoundaryDiagnostics) {
        report(json!({"result":"fail","error":error.to_string()}));
        std::process::exit(1);
    }
    let args = std::env::args().collect::<Vec<_>>();
    if args
        .get(1)
        .is_some_and(|value| value == "--callback-fields")
    {
        if let Err(error) = callback_field_child(&args) {
            report(json!({"result":"fail","error":error.to_string()}));
            std::process::exit(1);
        }
        return;
    }
    let mode = args.get(1).cloned().unwrap_or_default();
    if let Err(error) = run(&mode).await {
        report(
            json!({"scope":"formal_main_assembly_native_self","result":"fail","error":error.to_string(),"error_debug":format!("{error:?}")}),
        );
        std::process::exit(1);
    }
}
