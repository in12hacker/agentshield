//! Shared admission of the two existing log owners; no writer, thread or content store.
use crate::configuration::LogStorageSettings;
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

/// The two fixed participants; this is not an extensible service registry.
#[derive(Clone, Copy)]
pub enum LogDatabase {
    Business,
    Security,
}
/// Operations performed only by the original owner on its own SQLite connection.
pub trait LogRetention: Send + Sync {
    /// Retire one oldest complete eligible local unit and reclaim its physical pages.
    fn retire_oldest(&self) -> io::Result<bool>;
}
struct Participant {
    owner: Weak<dyn LogRetention>,
    path: std::path::PathBuf,
}
struct Participants {
    business: Option<Participant>,
    security: Option<Participant>,
}
/// Main-composed shared budget, with one blocking-owner coordination lock.
pub struct LogStorageBudget {
    limit: u64,
    participants: Mutex<Participants>,
    deferred: AtomicU64,
}
impl LogStorageBudget {
    /// Use the one configuration-owned value for both original database owners.
    pub fn new(settings: LogStorageSettings) -> Arc<Self> {
        Arc::new(Self {
            limit: settings.total_bytes.get(),
            participants: Mutex::new(Participants {
                business: None,
                security: None,
            }),
            deferred: AtomicU64::new(0),
        })
    }
    /// Bind one existing owner before intake; weak references do not extend database lifetime.
    pub fn bind(
        &self,
        kind: LogDatabase,
        path: &Path,
        owner: Weak<dyn LogRetention>,
    ) -> io::Result<()> {
        let mut ports = self
            .participants
            .lock()
            .map_err(|_| io::Error::other("log storage coordination poisoned"))?;
        let slot = match kind {
            LogDatabase::Business => &mut ports.business,
            LogDatabase::Security => &mut ports.security,
        };
        if slot.as_ref().and_then(|p| p.owner.upgrade()).is_some() {
            return Err(io::Error::other("log storage owner already bound"));
        }
        *slot = Some(Participant {
            owner,
            path: path.to_owned(),
        });
        Ok(())
    }
    /// Serialize database writes before taking either original connection lock.
    pub fn write(&self) -> io::Result<LogStorageWrite<'_>> {
        Ok(LogStorageWrite {
            limit: self.limit,
            deferred: &self.deferred,
            ports: self
                .participants
                .lock()
                .map_err(|_| io::Error::other("log storage coordination poisoned"))?,
        })
    }
    /// Physical current usage and deferred maintenance, separate from original SQL commits.
    pub fn usage(&self) -> io::Result<LogStorageUsage> {
        let ports = self
            .participants
            .lock()
            .map_err(|_| io::Error::other("log storage coordination poisoned"))?;
        let business = ports
            .business
            .as_ref()
            .map_or(Ok(0), |p| sqlite_file_bytes(&p.path))?;
        let security = ports
            .security
            .as_ref()
            .map_or(Ok(0), |p| sqlite_file_bytes(&p.path))?;
        Ok(LogStorageUsage {
            business_bytes: business,
            security_bytes: security,
            total_limit_bytes: self.limit,
            deferred_maintenance: self.deferred_maintenance(),
        })
    }
    /// Nonblocking diagnostic counter; reading it never waits behind SQL reclamation.
    pub fn deferred_maintenance(&self) -> u64 {
        self.deferred.load(Ordering::Acquire)
    }
}
/// Actual SQLite file usage; no checkpoint/eviction receipt is an execution receipt.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct LogStorageUsage {
    /// Business database and SQLite sidecars, in physical bytes.
    pub business_bytes: u64,
    /// Security audit database and SQLite sidecars, in physical bytes.
    pub security_bytes: u64,
    /// One combined configuration limit, in bytes.
    pub total_limit_bytes: u64,
    /// Reclamation attempts not completed; independent of committed original writes.
    pub deferred_maintenance: u64,
}
/// Held only inside an already-owned blocking SQL job, never by the HIPS caller.
pub struct LogStorageWrite<'a> {
    limit: u64,
    deferred: &'a AtomicU64,
    ports: MutexGuard<'a, Participants>,
}
impl LogStorageWrite<'_> {
    /// Converge committed physical retention; each callback locks only its own database.
    pub fn enforce(&mut self) -> io::Result<()> {
        let result = self.enforce_inner();
        if result.is_err() {
            self.deferred();
        }
        result
    }
    /// A temporary reclaim/checkpoint error never rewrites an already committed result.
    pub fn deferred(&mut self) {
        let _ = self
            .deferred
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                Some(v.saturating_add(1))
            });
    }
    fn enforce_inner(&self) -> io::Result<()> {
        let business = self.ports.business.as_ref().and_then(|p| p.owner.upgrade());
        let security = self.ports.security.as_ref().and_then(|p| p.owner.upgrade());
        loop {
            // A stopped owner's retained file still consumes the shared budget.
            let b = self
                .ports
                .business
                .as_ref()
                .map_or(Ok(0), |p| sqlite_file_bytes(&p.path))?;
            let s = self
                .ports
                .security
                .as_ref()
                .map_or(Ok(0), |p| sqlite_file_bytes(&p.path))?;
            if b.checked_add(s).ok_or_else(capacity)? <= self.limit {
                return Ok(());
            }
            let (first, second) = if b >= s {
                (&business, &security)
            } else {
                (&security, &business)
            };
            if first.as_ref().map_or(Ok(false), |p| p.retire_oldest())? {
                continue;
            }
            if second.as_ref().map_or(Ok(false), |p| p.retire_oldest())? {
                continue;
            }
            return Err(capacity());
        }
    }
}
fn capacity() -> io::Error {
    io::Error::new(
        io::ErrorKind::StorageFull,
        "combined log storage capacity cannot retain the current write",
    )
}
/// Count only the fixed database and SQLite-owned sidecars, never adjacent evidence directories.
pub fn sqlite_file_bytes(path: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let bytes = match std::fs::metadata(Path::new(&name)) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        total = total.checked_add(bytes).ok_or_else(capacity)?;
    }
    Ok(total)
}
