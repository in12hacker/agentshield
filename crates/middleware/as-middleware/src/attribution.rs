//! Read-only comparison of original operations and observed model tool calls.
use as_types::{
    dispatch_event::{EventData, EventPayload},
    event_operation::ProcessOperation,
    flow::{ChatContent, ChatMessageId, ChatPartRef},
    process_tree::{EventKey, ProcessKey},
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Identity condition applied to the actual model connection initiator.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorScope {
    /// Same lifetime as the operation's calling process.
    SameProcess,
    /// Same lifetime as the operation's recorded physical parent.
    PhysicalParent,
    /// Same process, TID, and thread birth as the operation's calling thread.
    SameThread,
}

/// Explicit argument mapping selected by the caller.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetPattern {
    /// One original argument of the requested exec image.
    ExecArgument { pointer: String, index: u32 },
    /// Original requested executable name.
    Executable { pointer: String },
    /// Original primary file path.
    FilePath { pointer: String },
    /// Original network destination address and port.
    NetworkPeer {
        address_pointer: String,
        port_pointer: String,
    },
}

/// Explicit conditions for comparing retained evidence.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionPattern {
    pub actor: ActorScope,
    #[serde(with = "as_types::decimal")]
    pub lookback_ns: u64,
    pub target: TargetPattern,
}

/// Caller-selected pattern and page for an Ask context request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionRequest {
    pub pattern: AttributionPattern,
    pub after: Option<ChatMessageId>,
    pub limit: u16,
}

/// Original event target used for comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObservedTarget {
    Bytes { value: Vec<u8> },
    NetworkPeer { address: IpAddr, port: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetComparison {
    Equal,
    Different,
    EventTargetUnavailable,
    ArgumentUnavailable,
    SourceIncomplete,
}

/// A tool call that meets the identity and time conditions, with its target comparison.
#[derive(Debug, Clone, Serialize)]
pub struct AttributionCandidate {
    pub call: ChatPartRef,
    #[serde(with = "as_types::decimal")]
    pub message_revision: u64,
    #[serde(with = "as_types::decimal")]
    pub response_observed_ns: u64,
    pub target_comparison: TargetComparison,
}

/// One read-only page of evidence comparisons. It carries no execution authority.
#[derive(Debug, Clone, Serialize)]
pub struct ToolAttribution {
    pub event: EventKey,
    pub pattern: AttributionPattern,
    /// Required connection identity, taken from the original event and actor condition.
    pub required_connection_process: ProcessKey,
    /// Required thread identity for SameThread; not an independently observed connection.
    pub required_connection_thread: Option<ThreadIdentity>,
    #[serde(with = "as_types::decimal")]
    pub event_boottime_ns: u64,
    pub event_target: Option<ObservedTarget>,
    pub candidates: Vec<AttributionCandidate>,
    pub next_after: Option<ChatMessageId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadIdentity {
    pub tid: u32,
    #[serde(with = "as_types::decimal")]
    pub birth_boottime_ns: u64,
}

impl TargetPattern {
    pub(crate) fn applies(&self, event: &EventData) -> bool {
        let valid = |p: &str| {
            (p.is_empty() || p.starts_with('/'))
                && p.split('~')
                    .skip(1)
                    .all(|tail| matches!(tail.as_bytes().first(), Some(b'0' | b'1')))
        };
        match (self, &event.payload) {
            (
                Self::ExecArgument { pointer, .. } | Self::Executable { pointer },
                EventPayload::Process(_),
            ) => event.header().operation == ProcessOperation::Execute as u32 && valid(pointer),
            (Self::FilePath { pointer }, EventPayload::File(_)) => valid(pointer),
            (
                Self::NetworkPeer {
                    address_pointer,
                    port_pointer,
                },
                EventPayload::Network(_),
            ) => valid(address_pointer) && valid(port_pointer),
            _ => false,
        }
    }

    pub(crate) fn observed_target(&self, event: &EventData) -> Option<ObservedTarget> {
        let value = match (self, &event.payload) {
            (Self::ExecArgument { index, .. }, EventPayload::Process(data)) => {
                data.target_argv.as_ref()?.get(*index as usize)?
            }
            (Self::Executable { .. }, EventPayload::Process(data)) => {
                data.target_executable.as_ref()?
            }
            (Self::FilePath { .. }, EventPayload::File(data)) => data.path.as_ref()?,
            (Self::NetworkPeer { .. }, EventPayload::Network(data)) => {
                return Some(ObservedTarget::NetworkPeer {
                    address: data.remote_ip?,
                    port: data.remote_port?,
                });
            }
            _ => return None,
        };
        Some(ObservedTarget::Bytes {
            value: value.clone(),
        })
    }

    pub(crate) fn compare(
        &self,
        target: Option<&ObservedTarget>,
        arguments: Option<&ChatContent>,
    ) -> TargetComparison {
        let Some(target) = target else {
            return TargetComparison::EventTargetUnavailable;
        };
        let text = |pointer: &str| match arguments {
            Some(ChatContent::Json(value)) => value.pointer(pointer)?.as_str(),
            Some(ChatContent::Text(value)) if pointer.is_empty() => Some(value.as_str()),
            _ => None,
        };
        let equal = match (self, target) {
            (
                Self::ExecArgument { pointer, .. }
                | Self::Executable { pointer }
                | Self::FilePath { pointer },
                ObservedTarget::Bytes { value },
            ) => text(pointer).map(|candidate| candidate.as_bytes() == value),
            (
                Self::NetworkPeer {
                    address_pointer,
                    port_pointer,
                },
                ObservedTarget::NetworkPeer { address, port },
            ) => {
                let ip = text(address_pointer).and_then(|value| value.parse::<IpAddr>().ok());
                let candidate_port = match arguments {
                    Some(ChatContent::Json(value)) => value
                        .pointer(port_pointer)
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u16::try_from(value).ok()),
                    _ => None,
                };
                ip.zip(candidate_port)
                    .map(|(ip, candidate_port)| ip == *address && candidate_port == *port)
            }
            _ => None,
        };
        match equal {
            Some(true) => TargetComparison::Equal,
            Some(false) => TargetComparison::Different,
            None => TargetComparison::ArgumentUnavailable,
        }
    }
}
