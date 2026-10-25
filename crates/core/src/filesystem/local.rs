//! Local disk adapter for the filesystem contract.

use super::{FileCursor, FileLock, Filesystem, platform};
use fs2::FileExt;
use std::{
    fs::{self, File, ReadDir},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalFilesystem;

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
            platform::restrict_file(temporary.as_file())?;
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
        file.lock_exclusive()?;
        Ok(Box::new(LocalFileLock(file)))
    }

    fn try_lock_file(&self, path: &Path) -> io::Result<Box<dyn FileLock>> {
        let file = platform::open_lock_file(path)?;
        file.try_lock_exclusive()?;
        Ok(Box::new(LocalFileLock(file)))
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

struct LocalFileLock(File);
impl FileLock for LocalFileLock {}
impl Drop for LocalFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    #[test]
    fn local_filesystem_contract_covers_atomic_read_cursor_and_removal() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let fs = LocalFilesystem;
        let nested = root.path().join("storage/sessions");
        fs.create_directories(&nested, false)?;
        let target = nested.join("record.json");
        assert_eq!(fs.read_file(&target, 1024)?, None);
        fs.write_file_atomically(&target, b"first value", false)?;
        assert_eq!(
            fs.read_file(&target, 1024)?.as_deref(),
            Some(&b"first value"[..])
        );
        fs.write_file_atomically(&target, b"replacement", false)?;
        assert_eq!(
            fs.read_file(&target, 1024)?.as_deref(),
            Some(&b"replacement"[..])
        );
        assert_eq!(
            fs.read_file(&target, 3).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let second = nested.join("second.json");
        fs.write_file_atomically(&second, b"second", false)?;
        fs.create_directories(&nested.join("child"), false)?;
        assert_eq!(fs.list_files(&nested)?.len(), 2);
        let mut cursor = fs.scan_files(&nested)?;
        let first = cursor.next()?.expect("first regular file");
        assert!(fs.remove_file(&first)?);
        assert!(cursor.next()?.is_some());
        assert_eq!(cursor.next()?, None);
        assert_eq!(fs.scan_files(&root.path().join("missing"))?.next()?, None);
        fs.remove_files(&[target.clone(), second.clone()])?;
        assert!(!fs.remove_file(&target)?);
        Ok(())
    }

    #[test]
    fn concurrent_atomic_replacements_never_expose_partial_contents() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let target = Arc::new(root.path().join("shared.data"));
        let start = Arc::new(Barrier::new(16));
        let writers: Vec<_> = (0..16)
            .map(|worker| {
                let target = Arc::clone(&target);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    let payload = vec![b'A' + worker as u8; 64 * 1024];
                    start.wait();
                    for _ in 0..12 {
                        LocalFilesystem
                            .write_file_atomically(&target, &payload, false)
                            .unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("writer thread");
        }
        let content = LocalFilesystem
            .read_file(&target, 64 * 1024)?
            .expect("written data");
        assert_eq!(content.len(), 64 * 1024);
        assert!(content.iter().all(|byte| *byte == content[0]));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn restricted_storage_uses_private_unix_modes() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir()?;
        let directory = root.path().join("private");
        LocalFilesystem.create_directories(&directory, true)?;
        assert_eq!(
            fs::metadata(&directory)?.permissions().mode() & 0o777,
            0o700
        );
        let path = directory.join("secret");
        LocalFilesystem.write_file_atomically(&path, b"secret", true)?;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn file_lock_serializes_threads_until_drop() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("resource.lock");
        let first = LocalFilesystem.lock_file(&path)?;
        let acquired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_acquired = Arc::clone(&acquired);
        let thread_path = path.clone();
        let worker = thread::spawn(move || {
            let _second = LocalFilesystem.lock_file(&thread_path).unwrap();
            thread_acquired.store(true, std::sync::atomic::Ordering::Release);
        });
        thread::sleep(std::time::Duration::from_millis(30));
        assert!(!acquired.load(std::sync::atomic::Ordering::Acquire));
        drop(first);
        worker.join().expect("lock worker");
        assert!(acquired.load(std::sync::atomic::Ordering::Acquire));
        Ok(())
    }
}
