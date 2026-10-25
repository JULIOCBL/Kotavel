use super::{ConfigError, ConfigMap, ConfigValue};
use std::collections::BTreeMap;

const MAX_KEY_BYTES: usize = 1024;
const MAX_MAP_DEPTH: usize = 32;
const MAX_MAP_NODES: usize = 10_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    values: BTreeMap<String, ConfigValue>,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a scalar/list/null value or replace a whole dotted-key group.
    pub fn set(&mut self, key: &str, value: ConfigValue) -> Result<(), ConfigError> {
        validate_key(key)?;
        if let ConfigValue::Object(object) = value {
            let mut pending = BTreeMap::new();
            let mut nodes = 0;
            flatten_object(String::new(), object, 0, &mut nodes, &mut pending)?;
            let prefix = format!("{key}.");
            let mut staged = self.values.clone();
            staged.retain(|existing, _| existing != key && !existing.starts_with(&prefix));
            if pending.is_empty() {
                staged.insert(key.to_owned(), ConfigValue::Object(ConfigMap::new()));
            } else {
                for (leaf, value) in pending {
                    let full = format!("{prefix}{leaf}");
                    validate_full_key(&full)?;
                    if staged.contains_key(&full) {
                        return Err(ConfigError::ConflictingStructure(full));
                    }
                    staged.insert(full, value);
                }
            }
            self.values = staged;
        } else {
            self.values.insert(key.to_owned(), value);
        }
        Ok(())
    }

    /// Apply a batch atomically; repeated keys are applied in order.
    pub fn set_many<'a>(
        &mut self,
        updates: impl IntoIterator<Item = (&'a str, ConfigValue)>,
    ) -> Result<(), ConfigError> {
        let mut staged = self.clone();
        for (key, value) in updates {
            staged.set(key, value)?;
        }
        self.values = staged.values;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn value(&self, key: &str) -> Result<&ConfigValue, ConfigError> {
        self.get(key)
            .ok_or_else(|| ConfigError::Missing(key.to_owned()))
    }
    pub fn get_typed<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        serde_json::from_value(self.value(key)?.clone())
            .map_err(|_| ConfigError::TypeMismatch(key.to_owned()))
    }

    /// Return a direct map value first; otherwise reconstruct dotted descendants.
    pub fn group(&self, prefix: &str) -> Result<Option<ConfigValue>, ConfigError> {
        validate_key(prefix)?;
        if let Some(direct) = self.values.get(prefix) {
            return Ok(direct.is_object().then(|| direct.clone()));
        }
        let start = format!("{prefix}.");
        let mut root = ConfigMap::new();
        let mut found = false;
        for (key, value) in self.values.range(start.clone()..) {
            if !key.starts_with(&start) {
                break;
            }
            found = true;
            insert_nested(&mut root, &key[start.len()..], value.clone())?;
        }
        Ok(found.then_some(ConfigValue::Object(root)))
    }

    /// Missing requested keys are returned with a null value.
    pub fn get_many<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<ConfigMap<String, ConfigValue>, ConfigError> {
        let mut result = ConfigMap::new();
        for key in keys {
            let value = match self.values.get(key) {
                Some(value) => value.clone(),
                None => self.group(key)?.unwrap_or(ConfigValue::Null),
            };
            result.insert(key.to_owned(), value);
        }
        Ok(result)
    }

    /// Merge a single named group, rejecting duplicate or structurally colliding keys.
    pub fn merge(
        &mut self,
        group: &str,
        values: &ConfigMap<String, ConfigValue>,
    ) -> Result<(), ConfigError> {
        validate_key(group)?;
        if group.contains('.') {
            return Err(ConfigError::InvalidGroup(group.to_owned()));
        }
        let prefix = format!("{group}.");
        let mut staged = self.clone();
        if values.is_empty() {
            if staged
                .values
                .keys()
                .any(|k| k == group || k.starts_with(&prefix))
            {
                return Err(ConfigError::DuplicateGroup(group.to_owned()));
            }
            staged
                .values
                .insert(group.to_owned(), ConfigValue::Object(ConfigMap::new()));
        }
        let mut flattened = BTreeMap::new();
        let mut nodes = 0;
        flatten_object(String::new(), values.clone(), 0, &mut nodes, &mut flattened)?;
        for (key, value) in flattened {
            let full = format!("{prefix}{key}");
            validate_full_key(&full)?;
            if path_collides(&staged.values, &full) {
                return Err(ConfigError::GroupCollision(full));
            }
            staged.values.insert(full, value);
        }
        self.values = staged.values;
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ConfigValue)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value))
    }
}

pub(super) fn validate_key(key: &str) -> Result<(), ConfigError> {
    if key.is_empty() {
        return Err(ConfigError::EmptyKey);
    }
    if key.bytes().any(|byte| byte < 32 || byte == 127) {
        return Err(ConfigError::ControlCharacter);
    }
    Ok(())
}

fn validate_full_key(key: &str) -> Result<(), ConfigError> {
    validate_key(key)?;
    if key.len() > MAX_KEY_BYTES {
        return Err(ConfigError::KeyTooLong);
    }
    Ok(())
}

fn flatten_object(
    prefix: String,
    object: ConfigMap<String, ConfigValue>,
    depth: usize,
    nodes: &mut usize,
    output: &mut BTreeMap<String, ConfigValue>,
) -> Result<(), ConfigError> {
    if depth > MAX_MAP_DEPTH {
        return Err(ConfigError::TooDeep);
    }
    for (key, value) in object {
        validate_map_key(&key)?;
        *nodes += 1;
        if *nodes > MAX_MAP_NODES {
            return Err(ConfigError::TooManyNodes);
        }
        let full = if prefix.is_empty() {
            key
        } else {
            format!("{prefix}.{key}")
        };
        validate_key(&full)?;
        if let ConfigValue::Object(nested) = value {
            if nested.is_empty() {
                insert_flattened(output, full, ConfigValue::Object(ConfigMap::new()))?;
            } else {
                flatten_object(full, nested, depth + 1, nodes, output)?;
            }
        } else {
            insert_flattened(output, full, value)?;
        }
    }
    Ok(())
}

fn validate_map_key(key: &str) -> Result<(), ConfigError> {
    validate_key(key)?;
    if key.starts_with('.') || key.ends_with('.') || key.contains("..") {
        return Err(ConfigError::ConflictingStructure(key.to_owned()));
    }
    Ok(())
}

fn insert_flattened(
    output: &mut BTreeMap<String, ConfigValue>,
    key: String,
    value: ConfigValue,
) -> Result<(), ConfigError> {
    let mut ancestor = String::new();
    for part in key.split('.').take(key.matches('.').count()) {
        if !ancestor.is_empty() {
            ancestor.push('.');
        }
        ancestor.push_str(part);
        if output.contains_key(&ancestor) {
            return Err(ConfigError::ConflictingStructure(key));
        }
    }
    let prefix = format!("{key}.");
    if output.contains_key(&key)
        || output
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(other, _)| other.starts_with(&prefix))
    {
        return Err(ConfigError::ConflictingStructure(key));
    }
    output.insert(key, value);
    Ok(())
}

fn path_collides(values: &BTreeMap<String, ConfigValue>, key: &str) -> bool {
    if values.contains_key(key) {
        return true;
    }
    let mut ancestor = String::new();
    for part in key.split('.').take(key.matches('.').count()) {
        if !ancestor.is_empty() {
            ancestor.push('.');
        }
        ancestor.push_str(part);
        if values.contains_key(&ancestor) {
            return true;
        }
    }
    let prefix = format!("{key}.");
    values
        .range(prefix.clone()..)
        .next()
        .is_some_and(|(existing, _)| existing.starts_with(&prefix))
}

fn insert_nested(
    root: &mut ConfigMap<String, ConfigValue>,
    key: &str,
    value: ConfigValue,
) -> Result<(), ConfigError> {
    let mut parts = key.split('.').peekable();
    let mut current = root;
    while let Some(part) = parts.next() {
        if part.is_empty() {
            return Err(ConfigError::ConflictingStructure(key.to_owned()));
        }
        if parts.peek().is_none() {
            if current.contains_key(part) {
                return Err(ConfigError::ConflictingStructure(key.to_owned()));
            }
            current.insert(part.to_owned(), value);
            break;
        }
        let entry = current
            .entry(part.to_owned())
            .or_insert_with(|| ConfigValue::Object(ConfigMap::new()));
        let ConfigValue::Object(child) = entry else {
            return Err(ConfigError::ConflictingStructure(key.to_owned()));
        };
        current = child;
    }
    Ok(())
}
