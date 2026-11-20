//! Semantic driver controls. Actual values can only be supplied by a connected driver.
use crate::{
    RuleVersion,
    configuration::{DriverConfigVersion, ProtectionDefaults, ProtectionKind},
    main_ports::ModuleError,
};
use serde::{Deserialize, Serialize};
/// The current communication contract, with no legacy negotiation.
pub const DRIVER_REVISION: u16 = 2;
/// Ordinary effective state; missing ability is distinct from intentional disable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureState {
    /// Driver reports active protection.
    Enabled,
    /// Driver reports intentional disable.
    Disabled,
    /// Actual source lacks the requested protection.
    Unavailable,
}
/// Nonpersistent override, reset only by full initialization or explicit resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOverride {
    /// No override; use the persistent default.
    Default,
    /// Explicit temporary enable.
    Enabled,
    /// Explicit temporary disable.
    Disabled,
}
/// One driver's actual category observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureStatus {
    /// Real persisted startup default.
    pub startup_default: bool,
    /// Real temporary setting.
    pub runtime_override: RuntimeOverride,
    /// Reported physical protection state.
    pub effective_state: FeatureState,
}
/// Driver's actual control/lifecycle condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// Not initialized.
    Dormant,
    /// Accepting originals.
    Running,
    /// Draining previously admitted originals.
    Quiescing,
    /// Source failure, never an ordinary pause.
    Failed,
}
/// Authoritative driver report. Main does not construct it from desired configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverStatus {
    /// Driver instance, not a frontend identity.
    pub instance: [u8; 16],
    /// Exact current ABI.
    pub revision: u16,
    /// Actual lifecycle condition.
    pub source_state: SourceState,
    /// Whole persisted adoption generation.
    pub persistent_generation: u64,
    /// Runtime settings and lifecycle revision.
    pub state_revision: u64,
    /// Native lifecycle scope invalidating pre-stop candidates; not an interface version.
    pub lifecycle_epoch: u32,
    /// Actually adopted driver configuration digest.
    pub driver_config_version: Option<DriverConfigVersion>,
    /// Actually adopted ordinary policy version.
    pub rule_version: Option<RuleVersion>,
    /// Persisted defaults if a whole state exists.
    pub startup_protection: Option<ProtectionDefaults>,
    /// Original category status.
    pub process: FeatureStatus,
    /// Ordinary file status, separate from self protection.
    pub file: FeatureStatus,
    /// Network status.
    pub network: FeatureStatus,
    /// Actual self-protection coverage state.
    pub self_protection: FeatureState,
    /// Source-confirmed pending count.
    pub pending: u32,
    /// Source-confirmed unread results.
    pub pending_receipts: u32,
    /// Source-confirmed loss count.
    pub dropped: u64,
    /// Native error, if one is currently reported.
    pub error: Option<String>,
}
/// Concrete common controls, never arbitrary key/value or shell execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DriverCommand {
    /// Query actual values.
    GetStatus,
    /// Set only a temporary ordinary-family override.
    SetRuntimeFeature {
        /// Ordinary protection category.
        feature: ProtectionKind,
        /// Temporary value, not a persistent edit.
        enabled: bool,
    },
    /// Remove an override and apply the current persisted default.
    ResumeRuntimeFeature {
        /// Category whose temporary override is removed.
        feature: ProtectionKind,
    },
    /// Stop new originals and wait for admitted work/results to drain.
    Quiesce,
    /// Preserve source failure semantics through retirement.
    DeclareFailure,
}
/// Narrow actual driver interface injected into main; there is no default or software implementation.
#[async_trait::async_trait]
pub trait DriverControl: Send + Sync {
    /// Return only driver-confirmed status after the requested effect, or an explicit failure.
    async fn request(&self, command: DriverCommand) -> Result<DriverStatus, ModuleError>;
}

/// Existing product feature flags, separate from operation selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DriverFeature {
    /// Direct File Driver rules may decide proven independent-view mount maintenance.
    SelfMountRules = 2048,
    /// One UDP/53 send-iterator snapshot for backend DNS-query rules; not native domain matching.
    DnsQuerySnapshot = 1024,
    /// Native last-reference write-close facts through the existing event stream.
    FileCloseAudit = 256,
    /// The original endpoint exposes Controlled, Reply and delivered-original AS_RESULT through PRI.
    ControlPriority = 512,
    /// Explicit exact-process SIGKILL utility; does not imply rule actor/parent execution.
    ProcessKill = 64,
    /// Native process observation, identity inspection and ordered facts barriers.
    ProcessFacts = 32,
    /// Complete fixed-file self-protection coverage.
    SelfProtection = 1,
    /// The authenticated adapter supplies exact runtime-instance facts to backend rules.
    ContainerBinding = 2,
    /// Confirmed original actor termination support.
    TerminateActor = 4,
    /// Confirmed original physical-parent termination support.
    TerminateParent = 8,
    /// Confirmed pipe operation control.
    PipeControl = 16,
}
/// Native compiled-condition selectors, matching the single native image contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum DriverCondition {
    /// Original host TGID.
    ActorPid = 1,
    /// Original physical parent TGID.
    ParentPid = 2,
    /// Original real UID.
    Uid = 3,
    /// Original real GID.
    Gid = 4,
    /// Original effective UID.
    Euid = 5,
    /// Original effective GID.
    Egid = 6,
    /// Original actor name.
    Name = 7,
    /// Original executable path.
    Executable = 8,
    /// Original actor argv position.
    Arguments = 9,
    /// Original working directory.
    Cwd = 10,
    /// Original parent name.
    ParentName = 11,
    /// Original parent executable.
    ParentExecutable = 12,
    /// Requested image.
    TargetExecutable = 13,
    /// Requested image arguments.
    TargetArguments = 14,
    /// Original file path.
    FilePath = 15,
    /// Original destination path.
    NewPath = 16,
    /// Filesystem device major/minor.
    Device = 17,
    /// Original inode sample.
    Inode = 18,
    /// Original access mask.
    AccessMask = 19,
    /// Original local address.
    LocalIp = 20,
    /// Original remote address.
    RemoteIp = 21,
    /// Original local port.
    LocalPort = 22,
    /// Original remote port.
    RemotePort = 23,
    /// IANA transport protocol.
    Protocol = 24,
    /// Verified domain fact.
    Domain = 25,
    /// Same-hook self-protection mount effect, using MountEffect's current numeric values.
    MountEffect = 26,
}
/// Actual native capacities and supported semantics obtained by Hello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriverCapabilities {
    /// Native maximum complete frame size, including its header.
    pub max_frame_bytes: u32,
    /// Native original-operation admission limit.
    pub max_pending: u32,
    /// Native maximum staged persistent request body.
    pub max_transfer_bytes: u64,
    /// Physical maximum original hold window in milliseconds.
    pub max_hold_ms: u32,
    /// Actual operation selector bits; observation bits do not imply interception.
    pub operation_mask: u32,
    /// Actual compiled-condition selector bits.
    pub condition_mask: u64,
    /// Actual independent feature bits; never inferred from configuration.
    pub features: u32,
}
impl DriverCapabilities {
    /// All operation capability bits defined by the current driver protocol.
    pub const OPERATION_MASK: u32 = 0x7fff;

    /// Whether one existing feature is actually reported by the native driver.
    pub const fn supports_feature(&self, feature: DriverFeature) -> bool {
        self.features & feature as u32 != 0
    }
    /// Whether this condition is actually captured at native matching boundaries.
    pub const fn supports_condition(&self, condition: DriverCondition) -> bool {
        self.condition_mask & (1u64 << (condition as u16 - 1)) != 0
    }
    /// Whether the actual hook supports a complete allow/deny/transfer control chain.
    pub fn supports_controlled(&self, kind: crate::main_ports::EventKind, operation: u32) -> bool {
        Self::controlled_operation_bit(kind, operation)
            .is_some_and(|bit| self.operation_mask & (1 << bit) != 0)
    }
    /// Capability bit index for a defined controlled operation in the current protocol.
    pub const fn controlled_operation_bit(
        kind: crate::main_ports::EventKind,
        operation: u32,
    ) -> Option<u32> {
        use crate::main_ports::EventKind;
        Some(match (kind, operation) {
            (EventKind::Process, 1) => 0,
            (EventKind::File, 1) => 1,
            (EventKind::Network, 1) => 2,
            (EventKind::Network, 2) => 3,
            (EventKind::File, 5) => 7,
            (EventKind::File, 6) => 8,
            (EventKind::File, 7) => 9,
            (EventKind::File, 9) => 10,
            (EventKind::File, 18) => 11,
            (EventKind::File, 19) => 12,
            (EventKind::File, 10) => 13,
            (EventKind::File, 4) => 14,
            _ => return None,
        })
    }
}
