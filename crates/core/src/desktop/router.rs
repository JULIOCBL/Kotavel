use super::{
    DesktopContext, DesktopError, DispatchResult, Handler, WindowManager, valid_route,
    valid_segmented_name,
};
use crate::session::SessionContext;
use std::collections::HashMap;
use std::sync::Arc;

struct Definition {
    name: Option<String>,
    handler: Handler,
}

#[derive(Default)]
pub struct DesktopRouter {
    routes: Vec<Definition>,
    paths: HashMap<String, usize>,
    names: HashMap<String, usize>,
}

impl DesktopRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains_key(path)
    }

    pub fn dispatch(&self, path: &str, windows: &mut dyn WindowManager) -> DispatchResult {
        self.dispatch_with_session_context(path, windows, None)
    }

    pub(crate) fn dispatch_with_session_context(
        &self,
        path: &str,
        windows: &mut dyn WindowManager,
        session_context: Option<Arc<SessionContext>>,
    ) -> DispatchResult {
        if !valid_route(path) {
            return DispatchResult::InvalidPath;
        }
        let Some(index) = self.paths.get(path) else {
            return DispatchResult::NotFound;
        };
        let mut context = DesktopContext::new(self, windows, session_context);
        (self.routes[*index].handler)(&mut context);
        DispatchResult::Dispatched
    }

    fn add(&mut self, path: &str, handler: Handler) -> Result<usize, DesktopError> {
        if !valid_route(path) {
            return Err(DesktopError::new("desktop route path is invalid"));
        }
        if self.paths.contains_key(path) {
            return Err(DesktopError::new(format!(
                "duplicate desktop route: {path}"
            )));
        }
        let index = self.routes.len();
        self.routes.push(Definition {
            name: None,
            handler,
        });
        self.paths.insert(path.into(), index);
        Ok(index)
    }

    fn set_name(&mut self, index: usize, name: &str) -> Result<(), DesktopError> {
        if index >= self.routes.len() || !valid_segmented_name(name, true) {
            return Err(DesktopError::new("desktop route name is invalid"));
        }
        if self.names.contains_key(name) {
            return Err(DesktopError::new(format!(
                "duplicate desktop route name: {name}"
            )));
        }
        self.routes[index].name = Some(name.into());
        self.names.insert(name.into(), index);
        Ok(())
    }
}

/// Laravel-inspired route builder; desktop routes remain separate from HTTP.
pub struct Route<'a> {
    router: &'a mut DesktopRouter,
    last_route: Option<usize>,
}

impl<'a> Route<'a> {
    pub fn desktop(router: &'a mut DesktopRouter) -> Self {
        Self {
            router,
            last_route: None,
        }
    }

    pub fn get(
        &mut self,
        path: &str,
        handler: impl for<'context> Fn(&mut DesktopContext<'context>) + Send + Sync + 'static,
    ) -> Result<&mut Self, DesktopError> {
        self.last_route = Some(self.router.add(path, Box::new(handler))?);
        Ok(self)
    }

    pub fn name(&mut self, name: &str) -> Result<&mut Self, DesktopError> {
        let index = self
            .last_route
            .ok_or_else(|| DesktopError::new("register a desktop route before naming it"))?;
        self.router.set_name(index, name)?;
        Ok(self)
    }
}
