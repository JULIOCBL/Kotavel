use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    thread,
};

#[derive(Default)]
struct MapSource(Mutex<BTreeMap<String, String>>);
impl EnvironmentSource for MapSource {
    fn read(&self, key: &str) -> Result<Option<String>, EnvironmentError> {
        value::validate_key(key)?;
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
}

#[test]
fn typed_values_and_application_settings_are_validated() {
    let source = MapSource::default();
    source.0.lock().unwrap().extend([
        ("APP_NAME".into(), "Kotavel Lab".into()),
        ("APP_ENV".into(), "testing".into()),
        ("APP_DEBUG".into(), "TrUe".into()),
    ]);
    let env = Env::new(&source);
    assert_eq!(
        AppEnvironment::load(&env).unwrap(),
        AppEnvironment {
            name: "Kotavel Lab".into(),
            environment: "testing".into(),
            debug: true,
        }
    );
}

#[test]
fn file_environment_parses_values_and_rejects_duplicate_keys() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env.development"),
        "APP_ENV=development\nTOKEN=from-file\n",
    )
    .unwrap();
    let source = FileEnvironment::load(dir.path(), "development").unwrap();
    assert_eq!(source.read("TOKEN").unwrap().as_deref(), Some("from-file"));
    std::fs::write(dir.path().join(".env"), "A=1\nA=2\n").unwrap();
    assert!(matches!(
        FileEnvironment::load(dir.path(), "production"),
        Err(EnvironmentError::InvalidFile { line: 2, .. })
    ));
}

#[test]
fn decoding_preserves_strings_and_recognizes_null_boolean_and_empty() {
    assert_eq!(
        value::test_decode("'(empty)'".into()).unwrap(),
        EnvironmentValue::String("(empty)".into())
    );
    assert_eq!(
        value::test_decode("(empty)".into()).unwrap(),
        EnvironmentValue::String(String::new())
    );
    assert_eq!(
        value::test_decode("NULL".into()).unwrap(),
        EnvironmentValue::Null
    );
}

#[test]
fn scoped_helpers_use_nested_sources_and_restore_the_previous_source() {
    let outer = Arc::new(MapSource::default());
    outer
        .0
        .lock()
        .unwrap()
        .insert("KOTAVEL_SCOPED_VALUE".into(), "outer".into());
    let inner = Arc::new(MapSource::default());
    inner
        .0
        .lock()
        .unwrap()
        .insert("KOTAVEL_SCOPED_VALUE".into(), "inner".into());

    {
        let _outer_scope = EnvScope::new(outer);
        assert_eq!(
            env_string("KOTAVEL_SCOPED_VALUE", "missing").unwrap(),
            "outer"
        );
        {
            let _inner_scope = EnvScope::new(inner);
            assert_eq!(
                env_string("KOTAVEL_SCOPED_VALUE", "missing").unwrap(),
                "inner"
            );
        }
        assert_eq!(
            env_string("KOTAVEL_SCOPED_VALUE", "missing").unwrap(),
            "outer"
        );
    }
    assert_eq!(
        env_string("KOTAVEL_SCOPED_VALUE", "missing").unwrap(),
        "missing"
    );
}

#[test]
fn application_environment_can_load_from_the_active_scope() {
    let source = Arc::new(MapSource::default());
    source.0.lock().unwrap().extend([
        ("APP_NAME".into(), "Scoped Kotavel".into()),
        ("APP_ENV".into(), "testing".into()),
        ("APP_DEBUG".into(), "true".into()),
    ]);
    let _scope = EnvScope::new(source);
    assert_eq!(
        AppEnvironment::load_current().unwrap(),
        AppEnvironment {
            name: "Scoped Kotavel".into(),
            environment: "testing".into(),
            debug: true,
        }
    );
}

#[test]
fn scoped_environment_is_thread_local() {
    let source = Arc::new(MapSource::default());
    source
        .0
        .lock()
        .unwrap()
        .insert("KOTAVEL_THREAD_SCOPE".into(), "visible".into());
    let scope = EnvScope::new(source);
    assert_eq!(
        env_string("KOTAVEL_THREAD_SCOPE", "missing").unwrap(),
        "visible"
    );
    assert_eq!(
        thread::spawn(|| env_string("KOTAVEL_THREAD_SCOPE", "missing").unwrap())
            .join()
            .unwrap(),
        "missing"
    );
    drop(scope);
}
