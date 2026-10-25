use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub fn restrict_directory(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
pub fn restrict_file(file: &File) -> io::Result<()> {
    file.set_permissions(fs::Permissions::from_mode(0o600))
}
pub fn restrict_file_path(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}
pub fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}
pub fn replace_file_atomically(temporary: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(temporary, destination)
}
pub fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}
