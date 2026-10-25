use crate::{
    runtime_paths::{RuntimePathResolver, RuntimePaths},
    system_paths::SystemPaths,
};
use std::{
    env, io,
    path::{Path, PathBuf},
};

/// Platform-native directories for application settings, persistent data, and cache.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSystemPaths;

impl SystemPaths for NativeSystemPaths {
    fn config_directory(&self, application: &str) -> io::Result<PathBuf> {
        let base = match crate::system::current_os() {
            crate::system::OperatingSystem::Windows => windows_roaming_root()?,
            crate::system::OperatingSystem::MacOS => home()?.join("Library/Preferences"),
            crate::system::OperatingSystem::Linux => xdg_root("XDG_CONFIG_HOME", ".config")?,
            crate::system::OperatingSystem::Unknown => return Err(unsupported()),
        };
        application_directory(base, application)
    }

    fn data_directory(&self, application: &str) -> io::Result<PathBuf> {
        let base = match crate::system::current_os() {
            crate::system::OperatingSystem::Windows => windows_roaming_root()?,
            crate::system::OperatingSystem::MacOS => home()?.join("Library/Application Support"),
            crate::system::OperatingSystem::Linux => xdg_root("XDG_DATA_HOME", ".local/share")?,
            crate::system::OperatingSystem::Unknown => return Err(unsupported()),
        };
        application_directory(base, application)
    }

    fn cache_directory(&self, application: &str) -> io::Result<PathBuf> {
        let base = match crate::system::current_os() {
            crate::system::OperatingSystem::Windows => windows_local_root()?
                .join(application_component(application)?)
                .join("Cache"),
            crate::system::OperatingSystem::MacOS => home()?
                .join("Library/Caches")
                .join(application_component(application)?),
            crate::system::OperatingSystem::Linux => {
                xdg_root("XDG_CACHE_HOME", ".cache")?.join(application_component(application)?)
            }
            crate::system::OperatingSystem::Unknown => return Err(unsupported()),
        };
        Ok(base)
    }
}

/// Resolves resources shipped beside an executable or within a macOS app bundle.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRuntimePathResolver;

impl RuntimePathResolver for NativeRuntimePathResolver {
    fn resolve(&self, executable: &Path) -> io::Result<RuntimePaths> {
        let binary = absolute_normalized(executable)?;
        match crate::system::current_os() {
            crate::system::OperatingSystem::Linux => {
                let bin = binary
                    .parent()
                    .ok_or_else(|| invalid_path("Executable has no parent directory"))?;
                let prefix = if bin.file_name().is_some_and(|name| name == "bin") {
                    bin.parent().unwrap_or(bin)
                } else {
                    bin
                };
                let app = binary
                    .file_stem()
                    .ok_or_else(|| invalid_path("Executable has no file name"))?;
                let share = prefix.join("share").join(app);
                Ok(RuntimePaths {
                    application_root: prefix.to_owned(),
                    plugins: prefix.join("lib/kotavel/plugins"),
                    translations: share.join("translations"),
                    certificates: share.join("certificates"),
                })
            }
            crate::system::OperatingSystem::MacOS => {
                let macos = binary
                    .parent()
                    .ok_or_else(|| invalid_path("Executable has no parent directory"))?;
                let contents = macos
                    .parent()
                    .ok_or_else(|| invalid_path("Executable has no bundle Contents directory"))?;
                let bundle = contents
                    .parent()
                    .ok_or_else(|| invalid_path("Executable has no app bundle"))?;
                if macos.file_name().is_some_and(|name| name == "MacOS")
                    && contents.file_name().is_some_and(|name| name == "Contents")
                    && bundle
                        .extension()
                        .is_some_and(|extension| extension == "app")
                {
                    let resources = contents.join("Resources");
                    Ok(RuntimePaths {
                        application_root: bundle.to_owned(),
                        plugins: contents.join("PlugIns"),
                        translations: resources.join("translations"),
                        certificates: resources.join("certificates"),
                    })
                } else {
                    sibling_runtime_paths(&binary)
                }
            }
            crate::system::OperatingSystem::Windows => sibling_runtime_paths(&binary),
            crate::system::OperatingSystem::Unknown => Err(unsupported()),
        }
    }
}

fn application_directory(base: PathBuf, application: &str) -> io::Result<PathBuf> {
    Ok(base.join(application_component(application)?))
}

fn application_component(application: &str) -> io::Result<&str> {
    if application.is_empty()
        || application == "."
        || application == ".."
        || application.contains('/')
        || application.contains('\\')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Application path name must be a single non-empty component",
        ));
    }
    Ok(application)
}

fn sibling_runtime_paths(binary: &Path) -> io::Result<RuntimePaths> {
    let root = binary
        .parent()
        .ok_or_else(|| invalid_path("Executable has no parent directory"))?;
    Ok(RuntimePaths {
        application_root: root.to_owned(),
        plugins: root.join("plugins"),
        translations: root.join("translations"),
        certificates: root.join("certificates"),
    })
}

/// Returns the packaged resource directory for Kotavel macOS bundles.
///
/// The marker distinguishes Kotavel bundles from unrelated `.app` packages.
pub(crate) fn packaged_resources_root(executable: &Path) -> Option<PathBuf> {
    if crate::system::current_os() != crate::system::OperatingSystem::MacOS {
        return None;
    }
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    if macos.file_name()? != "MacOS"
        || contents.file_name()? != "Contents"
        || bundle.extension()? != "app"
    {
        return None;
    }
    let resources = contents.join("Resources");
    let marker = resources.join("kotavel-environment");
    let environment = std::fs::read_to_string(marker).ok()?;
    if !matches!(environment.trim(), "local" | "development" | "production") {
        return None;
    }
    Some(resources)
}

fn absolute_normalized(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(invalid_path("Executable path cannot be empty"));
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        env::current_dir()?.join(path)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            component => result.push(component.as_os_str()),
        }
    }
    result
}

fn home() -> io::Result<PathBuf> {
    let key = if crate::system::current_os() == crate::system::OperatingSystem::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    };
    env_path(key)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{key} is unavailable")))
}

fn xdg_root(variable: &str, fallback: &str) -> io::Result<PathBuf> {
    if let Some(path) = env_path(variable).filter(|path| path.is_absolute()) {
        return Ok(path);
    }
    Ok(home()?.join(fallback))
}

fn windows_roaming_root() -> io::Result<PathBuf> {
    match env_path("APPDATA") {
        Some(path) => Ok(path),
        None => Ok(home()?.join("AppData/Roaming")),
    }
}

fn windows_local_root() -> io::Result<PathBuf> {
    match env_path("LOCALAPPDATA") {
        Some(path) => Ok(path),
        None => Ok(home()?.join("AppData/Local")),
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn invalid_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Native path provider is unavailable for this platform",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_system_paths_return_app_scoped_locations_and_reject_traversal() {
        let paths = NativeSystemPaths;
        let config = paths.config_directory("KotavelPathTest").unwrap();
        let data = paths.data_directory("KotavelPathTest").unwrap();
        let cache = paths.cache_directory("KotavelPathTest").unwrap();
        assert!(config.is_absolute() && data.is_absolute() && cache.is_absolute());
        assert_eq!(config.file_name().unwrap(), "KotavelPathTest");
        assert_eq!(data.file_name().unwrap(), "KotavelPathTest");
        let expected_cache =
            if crate::system::current_os() == crate::system::OperatingSystem::Windows {
                "Cache"
            } else {
                "KotavelPathTest"
            };
        assert_eq!(cache.file_name().unwrap(), expected_cache);

        for name in ["", ".", "..", "../escape", "nested/name", "nested\\name"] {
            assert_eq!(
                paths.config_directory(name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                paths.data_directory(name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                paths.cache_directory(name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn runtime_paths_match_the_active_platform_layout() {
        let resolver = NativeRuntimePathResolver;
        assert_eq!(
            resolver.resolve(Path::new("")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let root = env::current_dir().unwrap().join("kotavel-platform-fixture");
        let resolved = match crate::system::current_os() {
            crate::system::OperatingSystem::Linux => {
                let executable = root.join("prefix/bin/kotavel");
                let paths = resolver.resolve(&executable).unwrap();
                assert_eq!(paths.application_root, root.join("prefix"));
                assert_eq!(paths.plugins, root.join("prefix/lib/kotavel/plugins"));
                assert_eq!(
                    paths.translations,
                    root.join("prefix/share/kotavel/translations")
                );
                assert_eq!(
                    paths.certificates,
                    root.join("prefix/share/kotavel/certificates")
                );
                paths
            }
            crate::system::OperatingSystem::MacOS => {
                let bundle = root.join("Kotavel.app");
                let paths = resolver
                    .resolve(&bundle.join("Contents/MacOS/Kotavel"))
                    .unwrap();
                assert_eq!(paths.application_root, bundle);
                assert_eq!(paths.plugins, root.join("Kotavel.app/Contents/PlugIns"));
                assert_eq!(
                    paths.translations,
                    root.join("Kotavel.app/Contents/Resources/translations")
                );
                assert_eq!(
                    paths.certificates,
                    root.join("Kotavel.app/Contents/Resources/certificates")
                );
                paths
            }
            crate::system::OperatingSystem::Windows => {
                let directory = root.join("application");
                let paths = resolver.resolve(&directory.join("Kotavel.exe")).unwrap();
                assert_eq!(paths.application_root, directory);
                assert_eq!(paths.plugins, root.join("application/plugins"));
                assert_eq!(paths.translations, root.join("application/translations"));
                assert_eq!(paths.certificates, root.join("application/certificates"));
                paths
            }
            crate::system::OperatingSystem::Unknown => return,
        };
        assert!(resolved.application_root.is_absolute());
    }

    #[test]
    fn packaged_resources_require_a_valid_kotavel_bundle_marker() {
        let directory = tempfile::tempdir().unwrap();
        let resources = directory.path().join("Kotavel.app/Contents/Resources");
        let executable = directory.path().join("Kotavel.app/Contents/MacOS/Kotavel");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("kotavel-environment"), "development\n").unwrap();
        if crate::system::current_os() == crate::system::OperatingSystem::MacOS {
            assert_eq!(
                packaged_resources_root(&executable),
                Some(resources.clone())
            );
        } else {
            assert_eq!(packaged_resources_root(&executable), None);
        }
        std::fs::write(resources.join("kotavel-environment"), "invalid\n").unwrap();
        assert_eq!(packaged_resources_root(&executable), None);
    }
}
