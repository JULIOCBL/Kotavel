use super::super::{ContextField, DiagnosticContext, ErrorCause};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    str::FromStr,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl LogLevel {
    pub const ALL: [Self; 9] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Notice,
        Self::Warning,
        Self::Error,
        Self::Critical,
        Self::Alert,
        Self::Emergency,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Alert => "alert",
            Self::Emergency => "emergency",
        }
    }
}

impl FromStr for LogLevel {
    type Err = std::io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "notice" => Ok(Self::Notice),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            "critical" => Ok(Self::Critical),
            "alert" => Ok(Self::Alert),
            "emergency" => Ok(Self::Emergency),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported log level: {value}"),
            )),
        }
    }
}

/// Standard destination selected by the single `LOG_CHANNEL` configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogChannel {
    /// Write records to the rotating `kotavel.log` file.
    #[default]
    Single,
    /// Write records to standard error.
    Stderr,
    /// Write records to both the rotating file and standard error.
    Stack,
    /// Disable logger output.
    Null,
}

impl LogChannel {
    /// Whether this channel writes records to the rotating file.
    pub const fn writes_file(self) -> bool {
        matches!(self, Self::Single | Self::Stack)
    }

    /// Whether this channel writes records to standard error.
    pub const fn writes_console(self) -> bool {
        matches!(self, Self::Stderr | Self::Stack)
    }

    /// Whether this channel accepts log records.
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::Null)
    }
}

impl FromStr for LogChannel {
    type Err = std::io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "single" => Ok(Self::Single),
            "stderr" => Ok(Self::Stderr),
            "stack" => Ok(Self::Stack),
            "null" => Ok(Self::Null),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported log channel: {value}"),
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThrottleOptions {
    pub max_reports: usize,
    pub interval: Duration,
    pub max_keys: usize,
}

impl Default for ThrottleOptions {
    fn default() -> Self {
        Self {
            max_reports: 0,
            interval: Duration::from_secs(60),
            max_keys: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogOptions {
    pub directory: PathBuf,
    pub max_bytes: usize,
    pub backups: usize,
    /// Destination policy selected by `LOG_CHANNEL`.
    pub channel: LogChannel,
    pub min_level: LogLevel,
    pub module: String,
    pub throttle: ThrottleOptions,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            directory: PathBuf::new(),
            max_bytes: 512 * 1024 * 1024,
            backups: 3,
            channel: LogChannel::Stack,
            min_level: LogLevel::Info,
            module: "application".into(),
            throttle: ThrottleOptions::default(),
        }
    }
}

impl LogOptions {
    /// Builds application-facing logger options and injects framework-owned
    /// throttling defaults. The log directory is constrained to the project's
    /// `storage` directory, and the selected channel controls all output targets.
    pub fn configured(
        project_root: PathBuf,
        log_directory: String,
        max_bytes: usize,
        backups: usize,
        channel: String,
        min_level: String,
        module: String,
    ) -> Result<Self, std::io::Error> {
        let directory = storage_log_directory(&project_root, &log_directory)?;
        let channel = channel.parse::<LogChannel>()?;
        Ok(Self {
            directory,
            max_bytes,
            backups,
            channel,
            min_level: min_level.parse()?,
            module,
            throttle: ThrottleOptions::default(),
        })
    }
}

fn storage_log_directory(project_root: &Path, log_directory: &str) -> io::Result<PathBuf> {
    let relative = Path::new(log_directory);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOG_DIRECTORY must be a relative path inside storage",
        ));
    }

    let storage = project_root.join("storage");
    match fs::symlink_metadata(&storage) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LOG_DIRECTORY cannot use a symlink for storage",
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LOG_DIRECTORY must resolve to a directory inside storage",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let mut current = storage.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            unreachable!("validated LOG_DIRECTORY components are normal")
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LOG_DIRECTORY cannot contain symlinks inside storage",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LOG_DIRECTORY must resolve to a directory inside storage",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    Ok(storage.join(relative))
}

/// Borrowed record content remains valid only for the duration of `write`.
#[derive(Clone, Copy)]
pub struct LogRecord<'a> {
    pub level: LogLevel,
    pub code: &'a str,
    pub operation: &'a str,
    pub message: &'a str,
    pub module: &'a str,
    pub fields: &'a [ContextField],
    pub diagnostic: Option<&'a DiagnosticContext>,
    pub causes: &'a [ErrorCause],
    pub causes_truncated: bool,
}

impl Default for LogRecord<'_> {
    fn default() -> Self {
        Self {
            level: LogLevel::Error,
            code: "",
            operation: "",
            message: "",
            module: "",
            fields: &[],
            diagnostic: None,
            causes: &[],
            causes_truncated: false,
        }
    }
}
