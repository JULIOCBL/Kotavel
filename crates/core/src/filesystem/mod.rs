//! Filesystem contracts and the native local-disk adapter.

use std::{
    io,
    path::{Path, PathBuf},
};

mod local;
mod platform;

pub use local::LocalFilesystem;
pub(crate) use local::restrict_existing_file;

/// Resumable, non-recursive enumeration. Owners serialize calls to `next`.
pub trait FileCursor: Send {
    fn next(&mut self) -> io::Result<Option<PathBuf>>;
}

/// Storage boundary shared by framework facilities such as sessions and cache.
pub trait Filesystem: Send + Sync {
    fn create_directories(&self, path: &Path, restrict_permissions: bool) -> io::Result<()>;
    fn read_file(&self, path: &Path, maximum_bytes: usize) -> io::Result<Option<Vec<u8>>>;
    fn write_file_atomically(
        &self,
        path: &Path,
        bytes: &[u8],
        restrict_permissions: bool,
    ) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<bool>;
    fn remove_files(&self, paths: &[PathBuf]) -> io::Result<()> {
        for path in paths {
            let _ = self.remove_file(path)?;
        }
        Ok(())
    }
    fn list_files(&self, directory: &Path) -> io::Result<Vec<PathBuf>>;
    fn scan_files(&self, directory: &Path) -> io::Result<Box<dyn FileCursor>>;
    fn lock_file(&self, path: &Path) -> io::Result<Box<dyn FileLock>>;
    /// Try to acquire an exclusive lock without waiting. Backends may provide a
    /// blocking fallback when nonblocking locks are unavailable.
    fn try_lock_file(&self, path: &Path) -> io::Result<Box<dyn FileLock>> {
        self.lock_file(path)
    }
}

/// OS-level exclusive lock held until the value is dropped.
pub trait FileLock: Send {}
