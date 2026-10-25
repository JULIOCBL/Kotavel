use super::{SessionMutation, SessionRecord, SessionStore, StoreError};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::time::{Duration, Instant};

struct Entry {
    record: Arc<SessionRecord>,
    cached_at: Instant,
    /// Access order is tracked atomically so cache hits retain a shared read lock.
    last_access: AtomicU64,
    bytes: usize,
}

struct Shard {
    entries: RwLock<HashMap<String, Entry>>,
    generation: AtomicU64,
    /// Eviction ordering is shard-local to avoid a global hot-path counter.
    access_sequence: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct CacheOptions {
    pub max_sessions: usize,
    pub max_bytes: usize,
    pub ttl: Duration,
    pub shards: usize,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            max_sessions: 2048,
            max_bytes: 32 * 1024 * 1024,
            ttl: Duration::from_secs(1),
            shards: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStatistics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub resident_sessions: usize,
    pub resident_bytes: usize,
    /// Number of shard-lock acquisitions that had to wait after a try-lock failed.
    pub lock_contended: u64,
    /// Total time spent waiting after shard-lock contention was detected.
    pub lock_wait: Duration,
    /// Longest wait for a shard lock after contention was detected.
    pub max_lock_wait: Duration,
}

/// Sharded, read-through cache with independent shard locks, per-shard
/// occupancy limits, and atomically enforced global budgets. The durable store
/// remains the revision authority.
pub struct CachedSessionStore {
    backing: Arc<dyn SessionStore>,
    shards: Vec<Shard>,
    options: CacheOptions,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    resident_sessions: AtomicUsize,
    resident_bytes: AtomicUsize,
    lock_contended: AtomicU64,
    lock_wait_ns: AtomicU64,
    max_lock_wait_ns: AtomicU64,
}

impl CachedSessionStore {
    pub fn new(backing: Arc<dyn SessionStore>, options: CacheOptions) -> Result<Self, StoreError> {
        if options.shards == 0 || options.shards > 1024 {
            return Err(StoreError::InvalidOptions(
                "cache shards must be between 1 and 1024",
            ));
        }
        Ok(Self {
            backing,
            shards: (0..options.shards)
                .map(|_| Shard {
                    entries: RwLock::new(HashMap::new()),
                    generation: AtomicU64::new(0),
                    access_sequence: AtomicU64::new(0),
                })
                .collect(),
            options,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            resident_sessions: AtomicUsize::new(0),
            resident_bytes: AtomicUsize::new(0),
            lock_contended: AtomicU64::new(0),
            lock_wait_ns: AtomicU64::new(0),
            max_lock_wait_ns: AtomicU64::new(0),
        })
    }

    pub fn statistics(&self) -> CacheStatistics {
        CacheStatistics {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            resident_sessions: self.resident_sessions.load(Ordering::Relaxed),
            resident_bytes: self.resident_bytes.load(Ordering::Relaxed),
            lock_contended: self.lock_contended.load(Ordering::Relaxed),
            lock_wait: Duration::from_nanos(self.lock_wait_ns.load(Ordering::Relaxed)),
            max_lock_wait: Duration::from_nanos(self.max_lock_wait_ns.load(Ordering::Relaxed)),
        }
    }

    fn read_entries<'a>(&self, shard: &'a Shard) -> RwLockReadGuard<'a, HashMap<String, Entry>> {
        match shard.entries.try_read() {
            Ok(entries) => entries,
            Err(TryLockError::Poisoned(_)) => panic!("session cache shard poisoned"),
            Err(TryLockError::WouldBlock) => {
                self.lock_contended.fetch_add(1, Ordering::Relaxed);
                let started = Instant::now();
                let entries = shard.entries.read().expect("session cache shard poisoned");
                self.record_lock_wait(started.elapsed());
                entries
            }
        }
    }

    fn write_entries<'a>(&self, shard: &'a Shard) -> RwLockWriteGuard<'a, HashMap<String, Entry>> {
        match shard.entries.try_write() {
            Ok(entries) => entries,
            Err(TryLockError::Poisoned(_)) => panic!("session cache shard poisoned"),
            Err(TryLockError::WouldBlock) => {
                self.lock_contended.fetch_add(1, Ordering::Relaxed);
                let started = Instant::now();
                let entries = shard.entries.write().expect("session cache shard poisoned");
                self.record_lock_wait(started.elapsed());
                entries
            }
        }
    }

    fn record_lock_wait(&self, elapsed: Duration) {
        let nanos = elapsed.as_nanos().min(u64::MAX as u128) as u64;
        self.lock_wait_ns.fetch_add(nanos, Ordering::Relaxed);
        self.max_lock_wait_ns.fetch_max(nanos, Ordering::Relaxed);
    }

    fn shard_index(&self, id: &str) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut hasher);
        (hasher.finish() as usize) % self.shards.len()
    }

    fn load_cached(&self, id: &str) -> Option<Arc<SessionRecord>> {
        let shard = &self.shards[self.shard_index(id)];
        let entries = self.read_entries(shard);
        let entry = entries.get(id)?;
        if entry.cached_at.elapsed() <= self.options.ttl && !entry.record.is_expired(now_seconds())
        {
            entry.last_access.store(
                shard.access_sequence.fetch_add(1, Ordering::Relaxed),
                Ordering::Relaxed,
            );
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(Arc::clone(&entry.record));
        }
        let stale_record = Arc::clone(&entry.record);
        drop(entries);
        self.remove_cached_if_same(id, &stale_record);
        None
    }

    fn store_cached_if_unchanged(&self, record: Arc<SessionRecord>, generation: u64) {
        self.store_cached_at_generation(record, Some(generation));
    }

    fn store_cached_at_generation(&self, record: Arc<SessionRecord>, generation: Option<u64>) {
        if self.options.max_sessions == 0
            || self.options.max_bytes == 0
            || self.options.ttl.is_zero()
        {
            return;
        }
        let bytes = serde_json::to_vec(&record.values)
            .map_or(usize::MAX, |value| value.len())
            .saturating_add(record.id.len())
            .saturating_add(std::mem::size_of::<SessionRecord>());
        let per_shard_sessions = self.options.max_sessions.div_ceil(self.shards.len());
        let per_shard_bytes = self.options.max_bytes.div_ceil(self.shards.len());
        if bytes > per_shard_bytes || per_shard_sessions == 0 {
            return;
        }

        let shard = &self.shards[self.shard_index(&record.id)];
        let mut entries = self.write_entries(shard);
        if generation.is_some_and(|expected| shard.generation.load(Ordering::Acquire) != expected) {
            return;
        }
        if entries
            .get(&record.id)
            .is_some_and(|previous| previous.record.revision > record.revision)
        {
            return;
        }
        if let Some(previous) = entries.remove(&record.id) {
            self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
            self.resident_bytes
                .fetch_sub(previous.bytes, Ordering::Relaxed);
        }

        loop {
            let shard_bytes = entries
                .values()
                .fold(0_usize, |total, entry| total.saturating_add(entry.bytes));
            let shard_has_room = entries.len() < per_shard_sessions
                && shard_bytes.saturating_add(bytes) <= per_shard_bytes;
            if shard_has_room && self.reserve_usage(bytes) {
                break;
            }
            let Some(oldest_id) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access.load(Ordering::Relaxed))
                .map(|(id, _)| id.clone())
            else {
                return;
            };
            if let Some(removed) = entries.remove(&oldest_id) {
                self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
                self.resident_bytes
                    .fetch_sub(removed.bytes, Ordering::Relaxed);
                self.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }

        entries.insert(
            record.id.clone(),
            Entry {
                record,
                cached_at: Instant::now(),
                last_access: AtomicU64::new(shard.access_sequence.fetch_add(1, Ordering::Relaxed)),
                bytes,
            },
        );
    }

    /// Reserve global capacity before publishing an entry. Per-shard limits
    /// distribute occupancy, while these atomics enforce the configured total
    /// budget even when shard rounding or a skewed hash distribution would
    /// otherwise let the cache exceed it.
    fn reserve_usage(&self, bytes: usize) -> bool {
        if !reserve_bounded(&self.resident_sessions, 1, self.options.max_sessions) {
            return false;
        }
        if reserve_bounded(&self.resident_bytes, bytes, self.options.max_bytes) {
            return true;
        }
        self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
        false
    }

    fn remove_cached(&self, id: &str) {
        let shard = &self.shards[self.shard_index(id)];
        if let Some(entry) = self.write_entries(shard).remove(id) {
            self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
            self.resident_bytes
                .fetch_sub(entry.bytes, Ordering::Relaxed);
        }
    }

    fn remove_cached_if_same(&self, id: &str, expected: &Arc<SessionRecord>) {
        let shard = &self.shards[self.shard_index(id)];
        let mut entries = self.write_entries(shard);
        if entries
            .get(id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.record, expected))
            && let Some(entry) = entries.remove(id)
        {
            self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
            self.resident_bytes
                .fetch_sub(entry.bytes, Ordering::Relaxed);
        }
    }

    fn begin_mutation(&self, id: &str) -> (usize, u64) {
        let index = self.shard_index(id);
        let generation = self.shards[index]
            .generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.remove_cached(id);
        (index, generation)
    }

    fn finish_mutation(&self, shard_index: usize) {
        self.shards[shard_index]
            .generation
            .fetch_add(1, Ordering::Release);
    }
}

fn reserve_bounded(counter: &AtomicUsize, amount: usize, maximum: usize) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(amount) else {
            return false;
        };
        if next > maximum {
            return false;
        }
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

impl SessionStore for CachedSessionStore {
    fn load(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
        self.load_snapshot(id)
            .map(|record| record.map(|record| (*record).clone()))
    }

    fn load_snapshot(&self, id: &str) -> Result<Option<Arc<SessionRecord>>, StoreError> {
        if let Some(record) = self.load_cached(id) {
            return Ok(Some(record));
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let shard_index = self.shard_index(id);
        let generation = self.shards[shard_index].generation.load(Ordering::Acquire);
        let record = self.backing.load_snapshot(id)?;
        if let Some(record) = &record {
            if !record.is_expired(now_seconds()) {
                self.store_cached_if_unchanged(Arc::clone(record), generation);
            }
        }
        Ok(record)
    }

    fn commit(&self, record: &mut SessionRecord) -> Result<bool, StoreError> {
        let (shard_index, generation) = self.begin_mutation(&record.id);
        let result = self.backing.commit(record);
        if matches!(&result, Ok(true)) {
            self.store_cached_if_unchanged(Arc::new(record.clone()), generation);
        }
        self.finish_mutation(shard_index);
        if !matches!(&result, Ok(true)) {
            self.remove_cached(&record.id);
        }
        result
    }

    fn commit_mutation(
        &self,
        record: &mut SessionRecord,
        original: &SessionRecord,
        mutation: &SessionMutation,
        maximum_bytes: usize,
    ) -> Result<bool, StoreError> {
        let (shard_index, generation) = self.begin_mutation(&record.id);
        let result = self
            .backing
            .commit_mutation(record, original, mutation, maximum_bytes);
        if matches!(&result, Ok(true)) {
            self.store_cached_if_unchanged(Arc::new(record.clone()), generation);
        }
        self.finish_mutation(shard_index);
        if !matches!(&result, Ok(true)) {
            self.remove_cached(&record.id);
        }
        result
    }

    fn commit_rotation(
        &self,
        replacement: &mut SessionRecord,
        expected_previous_revision: u64,
        retired_ids: &[String],
    ) -> Result<bool, StoreError> {
        let mut mutations = vec![self.begin_mutation(&replacement.id)];
        for id in retired_ids {
            mutations.push(self.begin_mutation(id));
        }
        let result =
            self.backing
                .commit_rotation(replacement, expected_previous_revision, retired_ids);
        for (shard_index, _) in &mutations {
            self.finish_mutation(*shard_index);
        }
        self.remove_cached(&replacement.id);
        for id in retired_ids {
            self.remove_cached(id);
        }
        result
    }

    fn remove(&self, id: &str) -> Result<(), StoreError> {
        let (shard_index, _) = self.begin_mutation(id);
        let result = self.backing.remove(id);
        self.finish_mutation(shard_index);
        self.remove_cached(id);
        result
    }

    fn collect_expired(&self, now: i64) -> Result<usize, StoreError> {
        for shard in &self.shards {
            shard.generation.fetch_add(1, Ordering::AcqRel);
        }
        let result = self.backing.collect_expired(now);
        let Ok(removed) = result else {
            for shard in &self.shards {
                shard.generation.fetch_add(1, Ordering::Release);
            }
            return result;
        };
        for shard in &self.shards {
            shard.generation.fetch_add(1, Ordering::Release);
        }
        for shard in &self.shards {
            let mut entries = self.write_entries(shard);
            let expired: Vec<String> = entries
                .iter()
                .filter(|(_, entry)| entry.record.is_expired(now))
                .map(|(id, _)| id.clone())
                .collect();
            for id in expired {
                if let Some(entry) = entries.remove(&id) {
                    self.resident_sessions.fetch_sub(1, Ordering::Relaxed);
                    self.resident_bytes
                        .fetch_sub(entry.bytes, Ordering::Relaxed);
                }
            }
        }
        Ok(removed)
    }
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SQLiteSessionStore, SessionRecord, StoreOptions};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Mutex, mpsc};

    fn ids_for_distinct_shards(cache: &CachedSessionStore, count: usize) -> Vec<String> {
        let mut ids = vec![None; count];
        for candidate in 0_u64..1_000_000 {
            let id = format!("{candidate:064x}");
            let index = cache.shard_index(&id);
            if ids[index].is_none() {
                ids[index] = Some(id);
            }
            if ids.iter().all(Option::is_some) {
                break;
            }
        }
        ids.into_iter()
            .map(|id| id.expect("candidate IDs cover all configured shards"))
            .collect()
    }

    fn cache(path: &std::path::Path, options: CacheOptions) -> CachedSessionStore {
        let backing = Arc::new(SQLiteSessionStore::open(path, StoreOptions::default()).unwrap());
        CachedSessionStore::new(backing, options).unwrap()
    }

    fn commit_empty_record(cache: &CachedSessionStore, id: String) {
        let mut record = SessionRecord::new(id, 3600);
        assert!(cache.commit(&mut record).unwrap());
    }

    struct BlockingSnapshotStore {
        backing: Arc<SQLiteSessionStore>,
        pause_next_snapshot: AtomicBool,
        snapshot_loaded: mpsc::Sender<()>,
        continue_snapshot: Mutex<mpsc::Receiver<()>>,
    }

    impl SessionStore for BlockingSnapshotStore {
        fn load(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
            self.backing.load(id)
        }

        fn load_snapshot(&self, id: &str) -> Result<Option<Arc<SessionRecord>>, StoreError> {
            let snapshot = self.backing.load_snapshot(id)?;
            if self.pause_next_snapshot.swap(false, Ordering::SeqCst) {
                self.snapshot_loaded
                    .send(())
                    .expect("test is waiting for the snapshot");
                self.continue_snapshot
                    .lock()
                    .expect("test snapshot gate poisoned")
                    .recv()
                    .expect("test releases the snapshot");
            }
            Ok(snapshot)
        }

        fn commit(&self, record: &mut SessionRecord) -> Result<bool, StoreError> {
            self.backing.commit(record)
        }

        fn remove(&self, id: &str) -> Result<(), StoreError> {
            self.backing.remove(id)
        }

        fn collect_expired(&self, now: i64) -> Result<usize, StoreError> {
            self.backing.collect_expired(now)
        }
    }

    #[test]
    fn total_resident_session_limit_holds_across_shards() {
        let directory = tempfile::tempdir().unwrap();
        let shards = 32;
        let options = CacheOptions {
            max_sessions: 5,
            max_bytes: 1024 * 1024,
            ttl: Duration::from_secs(60),
            shards,
        };
        let cache = Arc::new(cache(&directory.path().join("sessions.sqlite3"), options));
        let ids = ids_for_distinct_shards(&cache, shards);
        let barrier = Arc::new(std::sync::Barrier::new(ids.len()));
        let workers: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let cache = Arc::clone(&cache);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    commit_empty_record(&cache, id);
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(cache.statistics().resident_sessions, 5);
    }

    #[test]
    fn eviction_preserves_hot_entries_over_older_cold_entries() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(
            &directory.path().join("sessions.sqlite3"),
            CacheOptions {
                max_sessions: 2,
                max_bytes: 1024 * 1024,
                ttl: Duration::from_secs(60),
                shards: 1,
            },
        );
        let oldest = format!("{:064x}", 1);
        let hot = format!("{:064x}", 2);
        let incoming = format!("{:064x}", 3);

        commit_empty_record(&cache, oldest.clone());
        commit_empty_record(&cache, hot.clone());
        assert!(cache.load_snapshot(&hot).unwrap().is_some());
        commit_empty_record(&cache, incoming.clone());

        assert!(cache.load_cached(&oldest).is_none());
        assert!(cache.load_cached(&hot).is_some());
        assert!(cache.load_cached(&incoming).is_some());
        assert_eq!(cache.statistics().evictions, 1);
    }

    #[test]
    fn cache_statistics_measure_waits_after_shard_lock_contention() {
        let directory = tempfile::tempdir().unwrap();
        let cache = Arc::new(cache(
            &directory.path().join("sessions.sqlite3"),
            CacheOptions {
                max_sessions: 1,
                max_bytes: 1024,
                ttl: Duration::from_secs(60),
                shards: 1,
            },
        ));
        let shard = &cache.shards[0];
        let held = shard.entries.write().unwrap();
        let (started, ready) = mpsc::channel();
        let worker_cache = Arc::clone(&cache);
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            worker_cache.load_cached("contended-session");
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while cache.statistics().lock_contended == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(cache.statistics().lock_contended, 1);
        drop(held);
        worker.join().unwrap();

        let statistics = cache.statistics();
        assert_eq!(statistics.lock_contended, 1);
        assert!(statistics.lock_wait > Duration::ZERO);
        assert!(statistics.max_lock_wait > Duration::ZERO);
    }

    #[test]
    fn total_resident_byte_limit_holds_after_shard_rounding() {
        let directory = tempfile::tempdir().unwrap();
        let shards = 4;
        let bytes_per_record = 2 + 64 + std::mem::size_of::<SessionRecord>();
        let maximum_bytes = shards * bytes_per_record - 1;
        let options = CacheOptions {
            max_sessions: 16,
            max_bytes: maximum_bytes,
            ttl: Duration::from_secs(60),
            shards,
        };
        let cache = cache(&directory.path().join("sessions.sqlite3"), options);
        for id in ids_for_distinct_shards(&cache, shards) {
            commit_empty_record(&cache, id);
        }

        let statistics = cache.statistics();
        assert!(statistics.resident_sessions < shards);
        assert!(statistics.resident_bytes <= maximum_bytes);
    }

    #[test]
    fn stale_read_cannot_repopulate_cache_after_session_removal() {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let mut record = SessionRecord::new("revoked-session".into(), 3600);
        record
            .values
            .insert("role".into(), serde_json::json!("admin"));
        assert!(database.commit(&mut record).unwrap());

        let (snapshot_loaded, loaded) = mpsc::channel();
        let (continue_snapshot, continue_read) = mpsc::channel();
        let backing = Arc::new(BlockingSnapshotStore {
            backing: database,
            pause_next_snapshot: AtomicBool::new(true),
            snapshot_loaded,
            continue_snapshot: Mutex::new(continue_read),
        });
        let cache = Arc::new(CachedSessionStore::new(backing, CacheOptions::default()).unwrap());
        let cache_reader = Arc::clone(&cache);
        let session_id = record.id.clone();
        let pending_read =
            std::thread::spawn(move || cache_reader.load_snapshot(&session_id).unwrap());

        loaded
            .recv_timeout(Duration::from_secs(5))
            .expect("SQLite snapshot completes before revocation");
        cache.remove(&record.id).unwrap();
        continue_snapshot.send(()).unwrap();

        assert!(pending_read.join().unwrap().is_some());
        assert_eq!(cache.statistics().resident_sessions, 0);

        assert!(cache.load(&record.id).unwrap().is_none());
        assert_eq!(cache.statistics().resident_sessions, 0);
    }

    #[test]
    fn stale_read_cannot_replace_a_newer_committed_revision() {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(
            SQLiteSessionStore::open(
                directory.path().join("sessions.sqlite3"),
                StoreOptions::default(),
            )
            .unwrap(),
        );
        let mut record = SessionRecord::new("updated-session".into(), 3600);
        record
            .values
            .insert("role".into(), serde_json::json!("member"));
        assert!(database.commit(&mut record).unwrap());

        let (snapshot_loaded, loaded) = mpsc::channel();
        let (continue_snapshot, continue_read) = mpsc::channel();
        let backing = Arc::new(BlockingSnapshotStore {
            backing: database,
            pause_next_snapshot: AtomicBool::new(true),
            snapshot_loaded,
            continue_snapshot: Mutex::new(continue_read),
        });
        let cache = Arc::new(CachedSessionStore::new(backing, CacheOptions::default()).unwrap());
        let cache_reader = Arc::clone(&cache);
        let session_id = record.id.clone();
        let pending_read =
            std::thread::spawn(move || cache_reader.load_snapshot(&session_id).unwrap());

        loaded
            .recv_timeout(Duration::from_secs(5))
            .expect("old SQLite snapshot completes before the update");
        let mut updated = record.clone();
        updated
            .values
            .insert("role".into(), serde_json::json!("admin"));
        assert!(cache.commit(&mut updated).unwrap());
        continue_snapshot.send(()).unwrap();

        assert_eq!(pending_read.join().unwrap().unwrap().revision, 1);
        let cached = cache.load(&record.id).unwrap().unwrap();
        assert_eq!(cached.revision, 2);
        assert_eq!(cached.values["role"], "admin");
    }

    #[test]
    fn stale_ttl_cleanup_cannot_remove_a_newer_cache_entry() {
        let directory = tempfile::tempdir().unwrap();
        let cache = cache(
            &directory.path().join("sessions.sqlite3"),
            CacheOptions::default(),
        );
        let mut old = SessionRecord::new("refreshed-session".into(), 3600);
        old.revision = 1;
        let old = Arc::new(old);
        let mut fresh = (*old).clone();
        fresh.revision = 2;
        let fresh = Arc::new(fresh);

        cache.store_cached_at_generation(Arc::clone(&fresh), None);
        cache.remove_cached_if_same(&old.id, &old);

        assert_eq!(cache.load_cached(&old.id).unwrap().revision, 2);
    }
}
