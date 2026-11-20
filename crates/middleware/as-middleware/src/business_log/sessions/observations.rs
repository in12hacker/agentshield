//! Read-only materialization of selected original chat fields; write projections own no content.
use super::*;

/// Materialize one selected observation after SQL has supplied its already-decided membership.
/// The caller borrows the same decoded document used by project_gateway; no identity is rejudged.
pub(crate) fn materialize_chat(
    doc: &GatewayDocument<'_>,
    chat: ProjectedChat,
    session: Option<ChatSessionRef>,
    handoff: Option<GatewaySessionObservation>,
) -> GatewayChatObservation {
    let path = &chat.reference.pointer;
    let value = doc.value.pointer(path);
    let direct = chat.input && value.is_some_and(Value::is_string);
    let role = if direct {
        Some(GatewayChatRole::ProtocolInput {
            value: ChatRole::User,
            method: doc.field("/method"),
        })
    } else {
        value.and_then(|v| v.get("role")).map(|role| {
            let field = doc.field(&format!("{path}/role"));
            if let Some(s) = role.as_str() {
                GatewayChatRole::String {
                    original: s.to_owned(),
                    field,
                    value: match s {
                        "system" => ChatRole::System,
                        "developer" => ChatRole::Developer,
                        "user" => ChatRole::User,
                        "assistant" => ChatRole::Assistant,
                        "tool" | "toolResult" => ChatRole::Tool,
                        other => ChatRole::Other(other.to_owned()),
                    },
                }
            } else {
                GatewayChatRole::Json {
                    value: role.clone(),
                    field,
                }
            }
        })
    };
    let content = if chat.input || !value.is_some_and(Value::is_object) {
        value
    } else {
        value.and_then(|v| v.get("content"))
    }
    .map(|v| match v {
        Value::String(s) => ChatContent::Text(s.clone()),
        v => ChatContent::Json(v.clone()),
    });
    let mut native_fields = Vec::new();
    for field in [
        "/__openclaw/id",
        "/idempotencyKey",
        "/__openclaw/idempotencyKey",
        "/timestamp",
        "/responseId",
        "/api",
        "/provider",
        "/model",
        "/responseModel",
        "/toolCallId",
        "/toolName",
        "/isError",
        "/stopReason",
        "/usage",
    ] {
        let pointer = format!("{path}{field}");
        if doc.value.pointer(&pointer).is_some() {
            native_fields.push(doc.field(&pointer));
        }
    }
    if let Some(parts) = value
        .and_then(|v| v.get("content"))
        .and_then(Value::as_array)
    {
        for (i, part) in parts.iter().enumerate() {
            if part.get("type").and_then(Value::as_str) == Some("toolCall") {
                for field in ["id", "name"] {
                    if part.get(field).is_some() {
                        native_fields.push(doc.field(&format!("{path}/content/{i}/{field}")));
                    }
                }
            }
        }
    }
    GatewayChatObservation {
        source: Arc::new(doc.connection.clone()),
        record_timestamp_ms: doc
            .value
            .pointer(&format!("{path}/__openclaw/recordTimestampMs"))
            .cloned(),
        transcript_position: doc
            .value
            .pointer(&format!("{path}/__openclaw/transcriptPosition"))
            .cloned(),
        state: if doc.value.get("type").and_then(Value::as_str) == Some("event") {
            doc.value.pointer("/payload/state").cloned()
        } else {
            None
        },
        reference: chat.reference,
        wrapper: chat.wrapper,
        history: chat.history,
        session,
        handoff,
        run_id: chat.run_id,
        message_run_id: chat.message_run_id,
        message_sequence: chat.message_sequence,
        wrapper_sequence: chat.wrapper_sequence,
        revision: chat.revision,
        native_fields,
        role,
        content,
    }
}
