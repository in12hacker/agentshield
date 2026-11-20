//! Current external JSON envelope. Backend interfaces exchange the original typed values.
use as_main::messages::protocol::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use serde_json::value::RawValue;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Incoming<'a> {
    version: u16,
    #[serde(borrow, default, deserialize_with = "present")]
    pub message: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    pub stream: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    pub private: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    pub business: Option<&'a RawValue>,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'de RawValue>, D::Error> {
    // An explicit null still occupies a lane and cannot hide an extra field.
    <&RawValue>::deserialize(d).map(Some)
}

pub(crate) fn decode_incoming(bytes: &[u8]) -> Result<Incoming<'_>, ProtocolError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError);
    }
    let input: Incoming<'_> = serde_json::from_slice(bytes).map_err(|_| ProtocolError)?;
    let count = [input.message, input.stream, input.private, input.business]
        .into_iter()
        .flatten()
        .count();
    if input.version != VERSION || count != 1 {
        return Err(ProtocolError);
    }
    Ok(input)
}
struct Outgoing<'a, T: ?Sized> {
    lane: &'static str,
    value: &'a T,
}
impl<T: Serialize + ?Sized> Serialize for Outgoing<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut envelope = serializer.serialize_struct("Envelope", 2)?;
        envelope.serialize_field("version", &VERSION)?;
        envelope.serialize_field(self.lane, self.value)?;
        envelope.end()
    }
}
pub(crate) fn encode_lane<T: Serialize + ?Sized>(
    lane: &'static str,
    value: &T,
) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(&Outgoing { lane, value }).map_err(|_| ProtocolError)
}
/// Invalid shape, unknown/old revision or unsupported request.
#[derive(Debug, thiserror::Error)]
#[error("invalid current middleware message")]
pub struct ProtocolError;
/// Decode only the current whole DTO after session authentication.
pub fn decode_down(bytes: &[u8]) -> Result<Down, ProtocolError> {
    let raw = decode_incoming(bytes)?.message.ok_or(ProtocolError)?;
    serde_json::from_str(raw.get()).map_err(|_| ProtocolError)
}
/// Encode a middleware request for the one current schema.
pub fn encode_down(message: &Down) -> Result<Vec<u8>, ProtocolError> {
    encode_lane("message", message)
}
/// Encode a genuine backend result or event.
pub fn encode_up(message: &Up) -> Result<Vec<u8>, ProtocolError> {
    encode_lane("message", message)
}

/// Decode the sole current completion envelope after authenticating its source.
/// This decodes facts only; it does not create a retained original-operation capability.
pub fn decode_operation_result(bytes: &[u8]) -> Result<OperationResult, ProtocolError> {
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum CompletionMessage {
        OperationResult(OperationResult),
    }
    let raw = decode_incoming(bytes)?.message.ok_or(ProtocolError)?;
    let CompletionMessage::OperationResult(result) =
        serde_json::from_str(raw.get()).map_err(|_| ProtocolError)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_null_cannot_hide_a_second_lane() {
        let valid =
            encode_down(&Down::ReviewAvailability { available: true }).expect("current message");
        assert!(matches!(
            decode_down(&valid),
            Ok(Down::ReviewAvailability { available: true })
        ));
        let mixed = format!(
            r#"{{"version":{VERSION},"message":{{"type":"review_availability","available":true}},"stream":null}}"#
        );
        assert!(decode_incoming(mixed.as_bytes()).is_err());
        let null = format!(r#"{{"version":{VERSION},"message":null}}"#);
        assert!(decode_down(null.as_bytes()).is_err());
    }
}
