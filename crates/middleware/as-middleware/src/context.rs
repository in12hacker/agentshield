//! Shared immutable activity evidence required by execution feedback, without a second fact store.
//!
//! These values are supplied by the trusted in-process activity producer. They deliberately
//! have no deserializer: client messages cannot register an execution relation or completion.
//! The generic tool-to-operation execution-context producer is not implemented here.

use as_main::messages::protocol::OperationResult;
use as_main::messages::protocol::Up;
use as_types::{
    DecisionId,
    flow::{
        ChatMessage, ChatMessageId, ChatPartRef, ProtocolObservation, ProtocolRecordId,
        RuleFeedbackFact,
    },
    process_tree::{EventKey, ProcessKey},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Reference to an original fact, retaining its complete native identifier domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FactReference {
    /// Original source event; neither a PID nor a decision identifier.
    Event(EventKey),
    /// Original runtime observation, distinct from an LSM event.
    NativeObservation(EventKey),
    /// Original host process lifetime; requires an actual process-query source.
    Process(ProcessKey),
    /// Original protocol observation in its actual HTTP exchange.
    Protocol(ProtocolRecordId),
    /// Original Flow message projection in its actual HTTP exchange.
    Message(ChatMessageId),
    /// Message-local content position, not a provider-global tool identifier.
    Part(ChatPartRef),
    /// Actual received Main question, never inferred from a completion's decision ID.
    Question(DecisionId),
    /// Actual received Main operation result for the original event.
    Completion(EventKey),
}

/// Evidence kind determines whether a relation is an explicit reference or a temporal pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// Request and response within the same explicit exchange.
    SameExchange,
    /// Native call/result protocol references, without a kernel-execution claim.
    NativeToolReference,
    /// Original ToolCall part to an event or reported result, supported only as a candidate.
    /// This relation never supplies ToolExecution or authorizes denial-feedback injection.
    ToolReferenceCandidate,
    /// Same original container instance and boot, with the model observation no later than the event.
    /// This is a workload/time candidate, not a process ancestry or tool-execution proof.
    ToolContainerCandidate,
    /// An actual question and its original completion.
    QuestionCompletion,
    /// Explicit execution bridge: an original ToolCall part to an original event.
    ToolExecution,
    /// A raw request field explicitly names its opening observation.
    NativeRequestReference,
    /// An observed enclosing native API; it does not prove ownership of flushed queue entries.
    NativeCallScope,
    /// The next retained same-thread WorkLeave and matching Done result; a temporal pattern.
    SameThreadReturnSequence,
}
impl RelationKind {
    pub(crate) fn is_tool_candidate(self) -> bool {
        matches!(
            self,
            Self::ToolReferenceCandidate | Self::ToolContainerCandidate
        )
    }
}

/// Evidence-backed relationship; execution feedback checks that every reference is present.
///
/// `ToolExecution` has `from = Part(call)` and `to = Event(original_event)`.
/// Evidence must be nonempty. Merely putting two facts in a view never creates this relation.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRelation {
    /// Actual relationship kind, not a confidence score.
    pub kind: RelationKind,
    /// Original relationship source.
    pub from: FactReference,
    /// Original relationship target.
    pub to: FactReference,
    /// Original evidence references supplied by the trusted producer.
    pub evidence: Vec<FactReference>,
}

/// Explicit limit of the supplied activity view; absence is not evidence of success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextGapReason {
    /// No explicit source bound the original operation to an Agent root instance.
    AgentBindingUnavailable,
    /// No explicit tool-argument/argv field mapping was supplied for a content check.
    CommandMappingUnavailable,
    /// The explicitly mapped values do not provide a comparable string/argv pair.
    CommandInputUnavailable,
    /// The producer did not supply the original fact.
    NotProvided,
    /// The original fact was actually evicted or expired.
    NotRetained,
    /// The source could not supply the original fact.
    SourceUnavailable,
    /// No explicit tool-execution bridge was supplied.
    NoExplicitRelation,
    /// Supplied identifiers or original values contradict one another.
    ConflictingEvidence,
}

/// Missing or conflicting original reference; no raw body or secret is copied into the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextGap {
    /// Original fact whose relationship or source is unavailable.
    pub at: FactReference,
    /// Actual category of the missing evidence.
    pub reason: ContextGapReason,
}

/// Shared immutable original records consumed by execution feedback, without rewritten DTO fields.
///
/// Questions are original received Up::Ask values. A process-query reference still cannot be
/// satisfied by substituting an event's process attributes or a completion's decision ID.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceRecord {
    /// Retained original available by reference; absence from this response is not source loss.
    Deferred(FactReference),
    /// A page of an original protocol body, preserving absolute segment offsets and source metadata.
    ProtocolPage {
        observation: Arc<ProtocolObservation>,
        #[serde(with = "as_types::decimal::optional")]
        next_offset: Option<u64>,
    },
    /// Raw native-library lifecycle facts; only an actual EventBound supplies an LSM reference.
    RequestHandoff(Arc<as_types::runtime_observation::NativeRequestHandoff>),
    /// The original native request return, independent of the gate receipt.
    RequestReturn(Arc<as_types::driver::NativeRequestReturn>),
    /// Private futex selection. The original waiter proof may be absent.
    FutexSelection(Arc<as_types::driver::NativeFutexSelection>),
    /// Tool query facts requiring independent execution authority, beyond public message candidates.
    ToolExecution(Arc<as_types::main_ports::ToolExecutionFacts>),
    /// Original received question; consumers validate that this is an actual Up::Ask.
    Question(Arc<Up>),
    /// Original Main operation-result value.
    OperationResult(Arc<OperationResult>),
    /// Original Flow protocol observation, including its native ID provenance.
    Protocol(Arc<ProtocolObservation>),
    /// Original Flow message projection, including its actual record references.
    Message(Arc<ChatMessage>),
    /// Actual retained Flow feedback stage for this original denial and tool call.
    RuleFeedback(Arc<RuleFeedbackFact>),
}
impl SourceRecord {
    /// Share an original received fact; deltas do not masquerade as full message snapshots.
    pub fn from_up(up: Arc<Up>) -> Option<Self> {
        match up.as_ref() {
            Up::Ask { .. } => Some(Self::Question(up)),
            Up::OperationResult(result) => Some(Self::OperationResult(result.clone())),
            Up::Chat { envelope, .. } => match envelope.event.as_ref() {
                as_types::flow::ChatEvent::Observation(p) => Some(Self::Protocol(p.clone())),
                as_types::flow::ChatEvent::Started(m) | as_types::flow::ChatEvent::Finished(m) => {
                    Some(Self::Message(m.clone()))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// Shared immutable activity view passed explicitly to a consumer; no global context or mutable cache.
#[derive(Debug, Clone, Serialize)]
pub struct ContextView {
    /// Original query anchor, independent of the relationship's direction.
    pub anchor: FactReference,
    /// Actual retained source values, not fabricated records for an absent bridge.
    pub source_records: Vec<SourceRecord>,
    /// Evidence relationships, with the meaning and strength defined by their kind.
    pub relations: Vec<EvidenceRelation>,
    /// Known source/relationship gaps retained as supplied.
    pub gaps: Vec<ContextGap>,
}
impl ContextView {
    /// Keep heavy immutable originals addressable, without embedding them in every business view.
    pub(crate) fn defer_originals(&mut self) {
        for record in &mut self.source_records {
            let reference = match record {
                SourceRecord::Protocol(value) => Some(FactReference::Protocol(value.id)),
                SourceRecord::RequestHandoff(value) => {
                    Some(FactReference::NativeObservation(value.observation))
                }
                _ => None,
            };
            if let Some(reference) = reference {
                *record = SourceRecord::Deferred(reference);
            }
        }
    }
}
