//! 基准用的进程内存读数：当前常驻内存和峰值，单位 MiB。
//!
//! Process memory readings for the benchmarks: current and peak resident
//! memory in MiB.

/// `(当前工作集, 峰值工作集)`，读不到时是 `(0.0, 0.0)`。
///
/// `(current working set, peak working set)`; `(0.0, 0.0)` when unavailable.
#[cfg(windows)]
pub fn process_memory_mb() -> (f64, f64) {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // 当前进程的伪句柄无需释放。
    // The current process pseudo-handle does not need closing.
    if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, size) } == 0 {
        return (0.0, 0.0);
    }
    let mb = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
    (mb(counters.WorkingSetSize), mb(counters.PeakWorkingSetSize))
}

/// Unix 上只有峰值（`ru_maxrss`：Linux 是 KiB，macOS 是字节）；当前值填 0。
///
/// Unix only reports the peak (`ru_maxrss`: KiB on Linux, bytes on macOS); the
/// current value is 0.
#[cfg(unix)]
pub fn process_memory_mb() -> (f64, f64) {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) } != 0 {
        return (0.0, 0.0);
    }
    let unit = if cfg!(target_os = "macos") {
        1024.0 * 1024.0
    } else {
        1024.0
    };
    (0.0, usage.ru_maxrss as f64 / unit)
}
