//! Explicit rule execution placement, independent of event stages and verdicts.
use serde::{Deserialize, Serialize};

/// Mutually exclusive legal execution locations. No inferred placement or parallel location field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "location", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleExecution {
    /// Driver allow, deny or transfer before any backend matching.
    Driver {},
    /// Backend rule processing with an explicit stage.
    Backend {
        /// Remembered, ordinary quick or interactive stage.
        stage: BackendStage,
    },
}

/// Backend precedence; each family still has one quick pool and one question pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStage {
    /// User-remembered direct rules, before ordinary quick rules.
    Remembered,
    /// Ordinary direct rules.
    Fast,
    /// Interactive rules, after the quick pool.
    Ask,
}

impl RuleExecution {
    /// Whether this placement belongs to the actual driver.
    pub const fn is_driver(self) -> bool {
        matches!(self, Self::Driver {})
    }
    /// Whether this placement is the backend interactive pool.
    pub const fn is_ask(self) -> bool {
        matches!(
            self,
            Self::Backend {
                stage: BackendStage::Ask
            }
        )
    }
}
