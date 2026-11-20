//! Immutable interface data. Discovery belongs to configuration; composition belongs to main.
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Build target, not a kernel capability or authentication claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPlatform {
    /// Linux native paths and credentials.
    Linux,
    /// Windows native paths and credentials.
    Windows,
}
impl TargetPlatform {
    /// Build-time platform selection without probing the machine.
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        compile_error!("AgentShield has no target-platform contract for this operating system");
    }
}
/// Resolved data produced only by the configuration implementation. No discovery happens here.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedPaths {
    /// Actual executable, not `argv[0]`.
    #[serde(serialize_with = "serialize_native_path")]
    pub backend_executable: PathBuf,
    /// Fixed installation root.
    #[serde(serialize_with = "serialize_native_path")]
    pub self_protection_root: PathBuf,
    /// Sole configuration source.
    #[serde(serialize_with = "serialize_native_path")]
    pub configuration_file: PathBuf,
    /// Sole ordinary policy source.
    #[serde(serialize_with = "serialize_native_path")]
    pub rules_file: PathBuf,
    /// Private packaged driver inputs.
    #[serde(serialize_with = "serialize_native_path")]
    pub driver_assets_directory: PathBuf,
    /// Fixed local middleware endpoint.
    #[serde(serialize_with = "serialize_native_path")]
    pub middleware_endpoint: PathBuf,
    /// Reserved data location, not proof a storage module exists.
    #[serde(serialize_with = "serialize_native_path")]
    pub data_directory: PathBuf,
    /// Fixed private credential store owned by the credential module.
    #[serde(serialize_with = "serialize_native_path")]
    pub credential_store_file: PathBuf,
    /// Exported public AS CA; contains no private signing material.
    #[serde(serialize_with = "serialize_native_path")]
    pub ca_public_file: PathBuf,
    /// Fixed backend security audit database.
    #[serde(serialize_with = "serialize_native_path")]
    pub security_log_file: PathBuf,
    /// Fixed middleware-owned business database, distinct from backend security logs.
    #[serde(serialize_with = "serialize_native_path")]
    pub middleware_database_file: PathBuf,
}
/// One read-only path set. Modules share the same Arc rather than discovering another root.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct ConfigPaths(ResolvedPaths);
impl ConfigPaths {
    /// Accept configuration-owned resolved data without deriving or touching filesystem paths.
    pub fn from_configuration(paths: ResolvedPaths) -> Self {
        Self(paths)
    }
    /// Original executable.
    pub fn backend_executable(&self) -> &Path {
        &self.0.backend_executable
    }
    /// Bound installation root.
    pub fn self_protection_root(&self) -> &Path {
        &self.0.self_protection_root
    }
    /// Fixed configuration source.
    pub fn configuration_file(&self) -> &Path {
        &self.0.configuration_file
    }
    /// Fixed ordinary rules source.
    pub fn rules_file(&self) -> &Path {
        &self.0.rules_file
    }
    /// Packaged driver input directory.
    pub fn driver_assets_directory(&self) -> &Path {
        &self.0.driver_assets_directory
    }
    /// Fixed local middleware endpoint.
    pub fn middleware_endpoint(&self) -> &Path {
        &self.0.middleware_endpoint
    }
    /// Reserved data location.
    pub fn data_directory(&self) -> &Path {
        &self.0.data_directory
    }
    /// Fixed private store; consumers do not derive their own root.
    pub fn credential_store_file(&self) -> &Path {
        &self.0.credential_store_file
    }
    /// Fixed public CA export.
    pub fn ca_public_file(&self) -> &Path {
        &self.0.ca_public_file
    }
    /// Fixed backend security log database, derived by configuration.
    pub fn security_log_file(&self) -> &Path {
        &self.0.security_log_file
    }
    /// One configuration-derived middleware business store; callers do not choose a target path.
    pub fn middleware_database_file(&self) -> &Path {
        &self.0.middleware_database_file
    }
}
/// Common immutable data composed by main after configuration initialization.
#[derive(Debug, Clone)]
pub struct MainContext {
    paths: Arc<ConfigPaths>,
    platform: TargetPlatform,
}
impl MainContext {
    /// Main borrows configuration's already resolved path object.
    pub fn new(paths: Arc<ConfigPaths>, platform: TargetPlatform) -> Self {
        Self { paths, platform }
    }
    /// Same shared path allocation, with no additional discovery or cache.
    pub fn paths(&self) -> &Arc<ConfigPaths> {
        &self.paths
    }
    /// Explicit build target.
    pub fn platform(&self) -> TargetPlatform {
        self.platform
    }
}

/// Encode native path bytes without loss or caller-controlled reinterpretation.
pub fn serialize_native_path<S: serde::Serializer>(
    path: &Path,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut value = serializer.serialize_struct("NativePath", 2)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.serialize_field("encoding", "unix_bytes")?;
        value.serialize_field("value", path.as_os_str().as_bytes())?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.serialize_field("encoding", "utf16")?;
        value.serialize_field("value", &path.as_os_str().encode_wide().collect::<Vec<_>>())?;
    }
    value.end()
}
