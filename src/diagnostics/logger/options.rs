use super::super::{ContextField, DiagnosticContext, ErrorCause};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl LogLevel {
    pub const ALL: [Self; 9] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Notice,
        Self::Warning,
        Self::Error,
        Self::Critical,
        Self::Alert,
        Self::Emergency,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Alert => "alert",
            Self::Emergency => "emergency",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThrottleOptions {
    pub max_reports: usize,
    pub interval: Duration,
    pub max_keys: usize,
}

impl Default for ThrottleOptions {
    fn default() -> Self {
        Self {
            max_reports: 0,
            interval: Duration::from_secs(60),
            max_keys: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogOptions {
    pub directory: PathBuf,
    pub max_bytes: usize,
    pub backups: usize,
    pub console: bool,
    pub min_level: LogLevel,
    pub module: String,
    pub throttle: ThrottleOptions,
    pub file: bool,
    pub enabled: bool,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            directory: PathBuf::new(),
            max_bytes: 512 * 1024 * 1024,
            backups: 3,
            console: true,
            min_level: LogLevel::Info,
            module: "application".into(),
            throttle: ThrottleOptions::default(),
            file: true,
            enabled: true,
        }
    }
}

/// Borrowed record content remains valid only for the duration of `write`.
#[derive(Clone, Copy)]
pub struct LogRecord<'a> {
    pub level: LogLevel,
    pub code: &'a str,
    pub operation: &'a str,
    pub message: &'a str,
    pub module: &'a str,
    pub fields: &'a [ContextField],
    pub diagnostic: Option<&'a DiagnosticContext>,
    pub causes: &'a [ErrorCause],
    pub causes_truncated: bool,
}

impl Default for LogRecord<'_> {
    fn default() -> Self {
        Self {
            level: LogLevel::Error,
            code: "",
            operation: "",
            message: "",
            module: "",
            fields: &[],
            diagnostic: None,
            causes: &[],
            causes_truncated: false,
        }
    }
}
