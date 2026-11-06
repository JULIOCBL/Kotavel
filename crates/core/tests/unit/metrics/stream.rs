use super::*;
use crate::metrics::{MetricsSample, StartupMetrics};
use std::{
    io::{BufRead, BufReader},
    time::Instant,
};

#[test]
fn loopback_stream_publishes_startup_and_samples_and_removes_its_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = root
        .path()
        .join("storage/framework/metrics")
        .join(format!("kotavel-{}.endpoint", std::process::id()));
    let mut stream = MetricsStream::default();
    stream.start(root.path()).unwrap();
    assert!(stream.is_running());
    stream.publish_startup(&StartupMetrics {
        environment: Duration::from_millis(1),
        ..StartupMetrics::default()
    });
    stream.publish(&MetricsSample {
        captured_at: Instant::now(),
        process_cpu_percent: Some(3.5),
        resident_memory_bytes: 2048,
    });

    let port = std::fs::read_to_string(&endpoint)
        .unwrap()
        .trim()
        .parse::<u16>()
        .unwrap();
    let connection = TcpStream::connect(("127.0.0.1", port)).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut lines = BufReader::new(connection).lines();
    let startup: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let sample: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(startup["type"], "startup");
    assert_eq!(sample["type"], "sample");
    assert_eq!(sample["resident_memory_bytes"], 2048);
    stream.stop();
    assert!(!stream.is_running());
    assert!(!endpoint.exists());
}
