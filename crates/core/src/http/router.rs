use super::{Method, Request, Response, RouteKind};
use std::collections::BTreeMap;
use std::sync::Arc;

pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync + 'static>;
pub type Next<'a> = dyn Fn() -> Response + 'a;
pub type Middleware = Arc<dyn for<'a> Fn(&Request, &Next<'a>) -> Response + Send + Sync + 'static>;

#[derive(Clone)]
pub struct RouteDefinition {
    pub method: Method,
    pub kind: RouteKind,
    pub path: String,
    pub name: String,
    pub handler: Handler,
    pub middleware: Vec<Middleware>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterError(pub String);

impl std::fmt::Display for RouterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RouterError {}

#[derive(Clone, Default)]
pub struct Router {
    routes: Vec<RouteDefinition>,
    named_routes: BTreeMap<String, usize>,
    middleware_aliases: BTreeMap<String, Middleware>,
    sealed: bool,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    pub fn seal(&mut self) {
        self.sealed = true;
    }
    pub fn api(&mut self) -> Route<'_> {
        Route::new(self, RouteKind::Api)
    }
    pub fn web(&mut self) -> Route<'_> {
        Route::new(self, RouteKind::Web)
    }

    pub fn alias_middleware(
        &mut self,
        alias: &str,
        middleware: Middleware,
    ) -> Result<(), RouterError> {
        self.ensure_open("HTTP middleware aliases")?;
        if alias.is_empty() {
            return Err(RouterError("HTTP middleware alias cannot be empty".into()));
        }
        if self.middleware_aliases.contains_key(alias) {
            return Err(RouterError(format!(
                "Duplicate HTTP middleware alias: {alias}"
            )));
        }
        self.middleware_aliases.insert(alias.into(), middleware);
        Ok(())
    }

    pub fn url_for(&self, name: &str) -> Result<&str, RouterError> {
        let index = self
            .named_routes
            .get(name)
            .ok_or_else(|| RouterError(format!("Named HTTP route not found: {name}")))?;
        Ok(&self.routes[*index].path)
    }

    pub fn api_get(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Api, Method::Get, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn api_post(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Api, Method::Post, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn api_put(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Api, Method::Put, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn api_patch(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Api, Method::Patch, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn api_delete(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Api, Method::Delete, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn web_get(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Web, Method::Get, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn web_post(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Web, Method::Post, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn web_put(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Web, Method::Put, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn web_patch(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Web, Method::Patch, path, handler, Vec::new())
            .map(|_| ())
    }
    pub fn web_delete(&mut self, path: &str, handler: Handler) -> Result<(), RouterError> {
        self.register(RouteKind::Web, Method::Delete, path, handler, Vec::new())
            .map(|_| ())
    }

    fn ensure_open(&self, target: &str) -> Result<(), RouterError> {
        if self.sealed {
            Err(RouterError(format!(
                "{target} are sealed after the kernel starts"
            )))
        } else {
            Ok(())
        }
    }

    fn register(
        &mut self,
        kind: RouteKind,
        method: Method,
        path: &str,
        handler: Handler,
        middleware: Vec<Middleware>,
    ) -> Result<usize, RouterError> {
        self.ensure_open("HTTP routes")?;
        if path.is_empty() || !path.starts_with('/') || path.contains('?') || path.contains('#') {
            return Err(RouterError(
                "Route path must be an absolute path without query or fragment".into(),
            ));
        }
        let path = if kind == RouteKind::Api && path != "/api" && !path.starts_with("/api/") {
            if path == "/" {
                "/api".into()
            } else {
                format!("/api{path}")
            }
        } else {
            path.to_owned()
        };
        if self
            .routes
            .iter()
            .any(|route| route.method == method && route.path == path)
        {
            return Err(RouterError(format!("Duplicate HTTP route: {path}")));
        }
        let index = self.routes.len();
        self.routes.push(RouteDefinition {
            method,
            kind,
            path,
            name: String::new(),
            handler,
            middleware,
        });
        Ok(index)
    }

    fn set_name(&mut self, index: usize, name: &str) -> Result<(), RouterError> {
        self.ensure_open("HTTP routes")?;
        if name.is_empty() {
            return Err(RouterError("Route name cannot be empty".into()));
        }
        let route = self
            .routes
            .get(index)
            .ok_or_else(|| RouterError("Route name requires a registered endpoint".into()))?;
        if self
            .named_routes
            .get(name)
            .is_some_and(|registered| *registered != index)
        {
            return Err(RouterError(format!("Duplicate HTTP route name: {name}")));
        }
        let previous = route.name.clone();
        self.named_routes.insert(name.into(), index);
        if !previous.is_empty() && previous != name {
            self.named_routes.remove(&previous);
        }
        self.routes[index].name = name.into();
        Ok(())
    }

    fn add_middleware(&mut self, index: usize, middleware: Middleware) -> Result<(), RouterError> {
        self.ensure_open("HTTP routes")?;
        let route = self
            .routes
            .get_mut(index)
            .ok_or_else(|| RouterError("Route middleware requires a registered endpoint".into()))?;
        route.middleware.push(middleware);
        Ok(())
    }

    fn middleware_for(&self, alias: &str) -> Result<Middleware, RouterError> {
        self.middleware_aliases
            .get(alias)
            .cloned()
            .ok_or_else(|| RouterError(format!("HTTP middleware alias not registered: {alias}")))
    }

    pub(super) fn contains_middleware_alias(&self, alias: &str) -> bool {
        self.middleware_aliases.contains_key(alias)
    }
}

pub struct Route<'a> {
    router: &'a mut Router,
    kind: RouteKind,
    prefix: String,
    middleware: Vec<Middleware>,
    last_route_index: Option<usize>,
}

impl<'a> Route<'a> {
    /// Laravel-style entry point for registering API routes on a router.
    pub fn api(router: &'a mut Router) -> Self {
        router.api()
    }

    /// Laravel-style entry point for registering browser routes on a router.
    pub fn web(router: &'a mut Router) -> Self {
        router.web()
    }

    fn new(router: &'a mut Router, kind: RouteKind) -> Self {
        Self {
            router,
            kind,
            prefix: String::new(),
            middleware: Vec::new(),
            last_route_index: None,
        }
    }

    pub fn get(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.add(Method::Get, path, handler)
    }
    pub fn post(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.add(Method::Post, path, handler)
    }
    pub fn put(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.add(Method::Put, path, handler)
    }
    pub fn patch(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.add(Method::Patch, path, handler)
    }
    pub fn delete(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.add(Method::Delete, path, handler)
    }
    pub fn remove(&mut self, path: &str, handler: Handler) -> Result<&mut Self, RouterError> {
        self.delete(path, handler)
    }

    pub fn name(&mut self, name: &str) -> Result<&mut Self, RouterError> {
        let index = self
            .last_route_index
            .ok_or_else(|| RouterError("Route name must follow an endpoint registration".into()))?;
        self.router.set_name(index, name)?;
        Ok(self)
    }

    pub fn middleware(&mut self, middleware: Middleware) -> Result<&mut Self, RouterError> {
        if let Some(index) = self.last_route_index {
            self.router.add_middleware(index, middleware)?;
        } else {
            self.middleware.push(middleware);
        }
        Ok(self)
    }

    pub fn middleware_alias(&mut self, alias: &str) -> Result<&mut Self, RouterError> {
        let middleware = self.router.middleware_for(alias)?;
        self.middleware(middleware)
    }

    pub fn prefix(&mut self, prefix: &str) -> Result<&mut Self, RouterError> {
        if prefix.is_empty() {
            return Err(RouterError("Route prefix cannot be empty".into()));
        }
        let normalized = prefix.trim_matches('/');
        if normalized.is_empty() {
            return Err(RouterError("Route prefix cannot be root".into()));
        }
        if self.prefix.is_empty() {
            self.prefix = normalized.into();
        } else {
            self.prefix.push('/');
            self.prefix.push_str(normalized);
        }
        Ok(self)
    }

    pub fn group<F>(&mut self, routes: F) -> Result<(), RouterError>
    where
        F: FnOnce(&mut Route<'_>) -> Result<(), RouterError>,
    {
        routes(self)
    }

    fn add(
        &mut self,
        method: Method,
        path: &str,
        handler: Handler,
    ) -> Result<&mut Self, RouterError> {
        let full_path = if self.prefix.is_empty() {
            if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            }
        } else if path.is_empty() || path == "/" {
            format!("/{}", self.prefix)
        } else if path.starts_with('/') {
            format!("/{}{}", self.prefix, path)
        } else {
            format!("/{}/{path}", self.prefix)
        };
        let index = self.router.register(
            self.kind,
            method,
            &full_path,
            handler,
            self.middleware.clone(),
        )?;
        self.last_route_index = Some(index);
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
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
    fn laravel_style_facade_registers_named_web_and_delete_routes() {
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
}
