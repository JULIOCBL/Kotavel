use super::*;
use crate::diagnostics::{ContextField, LogChannel, ReportStatus};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[test]
fn log_channel_enum_documents_and_selects_output_destinations() {
    for (name, file, console, enabled) in [
        ("single", true, false, true),
        ("stderr", false, true, true),
        ("stack", true, true, true),
        ("null", false, false, false),
    ] {
        let options = LogOptions::configured(
            std::env::temp_dir().join("kotavel-channel-config"),
            "logs".into(),
            512 * 1024 * 1024,
            3,
            name.into(),
            "trace".into(),
            "app".into(),
        )
        .unwrap();

        assert_eq!(options.channel.writes_file(), file, "channel {name}");
        assert_eq!(options.channel.writes_console(), console, "channel {name}");
        assert_eq!(options.channel.is_enabled(), enabled, "channel {name}");
    }
}

#[test]
fn log_directory_is_resolved_inside_project_storage() {
    let project_root =
        std::env::temp_dir().join(format!("kotavel-log-directory-{}", std::process::id()));
    let options = LogOptions::configured(
        project_root.clone(),
        "logs".into(),
        512 * 1024 * 1024,
        3,
        "single".into(),
        "trace".into(),
        "app".into(),
    )
    .unwrap();

    assert_eq!(options.directory, project_root.join("storage/logs"));
}

#[test]
fn log_directory_rejects_paths_that_can_escape_storage() {
    for directory in ["", "../outside", "logs/../../outside", "/tmp/outside"] {
        let result = LogOptions::configured(
            std::env::temp_dir().join("kotavel-invalid-log-directory"),
            directory.into(),
            512 * 1024 * 1024,
            3,
            "single".into(),
            "trace".into(),
            "app".into(),
        );

        assert!(
            result.is_err(),
            "accepted unsafe log directory {directory:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn log_directory_rejects_symlinks_that_escape_storage() {
    let project = TestDirectory::new();
    let storage = project.0.join("storage");
    let outside = project.0.join("outside");
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, storage.join("logs")).unwrap();

    let result = LogOptions::configured(
        project.0.clone(),
        "logs".into(),
        512 * 1024 * 1024,
        3,
        "single".into(),
        "trace".into(),
        "app".into(),
    );

    assert!(
        result.is_err(),
        "accepted a log symlink that escapes storage"
    );
}

struct TestDirectory(std::path::PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("kotavel-logger-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn options(&self) -> LogOptions {
        LogOptions {
            directory: self.0.clone(),
            channel: LogChannel::Single,
            max_bytes: 16 * 1024,
            ..LogOptions::default()
        }
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn levels_filter_before_disk_access_and_invalid_configuration_fails() {
    let directory = TestDirectory::new();
    let mut options = directory.options();
    options.min_level = LogLevel::Warning;
    let logger = Logger::new(options).unwrap();
    let path = logger.log_path();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let record = LogRecord {
        level: LogLevel::Info,
        code: "filtered",
        ..LogRecord::default()
    };
    assert_eq!(logger.write(&record), ReportStatus::Filtered);
    assert!(path.is_dir());

    let mut bad = directory.options();
    bad.max_bytes = 100;
    assert!(matches!(
        Logger::new(bad),
        Err(LoggerInitError::InvalidOptions(_))
    ));
}

#[test]
fn formatting_escapes_newlines_preserves_unicode_and_keeps_one_record_per_line() {
    let directory = TestDirectory::new();
    let logger = Logger::new(directory.options()).unwrap();
    let fields = [ContextField {
        key: "tenant".into(),
        value: "José\nadmin".into(),
    }];
    let record = LogRecord {
        level: LogLevel::Error,
        code: "db.failed",
        operation: "save",
        message: "falló\npassword=secret",
        fields: &fields,
        ..LogRecord::default()
    };
    assert_eq!(logger.write(&record), ReportStatus::Written);
    let contents = fs::read_to_string(logger.log_path()).unwrap();
    assert!(contents.contains("application.ERROR: falló\\npassword=secret"));
    assert!(contents.contains("José\\nadmin"));
    assert_eq!(contents.lines().count(), 1);
    assert!(contents.starts_with('['));
}

#[test]
fn concurrent_writes_are_complete_and_directory_has_one_owner() {
    let directory = TestDirectory::new();
    let mut options = directory.options();
    options.max_bytes = 512 * 1024;
    let logger = Arc::new(Logger::new(options.clone()).unwrap());
    assert!(matches!(
        Logger::new(options),
        Err(LoggerInitError::AlreadyOwned)
    ));
    let mut writers = Vec::new();
    for worker in 0..8 {
        let logger = Arc::clone(&logger);
        writers.push(thread::spawn(move || {
            for row in 0..50 {
                let message = format!("worker={worker} row={row}");
                let record = LogRecord {
                    level: LogLevel::Info,
                    code: "parallel",
                    operation: "load",
                    message: &message,
                    ..LogRecord::default()
                };
                assert_eq!(logger.write(&record), ReportStatus::Written);
            }
        }));
    }
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(logger.stats().written, 400);
    let contents = fs::read_to_string(logger.log_path()).unwrap();
    assert_eq!(contents.lines().count(), 400);
    assert!(contents.lines().all(|line| line.starts_with('[')
        && line.contains(".INFO: worker=")
        && line.ends_with('}')));
}

#[test]
fn throttling_is_per_module_code_and_level_and_expired_keys_are_reclaimed() {
    let directory = TestDirectory::new();
    let mut options = directory.options();
    options.throttle = ThrottleOptions {
        max_reports: 2,
        interval: Duration::from_millis(30),
        max_keys: 2,
    };
    let logger = Logger::new(options).unwrap();
    let record = LogRecord {
        level: LogLevel::Error,
        code: "repeat",
        ..LogRecord::default()
    };
    assert_eq!(logger.write(&record), ReportStatus::Written);
    assert_eq!(logger.write(&record), ReportStatus::Written);
    assert_eq!(logger.write(&record), ReportStatus::Throttled);
    let other_module = LogRecord {
        module: "payments",
        ..record
    };
    assert_eq!(logger.write(&other_module), ReportStatus::Written);
    let overflow = LogRecord {
        code: "other",
        ..record
    };
    assert_eq!(logger.write(&overflow), ReportStatus::Throttled);
    thread::sleep(Duration::from_millis(40));
    assert_eq!(logger.write(&overflow), ReportStatus::Written);
    assert_eq!(logger.stats().throttled, 2);
}

#[test]
fn rotation_retains_only_configured_backups_and_caps_file_size() {
    let directory = TestDirectory::new();
    let mut options = directory.options();
    options.backups = 2;
    let logger = Logger::new(options.clone()).unwrap();
    for _ in 0..100 {
        let record = LogRecord {
            level: LogLevel::Info,
            code: "rotate",
            message: &"x".repeat(500),
            ..LogRecord::default()
        };
        assert_eq!(logger.write(&record), ReportStatus::Written);
    }
    for suffix in ["", ".1", ".2"] {
        let mut path = logger.log_path().into_os_string();
        path.push(suffix);
        let path = std::path::PathBuf::from(path);
        if path.exists() {
            assert!(fs::metadata(path).unwrap().len() <= options.max_bytes as u64);
        }
    }
    assert!(!directory.0.join("kotavel.log.3").exists());
}

#[test]
fn disabled_logger_needs_no_directory_and_counts_filtered_records() {
    let logger = Logger::new(LogOptions {
        channel: LogChannel::Null,
        ..LogOptions::default()
    })
    .unwrap();
    assert_eq!(logger.write(&LogRecord::default()), ReportStatus::Filtered);
    assert_eq!(logger.stats().filtered, 1);
}

#[test]
fn write_failures_are_reported_and_do_not_unwind() {
    let directory = TestDirectory::new();
    let logger = Logger::new(directory.options()).unwrap();
    let path = logger.log_path();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let record = LogRecord {
        level: LogLevel::Error,
        code: "disk.failure",
        ..LogRecord::default()
    };
    assert_eq!(logger.write(&record), ReportStatus::Failed);
    assert_eq!(logger.stats().failed, 1);
}
