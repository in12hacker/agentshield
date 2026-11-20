//! Original actor workload facts, supplied by the platform facts module.
use serde::{Deserialize, Serialize};

/// Runtime-scoped full container object identity; display names are not identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerInstance {
    /// Platform runtime connection identity.
    pub runtime: String,
    /// Runtime namespace, if that runtime uses namespaces.
    pub namespace: Option<String>,
    /// Full container object ID from that runtime.
    pub id: String,
}

/// Confirmed original actor membership. Missing facts are represented by Option outside this enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "instance", rename_all = "snake_case")]
pub enum Workload {
    /// Platform positively identified a protected host process.
    Host,
    /// Platform bound the actor to this original container instance.
    Container(ContainerInstance),
}
