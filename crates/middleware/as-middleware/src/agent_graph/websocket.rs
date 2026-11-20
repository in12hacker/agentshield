//! Passive framing of owner-verified WebSocket bytes; never opens or writes a socket.
use std::{
    io::{self, Cursor, Read, Write},
    ops::Range,
};
use tokio_tungstenite::tungstenite::{
    self, Message as WsMessage,
    protocol::{
        Role, WebSocketConfig, WebSocketContext,
        frame::{
            FrameHeader,
            coding::{Data, OpCode},
        },
    },
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid WebSocket frame: {0}")]
    Protocol(String),
    #[error("WebSocket wire or message exceeds the configured byte limit")]
    SizeLimit,
    #[error("WebSocket frame has incomplete or mismatched wire bytes")]
    IncompleteFrame,
    #[error("WebSocket stream offset is discontinuous or overflows")]
    StreamOffset,
    #[error("WebSocket extensions are unsupported for message indexing")]
    UnsupportedExtension,
    #[error("WebSocket decoder attempted output on passive input")]
    WriteAttempt,
    #[error("WebSocket data arrived after this direction's close frame")]
    DataAfterClose,
}

fn protocol(error: tungstenite::Error) -> Error {
    Error::Protocol(error.to_string())
}

/// Positions are byte indices in the supplied slice, including the original extended-length form.
#[derive(Debug)]
pub(crate) struct DecodedFrame {
    pub(super) header: FrameHeader,
    pub(super) payload: Range<usize>,
    pub(super) wire_len: usize,
}

/// Only complete frames are returned. TCP reassembly and actual byte ownership stay with the caller.
pub(crate) fn decode_frame(
    data: &[u8],
    from_client: bool,
    maximum: usize,
) -> Result<Option<DecodedFrame>, Error> {
    let mut cursor = Cursor::new(data);
    let Some((header, payload_len)) = FrameHeader::parse(&mut cursor).map_err(protocol)? else {
        return Ok(None);
    };
    let header_len = usize::try_from(cursor.position()).map_err(|_| Error::SizeLimit)?;
    let payload_len = usize::try_from(payload_len).map_err(|_| Error::SizeLimit)?;
    let wire_len = header_len
        .checked_add(payload_len)
        .filter(|length| *length <= maximum)
        .ok_or(Error::SizeLimit)?;
    if header.mask.is_some() != from_client {
        return Err(Error::Protocol(
            "frame mask does not match its direction".into(),
        ));
    }
    if let OpCode::Control(_) = header.opcode
        && (!header.is_final || payload_len > 125)
    {
        return Err(Error::Protocol(
            "fragmented or oversized control frame".into(),
        ));
    }
    if data.len() < wire_len {
        return Ok(None);
    }
    Ok(Some(DecodedFrame {
        header,
        payload: header_len..wire_len,
        wire_len,
    }))
}

/// Offset is the caller's original direction-local byte position, independent of TCP sequence wrap.
#[derive(Debug)]
pub(crate) struct WireFrame {
    pub(crate) offset: u64,
    pub(crate) wire: Vec<u8>,
}

#[derive(Debug)]
enum Payload {
    /// Single unmasked data frames borrow their original bytes without copying the payload.
    Original {
        range: Range<usize>,
        text: bool,
    },
    Decoded(WsMessage),
}

#[derive(Debug)]
pub(crate) struct Message {
    /// Data fragments only; interleaved controls are returned independently with their own offsets.
    pub(crate) frames: Vec<WireFrame>,
    payload: Payload,
}

impl Message {
    /// Only protocol text messages can enter the caller's JSON interpretation.
    pub(crate) fn text(&self) -> Option<&str> {
        match &self.payload {
            Payload::Original { range, text: true } => {
                std::str::from_utf8(self.frames.first()?.wire.get(range.clone())?).ok()
            }
            Payload::Decoded(WsMessage::Text(text)) => Some(text.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Rejected {
    pub(super) error: Error,
    /// All consumed original frames are returned to the owner for gap persistence.
    pub(super) frames: Vec<WireFrame>,
}

struct Pending {
    context: WebSocketContext,
    frames: Vec<WireFrame>,
    wire_bytes: usize,
    payload_bytes: usize,
}

/// One direction of an already verified, unextended HTTP Upgrade connection.
pub(crate) struct Assembler {
    from_client: bool,
    maximum: usize,
    next_offset: Option<u64>,
    pending: Option<Pending>,
    closed: bool,
}

impl Assembler {
    pub(crate) fn new(from_client: bool, maximum: usize) -> Self {
        Self {
            from_client,
            maximum,
            next_offset: None,
            pending: None,
            closed: false,
        }
    }

    /// Original fragments plus the necessary decoded assembly, excluding already emitted messages.
    pub(super) fn retained(&self) -> usize {
        self.pending
            .as_ref()
            .map_or(0, |pending| pending.wire_bytes + pending.payload_bytes)
    }

    /// The connection owner calls this when an incomplete message loses its source or closes.
    pub(super) fn take_pending(&mut self) -> Vec<WireFrame> {
        self.pending
            .take()
            .map_or_else(Vec::new, |pending| pending.frames)
    }

    fn context(&self) -> WebSocketContext {
        // These fixed write limits satisfy the library constructor. No output method is invoked.
        let config = WebSocketConfig::default()
            .read_buffer_size(0)
            .write_buffer_size(0)
            .max_write_buffer_size(1)
            .max_message_size(Some(self.maximum))
            .max_frame_size(Some(self.maximum));
        WebSocketContext::new(
            if self.from_client {
                Role::Server
            } else {
                Role::Client
            },
            Some(config),
        )
    }

    fn reject(&mut self, error: Error, frame: WireFrame) -> Rejected {
        let mut frames = self.take_pending();
        frames.push(frame);
        Rejected { error, frames }
    }

    /// Takes exactly one complete frame after its full original wire range was owner-verified.
    pub(crate) fn push(
        &mut self,
        offset: u64,
        wire: Vec<u8>,
        frame: DecodedFrame,
    ) -> Result<Option<Message>, Rejected> {
        let original = WireFrame { offset, wire };
        if frame.wire_len != original.wire.len()
            || frame.payload.end != frame.wire_len
            || frame.payload.start > frame.payload.end
            || frame.header.mask.is_some() != self.from_client
        {
            return Err(self.reject(Error::IncompleteFrame, original));
        }
        let Some(next_offset) = offset.checked_add(frame.wire_len as u64) else {
            return Err(self.reject(Error::StreamOffset, original));
        };
        if self.next_offset.is_some_and(|expected| expected != offset) {
            return Err(self.reject(Error::StreamOffset, original));
        }
        self.next_offset = Some(next_offset);
        if frame.header.rsv1 || frame.header.rsv2 || frame.header.rsv3 {
            return Err(self.reject(Error::UnsupportedExtension, original));
        }
        if frame.wire_len > self.maximum {
            return Err(self.reject(Error::SizeLimit, original));
        }
        if self.closed && matches!(frame.header.opcode, OpCode::Data(_)) {
            return Err(self.reject(Error::DataAfterClose, original));
        }

        if let OpCode::Control(_) = frame.header.opcode {
            // The context is discarded after one read, before it can flush automatic pong/close.
            let decoded = read_once(&mut self.context(), &original.wire);
            return match decoded {
                Ok(Some(message)) => {
                    self.closed |= message.is_close();
                    Ok(Some(Message {
                        frames: vec![original],
                        payload: Payload::Decoded(message),
                    }))
                }
                Ok(None) => Err(self.reject(Error::IncompleteFrame, original)),
                Err(error) => Err(self.reject(error, original)),
            };
        }

        if self.pending.is_none()
            && frame.header.is_final
            && frame.header.mask.is_none()
            && matches!(frame.header.opcode, OpCode::Data(Data::Text | Data::Binary))
        {
            let text = frame.header.opcode == OpCode::Data(Data::Text);
            if text && std::str::from_utf8(&original.wire[frame.payload.clone()]).is_err() {
                return Err(self.reject(Error::Protocol("invalid UTF-8 text".into()), original));
            }
            return Ok(Some(Message {
                frames: vec![original],
                payload: Payload::Original {
                    range: frame.payload,
                    text,
                },
            }));
        }

        let mut pending = match self.pending.take() {
            Some(pending) => pending,
            None => Pending {
                context: self.context(),
                frames: Vec::new(),
                wire_bytes: 0,
                payload_bytes: 0,
            },
        };
        let total = pending.wire_bytes.checked_add(frame.wire_len);
        let Some(total) = total.filter(|total| *total <= self.maximum) else {
            self.pending = Some(pending);
            return Err(self.reject(Error::SizeLimit, original));
        };
        pending.wire_bytes = total;
        pending.payload_bytes += frame.payload.len();
        let decoded = read_once(&mut pending.context, &original.wire);
        pending.frames.push(original);
        match decoded {
            Ok(Some(message)) => Ok(Some(Message {
                frames: pending.frames,
                payload: Payload::Decoded(message),
            })),
            Ok(None) => {
                self.pending = Some(pending);
                Ok(None)
            }
            Err(error) => Err(Rejected {
                error,
                frames: pending.frames,
            }),
        }
    }
}

/// An exhausted input means "wait for the next verified frame", never a fabricated peer close.
struct PassiveInput<'a> {
    bytes: &'a [u8],
    attempted_output: bool,
}

impl Read for PassiveInput<'_> {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        if self.bytes.is_empty() {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        self.bytes.read(target)
    }
}

impl Write for PassiveInput<'_> {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        self.attempted_output = true;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.attempted_output = true;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

fn read_once(context: &mut WebSocketContext, bytes: &[u8]) -> Result<Option<WsMessage>, Error> {
    let mut input = PassiveInput {
        bytes,
        attempted_output: false,
    };
    let result = context.read(&mut input);
    if input.attempted_output {
        return Err(Error::WriteAttempt);
    }
    match result {
        Ok(message) if input.bytes.is_empty() => Ok(Some(message)),
        Ok(_) => Err(Error::IncompleteFrame),
        Err(tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(protocol(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::{
        Bytes,
        protocol::frame::{Frame, coding::Control},
    };

    const MAXIMUM: usize = 2 * 1024 * 1024;

    fn wire(opcode: OpCode, final_frame: bool, payload: &[u8], masked: bool) -> Vec<u8> {
        let header = FrameHeader {
            opcode,
            is_final: final_frame,
            mask: masked.then_some([0x12, 0x34, 0x56, 0x78]),
            ..FrameHeader::default()
        };
        let mut wire = Vec::new();
        Frame::from_payload(header, Bytes::copy_from_slice(payload))
            .format(&mut wire)
            .expect("test frame encoding");
        wire
    }

    fn push(
        assembler: &mut Assembler,
        offset: u64,
        wire: Vec<u8>,
    ) -> Result<Option<Message>, Rejected> {
        let frame = decode_frame(&wire, assembler.from_client, assembler.maximum)
            .expect("test frame header")
            .expect("complete test frame");
        assembler.push(offset, wire, frame)
    }

    #[test]
    fn split_tcp_and_nonminimal_length_preserve_original_wire_and_borrow_payload() {
        // The locked parser accepts this 16-bit encoding of a three-byte payload.
        let original = vec![0x81, 126, 0, 3, b'a', b'b', b'c'];
        for boundary in 0..original.len() {
            assert!(
                decode_frame(&original[..boundary], false, MAXIMUM)
                    .expect("valid partial frame")
                    .is_none()
            );
        }
        let frame = decode_frame(&original, false, MAXIMUM)
            .expect("header")
            .expect("frame");
        assert_eq!(frame.wire_len, original.len());
        assert_eq!(frame.payload, 4..7);
        let mut assembler = Assembler::new(false, MAXIMUM);
        let message = assembler
            .push(91, original.clone(), frame)
            .expect("valid message")
            .expect("text");
        assert_eq!(message.text(), Some("abc"));
        assert_eq!(message.frames[0].offset, 91);
        assert_eq!(message.frames[0].wire, original);
        assert_eq!(
            message.text().expect("text").as_ptr(),
            message.frames[0].wire[4..].as_ptr()
        );
        assert_eq!(assembler.retained(), 0);
    }

    #[test]
    fn masked_fragments_and_interleaved_controls_reuse_library_without_output() {
        let first = wire(OpCode::Data(Data::Text), false, &[0xe4], true);
        let ping = wire(OpCode::Control(Control::Ping), true, b"", true);
        let last = wire(OpCode::Data(Data::Continue), true, &[0xb8, 0xad], true);
        let mut assembler = Assembler::new(true, MAXIMUM);
        assert!(
            push(&mut assembler, 100, first.clone())
                .expect("first fragment")
                .is_none()
        );
        assert!(assembler.retained() > first.len());
        let control = push(&mut assembler, 100 + first.len() as u64, ping.clone())
            .expect("passive ping")
            .expect("control");
        assert!(control.text().is_none());
        assert_eq!(control.frames[0].wire, ping);
        let message = push(
            &mut assembler,
            100 + (first.len() + ping.len()) as u64,
            last.clone(),
        )
        .expect("last fragment")
        .expect("complete message");
        assert_eq!(message.text(), Some("中"));
        assert_eq!(message.frames.len(), 2);
        assert_eq!(message.frames[0].wire, first);
        assert_eq!(message.frames[1].wire, last);
        assert_eq!(
            message.frames[1].offset,
            100 + (first.len() + ping.len()) as u64
        );
        assert_eq!(assembler.retained(), 0);
        let close = wire(OpCode::Control(Control::Close), true, &[], true);
        let close_len = close.len();
        assert!(
            push(
                &mut assembler,
                100 + (first.len() + ping.len() + last.len()) as u64,
                close
            )
            .expect("passive close")
            .expect("close")
            .text()
            .is_none()
        );
        let after_close = wire(OpCode::Data(Data::Text), true, b"later", true);
        let rejected = push(
            &mut assembler,
            100 + (first.len() + ping.len() + last.len() + close_len) as u64,
            after_close.clone(),
        )
        .expect_err("data after close");
        assert!(matches!(rejected.error, Error::DataAfterClose));
        assert_eq!(rejected.frames[0].wire, after_close);
    }

    #[test]
    fn rejects_extensions_invalid_utf8_and_invalid_fragment_order_without_losing_originals() {
        assert!(decode_frame(&[0x81, 0], true, MAXIMUM).is_err());
        assert!(decode_frame(&[0x81, 0x80, 0, 0, 0, 0], false, MAXIMUM).is_err());
        assert!(decode_frame(&[0x09, 0], false, MAXIMUM).is_err());
        let compressed = vec![0xc1, 1, b'x'];
        let mut assembler = Assembler::new(false, MAXIMUM);
        let rejected =
            push(&mut assembler, 0, compressed.clone()).expect_err("unsupported compression");
        assert!(matches!(rejected.error, Error::UnsupportedExtension));
        assert_eq!(rejected.frames[0].wire, compressed);

        let first = wire(OpCode::Data(Data::Text), false, &[0xe4], false);
        let invalid = wire(OpCode::Data(Data::Continue), true, b"x", false);
        let mut assembler = Assembler::new(false, MAXIMUM);
        assert!(
            push(&mut assembler, 0, first.clone())
                .expect("partial UTF-8")
                .is_none()
        );
        let rejected = push(&mut assembler, first.len() as u64, invalid.clone())
            .expect_err("invalid split UTF-8");
        assert_eq!(rejected.frames[0].wire, first);
        assert_eq!(rejected.frames[1].wire, invalid);
        assert_eq!(assembler.retained(), 0);

        let mut assembler = Assembler::new(false, MAXIMUM);
        let unexpected = wire(OpCode::Data(Data::Continue), true, b"", false);
        assert!(push(&mut assembler, 0, unexpected).is_err());
    }

    #[test]
    fn bounds_aggregate_wire_and_returns_pending_on_close_or_source_gap() {
        let first = wire(OpCode::Data(Data::Text), false, b"a", false);
        let last = wire(OpCode::Data(Data::Continue), true, b"b", false);
        let mut assembler = Assembler::new(false, first.len() + last.len() - 1);
        assert!(
            push(&mut assembler, 0, first.clone())
                .expect("first")
                .is_none()
        );
        let rejected =
            push(&mut assembler, first.len() as u64, last.clone()).expect_err("wire limit");
        assert!(matches!(rejected.error, Error::SizeLimit));
        assert_eq!(rejected.frames.len(), 2);
        assert_eq!(assembler.retained(), 0);

        let mut assembler = Assembler::new(false, MAXIMUM);
        assert!(
            push(&mut assembler, 0, first.clone())
                .expect("first")
                .is_none()
        );
        let rejected = push(&mut assembler, first.len() as u64 + 1, last).expect_err("source gap");
        assert!(matches!(rejected.error, Error::StreamOffset));
        assert_eq!(rejected.frames[0].wire, first);
        assert_eq!(assembler.retained(), 0);
    }
}
