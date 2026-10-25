use super::{Config, ConfigError, ConfigValue};
use serde::de::DeserializeOwned;
use std::cell::Cell;

thread_local! {
    static ACTIVE_CONFIG: Cell<*mut Config> = const { Cell::new(std::ptr::null_mut()) };
}

/// Run a closure with this configuration active on the current thread.
/// Nested scopes restore the previous configuration, including during unwinding.
pub fn with_config_scope<R>(config: &mut Config, action: impl FnOnce() -> R) -> R {
    struct Restore(*mut Config);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_CONFIG.with(|slot| slot.set(self.0));
        }
    }
    let previous = ACTIVE_CONFIG.with(|slot| slot.replace(config as *mut Config));
    let _restore = Restore(previous);
    action()
}

fn with_active<R>(action: impl FnOnce(&mut Config) -> R) -> Result<R, ConfigError> {
    ACTIVE_CONFIG.with(|slot| {
        let pointer = slot.get();
        if pointer.is_null() {
            return Err(ConfigError::NoActiveConfiguration);
        }
        // The pointer is installed only for the dynamic extent of `with_config_scope`.
        // Access is thread-local and scoped; safe APIs never return a borrowed reference.
        Ok(unsafe { action(&mut *pointer) })
    })
}

/// Clone a scalar or reconstruct and own a group. Missing entries map to null.
pub fn config(key: &str) -> Result<ConfigValueView, ConfigError> {
    with_active(|config| {
        let value = config
            .get(key)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| config.group(key).map(|v| v.unwrap_or(ConfigValue::Null)));
        value.map(ConfigValueView)
    })?
}

pub fn config_get<T: DeserializeOwned>(key: &str) -> Result<T, ConfigError> {
    with_active(|config| config.get_typed(key))?
}

pub fn config_get_many<'a>(
    keys: impl IntoIterator<Item = &'a str>,
) -> Result<super::ConfigMap<String, ConfigValue>, ConfigError> {
    let keys: Vec<&'a str> = keys.into_iter().collect();
    with_active(|config| config.get_many(keys))?
}

pub fn config_set(key: &str, value: ConfigValue) -> Result<(), ConfigError> {
    with_active(|config| config.set(key, value))?
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigValueView(pub ConfigValue);

impl ConfigValueView {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn value(&self) -> &ConfigValue {
        &self.0
    }
}

impl std::fmt::Display for ConfigValueView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            ConfigValue::Null => f.write_str("null"),
            ConfigValue::Bool(value) => write!(f, "{value}"),
            ConfigValue::Number(value) => write!(f, "{value}"),
            ConfigValue::String(value) => f.write_str(value),
            ConfigValue::Array(_) => f.write_str("[configuration list]"),
            ConfigValue::Object(_) => f.write_str("[configuration group]"),
        }
    }
}
