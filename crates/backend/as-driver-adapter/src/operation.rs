//! One execution state per actual native original; cancellation never invents a result.
use crate::{Shared, wire};
use as_types::{
    DecisionId, RuleVersion,
    dispatch_event::EventData,
    driver_control::DriverFeature,
    hold_budget::HoldWindow,
    main_ports::{Decision, ModuleError, RuleChoice, Termination},
    operation::{OperationReceipt, OriginalOperation, ReviewBinding, StopReason},
};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{Notify, watch};

pub(crate) fn native_decision_id(session: [u8; 16], wait_id: u64) -> DecisionId {
    let mut hash = Sha256::new();
    hash.update(session);
    hash.update(wait_id.to_le_bytes());
    let digest = hash.finalize();
    let mut key = [0; 16];
    key.copy_from_slice(&digest[..16]);
    DecisionId(key)
}

#[derive(Clone, Copy)]
struct ClockAnchor {
    native: u64,
    instant: tokio::time::Instant,
}
impl ClockAnchor {
    fn sample() -> Result<Self, ModuleError> {
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let secs = u64::try_from(now.tv_sec).map_err(|_| ModuleError::Unavailable)?;
        let nanos = u64::try_from(now.tv_nsec).map_err(|_| ModuleError::Unavailable)?;
        Ok(Self {
            native: secs
                .checked_mul(1_000_000_000)
                .and_then(|n| n.checked_add(nanos))
                .ok_or(ModuleError::Unavailable)?,
            instant: tokio::time::Instant::now(),
        })
    }
    fn instant(self, value: u64) -> Result<tokio::time::Instant, ModuleError> {
        if value >= self.native {
            self.instant
                .checked_add(Duration::from_nanos(value - self.native))
        } else {
            self.instant
                .checked_sub(Duration::from_nanos(self.native - value))
        }
        .ok_or(ModuleError::InvalidInput)
    }
    fn native(self, value: tokio::time::Instant) -> Result<u64, ModuleError> {
        if value >= self.instant {
            self.native.checked_add(
                u64::try_from((value - self.instant).as_nanos())
                    .map_err(|_| ModuleError::InvalidInput)?,
            )
        } else {
            self.native.checked_sub(
                u64::try_from((self.instant - value).as_nanos())
                    .map_err(|_| ModuleError::InvalidInput)?,
            )
        }
        .ok_or(ModuleError::InvalidInput)
    }
}
#[derive(Clone)]
enum Phase {
    Held,
    Bound,
    Applying,
    Finished(Result<OperationReceipt, ModuleError>),
}
/// Runtime capability for exactly one kernel-retained original, never reconstructed from PID.
pub(crate) struct Original {
    pub id: u64,
    pub generation: u64,
    pub rules: RuleVersion,
    key: DecisionId,
    source: Arc<EventData>,
    deadline: u64,
    window: HoldWindow,
    clock: ClockAnchor,
    owner: Weak<Shared>,
    phase: Mutex<Phase>,
    changed: Notify,
    stop: watch::Sender<Option<StopReason>>,
}
impl std::fmt::Debug for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeOriginal")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}
impl Original {
    pub(crate) fn new(
        owner: &Arc<Shared>,
        request: wire::Controlled,
        session: [u8; 16],
    ) -> Result<Arc<Self>, ModuleError> {
        let clock = ClockAnchor::sample()?;
        let max_hold = u64::from(owner.capabilities()?.max_hold_ms)
            .checked_mul(1_000_000)
            .ok_or(ModuleError::InvalidInput)?;
        if request.id == 0
            || request.deadline <= request.started
            || request.started > clock.native
            || request.deadline - request.started > max_hold
        {
            return Err(ModuleError::InvalidInput);
        }
        let window = HoldWindow::from_source(
            clock.instant(request.started)?,
            clock.instant(request.deadline)?,
        )
        .map_err(|_| ModuleError::InvalidInput)?;
        let (stop, _) = watch::channel(None);
        Ok(Arc::new(Self {
            id: request.id,
            generation: request.generation,
            rules: request.rules,
            key: native_decision_id(session, request.id),
            source: Arc::new(request.event),
            deadline: request.deadline,
            window,
            clock,
            owner: Arc::downgrade(owner),
            phase: Mutex::new(Phase::Held),
            changed: Notify::new(),
            stop,
        }))
    }
    pub(crate) fn event(&self) -> Arc<EventData> {
        self.source.clone()
    }
    pub(crate) fn complete(&self, result: Result<OperationReceipt, ModuleError>) {
        {
            let Ok(mut phase) = self.phase.lock() else {
                return;
            };
            if matches!(*phase, Phase::Finished(_)) {
                return;
            }
            *phase = Phase::Finished(result);
        }
        self.stop
            .send_replace(Some(if matches!(result, Err(ModuleError::Unavailable)) {
                StopReason::SourceFailure
            } else {
                StopReason::Orderly
            }));
        self.changed.notify_waiters();
    }
    pub(crate) fn receipt(&self, result: wire::NativeResult) -> Result<(), ModuleError> {
        if result.id != self.id
            || result.generation != self.generation
            || result.rules != self.rules
        {
            return Err(ModuleError::InvalidInput);
        }
        self.complete(Ok(OperationReceipt {
            decision_id: self.key,
            outcome: result.outcome,
            termination: result.termination,
        }));
        Ok(())
    }
    pub(crate) async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        loop {
            let changed = self.changed.notified();
            if let Phase::Finished(result) =
                &*self.phase.lock().map_err(|_| ModuleError::Unavailable)?
            {
                return *result;
            }
            changed.await;
        }
    }
    fn supports(&self, decision: Decision) -> Result<(), ModuleError> {
        let owner = self.owner.upgrade().ok_or(ModuleError::Unavailable)?;
        if let Decision::Deny { terminate } = decision {
            let caps = owner.capabilities()?;
            if matches!(terminate, Termination::Actor | Termination::ActorAndParent)
                && !caps.supports_feature(DriverFeature::TerminateActor)
            {
                return Err(ModuleError::Unsupported);
            }
            if terminate == Termination::ActorAndParent
                && !caps.supports_feature(DriverFeature::TerminateParent)
            {
                return Err(ModuleError::Unsupported);
            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl OriginalOperation for Original {
    fn decision_id(&self) -> DecisionId {
        self.key
    }
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>> {
        match &*self.phase.lock().ok()? {
            Phase::Finished(result) => Some(*result),
            _ => None,
        }
    }
    fn policy_version(&self) -> Option<RuleVersion> {
        Some(self.rules)
    }
    fn hold_window(&self) -> HoldWindow {
        self.window
    }
    fn stop_signal(&self) -> Option<watch::Receiver<Option<StopReason>>> {
        Some(self.stop.subscribe())
    }
    fn validate(&self, event: &EventData) -> Result<(), ModuleError> {
        if self.source.accepts_enrichment(event) {
            Ok(())
        } else {
            Err(ModuleError::InvalidInput)
        }
    }
    async fn bind_review(&self, binding: ReviewBinding) -> Result<(), ModuleError> {
        self.supports(binding.timeout)?;
        if binding.deadline > self.window.deadline()
            || binding.deadline <= tokio::time::Instant::now()
            || !matches!(binding.hit.source,as_types::main_ports::RuleSourceVersion::RulesDocument(v) if v==self.rules)
        {
            return Err(ModuleError::InvalidInput);
        }
        {
            let phase = self.phase.lock().map_err(|_| ModuleError::Unavailable)?;
            if !matches!(*phase, Phase::Held) {
                return Err(ModuleError::Stopped);
            }
        }
        let deadline = self.clock.native(binding.deadline)?.min(self.deadline);
        let (decision, terminate) = wire::decision(binding.timeout);
        let mut body = Vec::with_capacity(56);
        body.extend(self.id.to_le_bytes());
        body.extend(self.rules.digest());
        body.extend(deadline.to_le_bytes());
        body.extend(decision.to_le_bytes());
        body.extend(terminate.to_le_bytes());
        self.owner
            .upgrade()
            .ok_or(ModuleError::Unavailable)?
            .command(7, body)
            .await?;
        let mut phase = self.phase.lock().map_err(|_| ModuleError::Unavailable)?;
        if matches!(*phase, Phase::Held) {
            *phase = Phase::Bound;
            Ok(())
        } else {
            Err(ModuleError::Stopped)
        }
    }
    async fn apply(
        &self,
        event: &EventData,
        choice: &RuleChoice,
    ) -> Result<OperationReceipt, ModuleError> {
        self.validate(event)?;
        if let Some(result) = self.try_result() {
            return result;
        }
        if let Err(error) = self.supports(choice.decision) {
            return match self.try_result() {
                Some(result) => result,
                None => Err(error),
            };
        }
        let submit = {
            let mut phase = self.phase.lock().map_err(|_| ModuleError::Unavailable)?;
            match &*phase {
                Phase::Finished(result) => return *result,
                Phase::Applying => false,
                _ if tokio::time::Instant::now() >= self.window.deadline() => false,
                _ => {
                    *phase = Phase::Applying;
                    true
                }
            }
        };
        if submit {
            let owner = self.owner.upgrade().ok_or(ModuleError::Unavailable)?;
            let (decision, terminate) = wire::decision(choice.decision);
            let mut body = Vec::with_capacity(16);
            body.extend(self.id.to_le_bytes());
            body.extend(decision.to_le_bytes());
            body.extend(terminate.to_le_bytes());
            match owner.command(8, body).await {
                Ok(_) | Err(ModuleError::Stopped) => {}
                Err(error) => {
                    self.cancel();
                    return Err(error);
                }
            }
        }
        self.result().await
    }
    async fn result(&self) -> Result<OperationReceipt, ModuleError> {
        Original::result(self).await
    }
    fn cancel(&self) {
        if self
            .phase
            .lock()
            .is_ok_and(|phase| matches!(*phase, Phase::Finished(_)))
        {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            owner.cancel(self.id);
        }
    }
}
