use super::{
    Env, EnvironmentError,
    value::{env_boolean, env_string},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEnvironment {
    pub name: String,
    pub environment: String,
    pub debug: bool,
}

impl AppEnvironment {
    pub fn load(env: &Env<'_>) -> Result<Self, EnvironmentError> {
        Self::validate(
            env.string("APP_NAME", "Kotavel")?,
            env.string("APP_ENV", "production")?,
            env.boolean("APP_DEBUG", false)?,
        )
    }

    /// Loads application identity from the active thread environment scope,
    /// falling back to process variables when no scope is installed.
    pub fn load_current() -> Result<Self, EnvironmentError> {
        Self::validate(
            env_string("APP_NAME", "Kotavel")?,
            env_string("APP_ENV", "production")?,
            env_boolean("APP_DEBUG", false)?,
        )
    }

    fn validate(name: String, environment: String, debug: bool) -> Result<Self, EnvironmentError> {
        let result = Self {
            name,
            environment,
            debug,
        };
        if result.name.is_empty()
            || result.name.len() > 64
            || result.name.bytes().any(|byte| byte < 32 || byte == 127)
        {
            return Err(EnvironmentError::InvalidApplication(
                "APP_NAME requires 1 to 64 bytes without control characters".into(),
            ));
        }
        if !matches!(
            result.environment.as_str(),
            "local" | "development" | "production" | "testing" | "staging"
        ) {
            return Err(EnvironmentError::InvalidApplication(
                "APP_ENV must be local, development, production, testing or staging".into(),
            ));
        }
        Ok(result)
    }
}
