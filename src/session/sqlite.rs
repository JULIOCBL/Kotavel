use super::{
    SessionContextId, SessionInstanceId, SessionLinkGrant, SessionMutation, SessionRecord,
    SessionStore, generate_session_id, valid_session_id,
};
use crate::filesystem::{Filesystem, LocalFilesystem, restrict_existing_file};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAXIMUM_CONTEXTS: i64 = 4096;
const MAXIMUM_CONTEXT_GRANTS: i64 = 32;
const MAXIMUM_CONTEXT_VALUES: i64 = 1024;
const MAXIMUM_CONTEXT_BYTES: i64 = 4 * 1024 * 1024;
const MAXIMUM_CONTEXT_VALUE_BYTES: usize = 256 * 1024;
const MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP: i64 = 256;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    Random(getrandom::Error),
    WorkerStopped,
    BatchFailed(String),
    InvalidOptions(&'static str),
    SessionTooLarge { actual: usize, maximum: usize },
    ResourceLimit(&'static str),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite session store: {error}"),
            Self::Json(error) => write!(formatter, "Session JSON: {error}"),
            Self::Io(error) => write!(formatter, "Session database I/O: {error}"),
            Self::Random(error) => write!(formatter, "Session ID random source: {error}"),
            Self::WorkerStopped => formatter.write_str("SQLite session writer stopped"),
            Self::BatchFailed(message) => write!(formatter, "SQLite write batch failed: {message}"),
            Self::InvalidOptions(message) => {
                write!(formatter, "Invalid session store options: {message}")
            }
            Self::SessionTooLarge { actual, maximum } => write!(
                formatter,
                "Session data is {actual} bytes; configured maximum is {maximum} bytes"
            ),
            Self::ResourceLimit(message) => write!(formatter, "Session context limit: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
pub struct StoreOptions {
    pub read_connections: usize,
    pub write_queue_capacity: usize,
    pub busy_timeout: Duration,
    pub write_batch_window: Duration,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            read_connections: 4,
            write_queue_capacity: 1024,
            busy_timeout: Duration::from_secs(5),
            write_batch_window: Duration::ZERO,
        }
    }
}

type JobRun = Box<dyn FnOnce(&mut Connection) -> bool + Send + 'static>;

struct WriteJob {
    run: Option<JobRun>,
    complete: Box<dyn FnOnce() + Send + 'static>,
    fail: Box<dyn FnOnce(StoreError) + Send + 'static>,
    batchable: bool,
    submitted_at: Instant,
}

#[derive(Default)]
struct WriterCounters {
    write_jobs: AtomicU64,
    queue_wait_ns: AtomicU64,
    max_queue_wait_ns: AtomicU64,
    service_ns: AtomicU64,
    max_service_ns: AtomicU64,
    outstanding: AtomicUsize,
    peak_outstanding: AtomicUsize,
    write_batches: AtomicU64,
    batched_write_jobs: AtomicU64,
    max_batch_size: AtomicUsize,
}

/// Snapshot of SQLite writer queue and durable execution activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriterStatistics {
    /// Jobs submitted to the bounded SQLite writer.
    pub write_jobs: u64,
    /// Total duration from submission to the start of execution, including
    /// backpressure while waiting for queue admission and configured batch delay.
    pub queue_wait: Duration,
    pub max_queue_wait: Duration,
    /// Total time spent executing single jobs and complete batches on the writer.
    pub service_time: Duration,
    /// Longest single job or batch execution interval.
    pub max_service_time: Duration,
    /// Jobs queued, being admitted, or executing when sampled.
    pub outstanding_jobs: usize,
    /// Highest observed number of queued, admitted, or executing jobs.
    pub peak_outstanding_jobs: usize,
    /// Number of SQLite transactions used for batched writes.
    pub write_batches: u64,
    /// Jobs included in batched write transactions.
    pub batched_write_jobs: u64,
    /// Largest number of jobs included in one batch.
    pub max_batch_size: usize,
}

impl WriterCounters {
    fn snapshot(&self) -> WriterStatistics {
        WriterStatistics {
            write_jobs: self.write_jobs.load(Ordering::Relaxed),
            queue_wait: Duration::from_nanos(self.queue_wait_ns.load(Ordering::Relaxed)),
            max_queue_wait: Duration::from_nanos(self.max_queue_wait_ns.load(Ordering::Relaxed)),
            service_time: Duration::from_nanos(self.service_ns.load(Ordering::Relaxed)),
            max_service_time: Duration::from_nanos(self.max_service_ns.load(Ordering::Relaxed)),
            outstanding_jobs: self.outstanding.load(Ordering::Relaxed),
            peak_outstanding_jobs: self.peak_outstanding.load(Ordering::Relaxed),
            write_batches: self.write_batches.load(Ordering::Relaxed),
            batched_write_jobs: self.batched_write_jobs.load(Ordering::Relaxed),
            max_batch_size: self.max_batch_size.load(Ordering::Relaxed),
        }
    }

    fn update_max(target: &AtomicU64, candidate: u64) {
        target.fetch_max(candidate, Ordering::Relaxed);
    }

    fn update_max_size(target: &AtomicUsize, candidate: usize) {
        target.fetch_max(candidate, Ordering::Relaxed);
    }

    fn track_queue_wait(&self, submitted_at: Instant) {
        let elapsed = submitted_at.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.queue_wait_ns.fetch_add(elapsed, Ordering::Relaxed);
        Self::update_max(&self.max_queue_wait_ns, elapsed);
    }

    fn track_service(&self, started_at: Instant) {
        let elapsed = started_at.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.service_ns.fetch_add(elapsed, Ordering::Relaxed);
        Self::update_max(&self.max_service_ns, elapsed);
    }

    fn finish_job(&self) {
        self.outstanding.fetch_sub(1, Ordering::Relaxed);
    }
}

enum WriterMessage {
    Run(WriteJob),
    Stop,
}

struct ReaderPool {
    available: Mutex<Vec<Connection>>,
    ready: Condvar,
}

impl ReaderPool {
    fn acquire(self: &Arc<Self>) -> ReaderLease {
        let mut connections = self.available.lock().expect("SQLite reader pool poisoned");
        while connections.is_empty() {
            connections = self
                .ready
                .wait(connections)
                .expect("SQLite reader pool poisoned");
        }
        ReaderLease {
            pool: Arc::clone(self),
            connection: connections.pop(),
        }
    }
}

struct ReaderLease {
    pool: Arc<ReaderPool>,
    connection: Option<Connection>,
}

impl std::ops::Deref for ReaderLease {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.connection.as_ref().expect("leased connection")
    }
}

impl Drop for ReaderLease {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool
                .available
                .lock()
                .expect("SQLite reader pool poisoned")
                .push(connection);
            self.pool.ready.notify_one();
        }
    }
}

struct Inner {
    path: PathBuf,
    instance_id: String,
    writer: SyncSender<WriterMessage>,
    readers: Arc<ReaderPool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    writer_counters: Arc<WriterCounters>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.writer.send(WriterMessage::Stop);
        if let Some(worker) = self
            .worker
            .get_mut()
            .expect("SQLite writer handle poisoned")
            .take()
        {
            let _ = worker.join();
        }
    }
}

/// SQLite WAL store with a bounded single-writer queue and pooled read-only
/// connections. The queue applies backpressure instead of growing indefinitely.
#[derive(Clone)]
pub struct SQLiteSessionStore {
    inner: Arc<Inner>,
}

impl SQLiteSessionStore {
    pub fn open(path: impl AsRef<Path>, options: StoreOptions) -> Result<Self, StoreError> {
        if options.read_connections == 0 {
            return Err(StoreError::InvalidOptions(
                "read_connections must be positive",
            ));
        }
        if options.write_queue_capacity == 0 {
            return Err(StoreError::InvalidOptions(
                "write_queue_capacity must be positive",
            ));
        }
        if options.write_batch_window > Duration::from_micros(10_000) {
            return Err(StoreError::InvalidOptions(
                "write_batch_window must not exceed 10000 microseconds",
            ));
        }

        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if !parent.exists() {
                LocalFilesystem.create_directories(parent, true)?;
            } else if !parent.is_dir() {
                return Err(StoreError::InvalidOptions(
                    "session database parent path is not a directory",
                ));
            }
        }

        let mut writer = Connection::open(&path)?;
        writer.busy_timeout(options.busy_timeout)?;
        writer.pragma_update(None, "foreign_keys", "ON")?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        writer.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY NOT NULL,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revision INTEGER NOT NULL,
                data TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS sessions_expires_at ON sessions(expires_at);
             CREATE TABLE IF NOT EXISTS kotavel_session_meta (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_contexts (
                context_id TEXT PRIMARY KEY NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_context_grants (
                grant_token TEXT PRIMARY KEY NOT NULL,
                context_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS session_context_links (
                session_id TEXT PRIMARY KEY NOT NULL,
                context_id TEXT NOT NULL,
                linked_at INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS session_context_values (
                context_id TEXT NOT NULL,
                key TEXT NOT NULL,
                data TEXT NOT NULL,
                expires_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(context_id, key),
                FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS session_context_grants_expiry
                ON session_context_grants(expires_at, context_id);
             CREATE INDEX IF NOT EXISTS session_context_values_expiry
                ON session_context_values(expires_at, context_id);",
        )?;
        let generated_instance_id = generate_session_id()?;
        writer.execute(
            "INSERT OR IGNORE INTO kotavel_session_meta(key, value) VALUES('instance_id', ?1)",
            [&generated_instance_id],
        )?;
        let instance_id = writer.query_row(
            "SELECT value FROM kotavel_session_meta WHERE key='instance_id'",
            [],
            |row| row.get(0),
        )?;
        restrict_existing_file(&path)?;

        let mut readers = Vec::with_capacity(options.read_connections);
        for _ in 0..options.read_connections {
            let connection = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
            )?;
            connection.busy_timeout(options.busy_timeout)?;
            readers.push(connection);
        }
        let readers = Arc::new(ReaderPool {
            available: Mutex::new(readers),
            ready: Condvar::new(),
        });

        let (sender, receiver) = mpsc::sync_channel(options.write_queue_capacity);
        let batch_window = options.write_batch_window;
        let writer_counters = Arc::new(WriterCounters::default());
        let worker_counters = Arc::clone(&writer_counters);
        let worker = thread::Builder::new()
            .name("kotavel-session-sqlite-writer".into())
            .spawn(move || writer_loop(&mut writer, receiver, batch_window, worker_counters))?;

        Ok(Self {
            inner: Arc::new(Inner {
                path,
                instance_id,
                writer: sender,
                readers,
                worker: Mutex::new(Some(worker)),
                writer_counters,
            }),
        })
    }

    pub fn instance_id(&self) -> SessionInstanceId {
        SessionInstanceId {
            value: self.inner.instance_id.clone(),
        }
    }

    pub fn create_context(&self) -> Result<SessionContextId, StoreError> {
        let context_id = generate_session_id()?;
        let created_at = now_seconds();
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let count: i64 =
                transaction.query_row("SELECT count(*) FROM session_contexts", [], |row| {
                    row.get(0)
                })?;
            if count >= MAXIMUM_CONTEXTS {
                return Err(StoreError::ResourceLimit(
                    "maximum number of shared contexts reached",
                ));
            }
            transaction.execute(
                "INSERT INTO session_contexts(context_id, created_at) VALUES(?1, ?2)",
                params![context_id, created_at],
            )?;
            transaction.commit()?;
            Ok(SessionContextId { value: context_id })
        })
    }

    pub fn default_context(&self) -> Result<SessionContextId, StoreError> {
        let generated_id = generate_session_id()?;
        let created_at = now_seconds();
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT value FROM kotavel_session_meta WHERE key='default_context_id'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(context_id) = existing {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM session_contexts WHERE context_id=?1)",
                    [&context_id],
                    |row| row.get(0),
                )?;
                if exists {
                    transaction.commit()?;
                    return Ok(SessionContextId { value: context_id });
                }
            }

            let count: i64 =
                transaction.query_row("SELECT count(*) FROM session_contexts", [], |row| {
                    row.get(0)
                })?;
            if count >= MAXIMUM_CONTEXTS {
                return Err(StoreError::ResourceLimit(
                    "maximum number of shared contexts reached",
                ));
            }
            transaction.execute(
                "INSERT INTO session_contexts(context_id, created_at) VALUES(?1, ?2)",
                params![generated_id, created_at],
            )?;
            transaction.execute(
                "INSERT INTO kotavel_session_meta(key, value) VALUES('default_context_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&generated_id],
            )?;
            transaction.commit()?;
            Ok(SessionContextId {
                value: generated_id,
            })
        })
    }

    pub fn delete_context(&self, context: &SessionContextId) -> Result<bool, StoreError> {
        if !valid_session_id(&context.value) {
            return Ok(false);
        }
        let context_id = context.value.clone();
        self.write(move |connection| {
            Ok(connection.execute(
                "DELETE FROM session_contexts WHERE context_id=?1",
                [context_id],
            )? == 1)
        })
    }

    pub fn issue_link_grant(
        &self,
        context: &SessionContextId,
        lifetime: Duration,
    ) -> Result<SessionLinkGrant, StoreError> {
        if !valid_session_id(&context.value) {
            return Err(StoreError::InvalidOptions(
                "invalid session context identifier",
            ));
        }
        if lifetime.is_zero() || lifetime > Duration::from_secs(600) {
            return Err(StoreError::InvalidOptions(
                "session link grant lifetime must be between 1 second and 10 minutes",
            ));
        }
        let token = generate_session_id()?;
        let context_id = context.value.clone();
        let expires_at = now_seconds().saturating_add(duration_seconds(lifetime));
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let now = now_seconds();
            transaction.execute(
                "DELETE FROM session_context_grants WHERE grant_token IN
                 (SELECT grant_token FROM session_context_grants WHERE expires_at<=?1
                  ORDER BY expires_at LIMIT 256)",
                [now],
            )?;
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_contexts WHERE context_id=?1)",
                [&context_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StoreError::InvalidOptions("unknown session context"));
            }
            let active: i64 = transaction.query_row(
                "SELECT count(*) FROM session_context_grants WHERE context_id=?1 AND expires_at>?2",
                params![context_id, now],
                |row| row.get(0),
            )?;
            if active >= MAXIMUM_CONTEXT_GRANTS {
                return Err(StoreError::ResourceLimit(
                    "too many active link grants for this context",
                ));
            }
            transaction.execute(
                "INSERT INTO session_context_grants(grant_token, context_id, expires_at)
                 VALUES(?1, ?2, ?3)",
                params![token, context_id, expires_at],
            )?;
            transaction.commit()?;
            Ok(SessionLinkGrant { value: token })
        })
    }

    pub fn redeem_link_grant(
        &self,
        grant: &SessionLinkGrant,
        session_id: &str,
    ) -> Result<bool, StoreError> {
        if !valid_session_id(&grant.value) || !valid_session_id(session_id) {
            return Ok(false);
        }
        let token = grant.value.clone();
        let session_id = session_id.to_owned();
        let now = now_seconds();
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let active_session: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND expires_at>?2)",
                params![session_id, now],
                |row| row.get(0),
            )?;
            if !active_session {
                return Ok(false);
            }
            let context_id: Option<String> = transaction
                .query_row(
                    "SELECT context_id FROM session_context_grants
                     WHERE grant_token=?1 AND expires_at>?2",
                    params![token, now],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(context_id) = context_id else {
                return Ok(false);
            };
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO session_context_links(session_id, context_id, linked_at)
                 VALUES(?1, ?2, ?3)",
                params![session_id, context_id, now],
            )? == 1;
            if !inserted {
                return Ok(false);
            }
            transaction.execute(
                "DELETE FROM session_context_grants WHERE grant_token=?1",
                [token],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn context_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionContextId>, StoreError> {
        if !valid_session_id(session_id) {
            return Ok(None);
        }
        let reader = self.inner.readers.acquire();
        let context_id: Option<String> = reader
            .query_row(
                "SELECT link.context_id FROM session_context_links link
                 JOIN sessions session ON session.id=link.session_id
                 WHERE link.session_id=?1 AND session.expires_at>?2",
                params![session_id, now_seconds()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(context_id.map(|value| SessionContextId { value }))
    }

    pub fn revoke_context_link(&self, session_id: &str) -> Result<bool, StoreError> {
        if !valid_session_id(session_id) {
            return Ok(false);
        }
        let session_id = session_id.to_owned();
        self.write(move |connection| {
            Ok(connection.execute(
                "DELETE FROM session_context_links WHERE session_id=?1",
                [session_id],
            )? == 1)
        })
    }

    pub fn put_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
        value: serde_json::Value,
        lifetime: Option<Duration>,
    ) -> Result<(), StoreError> {
        if !valid_session_id(&context.value) {
            return Err(StoreError::InvalidOptions(
                "invalid session context identifier",
            ));
        }
        let context_id = context.value.clone();
        let (key, data, expires_at) = prepare_context_value(key, value, lifetime)?;
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            write_context_value(&transaction, &context_id, &key, &data, expires_at)?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Store a value only if the session is still actively linked. Link
    /// validation and value update share one immediate SQLite transaction, so a
    /// concurrent revocation cannot race through the authorization check.
    pub fn put_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
        value: serde_json::Value,
        lifetime: Option<Duration>,
    ) -> Result<bool, StoreError> {
        if !valid_session_id(session_id) {
            return Ok(false);
        }
        let (key, data, expires_at) = prepare_context_value(key, value, lifetime)?;
        let session_id = session_id.to_owned();
        self.write(move |connection| {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let now = now_seconds();
            let context_id: Option<String> = transaction
                .query_row(
                    "SELECT link.context_id FROM session_context_links link
                     JOIN sessions session ON session.id=link.session_id
                     WHERE link.session_id=?1 AND session.expires_at>?2",
                    params![session_id, now],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(context_id) = context_id else {
                return Ok(false);
            };
            write_context_value(&transaction, &context_id, &key, &data, expires_at)?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub fn get_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        if !valid_session_id(&context.value) || key.is_empty() || key.len() > 256 {
            return Ok(None);
        }
        let reader = self.inner.readers.acquire();
        let data: Option<String> = reader
            .query_row(
                "SELECT data FROM session_context_values WHERE context_id=?1 AND key=?2
                 AND (expires_at=0 OR expires_at>?3)",
                params![context.value, key, now_seconds()],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|encoded| serde_json::from_str(&encoded).map_err(StoreError::from))
            .transpose()
    }

    pub fn get_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError> {
        if !valid_session_id(session_id) || key.is_empty() || key.len() > 256 {
            return Ok(None);
        }
        let reader = self.inner.readers.acquire();
        let data: Option<String> = reader
            .query_row(
                "SELECT value.data FROM session_context_links link
                 JOIN sessions session ON session.id=link.session_id
                 JOIN session_context_values value ON value.context_id=link.context_id
                 WHERE link.session_id=?1 AND session.expires_at>?2 AND value.key=?3
                 AND (value.expires_at=0 OR value.expires_at>?2)",
                params![session_id, now_seconds(), key],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|encoded| serde_json::from_str(&encoded).map_err(StoreError::from))
            .transpose()
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Returns cumulative write queue and durable execution statistics.
    pub fn statistics(&self) -> WriterStatistics {
        self.inner.writer_counters.snapshot()
    }

    fn enqueue(&self, job: WriteJob) -> Result<(), StoreError> {
        let counters = &self.inner.writer_counters;
        counters.write_jobs.fetch_add(1, Ordering::Relaxed);
        let outstanding = counters.outstanding.fetch_add(1, Ordering::Relaxed) + 1;
        WriterCounters::update_max_size(&counters.peak_outstanding, outstanding);
        if self.inner.writer.send(WriterMessage::Run(job)).is_err() {
            counters.finish_job();
            return Err(StoreError::WorkerStopped);
        }
        Ok(())
    }

    fn write<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (complete, result) = mpsc::sync_channel(1);
        let result_slot = Arc::new(Mutex::new(None));
        let run_slot = Arc::clone(&result_slot);
        let complete_slot = Arc::clone(&result_slot);
        self.enqueue(WriteJob {
            run: Some(Box::new(move |connection| {
                *run_slot.lock().expect("SQLite write result poisoned") =
                    Some(operation(connection));
                true
            })),
            complete: Box::new(move || {
                let outcome = complete_slot
                    .lock()
                    .expect("SQLite write result poisoned")
                    .take()
                    .unwrap_or(Err(StoreError::WorkerStopped));
                let _ = complete.send(outcome);
            }),
            fail: Box::new(|_| {}),
            batchable: false,
            submitted_at: Instant::now(),
        })?;
        result.recv().map_err(|_| StoreError::WorkerStopped)?
    }

    fn write_batched<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (complete, result) = mpsc::sync_channel(1);
        let result_slot = Arc::new(Mutex::new(None));
        let run_slot = Arc::clone(&result_slot);
        let complete_slot = Arc::clone(&result_slot);
        let run_complete = complete.clone();
        self.enqueue(WriteJob {
            run: Some(Box::new(move |connection| {
                let outcome = operation(connection);
                let succeeded = outcome.is_ok();
                *run_slot.lock().expect("SQLite write result poisoned") = Some(outcome);
                succeeded
            })),
            complete: Box::new(move || {
                let outcome = complete_slot
                    .lock()
                    .expect("SQLite write result poisoned")
                    .take()
                    .unwrap_or(Err(StoreError::WorkerStopped));
                let _ = run_complete.send(outcome);
            }),
            fail: Box::new(move |error| {
                let _ = complete.send(Err(error));
            }),
            batchable: true,
            submitted_at: Instant::now(),
        })?;
        result.recv().map_err(|_| StoreError::WorkerStopped)?
    }
}

impl SessionStore for SQLiteSessionStore {
    fn load(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
        let id = id.to_owned();
        let reader = self.inner.readers.acquire();
        let encoded: Option<(i64, i64, i64, i64, String)> = reader
            .query_row(
                "SELECT created_at, last_activity, expires_at, revision, data FROM sessions WHERE id = ?1",
                [id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        encoded
            .map(|(created_at, last_activity, expires_at, revision, data)| {
                Ok(SessionRecord {
                    id,
                    created_at,
                    last_activity,
                    expires_at,
                    revision: u64::try_from(revision)
                        .map_err(|_| StoreError::InvalidOptions("negative stored revision"))?,
                    values: serde_json::from_str(&data)?,
                })
            })
            .transpose()
    }

    fn commit(&self, record: &mut SessionRecord) -> Result<bool, StoreError> {
        let next = record.clone();
        let encoded = serde_json::to_string(&next.values)?;
        let expected_revision = next.revision;
        let new_revision = expected_revision
            .checked_add(1)
            .ok_or(StoreError::InvalidOptions("session revision exhausted"))?;
        let changed = self.write_batched(move |connection| {
            let changed = if expected_revision == 0 {
                connection.execute(
                    "INSERT OR IGNORE INTO sessions(id, created_at, last_activity, expires_at, revision, data)
                     VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                    params![next.id, next.created_at, next.last_activity, next.expires_at, encoded],
                )? == 1
            } else {
                let expected = i64::try_from(expected_revision)
                    .map_err(|_| StoreError::InvalidOptions("revision exceeds SQLite INTEGER"))?;
                let new_revision = i64::try_from(new_revision)
                    .map_err(|_| StoreError::InvalidOptions("revision exceeds SQLite INTEGER"))?;
                let count = connection.execute(
                    "UPDATE sessions SET last_activity = ?1, expires_at = ?2, revision = ?3, data = ?4
                     WHERE id = ?5 AND revision = ?6",
                    params![next.last_activity, next.expires_at, new_revision, encoded, next.id, expected],
                )?;
                count == 1
            };
            Ok(changed)
        })?;
        if changed {
            record.revision = new_revision;
        }
        Ok(changed)
    }

    fn commit_mutation(
        &self,
        record: &mut SessionRecord,
        original: &SessionRecord,
        mutation: &SessionMutation,
        maximum_bytes: usize,
    ) -> Result<bool, StoreError> {
        let proposed = record.clone();
        let original = original.clone();
        let mutation = mutation.clone();
        let committed = self.write_batched(move |connection| {
            apply_sqlite_session_mutation(
                connection,
                &proposed,
                &original,
                &mutation,
                maximum_bytes,
            )
        })?;
        let Some(committed) = committed else {
            return Ok(false);
        };
        *record = committed;
        Ok(true)
    }

    fn commit_rotation(
        &self,
        replacement: &mut SessionRecord,
        expected_previous_revision: u64,
        retired_ids: &[String],
    ) -> Result<bool, StoreError> {
        if !valid_session_id(&replacement.id) || retired_ids.is_empty() {
            return Ok(false);
        }
        if replacement.created_at < 0 || replacement.last_activity < 0 || replacement.expires_at < 0
        {
            return Err(StoreError::InvalidOptions(
                "session timestamps must be nonnegative",
            ));
        }
        let expected = i64::try_from(expected_previous_revision)
            .map_err(|_| StoreError::InvalidOptions("session revision exceeds SQLite INTEGER"))?;
        let retired = retired_ids.to_vec();
        if retired
            .iter()
            .any(|id| !valid_session_id(id) || id == &replacement.id)
        {
            return Ok(false);
        }
        let next = replacement.clone();
        let encoded = serde_json::to_string(&next.values)?;
        let now = now_seconds();
        let committed = self.write_batched(move |connection| {
            for (index, retired_id) in retired.iter().enumerate() {
                let previous: Option<(i64, i64)> = connection
                    .query_row(
                        "SELECT revision, expires_at FROM sessions WHERE id=?1",
                        [retired_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                match (index, expected_previous_revision, previous) {
                    (0, 0, Some(_)) => return Ok(false),
                    (0, 0, None) => {}
                    (0, _, Some((revision, expires_at)))
                        if revision == expected && expires_at > now => {}
                    (0, _, _) => return Ok(false),
                    (_, _, Some(_)) => return Ok(false),
                    (_, _, None) => {}
                }
            }
            let replacement_exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                [&next.id],
                |row| row.get(0),
            )?;
            if replacement_exists {
                return Ok(false);
            }
            connection.execute(
                "INSERT INTO sessions(id, created_at, last_activity, expires_at, revision, data)
                 VALUES(?1, ?2, ?3, ?4, 1, ?5)",
                params![
                    next.id,
                    next.created_at,
                    next.last_activity,
                    next.expires_at,
                    encoded
                ],
            )?;
            for (index, retired_id) in retired.iter().enumerate() {
                connection.execute(
                    "DELETE FROM session_context_links WHERE session_id=?1",
                    [retired_id],
                )?;
                let removed =
                    connection.execute("DELETE FROM sessions WHERE id=?1", [retired_id])?;
                if index == 0 && expected_previous_revision != 0 && removed != 1 {
                    return Err(StoreError::InvalidOptions(
                        "previous session disappeared during atomic rotation",
                    ));
                }
            }
            Ok(true)
        })?;
        if committed {
            replacement.revision = 1;
        }
        Ok(committed)
    }

    fn remove(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_owned();
        self.write(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "DELETE FROM session_context_links WHERE session_id=?1",
                [id.as_str()],
            )?;
            transaction.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
            transaction.commit()?;
            Ok(())
        })
    }

    fn collect_expired(&self, now: i64) -> Result<usize, StoreError> {
        self.write(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "DELETE FROM session_context_links WHERE session_id IN
                 (SELECT id FROM sessions WHERE expires_at<=?1
                  ORDER BY expires_at, id LIMIT ?2)",
                params![now, MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP],
            )?;
            let removed = transaction.execute(
                "DELETE FROM sessions WHERE id IN
                 (SELECT id FROM sessions WHERE expires_at<=?1
                  ORDER BY expires_at, id LIMIT ?2)",
                params![now, MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP],
            )?;
            transaction.execute(
                "DELETE FROM session_context_grants WHERE rowid IN
                 (SELECT rowid FROM session_context_grants WHERE expires_at<=?1
                  ORDER BY expires_at LIMIT 256)",
                [now],
            )?;
            transaction.execute(
                "DELETE FROM session_context_values WHERE rowid IN
                 (SELECT rowid FROM session_context_values WHERE expires_at>0 AND expires_at<=?1
                  ORDER BY expires_at LIMIT 256)",
                [now],
            )?;
            transaction.commit()?;
            Ok(removed)
        })
    }
}

/// Apply a request's session delta against the latest SQLite revision while
/// holding the writer transaction. This avoids sending stale compare-and-swap
/// writes back through the queue when many requests increment one hot session.
fn apply_sqlite_session_mutation(
    connection: &mut Connection,
    proposed: &SessionRecord,
    original: &SessionRecord,
    mutation: &SessionMutation,
    maximum_bytes: usize,
) -> Result<Option<SessionRecord>, StoreError> {
    let owns_transaction = connection.is_autocommit();
    if owns_transaction {
        connection.execute_batch("BEGIN IMMEDIATE")?;
    }
    if let Err(error) = connection.execute_batch("SAVEPOINT kotavel_session_mutation") {
        if owns_transaction {
            let _ = connection.execute_batch("ROLLBACK");
        }
        return Err(error.into());
    }

    let result = (|| -> Result<Option<SessionRecord>, StoreError> {
        if proposed.id != original.id {
            return Ok(None);
        }
        let stored: Option<(i64, i64, i64, i64, String)> = connection
            .query_row(
                "SELECT created_at, last_activity, expires_at, revision, data FROM sessions WHERE id=?1",
                [proposed.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let is_new = proposed.revision == 0;
        let mut current = if is_new {
            if stored.is_some() {
                return Ok(None);
            }
            original.clone()
        } else {
            let Some((created_at, last_activity, expires_at, revision, data)) = stored else {
                return Ok(None);
            };
            SessionRecord {
                id: proposed.id.clone(),
                created_at,
                last_activity,
                expires_at,
                revision: u64::try_from(revision)
                    .map_err(|_| StoreError::InvalidOptions("negative stored revision"))?,
                values: serde_json::from_str(&data)?,
            }
        };

        if mutation.exclusive && current.revision != original.revision {
            return Ok(None);
        }
        for key in mutation.changes.keys() {
            if !mutation.increments.contains_key(key)
                && current.values.get(key) != original.values.get(key)
            {
                return Ok(None);
            }
        }
        for (key, amount) in &mutation.increments {
            let current_value = match current.values.get(key) {
                None => 0,
                Some(value) => value.as_u64().ok_or(StoreError::InvalidOptions(
                    "session counter must be a nonnegative integer",
                ))?,
            };
            let next_value = current_value
                .checked_add(*amount)
                .ok_or(StoreError::InvalidOptions("session counter overflow"))?;
            current
                .values
                .insert(key.clone(), serde_json::Value::from(next_value));
        }
        for (key, value) in &mutation.changes {
            if mutation.increments.contains_key(key) {
                continue;
            }
            match value {
                Some(value) => {
                    current.values.insert(key.clone(), value.clone());
                }
                None => {
                    current.values.remove(key);
                }
            }
        }
        let encoded_values = serde_json::to_vec(&current.values)?;
        if encoded_values.len() > maximum_bytes {
            return Err(StoreError::SessionTooLarge {
                actual: encoded_values.len(),
                maximum: maximum_bytes,
            });
        }
        current.last_activity = proposed.last_activity;
        current.expires_at = proposed.expires_at;
        let next_revision = current
            .revision
            .checked_add(1)
            .ok_or(StoreError::InvalidOptions("session revision exhausted"))?;
        let sql_revision = i64::try_from(next_revision)
            .map_err(|_| StoreError::InvalidOptions("revision exceeds SQLite INTEGER"))?;
        let encoded_values = String::from_utf8(encoded_values)
            .expect("JSON serialization always produces valid UTF-8");

        let changed = if current.revision == 0 {
            connection.execute(
                "INSERT OR IGNORE INTO sessions(id, created_at, last_activity, expires_at, revision, data)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    current.id,
                    current.created_at,
                    current.last_activity,
                    current.expires_at,
                    sql_revision,
                    encoded_values
                ],
            )?
        } else {
            let previous_revision = i64::try_from(current.revision)
                .map_err(|_| StoreError::InvalidOptions("revision exceeds SQLite INTEGER"))?;
            connection.execute(
                "UPDATE sessions SET last_activity=?1, expires_at=?2, revision=?3, data=?4
                 WHERE id=?5 AND revision=?6",
                params![
                    current.last_activity,
                    current.expires_at,
                    sql_revision,
                    encoded_values,
                    current.id,
                    previous_revision
                ],
            )?
        };
        if changed != 1 {
            return Ok(None);
        }
        current.revision = next_revision;
        Ok(Some(current))
    })();

    match result {
        Ok(value) => {
            if let Err(error) = connection.execute_batch("RELEASE kotavel_session_mutation") {
                let _ = connection.execute_batch(
                    "ROLLBACK TO kotavel_session_mutation; RELEASE kotavel_session_mutation",
                );
                if owns_transaction {
                    let _ = connection.execute_batch("ROLLBACK");
                }
                return Err(error.into());
            }
            if owns_transaction {
                if let Err(error) = connection.execute_batch("COMMIT") {
                    let _ = connection.execute_batch("ROLLBACK");
                    return Err(error.into());
                }
            }
            Ok(value)
        }
        Err(error) => {
            let _ = connection.execute_batch(
                "ROLLBACK TO kotavel_session_mutation; RELEASE kotavel_session_mutation",
            );
            if owns_transaction {
                let _ = connection.execute_batch("ROLLBACK");
            }
            Err(error)
        }
    }
}

fn writer_loop(
    connection: &mut Connection,
    receiver: Receiver<WriterMessage>,
    batch_window: Duration,
    counters: Arc<WriterCounters>,
) {
    let mut pending = None;
    loop {
        let message = match pending.take() {
            Some(message) => message,
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            },
        };
        match message {
            WriterMessage::Run(first) if first.batchable => {
                let mut jobs = vec![first];
                let deadline = std::time::Instant::now() + batch_window;
                while jobs.len() < 64 && !batch_window.is_zero() {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match receiver.recv_timeout(remaining) {
                        Ok(WriterMessage::Run(job)) if job.batchable => jobs.push(job),
                        Ok(other) => {
                            pending = Some(other);
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                execute_batch(connection, jobs, &counters);
            }
            WriterMessage::Run(job) => execute_one(connection, job, &counters),
            WriterMessage::Stop => break,
        }
    }
}

fn execute_one(connection: &mut Connection, job: WriteJob, counters: &WriterCounters) {
    counters.track_queue_wait(job.submitted_at);
    let service_started = Instant::now();
    (job.run.expect("writer job is run once"))(connection);
    counters.track_service(service_started);
    counters.finish_job();
    (job.complete)();
}

fn execute_batch(connection: &mut Connection, mut jobs: Vec<WriteJob>, counters: &WriterCounters) {
    let batch_size = jobs.len();
    let service_started = Instant::now();
    for job in &jobs {
        counters.track_queue_wait(job.submitted_at);
    }
    let result = (|| -> Result<(), StoreError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        for job in &mut jobs {
            connection.execute_batch("SAVEPOINT kotavel_session_commit")?;
            if !(job.run.take().expect("writer job is run once"))(connection) {
                connection.execute_batch(
                    "ROLLBACK TO kotavel_session_commit; RELEASE kotavel_session_commit",
                )?;
            } else {
                connection.execute_batch("RELEASE kotavel_session_commit")?;
            }
        }
        connection.execute_batch("COMMIT")?;
        Ok(())
    })();
    counters.write_batches.fetch_add(1, Ordering::Relaxed);
    counters
        .batched_write_jobs
        .fetch_add(batch_size as u64, Ordering::Relaxed);
    WriterCounters::update_max_size(&counters.max_batch_size, batch_size);
    counters.track_service(service_started);

    match result {
        Ok(()) => {
            for job in jobs {
                counters.finish_job();
                (job.complete)();
            }
        }
        Err(error) => {
            let message = error.to_string();
            let _ = connection.execute_batch("ROLLBACK");
            for job in jobs {
                counters.finish_job();
                (job.fail)(StoreError::BatchFailed(message.clone()));
            }
        }
    }
}

fn prepare_context_value(
    key: &str,
    value: serde_json::Value,
    lifetime: Option<Duration>,
) -> Result<(String, String, i64), StoreError> {
    if key.is_empty() || key.len() > 256 {
        return Err(StoreError::InvalidOptions(
            "session context keys must contain between 1 and 256 bytes",
        ));
    }
    if lifetime
        .is_some_and(|value| value.is_zero() || value > Duration::from_secs(30 * 24 * 60 * 60))
    {
        return Err(StoreError::InvalidOptions(
            "session context value lifetime must be between 1 second and 30 days",
        ));
    }
    let data = serde_json::to_string(&value)?;
    if data.len() > MAXIMUM_CONTEXT_VALUE_BYTES {
        return Err(StoreError::ResourceLimit(
            "session context value exceeds 256 KiB",
        ));
    }
    let expires_at = lifetime
        .map(|lifetime| now_seconds().saturating_add(duration_seconds(lifetime)))
        .unwrap_or(0);
    Ok((key.to_owned(), data, expires_at))
}

fn write_context_value(
    transaction: &Transaction<'_>,
    context_id: &str,
    key: &str,
    data: &str,
    expires_at: i64,
) -> Result<(), StoreError> {
    let now = now_seconds();
    transaction.execute(
        "DELETE FROM session_context_values
         WHERE context_id=?1 AND expires_at>0 AND expires_at<=?2",
        params![context_id, now],
    )?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_contexts WHERE context_id=?1)",
        [context_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StoreError::InvalidOptions("unknown session context"));
    }
    let (count, bytes): (i64, i64) = transaction.query_row(
        "SELECT count(*), coalesce(sum(length(data)), 0) FROM session_context_values
         WHERE context_id=?1 AND key<>?2",
        params![context_id, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count >= MAXIMUM_CONTEXT_VALUES
        || bytes.saturating_add(data.len() as i64) > MAXIMUM_CONTEXT_BYTES
    {
        return Err(StoreError::ResourceLimit(
            "session context storage budget is exhausted",
        ));
    }
    transaction.execute(
        "INSERT INTO session_context_values(context_id, key, data, expires_at)
         VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(context_id, key) DO UPDATE
         SET data=excluded.data, expires_at=excluded.expires_at",
        params![context_id, key, data, expires_at],
    )?;
    Ok(())
}

fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn duration_seconds(duration: Duration) -> i64 {
    duration.as_secs().min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, SQLiteSessionStore) {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("sessions.sqlite3");
        let store = SQLiteSessionStore::open(database, StoreOptions::default()).unwrap();
        (directory, store)
    }

    #[test]
    fn commits_use_compare_and_swap_revisions() {
        let (_directory, store) = store();
        let mut record = SessionRecord::new("session-a".into(), 120);
        record.values.insert("visits".into(), json!(1));
        assert!(store.commit(&mut record).unwrap());
        assert_eq!(record.revision, 1);

        let mut first = store.load("session-a").unwrap().unwrap();
        let mut stale = first.clone();
        first.values.insert("visits".into(), json!(2));
        assert!(store.commit(&mut first).unwrap());
        stale.values.insert("visits".into(), json!(3));
        assert!(!store.commit(&mut stale).unwrap());
        assert_eq!(
            store.load("session-a").unwrap().unwrap().values["visits"],
            json!(2)
        );
    }

    #[test]
    fn writer_statistics_track_completed_durable_batches() {
        let (_directory, store) = store();
        let mut record = SessionRecord::new("session-statistics".into(), 120);
        record.values.insert("visits".into(), json!(1));
        assert!(store.commit(&mut record).unwrap());

        let statistics = store.statistics();
        assert_eq!(statistics.write_jobs, 1);
        assert_eq!(statistics.outstanding_jobs, 0);
        assert!(statistics.peak_outstanding_jobs >= 1);
        assert_eq!(statistics.write_batches, 1);
        assert_eq!(statistics.batched_write_jobs, 1);
        assert_eq!(statistics.max_batch_size, 1);
        assert!(statistics.service_time > Duration::ZERO);
    }

    #[test]
    fn committed_sessions_survive_store_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("sessions.sqlite3");
        let mut record = SessionRecord::new("durable-session".into(), 120);
        {
            let store = SQLiteSessionStore::open(&database, StoreOptions::default()).unwrap();
            record.values.insert("user_id".into(), json!(42));
            assert!(store.commit(&mut record).unwrap());
        }
        let store = SQLiteSessionStore::open(database, StoreOptions::default()).unwrap();
        assert_eq!(
            store.load("durable-session").unwrap().unwrap().values["user_id"],
            json!(42)
        );
    }

    #[test]
    fn expired_session_cleanup_removes_a_bounded_batch_per_pass() {
        let (_directory, store) = store();
        let expired_count = MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP as usize + 1;

        for index in 0..expired_count {
            let mut record = SessionRecord::new(format!("expired-{index:04}"), 300);
            record.expires_at = 10;
            assert!(store.commit(&mut record).unwrap());
        }
        let remaining_id = format!("expired-{:04}", expired_count - 1);
        let mut active = SessionRecord::new("active-session".into(), 300);
        active.expires_at = 11;
        assert!(store.commit(&mut active).unwrap());

        assert_eq!(
            store.collect_expired(10).unwrap(),
            MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP as usize
        );
        assert!(store.load(&remaining_id).unwrap().is_some());
        assert!(store.load("active-session").unwrap().is_some());

        assert_eq!(store.collect_expired(10).unwrap(), 1);
        assert!(store.load(&remaining_id).unwrap().is_none());
        assert!(store.load("active-session").unwrap().is_some());
        assert_eq!(store.collect_expired(10).unwrap(), 0);
    }

    #[test]
    fn bounded_writer_accepts_concurrent_independent_sessions() {
        use std::thread;

        let (_directory, store) = store();
        let store = Arc::new(store);
        let workers = 32;
        let mut handles = Vec::new();
        for worker in 0..workers {
            let store = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                let mut record = SessionRecord::new(format!("session-{worker}"), 300);
                record.values.insert("worker".into(), json!(worker));
                assert!(store.commit(&mut record).unwrap());
                store.load(&record.id).unwrap().unwrap()
            }));
        }
        for handle in handles {
            let result = handle.join().unwrap();
            assert_eq!(result.revision, 1);
        }
    }

    #[test]
    fn batched_writer_isolates_failed_commits_and_persists_successful_peers() {
        use std::sync::Barrier;
        use std::thread;

        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions {
                    write_batch_window: Duration::from_millis(10),
                    ..StoreOptions::default()
                },
            )
            .unwrap(),
        );
        let ready = Arc::new(Barrier::new(3));

        let failing_store = Arc::clone(&store);
        let failing_ready = Arc::clone(&ready);
        let failing = thread::spawn(move || {
            failing_ready.wait();
            failing_store.write_batched(|connection| {
                connection.execute(
                    "INSERT INTO sessions(id, created_at, last_activity, expires_at, revision, data)
                     VALUES ('rolled-back', 1, 1, 100, 1, '{}')",
                    [],
                )?;
                Err::<bool, _>(StoreError::InvalidOptions("injected commit failure"))
            })
        });

        let successful_store = Arc::clone(&store);
        let successful_ready = Arc::clone(&ready);
        let successful = thread::spawn(move || {
            successful_ready.wait();
            let mut record = SessionRecord::new("batch-success".into(), 120);
            record
                .values
                .insert("saved".into(), serde_json::json!(true));
            assert!(successful_store.commit(&mut record).unwrap());
            record
        });

        ready.wait();
        assert!(matches!(
            failing.join().unwrap(),
            Err(StoreError::InvalidOptions("injected commit failure"))
        ));
        let committed = successful.join().unwrap();
        assert_eq!(committed.revision, 1);
        assert!(store.load("rolled-back").unwrap().is_none());
        assert_eq!(
            store.load("batch-success").unwrap().unwrap().values["saved"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn rejects_batch_windows_above_the_ten_millisecond_limit() {
        let directory = tempfile::tempdir().unwrap();
        let result = SQLiteSessionStore::open(
            directory.path().join("sessions.sqlite3"),
            StoreOptions {
                write_batch_window: Duration::from_micros(10_001),
                ..StoreOptions::default()
            },
        );
        assert!(matches!(result, Err(StoreError::InvalidOptions(_))));
    }
}
