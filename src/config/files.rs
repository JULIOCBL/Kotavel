use super::{Config, ConfigError, ConfigMap};

/// Ordered configuration loader. `group` is a lowercase identifier, not a path.
#[derive(Clone, Copy)]
pub struct ConfigFile<'a> {
    pub group: &'a str,
    pub load: Option<fn() -> ConfigMap<String, super::ConfigValue>>,
}

pub fn load_config_files(files: &[ConfigFile<'_>]) -> Result<Config, ConfigError> {
    let mut result = Config::new();
    for file in files {
        if !valid_group(file.group) {
            return Err(ConfigError::InvalidGroup(file.group.to_owned()));
        }
        let load = file.load.ok_or(ConfigError::MissingLoader)?;
        result.merge(file.group, &load())?;
    }
    Ok(result)
}

fn valid_group(group: &str) -> bool {
    let mut bytes = group.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
