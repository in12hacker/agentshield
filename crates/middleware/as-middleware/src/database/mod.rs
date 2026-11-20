//! Original SQLite connection, transaction boundary and actual admitted-work lifetime.
mod attribution;
mod chat;
mod event;
pub(crate) mod group;
pub(crate) mod group_gateway;
mod native_execution;
mod native_handoff;
mod parts;
pub(crate) mod query;
mod relations;
mod retention;
mod row;
mod schema;
#[cfg(test)]
mod session_tests;
pub(crate) mod sessions;
mod source;
mod stream;
#[cfg(test)]
mod tests;

use as_types::context::ConfigPaths;
use rusqlite::Connection;
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;
use tokio_util::task::TaskTracker;

/// Explicit store failure; no bodies, secrets or paths are included in public errors.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Actual SQLite operation failed; no Stored acknowledgement was produced.
    #[error("business database operation failed")]
    Sql(#[source] rusqlite::Error),
    /// The fixed location could not be initialized.
    #[error("business database filesystem operation failed")]
    Io(#[source] std::io::Error),
    /// Original structured data conflicts with already stored facts.
    #[error("conflicting business evidence")]
    Conflict,
    /// A required original fact was not supplied or stored.
    #[error("original business fact not provided")]
    NotProvided,
    /// Stored column type or source shape is invalid; no empty success is substituted.
    #[error("invalid structured business fact")]
    Invalid,
    /// The real database owner has stopped admission.
    #[error("business database is closed")]
    Closed,
    /// A started SQL worker did not return a confirmed result.
    #[error("business database work completion is unconfirmed")]
    Unconfirmed,
    /// The one physical close attempt returned a real error.
    #[error("business database close failed")]
    CloseFailed(#[source] Arc<StoreError>),
}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::Invalid
    }
}

/// One actual committed intake record; this is not an execution, frontend or model receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StoredReceipt {
    /// SQLite-local positive commit record identifier, not a source sequence or causal ID.
    pub record: i64,
}
/// An input was committed, or is explicitly outside LLM/interaction business logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "receipt", rename_all = "snake_case")]
pub enum RecordOutcome {
    /// Actual transaction commit.
    Stored(StoredReceipt),
    /// Ordinary unassociated system audit remains in the backend.
    NotApplicable,
}

type CloseResult = Option<Result<(), Arc<StoreError>>>;

struct Inner {
    database: Mutex<Option<Connection>>,
    closing: Mutex<Option<watch::Receiver<CloseResult>>>,
    work: TaskTracker,
    write_failures: AtomicU64,
    failed: tokio::sync::Notify,
    chat_changed: Arc<tokio::sync::Notify>,
    budget: Arc<as_types::log_storage::LogStorageBudget>,
    path: std::path::PathBuf,
    pins: Mutex<std::collections::BTreeMap<String, usize>>,
    questions: Mutex<std::collections::BTreeSet<[u8; 16]>>,
}
/// Shared capability to one physical connection, without a second writer, queue or history cache.
#[derive(Clone)]
pub struct BusinessLog {
    inner: Arc<Inner>,
}
impl BusinessLog {
    /// Initialize only configuration's already resolved fixed business database.
    pub async fn initialize(
        paths: &ConfigPaths,
        budget: Arc<as_types::log_storage::LogStorageBudget>,
    ) -> Result<Self, StoreError> {
        let path = paths.middleware_database_file().to_path_buf();
        tokio::task::spawn_blocking(move || Self::open_budget(&path, budget))
            .await
            .map_err(|_| StoreError::Unconfirmed)?
    }
    #[cfg(test)]
    fn open_path(path: &Path) -> Result<Self, StoreError> {
        Self::open_budget(
            path,
            as_types::log_storage::LogStorageBudget::new(Default::default()),
        )
    }
    fn open_budget(
        path: &Path,
        budget: Arc<as_types::log_storage::LogStorageBudget>,
    ) -> Result<Self, StoreError> {
        let parent = path.parent().ok_or(StoreError::Invalid)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(parent)?;
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA auto_vacuum=FULL;")?;
        let vacuum: u32 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
        let wal: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        if wal != "wal" {
            return Err(StoreError::Invalid);
        }
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;",
        )?;
        schema::initialize(&conn)?;
        if vacuum != 1 {
            conn.execute_batch("VACUUM;")?;
        }
        retention::initialize(&conn)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let inner = Arc::new(Inner {
            database: Mutex::new(Some(conn)),
            closing: Mutex::new(None),
            work: TaskTracker::new(),
            write_failures: AtomicU64::new(0),
            failed: tokio::sync::Notify::new(),
            chat_changed: Arc::new(tokio::sync::Notify::new()),
            budget: budget.clone(),
            path: path.to_owned(),
            pins: Mutex::new(std::collections::BTreeMap::new()),
            questions: Mutex::new(std::collections::BTreeSet::new()),
        });
        let participant: Arc<dyn as_types::log_storage::LogRetention> = inner.clone();
        budget.bind(
            as_types::log_storage::LogDatabase::Business,
            path,
            Arc::downgrade(&participant),
        )?;
        Ok(Self { inner })
    }
    fn write_result<T>(&self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if let Err(error) = &result {
            // Preserve the owned local cause before a caller cancellation or failure wakeup.
            // Public Display/wire errors remain the stable category, not SQLite diagnostics.
            // The admitted SQL work still holds its tracker token while this
            // finite failure diagnostic is handed off. Never wait on stdio
            // while holding the physical connection or before waking the host.
            self.inner
                .work
                .spawn(as_types::diagnostics::record_stderr(format!(
                    "AS_BUSINESS_WRITE_FAILED code={error} source={error:?}"
                )));
            self.inner.write_failures.fetch_add(1, Ordering::AcqRel);
            self.inner.failed.notify_waiters();
        }
        result
    }
    /// Actual failed writes, independent of frontend delivery and ordinary query misses.
    pub fn write_failure_count(&self) -> u64 {
        self.inner.write_failures.load(Ordering::Acquire)
    }
    /// Wake the real host after a failed business write; no content or success is synthesized.
    pub async fn write_failed(&self) {
        loop {
            let failed = self.inner.failed.notified();
            if self.write_failure_count() != 0 {
                return;
            }
            failed.await;
        }
    }
    // Account at actual SQL completion, even if the async caller was dropped after admission.
    pub(crate) fn write_call<T, F>(
        &self,
        work: F,
    ) -> Result<
        impl std::future::Future<Output = Result<T, StoreError>> + Send + 'static + use<T, F>,
        StoreError,
    >
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        self.job(work, true, true)
    }
    pub(crate) fn call<T, F>(
        &self,
        work: F,
    ) -> Result<
        impl std::future::Future<Output = Result<T, StoreError>> + Send + 'static + use<T, F>,
        StoreError,
    >
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        self.job(work, false, false)
    }
    fn job<T, F>(
        &self,
        work: F,
        writing: bool,
        count_failure: bool,
    ) -> Result<
        impl std::future::Future<Output = Result<T, StoreError>> + Send + 'static + use<T, F>,
        StoreError,
    >
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        // Register under the same admission lock used by close, before returning
        // the waiter. Dropping or never polling the waiter cannot cancel SQL.
        let job = {
            let admission = self
                .inner
                .closing
                .lock()
                .map_err(|_| StoreError::Unconfirmed)?;
            if admission.is_some() {
                return Err(StoreError::Closed);
            }
            tokio::runtime::Handle::try_current().map_err(|_| StoreError::Unconfirmed)?;
            let inner = self.inner.clone();
            let owner = self.clone();
            self.inner.work.spawn_blocking(move || {
                let result = (|| {
                    let mut coordination = if writing {
                        Some(inner.budget.write()?)
                    } else {
                        None
                    };
                    let result = {
                        let mut connection =
                            inner.database.lock().map_err(|_| StoreError::Unconfirmed)?;
                        let connection = connection.as_mut().ok_or(StoreError::Closed)?;
                        if writing {
                            retention::begin_write(connection)?;
                        }
                        let result = work(connection);
                        if writing
                            && result.is_ok()
                            && let Err(error) = retention::checkpoint(connection)
                        {
                            if let Some(coordination) = coordination.as_mut() {
                                coordination.deferred();
                            }
                            tracing::warn!(?error, "AS_LOG_RETENTION_DEFERRED checkpoint");
                        }
                        result
                    }?;
                    if let Some(mut coordination) = coordination
                        && let Err(error) = coordination.enforce()
                    {
                        tracing::warn!(?error, "AS_LOG_RETENTION_DEFERRED quota");
                    }
                    Ok(result)
                })();
                if count_failure {
                    owner.write_result(result)
                } else {
                    result
                }
            })
        };
        Ok(async move { job.await.map_err(|_| StoreError::Unconfirmed)? })
    }
    /// Start one owner-driven close and observe the shared physical result.
    pub fn close(
        &self,
    ) -> impl std::future::Future<Output = Result<(), StoreError>> + Send + 'static + use<> {
        let closing = self.start_close();
        async move {
            let mut closing = closing?;
            let observed = closing
                .wait_for(Option::is_some)
                .await
                .map_err(|_| StoreError::Unconfirmed)?;
            match observed.as_ref() {
                Some(Ok(())) => Ok(()),
                Some(Err(error)) if matches!(error.as_ref(), StoreError::Unconfirmed) => {
                    Err(StoreError::Unconfirmed)
                }
                Some(Err(error)) => Err(StoreError::CloseFailed(error.clone())),
                None => Err(StoreError::Unconfirmed),
            }
        }
    }

    fn start_close(&self) -> Result<watch::Receiver<CloseResult>, StoreError> {
        let mut admission = self
            .inner
            .closing
            .lock()
            .map_err(|_| StoreError::Unconfirmed)?;
        if let Some(closing) = admission.as_ref() {
            return Ok(closing.clone());
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| StoreError::Unconfirmed)?;
        let (finished, closing) = watch::channel(None);
        *admission = Some(closing.clone());
        self.inner.work.close();
        self.inner.chat_changed.notify_waiters();
        let inner = self.inner.clone();
        runtime.spawn(async move {
            inner.work.wait().await;
            let closed = tokio::task::spawn_blocking(move || {
                let mut owner = inner.database.lock().map_err(|_| StoreError::Unconfirmed)?;
                match owner.take() {
                    Some(conn) => conn.close().map_err(|(_, error)| StoreError::Sql(error)),
                    None => Ok(()),
                }
            })
            .await
            .map_err(|_| StoreError::Unconfirmed)
            .and_then(|result| result);
            finished.send_replace(Some(closed.map_err(Arc::new)));
        });
        Ok(closing)
    }
}

pub(crate) fn record(conn: &Connection, kind: &str) -> Result<i64, StoreError> {
    conn.execute("INSERT INTO deliveries(kind) VALUES(?1)", [kind])?;
    Ok(conn.last_insert_rowid())
}
fn key<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}
/// Necessary live operation references, released with that exact operation.
pub(crate) struct RetentionPin {
    inner: std::sync::Weak<Inner>,
    references: Vec<String>,
}
impl Drop for RetentionPin {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        if let Ok(mut pins) = inner.pins.lock() {
            for reference in &self.references {
                if let Some(count) = pins.get_mut(reference) {
                    if *count > 1 {
                        *count -= 1;
                    } else {
                        pins.remove(reference);
                    }
                }
            }
        }
    }
}
impl BusinessLog {
    pub(crate) fn retain_facts(
        &self,
        references: &[crate::context::FactReference],
    ) -> Result<RetentionPin, StoreError> {
        let references = references
            .iter()
            .map(key)
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?
            .into_iter()
            .collect::<Vec<_>>();
        let mut pins = self
            .inner
            .pins
            .lock()
            .map_err(|_| StoreError::Unconfirmed)?;
        for reference in &references {
            let count = pins.entry(reference.clone()).or_default();
            *count = count.checked_add(1).ok_or(StoreError::Unconfirmed)?;
        }
        Ok(RetentionPin {
            inner: Arc::downgrade(&self.inner),
            references,
        })
    }
}
