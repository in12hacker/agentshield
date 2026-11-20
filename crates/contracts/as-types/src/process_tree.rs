//! Process-tree identity, bounded configuration and authenticated query inputs.
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU64, NonZeroUsize};
/// One host process lifetime; driver reconnection does not change this key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessKey {
    /// Actual host boot UUID.
    pub boot_id: [u8; 16],
    /// Host thread-group identifier.
    pub host_tgid: u32,
    /// Kernel BOOTTIME group birth, without proc tick conversion.
    #[serde(with = "crate::decimal")]
    pub birth_boottime_ns: u64,
}
/// Source event identity, independent of process and decision identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventKey {
    /// Source activation.
    pub source_instance: [u8; 16],
    /// Source-local domain.
    #[serde(with = "crate::decimal")]
    pub sequence_domain: u64,
    /// Original source sequence.
    #[serde(with = "crate::decimal")]
    pub sequence: u64,
}
impl From<&crate::dispatch_event::EventData> for EventKey {
    fn from(event: &crate::dispatch_event::EventData) -> Self {
        let h = event.header();
        Self {
            source_instance: h.source_instance,
            sequence_domain: h.sequence_domain,
            sequence: h.sequence,
        }
    }
}
/// Sole configured process-tree resource policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessTreeSettings {
    /// Total charged live/retired snapshots and indexes, including retained query views.
    pub memory_budget_bytes: NonZeroUsize,
    /// Retired-node retention in milliseconds.
    pub retired_retention_ms: NonZeroU64,
    /// Background reconciliation interval in milliseconds.
    pub reconcile_interval_ms: NonZeroU64,
    /// Maximum ordinary query page size.
    pub query_page_size: NonZeroUsize,
}
/// Actual process lifetime observation, not a termination receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    /// Process was observed present, not necessarily scheduled/running.
    Present,
    /// Driver observed the final group exit.
    ExitObserved,
    /// Reconciliation confirmed the old instance no longer exists.
    NoLongerPresent,
}
/// Exact filters; process queries are not another HIPS matcher.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessFilter {
    /// Optional exact name bytes.
    pub name: Option<Vec<u8>>,
    /// Optional exact executable bytes.
    pub executable: Option<Vec<u8>>,
    /// Optional lifetime filter.
    pub lifecycle: Option<Lifecycle>,
    /// Optional native process kind.
    pub kind: Option<crate::process_source::ProcessKind>,
    /// Only positively identified host/container membership.
    pub workload: Option<crate::workload::Workload>,
}
/// Keyset cursor scoped to one tree, scan generation and query shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessCursor {
    /// Tree instance.
    pub instance: [u8; 16],
    /// Rebuild generation.
    #[serde(with = "crate::decimal")]
    pub generation: u64,
    /// Exact query digest.
    pub query: [u8; 32],
    /// Last returned immutable key.
    pub after: ProcessKey,
    /// Last aggregate digest for rule-hit pages only.
    pub hit_after: Option<String>,
}
/// Bounded query page, independent of arbitrary unrelated process updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    /// Requested positive page size.
    pub limit: NonZeroUsize,
    /// Continue this exact query.
    pub cursor: Option<ProcessCursor>,
}
/// Read-only tree commands handled by main's authenticated message thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessQuery {
    /// Actual tree completeness/resource status.
    Status,
    /// One exact instance.
    Get {
        /// Original key.
        key: ProcessKey,
    },
    /// Cached current PID hint only.
    ResolvePid {
        /// Host PID.
        pid: u32,
    },
    /// Filtered live keyset listing.
    List {
        /// Exact filter.
        filter: ProcessFilter,
        /// Page input.
        page: PageRequest,
    },
    /// Direct children by actual current parent key.
    Children {
        /// Parent key.
        key: ProcessKey,
        /// Page input.
        page: PageRequest,
    },
    /// Known ancestor path; missing nodes are reported.
    Ancestors {
        /// Child key.
        key: ProcessKey,
        /// Maximum path depth.
        limit: NonZeroUsize,
    },
    /// Descendants, iteratively traversed with cycle protection.
    Subtree {
        /// Root key.
        key: ProcessKey,
        /// Page input.
        page: PageRequest,
    },
    /// Per-instance actual observed rule hits.
    Hits {
        /// Instance key.
        key: ProcessKey,
        /// Page input.
        page: PageRequest,
    },
}
/// Explicit managed refresh target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RefreshTarget {
    /// Reconcile the full host inventory.
    System,
    /// Verify and update this original instance, never a later same PID.
    Process {
        /// Exact process key.
        key: ProcessKey,
    },
}
