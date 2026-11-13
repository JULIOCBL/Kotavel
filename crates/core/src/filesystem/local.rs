//! Local disk adapter for the filesystem contract.

use super::{FileCursor, FileLock, Filesystem, platform};
use fs2::FileExt;
#[cfg(windows)]
use std::collections::HashSet;
#[cfg(windows)]
use std::sync::{Condvar, Mutex, OnceLock};
use std::{
    fs::{self, File, ReadDir},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalFilesystem;

#[cfg(windows)]
static HELD_LOCKS: OnceLock<(Mutex<HashSet<PathBuf>>, Condvar)> = OnceLock::new();

pub(crate) fn restrict_existing_file(path: &Path) -> io::Result<()> {
    platform::restrict_file_path(path)
}

impl Filesystem for LocalFilesystem {
    fn create_directories(&self, path: &Path, restrict_permissions: bool) -> io::Result<()> {
        if path.as_os_str().is_empty() {
            return Err(invalid_input("directory path cannot be empty"));
        }
        fs::create_dir_all(path)?;
        if restrict_permissions {
            platform::restrict_directory(path)?;
        }
        Ok(())
    }

    fn read_file(&self, path: &Path, maximum_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        let limit = maximum_bytes.saturating_add(1) as u64;
        (&mut file).take(limit).read_to_end(&mut bytes)?;
        if bytes.len() > maximum_bytes {
            return Err(invalid_data("file exceeds the configured read limit"));
        }
        Ok(Some(bytes))
    }

    fn write_file_atomically(
        &self,
        path: &Path,
        bytes: &[u8],
        restrict_permissions: bool,
    ) -> io::Result<()> {
        if path.as_os_str().is_empty() {
            return Err(invalid_input("file path cannot be empty"));
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::Builder::new()
            .prefix(".kotavel-tmp-")
            .tempfile_in(parent)?;
        if restrict_permissions {
            platform::restrict_file_path(temporary.path())?;
        }
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        platform::replace_file_atomically(temporary.path(), path)?;
        platform::sync_directory(parent)?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn list_files(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            match entry.path().metadata() {
                Ok(metadata) if metadata.is_file() => files.push(entry.path()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(files)
    }

    fn scan_files(&self, directory: &Path) -> io::Result<Box<dyn FileCursor>> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => Some(entries),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Box::new(LocalFileCursor { entries }))
    }

    fn lock_file(&self, path: &Path) -> io::Result<Box<dyn FileLock>> {
        let file = platform::open_lock_file(path)?;
        #[cfg(windows)]
        let process_path = reserve_process_lock(path, true)?;
        if let Err(error) = file.lock_exclusive() {
            #[cfg(windows)]
            release_process_lock(&process_path);
            return Err(error);
        }
        Ok(Box::new(LocalFileLock {
            file,
            #[cfg(windows)]
            process_path,
        }))
    }

    fn try_lock_file(&self, path: &Path) -> io::Result<Box<dyn FileLock>> {
        let file = platform::open_lock_file(path)?;
        #[cfg(windows)]
        let process_path = reserve_process_lock(path, false)?;
        if let Err(error) = file.try_lock_exclusive() {
            #[cfg(windows)]
            release_process_lock(&process_path);
            return Err(platform::normalize_lock_error(error));
        }
        #[cfg(not(windows))]
        let process_path = ();
        Ok(Box::new(LocalFileLock {
            file,
            #[cfg(windows)]
            process_path,
        }))
    }
}

#[cfg(windows)]
fn reserve_process_lock(path: &Path, wait: bool) -> io::Result<Option<PathBuf>> {
    let canonical = fs::canonicalize(path)?;
    let (held_locks, changed) =
        HELD_LOCKS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
    let mut held = held_locks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while held.contains(&canonical) {
        if !wait {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "lock already held in this process",
            ));
        }
        held = changed
            .wait(held)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    held.insert(canonical.clone());
    Ok(Some(canonical))
}

#[cfg(windows)]
fn release_process_lock(path: &Option<PathBuf>) {
    let Some(path) = path else { return };
    if let Some((held_locks, changed)) = HELD_LOCKS.get() {
        held_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(path);
        changed.notify_one();
    }
}

struct LocalFileCursor {
    entries: Option<ReadDir>,
}
impl FileCursor for LocalFileCursor {
    fn next(&mut self) -> io::Result<Option<PathBuf>> {
        let Some(entries) = self.entries.as_mut() else {
            return Ok(None);
        };
        loop {
            let Some(entry) = entries.next() else {
                return Ok(None);
            };
            let entry = entry?;
            let path = entry.path();
            match path.metadata() {
                Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
}

struct LocalFileLock {
    file: File,
    #[cfg(windows)]
    process_path: Option<PathBuf>,
}
impl FileLock for LocalFileLock {}
impl Drop for LocalFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        #[cfg(windows)]
        if let Some(path) = self.process_path.take() {
            release_process_lock(&Some(path));
        }
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
#[path = "../../tests/unit/filesystem/local.rs"]
mod tests;
