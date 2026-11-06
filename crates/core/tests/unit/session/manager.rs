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
