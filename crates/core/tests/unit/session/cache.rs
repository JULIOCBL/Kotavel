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
    let pending_read = std::thread::spawn(move || cache_reader.load_snapshot(&session_id).unwrap());

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
    let pending_read = std::thread::spawn(move || cache_reader.load_snapshot(&session_id).unwrap());

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
