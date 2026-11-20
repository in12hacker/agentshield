//! Narrow stateless driver tools; lifecycle, rules and original decisions stay separate.
use crate::{main_ports::ModuleError, process_source::ProcessBinding};
use serde::{Deserialize, Serialize};

/// One exact host process in the driver instance that supplied its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverProcessTarget {
    /// Actual DriverStatus.instance sampled with the process binding.
    pub driver_instance: [u8; 16],
    /// Actual ProcessBinding.host_tgid, never a namespace-local PID or process group.
    pub host_tgid: u32,
    /// Actual ProcessBinding.birth_boottime_ns, never proc clock ticks.
    #[serde(with = "crate::decimal")]
    pub birth_boottime_ns: u64,
}
impl DriverProcessTarget {
    /// Capture all identity fields together; never refresh only the instance of an old target.
    pub fn from_binding(driver_instance: [u8; 16], binding: &ProcessBinding) -> Self {
        Self {
            driver_instance,
            host_tgid: binding.host_tgid,
            birth_boottime_ns: binding.birth_boottime_ns,
        }
    }
}
/// Actual signal submission outcome; neither variant promises a completed exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillProcessOutcome {
    /// The kernel accepted SIGKILL for the fixed thread group.
    SignalSent,
    /// The fixed process object no longer has a signal target.
    NotRunning,
}
/// Effectful utilities, separate from the read-only ProcessFactsPort supplied to the tree.
#[async_trait::async_trait]
pub trait DriverTools: Send + Sync {
    /// Request SIGKILL once for an exact process. No implicit retries or wait-for-exit.
    async fn kill_process(
        &self,
        target: DriverProcessTarget,
    ) -> Result<KillProcessOutcome, ModuleError>;
}
