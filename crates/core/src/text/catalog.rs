use super::MessageError;
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

static CATALOG: OnceLock<Result<BTreeMap<String, String>, MessageError>> = OnceLock::new();

pub(super) fn get(key: &str) -> Result<&'static str, MessageError> {
    let catalog = CATALOG.get_or_init(build).as_ref().map_err(Clone::clone)?;
    catalog
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| MessageError::UnknownKey(key.to_owned()))
}

fn build() -> Result<BTreeMap<String, String>, MessageError> {
    let mut catalog = BTreeMap::new();
    for (namespace, source) in [
        ("env", include_str!("../../resources/messages/env.json")),
        (
            "runtime",
            include_str!("../../resources/messages/runtime.json"),
        ),
    ] {
        let root: Value = serde_json::from_str(source)
            .map_err(|error| MessageError::InvalidCatalog(error.to_string()))?;
        flatten(&mut catalog, namespace.to_owned(), &root)?;
    }
    Ok(catalog)
}

fn flatten(
    catalog: &mut BTreeMap<String, String>,
    prefix: String,
    value: &Value,
) -> Result<(), MessageError> {
    match value {
        Value::Object(entries) if !entries.is_empty() => {
            for (segment, child) in entries {
                if !valid_name(segment) {
                    return Err(MessageError::InvalidCatalog(format!(
                        "invalid catalog segment: {segment}"
                    )));
                }
                let key = if prefix.is_empty() {
                    segment.clone()
                } else {
                    format!("{prefix}.{segment}")
                };
                flatten(catalog, key, child)?;
            }
        }
        Value::String(text) if !prefix.is_empty() => {
            if catalog.insert(prefix.clone(), text.clone()).is_some() {
                return Err(MessageError::InvalidCatalog(format!(
                    "duplicate message key: {prefix}"
                )));
            }
        }
        _ => {
            return Err(MessageError::InvalidCatalog(format!(
                "expected a nested object or message string at {prefix}"
            )));
        }
    }
    Ok(())
}

pub(super) fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.split('.').all(valid_name)
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(name_start) && bytes.all(name_part)
}

pub(super) fn name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(super) fn name_part(byte: u8) -> bool {
    name_start(byte) || byte.is_ascii_digit()
}
