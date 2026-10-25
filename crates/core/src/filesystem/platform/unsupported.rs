use std::{fs::File, io, path::Path};

pub fn restrict_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private directory permissions are unsupported on this platform",
    ))
}
pub fn restrict_file(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private file permissions are unsupported on this platform",
    ))
}
pub fn restrict_file_path(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private file permissions are unsupported on this platform",
    ))
}
pub fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
pub fn replace_file_atomically(temporary: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(temporary, destination)
}
pub fn open_lock_file(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}
