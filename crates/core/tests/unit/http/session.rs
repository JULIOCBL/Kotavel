use super::*;
use crate::http::{Method, RequestData};
use crate::session::{
    CacheOptions, CookieTransportOptions, SQLiteSessionStore, SessionConfig, SessionContext,
    SessionContextResolver, StoreOptions,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Barrier, Mutex};

fn manager(path: &std::path::Path) -> Arc<SessionManager> {
    Arc::new(
        SessionManager::sqlite(
            path,
            SessionConfig::default(),
            StoreOptions::default(),
            CacheOptions::default(),
        )
        .unwrap(),
    )
}

fn cookie_transport() -> SessionIdentifierTransport {
    SessionIdentifierTransport::cookie(CookieTransportOptions::default()).unwrap()
}

fn request(cookie: Option<&str>) -> Request {
    let headers = cookie
        .map(|cookie| BTreeMap::from([("cookie".into(), cookie.into())]))
        .unwrap_or_default();
    Request::new(RequestData {
        method: Method::Get,
        kind: RouteKind::Web,
        path: "/session".into(),
        headers,
        ..RequestData::default()
    })
}

#[test]
fn middleware_binds_session_for_handler_persists_it_and_sets_cookie() {
    let directory = tempfile::tempdir().unwrap();
    let manager = manager(&directory.path().join("sessions.sqlite3"));
    let middleware = start_session(Arc::clone(&manager), cookie_transport());

    let first_request = request(None);
    assert!(first_request.session().is_err());
    let next = || {
        let session = first_request.session().unwrap();
        assert_eq!(session.increment("visits", 1).unwrap(), 1);
        session.put("role", "guest").unwrap();
        assert_eq!(
            session.get::<String>("role").unwrap().as_deref(),
            Some("guest")
        );
        Response::new(200, "text/plain", "ok")
    };
    let first_response = middleware(&first_request, &next);
    assert_eq!(first_response.status, 200);
    let cookie = first_response.headers.get("Set-Cookie").unwrap();
    let cookie_pair = cookie.split(';').next().unwrap();
    let session_id = cookie_pair.split_once('=').unwrap().1.to_owned();
    assert!(first_request.with_session(|_| ()).is_err());

    let second_request = request(Some(cookie_pair));
    let next = || {
        let session = second_request.session().unwrap();
        assert_eq!(session.increment("visits", 1).unwrap(), 2);
        assert_eq!(session.get::<u64>("visits").unwrap(), Some(2));
        Response::new(200, "text/plain", "ok")
    };
    let second_response = middleware(&second_request, &next);
    assert_eq!(second_response.status, 200);
    let persisted = manager.begin(Some(&session_id)).unwrap();
    assert_eq!(persisted.get("visits"), Some(&json!(2)));
}

#[test]
fn context_aware_middleware_links_only_the_granted_http_session_and_honors_revocation() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite3");
    let manager = manager(&database);
    let context_store =
        Arc::new(SQLiteSessionStore::open(&database, StoreOptions::default()).unwrap());
    let resolver = Arc::new(SessionContextResolver::new(context_store));
    let owner = SessionContext::create(Arc::clone(&resolver)).unwrap();
    owner.put("company.id", json!(15), None).unwrap();
    let middleware = start_session_with_context(
        Arc::clone(&manager),
        SessionIdentifierTransport::header("X-Kotavel-Session").unwrap(),
        Arc::clone(&resolver),
    );

    let unlinked_request = Request::new(RequestData {
        kind: RouteKind::Api,
        path: "/api/context".into(),
        ..RequestData::default()
    });
    let unlinked_response = middleware(&unlinked_request, &|| {
        assert!(unlinked_request.session_context().unwrap().is_none());
        Response::new(200, "application/json", "{}")
    });
    assert_eq!(unlinked_response.status, 200);
    let session_id = unlinked_response.headers["X-Kotavel-Session"].clone();
    assert!(unlinked_request.session_context().is_err());

    let grant = owner.issue_link_grant().unwrap();
    let linked_request = Request::new(RequestData {
        kind: RouteKind::Api,
        path: "/api/context/link".into(),
        headers: BTreeMap::from([("x-kotavel-session".into(), session_id.clone())]),
        ..RequestData::default()
    });
    let captured_context = Arc::new(Mutex::new(None));
    let captured_for_handler = Arc::clone(&captured_context);
    let link_response = middleware(&linked_request, &|| {
        assert!(linked_request.redeem_session_context_link(&grant).unwrap());
        let context = linked_request.session_context().unwrap().unwrap();
        assert_eq!(context.get("company.id").unwrap(), Some(json!(15)));
        assert!(context.put("desktop.notice", json!("ready"), None).unwrap());
        *captured_for_handler.lock().unwrap() = Some(context);
        Response::new(200, "application/json", "{}")
    });
    assert_eq!(link_response.status, 200);

    let second_redemption = Request::new(RequestData {
        kind: RouteKind::Api,
        path: "/api/context/link".into(),
        headers: BTreeMap::from([("x-kotavel-session".into(), session_id.clone())]),
        ..RequestData::default()
    });
    let replay_response = middleware(&second_redemption, &|| {
        assert!(
            !second_redemption
                .redeem_session_context_link(&grant)
                .unwrap()
        );
        Response::new(200, "application/json", "{}")
    });
    assert_eq!(replay_response.status, 200);

    assert!(resolver.revoke_context_link(&session_id).unwrap());
    let stale_handle = captured_context.lock().unwrap().clone().unwrap();
    assert_eq!(stale_handle.get("company.id").unwrap(), None);
    assert!(
        !stale_handle
            .put("desktop.notice", json!("revoked"), None)
            .unwrap()
    );
}

#[test]
fn concurrent_conflicting_requests_return_409_for_the_loser() {
    let directory = tempfile::tempdir().unwrap();
    let manager = manager(&directory.path().join("sessions.sqlite3"));
    let middleware = start_session(
        Arc::clone(&manager),
        SessionIdentifierTransport::header("X-Kotavel-Session").unwrap(),
    );
    let mut initial = manager.begin(None).unwrap();
    initial.put("role", json!("guest")).unwrap();
    assert!(initial.commit().unwrap());
    let session_id = initial.id().to_owned();

    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for role in ["admin", "member"] {
        let middleware = Arc::clone(&middleware);
        let barrier = Arc::clone(&barrier);
        let session_id = session_id.clone();
        workers.push(std::thread::spawn(move || {
            let request = Request::new(RequestData {
                kind: RouteKind::Api,
                path: "/api/profile".into(),
                headers: BTreeMap::from([("x-kotavel-session".into(), session_id)]),
                ..RequestData::default()
            });
            let next = || {
                request
                    .with_session(|session| session.put("role", json!(role)).unwrap())
                    .unwrap();
                barrier.wait();
                Response::new(200, "application/json", "{}")
            };
            middleware(&request, &next).status
        }));
    }
    let statuses = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(statuses.iter().filter(|status| **status == 200).count(), 1);
    assert_eq!(statuses.iter().filter(|status| **status == 409).count(), 1);
    let persisted = manager.begin(Some(&session_id)).unwrap();
    assert!(matches!(
        persisted.get("role"),
        Some(serde_json::Value::String(role)) if role == "admin" || role == "member"
    ));
}
