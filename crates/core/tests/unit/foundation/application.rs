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
            "kotavel-app-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
    fn options(&self) -> LogOptions {
        LogOptions {
            directory: self.0.clone(),
            channel: crate::diagnostics::LogChannel::Single,
            ..LogOptions::default()
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lifecycle_recovers_only_explicit_recoverable_errors_and_clears_old_context() {
    let directory = Directory::new();
    let mut app = Application::new(directory.options()).unwrap();
    assert_eq!(app.state(), ApplicationState::Created);
    assert_eq!(
        app.log(LogLevel::Info, "boot", "before", "message", &[]),
        ReportStatus::Ignored
    );
    assert!(matches!(
        app.execute("before-boot", || Ok::<(), std::io::Error>(())),
        Err(ApplicationError::InvalidState(_))
    ));
    app.boot().unwrap();
    assert_eq!(app.state(), ApplicationState::Ready);
    assert!(matches!(app.boot(), Err(ApplicationError::InvalidState(_))));

    let recovered = RecoverableError::new("recoverable detail").unwrap();
    assert_eq!(
        app.execute("request-1", || Err::<(), _>(recovered))
            .unwrap(),
        OperationResult::RecoveredError
    );
    assert_eq!(app.state(), ApplicationState::Ready);
    assert!(app.last_error().unwrap().recoverable);
    assert_eq!(app.last_error().unwrap().code, "operation.recoverable");

    assert_eq!(
        app.execute("next", || Ok::<(), std::io::Error>(()))
            .unwrap(),
        OperationResult::Success
    );
    assert!(app.last_error().is_none());
    app.shutdown();
    app.shutdown();
    assert_eq!(app.state(), ApplicationState::Stopped);
}

#[test]
fn fatal_errors_are_retained_redacted_and_follow_rules_without_changing_recovery() {
    let directory = Directory::new();
    let mut rules = BTreeMap::new();
    rules.insert(
        "private.failure".into(),
        ErrorRule {
            report: false,
            level: Some(LogLevel::Notice),
            public_message: "Mensaje seguro".into(),
        },
    );
    let options = ErrorHandlingOptions {
        rules,
        ..ErrorHandlingOptions::default()
    };
    let mut app = Application::with_error_handling(directory.options(), options).unwrap();
    app.boot().unwrap();
    let diagnostic = DiagnosticError::new("private.failure", "db-host password=secret").unwrap();
    assert_eq!(
        app.execute("save-42", || Err::<(), _>(diagnostic)).unwrap(),
        OperationResult::FatalError
    );
    assert_eq!(app.state(), ApplicationState::Failed);
    assert_eq!(
        app.last_error().unwrap().report_status,
        ReportStatus::Ignored
    );
    let public = app.present_error().unwrap();
    assert_eq!(public.message, "Mensaje seguro");
    assert!(public.detail.is_empty());
    assert!(!public.console_text().contains("secret"));
    assert_eq!(
        fs::read_to_string(directory.0.join("kotavel.log")).unwrap(),
        ""
    );
}

#[test]
fn operation_context_is_validated_before_action_and_trace_policy_is_scoped() {
    let directory = Directory::new();
    let errors = ErrorHandlingOptions {
        capture_traces: false,
        ..ErrorHandlingOptions::default()
    };
    let mut app = Application::with_error_handling(directory.options(), errors).unwrap();
    app.boot().unwrap();
    let duplicate = [
        ContextField {
            key: "id".into(),
            value: "1".into(),
        },
        ContextField {
            key: "id".into(),
            value: "2".into(),
        },
    ];
    let mut called = false;
    let context = OperationContext {
        id: "invalid",
        module: "orders",
        fields: &duplicate,
    };
    assert!(matches!(
        app.execute_with_context(context, || {
            called = true;
            Ok::<(), std::io::Error>(())
        }),
        Err(ApplicationError::InvalidOperation(_))
    ));
    assert!(!called);

    assert_eq!(
        app.execute("trace-off", || {
            Err::<(), _>(DiagnosticError::new("controlled", "private").unwrap())
        })
        .unwrap(),
        OperationResult::FatalError
    );
    assert!(!app.last_error().unwrap().context.stack.enabled());
    assert!(crate::diagnostics::StackTrace::capture().enabled());
}

#[test]
fn panics_become_fatal_unknown_errors_and_invalid_boot_fails_closed() {
    let directory = Directory::new();
    let mut app = Application::new(directory.options()).unwrap();
    app.boot().unwrap();
    assert_eq!(
        app.execute::<_, std::io::Error>("panic", || panic!("unexpected private panic"))
            .unwrap(),
        OperationResult::FatalError
    );
    assert_eq!(app.last_error().unwrap().code, "operation.unknown");
    assert!(app.present_error().unwrap().detail.is_empty());

    let mut invalid = Application::new(LogOptions::default()).unwrap();
    assert!(invalid.boot().is_err());
    assert_eq!(invalid.state(), ApplicationState::Failed);
}
