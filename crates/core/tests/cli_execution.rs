#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::TempDir;

struct CliFixture {
    _directory: TempDir,
    core: PathBuf,
    laboratory: PathBuf,
    fake_path: PathBuf,
    capture: PathBuf,
}

impl CliFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let core = root.join("Kotavel/crates/core");
        let laboratory = root.join("Laboratory-Kotavel");
        let fake_path = root.join("fake-path");
        fs::create_dir_all(&core).unwrap();
        fs::create_dir_all(&laboratory).unwrap();
        fs::create_dir_all(&fake_path).unwrap();
        fs::write(
            root.join("Kotavel/Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"crates/platform\"]\n",
        )
        .unwrap();
        fs::write(core.join("Cargo.toml"), "[package]\nname = \"core\"\n").unwrap();
        fs::write(
            laboratory.join("Cargo.toml"),
            "[package]\nname = \"kotavel-laboratory\"\n",
        )
        .unwrap();

        let capture = root.join("cargo-arguments.bin");
        write_executable(
            &fake_path.join("cargo"),
            "#!/bin/sh\nprintf '\\036' >> \"$KOTAVEL_TEST_CAPTURE\"\nfor arg do printf '%s\\000' \"$arg\" >> \"$KOTAVEL_TEST_CAPTURE\"; done\nexit \"${KOTAVEL_TEST_EXIT:-0}\"\n",
        );
        write_executable(&fake_path.join("rustc"), "#!/bin/sh\nexit 0\n");

        Self {
            _directory: directory,
            core,
            laboratory,
            fake_path,
            capture,
        }
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kotavel"))
            .arg("--project")
            .arg(&self.laboratory)
            .args(arguments)
            .env("PATH", &self.fake_path)
            .env("KOTAVEL_TEST_CAPTURE", &self.capture)
            .output()
            .unwrap()
    }

    fn calls(&self) -> Vec<Vec<String>> {
        let bytes = fs::read(&self.capture).unwrap_or_default();
        bytes
            .split(|byte| *byte == 0x1e)
            .filter(|call| !call.is_empty())
            .map(|call| {
                call.split(|byte| *byte == 0)
                    .filter(|argument| !argument.is_empty())
                    .map(|argument| String::from_utf8(argument.to_vec()).unwrap())
                    .collect()
            })
            .collect()
    }

    fn clear_calls(&self) {
        let _ = fs::remove_file(&self.capture);
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn run_executes_cargo_with_the_laboratory_application_binary_selected() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["run", "--no-default-features"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.calls(),
        [vec![
            "run".into(),
            "--manifest-path".into(),
            fixture.laboratory.join("Cargo.toml").display().to_string(),
            "--bin".into(),
            "kotavel-laboratory".into(),
            "--no-default-features".into(),
            "--".into(),
            format!("--project-root={}", fixture.laboratory.display()),
        ]]
    );
}

#[test]
fn run_honors_an_explicit_binary_instead_of_the_laboratory_default() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["run", "--bin", "kotavel-lab-cli", "--no-default-features"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.calls()[0][4], "kotavel-lab-cli");
}

#[test]
fn install_test_and_build_execute_the_expected_cargo_subcommands() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["install", "--offline"]);
    assert!(output.status.success());
    assert_eq!(fixture.calls()[0][0], "fetch");
    assert_eq!(
        fixture.calls()[0][2],
        fixture.laboratory.join("Cargo.toml").display().to_string()
    );

    fixture.clear_calls();
    let output = fixture.invoke(&["test", "--framework"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let workspace = fixture
        .core
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml");
    assert_eq!(
        fixture.calls(),
        [
            vec![
                "test".into(),
                "--manifest-path".into(),
                workspace.display().to_string(),
                "--workspace".into(),
            ],
            vec![
                "test".into(),
                "--manifest-path".into(),
                fixture.laboratory.join("Cargo.toml").display().to_string(),
            ],
        ]
    );

    fixture.clear_calls();
    let output = fixture.invoke(&["build", "--release"]);
    assert!(output.status.success());
    assert_eq!(
        fixture.calls(),
        [vec![
            "build".into(),
            "--manifest-path".into(),
            fixture.laboratory.join("Cargo.toml").display().to_string(),
            "--release".into(),
        ]]
    );
}

#[test]
fn prepare_validates_versions_then_the_laboratory_manifest() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["prepare"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.calls(),
        [
            vec!["--version".into()],
            vec![
                "metadata".into(),
                "--no-deps".into(),
                "--format-version=1".into(),
                "--manifest-path".into(),
                fixture.laboratory.join("Cargo.toml").display().to_string(),
            ],
        ]
    );
}

#[test]
fn package_rejects_an_invalid_format_before_starting_packaging_tools() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["package", "--format", "unknown"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--format acepta"));
    assert!(fixture.calls().is_empty());
}

#[test]
fn shell_runs_preflight_and_refuses_non_interactive_execution_without_hanging() {
    let fixture = CliFixture::new();
    let output = fixture.invoke(&["shell"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("terminal interactiva"));
    assert_eq!(
        fixture.calls(),
        [
            vec!["--version".into()],
            vec![
                "metadata".into(),
                "--no-deps".into(),
                "--format-version=1".into(),
                "--manifest-path".into(),
                fixture.laboratory.join("Cargo.toml").display().to_string(),
            ],
        ]
    );
}

#[test]
fn monitor_stop_and_benchmark_execute_core_binaries_scoped_to_the_laboratory() {
    let fixture = CliFixture::new();
    for (command, binary) in [("monitor", "kotavel-monitor"), ("stop", "kotavel-stop")] {
        let output = fixture.invoke(&[command, "--pid", "123"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fixture.calls()[0],
            [
                "run".to_owned(),
                "--manifest-path".to_owned(),
                fixture.core.join("Cargo.toml").display().to_string(),
                "--bin".to_owned(),
                binary.to_owned(),
                "--".to_owned(),
                "--project".to_owned(),
                fixture.laboratory.display().to_string(),
                "--pid".to_owned(),
                "123".to_owned(),
            ]
        );
        fixture.clear_calls();
    }

    let output = fixture.invoke(&["benchmark", "--release", "100", "5", "shared"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fixture.calls()[0],
        [
            "run".to_owned(),
            "--release".to_owned(),
            "--manifest-path".to_owned(),
            fixture.core.join("Cargo.toml").display().to_string(),
            "--bin".to_owned(),
            "kotavel-session-benchmark".to_owned(),
            "--".to_owned(),
            "100".to_owned(),
            "5".to_owned(),
            "shared".to_owned(),
        ]
    );
}

#[test]
fn failed_child_process_status_is_returned_to_the_caller() {
    let fixture = CliFixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_kotavel"))
        .arg("--project")
        .arg(&fixture.laboratory)
        .args(["build"])
        .env("PATH", &fixture.fake_path)
        .env("KOTAVEL_TEST_CAPTURE", &fixture.capture)
        .env("KOTAVEL_TEST_EXIT", "23")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(23));
}
