use std::{
    env,
    ffi::OsString,
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
    let project = options
        .project
        .canonicalize()
        .map_err(|error| format!("no se pudo abrir el proyecto: {error}"))?;

    match options.command.as_str() {
        "prepare" => prepare(&project),
        "install" => {
            let manifest = application_manifest(&project)?;
            let operation = install_operation(&options.arguments)?;
            if operation.reset {
                let code = cargo(&manifest, "clean", &[])?;
                if code != 0 {
                    return Ok(code);
                }
            }
            cargo(&manifest, operation.subcommand, &operation.arguments)
        }
        "test" => {
            let framework = options.arguments.iter().any(|arg| arg == "--framework");
            let arguments = options
                .arguments
                .iter()
                .filter(|arg| *arg != "--framework")
                .cloned()
                .collect::<Vec<_>>();
            if framework {
                let manifests = framework_manifests(&project)?;
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
            } else {
                cargo(&application_manifest(&project)?, "test", &options.arguments)
            }
        }
        "build" => cargo(
            &application_manifest(&project)?,
            "build",
            &options.arguments,
        ),
        "package" => kotavel::console::package_project(
            &project,
            &application_manifest(&project)?,
            &options.arguments,
        ),
        "run" => {
            let manifest = application_manifest(&project)?;
            let (binary, mut cargo_arguments, application_arguments) =
                parse_run_arguments(&options.arguments)?;
            if is_laboratory_manifest(&manifest)
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
            let runtime_arguments = runtime_arguments(&project, &application_arguments)?;
            let mut command = Command::new("cargo");
            command.arg("run").arg("--manifest-path").arg(manifest);
            if let Some(binary) = binary {
                command.arg("--bin").arg(binary);
            }
            command.args(cargo_arguments).arg("--");
            command.args(runtime_arguments).current_dir(&project);
            execute(command)
        }
        "shell" => {
            if !options.arguments.is_empty() {
                return Err("shell no acepta argumentos adicionales".into());
            }
            let code = prepare(&project)?;
            if code == 0 {
                enter_shell(&project)
            } else {
                Ok(code)
            }
        }
        "monitor" | "stop" => {
            let manifest = core_manifest(&project)?;
            let binary = if options.command == "monitor" {
                "kotavel-monitor"
            } else {
                "kotavel-stop"
            };
            let mut command = Command::new("cargo");
            command
                .arg("run")
                .arg("--manifest-path")
                .arg(manifest)
                .arg("--bin")
                .arg(binary)
                .arg("--")
                .arg("--project")
                .arg(project);
            command.args(&options.arguments);
            execute(command)
        }
        "benchmark" => {
            let manifest = core_manifest(&project)?;
            let release = options
                .arguments
                .iter()
                .any(|argument| argument == "--release");
            let mut command = Command::new("cargo");
            command.arg("run");
            if release {
                command.arg("--release");
            }
            command
                .arg("--manifest-path")
                .arg(manifest)
                .arg("--bin")
                .arg("kotavel-session-benchmark")
                .arg("--");
            command.args(
                options
                    .arguments
                    .iter()
                    .filter(|argument| *argument != "--release"),
            );
            execute(command)
        }
        _ => unreachable!("the parser validates command names"),
    }
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
        project.join("../Kotavel/Cargo.toml"),
    ];
    candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .find(|candidate| {
            std::fs::read_to_string(candidate)
                .map(|manifest| {
                    manifest.lines().any(|line| {
                        line.trim() == "name = \"kotavel\"" || line.trim() == "name=\'kotavel\'"
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
    let mut command = Command::new("cargo");
    command
        .arg(subcommand)
        .arg("--manifest-path")
        .arg(manifest)
        .args(arguments);
    execute(command)
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
    for (program, version_flag) in [("rustc", "--version"), ("cargo", "--version")] {
        let status = Command::new(program)
            .arg(version_flag)
            .status()
            .map_err(|error| format!("no se pudo iniciar {program}: {error}"))?;
        if !status.success() {
            return Ok(exit_code(status));
        }
    }

    let status = Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version=1")
        .arg("--manifest-path")
        .arg(&manifest)
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("no se pudo validar el workspace Cargo: {error}"))?;
    if status.success() {
        println!("Workspace Rust listo: {}", manifest.display());
    }
    Ok(exit_code(status))
}

fn execute(mut command: Command) -> Result<i32, String> {
    let status = command
        .status()
        .map_err(|error| format!("no se pudo iniciar el proceso: {error}"))?;
    Ok(exit_code(status))
}

fn enter_shell(project: &Path) -> Result<i32, String> {
    kotavel::console::run_interactive_shell(project).map_err(|error| error.to_string())
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

fn parse_run_arguments(
    arguments: &[OsString],
) -> Result<(Option<OsString>, Vec<OsString>, Vec<OsString>), String> {
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
    Ok((binary, cargo_arguments, application_arguments))
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
        "Kotavel · herramientas Rust\n\
         Uso: cargo run --bin kotavel -- [--project RUTA] <comando> [opciones]\n\
         Comandos: prepare, install, test [--framework], build, package, run, shell, monitor, stop, benchmark\n\
         prepare valida rustc, Cargo y el manifiesto; install descarga, --reset limpia artefactos, add crate@versión añade una dependencia.\n\
         --framework prueba el núcleo y el Laboratorio Rust.\n\
         run admite --bin NOMBRE, --features FEATURES, --release, --no-default-features, --env local|development|production y --deep-link URI; activa Qt por defecto en Laboratory-Kotavel.\n\
         package compila un binario Cargo y genera un bundle macOS (.app, .dmg, .pkg).\n\
         shell abre una terminal interactiva en la raíz del proyecto.\n\
         benchmark --release usa el perfil optimizado para la medición de sesiones."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let core_root = root.join("Kotavel");
        let laboratory_root = root.join("Laboratory-Kotavel");
        let core = core_root.join("Cargo.toml");
        let laboratory = laboratory_root.join("Cargo.toml");
        std::fs::create_dir_all(&core_root).unwrap();
        std::fs::create_dir_all(&laboratory_root).unwrap();
        std::fs::write(&core, "[package]\nname = \"kotavel\"\n").unwrap();
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
            (
                Some("desktop-app".into()),
                vec![],
                vec![OsString::from("--verbose")]
            )
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
