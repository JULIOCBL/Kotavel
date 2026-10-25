#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageError {
    InvalidKey(String),
    UnknownKey(String),
    InvalidParameterName(String),
    DuplicateParameter(String),
    MissingParameter { name: String, key: String },
    UnusedParameter { name: String, key: String },
    InvalidCatalog(String),
}

impl std::fmt::Display for MessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey(key) => write!(f, "Invalid message key: {key}"),
            Self::UnknownKey(key) => write!(f, "Unknown message key: {key}"),
            Self::InvalidParameterName(name) => {
                write!(f, "Invalid message parameter name: {name}")
            }
            Self::DuplicateParameter(name) => write!(f, "Duplicate message parameter: {name}"),
            Self::MissingParameter { name, key } => {
                write!(f, "Missing message parameter '{name}' for key: {key}")
            }
            Self::UnusedParameter { name, key } => {
                write!(f, "Unused message parameter '{name}' for key: {key}")
            }
            Self::InvalidCatalog(reason) => write!(f, "Invalid built-in message catalog: {reason}"),
        }
    }
}

impl std::error::Error for MessageError {}
