use std::{
    backtrace::Backtrace, cell::Cell, error::Error, fmt, marker::PhantomData, rc::Rc, sync::Arc,
};

thread_local! {
    static CAPTURE_ENABLED: Cell<bool> = const { Cell::new(true) };
}

/// Caller source information captured at diagnostic construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    file: &'static str,
    line: u32,
    column: u32,
}

impl SourceLocation {
    #[track_caller]
    pub fn caller() -> Self {
        let location = std::panic::Location::caller();
        Self {
            file: location.file(),
            line: location.line(),
            column: location.column(),
        }
    }
    pub const fn file_name(self) -> &'static str {
        self.file
    }
    pub const fn line(self) -> u32 {
        self.line
    }
    pub const fn column(self) -> u32 {
        self.column
    }
}

/// Temporarily enables or disables expensive trace capture for this thread.
pub struct TraceCaptureScope {
    previous: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl TraceCaptureScope {
    pub fn new(enabled: bool) -> Self {
        let previous = CAPTURE_ENABLED.with(|slot| slot.replace(enabled));
        Self {
            previous,
            _thread_bound: PhantomData,
        }
    }
}

impl Drop for TraceCaptureScope {
    fn drop(&mut self) {
        CAPTURE_ENABLED.with(|slot| slot.set(self.previous));
    }
}

/// Captured lazily formatted Rust backtrace. `Backtrace` performs symbol
/// resolution when rendered, keeping construction and reporting separate.
#[derive(Clone)]
pub struct StackTrace {
    enabled: bool,
    trace: Option<Arc<Backtrace>>,
}

impl fmt::Debug for StackTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StackTrace")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl StackTrace {
    pub fn capture() -> Self {
        let enabled = CAPTURE_ENABLED.with(Cell::get);
        let trace = enabled.then(|| Arc::new(Backtrace::force_capture()));
        Self { enabled, trace }
    }
    pub fn format(&self) -> String {
        if !self.enabled {
            return "Traza desactivada".into();
        }
        self.trace
            .as_ref()
            .map_or_else(|| "Traza no disponible".into(), |trace| trace.to_string())
    }
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceOrigin {
    ExceptionConstruction,
    Handler,
}

#[derive(Clone, Debug)]
pub struct DiagnosticContext {
    pub location: Option<SourceLocation>,
    pub origin: TraceOrigin,
    pub stack: StackTrace,
}

impl DiagnosticContext {
    pub fn from_handler() -> Self {
        Self {
            location: None,
            origin: TraceOrigin::Handler,
            stack: StackTrace::capture(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextField {
    pub key: String,
    pub value: String,
}

pub fn validate_context(fields: &[ContextField]) -> Result<(), ContextValidationError> {
    if fields.len() > 16 {
        return Err(ContextValidationError::TooManyFields);
    }
    for (index, field) in fields.iter().enumerate() {
        if field.key.is_empty() || field.key.len() > 64 || field.value.len() > 256 {
            return Err(ContextValidationError::InvalidFieldSize);
        }
        if fields[..index]
            .iter()
            .any(|previous| previous.key == field.key)
        {
            return Err(ContextValidationError::DuplicateKey);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextValidationError {
    TooManyFields,
    InvalidFieldSize,
    DuplicateKey,
}

impl fmt::Display for ContextValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooManyFields => "Context supports up to 16 fields",
            Self::InvalidFieldSize => "Invalid context field size",
            Self::DuplicateKey => "Duplicate context key",
        })
    }
}
impl Error for ContextValidationError {}

#[derive(Clone, Copy, Debug)]
pub struct OperationContext<'a> {
    pub id: &'a str,
    pub module: &'a str,
    pub fields: &'a [ContextField],
}

impl<'a> OperationContext<'a> {
    pub fn new(id: &'a str) -> Self {
        Self {
            id,
            module: "application",
            fields: &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ErrorCause {
    pub code: Option<String>,
    pub message: String,
    pub context: Option<DiagnosticContext>,
}

#[derive(Clone, Debug, Default)]
pub struct CauseChain {
    pub causes: Vec<ErrorCause>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportStatus {
    Written,
    Filtered,
    Throttled,
    Ignored,
    Failed,
}

/// Collect nested causes (excluding the root) with bounded message size/depth.
pub fn collect_causes(error: &(dyn Error + 'static)) -> CauseChain {
    let mut result = CauseChain::default();
    let mut current = error.source();
    while let Some(cause) = current {
        if result.causes.len() == 8 {
            result.truncated = true;
            break;
        }
        let diagnostic = cause.downcast_ref::<crate::diagnostics::DiagnosticError>();
        result.causes.push(ErrorCause {
            code: diagnostic.map(|value| value.code().to_owned()),
            message: truncate_utf8(&cause.to_string(), 512),
            context: diagnostic.map(|value| value.context().clone()),
        });
        current = cause.source();
    }
    result
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_owned();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
