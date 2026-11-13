use super::*;

fn laboratory_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let core = root.join("Kotavel/crates/core");
    let laboratory = root.join("Laboratory-Kotavel");
    std::fs::create_dir_all(&core).unwrap();
    std::fs::create_dir_all(&laboratory).unwrap();
    std::fs::write(
        root.join("Kotavel/Cargo.toml"),
        "[workspace]\nmembers = [\"crates/core\", \"crates/platform\"]\n",
    )
    .unwrap();
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
fn test_plans_workspace_and_laboratory_tests_from_the_laboratory_entrypoint() {
    let (_directory, root, _core, laboratory) = laboratory_fixture();
    let workspace = root.join("Kotavel/Cargo.toml");
    assert_eq!(
        plan_command(
            options("test", &["--framework", "--", "--nocapture"]),
            laboratory.clone()
        )
        .unwrap(),
        CommandPlan::Test {
            manifests: vec![workspace.clone(), laboratory.join("Cargo.toml")],
            arguments: vec!["--".into(), "--nocapture".into()],
        }
    );
    let args = ["--".into(), "--nocapture".into()];
    assert_eq!(
        [
            ProcessSpec::cargo(
                &workspace,
                "test",
                &["--workspace".into(), "--".into(), "--nocapture".into()]
            ),
            ProcessSpec::cargo(&laboratory.join("Cargo.toml"), "test", &args),
        ],
        [
            ProcessSpec::new(
                "cargo",
                [
                    "test".into(),
                    "--manifest-path".into(),
                    workspace.into_os_string(),
                    "--workspace".into(),
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
fn test_without_framework_targets_only_the_laboratory_workspace() {
    let (_directory, _root, _core, laboratory) = laboratory_fixture();
    let manifest = laboratory.join("Cargo.toml");
    assert_eq!(
        plan_command(options("test", &[]), laboratory).unwrap(),
        CommandPlan::Test {
            manifests: vec![manifest.clone()],
            arguments: Vec::new(),
        }
    );
    assert_eq!(
        ProcessSpec::cargo(&manifest, "test", &["--workspace".into()]),
        ProcessSpec::new(
            "cargo",
            [
                "test".into(),
                "--manifest-path".into(),
                manifest.into_os_string(),
                "--workspace".into(),
            ],
        )
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
fn run_selects_the_laboratory_binary_and_platform_default_feature() {
    let (_directory, _root, _core, laboratory) = laboratory_fixture();
    let desktop_arguments = if cfg!(any(target_os = "macos", target_os = "windows")) {
        vec![OsString::from("--features"), OsString::from("desktop-qt")]
    } else {
        Vec::new()
    };
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
            cargo_arguments: desktop_arguments.clone(),
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
            &desktop_arguments,
            &[
                "--env=development".into(),
                format!("--project-root={}", laboratory.display()).into(),
            ],
            &laboratory,
        ),
        ProcessSpec {
            program: "cargo".into(),
            arguments: {
                let mut arguments = vec![
                    "run".into(),
                    "--manifest-path".into(),
                    laboratory.join("Cargo.toml").into_os_string(),
                    "--bin".into(),
                    "kotavel-laboratory".into(),
                ];
                arguments.extend(desktop_arguments);
                arguments.extend([
                    "--".into(),
                    "--env=development".into(),
                    format!("--project-root={}", laboratory.display()).into(),
                ]);
                arguments
            },
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
    let workspace = root.join("Kotavel/Cargo.toml");
    let laboratory_root = root.join("Laboratory-Kotavel");
    let core = core_root.join("Cargo.toml");
    let laboratory = laboratory_root.join("Cargo.toml");
    std::fs::create_dir_all(&core_root).unwrap();
    std::fs::create_dir_all(&laboratory_root).unwrap();
    std::fs::write(
        &workspace,
        "[workspace]\nmembers = [\"crates/core\", \"crates/platform\"]\n",
    )
    .unwrap();
    std::fs::write(&core, "[package]\nname = \"core\"\n").unwrap();
    std::fs::write(&laboratory, "[package]\nname = \"kotavel-laboratory\"\n").unwrap();

    let manifests = framework_manifests(&core_root).unwrap();
    assert_eq!(manifests, [workspace.clone(), laboratory.clone()]);
    assert_eq!(core_manifest(&core_root).unwrap(), core.clone());
    assert_eq!(application_manifest(&core_root).unwrap(), core.clone());
    assert_eq!(
        framework_manifests(&laboratory_root).unwrap(),
        [workspace, laboratory.clone()]
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
        parse_run_arguments(&["--bin".into(), "desktop-app".into(), "--verbose".into()]).unwrap(),
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
