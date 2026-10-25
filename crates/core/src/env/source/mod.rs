//! Environment source adapters.
//!
//! The environment reader depends on this small port; process variables and
//! project files are replaceable adapters behind the same interface.

mod file;
mod process;

pub use file::FileEnvironment;
pub use process::ProcessEnvironment;

use super::EnvironmentError;

pub trait EnvironmentSource: Send + Sync {
    fn read(&self, key: &str) -> Result<Option<String>, EnvironmentError>;
}
