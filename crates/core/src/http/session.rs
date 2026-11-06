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
#[path = "../../tests/unit/http/session.rs"]
mod tests;
