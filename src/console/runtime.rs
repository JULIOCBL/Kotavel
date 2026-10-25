use super::{
    ConsoleCallbackError, ConsoleContext, ConsoleSettings, console_settings,
    parse_console_arguments, platform::NativePlatformProvider,
};
use crate::{
    container::{Container, ProviderLifecycle},
    contracts::ProcessMetricsProvider,
    diagnostics::{BootstrapLogger, ContextField, DiagnosticError, LogLevel, ReportStatus},
    env::{AppEnvironment, Env, EnvScope, EnvironmentError, FileEnvironment},
    foundation::{Application, ApplicationError, ApplicationState, OperationResult},
    metrics::{
        MetricsMonitor, MetricsSample, MetricsStream, ProcessMetricsCollector, StartupMetrics,
    },
    system,
    text::{MessageParameter, Messages},
};
use std::{
    error::Error,
    ffi::OsString,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub type ConfigureConsole =
    dyn for<'a> Fn(&ConsoleContext<'a>) -> Result<ConsoleSettings, ConsoleCallbackError> + Sync;
pub type ExecuteConsole = dyn for<'a> Fn(&mut Application, &ConsoleContext<'a>) -> Result<i32, ConsoleCallbackError>
    + Sync;
pub type LoadConsoleConfig =
    dyn for<'a> Fn(&Env<'a>) -> Result<crate::config::Config, ConsoleCallbackError> + Sync;
pub use crate::container::CreateServiceProviders;

static METRICS_OUTPUT: Mutex<()> = Mutex::new(());

#[derive(Debug)]
struct CallbackFailure(ConsoleCallbackError);

impl std::fmt::Display for CallbackFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for CallbackFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[derive(Debug)]
struct StartupState {
    root: PathBuf,
    log_directory: PathBuf,
    phase: &'static str,
    debug: bool,
}

impl Default for StartupState {
    fn default() -> Self {
        let root = std::env::current_dir().unwrap_or_default();
        Self {
            log_directory: root.join("storage/logs"),
            root,
            phase: "parse_arguments",
            debug: false,
        }
    }
}

/// Runs the synchronous Kotavel console lifecycle and returns the process exit code.
///
/// Common options are consumed before the application callback. Application
/// providers register before native fallbacks, then `Application` boots before
/// provider boot hooks. All successfully attempted provider boots are shut down
/// in reverse order.
pub fn run_console(
    arguments: impl IntoIterator<Item = OsString>,
    application_help: &str,
    configure: Option<&ConfigureConsole>,
    execute: &ExecuteConsole,
    load_config: Option<&LoadConsoleConfig>,
    create_providers: Option<&CreateServiceProviders>,
) -> i32 {
    let mut state = StartupState::default();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_inner(
            arguments,
            application_help,
            configure,
            execute,
            load_config,
            create_providers,
            &mut state,
        )
    }));
    match result {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => report_startup_failure(error.as_ref(), &state),
        Err(payload) => {
            let reason = panic_payload(payload);
            let error = io::Error::other(reason.clone());
            let code = report_startup_failure(&error, &state);
            if state.debug {
                eprintln!("{reason}");
            }
            code
        }
    }
}

fn run_inner(
    arguments: impl IntoIterator<Item = OsString>,
    application_help: &str,
    configure: Option<&ConfigureConsole>,
    execute: &ExecuteConsole,
    load_config: Option<&LoadConsoleConfig>,
    create_providers: Option<&CreateServiceProviders>,
    startup: &mut StartupState,
) -> Result<i32, ConsoleCallbackError> {
    let mut options = parse_console_arguments(arguments)?;
    startup.debug = options.debug;
    if options.help {
        println!(
            "Uso: aplicación [--env=local|development|production] [--project-root=RUTA] [--development]"
        );
        if !application_help.is_empty() {
            println!("{application_help}");
        }
        return Ok(0);
    }

    startup.phase = "resolve_project_root";
    let packaged_resources = if options.root.as_os_str().is_empty() {
        std::env::current_exe()
            .ok()
            .and_then(|executable| system::packaged_resources_root(&executable))
    } else {
        None
    };
    let root = if !options.root.as_os_str().is_empty() {
        options.root
    } else if let Some(root) = packaged_resources {
        if !options.environment_explicit {
            let selected = std::fs::read_to_string(root.join("kotavel-environment"))?;
            options.environment = selected.trim().to_owned();
        }
        root
    } else if let Some(root) = std::env::var_os("KOTAVEL_PROJECT_ROOT") {
        PathBuf::from(root)
    } else {
        std::env::current_dir()?
    };
    startup.root = root.clone();
    startup.log_directory =
        packaged_log_directory(&root).unwrap_or_else(|| root.join("storage/logs"));
    let root = std::fs::canonicalize(root)?;
    if !root.is_dir() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Project root must be a directory",
        )));
    }
    startup.root = root.clone();
    startup.log_directory =
        packaged_log_directory(&root).unwrap_or_else(|| root.join("storage/logs"));

    startup.phase = "load_environment";
    let run_started = Instant::now();
    let environment_started = Instant::now();
    let source = Arc::new(FileEnvironment::load(&root, &options.environment)?);
    let _environment_scope = EnvScope::new(source.clone());
    let reader = Env::new(source.as_ref());
    let identity = AppEnvironment::load(&reader)?;
    startup.debug |= identity.debug;
    let environment_duration = environment_started.elapsed();

    startup.phase = "load_configuration";
    let configuration_started = Instant::now();
    let config = match load_config {
        Some(load) => load(&reader)?,
        None => crate::config::Config::new(),
    };
    let configuration_duration = configuration_started.elapsed();

    let configuration = Arc::new(Mutex::new(config));
    let identity = Arc::new(identity);
    let services = Container::new();
    services.instance(Arc::clone(&configuration))?;
    services.instance(Arc::clone(&identity))?;

    let configure_context = ConsoleContext {
        root: &root,
        identity: &identity,
        arguments: &options.arguments,
        configuration: &configuration,
        container: None,
    };
    startup.phase = "configure_console";
    let settings_started = Instant::now();
    let mut settings = match configure {
        Some(configure) => configure(&configure_context)?,
        None => {
            let config = configure_context.configuration();
            console_settings(&root, &config)?
        }
    };
    if configure.is_none()
        && packaged_log_directory(&root).is_some()
        && lock(&configuration).get("logging.directory").is_none()
    {
        settings.logs.directory = startup.log_directory.clone();
    }
    startup.log_directory = if settings.logs.directory.as_os_str().is_empty() {
        root.join("storage/logs")
    } else {
        settings.logs.directory.clone()
    };
    settings.errors.environment = if startup.debug {
        crate::diagnostics::Environment::Development
    } else {
        crate::diagnostics::Environment::Production
    };
    let settings_duration = settings_started.elapsed();

    startup.phase = "configure_application";
    let application = Arc::new(Mutex::new(Application::with_error_handling(
        settings.logs,
        settings.errors,
    )?));
    services.instance(Arc::clone(&application))?;

    startup.phase = "register_services";
    let application_provider_list = create_providers.map_or_else(Vec::new, |factory| factory());
    let mut framework_providers =
        ProviderLifecycle::new(&services, vec![Box::new(NativePlatformProvider)]);
    let mut application_providers = ProviderLifecycle::new(&services, application_provider_list);
    application_providers.register_services()?;
    framework_providers.register_services()?;
    services.seal()?;

    startup.phase = "boot_application";
    let application_boot_started = Instant::now();
    lock(&application).boot()?;
    startup.phase = "boot_services";
    framework_providers.boot()?;
    application_providers.boot()?;
    let application_boot_duration = application_boot_started.elapsed();

    let startup_metrics = StartupMetrics {
        environment: environment_duration,
        configuration: configuration_duration,
        settings: settings_duration,
        application_boot: application_boot_duration,
        total: run_started.elapsed(),
    };

    startup.phase = "start_metrics";
    let stream = Arc::new(Mutex::new(MetricsStream::default()));
    let mut print_metrics = settings.metrics.console;
    if settings.metrics.enabled && settings.metrics.stream {
        match lock(&stream).start(&root) {
            Ok(()) => {}
            Err(error) => {
                let description = message("runtime.metrics_stream_failed.description", &[]);
                let action = message("runtime.metrics_stream_failed.action", &[]);
                let fields = [ContextField {
                    key: "reason".into(),
                    value: error.to_string(),
                }];
                let status = lock(&application).log(
                    LogLevel::Warning,
                    "metrics.stream_start_failed",
                    "start_metrics_stream",
                    &description,
                    &fields,
                );
                eprintln!("[Kotavel] {description} {action}");
                if status == ReportStatus::Failed {
                    eprintln!(
                        "{}",
                        message("runtime.diagnostic_not_saved.description", &[])
                    );
                }
                if startup.debug {
                    eprintln!("{error}");
                }
                print_metrics = true;
            }
        }
    }

    let monitor = MetricsMonitor::default();
    if settings.metrics.enabled {
        let service = services.get::<Arc<dyn ProcessMetricsProvider>>()?;
        let provider = Arc::clone(service.as_ref());
        let mut collector = ProcessMetricsCollector::new(provider);
        if settings.metrics.startup {
            if lock(&stream).is_running() {
                lock(&stream).publish_startup(&startup_metrics);
            }
            if print_metrics {
                print_startup_metrics(&startup_metrics);
            }
        }
        if print_metrics || lock(&stream).is_running() {
            if let Some(sample) = collector.sample() {
                publish_sample(&stream, print_metrics, &sample);
            }
            let stream_for_callback = Arc::clone(&stream);
            monitor.start(settings.metrics.interval, collector, move |sample| {
                publish_sample(&stream_for_callback, print_metrics, &sample)
            })?;
        }
    }

    startup.phase = "execute_application";
    let context = ConsoleContext {
        root: &root,
        identity: &identity,
        arguments: &options.arguments,
        configuration: &configuration,
        container: Some(&services),
    };
    let callback_result = catch_unwind(AssertUnwindSafe(|| {
        execute(&mut lock(&application), &context)
    }));
    let code = match callback_result {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            let failure = CallbackFailure(error);
            let _ = lock(&application).execute("console.callback", || Err(failure));
            present_application_error(&application);
            1
        }
        Err(payload) => {
            let reason = panic_payload(payload);
            let diagnostic = DiagnosticError::new("console.callback_panicked", reason.clone())
                .expect("the built-in diagnostic code is valid");
            let _ = lock(&application).execute("console.callback", || Err(diagnostic));
            present_application_error(&application);
            if startup.debug {
                eprintln!("{reason}");
            }
            1
        }
    };

    monitor.stop();
    lock(&stream).stop();
    application_providers.shutdown();
    framework_providers.shutdown();
    let mut app = lock(&application);
    let failed = app.state() == ApplicationState::Failed
        || app.last_error().is_some_and(|error| !error.recoverable);
    app.shutdown();
    drop(app);
    services.shutdown();
    Ok(if failed && code == 0 { 1 } else { code })
}

pub(super) fn packaged_log_directory(root: &Path) -> Option<PathBuf> {
    let selected_environment = std::fs::read_to_string(root.join("kotavel-environment")).ok()?;
    if !matches!(
        selected_environment.trim(),
        "local" | "development" | "production"
    ) {
        return None;
    }
    let contents = root.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    let application = bundle.file_stem()?.to_str()?;
    use crate::contracts::SystemPaths;
    crate::system::NativeSystemPaths
        .data_directory(application)
        .ok()
        .map(|directory| directory.join("logs"))
}

/// Executes one operation, prints its safe error presentation and returns its outcome.
pub fn execute_console<F, E>(
    application: &mut Application,
    id: &str,
    operation: F,
) -> Result<OperationResult, ApplicationError>
where
    F: FnOnce() -> Result<(), E>,
    E: Error + 'static,
{
    let result = application.execute(id, operation)?;
    if let Some(rendered) = application.present_error() {
        eprint!("{}", rendered.console_text());
    }
    if application
        .last_error()
        .is_some_and(|error| error.report_status == ReportStatus::Failed)
    {
        eprintln!(
            "{}",
            message("runtime.diagnostic_not_saved.description", &[])
        );
    }
    Ok(result)
}

fn present_application_error(application: &Mutex<Application>) {
    let application = lock(application);
    if let Some(rendered) = application.present_error() {
        eprint!("{}", rendered.console_text());
    }
    if application
        .last_error()
        .is_some_and(|error| error.report_status == ReportStatus::Failed)
    {
        eprintln!(
            "{}",
            message("runtime.diagnostic_not_saved.description", &[])
        );
    }
}

fn report_startup_failure(error: &(dyn Error + 'static), startup: &StartupState) -> i32 {
    if let Some(error) = error.downcast_ref::<EnvironmentError>() {
        match error {
            EnvironmentError::FileOpen(path) => {
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(".env");
                let example = format!("{file}.example");
                let base = format!("env.open_failed.{}.", system::current_os().key());
                let description = message(&format!("{base}description"), &[("file", file)]);
                let action = message(
                    &format!("{base}action"),
                    &[("file", file), ("example", &example)],
                );
                let command = message(
                    &format!("{base}command"),
                    &[("example", &example), ("file", file)],
                );
                let status = BootstrapLogger::environment_file_open_failed(
                    &startup.log_directory,
                    file,
                    &example,
                    &startup.root.display().to_string(),
                    &error.to_string(),
                    &description,
                );
                eprintln!("{description}\n{action}\n{command}");
                report_log_failure(status, &startup.log_directory);
            }
            EnvironmentError::InvalidFile { path, line, .. } => {
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(".env");
                let description = if *line == 0 {
                    message("env.invalid.file", &[("file", file)])
                } else {
                    message(
                        "env.invalid.line",
                        &[("file", file), ("line", &line.to_string())],
                    )
                };
                let status = BootstrapLogger::environment_file_invalid(
                    &startup.log_directory,
                    file,
                    &startup.root.display().to_string(),
                    *line,
                    &error.to_string(),
                    &description,
                );
                eprintln!("{description}\n{}", message("env.invalid.action", &[]));
                report_log_failure(status, &startup.log_directory);
            }
            _ => report_generic_startup_failure(error, startup),
        }
    } else if let Some(error) = error.downcast_ref::<super::ConsoleSettingsError>() {
        let description = message(
            "runtime.configuration_invalid.description",
            &[("key", &error.key)],
        );
        let status = BootstrapLogger::configuration_invalid(
            &startup.log_directory,
            &startup.root.display().to_string(),
            &error.key,
            error.reason,
            &description,
        );
        eprintln!(
            "{description}\n{}",
            message(
                "runtime.configuration_invalid.action",
                &[("reason", error.reason)],
            )
        );
        report_log_failure(status, &startup.log_directory);
    } else {
        report_generic_startup_failure(error, startup);
    }
    if startup.debug {
        eprintln!("{error}");
    }
    1
}

fn report_generic_startup_failure(error: &(dyn Error + 'static), startup: &StartupState) {
    let description = message("runtime.startup_failed.description", &[]);
    let log_file = startup.log_directory.join("kotavel.log");
    let status = BootstrapLogger::startup_failed(
        &startup.log_directory,
        &startup.root.display().to_string(),
        startup.phase,
        &error.to_string(),
        &description,
    );
    eprintln!(
        "{description}\n{}",
        message(
            "runtime.startup_failed.action",
            &[("log_file", &log_file.display().to_string())],
        )
    );
    report_log_failure(status, &startup.log_directory);
}

fn report_log_failure(status: ReportStatus, directory: &Path) {
    if status == ReportStatus::Failed {
        eprintln!(
            "{}",
            message(
                "runtime.startup_failed.log_failed",
                &[(
                    "log_file",
                    &directory.join("kotavel.log").display().to_string()
                )],
            )
        );
    }
}

fn message(key: &str, parameters: &[MessageParameter<'_>]) -> String {
    Messages::get(key, parameters).unwrap_or_else(|_| key.to_owned())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "Unknown panic".into()
    }
}

fn print_startup_metrics(startup: &StartupMetrics) {
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let _output = lock(&METRICS_OUTPUT);
    println!(
        "[Kotavel] Arranque: entorno {:.2} ms | configuración {:.2} ms | opciones {:.2} ms | aplicación {:.2} ms | total {:.2} ms",
        ms(startup.environment),
        ms(startup.configuration),
        ms(startup.settings),
        ms(startup.application_boot),
        ms(startup.total)
    );
}

fn publish_sample(stream: &Arc<Mutex<MetricsStream>>, print: bool, sample: &MetricsSample) {
    {
        let stream = lock(stream);
        if stream.is_running() {
            stream.publish(sample);
        }
    }
    if !print {
        return;
    }
    let cpu = sample
        .process_cpu_percent
        .map_or_else(|| "calculando".into(), |value| format!("{value:.1} %"));
    let memory = sample.resident_memory_bytes as f64 / (1024.0 * 1024.0);
    let _output = lock(&METRICS_OUTPUT);
    println!("[Kotavel] CPU: {cpu} | RAM residente: {memory:.1} MiB");
}
