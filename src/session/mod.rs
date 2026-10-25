//! Session contracts and the initial SQLite WAL driver.

mod cache;
mod context;
mod manager;
mod settings;
mod sqlite;
mod transport;

use serde::{Deserialize, Serialize};
use serde_json::Map;
use std::time::{SystemTime, UNIX_EPOCH};

pub use cache::{CacheOptions, CacheStatistics, CachedSessionStore};
pub use context::{
    LinkedSessionContext, SessionContext, SessionContextId, SessionContextResolver,
    SessionContextStore, SessionInstanceId, SessionLinkGrant,
};
pub use manager::{SessionConfig, SessionManager, SessionScope};
pub use settings::{SessionDriver, SessionSettings, SessionSettingsError};
pub use sqlite::{SQLiteSessionStore, StoreError, StoreOptions, WriterStatistics};
pub use transport::{CookieTransportOptions, SameSite, SessionIdentifierTransport};

/// Application data persisted for one authenticated client.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionRecord {
    pub id: String,
    pub created_at: i64,
    pub last_activity: i64,
    pub expires_at: i64,
    pub revision: u64,
    pub values: Map<String, serde_json::Value>,
}

impl SessionRecord {
    pub fn new(id: String, lifetime_seconds: u64) -> Self {
        let now = now_seconds();
        Self {
            id,
            created_at: now,
            last_activity: now,
            expires_at: now.saturating_add(lifetime_seconds.min(i64::MAX as u64) as i64),
            revision: 0,
            values: Map::new(),
        }
    }

    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at <= now
    }
}

/// Durable storage boundary. A successful commit is an atomic revision CAS.
pub trait SessionStore: Send + Sync {
    fn load(&self, id: &str) -> Result<Option<SessionRecord>, StoreError>;
    fn load_snapshot(&self, id: &str) -> Result<Option<std::sync::Arc<SessionRecord>>, StoreError> {
        self.load(id).map(|record| record.map(std::sync::Arc::new))
    }
    fn commit(&self, record: &mut SessionRecord) -> Result<bool, StoreError>;
    /// Replace the authenticated identity and retire every prior ID. Durable
    /// stores should override this to make the transition atomic.
    fn commit_rotation(
        &self,
        replacement: &mut SessionRecord,
        expected_previous_revision: u64,
        retired_ids: &[String],
    ) -> Result<bool, StoreError> {
        if let Some(previous_id) = retired_ids.first() {
            let previous = self.load(previous_id)?;
            if (expected_previous_revision == 0 && previous.is_some())
                || previous
                    .as_ref()
                    .is_none_or(|record| record.revision != expected_previous_revision)
                    && expected_previous_revision != 0
            {
                return Ok(false);
            }
        }
        if !self.commit(replacement)? {
            return Ok(false);
        }
        for id in retired_ids {
            self.remove(id)?;
        }
        Ok(true)
    }
    fn commit_mutation(
        &self,
        record: &mut SessionRecord,
        original: &SessionRecord,
        mutation: &SessionMutation,
        maximum_bytes: usize,
    ) -> Result<bool, StoreError> {
        let is_new = record.revision == 0;
        let mut current = if is_new {
            if self.load(&record.id)?.is_some() {
                return Ok(false);
            }
            original.clone()
        } else {
            match self.load(&record.id)? {
                Some(current) => current,
                None => return Ok(false),
            }
        };

        for attempt in 0..4096 {
            if is_new && current.revision != 0 {
                return Ok(false);
            }
            if mutation.exclusive && current.revision != original.revision {
                return Ok(false);
            }
            for key in mutation.changes.keys() {
                if mutation.increments.contains_key(key) {
                    continue;
                }
                if current.values.get(key) != original.values.get(key) {
                    return Ok(false);
                }
            }

            for (key, amount) in &mutation.increments {
                let current_value = match current.values.get(key) {
                    None => 0,
                    Some(value) => value.as_u64().ok_or(StoreError::InvalidOptions(
                        "session counter must be a nonnegative integer",
                    ))?,
                };
                let next_value = current_value
                    .checked_add(*amount)
                    .ok_or(StoreError::InvalidOptions("session counter overflow"))?;
                current
                    .values
                    .insert(key.clone(), serde_json::Value::from(next_value));
            }
            for (key, value) in &mutation.changes {
                if mutation.increments.contains_key(key) {
                    continue;
                }
                match value {
                    Some(value) => {
                        current.values.insert(key.clone(), value.clone());
                    }
                    None => {
                        current.values.remove(key);
                    }
                }
            }
            let encoded = serde_json::to_vec(&current.values)?;
            if encoded.len() > maximum_bytes {
                return Err(StoreError::SessionTooLarge {
                    actual: encoded.len(),
                    maximum: maximum_bytes,
                });
            }
            current.last_activity = record.last_activity;
            current.expires_at = record.expires_at;
            if self.commit(&mut current)? {
                *record = current;
                return Ok(true);
            }
            if is_new {
                return Ok(false);
            }
            // A contention burst can leave many request snapshots on the same
            // revision. Yield so the winning writer and reader-pool waiters can
            // make progress instead of letting retries spin on the CPU.
            if attempt % 4 == 3 {
                std::thread::yield_now();
            }
            if attempt >= 32 && attempt % 16 == 15 {
                std::thread::sleep(std::time::Duration::from_micros(50));
            }
            current = match self.load(&record.id)? {
                Some(current) => current,
                None => return Ok(false),
            };
        }
        Ok(false)
    }
    fn remove(&self, id: &str) -> Result<(), StoreError>;
    fn collect_expired(&self, now: i64) -> Result<usize, StoreError>;
}

#[derive(Clone, Debug, Default)]
pub struct SessionMutation {
    pub changes: std::collections::HashMap<String, Option<serde_json::Value>>,
    pub increments: std::collections::HashMap<String, u64>,
    pub exclusive: bool,
}

/// Generate a 256-bit session identifier using the operating system RNG.
pub fn generate_session_id() -> Result<String, StoreError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(StoreError::Random)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(output)
}

pub fn valid_session_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique_and_have_256_bits_of_entropy() {
        let first = generate_session_id().unwrap();
        let second = generate_session_id().unwrap();
        assert!(valid_session_id(&first));
        assert!(valid_session_id(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn expiry_is_checked_at_the_server_timestamp() {
        let record = SessionRecord {
            id: "id".into(),
            created_at: 10,
            last_activity: 10,
            expires_at: 20,
            revision: 1,
            values: Map::new(),
        };
        assert!(!record.is_expired(19));
        assert!(record.is_expired(20));
    }
}
