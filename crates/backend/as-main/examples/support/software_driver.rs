//! Explicit software-only driver provider shared by unit and installed-layout wiring smoke.
use as_types::{
    configuration::ProtectionDefaults, driver::*, driver_control::*, driver_image::DriverInitParts,
    main_ports::ModuleError, process_source::*,
};
use std::{
    os::unix::fs::MetadataExt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
pub struct SoftwareDriver;
pub struct Session {
    info: Mutex<DriverInfo>,
    events: Mutex<Option<DriverCallbacks>>,
    marker: AtomicU64,
    loss: tokio::sync::watch::Sender<u64>,
}
impl Session {
    fn new(platform: as_types::context::TargetPlatform) -> Self {
        let (loss, _) = tokio::sync::watch::channel(0);
        let unavailable = FeatureStatus {
            startup_default: true,
            runtime_override: RuntimeOverride::Default,
            effective_state: FeatureState::Unavailable,
        };
        Self {
            events: Mutex::new(None),
            marker: AtomicU64::new(1),
            loss,
            info: Mutex::new(DriverInfo {
                session_id: [9; 16],
                platform,
                capabilities: DriverCapabilities {
                    max_frame_bytes: 16384,
                    max_pending: 64,
                    max_transfer_bytes: 4 * 1024 * 1024,
                    max_hold_ms: 300000,
                    operation_mask: DriverCapabilities::OPERATION_MASK,
                    condition_mask: (1 << 25) - 1,
                    features: DriverFeature::SelfProtection as u32
                        | DriverFeature::ProcessFacts as u32
                        | DriverFeature::TerminateActor as u32
                        | DriverFeature::TerminateParent as u32,
                },
                status: DriverStatus {
                    instance: [7; 16],
                    revision: DRIVER_REVISION,
                    source_state: SourceState::Dormant,
                    lifecycle_epoch: 0,
                    persistent_generation: 0,
                    state_revision: 0,
                    driver_config_version: None,
                    rule_version: None,
                    startup_protection: None,
                    process: unavailable.clone(),
                    file: unavailable.clone(),
                    network: unavailable,
                    self_protection: FeatureState::Unavailable,
                    pending: 0,
                    pending_receipts: 0,
                    dropped: 0,
                    error: None,
                },
            }),
        }
    }
    fn apply(&self, parts: &DriverInitParts) -> Result<AppliedPersistentState, ModuleError> {
        let mut info = self.info.lock().map_err(|_| ModuleError::Unavailable)?;
        if info.status.persistent_generation != parts.expected_persistent_generation {
            return Err(ModuleError::Conflict);
        }
        info.status.persistent_generation += 1;
        info.status.source_state = SourceState::Running;
        info.status.state_revision += 1;
        info.status.driver_config_version = Some(parts.configuration.driver_config_version);
        info.status.rule_version = Some(parts.rules.rule_version);
        info.status.self_protection = FeatureState::Enabled;
        info.status.startup_protection = Some(ProtectionDefaults {
            process: true,
            file: true,
            network: true,
        });
        Ok(AppliedPersistentState {
            persistent_generation: info.status.persistent_generation,
            status: info.status.clone(),
        })
    }
}
#[async_trait::async_trait]
impl DriverAdapter for SoftwareDriver {
    async fn connect(
        &self,
        input: &DriverConnectInput,
    ) -> Result<Arc<dyn DriverSession>, ModuleError> {
        Ok(Arc::new(Session::new(input.platform)))
    }
}
#[async_trait::async_trait]
impl DriverControl for Session {
    async fn request(&self, command: DriverCommand) -> Result<DriverStatus, ModuleError> {
        let mut info = self.info.lock().map_err(|_| ModuleError::Unavailable)?;
        match command {
            DriverCommand::GetStatus => {}
            DriverCommand::Quiesce => info.status.source_state = SourceState::Quiescing,
            DriverCommand::DeclareFailure => info.status.source_state = SourceState::Failed,
            _ => return Err(ModuleError::Unsupported),
        }
        Ok(info.status.clone())
    }
}
fn software_boottime() -> std::io::Result<u64> {
    let uptime = std::fs::read_to_string("/proc/uptime")?;
    let uptime = uptime
        .split_whitespace()
        .next()
        .ok_or_else(|| std::io::Error::other("uptime"))?;
    let (seconds, fraction) = uptime
        .split_once('.')
        .ok_or_else(|| std::io::Error::other("uptime units"))?;
    let seconds: u64 = seconds
        .parse()
        .map_err(|_| std::io::Error::other("uptime seconds"))?;
    let padded = format!("{fraction:0<9}");
    let nanos: u64 = padded
        .parse()
        .map_err(|_| std::io::Error::other("uptime fraction"))?;
    let begin = seconds.saturating_mul(1_000_000_000).saturating_add(nanos);
    Ok(begin)
}
fn synthetic_process(pid: u32) -> ProcessInspection {
    // This fixture uses raw proc clock ticks as a synthetic identity token.
    // It does not claim kernel nanosecond birth or exercise native protection.
    fn stat(pid: u32) -> std::io::Result<(u32, u64)> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let suffix = text
            .rsplit_once(") ")
            .ok_or_else(|| std::io::Error::other("stat shape"))?
            .1;
        let fields = suffix.split_whitespace().collect::<Vec<_>>();
        let parent = fields
            .get(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| std::io::Error::other("stat parent"))?;
        let birth = fields
            .get(19)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| std::io::Error::other("stat start"))?;
        Ok((parent, birth))
    }
    fn sample(pid: u32) -> std::io::Result<ProcessBinding> {
        let (parent, birth) = stat(pid)?;
        let parent_birth = if parent == 0 { 0 } else { stat(parent)?.1 };
        let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
        let kernel = status.lines().any(|line| {
            line.strip_prefix("Kthread:")
                .is_some_and(|v| v.trim() == "1")
        });
        let begin = software_boottime()?;
        Ok(ProcessBinding {
            host_tgid: pid,
            birth_boottime_ns: birth,
            parent_pid: parent,
            parent_birth_boottime_ns: parent_birth,
            kind: if kernel {
                ProcessKind::Kernel
            } else {
                ProcessKind::User
            },
            exec_cookie: 0,
            cgroup_id: None,
            window: SampleWindow {
                begin_boottime_ns: begin,
                end_boottime_ns: begin.saturating_add(10_000_000),
            },
        })
    }
    match sample(pid) {
        Ok(binding) => ProcessInspection::Bound { binding },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ProcessInspection::Gone { host_tgid: pid }
        }
        Err(_) => ProcessInspection::Inaccessible { host_tgid: pid },
    }
}
impl Session {
    async fn marker(&self) -> Result<ProcessFactsBarrier, ModuleError> {
        let events = self
            .events
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .as_ref()
            .map(|callbacks| callbacks.events.clone())
            .ok_or(ModuleError::Stopped)?;
        let namespace = std::fs::metadata("/proc/self/ns/pid")
            .map_err(|_| ModuleError::Unavailable)?
            .ino();
        let marker = ProcessFactsBarrier {
            host_pid_namespace_inode: namespace,
            tracking_epoch: 1,
            marker: self.marker.fetch_add(1, Ordering::Relaxed),
            loss_epoch: 0,
            backend_rejected: 0,
            cutoff_boottime_ns: software_boottime().map_err(|_| ModuleError::Unavailable)?,
            native_hit_loss: 0,
            backend_hit_rejected: 0,
        };
        let (processed, waiting) = tokio::sync::oneshot::channel();
        events
            .send(DriverEvent::FactsBarrier { marker, processed })
            .await
            .map_err(|_| ModuleError::Stopped)?;
        waiting.await.map_err(|_| ModuleError::Stopped)?;
        Ok(marker)
    }
}
#[async_trait::async_trait]
impl ProcessFactsPort for Session {
    async fn inspect(&self, pids: &[u32]) -> Result<Vec<ProcessInspection>, ModuleError> {
        let pids = pids.to_vec();
        tokio::task::spawn_blocking(move || pids.into_iter().map(synthetic_process).collect())
            .await
            .map_err(|_| ModuleError::Unavailable)
    }
    async fn facts_barrier(&self) -> Result<ProcessFactsBarrier, ModuleError> {
        self.marker().await
    }
}
#[async_trait::async_trait]
impl as_types::driver_tools::DriverTools for Session {
    async fn kill_process(
        &self,
        _: as_types::driver_tools::DriverProcessTarget,
    ) -> Result<as_types::driver_tools::KillProcessOutcome, ModuleError> {
        Err(ModuleError::Unsupported)
    }
}
#[async_trait::async_trait]
impl DriverSession for Session {
    fn info(&self) -> DriverInfo {
        self.info.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn rejected_callbacks(&self) -> u64 {
        *self.loss.borrow()
    }
    fn callback_loss(&self) -> tokio::sync::watch::Receiver<u64> {
        self.loss.subscribe()
    }
    async fn start_process_tracking(
        &self,
        callbacks: DriverCallbacks,
    ) -> Result<ProcessTrackingStart, ModuleError> {
        *self.events.lock().map_err(|_| ModuleError::Unavailable)? = Some(callbacks);
        let marker = self.marker().await?;
        Ok(ProcessTrackingStart {
            host_pid_namespace_inode: marker.host_pid_namespace_inode,
            tracking_epoch: marker.tracking_epoch,
            begin_marker: marker.marker,
            loss_epoch: marker.loss_epoch,
            backend_rejected: 0,
            cutoff_boottime_ns: marker.cutoff_boottime_ns,
        })
    }
    async fn initialize(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.apply(parts)
    }
    async fn replace_persistent(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.apply(parts)
    }
    fn stop_admission(&self) -> Result<(), ModuleError> {
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), ModuleError> {
        self.events
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .take();
        Ok(())
    }
}
