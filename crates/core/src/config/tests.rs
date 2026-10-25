use super::*;
use serde_json::{Map, json};

#[test]
fn maps_replace_groups_and_preserve_empty_groups() {
    let mut config = Config::new();
    config.set("app.old", json!(true)).unwrap();
    config.set("app", json!({"nested":{"port":8080}})).unwrap();
    assert_eq!(config.get("app.old"), None);
    assert_eq!(
        config.group("app").unwrap(),
        Some(json!({"nested":{"port":8080}}))
    );
    config.set("empty", json!({})).unwrap();
    assert_eq!(config.group("empty").unwrap(), Some(json!({})));
}

#[test]
fn batches_are_transactional_and_repeated_keys_apply_in_order() {
    let mut config = Config::new();
    config.set("stable", json!(7)).unwrap();
    let result = config.set_many([("stable", json!(8)), ("", json!(9))]);
    assert_eq!(result, Err(ConfigError::EmptyKey));
    assert_eq!(config.get("stable"), Some(&json!(7)));
    config
        .set_many([("stable", json!(8)), ("stable", json!(9))])
        .unwrap();
    assert_eq!(config.get("stable"), Some(&json!(9)));
}

#[test]
fn group_reconstruction_observes_direct_value_and_prefix_boundaries() {
    let mut config = Config::new();
    config.set("app.api.port", json!(8080)).unwrap();
    config.set("application.name", json!("other")).unwrap();
    assert_eq!(
        config.group("app").unwrap(),
        Some(json!({"api":{"port":8080}}))
    );
    assert_eq!(config.group("applicationx").unwrap(), None);
    config.set("app", json!(false)).unwrap();
    assert_eq!(config.group("app").unwrap(), None);
}

#[test]
fn limits_and_structural_collisions_reject_without_partial_updates() {
    let mut config = Config::new();
    let too_long_leaf = "x".repeat(1021);
    let mut overlong_group = Map::new();
    overlong_group.insert(too_long_leaf, json!(1));
    assert_eq!(
        config.merge("app", &overlong_group),
        Err(ConfigError::KeyTooLong)
    );
    // Scalar keys retain the explicit flat-key contract; size limits apply to
    // keys created by flattening a nested map.
    let scalar_key = "x".repeat(1025);
    assert!(config.set(&scalar_key, json!(1)).is_ok());
    config = Config::new();
    let collision = json!({"a": 1, "a.b": 2});
    assert!(config.set("group", collision).is_err());
    assert!(config.iter().next().is_none());

    let mut depth32 = json!({"leaf": 0});
    for i in (0..32).rev() {
        depth32 = json!({format!("k{i}"): depth32});
    }
    assert!(config.set("deep", depth32).is_ok());
    let mut depth33 = json!({"leaf": 0});
    for i in (0..33).rev() {
        depth33 = json!({format!("k{i}"): depth33});
    }
    assert_eq!(config.set("too_deep", depth33), Err(ConfigError::TooDeep));
}

#[test]
fn merge_accepts_exactly_ten_thousand_entries() {
    let mut config = Config::new();
    let values: Map<String, ConfigValue> = (0..10_000)
        .map(|i| (format!("key_{i}"), json!(i)))
        .collect();
    config.merge("large", &values).unwrap();
    assert_eq!(config.get("large.key_9999"), Some(&json!(9999)));
    let mut oversized = values;
    oversized.insert("key_10000".into(), json!(10000));
    assert_eq!(
        config.merge("oversized", &oversized),
        Err(ConfigError::TooManyNodes)
    );
    assert!(!config.contains("oversized.key_0"));
}

#[test]
fn merge_is_atomic_and_requires_a_single_lowercase_registry_group() {
    let mut config = Config::new();
    config.set("app.name", json!("Kotavel")).unwrap();
    let mut values = Map::new();
    values.insert("name".into(), json!("collision"));
    assert!(config.merge("app", &values).is_err());
    assert_eq!(config.get("app.name"), Some(&json!("Kotavel")));
    assert_eq!(
        config.merge("bad.group", &Map::new()),
        Err(ConfigError::InvalidGroup("bad.group".into()))
    );
}

#[test]
fn config_scope_nests_restores_and_does_not_cross_threads() {
    let mut outer = Config::new();
    outer.set("which", json!("outer")).unwrap();
    with_config_scope(&mut outer, || {
        assert_eq!(config_get::<String>("which").unwrap(), "outer");
        let mut inner = Config::new();
        inner.set("which", json!("inner")).unwrap();
        with_config_scope(&mut inner, || {
            assert_eq!(config_get::<String>("which").unwrap(), "inner")
        });
        assert_eq!(config_get::<String>("which").unwrap(), "outer");
        assert_eq!(
            std::thread::spawn(|| config_get::<String>("which"))
                .join()
                .unwrap(),
            Err(ConfigError::NoActiveConfiguration)
        );
    });
}

#[test]
fn helper_views_redact_nested_content() {
    let mut settings = Config::new();
    settings
        .set("credentials", json!({"password":"secret"}))
        .unwrap();
    with_config_scope(&mut settings, || {
        let view = config("credentials").unwrap();
        assert_eq!(view.to_string(), "[configuration group]");
        assert!(view.value().is_object());
        assert_eq!(config("missing").unwrap().to_string(), "null");
    });
}

#[test]
fn config_file_registry_validates_groups_and_loads_in_declared_order() {
    fn load_app() -> ConfigMap<String, ConfigValue> {
        Map::from_iter([("name".to_owned(), json!("Kotavel"))])
    }
    let files = [ConfigFile {
        group: "app",
        load: Some(load_app),
    }];
    let loaded = load_config_files(&files).unwrap();
    assert_eq!(loaded.get("app.name"), Some(&json!("Kotavel")));

    let missing_loader = [ConfigFile {
        group: "app",
        load: None,
    }];
    assert_eq!(
        load_config_files(&missing_loader).unwrap_err(),
        ConfigError::MissingLoader
    );
    let invalid_group = [ConfigFile {
        group: "Bad-Group",
        load: Some(load_app),
    }];
    assert_eq!(
        load_config_files(&invalid_group).unwrap_err(),
        ConfigError::InvalidGroup("Bad-Group".into())
    );
}
