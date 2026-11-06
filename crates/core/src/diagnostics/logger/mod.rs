//! Concurrent, bounded file logger with Laravel-style records and rotation.

mod core;
mod format;
mod options;
#[cfg(test)]
#[path = "../../../tests/unit/diagnostics/logger/mod.rs"]
mod tests;

pub use core::{LogStats, Logger, LoggerInitError};
pub use options::{LogChannel, LogLevel, LogOptions, LogRecord, ThrottleOptions};
