use super::*;
use serde_json::json;

fn store() -> (tempfile::TempDir, SQLiteSessionStore) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite3");
    let store = SQLiteSessionStore::open(database, StoreOptions::default()).unwrap();
    (directory, store)
}

#[test]
fn commits_use_compare_and_swap_revisions() {
    let (_directory, store) = store();
    let mut record = SessionRecord::new("session-a".into(), 120);
    record.values.insert("visits".into(), json!(1));
    assert!(store.commit(&mut record).unwrap());
    assert_eq!(record.revision, 1);

    let mut first = store.load("session-a").unwrap().unwrap();
    let mut stale = first.clone();
    first.values.insert("visits".into(), json!(2));
    assert!(store.commit(&mut first).unwrap());
    stale.values.insert("visits".into(), json!(3));
    assert!(!store.commit(&mut stale).unwrap());
    assert_eq!(
        store.load("session-a").unwrap().unwrap().values["visits"],
        json!(2)
    );
}

#[test]
fn writer_statistics_track_completed_durable_batches() {
    let (_directory, store) = store();
    let mut record = SessionRecord::new("session-statistics".into(), 120);
    record.values.insert("visits".into(), json!(1));
    assert!(store.commit(&mut record).unwrap());

    let statistics = store.statistics();
    assert_eq!(statistics.write_jobs, 1);
    assert_eq!(statistics.outstanding_jobs, 0);
    assert!(statistics.peak_outstanding_jobs >= 1);
    assert_eq!(statistics.write_batches, 1);
    assert_eq!(statistics.batched_write_jobs, 1);
    assert_eq!(statistics.max_batch_size, 1);
    assert!(statistics.service_time > Duration::ZERO);
}

#[test]
fn committed_sessions_survive_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite3");
    let mut record = SessionRecord::new("durable-session".into(), 120);
    {
        let store = SQLiteSessionStore::open(&database, StoreOptions::default()).unwrap();
        record.values.insert("user_id".into(), json!(42));
        assert!(store.commit(&mut record).unwrap());
    }
    let store = SQLiteSessionStore::open(database, StoreOptions::default()).unwrap();
    assert_eq!(
        store.load("durable-session").unwrap().unwrap().values["user_id"],
        json!(42)
    );
}

#[test]
fn expired_session_cleanup_removes_a_bounded_batch_per_pass() {
    let (_directory, store) = store();
    let expired_count = MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP as usize + 1;

    for index in 0..expired_count {
        let mut record = SessionRecord::new(format!("expired-{index:04}"), 300);
        record.expires_at = 10;
        assert!(store.commit(&mut record).unwrap());
    }
    let remaining_id = format!("expired-{:04}", expired_count - 1);
    let mut active = SessionRecord::new("active-session".into(), 300);
    active.expires_at = 11;
    assert!(store.commit(&mut active).unwrap());

    assert_eq!(
        store.collect_expired(10).unwrap(),
        MAXIMUM_EXPIRED_SESSIONS_PER_CLEANUP as usize
    );
    assert!(store.load(&remaining_id).unwrap().is_some());
    assert!(store.load("active-session").unwrap().is_some());

    assert_eq!(store.collect_expired(10).unwrap(), 1);
    assert!(store.load(&remaining_id).unwrap().is_none());
    assert!(store.load("active-session").unwrap().is_some());
    assert_eq!(store.collect_expired(10).unwrap(), 0);
}

#[test]
fn bounded_writer_accepts_concurrent_independent_sessions() {
    use std::thread;

    let (_directory, store) = store();
    let store = Arc::new(store);
    let workers = 32;
    let mut handles = Vec::new();
    for worker in 0..workers {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            let mut record = SessionRecord::new(format!("session-{worker}"), 300);
            record.values.insert("worker".into(), json!(worker));
            assert!(store.commit(&mut record).unwrap());
            store.load(&record.id).unwrap().unwrap()
        }));
    }
    for handle in handles {
        let result = handle.join().unwrap();
        assert_eq!(result.revision, 1);
    }
}

#[test]
fn batched_writer_isolates_failed_commits_and_persists_successful_peers() {
    use std::sync::Barrier;
    use std::thread;

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SQLiteSessionStore::open(
            directory.path().join("sessions.sqlite3"),
            StoreOptions {
                write_batch_window: Duration::from_millis(10),
                ..StoreOptions::default()
            },
        )
        .unwrap(),
    );
    let ready = Arc::new(Barrier::new(3));

    let failing_store = Arc::clone(&store);
    let failing_ready = Arc::clone(&ready);
    let failing = thread::spawn(move || {
        failing_ready.wait();
        failing_store.write_batched(|connection| {
            connection.execute(
                "INSERT INTO sessions(id, created_at, last_activity, expires_at, revision, data)
                     VALUES ('rolled-back', 1, 1, 100, 1, '{}')",
                [],
            )?;
            Err::<bool, _>(StoreError::InvalidOptions("injected commit failure"))
        })
    });

    let successful_store = Arc::clone(&store);
    let successful_ready = Arc::clone(&ready);
    let successful = thread::spawn(move || {
        successful_ready.wait();
        let mut record = SessionRecord::new("batch-success".into(), 120);
        record
            .values
            .insert("saved".into(), serde_json::json!(true));
        assert!(successful_store.commit(&mut record).unwrap());
        record
    });

    ready.wait();
    assert!(matches!(
        failing.join().unwrap(),
        Err(StoreError::InvalidOptions("injected commit failure"))
    ));
    let committed = successful.join().unwrap();
    assert_eq!(committed.revision, 1);
    assert!(store.load("rolled-back").unwrap().is_none());
    assert_eq!(
        store.load("batch-success").unwrap().unwrap().values["saved"],
        serde_json::json!(true)
    );
}

#[test]
fn rejects_batch_windows_above_the_ten_millisecond_limit() {
    let directory = tempfile::tempdir().unwrap();
    let result = SQLiteSessionStore::open(
        directory.path().join("sessions.sqlite3"),
        StoreOptions {
            write_batch_window: Duration::from_micros(10_001),
            ..StoreOptions::default()
        },
    );
    assert!(matches!(result, Err(StoreError::InvalidOptions(_))));
}
