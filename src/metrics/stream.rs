//! Loopback-only JSON Lines stream used by the local metrics monitor.

use crate::{contracts::Filesystem, filesystem::LocalFilesystem};
use std::{
    io::{self, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Default)]
struct Published {
    startup: Option<String>,
    latest: Option<(u64, String)>,
    generation: u64,
}

struct Shared {
    stopping: AtomicBool,
    active: AtomicBool,
    published: Mutex<Published>,
}

/// Serves metrics as JSON Lines on an ephemeral IPv4 loopback port.
pub struct MetricsStream {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    endpoint: Option<PathBuf>,
}

impl Default for MetricsStream {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                stopping: AtomicBool::new(true),
                active: AtomicBool::new(false),
                published: Mutex::new(Published::default()),
            }),
            worker: None,
            endpoint: None,
        }
    }
}

impl MetricsStream {
    pub fn start(&mut self, project_root: impl AsRef<Path>) -> io::Result<()> {
        if self.shared.active.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "metrics stream is already running",
            ));
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let directory = project_root
            .as_ref()
            .join("storage")
            .join("framework")
            .join("metrics");
        let filesystem = LocalFilesystem;
        filesystem.create_directories(&directory, true)?;
        remove_stale_endpoints(&filesystem, &directory);

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let endpoint = directory.join(format!("kotavel-{}.endpoint", std::process::id()));
        filesystem.write_file_atomically(&endpoint, format!("{port}\n").as_bytes(), true)?;
        {
            let mut published = self
                .shared
                .published
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *published = Published::default();
        }
        self.shared.stopping.store(false, Ordering::Release);
        let shared = Arc::clone(&self.shared);
        let worker = thread::Builder::new()
            .name("kotavel-metrics-stream".into())
            .spawn(move || serve(listener, shared));
        match worker {
            Ok(worker) => {
                self.endpoint = Some(endpoint);
                self.worker = Some(worker);
                self.shared.active.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                let _ = filesystem.remove_file(&endpoint);
                self.shared.stopping.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    pub fn publish(&self, sample: &super::MetricsSample) {
        let cpu = sample
            .process_cpu_percent
            .filter(|value| value.is_finite() && *value >= 0.0);
        let message = serde_json::json!({
            "type": "sample",
            "cpu_percent": cpu,
            "resident_memory_bytes": sample.resident_memory_bytes,
        });
        let mut published = self
            .shared
            .published
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        published.generation = published.generation.wrapping_add(1);
        published.latest = Some((published.generation, format!("{message}\n")));
    }

    pub fn publish_startup(&self, metrics: &super::StartupMetrics) {
        let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let message = serde_json::json!({
            "type": "startup",
            "environment_ms": milliseconds(metrics.environment),
            "configuration_ms": milliseconds(metrics.configuration),
            "settings_ms": milliseconds(metrics.settings),
            "application_boot_ms": milliseconds(metrics.application_boot),
            "total_ms": milliseconds(metrics.total),
        });
        self.shared
            .published
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .startup = Some(format!("{message}\n"));
    }

    pub fn stop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(endpoint) = self.endpoint.take() {
            let _ = LocalFilesystem.remove_file(&endpoint);
        }
        self.shared.active.store(false, Ordering::Release);
    }

    pub fn is_running(&self) -> bool {
        self.shared.active.load(Ordering::Acquire)
    }
}

impl Drop for MetricsStream {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut client: Option<TcpStream> = None;
    let mut sent_generation = 0;
    let mut sent_startup = false;
    while !shared.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                client = Some(stream);
                sent_generation = 0;
                sent_startup = false;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }

        let Some(connection) = client.as_mut() else {
            thread::sleep(Duration::from_millis(20));
            continue;
        };
        let (startup, sample) = {
            let published = shared
                .published
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let startup = (!sent_startup).then(|| published.startup.clone()).flatten();
            let sample = published
                .latest
                .as_ref()
                .filter(|(generation, _)| *generation != sent_generation)
                .cloned();
            (startup, sample)
        };
        let mut failed = false;
        if let Some(startup) = startup {
            if connection.write_all(startup.as_bytes()).is_err() {
                failed = true;
            } else {
                sent_startup = true;
            }
        } else if !sent_startup {
            // `None` also means startup metrics have not been published yet.
            sent_startup = false;
        }
        if !failed {
            if let Some((generation, message)) = sample {
                if connection.write_all(message.as_bytes()).is_err() {
                    failed = true;
                } else {
                    sent_generation = generation;
                }
            }
        }
        if failed {
            client = None;
            sent_startup = false;
            sent_generation = 0;
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(client);
    shared.active.store(false, Ordering::Release);
}

fn remove_stale_endpoints(filesystem: &LocalFilesystem, directory: &Path) {
    let Ok(files) = filesystem.list_files(directory) else {
        return;
    };
    let current = std::process::id();
    for path in files {
        let Some(process_id) = endpoint_process_id(&path) else {
            continue;
        };
        if process_id != current && !process_is_running(process_id) {
            let _ = filesystem.remove_file(&path);
        }
    }
}

fn endpoint_process_id(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let digits = name.strip_prefix("kotavel-")?.strip_suffix(".endpoint")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|pid| *pid != 0)
}

#[cfg(unix)]
fn process_is_running(process_id: u32) -> bool {
    // SAFETY: kill with signal 0 performs only a permission/existence check.
    let result = unsafe { libc::kill(process_id as libc::pid_t, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn process_is_running(process_id: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: the handle is closed after querying the process exit code.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let running =
            GetExitCodeProcess(process, &mut exit_code) != 0 && exit_code == STILL_ACTIVE as u32;
        let _ = CloseHandle(process);
        running
    }
}

#[cfg(not(any(unix, windows)))]
fn process_is_running(_process_id: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
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
        let startup: serde_json::Value =
            serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        let sample: serde_json::Value =
            serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(startup["type"], "startup");
        assert_eq!(sample["type"], "sample");
        assert_eq!(sample["resident_memory_bytes"], 2048);
        stream.stop();
        assert!(!stream.is_running());
        assert!(!endpoint.exists());
    }
}
