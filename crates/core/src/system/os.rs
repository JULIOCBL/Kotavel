/// Stable platform identity used by configuration keys and platform adapters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OperatingSystem {
    Windows,
    MacOS,
    Linux,
    Unknown,
}

impl OperatingSystem {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOS => "macOS",
            Self::Linux => "Linux",
            Self::Unknown => "Unknown",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Linux => "linux",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_windows(self) -> bool {
        matches!(self, Self::Windows)
    }
    pub const fn is_macos(self) -> bool {
        matches!(self, Self::MacOS)
    }
    pub const fn is_linux(self) -> bool {
        matches!(self, Self::Linux)
    }
}

/// Platform for which this crate was compiled, not a runtime guess.
pub const fn current_os() -> OperatingSystem {
    if cfg!(target_os = "windows") {
        OperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        OperatingSystem::MacOS
    } else if cfg!(target_os = "linux") {
        OperatingSystem::Linux
    } else {
        OperatingSystem::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_has_a_stable_name_and_key() {
        let os = current_os();
        assert!(!os.name().is_empty());
        assert!(os.key().bytes().all(|byte| byte.is_ascii_lowercase()));
        assert_eq!(os.is_windows(), os == OperatingSystem::Windows);
        assert_eq!(os.is_macos(), os == OperatingSystem::MacOS);
        assert_eq!(os.is_linux(), os == OperatingSystem::Linux);
    }
}
