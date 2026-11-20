//! Backend-side platform connection and initialization, independent of the native DriverContext.
use crate::{
    context::TargetPlatform,
    driver_control::{DriverCapabilities, DriverControl, DriverStatus},
    driver_image::DriverInitParts,
    driver_tools::DriverTools,
    main_ports::{DispatchInput, ModuleError},
    process_source::{ProcessFactsBarrier, ProcessFactsPort, ProcessTrackingStart},
};
use async_trait::async_trait;
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc;

/// Fixed packaged loader executable basename, shared with the configuration exporter.
pub const LOADER_FILE_NAME: &str = "as-bpf-loader";
/// Fixed packaged BPF object basename, shared with the configuration exporter.
pub const BPF_OBJECT_FILE_NAME: &str = "as_hooks.bpf.o";

/// Necessary initialization data borrowed from the configuration-owned layout by main.
#[derive(Debug, Clone)]
pub struct DriverConnectInput {
    /// Explicit expected build platform.
    pub platform: TargetPlatform,
    /// Actual host boot identity supplied by the process-tree owner.
    pub host_boot_id: [u8; 16],
    /// Existing configuration-resolved driver bundle directory; never discovered again here.
    pub driver_assets_directory: PathBuf,
}
/// Actual authenticated native connection information, not desired configuration.
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// Native-assigned authorization session; distinct from driver activation identity.
    pub session_id: [u8; 16],
    /// Platform verified by this concrete native transport.
    pub platform: TargetPlatform,
    /// Actual operations, fields, features and physical capacities.
    pub capabilities: DriverCapabilities,
    /// Actual status at this observation point.
    pub status: DriverStatus,
}
/// Native confirmation of one complete persistent adoption.
#[derive(Debug, Clone)]
pub struct AppliedPersistentState {
    /// Generation actually adopted by the driver.
    pub persistent_generation: u64,
    /// Actual status and both adopted digests, not a send acknowledgement.
    pub status: DriverStatus,
}
/// Backend platform adapter: connect first, export against actual capabilities, initialize second.
#[async_trait]
pub trait DriverAdapter: Send + Sync {
    /// Connect and validate actual device/loader/session identity. Failure retires owned resources.
    async fn connect(
        &self,
        input: &DriverConnectInput,
    ) -> Result<Arc<dyn DriverSession>, ModuleError>;
}
/// Fixed packaged controlled TLS ingress loader basename.
pub const TLS_INGRESS_LOADER_FILE_NAME: &str = "as-tls-ingress";
/// Fixed packaged connect marking and actual TCP lifecycle object.
pub const TLS_INGRESS_BPF_FILE_NAME: &str = "as_flow.bpf.o";
/// Explicit original connection scope resolved by the authenticated ingress owner.
#[derive(Debug, Clone)]
pub struct TlsIngressInput {
    /// Host boot identity supplied by the initialized process-tree owner.
    pub boot_id: [u8; 16],
    /// Exact configuration scope, resolved against live process and workload facts.
    pub scope: crate::flow::FlowScope,
    /// Actual target addresses resolved by the caller, in their TCP transport family.
    pub destinations: Vec<std::net::SocketAddr>,
    /// Source configuration identity retained on every admission.
    pub config_version: crate::configuration::ConfigVersion,
    /// Positive, bounded kernel and accepted connection capacity.
    pub max_connections: u32,
}
/// Actual accepted stream counters and generation.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TlsIngressStatus {
    /// This concrete lifetime's plan generation.
    #[serde(with = "crate::decimal")]
    pub generation: u64,
    /// Whether the helper still admits new marked connections.
    pub accepting: bool,
    /// Successfully transferred established streams.
    #[serde(with = "crate::decimal")]
    pub admitted: u64,
    /// Kernel-source or delivery failures observed by the adapter.
    #[serde(with = "crate::decimal")]
    pub rejected: u64,
}
/// Owned diversion lifetime, independent of HIPS enforcement.
#[async_trait]
pub trait TlsIngressSession: Send + Sync {
    /// Snapshot of actual local state, not configured intent.
    fn status(&self) -> Result<TlsIngressStatus, ModuleError>;
    /// Disable marking, remove this session's NAT table, detach and join its helper.
    async fn stop_accepting(&self) -> Result<TlsIngressStatus, ModuleError>;
}
/// Two bounded callback responsibilities on the same native session and existing Fast/Messages runtimes.
/// Controlled originals are independent of auxiliary observation/storage backpressure.
#[derive(Clone)]
pub struct DriverCallbacks {
    /// Original retained operations; their lifetime remains bounded by native max_pending.
    pub controlled: mpsc::Sender<DispatchInput>,
    /// Ordered auxiliary facts, direct results, status and retirement markers.
    pub events: mpsc::Sender<DriverEvent>,
}
/// Backend-owned native resources; does not expose or contain an in-kernel DriverContext.
#[async_trait]
pub trait DriverSession: DriverControl + ProcessFactsPort + DriverTools {
    /// Start the one supported controlled ingress, or explicitly report unavailable capability.
    async fn start_tls_ingress(
        &self,
        _input: TlsIngressInput,
        _output: mpsc::Sender<crate::flow::AdmittedConnection>,
    ) -> Result<Arc<dyn TlsIngressSession>, ModuleError> {
        Err(ModuleError::Unsupported)
    }
    /// Bind the scoped passive TCP consumer before the driver starts producing Group frames.
    fn bind_group_tcp(&self, _events: mpsc::Sender<NativeGroupTcp>) -> Result<(), ModuleError> {
        Err(ModuleError::Unsupported)
    }
    /// Actual handshake data. It never indicates desired settings are active.
    fn info(&self) -> DriverInfo;
    /// Actual backend delivery failures; temporary queue pressure is retained, not rejected.
    /// Separate from native DriverStatus.dropped.
    fn rejected_callbacks(&self) -> u64;
    /// Watch the same actual callback-rejection counter, independent of the bounded event queue.
    fn callback_loss(&self) -> tokio::sync::watch::Receiver<u64>;
    /// Bind both callback responsibilities and start process observation before scanning.
    async fn start_process_tracking(
        &self,
        callbacks: DriverCallbacks,
    ) -> Result<ProcessTrackingStart, ModuleError>;
    /// Atomically adopt complete images without replacing the observation queue.
    async fn initialize(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError>;
    /// Replace complete images without resetting actual runtime overrides.
    async fn replace_persistent(
        &self,
        parts: &DriverInitParts,
    ) -> Result<AppliedPersistentState, ModuleError>;
    /// Stop admitting originals; leave replies and already-admitted results live until drained.
    fn stop_admission(&self) -> Result<(), ModuleError>;
    /// Drain and release this connection's actual resources; retained handles become stopped.
    async fn shutdown(&self) -> Result<(), ModuleError>;
}

/// Disjoint increment produced at an actual native direct rule match.
pub struct DirectHitCount {
    /// Host thread group, bound by its original birth time.
    pub host_tgid: std::num::NonZeroU32,
    /// Kernel BOOTTIME group birth, in nanoseconds.
    pub birth_boottime_ns: u64,
    /// Actual category of the matched operation.
    pub kind: crate::main_ports::EventKind,
    /// Original matched rule identity and source revision.
    pub hit: crate::main_ports::RuleHit,
    /// Number of distinct original checks in this increment.
    pub count: std::num::NonZeroU64,
    /// Smallest original event key in this increment.
    pub first: crate::process_tree::EventKey,
    /// Largest original event key in this increment.
    pub last: crate::process_tree::EventKey,
}

/// Which admitted gate was observed inside the native syscall invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRequestOperation {
    /// File.Open within openat or openat2.
    FileOpen,
    /// Process.Execute within execve or execveat.
    Execute,
    /// Network.Connect within connect.
    Connect,
    /// Network.Send within sendto/sendmsg, socket write/writev, or sendmmsg.
    Send,
}

/// Return of one native syscall invocation containing an admitted driver wait.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequestReturn {
    /// The original gate event within the invocation.
    pub event: crate::process_tree::EventKey,
    /// Gate observed within this syscall.
    pub operation: NativeRequestOperation,
    /// The same decision identity used by the native wait and its LSM receipt.
    pub decision_id: crate::DecisionId,
    /// Driver-owned wait sequence, scoped by the authenticated native session.
    #[serde(with = "crate::decimal")]
    pub wait_id: u64,
    /// CLOCK_BOOTTIME sample at the outer native function return.
    #[serde(with = "crate::decimal")]
    pub returned_boottime_ns: u64,
    /// Original kernel syscall-wrapper return, retaining a negative error as supplied.
    pub return_value: i32,
}

/// Thread identity observed within the boot and source of a native fact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSyncThread {
    /// Original host thread-group ID.
    pub tgid: u32,
    /// Original host thread ID.
    pub tid: u32,
    /// Kernel BOOTTIME thread birth in nanoseconds.
    #[serde(with = "crate::decimal")]
    pub birth_boottime_ns: u64,
}

/// Wait evidence copied while the same queue entry was still live.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFutexWait {
    /// Driver-assigned ID for this observed lifetime of the entry.
    #[serde(with = "crate::decimal")]
    pub id: u64,
    /// BOOTTIME at the original wait callback.
    #[serde(with = "crate::decimal")]
    pub observed_boottime_ns: u64,
}

/// Original private futex selection. A missing wait leaves a candidate handoff.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFutexSelection {
    /// Unique original driver observation identity.
    pub event: crate::process_tree::EventKey,
    /// Actual waker at the selection callback.
    pub waker: NativeSyncThread,
    /// Actual waiter passed by the kernel to the wake queue.
    pub waiter: NativeSyncThread,
    /// BOOTTIME at the wake-queue callback.
    #[serde(with = "crate::decimal")]
    pub selected_boottime_ns: u64,
    /// Present only when the live queue entry and its thread both matched.
    pub wait: Option<NativeFutexWait>,
}

/// One self-owned observation from the authenticated Group TCP source.
#[derive(Debug, Serialize)]
pub struct NativeGroupTcp {
    /// Driver source instance and observation ordinal, with its own loss-detecting sequence.
    pub source: [u8; 16],
    /// Monotonic source-local observation ordinal, allocated even if its native frame batch is dropped.
    #[serde(with = "crate::decimal")]
    pub ordinal: u64,
    /// Driver configuration generation that selected the observation.
    #[serde(with = "crate::decimal")]
    pub generation: u64,
    /// Original TCP source port on IPv4 host loopback.
    pub source_port: u16,
    /// Original TCP destination port on IPv4 host loopback.
    pub destination_port: u16,
    /// TCP sequence number of the first byte, or ISN for SYN.
    pub sequence: u32,
    /// TCP acknowledgment number, used to check the original handshake.
    pub acknowledgment: u32,
    /// Kernel socket cookie, or zero when the packet has no full TCP socket.
    #[serde(with = "crate::decimal")]
    pub socket_cookie: u64,
    /// Packet bytes or the actual syscall actor for a TCP byte interval.
    pub observation: NativeGroupObservation,
}

/// The independently observed wire packet and actual completed I/O use one shared header.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeGroupObservation {
    /// First application send on an already established, unused OTLP TCP socket.
    FirstSend,
    /// IPv4 local output packet with its TCP flags and original payload.
    Packet {
        flags: u8,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        payload: Vec<u8>,
    },
    /// Actual accepted bytes of a locked TCP call with a consistent entry and exit sequence.
    Transfer {
        /// The direction of the TCP byte stream identifies sender and receiver independently.
        received: bool,
        /// Actual byte count, never including SYN/FIN sequence space.
        length: u32,
        /// Host process identifier of the actor.
        tgid: u32,
        /// Start of that process lifetime in CLOCK_BOOTTIME nanoseconds.
        #[serde(with = "crate::decimal")]
        birth_boottime_ns: u64,
    },
}

/// One actual native event, without a generic bus or executable capability for observations.
pub enum DriverEvent {
    /// Lifecycle/other source observation; no waiting or rule hit is invented.
    Observation(crate::dispatch_event::EventData),
    /// Writable-file release audit fact, without matching or actor attribution.
    FileClose(crate::security_log::FileCloseAuditRecord),
    /// Native syscall return, distinct from the original LSM verdict.
    RequestReturn(NativeRequestReturn),
    /// Private futex selection and its paired wait when observed.
    FutexSelection(NativeFutexSelection),
    /// Original native-library handoff, independent of interpreted tool semantics.
    RequestHandoff(crate::runtime_observation::NativeRequestHandoff),
    /// Disjoint direct-match increments, independent of result and log delivery.
    DirectHits(Vec<DirectHitCount>),
    /// A direct driver rule already completed its check; backend must not execute it again.
    DirectResult {
        /// Source facts used by that actual rule match.
        event: crate::dispatch_event::EventData,
        /// Actual direct driver choice and exact source hit.
        choice: crate::main_ports::RuleChoice,
        /// Actual check outcome, independent of subsequent I/O completion.
        outcome: crate::operation::ExecutionOutcome,
        /// Actual scope and independent signal results.
        termination: crate::operation::TerminationReport,
    },
    /// Actual native control status notification.
    Status(DriverStatus),
    /// Native confirmed that no old hook/original can still use this generation.
    PersistentRetired {
        /// Retired persistent generation; FIFO follows all its operation/result records.
        generation: u64,
    },
    /// Ordered source marker; main acknowledges only after preceding facts are processed.
    FactsBarrier {
        /// Actual source marker and process-loss generation.
        marker: ProcessFactsBarrier,
        /// Completion acknowledgement owned by the real fact consumer.
        processed: tokio::sync::oneshot::Sender<()>,
    },
    /// The native transport actually failed; a historical status is not a current status.
    TransportUnavailable,
}
