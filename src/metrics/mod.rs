//! Process metrics collection and an optional periodic monitor.
//!
//! Platform-specific sampling is injected through [`ProcessMetricsProvider`],
//! keeping the framework API independent from operating-system APIs.

mod native;
mod stream;

pub use native::NativeProcessMetricsProvider;
pub use stream::MetricsStream;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Raw process values read from a platform adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessMetricsSnapshot {
    pub cpu_time: Duration,
    pub resident_memory_bytes: u64,
}

/// Platform adapter used by [`ProcessMetricsCollector`].
pub trait ProcessMetricsProvider: Send + Sync + 'static {
    fn read(&self) -> Option<ProcessMetricsSnapshot>;
}

/// A point-in-time process sample. CPU may exceed 100% on multi-core systems.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricsSample {
    pub captured_at: Instant,
    pub process_cpu_percent: Option<f64>,
    pub resident_memory_bytes: u64,
}

/// Calculates CPU utilization from consecutive snapshots from one process.
pub struct ProcessMetricsCollector {
    provider: Arc<dyn ProcessMetricsProvider>,
    previous: Option<(Duration, Instant)>,
}

impl ProcessMetricsCollector {
    pub fn new(provider: Arc<dyn ProcessMetricsProvider>) -> Self {
        Self {
            provider,
            previous: None,
        }
    }

    pub fn sample(&mut self) -> Option<MetricsSample> {
        let current = catch_unwind(AssertUnwindSafe(|| self.provider.read())).ok()??;
        let now = Instant::now();
        let cpu = self.previous.and_then(|(previous_cpu, previous_wall)| {
            let wall_delta = now.checked_duration_since(previous_wall)?;
            let cpu_delta = current.cpu_time.checked_sub(previous_cpu)?;
            (wall_delta > Duration::ZERO)
                .then(|| cpu_delta.as_secs_f64() * 100.0 / wall_delta.as_secs_f64())
        });
        self.previous = Some((current.cpu_time, now));
        Some(MetricsSample {
            captured_at: now,
            process_cpu_percent: cpu.filter(|value| value.is_finite() && *value >= 0.0),
            resident_memory_bytes: current.resident_memory_bytes,
        })
    }
}

/// Durations captured during application startup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupMetrics {
    pub environment: Duration,
    pub configuration: Duration,
    pub settings: Duration,
    pub application_boot: Duration,
    pub total: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricsOptions {
    pub enabled: bool,
    pub interval: Duration,
    pub console: bool,
    pub startup: bool,
    pub stream: bool,
}

impl Default for MetricsOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: Duration::from_secs(1),
            console: true,
            startup: true,
            stream: false,
        }
    }
}

type SharedStop = Arc<(Mutex<bool>, Condvar)>;
struct MonitorState {
    worker: Option<JoinHandle<()>>,
    stop: SharedStop,
}

/// Calls back with process measurements on a dedicated worker thread.
/// Callback and collection failures never unwind through the caller.
#[derive(Default)]
pub struct MetricsMonitor {
    state: Mutex<MonitorState>,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            worker: None,
            stop: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }
}

impl MetricsMonitor {
    pub fn start<F>(
        &self,
        interval: Duration,
        collector: ProcessMetricsCollector,
        callback: F,
    ) -> Result<(), MetricsMonitorError>
    where
        F: Fn(MetricsSample) + Send + 'static,
    {
        if !(Duration::from_millis(100)..=Duration::from_secs(60)).contains(&interval) {
            return Err(MetricsMonitorError::InvalidInterval);
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return Err(MetricsMonitorError::AlreadyRunning);
        }
        if let Some(old) = state.worker.take() {
            let _ = old.join();
        }
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("kotavel-metrics".into())
            .spawn(move || {
                let (lock, wake) = &*worker_stop;
                let mut stopped = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut collector = collector;
                loop {
                    let (next, _) = wake
                        .wait_timeout(stopped, interval)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    stopped = next;
                    if *stopped {
                        break;
                    }
                    drop(stopped);
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        if let Some(sample) = collector.sample() {
                            callback(sample);
                        }
                    }));
                    stopped = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            })
            .map_err(|_| MetricsMonitorError::ThreadSpawn)?;
        state.stop = stop;
        state.worker = Some(worker);
        Ok(())
    }

    pub fn stop(&self) {
        let worker = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (lock, wake) = &*state.stop;
            *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            wake.notify_all();
            if state
                .worker
                .as_ref()
                .is_some_and(|worker| worker.thread().id() == thread::current().id())
            {
                return;
            }
            state.worker.take()
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    pub fn is_running(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }
}

impl Drop for MetricsMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricsMonitorError {
    InvalidInterval,
    AlreadyRunning,
    ThreadSpawn,
}

impl std::fmt::Display for MetricsMonitorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInterval => "metrics sampling interval must be between 100 ms and 60 s",
            Self::AlreadyRunning => "metrics monitor is already running",
            Self::ThreadSpawn => "failed to start metrics worker thread",
        })
    }
}
impl std::error::Error for MetricsMonitorError {}

#[cfg(test)]
mod tests {
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
}
