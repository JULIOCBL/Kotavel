use super::EnvironmentSource;
use crate::env::{EnvironmentError, value::validate_key};

const MAX_VALUE_BYTES: usize = 4096;

#[derive(Clone, Debug, Default)]
pub struct ProcessEnvironment;

impl EnvironmentSource for ProcessEnvironment {
    fn read(&self, key: &str) -> Result<Option<String>, EnvironmentError> {
        validate_key(key)?;
        match std::env::var(key) {
            Ok(value) if value.len() <= MAX_VALUE_BYTES => Ok(Some(value)),
            Ok(_) => Err(EnvironmentError::ValueTooLong),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(EnvironmentError::InvalidApplication(
                format!("Environment variable {key} is not valid Unicode"),
            )),
        }
    }
}
