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
