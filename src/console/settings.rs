use crate::{
    config::{Config, ConfigValue},
    diagnostics::{Environment, LogLevel, LogOptions},
    foundation::ErrorHandlingOptions,
    metrics::MetricsOptions,
};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug)]
pub struct ConsoleSettings {
    pub logs: LogOptions,
    pub errors: ErrorHandlingOptions,
    pub metrics: MetricsOptions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleSettingsError {
    pub key: String,
    pub reason: &'static str,
}

impl ConsoleSettingsError {
    fn new(key: &str, reason: &'static str) -> Self {
        Self {
            key: key.to_owned(),
            reason,
        }
    }
}

impl std::fmt::Display for ConsoleSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Invalid configuration value for {}: {}",
            self.key, self.reason
        )
    }
}

impl std::error::Error for ConsoleSettingsError {}

/// Builds default logging, error and metrics options from the application config.
pub fn console_settings(
    root: impl AsRef<Path>,
    configuration: &Config,
) -> Result<ConsoleSettings, ConsoleSettingsError> {
    let root = root.as_ref();
    let channel = read_string(configuration, "logging.channel", "single")?;
    if !matches!(channel.as_str(), "single" | "stderr" | "null") {
        return Err(ConsoleSettingsError::new(
            "logging.channel",
            "use single, stderr or null",
        ));
    }
    let level_name = read_string(configuration, "logging.level", "info")?;
    let min_level = LogLevel::ALL
        .into_iter()
        .find(|level| level.name() == level_name)
        .ok_or_else(|| {
            ConsoleSettingsError::new(
                "logging.level",
                "use trace, debug, info, notice, warning, error, critical, alert or emergency",
            )
        })?;
    let module = read_string(configuration, "logging.module", "app")?;
    if module.is_empty() || module.len() > 64 {
        return Err(ConsoleSettingsError::new(
            "logging.module",
            "requires between 1 and 64 bytes",
        ));
    }
    let directory = read_string(configuration, "logging.directory", "storage/logs")?;
    if directory.is_empty() {
        return Err(ConsoleSettingsError::new(
            "logging.directory",
            "path cannot be empty",
        ));
    }
    let max_bytes = read_integer(configuration, "logging.max_bytes", 512 * 1024 * 1024)?;
    if max_bytes < 16_384 {
        return Err(ConsoleSettingsError::new(
            "logging.max_bytes",
            "minimum is 16384 bytes",
        ));
    }
    let backups = read_integer(configuration, "logging.backups", 3)?;
    if !(0..=100).contains(&backups) {
        return Err(ConsoleSettingsError::new(
            "logging.backups",
            "use a value between 0 and 100",
        ));
    }

    let logs = LogOptions {
        enabled: channel != "null",
        file: channel == "single",
        console: channel == "stderr",
        min_level,
        module,
        directory: root.join(directory),
        max_bytes: usize::try_from(max_bytes)
            .map_err(|_| ConsoleSettingsError::new("logging.max_bytes", "value is too large"))?,
        backups: backups as usize,
        ..LogOptions::default()
    };

    let interval_ms = read_integer(configuration, "metrics.interval_ms", 1000)?;
    if !(100..=60_000).contains(&interval_ms) {
        return Err(ConsoleSettingsError::new(
            "metrics.interval_ms",
            "use a value between 100 and 60000 milliseconds",
        ));
    }
    let metrics = MetricsOptions {
        enabled: read_boolean(configuration, "metrics.enabled", false)?,
        console: read_boolean(configuration, "metrics.console", true)?,
        startup: read_boolean(configuration, "metrics.startup", true)?,
        stream: read_boolean(configuration, "metrics.stream", false)?,
        interval: Duration::from_millis(interval_ms as u64),
    };
    Ok(ConsoleSettings {
        logs,
        errors: ErrorHandlingOptions {
            environment: Environment::Production,
            ..ErrorHandlingOptions::default()
        },
        metrics,
    })
}

fn read_string(config: &Config, key: &str, default: &str) -> Result<String, ConsoleSettingsError> {
    match config.get(key) {
        None => Ok(default.to_owned()),
        Some(ConfigValue::String(value)) => Ok(value.clone()),
        Some(_) => Err(ConsoleSettingsError::new(key, "value must be a string")),
    }
}

fn read_integer(config: &Config, key: &str, default: i64) -> Result<i64, ConsoleSettingsError> {
    match config.get(key) {
        None => Ok(default),
        Some(ConfigValue::Number(value)) => value
            .as_i64()
            .ok_or_else(|| ConsoleSettingsError::new(key, "value must be an integer")),
        Some(_) => Err(ConsoleSettingsError::new(key, "value must be an integer")),
    }
}

fn read_boolean(config: &Config, key: &str, default: bool) -> Result<bool, ConsoleSettingsError> {
    match config.get(key) {
        None => Ok(default),
        Some(ConfigValue::Bool(value)) => Ok(*value),
        Some(_) => Err(ConsoleSettingsError::new(key, "value must be a boolean")),
    }
}
