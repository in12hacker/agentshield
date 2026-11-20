//! Unified current event model; missing fields stay None and source bytes are never normalized.
//!
//! Both common-process and kind-specific areas contain little-endian TLVs:
//! `field:u16, reserved:u16=0, value_len:u32, value[value_len]`. Missing fields are omitted.
//! Bytes may be empty; numeric widths are exact. Argv is `count:u32, (len:u32, bytes)*`.
//! IP values are 4/16 network-order octets. Device is major:u32 followed by minor:u32.
//! No legacy event body, JSON, guessed identity, hash, database or control lease is decoded here.
//!
//! Common IDs: 1 name(bytes), 2 executable(bytes), 3 argv, 4 cwd(bytes), 5 uid(u32),
//! 6 euid(u32), 7 gid(u32), 8 egid(u32), 9 physical-parent TGID(u32), 10 parent birth(u64),
//! 11 parent name(bytes), 12 parent executable(bytes), 13 parent argv,
//! 14 process kind(u32: 0 user, 1 kernel), 15 exec continuity cookie(u64),
//! 16 active PID namespace(u64), 17 network namespace(u64), 18 user namespace(u64),
//! 19 mount namespace(u64), 20 namespace TGIDs(count:u32, values:u32*),
//! 21 saved UID(u32), 22 filesystem UID(u32), 23 saved GID(u32), 24 filesystem GID(u32),
//! 25 calling task birth(u64; decoded as header provenance, not a process attribute),
//! 26 cgroup ancestry(u64*), 27 syscall registers(72 bytes), 28 kernel stack(flags:u32,count:u32,IP:u64*).
//! 29 namespace TIDs(count:u32,values:u32*), 30 user stack(result:i32,flags:u32,IP:u64*).
//! Process IDs: 1 target executable(bytes), 2 target argv, 3 child TGID(u32),
//! 4 child birth(u64), 5 raw exit code(i32), 6 child's physical-parent TGID(u32),
//! 7 child's physical-parent birth(u64), 8 child's actual cgroup(u64).
//! File IDs: 1 path(bytes), 2 new path(bytes), 3 device(pair), 4 inode(u64),
//! 5 original file.f_flags(u32), 6 original access mask(u32),
//! 7 path_rename flags(u32, Rename only), 8 path_mkdir mode(u32 carrying umode_t, <=65535).
//! Network IDs: 1 local IP, 2 remote IP, 3 local port(u16), 4 remote port(u16),
//! 5 IANA protocol(u8), 6 target domain(bytes), 7 socket cookie(u64),
//! 8 socket network namespace inode(u64), distinct from the actor's current namespace.

use crate::process_source::ProcessKind;
use as_kernel_common::event_frame::{EventFrame, FrameDecodeError};
/// Canonical current wire kinds/stages, shared with the producer instead of a second enum.
pub use as_kernel_common::event_frame::{EventKind, EventStage};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use zerocopy::{
    FromBytes, Immutable, KnownLayout, Unaligned,
    little_endian::{I32, U16, U32, U64},
};

/// One typed copy of semantic header fields; wire framing lengths are consumed at the decode boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHeader {
    /// Original operation family.
    #[serde(
        serialize_with = "serialize_kind",
        deserialize_with = "deserialize_kind"
    )]
    pub kind: EventKind,
    /// Original source-described stage; does not grant control authority.
    #[serde(
        serialize_with = "serialize_stage",
        deserialize_with = "deserialize_stage"
    )]
    pub stage: EventStage,
    /// Current kind-specific operation code.
    pub operation: u32,
    /// Original producer activation, never a boot or process identity.
    pub source_instance: [u8; 16],
    /// Original source-local sequence domain.
    #[serde(with = "crate::decimal")]
    pub sequence_domain: u64,
    /// Original sequence in that domain; not a causal or execution claim.
    #[serde(with = "crate::decimal")]
    pub sequence: u64,
    /// Source CLOCK_BOOTTIME sample in nanoseconds, if supplied.
    #[serde(with = "crate::decimal::optional")]
    pub source_boottime_ns: Option<u64>,
    /// Original initiating host TGID; not a parent or domain target.
    pub actor_tgid: Option<u32>,
    /// Original initiating host TID.
    pub actor_tid: Option<u32>,
    /// Original group birth in kernel BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal::optional")]
    pub actor_birth_boottime_ns: Option<u64>,
    /// Original calling task birth, distinct from the group's lifetime.
    #[serde(with = "crate::decimal::optional")]
    pub actor_thread_birth_boottime_ns: Option<u64>,
    /// Original physical cgroup ID.
    #[serde(with = "crate::decimal::optional")]
    pub actor_cgroup_id: Option<u64>,
    /// Complete original cgroup ancestry, from the hierarchy root through the actor's cgroup.
    #[serde(default, with = "crate::decimal::optional_list")]
    pub actor_cgroup_lineage: Option<Vec<u64>>,
    /// Original calling thread IDs from host through active PID namespace; never process TGIDs.
    pub actor_namespace_tids: Option<Vec<u32>>,
}
/// Shared original actor/physical-parent attributes. Header actor identifiers are not repeated.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessInfo {
    /// Actual native process kind, never inferred from an absent executable.
    pub kind: Option<ProcessKind>,
    /// Kernel self_exec_id sampled for this original process instance.
    #[serde(with = "crate::decimal::optional")]
    pub exec_cookie: Option<u64>,
    /// Active PID namespace inode, not the namespace selected for future children.
    #[serde(with = "crate::decimal::optional")]
    pub pid_namespace_inode: Option<u64>,
    /// Actual network namespace inode at the observation point.
    #[serde(with = "crate::decimal::optional")]
    pub net_namespace_inode: Option<u64>,
    /// Actual user namespace inode; does not translate the host UID fields.
    #[serde(with = "crate::decimal::optional")]
    pub user_namespace_inode: Option<u64>,
    /// Actual mount namespace inode when the source supplies it.
    #[serde(with = "crate::decimal::optional")]
    pub mount_namespace_inode: Option<u64>,
    /// Group IDs ordered from the host PID namespace into nested namespaces.
    pub namespace_tgids: Option<Vec<u32>>,
    /// Original actor membership supplied by platform facts, never inferred from missing cgroup.
    pub workload: Option<crate::workload::Workload>,
    /// Original actor name bytes.
    pub name: Option<Vec<u8>>,
    /// Original actor executable name bytes, not an execution target or permanent object identity.
    pub executable: Option<Vec<u8>>,
    /// Original actor arguments, preserving each byte string and empty argument.
    pub argv: Option<Vec<Vec<u8>>>,
    /// Original actor working-directory name bytes.
    pub cwd: Option<Vec<u8>>,
    /// Captured actor real user ID.
    pub uid: Option<u32>,
    /// Captured actor effective user ID.
    pub euid: Option<u32>,
    /// Saved user ID in the host user namespace.
    pub suid: Option<u32>,
    /// Filesystem user ID in the host user namespace.
    pub fsuid: Option<u32>,
    /// Captured actor real group ID.
    pub gid: Option<u32>,
    /// Captured actor effective group ID.
    pub egid: Option<u32>,
    /// Saved group ID in the host user namespace.
    pub sgid: Option<u32>,
    /// Filesystem group ID in the host user namespace.
    pub fsgid: Option<u32>,
    /// Original physical parent's host TGID, not a creator or execution target inferred elsewhere.
    pub parent_pid: Option<u32>,
    /// Original physical-parent group birth in BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal::optional")]
    pub parent_birth_boottime_ns: Option<u64>,
    /// Original physical-parent name bytes.
    pub parent_name: Option<Vec<u8>>,
    /// Original physical-parent executable name bytes.
    pub parent_executable: Option<Vec<u8>>,
    /// Original physical-parent arguments.
    pub parent_argv: Option<Vec<Vec<u8>>>,
}
// The exhaustive patterns keep the two operations aligned with the public shape.
macro_rules! process_info_fields {
    ($apply:ident) => {
        $apply!(
            kind,
            exec_cookie,
            pid_namespace_inode,
            net_namespace_inode,
            user_namespace_inode,
            mount_namespace_inode,
            namespace_tgids,
            workload,
            name,
            executable,
            argv,
            cwd,
            uid,
            euid,
            suid,
            fsuid,
            gid,
            egid,
            sgid,
            fsgid,
            parent_pid,
            parent_birth_boottime_ns,
            parent_name,
            parent_executable,
            parent_argv,
        )
    };
}
impl ProcessInfo {
    /// Move only missing attributes; supplied empty/zero values remain the original values.
    pub fn fill_missing(&mut self, other: Self) {
        macro_rules! take_missing {
            ($($field:ident),+ $(,)?) => {
                let Self { $($field),+ } = other;
                $(if self.$field.is_none() { self.$field = $field; })+
            };
        }
        process_info_fields!(take_missing);
    }

    fn accepts_enrichment(&self, candidate: &Self) -> bool {
        macro_rules! compare {
            ($($field:ident),+ $(,)?) => {{
                let Self { $($field),+ } = self;
                $(kept($field, &candidate.$field))&&+
            }};
        }
        process_info_fields!(compare)
    }
}

fn kept<T: PartialEq>(original: &Option<T>, candidate: &Option<T>) -> bool {
    original.is_none() || original == candidate
}
/// Process-specific target/child/result facts, never substitutes for shared actor fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessData {
    /// Requested execution image name bytes.
    pub target_executable: Option<Vec<u8>>,
    /// Requested execution arguments.
    pub target_argv: Option<Vec<Vec<u8>>>,
    /// Captured child host TGID.
    pub child_pid: Option<u32>,
    /// Captured child group birth in BOOTTIME nanoseconds.
    #[serde(with = "crate::decimal::optional")]
    pub child_birth_boottime_ns: Option<u64>,
    /// Created process's actual physical parent, distinct from the creator.
    pub child_parent_pid: Option<u32>,
    /// Physical parent's original process-group birth.
    #[serde(with = "crate::decimal::optional")]
    pub child_parent_birth_boottime_ns: Option<u64>,
    /// Created process's actual cgroup, not the creator's inferred container.
    #[serde(with = "crate::decimal::optional")]
    pub child_cgroup_id: Option<u64>,
    /// Actual child attributes sampled at creation; never inherited by guess from the creator.
    pub child_process: Option<Box<ProcessInfo>>,
    /// Original signed exit-code value; no wait-status interpretation is added.
    pub exit_code: Option<i32>,
}
/// Same-hook effect of the requested mount on the bound self-protection objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum MountEffect {
    /// The actual mount operation changes only an independent view of those objects.
    IndependentView = 1,
    /// The operation affects the actual protected tree or its protected filesystem.
    ProtectedTree = 2,
    /// The source could not establish either effect from the current topology.
    Unknown = 3,
}

/// File-specific name/object/access samples; device/inode are not an open-file-description lease.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileData {
    /// Original file name bytes.
    pub path: Option<Vec<u8>>,
    /// Original destination name bytes for the relevant operation.
    pub new_path: Option<Vec<u8>>,
    /// Filesystem device major/minor, in that order; not raw dev_t/st_dev aliases.
    pub device: Option<(u32, u32)>,
    /// Original inode sample.
    #[serde(with = "crate::decimal::optional")]
    pub inode: Option<u64>,
    /// Original Linux file.f_flags, not the listener's generated FD mode.
    pub open_flags: Option<u32>,
    /// Original access mask, without deriving success from it.
    pub access_mask: Option<u32>,
    /// Original path_rename flags, only for Rename; Some(0) is a captured zero, not missing.
    pub rename_flags: Option<u32>,
    /// Original creation hook umode_t in a u32, in 0..=65535.
    pub creation_mode: Option<u32>,
    /// Original requested protection bitset at the mapping admission hook.
    #[serde(with = "crate::decimal::optional")]
    pub map_requested_prot: Option<u64>,
    /// Kernel-effective protection bitset supplied by the same hook.
    #[serde(with = "crate::decimal::optional")]
    pub map_effective_prot: Option<u64>,
    /// Original mapping flags bitset supplied by the same hook.
    #[serde(with = "crate::decimal::optional")]
    pub map_flags: Option<u64>,
    /// Original nonnegative target size, in bytes, from the file-object setattr check.
    #[serde(with = "crate::decimal::optional")]
    pub truncate_size: Option<u64>,
    /// Original iattr.ia_valid bitset at the same check.
    pub truncate_attributes: Option<u32>,
    /// Actual same-hook mount effect; None means this source did not provide the fact.
    pub mount_effect: Option<MountEffect>,
}
fn valid_file_parameters(operation: u32, data: &FileData) -> bool {
    use crate::event_operation::FileOperation;
    let mapping = match (
        data.map_requested_prot,
        data.map_effective_prot,
        data.map_flags,
    ) {
        (None, None, None) => true,
        (Some(_), Some(_), Some(_)) => operation == FileOperation::MapSharedWritable as u32,
        _ => false,
    };
    let truncate = match (data.truncate_size, data.truncate_attributes) {
        (None, None) => true,
        (Some(size), Some(_)) => {
            operation == FileOperation::TruncateOpenFile as u32 && size <= i64::MAX as u64
        }
        _ => false,
    };
    mapping
        && truncate
        && (data.mount_effect.is_none() || operation == FileOperation::Mount as u32)
        && (data.rename_flags.is_none() || operation == FileOperation::Rename as u32)
        && data.creation_mode.is_none_or(|mode| {
            (operation == FileOperation::CreateDirectory as u32
                || operation == FileOperation::CreateFile as u32)
                && mode <= u32::from(u16::MAX)
        })
}
/// One copied UDP/53 send iterator, without freezing later protocol consumption.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsSendSnapshot {
    /// Iterator bytes remaining at this hook, not a later send return value.
    #[serde(with = "crate::decimal")]
    pub requested_bytes: u64,
    /// Bytes this hook attempted to copy once, bounded by its original frame capacity.
    pub attempted_bytes: u32,
    /// Original msghdr flags observed with this iterator.
    pub send_flags: u32,
    /// Actual copied prefix. Its length is the copied byte count, including a successful zero read.
    pub data: Vec<u8>,
    /// One interpretation of these same bytes; None means not yet interpreted.
    pub analysis: Option<DnsQueryAnalysis>,
}
/// Interpretation of a send snapshot, never proof of a later packet or website connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum DnsQueryAnalysis {
    /// One standard query question; its name is NetworkData.target_domain.
    Question {
        /// Original question record type.
        qtype: u16,
        /// Original question class.
        qclass: u16,
    },
    /// The copied DNS header has QR set.
    Response,
    /// The copied DNS opcode is not QUERY.
    OtherOpcode {
        /// Original four-bit DNS opcode.
        opcode: u8,
    },
    /// The complete copied header declares zero questions.
    NoQuestion,
    /// The complete copied header declares more than one question.
    MultipleQuestions {
        /// Original question count.
        count: u16,
    },
    /// A completely copied payload cannot contain the fixed DNS header.
    NotDns,
    /// The sole question failed mature wire parsing; original bytes remain intact.
    Malformed {
        /// Parser's actual error, not a guessed network failure.
        reason: String,
    },
    /// The one copy returned fewer bytes than the attempted amount.
    ShortRead,
    /// The frame could retain only a prefix of the original iterator.
    CaptureLimited,
}

/// Network-specific endpoint/protocol samples, without attributing an actor from a tuple.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkData {
    /// Original kernel socket cookie; not a PID or a reusable FD number.
    #[serde(with = "crate::decimal::optional")]
    pub socket_cookie: Option<u64>,
    /// Namespace of the actual socket, which may differ from the caller's namespace.
    #[serde(with = "crate::decimal::optional")]
    pub socket_net_namespace_inode: Option<u64>,
    /// Original local address.
    pub local_ip: Option<IpAddr>,
    /// Original remote address.
    pub remote_ip: Option<IpAddr>,
    /// Original local port.
    pub local_port: Option<u16>,
    /// Original remote port.
    pub remote_port: Option<u16>,
    /// Original IANA IP protocol number.
    pub protocol: Option<u8>,
    /// The copied DNS question name in escaped presentation, preserving its original case.
    pub target_domain: Option<Vec<u8>>,
    /// Same-hook UDP/53 byte snapshot; None means this source did not capture one.
    pub dns_snapshot: Option<DnsSendSnapshot>,
}
/// Only the selected operation family's exclusive data.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum EventPayload {
    /// Process target/child/result fields.
    Process(ProcessData),
    /// File object/name/access fields.
    File(FileData),
    /// Network endpoints/protocol fields.
    Network(NetworkData),
}
impl EventPayload {
    /// Derive the payload family from its variant.
    pub fn kind(&self) -> EventKind {
        match self {
            Self::Process(_) => EventKind::Process,
            Self::File(_) => EventKind::File,
            Self::Network(_) => EventKind::Network,
        }
    }
}
/// Number of raw argument registers returned by Linux syscall_get_arguments.
pub const SYSCALL_ARGUMENT_COUNT: usize = 6;
/// Maximum returned kernel instruction pointers in the current capture buffer.
pub const KERNEL_STACK_LIMIT: usize = 32;

/// User register values copied at the original operation's hook, never dereferenced later.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyscallSnapshot {
    /// Actual Linux AUDIT_ARCH code supplied by syscall_get_arch.
    pub audit_arch: u32,
    /// Actual syscall_get_nr value; -1 is retained without claiming an active syscall.
    pub number: i32,
    /// Original user program-counter value in bytes, not a persistent identity.
    #[serde(with = "crate::decimal")]
    pub instruction_pointer: u64,
    /// Original user stack-pointer value in bytes, not a delayed-read capability.
    #[serde(with = "crate::decimal")]
    pub stack_pointer: u64,
    /// Exactly six original raw argument values in syscall_get_arguments order.
    #[serde(with = "crate::decimal::list")]
    pub arguments: Vec<u64>,
}

/// Kernel IP sequence returned by stack_trace_save at the original hook.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackSnapshot {
    /// Original order, repeated addresses and zero values are preserved; at most 32 entries.
    #[serde(with = "crate::decimal::list")]
    pub instruction_pointers: Vec<u64>,
    /// True exactly when the 32-slot buffer was filled; neither value proves completeness.
    pub buffer_full: bool,
}

/// Original BPF user-stack helper result and copied IP sequence, from one callback.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserStackSnapshot {
    /// Copied byte count on success; actual negative Linux errno on failure.
    pub result: i32,
    /// Original returned frames. A failed helper has no returned frames.
    pub snapshot: StackSnapshot,
}

fn valid_stack(value: &StackSnapshot) -> bool {
    value.instruction_pointers.len() <= KERNEL_STACK_LIMIT
        && value.buffer_full == (value.instruction_pointers.len() == KERNEL_STACK_LIMIT)
}

fn valid_user_stack(value: &UserStackSnapshot) -> bool {
    valid_stack(&value.snapshot)
        && if value.result < 0 {
            value.result >= -4095 && value.snapshot.instruction_pointers.is_empty()
        } else {
            value.result as usize == value.snapshot.instruction_pointers.len() * 8
        }
}

fn valid_snapshots(
    syscall: Option<&SyscallSnapshot>,
    stack: Option<&StackSnapshot>,
    user: Option<&UserStackSnapshot>,
) -> bool {
    syscall.is_none_or(|value| value.arguments.len() == SYSCALL_ARGUMENT_COUNT)
        && stack.is_none_or(valid_stack)
        && user.is_none_or(valid_user_stack)
}

/// One normalized current event with shared process fields stored once.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EventData {
    header: EventHeader,
    host_boot_id: Option<[u8; 16]>,
    /// Shared original process attributes; later enrichment fills None only.
    pub process: ProcessInfo,
    /// Family-specific original fields.
    pub payload: EventPayload,
    /// Original user registers, if the source supplied them. Never filled by later enrichment.
    pub syscall: Option<SyscallSnapshot>,
    /// Original kernel IP sequence, if supplied. Never inherited from a process snapshot.
    pub kernel_stack: Option<StackSnapshot>,
    /// Same-callback user IPs and actual helper result; never filled by later enrichment.
    pub user_stack: Option<UserStackSnapshot>,
}
impl<'de> serde::Deserialize<'de> for EventData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Incoming {
            header: EventHeader,
            host_boot_id: Option<[u8; 16]>,
            process: ProcessInfo,
            payload: serde_json::Value,
            syscall: Option<SyscallSnapshot>,
            kernel_stack: Option<StackSnapshot>,
            user_stack: Option<UserStackSnapshot>,
        }
        let value = <Incoming as serde::Deserialize>::deserialize(deserializer)?;
        let header = value.header;
        if header.source_instance == [0; 16]
            || header.sequence_domain == 0
            || header.sequence == 0
            || value.host_boot_id == Some([0; 16])
            || !valid_operation(header.kind, header.stage, header.operation)
            || !valid_cgroup_lineage(&header)
            || !valid_namespace_tids(&header)
            || !valid_snapshots(
                value.syscall.as_ref(),
                value.kernel_stack.as_ref(),
                value.user_stack.as_ref(),
            )
        {
            return Err(D::Error::custom(
                "invalid current event identity or operation",
            ));
        }
        let payload = match header.kind {
            EventKind::Process => EventPayload::Process(
                serde_json::from_value(value.payload).map_err(D::Error::custom)?,
            ),
            EventKind::File => {
                let data = serde_json::from_value(value.payload).map_err(D::Error::custom)?;
                if !valid_file_parameters(header.operation, &data) {
                    return Err(D::Error::custom("invalid original file parameters"));
                }
                EventPayload::File(data)
            }
            EventKind::Network => EventPayload::Network(
                serde_json::from_value(value.payload).map_err(D::Error::custom)?,
            ),
        };
        Ok(Self {
            header,
            host_boot_id: value.host_boot_id,
            process: value.process,
            payload,
            syscall: value.syscall,
            kernel_stack: value.kernel_stack,
            user_stack: value.user_stack,
        })
    }
}
fn valid_operation(kind: EventKind, stage: EventStage, code: u32) -> bool {
    use crate::event_operation::{FileOperation, NetworkOperation, ProcessOperation};
    match kind {
        EventKind::Process if code == ProcessOperation::Execute as u32 => true,
        EventKind::Process
            if code == ProcessOperation::Create as u32 || code == ProcessOperation::Exit as u32 =>
        {
            stage == EventStage::Observation
        }
        EventKind::File => FileOperation::is_defined_code(code),
        EventKind::Network => {
            code == NetworkOperation::Connect as u32 || code == NetworkOperation::Send as u32
        }
        _ => false,
    }
}
impl EventData {
    /// True only when this original's header and all supplied facts remain unchanged.
    /// A caller may fill missing process/payload facts, but cannot fill or change hook snapshots.
    pub fn accepts_enrichment(&self, candidate: &Self) -> bool {
        if self.header != candidate.header
            || !kept(&self.host_boot_id, &candidate.host_boot_id)
            || self.syscall != candidate.syscall
            || self.kernel_stack != candidate.kernel_stack
            || self.user_stack != candidate.user_stack
            || !valid_snapshots(
                candidate.syscall.as_ref(),
                candidate.kernel_stack.as_ref(),
                candidate.user_stack.as_ref(),
            )
        {
            return false;
        }
        if let EventPayload::File(data) = &candidate.payload
            && !valid_file_parameters(self.header.operation, data)
        {
            return false;
        }
        if !self.process.accepts_enrichment(&candidate.process) {
            return false;
        }
        macro_rules! fields {
            ($ty:ident, $original:ident, $candidate:ident, $($field:ident),+ $(,)?) => {{
                let $ty { $($field),+ } = $original;
                $(kept($field, &$candidate.$field))&&+
            }};
        }
        match (&self.payload, &candidate.payload) {
            (EventPayload::Process(original), EventPayload::Process(new)) => {
                let ProcessData {
                    target_executable,
                    target_argv,
                    child_pid,
                    child_birth_boottime_ns,
                    child_parent_pid,
                    child_parent_birth_boottime_ns,
                    child_cgroup_id,
                    child_process,
                    exit_code,
                } = original;
                kept(target_executable, &new.target_executable)
                    && kept(target_argv, &new.target_argv)
                    && kept(child_pid, &new.child_pid)
                    && kept(child_birth_boottime_ns, &new.child_birth_boottime_ns)
                    && kept(child_parent_pid, &new.child_parent_pid)
                    && kept(
                        child_parent_birth_boottime_ns,
                        &new.child_parent_birth_boottime_ns,
                    )
                    && kept(child_cgroup_id, &new.child_cgroup_id)
                    && kept(exit_code, &new.exit_code)
                    && child_process.as_ref().is_none_or(|old| {
                        new.child_process
                            .as_ref()
                            .is_some_and(|child| old.accepts_enrichment(child))
                    })
            }
            (EventPayload::File(original), EventPayload::File(new)) => fields!(
                FileData,
                original,
                new,
                path,
                new_path,
                device,
                inode,
                open_flags,
                access_mask,
                rename_flags,
                creation_mode,
                map_requested_prot,
                map_effective_prot,
                map_flags,
                truncate_size,
                truncate_attributes,
                mount_effect
            ),
            (EventPayload::Network(original), EventPayload::Network(new)) => fields!(
                NetworkData,
                original,
                new,
                local_ip,
                remote_ip,
                local_port,
                remote_port,
                protocol,
                socket_cookie,
                socket_net_namespace_inode,
                target_domain,
                dns_snapshot
            ),
            _ => false,
        }
    }
    /// Original typed header, without mutable access or a duplicated raw prefix.
    pub fn header(&self) -> &EventHeader {
        &self.header
    }
    /// Actual local boot scope attached by the native ingress owner.
    pub fn host_boot_id(&self) -> Option<[u8; 16]> {
        self.host_boot_id
    }
    /// Attach the boot identity once. Conflicting or empty identities are rejected.
    pub fn bind_host_boot(&mut self, id: [u8; 16]) -> bool {
        if id == [0; 16] || self.host_boot_id.is_some_and(|prior| prior != id) {
            return false;
        }
        self.host_boot_id = Some(id);
        true
    }
    /// Original validated operation family.
    pub fn kind(&self) -> EventKind {
        self.header.kind
    }
    /// Original validated source stage.
    pub fn stage(&self) -> EventStage {
        self.header.stage
    }
}
/// Structural rejection of the current frame/TLV schema; missing optional fields are valid.
#[derive(Debug, thiserror::Error)]
pub enum EventDecodeError {
    /// Operation code or event stage is not defined for this family.
    #[error("invalid event operation or stage")]
    Operation,
    /// Invalid current shared frame.
    #[error("invalid current event frame: {0:?}")]
    Frame(FrameDecodeError),
    /// An item, value or argument exceeds the remaining input, or leaves trailing argument bytes.
    #[error("invalid TLV length")]
    Length,
    /// The current item's reserved field is nonzero.
    #[error("nonzero TLV reserved field")]
    Reserved,
    /// This current region does not define the supplied field ID.
    #[error("unknown TLV field")]
    UnknownField,
    /// A field occurs twice in the same region.
    #[error("duplicate TLV field")]
    DuplicateField,
    /// A scalar/IP does not have its exact declared width.
    #[error("invalid TLV value width")]
    Width,
    /// A typed scalar has no value in the declared domain.
    #[error("invalid TLV scalar value")]
    Value,
}
#[repr(C)]
#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
struct Item {
    field: U16,
    reserved: U16,
    len: U32,
}
fn visit(
    mut input: &[u8],
    mut value: impl FnMut(u16, &[u8]) -> Result<(), EventDecodeError>,
) -> Result<(), EventDecodeError> {
    let mut seen = 0u32;
    while !input.is_empty() {
        let (item, tail) = Item::ref_from_prefix(input).map_err(|_| EventDecodeError::Length)?;
        if item.reserved.get() != 0 {
            return Err(EventDecodeError::Reserved);
        }
        let id = item.field.get();
        let index = id.checked_sub(1).ok_or(EventDecodeError::UnknownField)?;
        let bit = 1u32
            .checked_shl(u32::from(index))
            .ok_or(EventDecodeError::UnknownField)?;
        if seen & bit != 0 {
            return Err(EventDecodeError::DuplicateField);
        }
        seen |= bit;
        let len = usize::try_from(item.len.get()).map_err(|_| EventDecodeError::Length)?;
        let bytes = tail.get(..len).ok_or(EventDecodeError::Length)?;
        input = tail.get(len..).ok_or(EventDecodeError::Length)?;
        value(id, bytes)?;
    }
    Ok(())
}
fn u16_value(bytes: &[u8]) -> Result<u16, EventDecodeError> {
    U16::read_from_bytes(bytes)
        .map(|v| v.get())
        .map_err(|_| EventDecodeError::Width)
}
fn u32_value(bytes: &[u8]) -> Result<u32, EventDecodeError> {
    U32::read_from_bytes(bytes)
        .map(|v| v.get())
        .map_err(|_| EventDecodeError::Width)
}
fn u64_value(bytes: &[u8]) -> Result<u64, EventDecodeError> {
    U64::read_from_bytes(bytes)
        .map(|v| v.get())
        .map_err(|_| EventDecodeError::Width)
}
fn arguments(bytes: &[u8]) -> Result<Vec<Vec<u8>>, EventDecodeError> {
    let (count, mut tail) = U32::read_from_prefix(bytes).map_err(|_| EventDecodeError::Length)?;
    let count = usize::try_from(count.get()).map_err(|_| EventDecodeError::Length)?;
    if count > tail.len() / 4 {
        return Err(EventDecodeError::Length);
    }
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        let (len, remaining) = U32::read_from_prefix(tail).map_err(|_| EventDecodeError::Length)?;
        let len = usize::try_from(len.get()).map_err(|_| EventDecodeError::Length)?;
        args.push(
            remaining
                .get(..len)
                .ok_or(EventDecodeError::Length)?
                .to_vec(),
        );
        tail = remaining.get(len..).ok_or(EventDecodeError::Length)?;
    }
    if !tail.is_empty() {
        return Err(EventDecodeError::Length);
    }
    Ok(args)
}
fn ip(bytes: &[u8]) -> Result<IpAddr, EventDecodeError> {
    match bytes.len() {
        4 => Ok(IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(bytes).map_err(|_| EventDecodeError::Width)?,
        ))),
        16 => Ok(IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(bytes).map_err(|_| EventDecodeError::Width)?,
        ))),
        _ => Err(EventDecodeError::Width),
    }
}
const MAX_CGROUP_LINEAGE: usize = 32;
fn valid_cgroup_lineage(header: &EventHeader) -> bool {
    header.actor_cgroup_lineage.as_ref().is_none_or(|lineage| {
        !lineage.is_empty()
            && lineage.len() <= MAX_CGROUP_LINEAGE
            && !lineage.contains(&0)
            && lineage.last().copied() == header.actor_cgroup_id
    })
}

fn valid_namespace_tids(header: &EventHeader) -> bool {
    header.actor_namespace_tids.as_ref().is_none_or(|ids| {
        !ids.is_empty()
            && ids.len() <= 33
            && !ids.contains(&0)
            && ids.first().copied() == header.actor_tid
    })
}

fn namespace_ids(bytes: &[u8]) -> Result<Vec<u32>, EventDecodeError> {
    let (count, raw) = U32::read_from_prefix(bytes).map_err(|_| EventDecodeError::Length)?;
    let count = count.get() as usize;
    if count == 0 || count > 33 || raw.len() != count * 4 {
        return Err(EventDecodeError::Length);
    }
    let ids = raw
        .chunks_exact(4)
        .map(u32_value)
        .collect::<Result<Vec<_>, _>>()?;
    if ids.contains(&0) {
        return Err(EventDecodeError::Value);
    }
    Ok(ids)
}

struct CommonFacts {
    process: ProcessInfo,
    thread_birth: Option<u64>,
    lineage: Option<Vec<u64>>,
    syscall: Option<SyscallSnapshot>,
    kernel_stack: Option<StackSnapshot>,
    user_stack: Option<UserStackSnapshot>,
    namespace_tids: Option<Vec<u32>>,
}
fn process_info(bytes: &[u8]) -> Result<CommonFacts, EventDecodeError> {
    let mut process = ProcessInfo::default();
    let mut thread_birth = None;
    let mut lineage = None;
    let mut syscall = None;
    let mut kernel_stack = None;
    let mut user_stack = None;
    let mut namespace_tids = None;
    visit(bytes, |id, b| {
        match id {
            1 => process.name = Some(b.to_vec()),
            2 => process.executable = Some(b.to_vec()),
            3 => process.argv = Some(arguments(b)?),
            4 => process.cwd = Some(b.to_vec()),
            5 => process.uid = Some(u32_value(b)?),
            6 => process.euid = Some(u32_value(b)?),
            7 => process.gid = Some(u32_value(b)?),
            8 => process.egid = Some(u32_value(b)?),
            9 => process.parent_pid = Some(u32_value(b)?),
            10 => process.parent_birth_boottime_ns = Some(u64_value(b)?),
            11 => process.parent_name = Some(b.to_vec()),
            12 => process.parent_executable = Some(b.to_vec()),
            13 => process.parent_argv = Some(arguments(b)?),
            14 => {
                process.kind = Some(match u32_value(b)? {
                    0 => ProcessKind::User,
                    1 => ProcessKind::Kernel,
                    _ => return Err(EventDecodeError::Value),
                })
            }
            15 => process.exec_cookie = Some(u64_value(b)?),
            16 => process.pid_namespace_inode = Some(u64_value(b)?),
            17 => process.net_namespace_inode = Some(u64_value(b)?),
            18 => process.user_namespace_inode = Some(u64_value(b)?),
            19 => process.mount_namespace_inode = Some(u64_value(b)?),
            20 => {
                process.namespace_tgids = Some(namespace_ids(b)?);
            }
            21 => process.suid = Some(u32_value(b)?),
            22 => process.fsuid = Some(u32_value(b)?),
            23 => process.sgid = Some(u32_value(b)?),
            24 => process.fsgid = Some(u32_value(b)?),
            25 => thread_birth = Some(u64_value(b)?),
            26 => {
                if b.is_empty() || !b.len().is_multiple_of(8) || b.len() / 8 > MAX_CGROUP_LINEAGE {
                    return Err(EventDecodeError::Length);
                }
                lineage = Some(
                    b.chunks_exact(8)
                        .map(u64_value)
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            27 => syscall = Some(syscall_snapshot(b)?),
            28 => kernel_stack = Some(stack_snapshot(b)?),
            29 => namespace_tids = Some(namespace_ids(b)?),
            30 => user_stack = Some(user_stack_snapshot(b)?),
            _ => return Err(EventDecodeError::UnknownField),
        }
        Ok(())
    })?;
    Ok(CommonFacts {
        process,
        thread_birth,
        lineage,
        syscall,
        kernel_stack,
        user_stack,
        namespace_tids,
    })
}

fn user_stack_snapshot(bytes: &[u8]) -> Result<UserStackSnapshot, EventDecodeError> {
    if bytes.len() < 8 || !(bytes.len() - 8).is_multiple_of(8) {
        return Err(EventDecodeError::Width);
    }
    let flags = u32_value(&bytes[4..8])?;
    if flags > 1 {
        return Err(EventDecodeError::Value);
    }
    let value = UserStackSnapshot {
        result: I32::read_from_bytes(&bytes[..4])
            .map_err(|_| EventDecodeError::Width)?
            .get(),
        snapshot: StackSnapshot {
            instruction_pointers: bytes[8..]
                .chunks_exact(8)
                .map(u64_value)
                .collect::<Result<Vec<_>, _>>()?,
            buffer_full: flags == 1,
        },
    };
    if !valid_user_stack(&value) {
        return Err(EventDecodeError::Value);
    }
    Ok(value)
}

fn syscall_snapshot(bytes: &[u8]) -> Result<SyscallSnapshot, EventDecodeError> {
    if bytes.len() != 72 {
        return Err(EventDecodeError::Width);
    }
    Ok(SyscallSnapshot {
        audit_arch: u32_value(&bytes[..4])?,
        number: I32::read_from_bytes(&bytes[4..8])
            .map_err(|_| EventDecodeError::Width)?
            .get(),
        instruction_pointer: u64_value(&bytes[8..16])?,
        stack_pointer: u64_value(&bytes[16..24])?,
        arguments: bytes[24..]
            .chunks_exact(8)
            .map(u64_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn stack_snapshot(bytes: &[u8]) -> Result<StackSnapshot, EventDecodeError> {
    let header = bytes.get(..8).ok_or(EventDecodeError::Width)?;
    let flags = u32_value(&header[..4])?;
    let count = u32_value(&header[4..])? as usize;
    if count > KERNEL_STACK_LIMIT || flags > 1 || (flags == 1) != (count == KERNEL_STACK_LIMIT) {
        return Err(EventDecodeError::Value);
    }
    if bytes.len() != 8 + count * 8 {
        return Err(EventDecodeError::Width);
    }
    Ok(StackSnapshot {
        instruction_pointers: bytes[8..]
            .chunks_exact(8)
            .map(u64_value)
            .collect::<Result<Vec<_>, _>>()?,
        buffer_full: flags == 1,
    })
}
/// Decode only this current TLV format. All-None common and selected-domain fields are legal.
pub fn decode(record: &[u8]) -> Result<EventData, EventDecodeError> {
    let frame = EventFrame::decode(record).map_err(EventDecodeError::Frame)?;
    let raw = frame.header();
    if !valid_operation(frame.kind(), frame.stage(), raw.operation.get()) {
        return Err(EventDecodeError::Operation);
    }
    let mut header = EventHeader {
        kind: frame.kind(),
        stage: frame.stage(),
        operation: raw.operation.get(),
        source_instance: raw.source_instance,
        sequence_domain: raw.sequence_domain.get(),
        sequence: raw.sequence.get(),
        source_boottime_ns: frame.source_boottime_ns(),
        actor_tgid: frame.actor_tgid(),
        actor_tid: frame.actor_tid(),
        actor_birth_boottime_ns: frame.actor_birth_boottime_ns(),
        actor_thread_birth_boottime_ns: None,
        actor_cgroup_id: frame.actor_cgroup_id(),
        actor_cgroup_lineage: None,
        actor_namespace_tids: None,
    };
    let CommonFacts {
        process,
        thread_birth,
        lineage,
        syscall,
        kernel_stack,
        user_stack,
        namespace_tids,
    } = process_info(frame.common_process())?;
    header.actor_thread_birth_boottime_ns = thread_birth;
    header.actor_cgroup_lineage = lineage;
    header.actor_namespace_tids = namespace_tids;
    if !valid_cgroup_lineage(&header) || !valid_namespace_tids(&header) {
        return Err(EventDecodeError::Value);
    }
    let payload = match frame.kind() {
        EventKind::Process => {
            let mut data = ProcessData::default();
            let mut child_thread_birth = None;
            visit(frame.payload(), |id, b| {
                match id {
                    1 => data.target_executable = Some(b.to_vec()),
                    2 => data.target_argv = Some(arguments(b)?),
                    3 => data.child_pid = Some(u32_value(b)?),
                    4 => data.child_birth_boottime_ns = Some(u64_value(b)?),
                    5 => {
                        data.exit_code = Some(
                            I32::read_from_bytes(b)
                                .map_err(|_| EventDecodeError::Width)?
                                .get(),
                        )
                    }
                    6 => data.child_parent_pid = Some(u32_value(b)?),
                    7 => data.child_parent_birth_boottime_ns = Some(u64_value(b)?),
                    8 => data.child_cgroup_id = Some(u64_value(b)?),
                    9 => {
                        let CommonFacts {
                            process: child,
                            thread_birth: birth,
                            lineage,
                            syscall,
                            kernel_stack,
                            user_stack,
                            namespace_tids,
                        } = process_info(b)?;
                        if lineage.is_some()
                            || syscall.is_some()
                            || kernel_stack.is_some()
                            || user_stack.is_some()
                            || namespace_tids.is_some()
                            || child.parent_pid.is_some()
                            || child.parent_birth_boottime_ns.is_some()
                            || child.parent_name.is_some()
                            || child.parent_executable.is_some()
                            || child.parent_argv.is_some()
                        {
                            return Err(EventDecodeError::Value);
                        }
                        data.child_process = Some(Box::new(child));
                        child_thread_birth = birth;
                    }
                    _ => return Err(EventDecodeError::UnknownField),
                }
                Ok(())
            })?;
            if child_thread_birth.is_some() && child_thread_birth != data.child_birth_boottime_ns {
                return Err(EventDecodeError::Value);
            }
            EventPayload::Process(data)
        }
        EventKind::File => {
            let mut data = FileData::default();
            visit(frame.payload(), |id, b| {
                match id {
                    1 => data.path = Some(b.to_vec()),
                    2 => data.new_path = Some(b.to_vec()),
                    3 => {
                        let (major, minor) =
                            U32::read_from_prefix(b).map_err(|_| EventDecodeError::Width)?;
                        data.device = Some((major.get(), u32_value(minor)?));
                    }
                    4 => data.inode = Some(u64_value(b)?),
                    5 => data.open_flags = Some(u32_value(b)?),
                    6 => data.access_mask = Some(u32_value(b)?),
                    7 => data.rename_flags = Some(u32_value(b)?),
                    8 => data.creation_mode = Some(u32_value(b)?),
                    9 => {
                        if b.len() != 24 {
                            return Err(EventDecodeError::Width);
                        }
                        data.map_requested_prot = Some(u64_value(&b[..8])?);
                        data.map_effective_prot = Some(u64_value(&b[8..16])?);
                        data.map_flags = Some(u64_value(&b[16..])?);
                    }
                    10 => {
                        if b.len() != 12 {
                            return Err(EventDecodeError::Width);
                        }
                        data.truncate_size = Some(u64_value(&b[..8])?);
                        data.truncate_attributes = Some(u32_value(&b[8..])?);
                    }
                    11 => {
                        data.mount_effect = Some(match u32_value(b)? {
                            1 => MountEffect::IndependentView,
                            2 => MountEffect::ProtectedTree,
                            3 => MountEffect::Unknown,
                            _ => return Err(EventDecodeError::Value),
                        });
                    }
                    _ => return Err(EventDecodeError::UnknownField),
                }
                Ok(())
            })?;
            if !valid_file_parameters(header.operation, &data) {
                return Err(EventDecodeError::Value);
            }
            EventPayload::File(data)
        }
        EventKind::Network => {
            let mut data = NetworkData::default();
            visit(frame.payload(), |id, b| {
                match id {
                    1 => data.local_ip = Some(ip(b)?),
                    2 => data.remote_ip = Some(ip(b)?),
                    3 => data.local_port = Some(u16_value(b)?),
                    4 => data.remote_port = Some(u16_value(b)?),
                    5 => {
                        data.protocol = Some(
                            *b.first()
                                .filter(|_| b.len() == 1)
                                .ok_or(EventDecodeError::Width)?,
                        );
                    }
                    6 => data.target_domain = Some(b.to_vec()),
                    7 => data.socket_cookie = Some(u64_value(b)?),
                    8 => data.socket_net_namespace_inode = Some(u64_value(b)?),
                    9 => {
                        if b.len() < 16 {
                            return Err(EventDecodeError::Width);
                        }
                        let requested_bytes = u64_value(&b[..8])?;
                        let attempted_bytes = u32_value(&b[8..12])?;
                        if u64::from(attempted_bytes) > requested_bytes
                            || b.len() - 16 > attempted_bytes as usize
                            || header.operation
                                != crate::event_operation::NetworkOperation::Send as u32
                        {
                            return Err(EventDecodeError::Value);
                        }
                        data.dns_snapshot = Some(DnsSendSnapshot {
                            requested_bytes,
                            attempted_bytes,
                            send_flags: u32_value(&b[12..16])?,
                            data: b[16..].to_vec(),
                            analysis: None,
                        });
                    }
                    _ => return Err(EventDecodeError::UnknownField),
                }
                Ok(())
            })?;
            EventPayload::Network(data)
        }
    };
    Ok(EventData {
        header,
        host_boot_id: None,
        process,
        payload,
        syscall,
        kernel_stack,
        user_stack,
    })
}

fn serialize_kind<S: serde::Serializer>(
    kind: &EventKind,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match kind {
        EventKind::Process => "process",
        EventKind::File => "file",
        EventKind::Network => "network",
    })
}
fn serialize_stage<S: serde::Serializer>(
    stage: &EventStage,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match stage {
        EventStage::Observation => "observation",
        EventStage::ControlledRequest => "controlled_request",
        EventStage::ExecutionResult => "execution_result",
    })
}

fn deserialize_kind<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<EventKind, D::Error> {
    match <String as serde::Deserialize>::deserialize(deserializer)?.as_str() {
        "process" => Ok(EventKind::Process),
        "file" => Ok(EventKind::File),
        "network" => Ok(EventKind::Network),
        _ => Err(serde::de::Error::custom("invalid event kind")),
    }
}
fn deserialize_stage<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<EventStage, D::Error> {
    match <String as serde::Deserialize>::deserialize(deserializer)?.as_str() {
        "observation" => Ok(EventStage::Observation),
        "controlled_request" => Ok(EventStage::ControlledRequest),
        "execution_result" => Ok(EventStage::ExecutionResult),
        _ => Err(serde::de::Error::custom("invalid event stage")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use as_kernel_common::event_frame::{
        AVAILABLE_ACTOR_TGID, EVENT_FRAME_HEADER_LEN, EVENT_FRAME_VERSION, EventFrameHeader,
    };
    use zerocopy::IntoBytes;
    fn field(out: &mut Vec<u8>, id: u16, value: &[u8]) {
        out.extend(id.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend((value.len() as u32).to_le_bytes());
        out.extend(value);
    }

    fn callback_record(common: &[u8]) -> Vec<u8> {
        let header = EventFrameHeader {
            version: EVENT_FRAME_VERSION.into(),
            header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
            record_len: ((EVENT_FRAME_HEADER_LEN + common.len()) as u32).into(),
            kind: (EventKind::File as u16).into(),
            stage: (EventStage::ControlledRequest as u16).into(),
            operation: 1.into(),
            source_instance: [1; 16],
            sequence_domain: 1.into(),
            sequence: 1.into(),
            source_boottime_ns: 0.into(),
            actor_tgid: 0.into(),
            actor_tid: 0.into(),
            actor_birth_boottime_ns: 0.into(),
            actor_cgroup_id: 0.into(),
            available: 0.into(),
            common_process_len: (common.len() as u32).into(),
        };
        let mut record = header.as_bytes().to_vec();
        record.extend(common);
        record
    }

    #[test]
    fn dns_send_wire_keeps_a_partial_prefix_and_rejects_impossible_counts() {
        let mut record = callback_record(&[]);
        record[8..10].copy_from_slice(&3u16.to_le_bytes());
        record[12..16].copy_from_slice(&2u32.to_le_bytes());
        let mut snapshot = u64::MAX.to_le_bytes().to_vec();
        snapshot.extend(4u32.to_le_bytes());
        snapshot.extend(0x8000_0000u32.to_le_bytes());
        snapshot.extend([1, 2]);
        field(&mut record, 9, &snapshot);
        let size = record.len() as u32;
        record[4..8].copy_from_slice(&size.to_le_bytes());
        let event = decode(&record).expect("partial DNS bytes remain a valid source record");
        let EventPayload::Network(network) = &event.payload else {
            panic!("network")
        };
        let original = network.dns_snapshot.as_ref().expect("copied source");
        assert_eq!(original.requested_bytes, u64::MAX);
        assert_eq!(original.attempted_bytes, 4);
        assert_eq!(original.send_flags, 0x8000_0000);
        assert_eq!(original.data, [1, 2]);
        assert_eq!(original.analysis, None);
        let json = serde_json::to_value(&event).expect("same type");
        assert_eq!(
            json["payload"]["dns_snapshot"]["requested_bytes"],
            u64::MAX.to_string()
        );
        record[104..108].copy_from_slice(&1u32.to_le_bytes());
        assert!(
            decode(&record).is_err(),
            "copied count cannot exceed attempted"
        );
    }
    fn original_snapshots() -> Vec<u8> {
        let mut registers = 0u32.to_le_bytes().to_vec();
        registers.extend((-1i32).to_le_bytes());
        registers.extend(u64::MAX.to_le_bytes());
        registers.extend(0u64.to_le_bytes());
        for value in [0, u64::MAX, (1 << 53) + 1, 1 << 63, 17, 17] {
            registers.extend(value.to_le_bytes());
        }
        let mut stack = 0u32.to_le_bytes().to_vec();
        stack.extend(4u32.to_le_bytes());
        for value in [0, u64::MAX, 7, 7] {
            stack.extend(value.to_le_bytes());
        }
        let mut common = Vec::new();
        field(&mut common, 27, &registers);
        field(&mut common, 28, &stack);
        common
    }

    #[test]
    fn namespace_thread_ids_preserve_their_original_scope() {
        let mut common = Vec::new();
        let mut ids = 2u32.to_le_bytes().to_vec();
        ids.extend(123u32.to_le_bytes());
        ids.extend(5u32.to_le_bytes());
        field(&mut common, 29, &ids);
        let mut record = callback_record(&common);
        record[60..64].copy_from_slice(&123u32.to_le_bytes());
        record[80..84]
            .copy_from_slice(&as_kernel_common::event_frame::AVAILABLE_ACTOR_TID.to_le_bytes());
        let event = decode(&record).unwrap();
        assert_eq!(event.header().actor_namespace_tids, Some(vec![123, 5]));
        assert!(event.process.namespace_tgids.is_none());
        let mut json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            serde_json::from_value::<EventData>(json.clone()).unwrap(),
            event
        );
        for ids in [
            serde_json::json!([]),
            serde_json::json!([0, 5]),
            serde_json::json!([999, 5]),
            serde_json::json!(vec![123; 34]),
        ] {
            json["header"]["actor_namespace_tids"] = ids;
            assert!(serde_json::from_value::<EventData>(json.clone()).is_err());
        }
        let mut changed = event.clone();
        changed.header.actor_namespace_tids = None;
        assert!(!event.accepts_enrichment(&changed));
        assert!(!changed.accepts_enrichment(&event));
        record[60..64].copy_from_slice(&999u32.to_le_bytes());
        assert!(decode(&record).is_err());
    }

    #[test]
    fn user_stack_keeps_helper_errors_empty_success_and_original_ips() {
        for (result, ips) in [
            (-14i32, vec![]),
            (0, vec![]),
            (24, vec![0, u64::MAX, 0]),
            (256, vec![17; 32]),
        ] {
            let mut bytes = result.to_le_bytes().to_vec();
            bytes.extend(u32::from(ips.len() == KERNEL_STACK_LIMIT).to_le_bytes());
            for ip in &ips {
                bytes.extend(ip.to_le_bytes());
            }
            let mut common = Vec::new();
            field(&mut common, 30, &bytes);
            let event = decode(&callback_record(&common)).unwrap();
            let expected = UserStackSnapshot {
                result,
                snapshot: StackSnapshot {
                    buffer_full: ips.len() == KERNEL_STACK_LIMIT,
                    instruction_pointers: ips,
                },
            };
            assert_eq!(event.user_stack, Some(expected));
            assert!(event.kernel_stack.is_none());
            let mut json = serde_json::to_value(&event).unwrap();
            assert_eq!(
                serde_json::from_value::<EventData>(json.clone()).unwrap(),
                event
            );
            json["user_stack"]["result"] = serde_json::json!(-4096);
            assert!(serde_json::from_value::<EventData>(json).is_err());
            let mut changed = event.clone();
            changed.user_stack = None;
            assert!(!event.accepts_enrichment(&changed));
            assert!(!changed.accepts_enrichment(&event));
        }
        for (result, ips) in [
            (-14i32, vec![1u64]),
            (1, vec![]),
            (16, vec![1]),
            (264, vec![1; 33]),
        ] {
            let mut bytes = result.to_le_bytes().to_vec();
            bytes.extend(0u32.to_le_bytes());
            for ip in ips {
                bytes.extend(ip.to_le_bytes());
            }
            let mut common = Vec::new();
            field(&mut common, 30, &bytes);
            assert!(decode(&callback_record(&common)).is_err());
        }
    }

    #[test]
    fn hook_snapshots_preserve_original_bits_order_empty_and_decimal_json() {
        let original = decode(&callback_record(&original_snapshots())).unwrap();
        let syscall = original.syscall.as_ref().unwrap();
        assert_eq!(syscall.audit_arch, 0);
        assert_eq!(syscall.number, -1);
        assert_eq!(syscall.instruction_pointer, u64::MAX);
        assert_eq!(syscall.stack_pointer, 0);
        assert_eq!(
            syscall.arguments,
            [0, u64::MAX, (1 << 53) + 1, 1 << 63, 17, 17]
        );
        assert_eq!(
            original.kernel_stack.as_ref().unwrap().instruction_pointers,
            [0, u64::MAX, 7, 7]
        );
        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["syscall"]["instruction_pointer"], u64::MAX.to_string());
        assert_eq!(json["syscall"]["arguments"][2], "9007199254740993");
        assert_eq!(
            json["kernel_stack"]["instruction_pointers"],
            serde_json::json!(["0", u64::MAX.to_string(), "7", "7"])
        );
        assert_eq!(serde_json::from_value::<EventData>(json).unwrap(), original);

        let absent = decode(&callback_record(&[])).unwrap();
        assert!(absent.syscall.is_none() && absent.kernel_stack.is_none());
        let mut common = Vec::new();
        field(&mut common, 28, &[0; 8]);
        let empty = decode(&callback_record(&common)).unwrap();
        assert_eq!(
            empty.kernel_stack,
            Some(StackSnapshot {
                instruction_pointers: vec![],
                buffer_full: false
            })
        );
        assert_ne!(empty, absent);

        let mut full = 1u32.to_le_bytes().to_vec();
        full.extend((KERNEL_STACK_LIMIT as u32).to_le_bytes());
        full.extend([0; KERNEL_STACK_LIMIT * 8]);
        common.clear();
        field(&mut common, 28, &full);
        let full = decode(&callback_record(&common)).unwrap();
        assert!(full.kernel_stack.as_ref().unwrap().buffer_full);
        assert_eq!(
            full.kernel_stack.as_ref().unwrap().instruction_pointers,
            vec![0; KERNEL_STACK_LIMIT]
        );
    }

    #[test]
    fn hook_snapshots_reject_malformed_wire_json_and_child_placement() {
        for size in [0, 71, 73] {
            let mut common = Vec::new();
            field(&mut common, 27, &vec![0; size]);
            assert!(decode(&callback_record(&common)).is_err());
        }
        for (flags, count, actual) in [
            (2u32, 0u32, 0),
            (1, 0, 0),
            (0, 32, 32),
            (1, 33, 33),
            (0, 2, 1),
            (0, 0, 1),
        ] {
            let mut stack = flags.to_le_bytes().to_vec();
            stack.extend(count.to_le_bytes());
            stack.extend(vec![0; actual * 8]);
            let mut common = Vec::new();
            field(&mut common, 28, &stack);
            assert!(decode(&callback_record(&common)).is_err());
        }
        let mut duplicated = original_snapshots();
        field(&mut duplicated, 28, &[0; 8]);
        assert!(matches!(
            decode(&callback_record(&duplicated)),
            Err(EventDecodeError::DuplicateField)
        ));

        let original = decode(&callback_record(&original_snapshots())).unwrap();
        let json = serde_json::to_value(&original).unwrap();
        for invalid in [
            serde_json::json!(7),
            serde_json::json!("01"),
            serde_json::json!("-1"),
            serde_json::json!("18446744073709551616"),
        ] {
            let mut changed = json.clone();
            changed["syscall"]["arguments"][0] = invalid;
            assert!(serde_json::from_value::<EventData>(changed).is_err());
        }
        for count in [0, 5, 7] {
            let mut changed = json.clone();
            changed["syscall"]["arguments"] = serde_json::json!(vec!["0"; count]);
            assert!(serde_json::from_value::<EventData>(changed).is_err());
        }
        for (count, full) in [(0, true), (32, false), (33, true)] {
            let mut changed = json.clone();
            changed["kernel_stack"]["instruction_pointers"] = serde_json::json!(vec!["0"; count]);
            changed["kernel_stack"]["buffer_full"] = full.into();
            assert!(serde_json::from_value::<EventData>(changed).is_err());
        }

        let mut child = Vec::new();
        field(&mut child, 9, &original_snapshots());
        let mut record = callback_record(&[]);
        record[8..10].copy_from_slice(&(EventKind::Process as u16).to_le_bytes());
        record[10..12].copy_from_slice(&(EventStage::Observation as u16).to_le_bytes());
        record[12..16].copy_from_slice(
            &(crate::event_operation::ProcessOperation::Create as u32).to_le_bytes(),
        );
        record.extend(child);
        let len = record.len() as u32;
        record[4..8].copy_from_slice(&len.to_le_bytes());
        assert!(matches!(decode(&record), Err(EventDecodeError::Value)));
    }

    #[test]
    fn enrichment_cannot_fill_erase_or_change_hook_snapshots() {
        let original = decode(&callback_record(&original_snapshots())).unwrap();
        let absent = decode(&callback_record(&[])).unwrap();
        assert!(original.accepts_enrichment(&original));
        assert!(!absent.accepts_enrichment(&original));
        assert!(!original.accepts_enrichment(&absent));
        let mut enriched = original.clone();
        enriched.process.cwd = Some(b"/original-cwd".to_vec());
        assert!(original.accepts_enrichment(&enriched));
        enriched.syscall.as_mut().unwrap().arguments[0] = 1;
        assert!(!original.accepts_enrichment(&enriched));
        enriched = original.clone();
        enriched
            .kernel_stack
            .as_mut()
            .unwrap()
            .instruction_pointers
            .swap(0, 1);
        assert!(!original.accepts_enrichment(&enriched));
        enriched = original.clone();
        enriched.kernel_stack.as_mut().unwrap().buffer_full = true;
        assert!(!enriched.accepts_enrichment(&enriched));
    }

    #[test]
    fn snapshot_memory_charge_counts_owned_capacity_once() {
        let mut event = decode(&callback_record(&[])).unwrap();
        let before = event.retained_bytes();
        let mut arguments = Vec::with_capacity(20);
        arguments.extend([0; SYSCALL_ARGUMENT_COUNT]);
        let mut ips = Vec::with_capacity(40);
        ips.extend([0, 3, 3]);
        let expected = (arguments.capacity() + ips.capacity()) * size_of::<u64>();
        event.syscall = Some(SyscallSnapshot {
            audit_arch: 0,
            number: -1,
            instruction_pointer: 0,
            stack_pointer: 0,
            arguments,
        });
        event.kernel_stack = Some(StackSnapshot {
            instruction_pointers: ips,
            buffer_full: false,
        });
        assert_eq!(event.retained_bytes() - before, expected);
        let before = event.retained_bytes();
        let mut user_ips = Vec::with_capacity(12);
        user_ips.extend([0, u64::MAX]);
        let mut tids = Vec::with_capacity(5);
        tids.extend([123, 5]);
        let mut lineage = Vec::with_capacity(7);
        lineage.extend([1, 2]);
        let expected = (user_ips.capacity() + lineage.capacity()) * size_of::<u64>()
            + tids.capacity() * size_of::<u32>();
        event.user_stack = Some(UserStackSnapshot {
            result: 16,
            snapshot: StackSnapshot {
                instruction_pointers: user_ips,
                buffer_full: false,
            },
        });
        event.header.actor_namespace_tids = Some(tids);
        event.header.actor_cgroup_lineage = Some(lineage);
        assert_eq!(event.retained_bytes() - before, expected);
    }

    #[test]
    fn file_parameters_preserve_zero_and_reject_wrong_operations_widths_or_mode_domain() {
        let record = |operation: u32, id: u16, value: &[u8]| {
            let mut payload = Vec::new();
            if id != 0 {
                field(&mut payload, id, value);
            }
            let mut bytes = vec![0; EVENT_FRAME_HEADER_LEN];
            let length = (bytes.len() + payload.len()) as u32;
            bytes[..2].copy_from_slice(&EVENT_FRAME_VERSION.to_le_bytes());
            bytes[2..4].copy_from_slice(&(EVENT_FRAME_HEADER_LEN as u16).to_le_bytes());
            bytes[4..8].copy_from_slice(&length.to_le_bytes());
            bytes[8..10].copy_from_slice(&(EventKind::File as u16).to_le_bytes());
            bytes[10..12].copy_from_slice(&(EventStage::ControlledRequest as u16).to_le_bytes());
            bytes[12..16].copy_from_slice(&operation.to_le_bytes());
            bytes[16..32].copy_from_slice(&[1; 16]);
            bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
            bytes[40..48].copy_from_slice(&1u64.to_le_bytes());
            bytes.extend(payload);
            bytes
        };
        for (operation, tag, value) in [(9, 7, 0), (9, 7, u32::MAX), (5, 8, 0), (5, 8, 65535)] {
            let original =
                decode(&record(operation, tag, &value.to_le_bytes())).expect("original parameter");
            let EventPayload::File(data) = &original.payload else {
                panic!("file")
            };
            assert_eq!(
                if tag == 7 {
                    data.rename_flags
                } else {
                    data.creation_mode
                },
                Some(value)
            );
            let json = serde_json::to_value(&original).expect("original JSON");
            assert_eq!(
                serde_json::from_value::<EventData>(json.clone()).expect("same current value"),
                original
            );
            let absent = decode(&record(operation, 0, &[])).expect("parameter not acquired");
            assert!(absent.accepts_enrichment(&original));
            assert!(
                !original.accepts_enrichment(&absent),
                "captured zero cannot be erased"
            );
            let mut wrong = json;
            wrong["header"]["operation"] = 1.into();
            assert!(serde_json::from_value::<EventData>(wrong).is_err());
        }
        assert!(matches!(
            decode(&record(9, 7, &[0, 0])),
            Err(EventDecodeError::Width)
        ));
        for (operation, tag, value) in [(1, 7, 0u32), (9, 8, 0), (5, 8, 65536)] {
            assert!(matches!(
                decode(&record(operation, tag, &value.to_le_bytes())),
                Err(EventDecodeError::Value)
            ));
        }
        let mut impossible = decode(&record(5, 8, &0u32.to_le_bytes())).expect("original mkdir");
        if let EventPayload::File(data) = &mut impossible.payload {
            data.creation_mode = Some(65536);
        }
        assert!(!impossible.accepts_enrichment(&impossible));
        assert!(
            serde_json::from_value::<EventData>(serde_json::to_value(impossible).expect("JSON"))
                .is_err()
        );
        let mut captured = decode(&record(9, 7, &0u32.to_le_bytes())).expect("original zero");
        let original = captured.clone();
        if let EventPayload::File(data) = &mut captured.payload {
            data.rename_flags = Some(1);
        }
        assert!(
            !original.accepts_enrichment(&captured),
            "captured zero cannot be replaced"
        );
        assert!(
            decode(&record(8, 2, b"/old-self-link-target")).is_ok(),
            "existing self-only double-path facts remain legal"
        );

        let mut mapping = Vec::new();
        for value in [3u64, 3, 1] {
            mapping.extend(value.to_le_bytes());
        }
        let EventPayload::File(mapped) = decode(&record(19, 9, &mapping))
            .expect("mmap parameters")
            .payload
        else {
            panic!("file")
        };
        assert_eq!(
            (
                mapped.map_requested_prot,
                mapped.map_effective_prot,
                mapped.map_flags
            ),
            (Some(3), Some(3), Some(1))
        );
        let mut truncate = Vec::from(0u64.to_le_bytes());
        truncate.extend(0x2008u32.to_le_bytes());
        let EventPayload::File(truncated) = decode(&record(10, 10, &truncate))
            .expect("truncate parameters")
            .payload
        else {
            panic!("file")
        };
        assert_eq!(
            (truncated.truncate_size, truncated.truncate_attributes),
            (Some(0), Some(0x2008))
        );
        for (operation, tag, valid) in [(19, 9, mapping), (10, 10, truncate)] {
            assert!(matches!(
                decode(&record(operation, tag, &valid[..valid.len() - 1])),
                Err(EventDecodeError::Width)
            ));
            assert!(matches!(
                decode(&record(1, tag, &valid)),
                Err(EventDecodeError::Value)
            ));
        }
    }
    #[test]
    fn process_metadata_keeps_zero_values_and_namespace_identity_without_aliases() {
        let mut common = Vec::new();
        field(&mut common, 14, &0u32.to_le_bytes());
        field(&mut common, 15, &0u64.to_le_bytes());
        for id in 16..=19 {
            field(&mut common, id, &(u64::MAX - u64::from(id)).to_le_bytes());
        }
        let ids = [2u32, 921, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        field(&mut common, 20, &ids);
        for id in 21..=24 {
            field(&mut common, id, &0u32.to_le_bytes());
        }
        let CommonFacts {
            process: actual,
            thread_birth,
            ..
        } = process_info(&common).expect("current metadata");
        assert!(thread_birth.is_none());
        assert_eq!(actual.kind, Some(ProcessKind::User));
        assert_eq!(actual.exec_cookie, Some(0));
        assert_eq!(actual.pid_namespace_inode, Some(u64::MAX - 16));
        assert_eq!(actual.mount_namespace_inode, Some(u64::MAX - 19));
        assert_eq!(actual.namespace_tgids, Some(vec![921, 1]));
        assert_eq!(
            (actual.suid, actual.fsuid, actual.sgid, actual.fsgid),
            (Some(0), Some(0), Some(0), Some(0))
        );
        field(&mut common, 20, &ids);
        assert!(matches!(
            process_info(&common),
            Err(EventDecodeError::DuplicateField)
        ));
        let mut invalid = Vec::new();
        field(&mut invalid, 14, &2u32.to_le_bytes());
        assert!(matches!(
            process_info(&invalid),
            Err(EventDecodeError::Value)
        ));
    }
    #[test]
    fn common_and_target_fields_keep_empty_values_and_reject_truncated_tlv() {
        let mut common = Vec::new();
        field(&mut common, 1, b"");
        field(&mut common, 9, &7u32.to_le_bytes());
        field(&mut common, 11, b"parent");
        let mut body = Vec::new();
        field(&mut body, 1, b"\xff/target");
        let mut argv = Vec::new();
        argv.extend(2u32.to_le_bytes());
        argv.extend(0u32.to_le_bytes());
        argv.extend(1u32.to_le_bytes());
        argv.push(b'x');
        field(&mut body, 2, &argv);
        let header = EventFrameHeader {
            version: EVENT_FRAME_VERSION.into(),
            header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
            record_len: ((EVENT_FRAME_HEADER_LEN + common.len() + body.len()) as u32).into(),
            kind: (EventKind::Process as u16).into(),
            stage: (EventStage::Observation as u16).into(),
            operation: 1.into(),
            source_instance: [1; 16],
            sequence_domain: 1.into(),
            sequence: 1.into(),
            source_boottime_ns: 0.into(),
            actor_tgid: 42.into(),
            actor_tid: 0.into(),
            actor_birth_boottime_ns: 0.into(),
            actor_cgroup_id: 0.into(),
            available: AVAILABLE_ACTOR_TGID.into(),
            common_process_len: (common.len() as u32).into(),
        };
        let mut record = header.as_bytes().to_vec();
        record.extend(common);
        record.extend(body);
        let mut event = decode(&record).unwrap();
        assert_eq!(event.header().actor_tgid, Some(42));
        assert_eq!(event.header().actor_tid, None);
        assert_eq!(event.kind(), EventKind::Process);
        assert_eq!(event.stage(), EventStage::Observation);
        assert_eq!(event.process.name, Some(Vec::new()));
        assert_eq!(event.process.parent_pid, Some(7));
        let EventPayload::Process(target) = &event.payload else {
            panic!("process data");
        };
        assert_eq!(target.target_executable, Some(b"\xff/target".to_vec()));
        assert_eq!(target.target_argv, Some(vec![Vec::new(), b"x".to_vec()]));
        event.process.fill_missing(ProcessInfo {
            name: Some(b"later".to_vec()),
            uid: Some(1000),
            ..Default::default()
        });
        assert_eq!(event.process.name, Some(Vec::new()));
        assert_eq!(event.process.uid, Some(1000));
        record.pop();
        let len = record.len() as u32;
        record[4..8].copy_from_slice(&len.to_le_bytes());
        assert!(decode(&record).is_err());
        let mut empty = header;
        empty.record_len = (EVENT_FRAME_HEADER_LEN as u32).into();
        empty.common_process_len = 0.into();
        assert_eq!(
            decode(empty.as_bytes()).unwrap().process,
            ProcessInfo::default()
        );
    }
}
