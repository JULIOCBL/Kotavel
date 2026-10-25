use super::{
    format::format_record,
    options::{LogLevel, LogOptions, LogRecord, ThrottleOptions},
};
use crate::{
    diagnostics::{ReportStatus, validate_context},
    filesystem::{FileLock, Filesystem, LocalFilesystem},
};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LogStats {
    pub written: u64,
    pub filtered: u64,
    pub throttled: u64,
    pub failed: u64,
}

#[derive(Debug)]
pub enum LoggerInitError {
    InvalidOptions(&'static str),
    AlreadyOwned,
    Io(io::Error),
}
impl std::fmt::Display for LoggerInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOptions(message) => f.write_str(message),
            Self::AlreadyOwned => f.write_str("Another logger owns this directory"),
            Self::Io(error) => write!(f, "Cannot initialize Kotavel logger: {error}"),
        }
    }
}
impl std::error::Error for LoggerInitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

struct Window {
    start: Instant,
    reports: usize,
}
struct State {
    throttle: ThrottleOptions,
    windows: HashMap<(String, String, LogLevel), Window>,
}

/// Thread-safe logger. All writes and rotations are serialized within one owner;
/// a directory lease additionally excludes cooperating processes.
pub struct Logger {
    options: LogOptions,
    filesystem: Arc<dyn Filesystem>,
    _lease: Mutex<Option<Box<dyn FileLock>>>,
    state: Mutex<State>,
    written: AtomicU64,
    filtered: AtomicU64,
    throttled: AtomicU64,
    failed: AtomicU64,
}

impl Logger {
    pub fn new(options: LogOptions) -> Result<Self, LoggerInitError> {
        Self::with_filesystem(options, Arc::new(LocalFilesystem))
    }

    pub fn with_filesystem(
        options: LogOptions,
        filesystem: Arc<dyn Filesystem>,
    ) -> Result<Self, LoggerInitError> {
        if options.max_bytes < 16 * 1024 || options.backups > 100 {
            return Err(LoggerInitError::InvalidOptions(
                "Logger requires max_bytes >= 16384 and backups <= 100",
            ));
        }
        if options.module.is_empty() || options.module.len() > 64 {
            return Err(LoggerInitError::InvalidOptions(
                "Log module requires 1 to 64 bytes",
            ));
        }
        validate_throttle(options.throttle)?;
        if options.enabled && options.file && options.directory.as_os_str().is_empty() {
            return Err(LoggerInitError::InvalidOptions(
                "File logger requires a directory",
            ));
        }
        let lease = if options.enabled && options.file {
            filesystem
                .create_directories(&options.directory, false)
                .map_err(LoggerInitError::Io)?;
            let lock_path = options.directory.join(".kotavel.logger.lock");
            let lease = filesystem.try_lock_file(&lock_path).map_err(|error| {
                if error.kind() == io::ErrorKind::WouldBlock {
                    LoggerInitError::AlreadyOwned
                } else {
                    LoggerInitError::Io(error)
                }
            })?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(options.directory.join("kotavel.log"))
                .map_err(LoggerInitError::Io)?;
            Some(lease)
        } else {
            None
        };
        Ok(Self {
            state: Mutex::new(State {
                throttle: options.throttle,
                windows: HashMap::new(),
            }),
            options,
            filesystem,
            _lease: Mutex::new(lease),
            written: AtomicU64::new(0),
            filtered: AtomicU64::new(0),
            throttled: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        })
    }

    pub fn stats(&self) -> LogStats {
        LogStats {
            written: self.written.load(Ordering::Relaxed),
            filtered: self.filtered.load(Ordering::Relaxed),
            throttled: self.throttled.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }

    pub fn configure_report_throttling(
        &self,
        options: ThrottleOptions,
    ) -> Result<(), LoggerInitError> {
        validate_throttle(options)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.throttle = options;
        state.windows.clear();
        Ok(())
    }

    /// Returns written, filtered, throttled, or failed without unwinding.
    pub fn write(&self, record: &LogRecord<'_>) -> ReportStatus {
        if !self.options.enabled {
            self.filtered.fetch_add(1, Ordering::Relaxed);
            return ReportStatus::Filtered;
        }
        if record.level < self.options.min_level {
            self.filtered.fetch_add(1, Ordering::Relaxed);
            return ReportStatus::Filtered;
        }
        let result = catch_unwind(AssertUnwindSafe(|| self.write_inner(record)));
        match result.unwrap_or(Err(())) {
            Ok(WriteResult::Written) => {
                self.written.fetch_add(1, Ordering::Relaxed);
                ReportStatus::Written
            }
            Ok(WriteResult::Throttled) => {
                self.throttled.fetch_add(1, Ordering::Relaxed);
                ReportStatus::Throttled
            }
            Err(()) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                let _ = writeln!(
                    io::stderr(),
                    "Kotavel: diagnostic write failed; message could not be recorded."
                );
                ReportStatus::Failed
            }
        }
    }

    pub fn report(
        &self,
        level: LogLevel,
        code: &str,
        operation: &str,
        message: &str,
        diagnostic: Option<&crate::diagnostics::DiagnosticContext>,
    ) -> bool {
        let record = LogRecord {
            level,
            code,
            operation,
            message,
            diagnostic,
            ..LogRecord::default()
        };
        self.write(&record) != ReportStatus::Failed
    }

    pub fn report_error(&self, code: &str, operation: &str, message: &str) -> bool {
        self.report(LogLevel::Error, code, operation, message, None)
    }

    fn write_inner(&self, record: &LogRecord<'_>) -> Result<WriteResult, ()> {
        if record.module.len() > 64
            || record.code.len() > 512
            || record.operation.len() > 512
            || record.causes.len() > 8
        {
            return Err(());
        }
        validate_context(record.fields).map_err(|_| ())?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let module = if record.module.is_empty() {
            self.options.module.as_str()
        } else {
            record.module
        };
        if limited(&mut state, record, module) {
            return Ok(WriteResult::Throttled);
        }
        let line = format_record(record, module, self.options.max_bytes).map_err(|_| ())?;
        if self.options.file {
            self.append_with_rotation(&line).map_err(|_| ())?;
        }
        if self.options.console {
            let mut stderr = io::stderr().lock();
            stderr.write_all(line.as_bytes()).map_err(|_| ())?;
            stderr.flush().map_err(|_| ())?;
        }
        Ok(WriteResult::Written)
    }

    fn append_with_rotation(&self, line: &str) -> io::Result<()> {
        let path = self.options.directory.join("kotavel.log");
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.len() as usize > self.options.max_bytes.saturating_sub(line.len()) {
                if self.options.backups == 0 {
                    fs::remove_file(&path)?;
                } else {
                    for index in (1..=self.options.backups).rev() {
                        let destination = backup_path(&path, index);
                        let source = if index == 1 {
                            path.clone()
                        } else {
                            backup_path(&path, index - 1)
                        };
                        match fs::remove_file(&destination) {
                            Ok(()) => (),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                            Err(error) => return Err(error),
                        }
                        match fs::rename(&source, &destination) {
                            Ok(()) => (),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(line.as_bytes())?;
        file.flush()
    }

    pub fn log_path(&self) -> PathBuf {
        self.options.directory.join("kotavel.log")
    }
    pub fn filesystem(&self) -> &Arc<dyn Filesystem> {
        &self.filesystem
    }
}

enum WriteResult {
    Written,
    Throttled,
}

fn validate_throttle(options: ThrottleOptions) -> Result<(), LoggerInitError> {
    if options.interval.is_zero() || options.max_keys == 0 || options.max_keys > 4096 {
        return Err(LoggerInitError::InvalidOptions(
            "Throttle requires a positive interval and 1 to 4096 keys",
        ));
    }
    Ok(())
}

fn limited(state: &mut State, record: &LogRecord<'_>, module: &str) -> bool {
    let options = state.throttle;
    if options.max_reports == 0 {
        return false;
    }
    let now = Instant::now();
    let key = (module.to_owned(), record.code.to_owned(), record.level);
    if !state.windows.contains_key(&key) {
        if state.windows.len() >= options.max_keys {
            state
                .windows
                .retain(|_, window| now.duration_since(window.start) < options.interval);
            if state.windows.len() >= options.max_keys {
                return true;
            }
        }
        state.windows.insert(
            key,
            Window {
                start: now,
                reports: 1,
            },
        );
        return false;
    }
    let window = state.windows.get_mut(&key).expect("present above");
    if now.duration_since(window.start) >= options.interval {
        window.start = now;
        window.reports = 0;
    }
    if window.reports >= options.max_reports {
        return true;
    }
    window.reports += 1;
    false
}

fn backup_path(path: &std::path::Path, index: usize) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}
