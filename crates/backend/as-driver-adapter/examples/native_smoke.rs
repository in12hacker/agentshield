//! Real native device diagnostic. Run only inside the disposable smoke guest.
//! This profile does not enable formal fixed-file self protection.
#![forbid(unsafe_code)]
use as_driver_adapter::LinuxDriverAdapter;
use as_types::{
    RuleVersion,
    configuration::{ConfigVersion, DriverConfigVersion, ProtectionKind},
    context::TargetPlatform,
    dispatch_event::EventPayload,
    driver::{DriverAdapter, DriverCallbacks, DriverConnectInput, DriverEvent, DriverSession},
    driver_control::{DriverCommand, DriverStatus, FeatureState, RuntimeOverride, SourceState},
    driver_image::{DriverConfigImage, DriverInitParts, DriverPolicyImage},
    main_ports::{ChoiceReason, Decision, DispatchInput, RuleChoice, Termination},
    operation::{ExecutionOutcome, ReviewBinding},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    error::Error,
    io,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    sync::{Notify, mpsc, oneshot},
    task::JoinSet,
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
fn require(value: bool, message: &str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
fn report(value: Value) {
    println!("{value}");
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn condition(field: u16, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&field.to_le_bytes());
    out.extend_from_slice(&[1, 0]);
    u32le(&mut out, value.len() as u32);
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    out.extend_from_slice(value);
    out
}
fn row(number: u32, action: u16, path: Option<&str>, subject: Option<&str>, log: bool) -> Vec<u8> {
    let group = b"native-diagnostic";
    let id = format!("file-{number}");
    let mut conditions = Vec::new();
    if let Some(path) = path {
        conditions.push(condition(15, path.as_bytes()));
    }
    if let Some(subject) = subject {
        conditions.push(condition(8, subject.as_bytes()));
    }
    let bytes = 40 + group.len() + id.len() + conditions.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::new();
    u32le(&mut out, bytes as u32);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&action.to_le_bytes());
    for value in [1, 0, 2, number, u32::from(log), conditions.len() as u32] {
        u32le(&mut out, value);
    }
    out.extend_from_slice(&(group.len() as u16).to_le_bytes());
    out.extend_from_slice(&(id.len() as u16).to_le_bytes());
    u32le(&mut out, 0);
    out.extend_from_slice(group);
    out.extend_from_slice(id.as_bytes());
    for item in conditions {
        out.extend_from_slice(&item);
    }
    out
}
fn parts(suite: &Path, status: &DriverStatus, deny_allow: bool) -> DriverInitParts {
    // Exactly the current Native configuration/policy formats, not a saved product document.
    let mut config = Vec::new();
    for field in [2, 16, 1500, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        u32le(&mut config, field);
    }
    let subject = suite.join("subject").to_string_lossy().into_owned();
    let cases = [
        ("allow", if deny_allow { 1 } else { 0 }),
        ("deny", 1),
        ("held", 2),
        ("timeout", 2),
        ("unbound", 2),
        ("failure", 2),
    ];
    let mut rows = Vec::new();
    for (index, (name, action)) in cases.iter().enumerate() {
        let path = suite
            .join("files")
            .join(name)
            .to_string_lossy()
            .into_owned();
        rows.extend_from_slice(&row(
            index as u32 + 1,
            *action,
            Some(&path),
            if *name == "failure" {
                None
            } else {
                Some(&subject)
            },
            true,
        ));
    }
    // All unrelated file opens are explicit fast Allow, so this fixture controls only its subjects.
    rows.extend_from_slice(&row(7, 0, None, None, false));
    let mut policy = Vec::new();
    // Current as_protocol.h::as_policy: 32-byte header, then rules.
    // This raw protocol diagnostic has no backend operation/path routes.
    u32le(&mut policy, 32);
    u32le(&mut policy, 7);
    policy.extend_from_slice(&(32u64 + rows.len() as u64).to_le_bytes());
    for _ in 0..4 {
        u32le(&mut policy, 0);
    }
    policy.extend_from_slice(&rows);
    DriverInitParts {
        expected_persistent_generation: status.persistent_generation,
        expected_lifecycle_epoch: status.lifecycle_epoch,
        configuration: DriverConfigImage {
            config_version: ConfigVersion::from_digest(hash(&config)),
            driver_config_version: DriverConfigVersion(hash(&config)),
            platform: TargetPlatform::Linux,
            encoding_revision: 1,
            immutable_buffer: config.into(),
        },
        rules: DriverPolicyImage {
            rule_version: RuleVersion::from_digest(hash(&policy)),
            platform: TargetPlatform::Linux,
            encoding_revision: 1,
            immutable_buffer: policy.into(),
        },
    }
}
fn raw_images(parts: &DriverInitParts) -> Vec<u8> {
    let config = &parts.configuration.immutable_buffer;
    let rules = &parts.rules.immutable_buffer;
    let mut out = vec![0u8; 136];
    out[0..8].copy_from_slice(&parts.expected_persistent_generation.to_le_bytes());
    out[8..10].copy_from_slice(&1u16.to_le_bytes());
    out[12..16].copy_from_slice(&1u32.to_le_bytes());
    out[16..20].copy_from_slice(&1u32.to_le_bytes());
    out[20..24].copy_from_slice(&parts.expected_lifecycle_epoch.to_le_bytes());
    out[24..32].copy_from_slice(&(config.len() as u64).to_le_bytes());
    out[32..40].copy_from_slice(&(rules.len() as u64).to_le_bytes());
    out[40..72].copy_from_slice(&parts.configuration.driver_config_version.0);
    out[72..104].copy_from_slice(&parts.rules.rule_version.digest());
    let mut digest = Sha256::new();
    digest.update(config);
    digest.update(rules);
    out[104..136].copy_from_slice(&digest.finalize());
    out.extend_from_slice(config);
    out.extend_from_slice(rules);
    out
}
struct Bound {
    pid: u32,
    path: String,
    release: oneshot::Sender<()>,
}
#[derive(Default)]
struct Observed {
    retired: Mutex<HashSet<u64>>,
    notify: Notify,
    direct: Mutex<Vec<(u32, String, ExecutionOutcome)>>,
}
impl Observed {
    async fn retired(&self, generation: u64) -> Result<()> {
        loop {
            let notified = self.notify.notified();
            if self
                .retired
                .lock()
                .map_err(|_| io::Error::other("retirement observer poisoned"))?
                .contains(&generation)
            {
                return Ok(());
            }
            notified.await;
        }
    }
    fn has_retired(&self, generation: u64) -> Result<bool> {
        Ok(self
            .retired
            .lock()
            .map_err(|_| io::Error::other("retirement observer poisoned"))?
            .contains(&generation))
    }
}
async fn controlled(
    input: DispatchInput,
    bound: mpsc::Sender<Bound>,
    suite: Arc<Path>,
) -> Result<()> {
    let operation = input
        .operation
        .ok_or_else(|| io::Error::other("controlled event lacked its original"))?;
    operation.validate(&input.event)?;
    let path = match &input.event.payload {
        EventPayload::File(file) => file.path.as_deref(),
        _ => None,
    }
    .ok_or_else(|| io::Error::other("controlled event lacked native file path"))?;
    let path = String::from_utf8(path.to_vec())?;
    let name = Path::new(&path)
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| io::Error::other("file name missing"))?
        .to_owned();
    let pid = input
        .event
        .header()
        .actor_tgid
        .ok_or_else(|| io::Error::other("original actor PID missing"))?;
    require(
        input.event.process.executable.as_deref()
            == Some(suite.join("subject").as_os_str().as_encoded_bytes()),
        "controlled actor was not the independent subject",
    )?;
    let hit = input
        .transfer_hit
        .ok_or_else(|| io::Error::other("ToUser source hit missing"))?;
    if name == "unbound" {
        let receipt = operation.result().await?;
        require(
            receipt.outcome == ExecutionOutcome::Failed { native_error: -110 },
            "unbound expiry did not preserve its native timeout result",
        )?;
        report(json!({"case":"unbound_native_result","pid":pid,"outcome":receipt.outcome}));
        return Ok(());
    }
    require(
        name == "held" || name == "timeout",
        "unexpected controlled target",
    )?;
    let deadline =
        Instant::now() + Duration::from_millis(if name == "timeout" { 180 } else { 1200 });
    operation
        .bind_review(ReviewBinding {
            hit: hit.clone(),
            deadline,
            timeout: Decision::Deny {
                terminate: Termination::None,
            },
        })
        .await?;
    let receipt = if name == "held" {
        let (release, wait) = oneshot::channel();
        bound.send(Bound { pid, path, release }).await?;
        wait.await?;
        operation
            .apply(
                &input.event,
                &RuleChoice {
                    decision: Decision::Allow,
                    hit: Some(hit),
                    reason: ChoiceReason::User,
                },
            )
            .await?
    } else {
        operation.result().await?
    };
    require(
        receipt.outcome
            == if name == "held" {
                ExecutionOutcome::Allowed
            } else {
                ExecutionOutcome::Denied
            },
        "source receipt disagreed with bound decision/fallback",
    )?;
    report(
        json!({"case":"native_original_receipt","pid":pid,"target":name,"outcome":receipt.outcome}),
    );
    Ok(())
}
async fn consume(
    mut events: mpsc::Receiver<DriverEvent>,
    mut controls: mpsc::Receiver<DispatchInput>,
    bound: mpsc::Sender<Bound>,
    observed: Arc<Observed>,
    suite: Arc<Path>,
) -> Result<()> {
    let mut work = JoinSet::new();
    let mut observations = 0u64;
    let mut events_open = true;
    let mut controls_open = true;
    loop {
        tokio::select! {
            completed = work.join_next(), if !work.is_empty() => { if let Some(result) = completed { result??; } }
            input = controls.recv(), if controls_open => match input {
                Some(input) => {work.spawn(controlled(input, bound.clone(), suite.clone()));}
                None => controls_open = false,
            },
            event = events.recv(), if events_open => match event {
                Some(DriverEvent::PersistentRetired {generation}) => {
                    observed.retired.lock().map_err(|_| io::Error::other("retirement observer poisoned"))?.insert(generation);
                    observed.notify.notify_waiters(); report(json!({"event":"persistent_retired","generation":generation}));
                }
                Some(DriverEvent::DirectResult {event, outcome, ..}) => {
                    let path = match &event.payload { EventPayload::File(file) => file.path.as_deref(), _ => None }.unwrap_or_default();
                    let pid = event.header().actor_tgid.ok_or_else(|| io::Error::other("direct actor PID missing"))?;
                    observed.direct.lock().map_err(|_| io::Error::other("direct observer poisoned"))?.push((pid, String::from_utf8(path.to_vec())?, outcome));
                }
                Some(DriverEvent::Observation(_)) => observations += 1,
                Some(DriverEvent::FileClose(record)) => report(json!({"event":"native_file_close","record":record})),
                Some(DriverEvent::RequestReturn(fact)) => report(json!({"event":"native_request_return","fact":fact})),
                Some(DriverEvent::RequestHandoff(fact)) => report(json!({"event":"native_request_handoff","fact":fact})),
                Some(DriverEvent::FutexSelection(fact)) => report(json!({"event":"native_futex_selection","fact":fact})),
                Some(DriverEvent::DirectHits(_)) => {}
                Some(DriverEvent::FactsBarrier {processed,..}) => {let _=processed.send(());}
                Some(DriverEvent::Status(status)) => { report(json!({"event":"native_state","status":status})); }
                Some(DriverEvent::TransportUnavailable) => return Err(io::Error::other("unexpected Native transport failure").into()),
                None => events_open = false,
            },
            else => break,
        }
    }
    while let Some(result) = work.join_next().await {
        result??;
    }
    report(json!({"event":"native_observation_count","count":observations}));
    Ok(())
}
fn subject(suite: &Path, name: &str) -> Result<Child> {
    Ok(Command::new(suite.join("subject"))
        .arg("open")
        .arg(suite.join("files").join(name))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?)
}
async fn output(child: Child, name: &str, expected_errno: i64) -> Result<u32> {
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        report(
            json!({"case":name,"subject_exit":output.status.code(),"subject_stdout":output.stdout}),
        );
    }
    require(output.status.success(), "independent helper failed")?;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    report(json!({"case":name,"subject":value}));
    require(
        value["errno"].as_i64() == Some(expected_errno),
        "independent open errno differed from expected source result",
    )?;
    require(
        value["result"].as_i64() == Some(if expected_errno == 0 { 0 } else { -1 }),
        "independent open result mismatch",
    )?;
    value["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or_else(|| io::Error::other("helper PID missing").into())
}
async fn bound_next(
    bound: &mut mpsc::Receiver<Bound>,
    child: &mut Child,
    suite: &Path,
) -> Result<Bound> {
    let notice = bound
        .recv()
        .await
        .ok_or_else(|| io::Error::other("bound observer stopped"))?;
    require(
        Some(notice.pid) == child.id(),
        "bound event actor did not identify independent child",
    )?;
    require(
        notice.path == suite.join("files/held").to_string_lossy(),
        "wrong bound original target",
    )?;
    require(
        child.try_wait()?.is_none(),
        "independent original completed before bound user decision",
    )?;
    Ok(notice)
}
async fn replace(
    session: &dyn DriverSession,
    suite: &Path,
    deny_allow: bool,
) -> Result<as_types::driver::AppliedPersistentState> {
    let status = session.request(DriverCommand::GetStatus).await?;
    session
        .replace_persistent(&parts(suite, &status, deny_allow))
        .await
        .map_err(Into::into)
}
async fn run(suite: Arc<Path>) -> Result<()> {
    require(
        std::env::args().nth(1).as_deref() == Some("--guest-diagnostic"),
        "explicit --guest-diagnostic required",
    )?;
    require(
        std::fs::read_to_string("/proc/cmdline")?
            .split_ascii_whitespace()
            .any(|s| s == "as_native_smoke=1"),
        "this diagnostic is restricted to the disposable smoke guest",
    )?;
    report(
        json!({"scope":"diagnostic_ordinary_file_only","formal_self_protection":"not_tested_by_this_profile","kernel":std::fs::read_to_string("/proc/sys/kernel/osrelease")?.trim(),"architecture":std::env::consts::ARCH,"sources":serde_json::from_slice::<Value>(&std::fs::read(suite.join("source-sha256.json"))?)?}),
    );
    let sentinel = hash(&std::fs::read(suite.join("sentinel"))?);
    let tree_identity =
        as_process_tree::ProcessTree::prepare(as_types::process_tree::ProcessTreeSettings {
            memory_budget_bytes: std::num::NonZeroUsize::new(4096)
                .ok_or_else(|| io::Error::other("diagnostic tree budget"))?,
            retired_retention_ms: std::num::NonZeroU64::new(1)
                .ok_or_else(|| io::Error::other("diagnostic retention"))?,
            reconcile_interval_ms: std::num::NonZeroU64::new(1)
                .ok_or_else(|| io::Error::other("diagnostic reconcile"))?,
            query_page_size: std::num::NonZeroUsize::new(1)
                .ok_or_else(|| io::Error::other("diagnostic page size"))?,
        })?;
    let session = LinuxDriverAdapter
        .connect(&DriverConnectInput {
            platform: TargetPlatform::Linux,
            host_boot_id: tree_identity.boot_id(),
            driver_assets_directory: suite.join("driver"),
        })
        .await?;
    let info = session.info();
    report(json!({"case":"hello","capabilities":info.capabilities,"status":info.status}));
    require(
        info.status.self_protection != FeatureState::Enabled,
        "diagnostic refuses active formal self protection",
    )?;
    require(
        info.capabilities.operation_mask & 2 != 0,
        "native file operation control unavailable",
    )?;
    let initial = parts(&suite, &info.status, false);
    std::fs::write(suite.join("diagnostic-images.bin"), raw_images(&initial))?;
    let (tx, rx) = mpsc::channel(256);
    let (control_tx, control_rx) = mpsc::channel(256);
    let (bound_tx, mut bound_rx) = mpsc::channel(8);
    let observed = Arc::new(Observed::default());
    let mut consumer = tokio::spawn(consume(
        rx,
        control_rx,
        bound_tx,
        observed.clone(),
        suite.clone(),
    ));
    // One observer owns this normal phase. If it ends, propagate that real
    // result instead of waiting for a notification it can no longer publish.
    let normal = async {
        let tracking = session
            .start_process_tracking(DriverCallbacks {
                controlled: control_tx,
                events: tx,
            })
            .await?;
        report(json!({"case":"process_tracking_start","tracking":tracking}));
        let binding = session.inspect(&[std::process::id()]).await?;
        report(json!({"case":"process_inspect","bindings":binding}));
        let applied = session.initialize(&initial).await?;
        require(
            applied.status.source_state == SourceState::Running
                && applied.status.file.effective_state == FeatureState::Enabled,
            "initialize did not physically enable ordinary file protection",
        )?;
        report(json!({"case":"initialize","status":applied.status}));
        output(subject(&suite, "allow")?, "direct_allow", 0).await?;
        output(subject(&suite, "deny")?, "direct_deny", 1).await?;
        let mut held = subject(&suite, "held")?;
        let notice = bound_next(&mut bound_rx, &mut held, &suite).await?;
        notice
            .release
            .send(())
            .map_err(|_| io::Error::other("bound handler stopped"))?;
        output(held, "bound_then_allow", 0).await?;
        let start = Instant::now();
        output(subject(&suite, "timeout")?, "bound_timeout_deny", 1).await?;
        require(
            start.elapsed() >= Duration::from_millis(100),
            "bound fallback returned before its deadline",
        )?;
        output(subject(&suite, "unbound")?, "unbound_expiry_is_error", 110).await?;
        let old = session.request(DriverCommand::GetStatus).await?;
        let mut held = subject(&suite, "held")?;
        let notice = bound_next(&mut bound_rx, &mut held, &suite).await?;
        let updated = replace(session.as_ref(), &suite, true).await?;
        require(
            updated.persistent_generation > old.persistent_generation,
            "persistent generation did not advance",
        )?;
        require(
            !observed.has_retired(old.persistent_generation)?,
            "old generation retired while its original was still held",
        )?;
        notice
            .release
            .send(())
            .map_err(|_| io::Error::other("bound handler stopped"))?;
        output(held, "old_original_survives_replace", 0).await?;
        observed.retired(old.persistent_generation).await?;
        output(subject(&suite, "allow")?, "new_generation_has_new_rule", 1).await?;
        report(
            json!({"case":"persistent_replace_and_retire","old_generation":old.persistent_generation,"adopted_generation":updated.persistent_generation,"status_polling_for_completion":false}),
        );
        let disabled = session
            .request(DriverCommand::SetRuntimeFeature {
                feature: ProtectionKind::File,
                enabled: false,
            })
            .await?;
        require(
            disabled.file.effective_state == FeatureState::Disabled
                && disabled.file.runtime_override == RuntimeOverride::Disabled,
            "temporary disable not physically reported",
        )?;
        output(subject(&suite, "deny")?, "temporary_file_disable", 0).await?;
        let kept = replace(session.as_ref(), &suite, false).await?;
        require(
            kept.status.file.runtime_override == RuntimeOverride::Disabled
                && kept.status.file.effective_state == FeatureState::Disabled,
            "persistent replacement reset temporary override",
        )?;
        output(
            subject(&suite, "deny")?,
            "temporary_override_survives_replace",
            0,
        )
        .await?;
        let resumed = session
            .request(DriverCommand::ResumeRuntimeFeature {
                feature: ProtectionKind::File,
            })
            .await?;
        require(
            resumed.file.runtime_override == RuntimeOverride::Default
                && resumed.file.effective_state == FeatureState::Enabled,
            "Resume did not restore persistent default",
        )?;
        output(subject(&suite, "deny")?, "resume_file_default", 1).await?;
        require(
            hash(&std::fs::read(suite.join("sentinel"))?) == sentinel,
            "runtime operation modified sentinel",
        )?;
        report(
            json!({"case":"temporary_override","sentinel_unchanged":true,"persistent_configuration_writes":"none_requested"}),
        );
        let mut held = subject(&suite, "held")?;
        let notice = bound_next(&mut bound_rx, &mut held, &suite).await?;
        Ok::<_, Box<dyn Error + Send + Sync>>((held, notice))
    };
    let (held, notice) = tokio::select! {
        result = normal => result?,
        result = &mut consumer => {
            result??;
            return Err(io::Error::other("native consumer ended during active diagnostic").into());
        }
    };
    session.stop_admission()?;
    notice
        .release
        .send(())
        .map_err(|_| io::Error::other("bound handler stopped"))?;
    output(held, "admitted_original_completes_during_stop", 0).await?;
    session.shutdown().await?;
    consumer.await??;
    require(
        session.rejected_callbacks() == 0,
        "diagnostic rejected native callbacks",
    )?;
    require(
        !observed
            .direct
            .lock()
            .map_err(|_| io::Error::other("direct observer poisoned"))?
            .is_empty(),
        "no source-confirmed direct rule results observed",
    )?;
    report(
        json!({"case":"normal_drain","status_polling":false,"rejected_callbacks":session.rejected_callbacks()}),
    );
    let lost = Command::new(suite.join("fd_loss"))
        .arg(suite.as_ref())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .status()
        .await?;
    require(
        lost.success(),
        "native control lifecycle diagnostic failed; see recorded cases",
    )?;
    println!("AS_NATIVE_SDK_SMOKE_PASS diagnostic_ordinary_file_only");
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let suite: Arc<Path> = std::env::args()
        .nth(2)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/suite".into())
        .into();
    if let Err(error) = run(suite).await {
        report(
            json!({"result":"fail","error":error.to_string(),"scope":"diagnostic_ordinary_file_only"}),
        );
        std::process::exit(1);
    }
}
