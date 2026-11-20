//! Main-dispatch contracts. Functional modules own matching, facts, execution and messages.
use crate::{
    dispatch_event::{EventData, ProcessInfo},
    operation::{OperationReceipt, OriginalOperation},
};
pub use as_kernel_common::event_frame::{EventKind, EventStage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, oneshot};

/// Direct operation proposal; asking and its wait stay inside the rule module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    /// Permit this original operation.
    Allow,
    /// Deny this original operation.
    Deny {
        /// Source-owned termination after blocking the original operation.
        terminate: Termination,
    },
}

/// Exact termination scope, always subordinate to denying the current operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    /// Block only.
    None,
    /// Block and terminate the original actor instance.
    Actor,
    /// Block and terminate the original actor and its original physical parent.
    ActorAndParent,
}

/// Why a proposal was produced, independent of physical execution success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceReason {
    /// Direct matched rule.
    Direct,
    /// User answered the original request.
    User,
    /// Rule timeout, source stop or loss of the last interaction channel.
    Timeout,
    /// Only quick matching ran because no interaction channel was available.
    NoInteraction,
    /// No applicable rule.
    NoMatch,
}

/// One tagged version, so configuration and ordinary-rule hits cannot contradict their origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "version",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RuleSourceVersion {
    /// Fixed system self-protection rule from the actual driver configuration projection.
    Configuration(crate::configuration::DriverConfigVersion),
    /// Ordinary rule document version.
    RulesDocument(crate::RuleVersion),
}
/// One rule's stable identity and visible ordering at the actual hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleHit {
    /// Stable group identity.
    pub group_id: String,
    /// Visible group order.
    pub group_number: std::num::NonZeroU32,
    /// Original rule identity.
    pub rule_id: crate::RuleId,
    /// Visible order within the group.
    pub rule_number: std::num::NonZeroU32,
    /// Exact original semantics selected for this request.
    pub source: RuleSourceVersion,
    /// Explicit per-rule logging selection, carried unchanged to the message thread.
    pub log: bool,
    /// Fixed text from this exact rule version; None is not yet retrieved, Some("") is retrieved empty.
    pub deny_message: Option<String>,
}

/// An actual matching rule; None is reserved for the explicit no-match allow.
#[derive(Debug, Clone)]
pub struct RuleChoice {
    /// The original operation proposal.
    pub decision: Decision,
    /// Actual matched rule, if any.
    pub hit: Option<RuleHit>,
    /// Decision provenance; never a physical execution receipt.
    pub reason: ChoiceReason,
}

/// Fixed failure categories; no source bodies, credentials or raw errors in messages.
#[derive(Debug, Clone, Copy, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleError {
    /// Actual source does not implement the requested operation/condition/action.
    #[error("source capability unsupported")]
    Unsupported,
    /// The source's persistent generation or expected revision no longer matches.
    #[error("source generation conflict")]
    Conflict,
    /// Authenticated session lacks permission for this operation.
    #[error("operation not authorized")]
    Forbidden,
    /// The real communication peer was not authenticated/authorized for this endpoint.
    #[error("communication peer not authenticated")]
    Unauthenticated,
    /// Required module or its original source is unavailable.
    #[error("module unavailable")]
    Unavailable,
    /// Typed event or original operation binding is inconsistent.
    #[error("invalid module input")]
    InvalidInput,
    /// An input queue no longer accepts work.
    #[error("module stopped")]
    Stopped,
    /// The configured queue budget is occupied.
    #[error("module queue full")]
    Busy,
}

/// A quick pool result; missing facts are never a definite rule miss.
#[derive(Debug, Clone)]
pub enum FastMatch {
    /// A complete rule match determines this original operation.
    Matched(RuleChoice),
    /// A higher-priority candidate requires facts not yet available.
    NeedsFacts,
    /// All applicable quick rules were checked and did not match.
    NoMatch,
    /// The quick rules missed and current facts rule out every interactive candidate.
    NoReviewMatch,
}
/// One immutable selected process/file/network rule module for both stages.
#[async_trait]
pub trait RulesModule: Send + Sync {
    /// Whether an authenticated frontend/IM decision channel is currently available.
    fn can_prompt(&self) -> bool {
        false
    }
    /// Remembered and quick matching, plus a definite-miss check for interactive candidates.
    fn fast(&self, event: &EventData) -> Result<FastMatch, ModuleError>;
    /// Popup matching and asynchronous selection; None means no popup rule matched.
    async fn prompt(&self, request: ReviewRequest<'_>) -> Result<Option<RuleChoice>, ModuleError>;
}

/// Readonly original request window supplied to popup matching and waiting.
pub struct ReviewRequest<'a> {
    /// Per-request write-once hit provenance, retained by the host if waiting is cancelled.
    pub matched: &'a OnceLock<RuleHit>,
    /// Event after fast and optional slow filling.
    pub event: Arc<EventData>,
    /// Same source-owned operation used for binding, cancellation and eventual execution.
    pub operation: Arc<dyn OriginalOperation>,
}

/// Factory for the three derived rule modules. Selection occurs once per event.
pub trait RulesFactory: Send + Sync {
    /// Return the selected immutable rule module; do not reinterpret another pool.
    fn select(&self, kind: EventKind) -> Arc<dyn RulesModule>;
    /// Select the exact driver version; a factory without that version must refuse it.
    fn select_version(
        &self,
        kind: EventKind,
        version: Option<crate::RuleVersion>,
    ) -> Result<Arc<dyn RulesModule>, ModuleError> {
        if version.is_some() {
            return Err(ModuleError::Unavailable);
        }
        Ok(self.select(kind))
    }
}

/// Fast in-memory process/basic-fact owner; no blocking I/O on this interface.
pub trait FastFacts: Send + Sync {
    /// Invalidate supplements after an actual source or callback-queue gap.
    fn source_gap(&self);
    /// Release this admitted event's in-flight deduplication state after its last hit.
    fn finish_event(&self, event: crate::process_tree::EventKey);
    /// Observe the untouched source event and return instance-bound, event-valid process facts.
    /// Main merges only missing fields before matching; the provider cannot rewrite the source.
    fn prepare(&self, event: &EventData) -> Result<ProcessInfo, ModuleError>;
    /// Record one actual hit keyed by original event and process instance; deduplicate deliveries.
    fn record_hit(&self, event: &EventData, hit: &RuleHit);
    /// Consume disjoint native direct-match increments after preceding fast facts.
    fn record_direct_hits(&self, hits: Vec<crate::driver::DirectHitCount>);
}

/// Optional slow-information hook. Its fields and implementation belong to later modules.
#[async_trait]
pub trait SlowFacts: Send + Sync {
    /// Return supplemental process facts valid for this original event, without mutating it.
    /// Main merges only missing fields after successful completion, never on error/cancellation.
    async fn prepare(&self, event: &EventData) -> Result<ProcessInfo, ModuleError>;
}

/// Owned ingress data; observation and controlled-request stages remain distinct.
pub struct DispatchInput {
    /// Actual driver ToUser match; None is a definite driver miss, never a fabricated user hit.
    pub transfer_hit: Option<RuleHit>,
    /// The one common event model.
    pub event: Arc<EventData>,
    /// Original runtime-only operation; never serialized or reconstructed from PID.
    pub operation: Option<Arc<dyn OriginalOperation>>,
}

/// The actual result of core processing, not an audit or frontend-delivery ACK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchOutcome {
    /// A driver rule already decided its own check; there is no backend OriginalOperation.
    Driver {
        /// Confirmed result at that native check, independent of subsequent I/O.
        outcome: crate::operation::ExecutionOutcome,
        /// Actual per-target termination attempts at this same native check.
        termination: crate::operation::TerminationReport,
    },
    /// An observation has no operation control.
    Observed,
    /// Original execution outcome, including its original unknown/failure states.
    Execution(OperationReceipt),
}

/// Original completed event, asynchronously consumed by message/log submodules.
#[derive(Debug, Clone)]
pub struct MainNotice {
    /// Actual retained original's identity; direct decisions and observations have none.
    pub decision_id: Option<crate::DecisionId>,
    /// Shared event and filled fields.
    pub event: Arc<EventData>,
    /// Matching rule, or no matching rule.
    pub choice: Option<RuleChoice>,
    /// Real observation/execution result.
    pub result: Result<DispatchOutcome, ModuleError>,
}

/// A selected rule's structured security log record, before or after persistence.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleLogRequest {
    /// Original controlled request identity, if this log belongs to one.
    pub decision_id: Option<crate::DecisionId>,
    /// Original event with the facts used for matching.
    pub event: Arc<EventData>,
    /// Actual matched rule, retained even if Ask is cancelled before a choice exists.
    pub hit: RuleHit,
    /// Pending or resolved decision; cancellation never invents an Allow/Deny.
    pub decision: RuleLogDecision,
    /// Original execution result or explicit source error.
    pub result: Result<DispatchOutcome, ModuleError>,
}

/// Original security records carried by the one log queue and durable log store.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum SecurityLogEntry {
    /// Actual rule hit, decision and result.
    Rule(RuleLogRequest),
    /// Actual writable-file release; no rule or executable capability.
    FileClose(crate::security_log::FileCloseAuditRecord),
}

/// Decision state at the time a matched rule's log request is formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleLogDecision {
    /// The driver transferred this original; this is not its final Allow/Deny proposal.
    Transferred,
    /// Ask matched but no decision was produced before cancellation.
    Pending,
    /// An actual proposal exists, independently of its physical execution result.
    Resolved {
        /// Selected proposal.
        decision: Decision,
        /// Its actual provenance.
        reason: ChoiceReason,
    },
}

/// Public message values copied at a native submission; not current-execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeToolContext {
    /// Actual process lifetime at the native submission where these message values were copied.
    pub process: crate::process_tree::ProcessKey,
    /// Identifier from the captured public tool message, including its original compound form.
    pub tool_call_id: String,
    /// Name from the captured public tool message; not a current-execution claim.
    pub tool_name: String,
    /// Identifier read from the original assistant response.
    pub response_id: Option<String>,
    /// Original application execution scope.
    pub run_id: Option<String>,
    /// Original application session identifier.
    pub session_id: Option<String>,
    /// Original application session key.
    pub session_key: Option<String>,
    /// Original application agent identifier.
    pub agent_id: Option<String>,
    /// Provider label read from the assistant object.
    pub provider: Option<String>,
    /// API label read from the assistant object.
    pub api: Option<String>,
    /// Model label read from the assistant object.
    pub model: Option<String>,
    /// Actual tool arguments, preserving text, JSON, empty and missing values.
    pub arguments: Option<crate::flow::ChatContent>,
}

/// Query facts requiring an independently established current-tool execution handoff.
/// HTTP relation verification belongs to middleware; this is not a Main intake message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolExecutionFacts {
    /// Original system operation to which this native submission was traced.
    pub event: crate::process_tree::EventKey,
    /// Self-owned facts from the original tool context.
    pub context: NativeToolContext,
}

/// Messages consumed only on the third processing thread.
pub enum MainMessage {
    /// Actual source status observation, forwarded by the backend message consumer.
    DriverStatus(crate::driver_control::DriverStatus),
    /// Actual native transport loss; this is not an invented current driver state.
    DriverUnavailable,
    /// Ordinary completed-event distribution, independent of rule logging.
    Completed(Box<MainNotice>),
}

/// Original auxiliary native facts consumed on Messages independently of control-notice output.
#[expect(
    clippy::large_enum_variant,
    reason = "Original facts move through one awaited consumer without an extra per-fact allocation"
)]
pub enum NativeFact {
    /// Original native-library occurrence; unknown tool fields do not prevent delivery.
    RequestHandoff(crate::runtime_observation::NativeRequestHandoff),
    /// Return of a native syscall invocation containing the observed wait.
    RequestReturn(crate::driver::NativeRequestReturn),
    /// Actual private futex wake selection.
    FutexSelection(crate::driver::NativeFutexSelection),
}

/// Message thread entry; it may also consume its own frontend query queue.
#[async_trait]
pub trait MessageModule: Send + Sync {
    /// Drain completion and status notices to closure.
    async fn run(&self, notices: mpsc::Receiver<MainMessage>) -> Result<(), ModuleError>;
    /// Preserve one original native fact through its actual storage result, without a control-notice hop.
    async fn consume_native_fact(&self, fact: NativeFact) -> Result<(), ModuleError>;
    /// Consume the single passive TCP evidence stream on the existing Messages runtime.
    async fn consume_group(
        &self,
        _events: mpsc::Receiver<crate::driver::NativeGroupTcp>,
        _boot: [u8; 16],
    ) -> Result<(), ModuleError> {
        Err(ModuleError::Unsupported)
    }
    /// Consume Flow's unique original chat queue on the same Messages runtime.
    /// Business storage must continue without a frontend and drain queued facts to EOF.
    async fn consume_chat(
        &self,
        events: crate::flow::ChatReceiver,
        feedback: Option<Arc<dyn crate::flow::FlowFeedbackPort>>,
    ) -> Result<(), ModuleError>;
}

/// The sole consumer of the existing security-log queue.
#[async_trait]
pub trait SecurityLogSink: Send {
    /// Retain the storage owner through actual drain and close.
    async fn run(self: Box<Self>, logs: mpsc::Receiver<SecurityLogEntry>) -> std::io::Result<()>;
}

/// Components are supplied directly; no registry, service locator or placeholder implementation.
#[derive(Clone)]
pub struct MainModules {
    /// The three-pool rule factory.
    pub rules: Arc<dyn RulesFactory>,
    /// Existing fast memory facts.
    pub fast_facts: Arc<dyn FastFacts>,
    /// Explicitly reserved slow information, when implemented.
    pub slow_facts: Option<Arc<dyn SlowFacts>>,
    /// Frontend messages.
    pub messages: Arc<dyn MessageModule>,
}

/// Completion for one admitted input. Source failures are not implicit allows.
pub type DispatchReply = oneshot::Receiver<Result<DispatchOutcome, ModuleError>>;
