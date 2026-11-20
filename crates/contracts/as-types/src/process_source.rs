//! Narrow native process facts, independent of the process-tree implementation.
use crate::main_ports::ModuleError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Actual thread-group kind, not inferred from missing executable information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessKind {
    /// A userspace thread group.
    User,
    /// A kernel thread group.
    Kernel,
}

/// Bounds sampled in the native source's CLOCK_BOOTTIME domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleWindow {
    /// Source time before sampling.
    #[serde(with = "crate::decimal")]
    pub begin_boottime_ns: u64,
    /// Source time after sampling.
    #[serde(with = "crate::decimal")]
    pub end_boottime_ns: u64,
}

/// Native identity and cheap facts for one actual thread group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessBinding {
    /// TGID in the host PID namespace.
    pub host_tgid: u32,
    /// Actual kernel group birth; never reconstructed from proc clock ticks.
    #[serde(with = "crate::decimal")]
    pub birth_boottime_ns: u64,
    /// Actual physical parent TGID; zero denotes the source's root relationship.
    pub parent_pid: u32,
    /// Actual physical parent's group birth.
    #[serde(with = "crate::decimal")]
    pub parent_birth_boottime_ns: u64,
    /// Actual native process kind.
    pub kind: ProcessKind,
    /// Kernel self_exec_id sampled for this group.
    #[serde(with = "crate::decimal")]
    pub exec_cookie: u64,
    /// Actual default cgroup identity, when available.
    #[serde(with = "crate::decimal::optional")]
    pub cgroup_id: Option<u64>,
    /// Source sampling bounds.
    pub window: SampleWindow,
}

/// One input PID's explicit lookup result; absent and inaccessible are distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProcessInspection {
    /// Bound an actual process instance.
    Bound {
        /// Actual sampled binding.
        binding: ProcessBinding,
    },
    /// No task existed at lookup time.
    Gone {
        /// Exact requested TGID.
        host_tgid: u32,
    },
    /// The source refused to disclose this task.
    Inaccessible {
        /// Exact requested TGID.
        host_tgid: u32,
    },
}

/// Actual beginning of the session's process observation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessTrackingStart {
    /// Native initial PID namespace inode, used to validate proc visibility.
    #[serde(with = "crate::decimal")]
    pub host_pid_namespace_inode: u64,
    /// Native process-observation activation.
    #[serde(with = "crate::decimal")]
    pub tracking_epoch: u64,
    /// First source marker for this activation.
    #[serde(with = "crate::decimal")]
    pub begin_marker: u64,
    /// Native process-observation loss generation at the beginning.
    #[serde(with = "crate::decimal")]
    pub loss_epoch: u64,
    /// Actual adapter delivery failures after the beginning marker was consumed; not backpressure.
    #[serde(with = "crate::decimal")]
    pub backend_rejected: u64,
    /// Source BOOTTIME cutoff covered by the consumed beginning marker.
    #[serde(with = "crate::decimal")]
    pub cutoff_boottime_ns: u64,
}

/// Marker following every observation already entered before the barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFactsBarrier {
    /// Same native initial PID namespace inode as tracking start.
    #[serde(with = "crate::decimal")]
    pub host_pid_namespace_inode: u64,
    /// Native process-observation activation.
    #[serde(with = "crate::decimal")]
    pub tracking_epoch: u64,
    /// Monotonic source marker within that activation.
    #[serde(with = "crate::decimal")]
    pub marker: u64,
    /// Native process-observation loss generation at publication.
    #[serde(with = "crate::decimal")]
    pub loss_epoch: u64,
    /// Actual adapter delivery failures after this marker was consumed; not backpressure.
    #[serde(with = "crate::decimal")]
    pub backend_rejected: u64,
    /// Source BOOTTIME cutoff covered by all callbacks before this marker.
    #[serde(with = "crate::decimal")]
    pub cutoff_boottime_ns: u64,
    /// Direct matches lost before the native FIFO accepted their count increment.
    #[serde(with = "crate::decimal")]
    pub native_hit_loss: u64,
    /// Direct count batches refused by the adapter before the marker.
    #[serde(with = "crate::decimal")]
    pub backend_hit_rejected: u64,
}

/// Read-only fact source injected by main; no driver lifecycle or rule mutation capability.
#[async_trait]
pub trait ProcessFactsPort: Send + Sync {
    /// Inspect the exact requested host TGIDs in one bounded native transaction.
    async fn inspect(&self, host_tgids: &[u32]) -> Result<Vec<ProcessInspection>, ModuleError>;
    /// Resolve membership only when this exact sampled cgroup has a current verified binding.
    fn workload(
        &self,
        _cgroup_id: u64,
        _observed_boottime_ns: u64,
    ) -> Option<crate::workload::Workload> {
        None
    }
    /// Prove that complete original cgroup ancestry excludes every selected live instance.
    fn outside_containers(
        &self,
        _instances: &[crate::workload::ContainerInstance],
        _lineage: &[u64],
        _observed_boottime_ns: u64,
    ) -> bool {
        false
    }
    /// Return only after main has processed all observations preceding this source marker.
    async fn facts_barrier(&self) -> Result<ProcessFactsBarrier, ModuleError>;
}
