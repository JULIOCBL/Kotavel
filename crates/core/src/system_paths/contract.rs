use std::{io, path::PathBuf};

/// Resolve standard per-application directories through a native adapter.
pub trait SystemPaths: Send + Sync {
    fn config_directory(&self, application: &str) -> io::Result<PathBuf>;
    fn data_directory(&self, application: &str) -> io::Result<PathBuf>;
    fn cache_directory(&self, application: &str) -> io::Result<PathBuf>;
}
