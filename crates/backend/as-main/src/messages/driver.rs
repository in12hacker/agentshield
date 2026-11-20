//! Main's one native connection and installed-generation lease bookkeeping.
use as_policy_engine::PolicyLease;
use as_types::{
    driver::{AppliedPersistentState, DriverSession},
    driver_control::{DriverCommand, DriverStatus},
    driver_image::DriverInitParts,
    driver_tools::{DriverProcessTarget, KillProcessOutcome},
    main_ports::ModuleError,
    process_source::{ProcessFactsPort, ProcessTrackingStart},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
struct Generation {
    parts: DriverInitParts,
    lease: PolicyLease,
}
struct Bound {
    session: Arc<dyn DriverSession>,
    generations: Mutex<BTreeMap<u64, Generation>>,
}
/// Main-owned native call port. Clones share the same real session and exact generation leases.
#[derive(Clone, Default)]
pub struct DriverPort(Option<Arc<Bound>>);
impl DriverPort {
    /// Start actual controlled ingress using the same authenticated driver session.
    pub async fn start_tls_ingress(
        &self,
        input: as_types::driver::TlsIngressInput,
        ingress: mpsc::Sender<as_types::flow::AdmittedConnection>,
    ) -> Result<Arc<dyn as_types::driver::TlsIngressSession>, ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .start_tls_ingress(input, ingress)
            .await
    }
    /// Submit one exact-process SIGKILL through the same authenticated driver; no retry or exit claim.
    pub async fn kill_process(
        &self,
        target: DriverProcessTarget,
    ) -> Result<KillProcessOutcome, ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .kill_process(target)
            .await
    }
    /// The same authenticated session's read-only process fact interface.
    pub fn process_facts(&self) -> Result<Arc<dyn ProcessFactsPort>, ModuleError> {
        Ok(self
            .0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .clone())
    }
    pub(crate) fn bind_group_tcp(
        &self,
        events: mpsc::Sender<as_types::driver::NativeGroupTcp>,
    ) -> Result<(), ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .bind_group_tcp(events)
    }
    /// Start the one observation stream before the process-tree scan begins.
    pub async fn start_tracking(
        &self,
        callbacks: as_types::driver::DriverCallbacks,
    ) -> Result<ProcessTrackingStart, ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .start_process_tracking(callbacks)
            .await
    }
    /// Physical capabilities from this actual connection, without inferring support from settings.
    pub fn capabilities(
        &self,
    ) -> Result<as_types::driver_control::DriverCapabilities, ModuleError> {
        Ok(self
            .0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .info()
            .capabilities)
    }
    /// Explicit unavailable lower port, never an implied driver implementation.
    pub fn unavailable() -> Self {
        Self(None)
    }
    /// Bind the actual session returned by the backend adapter; no independent connection is created.
    pub fn bind(session: Arc<dyn DriverSession>) -> Self {
        Self(Some(Arc::new(Bound {
            session,
            generations: Mutex::new(BTreeMap::new()),
        })))
    }
    /// Forward a small command and return only the actual native status.
    pub async fn request(&self, command: DriverCommand) -> Result<DriverStatus, ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .request(command)
            .await
    }
    /// Retain both immutable inputs and their exact user policy before native events can name them.
    pub async fn initialize(
        &self,
        parts: DriverInitParts,
        lease: PolicyLease,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.adopt(parts, lease, true).await
    }
    /// Replace the complete state, preserving actual native runtime overrides.
    pub async fn replace(
        &self,
        parts: DriverInitParts,
        lease: PolicyLease,
    ) -> Result<AppliedPersistentState, ModuleError> {
        self.adopt(parts, lease, false).await
    }
    async fn adopt(
        &self,
        parts: DriverInitParts,
        lease: PolicyLease,
        initialize: bool,
    ) -> Result<AppliedPersistentState, ModuleError> {
        if lease.version() != parts.rules.rule_version {
            return Err(ModuleError::InvalidInput);
        }
        let bound = self.0.as_ref().ok_or(ModuleError::Unavailable)?;
        let generation = parts
            .expected_persistent_generation
            .checked_add(1)
            .ok_or(ModuleError::Conflict)?;
        {
            let mut generations = bound
                .generations
                .lock()
                .map_err(|_| ModuleError::Unavailable)?;
            if generations.contains_key(&generation) {
                return Err(ModuleError::Conflict);
            }
            generations.insert(
                generation,
                Generation {
                    parts: parts.clone(),
                    lease,
                },
            );
        }
        let result = if initialize {
            bound.session.initialize(&parts).await
        } else {
            bound.session.replace_persistent(&parts).await
        };
        match result {
            Ok(applied)
                if applied.persistent_generation == generation
                    && applied.status.driver_config_version
                        == Some(parts.configuration.driver_config_version)
                    && applied.status.rule_version == Some(parts.rules.rule_version) =>
            {
                Ok(applied)
            }
            Ok(_) => {
                let _ = bound.session.request(DriverCommand::DeclareFailure).await;
                Err(ModuleError::InvalidInput)
            }
            Err(error) => {
                if matches!(
                    error,
                    ModuleError::Unsupported
                        | ModuleError::InvalidInput
                        | ModuleError::Conflict
                        | ModuleError::Busy
                ) {
                    bound
                        .generations
                        .lock()
                        .map_err(|_| ModuleError::Unavailable)?
                        .remove(&generation);
                }
                // Unknown confirmation keeps the candidate alive until real status/retirement/shutdown.
                Err(error)
            }
        }
    }
    /// Pin the selected policy until this already-admitted operation really finishes.
    pub fn lease(&self, version: as_types::RuleVersion) -> Result<PolicyLease, ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .generations
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .values()
            .find(|g| g.parts.rules.rule_version == version)
            .map(|g| g.lease.clone())
            .ok_or(ModuleError::Unavailable)
    }
    /// Release only a generation the native snapshot owner actually reported retired.
    pub fn retired(&self, generation: u64) -> Result<(), ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .generations
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .remove(&generation);
        Ok(())
    }
    /// Stop new original admission while preserving native replies and receipts.
    pub fn stop_admission(&self) -> Result<(), ModuleError> {
        self.0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .stop_admission()
    }
    /// Actual backend callback loss, separate from the driver's own dropped counter.
    pub fn rejected_callbacks(&self) -> u64 {
        self.0
            .as_ref()
            .map_or(0, |b| b.session.rejected_callbacks())
    }
    /// Observe callback loss without consuming capacity in the actual event queue.
    pub fn callback_loss(&self) -> Result<tokio::sync::watch::Receiver<u64>, ModuleError> {
        Ok(self
            .0
            .as_ref()
            .ok_or(ModuleError::Unavailable)?
            .session
            .callback_loss())
    }
    /// Retire native I/O while its owning fast runtime is still running.
    pub async fn shutdown(&self) -> Result<(), ModuleError> {
        let bound = self.0.as_ref().ok_or(ModuleError::Unavailable)?;
        bound.session.shutdown().await
    }
    pub(crate) fn release_leases(&self) -> Result<(), ModuleError> {
        // Called only after source callbacks and already-started management tasks really joined.
        if let Some(bound) = &self.0 {
            bound
                .generations
                .lock()
                .map_err(|_| ModuleError::Unavailable)?
                .clear();
        }
        Ok(())
    }
}
