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
                module: if record.module.is_empty() {
                    "bootstrap".into()
                } else {
                    record.module.into()
                },
                min_level: LogLevel::Trace,
                console: false,
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
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "kotavel-bootstrap-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn helpers_write_before_application_boot_with_expected_codes_and_fields() {
        let directory = Directory::new();
        assert_eq!(
            BootstrapLogger::environment_file_open_failed(
                &directory.0,
                ".env",
                ".env.example",
                "/project",
                "permission denied",
                "Could not load environment"
            ),
            ReportStatus::Written
        );
        assert_eq!(
            BootstrapLogger::environment_file_invalid(
                &directory.0,
                ".env",
                "/project",
                0,
                "invalid syntax",
                "Invalid environment"
            ),
            ReportStatus::Written
        );
        assert_eq!(
            BootstrapLogger::configuration_invalid(
                &directory.0,
                "/project",
                "server.port",
                "must be an integer",
                "Invalid configuration"
            ),
            ReportStatus::Written
        );
        assert_eq!(
            BootstrapLogger::startup_failed(
                &directory.0,
                "/project",
                "boot",
                "database unavailable",
                "Startup failed"
            ),
            ReportStatus::Written
        );

        let log = fs::read_to_string(directory.0.join("kotavel.log")).unwrap();
        assert_eq!(log.lines().count(), 4);
        assert!(log.contains("environment.file_open_failed"));
        assert!(log.contains("\"example\":\".env.example\""));
        assert!(log.contains("\"line\":\"unknown\""));
        assert!(log.contains("configuration.invalid"));
        assert!(log.contains("runtime.startup_failed"));
    }

    #[test]
    fn custom_record_honors_its_module_and_unavailable_directory_returns_failed() {
        let directory = Directory::new();
        let record = LogRecord {
            level: LogLevel::Trace,
            code: "startup.trace",
            module: "runtime",
            message: "early",
            ..LogRecord::default()
        };
        assert_eq!(
            BootstrapLogger::write(&directory.0, &record),
            ReportStatus::Written
        );
        assert!(
            fs::read_to_string(directory.0.join("kotavel.log"))
                .unwrap()
                .contains("runtime.TRACE: early")
        );
        assert_eq!(
            BootstrapLogger::startup_failed(Path::new(""), "/project", "boot", "reason", "message"),
            ReportStatus::Failed
        );
    }
}
