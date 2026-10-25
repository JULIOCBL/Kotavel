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
mod tests {
    use super::*;
    use crate::http::{Method, RequestData, RouteKind, ValidationRules};
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct StoreForm;

    impl FormRequest for StoreForm {
        fn rules(&self) -> ValidationRules {
            BTreeMap::from([("name".into(), vec!["required".into(), "string".into()])])
        }
    }

    struct ItemController {
        calls: AtomicUsize,
    }

    fn request(body: &str) -> Request {
        Request::new(RequestData {
            method: Method::Post,
            kind: RouteKind::Api,
            path: "/api/items".into(),
            body: body.as_bytes().to_vec(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            ..RequestData::default()
        })
    }

    #[test]
    fn form_action_validates_before_calling_shared_controller() {
        let controller = Controller::new(ItemController {
            calls: AtomicUsize::new(0),
        });
        let handler = controller.form_action(
            |_| StoreForm,
            |controller, _form, validation| {
                controller.calls.fetch_add(1, Ordering::Relaxed);
                let name = validation.validated().unwrap().get("name").unwrap();
                Response::new(201, "text/plain", name.as_bytes().to_vec())
            },
        );

        let created = handler(&request(r#"{"name":"Kotavel"}"#));
        let rejected = handler(&request("{}"));

        assert_eq!(created.status, 201);
        assert_eq!(created.body, b"Kotavel");
        assert_eq!(rejected.status, 422);
        assert_eq!(controller.instance.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn action_retains_the_controller_instance_after_setup() {
        let controller = Controller::new(ItemController {
            calls: AtomicUsize::new(0),
        });
        let handler = controller.action(|controller, _request| {
            let count = controller.calls.fetch_add(1, Ordering::Relaxed) + 1;
            Response::new(200, "text/plain", count.to_string())
        });
        drop(controller);

        assert_eq!(handler(&request("{}")).body, b"1");
    }
}
