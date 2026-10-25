//! Application-scoped configuration, data, and cache directory contract.

mod contract;
mod native;

pub use contract::SystemPaths;
pub(crate) use native::packaged_resources_root;
pub use native::{NativeRuntimePathResolver, NativeSystemPaths};
