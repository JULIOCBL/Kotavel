use super::{FormRequest, FormValidation, Handler, Request, Response, form_handler};
use std::sync::Arc;

/// Owns a controller instance and adapts its actions to thread-safe HTTP handlers.
///
/// Route handlers can outlive the setup scope, so the controller is kept in an
/// `Arc` and must be safe to share between request threads.
pub struct Controller<C> {
    instance: Arc<C>,
}

impl<C> Controller<C>
where
    C: Send + Sync + 'static,
{
    pub fn new(instance: C) -> Self {
        Self {
            instance: Arc::new(instance),
        }
    }

    pub fn from_shared(instance: Arc<C>) -> Self {
        Self { instance }
    }

    /// Adapts a controller action that receives the current request.
    pub fn action<Action>(&self, action: Action) -> Handler
    where
        Action: Fn(&C, &Request) -> Response + Send + Sync + 'static,
    {
        let instance = Arc::clone(&self.instance);
        Arc::new(move |request| action(&instance, request))
    }

    /// Validates a typed form request before invoking its controller action.
    ///
    /// Authorization and validation failures are converted to the standard
    /// HTTP responses by `form_handler`; the action only runs for valid input.
    pub fn form_action<Form, Build, Action>(&self, build: Build, action: Action) -> Handler
    where
        Form: FormRequest + 'static,
        Build: Fn(&Request) -> Form + Send + Sync + 'static,
        Action: Fn(&C, &Form, &FormValidation) -> Response + Send + Sync + 'static,
    {
        let instance = Arc::clone(&self.instance);
        form_handler(build, move |form, validation| {
            action(&instance, form, validation)
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/http/controller.rs"]
mod tests;
