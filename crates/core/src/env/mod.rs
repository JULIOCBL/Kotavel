//! Environment access, `.env` loading, and validated application settings.

mod error;
mod settings;
mod source;
mod value;

pub use error::EnvironmentError;
pub use settings::AppEnvironment;
pub use source::{EnvironmentSource, FileEnvironment, ProcessEnvironment};
pub use value::{
    Env, EnvScope, EnvironmentInteger, EnvironmentValue, env, env_boolean, env_int, env_int_or,
    env_string, env_try_int_or,
};

#[cfg(test)]
#[path = "../../tests/unit/env/mod.rs"]
mod tests;
