use super::*;
use crate::http::HttpKernel;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn defaults_match_the_framework_listener_contract() {
    let settings = ServerSettings::default();
    assert_eq!(settings.host, "0.0.0.0");
    assert_eq!(settings.port, None);
    assert_eq!(settings.threads, 0);
    assert_eq!(settings.socket_address().unwrap(), None);
}

#[test]
fn rejects_invalid_listener_settings_and_preserves_dns_hosts() {
    let mut settings = ServerSettings::default();
    settings.host.clear();
    assert_eq!(
        settings.validate().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    settings.host = "localhost".into();
    settings.port = None;
    assert!(settings.validate().is_ok());
    settings.port = Some(8080);
    assert_eq!(settings.socket_address().unwrap(), None);
    settings.port = Some(0);
    assert_eq!(
        settings.validate().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn automatic_listener_uses_first_free_candidate_and_explicit_port_is_preserved() {
    let first = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let first_port = first.local_addr().unwrap().port();
    let second = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let second_port = second.local_addr().unwrap().port();
    drop(second);

    let listener = bind_first_available("127.0.0.1", &[first_port, second_port]).unwrap();
    assert_eq!(listener.local_addr().unwrap().port(), second_port);

    let explicit = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let explicit_port = explicit.local_addr().unwrap().port();
    drop(explicit);
    let settings = ServerSettings {
        host: "127.0.0.1".into(),
        port: Some(explicit_port),
        threads: 0,
    };
    assert_eq!(
        settings
            .bind_listener()
            .unwrap()
            .local_addr()
            .unwrap()
            .port(),
        explicit_port
    );
    drop(first);
}

#[test]
fn automatic_listener_reports_when_every_candidate_is_occupied() {
    let first = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let second = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let ports = [
        first.local_addr().unwrap().port(),
        second.local_addr().unwrap().port(),
    ];

    let error = bind_first_available("127.0.0.1", &ports).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
}

struct FakeServer {
    runs: AtomicUsize,
    stopped: AtomicBool,
}

impl Server for FakeServer {
    fn run<'a>(
        &'a self,
        kernel: Arc<HttpKernel>,
        settings: ServerSettings,
        started: Option<ServerStartedCallback>,
    ) -> ServerFuture<'a> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            assert!(kernel.started());
            assert!(kernel.routes().sealed());
            assert_eq!(settings.port, None);
            if let Some(started) = started {
                started();
            }
            Ok(())
        })
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

#[test]
fn kernel_seals_once_delegates_start_and_forwards_stop() {
    let server = FakeServer {
        runs: AtomicUsize::new(0),
        stopped: AtomicBool::new(false),
    };
    let started_count = Arc::new(AtomicUsize::new(0));
    let callback_count = Arc::clone(&started_count);
    let callback: ServerStartedCallback = Arc::new(move || {
        callback_count.fetch_add(1, Ordering::SeqCst);
    });
    let mut kernel = HttpKernel::new();

    block_on(kernel.run(&server, ServerSettings::default(), Some(callback))).unwrap();
    assert!(kernel.started());
    assert_eq!(server.runs.load(Ordering::SeqCst), 1);
    assert_eq!(started_count.load(Ordering::SeqCst), 1);
    assert!(block_on(kernel.run(&server, ServerSettings::default(), None)).is_err());
    assert_eq!(server.runs.load(Ordering::SeqCst), 1);

    kernel.stop(&server);
    assert!(server.stopped.load(Ordering::SeqCst));
}

fn block_on<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("fake server future unexpectedly suspended"),
    }
}
