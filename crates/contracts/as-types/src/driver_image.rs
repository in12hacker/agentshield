//! Immutable exported driver inputs. File owners create images; the adapter only transports them.
use crate::{
    RuleVersion,
    configuration::{ConfigVersion, DriverConfigVersion},
    context::TargetPlatform,
};
use std::sync::Arc;

/// Configuration owner's immutable driver projection, distinct from the whole saved document.
#[derive(Debug, Clone)]
pub struct DriverConfigImage {
    /// Whole source document used for this export.
    pub config_version: ConfigVersion,
    /// Exact effective projection digest.
    pub driver_config_version: DriverConfigVersion,
    /// Native target representation.
    pub platform: TargetPlatform,
    /// One current image encoding; never negotiated as a fallback.
    pub encoding_revision: u32,
    /// Complete immutable body owned until the native result is confirmed.
    pub immutable_buffer: Arc<[u8]>,
}
/// Rule owner's immutable ordinary driver policy.
#[derive(Debug, Clone)]
pub struct DriverPolicyImage {
    /// Exact complete source policy digest.
    pub rule_version: RuleVersion,
    /// Native target representation.
    pub platform: TargetPlatform,
    /// One current image encoding.
    pub encoding_revision: u32,
    /// Complete immutable native representation.
    pub immutable_buffer: Arc<[u8]>,
}
/// One atomic persistent adoption, not two independently installable updates.
#[derive(Debug, Clone)]
pub struct DriverInitParts {
    /// Generation read from the actual driver, for optimistic replacement.
    pub expected_persistent_generation: u64,
    /// Actual lifecycle scope read from the native driver before publication is admitted.
    pub expected_lifecycle_epoch: u32,
    /// Configuration and fixed self-protection projection.
    pub configuration: DriverConfigImage,
    /// Three ordinary driver rule families.
    pub rules: DriverPolicyImage,
}
