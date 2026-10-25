//! Native filesystem operations, grouped by capability and target OS.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_shared;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
#[cfg(target_os = "macos")]
pub(crate) use macos::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) use unsupported::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
#[cfg(windows)]
pub(crate) use windows::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
