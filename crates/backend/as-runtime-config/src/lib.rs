//! Configuration-owned fixed layout, strict current document, real writes and one readonly cache.
#![forbid(unsafe_code)]
use arc_swap::ArcSwap;
use as_types::{
    configuration::*,
    context::{ConfigPaths, ResolvedPaths, TargetPlatform},
};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    sync::Arc,
};
mod driver_image;
mod flow;

/// One actual saved document and the same lifetime-bound path allocation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigSnapshot {
    document: Arc<ConfigDocument>,
    version: ConfigVersion,
    paths: Arc<ConfigPaths>,
}
impl ConfigSnapshot {
    /// Effects required between these two owner-validated snapshots; used by save and reload consumers.
    pub fn required_effects_from(&self, before: &Self) -> Vec<ConfigEffect> {
        effects(&before.document, &self.document)
    }
    /// Current saved semantic content.
    pub fn document(&self) -> &Arc<ConfigDocument> {
        &self.document
    }
    /// Optimistic management version.
    pub fn version(&self) -> ConfigVersion {
        self.version
    }
    /// Original path allocation; reload never discovers another root.
    pub fn paths(&self) -> &Arc<ConfigPaths> {
        &self.paths
    }
}
/// Failure is explicit; parsing or transport success never claims driver adoption.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required native capability is absent; the saved configuration remains unchanged.
    #[error("driver configuration unsupported: {0}")]
    Unsupported(&'static str),
    /// A fixed source or binding is unavailable.
    #[error("configuration I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Exact layout cannot be determined safely.
    #[error("backend must be an actual executable under the fixed bin directory")]
    Layout,
    /// Unknown/old format or wrong-platform principal.
    #[error("invalid current configuration: {0}")]
    Invalid(String),
    /// Caller edited an old version or an external writer changed the source.
    #[error("configuration version conflict")]
    Conflict,
    /// Owned management job failed.
    #[error("configuration management task unavailable")]
    Worker,
    /// Rename happened and the cache reflects it, but directory durability was not confirmed.
    #[error(
        "configuration {saved_version:?} is visible, directory synchronization failed: {source}"
    )]
    Durability {
        /// Actual visible version, never an assumed rollback.
        saved_version: ConfigVersion,
        /// Failed synchronization.
        source: std::io::Error,
    },
}
/// Sole layout discoverer and configuration file owner.
pub struct ConfigModule {
    active: ArcSwap<ConfigSnapshot>,
    startup: Arc<ConfigDocument>,
    management: Arc<tokio::sync::Mutex<()>>,
    root: File,
    directory: File,
}
impl ConfigModule {
    /// Discover once from the actual executable and read the one fixed config/as.yaml.
    pub fn initialize() -> Result<Self, ConfigError> {
        Self::from_executable(std::env::current_exe()?)
    }
    fn from_executable(executable: PathBuf) -> Result<Self, ConfigError> {
        let executable = std::fs::canonicalize(executable)?;
        let bin = executable.parent().ok_or(ConfigError::Layout)?;
        if bin.file_name() != Some(std::ffi::OsStr::new("bin")) {
            return Err(ConfigError::Layout);
        }
        let root_path = bin.parent().ok_or(ConfigError::Layout)?.to_path_buf();
        let root = File::open(&root_path)?;
        let directory = bound_directory(&root)?;
        let paths = Arc::new(ConfigPaths::from_configuration(ResolvedPaths {
            configuration_file: root_path.join("config/as.yaml"),
            rules_file: root_path.join("config/rules.yaml"),
            driver_assets_directory: root_path.join("driver"),
            middleware_endpoint: root_path.join("run/middleware.sock"),
            data_directory: root_path.join("data"),
            middleware_database_file: root_path.join("data/middleware/business.sqlite"),
            credential_store_file: root_path.join("data/credentials/store"),
            ca_public_file: root_path.join("data/credentials/ca.crt"),
            security_log_file: root_path.join("log/security.sqlite"),
            backend_executable: executable,
            self_protection_root: root_path,
        }));
        let document = Arc::new(read(&directory)?);
        let version = version(&document)?;
        Ok(Self {
            startup: document.clone(),
            active: ArcSwap::from_pointee(ConfigSnapshot {
                document,
                version,
                paths,
            }),
            management: Arc::new(tokio::sync::Mutex::new(())),
            root,
            directory,
        })
    }
    /// Use configuration's path allocation to compose MainContext; no extra discovery.
    pub fn shared_paths(&self) -> Arc<ConfigPaths> {
        self.snapshot().paths.clone()
    }
    /// Read the one current saved version/content pair.
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        self.active.load_full()
    }
    /// The Group scope shared by the driver and consumer for this service instance.
    pub fn startup_group_observation(&self) -> Option<GroupObservation> {
        self.startup.group_observation
    }
    /// Pending initialization-only effects survive later saves and reloads.
    pub fn reinitialization_pending(&self, snapshot: &ConfigSnapshot) -> bool {
        requires_reinitialization(&self.startup, &snapshot.document)
    }
    /// Validate and atomically save to the fixed bound source off the business thread.
    /// The actual job owns serialization even when this caller's future is cancelled.
    pub async fn update(
        self: &Arc<Self>,
        expected: ConfigVersion,
        document: ConfigDocument,
    ) -> Result<ConfigWriteReceipt, ConfigError> {
        let guard = self.management.clone().lock_owned().await;
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            owner.check_binding()?;
            let previous = owner.snapshot();
            if previous.version != expected || version(&read(&owner.directory)?)? != expected {
                return Err(ConfigError::Conflict);
            }
            validate(&document)?;
            let next = version(&document)?;
            let effects = effects(&previous.document, &document);
            let changed_sections = sections(&previous.document, &document);
            let text = serde_saphyr::to_string(&document)
                .map_err(|e| ConfigError::Invalid(e.to_string()))?;
            // The temporary name and target both resolve through our retained directory, not a client path.
            let bound = PathBuf::from(format!("/proc/self/fd/{}", owner.directory.as_raw_fd()));
            let mut temporary = tempfile::NamedTempFile::new_in(&bound)?;
            temporary.write_all(text.as_bytes())?;
            temporary.as_file().sync_all()?;
            owner.check_binding()?;
            temporary
                .persist(bound.join("as.yaml"))
                .map_err(|e| ConfigError::Io(e.error))?;
            owner.active.store(Arc::new(ConfigSnapshot {
                document: Arc::new(document),
                version: next,
                paths: previous.paths.clone(),
            }));
            owner
                .directory
                .sync_all()
                .map_err(|source| ConfigError::Durability {
                    saved_version: next,
                    source,
                })?;
            Ok(ConfigWriteReceipt {
                saved_version: next,
                changed_sections,
                required_effects: effects,
            })
        })
        .await
        .map_err(|_| ConfigError::Worker)?
    }
    /// Reload the same binding and preserve its exact path allocation; never rewrite a file.
    pub async fn reload(
        self: &Arc<Self>,
        expected: ConfigVersion,
    ) -> Result<Arc<ConfigSnapshot>, ConfigError> {
        let guard = self.management.clone().lock_owned().await;
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            owner.check_binding()?;
            let previous = owner.snapshot();
            if previous.version != expected {
                return Err(ConfigError::Conflict);
            }
            let document = read(&owner.directory)?;
            let version = version(&document)?;
            let snapshot = Arc::new(ConfigSnapshot {
                document: Arc::new(document),
                version,
                paths: previous.paths.clone(),
            });
            owner.active.store(snapshot.clone());
            Ok(snapshot)
        })
        .await
        .map_err(|_| ConfigError::Worker)?
    }
    fn check_binding(&self) -> Result<(), ConfigError> {
        use std::os::unix::fs::MetadataExt;
        let actual = bound_directory(&self.root)?.metadata()?;
        let bound = self.directory.metadata()?;
        if actual.dev() != bound.dev() || actual.ino() != bound.ino() {
            return Err(ConfigError::Layout);
        }
        Ok(())
    }
}
fn bound_directory(root: &File) -> Result<File, ConfigError> {
    Ok(File::from(
        openat2(
            root,
            "config",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        )
        .map_err(std::io::Error::from)?,
    ))
}
fn read(directory: &File) -> Result<ConfigDocument, ConfigError> {
    let mut file = File::from(
        openat2(
            directory,
            "as.yaml",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        )
        .map_err(std::io::Error::from)?,
    );
    if !file.metadata()?.is_file() {
        return Err(ConfigError::Invalid("source is not a regular file".into()));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let document =
        serde_saphyr::from_str(&text).map_err(|e| ConfigError::Invalid(e.to_string()))?;
    validate(&document)?;
    Ok(document)
}
fn validate(document: &ConfigDocument) -> Result<(), ConfigError> {
    if let Some(scope) = document.group_observation {
        let mut ports = std::collections::HashSet::new();
        for port in [scope.a2a_port, scope.otlp_http_port, scope.gateway_ws_port]
            .into_iter()
            .flatten()
        {
            if !ports.insert(port) {
                return Err(ConfigError::Invalid(
                    "Group observation endpoints must differ".into(),
                ));
            }
        }
        if ports.is_empty() {
            return Err(ConfigError::Invalid(
                "Group observation requires an enabled source".into(),
            ));
        }
    }
    flow::validate(document)?;
    let mut audit_directories = std::collections::BTreeSet::new();
    if document.file_close_audit.iter().any(|path| {
        !path.is_absolute()
            || path.as_os_str().as_encoded_bytes().contains(&0)
            || path
                .components()
                .any(|part| part == std::path::Component::ParentDir)
            || !audit_directories.insert(path)
    }) {
        return Err(ConfigError::Invalid(
            "invalid or duplicate write-close audit directory".into(),
        ));
    }
    if document.format_version != 2 {
        return Err(ConfigError::Invalid("format_version must equal 2".into()));
    }
    if document.process_tree.memory_budget_bytes.get() < 4096
        || document.process_tree.query_page_size.get() > 4096
    {
        return Err(ConfigError::Invalid(
            "process tree resource limit invalid".into(),
        ));
    }
    let queues = &document.queues;
    let max = tokio::sync::Semaphore::MAX_PERMITS;
    if queues.callbacks.get() >= max
        || [queues.review, queues.messages, queues.driver_urgent]
            .iter()
            .any(|capacity| capacity.get() > max)
    {
        return Err(ConfigError::Invalid(
            "queue capacity exceeds the runtime limit (callbacks reserves one generation slot)"
                .into(),
        ));
    }
    let principal_matches = matches!(
        (&document.middleware.peer, TargetPlatform::current()),
        (Principal::UnixUid { .. }, TargetPlatform::Linux)
            | (Principal::WindowsSid { .. }, TargetPlatform::Windows)
    );
    if !principal_matches {
        return Err(ConfigError::Invalid(
            "middleware principal belongs to another platform".into(),
        ));
    }
    if let Some(ws) = &document.middleware.websocket
        && (!ws.listen.ip().is_loopback()
            || ws.listen.port() == 0
            || ws.origin.as_ref().is_some_and(|origin| {
                let authority = origin
                    .strip_prefix("https://")
                    .or_else(|| origin.strip_prefix("http://"));
                authority.is_none_or(|value| {
                    value.is_empty()
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte))
                })
            }))
    {
        return Err(ConfigError::Invalid(
            "invalid middleware WebSocket listener or Origin".into(),
        ));
    }
    Ok(())
}
fn sections(before: &ConfigDocument, after: &ConfigDocument) -> Vec<ConfigSection> {
    let mut out = Vec::new();
    if before.log_storage != after.log_storage {
        out.push(ConfigSection::LogStorage);
    }
    if before.flow != after.flow {
        out.push(ConfigSection::Flow);
    }
    if before.credentials != after.credentials {
        out.push(ConfigSection::Credentials);
    }
    if before.driver != after.driver {
        out.push(ConfigSection::Driver);
    }
    if before.middleware != after.middleware {
        out.push(ConfigSection::Middleware);
    }
    if before.queues != after.queues {
        out.push(ConfigSection::Queues);
    }
    if before.process_tree != after.process_tree {
        out.push(ConfigSection::ProcessTree);
    }
    if before.file_close_audit != after.file_close_audit {
        out.push(ConfigSection::FileCloseAudit);
    }
    if before.group_observation != after.group_observation {
        out.push(ConfigSection::GroupObservation);
    }
    out
}
fn version(document: &ConfigDocument) -> Result<ConfigVersion, ConfigError> {
    // Fixed struct field order and no maps: one owner, one exact semantic representation.
    let bytes = serde_json::to_vec(document).map_err(|e| ConfigError::Invalid(e.to_string()))?;
    Ok(ConfigVersion::from_digest(Sha256::digest(bytes).into()))
}
fn effects(before: &ConfigDocument, after: &ConfigDocument) -> Vec<ConfigEffect> {
    let mut out = Vec::new();
    if before.credentials != after.credentials {
        out.push(ConfigEffect::ApplyCredentials);
    }
    if before.driver != after.driver {
        out.push(ConfigEffect::ApplyDriver);
    }
    if before.middleware != after.middleware {
        out.push(ConfigEffect::ReconnectMiddleware);
    }
    if requires_reinitialization(before, after) {
        out.push(ConfigEffect::ReinitializeMain);
    }
    out
}
fn requires_reinitialization(before: &ConfigDocument, after: &ConfigDocument) -> bool {
    before.log_storage != after.log_storage
        || before.queues != after.queues
        || before.process_tree != after.process_tree
        || before.flow != after.flow
        || before.file_close_audit != after.file_close_audit
        || before.group_observation != after.group_observation
        || before.middleware.websocket.as_ref().map(|ws| ws.listen)
            != after.middleware.websocket.as_ref().map(|ws| ws.listen)
}
#[cfg(test)]
mod tests;
