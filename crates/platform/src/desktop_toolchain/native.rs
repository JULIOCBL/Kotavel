use framework_core::contracts::DesktopToolchain;
use std::{path::Path, process::Command};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeDesktopToolchain;

impl DesktopToolchain for NativeDesktopToolchain {
    fn install(&self, project: &Path) -> Result<(), String> {
        #[cfg(windows)]
        return windows::install(project);

        #[cfg(not(windows))]
        {
            let _ = project;
            Ok(())
        }
    }

    fn configure_cargo(&self, command: &mut Command, project: &Path) {
        #[cfg(windows)]
        windows::configure_cargo(command, project);

        #[cfg(not(windows))]
        let _ = (command, project);
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        env,
        path::{Path, PathBuf},
        process::Command,
    };

    pub(super) fn install(project: &Path) -> Result<(), String> {
        let script = project.join("tools/install_windows_desktop.ps1");
        let status = Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .status()
            .map_err(|error| format!("no se pudo iniciar la preparación de Qt/MinGW: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "la preparación de Qt/MinGW terminó con código {}",
                status.code().unwrap_or(1)
            ))
        }
    }

    pub(super) fn configure_cargo(cargo: &mut Command, project: &Path) {
        if env::var_os("CARGO_TARGET_DIR").is_none() {
            let drive = env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
            let target_root = PathBuf::from(format!("{drive}\\ktarget"));
            let project_name = project
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("kotavel"));
            cargo.env("CARGO_TARGET_DIR", target_root.join(project_name));
        }
        if env::var_os("RUSTDOCFLAGS").is_none() {
            cargo.env("RUSTDOCFLAGS", "-C link-self-contained=no");
        }
        if env::var_os("RUSTFLAGS").is_none() && env::var_os("CARGO_ENCODED_RUSTFLAGS").is_none() {
            cargo.env("RUSTFLAGS", "-C link-self-contained=no");
        }

        let mut roots = Vec::new();
        if let Some(root) = env::var_os("MSYS2_ROOT") {
            roots.push(PathBuf::from(root));
        }
        roots.push(PathBuf::from(r"C:\msys64"));
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local_app_data).join("Programs/MSYS2"));
        }
        let Some(bin) = roots
            .into_iter()
            .map(|root| root.join("mingw64/bin"))
            .find(|bin| bin.join("qmake.exe").is_file() && bin.join("gcc.exe").is_file())
        else {
            return;
        };

        if let Some(existing_path) = env::var_os("PATH") {
            let paths = env::split_paths(&existing_path).collect::<Vec<_>>();
            if paths.first() != Some(&bin) {
                let updated_paths = std::iter::once(bin.clone())
                    .chain(paths.into_iter().filter(|path| path != &bin))
                    .collect::<Vec<_>>();
                if let Ok(updated_path) = env::join_paths(updated_paths) {
                    cargo.env("PATH", updated_path);
                }
            }
        }
        set_env_if_missing(cargo, "QMAKE", bin.join("qmake.exe").into_os_string());
        set_env_if_missing(
            cargo,
            "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER",
            bin.join("gcc.exe").into_os_string(),
        );
        set_env_if_missing(
            cargo,
            "CC_x86_64_pc_windows_gnu",
            bin.join("gcc.exe").into_os_string(),
        );
        set_env_if_missing(
            cargo,
            "CXX_x86_64_pc_windows_gnu",
            bin.join("g++.exe").into_os_string(),
        );
        set_qt_query_path(cargo, &bin, "QT_INSTALL_PLUGINS", "QT_PLUGIN_PATH");
        set_qt_query_path(cargo, &bin, "QT_INSTALL_QML", "QML2_IMPORT_PATH");
        if env::var_os("QT_QPA_PLATFORM_PLUGIN_PATH").is_none() {
            set_qt_query_path(
                cargo,
                &bin,
                "QT_INSTALL_PLUGINS",
                "QT_QPA_PLATFORM_PLUGIN_PATH",
            );
        }
    }

    fn set_env_if_missing(cargo: &mut Command, key: &str, value: std::ffi::OsString) {
        if env::var_os(key).is_none() {
            cargo.env(key, value);
        }
    }

    fn set_qt_query_path(cargo: &mut Command, bin: &Path, query: &str, key: &str) {
        if env::var_os(key).is_some() {
            return;
        }
        let Ok(output) = Command::new(bin.join("qmake.exe"))
            .args(["-query", query])
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !value.is_empty() {
            let value = if key == "QT_QPA_PLATFORM_PLUGIN_PATH" {
                PathBuf::from(value).join("platforms").into_os_string()
            } else {
                value.into()
            };
            cargo.env(key, value);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{env, ffi::OsStr, path::PathBuf};

    #[test]
    fn msys2_precedes_other_runtimes_in_the_cargo_path() {
        let bin = Path::new(r"C:\msys64\mingw64\bin");
        if !bin.join("qmake.exe").is_file() || !bin.join("gcc.exe").is_file() {
            return;
        }

        let mut command = Command::new("cargo");
        NativeDesktopToolchain.configure_cargo(&mut command, Path::new("C:\\project"));
        let path = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PATH"))
            .and_then(|(_, value)| value)
            .expect("toolchain configuration should set PATH");
        let entries = env::split_paths(path).collect::<Vec<_>>();
        assert_eq!(entries.first().map(PathBuf::as_path), Some(bin));
    }
}
