use super::{EnvironmentError, EnvironmentSource};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_KEY_BYTES: usize = 256;
const MAX_VALUE_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentValue {
    Null,
    Bool(bool),
    String(String),
}

pub struct Env<'a> {
    source: &'a dyn EnvironmentSource,
}

impl<'a> Env<'a> {
    pub fn new(source: &'a dyn EnvironmentSource) -> Self {
        Self { source }
    }

    pub fn get(
        &self,
        key: &str,
        fallback: EnvironmentValue,
    ) -> Result<EnvironmentValue, EnvironmentError> {
        validate_key(key)?;
        match self.source.read(key)? {
            Some(value) => decode(value).map_err(|_| EnvironmentError::ValueTooLong),
            None => Ok(fallback),
        }
    }
    pub fn require(&self, key: &str) -> Result<EnvironmentValue, EnvironmentError> {
        match self.get(key, EnvironmentValue::Null)? {
            EnvironmentValue::Null => Err(EnvironmentError::MissingRequired(key.to_owned())),
            value => Ok(value),
        }
    }
    pub fn string(&self, key: &str, fallback: &str) -> Result<String, EnvironmentError> {
        match self.get(key, EnvironmentValue::String(fallback.to_owned()))? {
            EnvironmentValue::String(value) => Ok(value),
            _ => Err(EnvironmentError::TypeMismatch {
                key: key.to_owned(),
                expected: "a string",
            }),
        }
    }
    pub fn boolean(&self, key: &str, fallback: bool) -> Result<bool, EnvironmentError> {
        match self.get(key, EnvironmentValue::Bool(fallback))? {
            EnvironmentValue::Bool(value) => Ok(value),
            _ => Err(EnvironmentError::TypeMismatch {
                key: key.to_owned(),
                expected: "true or false",
            }),
        }
    }
}

thread_local! {
    /// Active environment readers are scoped to the current thread and own
    /// their adapters, so a forgotten scope cannot leave a dangling borrow.
    static ACTIVE_ENVIRONMENTS: RefCell<Vec<(u64, Arc<dyn EnvironmentSource>)>> = const { RefCell::new(Vec::new()) };
}

static NEXT_ENVIRONMENT_SCOPE_ID: AtomicU64 = AtomicU64::new(1);

/// Temporarily makes an environment source available to the `env_*` helpers
/// on the current thread. Nested scopes restore the previous active source.
///
/// The source is owned by the scope, and the scope is deliberately `!Send` so
/// it cannot be dropped on a different thread from the one it configured.
pub struct EnvScope {
    id: u64,
    _not_send: PhantomData<Rc<()>>,
}

impl EnvScope {
    pub fn new(source: Arc<dyn EnvironmentSource>) -> Self {
        let id = NEXT_ENVIRONMENT_SCOPE_ID.fetch_add(1, Ordering::Relaxed);
        ACTIVE_ENVIRONMENTS.with(|active| active.borrow_mut().push((id, source)));
        Self {
            id,
            _not_send: PhantomData,
        }
    }
}

impl Drop for EnvScope {
    fn drop(&mut self) {
        ACTIVE_ENVIRONMENTS.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(index) = active.iter().position(|(id, _)| *id == self.id) {
                active.remove(index);
            }
        });
    }
}

/// Reads a decoded environment value from the active thread scope, or from
/// the process environment when no scope has been installed.
pub fn env(key: &str) -> Result<EnvironmentValue, EnvironmentError> {
    let source = ACTIVE_ENVIRONMENTS
        .with(|active| active.borrow().last().map(|(_, source)| Arc::clone(source)));
    match source {
        Some(source) => Env::new(source.as_ref()).get(key, EnvironmentValue::Null),
        None => Env::new(&super::ProcessEnvironment).get(key, EnvironmentValue::Null),
    }
}

pub fn env_string(key: &str, fallback: &str) -> Result<String, EnvironmentError> {
    let source = active_source();
    Env::new(source.as_deref().unwrap_or(&super::ProcessEnvironment)).string(key, fallback)
}

pub fn env_boolean(key: &str, fallback: bool) -> Result<bool, EnvironmentError> {
    let source = active_source();
    Env::new(source.as_deref().unwrap_or(&super::ProcessEnvironment)).boolean(key, fallback)
}

fn active_source() -> Option<Arc<dyn EnvironmentSource>> {
    ACTIVE_ENVIRONMENTS.with(|active| active.borrow().last().map(|(_, source)| Arc::clone(source)))
}

pub(super) fn validate_key(key: &str) -> Result<(), EnvironmentError> {
    if key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || key.bytes().any(|byte| byte == 0 || byte == b'=')
    {
        return Err(EnvironmentError::InvalidKey);
    }
    Ok(())
}

fn decode(mut value: String) -> Result<EnvironmentValue, ()> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(());
    }
    let lower = value.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "(true)") {
        return Ok(EnvironmentValue::Bool(true));
    }
    if matches!(lower.as_str(), "false" | "(false)") {
        return Ok(EnvironmentValue::Bool(false));
    }
    if matches!(lower.as_str(), "null" | "(null)") {
        return Ok(EnvironmentValue::Null);
    }
    if matches!(lower.as_str(), "empty" | "(empty)") {
        return Ok(EnvironmentValue::String(String::new()));
    }
    if value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
    {
        value.remove(0);
        value.pop();
    }
    Ok(EnvironmentValue::String(value))
}

#[cfg(test)]
pub(super) fn test_decode(value: String) -> Result<EnvironmentValue, ()> {
    decode(value)
}
