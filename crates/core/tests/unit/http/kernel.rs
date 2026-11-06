use super::super::Handler;
use super::*;
use std::sync::{Arc, Mutex};

fn request(kind: RouteKind, accept: &str, raw_method: &str, body: &[u8]) -> Request {
    let mut headers = std::collections::BTreeMap::new();
    if !accept.is_empty() {
        headers.insert("Accept".into(), accept.into());
    }
    if !body.is_empty() {
        headers.insert("Content-Type".into(), "application/json".into());
    }
    Request::new(super::super::RequestData {
        kind,
        path: if kind == RouteKind::Api {
            "/api/test".into()
        } else {
            "/".into()
        },
        raw_method: raw_method.into(),
        body: body.to_vec(),
        headers,
        ..Default::default()
    })
}
fn handler<F>(handler: F) -> Handler
where
    F: Fn(&Request) -> Response + Send + Sync + 'static,
{
    Arc::new(handler)
}
fn pass<F>(middleware: F) -> Middleware
where
    F: for<'a> Fn(&Request, &super::super::Next<'a>) -> Response + Send + Sync + 'static,
{
    Arc::new(middleware)
}

#[test]
fn media_negotiation_honors_specific_exclusions_before_middleware() {
    let mut kernel = HttpKernel::new();
    let calls = Arc::new(Mutex::new(0));
    let tracked = Arc::clone(&calls);
    kernel
        .use_api(pass(move |_, next| {
            *tracked.lock().unwrap() += 1;
            next()
        }))
        .unwrap();
    let routes = kernel.routes_mut().unwrap();
    routes
        .api_get("/items", handler(|_| Response::default()))
        .unwrap();
    let route = routes.routes()[0].clone();
    let response = kernel.handle(
        &route,
        &request(RouteKind::Api, "application/json;q=0, */*;q=1", "GET", b""),
    );
    assert_eq!(response.status, 406);
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[test]
fn middleware_runs_in_order_and_can_short_circuit() {
    let mut kernel = HttpKernel::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    for name in ["global", "group"] {
        let events = Arc::clone(&events);
        let middleware = pass(move |_, next| {
            events.lock().unwrap().push(format!("{name}:in"));
            let response = next();
            events.lock().unwrap().push(format!("{name}:out"));
            response
        });
        if name == "global" {
            kernel.use_global(middleware).unwrap();
        } else {
            kernel.use_api(middleware).unwrap();
        }
    }
    let events_handler = Arc::clone(&events);
    let route_middleware = pass(move |_, next| {
        events_handler.lock().unwrap().push("route:in".into());
        let response = next();
        events_handler.lock().unwrap().push("route:out".into());
        response
    });
    kernel
        .routes_mut()
        .unwrap()
        .api()
        .middleware(route_middleware)
        .unwrap()
        .get(
            "/items",
            handler({
                let events = Arc::clone(&events);
                move |_| {
                    events.lock().unwrap().push("handler".into());
                    Response::default()
                }
            }),
        )
        .unwrap();
    let route = kernel.routes().routes().first().unwrap().clone();
    let response = kernel.handle(&route, &request(RouteKind::Api, "", "GET", b""));
    assert_eq!(response.status, 200);
    assert_eq!(
        *events.lock().unwrap(),
        [
            "global:in",
            "group:in",
            "route:in",
            "handler",
            "route:out",
            "group:out",
            "global:out"
        ]
    );
}

#[test]
fn errors_and_panics_are_converted_to_safe_responses() {
    let mut kernel = HttpKernel::new();
    let routes = kernel.routes_mut().unwrap();
    routes
        .api_get("/panic", handler(|_| panic!("secret internal detail")))
        .unwrap();
    let route = routes.routes()[0].clone();
    let response = kernel.handle(
        &route,
        &request(RouteKind::Api, "application/json", "GET", b""),
    );
    assert_eq!(response.status, 500);
    assert!(!String::from_utf8_lossy(&response.body).contains("secret internal detail"));
    assert_eq!(
        kernel
            .not_found(&request(RouteKind::Api, "application/json", "GET", b""))
            .status,
        404
    );
    assert_eq!(
        kernel
            .method_not_allowed(&request(RouteKind::Web, "text/html", "OPTIONS", b""))
            .status,
        403
    );
}
