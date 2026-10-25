use super::{ApiResponse, Middleware, Request, Response, RouteKind};
use crate::session::{
    SessionContextResolver, SessionIdentifierTransport, SessionManager, StoreError,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

struct SessionMaintenance {
    manager: Arc<SessionManager>,
    requests: AtomicU64,
    last_collection: AtomicI64,
    running: AtomicBool,
}

impl SessionMaintenance {
    fn collect_if_due(self: &Arc<Self>) {
        let request = self.requests.fetch_add(1, Ordering::Relaxed);
        let now = now_seconds();
        let last = self.last_collection.load(Ordering::Relaxed);
        if request % 100 != 0 && last != 0 && now.saturating_sub(last) < 60 {
            return;
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        self.last_collection.store(now, Ordering::Relaxed);
        let maintenance = Arc::clone(self);
        if thread::Builder::new()
            .name("kotavel-session-maintenance".into())
            .spawn(move || {
                let _ = maintenance.manager.collect_expired();
                maintenance.running.store(false, Ordering::Release);
            })
            .is_err()
        {
            self.running.store(false, Ordering::Release);
        }
    }
}

/// Persists one isolated session around each request and returns its ID using
/// the configured cookie or header transport. Install only on groups that need
/// session state; API routes remain stateless otherwise.
pub fn start_session(
    manager: Arc<SessionManager>,
    transport: SessionIdentifierTransport,
) -> Middleware {
    start_session_inner(manager, transport, None)
}

/// Installs an isolated HTTP session and makes shared-context operations
/// available only after that session redeems a valid link grant.
pub fn start_session_with_context(
    manager: Arc<SessionManager>,
    transport: SessionIdentifierTransport,
    resolver: Arc<SessionContextResolver>,
) -> Middleware {
    start_session_inner(manager, transport, Some(resolver))
}

fn start_session_inner(
    manager: Arc<SessionManager>,
    transport: SessionIdentifierTransport,
    resolver: Option<Arc<SessionContextResolver>>,
) -> Middleware {
    let maintenance = Arc::new(SessionMaintenance {
        manager: Arc::clone(&manager),
        requests: AtomicU64::new(0),
        last_collection: AtomicI64::new(0),
        running: AtomicBool::new(false),
    });
    Arc::new(move |request, next| {
        maintenance.collect_if_due();
        let header_value = (!transport.is_cookie())
            .then(|| request.header(transport.input_name()))
            .flatten();
        let candidate = transport.read(request.header("cookie"), header_value);
        let session = match manager.begin(candidate.as_deref()) {
            Ok(session) => session,
            Err(_) => return internal_error(),
        };
        if !request.install_session(session) {
            return internal_error();
        }
        if resolver
            .as_ref()
            .is_some_and(|resolver| !request.install_session_context_resolver(Arc::clone(resolver)))
        {
            let _ = request.take_session();
            return internal_error();
        }

        let mut response = next();
        let committed = request.with_session(|session| -> Result<Option<String>, StoreError> {
            if session.commit()? {
                Ok(Some(session.id().to_owned()))
            } else {
                Ok(None)
            }
        });
        let _ = request.take_session();
        request.take_session_context_resolver();
        match committed {
            Ok(Ok(Some(id))) => match transport.response_header(&id) {
                Ok((name, value)) => {
                    response.headers.insert(name, value);
                    response
                }
                Err(_) => internal_error(),
            },
            Ok(Ok(None)) => conflict_response(request),
            Ok(Err(_)) | Err(_) => internal_error(),
        }
    })
}

fn conflict_response(request: &Request) -> Response {
    if request.kind() == RouteKind::Api {
        ApiResponse::single_error(
            409,
            "Session Conflict",
            "Session changed during this request; retry with fresh state.",
        )
        .unwrap_or_else(|_| Response::new(500, "application/json; charset=utf-8", b"{}".to_vec()))
    } else {
        Response::new(
            409,
            "text/plain; charset=utf-8",
            "Session changed during this request; retry with fresh state.",
        )
    }
}

fn internal_error() -> Response {
    Response::new(
        500,
        "text/plain; charset=utf-8",
        "The session could not be persisted.",
    )
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
}
