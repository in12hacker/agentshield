//! Shared, passive public Gateway document. No business state or transport acquisition.
use crate::agent_graph::websocket::Message;
use as_types::process_tree::ProcessKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One owner-verified TCP/Upgrade connection in the documented host IPv4-loopback Gateway scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConnection {
    /// Original verified connection identifier; an evidence/RPC scope, not an application Session.
    pub capture: [u8; 32],
    /// Original 16-byte driver observation source identifier.
    pub source: [u8; 16],
    /// Original driver initialization generation, full u64 encoded as a decimal string.
    #[serde(with = "as_types::decimal")]
    pub generation: u64,
    /// Original first-SYN observation ordinal, full u64 encoded as a decimal string.
    #[serde(with = "as_types::decimal")]
    pub first_syn_ordinal: u64,
    /// Actual client endpoint port, 1..=65535 and distinct from server_port.
    pub client_port: u16,
    /// Actual server endpoint port, 1..=65535, used with the full server lifetime as issuer scope.
    pub server_port: u16,
    /// Actual verified client process lifetime, not a name or bare PID.
    pub client: ProcessKey,
    /// Actual verified server process lifetime, including boot and birth identity.
    pub server: ProcessKey,
    /// Actual nonzero client socket cookie, full u64 encoded as a decimal string.
    #[serde(with = "as_types::decimal")]
    pub client_socket_cookie: u64,
    /// Actual nonzero server socket cookie, independently verified and encoded as decimal u64.
    #[serde(with = "as_types::decimal")]
    pub server_socket_cookie: u64,
}

/// Raw stream provenance without verified Upgrade/process ownership; it cannot issue business IDs.
#[derive(Debug, Clone)]
pub(crate) struct UnverifiedGatewaySource {
    /// Original driver source, without a synthesized verified capture ID.
    pub source: [u8; 16],
    /// Original driver initialization generation.
    pub generation: u64,
    /// Original first-SYN source ordinal.
    pub first_syn_ordinal: u64,
    /// Observed client port, 1..=65535.
    pub client_port: u16,
    /// Observed server port, 1..=65535 and distinct from client_port.
    pub server_port: u16,
}

/// One original frame's location in its direction-local TCP application stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayFrameReference {
    /// Original wire-byte offset after SYN, including Upgrade bytes; decimal u64, not TCP32 seq.
    #[serde(with = "as_types::decimal")]
    pub offset: u64,
    /// Positive original wire-byte length of this frame, encoded as decimal u64.
    #[serde(with = "as_types::decimal")]
    pub length: u64,
}

/// Same-database original complete-message identity, independent of HTTP and application IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayReference {
    /// Positive original group_gateway_messages row ID, 1..=i64::MAX, JSON decimal string.
    #[serde(with = "as_types::decimal")]
    pub id: u64,
    /// Actual verified connection of this message; reconnects do not redefine application Sessions.
    pub capture: [u8; 32],
    /// True for original client-to-server bytes, false for original server-to-client bytes.
    pub from_client: bool,
    /// Nonempty original frame positions in source order, nonoverlapping for a complete message.
    pub frames: Vec<GatewayFrameReference>,
}

/// An original JSON field/object location, without a duplicate content value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayFieldReference {
    /// Actual stored complete message and its source frame references.
    pub message: GatewayReference,
    /// RFC6901 JSON pointer; empty string denotes the root and arrays are zero-based.
    pub pointer: String,
}

/// Complete-message decoding outcome separate from original transport/source gap reasons.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GatewayDecodeIssue {
    /// Original complete WS message is not protocol text.
    NonText,
    /// Text is not valid unique-key JSON under the shared precise codec.
    InvalidJson,
}

/// Decode before admitting the SQL transaction; the SQL work owns this tree and the original wire.
pub(crate) fn decode_message(message: &Message) -> Result<Value, GatewayDecodeIssue> {
    let text = message.text().ok_or(GatewayDecodeIssue::NonText)?;
    as_types::json::parse(text.as_bytes()).map_err(|_| GatewayDecodeIssue::InvalidJson)
}

/// Call-local view of a verified connection, assigned original reference and one decoded JSON tree.
pub(crate) struct GatewayDocument<'a> {
    /// Actual verified connection owning this message.
    pub connection: &'a GatewayConnection,
    /// Original assigned message/frame reference, matching connection.capture.
    pub reference: &'a GatewayReference,
    /// Borrowed original JSON tree; never serialized as a second log or cached between calls.
    pub value: &'a Value,
}

/// Borrowed public req/res envelope fields, without application interpretation or copied payloads.
pub(crate) struct GatewayRpc<'a> {
    /// Exactly req from client or res from server; other directions/shapes have no RPC projection.
    pub kind: &'static str,
    /// Original RPC string ID, including empty; unsupported/missing values are None.
    pub id: Option<&'a str>,
    /// Original method string when present; response IDs are not method names.
    pub method: Option<&'a str>,
    /// Original boolean ok when present, not inferred from missing error or payload.
    pub ok: Option<bool>,
}

impl GatewayDocument<'_> {
    /// Borrow only a direction-correct public RPC envelope; events cannot pair by a same-text id.
    pub(crate) fn rpc(&self) -> Option<GatewayRpc<'_>> {
        let kind = match (
            self.reference.from_client,
            self.value.get("type").and_then(Value::as_str),
        ) {
            (true, Some("req")) => "req",
            (false, Some("res")) => "res",
            _ => return None,
        };
        Some(GatewayRpc {
            kind,
            id: self.value.get("id").and_then(Value::as_str),
            method: self.value.get("method").and_then(Value::as_str),
            ok: self.value.get("ok").and_then(Value::as_bool),
        })
    }
    /// Reference the caller-selected original field/object without copying its JSON value.
    pub(crate) fn field(&self, pointer: &str) -> GatewayFieldReference {
        GatewayFieldReference {
            message: self.reference.clone(),
            pointer: pointer.to_owned(),
        }
    }
}
