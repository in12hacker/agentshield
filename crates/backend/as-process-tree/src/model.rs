//! Read-only process views; retained Arc views remain charged to their original tree.
use as_types::{dispatch_event::ProcessInfo, process_source::SampleWindow, process_tree::*};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Debug)]
pub(crate) struct Budget {
    pub used: AtomicUsize,
    pub max: usize,
}
#[derive(Debug)]
pub(crate) struct Charge {
    pub budget: Arc<Budget>,
    pub bytes: usize,
}
impl Charge {
    pub fn grow(&mut self, bytes: usize) -> Result<(), crate::Error> {
        let next = self
            .bytes
            .checked_add(bytes)
            .ok_or(crate::Error::ResourceLimit)?;
        self.budget.reserve(bytes)?;
        self.bytes = next;
        Ok(())
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
impl Budget {
    fn reserve(&self, bytes: usize) -> Result<(), crate::Error> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes).filter(|v| *v <= self.max)
            })
            .map_err(|_| crate::Error::ResourceLimit)?;
        Ok(())
    }
    pub fn take(self: &Arc<Self>, bytes: usize) -> Result<Arc<Charge>, crate::Error> {
        self.reserve(bytes)?;
        Ok(Arc::new(Charge {
            budget: self.clone(),
            bytes,
        }))
    }
}
/// Current inventory confidence, independent of driver enforcement state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TreePhase {
    /// Constructed, initial scan not yet confirmed.
    Building,
    /// Inventory and stream fence confirmed.
    Ready,
    /// A source gap or resource failure is known.
    Uncertain,
    /// No new tree work accepted.
    Closed,
}
/// Defined mutable fields, not arbitrary extensible metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// Active PID namespace inode.
    PidNamespace,
    /// Network namespace inode.
    NetNamespace,
    /// User namespace inode.
    UserNamespace,
    /// Mount namespace inode.
    MountNamespace,
    /// Group IDs across nested PID namespaces.
    NamespaceTgids,
    /// Saved user ID.
    Suid,
    /// Filesystem user ID.
    Fsuid,
    /// Saved group ID.
    Sgid,
    /// Filesystem group ID.
    Fsgid,
    /// Process name.
    Name,
    /// Executable pathname.
    Executable,
    /// Argument vector.
    Argv,
    /// Working directory.
    Cwd,
    /// Real user.
    Uid,
    /// Effective user.
    Euid,
    /// Real group.
    Gid,
    /// Effective group.
    Egid,
    /// Parent PID.
    ParentPid,
    /// Parent birth.
    ParentBirth,
    /// Parent name.
    ParentName,
    /// Parent image path.
    ParentExecutable,
    /// Parent argument vector.
    ParentArgv,
    /// Confirmed workload.
    Workload,
    /// Kernel cgroup membership.
    Cgroup,
    /// Exec continuity cookie.
    ExecCookie,
    /// Process kind.
    Kind,
    /// Sampled leader status, not group liveness.
    LeaderState,
}
impl Field {
    pub(crate) fn thread_local(self) -> bool {
        matches!(
            self,
            Self::Name
                | Self::Cwd
                | Self::Uid
                | Self::Euid
                | Self::Suid
                | Self::Fsuid
                | Self::Gid
                | Self::Egid
                | Self::Sgid
                | Self::Fsgid
                | Self::NetNamespace
                | Self::UserNamespace
                | Self::MountNamespace
                | Self::Workload
                | Self::Cgroup
        )
    }
    pub(crate) const ALL: [Self; 27] = [
        Self::PidNamespace,
        Self::NetNamespace,
        Self::UserNamespace,
        Self::MountNamespace,
        Self::NamespaceTgids,
        Self::Suid,
        Self::Fsuid,
        Self::Sgid,
        Self::Fsgid,
        Self::Name,
        Self::Executable,
        Self::Argv,
        Self::Cwd,
        Self::Uid,
        Self::Euid,
        Self::Gid,
        Self::Egid,
        Self::ParentPid,
        Self::ParentBirth,
        Self::ParentName,
        Self::ParentExecutable,
        Self::ParentArgv,
        Self::Workload,
        Self::Cgroup,
        Self::ExecCookie,
        Self::Kind,
        Self::LeaderState,
    ];
}
/// Observation provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// Initial proc read.
    StartupScan,
    /// Driver's original event.
    DriverEvent {
        /// Original source event key.
        event: EventKey,
        /// Actual host calling TID, if supplied.
        actor_tid: Option<u32>,
        /// Actual calling task lifetime, never inferred from TGID alone.
        #[serde(with = "as_types::decimal::optional")]
        actor_thread_birth_boottime_ns: Option<u64>,
    },
    /// Explicit/background proc read.
    Refresh,
}
impl Origin {
    pub(crate) fn same_thread(
        &self,
        process: ProcessKey,
        tid: Option<u32>,
        birth: Option<u64>,
    ) -> bool {
        match self {
            Self::DriverEvent {
                actor_tid: Some(t),
                actor_thread_birth_boottime_ns: Some(b),
                ..
            } => tid == Some(*t) && birth == Some(*b),
            Self::DriverEvent { .. } => false,
            Self::StartupScan | Self::Refresh => {
                tid == Some(process.host_tgid) && birth == Some(process.birth_boottime_ns)
            }
        }
    }
}
/// Why a field does or does not supply a current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldState {
    /// Actual sample available.
    Known,
    /// Source omitted it.
    Missing,
    /// Access was denied.
    Inaccessible,
    /// Read exceeded the configured resource budget.
    Truncated,
    /// Known invalidation or conflicting interval.
    Stale,
}
/// Per-field provenance and kernel-clock sampling interval.
#[derive(Debug, Clone, Serialize)]
pub struct FieldStamp {
    /// Cache evidence generation; a source gap invalidates older observations.
    #[serde(with = "as_types::decimal")]
    pub evidence_epoch: u64,
    /// Real source.
    pub origin: Origin,
    /// Kernel BOOTTIME interval.
    pub window: SampleWindow,
    /// Value validity.
    pub state: FieldState,
}
/// One immutable observed attribute set. Only two views are owned by a node.
#[derive(Debug, Serialize)]
pub struct ProcessSnapshot {
    /// Shared common process fields, never another schema.
    pub process: ProcessInfo,
    /// Actual cgroup ID, not a container identity.
    #[serde(with = "as_types::decimal::optional")]
    pub cgroup_id: Option<u64>,
    /// proc leader state only.
    pub leader_state: Option<u8>,
    /// Fixed-field observation stamps.
    pub stamps: BTreeMap<Field, FieldStamp>,
    #[serde(skip)]
    pub(crate) _charge: Arc<Charge>,
}
/// One actual process lifetime and its frozen first/latest observations.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessRecord {
    /// Strong process identity.
    pub key: ProcessKey,
    /// First accepted sample; never rewritten.
    pub initial: Arc<ProcessSnapshot>,
    /// Most recent confirmed field view.
    pub latest: Arc<ProcessSnapshot>,
    /// Group lifetime state.
    pub lifecycle: Lifecycle,
    /// Initial observation interval.
    pub first_observed: SampleWindow,
    /// Actual exit observation, not inferred wall time.
    pub exit_observed: Option<SampleWindow>,
    /// Actual raw group exit code if supplied.
    pub exit_code: Option<i32>,
    /// Real creator, distinct from actual parent.
    pub creator: Option<ProcessKey>,
    /// This record's revision.
    #[serde(with = "as_types::decimal")]
    pub revision: u64,
}
/// Parent identity and whether its record remains available.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentLink {
    /// Source explicitly identified the system root.
    Root,
    /// Exact parent lifetime.
    Known {
        /// Parent identity.
        key: ProcessKey,
        /// Parent record is retained.
        node_present: bool,
    },
    /// Parent identity is incomplete.
    Unresolved {
        /// Actual reported PID, if any.
        observed_pid: Option<u32>,
    },
}
/// Exact instance detail; no query creates a missing node.
#[derive(Debug, Serialize)]
pub struct ProcessDetails {
    /// Current cache evidence generation; initial history retains its original generation.
    #[serde(with = "as_types::decimal")]
    pub evidence_epoch: u64,
    /// Instance and attribute views.
    pub record: ProcessRecord,
    /// Parent at first observation.
    pub initial_parent: ParentLink,
    /// Latest confirmed parent.
    pub latest_parent: ParentLink,
    /// Known current children.
    pub children: usize,
}
/// Actual module completeness/resource status.
#[derive(Debug, Clone, Serialize)]
pub struct TreeStatus {
    /// Current evidence generation, advanced only by actual invalidation.
    #[serde(with = "as_types::decimal")]
    pub evidence_epoch: u64,
    /// Confidence/lifetime.
    pub phase: TreePhase,
    /// Stable tree instance.
    pub instance: [u8; 16],
    /// Rebuild generation.
    #[serde(with = "as_types::decimal")]
    pub generation: u64,
    /// Visible data revision.
    #[serde(with = "as_types::decimal")]
    pub revision: u64,
    /// Retained nodes.
    pub nodes: usize,
    /// Retired nodes.
    pub retired: usize,
    /// Charged bytes including external snapshot Arcs.
    pub memory_used: usize,
    /// Current source epoch.
    #[serde(with = "as_types::decimal")]
    pub tracking_epoch: u64,
    /// Last consumed fence.
    #[serde(with = "as_types::decimal")]
    pub marker: u64,
    /// Last native fact loss epoch.
    #[serde(with = "as_types::decimal")]
    pub loss_epoch: u64,
    /// Adapter-side rejected callbacks.
    #[serde(with = "as_types::decimal")]
    pub backend_rejected: u64,
    /// Gaps cannot be erased by a successful later scan.
    pub historical_gap: bool,
}
/// Bounded live keyset page.
#[derive(Debug, Serialize)]
pub struct ProcessPage {
    /// Exact per-node views.
    pub records: Vec<ProcessRecord>,
    /// Continue this shape.
    pub next_cursor: Option<ProcessCursor>,
    /// Revision of this page.
    #[serde(with = "as_types::decimal")]
    pub tree_revision: u64,
    /// Confidence at sampling.
    pub status: TreeStatus,
}
/// One retained actual rule hit aggregate.
#[derive(Debug, Clone, Serialize)]
pub struct RuleHitSummary {
    /// Original rule metadata.
    pub hit: as_types::main_ports::RuleHit,
    /// Process/file/network kind code.
    pub event_kind: u16,
    /// Observed unique hits.
    #[serde(with = "as_types::decimal")]
    pub count: u64,
    /// First source event.
    pub first: EventKey,
    /// Last source event.
    pub last: EventKey,
}
/// Results returned through the same authenticated main message entry.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum QueryReply {
    /// Actual current status.
    Status(TreeStatus),
    /// Exact retained instance.
    Details(ProcessDetails),
    /// Cached current PID hint.
    Pid(Option<ProcessKey>),
    /// Bounded process page.
    Page(ProcessPage),
    /// Known ancestor chain and any unresolved endpoint.
    Ancestors {
        /// Nearest parent first.
        records: Vec<ProcessRecord>,
        /// Incomplete or depth-limited path.
        incomplete: bool,
    },
    /// Bounded aggregate page.
    Hits {
        /// Observed actual hits.
        records: Vec<RuleHitSummary>,
        /// Offset for next hit page.
        next_cursor: Option<ProcessCursor>,
        /// Counts have known omissions.
        incomplete: bool,
    },
}
/// Initialization/refresh evidence, independent of rule enforcement.
#[derive(Debug, Serialize)]
pub struct ScanReceipt {
    /// Published status after the consumed source fence.
    pub status: TreeStatus,
    /// Number of distinct scan candidates accepted.
    pub scanned: usize,
    /// Actual scan kernel-clock interval.
    pub window: SampleWindow,
}
