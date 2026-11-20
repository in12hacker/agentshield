//! Persistent consumer of the original security-audit queue.

use as_types::log_storage::{LogDatabase, LogRetention, LogStorageBudget};
use as_types::{
    context::ConfigPaths,
    main_ports::{ModuleError, SecurityLogEntry, SecurityLogSink},
};
use rusqlite::{Connection, OpenFlags, params};
use rustix::fs::{self, FileType, Mode, OFlags, ResolveFlags};
use serde::{Deserialize, Serialize};
use std::{
    fmt, io,
    path::Path,
    sync::{Arc, Mutex, TryLockError, Weak},
};
use tokio::sync::mpsc;

const INSERT: &str = "INSERT INTO rule_logs (record) VALUES (?1)";

/// Owns one connection to the configuration-derived security audit file.
pub struct SecurityLog {
    database: Arc<Mutex<Option<Connection>>>,
    _retention: Arc<AuditRetention>,
    budget: Arc<LogStorageBudget>,
}
struct AuditRetention {
    database: Arc<Mutex<Option<Connection>>>,
}
impl LogRetention for AuditRetention {
    fn retire_oldest(&self) -> io::Result<bool> {
        let guard = self
            .database
            .lock()
            .map_err(|_| io::Error::other("security log state"))?;
        let Some(conn) = guard.as_ref() else {
            return Ok(false);
        };
        // Keep the newest original write; a single oversize value reports real capacity failure.
        Ok(conn.execute("DELETE FROM rule_logs WHERE id=(SELECT min(id) FROM rule_logs) AND id<(SELECT max(id) FROM rule_logs)", []).map_err(io::Error::other)? != 0)
    }
}

/// Query one bounded page of committed original log records.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityLogQuery {
    /// Exclusive upper row boundary, or the current newest committed row.
    #[serde(default, with = "as_types::decimal::optional")]
    pub before: Option<u64>,
    /// Positive page size, at most 64.
    pub limit: u16,
}

/// An original committed record with its local ordering key.
#[derive(Serialize)]
pub struct SecurityLogRecord {
    /// The database's positive integer row identity.
    #[serde(with = "as_types::decimal")]
    pub id: u64,
    /// The original log value reconstructed from the one durable source.
    pub record: SecurityLogEntry,
}

/// The requested page and the exclusive boundary for an older page.
#[derive(Serialize)]
pub struct SecurityLogPage {
    /// Original records, newest first.
    pub records: Vec<SecurityLogRecord>,
    /// Last returned row if more rows exist.
    #[serde(with = "as_types::decimal::optional")]
    pub next_before: Option<u64>,
}

impl fmt::Debug for SecurityLogPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityLogPage")
            .field("record_count", &self.records.len())
            .field("next_before", &self.next_before)
            .finish()
    }
}

/// Read only while the original security log owner is live.
#[derive(Clone)]
pub struct SecurityLogReader(Weak<Mutex<Option<Connection>>>);

impl SecurityLogReader {
    /// Read committed values on the original connection, without waiting behind another query.
    pub async fn page(&self, query: SecurityLogQuery) -> Result<SecurityLogPage, ModuleError> {
        if !(1..=64).contains(&query.limit) {
            return Err(ModuleError::InvalidInput);
        }
        let upper = match query.before {
            None => i64::MAX,
            Some(before) => {
                i64::try_from(before)
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or(ModuleError::InvalidInput)?
                    - 1
            }
        };
        let state = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let state = state.upgrade().ok_or(ModuleError::Stopped)?;
            let (rows, more) = {
                let guard = state.try_lock().map_err(|error| match error {
                    TryLockError::WouldBlock => ModuleError::Busy,
                    TryLockError::Poisoned(_) => ModuleError::Unavailable,
                })?;
                let connection = guard.as_ref().ok_or(ModuleError::Stopped)?;
                read_page(connection, upper, query.limit).map_err(|_| ModuleError::Unavailable)?
            };
            let records = rows
                .into_iter()
                .map(|(id, json)| {
                    let id = u64::try_from(id)
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or(ModuleError::Unavailable)?;
                    let record =
                        serde_json::from_str(&json).map_err(|_| ModuleError::Unavailable)?;
                    Ok(SecurityLogRecord { id, record })
                })
                .collect::<Result<Vec<_>, ModuleError>>()?;
            let next_before = records.last().filter(|_| more).map(|row| row.id);
            Ok(SecurityLogPage {
                records,
                next_before,
            })
        })
        .await
        .map_err(|_| ModuleError::Unavailable)?
    }
}

fn read_page(
    connection: &Connection,
    upper: i64,
    limit: u16,
) -> rusqlite::Result<(Vec<(i64, String)>, bool)> {
    let mut statement = connection
        .prepare("SELECT id,record FROM rule_logs WHERE id<=?1 ORDER BY id DESC LIMIT ?2")?;
    let mut rows = statement.query(params![upper, limit + 1])?;
    let mut records = Vec::with_capacity(usize::from(limit));
    while records.len() < usize::from(limit) {
        let Some(row) = rows.next()? else {
            break;
        };
        records.push((row.get(0)?, row.get(1)?));
    }
    Ok((records, rows.next()?.is_some()))
}

impl SecurityLog {
    /// Borrow the one durable source through its read-only operation.
    pub fn reader(&self) -> SecurityLogReader {
        SecurityLogReader(Arc::downgrade(&self.database))
    }

    /// Prepare the one fixed durable log before admitting native work.
    pub async fn open(paths: &ConfigPaths, budget: Arc<LogStorageBudget>) -> io::Result<Self> {
        let root = paths.self_protection_root().to_owned();
        let path = paths.security_log_file().to_owned();
        let opened_path = path.clone();
        let connection = tokio::task::spawn_blocking(move || open(&root, &opened_path))
            .await
            .map_err(io::Error::other)??;
        let database = Arc::new(Mutex::new(Some(connection)));
        let retention = Arc::new(AuditRetention {
            database: database.clone(),
        });
        let participant: Arc<dyn LogRetention> = retention.clone();
        budget.bind(LogDatabase::Security, &path, Arc::downgrade(&participant))?;
        Ok(Self {
            database,
            _retention: retention,
            budget,
        })
    }

    async fn write<T: Send + 'static>(
        &self,
        task: impl FnOnce(&Mutex<Option<Connection>>) -> io::Result<T> + Send + 'static,
    ) -> io::Result<T> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || task(&database))
            .await
            .map_err(io::Error::other)?
    }
}

fn open(root: &Path, path: &Path) -> io::Result<Connection> {
    let parent = path.parent().ok_or_else(invalid_location)?;
    let directory_name = parent.strip_prefix(root).map_err(|_| invalid_location())?;
    let mut components = directory_name.components();
    if !matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err(invalid_location());
    }
    let uid = rustix::process::geteuid().as_raw();
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let root = fs::openat2(
        fs::CWD,
        root,
        directory_flags,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?;
    let root_stat = fs::fstat(&root)?;
    if root_stat.st_uid != uid || root_stat.st_mode & 0o022 != 0 {
        return Err(invalid_location());
    }
    match fs::mkdirat(&root, directory_name, Mode::from_raw_mode(0o700)) {
        Ok(()) => fs::fsync(&root)?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    let directory = fs::openat(&root, directory_name, directory_flags, Mode::empty())?;
    let directory_stat = fs::fstat(&directory)?;
    if directory_stat.st_uid != uid || directory_stat.st_mode & 0o077 != 0 {
        return Err(invalid_location());
    }
    let file = fs::openat(
        &directory,
        path.file_name().ok_or_else(invalid_location)?,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::from_raw_mode(0o600),
    )?;
    let stat = fs::fstat(&file)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != uid
        || stat.st_nlink != 1
        || stat.st_mode & 0o077 != 0
    {
        return Err(invalid_location());
    }
    drop(file);

    // SQLite also opens journal files. Its pathname and sidecars stay in the
    // private directory checked above, which no other UID can mutate.
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(io::Error::other)?;
    connection
        .execute_batch("PRAGMA auto_vacuum=FULL;")
        .map_err(io::Error::other)?;
    let auto: u32 = connection
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .map_err(io::Error::other)?;
    if auto != 1 {
        connection
            .execute_batch("VACUUM;")
            .map_err(io::Error::other)?;
    }
    let journal: String = connection
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(io::Error::other)?;
    if journal != "delete" {
        return Err(io::Error::other(
            "security log rollback journal unavailable",
        ));
    }
    connection
        .execute_batch(
            "PRAGMA synchronous=EXTRA;
             PRAGMA trusted_schema=OFF;
             CREATE TABLE IF NOT EXISTS rule_logs (
                 id INTEGER PRIMARY KEY, record TEXT NOT NULL
             ) STRICT;",
        )
        .map_err(io::Error::other)?;
    connection.prepare(INSERT).map_err(io::Error::other)?;
    fs::fsync(&directory)?;
    Ok(connection)
}

fn commit(database: &Mutex<Option<Connection>>, entry: SecurityLogEntry) -> io::Result<()> {
    let record = serde_json::to_string(&entry).map_err(io::Error::other)?;
    let guard = database
        .lock()
        .map_err(|_| io::Error::other("security log state"))?;
    let connection = guard
        .as_ref()
        .ok_or_else(|| io::Error::other("security log closed"))?;
    connection
        .execute(INSERT, [record])
        .map_err(io::Error::other)?;
    Ok(())
}

fn invalid_location() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid security log location")
}

#[async_trait::async_trait]
impl SecurityLogSink for SecurityLog {
    async fn run(self: Box<Self>, mut logs: mpsc::Receiver<SecurityLogEntry>) -> io::Result<()> {
        let writing = async {
            while let Some(record) = logs.recv().await {
                let budget = self.budget.clone();
                self.write(move |database| {
                    let mut coordination = budget.write()?;
                    commit(database, record)?;
                    if let Err(error) = coordination.enforce() {
                        tracing::warn!(?error, "AS_LOG_RETENTION_DEFERRED quota");
                    }
                    Ok(())
                })
                .await?;
            }
            Ok(())
        }
        .await;
        let closed = self
            .write(|database| {
                database
                    .lock()
                    .map_err(|_| io::Error::other("security log state"))?
                    .take()
                    .map_or(Ok(()), |connection| {
                        connection
                            .close()
                            .map_err(|(_, error)| io::Error::other(error))
                    })
            })
            .await;
        writing.and(closed)
    }
}
