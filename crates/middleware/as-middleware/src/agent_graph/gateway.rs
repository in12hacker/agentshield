//! Public Gateway event fields; transport acquisition and persistence have separate owners.
use crate::gateway::GatewayDocument;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// An accepted public tool result; these are the original Gateway identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AcceptedSpawn {
    pub(crate) parent_run_id: String,
    pub(crate) parent_session_key: String,
    pub(crate) parent_agent_id: String,
    pub(crate) tool_call_id: String,
    pub(crate) child_run_id: String,
    pub(crate) child_session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sequence: Option<u64>,
}

impl AcceptedSpawn {
    pub(crate) fn same_delegation(&self, other: &Self) -> bool {
        self.parent_run_id == other.parent_run_id
            && self.parent_session_key == other.parent_session_key
            && self.parent_agent_id == other.parent_agent_id
            && self.tool_call_id == other.tool_call_id
            && self.child_run_id == other.child_run_id
            && self.child_session_key == other.child_session_key
    }
}

/// Public terminal sources retain their distinct original vocabularies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TerminalSource {
    Lifecycle,
    SessionProjection,
}

/// An explicit settlement or exact-session terminal projection, never a model response end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChildTerminal {
    pub(crate) source: TerminalSource,
    pub(crate) run_id: String,
    pub(crate) session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) phase: Option<String>,
    // A present JSON null remains Some(Value::Null), independently of absent properties.
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) status: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) outcome: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) aborted: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) stop_reason: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) execution_settled: Option<bool>,
}

fn present_value<'de, D: Deserializer<'de>>(value: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(value).map(Some)
}

fn acquired_conflict<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    matches!((left, right), (Some(a), Some(b)) if a != b)
}

fn acquired_value_conflict(left: &Option<Value>, right: &Option<Value>) -> bool {
    match (left, right) {
        (Some(a), Some(b)) if !a.is_null() && !b.is_null() => a != b,
        _ => false,
    }
}

impl ChildTerminal {
    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        self.run_id != other.run_id
            || self.session_key != other.session_key
            || acquired_conflict(&self.agent_id, &other.agent_id)
            || acquired_conflict(&self.session_id, &other.session_id)
            || (self.source == other.source
                && (acquired_conflict(&self.phase, &other.phase)
                    || acquired_value_conflict(&self.status, &other.status)
                    || acquired_value_conflict(&self.outcome, &other.outcome)
                    || acquired_value_conflict(&self.aborted, &other.aborted)
                    || acquired_value_conflict(&self.stop_reason, &other.stop_reason)))
    }
}

/// A public parent announce result, distinct from durable delivery or child settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ParentPublication {
    pub(crate) run_id: String,
    pub(crate) session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sequence: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) yielded: Option<Value>,
}

impl ParentPublication {
    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        self.run_id != other.run_id
            || self.session_key != other.session_key
            || acquired_conflict(&self.agent_id, &other.agent_id)
            || acquired_value_conflict(&self.yielded, &other.yielded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum GatewayFact {
    AcceptedSpawn(AcceptedSpawn),
    ChildTerminal(ChildTerminal),
    ParentPublication(ParentPublication),
}

impl GatewayFact {
    pub(crate) fn index(&self) -> (&'static str, &str, &str) {
        match self {
            Self::AcceptedSpawn(fact) => (
                "accepted_spawn",
                &fact.child_run_id,
                &fact.child_session_key,
            ),
            Self::ChildTerminal(fact) => ("child_terminal", &fact.run_id, &fact.session_key),
            Self::ParentPublication(fact) => {
                ("parent_publication", &fact.run_id, &fact.session_key)
            }
        }
    }
}

fn required(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn optional(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

/// Group alone interprets delegation and terminal fields from the shared document.
pub(crate) fn project(document: &GatewayDocument<'_>) -> Option<GatewayFact> {
    if document.reference.from_client {
        return None;
    }
    project_value(document.value)
}

fn project_value(event: &Value) -> Option<GatewayFact> {
    if event.get("type")?.as_str()? != "event" {
        return None;
    }
    let payload = event.get("payload")?;
    let event_sequence = event.get("seq").and_then(Value::as_u64);
    let sequence = payload.get("seq").and_then(Value::as_u64);
    match event.get("event")?.as_str()? {
        "agent" if payload.get("stream")?.as_str()? == "tool" => {
            let data = payload.get("data")?;
            if data.get("name")?.as_str()? != "sessions_spawn"
                || data.get("phase")?.as_str()? != "result"
                || data.get("isError")?.as_bool()?
            {
                return None;
            }
            let details = data.get("result")?.get("details")?;
            if details.get("status")?.as_str()? != "accepted" {
                return None;
            }
            let fact = AcceptedSpawn {
                parent_run_id: required(payload, "runId")?,
                parent_session_key: required(payload, "sessionKey")?,
                parent_agent_id: required(payload, "agentId")?,
                tool_call_id: required(data, "toolCallId")?,
                child_run_id: required(details, "runId")?,
                child_session_key: required(details, "childSessionKey")?,
                event_sequence,
                sequence,
            };
            (fact.parent_run_id != fact.child_run_id
                && fact.parent_session_key != fact.child_session_key)
                .then_some(GatewayFact::AcceptedSpawn(fact))
        }
        "agent" if payload.get("stream")?.as_str()? == "lifecycle" => {
            let data = payload.get("data")?;
            let phase = required(data, "phase")?;
            if !matches!(phase.as_str(), "end" | "error")
                || data.get("executionSettled").and_then(Value::as_bool) != Some(true)
                || data.get("yielded").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            Some(GatewayFact::ChildTerminal(ChildTerminal {
                source: TerminalSource::Lifecycle,
                run_id: required(payload, "runId")?,
                session_key: required(payload, "sessionKey")?,
                agent_id: optional(payload, "agentId"),
                session_id: optional(payload, "sessionId"),
                event_sequence,
                sequence,
                phase: Some(phase),
                status: data.get("status").cloned(),
                outcome: data.get("outcome").cloned(),
                aborted: data.get("aborted").cloned(),
                stop_reason: data.get("stopReason").cloned(),
                execution_settled: Some(true),
            }))
        }
        "sessions.changed" => {
            let session = payload.get("session")?;
            let status = session.get("status")?;
            if !matches!(
                status.as_str()?,
                "done" | "failed" | "interrupted" | "killed" | "timeout"
            ) {
                return None;
            }
            Some(GatewayFact::ChildTerminal(ChildTerminal {
                source: TerminalSource::SessionProjection,
                run_id: required(session, "lastRunId")?,
                session_key: required(session, "key")?,
                agent_id: optional(session, "agentId"),
                session_id: optional(session, "sessionId"),
                event_sequence,
                sequence,
                phase: None,
                status: Some(status.clone()),
                outcome: None,
                aborted: None,
                stop_reason: None,
                execution_settled: None,
            }))
        }
        "chat" if payload.get("state")?.as_str()? == "final" => {
            let run_id = required(payload, "runId")?;
            if !run_id.starts_with("announce:v1:") {
                return None;
            }
            Some(GatewayFact::ParentPublication(ParentPublication {
                run_id,
                session_key: required(payload, "sessionKey")?,
                agent_id: optional(payload, "agentId"),
                event_sequence,
                sequence,
                yielded: payload.get("yielded").cloned(),
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(text: &str) -> Option<GatewayFact> {
        project_value(&as_types::json::parse(text.as_bytes()).ok()?)
    }

    #[test]
    fn terminal_requires_exact_public_source_and_explicit_settlement() {
        let mut event = json!({"type":"event","event":"agent","payload":{
            "runId":"child","sessionKey":"child-session","stream":"lifecycle",
            "data":{"phase":"end","executionSettled":true,"status":null,
                "outcome":{"empty":"","items":["",0,false]},"aborted":false,"stopReason":""}}});
        let Some(GatewayFact::ChildTerminal(terminal)) = parse(&event.to_string()) else {
            panic!("explicit terminal must be retained");
        };
        let stored = GatewayFact::ChildTerminal(terminal.clone());
        let restored: GatewayFact =
            serde_json::from_str(&serde_json::to_string(&stored).expect("fact JSON"))
                .expect("typed roundtrip");
        assert!(matches!(
            restored,
            GatewayFact::ChildTerminal(ChildTerminal {
                status: Some(Value::Null),
                ..
            })
        ));
        let mut later_status = terminal.clone();
        later_status.status = Some(json!("ok"));
        assert!(!terminal.conflicts(&later_status));
        assert_eq!(terminal.status, Some(Value::Null));
        assert_eq!(terminal.aborted, Some(json!(false)));
        assert_eq!(terminal.stop_reason, Some(json!("")));
        assert_eq!(
            terminal.outcome,
            Some(json!({"empty":"","items":["",0,false]}))
        );
        event["payload"]["data"]["executionSettled"] = json!(false);
        assert!(parse(&event.to_string()).is_none());
        event["payload"]["data"]["executionSettled"] = json!(true);
        event["payload"]["data"]["yielded"] = json!(true);
        assert!(parse(&event.to_string()).is_none());
        event["payload"]["data"]["yielded"] = json!(false);
        event["payload"]["data"]["phase"] = json!("finishing");
        assert!(parse(&event.to_string()).is_none());
    }

    #[test]
    fn duplicate_object_keys_never_choose_a_gateway_identity() {
        assert!(parse(r#"{"type":"event","event":"chat","payload":{"state":"final","runId":"announce:v1:child:run","sessionKey":"first","sessionKey":"second"}}"#).is_none());
    }

    #[test]
    fn session_status_without_the_same_rows_last_run_identity_is_not_terminal() {
        let mut event = json!({"type":"event","event":"sessions.changed","payload":{
            "runId":"unrelated","session":{"key":"child-session","status":"done"}}});
        assert!(parse(&event.to_string()).is_none());
        event["payload"]["session"]["lastRunId"] = json!("child");
        let Some(GatewayFact::ChildTerminal(terminal)) = parse(&event.to_string()) else {
            panic!("exact session terminal must be retained");
        };
        assert_eq!(terminal.run_id, "child");
        assert_eq!(terminal.source, TerminalSource::SessionProjection);
        event["payload"]["session"]["status"] = json!("idle");
        assert!(parse(&event.to_string()).is_none());
    }
}
