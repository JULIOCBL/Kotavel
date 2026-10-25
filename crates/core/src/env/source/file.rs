use super::{EnvironmentSource, ProcessEnvironment};
use crate::env::{Env, EnvironmentError, value::validate_key};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MAX_VALUE_BYTES: usize = 4096;
const MAX_FILE_BYTES: usize = 64 * 1024;

/// Reads project `.env` values while preserving process-environment precedence.
pub struct FileEnvironment {
    process: ProcessEnvironment,
    values: BTreeMap<String, String>,
}

impl FileEnvironment {
    pub fn load(
        project_root: impl AsRef<Path>,
        environment: &str,
    ) -> Result<Self, EnvironmentError> {
        if !matches!(environment, "production" | "local" | "development") {
            return Err(EnvironmentError::InvalidApplication(
                "Selected environment must be local, development or production".into(),
            ));
        }
        let path = project_root.as_ref().join(if environment == "production" {
            ".env".to_owned()
        } else {
            format!(".env.{environment}")
        });
        let bytes = std::fs::read(&path).map_err(|_| EnvironmentError::FileOpen(path.clone()))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(invalid_file(&path, 0, "Environment file exceeds 64 KiB"));
        }
        if bytes.contains(&0) {
            return Err(invalid_file(
                &path,
                0,
                "Environment file contains a null byte",
            ));
        }
        let contents = std::str::from_utf8(&bytes)
            .map_err(|_| invalid_file(&path, 0, "Environment file must be UTF-8"))?;
        let mut values = BTreeMap::new();
        for (index, raw_line) in contents.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.trim_matches([' ', '\t', '\r']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((raw_key, raw_value)) = line.split_once('=') else {
                return Err(invalid_file(
                    &path,
                    line_number,
                    "Environment file requires KEY=VALUE",
                ));
            };
            let key = raw_key.trim_matches([' ', '\t', '\r']);
            validate_key(key)
                .map_err(|_| invalid_file(&path, line_number, "Invalid environment file key"))?;
            if !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || key.as_bytes()[0].is_ascii_digit()
            {
                return Err(invalid_file(
                    &path,
                    line_number,
                    "Invalid environment file key",
                ));
            }
            let value = parse_file_value(raw_value, &path, line_number)?;
            if value.len() > MAX_VALUE_BYTES {
                return Err(invalid_file(
                    &path,
                    line_number,
                    "Environment value exceeds 4096 bytes",
                ));
            }
            if values.insert(key.to_owned(), value).is_some() {
                return Err(invalid_file(
                    &path,
                    line_number,
                    &format!("Duplicate environment file key: {key}"),
                ));
            }
        }
        values
            .entry("APP_ENV".into())
            .or_insert_with(|| environment.to_owned());
        let result = Self {
            process: ProcessEnvironment,
            values,
        };
        if Env::new(&result).string("APP_ENV", environment)? != environment {
            return Err(invalid_file(
                &path,
                0,
                "APP_ENV does not match selected environment",
            ));
        }
        Ok(result)
    }
}

impl EnvironmentSource for FileEnvironment {
    fn read(&self, key: &str) -> Result<Option<String>, EnvironmentError> {
        if let Some(value) = self.process.read(key)? {
            return Ok(Some(value));
        }
        validate_key(key)?;
        Ok(self.values.get(key).cloned())
    }
}

fn parse_file_value(raw: &str, path: &Path, line: usize) -> Result<String, EnvironmentError> {
    let value = raw.trim_matches([' ', '\t', '\r']);
    if let Some(quote @ ('\'' | '"')) = value.chars().next() {
        let end = value[1..]
            .find(quote)
            .map(|index| index + 1)
            .ok_or_else(|| invalid_file(path, line, "Unclosed environment quote"))?;
        let tail = value[end + 1..].trim_matches([' ', '\t', '\r']);
        if !tail.is_empty() && !tail.starts_with('#') {
            return Err(invalid_file(path, line, "Invalid environment quote suffix"));
        }
        Ok(value[..=end].to_owned())
    } else {
        let bytes = value.as_bytes();
        for index in 0..bytes.len() {
            if bytes[index] == b'#'
                && (index == 0 || bytes[index - 1] == b' ' || bytes[index - 1] == b'\t')
            {
                return Ok(value[..index]
                    .trim_end_matches([' ', '\t', '\r'])
                    .to_owned());
            }
        }
        Ok(value.to_owned())
    }
}

fn invalid_file(path: &Path, line: usize, reason: &str) -> EnvironmentError {
    EnvironmentError::InvalidFile {
        path: PathBuf::from(path),
        line,
        reason: reason.to_owned(),
    }
}
