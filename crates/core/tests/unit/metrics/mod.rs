use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct FakeProvider(AtomicU64);
impl ProcessMetricsProvider for FakeProvider {
    fn read(&self) -> Option<ProcessMetricsSnapshot> {
        Some(ProcessMetricsSnapshot {
            cpu_time: Duration::from_millis(self.0.fetch_add(100, Ordering::Relaxed)),
            resident_memory_bytes: 4096,
        })
    }
}

#[test]
fn collector_uses_delta_cpu_and_reports_memory() {
    let provider = Arc::new(FakeProvider(AtomicU64::new(0)));
    let mut collector = ProcessMetricsCollector::new(provider);
    let first = collector.sample().unwrap();
    assert_eq!(first.process_cpu_percent, None);
    let second = collector.sample().unwrap();
    assert!(
        second
            .process_cpu_percent
            .is_some_and(|percent| percent >= 0.0)
    );
    assert_eq!(second.resident_memory_bytes, 4096);
}

#[test]
fn monitor_rejects_interval_outside_contract_and_stops_cleanly() {
    let monitor = MetricsMonitor::default();
    let provider = Arc::new(FakeProvider(AtomicU64::new(0)));
    let collector = ProcessMetricsCollector::new(provider);
    assert_eq!(
        monitor.start(Duration::from_millis(99), collector, |_| {}),
        Err(MetricsMonitorError::InvalidInterval)
    );
    let provider = Arc::new(FakeProvider(AtomicU64::new(0)));
    monitor
        .start(
            Duration::from_millis(100),
            ProcessMetricsCollector::new(provider),
            |_| {},
        )
        .unwrap();
    assert!(monitor.is_running());
    monitor.stop();
    assert!(!monitor.is_running());
}
