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
fn int_infers_integer_types_and_int_or_uses_a_fallback() {
    let source = MapSource::default();
    source.0.lock().unwrap().extend([
        ("POSITIVE_INT".into(), "42".into()),
        ("NEGATIVE_INT".into(), "-17".into()),
        ("INVALID_INT".into(), "4.2".into()),
        ("OVERFLOW_INT".into(), "9223372036854775808".into()),
        ("BOOLEAN_INT".into(), "true".into()),
    ]);
    let env = Env::new(&source);

    let positive: u16 = env.int("POSITIVE_INT").unwrap();
    let negative: i64 = env.int("NEGATIVE_INT").unwrap();
    assert_eq!(positive, 42);
    assert_eq!(negative, -17);
    assert_eq!(env.int_or("MISSING_INT", -9_i64), -9);
    for key in ["INVALID_INT", "OVERFLOW_INT", "BOOLEAN_INT"] {
        assert_eq!(env.int_or(key, 0_i64), 0);
        assert!(matches!(
            env.int::<i64>(key),
            Err(EnvironmentError::TypeMismatch {
                expected: "an integer",
                ..
            })
        ));
    }
    assert_eq!(env.int::<i64>("NEGATIVE_INT").unwrap(), -17);
    assert!(matches!(
        env.int::<i64>("MISSING_INT"),
        Err(EnvironmentError::MissingRequired(key)) if key == "MISSING_INT"
    ));
}

#[test]
fn contains_distinguishes_present_values_from_missing_keys() {
    let source = MapSource::default();
    source
        .0
        .lock()
        .unwrap()
        .insert("DEFINED_VALUE".into(), "".into());
    let env = Env::new(&source);

    assert!(env.contains("DEFINED_VALUE").unwrap());
    assert!(!env.contains("MISSING_VALUE").unwrap());
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
    outer.0.lock().unwrap().extend([
        ("KOTAVEL_SCOPED_VALUE".into(), "outer".into()),
        ("KOTAVEL_SCOPED_INT".into(), "27".into()),
    ]);
    let inner = Arc::new(MapSource::default());
    inner
        .0
        .lock()
        .unwrap()
        .insert("KOTAVEL_SCOPED_VALUE".into(), "inner".into());

    {
        let _outer_scope = EnvScope::new(outer);
        assert_eq!(env_string("KOTAVEL_SCOPED_VALUE", "missing"), "outer");
        let scoped_int: i32 = env_int("KOTAVEL_SCOPED_INT").unwrap();
        assert_eq!(scoped_int, 27);
        assert_eq!(env_int_or("KOTAVEL_SCOPED_INT", 5_i32), 27);
        {
            let _inner_scope = EnvScope::new(inner);
            assert_eq!(env_string("KOTAVEL_SCOPED_VALUE", "missing"), "inner");
        }
        assert_eq!(env_string("KOTAVEL_SCOPED_VALUE", "missing"), "outer");
    }
    assert_eq!(env_string("KOTAVEL_SCOPED_VALUE", "missing"), "missing");
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
    assert_eq!(env_string("KOTAVEL_THREAD_SCOPE", "missing"), "visible");
    assert_eq!(
        thread::spawn(|| env_string("KOTAVEL_THREAD_SCOPE", "missing"))
            .join()
            .unwrap(),
        "missing"
    );
    drop(scope);
}
