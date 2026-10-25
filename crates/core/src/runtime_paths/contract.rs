use std::{
    io,
    path::{Path, PathBuf},
};

/// Resource locations that travel with an executable or application bundle.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimePaths {
    pub application_root: PathBuf,
    pub plugins: PathBuf,
    pub translations: PathBuf,
    pub certificates: PathBuf,
}

pub trait RuntimePathResolver: Send + Sync {
    fn resolve(&self, executable: &Path) -> io::Result<RuntimePaths>;
}
