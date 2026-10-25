use super::{DiagnosticContext, SourceLocation, TraceOrigin, collect_causes};
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorConstructionError {
    InvalidCode,
}
impl fmt::Display for ErrorConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Diagnostic error requires a code of 1 to 512 bytes")
    }
}
impl Error for ErrorConstructionError {}

/// Error carrying a stable machine-readable code and capture-time context.
#[derive(Debug)]
pub struct DiagnosticError {
    code: String,
    message: String,
    context: DiagnosticContext,
}

impl DiagnosticError {
    #[track_caller]
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, ErrorConstructionError> {
        let code = code.into();
        if code.is_empty() || code.len() > 512 {
            return Err(ErrorConstructionError::InvalidCode);
        }
        Ok(Self {
            code,
            message: message.into(),
            context: DiagnosticContext {
                location: Some(SourceLocation::caller()),
                origin: TraceOrigin::ExceptionConstruction,
                stack: super::StackTrace::capture(),
            },
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn context(&self) -> &DiagnosticContext {
        &self.context
    }
    pub fn cause_chain(&self) -> super::CauseChain {
        collect_causes(self)
    }
}

impl fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for DiagnosticError {}

/// Error that marks an operation as recoverable without changing its code/context.
#[derive(Debug)]
pub struct RecoverableError(DiagnosticError);

impl RecoverableError {
    #[track_caller]
    pub fn new(message: impl Into<String>) -> Result<Self, ErrorConstructionError> {
        Self::with_code("operation.recoverable", message)
    }

    #[track_caller]
    pub fn with_code(
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, ErrorConstructionError> {
        Ok(Self(DiagnosticError::new(code, message)?))
    }

    pub fn code(&self) -> &str {
        self.0.code()
    }
    pub fn context(&self) -> &DiagnosticContext {
        self.0.context()
    }
    pub fn diagnostic(&self) -> &DiagnosticError {
        &self.0
    }
}

impl fmt::Display for RecoverableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Error for RecoverableError {}
