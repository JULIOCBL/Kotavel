//! Platform boundary for local permissions, durability and lock-file creation.

#[cfg(unix)]
mod unix;
#[cfg(not(any(unix, windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
#[cfg(not(any(unix, windows)))]
pub(super) use unsupported::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
#[cfg(windows)]
pub(super) use windows::{
    open_lock_file, replace_file_atomically, restrict_directory, restrict_file, restrict_file_path,
    sync_directory,
};
