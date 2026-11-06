use super::*;

#[test]
fn parses_project_and_pid_flags_with_both_forms() {
    let options = parse_arguments(["--project".into(), "/tmp/kotavel".into(), "--pid=42".into()])
        .unwrap()
        .unwrap();
    assert_eq!(options.project, PathBuf::from("/tmp/kotavel"));
    assert_eq!(options.pid, Some(42));
}

#[test]
fn rejects_invalid_pid_and_unknown_options() {
    assert!(parse_arguments(["--pid=0".into()]).is_err());
    assert!(parse_arguments(["--pid=nope".into()]).is_err());
    assert!(parse_arguments(["--unexpected".into()]).is_err());
}

#[test]
fn renders_startup_sample_and_null_cpu_events() {
    let startup = serde_json::json!({
        "type":"startup", "environment_ms":1.0, "configuration_ms":2.0,
        "settings_ms":3.0, "application_boot_ms":4.0, "total_ms":10.0
    });
    let mut output = Vec::new();
    render_event(&startup, &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("total 10.00 ms")
    );

    let sample = serde_json::json!({"type":"sample", "cpu_percent":null,
            "resident_memory_bytes":1048576});
    let mut output = Vec::new();
    render_event(&sample, &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("CPU: calculando"));
    assert!(output.contains("RAM residente: 1.0 MiB"));
}

#[test]
fn connects_only_to_the_requested_loopback_endpoint() {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = env::temp_dir().join(format!("kotavel-monitor-{unique}"));
    let directory = project.join("storage/framework/metrics");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("kotavel-321.endpoint"), port.to_string()).unwrap();
    fs::write(directory.join("kotavel-999.endpoint"), "not-a-port").unwrap();

    let stream = connect(&project, Some(321)).unwrap();
    assert_eq!(stream.peer_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
    assert_eq!(
        listener.accept().unwrap().0.peer_addr().unwrap().ip(),
        Ipv4Addr::LOCALHOST
    );

    drop(stream);
    fs::remove_dir_all(project).unwrap();
}
