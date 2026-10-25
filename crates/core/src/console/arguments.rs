use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

/// Common runtime options removed before application-specific argument parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleArguments {
    pub environment: String,
    pub root: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment_explicit: bool,
    pub help: bool,
    pub debug: bool,
}

impl Default for ConsoleArguments {
    fn default() -> Self {
        Self {
            environment: "production".into(),
            root: PathBuf::new(),
            arguments: Vec::new(),
            environment_explicit: false,
            help: false,
            debug: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleArgumentError {
    InvalidUnicodeEnvironment,
    EmptyProjectRoot,
}

impl std::fmt::Display for ConsoleArgumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUnicodeEnvironment => {
                f.write_str("Console environment option must be valid Unicode")
            }
            Self::EmptyProjectRoot => f.write_str("Empty project root"),
        }
    }
}

impl std::error::Error for ConsoleArgumentError {}

/// Parses common options from user arguments (excluding the executable path).
/// Unknown arguments are retained unchanged for the application callback.
pub fn parse_console_arguments<I, S>(arguments: I) -> Result<ConsoleArguments, ConsoleArgumentError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut options = ConsoleArguments::default();
    for argument in arguments {
        let argument = argument.as_ref();
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            options.help = true;
        } else if argument == OsStr::new("--development") {
            options.debug = true;
        } else if let Some(value) = argument
            .to_str()
            .and_then(|value| value.strip_prefix("--env="))
        {
            options.environment = value.to_owned();
            options.environment_explicit = true;
        } else if let Some(value) = argument
            .to_str()
            .and_then(|value| value.strip_prefix("--project-root="))
        {
            if value.is_empty() {
                return Err(ConsoleArgumentError::EmptyProjectRoot);
            }
            options.root = PathBuf::from(value);
        } else if argument.to_string_lossy().starts_with("--env=") {
            return Err(ConsoleArgumentError::InvalidUnicodeEnvironment);
        } else {
            options.arguments.push(argument.to_owned());
        }
    }
    Ok(options)
}
