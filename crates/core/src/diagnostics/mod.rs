//! Structured errors, logging and safe presentation. Platform-specific runtime
//! services remain isolated behind their contracts.

mod bootstrap;
mod context;
mod error;
pub mod logger;
mod pending;
mod render;

pub use bootstrap::BootstrapLogger;
pub use context::{
    CauseChain, ContextField, ContextValidationError, DiagnosticContext, ErrorCause,
    OperationContext, ReportStatus, SourceLocation, StackTrace, TraceCaptureScope, TraceOrigin,
    collect_causes, validate_context,
};
pub use error::{DiagnosticError, ErrorConstructionError, RecoverableError};
pub use logger::{
    LogLevel, LogOptions, LogRecord, LogStats, Logger, LoggerInitError, ThrottleOptions,
};
pub use pending::{FeatureStatus, configure_remote_logging, enable_fatal_system_capture};
pub use render::{Environment, ErrorDetails, RenderedError, render_error};

#[cfg(test)]
mod tests;
