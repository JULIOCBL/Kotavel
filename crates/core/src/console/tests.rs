use super::*;
use crate::config::{Config, ConfigValue};
use std::{ffi::OsString, path::PathBuf, time::Duration};

#[test]
fn common_arguments_are_removed_and_unknown_arguments_are_preserved() {
    let arguments = parse_console_arguments([
        OsString::from("--help"),
        OsString::from("--development"),
        OsString::from("--env=local"),
        OsString::from("--project-root=./app"),
        OsString::from("serve"),
        OsString::from("--port=8080"),
    ])
    .unwrap();
    assert!(arguments.help && arguments.debug && arguments.environment_explicit);
    assert_eq!(arguments.environment, "local");
    assert_eq!(arguments.root, PathBuf::from("./app"));
    assert_eq!(arguments.arguments, ["serve", "--port=8080"]);
}

#[test]
fn invalid_project_root_is_rejected_without_consuming_unknown_options() {
    assert_eq!(
        parse_console_arguments([OsString::from("--project-root=")]),
        Err(ConsoleArgumentError::EmptyProjectRoot)
    );
}

#[test]
fn console_settings_apply_channels_paths_and_metrics_ranges() {
    let mut config = Config::new();
    config
        .set_many([
            ("logging.channel", ConfigValue::String("stderr".into())),
            ("logging.level", ConfigValue::String("warning".into())),
            ("logging.directory", ConfigValue::String("var/logs".into())),
            ("logging.backups", ConfigValue::from(8)),
            ("metrics.enabled", ConfigValue::Bool(true)),
            ("metrics.interval_ms", ConfigValue::from(250)),
        ])
        .unwrap();
    let settings = console_settings("/project", &config).unwrap();
    assert!(settings.logs.enabled && !settings.logs.file && settings.logs.console);
    assert_eq!(
        settings.logs.min_level,
        crate::diagnostics::LogLevel::Warning
    );
    assert_eq!(settings.logs.directory, PathBuf::from("/project/var/logs"));
    assert_eq!(settings.logs.backups, 8);
    assert!(settings.metrics.enabled);
    assert_eq!(settings.metrics.interval, Duration::from_millis(250));
}

#[test]
fn console_settings_reject_invalid_values_and_keep_missing_defaults() {
    let settings = console_settings("/project", &Config::new()).unwrap();
    assert!(settings.logs.enabled && settings.logs.file && !settings.logs.console);
    assert!(!settings.metrics.enabled);

    let mut config = Config::new();
    config
        .set("logging.backups", ConfigValue::from(-1))
        .unwrap();
    assert_eq!(
        console_settings("/project", &config).unwrap_err().key,
        "logging.backups"
    );
}
