//! One host process-tree owner: native-bound proc reads, immutable views and bounded queries.
#![forbid(unsafe_code)]
mod model;
mod query;
mod scan;
mod store;
use as_types::{
    dispatch_event::{EventData, EventPayload, ProcessInfo},
    main_ports::{FastFacts, ModuleError, RuleHit},
    process_source::*,
    process_tree::*,
};
pub use model::*;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use store::{Candidate, State};
/// Explicit process-tree failure; no empty successful substitute.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Native source failed.
    #[error(transparent)]
    Source(#[from] ModuleError),
    /// Actual OS read failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Identity, interval or typed query was invalid.
    #[error("invalid process-tree input")]
    Invalid,
    /// An exact requested record does not exist.
    #[error("process instance not found")]
    NotFound,
    /// The cursor belongs to another query or tree generation.
    #[error("process cursor expired")]
    CursorExpired,
    /// Module is closed.
    #[error("process tree stopped")]
    Stopped,
    /// Real memory or runtime capacity is exhausted.
    #[error("process tree resource limit")]
    ResourceLimit,
    /// A concurrent scan or identity mutation prevents a valid sample.
    #[error("process facts changed during sampling")]
    Conflict,
    /// The selected view cannot cover the host system.
    #[error("host proc coverage unavailable")]
    Coverage,
    /// Module state lock failed.
    #[error("process tree unavailable")]
    Unavailable,
}
/// Sole process-tree owner. Interfaces are shared; no second writable view is exposed.
pub struct ProcessTree {
    settings: ProcessTreeSettings,
    boot: [u8; 16],
    instance: [u8; 16],
    state: RwLock<State>,
    budget: Arc<model::Budget>,
    owner: std::sync::Weak<ProcessTree>,
    source: Mutex<Option<Arc<dyn ProcessFactsPort>>>,
    admission: Mutex<()>,
    scan: Arc<tokio::sync::Mutex<()>>,
    work: tokio_util::task::TaskTracker,
}
impl ProcessTree {
    /// Return the boot component used by this tree's exact process identities.
    pub fn boot_id(&self) -> [u8; 16] {
        self.boot
    }

    /// Construct with fixed OS source and validated settings; no Ready claim before initialize.
    pub fn prepare(settings: ProcessTreeSettings) -> Result<Arc<Self>, Error> {
        if settings.memory_budget_bytes.get() < 4096 || settings.query_page_size.get() > 4096 {
            return Err(Error::Invalid);
        }
        let boot = scan::boot_id()?;
        let mut hash = Sha256::new();
        hash.update(boot);
        hash.update(std::process::id().to_le_bytes());
        hash.update(scan::now_ns().to_le_bytes());
        let digest = hash.finalize();
        let mut instance = [0; 16];
        instance.copy_from_slice(&digest[..16]);
        let budget = Arc::new(model::Budget {
            used: AtomicUsize::new(0),
            max: settings.memory_budget_bytes.get(),
        });
        Ok(Arc::new_cyclic(|owner| Self {
            owner: owner.clone(),
            settings,
            boot,
            instance,
            state: RwLock::new(State::new()),
            budget,
            source: Mutex::new(None),
            admission: Mutex::new(()),
            scan: Arc::new(tokio::sync::Mutex::new(())),
            work: tokio_util::task::TaskTracker::new(),
        }))
    }
    /// Adopt the actual observation start and obtain the initial full-host image plus consumed fence.
    pub async fn initialize(
        self: &Arc<Self>,
        start: ProcessTrackingStart,
        source: Arc<dyn ProcessFactsPort>,
    ) -> Result<ScanReceipt, Error> {
        {
            let _admission = self.admission.lock().map_err(|_| Error::Unavailable)?;
            let mut bound = self.source.lock().map_err(|_| Error::Unavailable)?;
            if self.work.is_closed() {
                return Err(Error::Stopped);
            }
            if bound.is_some() {
                return Err(Error::Conflict);
            }
            let mut s = self.state.write().map_err(|_| Error::Unavailable)?;
            s.epoch = start.tracking_epoch;
            s.marker = start.begin_marker;
            s.loss = start.loss_epoch;
            s.rejected = start.backend_rejected;
            s.initial_expected = Some((
                start.tracking_epoch,
                start.loss_epoch,
                start.backend_rejected,
            ));
            *bound = Some(source);
        }
        self.refresh(RefreshTarget::System).await
    }
    pub(crate) fn key(&self, pid: u32, birth: u64) -> ProcessKey {
        ProcessKey {
            boot_id: self.boot,
            host_tgid: pid,
            birth_boottime_ns: birth,
        }
    }
    /// Invalidate cached supplements immediately when a real source/queue gap is observed.
    pub fn source_gap(&self) {
        if let Ok(mut s) = self.state.write() {
            s.uncertain();
        }
    }
    /// Consume a real native fence after prior observation work was accepted by FastFacts.
    pub fn fence(&self, marker: &ProcessFactsBarrier) {
        if let Ok(mut s) = self.state.write() {
            s.hit_gap |= marker.native_hit_loss != 0 || marker.backend_hit_rejected != 0;
            if s.epoch != 0
                && (s.epoch != marker.tracking_epoch
                    || s.loss != marker.loss_epoch
                    || s.rejected != marker.backend_rejected)
            {
                s.uncertain();
            }
            if s.epoch != 0 && s.epoch != marker.tracking_epoch {
                s.generation = s.generation.saturating_add(1);
            }
            s.epoch = marker.tracking_epoch;
            s.marker = marker.marker;
            s.loss = marker.loss_epoch;
            s.rejected = marker.backend_rejected;
            s.closed_before = s.closed_before.max(marker.cutoff_boottime_ns);
            let keys = s
                .event_times
                .iter()
                .filter(|(k, t)| **t < s.closed_before && !s.events.contains_key(k))
                .map(|(k, _)| *k)
                .collect::<Vec<_>>();
            for k in keys {
                s.event_times.remove(&k);
                s.event_charges.remove(&k);
            }
        }
    }
    /// Release an event's active hit set; closed identity stays until a real native fence covers it.
    pub fn finish_event(&self, key: EventKey) {
        if let Ok(mut s) = self.state.write() {
            s.events.remove(&key);
            if let Some(process) = s.event_process.remove(&key)
                && let Some(count) = s.pins.get_mut(&process)
            {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    s.pins.remove(&process);
                }
            }
            if s.event_times
                .get(&key)
                .is_some_and(|t| *t < s.closed_before)
            {
                s.event_times.remove(&key);
                s.event_charges.remove(&key);
            }
        }
    }
    /// Get real in-memory status without OS reads.
    pub fn status(&self) -> Result<TreeStatus, Error> {
        let s = self.state.read().map_err(|_| Error::Unavailable)?;
        Ok(self.status_locked(&s))
    }
    pub(crate) fn status_locked(&self, s: &State) -> TreeStatus {
        TreeStatus {
            evidence_epoch: s.gap_serial,
            phase: s.phase,
            instance: self.instance,
            generation: s.generation,
            revision: s.revision,
            nodes: s.nodes.len(),
            retired: s.retire_queue.len(),
            memory_used: self.budget.used.load(Ordering::Acquire),
            tracking_epoch: s.epoch,
            marker: s.marker,
            loss_epoch: s.loss,
            backend_rejected: s.rejected,
            historical_gap: s.gap,
        }
    }
    /// Start one owned, serialized refresh; caller cancellation does not abandon a blocking read.
    pub async fn refresh(self: &Arc<Self>, target: RefreshTarget) -> Result<ScanReceipt, Error> {
        if matches!(&target,RefreshTarget::Process{key} if key.boot_id!=self.boot || key.host_tgid==0)
        {
            return Err(Error::Invalid);
        }
        let guard = self
            .scan
            .clone()
            .try_lock_owned()
            .map_err(|_| Error::Conflict)?;
        let detail_attempt = {
            let mut state = self.state.write().map_err(|_| Error::Unavailable)?;
            match &target {
                RefreshTarget::System => None,
                RefreshTarget::Process { key } => {
                    if let Some(node) = state.nodes.get_mut(key) {
                        node.detail_attempt = Some(store::DetailAttempt::Pending);
                        Some(*key)
                    } else {
                        None
                    }
                }
            }
        };
        self.refresh_locked(target, guard, detail_attempt).await
    }
    async fn refresh_locked(
        self: &Arc<Self>,
        target: RefreshTarget,
        guard: tokio::sync::OwnedMutexGuard<()>,
        detail_attempt: Option<ProcessKey>,
    ) -> Result<ScanReceipt, Error> {
        let retention = ScanRetention(self.clone(), detail_attempt);
        let admission = self.admission.lock().map_err(|_| Error::Unavailable)?;
        if self.work.is_closed() {
            return Err(Error::Stopped);
        }
        if self
            .source
            .lock()
            .map_err(|_| Error::Unavailable)?
            .is_none()
        {
            return Err(Error::Unavailable);
        }
        let owner = self.clone();
        let (done, waiting) = tokio::sync::oneshot::channel();
        self.work.spawn(async move {
            let _guard = guard;
            let _retention = retention;
            {
                match owner.state.write() {
                    Ok(mut state) => state.scan_active = true,
                    Err(_) => {
                        let _ = done.send(Err(Error::Unavailable));
                        return;
                    }
                }
            }
            let result = owner.scan(target).await;
            if result.is_err() {
                owner.source_gap();
            }
            let _ = done.send(result);
        });
        drop(admission);
        waiting.await.map_err(|_| Error::Unavailable)?
    }
    /// Reconciliation loop, run by main on the existing message runtime.
    pub async fn run_reconciliation(self: Arc<Self>, mut stop: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_millis(
            self.settings.reconcile_interval_ms.get(),
        ));
        interval.tick().await;
        loop {
            tokio::select! {biased;_ = stop.changed()=>break,_=interval.tick()=>{match self.refresh(RefreshTarget::System).await{Ok(_)|Err(Error::Conflict)|Err(Error::Stopped)=>{},Err(_)=>self.source_gap()}}}
        }
    }
    /// Stop new refresh work and join all already accepted tasks; snapshots retire through Arc.
    pub fn close(&self) -> impl std::future::Future<Output = Result<(), Error>> + Send + 'static {
        let owner = self.owner.upgrade();
        let work = tokio::spawn(async move {
            let owner = owner.ok_or(Error::Stopped)?;
            owner.stop_refresh().await;
            let mut s = owner.state.write().map_err(|_| Error::Unavailable)?;
            s.phase = TreePhase::Closed;
            s.events.clear();
            s.event_process.clear();
            s.pins.clear();
            s.event_charges.clear();
            s.event_times.clear();
            s.retire_queue.clear();
            s.nodes.clear();
            s.ordered.clear();
            s.children.clear();
            s.pids.clear();
            Ok(())
        });
        async move { work.await.map_err(|_| Error::Unavailable)? }
    }
    /// Retire refresh producers while the source and fast fence consumer are still available.
    pub async fn stop_refresh(&self) {
        {
            let _admission = match self.admission.lock() {
                Ok(guard) => guard,
                Err(poison) => poison.into_inner(),
            };
            self.work.close();
        }
        self.work.wait().await;
    }
    fn observe(&self, event: &EventData) -> Result<(), Error> {
        let h = event.header();
        let (Some(pid), Some(birth), Some(time)) = (
            h.actor_tgid,
            h.actor_birth_boottime_ns,
            h.source_boottime_ns,
        ) else {
            return Ok(());
        };
        let key = self.key(pid, birth);
        let window = SampleWindow {
            begin_boottime_ns: time,
            end_boottime_ns: time,
        };
        let mut s = self.state.write().map_err(|_| Error::Unavailable)?;
        if s.phase == TreePhase::Closed {
            return Err(Error::Stopped);
        }
        s.collect(
            Duration::from_millis(self.settings.retired_retention_ms.get()),
            false,
        );
        if !s.events.contains_key(&EventKey::from(event))
            && s.event_times.contains_key(&EventKey::from(event))
        {
            return Ok(());
        }
        if !s.events.contains_key(&EventKey::from(event)) && (time < s.closed_before) {
            return Err(Error::Conflict);
        }
        if let std::collections::hash_map::Entry::Vacant(entry) =
            s.events.entry(EventKey::from(event))
        {
            let charge = self.budget.take(512)?;
            entry.insert(HashSet::new());
            s.event_process.insert(EventKey::from(event), key);
            *s.pins.entry(key).or_default() += 1;
            s.event_times.insert(EventKey::from(event), time);
            s.event_charges.insert(EventKey::from(event), charge);
        }
        let executed = h.kind == as_types::main_ports::EventKind::Process
            && h.stage == as_types::main_ports::EventStage::Observation
            && h.operation == 1;
        let mut facts = event.process.clone();
        if executed && let EventPayload::Process(payload) = &event.payload {
            // Only a successful exec fact names the actor's new image. A pre-exec
            // request still describes the old actor and must never update it.
            if facts.executable.is_none() {
                facts.executable = payload.target_executable.clone();
            }
        }
        let c = Candidate {
            key,
            facts,
            cgroup: h.actor_cgroup_id,
            leader: None,
            window,
            origin: Origin::DriverEvent {
                event: EventKey::from(event),
                actor_tid: h.actor_tid,
                actor_thread_birth_boottime_ns: h.actor_thread_birth_boottime_ns,
            },
            executed,
            states: Default::default(),
        };
        if pid != 0
            && let Err(error) = s.put(c, &self.budget)
        {
            s.uncertain();
            return Err(error);
        }
        if h.stage == as_types::main_ports::EventStage::Observation
            && let EventPayload::Process(p) = &event.payload
        {
            if h.operation == 2
                && let (Some(child), Some(birth)) = (p.child_pid, p.child_birth_boottime_ns)
            {
                let child = self.key(child, birth);
                let mut facts = p.child_process.as_deref().cloned().unwrap_or_default();
                facts.parent_pid = p.child_parent_pid;
                facts.parent_birth_boottime_ns = p.child_parent_birth_boottime_ns;
                s.put(
                    Candidate {
                        key: child,
                        facts,
                        cgroup: p.child_cgroup_id,
                        leader: None,
                        window,
                        origin: Origin::DriverEvent {
                            event: EventKey::from(event),
                            actor_tid: Some(child.host_tgid),
                            actor_thread_birth_boottime_ns: Some(child.birth_boottime_ns),
                        },
                        executed: false,
                        states: Default::default(),
                    },
                    &self.budget,
                )?;
                if let Some(n) = s.nodes.get_mut(&child) {
                    n.record.creator = (pid != 0).then_some(key);
                }
            } else if h.operation == 3 {
                s.retire(key, Some(window), p.exit_code);
            }
        }
        Ok(())
    }
    fn supplement(&self, event: &EventData) -> Result<ProcessInfo, Error> {
        let h = event.header();
        let (Some(pid), Some(birth), Some(time)) = (
            h.actor_tgid,
            h.actor_birth_boottime_ns,
            h.source_boottime_ns,
        ) else {
            return Ok(ProcessInfo::default());
        };
        let key = self.key(pid, birth);
        let (snap, gap_serial) = {
            let state = self.state.read().map_err(|_| Error::Unavailable)?;
            if state.phase != TreePhase::Ready {
                return Ok(ProcessInfo::default());
            }
            let Some(node) = state.nodes.get(&key) else {
                return Ok(ProcessInfo::default());
            };
            (node.record.latest.clone(), state.gap_serial)
        };
        let mut out = ProcessInfo::default();
        let valid_stamp = |field: Field| {
            snap.stamps.get(&field).filter(|st| {
                st.state == FieldState::Known
                    && st.evidence_epoch == gap_serial
                    && st.window.end_boottime_ns <= time
                    && (!field.thread_local()
                        || st.origin.same_thread(
                            key,
                            h.actor_tid,
                            h.actor_thread_birth_boottime_ns,
                        ))
            })
        };
        macro_rules! valid {
            ($field:ident,$tag:ident) => {
                if event.process.$field.is_none() && valid_stamp(Field::$tag).is_some() {
                    out.$field = snap.process.$field.clone();
                }
            };
            (copy $field:ident,$tag:ident) => {
                if event.process.$field.is_none() && valid_stamp(Field::$tag).is_some() {
                    out.$field = snap.process.$field;
                }
            };
        }
        valid!(name, Name);
        valid!(executable, Executable);
        valid!(argv, Argv);
        valid!(cwd, Cwd);
        valid!(copy uid, Uid);
        valid!(copy euid, Euid);
        valid!(copy gid, Gid);
        valid!(copy egid, Egid);
        // Membership for this operation comes from its original driver sample.
        valid!(copy kind, Kind);
        valid!(copy exec_cookie, ExecCookie);
        valid!(copy pid_namespace_inode, PidNamespace);
        valid!(copy net_namespace_inode, NetNamespace);
        valid!(copy user_namespace_inode, UserNamespace);
        valid!(copy mount_namespace_inode, MountNamespace);
        valid!(namespace_tgids, NamespaceTgids);
        valid!(copy suid, Suid);
        valid!(copy fsuid, Fsuid);
        valid!(copy sgid, Sgid);
        valid!(copy fsgid, Fsgid);
        let parent_pid = valid_stamp(Field::ParentPid).and(snap.process.parent_pid);
        let parent_birth =
            valid_stamp(Field::ParentBirth).and(snap.process.parent_birth_boottime_ns);
        if event
            .process
            .parent_pid
            .is_none_or(|pid| Some(pid) == parent_pid)
            && event
                .process
                .parent_birth_boottime_ns
                .is_none_or(|birth| Some(birth) == parent_birth)
        {
            valid!(copy parent_pid, ParentPid);
            valid!(copy parent_birth_boottime_ns, ParentBirth);
            valid!(parent_name, ParentName);
            valid!(parent_executable, ParentExecutable);
            valid!(parent_argv, ParentArgv);
        }
        // An empty proc cmdline read is a detail value, not evidence of argc=0 for an original operation.
        for (value, field) in [
            (&mut out.argv, Field::Argv),
            (&mut out.parent_argv, Field::ParentArgv),
        ] {
            if value.as_ref().is_some_and(Vec::is_empty)
                && valid_stamp(field).is_some_and(|stamp| {
                    matches!(stamp.origin, Origin::StartupScan | Origin::Refresh)
                })
            {
                *value = None;
            }
        }
        Ok(out)
    }
}
struct ScanRetention(Arc<ProcessTree>, Option<ProcessKey>);
impl Drop for ScanRetention {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.write() {
            state.scan_active = false;
            if let Some(key) = self.1
                && let Some(node) = state.nodes.get_mut(&key)
                && node.detail_attempt == Some(store::DetailAttempt::Pending)
            {
                // The scan lock is still held. An exec's None cannot be replaced
                // by another query's Pending until this retention has finished.
                node.detail_attempt = Some(store::DetailAttempt::Completed);
            }
        }
    }
}
impl FastFacts for ProcessTree {
    fn source_gap(&self) {
        ProcessTree::source_gap(self)
    }
    fn finish_event(&self, event: EventKey) {
        ProcessTree::finish_event(self, event)
    }
    fn prepare(&self, event: &EventData) -> Result<ProcessInfo, ModuleError> {
        if self.observe(event).is_err() {
            self.source_gap();
            return Ok(ProcessInfo::default());
        }
        match self.supplement(event) {
            Ok(facts) => Ok(facts),
            Err(_) => {
                self.source_gap();
                Ok(ProcessInfo::default())
            }
        }
    }
    fn record_hit(&self, event: &EventData, hit: &RuleHit) {
        if self.hit(event, hit).is_err() {
            self.source_gap();
        }
    }
    fn record_direct_hits(&self, hits: Vec<as_types::driver::DirectHitCount>) {
        self.direct_hits(hits);
    }
}
#[cfg(test)]
mod tests;
