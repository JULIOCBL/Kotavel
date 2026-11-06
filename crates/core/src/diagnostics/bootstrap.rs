use super::{ContextField, LogLevel, LogOptions, LogRecord, Logger, ReportStatus};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

/// Temporary logger for diagnostics emitted before `Application::boot()`.
pub struct BootstrapLogger;

impl BootstrapLogger {
    pub fn write(directory: &Path, record: &LogRecord<'_>) -> ReportStatus {
        attempt(|| {
            let options = LogOptions {
                directory: directory.to_owned(),
                channel: super::LogChannel::Single,
                module: if record.module.is_empty() {
                    "bootstrap".into()
                } else {
                    record.module.into()
                },
                min_level: LogLevel::Trace,
                ..LogOptions::default()
            };
            let logger = Logger::new(options).map_err(|_| ())?;
            let status = logger.write(record);
            (status != ReportStatus::Failed).then_some(status).ok_or(())
        })
    }

    pub fn environment_file_open_failed(
        directory: &Path,
        file: &str,
        example: &str,
        project_root: &str,
        reason: &str,
        message: &str,
    ) -> ReportStatus {
        let fields = [
            field("file", file),
            field("example", example),
            field("project_root", project_root),
            field("reason", reason),
        ];
        Self::write(
            directory,
            &LogRecord {
                level: LogLevel::Error,
                code: "environment.file_open_failed",
                operation: "load_environment",
                message,
                module: "bootstrap",
                fields: &fields,
                ..LogRecord::default()
            },
        )
    }

    pub fn environment_file_invalid(
        directory: &Path,
        file: &str,
        project_root: &str,
        line: usize,
        reason: &str,
        message: &str,
    ) -> ReportStatus {
        let line = if line == 0 {
            "unknown".to_owned()
        } else {
            line.to_string()
        };
        let fields = [
            field("file", file),
            field("project_root", project_root),
            field("line", &line),
            field("reason", reason),
        ];
        Self::write(
            directory,
            &LogRecord {
                level: LogLevel::Error,
                code: "environment.file_invalid",
                operation: "parse_environment",
                message,
                module: "bootstrap",
                fields: &fields,
                ..LogRecord::default()
            },
        )
    }

    pub fn configuration_invalid(
        directory: &Path,
        project_root: &str,
        key: &str,
        reason: &str,
        message: &str,
    ) -> ReportStatus {
        let fields = [
            field("project_root", project_root),
            field("key", key),
            field("reason", reason),
        ];
        Self::write(
            directory,
            &LogRecord {
                level: LogLevel::Error,
                code: "configuration.invalid",
                operation: "configure_console",
                message,
                module: "bootstrap",
                fields: &fields,
                ..LogRecord::default()
            },
        )
    }

    pub fn startup_failed(
        directory: &Path,
        project_root: &str,
        phase: &str,
        reason: &str,
        message: &str,
    ) -> ReportStatus {
        let fields = [
            field("project_root", project_root),
            field("phase", phase),
            field("reason", reason),
        ];
        Self::write(
            directory,
            &LogRecord {
                level: LogLevel::Error,
                code: "runtime.startup_failed",
                operation: phase,
                message,
                module: "bootstrap",
                fields: &fields,
                ..LogRecord::default()
            },
        )
    }
}

fn field(key: &str, value: &str) -> ContextField {
    ContextField {
        key: key.into(),
        value: value.into(),
    }
}

fn attempt(action: impl FnOnce() -> Result<ReportStatus, ()>) -> ReportStatus {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(status)) => status,
        Ok(Err(())) | Err(_) => ReportStatus::Failed,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/diagnostics/bootstrap.rs"]
mod tests;
