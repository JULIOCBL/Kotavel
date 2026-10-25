use super::{
    CacheOptions, CachedSessionStore, SQLiteSessionStore, SessionMutation, SessionRecord,
    SessionStore, StoreError, StoreOptions, generate_session_id, valid_session_id,
};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CSRF_SESSION_KEY: &str = "_kotavel.csrf_token";

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub lifetime: Duration,
    pub max_lifetime: Option<Duration>,
    pub max_session_bytes: usize,
    pub activity_touch_interval: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            lifetime: Duration::from_secs(120 * 60),
            max_lifetime: Some(Duration::from_secs(24 * 60 * 60)),
            max_session_bytes: 1024 * 1024,
            activity_touch_interval: Duration::from_secs(60),
        }
    }
}

/// Owns the store and applies session lifetime, size, and identity rules.
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    config: SessionConfig,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, config: SessionConfig) -> Result<Self, StoreError> {
        if config.lifetime.is_zero() {
            return Err(StoreError::InvalidOptions(
                "session lifetime must be positive",
            ));
        }
        if config.max_session_bytes == 0 {
            return Err(StoreError::InvalidOptions(
                "maximum session size must be positive",
            ));
        }
        Ok(Self { store, config })
    }

    pub fn sqlite(
        path: impl AsRef<Path>,
        config: SessionConfig,
        store_options: StoreOptions,
        cache_options: CacheOptions,
    ) -> Result<Self, StoreError> {
        let durable = Arc::new(SQLiteSessionStore::open(path, store_options)?);
        let cached = Arc::new(CachedSessionStore::new(durable, cache_options)?);
        Self::new(cached, config)
    }

    /// Resolve an existing opaque ID or create a fresh independent session.
    /// Invalid, missing, inactive, and absolutely expired IDs are never resumed.
    pub fn begin(&self, candidate_id: Option<&str>) -> Result<SessionScope, StoreError> {
        let now = now_seconds();
        let mut loaded = None;
        if let Some(id) = candidate_id.filter(|id| valid_session_id(id)) {
            if let Some(record) = self.store.load_snapshot(id)? {
                let max_expired = self.config.max_lifetime.is_some_and(|maximum| {
                    now >= record.created_at.saturating_add(duration_seconds(maximum))
                });
                if record.is_expired(now) || max_expired {
                    self.store.remove(id)?;
                } else {
                    loaded = Some((*record).clone());
                }
            }
        }

        let record = match loaded {
            Some(record) => record,
            None => SessionRecord::new(
                generate_session_id()?,
                duration_seconds(self.config.lifetime).max(0) as u64,
            ),
        };
        let mut record = record;
        set_expiry(&mut record, now, &self.config);
        let original = record.clone();
        let original_revision = record.revision;
        let new = record.revision == 0;
        Ok(SessionScope {
            store: Arc::clone(&self.store),
            config: self.config.clone(),
            record,
            original,
            original_revision,
            retired_ids: Vec::new(),
            identity_changed: false,
            mutation: SessionMutation::default(),
            dirty: new,
        })
    }

    pub fn collect_expired(&self) -> Result<usize, StoreError> {
        self.store.collect_expired(now_seconds())
    }
}

/// Per-request isolated snapshot and overlay. No lock is held while controller
/// code runs; commits use revisions and rebase only explicit counter increments.
pub struct SessionScope {
    store: Arc<dyn SessionStore>,
    config: SessionConfig,
    record: SessionRecord,
    original: SessionRecord,
    original_revision: u64,
    retired_ids: Vec<String>,
    identity_changed: bool,
    mutation: SessionMutation,
    dirty: bool,
}

impl SessionScope {
    pub fn id(&self) -> &str {
        &self.record.id
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        if key == CSRF_SESSION_KEY {
            return None;
        }
        if let Some(value) = self.mutation.changes.get(key) {
            return value.as_ref();
        }
        self.record.values.get(key)
    }

    /// Whether a non-null application value is present under the key.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some_and(|value| !value.is_null())
    }

    /// Whether the key exists, including when its value is JSON `null`.
    pub fn exists(&self, key: &str) -> bool {
        if key == CSRF_SESSION_KEY {
            return false;
        }
        self.mutation
            .changes
            .get(key)
            .map_or_else(|| self.record.values.contains_key(key), Option::is_some)
    }

    pub fn all(&self) -> serde_json::Map<String, Value> {
        let mut values = self.materialize_values();
        values.remove(CSRF_SESSION_KEY);
        values
    }

    pub fn csrf_token(&mut self) -> Result<String, StoreError> {
        if let Some(token) = self
            .materialize_values()
            .get(CSRF_SESSION_KEY)
            .and_then(Value::as_str)
            .filter(|token| super::valid_session_id(token))
        {
            return Ok(token.to_owned());
        }
        let token = super::generate_session_id()?;
        let mut candidate = self.materialize_values();
        candidate.insert(CSRF_SESSION_KEY.to_owned(), Value::String(token.clone()));
        self.check_size(&candidate)?;
        self.mutation.changes.insert(
            CSRF_SESSION_KEY.to_owned(),
            Some(Value::String(token.clone())),
        );
        self.dirty = true;
        Ok(token)
    }

    pub fn verify_csrf_token(&self, candidate: &str) -> bool {
        if !super::valid_session_id(candidate) {
            return false;
        }
        let values = self.materialize_values();
        let Some(expected) = values.get(CSRF_SESSION_KEY).and_then(Value::as_str) else {
            return false;
        };
        constant_time_eq(expected.as_bytes(), candidate.as_bytes())
    }

    pub fn put(&mut self, key: impl Into<String>, value: Value) -> Result<(), StoreError> {
        let key = key.into();
        if key.is_empty() {
            return Err(StoreError::InvalidOptions("session key cannot be empty"));
        }
        if key == CSRF_SESSION_KEY {
            return Err(StoreError::InvalidOptions(
                "the session CSRF token is reserved by Kotavel",
            ));
        }
        let mut candidate = self.materialize_values();
        candidate.insert(key.clone(), value.clone());
        self.check_size(&candidate)?;
        self.mutation.increments.remove(&key);
        self.mutation.changes.insert(key, Some(value));
        self.dirty = true;
        Ok(())
    }

    pub fn remove(&mut self, key: &str) {
        let _ = self.forget(key);
    }

    pub fn forget(&mut self, key: &str) -> Result<(), StoreError> {
        if key == CSRF_SESSION_KEY {
            return Err(StoreError::InvalidOptions(
                "the session CSRF token is reserved by Kotavel",
            ));
        }
        self.mutation.increments.remove(key);
        if self.exists(key) {
            self.mutation.changes.insert(key.to_owned(), None);
            self.dirty = true;
        }
        Ok(())
    }

    pub fn increment(&mut self, key: impl Into<String>, amount: u64) -> Result<u64, StoreError> {
        let key = key.into();
        if key.is_empty() {
            return Err(StoreError::InvalidOptions("session key cannot be empty"));
        }
        if key == CSRF_SESSION_KEY {
            return Err(StoreError::InvalidOptions(
                "the session CSRF token is reserved by Kotavel",
            ));
        }
        if amount == 0 {
            return match self.get(&key) {
                None => Ok(0),
                Some(value) => as_nonnegative_integer(value).ok_or(StoreError::InvalidOptions(
                    "session counter must be a nonnegative integer",
                )),
            };
        }
        let current = match self.get(&key) {
            None => 0,
            Some(value) => as_nonnegative_integer(value).ok_or(StoreError::InvalidOptions(
                "session counter must be a nonnegative integer",
            ))?,
        };
        let next = current
            .checked_add(amount)
            .ok_or(StoreError::InvalidOptions("session counter overflow"))?;
        let accumulated = self
            .mutation
            .increments
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(amount)
            .ok_or(StoreError::InvalidOptions(
                "session counter increment overflow",
            ))?;
        let mut candidate = self.materialize_values();
        candidate.insert(key.clone(), Value::from(next));
        self.check_size(&candidate)?;
        self.mutation.increments.insert(key.clone(), accumulated);
        self.mutation.changes.insert(key, Some(Value::from(next)));
        self.dirty = true;
        Ok(next)
    }

    pub fn flush(&mut self) {
        self.mutation.changes.clear();
        self.mutation.increments.clear();
        self.mutation.exclusive = true;
        for key in self.record.values.keys() {
            self.mutation.changes.insert(key.clone(), None);
        }
        self.dirty = true;
    }

    /// Retire the current ID while preserving the session's application data.
    pub fn regenerate(&mut self) -> Result<(), StoreError> {
        self.rotate(false)
    }

    /// Retire the current ID and remove all data from the session.
    pub fn invalidate(&mut self) -> Result<(), StoreError> {
        self.rotate(true)
    }

    fn rotate(&mut self, invalidate: bool) -> Result<(), StoreError> {
        self.retired_ids.push(self.record.id.clone());
        let now = now_seconds();
        self.record.id = super::generate_session_id()?;
        self.record.revision = 0;
        self.record.created_at = now;
        self.record.last_activity = now;
        set_expiry(&mut self.record, now, &self.config);

        let mut values = self.materialize_values();
        if invalidate {
            values.clear();
        } else {
            values.remove(CSRF_SESSION_KEY);
        }
        self.record.values = values;
        self.mutation = SessionMutation::default();
        self.identity_changed = true;
        self.dirty = true;
        Ok(())
    }

    pub fn commit(&mut self) -> Result<bool, StoreError> {
        let now = now_seconds();
        if let Some(maximum) = self.config.max_lifetime {
            if now
                >= self
                    .record
                    .created_at
                    .saturating_add(duration_seconds(maximum))
            {
                return Ok(false);
            }
        }
        let touch_due = now.saturating_sub(self.record.last_activity)
            >= duration_seconds(self.config.activity_touch_interval);
        if !self.dirty && !touch_due {
            return Ok(true);
        }

        set_expiry(&mut self.record, now, &self.config);
        if self.identity_changed {
            self.record.values = self.materialize_values();
            self.check_size(&self.record.values)?;
            if !self.store.commit_rotation(
                &mut self.record,
                self.original_revision,
                &self.retired_ids,
            )? {
                return Ok(false);
            }
            self.original = self.record.clone();
            self.original_revision = self.record.revision;
            self.retired_ids.clear();
            self.identity_changed = false;
            self.mutation = SessionMutation::default();
            self.dirty = false;
            return Ok(true);
        }
        if !self.store.commit_mutation(
            &mut self.record,
            &self.original,
            &self.mutation,
            self.config.max_session_bytes,
        )? {
            return Ok(false);
        }
        self.original = self.record.clone();
        self.original_revision = self.record.revision;
        self.mutation = SessionMutation::default();
        self.dirty = false;
        Ok(true)
    }

    fn materialize_values(&self) -> serde_json::Map<String, Value> {
        let mut values = self.record.values.clone();
        for (key, value) in &self.mutation.changes {
            match value {
                Some(value) => {
                    values.insert(key.clone(), value.clone());
                }
                None => {
                    values.remove(key);
                }
            }
        }
        values
    }

    fn check_size(&self, values: &serde_json::Map<String, Value>) -> Result<(), StoreError> {
        let actual = serde_json::to_vec(values)?.len();
        if actual > self.config.max_session_bytes {
            return Err(StoreError::SessionTooLarge {
                actual,
                maximum: self.config.max_session_bytes,
            });
        }
        Ok(())
    }
}

fn as_nonnegative_integer(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn set_expiry(record: &mut SessionRecord, now: i64, config: &SessionConfig) {
    record.last_activity = now;
    let inactivity_expiry = now.saturating_add(duration_seconds(config.lifetime));
    record.expires_at = config
        .max_lifetime
        .map(|maximum| {
            inactivity_expiry.min(record.created_at.saturating_add(duration_seconds(maximum)))
        })
        .unwrap_or(inactivity_expiry);
}

fn duration_seconds(duration: Duration) -> i64 {
    duration.as_secs().min(i64::MAX as u64) as i64
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
    use crate::session::{SQLiteSessionStore, StoreOptions};
    use serde_json::json;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn independent_requests_rebase_shared_counter_increments() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let manager = SessionManager::new(
            Arc::clone(&store) as Arc<dyn SessionStore>,
            SessionConfig::default(),
        )
        .unwrap();
        let mut initial = manager.begin(None).unwrap();
        let id = initial.id().to_owned();
        initial.put("visits", Value::from(0)).unwrap();
        assert!(initial.commit().unwrap());

        let workers = 256;
        let start = Arc::new(Barrier::new(workers));
        let mut requests = Vec::new();
        for _ in 0..workers {
            let manager = SessionManager::new(
                Arc::clone(&store) as Arc<dyn SessionStore>,
                SessionConfig::default(),
            )
            .unwrap();
            let candidate = id.clone();
            let start = Arc::clone(&start);
            requests.push(thread::spawn(move || {
                start.wait();
                let mut scope = manager.begin(Some(&candidate)).unwrap();
                scope.increment("visits", 1).unwrap();
                scope.commit().unwrap()
            }));
        }
        for request in requests {
            assert!(request.join().unwrap());
        }
        let final_scope = manager.begin(Some(&id)).unwrap();
        assert_eq!(final_scope.get("visits"), Some(&json!(workers)));
    }

    #[test]
    fn increment_rejects_existing_non_integer_values() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let manager = SessionManager::new(
            Arc::clone(&store) as Arc<dyn SessionStore>,
            SessionConfig::default(),
        )
        .unwrap();
        let mut scope = manager.begin(None).unwrap();
        scope.put("visits", json!("not-a-number")).unwrap();
        assert!(scope.commit().unwrap());

        let mut scope = manager.begin(Some(scope.id())).unwrap();
        assert!(matches!(
            scope.increment("visits", 1),
            Err(StoreError::InvalidOptions(
                "session counter must be a nonnegative integer"
            ))
        ));
    }

    #[test]
    fn session_lookup_distinguishes_missing_null_and_non_null_values() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let manager = SessionManager::new(
            Arc::clone(&store) as Arc<dyn SessionStore>,
            SessionConfig::default(),
        )
        .unwrap();
        let mut scope = manager.begin(None).unwrap();
        scope.put("nullable", Value::Null).unwrap();
        scope.put("active", json!(true)).unwrap();
        assert!(scope.exists("nullable"));
        assert!(!scope.has("nullable"));
        assert!(scope.has("active"));
        assert!(!scope.exists("missing"));
        scope.forget("nullable").unwrap();
        assert!(!scope.exists("nullable"));
        assert!(matches!(
            scope.forget(CSRF_SESSION_KEY),
            Err(StoreError::InvalidOptions(_))
        ));
    }

    #[test]
    fn regeneration_preserves_values_rotates_csrf_and_invalidation_clears_data() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let manager = SessionManager::new(
            Arc::clone(&store) as Arc<dyn SessionStore>,
            SessionConfig::default(),
        )
        .unwrap();

        let mut initial = manager.begin(None).unwrap();
        let original_id = initial.id().to_owned();
        initial.put("user_id", json!(42)).unwrap();
        let original_csrf = initial.csrf_token().unwrap();
        assert!(initial.commit().unwrap());
        let context = store.create_context().unwrap();
        let grant = store
            .issue_link_grant(&context, Duration::from_secs(60))
            .unwrap();
        assert!(store.redeem_link_grant(&grant, &original_id).unwrap());

        let mut regenerated = manager.begin(Some(&original_id)).unwrap();
        assert!(regenerated.verify_csrf_token(&original_csrf));
        regenerated.regenerate().unwrap();
        let regenerated_id = regenerated.id().to_owned();
        assert_ne!(regenerated_id, original_id);
        assert_eq!(regenerated.get("user_id"), Some(&json!(42)));
        assert!(regenerated.get(CSRF_SESSION_KEY).is_none());
        assert!(!regenerated.verify_csrf_token(&original_csrf));
        let regenerated_csrf = regenerated.csrf_token().unwrap();
        assert_ne!(regenerated_csrf, original_csrf);
        assert!(regenerated.verify_csrf_token(&regenerated_csrf));
        assert!(regenerated.all().get(CSRF_SESSION_KEY).is_none());
        assert!(matches!(
            regenerated.put(CSRF_SESSION_KEY, json!("forged")),
            Err(StoreError::InvalidOptions(_))
        ));
        assert!(regenerated.commit().unwrap());
        assert!(store.load(&original_id).unwrap().is_none());
        assert!(store.context_for_session(&original_id).unwrap().is_none());
        assert!(
            store
                .context_for_session(&regenerated_id)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.load(&regenerated_id).unwrap().unwrap().values["user_id"],
            json!(42)
        );

        let mut invalidated = manager.begin(Some(&regenerated_id)).unwrap();
        invalidated.invalidate().unwrap();
        let invalidated_id = invalidated.id().to_owned();
        assert_ne!(invalidated_id, regenerated_id);
        assert!(invalidated.all().is_empty());
        assert!(invalidated.commit().unwrap());
        assert!(store.load(&regenerated_id).unwrap().is_none());
        assert!(
            store
                .load(&invalidated_id)
                .unwrap()
                .unwrap()
                .values
                .is_empty()
        );
    }

    #[test]
    fn concurrent_regeneration_cannot_restore_a_retired_session_id() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let manager = Arc::new(
            SessionManager::new(
                Arc::clone(&store) as Arc<dyn SessionStore>,
                SessionConfig::default(),
            )
            .unwrap(),
        );
        let mut initial = manager.begin(None).unwrap();
        let retired_id = initial.id().to_owned();
        initial.put("user_id", json!(42)).unwrap();
        assert!(initial.commit().unwrap());

        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let retired_id = retired_id.clone();
            workers.push(thread::spawn(move || {
                let manager =
                    SessionManager::new(store as Arc<dyn SessionStore>, SessionConfig::default())
                        .unwrap();
                let mut scope = manager.begin(Some(&retired_id)).unwrap();
                scope.regenerate().unwrap();
                let replacement_id = scope.id().to_owned();
                barrier.wait();
                (replacement_id, scope.commit().unwrap())
            }));
        }
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            results.iter().filter(|(_, committed)| *committed).count(),
            1
        );
        assert!(store.load(&retired_id).unwrap().is_none());
        let committed_id = results
            .iter()
            .find(|(_, committed)| *committed)
            .map(|(id, _)| id)
            .unwrap();
        assert!(store.load(committed_id).unwrap().is_some());
        let rejected_id = results
            .iter()
            .find(|(_, committed)| !committed)
            .map(|(id, _)| id)
            .unwrap();
        assert!(store.load(rejected_id).unwrap().is_none());
    }
}
