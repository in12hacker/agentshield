//! Three-family HIPS rule factory with immutable snapshots, typed driver plans and bounded Ask.
#![forbid(unsafe_code)]
mod compile;
mod driver_image;
pub mod interaction;
mod maintenance;
mod management;
pub mod model;
use arc_swap::ArcSwap;

use as_types::{RuleVersion, dispatch_event::*, main_ports::*, process_source::ProcessFactsPort};
use compile::{CompiledRule, Compiler, Match};
use interaction::Interaction;
use model::*;
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

/// Rule loading or publication failed; the previous active snapshot is retained.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Actual driver capabilities cannot preserve a requested rule condition or action.
    #[error("driver rule unsupported: {0}")]
    Unsupported(&'static str),
    /// Source file or atomic file replacement failed.
    #[error("rule source I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Invalid rule data or pattern.
    #[error("invalid rules: {0}")]
    Invalid(String),
    /// The requested current rule or group does not exist.
    #[error("rule or group not found")]
    NotFound,
    /// A stale management request must not replace a newer snapshot.
    #[error("rule version conflict; read the current snapshot before editing")]
    Conflict,
    /// Blocking management task failed; never used as an implicit allow.
    #[error("rule management task unavailable")]
    Worker,
    /// Source was replaced and published, but directory durability is not confirmed.
    #[error(
        "rule version {saved_version:?} is visible; directory synchronization failed: {source}"
    )]
    Durability {
        /// Actual visible saved source version.
        saved_version: RuleVersion,
        /// Failed directory synchronization.
        source: std::io::Error,
    },
}

type ScopeSource = Arc<OnceLock<Arc<dyn ProcessFactsPort>>>;

struct Core {
    kind: EventKind,
    rules: Vec<CompiledRule>,
    interaction: Arc<Interaction>,
    scope_source: ScopeSource,
}
impl Core {
    fn find(
        &self,
        event: &EventData,
        enriched: bool,
        ask: bool,
    ) -> Result<Option<&CompiledRule>, ModuleError> {
        if event.stage() != EventStage::ControlledRequest
            || event.kind() != self.kind
            || event.payload.kind() != self.kind
        {
            return Err(ModuleError::InvalidInput);
        }
        for rule in self.rules.iter().filter(|v| v.execution.is_ask() == ask) {
            match rule.matches(event, enriched, self.scope_source.get().map(Arc::as_ref))? {
                Match::Yes => return Ok(Some(rule)),
                Match::No => {}
                Match::Missing if !self.interaction.available() => {
                    return Err(ModuleError::Unavailable);
                }
                Match::Missing => return Ok(None),
            }
        }
        Ok(None)
    }
    fn fast(&self, event: &EventData) -> Result<FastMatch, ModuleError> {
        if event.stage() != EventStage::ControlledRequest
            || event.kind() != self.kind
            || event.payload.kind() != self.kind
        {
            return Err(ModuleError::InvalidInput);
        }
        for rule in self.rules.iter().filter(|v| !v.execution.is_ask()) {
            match rule.matches(event, false, self.scope_source.get().map(Arc::as_ref))? {
                Match::Yes => {
                    return Ok(FastMatch::Matched(rule.choice(
                        rule.action.decision().ok_or(ModuleError::InvalidInput)?,
                        ChoiceReason::Direct,
                    )));
                }
                Match::Missing => return Ok(FastMatch::NeedsFacts),
                Match::No => {}
            }
        }
        if self.interaction.available() {
            for rule in self.rules.iter().filter(|rule| rule.execution.is_ask()) {
                if !matches!(
                    rule.matches(event, false, self.scope_source.get().map(Arc::as_ref))?,
                    Match::No
                ) {
                    return Ok(FastMatch::NoMatch);
                }
            }
            return Ok(FastMatch::NoReviewMatch);
        }
        Ok(FastMatch::NoMatch)
    }
    async fn prompt(&self, request: ReviewRequest<'_>) -> Result<Option<RuleChoice>, ModuleError> {
        if !self.interaction.available() {
            return match self.fast(&request.event)? {
                FastMatch::Matched(choice) => Ok(Some(choice)),
                FastMatch::NeedsFacts => Err(ModuleError::Unavailable),
                FastMatch::NoMatch | FastMatch::NoReviewMatch => Ok(Some(RuleChoice {
                    decision: Decision::Allow,
                    hit: None,
                    reason: ChoiceReason::NoInteraction,
                })),
            };
        }
        if let Some(rule) = self.find(&request.event, true, false)? {
            request
                .matched
                .set(rule.hit.clone())
                .map_err(|_| ModuleError::InvalidInput)?;
            return Ok(rule
                .action
                .decision()
                .map(|d| rule.choice(d, ChoiceReason::Direct)));
        }
        let Some(rule) = self.find(&request.event, true, true)? else {
            return Ok(None);
        };
        request
            .matched
            .set(rule.hit.clone())
            .map_err(|_| ModuleError::InvalidInput)?;
        let Action::Ask {
            timeout_ms,
            timeout,
        } = &rule.action
        else {
            return Err(ModuleError::InvalidInput);
        };
        let (decision, reason) = self
            .interaction
            .ask(request, rule.hit.clone(), timeout_ms.get(), *timeout)
            .await?;
        Ok(Some(rule.choice(decision, reason)))
    }
}
/// Process family implementation selected by the factory.
pub struct ProcessRules(Core);
/// File family implementation selected by the factory.
pub struct FileRules(Core);
/// Network family implementation selected by the factory.
pub struct NetworkRules(Core);
macro_rules! rules_module {
    ($($ty:ty),+) => {$(
        #[async_trait::async_trait]
        impl RulesModule for $ty {
            fn can_prompt(&self) -> bool { self.0.interaction.available() }
            fn fast(&self,event:&EventData)->Result<FastMatch,ModuleError>{self.0.fast(event)}
            async fn prompt(&self,request:ReviewRequest<'_>)->Result<Option<RuleChoice>,ModuleError>{self.0.prompt(request).await}
        }
    )+};
}
rules_module!(ProcessRules, FileRules, NetworkRules);

/// Immutable validated driver contract. This does not claim actual kernel installation.
struct DriverPolicy {
    process: Vec<CompiledRule>,
    file: Vec<CompiledRule>,
    network: Vec<CompiledRule>,
}
/// Reference driver decision; ToUser is distinct from allowing an operation.
#[cfg(test)]
struct DriverMatch {
    /// Matched action, or ToUser on a definite miss.
    pub action: Action,
    /// Actual hit; a miss never invents a rule identity.
    pub hit: Option<RuleHit>,
}
#[cfg(test)]
impl DriverPolicy {
    /// Validate all rules and export only driver-stage groups for a later platform adapter.
    pub fn compile(document: &PolicyDocument) -> Result<Self, Error> {
        Ok(compile_groups(document)?.driver)
    }
    /// Shared ordered semantics for adapter validation, not a replacement user-mode driver.
    pub fn evaluate(&self, event: &EventData) -> Result<DriverMatch, ModuleError> {
        if event.stage() != EventStage::ControlledRequest || event.kind() != event.payload.kind() {
            return Err(ModuleError::InvalidInput);
        }
        let pool = match event.kind() {
            EventKind::Process => &self.process,
            EventKind::File => &self.file,
            EventKind::Network => &self.network,
        };
        for rule in pool {
            match rule.matches(event, false, None)? {
                Match::Yes => {
                    return Ok(DriverMatch {
                        action: rule.action.clone(),
                        hit: Some(rule.hit.clone()),
                    });
                }
                Match::No => {}
                Match::Missing => return Err(ModuleError::Unavailable),
            }
        }
        Ok(DriverMatch {
            action: Action::ToUser,
            hit: None,
        })
    }
}

struct Snapshot {
    document: Arc<PolicyDocument>,
    version: RuleVersion,
    process: Arc<ProcessRules>,
    file: Arc<FileRules>,
    network: Arc<NetworkRules>,
    driver: DriverPolicy,
}
impl Snapshot {
    fn select(&self, kind: EventKind) -> Arc<dyn RulesModule> {
        match kind {
            EventKind::Process => self.process.clone(),
            EventKind::File => self.file.clone(),
            EventKind::Network => self.network.clone(),
        }
    }
}
struct CompiledDocument {
    version: RuleVersion,
    groups: Vec<(EventKind, Vec<CompiledRule>)>,
    driver: DriverPolicy,
}
fn document_version(document: &PolicyDocument) -> Result<RuleVersion, Error> {
    let encoded = serde_saphyr::to_string(document).map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(RuleVersion::from_digest(
        Sha256::digest(encoded.as_bytes()).into(),
    ))
}
fn compile_groups(document: &PolicyDocument) -> Result<CompiledDocument, Error> {
    let version = document_version(document)?;
    let mut compiler = Compiler::new(version);
    let mut groups = Vec::new();
    for group in &document.groups {
        groups.push(match group {
            RuleGroup::Process(g) => (EventKind::Process, compiler.group(g, 1)?),
            RuleGroup::File(g) => (EventKind::File, compiler.group(g, 2)?),
            RuleGroup::Network(g) => (EventKind::Network, compiler.group(g, 3)?),
        });
    }
    let mut driver = DriverPolicy {
        process: Vec::new(),
        file: Vec::new(),
        network: Vec::new(),
    };
    let mut backend = Vec::new();
    for (kind, rules) in groups {
        let (native, rules): (Vec<_>, Vec<_>) = rules
            .into_iter()
            .partition(|rule| rule.execution.is_driver());
        match kind {
            EventKind::Process => driver.process.extend(native),
            EventKind::File => driver.file.extend(native),
            EventKind::Network => driver.network.extend(native),
        }
        backend.push((kind, rules));
    }
    for pool in [&mut driver.process, &mut driver.file, &mut driver.network] {
        pool.sort_by_key(|r| (r.hit.group_number, r.hit.rule_number));
    }
    Ok(CompiledDocument {
        version,
        groups: backend,
        driver,
    })
}
fn build(
    document: PolicyDocument,
    interaction: Arc<Interaction>,
    scope_source: ScopeSource,
) -> Result<Snapshot, Error> {
    let CompiledDocument {
        version,
        groups,
        driver,
    } = compile_groups(&document)?;
    let mut process = Vec::new();
    let mut file = Vec::new();
    let mut network = Vec::new();
    for (kind, rules) in groups {
        match kind {
            EventKind::Process => process.extend(rules),
            EventKind::File => file.extend(rules),
            EventKind::Network => network.extend(rules),
        }
    }
    let core = |kind, mut rules: Vec<CompiledRule>| {
        rules.sort_by_key(|r| (r.execution, r.hit.group_number, r.hit.rule_number));
        Core {
            kind,
            rules,
            interaction: interaction.clone(),
            scope_source: scope_source.clone(),
        }
    };
    Ok(Snapshot {
        document: Arc::new(document),
        version,
        process: Arc::new(ProcessRules(core(EventKind::Process, process))),
        file: Arc::new(FileRules(core(EventKind::File, file))),
        network: Arc::new(NetworkRules(core(EventKind::Network, network))),
        driver,
    })
}
/// Management read result from a single atomic snapshot load.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PolicySnapshot {
    /// Source bound when this rule engine was initialized.
    #[serde(serialize_with = "as_types::context::serialize_native_path")]
    pub source: PathBuf,
    /// The exact immutable source document for this version.
    pub document: Arc<PolicyDocument>,
    /// Version used for optimistic management writes.
    pub version: RuleVersion,
}
/// Required effect following a real rule source save.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleEffect {
    /// Coordinate the selected version with the current driver before claiming overall adoption.
    ApplyDriver,
}
/// Actual source save/publication, independent of current-driver adoption.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuleWriteReceipt {
    /// Exact visible rule document version.
    pub saved_version: RuleVersion,
    /// Owners that still need to confirm their effect.
    pub required_effects: Vec<RuleEffect>,
}
/// Sole user-policy publisher. Matches are lock-free, while update/reload serialize source-bound management.
pub struct PolicyEngine {
    active: Arc<ArcSwap<Snapshot>>,
    management: Arc<tokio::sync::Mutex<()>>,
    interaction: Arc<Interaction>,
    source: PathBuf,
    paths: Arc<as_types::context::ConfigPaths>,
    directory: Arc<File>,
    history: std::sync::Mutex<Vec<std::sync::Weak<Snapshot>>>,
    scope_source: ScopeSource,
}
impl PolicyEngine {
    /// Read and persist canonical maintenance rules at the one supplied fixed source.
    /// Root discovery remains with configuration; compilation runs off the business threads.
    pub async fn initialize(
        context: &as_types::context::MainContext,
        interaction: Arc<Interaction>,
    ) -> Result<Self, Error> {
        let paths = context.paths().clone();
        let initializing_paths = paths.clone();
        let path = paths.rules_file().to_path_buf();
        let shared = interaction.clone();
        let scope_source = ScopeSource::default();
        let initial_scope = scope_source.clone();
        let (source, directory, snapshot) = tokio::task::spawn_blocking(move || {
            if path.file_name() != Some(std::ffi::OsStr::new("rules.yaml")) {
                return Err(Error::Invalid("fixed rule filename required".into()));
            }
            let directory = Arc::new(open_directory(&path)?);
            let original = read(&directory)?;
            let document = maintenance::normalize(original.clone(), &initializing_paths)?;
            let changed = document != original;
            let snapshot = build(document, shared, initial_scope)?;
            if changed {
                check_binding(&path, &directory)?;
                if read(&directory)? != original {
                    return Err(Error::Conflict);
                }
                write(&directory, &snapshot.document)?;
                directory.sync_all().map_err(|source| Error::Durability {
                    saved_version: snapshot.version,
                    source,
                })?;
            }
            Ok::<_, Error>((path, directory, snapshot))
        })
        .await
        .map_err(|_| Error::Worker)??;
        Ok(Self {
            active: Arc::new(ArcSwap::from_pointee(snapshot)),
            management: Arc::new(tokio::sync::Mutex::new(())),
            interaction,
            source,
            paths,
            directory,
            history: std::sync::Mutex::new(Vec::new()),
            scope_source,
        })
    }
    /// Bind the authenticated, read-only scope evidence for all snapshots in this lifetime.
    pub fn bind_scope_source(&self, source: Arc<dyn ProcessFactsPort>) -> Result<(), Error> {
        self.scope_source.set(source).map_err(|_| Error::Conflict)
    }
    /// Retain the exact immutable policy for driver requests until that generation drains.
    #[cfg(test)]
    fn pin(&self) -> PolicyLease {
        PolicyLease(self.active.load_full())
    }
    fn retain_version(&self) -> Result<(), Error> {
        let mut versions = self.history.lock().map_err(|_| Error::Worker)?;
        versions.retain(|v| v.strong_count() != 0);
        let current = self.active.load_full();
        if !versions
            .iter()
            .any(|v| std::sync::Weak::ptr_eq(v, &Arc::downgrade(&current)))
        {
            versions.push(Arc::downgrade(&current));
        }
        Ok(())
    }
    /// Read document and version together from one immutable snapshot.
    pub fn snapshot(&self) -> PolicySnapshot {
        let snapshot = self.active.load();
        PolicySnapshot {
            source: self.source.clone(),
            document: snapshot.document.clone(),
            version: snapshot.version,
        }
    }
    /// Validate, atomically save, then publish one authorized management update.
    /// The actual job owns the guard until completion even if its requester disconnects.
    pub async fn update(
        &self,
        expected: RuleVersion,
        document: PolicyDocument,
    ) -> Result<RuleWriteReceipt, Error> {
        let serial = self.management.clone().lock_owned().await;
        let interaction = self.interaction.clone();
        let scope_source = self.scope_source.clone();
        let active = self.active.clone();
        self.retain_version()?;
        let path = self.source.clone();
        let directory = self.directory.clone();
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || {
            let _serial = serial;
            if active.load().version != expected {
                return Err(Error::Conflict);
            }
            check_binding(&path, &directory)?;
            if document_version(&read(&directory)?)? != expected {
                return Err(Error::Conflict);
            }
            let candidate = build(
                maintenance::normalize(document, &paths)?,
                interaction,
                scope_source,
            )?;
            write(&directory, &candidate.document)?;
            let version = candidate.version;
            active.store(Arc::new(candidate));
            directory.sync_all().map_err(|source| Error::Durability {
                saved_version: version,
                source,
            })?;
            Ok(RuleWriteReceipt {
                saved_version: version,
                required_effects: vec![RuleEffect::ApplyDriver],
            })
        })
        .await
        .map_err(|_| Error::Worker)?
    }
    /// Reload the server-configured source, refusing a stale management request.
    pub async fn reload(&self, expected: RuleVersion) -> Result<PolicySnapshot, Error> {
        let serial = self.management.clone().lock_owned().await;
        let path = self.source.clone();
        let directory = self.directory.clone();
        let interaction = self.interaction.clone();
        let scope_source = self.scope_source.clone();
        let active = self.active.clone();
        let paths = self.paths.clone();
        self.retain_version()?;
        tokio::task::spawn_blocking(move || {
            let _serial = serial;
            if active.load().version != expected {
                return Err(Error::Conflict);
            }
            check_binding(&path, &directory)?;
            let original = read(&directory)?;
            let document = maintenance::normalize(original.clone(), &paths)?;
            let changed = document != original;
            let candidate = build(document, interaction, scope_source)?;
            if changed {
                check_binding(&path, &directory)?;
                if read(&directory)? != original {
                    return Err(Error::Conflict);
                }
                write(&directory, &candidate.document)?;
            }
            let version = candidate.version;
            let document = candidate.document.clone();
            active.store(Arc::new(candidate));
            if changed {
                directory.sync_all().map_err(|source| Error::Durability {
                    saved_version: version,
                    source,
                })?;
            }
            Ok(PolicySnapshot {
                source: path,
                document,
                version,
            })
        })
        .await
        .map_err(|_| Error::Worker)?
    }
}
/// Immutable generation lease, owned by the driver while requests can still name it.
#[derive(Clone)]
pub struct PolicyLease(Arc<Snapshot>);
impl PolicyLease {
    /// Rule generation carried by native requests.
    pub fn version(&self) -> RuleVersion {
        self.0.version
    }
    /// Resolve fixed feedback in this exact retained snapshot without evaluating an event again.
    /// Forged identity, ordering, logging or already-retrieved text is rejected.
    pub fn resolve_hit_feedback(&self, hit: &RuleHit) -> Result<RuleHit, ModuleError> {
        if hit.source != RuleSourceVersion::RulesDocument(self.0.version) {
            return Err(ModuleError::InvalidInput);
        }
        let rule = [
            self.0.driver.process.as_slice(),
            self.0.driver.file.as_slice(),
            self.0.driver.network.as_slice(),
            self.0.process.0.rules.as_slice(),
            self.0.file.0.rules.as_slice(),
            self.0.network.0.rules.as_slice(),
        ]
        .into_iter()
        .flatten()
        .find(|rule| rule.hit.group_id == hit.group_id && rule.hit.rule_id == hit.rule_id)
        .ok_or(ModuleError::InvalidInput)?;
        let original = &rule.hit;
        if original.group_number != hit.group_number
            || original.rule_number != hit.rule_number
            || original.log != hit.log
            || hit
                .deny_message
                .as_ref()
                .is_some_and(|text| original.deny_message.as_ref() != Some(text))
        {
            return Err(ModuleError::InvalidInput);
        }
        Ok(original.clone())
    }
}
impl RulesFactory for PolicyEngine {
    fn select_version(
        &self,
        kind: EventKind,
        version: Option<RuleVersion>,
    ) -> Result<Arc<dyn RulesModule>, ModuleError> {
        let current = self.active.load_full();
        let snapshot = match version {
            None => current,
            Some(expected) if current.version == expected => current,
            Some(expected) => self
                .history
                .lock()
                .map_err(|_| ModuleError::Unavailable)?
                .iter()
                .filter_map(std::sync::Weak::upgrade)
                .find(|v| v.version == expected)
                .ok_or(ModuleError::Unavailable)?,
        };
        Ok(snapshot.select(kind))
    }

    fn select(&self, kind: EventKind) -> Arc<dyn RulesModule> {
        self.active.load().select(kind)
    }
}
fn open_directory(path: &Path) -> Result<File, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("rule source has no parent".into()))?;
    Ok(File::from(
        openat2(
            rustix::fs::CWD,
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map_err(std::io::Error::from)?,
    ))
}
fn check_binding(path: &Path, directory: &File) -> Result<(), Error> {
    use std::os::unix::fs::MetadataExt;
    let current = open_directory(path)?.metadata()?;
    let bound = directory.metadata()?;
    if current.dev() != bound.dev() || current.ino() != bound.ino() {
        return Err(Error::Invalid("fixed rules directory was replaced".into()));
    }
    Ok(())
}
fn read(directory: &File) -> Result<PolicyDocument, Error> {
    use std::io::Read;
    let mut file = File::from(
        openat2(
            directory,
            "rules.yaml",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        )
        .map_err(std::io::Error::from)?,
    );
    if !file.metadata()?.is_file() {
        return Err(Error::Invalid("rule source is not a regular file".into()));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    serde_saphyr::from_str(&text).map_err(|e| Error::Invalid(e.to_string()))
}
fn write(directory: &File, document: &PolicyDocument) -> Result<(), Error> {
    use std::io::Write;
    let bound = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let mut temporary = tempfile::NamedTempFile::new_in(&bound)?;
    let text = serde_saphyr::to_string(document).map_err(|e| Error::Invalid(e.to_string()))?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(bound.join("rules.yaml"))
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
