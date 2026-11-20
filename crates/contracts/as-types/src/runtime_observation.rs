//! Original native-library observations. Missing semantics never discard acquired fields.
use crate::{
    driver::{NativeRequestOperation, NativeSyncThread},
    process_tree::EventKey,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// End condition of this field's actual byte read, independently of the acquired prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeCaptureStatus {
    /// Reached the original terminator; a zero-length prefix is a real empty string.
    Complete,
    /// The original read failed after the retained prefix.
    ReadFault {
        /// Original negative native errno.
        native_error: i32,
    },
    /// The current original capture container filled; no trailing bytes are invented.
    Capacity,
}

/// One observation's own source identity, not an assertion of a ToolCall or LSM operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequestHandoff {
    /// Unique original observation in the runtime source's sequence domain.
    pub observation: EventKey,
    /// Original Submission observation identifying this request lifetime, when actually handed off.
    /// The opening record itself has None; absent openings never prevent other observations.
    pub request: Option<EventKey>,
    /// Actual library operation, only when this occurrence was identified.
    pub operation: Option<NativeRequestOperation>,
    /// Actual calling thread instance at this observation.
    pub actor: NativeSyncThread,
    /// Source CLOCK_BOOTTIME in nanoseconds.
    #[serde(with = "crate::decimal")]
    pub observed_boottime_ns: u64,
    /// Fields acquired at this particular callback; no later target-memory reads.
    pub phase: NativeRequestPhase,
}

/// Original callback facts. Addresses are this occurrence's values, never cross-time identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeRequestPhase {
    /// Original owner callback: one site, one actual API payload and one owned projection.
    /// An API entry is not a claim that the requested kernel operation succeeded.
    Submission {
        /// Actual submitting process/exec and registers, acquired at this same callback.
        site: NativeApiSite,
        /// Arguments from the actual registered native API; no Agent-specific adapter.
        call: NativeLibraryCall,
        /// Unmodified immutable technical projection, encoded by as_runtime_capture.h.
        /// Shared slices retain the original wire allocation; they are not target pointers.
        projection: Option<Bytes>,
        /// This adapter's lossless public-format interpretation of that same projection.
        /// Structural candidates are not ToolExecution or HTTP bindings on their own.
        analysis: Option<CaptureAnalysis>,
        /// Actual acquisition or interpretation failure. Acquired raw bytes remain present.
        projection_error: Option<NativeProjectionError>,
    },
    /// Actual work submission. A missing request association remains missing.
    Submit {
        /// Original loop argument.
        #[serde(with = "crate::decimal")]
        loop_address: u64,
        /// Original submitted work object address.
        #[serde(with = "crate::decimal")]
        work_address: u64,
        /// Original worker callback address.
        #[serde(with = "crate::decimal")]
        work_callback: u64,
        /// Original completion callback address.
        #[serde(with = "crate::decimal")]
        done_callback: u64,
    },
    /// Entry into the actual worker callback.
    WorkEnter {
        /// Original work argument.
        #[serde(with = "crate::decimal")]
        work_address: u64,
    },
    /// Return from that worker callback, without inventing a C void return value.
    WorkLeave {
        /// Original work argument retained by this live callback scope.
        #[serde(with = "crate::decimal")]
        work_address: u64,
    },
    /// Actual completion callback and any result acquired there.
    Done {
        /// Original work argument.
        #[serde(with = "crate::decimal")]
        work_address: u64,
        /// Original signed native result; None means it was not acquired.
        result: Option<i64>,
        /// Actual negative nofault-read errno; None when no read failed or no read was attempted.
        result_read_error: Option<i32>,
    },
    /// Original request cleanup, the end of its recorded lifetime.
    Cleanup {
        /// Original request argument.
        #[serde(with = "crate::decimal")]
        request_address: u64,
    },
    /// A driver wait actually admitted inside this worker's original syscall scope.
    EventBound {
        /// Actual LSM event, distinct from this runtime observation.
        event: EventKey,
        /// Actual source path through this check; direct checks do not invent wait IDs.
        binding: NativeEventBinding,
        /// Actual enclosing owner API, when observed. This is not request ownership:
        /// a write API can flush earlier queue entries from another submission.
        enclosing_call: Option<EventKey>,
    },
    /// The original public native API returned; zero is not a completed-I/O claim.
    ApiReturned {
        /// Original signed C int return value.
        result: i32,
    },
    /// Actual process task created while this submission's spawn scope was active.
    ChildCreated {
        /// Kernel-observed child task-group lifetime, never a PID-only lookup.
        child: crate::process_tree::ProcessKey,
    },
}

/// One actual native submission site. Process birth and thread birth remain distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeApiSite {
    /// Host process lifetime; boot ID comes from the authenticated driver session.
    pub process: crate::process_tree::ProcessKey,
    /// Kernel self_exec_id of this observed process image.
    #[serde(with = "crate::decimal")]
    pub exec_cookie: u64,
    /// Instruction pointer at the original native entry.
    #[serde(with = "crate::decimal")]
    pub instruction_pointer: u64,
    /// Stack pointer at the original native entry.
    #[serde(with = "crate::decimal")]
    pub stack_pointer: u64,
    /// Actual architecture LR, when that register exists.
    #[serde(with = "crate::decimal::optional")]
    pub link_register: Option<u64>,
    /// Actual architecture frame-pointer register, when provided.
    #[serde(with = "crate::decimal::optional")]
    pub frame_pointer: Option<u64>,
}

/// Projection processing failure, independent of native request acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProjectionError {
    /// The original callback's snapshot allocation failed.
    Allocation,
    /// The acquired projection could not be decoded under the current technical contract.
    Malformed,
    /// Decoded capture ownership disagreed with this original submission site.
    OwnerMismatch,
}

/// The actual native check that connected a library invocation to an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeEventBinding {
    /// A real driver/consumer wait was admitted for this event.
    ControllerWait {
        /// Original source wait ID, not a frontend widget identity.
        #[serde(with = "crate::decimal")]
        wait_id: u64,
    },
    /// The native driver rule decided this check without a controller wait.
    DirectCheck,
}

/// Actual libuv API arguments. Addresses are this occurrence's values only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeLibraryCall {
    /// uv_fs_open(loop, request, path, flags, mode, callback).
    FileOpen {
        /// Original uv_fs_t address.
        #[serde(with = "crate::decimal")]
        request_address: u64,
        /// Original loop pointer, including zero for a synchronous request.
        #[serde(with = "crate::decimal")]
        loop_address: u64,
        /// Original C pathname pointer.
        #[serde(with = "crate::decimal")]
        path_address: u64,
        /// Original pathname byte prefix without its NUL terminator.
        path: Bytes,
        /// Actual read status of that prefix.
        path_status: NativeCaptureStatus,
        /// Original signed C flags.
        flags: i32,
        /// Original signed C mode.
        mode: i32,
        /// Original completion callback pointer, including zero.
        #[serde(with = "crate::decimal")]
        callback_address: u64,
    },
    /// uv_spawn; the child task and execution are separate later facts.
    Spawn {
        /// Original loop pointer.
        #[serde(with = "crate::decimal")]
        loop_address: u64,
        /// Original uv_process_t address, not a PID.
        #[serde(with = "crate::decimal")]
        process_address: u64,
        /// Original uv_process_options_t pointer.
        #[serde(with = "crate::decimal")]
        options_address: u64,
    },
    /// uv_tcp_connect; completion may occur after the API returns.
    TcpConnect {
        /// Original uv_connect_t address.
        #[serde(with = "crate::decimal")]
        request_address: u64,
        /// Original uv_tcp_t address.
        #[serde(with = "crate::decimal")]
        stream_address: u64,
        /// Original sockaddr pointer.
        #[serde(with = "crate::decimal")]
        address: u64,
        /// Original completion callback pointer.
        #[serde(with = "crate::decimal")]
        callback_address: u64,
    },
    /// uv_write or uv_write2; a submission is not a completed send.
    StreamWrite {
        /// Actual registered entry; it determines whether send_stream_address exists.
        entry: NativeStreamWriteEntry,
        /// Original uv_write_t address.
        #[serde(with = "crate::decimal")]
        request_address: u64,
        /// Original uv_stream_t address.
        #[serde(with = "crate::decimal")]
        stream_address: u64,
        /// Original uv_buf_t array pointer.
        #[serde(with = "crate::decimal")]
        buffers_address: u64,
        /// Original unsigned C buffer count.
        buffer_count: u32,
        /// Actual uv_write2 send_handle argument, including Some(0); uv_write has no such argument.
        #[serde(with = "crate::decimal::optional")]
        send_stream_address: Option<u64>,
        /// Original completion callback pointer.
        #[serde(with = "crate::decimal")]
        callback_address: u64,
    },
    /// Synchronous public stream try-write. It never owns a queued uv_write_t.
    StreamTryWrite {
        /// Exact original native entry, independent of queued write APIs.
        entry: NativeStreamTryWriteEntry,
        /// Original stream; it can be a socket, pipe or TTY.
        #[serde(with = "crate::decimal")]
        stream_address: u64,
        /// Original uv_buf_t array pointer in this invocation.
        #[serde(with = "crate::decimal")]
        buffers_address: u64,
        /// Original unsigned C buffer count.
        buffer_count: u32,
        /// Only uv_try_write2 has this parameter; its acquired zero remains Some(0).
        #[serde(with = "crate::decimal::optional")]
        send_stream_address: Option<u64>,
    },
    /// uv_udp_send; actual batch positions and results are separate facts.
    UdpSend {
        /// Original uv_udp_send_t address.
        #[serde(with = "crate::decimal")]
        request_address: u64,
        /// Original uv_udp_t address.
        #[serde(with = "crate::decimal")]
        stream_address: u64,
        /// Original uv_buf_t array pointer.
        #[serde(with = "crate::decimal")]
        buffers_address: u64,
        /// Original unsigned C buffer count.
        buffer_count: u32,
        /// Original sockaddr pointer, including zero for a connected socket.
        #[serde(with = "crate::decimal")]
        address: u64,
        /// Original completion callback pointer.
        #[serde(with = "crate::decimal")]
        callback_address: u64,
    },
}

/// Public native entry identity, without a version or Agent-name selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeStreamWriteEntry {
    /// uv_write has five declared arguments.
    Write,
    /// uv_write2 has six declared arguments.
    Write2,
}

/// Exact public synchronous entry, without a request object or completion callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeStreamTryWriteEntry {
    /// uv_try_write has three declared parameters.
    TryWrite,
    /// uv_try_write2 additionally has a send_handle parameter.
    TryWrite2,
}

/// Object references within this submission's original projection only.
/// Their meaning ends with the captured callback; they are never cross-time IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedMessageProof {
    /// Original current continuation Frame reference.
    #[serde(with = "crate::decimal")]
    pub frame: u64,
    /// Live Frame value whose own data fields directly held both objects.
    #[serde(with = "crate::decimal")]
    pub store: u64,
    /// Original public ToolCall object reference.
    #[serde(with = "crate::decimal")]
    pub tool: u64,
    /// Original public Assistant object reference.
    #[serde(with = "crate::decimal")]
    pub assistant: u64,
    /// Original Assistant.content array reference.
    #[serde(with = "crate::decimal")]
    pub content: u64,
    /// Actual zero-based content array index holding the same ToolCall object.
    #[serde(with = "crate::decimal")]
    pub content_index: u64,
    /// Every acquired byte was rechecked before that original callback returned.
    pub reads_consistent: bool,
}

/// Public shared-message structural match, independent of native/HTTP handoff.
/// Candidate count, process equality and time never establish an execution fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedMessageCandidate {
    /// Original public values; process identity is the submitting owner group.
    pub context: crate::main_ports::NativeToolContext,
    /// Object-identity basis in this same submission's retained raw projection.
    pub proof: SharedMessageProof,
    /// Missing/unrepresentable values with their original raw node references.
    pub issues: Vec<CaptureIssue>,
}

/// Explanation attached to a missing interpretation, separately from raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureIssue {
    /// Original node reference within this submission's projection.
    #[serde(with = "crate::decimal")]
    pub object: u64,
    /// Public-format field being interpreted, not a target memory lookup path.
    pub field: String,
    /// Actual interpreter outcome; the raw projection remains unchanged.
    pub reason: String,
}

/// One adapter interpretation of the original immutable submission projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureAnalysis {
    /// Original C status: 0 Complete, 1 Partial, 2 Unavailable, 3 Inconsistent.
    pub status: u32,
    /// All demonstrated structural matches, without choosing by count or time.
    pub candidates: Vec<SharedMessageCandidate>,
}
