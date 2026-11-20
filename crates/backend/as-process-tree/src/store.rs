use crate::{Error, model::*};
use as_types::{dispatch_event::ProcessInfo, process_source::SampleWindow, process_tree::*};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
};
pub(crate) struct Candidate {
    pub key: ProcessKey,
    pub facts: ProcessInfo,
    pub cgroup: Option<u64>,
    pub leader: Option<u8>,
    pub window: SampleWindow,
    pub origin: Origin,
    pub executed: bool,
    pub states: BTreeMap<Field, FieldState>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailAttempt {
    Pending,
    Completed,
}
pub(crate) struct Node {
    pub record: ProcessRecord,
    pub detail_attempt: Option<DetailAttempt>,
    pub retired_at: Option<std::time::Instant>,
    pub hits: BTreeMap<String, RuleHitSummary>,
    pub hit_charges: Vec<Arc<Charge>>,
    pub _charge: Arc<Charge>,
}
impl Node {
    pub fn needs_details(&self) -> bool {
        if self.record.lifecycle != Lifecycle::Present {
            return false;
        }
        match self.detail_attempt {
            Some(DetailAttempt::Pending) => return true,
            Some(DetailAttempt::Completed) => return false,
            _ => {}
        }
        let p = &self.record.latest.process;
        let user_image_missing = p.kind != Some(as_types::process_source::ProcessKind::Kernel)
            && (p.executable.is_none() || p.argv.is_none() || p.cwd.is_none());
        user_image_missing
            || p.name.is_none()
            || p.uid.is_none()
            || p.euid.is_none()
            || p.suid.is_none()
            || p.fsuid.is_none()
            || p.gid.is_none()
            || p.egid.is_none()
            || p.sgid.is_none()
            || p.fsgid.is_none()
            || p.pid_namespace_inode.is_none()
            || p.net_namespace_inode.is_none()
            || p.user_namespace_inode.is_none()
            || p.mount_namespace_inode.is_none()
            || p.namespace_tgids.is_none()
            || p.parent_pid.is_none()
            || p.parent_birth_boottime_ns.is_none()
            || p.kind.is_none()
            || p.exec_cookie.is_none()
            || self.record.latest.cgroup_id.is_none()
            || self.record.latest.leader_state.is_none()
    }
}
pub(crate) struct State {
    pub scan_active: bool,
    pub retire_queue: VecDeque<(std::time::Instant, ProcessKey)>,
    pub phase: TreePhase,
    pub generation: u64,
    pub revision: u64,
    pub epoch: u64,
    pub marker: u64,
    pub loss: u64,
    pub rejected: u64,
    pub gap: bool,
    pub hit_gap: bool,
    pub gap_serial: u64,
    pub initial_expected: Option<(u64, u64, u64)>,
    pub nodes: HashMap<ProcessKey, Node>,
    pub ordered: BTreeSet<ProcessKey>,
    pub pids: HashMap<u32, ProcessKey>,
    pub children: HashMap<ProcessKey, BTreeSet<ProcessKey>>,
    pub events: HashMap<EventKey, HashSet<String>>,
    pub event_process: HashMap<EventKey, ProcessKey>,
    pub pins: HashMap<ProcessKey, usize>,
    pub event_charges: HashMap<EventKey, Arc<Charge>>,
    pub event_times: HashMap<EventKey, u64>,
    pub closed_before: u64,
}
impl State {
    pub fn new() -> Self {
        Self {
            scan_active: false,
            retire_queue: VecDeque::new(),
            phase: TreePhase::Building,
            generation: 1,
            revision: 0,
            epoch: 0,
            marker: 0,
            loss: 0,
            rejected: 0,
            gap: false,
            hit_gap: false,
            gap_serial: 0,
            initial_expected: None,
            nodes: HashMap::new(),
            ordered: BTreeSet::new(),
            pids: HashMap::new(),
            children: HashMap::new(),
            events: HashMap::new(),
            event_process: HashMap::new(),
            pins: HashMap::new(),
            event_charges: HashMap::new(),
            event_times: HashMap::new(),
            closed_before: 0,
        }
    }
    pub fn uncertain(&mut self) {
        if self.phase != TreePhase::Closed {
            self.phase = TreePhase::Uncertain;
            self.gap = true;
            self.gap_serial = self.gap_serial.saturating_add(1);
        }
    }
    pub fn parent(key: ProcessKey, facts: &ProcessInfo) -> Option<ProcessKey> {
        match (facts.parent_pid, facts.parent_birth_boottime_ns) {
            (Some(pid), Some(birth)) if pid != 0 => Some(ProcessKey {
                boot_id: key.boot_id,
                host_tgid: pid,
                birth_boottime_ns: birth,
            }),
            _ => None,
        }
    }
    pub fn put(&mut self, c: Candidate, budget: &Arc<Budget>) -> Result<(), Error> {
        if self.phase == TreePhase::Closed {
            return Err(Error::Stopped);
        }
        if c.key.host_tgid == 0 || c.window.begin_boottime_ns > c.window.end_boottime_ns {
            return Err(Error::Invalid);
        }
        let old = self.nodes.get(&c.key);
        let was_pending = old.is_some_and(|n| n.detail_attempt == Some(DetailAttempt::Pending));
        let sampled_exec = (!c.executed
            && matches!(c.origin, Origin::StartupScan | Origin::Refresh))
        .then_some(c.facts.exec_cookie)
        .flatten();
        let old_parent = old.and_then(|n| Self::parent(c.key, &n.record.latest.process));
        let reset = old.is_some_and(|n| {
            c.executed
                && n.record
                    .latest
                    .stamps
                    .get(&Field::Executable)
                    .is_none_or(|v| v.window.end_boottime_ns <= c.window.end_boottime_ns)
        }) || old.is_some_and(|n| {
            c.facts.exec_cookie.is_some()
                && n.record.latest.process.exec_cookie.is_some()
                && c.facts.exec_cookie != n.record.latest.process.exec_cookie
                && n.record
                    .latest
                    .stamps
                    .get(&Field::ExecCookie)
                    .is_none_or(|st| st.window.end_boottime_ns <= c.window.begin_boottime_ns)
        });
        let mut facts = if reset {
            ProcessInfo::default()
        } else {
            old.map(|n| n.record.latest.process.clone())
                .unwrap_or_default()
        };
        let mut stamps = old
            .map(|n| n.record.latest.stamps.clone())
            .unwrap_or_default();
        if reset {
            for field in Field::ALL {
                stamps.insert(
                    field,
                    FieldStamp {
                        evidence_epoch: self.gap_serial,
                        origin: c.origin.clone(),
                        window: as_types::process_source::SampleWindow {
                            begin_boottime_ns: c.window.begin_boottime_ns,
                            end_boottime_ns: c.window.begin_boottime_ns,
                        },
                        state: FieldState::Stale,
                    },
                );
            }
        }
        let parent_changed = old.is_some_and(|n| {
            let fresh = n
                .record
                .latest
                .stamps
                .get(&Field::ParentPid)
                .is_none_or(|st| st.window.end_boottime_ns <= c.window.begin_boottime_ns)
                && n.record
                    .latest
                    .stamps
                    .get(&Field::ParentBirth)
                    .is_none_or(|st| st.window.end_boottime_ns <= c.window.begin_boottime_ns);
            fresh
                && (c
                    .facts
                    .parent_pid
                    .is_some_and(|v| Some(v) != n.record.latest.process.parent_pid)
                    || c.facts.parent_birth_boottime_ns.is_some_and(|v| {
                        Some(v) != n.record.latest.process.parent_birth_boottime_ns
                    }))
        });
        if parent_changed {
            facts.parent_pid = None;
            facts.parent_birth_boottime_ns = None;
            facts.parent_name = None;
            facts.parent_executable = None;
            facts.parent_argv = None;
            for field in [
                Field::ParentPid,
                Field::ParentBirth,
                Field::ParentName,
                Field::ParentExecutable,
                Field::ParentArgv,
            ] {
                stamps.insert(
                    field,
                    FieldStamp {
                        evidence_epoch: self.gap_serial,
                        origin: c.origin.clone(),
                        window: as_types::process_source::SampleWindow {
                            begin_boottime_ns: c.window.begin_boottime_ns,
                            end_boottime_ns: c.window.begin_boottime_ns,
                        },
                        state: FieldState::Stale,
                    },
                );
            }
        }
        macro_rules! invalidate {
            ($field:ident,$tag:ident) => {
                if stamps
                    .get(&Field::$tag)
                    .is_some_and(|st| st.evidence_epoch != self.gap_serial)
                {
                    facts.$field = None;
                    if let Some(st) = stamps.get_mut(&Field::$tag) {
                        st.state = FieldState::Stale;
                    }
                }
            };
        }
        invalidate!(name, Name);
        invalidate!(executable, Executable);
        invalidate!(argv, Argv);
        invalidate!(cwd, Cwd);
        invalidate!(uid, Uid);
        invalidate!(euid, Euid);
        invalidate!(gid, Gid);
        invalidate!(egid, Egid);
        invalidate!(parent_pid, ParentPid);
        invalidate!(parent_birth_boottime_ns, ParentBirth);
        invalidate!(parent_name, ParentName);
        invalidate!(parent_executable, ParentExecutable);
        invalidate!(parent_argv, ParentArgv);
        invalidate!(workload, Workload);
        invalidate!(kind, Kind);
        invalidate!(exec_cookie, ExecCookie);
        invalidate!(pid_namespace_inode, PidNamespace);
        invalidate!(net_namespace_inode, NetNamespace);
        invalidate!(user_namespace_inode, UserNamespace);
        invalidate!(mount_namespace_inode, MountNamespace);
        invalidate!(namespace_tgids, NamespaceTgids);
        invalidate!(suid, Suid);
        invalidate!(fsuid, Fsuid);
        invalidate!(sgid, Sgid);
        invalidate!(fsgid, Fsgid);
        macro_rules! merge {
            ($field:ident,$tag:ident) => {
                merge(
                    &mut facts.$field,
                    &c.facts.$field,
                    Field::$tag,
                    &mut stamps,
                    &c.origin,
                    c.window,
                    self.gap_serial,
                );
            };
        }
        merge!(name, Name);
        merge!(executable, Executable);
        merge!(argv, Argv);
        merge!(cwd, Cwd);
        merge!(uid, Uid);
        merge!(euid, Euid);
        merge!(gid, Gid);
        merge!(egid, Egid);
        merge!(parent_pid, ParentPid);
        merge!(parent_birth_boottime_ns, ParentBirth);
        merge!(parent_name, ParentName);
        merge!(parent_executable, ParentExecutable);
        merge!(parent_argv, ParentArgv);
        if c.cgroup.is_some()
            && c.facts.workload.is_none()
            && stamps
                .get(&Field::Workload)
                .is_some_and(|stamp| stamp.window.end_boottime_ns <= c.window.end_boottime_ns)
        {
            facts.workload = None;
            stamps.remove(&Field::Workload);
        } else {
            merge!(workload, Workload);
        }
        merge!(kind, Kind);
        merge!(exec_cookie, ExecCookie);
        merge!(pid_namespace_inode, PidNamespace);
        merge!(net_namespace_inode, NetNamespace);
        merge!(user_namespace_inode, UserNamespace);
        merge!(mount_namespace_inode, MountNamespace);
        merge!(namespace_tgids, NamespaceTgids);
        merge!(suid, Suid);
        merge!(fsuid, Fsuid);
        merge!(sgid, Sgid);
        merge!(fsgid, Fsgid);
        let mut cgroup = old.and_then(|n| n.record.latest.cgroup_id);
        let mut leader = old.and_then(|n| n.record.latest.leader_state);
        merge(
            &mut cgroup,
            &c.cgroup,
            Field::Cgroup,
            &mut stamps,
            &c.origin,
            c.window,
            self.gap_serial,
        );
        merge(
            &mut leader,
            &c.leader,
            Field::LeaderState,
            &mut stamps,
            &c.origin,
            c.window,
            self.gap_serial,
        );
        for (field, state) in c.states {
            if stamps
                .get(&field)
                .is_none_or(|st| st.window.end_boottime_ns <= c.window.begin_boottime_ns)
            {
                match field {
                    Field::Name => facts.name = None,
                    Field::Executable => facts.executable = None,
                    Field::Argv => facts.argv = None,
                    Field::Cwd => facts.cwd = None,
                    Field::Uid => facts.uid = None,
                    Field::Euid => facts.euid = None,
                    Field::Gid => facts.gid = None,
                    Field::Egid => facts.egid = None,
                    Field::Kind => facts.kind = None,
                    Field::ExecCookie => facts.exec_cookie = None,
                    Field::PidNamespace => facts.pid_namespace_inode = None,
                    Field::NetNamespace => facts.net_namespace_inode = None,
                    Field::UserNamespace => facts.user_namespace_inode = None,
                    Field::MountNamespace => facts.mount_namespace_inode = None,
                    Field::NamespaceTgids => facts.namespace_tgids = None,
                    Field::Suid => facts.suid = None,
                    Field::Fsuid => facts.fsuid = None,
                    Field::Sgid => facts.sgid = None,
                    Field::Fsgid => facts.fsgid = None,
                    Field::LeaderState => leader = None,
                    _ => {}
                }
                stamps.insert(
                    field,
                    FieldStamp {
                        evidence_epoch: self.gap_serial,
                        origin: c.origin.clone(),
                        window: c.window,
                        state,
                    },
                );
            }
        }
        // Preserve parent attributes with the parent's own sample provenance.
        // This fills an observed relationship; it never inherits child facts.
        if let Some(parent_key) = Self::parent(c.key, &facts)
            && let Some(parent) = self.nodes.get(&parent_key)
        {
            let parent = &parent.record.latest;
            macro_rules! parent_field {
                ($target:ident, $target_tag:ident, $source:ident, $source_tag:ident) => {
                    if c.facts.$target.is_none()
                        && let (Some(value), Some(stamp)) = (
                            parent.process.$source.as_ref(),
                            parent.stamps.get(&Field::$source_tag),
                        )
                        && stamp.state == FieldState::Known
                        && stamp.evidence_epoch == self.gap_serial
                        && stamp.window.end_boottime_ns <= c.window.begin_boottime_ns
                        && (!Field::$source_tag.thread_local()
                            || stamp.origin.same_thread(
                                parent_key,
                                Some(parent_key.host_tgid),
                                Some(parent_key.birth_boottime_ns),
                            ))
                        && (facts.$target.is_none()
                            || stamps.get(&Field::$target_tag).is_none_or(|old| {
                                old.window.end_boottime_ns < stamp.window.begin_boottime_ns
                            }))
                    {
                        facts.$target = Some(value.clone());
                        stamps.insert(Field::$target_tag, stamp.clone());
                    }
                };
            }
            parent_field!(parent_name, ParentName, name, Name);
            parent_field!(parent_executable, ParentExecutable, executable, Executable);
            parent_field!(parent_argv, ParentArgv, argv, Argv);
        }
        let is_new = old.is_none();
        let size = facts_bytes(&facts).saturating_add(Field::ALL.len().saturating_mul(192));
        let charge = match budget.take(size) {
            Ok(c) => c,
            Err(_) => {
                self.collect_except(std::time::Duration::ZERO, true, Some(c.key));
                budget.take(size)?
            }
        };
        let snapshot = Arc::new(ProcessSnapshot {
            process: facts,
            cgroup_id: cgroup,
            leader_state: leader,
            stamps,
            _charge: charge,
        });
        let node_charge = if is_new {
            Some(budget.take(1024)?)
        } else {
            None
        };
        let new_parent = Self::parent(c.key, &snapshot.process);
        if old_parent != new_parent
            && let Some(parent) = new_parent
        {
            let mut at = Some(parent);
            let mut seen = HashSet::new();
            while let Some(p) = at {
                if p == c.key || !seen.insert(p) {
                    return Err(Error::Invalid);
                }
                at = self
                    .nodes
                    .get(&p)
                    .and_then(|n| Self::parent(p, &n.record.latest.process));
            }
        }
        if old_parent != new_parent {
            if let Some(p) = old_parent
                && let Some(set) = self.children.get_mut(&p)
            {
                set.remove(&c.key);
                if set.is_empty() {
                    self.children.remove(&p);
                }
            }
            if let Some(p) = new_parent {
                self.children.entry(p).or_default().insert(c.key);
            }
        }
        self.revision = self.revision.saturating_add(1);
        if let Some(n) = self.nodes.get_mut(&c.key) {
            if reset {
                n.detail_attempt = None;
            }
            n.record.latest = snapshot;
            n.record.revision = self.revision;
        } else {
            let charge = node_charge.ok_or(Error::Invalid)?;
            self.nodes.insert(
                c.key,
                Node {
                    record: ProcessRecord {
                        key: c.key,
                        initial: snapshot.clone(),
                        latest: snapshot,
                        lifecycle: Lifecycle::Present,
                        first_observed: c.window,
                        exit_observed: None,
                        exit_code: None,
                        creator: None,
                        revision: self.revision,
                    },
                    detail_attempt: None,
                    retired_at: None,
                    hits: BTreeMap::new(),
                    hit_charges: Vec::new(),
                    _charge: charge,
                },
            );
            self.ordered.insert(c.key);
        }
        // A completed proc/Inspect read already tried the source-backed fields,
        // including unavailable ones. Do not finish an owned pending refresh early
        // or let an older image's sample suppress details for a newer exec.
        if !was_pending
            && let Some(cookie) = sampled_exec
            && let Some(node) = self.nodes.get_mut(&c.key)
            && node.record.latest.process.exec_cookie == Some(cookie)
        {
            node.detail_attempt = Some(DetailAttempt::Completed);
        }
        let present = self
            .nodes
            .get(&c.key)
            .is_some_and(|n| n.record.lifecycle == Lifecycle::Present);
        if present
            && self
                .pids
                .get(&c.key.host_tgid)
                .is_none_or(|old| old.birth_boottime_ns < c.key.birth_boottime_ns)
        {
            self.pids.insert(c.key.host_tgid, c.key);
        }
        Ok(())
    }
    pub fn retire(&mut self, key: ProcessKey, window: Option<SampleWindow>, code: Option<i32>) {
        if window.is_none()
            && self
                .nodes
                .get(&key)
                .is_some_and(|n| n.record.lifecycle == Lifecycle::ExitObserved)
        {
            return;
        }
        if let Some(n) = self.nodes.get_mut(&key) {
            n.record.lifecycle = if window.is_some() {
                Lifecycle::ExitObserved
            } else {
                Lifecycle::NoLongerPresent
            };
            n.record.exit_observed = window;
            n.record.exit_code = code;
            if n.retired_at.is_none() {
                let now = std::time::Instant::now();
                n.retired_at = Some(now);
                self.retire_queue.push_back((now, key));
            }
            self.revision = self.revision.saturating_add(1);
            n.record.revision = self.revision;
        }
        if self.pids.get(&key.host_tgid) == Some(&key) {
            self.pids.remove(&key.host_tgid);
        }
    }
    pub fn collect(&mut self, retention: std::time::Duration, pressure: bool) {
        self.collect_except(retention, pressure, None)
    }
    fn collect_except(
        &mut self,
        retention: std::time::Duration,
        pressure: bool,
        protected: Option<ProcessKey>,
    ) {
        if self.scan_active {
            return;
        }
        let mut dead = Vec::new();
        let mut attempts = 0;
        while attempts < 32
            && self
                .retire_queue
                .front()
                .is_some_and(|(t, _)| pressure || t.elapsed() >= retention)
        {
            if let Some((time, key)) = self.retire_queue.pop_front() {
                attempts += 1;
                let awaiting_fence = self
                    .nodes
                    .get(&key)
                    .and_then(|n| n.record.exit_observed)
                    .is_some_and(|window| window.end_boottime_ns >= self.closed_before);
                if Some(key) == protected || self.pins.contains_key(&key) || awaiting_fence {
                    self.retire_queue.push_back((time, key));
                    continue;
                }
                dead.push(key)
            }
        }
        for key in dead {
            if let Some(n) = self.nodes.remove(&key) {
                if let Some(p) = Self::parent(key, &n.record.latest.process)
                    && let Some(set) = self.children.get_mut(&p)
                {
                    set.remove(&key);
                    if set.is_empty() {
                        self.children.remove(&p);
                    }
                }
                self.ordered.remove(&key);
            }
        }
    }
}
fn merge<T: Clone + PartialEq>(
    value: &mut Option<T>,
    incoming: &Option<T>,
    field: Field,
    stamps: &mut BTreeMap<Field, FieldStamp>,
    origin: &Origin,
    window: SampleWindow,
    evidence_epoch: u64,
) {
    let Some(new) = incoming else { return };
    if let Some(old) = stamps.get(&field) {
        if window.end_boottime_ns < old.window.begin_boottime_ns {
            return;
        }
        if window.begin_boottime_ns < old.window.end_boottime_ns && value.as_ref() != Some(new) {
            *value = None;
            stamps.insert(
                field,
                FieldStamp {
                    evidence_epoch,
                    origin: origin.clone(),
                    window,
                    state: FieldState::Stale,
                },
            );
            return;
        }
    }
    *value = Some(new.clone());
    stamps.insert(
        field,
        FieldStamp {
            evidence_epoch,
            origin: origin.clone(),
            window,
            state: FieldState::Known,
        },
    );
}
pub(crate) fn facts_bytes(f: &ProcessInfo) -> usize {
    let bytes = [
        &f.name,
        &f.executable,
        &f.cwd,
        &f.parent_name,
        &f.parent_executable,
    ]
    .iter()
    .filter_map(|v| v.as_ref())
    .map(Vec::len)
    .sum::<usize>();
    bytes
        + [&f.argv, &f.parent_argv]
            .iter()
            .filter_map(|v| v.as_ref())
            .flat_map(|v| v.iter())
            .map(|v| v.len() + 32)
            .sum::<usize>()
        + f.namespace_tgids.as_ref().map_or(0, |ids| ids.len() * 4)
        + 512
}
