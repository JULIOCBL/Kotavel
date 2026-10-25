use crate::diagnostics::{
    CauseChain, ContextField, DiagnosticContext, DiagnosticError, Environment, ErrorDetails,
    LogLevel, LogOptions, LogRecord, Logger, LoggerInitError, OperationContext, RecoverableError,
    RenderedError, ReportStatus, TraceCaptureScope, collect_causes, render_error, validate_context,
};
use std::{
    collections::BTreeMap,
    error::Error,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[derive(Clone, Debug)]
pub struct ErrorRule {
    pub report: bool,
    pub level: Option<LogLevel>,
    pub public_message: String,
}

impl Default for ErrorRule {
    fn default() -> Self {
        Self {
            report: true,
            level: None,
            public_message: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ErrorHandlingOptions {
    pub environment: Environment,
    pub capture_traces: bool,
    pub rules: BTreeMap<String, ErrorRule>,
}

impl Default for ErrorHandlingOptions {
    fn default() -> Self {
        Self {
            environment: Environment::Production,
            capture_traces: true,
            rules: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationState {
    Created,
    Ready,
    Running,
    Failed,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationResult {
    Success,
    RecoveredError,
    FatalError,
}

#[derive(Debug)]
pub enum ApplicationError {
    InvalidState(&'static str),
    InvalidOperation(&'static str),
    InvalidOptions(&'static str),
    Logger(LoggerInitError),
}

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidState(message)
            | Self::InvalidOperation(message)
            | Self::InvalidOptions(message) => f.write_str(message),
            Self::Logger(error) => error.fmt(f),
        }
    }
}
impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Logger(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

/// Synchronous application lifecycle; the instance is not reentrant or shared.
pub struct Application {
    options: LogOptions,
    errors: ErrorHandlingOptions,
    last_error: Option<ErrorDetails>,
    logger: Option<Logger>,
    state: ApplicationState,
}

impl Application {
    pub fn new(options: LogOptions) -> Result<Self, ApplicationError> {
        Self::with_error_handling(options, ErrorHandlingOptions::default())
    }

    pub fn with_error_handling(
        options: LogOptions,
        errors: ErrorHandlingOptions,
    ) -> Result<Self, ApplicationError> {
        if errors
            .rules
            .iter()
            .any(|(code, _)| code.is_empty() || code.len() > 512)
        {
            return Err(ApplicationError::InvalidOptions(
                "Error rule code requires 1 to 512 bytes",
            ));
        }
        Ok(Self {
            options,
            errors,
            last_error: None,
            logger: None,
            state: ApplicationState::Created,
        })
    }

    pub fn boot(&mut self) -> Result<(), ApplicationError> {
        if self.state != ApplicationState::Created {
            return Err(ApplicationError::InvalidState(
                "Application can only boot once",
            ));
        }
        match Logger::new(self.options.clone()) {
            Ok(logger) => {
                self.logger = Some(logger);
                self.state = ApplicationState::Ready;
                Ok(())
            }
            Err(error) => {
                self.state = ApplicationState::Failed;
                Err(ApplicationError::Logger(error))
            }
        }
    }

    pub fn execute<F, E>(
        &mut self,
        operation: &str,
        action: F,
    ) -> Result<OperationResult, ApplicationError>
    where
        F: FnOnce() -> Result<(), E>,
        E: Error + 'static,
    {
        let module = self.options.module.clone();
        let context = OperationContext {
            id: operation,
            module: &module,
            fields: &[],
        };
        self.execute_with_context(context, action)
    }

    pub fn execute_with_context<F, E>(
        &mut self,
        operation: OperationContext<'_>,
        action: F,
    ) -> Result<OperationResult, ApplicationError>
    where
        F: FnOnce() -> Result<(), E>,
        E: Error + 'static,
    {
        if self.state != ApplicationState::Ready {
            return Err(ApplicationError::InvalidState("Application is not ready"));
        }
        if operation.id.is_empty() || operation.id.len() > 512 {
            return Err(ApplicationError::InvalidOperation(
                "Operation identifier requires 1 to 512 bytes",
            ));
        }
        if operation.module.is_empty() || operation.module.len() > 64 {
            return Err(ApplicationError::InvalidOperation(
                "Operation module requires 1 to 64 bytes",
            ));
        }
        validate_context(operation.fields)
            .map_err(|_| ApplicationError::InvalidOperation("Invalid operation context"))?;

        let _trace_scope = TraceCaptureScope::new(self.errors.capture_traces);
        self.last_error = None;
        self.state = ApplicationState::Running;
        match catch_unwind(AssertUnwindSafe(action)) {
            Ok(Ok(())) => {
                self.state = ApplicationState::Ready;
                Ok(OperationResult::Success)
            }
            Ok(Err(error)) => {
                let error: Box<dyn Error> = Box::new(error);
                let chain = collect_causes(error.as_ref());
                let recoverable = error.downcast_ref::<RecoverableError>();
                let diagnostic = error.downcast_ref::<DiagnosticError>();
                let (code, message, context, is_recoverable) = if let Some(error) = recoverable {
                    (
                        error.code(),
                        error.to_string(),
                        error.context().clone(),
                        true,
                    )
                } else if let Some(error) = diagnostic {
                    (
                        error.code(),
                        error.to_string(),
                        error.context().clone(),
                        false,
                    )
                } else {
                    (
                        "operation.unexpected",
                        error.to_string(),
                        DiagnosticContext::from_handler(),
                        false,
                    )
                };
                self.finish_error(code, operation, &message, context, is_recoverable, chain)
            }
            Err(payload) => {
                let message = panic_message(payload);
                self.finish_error(
                    "operation.unknown",
                    operation,
                    &message,
                    DiagnosticContext::from_handler(),
                    false,
                    CauseChain::default(),
                )
            }
        }
    }

    pub fn log(
        &self,
        level: LogLevel,
        code: &str,
        operation: &str,
        message: &str,
        fields: &[ContextField],
    ) -> ReportStatus {
        let Some(logger) = &self.logger else {
            return ReportStatus::Ignored;
        };
        let record = LogRecord {
            level,
            code,
            operation,
            message,
            fields,
            ..LogRecord::default()
        };
        logger.write(&record)
    }

    pub fn shutdown(&mut self) {
        if self.state == ApplicationState::Running {
            return;
        }
        self.logger = None;
        self.state = ApplicationState::Stopped;
    }

    pub fn last_error(&self) -> Option<&ErrorDetails> {
        self.last_error.as_ref()
    }
    pub fn state(&self) -> ApplicationState {
        self.state
    }

    pub fn present_error(&self) -> Option<RenderedError> {
        let details = self.last_error.as_ref()?;
        let public_message = self
            .errors
            .rules
            .get(&details.code)
            .map(|rule| rule.public_message.as_str());
        Some(render_error(
            details,
            self.errors.environment,
            public_message,
        ))
    }

    fn finish_error(
        &mut self,
        code: &str,
        operation: OperationContext<'_>,
        message: &str,
        context: DiagnosticContext,
        recoverable: bool,
        chain: CauseChain,
    ) -> Result<OperationResult, ApplicationError> {
        self.state = ApplicationState::Failed;
        let Some(logger) = self.logger.as_ref() else {
            return Ok(OperationResult::FatalError);
        };
        let mut details = ErrorDetails {
            code: code.chars().take(512).collect(),
            operation: operation.id.to_owned(),
            message: truncate_utf8(message, 4096),
            context: context.clone(),
            recoverable,
            module: operation.module.to_owned(),
            fields: operation.fields.to_vec(),
            chain,
            report_status: ReportStatus::Ignored,
        };
        let rule = self.errors.rules.get(code);
        if rule.is_none_or(|rule| rule.report) {
            let level = rule.and_then(|rule| rule.level).unwrap_or(if recoverable {
                LogLevel::Error
            } else {
                LogLevel::Critical
            });
            let record = LogRecord {
                level,
                code,
                operation: operation.id,
                message,
                module: operation.module,
                fields: operation.fields,
                diagnostic: Some(&context),
                causes: &details.chain.causes,
                causes_truncated: details.chain.truncated,
            };
            details.report_status = logger.write(&record);
        }
        self.last_error = Some(details);
        if recoverable {
            self.state = ApplicationState::Ready;
            Ok(OperationResult::RecoveredError)
        } else {
            Ok(OperationResult::FatalError)
        }
    }
}

fn truncate_utf8(message: &str, maximum: usize) -> String {
    if message.len() <= maximum {
        return message.to_owned();
    }
    let mut end = maximum;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "Unknown panic".into()
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
                console: false,
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
        let diagnostic =
            DiagnosticError::new("private.failure", "db-host password=secret").unwrap();
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
}
