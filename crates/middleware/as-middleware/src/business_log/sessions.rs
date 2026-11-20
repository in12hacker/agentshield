//! Application Session and chat semantics. Pure projections borrow one decoded Gateway document.
use crate::gateway::GatewayDocument;
pub use crate::gateway::{
    GatewayConnection, GatewayFieldReference, GatewayFrameReference, GatewayReference,
};
use as_types::{flow::*, process_tree::ProcessKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

mod observations;
pub(crate) use observations::materialize_chat;

/// Verified Gateway runtime issuer for the documented host IPv4-loopback endpoint scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySessionNamespace {
    /// Full server process lifetime from the verified connection, not a bare PID.
    pub server: ProcessKey,
    /// Actual server endpoint port, 1..=65535; client ports and capture IDs are excluded.
    pub server_port: u16,
}
impl From<&GatewayConnection> for GatewaySessionNamespace {
    fn from(c: &GatewayConnection) -> Self {
        Self {
            server: c.server,
            server_port: c.server_port,
        }
    }
}

/// Immutable database anchor for one source-backed Session group; not an application-issued ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatSessionRef {
    /// Selected model-request field group in its original complete HTTP source domain.
    ModelHttp {
        /// First original request record used to register this group.
        record: ProtocolRecordId,
    },
    /// Gateway Session identified by its verified issuer and nonempty original sessionId.
    NativeGateway {
        /// Fixed original session object anchor; capture locates evidence, not application identity.
        object: GatewayFieldReference,
    },
}
/// Fields acquired from one specified public Gateway session object, never a merged snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySessionObservation {
    /// Original JSON object and its stored message/frame location.
    pub reference: GatewayFieldReference,
    /// This object's sessionId string; None is unacquired/unsupported, Some("") preserves empty.
    pub session_id: Option<String>,
    /// This object's designated key/sessionKey string; selectors and effective aliases stay distinct.
    pub session_key: Option<String>,
    /// This object's runId string, not substituted from lastRunId or a paired request.
    pub run_id: Option<String>,
    /// This object's lastRunId string, which refers only to the explicitly named previous/latest run.
    pub last_run_id: Option<String>,
    /// This object's explicit agentId string; not a process or Session identity.
    pub agent_id: Option<String>,
    /// Original request only when its same-capture RPC pairing has been proven; otherwise None.
    pub request: Option<GatewayReference>,
}
/// One directory item, preserving the anchor's source rather than manufacturing a current snapshot.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatSession {
    /// Original model HTTP requests grouped by the configured selected field and complete source domain.
    ModelHttp {
        /// Fixed model_http anchor for this group.
        reference: ChatSessionRef,
        /// Original source of the representative request.
        source: Arc<ProtocolSource>,
        /// Original selected field, scalar types, values and acquisition issue.
        session: SessionObservation,
        /// Distinct actual model exchanges observed in this group; decimal-string u64 on JSON wire.
        #[serde(with = "as_types::decimal")]
        exchanges: u64,
    },
    /// Actual application Session observed through a verified public Gateway connection.
    NativeGateway {
        /// Fixed native_gateway anchor for this Session.
        reference: ChatSessionRef,
        /// Verified runtime issuer used with the anchor's sessionId to establish identity.
        namespace: GatewaySessionNamespace,
        /// Actual connection of the anchor only; other observations retain their own connections.
        source: Arc<GatewayConnection>,
        /// Unmodified fields of the anchor object, not fields filled from later observations.
        session: Box<GatewaySessionObservation>,
        /// Confirmed original chat observations, not logical messages/runs/tasks; JSON decimal u64.
        #[serde(with = "as_types::decimal")]
        chat_observations: u64,
        /// Retained gaps and conflicts concerning the original anchor.
        gaps: Vec<SessionGap>,
    },
}
impl ChatSession {
    /// Borrow the fixed original anchor regardless of source variant.
    pub(crate) fn reference(&self) -> &ChatSessionRef {
        match self {
            Self::ModelHttp { reference, .. } | Self::NativeGateway { reference, .. } => reference,
        }
    }
}
/// Explicit source and membership scope for one chat-history read.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatMessageScope {
    /// Only messages from one original model HTTP exchange.
    ModelHttp {
        /// Complete original exchange identity, without application Session substitution.
        exchange: ExchangeKey,
    },
    /// Gateway observations with a proven Session, or individually unassociated observations.
    NativeGateway {
        /// Some requires a native_gateway anchor; None does not imply a shared Session.
        session: Option<ChatSessionRef>,
    },
}
/// Immutable source reference for a logical message or an identity-unresolved original observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatMessageRef {
    /// Existing model message identity.
    ModelHttp {
        /// Original Flow message ID within its actual exchange.
        message: ChatMessageId,
    },
    /// Original Gateway message object or direct-input field, without a fabricated HTTP exchange.
    NativeGateway {
        /// First fixed committed object anchor for this message/observation.
        object: GatewayFieldReference,
    },
}
/// Page position, separate from the immutable message reference and from source-native cursors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatMessageCursor {
    /// Continuation in the original model exchange's first-consumption order.
    ModelHttp {
        /// Last original model message returned on the previous page.
        message: ChatMessageId,
    },
    /// Gateway continuation at the first page's fixed committed knowledge cutoff.
    NativeGateway {
        /// Last returned native_gateway message/observation anchor in the same scope.
        reference: ChatMessageRef,
        /// SQLite deliveries cutoff, 1..=i64::MAX, encoded as decimal string; not time or source seq.
        #[serde(with = "as_types::decimal")]
        read_through: u64,
    },
}
/// Source-specific history item; only explicit native identity merges multiple original observations.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatMessageEntry {
    /// The unchanged existing model HTTP message DTO.
    ModelHttp {
        /// Original structured Flow message, including its original source and revisions.
        message: Arc<ChatMessage>,
    },
    /// Native logical message or unresolved observation, retaining every original observation.
    NativeGateway {
        /// Fixed native_gateway object anchor for this item.
        reference: ChatMessageRef,
        /// Proven original message ID scalar; None when missing, invalid or conflicting.
        native_message_id: Option<NativeIdValue>,
        /// Proven Session sequence, local full-window order, or explicitly unresolved position.
        order: Box<GatewayChatOrder>,
        /// Nonempty original observations; only explicit revisions establish revision order.
        observations: Vec<GatewayChatObservation>,
        /// Original identity, membership, ordering or revision evidence gaps.
        gaps: Vec<SessionGap>,
    },
}
/// Business observation gap; these values are not driver failures or HIPS decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionGapReason {
    /// The original value does not have the specified public object/message shape.
    UnsupportedShape,
    /// A present field has an unsupported scalar type; the original field reference remains.
    FieldTypeUnsupported,
    /// The verified source cannot establish the documented Gateway issuer domain.
    SessionNamespaceUnavailable,
    /// No supported nonempty original Session ID or proven handoff is available.
    SessionIdentityUnavailable,
    /// A key/alias exists but cannot by itself establish a Session instance.
    AliasBindingUnproven,
    /// Only one side of the original same-capture RPC has been acquired.
    RpcUnpaired,
    /// The original paired RPC did not establish successful application acceptance.
    RpcFailed,
    /// Competing requests/responses prevent a unique same-capture RPC handoff.
    RpcConflict,
    /// Explicit identity evidence or duplicate effective fields contradict one another.
    IdentityConflict,
    /// No supported original message ID is available for logical-message grouping.
    MessageIdentityUnavailable,
    /// No supported Session sequence or full-window local position is proven.
    MessageOrderUnproven,
    /// Acquired message-position evidence is mutually inconsistent.
    MessageOrderConflict,
    /// Differing original observations lack an explicit ordered revision handoff.
    RevisionOrderUnproven,
}
/// Gap with nonempty references to the original acquired objects/fields it concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGap {
    /// Single defined business reason; not a replacement for an original field value.
    pub reason: SessionGapReason,
    /// Original evidence locations; absent fields are represented by their containing object.
    pub references: Vec<GatewayFieldReference>,
}
/// Construct one gap at an original object/field already selected by the projection.
pub(crate) fn gap(reason: SessionGapReason, reference: &GatewayFieldReference) -> SessionGap {
    SessionGap {
        reason,
        references: vec![reference.clone()],
    }
}
/// Acquired original role and its protocol-specific projection, or a method-defined input role.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GatewayChatRole {
    /// An actual string role property on the original message object.
    String {
        /// Public protocol interpretation; toolResult maps to Tool, unknown/empty strings to Other.
        value: ChatRole,
        /// Exact decoded original role string, not rewritten to its projected vocabulary.
        original: String,
        /// Original role property's location.
        field: GatewayFieldReference,
    },
    /// A present non-string role property, including JSON null/false/containers.
    Json {
        /// Original JSON type and value.
        value: Value,
        /// Original role property's location.
        field: GatewayFieldReference,
    },
    /// User role defined by sessions.create/chat.send direct-text input semantics.
    ProtocolInput {
        /// Protocol User; it does not assert a human identity or application acceptance.
        value: ChatRole,
        /// Original /method field proving this interpretation; no original role is fabricated.
        method: GatewayFieldReference,
    },
}
/// One original Gateway chat observation, materialized only for a selected read result.
#[derive(Debug, Clone, Serialize)]
pub struct GatewayChatObservation {
    /// This observation's actual connection, matching reference.message.capture.
    pub source: Arc<GatewayConnection>,
    /// Original message object or direct input field.
    pub reference: GatewayFieldReference,
    /// Delta wrapper object; None for non-wrapped sources.
    pub wrapper: Option<GatewayFieldReference>,
    /// Original history payload; None for non-window sources.
    pub history: Option<GatewayFieldReference>,
    /// Already-proven native_gateway Session anchor, or None for unassociated evidence.
    pub session: Option<ChatSessionRef>,
    /// Original same-capture successful response object for a direct input; not copied request fields.
    pub handoff: Option<GatewaySessionObservation>,
    /// Original event /payload/runId string; None outside that source.
    pub run_id: Option<String>,
    /// Original message /__openclaw/runId string, never substituted from event/handoff run.
    pub message_run_id: Option<String>,
    /// Original message /__openclaw/seq string/number scalar with its original type.
    pub message_sequence: Option<NativeIdValue>,
    /// Original wrapper /messageSeq string/number scalar, independent of message_sequence.
    pub wrapper_sequence: Option<NativeIdValue>,
    /// Original /__openclaw/recordTimestampMs JSON value declaring Unix milliseconds; missing omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_timestamp_ms: Option<Value>,
    /// Original /__openclaw/transcriptPosition JSON value; Some(null) differs from absent None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_position: Option<Value>,
    /// References only to the specified actually present message metadata/tool fields.
    pub native_fields: Vec<GatewayFieldReference>,
    /// Actual acquired role or supported method-defined role; no property means None.
    pub role: Option<GatewayChatRole>,
    /// Original content; Text("") and Json(null) remain acquired values, absence is None.
    pub content: Option<ChatContent>,
    /// Only the original chat event /payload/state JSON value; absence is omitted, not completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    /// Only the original message revision string/number; not a sequence, run, RPC ID or SQL row.
    pub revision: Option<NativeIdValue>,
}
/// Business-provided position, without using SQL arrival as a cross-window conversation order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GatewayChatOrder {
    /// A comparable original nonnegative integer message position in one proven Session.
    SessionSequence {
        /// Original JSON-number scalar, exactly representable as u64, not a numeric string.
        sequence: NativeIdValue,
        /// Nonempty references to the original __openclaw.seq/messageSeq fields.
        references: Vec<GatewayFieldReference>,
    },
    /// Local order within a single actual full-history window.
    HistoryWindow {
        /// Original window /payload reference, not a synthesized Session.
        window: GatewayFieldReference,
        /// Original zero-based /messages array index, in 0..=u32::MAX.
        index: u32,
    },
    /// Original observations with insufficient or conflicting order evidence.
    Unresolved {
        /// Nonempty original references; their displayed order is not claimed as conversation order.
        references: Vec<GatewayFieldReference>,
    },
}
/// Supported public history response kind; Full is a classification of the normal untagged window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayHistoryKind {
    /// Original normal complete-window shape with no explicit kind property.
    Full,
    /// Original kind="delta" response; an empty message array is still an observed result.
    Delta,
    /// Original kind="reset" window handoff reset, not a new Session or deletion.
    Reset,
}
/// Narrow original history-window references; no content tree or second message list is stored here.
#[derive(Debug, Clone)]
pub struct GatewayHistoryWindow {
    /// Original response /payload object.
    pub reference: GatewayFieldReference,
    /// Proven original same-capture request.
    pub request: GatewayReference,
    /// Supported public kind without inventing an original full tag.
    pub kind: GatewayHistoryKind,
    /// Only the window's own explicit session fields; inherited membership is stored separately.
    pub session: Option<GatewaySessionObservation>,
    /// Actual /payload/messages field, preserving present [] versus absent None.
    pub messages: Option<GatewayFieldReference>,
    /// Original request /params/cursor field; its contents are opaque.
    pub input_cursor: Option<GatewayFieldReference>,
    /// Original response /payload/deltaCursor field; not a Session or revision number.
    pub output_cursor: Option<GatewayFieldReference>,
}
/// Content-free write projection of one chat object/direct input and its original index fields.
#[derive(Debug, Clone)]
pub(crate) struct ProjectedChat {
    /// Original message object or direct-input field; no content is owned by this index projection.
    pub reference: GatewayFieldReference,
    /// Delta wrapper when the message is nested under its /message field.
    pub wrapper: Option<GatewayFieldReference>,
    /// Original history response payload, when present.
    pub history: Option<GatewayFieldReference>,
    /// Original event payload runId, not the message's runId.
    pub run_id: Option<String>,
    /// Original message __openclaw.runId, kept independently of the event run.
    pub message_run_id: Option<String>,
    /// Original message __openclaw.seq scalar, without numeric coercion.
    pub message_sequence: Option<NativeIdValue>,
    /// Original wrapper.messageSeq scalar, without numeric coercion.
    pub wrapper_sequence: Option<NativeIdValue>,
    /// Only the original message revision scalar.
    pub revision: Option<NativeIdValue>,
    /// Whether the method defines this original field as direct input, including unsupported values.
    pub(super) input: bool,
    /// Original session objects that may prove this observation's membership.
    pub session_fields: Vec<GatewaySessionObservation>,
    /// Message's own original __openclaw.id scalar.
    pub native_id: Option<NativeIdValue>,
    /// Delta wrapper's original messageId scalar, kept separately for consistency checks.
    pub wrapper_id: Option<NativeIdValue>,
    /// Original zero-based history-array position; None outside an acquired window array.
    pub history_index: Option<u32>,
    /// Whether this position came from a normal full window, rather than delta/reset.
    pub full_window: bool,
    /// Projection gaps with original references; no content values are retained here.
    pub gaps: Vec<SessionGap>,
}
/// One pure document projection shared with the original SQL work; never a history cache.
#[derive(Default)]
pub(crate) struct GatewayProjection {
    /// Explicit original session objects, including partial values.
    pub sessions: Vec<GatewaySessionObservation>,
    /// Content-free original chat-object index projections.
    pub chats: Vec<ProjectedChat>,
    /// Original history-window references and kind.
    pub windows: Vec<GatewayHistoryWindow>,
    /// Document/session projection gaps with their original locations.
    pub gaps: Vec<SessionGap>,
}

fn string(doc: &GatewayDocument<'_>, path: &str, gaps: &mut Vec<SessionGap>) -> Option<String> {
    match doc.value.pointer(path) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            gaps.push(gap(
                SessionGapReason::FieldTypeUnsupported,
                &doc.field(path),
            ));
            None
        }
        None => None,
    }
}
/// Read only supported original string/number ID scalars, preserving number spelling via the codec.
pub(crate) fn native_id(value: Option<&Value>) -> Option<NativeIdValue> {
    match value? {
        Value::String(s) => Some(NativeIdValue::JsonString(s.clone())),
        Value::Number(n) => Some(NativeIdValue::JsonNumber(n.to_string())),
        _ => None,
    }
}
fn id(doc: &GatewayDocument<'_>, path: &str, gaps: &mut Vec<SessionGap>) -> Option<NativeIdValue> {
    let value = doc.value.pointer(path);
    let id = native_id(value);
    if value.is_some() && id.is_none() {
        gaps.push(gap(
            SessionGapReason::FieldTypeUnsupported,
            &doc.field(path),
        ));
    }
    id
}
fn session(
    doc: &GatewayDocument<'_>,
    path: &str,
    key: &str,
    runs: bool,
    last: bool,
    request: Option<&GatewayReference>,
    gaps: &mut Vec<SessionGap>,
) -> Option<GatewaySessionObservation> {
    if !doc.value.pointer(path).is_some_and(Value::is_object) {
        gaps.push(gap(
            SessionGapReason::UnsupportedShape,
            &observed_reference(doc, path),
        ));
        return None;
    }
    Some(GatewaySessionObservation {
        reference: doc.field(path),
        session_id: string(doc, &format!("{path}/sessionId"), gaps),
        session_key: string(doc, &format!("{path}/{key}"), gaps),
        run_id: if runs {
            string(doc, &format!("{path}/runId"), gaps)
        } else {
            None
        },
        last_run_id: if last {
            string(doc, &format!("{path}/lastRunId"), gaps)
        } else {
            None
        },
        agent_id: string(doc, &format!("{path}/agentId"), gaps),
        request: request.cloned(),
    })
}
fn observed_reference(doc: &GatewayDocument<'_>, mut pointer: &str) -> GatewayFieldReference {
    while doc.value.pointer(pointer).is_none() {
        pointer = pointer
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
    }
    doc.field(pointer)
}
// These are duplicate effective views within one publication, never independent aliases or selectors.
fn same_object_conflicts(a: &GatewaySessionObservation, b: &GatewaySessionObservation) -> bool {
    [&a.session_id, &a.session_key, &a.agent_id]
        .into_iter()
        .zip([&b.session_id, &b.session_key, &b.agent_id])
        .any(|(a, b)| matches!((a,b),(Some(a),Some(b)) if a != b))
}
enum ChatShape {
    Input,
    Full,
    Partial,
}

fn chat(
    doc: &GatewayDocument<'_>,
    path: &str,
    wrapper: Option<&str>,
    history: Option<&str>,
    index: Option<u32>,
    shape: ChatShape,
    fields: Vec<GatewaySessionObservation>,
) -> ProjectedChat {
    let full = matches!(shape, ChatShape::Full);
    let input = matches!(shape, ChatShape::Input);
    let mut gaps = Vec::new();
    let value = doc.value.pointer(path);
    let direct = input && value.is_some_and(Value::is_string);
    if (input && !direct) || (!input && !value.is_some_and(Value::is_object)) {
        gaps.push(gap(SessionGapReason::UnsupportedShape, &doc.field(path)));
    }
    let native_id = id(doc, &format!("{path}/__openclaw/id"), &mut gaps);
    let wrapper_id = wrapper.and_then(|p| id(doc, &format!("{p}/messageId"), &mut gaps));
    let message_sequence = id(doc, &format!("{path}/__openclaw/seq"), &mut gaps);
    let wrapper_sequence = wrapper.and_then(|p| id(doc, &format!("{p}/messageSeq"), &mut gaps));
    if matches!((&native_id,&wrapper_id),(Some(a),Some(b)) if a!=b) {
        gaps.push(gap(SessionGapReason::IdentityConflict, &doc.field(path)));
    }
    if matches!((&message_sequence,&wrapper_sequence),(Some(a),Some(b)) if a!=b) {
        gaps.push(gap(
            SessionGapReason::MessageOrderConflict,
            &doc.field(path),
        ));
    }
    let event = doc.value.get("type").and_then(Value::as_str) == Some("event");
    ProjectedChat {
        reference: doc.field(path),
        wrapper: wrapper.map(|p| doc.field(p)),
        history: history.map(|p| doc.field(p)),
        run_id: if event {
            string(doc, "/payload/runId", &mut gaps)
        } else {
            None
        },
        message_run_id: string(doc, &format!("{path}/__openclaw/runId"), &mut gaps),
        message_sequence,
        wrapper_sequence,
        revision: id(doc, &format!("{path}/revision"), &mut gaps),
        input,
        session_fields: fields,
        native_id,
        wrapper_id,
        history_index: index,
        full_window: full,
        gaps,
    }
}

/// Interpret only the public method/event objects. Requests are passed explicitly after RPC validation.
pub(crate) fn project_gateway(
    doc: &GatewayDocument<'_>,
    request: Option<&GatewayDocument<'_>>,
) -> GatewayProjection {
    let mut p = GatewayProjection::default();
    let kind = doc.value.get("type").and_then(Value::as_str);
    let mut add =
        |path: &str, key: &str, runs: bool, last: bool, req: Option<&GatewayReference>| {
            if let Some(s) = session(doc, path, key, runs, last, req, &mut p.gaps) {
                p.sessions.push(s);
            }
        };
    if doc.reference.from_client && kind == Some("req") {
        if matches!(
            doc.value.get("method").and_then(Value::as_str),
            Some("sessions.create" | "chat.send")
        ) {
            let key = if doc.value["method"] == "sessions.create" {
                "key"
            } else {
                "sessionKey"
            };
            p.sessions.push(GatewaySessionObservation {
                reference: doc.field("/params"),
                session_id: None,
                session_key: string(doc, &format!("/params/{key}"), &mut p.gaps),
                agent_id: string(doc, "/params/agentId", &mut p.gaps),
                run_id: None,
                last_run_id: None,
                request: None,
            });
            if doc.value.pointer("/params/message").is_some() {
                p.chats.push(chat(
                    doc,
                    "/params/message",
                    None,
                    None,
                    None,
                    ChatShape::Input,
                    p.sessions.clone(),
                ));
            }
        }
    } else if !doc.reference.from_client && kind == Some("event") {
        match doc.value.get("event").and_then(Value::as_str) {
            Some("sessions.changed") => {
                if doc.value.pointer("/payload/session").is_some() {
                    add("/payload/session", "key", false, true, None);
                }
                if doc
                    .value
                    .pointer("/payload")
                    .and_then(Value::as_object)
                    .is_some_and(|v| {
                        ["sessionId", "sessionKey", "runId", "agentId"]
                            .iter()
                            .any(|k| v.contains_key(*k))
                    })
                {
                    add("/payload", "sessionKey", true, false, None);
                }
            }
            Some("chat" | "agent") => {
                add("/payload", "sessionKey", true, false, None);
                if doc.value["event"] == "chat" && doc.value.pointer("/payload/message").is_some() {
                    p.chats.push(chat(
                        doc,
                        "/payload/message",
                        None,
                        None,
                        None,
                        ChatShape::Partial,
                        p.sessions.clone(),
                    ));
                }
            }
            _ => {}
        }
    } else if !doc.reference.from_client
        && kind == Some("res")
        && doc.value.get("ok").and_then(Value::as_bool) == Some(true)
        && let Some(req) = request
    {
        match req.value.get("method").and_then(Value::as_str) {
            Some("sessions.create")
                if doc.value.pointer("/payload/ok").and_then(Value::as_bool) == Some(true) =>
            {
                add("/payload", "key", true, false, Some(req.reference));
                if doc.value.pointer("/payload/entry").is_some() {
                    add("/payload/entry", "key", true, true, Some(req.reference));
                }
            }
            Some("chat.send") => {
                add("/payload", "sessionKey", true, false, Some(req.reference));
            }
            Some("sessions.describe") => {
                add("/payload/session", "key", false, true, Some(req.reference));
            }
            Some("sessions.list") => {
                if let Some(items) = doc
                    .value
                    .pointer("/payload/sessions")
                    .and_then(Value::as_array)
                {
                    for i in 0..items.len() {
                        add(
                            &format!("/payload/sessions/{i}"),
                            "key",
                            false,
                            true,
                            Some(req.reference),
                        );
                    }
                } else {
                    p.gaps.push(gap(
                        SessionGapReason::UnsupportedShape,
                        &observed_reference(doc, "/payload"),
                    ));
                }
            }
            Some("chat.startup" | "chat.history") => {
                let kind = match doc.value.pointer("/payload/kind") {
                    None if doc
                        .value
                        .pointer("/payload/messages")
                        .is_some_and(Value::is_array) =>
                    {
                        Some(GatewayHistoryKind::Full)
                    }
                    Some(Value::String(s)) if s == "delta" => Some(GatewayHistoryKind::Delta),
                    Some(Value::String(s)) if s == "reset" => Some(GatewayHistoryKind::Reset),
                    _ => None,
                };
                if let Some(kind) = kind {
                    add("/payload", "sessionKey", false, false, Some(req.reference));
                    let s = p.sessions.last().cloned();
                    let messages = doc.value.pointer("/payload/messages");
                    p.windows.push(GatewayHistoryWindow {
                        reference: doc.field("/payload"),
                        request: req.reference.clone(),
                        kind,
                        session: s.clone(),
                        messages: messages.map(|_| doc.field("/payload/messages")),
                        input_cursor: req
                            .value
                            .pointer("/params/cursor")
                            .map(|_| req.field("/params/cursor")),
                        output_cursor: doc
                            .value
                            .pointer("/payload/deltaCursor")
                            .map(|_| doc.field("/payload/deltaCursor")),
                    });
                    if let Some(items) = messages.and_then(Value::as_array) {
                        for (i, item) in items.iter().enumerate() {
                            let path = format!("/payload/messages/{i}");
                            let mut fields: Vec<_> = s.iter().cloned().collect();
                            let wrapper =
                                kind == GatewayHistoryKind::Delta && item.get("message").is_some();
                            if wrapper {
                                for (path, key) in [
                                    (path.clone(), "sessionKey"),
                                    (format!("{path}/session"), "key"),
                                ] {
                                    if doc.value.pointer(&path).is_some_and(Value::is_object)
                                        && let Some(s) = session(
                                            doc,
                                            &path,
                                            key,
                                            false,
                                            true,
                                            Some(req.reference),
                                            &mut p.gaps,
                                        )
                                    {
                                        fields.push(s.clone());
                                        p.sessions.push(s);
                                    }
                                }
                            }
                            let message = if wrapper {
                                format!("{path}/message")
                            } else {
                                path.clone()
                            };
                            p.chats.push(chat(
                                doc,
                                &message,
                                wrapper.then_some(path.as_str()),
                                Some("/payload"),
                                u32::try_from(i).ok(),
                                if kind == GatewayHistoryKind::Full {
                                    ChatShape::Full
                                } else {
                                    ChatShape::Partial
                                },
                                fields,
                            ));
                        }
                    } else if messages.is_some() {
                        p.gaps.push(gap(
                            SessionGapReason::UnsupportedShape,
                            &doc.field("/payload/messages"),
                        ));
                    }
                } else {
                    p.gaps.push(gap(
                        SessionGapReason::UnsupportedShape,
                        &observed_reference(doc, "/payload/kind"),
                    ));
                }
            }
            _ => {}
        }
    }
    for c in &p.chats {
        let ids_conflict=c.session_fields
            .iter()
            .enumerate()
            .any(|(i,a)|c.session_fields[..i].iter().any(|b|matches!((nonempty(&a.session_id),nonempty(&b.session_id)),(Some(a),Some(b)) if a!=b)));
        if ids_conflict {
            p.gaps.push(SessionGap {
                reason: SessionGapReason::IdentityConflict,
                references: c
                    .session_fields
                    .iter()
                    .map(|s| s.reference.clone())
                    .collect(),
            });
        } else if let Some(wrapper) = &c.wrapper {
            // History's root sessionKey may retain the request selector. Only wrapper/session
            // duplicate effective fields are comparable as the same original session projection.
            let flat = c.session_fields.iter().find(|s| s.reference == *wrapper);
            let nested = c
                .session_fields
                .iter()
                .find(|s| s.reference.pointer == format!("{}/session", wrapper.pointer));
            if let (Some(flat), Some(nested)) = (flat, nested)
                && same_object_conflicts(flat, nested)
            {
                p.gaps.push(SessionGap {
                    reason: SessionGapReason::IdentityConflict,
                    references: vec![flat.reference.clone(), nested.reference.clone()],
                });
            }
        }
    }
    // Only co-present objects describing one result are compared; list items are independent.
    if (matches!(
        request
            .and_then(|r| r.value.get("method"))
            .and_then(Value::as_str),
        Some("sessions.create")
    ) || doc.value.get("event").and_then(Value::as_str) == Some("sessions.changed"))
        && p.sessions
            .iter()
            .enumerate()
            .any(|(i, a)| p.sessions[..i].iter().any(|b| same_object_conflicts(a, b)))
    {
        p.gaps.push(SessionGap {
            reason: SessionGapReason::IdentityConflict,
            references: p.sessions.iter().map(|s| s.reference.clone()).collect(),
        });
    }
    p
}

/// Only exact, nonempty native IDs can select an application identity.
pub(crate) fn nonempty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|s| !s.is_empty())
}

/// Whether a retained identity conflict explicitly names this original object.
pub(crate) fn identity_disputed(gaps: &[SessionGap], reference: &GatewayFieldReference) -> bool {
    gaps.iter()
        .any(|g| g.reason == SessionGapReason::IdentityConflict && g.references.contains(reference))
}
/// Borrow an acquired message ID, excluding only an empty string from identity selection.
pub(crate) fn usable_id(value: &Option<NativeIdValue>) -> Option<&NativeIdValue> {
    value
        .as_ref()
        .filter(|v| !matches!(v,NativeIdValue::JsonString(s) if s.is_empty()))
}
/// Compare only original JSON nonnegative integer numbers that fit u64, never numeric strings/floats.
pub(crate) fn sequence(value: &NativeIdValue) -> Option<u64> {
    match value {
        NativeIdValue::JsonNumber(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().ok()
        }
        _ => None,
    }
}

/// Compare acquired Session IDs only; run candidates are scoped by the caller's verified namespace.
/// Selectors, aliases and agent metadata neither prove membership nor contradict an already proven ID.
pub(crate) fn bind_session(
    fields: &[GatewaySessionObservation],
    run: Option<&str>,
    candidates: &[GatewaySessionObservation],
) -> (Option<GatewaySessionObservation>, Vec<SessionGap>) {
    let mut proof: Vec<_> = fields
        .iter()
        .filter(|s| nonempty(&s.session_id).is_some())
        .cloned()
        .collect();
    if let Some(run) = run.filter(|r| !r.is_empty()) {
        proof.extend(
            candidates
                .iter()
                .filter(|s| {
                    nonempty(&s.session_id).is_some()
                        && (s.run_id.as_deref() == Some(run)
                            || s.last_run_id.as_deref() == Some(run))
                })
                .cloned(),
        );
    }
    let conflict = proof
        .iter()
        .enumerate()
        .any(|(i, a)| proof[..i].iter().any(|b| a.session_id != b.session_id));
    if conflict {
        let mut references = Vec::new();
        for source in &proof {
            if !references.contains(&source.reference) {
                references.push(source.reference.clone());
            }
        }
        return (
            None,
            vec![SessionGap {
                reason: SessionGapReason::IdentityConflict,
                references,
            }],
        );
    }
    (proof.into_iter().next(), Vec::new())
}

/// Borrow the opaque original cursor only for the supported chat.history delta continuation method.
pub(crate) fn history_input_cursor<'a>(
    req: &GatewayDocument<'a>,
    p: &GatewayProjection,
) -> Option<&'a Value> {
    if req.value.get("method").and_then(Value::as_str) != Some("chat.history")
        || !p
            .windows
            .iter()
            .any(|w| w.kind == GatewayHistoryKind::Delta)
    {
        return None;
    }
    req.value.pointer("/params/cursor")
}
/// Retain explicit Session references from matched original windows; a reset breaks this handoff.
pub(crate) fn continue_history(
    candidates: Vec<(GatewayHistoryKind, GatewaySessionObservation)>,
) -> Vec<GatewaySessionObservation> {
    if candidates
        .iter()
        .any(|(kind, _)| *kind == GatewayHistoryKind::Reset)
    {
        return Vec::new();
    }
    // Every competing reference is retained for the membership matcher; uniqueness alone is not proof.
    candidates
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| nonempty(&s.session_id).is_some())
        .collect()
}

/// Original index facts supplied by one SQL transaction for pure membership matching.
pub(crate) struct MembershipInput {
    /// Local positive gateway_chat_observations row ID, not an application message identity.
    pub id: i64,
    /// Original chat/input object whose membership is being decided.
    pub reference: GatewayFieldReference,
    /// Original explicit session-object references valid in this transaction.
    pub fields: Vec<GatewaySessionObservation>,
    /// Message's own original __openclaw.id scalar.
    pub native_id: Option<NativeIdValue>,
    /// Wrapper's original messageId scalar, distinct from the nested message field.
    pub wrapper_id: Option<NativeIdValue>,
    /// Original event runId for exact-run matching, not inferred from an alias.
    pub run_id: Option<String>,
    /// Original message __openclaw.runId, kept independently for conflict checking.
    pub message_run_id: Option<String>,
    /// Actual RPC pairing/failure gap, or None when the original handoff is established.
    pub rpc: Option<SessionGapReason>,
    /// Same-capture original successful response objects for this input, without copied request fields.
    pub handoffs: Vec<GatewaySessionObservation>,
    /// Already-retained original shape, identity or source-validation gaps.
    pub gaps: Vec<SessionGap>,
}
/// Pure membership result; SQL stores its original references and this transaction's delivery.
pub(crate) struct Membership {
    /// An actual original object proving the Session ID, or None when unproven/conflicting.
    pub proof: Option<GatewaySessionObservation>,
    /// The original successful input handoff object, separate from proof of current membership.
    pub handoff: Option<GatewaySessionObservation>,
    /// Retained original and newly identified matching gaps, never synthetic success.
    pub gaps: Vec<SessionGap>,
}
/// Match explicit IDs and exact runs using only caller-supplied same-issuer original facts.
pub(crate) fn resolve_membership(
    input: &MembershipInput,
    candidates: &[GatewaySessionObservation],
) -> Membership {
    let mut gaps = input.gaps.clone();
    let mut fields = input.fields.clone();
    fields.extend(input.handoffs.iter().cloned());
    if let Some(reason) = &input.rpc {
        gaps.push(gap(reason.clone(), &input.reference));
    }
    let (mut proof, mut binding_gaps) = bind_session(&fields, input.run_id.as_deref(), candidates);
    gaps.append(&mut binding_gaps);
    let (message_proof, mut message_gaps) =
        bind_session(&fields, input.message_run_id.as_deref(), candidates);
    gaps.append(&mut message_gaps);
    if matches!((&proof,&message_proof),(Some(a),Some(b)) if a.session_id!=b.session_id) {
        gaps.push(gap(SessionGapReason::IdentityConflict, &input.reference));
    }
    if proof.is_none() {
        proof = message_proof;
    }
    for handoff in &input.handoffs {
        let (handoff_proof, mut handoff_gaps) =
            bind_session(&fields, handoff.run_id.as_deref(), candidates);
        gaps.append(&mut handoff_gaps);
        if matches!((&proof,&handoff_proof),(Some(a),Some(b)) if a.session_id!=b.session_id) {
            gaps.push(gap(SessionGapReason::IdentityConflict, &input.reference));
        }
        if proof.is_none() {
            proof = handoff_proof;
        }
    }
    if gaps
        .iter()
        .any(|g| g.reason == SessionGapReason::IdentityConflict)
    {
        proof = None;
    }
    if proof.is_none() {
        gaps.push(gap(
            if fields.iter().any(|s| s.session_key.is_some()) {
                SessionGapReason::AliasBindingUnproven
            } else {
                SessionGapReason::SessionIdentityUnavailable
            },
            &input.reference,
        ));
    }
    Membership {
        proof,
        handoff: input
            .handoffs
            .first()
            .cloned()
            .filter(|_| input.rpc.is_none()),
        gaps,
    }
}

/// A query-time view of indexed source positions, with membership fixed at the SQL read cutoff.
pub(crate) struct ChatIndex {
    /// Local positive original chat-observation row ID.
    pub id: i64,
    /// This observation's actual message/input object reference.
    pub reference: GatewayFieldReference,
    /// Fixed logical-message object anchor established by committed membership evidence.
    pub anchor: GatewayFieldReference,
    /// Proven Session at the query's fixed cutoff, or None for an unassociated observation.
    pub session: Option<ChatSessionRef>,
    /// Original nested message ID scalar.
    pub native_id: Option<NativeIdValue>,
    /// Original wrapper messageId scalar.
    pub wrapper_id: Option<NativeIdValue>,
    /// Original message __openclaw.seq scalar.
    pub message_sequence: Option<NativeIdValue>,
    /// Original wrapper.messageSeq scalar, not substituted into message_sequence.
    pub wrapper_sequence: Option<NativeIdValue>,
    /// Actual window /payload reference, if this observation came from history.
    pub history: Option<GatewayFieldReference>,
    /// Original zero-based messages-array position, if acquired.
    pub history_index: Option<u32>,
    /// Whether history_index proves local order inside a full window.
    pub full_window: bool,
    /// Already-decided original input handoff at the same fixed cutoff.
    pub handoff: Option<GatewaySessionObservation>,
    /// Original evidence gaps visible at that cutoff.
    pub gaps: Vec<SessionGap>,
}
/// One query-local logical message/observation group formed from original indexes, without content.
pub(crate) struct ChatGroup {
    /// Immutable original message/observation anchor returned to the caller.
    pub reference: ChatMessageRef,
    /// Proven original message ID scalar, or None when unavailable/conflicting.
    pub native_id: Option<NativeIdValue>,
    /// Business-derived source position, not SQL arrival as conversation order.
    pub order: GatewayChatOrder,
    /// Original observation indexes belonging to this group at the fixed cutoff.
    pub indexes: Vec<ChatIndex>,
    /// Identity and ordering gaps with their original references.
    pub gaps: Vec<SessionGap>,
}
/// Group same-Session native message IDs and derive only documented source positions.
pub(crate) fn group_messages(indexes: Vec<ChatIndex>) -> Vec<ChatGroup> {
    let mut groups: Vec<ChatGroup> = Vec::new();
    for index in indexes {
        let id = if index
            .gaps
            .iter()
            .any(|g| g.reason == SessionGapReason::IdentityConflict)
        {
            None
        } else {
            usable_id(&index.native_id)
                .or_else(|| usable_id(&index.wrapper_id))
                .cloned()
        };
        let existing = if index.session.is_some() {
            id.as_ref().and_then(|id| {
                groups.iter().position(|g| {
                    g.native_id.as_ref() == Some(id)
                        && g.indexes
                            .first()
                            .is_some_and(|i| i.session == index.session)
                })
            })
        } else {
            None
        };
        if let Some(i) = existing {
            groups[i].indexes.push(index);
        } else {
            groups.push(ChatGroup {
                reference: ChatMessageRef::NativeGateway {
                    object: index.anchor.clone(),
                },
                native_id: id,
                order: GatewayChatOrder::Unresolved {
                    references: vec![index.reference.clone()],
                },
                indexes: vec![index],
                gaps: Vec::new(),
            });
        }
    }
    for group in &mut groups {
        if group.native_id.is_none() {
            group.gaps.push(gap(
                SessionGapReason::MessageIdentityUnavailable,
                &group.indexes[0].reference,
            ));
        }
        let mut positions = Vec::new();
        let mut refs = Vec::new();
        for index in &group.indexes {
            group.gaps.extend(index.gaps.iter().cloned());
            for (value, pointer) in [
                (
                    &index.message_sequence,
                    format!("{}/__openclaw/seq", index.reference.pointer),
                ),
                (
                    &index.wrapper_sequence,
                    index
                        .reference
                        .pointer
                        .strip_suffix("/message")
                        .map(|p| format!("{p}/messageSeq"))
                        .unwrap_or_default(),
                ),
            ] {
                if let Some(value) = value {
                    if let Some(seq) = sequence(value) {
                        positions.push((seq, value.clone()));
                        refs.push(GatewayFieldReference {
                            message: index.reference.message.clone(),
                            pointer,
                        });
                    } else {
                        group.gaps.push(gap(
                            SessionGapReason::MessageOrderUnproven,
                            &index.reference,
                        ));
                    }
                }
            }
        }
        let conflict = positions
            .first()
            .is_some_and(|(first, _)| positions.iter().any(|(n, _)| n != first))
            || group
                .gaps
                .iter()
                .any(|g| g.reason == SessionGapReason::MessageOrderConflict);
        if conflict {
            group.gaps.push(gap(
                SessionGapReason::MessageOrderConflict,
                &group.indexes[0].reference,
            ));
        } else if group.indexes[0].session.is_some()
            && let Some((_, value)) = positions.first()
        {
            group.order = GatewayChatOrder::SessionSequence {
                sequence: value.clone(),
                references: refs,
            };
            continue;
        }
        if !conflict
            && let Some((window, index)) = group
                .indexes
                .iter()
                .find(|i| i.full_window && i.history.is_some() && i.history_index.is_some())
                .and_then(|i| Some((i.history.clone()?, i.history_index?)))
        {
            group.order = GatewayChatOrder::HistoryWindow { window, index };
            continue;
        }
        group.gaps.push(gap(
            SessionGapReason::MessageOrderUnproven,
            &group.indexes[0].reference,
        ));
    }
    let collisions:Vec<_>=groups.iter().enumerate().filter_map(|(i,a)| {
        let GatewayChatOrder::SessionSequence {sequence:seq,..}=&a.order else{return None;};
        groups.iter().enumerate().any(|(j,b)| i!=j && matches!(&b.order,GatewayChatOrder::SessionSequence {sequence:other,..} if sequence(seq)==sequence(other))).then_some(i)
    }).collect();
    for i in collisions {
        let g = &mut groups[i];
        g.gaps.push(gap(
            SessionGapReason::MessageOrderConflict,
            &g.indexes[0].reference,
        ));
        g.order = GatewayChatOrder::Unresolved {
            references: g.indexes.iter().map(|i| i.reference.clone()).collect(),
        };
    }
    groups.sort_by_key(|g| match &g.order {
        GatewayChatOrder::SessionSequence { sequence: s, .. } => {
            (0, sequence(s).unwrap_or_default(), 0, 0)
        }
        GatewayChatOrder::HistoryWindow { window, index } => {
            (1, window.message.id, u64::from(*index), 0)
        }
        GatewayChatOrder::Unresolved { .. } => {
            (2, g.indexes[0].reference.message.id, 0, g.indexes[0].id)
        }
    });
    groups
}

/// Present acquired observations together, retaining conflicts and only explicitly ordered revisions.
pub(crate) fn assemble_entry(
    group: ChatGroup,
    mut observations: Vec<GatewayChatObservation>,
) -> ChatMessageEntry {
    let mut gaps = group.gaps;
    for (i, a) in observations.iter().enumerate() {
        for b in &observations[..i] {
            let role_conflict = match (&a.role, &b.role) {
                (
                    Some(GatewayChatRole::String { original: a, .. }),
                    Some(GatewayChatRole::String { original: b, .. }),
                ) => a != b,
                (
                    Some(GatewayChatRole::Json { value: a, .. }),
                    Some(GatewayChatRole::Json { value: b, .. }),
                ) => a != b,
                (Some(GatewayChatRole::Json { .. }), Some(GatewayChatRole::String { .. }))
                | (Some(GatewayChatRole::String { .. }), Some(GatewayChatRole::Json { .. })) => {
                    true
                }
                _ => false,
            };
            let content_conflict = matches!((&a.content,&b.content),(Some(a),Some(b)) if a!=b);
            let ordered_revision = matches!((&a.revision,&b.revision),(Some(a),Some(b)) if a!=b && sequence(a).is_some() && sequence(b).is_some());
            if (role_conflict || content_conflict) && !ordered_revision {
                gaps.push(gap(
                    if a.revision.is_some() && a.revision == b.revision {
                        SessionGapReason::IdentityConflict
                    } else {
                        SessionGapReason::RevisionOrderUnproven
                    },
                    &a.reference,
                ));
            }
        }
    }
    // Explicit numeric revisions can be presented in source revision order. SQL arrival is not one.
    if observations
        .iter()
        .all(|o| o.revision.as_ref().and_then(sequence).is_some())
    {
        observations.sort_by_key(|o| o.revision.as_ref().and_then(sequence));
    }
    ChatMessageEntry::NativeGateway {
        reference: group.reference,
        native_message_id: group.native_id,
        order: Box::new(group.order),
        observations,
        gaps,
    }
}
