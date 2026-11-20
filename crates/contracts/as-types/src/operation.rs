//! Runtime-only original execution capability, independent of serialized event data.
use crate::{
    DecisionId,
    dispatch_event::EventData,
    hold_budget::HoldWindow,
    main_ports::{Decision, ModuleError, RuleChoice, RuleHit, Termination},
};
use async_trait::async_trait;
use tokio::sync::watch;
/// The original source's retirement reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Planned retirement; the selected rule's stop semantics apply.
    Orderly,
    /// The original source failed.
    SourceFailure,
}
/// Outcome actually observed by the original executor, never inferred from admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    /// Original source confirmed allow.
    Allowed,
    /// Original source confirmed deny.
    Denied,
    /// Original request was cancelled.
    Cancelled,
    /// This Linux original returned an actual negative errno, without classifying its stage or source health.
    Failed {
        /// Negative Linux errno returned by the same original operation.
        native_error: i32,
    },
    /// Source has not confirmed a physical result.
    Unknown,
}
/// One actual signal submission result, independent of the original check's deny result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SignalOutcome {
    /// This target was not selected for termination.
    NotRequested,
    /// The source accepted SIGKILL, without claiming a completed exit.
    SignalSent,
    /// The pinned target no longer has a signal recipient.
    NotRunning,
    /// Native signal submission failed; preserve its actual negative error code.
    Failed {
        /// Error in the reporting source platform's native domain.
        native_error: i32,
    },
}
/// Exact selected scope and the two independent signal results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminationReport {
    /// Scope actually committed by the original executor.
    pub requested: Termination,
    /// Original actor instance result.
    pub actor: SignalOutcome,
    /// Physical parent captured with the original, never a later replacement.
    pub parent: SignalOutcome,
}
impl TerminationReport {
    /// Explicit block/allow without either termination attempt.
    pub const NONE: Self = Self {
        requested: Termination::None,
        actor: SignalOutcome::NotRequested,
        parent: SignalOutcome::NotRequested,
    };
}
/// Receipt from the same bound operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationReceipt {
    /// Original source identity.
    pub decision_id: DecisionId,
    /// Actual observed result; submission alone is not confirmation.
    pub outcome: ExecutionOutcome,
    /// Actual signal results; failures do not turn a confirmed deny into an allow.
    pub termination: TerminationReport,
}
/// Exact Ask fallback accepted by the original executor before a question is published.
#[derive(Debug, Clone)]
pub struct ReviewBinding {
    /// Original matching rule and version.
    pub hit: RuleHit,
    /// Effective source/rule deadline; it may only shorten the original window.
    pub deadline: tokio::time::Instant,
    /// Action to apply on timeout or loss of the control session.
    pub timeout: Decision,
}
/// Capability supplied by the execution module for exactly one original request.
#[async_trait]
pub trait OriginalOperation: std::fmt::Debug + Send + Sync {
    /// Nonblocking read of this same original's confirmed completion, without a decision.
    fn try_result(&self) -> Option<Result<OperationReceipt, ModuleError>>;
    /// Await or retrieve this original's actual completion without submitting a new decision.
    /// Queue expiry or cancellation must not discard a result the source already produced.
    async fn result(&self) -> Result<OperationReceipt, ModuleError>;
    /// Stable identity assigned by that original source.
    fn decision_id(&self) -> DecisionId;
    /// Kernel-selected rule version, when this request follows driver matching.
    fn policy_version(&self) -> Option<crate::RuleVersion> {
        None
    }
    /// Install the original Ask fallback before exposing a user question.
    async fn bind_review(&self, binding: ReviewBinding) -> Result<(), ModuleError>;
    /// Lifetime captured before queueing.
    fn hold_window(&self) -> HoldWindow;
    /// Original source retirement signal, when supported.
    fn stop_signal(&self) -> Option<watch::Receiver<Option<StopReason>>> {
        None
    }
    /// Verify this event still describes the original live source and target.
    fn validate(&self, event: &EventData) -> Result<(), ModuleError>;
    /// Apply at the original release point and retain started work until its real return.
    async fn apply(
        &self,
        event: &EventData,
        choice: &RuleChoice,
    ) -> Result<OperationReceipt, ModuleError>;
    /// Request source-owned cancellation; this does not claim completion.
    fn cancel(&self);
}
