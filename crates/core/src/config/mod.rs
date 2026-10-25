//! Explicit, thread-safe-by-ownership configuration primitives.
//!
//! Values are stored under complete dotted keys. Reads do not mutate the store;
//! group reconstruction is performed only when requested. Mutations require an
//! exclusive `&mut Config`, so concurrent access follows Rust's ownership rules.

mod error;
mod files;
mod scope;
mod store;

pub use error::ConfigError;
pub use files::{ConfigFile, load_config_files};
pub use scope::{
    ConfigValueView, config, config_get, config_get_many, config_set, with_config_scope,
};
pub use serde_json::{Map as ConfigMap, Value as ConfigValue};
pub use store::Config;

#[cfg(test)]
mod tests;
