//! Concrete configuration inputs shared across module interfaces, without module implementations.
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU16, NonZeroU32, NonZeroUsize};
/// Complete current persisted document. No path overrides, legacy defaults or unknown keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDocument {
    /// Exact configuration schema revision, validated by its owner.
    pub format_version: u16,
    /// Bounded queues, not a thread topology.
    pub queues: QueueSettings,
    /// Combined physical retention budget for the business and security audit databases.
    pub log_storage: LogStorageSettings,
    /// Validated process-tree memory, retention and reconciliation policy.
    pub process_tree: crate::process_tree::ProcessTreeSettings,
    /// Persistent driver inputs only.
    pub driver: DriverPersistentConfig,
    /// Server-owned middleware access policy.
    pub middleware: MiddlewareAccess,
    /// Controlled egress; explicit None disables TLS mediation.
    pub flow: Option<crate::flow::FlowSettings>,
    /// Non-secret scope bindings; private material is owned by credentials.
    pub credentials: crate::flow::CredentialSettings,
    /// Linux directories whose direct children's write-open releases are audited.
    pub file_close_audit: Vec<std::path::PathBuf>,
    /// Explicit host loopback TCP endpoints selected for passive Group observation.
    pub group_observation: Option<GroupObservation>,
}
/// Port selection for the proven IPv4 loopback Group transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupObservation {
    /// A2A HTTP/1.1 service port in the host's initial network namespace.
    #[serde(deserialize_with = "configured_group_port")]
    pub a2a_port: Option<NonZeroU16>,
    /// Direct OTLP/HTTP collector port in the same namespace.
    #[serde(deserialize_with = "configured_group_port")]
    pub otlp_http_port: Option<NonZeroU16>,
    /// Public plaintext Gateway WebSocket port in the same namespace.
    #[serde(deserialize_with = "configured_group_port")]
    pub gateway_ws_port: Option<NonZeroU16>,
}
// An inactive source is explicit null; a missing current field is not an old-format default.
fn configured_group_port<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NonZeroU16>, D::Error> {
    Option::deserialize(deserializer)
}
/// Independent capacities; questions remain serial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSettings {
    /// Event admission and quick work.
    pub callbacks: NonZeroUsize,
    /// Work queued for the one slow/Ask loop.
    pub review: NonZeroUsize,
    /// Ordinary messages and management.
    pub messages: NonZeroUsize,
    /// Reserved urgent driver command capacity.
    pub driver_urgent: NonZeroUsize,
}
/// One shared database retention budget; not a per-database allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogStorageSettings {
    /// Bytes across both SQLite main files and their WAL, SHM and rollback journals.
    pub total_bytes: std::num::NonZeroU64,
}
impl Default for LogStorageSettings {
    fn default() -> Self {
        Self {
            total_bytes: std::num::NonZeroU64::new(1 << 30).unwrap_or(std::num::NonZeroU64::MIN),
        }
    }
}
/// Ordinary protection families. Self protection is not an ordinary feature switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionKind {
    /// Ordinary process rules.
    Process,
    /// Ordinary file rules.
    File,
    /// Ordinary network rules.
    Network,
}
/// Persistent defaults, distinct from current runtime overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectionDefaults {
    /// Process protection on complete initialization.
    pub process: bool,
    /// Ordinary file protection; never disables self protection.
    pub file: bool,
    /// Network protection.
    pub network: bool,
}
/// Source limits whose physical adoption requires a driver acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverLimits {
    /// Maximum original requests admitted by the driver.
    pub max_pending: NonZeroU32,
    /// Maximum original lifetime, including queueing, in milliseconds.
    pub max_hold_ms: NonZeroU32,
}
/// Parameters of fixed file self-protection semantics, with no configurable target or writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfProtection {
    /// Request logging at an actual system-rule hit.
    pub log: bool,
}
/// Persistent settings exported to a driver implementation, never claimed effective by parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverPersistentConfig {
    /// Defaults restored by full initialization.
    pub startup_protection: ProtectionDefaults,
    /// Original source admission/lifetime limits.
    pub limits: DriverLimits,
    /// Fixed self-protection policy parameters.
    pub self_protection: SelfProtection,
}
/// OS-verified principal selected by the server, not by message authentication claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Principal {
    /// Unix peer credentials.
    UnixUid {
        /// Exact peer UID verified by the operating system.
        uid: u32,
    },
    /// Windows service/account SID.
    WindowsSid {
        /// Exact account SID expected by the server.
        sid: String,
    },
}
/// Server-granted hierarchy. A service manager includes ordinary rule management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Queries only.
    ReadOnly,
    /// Queries and ordinary rule writes.
    ManageRules,
    /// Rule writes, configuration writes and lifecycle/runtime controls.
    ManageService,
}
/// Persistent local middleware admission policy; endpoint location is fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiddlewareAccess {
    /// Expected real peer.
    pub peer: Principal,
    /// Permission granted by the backend.
    pub access: Access,
    /// Independent authorization to read captured LLM content.
    pub chat_content: bool,
    /// Explicit WebSocket admission on the local host.
    pub websocket: Option<WebSocketAccess>,
}

/// Admission policy for the management WebSocket, owned by the configuration module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSocketAccess {
    /// Explicit loopback address and port.
    pub listen: std::net::SocketAddr,
    /// Exact browser Origin, if configured. Native clients may omit Origin.
    pub origin: Option<String>,
    /// SHA-256 of the operator-provisioned high-entropy token's UTF-8 bytes.
    pub token_sha256: [u8; 32],
}
/// Semantic current configuration digest owned by the configuration module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigVersion([u8; 32]);
impl ConfigVersion {
    /// Adopt the owner's digest.
    pub const fn from_digest(value: [u8; 32]) -> Self {
        Self(value)
    }
    /// Exact bytes, without another interpretation.
    pub const fn digest(self) -> [u8; 32] {
        self.0
    }
}
/// Driver projection version; distinct from the complete configuration document version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DriverConfigVersion(pub [u8; 32]);
/// Specific effects required after a successful file save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigEffect {
    /// Publish current non-secret bindings through the credential owner.
    ApplyCredentials,
    /// Driver must confirm its actual adoption.
    ApplyDriver,
    /// Current middleware permission must be reauthenticated.
    ReconnectMiddleware,
    /// Queue construction changes require main reinitialization.
    ReinitializeMain,
}
/// Persisted sections whose semantic content changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSection {
    /// Shared security/business database retention budget.
    LogStorage,
    /// Controlled TLS egress settings.
    Flow,
    /// Non-secret credential scope bindings.
    Credentials,
    /// Process-tree ownership/resource policy.
    ProcessTree,
    /// Driver defaults, source limits and fixed self-protection parameters.
    Driver,
    /// Middleware principal and permissions.
    Middleware,
    /// Queue construction settings.
    Queues,
    /// Direct directory marks for Linux write-close audit.
    FileCloseAudit,
    /// Scoped Group observation, applied at initialization.
    GroupObservation,
}
/// Actual persistence receipt; not a kernel-adoption acknowledgement.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigWriteReceipt {
    /// Version whose write and cache publication completed.
    pub saved_version: ConfigVersion,
    /// Changed source sections, independent of whether their effects were adopted.
    pub changed_sections: Vec<ConfigSection>,
    /// Real changes requiring their respective owners to act.
    pub required_effects: Vec<ConfigEffect>,
}
