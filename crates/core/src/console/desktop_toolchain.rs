use std::{path::Path, process::Command};

/// Host-specific setup required to install and build the desktop application.
pub trait DesktopToolchain: Send + Sync {
    /// Install the native desktop dependencies required by the current host.
    fn install(&self, project: &Path) -> Result<(), String>;

    /// Configure a Cargo command to use the host's native desktop toolchain.
    fn configure_cargo(&self, command: &mut Command, project: &Path);
}
