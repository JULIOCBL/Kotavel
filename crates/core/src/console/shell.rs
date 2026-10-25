#[cfg(any(unix, windows))]
use std::env;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(windows)]
use std::path::PathBuf;
use std::{
    io::{self, IsTerminal},
    path::Path,
    process::Command,
};

/// Errors returned while opening an interactive shell for a project.
#[derive(Debug)]
pub enum InteractiveShellError {
    NonInteractive,
    PowerShellNotFound,
    ProjectRoot(io::Error),
    UnsupportedPlatform,
    Spawn(io::Error),
}

impl std::fmt::Display for InteractiveShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonInteractive => f.write_str("shell requiere una terminal interactiva"),
            Self::PowerShellNotFound => {
                f.write_str("no se encontró PowerShell (pwsh o powershell) en PATH")
            }
            Self::ProjectRoot(error) => {
                write!(f, "no se pudo resolver la raíz del proyecto: {error}")
            }
            Self::UnsupportedPlatform => {
                f.write_str("la terminal interactiva no está implementada para este sistema")
            }
            Self::Spawn(error) => write!(f, "no se pudo iniciar la terminal: {error}"),
        }
    }
}

impl std::error::Error for InteractiveShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ProjectRoot(error) | Self::Spawn(error) => Some(error),
            _ => None,
        }
    }
}

/// Opens the native interactive shell in `project` and exports its canonical root.
///
/// The caller must validate or prepare the project before entering the shell.
pub fn run_interactive_shell(project: &Path) -> Result<i32, InteractiveShellError> {
    if !io::stdin().is_terminal() {
        return Err(InteractiveShellError::NonInteractive);
    }

    let mut command = shell_command(project)?;
    println!(
        "Terminal Kotavel activa en {}. Escribe exit para salir.",
        project.display()
    );
    command
        .status()
        .map(|status| status.code().unwrap_or(1))
        .map_err(InteractiveShellError::Spawn)
}

fn shell_command(project: &Path) -> Result<Command, InteractiveShellError> {
    let project = project
        .canonicalize()
        .map_err(InteractiveShellError::ProjectRoot)?;

    #[cfg(windows)]
    let mut command = {
        let shell = find_program_in_path(&["pwsh.exe", "powershell.exe", "pwsh", "powershell"])
            .ok_or(InteractiveShellError::PowerShellNotFound)?;
        let mut command = Command::new(shell);
        command.args(power_shell_arguments());
        command
    };

    #[cfg(unix)]
    let mut command = {
        let shell = env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
        let mut command = Command::new(&shell);
        command.args(unix_shell_arguments(Path::new(&shell)));
        command
    };

    #[cfg(not(any(unix, windows)))]
    return Err(InteractiveShellError::UnsupportedPlatform);

    #[cfg(any(unix, windows))]
    {
        command
            .current_dir(&project)
            .env("KOTAVEL_PROJECT_ROOT", project);
        Ok(command)
    }
}

#[cfg(any(unix, test))]
fn unix_shell_arguments(shell: &Path) -> &'static [&'static str] {
    if shell.file_name().is_some_and(|name| name == "bash") {
        &["--noprofile", "--norc", "-i"]
    } else {
        &["-i"]
    }
}

#[cfg(any(windows, test))]
fn power_shell_arguments() -> &'static [&'static str] {
    &["-NoLogo", "-NoProfile"]
}

#[cfg(windows)]
fn find_program_in_path(candidates: &[&str]) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).find_map(|directory| {
        candidates
            .iter()
            .map(|candidate| directory.join(candidate))
            .find(|path| path.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_clean_interactive_shell_arguments_for_supported_shells() {
        assert_eq!(
            unix_shell_arguments(Path::new("/bin/bash")),
            ["--noprofile", "--norc", "-i"]
        );
        assert_eq!(unix_shell_arguments(Path::new("/bin/zsh")), ["-i"]);
        assert_eq!(power_shell_arguments(), ["-NoLogo", "-NoProfile"]);
    }

    #[cfg(unix)]
    #[test]
    fn shell_starts_in_project_and_exports_its_root() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().canonicalize().unwrap();
        let command = shell_command(&project.path().join(".")).unwrap();

        assert_eq!(command.get_current_dir(), Some(path.as_path()));
        assert!(command.get_envs().any(|(key, value)| {
            key == "KOTAVEL_PROJECT_ROOT" && value == Some(path.as_os_str())
        }));
    }

    #[test]
    fn refuses_to_spawn_a_shell_without_an_interactive_terminal() {
        if !io::stdin().is_terminal() {
            assert!(matches!(
                run_interactive_shell(Path::new(".")),
                Err(InteractiveShellError::NonInteractive)
            ));
        }
    }
}
