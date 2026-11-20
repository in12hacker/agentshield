//! Current unified source frame: one shared actor prefix, common process bytes, then domain bytes.
//!
//! All integers are little-endian. A record is exactly one frame, without padding or a second frame.
//! This header is an observation contract, never an FD, lease or authority to control an operation.
//! Domain producers must define their new common-process/domain encodings; legacy event bodies are
//! not a supported encoding. Names, executable paths, argv and original physical-parent facts belong
//! once in the common-process area. They never substitute for the actor or an execution target.

use core::mem::{align_of, offset_of, size_of};
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
    little_endian::{U16, U32, U64},
};

/// The only format accepted by this module; no legacy-frame decoding or version fallback.
pub const EVENT_FRAME_VERSION: u16 = 1;
/// Exact fixed prefix length. Neither process strings nor domain unions are allocated here.
pub const EVENT_FRAME_HEADER_LEN: usize = 88;
/// The source BOOTTIME sample was supplied.
pub const AVAILABLE_SOURCE_TIME: u32 = 1;
/// The original initiating task's host TGID was supplied.
pub const AVAILABLE_ACTOR_TGID: u32 = 2;
/// The original initiating task's host TID was supplied.
pub const AVAILABLE_ACTOR_TID: u32 = 4;
/// The original initiating group's kernel BOOTTIME birth was supplied.
pub const AVAILABLE_ACTOR_BIRTH: u32 = 8;
/// The original initiating task's physical cgroup ID was supplied.
pub const AVAILABLE_ACTOR_CGROUP: u32 = 16;
const AVAILABLE_FIELDS: u32 = AVAILABLE_SOURCE_TIME
    | AVAILABLE_ACTOR_TGID
    | AVAILABLE_ACTOR_TID
    | AVAILABLE_ACTOR_BIRTH
    | AVAILABLE_ACTOR_CGROUP;

/// Disjoint operation families. Shared process information is not repeated in their domain bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EventKind {
    /// Process operations; an execution image is a domain target, not the initiating actor.
    Process = 1,
    /// File operations; object and access facts belong in the file domain bytes.
    File = 2,
    /// Network operations; endpoint and protocol facts belong in the network domain bytes.
    Network = 3,
}
impl TryFrom<u16> for EventKind {
    type Error = FrameDecodeError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Process),
            2 => Ok(Self::File),
            3 => Ok(Self::Network),
            _ => Err(FrameDecodeError::Kind),
        }
    }
}
/// Source-described boundary; the tag does not establish successful or durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EventStage {
    /// Facts observed by the source, without a retained-operation claim.
    Observation = 1,
    /// The source describes a retained request; its actual control resource remains out of band.
    ControlledRequest = 2,
    /// A result at the named operation's original boundary; its domain defines the actual result.
    ExecutionResult = 3,
}
impl TryFrom<u16> for EventStage {
    type Error = FrameDecodeError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Observation),
            2 => Ok(Self::ControlledRequest),
            3 => Ok(Self::ExecutionResult),
            _ => Err(FrameDecodeError::Stage),
        }
    }
}
/// Raw fixed ABI prefix. Use [`EventFrame::decode`] before consuming availability or body ranges.
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct EventFrameHeader {
    /// Exactly EVENT_FRAME_VERSION.
    pub version: U16,
    /// Exactly EVENT_FRAME_HEADER_LEN, measured in bytes.
    pub header_len: U16,
    /// Entire record length in bytes, including this header and both following areas.
    pub record_len: U32,
    /// EventKind's current numeric tag.
    pub kind: U16,
    /// EventStage's current numeric tag.
    pub stage: U16,
    /// Nonzero operation code in this kind's current domain format, not a legacy event number.
    pub operation: U32,
    /// Nonzero opaque ID assigned to one real producer activation before emission.
    /// It changes on restart, reattachment or sequence reset and is never inferred from PID/time.
    /// This is not a boot UUID or a process identity.
    pub source_instance: [u8; 16],
    /// Nonzero producer-assigned stream ID, scoped by source_instance; not a task or causal key.
    pub sequence_domain: U64,
    /// Starts at 1 and never wraps within that source/domain. Assigned before publication attempts;
    /// gaps can include lost frames. Neither sequence nor arrival order proves causality.
    pub sequence: U64,
    /// Source sample boundary in nanoseconds of kernel CLOCK_BOOTTIME, not UTC or ingestion time.
    pub source_boottime_ns: U64,
    /// Original initiating group's host ID, never a parent, target or flushing worker's ID.
    pub actor_tgid: U32,
    /// Original initiating task's host ID; never filled by copying actor_tgid.
    pub actor_tid: U32,
    /// Original group-leader birth in kernel BOOTTIME nanoseconds; not proc ticks or UTC.
    /// Birth/IDs remain source-scoped samples until the original source supplies their boot identity.
    pub actor_birth_boottime_ns: U64,
    /// Original initiating task's physical cgroup ID, not an inferred container name or policy scope.
    pub actor_cgroup_id: U64,
    /// AVAILABLE_* bits only. A missing value has its bit clear and numeric slot zero.
    /// A zero slot alone never establishes availability, absence of a task, or native identity.
    pub available: U32,
    /// Bytes immediately following the fixed header occupied by the shared process area.
    /// Zero means that no further process fields were supplied; all remaining bytes are domain data.
    pub common_process_len: U32,
}
/// Structural rejection only; this is not a process-gap or policy decision state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDecodeError {
    /// The fixed prefix is incomplete.
    Header,
    /// The source supplied a different format version.
    Version,
    /// A declared header, record or common-area length disagrees with the exact input.
    Length,
    /// The current kind tag is unknown.
    Kind,
    /// The current stage tag is unknown.
    Stage,
    /// No current domain operation was supplied.
    Operation,
    /// No producer activation was supplied.
    Source,
    /// No sequence domain or sequence was supplied.
    Sequence,
    /// Unknown availability bits or nonzero unavailable slots were supplied.
    Availability,
}
/// Borrowed, structurally validated frame. Common/domain payload formats remain their producers' job.
pub struct EventFrame<'a> {
    header: &'a EventFrameHeader,
    kind: EventKind,
    stage: EventStage,
    common_process: &'a [u8],
    payload: &'a [u8],
}
impl<'a> EventFrame<'a> {
    /// Decode exactly one current record without allocation, copying, alignment assumptions or unsafe.
    /// No arbitrary payload ceiling is added to the protocol's u32 byte length.
    pub fn decode(record: &'a [u8]) -> Result<Self, FrameDecodeError> {
        let (header, body) =
            EventFrameHeader::ref_from_prefix(record).map_err(|_| FrameDecodeError::Header)?;
        if header.version.get() != EVENT_FRAME_VERSION {
            return Err(FrameDecodeError::Version);
        }
        if usize::from(header.header_len.get()) != EVENT_FRAME_HEADER_LEN
            || usize::try_from(header.record_len.get()).ok() != Some(record.len())
        {
            return Err(FrameDecodeError::Length);
        }
        let kind = EventKind::try_from(header.kind.get())?;
        let stage = EventStage::try_from(header.stage.get())?;
        if header.operation.get() == 0 {
            return Err(FrameDecodeError::Operation);
        }
        if header.source_instance == [0; 16] {
            return Err(FrameDecodeError::Source);
        }
        if header.sequence_domain.get() == 0 || header.sequence.get() == 0 {
            return Err(FrameDecodeError::Sequence);
        }
        let available = header.available.get();
        if available & !AVAILABLE_FIELDS != 0
            || (available & AVAILABLE_SOURCE_TIME == 0 && header.source_boottime_ns.get() != 0)
            || (available & AVAILABLE_ACTOR_TGID == 0 && header.actor_tgid.get() != 0)
            || (available & AVAILABLE_ACTOR_TID == 0 && header.actor_tid.get() != 0)
            || (available & AVAILABLE_ACTOR_BIRTH == 0 && header.actor_birth_boottime_ns.get() != 0)
            || (available & AVAILABLE_ACTOR_CGROUP == 0 && header.actor_cgroup_id.get() != 0)
        {
            return Err(FrameDecodeError::Availability);
        }
        let process_len = usize::try_from(header.common_process_len.get())
            .map_err(|_| FrameDecodeError::Length)?;
        let common_process = body.get(..process_len).ok_or(FrameDecodeError::Length)?;
        let payload = body.get(process_len..).ok_or(FrameDecodeError::Length)?;
        Ok(Self {
            header,
            kind,
            stage,
            common_process,
            payload,
        })
    }
    /// The original shared prefix; domain payloads must not duplicate its actor fields.
    pub fn header(&self) -> &'a EventFrameHeader {
        self.header
    }
    /// Validated current operation family.
    pub fn kind(&self) -> EventKind {
        self.kind
    }
    /// Validated source-described stage, not an execution or custody acknowledgement.
    pub fn stage(&self) -> EventStage {
        self.stage
    }
    /// Shared actor names/path/argv and physical-parent fields, once in their current encoding.
    pub fn common_process(&self) -> &'a [u8] {
        self.common_process
    }
    /// Only the selected kind's current target/object/endpoint-specific bytes.
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
    /// Source BOOTTIME sample, if actually supplied.
    pub fn source_boottime_ns(&self) -> Option<u64> {
        self.value(AVAILABLE_SOURCE_TIME, self.header.source_boottime_ns.get())
    }
    /// Original actor TGID, if actually supplied; availability is never inferred from numeric value.
    pub fn actor_tgid(&self) -> Option<u32> {
        (self.header.available.get() & AVAILABLE_ACTOR_TGID != 0)
            .then_some(self.header.actor_tgid.get())
    }
    /// Original actor TID, if actually supplied.
    pub fn actor_tid(&self) -> Option<u32> {
        (self.header.available.get() & AVAILABLE_ACTOR_TID != 0)
            .then_some(self.header.actor_tid.get())
    }
    /// Original group birth sample, without manufacturing a cross-source native identity.
    pub fn actor_birth_boottime_ns(&self) -> Option<u64> {
        self.value(
            AVAILABLE_ACTOR_BIRTH,
            self.header.actor_birth_boottime_ns.get(),
        )
    }
    /// Original physical cgroup sample, if actually supplied.
    pub fn actor_cgroup_id(&self) -> Option<u64> {
        self.value(AVAILABLE_ACTOR_CGROUP, self.header.actor_cgroup_id.get())
    }
    fn value(&self, bit: u32, value: u64) -> Option<u64> {
        (self.header.available.get() & bit != 0).then_some(value)
    }
}
const _: () = {
    assert!(size_of::<EventFrameHeader>() == EVENT_FRAME_HEADER_LEN);
    assert!(align_of::<EventFrameHeader>() == 1);
    assert!(offset_of!(EventFrameHeader, version) == 0);
    assert!(offset_of!(EventFrameHeader, header_len) == 2);
    assert!(offset_of!(EventFrameHeader, record_len) == 4);
    assert!(offset_of!(EventFrameHeader, kind) == 8);
    assert!(offset_of!(EventFrameHeader, stage) == 10);
    assert!(offset_of!(EventFrameHeader, operation) == 12);
    assert!(offset_of!(EventFrameHeader, source_instance) == 16);
    assert!(offset_of!(EventFrameHeader, sequence_domain) == 32);
    assert!(offset_of!(EventFrameHeader, sequence) == 40);
    assert!(offset_of!(EventFrameHeader, source_boottime_ns) == 48);
    assert!(offset_of!(EventFrameHeader, actor_tgid) == 56);
    assert!(offset_of!(EventFrameHeader, actor_tid) == 60);
    assert!(offset_of!(EventFrameHeader, actor_birth_boottime_ns) == 64);
    assert!(offset_of!(EventFrameHeader, actor_cgroup_id) == 72);
    assert!(offset_of!(EventFrameHeader, available) == 80);
    assert!(offset_of!(EventFrameHeader, common_process_len) == 84);
};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_frame_borrows_both_areas_and_rejects_truncated_records() {
        let header = EventFrameHeader {
            version: EVENT_FRAME_VERSION.into(),
            header_len: (EVENT_FRAME_HEADER_LEN as u16).into(),
            record_len: ((EVENT_FRAME_HEADER_LEN + 5) as u32).into(),
            kind: (EventKind::File as u16).into(),
            stage: (EventStage::Observation as u16).into(),
            operation: 1.into(),
            source_instance: [1; 16],
            sequence_domain: 1.into(),
            sequence: 1.into(),
            source_boottime_ns: 100.into(),
            actor_tgid: 42.into(),
            actor_tid: 0.into(),
            actor_birth_boottime_ns: 0.into(),
            actor_cgroup_id: 0.into(),
            available: (AVAILABLE_SOURCE_TIME | AVAILABLE_ACTOR_TGID).into(),
            common_process_len: 2.into(),
        };
        let mut record = [0; EVENT_FRAME_HEADER_LEN + 5];
        record[..EVENT_FRAME_HEADER_LEN].copy_from_slice(header.as_bytes());
        record[EVENT_FRAME_HEADER_LEN..].copy_from_slice(b"pcobj");
        let frame = EventFrame::decode(&record).unwrap();
        assert_eq!(frame.kind(), EventKind::File);
        assert_eq!(frame.stage(), EventStage::Observation);
        assert_eq!(frame.actor_tgid(), Some(42));
        assert_eq!(frame.actor_tid(), None);
        assert_eq!(frame.common_process(), b"pc");
        assert_eq!(frame.payload(), b"obj");
        assert_eq!(
            frame.payload().as_ptr(),
            record[EVENT_FRAME_HEADER_LEN + 2..].as_ptr()
        );
        for length in [
            0,
            1,
            EVENT_FRAME_HEADER_LEN - 1,
            EVENT_FRAME_HEADER_LEN,
            record.len() - 1,
        ] {
            assert!(EventFrame::decode(&record[..length]).is_err());
        }
    }
}
