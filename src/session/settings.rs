use super::{
    CacheOptions, CachedSessionStore, CookieTransportOptions, SQLiteSessionStore, SameSite,
    SessionConfig, SessionIdentifierTransport, SessionManager, StoreError, StoreOptions,
};
use crate::{
    config::{Config, ConfigValue},
    env::{Env, EnvironmentError, EnvironmentValue},
};
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionDriver {
    Sqlite,
}

/// Validated settings for the manager, storage driver, cache, and HTTP
/// identifier transport.
#[derive(Clone, Debug)]
pub struct SessionSettings {
    pub driver: SessionDriver,
    pub database: std::path::PathBuf,
    pub manager: SessionConfig,
    pub store: StoreOptions,
    pub cache: CacheOptions,
    pub transport: SessionIdentifierTransport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSettingsError {
    pub key: String,
    pub reason: String,
}

impl SessionSettingsError {
    fn invalid(key: &str, reason: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for SessionSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Invalid session configuration for {}: {}",
            self.key, self.reason
        )
    }
}

impl std::error::Error for SessionSettingsError {}

impl SessionSettings {
    /// Resolve session settings from the application's dotted-key config.
    pub fn from_config(configuration: &Config) -> Result<Self, SessionSettingsError> {
        let driver = read_string(configuration, "session.driver", "sqlite")?;
        if driver != "sqlite" {
            return Err(SessionSettingsError::invalid(
                "session.driver",
                "only the SQLite driver is available",
            ));
        }
        let database = std::path::PathBuf::from(read_string(
            configuration,
            "session.database",
            "storage/framework/sessions.sqlite3",
        )?);
        let lifetime_minutes = read_unsigned(configuration, "session.lifetime", 120)?;
        if lifetime_minutes == 0 {
            return Err(SessionSettingsError::invalid(
                "session.lifetime",
                "must be greater than zero",
            ));
        }
        let maximum_minutes = read_unsigned(configuration, "session.max_lifetime", 1440)?;
        let cache_max_sessions = read_usize(configuration, "session.cache_max_sessions", 2048)?;
        let cache_max_bytes = read_usize(configuration, "session.cache_max_bytes", 33_554_432)?;
        let cache_shards = read_usize(configuration, "session.cache_shards", 64)?;
        if cache_shards == 0 || cache_shards > 1024 {
            return Err(SessionSettingsError::invalid(
                "session.cache_shards",
                "must be between 1 and 1024",
            ));
        }
        let cache_ttl_ms = read_unsigned(configuration, "session.cache_ttl_ms", 1000)?;
        if cache_ttl_ms == 0 {
            return Err(SessionSettingsError::invalid(
                "session.cache_ttl_ms",
                "must be greater than zero",
            ));
        }
        let max_session_bytes = read_usize(configuration, "session.max_session_bytes", 1_048_576)?;
        if max_session_bytes == 0 {
            return Err(SessionSettingsError::invalid(
                "session.max_session_bytes",
                "must be greater than zero",
            ));
        }
        let read_connections = read_usize(configuration, "session.sqlite_read_connections", 4)?;
        if read_connections == 0 {
            return Err(SessionSettingsError::invalid(
                "session.sqlite_read_connections",
                "must be greater than zero",
            ));
        }
        let write_queue_capacity =
            read_usize(configuration, "session.sqlite_write_queue_capacity", 1024)?;
        if write_queue_capacity == 0 {
            return Err(SessionSettingsError::invalid(
                "session.sqlite_write_queue_capacity",
                "must be greater than zero",
            ));
        }
        let write_batch_window_us =
            read_unsigned(configuration, "session.sqlite_write_batch_window_us", 0)?;
        if write_batch_window_us > 10_000 {
            return Err(SessionSettingsError::invalid(
                "session.sqlite_write_batch_window_us",
                "must be between 0 and 10000",
            ));
        }

        let lifetime = minutes_duration(lifetime_minutes, "session.lifetime")?;
        let max_lifetime = (maximum_minutes > 0)
            .then(|| minutes_duration(maximum_minutes, "session.max_lifetime"))
            .transpose()?;
        let manager = SessionConfig {
            lifetime,
            max_lifetime,
            max_session_bytes,
            activity_touch_interval: Duration::from_secs(60).min(lifetime / 2),
        };
        let store = StoreOptions {
            read_connections,
            write_queue_capacity,
            write_batch_window: Duration::from_micros(write_batch_window_us),
            ..StoreOptions::default()
        };
        let cache = CacheOptions {
            max_sessions: cache_max_sessions,
            max_bytes: cache_max_bytes,
            shards: cache_shards,
            ttl: Duration::from_millis(cache_ttl_ms),
        };

        if read_boolean(configuration, "session.encrypt", false)? {
            return Err(SessionSettingsError::invalid(
                "session.encrypt",
                "encryption is not implemented",
            ));
        }
        let transport_name = read_string(configuration, "session.transport", "cookie")?;
        let transport = match transport_name.as_str() {
            "cookie" => {
                let same_site_name = read_string(configuration, "session.same_site", "lax")?;
                let same_site = match same_site_name.as_str() {
                    "lax" => SameSite::Lax,
                    "strict" => SameSite::Strict,
                    "none" => SameSite::None,
                    _ => {
                        return Err(SessionSettingsError::invalid(
                            "session.same_site",
                            "must be lax, strict or none",
                        ));
                    }
                };
                let domain = match configuration.get("session.domain") {
                    None | Some(ConfigValue::Null) => None,
                    Some(ConfigValue::String(value)) if !value.is_empty() => Some(value.clone()),
                    Some(ConfigValue::String(_)) => None,
                    Some(_) => {
                        return Err(SessionSettingsError::invalid(
                            "session.domain",
                            "must be a string or null",
                        ));
                    }
                };
                let expire_on_close =
                    read_boolean(configuration, "session.expire_on_close", false)?;
                let options = CookieTransportOptions {
                    name: read_string(configuration, "session.cookie", "kotavel_session")?,
                    path: read_string(configuration, "session.path", "/")?,
                    domain,
                    max_age_seconds: (!expire_on_close).then_some(lifetime.as_secs()),
                    http_only: read_boolean(configuration, "session.http_only", true)?,
                    secure: read_boolean(configuration, "session.secure", false)?,
                    same_site,
                };
                SessionIdentifierTransport::cookie(options)
                    .map_err(|reason| SessionSettingsError::invalid("session.cookie", reason))?
            }
            "header" => SessionIdentifierTransport::header(read_string(
                configuration,
                "session.header",
                "X-Kotavel-Session",
            )?)
            .map_err(|reason| SessionSettingsError::invalid("session.header", reason))?,
            _ => {
                return Err(SessionSettingsError::invalid(
                    "session.transport",
                    "must be cookie or header",
                ));
            }
        };

        Ok(Self {
            driver: SessionDriver::Sqlite,
            database,
            manager,
            store,
            cache,
            transport,
        })
    }

    /// Build a validated configuration from `.env` or process environment
    /// names used by Kotavel's session settings.
    pub fn load(env: &Env<'_>) -> Result<Self, SessionSettingsError> {
        let mut configuration = Config::new();
        set_env_string(
            &mut configuration,
            env,
            "SESSION_DRIVER",
            "session.driver",
            "sqlite",
        )?;
        set_env_string(
            &mut configuration,
            env,
            "SESSION_DATABASE",
            "session.database",
            "storage/framework/sessions.sqlite3",
        )?;
        for (environment, key, fallback) in [
            ("SESSION_LIFETIME", "session.lifetime", 120),
            ("SESSION_MAX_LIFETIME", "session.max_lifetime", 1440),
            (
                "SESSION_CACHE_MAX_SESSIONS",
                "session.cache_max_sessions",
                2048,
            ),
            (
                "SESSION_CACHE_MAX_BYTES",
                "session.cache_max_bytes",
                33_554_432,
            ),
            ("SESSION_CACHE_SHARDS", "session.cache_shards", 64),
            ("SESSION_CACHE_TTL_MS", "session.cache_ttl_ms", 1000),
            ("SESSION_MAX_BYTES", "session.max_session_bytes", 1_048_576),
            (
                "SESSION_READ_CONNECTIONS",
                "session.sqlite_read_connections",
                4,
            ),
            (
                "SESSION_WRITE_QUEUE_CAPACITY",
                "session.sqlite_write_queue_capacity",
                1024,
            ),
            (
                "SESSION_WRITE_BATCH_WINDOW_US",
                "session.sqlite_write_batch_window_us",
                0,
            ),
        ] {
            let raw = env
                .string(environment, &fallback.to_string())
                .map_err(|error| SessionSettingsError::invalid(environment, error.to_string()))?;
            let value = raw.parse::<u64>().map_err(|_| {
                SessionSettingsError::invalid(environment, "must be a nonnegative integer")
            })?;
            set_value(&mut configuration, key, ConfigValue::from(value))?;
        }
        for (environment, key, fallback) in [
            ("SESSION_SECURE_COOKIE", "session.secure", false),
            ("SESSION_HTTP_ONLY", "session.http_only", true),
            ("SESSION_EXPIRE_ON_CLOSE", "session.expire_on_close", false),
            ("SESSION_ENCRYPT", "session.encrypt", false),
        ] {
            let value = env
                .boolean(environment, fallback)
                .map_err(|error| SessionSettingsError::invalid(environment, error.to_string()))?;
            set_value(&mut configuration, key, ConfigValue::from(value))?;
        }
        for (environment, key, fallback) in [
            ("SESSION_COOKIE", "session.cookie", "kotavel_session"),
            ("SESSION_TRANSPORT", "session.transport", "cookie"),
            ("SESSION_HEADER", "session.header", "X-Kotavel-Session"),
            ("SESSION_PATH", "session.path", "/"),
            ("SESSION_SAME_SITE", "session.same_site", "lax"),
        ] {
            set_env_string(&mut configuration, env, environment, key, fallback)?;
        }
        let domain = env
            .get("SESSION_DOMAIN", EnvironmentValue::Null)
            .map_err(|error| SessionSettingsError::invalid("SESSION_DOMAIN", error.to_string()))?;
        match domain {
            EnvironmentValue::Null => {}
            EnvironmentValue::String(value) => {
                set_value(
                    &mut configuration,
                    "session.domain",
                    ConfigValue::String(value),
                )?;
            }
            EnvironmentValue::Bool(_) => {
                return Err(SessionSettingsError::invalid(
                    "SESSION_DOMAIN",
                    "must be a string",
                ));
            }
        }
        Self::from_config(&configuration)
    }

    /// Open the durable store with a database path resolved from the
    /// consuming application's root. Consumers that need session context and
    /// request sessions can share this handle and its SQLite writer.
    pub fn create_store(
        &self,
        project_root: impl AsRef<Path>,
    ) -> Result<std::sync::Arc<SQLiteSessionStore>, StoreError> {
        Ok(std::sync::Arc::new(SQLiteSessionStore::open(
            self.resolved_database_path(project_root),
            self.store.clone(),
        )?))
    }

    /// Build a manager over an already-open durable store.
    pub fn create_manager_with_store(
        &self,
        store: std::sync::Arc<SQLiteSessionStore>,
    ) -> Result<SessionManager, StoreError> {
        let cached = std::sync::Arc::new(CachedSessionStore::new(store, self.cache.clone())?);
        SessionManager::new(cached, self.manager.clone())
    }

    /// Open the configured store with a database path resolved from the
    /// consuming application's root.
    pub fn create_manager(
        &self,
        project_root: impl AsRef<Path>,
    ) -> Result<SessionManager, StoreError> {
        self.create_manager_with_store(self.create_store(project_root)?)
    }

    pub fn resolved_database_path(&self, project_root: impl AsRef<Path>) -> std::path::PathBuf {
        if self.database.is_absolute() {
            self.database.clone()
        } else {
            project_root.as_ref().join(&self.database)
        }
    }
}

fn read_string(
    configuration: &Config,
    key: &str,
    fallback: &str,
) -> Result<String, SessionSettingsError> {
    if !configuration.contains(key) {
        return Ok(fallback.to_owned());
    }
    configuration
        .get_typed(key)
        .map_err(|_| SessionSettingsError::invalid(key, "must be a string"))
}

fn read_boolean(
    configuration: &Config,
    key: &str,
    fallback: bool,
) -> Result<bool, SessionSettingsError> {
    if !configuration.contains(key) {
        return Ok(fallback);
    }
    configuration
        .get_typed(key)
        .map_err(|_| SessionSettingsError::invalid(key, "must be true or false"))
}

fn read_unsigned(
    configuration: &Config,
    key: &str,
    fallback: u64,
) -> Result<u64, SessionSettingsError> {
    if !configuration.contains(key) {
        return Ok(fallback);
    }
    configuration
        .get_typed(key)
        .map_err(|_| SessionSettingsError::invalid(key, "must be a nonnegative integer"))
}

fn read_usize(
    configuration: &Config,
    key: &str,
    fallback: usize,
) -> Result<usize, SessionSettingsError> {
    usize::try_from(read_unsigned(configuration, key, fallback as u64)?)
        .map_err(|_| SessionSettingsError::invalid(key, "value is too large"))
}

fn minutes_duration(minutes: u64, key: &str) -> Result<Duration, SessionSettingsError> {
    minutes
        .checked_mul(60)
        .map(Duration::from_secs)
        .ok_or_else(|| SessionSettingsError::invalid(key, "duration is too large"))
}

fn set_value(
    configuration: &mut Config,
    key: &str,
    value: ConfigValue,
) -> Result<(), SessionSettingsError> {
    configuration
        .set(key, value)
        .map_err(|error| SessionSettingsError::invalid(key, error.to_string()))
}

fn set_env_string(
    configuration: &mut Config,
    env: &Env<'_>,
    environment: &str,
    key: &str,
    fallback: &str,
) -> Result<(), SessionSettingsError> {
    let value = env
        .string(environment, fallback)
        .map_err(|error: EnvironmentError| {
            SessionSettingsError::invalid(environment, error.to_string())
        })?;
    set_value(configuration, key, ConfigValue::String(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::EnvironmentSource;
    use std::{collections::HashMap, path::PathBuf};

    struct MapEnvironment(HashMap<String, String>);

    impl EnvironmentSource for MapEnvironment {
        fn read(&self, key: &str) -> Result<Option<String>, EnvironmentError> {
            Ok(self.0.get(key).cloned())
        }
    }

    #[test]
    fn defaults_match_the_framework_session_contract() {
        let settings = SessionSettings::from_config(&Config::new()).unwrap();
        assert_eq!(settings.driver, SessionDriver::Sqlite);
        assert_eq!(
            settings.database,
            PathBuf::from("storage/framework/sessions.sqlite3")
        );
        assert_eq!(settings.manager.lifetime, Duration::from_secs(7200));
        assert_eq!(
            settings.manager.max_lifetime,
            Some(Duration::from_secs(86_400))
        );
        assert_eq!(settings.cache.max_sessions, 2048);
        assert_eq!(settings.cache.max_bytes, 33_554_432);
        assert_eq!(settings.cache.shards, 64);
        assert_eq!(settings.store.read_connections, 4);
        assert_eq!(settings.store.write_queue_capacity, 1024);
        assert!(matches!(
            settings.transport,
            SessionIdentifierTransport::Cookie(_)
        ));
    }

    #[test]
    fn config_resolves_storage_cache_lifetime_and_cookie_options() {
        let mut configuration = Config::new();
        configuration
            .set_many([
                (
                    "session.database",
                    ConfigValue::String("data/sessions.sqlite".into()),
                ),
                ("session.lifetime", ConfigValue::from(30)),
                ("session.max_lifetime", ConfigValue::from(0)),
                ("session.cache_max_sessions", ConfigValue::from(0)),
                ("session.cache_shards", ConfigValue::from(8)),
                (
                    "session.sqlite_write_batch_window_us",
                    ConfigValue::from(250),
                ),
                ("session.secure", ConfigValue::from(true)),
                ("session.same_site", ConfigValue::String("none".into())),
                ("session.expire_on_close", ConfigValue::from(true)),
            ])
            .unwrap();

        let settings = SessionSettings::from_config(&configuration).unwrap();
        assert_eq!(settings.database, PathBuf::from("data/sessions.sqlite"));
        assert_eq!(settings.manager.lifetime, Duration::from_secs(1800));
        assert_eq!(settings.manager.max_lifetime, None);
        assert_eq!(
            settings.manager.activity_touch_interval,
            Duration::from_secs(60)
        );
        assert_eq!(settings.cache.max_sessions, 0);
        assert_eq!(settings.cache.shards, 8);
        assert_eq!(
            settings.store.write_batch_window,
            Duration::from_micros(250)
        );
        let SessionIdentifierTransport::Cookie(cookie) = settings.transport else {
            panic!("cookie transport expected");
        };
        assert!(cookie.secure);
        assert_eq!(cookie.same_site, SameSite::None);
        assert_eq!(cookie.max_age_seconds, None);
    }

    #[test]
    fn config_rejects_unsupported_drivers_encryption_and_invalid_bounds() {
        for (key, value) in [
            ("session.driver", ConfigValue::String("memory".into())),
            ("session.encrypt", ConfigValue::from(true)),
            ("session.cache_shards", ConfigValue::from(1025)),
            (
                "session.sqlite_write_batch_window_us",
                ConfigValue::from(10_001),
            ),
            ("session.lifetime", ConfigValue::from(0)),
        ] {
            let mut configuration = Config::new();
            configuration.set(key, value).unwrap();
            let error = SessionSettings::from_config(&configuration).unwrap_err();
            assert!(!error.key.is_empty());
        }
    }

    #[test]
    fn environment_loading_maps_session_variables_through_validated_config() {
        let source = MapEnvironment(HashMap::from([
            ("SESSION_TRANSPORT".into(), "header".into()),
            ("SESSION_HEADER".into(), "X-Local-Session".into()),
            ("SESSION_LIFETIME".into(), "45".into()),
            ("SESSION_WRITE_BATCH_WINDOW_US".into(), "500".into()),
        ]));
        let settings = SessionSettings::load(&Env::new(&source)).unwrap();
        assert_eq!(settings.manager.lifetime, Duration::from_secs(2700));
        assert_eq!(
            settings.store.write_batch_window,
            Duration::from_micros(500)
        );
        assert_eq!(
            settings.transport,
            SessionIdentifierTransport::header("X-Local-Session").unwrap()
        );
    }

    #[test]
    fn relative_database_path_is_resolved_from_the_application_root() {
        let directory = tempfile::tempdir().unwrap();
        let settings = SessionSettings::from_config(&Config::new()).unwrap();
        assert_eq!(
            settings.resolved_database_path(directory.path()),
            directory.path().join("storage/framework/sessions.sqlite3")
        );
    }
}
