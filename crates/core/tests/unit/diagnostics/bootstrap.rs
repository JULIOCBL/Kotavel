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
