use super::*;
use crate::session::{SessionRecord, SessionStore, StoreOptions, generate_session_id};
use serde_json::json;
use std::{sync::Barrier, thread};

#[test]
fn link_grant_is_single_use_and_context_values_are_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SQLiteSessionStore::open(
            directory.path().join("sessions.sqlite3"),
            StoreOptions::default(),
        )
        .unwrap(),
    );
    let context_store: Arc<dyn SessionContextStore> = store.clone();
    let resolver = Arc::new(SessionContextResolver::with_store(context_store));
    let context = SessionContext::create(Arc::clone(&resolver)).unwrap();
    context.put("company.id", json!(15), None).unwrap();

    let sessions = (0..24)
        .map(|_| {
            let mut record = SessionRecord::new(generate_session_id().unwrap(), 3600);
            store.commit(&mut record).unwrap();
            record.id
        })
        .collect::<Vec<_>>();
    assert!(
        resolver
            .get_for_session(&sessions[0], "company.id")
            .unwrap()
            .is_none()
    );

    let grant = context
        .issue_link_grant_for(Duration::from_secs(60))
        .unwrap();
    let barrier = Arc::new(Barrier::new(sessions.len()));
    let mut workers = Vec::with_capacity(sessions.len());
    for session_id in &sessions {
        let resolver = Arc::clone(&resolver);
        let barrier = Arc::clone(&barrier);
        let grant = grant.clone();
        let session_id = session_id.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            resolver.redeem_link_grant(&grant, &session_id).unwrap()
        }));
    }
    let redeemed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(redeemed.iter().filter(|value| **value).count(), 1);
    let winner = redeemed.iter().position(|value| *value).unwrap();
    assert_eq!(
        resolver
            .get_for_session(&sessions[winner], "company.id")
            .unwrap(),
        Some(json!(15))
    );
    for (index, session_id) in sessions.iter().enumerate() {
        if index != winner {
            assert!(
                resolver
                    .get_for_session(session_id, "company.id")
                    .unwrap()
                    .is_none()
            );
        }
    }

    assert!(
        resolver
            .context_for_session(&sessions[winner])
            .unwrap()
            .is_some()
    );
    assert!(resolver.revoke_context_link(&sessions[winner]).unwrap());
    assert!(
        resolver
            .get_for_session(&sessions[winner], "company.id")
            .unwrap()
            .is_none()
    );

    let grant = context
        .issue_link_grant_for(Duration::from_secs(60))
        .unwrap();
    assert!(resolver.redeem_link_grant(&grant, &sessions[0]).unwrap());
    resolver
        .put(
            context.id(),
            "temporary",
            json!("short-lived"),
            Some(Duration::from_secs(1)),
        )
        .unwrap();
    thread::sleep(Duration::from_millis(1_100));
    assert!(resolver.get(context.id(), "temporary").unwrap().is_none());
    store.remove(&sessions[0]).unwrap();
    assert!(
        resolver
            .context_for_session(&sessions[0])
            .unwrap()
            .is_none()
    );
    let outstanding = context
        .issue_link_grant_for(Duration::from_secs(60))
        .unwrap();
    assert!(context.revoke().unwrap());
    assert!(
        resolver
            .get_for_session(&sessions[0], "company.id")
            .unwrap()
            .is_none()
    );
    assert!(resolver.get(context.id(), "company.id").unwrap().is_none());
    assert!(
        !resolver
            .redeem_link_grant(&outstanding, &sessions[1])
            .unwrap()
    );
}

#[test]
fn default_context_and_installation_identity_survive_reopening_sqlite() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.sqlite3");
    let (instance_id, context_id) = {
        let store = Arc::new(SQLiteSessionStore::open(&path, StoreOptions::default()).unwrap());
        let resolver = SessionContextResolver::new(store);
        let context = resolver.default_context().unwrap();
        assert_eq!(resolver.default_context().unwrap(), context);
        (resolver.instance_id(), context)
    };

    let store = Arc::new(SQLiteSessionStore::open(path, StoreOptions::default()).unwrap());
    let resolver = SessionContextResolver::new(store);
    assert_eq!(resolver.instance_id(), instance_id);
    assert_eq!(resolver.default_context().unwrap(), context_id);
    assert!(resolver.delete_context(&context_id).unwrap());
    assert_ne!(resolver.default_context().unwrap(), context_id);
}

#[test]
fn context_values_enforce_size_and_lifetime_limits() {
    let directory = tempfile::tempdir().unwrap();
    let store: Arc<dyn SessionContextStore> = Arc::new(
        SQLiteSessionStore::open(
            directory.path().join("sessions.sqlite3"),
            StoreOptions::default(),
        )
        .unwrap(),
    );
    let resolver = SessionContextResolver::with_store(store);
    let context = resolver.create_context().unwrap();

    assert!(matches!(
        resolver.issue_link_grant_for(&context, Duration::from_secs(601)),
        Err(StoreError::InvalidOptions(_))
    ));
    assert!(matches!(
        resolver.put(&context, "too-large", json!("x".repeat(256 * 1024)), None,),
        Err(StoreError::ResourceLimit(_))
    ));
    assert!(matches!(
        resolver.put(&context, "temporary", json!(true), Some(Duration::ZERO)),
        Err(StoreError::InvalidOptions(_))
    ));
}

#[test]
fn opens_existing_cpp_session_context_schema_and_removes_stale_links() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite3");
    let session_id = "a".repeat(64);
    let context_id = "b".repeat(64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy
            .execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY NOT NULL, created_at INTEGER NOT NULL,
                    last_activity INTEGER NOT NULL, expires_at INTEGER NOT NULL,
                    revision INTEGER NOT NULL, data TEXT NOT NULL
                 );
                 CREATE TABLE kotavel_session_meta (
                    key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL
                 );
                 CREATE TABLE session_contexts (
                    context_id TEXT PRIMARY KEY NOT NULL, created_at INTEGER NOT NULL
                 );
                 CREATE TABLE session_context_grants (
                    grant_token TEXT PRIMARY KEY NOT NULL, context_id TEXT NOT NULL,
                    expires_at INTEGER NOT NULL,
                    FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
                 );
                 CREATE TABLE session_context_links (
                    session_id TEXT PRIMARY KEY NOT NULL, context_id TEXT NOT NULL,
                    linked_at INTEGER NOT NULL,
                    FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
                 );
                 CREATE TABLE session_context_values (
                    context_id TEXT NOT NULL, key TEXT NOT NULL, data TEXT NOT NULL,
                    expires_at INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(context_id, key),
                    FOREIGN KEY(context_id) REFERENCES session_contexts(context_id) ON DELETE CASCADE
                 );",
            )
            .unwrap();
    legacy
        .execute(
            "INSERT INTO sessions VALUES(?1, ?2, ?2, ?3, 1, '{}')",
            rusqlite::params![session_id, now, now + 3600],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO kotavel_session_meta VALUES('instance_id', ?1)",
            [&"c".repeat(64)],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO session_contexts VALUES(?1, ?2)",
            rusqlite::params![context_id, now],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO session_context_links VALUES(?1, ?2, ?3)",
            rusqlite::params![session_id, context_id, now],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO session_context_values VALUES(?1, 'company.id', '15', 0)",
            [&context_id],
        )
        .unwrap();
    drop(legacy);

    let store = Arc::new(SQLiteSessionStore::open(path, StoreOptions::default()).unwrap());
    let resolver = SessionContextResolver::new(Arc::clone(&store));
    assert_eq!(resolver.instance_id().value, "c".repeat(64));
    assert_eq!(
        resolver.get_for_session(&session_id, "company.id").unwrap(),
        Some(json!(15))
    );
    store.remove(&session_id).unwrap();
    assert!(resolver.context_for_session(&session_id).unwrap().is_none());
}
