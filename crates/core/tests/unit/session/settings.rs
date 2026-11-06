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
