//! Linux filesystem operations share the POSIX implementation.

pub(crate) use super::unix_shared::{
    normalize_lock_error, open_lock_file, replace_file_atomically, restrict_directory,
    restrict_file_path, sync_directory,
};
