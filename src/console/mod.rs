//! Portable command-line argument and runtime-settings support.

mod arguments;
mod context;
mod package;
mod platform;
mod runtime;
mod settings;
mod shell;

pub use arguments::{ConsoleArgumentError, ConsoleArguments, parse_console_arguments};
pub use context::{ConsoleCallbackError, ConsoleContext};
pub use package::package_project;
pub use runtime::{
    ConfigureConsole, CreateServiceProviders, ExecuteConsole, LoadConsoleConfig, execute_console,
    run_console,
};
pub use settings::{ConsoleSettings, ConsoleSettingsError, console_settings};
pub use shell::{InteractiveShellError, run_interactive_shell};

#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;
