use super::*;
use crate::{
    config::{Config, ConfigValue},
    container::{ProviderError, ServiceProvider, ServiceProviderList},
    contracts::PlatformServices,
    foundation::{Application, ApplicationState},
};
use std::{
    ffi::OsString,
    io::{BufRead, BufReader},
    net::TcpStream,
    sync::{Arc, Mutex},
    time::Duration,
};

struct ProbeProvider(Arc<Mutex<Vec<&'static str>>>);

impl ServiceProvider for ProbeProvider {
    fn register_services(
        &mut self,
        services: &crate::container::Container,
    ) -> Result<(), ProviderError> {
        assert!(services.contains::<Mutex<Application>>());
        self.0.lock().unwrap().push("register");
        Ok(())
    }

    fn boot(&mut self, services: &crate::container::Container) -> Result<(), ProviderError> {
        let platform = services.get::<PlatformServices>()?;
        assert!(platform.process_metrics.read().is_some());
        let application = services.get::<Mutex<Application>>()?;
        assert_eq!(application.lock().unwrap().state(), ApplicationState::Ready);
        self.0.lock().unwrap().push("boot");
        Ok(())
    }

    fn shutdown(&mut self, _services: &crate::container::Container) {
        self.0.lock().unwrap().push("shutdown");
    }
}

#[test]
fn help_returns_before_loading_environment_or_running_callbacks() {
    let execute: &ExecuteConsole = &|_, _| panic!("help must bypass application callbacks");
    assert_eq!(
        run_console(
            [OsString::from("--help")],
            "Consumer help",
            None,
            execute,
            None,
            None,
        ),
        0
    );
}

#[test]
fn console_runtime_loads_environment_registers_platform_fallbacks_and_shuts_down() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".env"),
        "APP_NAME=Kotavel Runtime Test\nAPP_ENV=production\nAPP_DEBUG=false\n",
    )
    .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let provider_events = Arc::clone(&events);
    let providers: &CreateServiceProviders = &move || {
        let provider: Box<dyn ServiceProvider> =
            Box::new(ProbeProvider(Arc::clone(&provider_events)));
        ServiceProviderList::from([provider])
    };
    let expected_root = std::fs::canonicalize(root.path()).unwrap();
    let execute: &ExecuteConsole = &move |_application, context| {
        assert!(context.container.is_some());
        assert_eq!(context.identity.name, "Kotavel Runtime Test");
        assert_eq!(context.root, expected_root);
        Ok(7)
    };

    let code = run_console(
        [
            OsString::from(format!("--project-root={}", root.path().display())),
            OsString::from("serve"),
        ],
        "",
        None,
        execute,
        None,
        Some(providers),
    );
    assert_eq!(code, 7);
    assert_eq!(*events.lock().unwrap(), ["register", "boot", "shutdown"]);
}

#[test]
fn console_runtime_publishes_monitor_startup_and_samples_to_loopback_clients() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".env"), "APP_ENV=production\n").unwrap();
    let load_config: &LoadConsoleConfig = &|_| {
        let mut config = Config::new();
        config.set_many([
            ("metrics.enabled", ConfigValue::Bool(true)),
            ("metrics.console", ConfigValue::Bool(false)),
            ("metrics.stream", ConfigValue::Bool(true)),
        ])?;
        Ok(config)
    };
    let expected_root = std::fs::canonicalize(root.path()).unwrap();
    let execute: &ExecuteConsole = &move |_application, context| {
        let endpoint = context
            .root
            .join("storage/framework/metrics")
            .join(format!("kotavel-{}.endpoint", std::process::id()));
        let port = std::fs::read_to_string(endpoint)?.trim().parse::<u16>()?;
        let connection = TcpStream::connect(("127.0.0.1", port))?;
        connection.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut lines = BufReader::new(connection).lines();
        let startup: serde_json::Value = serde_json::from_str(&lines.next().unwrap()?)?;
        let sample: serde_json::Value = serde_json::from_str(&lines.next().unwrap()?)?;
        assert_eq!(startup["type"], "startup");
        assert_eq!(sample["type"], "sample");
        assert_eq!(context.root, expected_root);
        Ok(0)
    };

    let code = run_console(
        [OsString::from(format!(
            "--project-root={}",
            root.path().display()
        ))],
        "",
        None,
        execute,
        Some(load_config),
        None,
    );
    assert_eq!(code, 0);
    assert!(
        !root
            .path()
            .join("storage/framework/metrics")
            .join(format!("kotavel-{}.endpoint", std::process::id()))
            .exists()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn packaged_runtime_writes_logs_to_user_application_support() {
    let directory = tempfile::tempdir().unwrap();
    let resources = directory.path().join("Kotavel.app/Contents/Resources");
    std::fs::create_dir_all(&resources).unwrap();
    std::fs::write(resources.join("kotavel-environment"), "production\n").unwrap();

    let logs = super::runtime::packaged_log_directory(&resources).unwrap();
    assert_eq!(logs.file_name().unwrap(), "logs");
    assert_eq!(logs.parent().unwrap().file_name().unwrap(), "Kotavel");
    assert!(
        logs.to_string_lossy()
            .contains("Library/Application Support")
    );
}
