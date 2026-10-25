//! Concurrent, bounded file logger with Laravel-style records and rotation.

mod core;
mod format;
mod options;
#[cfg(test)]
mod tests;

pub use core::{LogStats, Logger, LoggerInitError};
pub use options::{LogLevel, LogOptions, LogRecord, ThrottleOptions};
