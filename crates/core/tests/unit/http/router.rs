use super::*;

fn handler() -> Handler {
    Arc::new(|_| Response::new(200, "text/plain", b"ok".to_vec()))
}
fn middleware() -> Middleware {
    Arc::new(|request, next| {
        let _ = request.path();
        next()
    })
}

#[test]
fn registers_prefixed_named_api_route_and_resolves_url() {
    let mut router = Router::new();
    {
        let mut routes = router.api();
        routes
            .prefix("v1/")
            .unwrap()
            .get("items", handler())
            .unwrap()
            .name("items.index")
            .unwrap();
    }
    assert_eq!(router.routes()[0].path, "/api/v1/items");
    assert_eq!(router.url_for("items.index").unwrap(), "/api/v1/items");
}

#[test]
fn route_builder_registers_named_web_get_and_api_delete_routes() {
    let mut router = Router::new();
    Route::web(&mut router)
        .get("/dashboard", handler())
        .unwrap()
        .name("web.dashboard")
        .unwrap();
    Route::api(&mut router)
        .delete("/items/{id}", handler())
        .unwrap()
        .name("api.items.destroy")
        .unwrap();

    assert_eq!(router.url_for("web.dashboard").unwrap(), "/dashboard");
    assert_eq!(
        router.url_for("api.items.destroy").unwrap(),
        "/api/items/{id}"
    );
    assert_eq!(router.routes()[1].method, Method::Delete);
}

#[test]
fn direct_router_api_registers_delete_routes() {
    let mut router = Router::new();
    router.api_delete("/items/{id}", handler()).unwrap();
    router.web_delete("/resources/{id}", handler()).unwrap();

    assert_eq!(router.routes()[0].kind, RouteKind::Api);
    assert_eq!(router.routes()[0].method, Method::Delete);
    assert_eq!(router.routes()[0].path, "/api/items/{id}");
    assert_eq!(router.routes()[1].kind, RouteKind::Web);
    assert_eq!(router.routes()[1].method, Method::Delete);
    assert_eq!(router.routes()[1].path, "/resources/{id}");
}

#[test]
fn group_middleware_is_inherited_and_aliases_are_resolved() {
    let mut router = Router::new();
    router.alias_middleware("audit", middleware()).unwrap();
    router
        .web()
        .middleware_alias("audit")
        .unwrap()
        .prefix("admin")
        .unwrap()
        .get("/users", handler())
        .unwrap();
    assert_eq!(router.routes()[0].path, "/admin/users");
    assert_eq!(router.routes()[0].middleware.len(), 1);
}

#[test]
fn duplicate_routes_and_registration_after_seal_are_rejected() {
    let mut router = Router::new();
    router.api_get("/health", handler()).unwrap();
    assert!(router.api_get("/api/health", handler()).is_err());
    router.seal();
    assert!(router.web_get("/", handler()).is_err());
    assert!(router.alias_middleware("late", middleware()).is_err());
}
