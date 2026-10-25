//! Application-level services that are independent of runtime adapters.

mod app;
mod application;

pub use app::{App, AppLocaleScope, LocaleError, app};
pub use application::{
    Application, ApplicationError, ApplicationState, ErrorHandlingOptions, ErrorRule,
    OperationResult,
};
