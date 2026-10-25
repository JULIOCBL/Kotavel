use crate::{config::Config, container::Container, env::AppEnvironment};
use std::{
    ffi::OsString,
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub type ConsoleCallbackError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Values shared with a console application's configure and execute callbacks.
pub struct ConsoleContext<'a> {
    pub root: &'a Path,
    pub identity: &'a AppEnvironment,
    pub arguments: &'a [OsString],
    pub(super) configuration: &'a Mutex<Config>,
    pub container: Option<&'a Container>,
}

impl ConsoleContext<'_> {
    /// Accesses the active configuration without exposing a borrowed alias.
    pub fn configuration(&self) -> MutexGuard<'_, Config> {
        self.configuration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
