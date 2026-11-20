//! Event model and narrow contracts for the clean AgentShield rebuild.
#![forbid(unsafe_code)]
pub mod configuration;
pub mod context;
pub mod decimal;
pub mod diagnostics;
pub mod dispatch_event;
pub mod driver;
pub mod driver_control;
pub mod driver_image;
pub mod driver_tools;
pub mod driver_wire;
pub mod event_operation;
pub mod flow;
mod flow_memory;
pub mod hold_budget;
pub mod json;
pub mod log_storage;
pub mod main_ports;
pub mod operation;
pub mod process_source;
pub mod process_tree;
pub mod rule_execution;
pub mod runtime_observation;
pub mod security_log;
pub mod workload;
/// Stable rule identity; matching and persistence belong to the rule module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RuleId(String);
impl From<&str> for RuleId {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl From<String> for RuleId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl RuleId {
    /// Original rule identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Original operation identity supplied by its source, never inferred by the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DecisionId(pub [u8; 16]);
/// Semantic digest of one complete selected policy document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RuleVersion([u8; 32]);
impl RuleVersion {
    /// Adopt the rule owner's semantic digest.
    pub const fn from_digest(value: [u8; 32]) -> Self {
        Self(value)
    }
    /// Read the selected digest without reinterpreting it.
    pub const fn digest(self) -> [u8; 32] {
        self.0
    }
}
