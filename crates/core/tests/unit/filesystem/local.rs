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
