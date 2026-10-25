//! Native application data and resource paths by operating system.

pub use framework_core::system_paths::{NativeSystemPaths, SystemPaths};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
