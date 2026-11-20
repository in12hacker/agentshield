//! Typed HIPS rules: subject, operations, typed target, scope, action and logging.
use as_types::event_operation::{FileOperation, NetworkOperation, ProcessOperation};
use as_types::rule_execution::RuleExecution;
use as_types::{main_ports::Decision, workload::ContainerInstance};
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroU64};

/// One current rule file. Unknown fields and invalid variants are rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    /// Typed groups from the three independent rule families.
    pub groups: Vec<RuleGroup>,
}
/// Family is declared once at the group boundary; target fields are type checked accordingly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleGroup {
    /// Process operations and process-specific targets.
    Process(Group<ProcessTarget, ProcessOperation>),
    /// File operations and file-specific targets.
    File(Group<FileTarget, FileOperation>),
    /// Network operations and network-specific targets.
    Network(Group<NetworkTarget, NetworkOperation>),
}
/// One family-tagged original rule, without a second copy of its fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "rule",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RuleDefinition {
    /// Process operations with the original process target.
    Process(Rule<ProcessTarget, ProcessOperation>),
    /// File operations with the original file target.
    File(Rule<FileTarget, FileOperation>),
    /// Network operations with the original network target.
    Network(Rule<NetworkTarget, NetworkOperation>),
}
/// A single rule and its group read from the same immutable document version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSnapshot {
    /// Complete source document version, also used by the ordinary update path.
    pub version: as_types::RuleVersion,
    /// Original containing group identity; execution placement belongs to that group.
    pub group_id: String,
    /// Original typed rule fields.
    pub rule: RuleDefinition,
}
/// One explicit edit to the existing document; conditions and placement are never inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleChange {
    /// Add a group with explicitly selected execution placement and original rules.
    CreateGroup {
        /// Complete typed group.
        group: Box<RuleGroup>,
    },
    /// Replace a group, retaining its identity and family.
    ReplaceGroup {
        /// Existing group identity.
        group_id: String,
        /// Complete replacement; execution placement may explicitly change.
        group: Box<RuleGroup>,
    },
    /// Delete one ordinary group and its rules.
    DeleteGroup {
        /// Existing group identity.
        group_id: String,
    },
    /// Add one rule to an existing group of the same family.
    CreateRule {
        /// Existing containing group identity.
        group_id: String,
        /// Complete original typed rule.
        rule: Box<RuleDefinition>,
    },
    /// Replace one rule, retaining its identity and family.
    ReplaceRule {
        /// Existing rule identity, unique across the document.
        rule_id: String,
        /// Complete original replacement fields.
        rule: Box<RuleDefinition>,
    },
    /// Delete one ordinary rule, retaining its group.
    DeleteRule {
        /// Existing rule identity, unique across the document.
        rule_id: String,
    },
}
/// A numbered group of rules belonging to one family and one processing stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group<T, O> {
    /// Stable management identity.
    pub id: String,
    /// Visible positive group order within this stage and family.
    pub number: NonZeroU32,
    /// Disabled groups retain their configuration but do not match.
    pub enabled: bool,
    /// Explicit driver/backend placement and legal backend stage.
    pub execution: RuleExecution,
    /// Target-typed rules.
    pub rules: Vec<Rule<T, O>>,
}
/// One rule. Empty conditions mean unrestricted, but scope/operations/action/log are explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule<T, O> {
    /// Stable rule identity, unique across the document.
    pub id: String,
    /// Order within its group.
    pub number: NonZeroU32,
    /// Whether this rule participates in matching.
    pub enabled: bool,
    /// All protected actors or a nonempty explicit container set.
    pub scope: Scope,
    /// Nonempty family-specific operation names; OR within the list.
    pub operations: Vec<O>,
    /// Shared original actor conditions; AND with operations, scope and target.
    #[serde(default)]
    pub source: ProcessMatch,
    /// Family-specific original target conditions.
    pub target: T,
    /// The only action for this rule.
    pub action: Action,
    /// Queue a structured log request when this rule hits.
    pub log: bool,
    /// Fixed denial feedback, preserving UTF-8 and whitespace; nonempty only for Deny or Ask.
    #[serde(default)]
    pub deny_message: String,
}
/// Rule applicability never changes matching precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    /// Protected host and container actors, including future protected objects.
    AllProtected,
    /// OR across exact runtime-scoped container object identities.
    Containers {
        /// Must be nonempty, distinct and complete.
        instances: Vec<ContainerInstance>,
    },
}
/// A single action, with deny and timeout actions retaining termination scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Direct allow.
    Allow,
    /// Direct deny, retaining the full requested termination scope.
    Deny {
        /// Termination happens only after denying the original operation.
        terminate: as_types::main_ports::Termination,
    },
    /// Driver transfer; never encoded as Allow.
    ToUser,
    /// Interactive selection within the original source window.
    Ask {
        /// Maximum waiting period, capped by the original deadline.
        timeout_ms: NonZeroU64,
        /// Action on timeout, source retirement or last-channel loss after admission.
        timeout: Decision,
    },
}
impl Action {
    /// Direct proposal, if this action is neither a transfer nor an interaction.
    pub fn decision(&self) -> Option<Decision> {
        match self {
            Self::Allow => Some(Decision::Allow),
            Self::Deny { terminate } => Some(Decision::Deny {
                terminate: *terminate,
            }),
            _ => None,
        }
    }
}
/// Explicit text semantics, compiled once at load time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operator",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TextMatch {
    /// Exact original bytes of the UTF-8 configuration string.
    Exact(String),
    /// Original byte prefix; no path normalization or case folding.
    Prefix(String),
    /// globset glob, with slash as a literal separator.
    Glob(String),
    /// regex bytes pattern; anchors are explicit in the pattern.
    Regex(String),
}
/// One argv element; boundaries are never flattened into a command string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgumentMatch {
    /// Zero-based original argument position.
    pub index: usize,
    /// Match this argument only.
    pub pattern: TextMatch,
}
/// Shared original actor and physical parent conditions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessMatch {
    /// Original actor host PID sample, populated before matching, not a global rule variable.
    pub pid: Option<u32>,
    /// Original physical parent host PID sample.
    pub ppid: Option<u32>,
    /// Actor name.
    pub name: Option<TextMatch>,
    /// Actor executable path.
    pub executable: Option<TextMatch>,
    /// Original argv positions, all required.
    pub arguments: Vec<ArgumentMatch>,
    /// Actor working directory.
    pub cwd: Option<TextMatch>,
    /// Captured real UID.
    pub uid: Option<u32>,
    /// Captured real GID.
    pub gid: Option<u32>,
    /// Captured effective UID.
    pub euid: Option<u32>,
    /// Captured effective GID.
    pub egid: Option<u32>,
    /// Physical parent name.
    pub parent_name: Option<TextMatch>,
    /// Physical parent executable.
    pub parent_executable: Option<TextMatch>,
}
/// Process-specific requested execution target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessTarget {
    /// Requested image, distinct from the actor executable.
    pub executable: Option<TextMatch>,
    /// Requested argument positions.
    pub arguments: Vec<ArgumentMatch>,
}
/// File-specific original target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileTarget {
    /// Original path bytes.
    pub path: Option<TextMatch>,
    /// Rename/link destination, when present.
    pub new_path: Option<TextMatch>,
    /// Original device major/minor.
    pub device: Option<(u32, u32)>,
    /// Original inode sample.
    pub inode: Option<u64>,
    /// All specified access bits must be present.
    pub access_mask: Option<u32>,
    /// Same-hook self-protection mount effect; independent_view is the supported maintenance scope.
    pub mount_effect: Option<as_types::dispatch_event::MountEffect>,
}
/// Inclusive transport port range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRange {
    /// First included port.
    pub first: u16,
    /// Last included port; must be >= first.
    pub last: u16,
}
/// Network-specific original endpoint conditions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkTarget {
    /// Local IP network in CIDR notation.
    pub local_cidr: Option<String>,
    /// Remote IP network in CIDR notation.
    pub remote_cidr: Option<String>,
    /// Local port or range.
    pub local_port: Option<PortRange>,
    /// Remote port or range.
    pub remote_port: Option<PortRange>,
    /// IANA transport protocol.
    pub protocol: Option<u8>,
    /// Source-supplied domain; no reverse-DNS inference.
    pub domain: Option<TextMatch>,
}
