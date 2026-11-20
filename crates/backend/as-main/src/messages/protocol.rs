//! Current middleware DTOs. Driver binary framing has its own lower port; no legacy DTO decoder.
use as_policy_engine::{
    PolicySnapshot, RuleWriteReceipt,
    model::{PolicyDocument, RuleChange, RuleSnapshot},
};
use as_runtime_config::ConfigSnapshot;
use as_security_log::{SecurityLogPage, SecurityLogQuery};
use as_types::{
    DecisionId, RuleVersion,
    configuration::{ConfigDocument, ConfigVersion, ConfigWriteReceipt, ProtectionKind},
    dispatch_event::EventData,
    driver_control::DriverStatus,
    main_ports::{ChoiceReason, Decision, DispatchOutcome, MainNotice, ModuleError, RuleHit},
    operation::ExecutionOutcome,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
/// Exact current middleware revision.
pub const VERSION: u16 = 2;
/// Maximum encoded JSON bytes in the current local message transport.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Middle-tier command correlation, distinct from driver request/original identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandId(pub [u8; 16]);
/// Fixed-source configuration management, never a path-based write API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigCommand {
    /// Read one consistent saved snapshot.
    Get,
    /// Validate and save the complete current document.
    Update {
        /// Saved version the editor read.
        expected_version: ConfigVersion,
        /// Complete replacement content, without path overrides.
        document: Box<ConfigDocument>,
    },
    /// Read the same bound source again without writing it.
    Reload {
        /// Cached version the caller expects to replace.
        expected_version: ConfigVersion,
    },
}
/// Ordinary rules only; system self-protection is owned by configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleCommand {
    /// Read the rule owner's current snapshot.
    Get,
    /// Read one rule by its document-wide identity from one consistent snapshot.
    GetRule {
        /// Exact rule identity, independent of its matching order.
        rule_id: String,
    },
    /// Edit one ordinary or remembered group/rule through the same source writer.
    Edit {
        /// Saved version the editor read.
        expected_version: RuleVersion,
        /// Explicit typed mutation; no inferred group, scope or conditions.
        change: Box<RuleChange>,
    },
    /// Save ordinary rules through their sole writer.
    Update {
        /// Saved version the editor read.
        expected_version: RuleVersion,
        /// Complete typed ordinary rule document.
        document: PolicyDocument,
    },
    /// Reload the one bound source without a write receipt.
    Reload {
        /// Expected current snapshot version.
        expected_version: RuleVersion,
    },
}
/// Common controls whose actual driver results must be forwarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlCommand {
    /// Query the actual connected driver.
    GetStatus,
    /// Set a temporary ordinary-category override.
    SetRuntimeFeature {
        /// Ordinary protection category.
        feature: ProtectionKind,
        /// Explicit temporary value, never persisted.
        enabled: bool,
    },
    /// Remove the override and use the current persistent default.
    ResumeRuntimeFeature {
        /// Ordinary protection category.
        feature: ProtectionKind,
    },
    /// Ask this instance's bound host to take ownership of retirement, without claiming exit.
    RequestStop,
}
/// Authenticated normalized requests; no client-selected source, identity or permission fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessCommand {
    /// Read-only memory query.
    Query {
        /// Exact bounded query.
        query: Box<as_types::process_tree::ProcessQuery>,
    },
    /// Managed OS refresh; caller cannot choose a filesystem path.
    Refresh {
        /// Exact original instance or full host.
        target: as_types::process_tree::RefreshTarget,
    },
}
/// Authenticated normalized requests; no client-selected source, identity or permission fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowCommand {
    /// Actual current mediation counters, without historical content.
    Status,
}
/// Authenticated normalized requests; no client-selected source, identity or permission fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Down {
    /// Actual TLS status or retained content query.
    Flow {
        /// Same command correlation as other queries.
        id: CommandId,
        /// Read-only module operation.
        command: FlowCommand,
    },
    /// Queries and explicit refresh of the one main-owned process tree.
    Processes {
        /// Original correlation.
        id: CommandId,
        /// Tree-owned typed operation.
        command: ProcessCommand,
    },
    /// Page the original committed security log through its owner.
    SecurityLog {
        /// Original correlation.
        id: CommandId,
        /// Bounded exclusive-row query.
        command: SecurityLogQuery,
    },
    /// Fixed-source configuration management.
    Configuration {
        /// Original command correlation.
        id: CommandId,
        /// Configuration-owned operation.
        command: ConfigCommand,
    },
    /// Ordinary rule management.
    Rules {
        /// Original command correlation.
        id: CommandId,
        /// Rule-owned operation.
        command: RuleCommand,
    },
    /// Common controls routed by main.
    Control {
        /// Original command correlation.
        id: CommandId,
        /// Driver or host-owned operation.
        command: ControlCommand,
    },
    /// Reply to the original still-live question on this session.
    Answer {
        /// Identity from the actual question.
        decision_id: DecisionId,
        /// User proposal; never an execution receipt.
        decision: Decision,
    },
    /// Real ability to deliver questions and return answers.
    ReviewAvailability {
        /// Availability of this authenticated transport.
        available: bool,
    },
}
impl Down {
    /// Original reply correlation, present only for request/reply commands.
    pub fn command_id(&self) -> Option<CommandId> {
        match self {
            Self::Flow { id, .. }
            | Self::Processes { id, .. }
            | Self::SecurityLog { id, .. }
            | Self::Configuration { id, .. }
            | Self::Rules { id, .. }
            | Self::Control { id, .. } => Some(*id),
            Self::Answer { .. } | Self::ReviewAvailability { .. } => None,
        }
    }
}
/// Real command evidence. A persistence receipt is never called kernel Applied.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplyData {
    /// The current host's one-shot stop port actually accepted this request, not an exit receipt.
    MainStopRequested,
    /// Actual runtime counters, independent of stored configuration.
    FlowStatus {
        /// Live report from the same actual ingress owner.
        ingress: Option<as_types::driver::TlsIngressStatus>,
        /// Current runtime report.
        status: as_types::flow::FlowStatus,
    },

    /// Real in-memory process-tree result.
    Processes {
        /// Tree-owned query output.
        result: as_process_tree::QueryReply,
    },
    /// Original committed security log records.
    SecurityLog {
        /// Page returned by the one durable owner.
        page: SecurityLogPage,
    },
    /// Real completed scan and consumed native fence.
    ProcessesRefreshed {
        /// Actual refresh evidence.
        receipt: as_process_tree::ScanReceipt,
    },
    /// Same fixed source reloaded and required native adoption actually confirmed.
    ConfigurationReloaded {
        /// Actual reloaded saved content.
        snapshot: Arc<ConfigSnapshot>,
        /// Native application evidence; not a send acknowledgement.
        applied: DriverStatus,
    },
    /// Same ordinary rule source reloaded and its native generation confirmed.
    RulesReloaded {
        /// Actual reloaded policy.
        snapshot: PolicySnapshot,
        /// Actual source versions and state.
        applied: DriverStatus,
    },
    /// Saved configuration read, including the original readonly paths.
    Configuration {
        /// One actual consistent snapshot.
        snapshot: Arc<ConfigSnapshot>,
    },
    /// Successful source write, independent of effect adoption.
    ConfigurationSaved {
        /// Actual saved version and required effects.
        receipt: ConfigWriteReceipt,
        /// Actual native adoption, or None when only persistence is confirmed.
        applied: Option<DriverStatus>,
    },
    /// Rule read or reload, never a claimed write.
    Rules {
        /// The rule owner's actual snapshot.
        snapshot: PolicySnapshot,
    },
    /// One ordinary or remembered rule read from the rule owner's snapshot.
    Rule {
        /// Original typed rule, containing group identity and saved source version.
        snapshot: Box<RuleSnapshot>,
    },
    /// Successful rule source write.
    RulesSaved {
        /// Actual saved version and required effects.
        receipt: RuleWriteReceipt,
        /// Actual native adoption, independent of the file-write receipt.
        applied: Option<DriverStatus>,
    },
    /// Authoritative lower-port result.
    DriverStatus {
        /// Actual driver report.
        snapshot: DriverStatus,
    },
}
/// Exact result category; missing capability is not an empty successful result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    /// The requested rule or original instance does not exist in the queried snapshot.
    NotFound,
    /// Cursor is from a different tree generation/query.
    CursorExpired,
    /// The actual source cannot implement a requested field or action.
    Unsupported,
    /// Session lacks a current permission grant.
    Unauthorized,
    /// Required concrete capability is absent.
    Unavailable,
    /// Bounded resources are occupied.
    Busy,
    /// Optimistic saved version no longer matches.
    Conflict,
    /// Current typed input was rejected.
    Invalid,
    /// An actual operation failed.
    Failed,
}
impl From<ModuleError> for FailureCode {
    fn from(error: ModuleError) -> Self {
        match error {
            ModuleError::Forbidden | ModuleError::Unauthenticated => Self::Unauthorized,
            ModuleError::Conflict => Self::Conflict,
            ModuleError::InvalidInput => Self::Invalid,
            ModuleError::Unsupported => Self::Unsupported,
            ModuleError::Busy => Self::Busy,
            ModuleError::Unavailable | ModuleError::Stopped => Self::Unavailable,
        }
    }
}
/// Explicit admission, completed effects or failure with any actual persistence facts attached.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandResult {
    /// The actual host accepted a request whose lifecycle effects are not yet complete.
    Accepted {
        /// Concrete admission fact, never a stopped or exited state.
        data: ReplyData,
    },
    /// All effects required by this command actually completed.
    Completed {
        /// Concrete completed result, never queue admission.
        data: ReplyData,
    },
    /// Failure with any real partial effects attached.
    Failed {
        /// Stable category.
        code: FailureCode,
        /// Concrete reason.
        message: String,
        /// Confirmed facts, including an already-visible save.
        facts: Option<ReplyData>,
    },
}
/// Main forwards original operation outcomes; it does not invent a confirmed effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationProposal {
    /// Proposal made for this original, independent of what the source actually completed.
    pub decision: Decision,
    /// Actual proposal provenance, when a proposal exists.
    pub reason: ChoiceReason,
}
/// Actual completion and the optional original proposal; unknown proposals remain absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationReport {
    /// Original decision proposal and actual source outcome.
    Receipt {
        /// None when the source completed before a proposal was selected.
        proposal: Option<OperationProposal>,
        /// Confirmed physical result or explicit Unknown.
        outcome: ExecutionOutcome,
        /// Original executor's actual signal attempts, separate from the decision proposal.
        termination: as_types::operation::TerminationReport,
    },
    /// The original operation could not complete through its owner.
    Failed {
        /// A known proposal, if one was actually selected before failure.
        proposal: Option<OperationProposal>,
        /// Stable failure category.
        code: FailureCode,
        /// Actual reason.
        message: String,
    },
}
/// Project an actual controlled or direct completion, preserving its source facts.
/// Direct driver results have no invented decision identity; observations are not completions.
/// This converts data only and performs neither execution nor middleware transport I/O.
pub fn operation_result(notice: &MainNotice) -> Option<Up> {
    let decision_id = notice.decision_id;
    if decision_id.is_none() && !matches!(&notice.result, Ok(DispatchOutcome::Driver { .. })) {
        return None;
    }
    let proposal = notice.choice.as_ref().map(|choice| OperationProposal {
        decision: choice.decision,
        reason: choice.reason,
    });
    let result = match &notice.result {
        Ok(DispatchOutcome::Execution(receipt)) if Some(receipt.decision_id) == decision_id => {
            OperationReport::Receipt {
                proposal,
                outcome: receipt.outcome,
                termination: receipt.termination,
            }
        }
        Ok(DispatchOutcome::Execution(_)) => OperationReport::Failed {
            proposal,
            code: FailureCode::Invalid,
            message: "original completion identity mismatch".into(),
        },
        Ok(DispatchOutcome::Driver {
            outcome,
            termination,
        }) => OperationReport::Receipt {
            proposal,
            outcome: *outcome,
            termination: *termination,
        },
        Ok(DispatchOutcome::Observed) => return None,
        Err(error) => OperationReport::Failed {
            proposal,
            code: match error {
                ModuleError::Unsupported => FailureCode::Unsupported,
                ModuleError::Conflict => FailureCode::Conflict,
                ModuleError::Forbidden | ModuleError::Unauthenticated => FailureCode::Unauthorized,
                ModuleError::Unavailable => FailureCode::Unavailable,
                ModuleError::InvalidInput => FailureCode::Invalid,
                ModuleError::Busy => FailureCode::Busy,
                ModuleError::Stopped => FailureCode::Failed,
            },
            message: error.to_string(),
        },
    };
    Some(Up::OperationResult(std::sync::Arc::new(OperationResult {
        decision_id,
        event: notice.event.clone(),
        hit: notice.choice.as_ref().and_then(|choice| choice.hit.clone()),
        result,
    })))
}
/// Real upstream notifications, without speculative logs/LLM/process-tree DTOs.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Up {
    /// Actual authorized LLM evidence from the one runtime stream.
    Chat {
        /// Subscription selected on this authenticated connection.
        subscription_id: CommandId,
        /// Original evidence or projection; no middleware association added.
        envelope: std::sync::Arc<as_types::flow::ChatEnvelope>,
    },
    /// Original management command's final result.
    Reply {
        /// Correlation from Down.
        id: CommandId,
        /// Explicit admission, completed effect or failure and facts.
        result: CommandResult,
    },
    /// Source notification, independent of a command waiter.
    DriverStatus {
        /// Actual source report.
        snapshot: DriverStatus,
    },
    /// One live ordinary-rule question.
    Ask {
        /// Original operation decision identity.
        decision_id: DecisionId,
        /// Exact matched rule and tagged source version.
        hit: RuleHit,
        /// Facts used by that original rule match.
        event: std::sync::Arc<EventData>,
        /// Explicit decision forms.
        choices: Vec<Decision>,
        /// Timeout decision bound by the original operation.
        timeout: Decision,
        /// Remaining original effective window; never a renewal.
        remaining_ms: u64,
    },
    /// Actual original operation result, not a UI close instruction.
    OperationResult(std::sync::Arc<OperationResult>),
}
/// One original completion value, reused by the public envelope and middleware routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationResult {
    /// Original controlled identity; direct driver results have none.
    pub decision_id: Option<DecisionId>,
    /// Original event with the facts used for this operation, never a current tree substitute.
    pub event: std::sync::Arc<EventData>,
    /// Actual selected rule and source version; absent when no rule hit was obtained.
    pub hit: Option<as_types::main_ports::RuleHit>,
    /// Execution evidence or source failure.
    pub result: OperationReport,
}
/// Project an actual validated source snapshot without a second state or transport owner.
pub fn driver_status(snapshot: DriverStatus) -> Up {
    Up::DriverStatus { snapshot }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_contract_has_one_command_and_reports_admission_without_claiming_exit() {
        assert!(matches!(
            serde_json::from_str::<ControlCommand>(r#"{"type":"request_stop"}"#)
                .expect("current command"),
            ControlCommand::RequestStop
        ));
        assert!(serde_json::from_str::<ControlCommand>(r#"{"type":"stop_main"}"#).is_err());
        assert_eq!(
            serde_json::to_value(CommandResult::Accepted {
                data: ReplyData::MainStopRequested,
            })
            .expect("admission result"),
            serde_json::json!({"status":"accepted","data":{"type":"main_stop_requested"}})
        );
    }
}
