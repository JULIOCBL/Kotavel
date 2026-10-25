use super::*;
use std::{error::Error, fmt};

#[test]
fn diagnostic_captures_caller_context_and_enforces_codes() {
    let expected_line = line!() + 1;
    let error = DiagnosticError::new("database.unavailable", "private reason").unwrap();
    let location = error.context().location.unwrap();
    assert_eq!(location.line(), expected_line);
    assert!(location.file_name().ends_with("diagnostics/tests.rs"));
    assert_eq!(error.context().origin, TraceOrigin::ExceptionConstruction);
    assert!(DiagnosticError::new("", "invalid").is_err());
    assert!(DiagnosticError::new("x".repeat(513), "invalid").is_err());
}

#[test]
fn trace_capture_scope_is_nested_and_thread_local() {
    let _disabled = TraceCaptureScope::new(false);
    assert_eq!(StackTrace::capture().format(), "Traza desactivada");
    {
        let _enabled = TraceCaptureScope::new(true);
        assert!(StackTrace::capture().format().contains("diagnostics"));
    }
    assert_eq!(StackTrace::capture().format(), "Traza desactivada");
    assert!(
        std::thread::spawn(|| StackTrace::capture().enabled())
            .join()
            .unwrap()
    );
}

#[test]
fn context_validation_limits_size_and_rejects_duplicate_keys() {
    let duplicate = [
        ContextField {
            key: "request".into(),
            value: "1".into(),
        },
        ContextField {
            key: "request".into(),
            value: "2".into(),
        },
    ];
    assert_eq!(
        validate_context(&duplicate),
        Err(ContextValidationError::DuplicateKey)
    );
    let long = [ContextField {
        key: "k".into(),
        value: "x".repeat(257),
    }];
    assert_eq!(
        validate_context(&long),
        Err(ContextValidationError::InvalidFieldSize)
    );
    let too_many: Vec<_> = (0..17)
        .map(|i| ContextField {
            key: format!("k{i}"),
            value: String::new(),
        })
        .collect();
    assert_eq!(
        validate_context(&too_many),
        Err(ContextValidationError::TooManyFields)
    );
}

#[test]
fn production_error_presentation_hides_internal_details() {
    let error =
        DiagnosticError::new("database.unavailable", "db.internal password=hunter2").unwrap();
    let details = details_from(&error);
    let production = render_error(
        &details,
        Environment::Production,
        Some("Intenta más tarde."),
    );
    assert_eq!(production.message, "Intenta más tarde.");
    assert!(production.detail.is_empty());
    let visible = production.console_text();
    assert!(!visible.contains("hunter2"));
    assert!(!visible.contains("diagnostics/tests.rs"));

    let development = render_error(&details, Environment::Development, None);
    assert!(development.detail.contains("password=hunter2"));
    assert!(development.detail.contains("diagnostics/tests.rs"));
}

#[test]
fn cause_chain_is_bounded_and_keeps_diagnostic_codes() {
    let root = ChainError {
        source: Some(Box::new(
            DiagnosticError::new("inner.failure", "inner").unwrap(),
        )),
    };
    let chain = collect_causes(&root);
    assert_eq!(chain.causes.len(), 1);
    assert_eq!(chain.causes[0].code.as_deref(), Some("inner.failure"));

    let long = ChainError {
        source: Some(Box::new(std::io::Error::other("é".repeat(300)))),
    };
    let bounded = collect_causes(&long);
    assert!(bounded.causes[0].message.len() <= 512);
    assert!(
        bounded.causes[0]
            .message
            .is_char_boundary(bounded.causes[0].message.len())
    );
}

fn details_from(error: &DiagnosticError) -> ErrorDetails {
    ErrorDetails {
        code: error.code().into(),
        operation: "request-42".into(),
        message: error.to_string(),
        context: error.context().clone(),
        recoverable: false,
        module: "database".into(),
        fields: vec![ContextField {
            key: "tenant".into(),
            value: "private".into(),
        }],
        chain: error.cause_chain(),
        report_status: ReportStatus::Written,
    }
}

#[derive(Debug)]
struct ChainError {
    source: Option<Box<dyn Error>>,
}
impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("root")
    }
}
impl Error for ChainError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref()
    }
}
