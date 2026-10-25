use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct FakeWindows {
    views: HashMap<String, String>,
    visible: HashMap<String, bool>,
}

impl WindowManager for FakeWindows {
    fn show(&mut self, id: &str, _: &WindowSettings, view: &View) -> Result<(), DesktopError> {
        self.views.insert(id.into(), view.name.clone());
        self.visible.insert(id.into(), true);
        Ok(())
    }

    fn close(&mut self, id: &str) {
        self.visible.insert(id.into(), false);
    }

    fn is_visible(&self, id: &str) -> bool {
        self.visible.get(id).copied().unwrap_or(false)
    }

    fn has_visible_window(&self) -> bool {
        self.visible.values().any(|visible| *visible)
    }
}

struct FakePlatform {
    observation: Arc<Mutex<Option<PlatformObservation>>>,
    windows: FakeWindows,
    deep_link_event: Option<String>,
}

#[derive(Debug)]
struct PlatformObservation {
    arguments: Vec<String>,
    views_root: String,
    startup_view: Option<String>,
    deep_link_view: Option<String>,
}

impl DesktopPlatform for FakePlatform {
    fn run(
        &mut self,
        arguments: &[String],
        views_root: &std::path::Path,
        on_start: &mut dyn FnMut(&mut dyn WindowManager),
        on_deep_link: &mut dyn FnMut(&str, &mut dyn WindowManager),
    ) -> Result<i32, DesktopError> {
        let mut observation = PlatformObservation {
            arguments: arguments.to_vec(),
            views_root: views_root.to_string_lossy().into_owned(),
            startup_view: None,
            deep_link_view: None,
        };
        on_start(&mut self.windows);
        observation.startup_view = self.windows.views.get("main").cloned();
        if let Some(uri) = &self.deep_link_event {
            on_deep_link(uri, &mut self.windows);
            observation.deep_link_view = self.windows.views.get("main").cloned();
        }
        *self.observation.lock().unwrap() = Some(observation);
        Ok(17)
    }
}

#[test]
fn views_and_routes_reject_invalid_names_and_duplicate_paths() {
    assert_eq!(view("desktop.home").unwrap().name, "desktop.home");
    assert_eq!(view("desktop.pos.home").unwrap().name, "desktop.pos.home");
    assert_eq!(
        view("desktop.pos.home").unwrap().resource_path().unwrap(),
        std::path::PathBuf::from("desktop/pos/home.qml")
    );
    assert!(view("web.home").is_err());
    assert!(view("desktop../home").is_err());

    let mut routes = DesktopRouter::new();
    {
        let mut route = Route::desktop(&mut routes);
        route
            .get("/", |_| {})
            .unwrap()
            .name("desktop.home")
            .unwrap();
        assert!(route.get("/", |_| {}).is_err());
        assert!(route.get("/bad?query", |_| {}).is_err());
        route.get("/other", |_| {}).unwrap();
        assert!(route.name("desktop.home").is_err());
    }
    assert!(routes.contains("/"));
    assert_eq!(
        routes.dispatch("bad", &mut FakeWindows::default()),
        DispatchResult::InvalidPath
    );
    assert_eq!(
        routes.dispatch("/missing", &mut FakeWindows::default()),
        DispatchResult::NotFound
    );
}

#[test]
fn desktop_context_shows_closes_and_navigates_windows() {
    let mut routes = DesktopRouter::new();
    {
        let mut route = Route::desktop(&mut routes);
        route
            .get("/", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
        route
            .get("/pos/home", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings {
                            title: "Punto de venta".into(),
                            width: 1200,
                            height: 800,
                        },
                        &view("desktop.pos.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
    }

    let mut windows = FakeWindows::default();
    assert_eq!(
        routes.dispatch("/", &mut windows),
        DispatchResult::Dispatched
    );
    assert_eq!(
        windows.views.get("main").map(String::as_str),
        Some("desktop.home")
    );
    assert_eq!(
        routes.dispatch("/pos/home", &mut windows),
        DispatchResult::Dispatched
    );
    assert_eq!(
        windows.views.get("main").map(String::as_str),
        Some("desktop.pos.home")
    );
    windows.close("main");
    assert!(!windows.is_visible("main"));
}

#[test]
fn desktop_context_rejects_invalid_window_and_view_settings() {
    let routes = DesktopRouter::new();
    let mut windows = FakeWindows::default();
    let mut context = DesktopContext::new(&routes, &mut windows, None);
    let view = view("desktop.home").unwrap();
    assert!(
        context
            .show(
                "main",
                &WindowSettings {
                    width: 0,
                    ..WindowSettings::default()
                },
                &view
            )
            .is_err()
    );
    assert!(
        context
            .show("../main", &WindowSettings::default(), &view)
            .is_err()
    );
    assert!(
        context
            .show(
                "main",
                &WindowSettings::default(),
                &View {
                    name: "desktop../escape".into()
                },
            )
            .is_err()
    );
}

#[test]
fn view_resolution_requires_an_existing_file_inside_the_views_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("views");
    let nested = root.join("desktop/pos");
    std::fs::create_dir_all(&nested).unwrap();
    let file = nested.join("home.qml");
    std::fs::write(&file, "Item {}").unwrap();

    let definition = view("desktop.pos.home").unwrap();
    assert_eq!(
        definition.resolve(&root).unwrap(),
        file.canonicalize().unwrap()
    );
    assert!(view("desktop.pos.missing").unwrap().resolve(&root).is_err());
    assert!(
        definition
            .resolve(directory.path().join("missing-root"))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn view_resolution_rejects_symlinks_that_escape_the_views_root() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("views");
    let outside = directory.path().join("outside.qml");
    std::fs::create_dir_all(root.join("desktop")).unwrap();
    std::fs::write(&outside, "Item {}").unwrap();
    symlink(&outside, root.join("desktop/home.qml")).unwrap();

    assert!(view("desktop.home").unwrap().resolve(&root).is_err());
}

#[test]
fn kernel_uses_root_or_startup_deep_link_and_filters_runtime_arguments() {
    let observation = Arc::new(Mutex::new(None));
    let mut kernel = DesktopKernel::new(FakePlatform {
        observation: Arc::clone(&observation),
        windows: FakeWindows::default(),
        deep_link_event: Some("kotavel://pos/home".into()),
    });
    {
        let mut route = Route::desktop(kernel.routes());
        route
            .get("/", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
        route
            .get("/pos/home", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.pos.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
    }
    let arguments = vec![
        "kotavel-lab".into(),
        "--env=.env.testing".into(),
        "--development".into(),
        "--platform-flag".into(),
    ];
    assert_eq!(kernel.run(&arguments).unwrap(), 17);
    {
        let guard = observation.lock().unwrap();
        let observed = guard.as_ref().unwrap();
        assert_eq!(observed.arguments, ["kotavel-lab", "--platform-flag"]);
        assert_eq!(observed.views_root, "resources/views");
        assert_eq!(observed.startup_view.as_deref(), Some("desktop.home"));
        assert_eq!(observed.deep_link_view.as_deref(), Some("desktop.pos.home"));
    }

    assert_eq!(
        dispatch_deep_link(
            kernel.routes(),
            "kotavel://pos/home",
            "kotavel",
            &mut FakeWindows::default(),
        ),
        DispatchResult::Dispatched
    );
    assert_eq!(
        dispatch_deep_link(
            kernel.routes(),
            "kotavel://pos/home?next=/admin",
            "kotavel",
            &mut FakeWindows::default(),
        ),
        DispatchResult::InvalidPath
    );
    assert_eq!(
        dispatch_deep_link(
            kernel.routes(),
            "kotavel://pos/unknown",
            "kotavel",
            &mut FakeWindows::default(),
        ),
        DispatchResult::NotFound
    );
}

#[test]
fn kernel_requires_registered_home_and_resolves_startup_deep_link() {
    let mut missing_home = DesktopKernel::new(FakePlatform {
        observation: Arc::new(Mutex::new(None)),
        windows: FakeWindows::default(),
        deep_link_event: None,
    });
    assert!(missing_home.run(&[]).is_err());

    let observation = Arc::new(Mutex::new(None));
    let mut kernel = DesktopKernel::new(FakePlatform {
        observation: Arc::clone(&observation),
        windows: FakeWindows::default(),
        deep_link_event: None,
    });
    {
        let mut route = Route::desktop(kernel.routes());
        route
            .get("/", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
        route
            .get("/pos/home", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.pos.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
    }
    assert!(kernel.run(&["kotavel://pos/home".into()]).is_ok());
    assert_eq!(
        observation
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .startup_view
            .as_deref(),
        Some("desktop.pos.home")
    );
    assert!(kernel.run(&["kotavel://pos/missing".into()]).is_err());
}

#[test]
fn desktop_kernel_passes_explicit_session_context_to_startup_and_deep_link_routes() {
    use crate::session::{
        SQLiteSessionStore, SessionContext, SessionContextResolver, StoreOptions,
    };
    use serde_json::json;

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        SQLiteSessionStore::open(
            directory.path().join("sessions.sqlite3"),
            StoreOptions::default(),
        )
        .unwrap(),
    );
    let resolver = Arc::new(SessionContextResolver::new(store));
    let shared = Arc::new(SessionContext::default_for(resolver).unwrap());
    shared.put("company.id", json!(15), None).unwrap();

    let observed = Arc::new(Mutex::new(Vec::new()));
    let observation = Arc::new(Mutex::new(None));
    let mut kernel = DesktopKernel::new(FakePlatform {
        observation,
        windows: FakeWindows::default(),
        deep_link_event: Some("kotavel://pos/home".into()),
    })
    .with_session_context(Arc::clone(&shared));
    {
        let mut route = Route::desktop(kernel.routes());
        let observed_home = Arc::clone(&observed);
        route
            .get("/", move |context| {
                let value = context
                    .session_context()
                    .expect("the desktop context should be attached")
                    .get("company.id")
                    .unwrap();
                observed_home.lock().unwrap().push(value);
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
        let observed_pos = Arc::clone(&observed);
        route
            .get("/pos/home", move |context| {
                let value = context
                    .session_context()
                    .expect("deep-link routes should receive the attached context")
                    .get("company.id")
                    .unwrap();
                observed_pos.lock().unwrap().push(value);
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.pos.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();
    }

    assert_eq!(kernel.run(&[]).unwrap(), 17);
    assert_eq!(
        *observed.lock().unwrap(),
        [Some(json!(15)), Some(json!(15))],
        "startup and incoming deep links should use the same attached context"
    );
}
