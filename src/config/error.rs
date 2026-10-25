#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    EmptyKey,
    ControlCharacter,
    KeyTooLong,
    TooDeep,
    TooManyNodes,
    InvalidGroup(String),
    ConflictingStructure(String),
    GroupCollision(String),
    Missing(String),
    TypeMismatch(String),
    NoActiveConfiguration,
    MissingLoader,
    DuplicateGroup(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKey => f.write_str("Configuration key cannot be empty"),
            Self::ControlCharacter => f.write_str("Configuration key contains a control character"),
            Self::KeyTooLong => f.write_str("Configuration key exceeds 1024 bytes"),
            Self::TooDeep => f.write_str("Configuration nesting exceeds 32 levels"),
            Self::TooManyNodes => f.write_str("Configuration exceeds 10000 map nodes"),
            Self::InvalidGroup(group) => write!(f, "Invalid configuration group: {group}"),
            Self::ConflictingStructure(key) => {
                write!(f, "Conflicting configuration structure at {key}")
            }
            Self::GroupCollision(key) => {
                write!(f, "Configuration group collides with existing key {key}")
            }
            Self::Missing(key) => write!(f, "Configuration key not found: {key}"),
            Self::TypeMismatch(key) => write!(f, "Configuration type mismatch for key: {key}"),
            Self::NoActiveConfiguration => f.write_str("No active application configuration"),
            Self::MissingLoader => f.write_str("Configuration file requires a loader"),
            Self::DuplicateGroup(group) => write!(f, "Duplicate configuration group: {group}"),
        }
    }
}

impl std::error::Error for ConfigError {}
