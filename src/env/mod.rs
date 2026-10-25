//! Environment access, `.env` loading, and validated application settings.

mod error;
mod settings;
mod source;
mod value;

pub use error::EnvironmentError;
pub use settings::AppEnvironment;
pub use source::{EnvironmentSource, FileEnvironment, ProcessEnvironment};
pub use value::{Env, EnvScope, EnvironmentValue, env, env_boolean, env_string};

#[cfg(test)]
mod tests;
