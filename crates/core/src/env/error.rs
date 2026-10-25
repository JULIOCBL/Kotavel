use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentError {
    InvalidKey,
    ValueTooLong,
    MissingRequired(String),
    TypeMismatch {
        key: String,
        expected: &'static str,
    },
    FileOpen(PathBuf),
    InvalidFile {
        path: PathBuf,
        line: usize,
        reason: String,
    },
    InvalidApplication(String),
}

impl std::fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => f.write_str("Invalid environment key"),
            Self::ValueTooLong => f.write_str("Environment value exceeds 4096 bytes"),
            Self::MissingRequired(key) => {
                write!(f, "Required environment variable is missing: {key}")
            }
            Self::TypeMismatch { key, expected } => {
                write!(f, "Environment variable {key} must be {expected}")
            }
            Self::FileOpen(path) => write!(f, "Cannot open environment file: {}", path.display()),
            Self::InvalidFile { path, line, reason } => {
                if *line == 0 {
                    write!(f, "Invalid environment file {}: {reason}", path.display())
                } else {
                    write!(
                        f,
                        "Invalid environment file {} at line {line}: {reason}",
                        path.display()
                    )
                }
            }
            Self::InvalidApplication(reason) => f.write_str(reason),
        }
    }
}
impl std::error::Error for EnvironmentError {}
