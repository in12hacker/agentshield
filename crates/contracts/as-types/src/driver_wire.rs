//! Current driver frame codec; no I/O, reassembly state or business owner.
use crate::{driver_control::DRIVER_REVISION, main_ports::ModuleError};
/// Fixed current envelope length.
pub const HEADER_BYTES: usize = 64;
/// Current header class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameClass {
    /// Downward management request.
    Request = 1,
    /// Correlated command result.
    Reply = 2,
    /// Unsolicited native notification.
    Event = 3,
}
/// One exact message fragment, independent of any native C struct layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverFrame {
    /// Request/reply/event direction.
    pub class: FrameClass,
    /// Exact current command/event identifier.
    pub message_type: u16,
    /// Actual authenticated driver session epoch.
    pub session: [u8; 16],
    /// Whole management command correlation; unsolicited events use 0.
    pub request_id: u64,
    /// Whole body size, not the fragment size.
    pub total_bytes: u64,
    /// Fragment's checked position.
    pub offset: u64,
    /// Only this fragment's actual copied bytes.
    pub body: Vec<u8>,
}
fn array<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N], ModuleError> {
    b.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(ModuleError::InvalidInput)
}
impl DriverFrame {
    /// Decode one actual observation callback; it never grants an OriginalOperation capability.
    pub fn observation(
        &self,
        expected_session: [u8; 16],
    ) -> Result<crate::dispatch_event::EventData, ModuleError> {
        self.validate()?;
        if expected_session == [0; 16]
            || self.session != expected_session
            || self.class != FrameClass::Event
            || self.message_type != 0x104
        {
            return Err(ModuleError::InvalidInput);
        }
        let event =
            crate::dispatch_event::decode(&self.body).map_err(|_| ModuleError::InvalidInput)?;
        if event.stage() != crate::main_ports::EventStage::Observation {
            return Err(ModuleError::InvalidInput);
        }
        Ok(event)
    }
    /// Encode explicitly; native pointers/long/enum representations never cross this boundary.
    pub fn encode(&self) -> Result<Vec<u8>, ModuleError> {
        self.validate()?;
        let size = u32::try_from(self.body.len()).map_err(|_| ModuleError::InvalidInput)?;
        let mut out = vec![0; HEADER_BYTES];
        out[0..4].copy_from_slice(&0x32435341u32.to_le_bytes());
        out[4..6].copy_from_slice(&DRIVER_REVISION.to_le_bytes());
        out[6..8].copy_from_slice(&(self.class as u16).to_le_bytes());
        out[8..10].copy_from_slice(&self.message_type.to_le_bytes());
        out[12..16].copy_from_slice(&size.to_le_bytes());
        out[16..32].copy_from_slice(&self.session);
        for (at, value) in [
            (32, self.request_id),
            (40, self.total_bytes),
            (48, self.offset),
        ] {
            out[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.body);
        Ok(out)
    }
    /// Decode the one current envelope within caller-supplied real transport/resource limits.
    pub fn decode(bytes: &[u8], max_frame: usize, max_transfer: u64) -> Result<Self, ModuleError> {
        if bytes.len() < HEADER_BYTES
            || bytes.len() > max_frame
            || u32::from_le_bytes(array(bytes, 0)?) != 0x32435341
            || u16::from_le_bytes(array(bytes, 4)?) != DRIVER_REVISION
            || bytes[10..12] != [0, 0]
            || bytes[56..64] != [0; 8]
        {
            return Err(ModuleError::InvalidInput);
        }
        let length = u32::from_le_bytes(array(bytes, 12)?) as usize;
        if length != bytes.len() - HEADER_BYTES {
            return Err(ModuleError::InvalidInput);
        }
        let class = match u16::from_le_bytes(array(bytes, 6)?) {
            1 => FrameClass::Request,
            2 => FrameClass::Reply,
            3 => FrameClass::Event,
            _ => return Err(ModuleError::InvalidInput),
        };
        let frame = Self {
            class,
            message_type: u16::from_le_bytes(array(bytes, 8)?),
            session: array(bytes, 16)?,
            request_id: u64::from_le_bytes(array(bytes, 32)?),
            total_bytes: u64::from_le_bytes(array(bytes, 40)?),
            offset: u64::from_le_bytes(array(bytes, 48)?),
            body: bytes[HEADER_BYTES..].to_vec(),
        };
        if frame.total_bytes > max_transfer {
            return Err(ModuleError::Busy);
        }
        frame.validate()?;
        Ok(frame)
    }
    fn validate(&self) -> Result<(), ModuleError> {
        let end = self
            .offset
            .checked_add(self.body.len() as u64)
            .ok_or(ModuleError::InvalidInput)?;
        let fragmented = self.offset != 0 || end != self.total_bytes;
        let fragments_allowed = matches!(
            (self.class, self.message_type),
            (FrameClass::Request, 2 | 3) | (FrameClass::Event, 0x10a)
        );
        if end > self.total_bytes
            || (fragmented && (self.body.is_empty() || !fragments_allowed))
            || (self.class == FrameClass::Event
                && (self.request_id != 0 || !matches!(self.message_type, 0x100..=0x10c)))
            || (self.class != FrameClass::Event
                && (self.request_id == 0 || !matches!(self.message_type, 1..=18)))
            || (self.session == [0; 16]
                && !(self.class == FrameClass::Request && self.message_type == 1))
        {
            return Err(ModuleError::InvalidInput);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_callbacks_keep_child_identity_and_reject_control_or_foreign_sessions() {
        let mut body = vec![0u8; 88];
        body[0..2].copy_from_slice(&1u16.to_le_bytes());
        body[2..4].copy_from_slice(&88u16.to_le_bytes());
        body[8..10].copy_from_slice(&1u16.to_le_bytes());
        body[10..12].copy_from_slice(&1u16.to_le_bytes());
        body[12..16].copy_from_slice(&2u32.to_le_bytes());
        body[16..32].copy_from_slice(&[4; 16]);
        body[32..40].copy_from_slice(&1u64.to_le_bytes());
        body[40..48].copy_from_slice(&1u64.to_le_bytes());
        for (id, value) in [
            (3u16, 222u32.to_le_bytes().to_vec()),
            (6, 333u32.to_le_bytes().to_vec()),
            (7, 444u64.to_le_bytes().to_vec()),
            (8, 555u64.to_le_bytes().to_vec()),
        ] {
            body.extend(id.to_le_bytes());
            body.extend([0, 0]);
            body.extend((value.len() as u32).to_le_bytes());
            body.extend(value);
        }
        let length = body.len() as u32;
        body[4..8].copy_from_slice(&length.to_le_bytes());
        let mut frame = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x104,
            session: [9; 16],
            request_id: 0,
            total_bytes: body.len() as u64,
            offset: 0,
            body,
        };
        let event = frame.observation([9; 16]).expect("bound observation");
        let crate::dispatch_event::EventPayload::Process(child) = event.payload else {
            panic!("process")
        };
        assert_eq!(child.child_pid, Some(222));
        assert_eq!(child.child_parent_pid, Some(333));
        assert_eq!(child.child_parent_birth_boottime_ns, Some(444));
        assert_eq!(child.child_cgroup_id, Some(555));
        assert!(frame.observation([8; 16]).is_err());
        frame.body[10..12].copy_from_slice(&2u16.to_le_bytes());
        assert!(frame.observation([9; 16]).is_err());
    }
    #[test]
    fn group_observation_uses_the_current_unsolicited_event_envelope() {
        let mut frame = DriverFrame {
            class: FrameClass::Event,
            message_type: 0x10a,
            session: [9; 16],
            request_id: 0,
            total_bytes: 64,
            offset: 0,
            body: vec![0; 64],
        };
        let bytes = frame.encode().expect("Group event envelope");
        assert!(DriverFrame::decode(&bytes, 1024, 1024).is_ok());
        frame.total_bytes = 18851 + 72;
        let bytes = frame.encode().expect("first Group body fragment");
        assert!(DriverFrame::decode(&bytes, 1024, 4 * 1024 * 1024).is_ok());
        for kind in [0x100, 0x104, 0x109] {
            frame.message_type = kind;
            assert!(
                frame.encode().is_err(),
                "other small events stay indivisible"
            );
        }
        frame.message_type = 0x10a;
        frame.request_id = 1;
        assert!(frame.encode().is_err());
    }
    #[test]
    fn current_envelope_rejects_old_headers_and_illegal_fragment_shapes() {
        let small = DriverFrame {
            class: FrameClass::Request,
            message_type: 10,
            session: [1; 16],
            request_id: 8,
            total_bytes: 0,
            offset: 0,
            body: vec![],
        };
        let bytes = small.encode().expect("current envelope");
        assert!(DriverFrame::decode(&bytes, 64, 1024).is_ok());
        let mut old = bytes.clone();
        old[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert!(DriverFrame::decode(&old, 64, 1024).is_err());
        let mut reserved = bytes;
        reserved[10] = 1;
        assert!(DriverFrame::decode(&reserved, 64, 1024).is_err());
        let bulk = DriverFrame {
            class: FrameClass::Request,
            message_type: 3,
            session: [1; 16],
            request_id: 7,
            total_bytes: 6,
            offset: 0,
            body: b"abc".to_vec(),
        };
        assert!(bulk.encode().is_ok());
        let mut illegal = bulk.clone();
        illegal.message_type = 10;
        assert!(
            illegal.encode().is_err(),
            "small commands cannot be fragmented"
        );
        illegal = bulk.clone();
        illegal.offset = 4;
        assert!(illegal.encode().is_err(), "fragment exceeds total body");
        illegal = bulk;
        illegal.offset = u64::MAX;
        assert!(
            illegal.encode().is_err(),
            "fragment offset addition overflows"
        );
    }
}
