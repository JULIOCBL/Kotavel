//! Filesystem adapter entry point for the host operating system.

pub use framework_core::filesystem::{FileCursor, FileLock, Filesystem, LocalFilesystem};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
