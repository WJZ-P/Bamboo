//! 基准用的进程 CPU 计数，不统计等待时间。
//!
//! Process CPU counters for the benchmarks, excluding elapsed wait time.

#[cfg(unix)]
pub fn process_cpu_ms() -> f64 {
    // 输出结构由 libc 提供，避免手写各平台 timeval 和 rusage 的布局。
    // Use libc's native timeval and rusage ABI rather than guessing field layouts.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) } != 0 {
        return 0.0;
    }
    let user = usage.ru_utime;
    let system = usage.ru_stime;
    (user.tv_sec as f64 + system.tv_sec as f64) * 1000.0
        + (user.tv_usec as f64 + system.tv_usec as f64) / 1000.0
}

#[cfg(windows)]
pub fn process_cpu_ms() -> f64 {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    filetime_cpu_ms(|created, exited, kernel, user| {
        // 当前进程的伪句柄无需释放。
        // The current process pseudo-handle does not need closing.
        unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }
    })
}

#[cfg(windows)]
pub fn filetime_cpu_ms(
    read: impl FnOnce(
        &mut windows_sys::Win32::Foundation::FILETIME,
        &mut windows_sys::Win32::Foundation::FILETIME,
        &mut windows_sys::Win32::Foundation::FILETIME,
        &mut windows_sys::Win32::Foundation::FILETIME,
    ) -> i32,
) -> f64 {
    use windows_sys::Win32::Foundation::FILETIME;
    let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
    if read(&mut created, &mut exited, &mut kernel, &mut user) == 0 {
        return 0.0;
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    // FILETIME 的 CPU 计数单位为 100 ns。
    // FILETIME CPU counts are in 100 ns units.
    (ticks(kernel) as f64 + ticks(user) as f64) / 10_000.0
}
