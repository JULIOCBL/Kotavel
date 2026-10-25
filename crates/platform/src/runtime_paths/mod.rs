//! Resource resolver adapters.
pub use framework_core::runtime_paths::{RuntimePathResolver, RuntimePaths};
pub use framework_core::system_paths::NativeRuntimePathResolver;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
