use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

struct Options {
    project: PathBuf,
    command: String,
    arguments: Vec<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
struct InstallOperation {
    reset: bool,
    subcommand: &'static str,
    arguments: Vec<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
struct RunArguments {
    binary: Option<OsString>,
    cargo_arguments: Vec<OsString>,
    application_arguments: Vec<OsString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessSpec {
    program: OsString,
    arguments: Vec<OsString>,
    current_dir: Option<PathBuf>,
    quiet_stdout: bool,
}

impl ProcessSpec {
    fn new(program: &str, arguments: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: arguments.into_iter().collect(),
            current_dir: None,
            quiet_stdout: false,
        }
    }

    fn cargo(manifest: &Path, subcommand: &str, arguments: &[OsString]) -> Self {
        let mut command_arguments = vec![subcommand.into(), "--manifest-path".into()];
        command_arguments.push(manifest.as_os_str().to_owned());
        command_arguments.extend_from_slice(arguments);
        Self::new("cargo", command_arguments)
    }

    fn run(
        manifest: &Path,
        binary: Option<&OsString>,
        cargo_arguments: &[OsString],
        runtime_arguments: &[OsString],
        project: &Path,
    ) -> Self {
        let mut command_arguments = vec!["run".into(), "--manifest-path".into()];
        command_arguments.push(manifest.as_os_str().to_owned());
        if let Some(binary) = binary {
            command_arguments.extend(["--bin".into(), binary.clone()]);
        }
        command_arguments.extend_from_slice(cargo_arguments);
        command_arguments.push("--".into());
        command_arguments.extend_from_slice(runtime_arguments);
        let mut spec = Self::new("cargo", command_arguments);
        spec.current_dir = Some(project.to_owned());
        spec
    }

    fn core_tool(manifest: &Path, project: &Path, binary: &str, arguments: &[OsString]) -> Self {
        let mut command_arguments = vec!["run".into(), "--manifest-path".into()];
        command_arguments.push(manifest.as_os_str().to_owned());
        command_arguments.extend(["--bin".into(), binary.into(), "--".into()]);
        command_arguments.extend(["--project".into(), project.as_os_str().to_owned()]);
        command_arguments.extend_from_slice(arguments);
        Self::new("cargo", command_arguments)
    }

    fn benchmark(manifest: &Path, release: bool, arguments: &[OsString]) -> Self {
        let mut command_arguments = vec!["run".into()];
        if release {
            command_arguments.push("--release".into());
        }
        command_arguments.extend(["--manifest-path".into(), manifest.as_os_str().to_owned()]);
        command_arguments.extend([
            "--bin".into(),
            "kotavel-session-benchmark".into(),
            "--".into(),
        ]);
        command_arguments.extend_from_slice(arguments);
        Self::new("cargo", command_arguments)
    }

    fn prepare(manifest: &Path) -> Vec<Self> {
        let mut metadata = Self::new(
            "cargo",
            [
                OsString::from("metadata"),
                OsString::from("--no-deps"),
                OsString::from("--format-version=1"),
                OsString::from("--manifest-path"),
                manifest.as_os_str().to_owned(),
            ],
        );
        metadata.quiet_stdout = true;
        vec![
            Self::new("rustc", [OsString::from("--version")]),
            Self::new("cargo", [OsString::from("--version")]),
            metadata,
        ]
    }

    fn as_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.arguments);
        if let Some(directory) = &self.current_dir {
            command.current_dir(directory);
        }
        if self.quiet_stdout {
            command.stdout(Stdio::null());
        }
        command
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CommandPlan {
    Prepare {
        project: PathBuf,
    },
    Install {
        manifest: PathBuf,
        operation: InstallOperation,
    },
    Test {
        manifests: Vec<PathBuf>,
        arguments: Vec<OsString>,
    },
    Build {
        manifest: PathBuf,
        arguments: Vec<OsString>,
    },
    Package {
        project: PathBuf,
        manifest: PathBuf,
        arguments: Vec<OsString>,
    },
    Run {
        project: PathBuf,
        manifest: PathBuf,
        binary: Option<OsString>,
        cargo_arguments: Vec<OsString>,
        runtime_arguments: Vec<OsString>,
    },
    Shell {
        project: PathBuf,
    },
    Monitor {
        manifest: PathBuf,
        project: PathBuf,
        arguments: Vec<OsString>,
    },
    Stop {
        manifest: PathBuf,
        project: PathBuf,
        arguments: Vec<OsString>,
    },
    Benchmark {
        manifest: PathBuf,
        release: bool,
        arguments: Vec<OsString>,
    },
}

fn main() {
    match run(env::args_os().skip(1)) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("ERROR: {error}");
            std::process::exit(1);
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<i32, String> {
    let Some(options) = parse_arguments(arguments)? else {
        print_help();
        return Ok(0);
    };
    if options
        .arguments
        .first()
        .is_some_and(|argument| argument == "--help" || argument == "-h")
    {
        print_command_help(&options.command);
        return Ok(0);
    }
    let project = options
        .project
        .canonicalize()
        .map_err(|error| format!("no se pudo abrir el proyecto: {error}"))?;

    execute_plan(plan_command(options, project)?)
}

fn plan_command(options: Options, project: PathBuf) -> Result<CommandPlan, String> {
    let command = options.command.as_str();
    let arguments = options.arguments;
    match command {
        "prepare" => Ok(CommandPlan::Prepare { project }),
        "install" => Ok(CommandPlan::Install {
            manifest: application_manifest(&project)?,
            operation: install_operation(&arguments)?,
        }),
        "test" => {
            let framework = arguments.iter().any(|arg| arg == "--framework");
            let arguments = arguments
                .into_iter()
                .filter(|arg| arg != "--framework")
                .collect::<Vec<_>>();
            let manifests = if framework {
                framework_manifests(&project)?
            } else {
                vec![application_manifest(&project)?]
            };
            Ok(CommandPlan::Test {
                manifests,
                arguments,
            })
        }
        "build" => Ok(CommandPlan::Build {
            manifest: application_manifest(&project)?,
            arguments,
        }),
        "package" => Ok(CommandPlan::Package {
            project: project.clone(),
            manifest: application_manifest(&project)?,
            arguments,
        }),
        "run" => {
            let manifest = application_manifest(&project)?;
            let RunArguments {
                mut binary,
                mut cargo_arguments,
                application_arguments,
            } = parse_run_arguments(&arguments)?;
            let laboratory = is_laboratory_manifest(&manifest);
            if laboratory && binary.is_none() {
                binary = Some("kotavel-laboratory".into());
            }
            if laboratory
                && !cargo_arguments
                    .iter()
                    .any(|argument| argument == "--no-default-features")
                && !cargo_arguments.windows(2).any(|arguments| {
                    arguments[0] == "--features"
                        && arguments[1]
                            .to_string_lossy()
                            .split(',')
                            .any(|feature| feature == "desktop-qt")
                })
                && !cargo_arguments.iter().any(|argument| {
                    argument
                        .to_string_lossy()
                        .strip_prefix("--features=")
                        .is_some_and(|features| {
                            features.split(',').any(|feature| feature == "desktop-qt")
                        })
                })
            {
                cargo_arguments.push("--features".into());
                cargo_arguments.push("desktop-qt".into());
            }
            Ok(CommandPlan::Run {
                project: project.clone(),
                manifest,
                binary,
                cargo_arguments,
                runtime_arguments: runtime_arguments(&project, &application_arguments)?,
            })
        }
        "shell" => {
            if !arguments.is_empty() {
                return Err("shell no acepta argumentos adicionales".into());
            }
            Ok(CommandPlan::Shell { project })
        }
        "monitor" | "stop" => {
            let manifest = core_manifest(&project)?;
            let plan = if command == "monitor" {
                CommandPlan::Monitor {
                    manifest,
                    project,
                    arguments,
                }
            } else {
                CommandPlan::Stop {
                    manifest,
                    project,
                    arguments,
                }
            };
            Ok(plan)
        }
        "benchmark" => {
            let release = arguments.iter().any(|argument| argument == "--release");
            Ok(CommandPlan::Benchmark {
                manifest: core_manifest(&project)?,
                release,
                arguments: arguments
                    .into_iter()
                    .filter(|argument| argument != "--release")
                    .collect(),
            })
        }
        _ => unreachable!("the parser validates command names"),
    }
}

fn execute_plan(plan: CommandPlan) -> Result<i32, String> {
    match plan {
        CommandPlan::Prepare { project } => prepare(&project),
        CommandPlan::Install {
            manifest,
            operation,
        } => {
            if operation.reset {
                let code = cargo(&manifest, "clean", &[])?;
                if code != 0 {
                    return Ok(code);
                }
            }
            cargo(&manifest, operation.subcommand, &operation.arguments)
        }
        CommandPlan::Test {
            manifests,
            arguments,
        } => {
            for (index, manifest) in manifests.iter().enumerate() {
                println!(
                    "{}/{} Pruebas Rust: {}",
                    index + 1,
                    manifests.len(),
                    manifest.display()
                );
                let code = cargo(manifest, "test", &arguments)?;
                if code != 0 {
                    return Ok(code);
                }
            }
            Ok(0)
        }
        CommandPlan::Build {
            manifest,
            arguments,
        } => cargo(&manifest, "build", &arguments),
        CommandPlan::Package {
            project,
            manifest,
            arguments,
        } => framework_core::console::package_project(&project, &manifest, &arguments),
        CommandPlan::Run {
            project,
            manifest,
            binary,
            cargo_arguments,
            runtime_arguments,
        } => execute(ProcessSpec::run(
            &manifest,
            binary.as_ref(),
            &cargo_arguments,
            &runtime_arguments,
            &project,
        )),
        CommandPlan::Shell { project } => {
            let code = prepare(&project)?;
            if code == 0 {
                enter_shell(&project)
            } else {
                Ok(code)
            }
        }
        CommandPlan::Monitor {
            manifest,
            project,
            arguments,
        } => execute_core_tool(&manifest, &project, "kotavel-monitor", &arguments),
        CommandPlan::Stop {
            manifest,
            project,
            arguments,
        } => execute_core_tool(&manifest, &project, "kotavel-stop", &arguments),
        CommandPlan::Benchmark {
            manifest,
            release,
            arguments,
        } => execute(ProcessSpec::benchmark(&manifest, release, &arguments)),
    }
}

fn execute_core_tool(
    manifest: &Path,
    project: &Path,
    binary: &str,
    arguments: &[OsString],
) -> Result<i32, String> {
    execute(ProcessSpec::core_tool(manifest, project, binary, arguments))
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<Options>, String> {
    let mut project = env::current_dir().map_err(|error| error.to_string())?;
    let mut arguments = arguments.into_iter();
    let mut command = None;
    while let Some(argument) = arguments.next() {
        let value = argument
            .to_str()
            .ok_or_else(|| "los argumentos globales deben ser texto UTF-8".to_owned())?;
        if value == "--help" || value == "-h" {
            return Ok(None);
        }
        if value == "--project" {
            project = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "--project necesita una ruta".to_owned())?,
            );
            continue;
        }
        if let Some(path) = value.strip_prefix("--project=") {
            if path.is_empty() {
                return Err("--project necesita una ruta".into());
            }
            project = PathBuf::from(path);
            continue;
        }
        if value.starts_with('-') {
            return Err(format!("opción global desconocida: {value}"));
        }
        if !matches!(
            value,
            "prepare"
                | "install"
                | "test"
                | "build"
                | "package"
                | "run"
                | "shell"
                | "monitor"
                | "stop"
                | "benchmark"
        ) {
            return Err(format!("comando desconocido: {value}"));
        }
        command = Some((value.to_owned(), arguments.collect::<Vec<_>>()));
        break;
    }
    let Some((command, arguments)) = command else {
        return Ok(None);
    };
    Ok(Some(Options {
        project,
        command,
        arguments,
    }))
}

fn application_manifest(project: &Path) -> Result<PathBuf, String> {
    let candidates = [project.join("Cargo.toml")];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
        .ok_or_else(|| format!("no se encontró Cargo.toml dentro de {}", project.display()))
}

fn core_manifest(project: &Path) -> Result<PathBuf, String> {
    let candidates = [
        project.join("Cargo.toml"),
        project.join("crates/core/Cargo.toml"),
        project.join("../Kotavel/crates/core/Cargo.toml"),
    ];
    candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .find(|candidate| {
            std::fs::read_to_string(candidate)
                .map(|manifest| {
                    manifest.lines().any(|line| {
                        line.trim() == "name = \"core\"" || line.trim() == "name = 'core'"
                    })
                })
                .unwrap_or(false)
        })
        .and_then(|candidate| candidate.canonicalize().ok())
        .ok_or_else(|| {
            format!(
                "no se encontró el crate central Rust para {}",
                project.display()
            )
        })
}

fn framework_manifests(project: &Path) -> Result<Vec<PathBuf>, String> {
    let core = core_manifest(project)?;
    let laboratory_candidates = [
        project.join("Cargo.toml"),
        project.join("Laboratory-Kotavel/Cargo.toml"),
        project.join("../Laboratory-Kotavel/Cargo.toml"),
        project.join("../../Laboratory-Kotavel/Cargo.toml"),
        project.join("../../../Laboratory-Kotavel/Cargo.toml"),
    ];
    let laboratory = laboratory_candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .filter_map(|candidate| candidate.canonicalize().ok())
        .find(|candidate| candidate != &core)
        .ok_or_else(|| {
            format!(
                "no se encontró el workspace Rust de Laboratory-Kotavel junto a {}",
                project.display()
            )
        })?;
    Ok(vec![core, laboratory])
}

fn cargo(manifest: &Path, subcommand: &str, arguments: &[OsString]) -> Result<i32, String> {
    execute(ProcessSpec::cargo(manifest, subcommand, arguments))
}

fn install_operation(arguments: &[OsString]) -> Result<InstallOperation, String> {
    let add = arguments.first().is_some_and(|argument| argument == "add");
    let contains_add = arguments.iter().any(|argument| argument == "add");
    let reset = arguments.iter().any(|argument| argument == "--reset");
    if contains_add && reset {
        return Err("install --reset solo se usa para descargar dependencias, no con add".into());
    }
    if add {
        let dependencies = arguments[1..].to_vec();
        if dependencies.is_empty() || dependencies[0].to_str().is_some_and(str::is_empty) {
            return Err("install add necesita un crate, por ejemplo serde@1".into());
        }
        Ok(InstallOperation {
            reset: false,
            subcommand: "add",
            arguments: dependencies,
        })
    } else {
        Ok(InstallOperation {
            reset,
            subcommand: "fetch",
            arguments: arguments
                .iter()
                .filter(|argument| *argument != "--reset")
                .cloned()
                .collect(),
        })
    }
}

fn prepare(project: &Path) -> Result<i32, String> {
    let manifest = application_manifest(project)?;
    let specs = ProcessSpec::prepare(&manifest);
    for spec in &specs[..2] {
        let code = execute(spec.clone()).map_err(|error| {
            format!(
                "no se pudo iniciar {}: {error}",
                spec.program.to_string_lossy()
            )
        })?;
        if code != 0 {
            return Ok(code);
        }
    }
    let metadata_code = execute(specs[2].clone())
        .map_err(|error| format!("no se pudo validar el workspace Cargo: {error}"))?;
    if metadata_code == 0 {
        println!("Workspace Rust listo: {}", manifest.display());
    }
    Ok(metadata_code)
}

fn execute(spec: ProcessSpec) -> Result<i32, String> {
    let status = spec
        .as_command()
        .status()
        .map_err(|error| format!("no se pudo iniciar el proceso: {error}"))?;
    Ok(exit_code(status))
}

fn enter_shell(project: &Path) -> Result<i32, String> {
    framework_core::console::run_interactive_shell(project).map_err(|error| error.to_string())
}

fn is_laboratory_manifest(manifest: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(manifest) else {
        return false;
    };
    let mut in_package = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && line == "name = \"kotavel-laboratory\"" {
            return true;
        }
    }
    false
}

fn parse_run_arguments(arguments: &[OsString]) -> Result<RunArguments, String> {
    let mut binary = None;
    let mut cargo_arguments = Vec::new();
    let mut application_arguments = Vec::with_capacity(arguments.len());
    let mut arguments = arguments.iter();
    let mut passthrough = false;
    while let Some(argument) = arguments.next() {
        let Some(value) = argument.to_str() else {
            application_arguments.push(argument.clone());
            continue;
        };
        if value == "--" {
            passthrough = true;
            continue;
        }
        if passthrough {
            application_arguments.push(argument.clone());
            continue;
        }
        let selected_binary = if value == "--bin" {
            Some(
                arguments
                    .next()
                    .ok_or_else(|| "--bin necesita el nombre de un binario".to_owned())?
                    .clone(),
            )
        } else {
            value.strip_prefix("--bin=").map(OsString::from)
        };
        if let Some(selected_binary) = selected_binary {
            if selected_binary.is_empty() {
                return Err("--bin necesita el nombre de un binario".into());
            }
            if binary.replace(selected_binary).is_some() {
                return Err("run solo acepta una opción --bin".into());
            }
        } else if value == "--features" {
            let features = arguments
                .next()
                .ok_or_else(|| "--features necesita una o más características Cargo".to_owned())?;
            if features.is_empty() {
                return Err("--features necesita una o más características Cargo".into());
            }
            cargo_arguments.push(OsString::from("--features"));
            cargo_arguments.push(features.clone());
        } else if let Some(features) = value.strip_prefix("--features=") {
            if features.is_empty() {
                return Err("--features necesita una o más características Cargo".into());
            }
            cargo_arguments.push(argument.clone());
        } else if value == "--release" || value == "--no-default-features" {
            cargo_arguments.push(argument.clone());
        } else {
            application_arguments.push(argument.clone());
        }
    }
    Ok(RunArguments {
        binary,
        cargo_arguments,
        application_arguments,
    })
}

fn runtime_arguments(project: &Path, arguments: &[OsString]) -> Result<Vec<OsString>, String> {
    let mut runtime_arguments = Vec::with_capacity(arguments.len() + 1);
    let mut arguments = arguments.iter();
    let mut passthrough = false;
    while let Some(argument) = arguments.next() {
        let Some(value) = argument.to_str() else {
            runtime_arguments.push(argument.clone());
            continue;
        };
        if value == "--" {
            passthrough = true;
            continue;
        }
        if passthrough {
            runtime_arguments.push(argument.clone());
            continue;
        }
        if value == "--deep-link" {
            let uri = arguments
                .next()
                .ok_or_else(|| "--deep-link necesita una URI".to_owned())?;
            if uri.is_empty() {
                return Err("--deep-link necesita una URI".into());
            }
            runtime_arguments.push(uri.clone());
        } else if let Some(uri) = value.strip_prefix("--deep-link=") {
            if uri.is_empty() {
                return Err("--deep-link necesita una URI".into());
            }
            runtime_arguments.push(uri.into());
        } else if value == "--env" {
            let environment = arguments
                .next()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "--env necesita local, development o production".to_owned())?;
            validate_runtime_environment(environment)?;
            runtime_arguments.push(format!("--env={environment}").into());
        } else if let Some(environment) = value.strip_prefix("--env=") {
            validate_runtime_environment(environment)?;
            runtime_arguments.push(argument.clone());
        } else {
            runtime_arguments.push(argument.clone());
        }
    }
    runtime_arguments.push(format!("--project-root={}", project.display()).into());
    Ok(runtime_arguments)
}

fn validate_runtime_environment(environment: &str) -> Result<(), String> {
    if matches!(environment, "local" | "development" | "production") {
        Ok(())
    } else {
        Err(format!(
            "entorno inválido: {environment}; usa local, development o production"
        ))
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn print_help() {
    println!(
        "{}\n{}\n\n{}\n  cargo kotavel [--project RUTA] <comando> [opciones]\n\n{}",
        style("Kotavel", "1;36"),
        style("Framework de aplicaciones Rust", "2"),
        style("Uso", "1"),
        style("Comandos disponibles", "1")
    );

    let commands = [
        ("prepare", "Validar Rust, Cargo y el manifiesto."),
        (
            "install",
            "Descargar dependencias; add CRATE@VERSIÓN agrega un crate.",
        ),
        (
            "test",
            "Ejecutar pruebas; --framework incluye ambos proyectos.",
        ),
        ("build", "Compilar el proyecto seleccionado."),
        ("package", "Empaquetar macOS en .app, .dmg o .pkg."),
        ("run", "Iniciar la aplicación."),
        ("shell", "Abrir un shell en la raíz del proyecto."),
        ("monitor", "Leer métricas de una instancia activa."),
        ("stop", "Detener instancias de este proyecto."),
        (
            "benchmark",
            "Medir sesiones; --release activa optimización.",
        ),
    ];
    for (name, description) in commands {
        println!("  {}  {description}", style(&format!("{name:<12}"), "1;32"));
    }

    println!(
        "\n{}\n  --project RUTA  Seleccionar la raíz del proyecto.\n  -h, --help      Mostrar ayuda.\n\nEn Laboratory-Kotavel, run abre Qt; --no-default-features inicia solo HTTP.\nUsa cargo kotavel <comando> --help para ver las opciones de un comando.",
        style("Opciones comunes", "1")
    );
}

fn print_command_help(command: &str) {
    let (usage, description, options) = match command {
        "prepare" => (
            "prepare [--project RUTA]",
            "Valida Rust, Cargo y el manifiesto del proyecto.",
            "",
        ),
        "install" => (
            "install [--reset] | install add CRATE@VERSIÓN",
            "Descarga dependencias o agrega un crate al proyecto.",
            "--reset limpia artefactos antes de descargar dependencias.",
        ),
        "test" => (
            "test [--framework] [FILTRO...]",
            "Ejecuta pruebas Rust. --framework cubre Kotavel y el Laboratorio.",
            "",
        ),
        "build" => (
            "build [OPCIONES CARGO]",
            "Compila el proyecto indicado por --project o la raíz actual.",
            "",
        ),
        "package" => (
            "package [opciones]",
            "Empaqueta una aplicación macOS; la salida predeterminada es dist/.",
            "--format app|dmg|pkg|all\n--build-type Debug|Release\n--features FEATURES\n--deploy-qt",
        ),
        "run" => (
            "run [opciones] [-- ARG...]",
            "Inicia la aplicación seleccionada.",
            "--bin NOMBRE     Elegir el binario.\n--features LISTA  Activar características Cargo.\n--release         Usar compilación optimizada.\n--env ENTORNO     Elegir local, development o production.\n--deep-link URI   Abrir una ruta Desktop.\nEn Laboratory-Kotavel, Qt se activa por defecto; --no-default-features inicia solo HTTP.",
        ),
        "shell" => (
            "shell [--project RUTA]",
            "Prepara el proyecto y abre una terminal interactiva en su raíz.",
            "",
        ),
        "monitor" => (
            "monitor [--pid PID]",
            "Lee el stream de métricas de una instancia activa.",
            "",
        ),
        "stop" => (
            "stop [--pid PID]",
            "Detiene ordenadamente las instancias de este proyecto.",
            "",
        ),
        "benchmark" => (
            "benchmark [--release] [ARGUMENTOS]",
            "Ejecuta la medición del motor de sesiones.",
            "--release usa el perfil optimizado.",
        ),
        _ => return print_help(),
    };
    println!(
        "{}\n\n{}\n  cargo kotavel {usage}\n\n{description}",
        style(&format!("Kotavel · {command}"), "1;36"),
        style("Uso", "1")
    );
    if !options.is_empty() {
        let options = options
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        println!("\n{}\n{options}", style("Opciones", "1"));
    }
}

fn style(value: &str, code: &str) -> String {
    if io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none() {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laboratory_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let core = root.join("Kotavel/crates/core");
        let laboratory = root.join("Laboratory-Kotavel");
        std::fs::create_dir_all(&core).unwrap();
        std::fs::create_dir_all(&laboratory).unwrap();
        std::fs::write(core.join("Cargo.toml"), "[package]\nname = \"core\"\n").unwrap();
        std::fs::write(
            laboratory.join("Cargo.toml"),
            "[package]\nname = \"kotavel-laboratory\"\n",
        )
        .unwrap();
        (directory, root, core, laboratory)
    }

    fn options(command: &str, arguments: &[&str]) -> Options {
        Options {
            project: PathBuf::new(),
            command: command.into(),
            arguments: arguments.iter().map(OsString::from).collect(),
        }
    }

    #[test]
    fn prepare_plans_validation_for_the_selected_laboratory_project() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        let manifest = laboratory.join("Cargo.toml");
        assert_eq!(
            plan_command(options("prepare", &[]), laboratory.clone()).unwrap(),
            CommandPlan::Prepare {
                project: laboratory
            }
        );
        let commands = ProcessSpec::prepare(&manifest);
        assert_eq!(
            commands,
            vec![
                ProcessSpec::new("rustc", ["--version".into()]),
                ProcessSpec::new("cargo", ["--version".into()]),
                ProcessSpec {
                    program: "cargo".into(),
                    arguments: [
                        "metadata".into(),
                        "--no-deps".into(),
                        "--format-version=1".into(),
                        "--manifest-path".into(),
                        manifest.as_os_str().to_owned(),
                    ]
                    .into(),
                    current_dir: None,
                    quiet_stdout: true,
                },
            ]
        );
    }

    #[test]
    fn install_plans_dependency_fetch_and_reset_without_running_cargo() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(
                options("install", &["--reset", "--offline"]),
                laboratory.clone()
            )
            .unwrap(),
            CommandPlan::Install {
                manifest: laboratory.join("Cargo.toml"),
                operation: InstallOperation {
                    reset: true,
                    subcommand: "fetch",
                    arguments: vec!["--offline".into()],
                },
            }
        );
        let manifest = laboratory.join("Cargo.toml");
        assert_eq!(
            [
                ProcessSpec::cargo(&manifest, "clean", &[]),
                ProcessSpec::cargo(&manifest, "fetch", &["--offline".into()]),
            ],
            [
                ProcessSpec::new(
                    "cargo",
                    [
                        "clean".into(),
                        "--manifest-path".into(),
                        manifest.as_os_str().into()
                    ],
                ),
                ProcessSpec::new(
                    "cargo",
                    [
                        "fetch".into(),
                        "--manifest-path".into(),
                        manifest.as_os_str().into(),
                        "--offline".into(),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn test_plans_core_and_laboratory_tests_from_the_laboratory_entrypoint() {
        let (_directory, _root, core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(
                options("test", &["--framework", "--", "--nocapture"]),
                laboratory.clone()
            )
            .unwrap(),
            CommandPlan::Test {
                manifests: vec![core.join("Cargo.toml"), laboratory.join("Cargo.toml")],
                arguments: vec!["--".into(), "--nocapture".into()],
            }
        );
        let args = ["--".into(), "--nocapture".into()];
        assert_eq!(
            [
                ProcessSpec::cargo(&core.join("Cargo.toml"), "test", &args),
                ProcessSpec::cargo(&laboratory.join("Cargo.toml"), "test", &args),
            ],
            [
                ProcessSpec::new(
                    "cargo",
                    [
                        "test".into(),
                        "--manifest-path".into(),
                        core.join("Cargo.toml").into_os_string(),
                        "--".into(),
                        "--nocapture".into(),
                    ],
                ),
                ProcessSpec::new(
                    "cargo",
                    [
                        "test".into(),
                        "--manifest-path".into(),
                        laboratory.join("Cargo.toml").into_os_string(),
                        "--".into(),
                        "--nocapture".into(),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn build_plans_cargo_build_for_the_selected_project() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(options("build", &["--release"]), laboratory.clone()).unwrap(),
            CommandPlan::Build {
                manifest: laboratory.join("Cargo.toml"),
                arguments: vec!["--release".into()],
            }
        );
        assert_eq!(
            ProcessSpec::cargo(
                &laboratory.join("Cargo.toml"),
                "build",
                &["--release".into()]
            ),
            ProcessSpec::new(
                "cargo",
                [
                    "build".into(),
                    "--manifest-path".into(),
                    laboratory.join("Cargo.toml").into_os_string(),
                    "--release".into(),
                ]
            )
        );
    }

    #[test]
    fn package_plans_the_selected_project_and_forwards_package_options() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(options("package", &["--format", "app"]), laboratory.clone()).unwrap(),
            CommandPlan::Package {
                project: laboratory.clone(),
                manifest: laboratory.join("Cargo.toml"),
                arguments: vec!["--format".into(), "app".into()],
            }
        );
    }

    #[test]
    fn run_plans_the_desktop_feature_and_project_root_for_laboratory() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        let plan = plan_command(
            options("run", &["--env", "development"]),
            laboratory.clone(),
        )
        .unwrap();
        assert_eq!(
            plan,
            CommandPlan::Run {
                project: laboratory.clone(),
                manifest: laboratory.join("Cargo.toml"),
                binary: Some("kotavel-laboratory".into()),
                cargo_arguments: vec!["--features".into(), "desktop-qt".into()],
                runtime_arguments: vec![
                    "--env=development".into(),
                    format!("--project-root={}", laboratory.display()).into(),
                ],
            }
        );
        assert_eq!(
            ProcessSpec::run(
                &laboratory.join("Cargo.toml"),
                Some(&OsString::from("kotavel-laboratory")),
                &["--features".into(), "desktop-qt".into()],
                &[
                    "--env=development".into(),
                    format!("--project-root={}", laboratory.display()).into(),
                ],
                &laboratory,
            ),
            ProcessSpec {
                program: "cargo".into(),
                arguments: [
                    "run".into(),
                    "--manifest-path".into(),
                    laboratory.join("Cargo.toml").into_os_string(),
                    "--bin".into(),
                    "kotavel-laboratory".into(),
                    "--features".into(),
                    "desktop-qt".into(),
                    "--".into(),
                    "--env=development".into(),
                    format!("--project-root={}", laboratory.display()).into(),
                ]
                .into(),
                current_dir: Some(laboratory.clone()),
                quiet_stdout: false,
            }
        );
        let no_desktop = plan_command(
            options("run", &["--no-default-features", "--", "--http-only"]),
            laboratory.clone(),
        )
        .unwrap();
        assert!(
            matches!(no_desktop, CommandPlan::Run { cargo_arguments, .. }
            if cargo_arguments == vec![OsString::from("--no-default-features")])
        );
        let selected = plan_command(
            options(
                "run",
                &["--bin", "kotavel-lab-cli", "--no-default-features"],
            ),
            laboratory,
        )
        .unwrap();
        assert!(matches!(selected, CommandPlan::Run { binary, .. }
            if binary == Some(OsString::from("kotavel-lab-cli"))));
    }

    #[test]
    fn shell_rejects_arguments_before_opening_an_interactive_terminal() {
        let (_directory, _root, _core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(options("shell", &[]), laboratory.clone()).unwrap(),
            CommandPlan::Shell {
                project: laboratory.clone()
            }
        );
        assert!(plan_command(options("shell", &["unexpected"]), laboratory).is_err());
    }

    #[test]
    fn monitor_plans_the_core_monitor_binary_with_the_laboratory_scope() {
        let (_directory, _root, core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(options("monitor", &["--pid", "42"]), laboratory.clone()).unwrap(),
            CommandPlan::Monitor {
                manifest: core.join("Cargo.toml"),
                project: laboratory.clone(),
                arguments: vec!["--pid".into(), "42".into()],
            }
        );
        assert_eq!(
            ProcessSpec::core_tool(
                &core.join("Cargo.toml"),
                &laboratory,
                "kotavel-monitor",
                &["--pid".into(), "42".into()],
            ),
            ProcessSpec::new(
                "cargo",
                [
                    "run".into(),
                    "--manifest-path".into(),
                    core.join("Cargo.toml").into_os_string(),
                    "--bin".into(),
                    "kotavel-monitor".into(),
                    "--".into(),
                    "--project".into(),
                    laboratory.clone().into_os_string(),
                    "--pid".into(),
                    "42".into(),
                ],
            )
        );
    }

    #[test]
    fn stop_plans_the_core_stop_binary_with_the_laboratory_scope() {
        let (_directory, _root, core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(options("stop", &["--pid", "42"]), laboratory.clone()).unwrap(),
            CommandPlan::Stop {
                manifest: core.join("Cargo.toml"),
                project: laboratory.clone(),
                arguments: vec!["--pid".into(), "42".into()],
            }
        );
        assert_eq!(
            ProcessSpec::core_tool(
                &core.join("Cargo.toml"),
                &laboratory,
                "kotavel-stop",
                &["--pid".into(), "42".into()],
            ),
            ProcessSpec::new(
                "cargo",
                [
                    "run".into(),
                    "--manifest-path".into(),
                    core.join("Cargo.toml").into_os_string(),
                    "--bin".into(),
                    "kotavel-stop".into(),
                    "--".into(),
                    "--project".into(),
                    laboratory.into_os_string(),
                    "--pid".into(),
                    "42".into(),
                ],
            )
        );
    }

    #[test]
    fn benchmark_plans_the_core_benchmark_and_preserves_profile_arguments() {
        let (_directory, _root, core, laboratory) = laboratory_fixture();
        assert_eq!(
            plan_command(
                options("benchmark", &["--release", "500", "10", "shared"]),
                laboratory,
            )
            .unwrap(),
            CommandPlan::Benchmark {
                manifest: core.join("Cargo.toml"),
                release: true,
                arguments: vec!["500".into(), "10".into(), "shared".into()],
            }
        );
        assert_eq!(
            ProcessSpec::benchmark(
                &core.join("Cargo.toml"),
                true,
                &["500".into(), "10".into(), "shared".into()],
            ),
            ProcessSpec::new(
                "cargo",
                [
                    "run".into(),
                    "--release".into(),
                    "--manifest-path".into(),
                    core.join("Cargo.toml").into_os_string(),
                    "--bin".into(),
                    "kotavel-session-benchmark".into(),
                    "--".into(),
                    "500".into(),
                    "10".into(),
                    "shared".into(),
                ],
            )
        );
    }

    #[test]
    fn parses_project_before_command_and_preserves_command_arguments() {
        let parsed = parse_arguments([
            "--project=/tmp/kotavel".into(),
            "test".into(),
            "--framework".into(),
            "--".into(),
            "--nocapture".into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(parsed.project, PathBuf::from("/tmp/kotavel"));
        assert_eq!(parsed.command, "test");
        assert_eq!(
            parsed.arguments,
            ["--framework", "--", "--nocapture"].map(OsString::from)
        );
    }

    #[test]
    fn rejects_unknown_commands_and_global_flags() {
        assert!(parse_arguments(["pack".into()]).is_err());
        assert_eq!(
            parse_arguments(["install".into()])
                .unwrap()
                .unwrap()
                .command,
            "install"
        );
        assert_eq!(
            parse_arguments(["package".into()])
                .unwrap()
                .unwrap()
                .command,
            "package"
        );
        assert!(parse_arguments(["test".into(), "--project".into()]).is_ok());
        assert!(parse_arguments(["--unknown".into()]).is_err());
        assert_eq!(
            parse_arguments(["shell".into()]).unwrap().unwrap().command,
            "shell"
        );
    }

    #[test]
    fn parser_exposes_all_ten_core_commands_from_the_laboratory_entrypoint() {
        for command in [
            "prepare",
            "install",
            "test",
            "build",
            "package",
            "run",
            "shell",
            "monitor",
            "stop",
            "benchmark",
        ] {
            let parsed =
                parse_arguments(["--project".into(), "/tmp/laboratory".into(), command.into()])
                    .unwrap()
                    .unwrap();
            assert_eq!(parsed.command, command);
        }
    }

    #[test]
    fn install_fetch_and_add_map_to_cargo_without_rewriting_dependency_arguments() {
        assert_eq!(
            install_operation(&[]).unwrap(),
            InstallOperation {
                reset: false,
                subcommand: "fetch",
                arguments: vec![]
            }
        );
        let offline = [OsString::from("--offline")];
        assert_eq!(
            install_operation(&offline).unwrap(),
            InstallOperation {
                reset: false,
                subcommand: "fetch",
                arguments: offline.to_vec()
            }
        );

        let add = [
            OsString::from("add"),
            OsString::from("serde@1"),
            OsString::from("--offline"),
        ];
        assert_eq!(
            install_operation(&add).unwrap(),
            InstallOperation {
                reset: false,
                subcommand: "add",
                arguments: add[1..].to_vec()
            }
        );
        assert!(install_operation(&[OsString::from("add")]).is_err());
        assert!(install_operation(&[OsString::from("add"), OsString::new()]).is_err());
        assert!(
            install_operation(&[
                OsString::from("add"),
                OsString::from("serde"),
                OsString::from("--reset")
            ])
            .is_err()
        );
        assert!(
            install_operation(&[
                OsString::from("--reset"),
                OsString::from("add"),
                OsString::from("serde")
            ])
            .is_err()
        );

        assert_eq!(
            install_operation(&[OsString::from("--offline"), OsString::from("--reset")]).unwrap(),
            InstallOperation {
                reset: true,
                subcommand: "fetch",
                arguments: vec![OsString::from("--offline")]
            }
        );
    }

    #[test]
    fn parses_benchmark_command_without_rewriting_its_profile_arguments() {
        let parsed = parse_arguments([
            "--project".into(),
            "/tmp/kotavel".into(),
            "benchmark".into(),
            "500".into(),
            "10".into(),
            "shared".into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(parsed.command, "benchmark");
        assert_eq!(
            parsed.arguments,
            ["500", "10", "shared"].map(OsString::from)
        );
    }

    #[test]
    fn locates_framework_and_laboratory_manifests_from_the_core_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let core_root = root.join("Kotavel/crates/core");
        let laboratory_root = root.join("Laboratory-Kotavel");
        let core = core_root.join("Cargo.toml");
        let laboratory = laboratory_root.join("Cargo.toml");
        std::fs::create_dir_all(&core_root).unwrap();
        std::fs::create_dir_all(&laboratory_root).unwrap();
        std::fs::write(&core, "[package]\nname = \"core\"\n").unwrap();
        std::fs::write(&laboratory, "[package]\nname = \"kotavel-laboratory\"\n").unwrap();

        let manifests = framework_manifests(&core_root).unwrap();
        assert_eq!(manifests, [core.clone(), laboratory.clone()]);
        assert_eq!(core_manifest(&core_root).unwrap(), core.clone());
        assert_eq!(application_manifest(&core_root).unwrap(), core.clone());
        assert_eq!(
            framework_manifests(&laboratory_root).unwrap(),
            [core.clone(), laboratory.clone()]
        );
        assert_eq!(core_manifest(&laboratory_root).unwrap(), core);
        assert_eq!(application_manifest(&laboratory_root).unwrap(), laboratory);
    }

    #[test]
    fn run_normalizes_environment_and_deep_link_options_for_the_runtime() {
        let project = Path::new("/tmp/kotavel-consumer");
        let arguments = [
            "--env".into(),
            "development".into(),
            "--deep-link=kotavel://pos/home".into(),
            "--verbose".into(),
        ];

        let runtime = runtime_arguments(project, &arguments).unwrap();

        assert_eq!(
            runtime,
            [
                "--env=development",
                "kotavel://pos/home",
                "--verbose",
                "--project-root=/tmp/kotavel-consumer"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn run_selects_one_binary_without_forwarding_the_option_to_the_application() {
        assert_eq!(
            parse_run_arguments(&["--bin".into(), "desktop-app".into(), "--verbose".into()])
                .unwrap(),
            RunArguments {
                binary: Some("desktop-app".into()),
                cargo_arguments: vec![],
                application_arguments: vec![OsString::from("--verbose")],
            }
        );
        assert!(parse_run_arguments(&["--bin=".into()]).is_err());
        assert!(parse_run_arguments(&["--bin".into()]).is_err());
    }

    #[test]
    fn run_rejects_invalid_wrapper_options_and_preserves_explicit_passthrough() {
        let project = Path::new("/tmp/kotavel-consumer");
        assert!(runtime_arguments(project, &["--deep-link".into()]).is_err());
        assert!(runtime_arguments(project, &["--env=staging".into()]).is_err());
        assert_eq!(
            runtime_arguments(
                project,
                &["--".into(), "--deep-link".into(), "literal-app-flag".into()]
            )
            .unwrap(),
            [
                "--deep-link",
                "literal-app-flag",
                "--project-root=/tmp/kotavel-consumer"
            ]
            .map(OsString::from)
        );
    }
}
