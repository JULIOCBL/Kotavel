//! Native process sampler selected by target OS.

use super::{ProcessMetricsProvider, ProcessMetricsSnapshot};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeProcessMetricsProvider;

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ProcessMetricsProvider for NativeProcessMetricsProvider {
    fn read(&self) -> Option<ProcessMetricsSnapshot> {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: getrusage initializes the supplied `rusage` on success.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: the successful call above initialized the structure.
        let usage = unsafe { usage.assume_init() };
        let cpu_time = timeval_duration(usage.ru_utime)? + timeval_duration(usage.ru_stime)?;
        let resident_memory_bytes = resident_memory_bytes()?;
        Some(ProcessMetricsSnapshot {
            cpu_time,
            resident_memory_bytes,
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn timeval_duration(value: libc::timeval) -> Option<Duration> {
    let seconds = u64::try_from(value.tv_sec).ok()?;
    let micros = u32::try_from(value.tv_usec).ok()?;
    if micros >= 1_000_000 {
        return None;
    }
    Some(Duration::from_secs(seconds) + Duration::from_micros(micros.into()))
}

#[cfg(target_os = "linux")]
fn resident_memory_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages = statm.split_ascii_whitespace().nth(1)?.parse::<u64>().ok()?;
    // SAFETY: sysconf has no pointer arguments and the selector is valid on Linux.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    resident_pages.checked_mul(page_size as u64)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc exposes the same Mach call as deprecated; machine-specific isolation stays in this adapter.
fn resident_memory_bytes() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::zeroed();
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: the Mach API writes the declared basic-info structure into `info`.
    let status = unsafe {
        libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return None;
    }
    // SAFETY: `task_info` returned KERN_SUCCESS.
    let info = unsafe { info.assume_init() };
    Some(info.resident_size)
}

#[cfg(target_os = "windows")]
impl ProcessMetricsProvider for NativeProcessMetricsProvider {
    fn read(&self) -> Option<ProcessMetricsSnapshot> {
        use windows_sys::Win32::{
            Foundation::FILETIME,
            System::{
                ProcessStatus::{
                    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
                },
                Threading::{GetCurrentProcess, GetProcessTimes},
            },
        };

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: all output pointers refer to initialized, writable FILETIME values.
        if unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return None;
        }
        let mut memory = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        // SAFETY: memory is large enough for PROCESS_MEMORY_COUNTERS_EX, whose prefix matches the base type.
        if unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                memory.cb,
            )
        } == 0
        {
            return None;
        }
        let ticks = filetime_ticks(kernel).checked_add(filetime_ticks(user))?;
        let cpu_nanos = ticks.checked_mul(100)?;
        Some(ProcessMetricsSnapshot {
            cpu_time: Duration::from_nanos(cpu_nanos),
            resident_memory_bytes: memory.WorkingSetSize as u64,
        })
    }
}

#[cfg(target_os = "windows")]
fn filetime_ticks(value: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl ProcessMetricsProvider for NativeProcessMetricsProvider {
    fn read(&self) -> Option<ProcessMetricsSnapshot> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sampler_reports_nonzero_resident_memory() {
        let Some(sample) = NativeProcessMetricsProvider.read() else {
            panic!("native process metrics unavailable on this target");
        };
        assert!(sample.resident_memory_bytes > 0);
        assert!(sample.cpu_time <= Duration::from_secs(365 * 24 * 60 * 60));
    }
}
