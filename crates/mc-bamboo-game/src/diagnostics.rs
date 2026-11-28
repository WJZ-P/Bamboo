//! 日志归档和崩溃报告的文件机制，放在这一层是为了让世界线程的判死点也能直接写报告。
//!
//! 这里只做同步的文件读写：panic 钩子和判死点都在自己的线程上，不碰世界状态、不取锁拿
//! 在线玩家位置。文件日志本身的非阻塞写入在 `mc-bamboo-server::logging`。
//!
//! File mechanics for log archiving and crash reports. This lives at this layer
//! so the world thread's island-death point can write a report directly. Only
//! synchronous file I/O happens here: the panic hook and the death point run on
//! their own threads, never touch world state, and never take a lock to fetch
//! online player positions. The non-blocking file *log* writer lives in
//! `mc-bamboo-server::logging`.

use std::{
    backtrace::Backtrace,
    fs,
    io::Write,
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// 崩溃报告目录和版本信息，启动时由 `mc-bamboo-server::logging` 配置一次。
///
/// The crash-report directory and build info, configured once at startup by
/// `mc-bamboo-server::logging`.
struct CrashState {
    dir: PathBuf,
    build_info: String,
}

static STATE: OnceLock<CrashState> = OnceLock::new();

/// 启动时配置崩溃报告目录并装 panic 钩子。重复调用只有第一次生效。
///
/// Configure the crash-report directory and install the panic hook at startup.
/// Only the first call takes effect.
pub fn configure(dir: &Path, build_info: &str) {
    let _ = fs::create_dir_all(dir);
    let _ = STATE.set(CrashState {
        dir: dir.to_path_buf(),
        build_info: build_info.to_owned(),
    });
    install_panic_hook();
}

/// 装 panic 钩子：panic 时写一份崩溃报告，然后照常展开/退出。
///
/// Install the panic hook: a panic writes a crash report, then unwinds/exits as
/// usual.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| write_panic_report(info)));
}

/// 把上一份 `logs/latest.log` 按官方 Log4j 的命名归档成 `logs/YYYY-MM-DD-N.log.gz`。
/// 日期用启动时的本地日期，N 取当天还没用过的最小正整数。gzip 用工作区已有的
/// `flate2`；压缩或写入失败就原样留着 `latest.log`，不丢日志。
///
/// Archive the previous `logs/latest.log` as `logs/YYYY-MM-DD-N.log.gz`, matching
/// the official Log4j naming. The date is the local date at startup and N is the
/// smallest positive integer not yet used that day. Gzip uses the workspace's
/// existing `flate2`; on any failure `latest.log` is left untouched so no log is
/// lost.
pub fn archive_latest_log(logs_dir: &Path) {
    let latest = logs_dir.join("latest.log");
    let Ok(bytes) = fs::read(&latest) else {
        return;
    };
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut index = 1usize;
    let destination = loop {
        let candidate = logs_dir.join(format!("{date}-{index}.log.gz"));
        if !candidate.exists() {
            break candidate;
        }
        index += 1;
    };
    let Ok(file) = fs::File::create(&destination) else {
        return;
    };
    let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    if encoder.write_all(&bytes).is_ok()
        && encoder.finish().is_ok()
    {
        let _ = fs::remove_file(&latest);
    } else {
        let _ = fs::remove_file(&destination);
    }
}

/// 岛判死时写一份 `crash-reports/island-dead-*.txt`：`log_body` 是判死日志的内容，
/// 后面附上所有岛的信标。
///
/// Write a `crash-reports/island-dead-*.txt` when an island is declared dead:
/// `log_body` is the death log's content, followed by every island's beacon.
pub fn write_island_dead_report(log_body: &str) {
    let mut body = String::from("---- Bamboo Island Death Report ----\n\n");
    body.push_str(&format!("Time: {}\n", timestamp()));
    body.push_str(&format!("Build: {}\n\n", build_info()));
    body.push_str(log_body);
    body.push_str("\n\n-- All island progress beacons --\n");
    push_beacons(&mut body);
    write_report("island-dead", &body);
}

/// panic 钩子的主体：收集消息、位置、线程名、backtrace、版本和信标，写报告。
///
/// The panic hook body: gather message, location, thread name, backtrace, build
/// info and beacons, then write the report.
fn write_panic_report(info: &PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>");
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".to_owned());
    // force_capture 无视 RUST_BACKTRACE，崩溃报告里总有一份回溯。
    // force_capture ignores RUST_BACKTRACE, so the report always has a trace.
    let backtrace = Backtrace::force_capture();

    let mut body = String::from("---- Bamboo Crash Report ----\n\n");
    body.push_str(&format!("Time: {}\n", timestamp()));
    body.push_str(&format!("Build: {}\n", build_info()));
    body.push_str(&format!("Thread: {thread_name}\n"));
    body.push_str(&format!("Location: {location}\n"));
    body.push_str(&format!("Message: {message}\n\n"));
    body.push_str("-- Backtrace --\n");
    body.push_str(&format!("{backtrace}\n"));
    body.push_str("\n-- Island progress beacons --\n");
    push_beacons(&mut body);
    body.push_str("\n-- Online player positions --\n");
    // 世界状态只归世界线程所有，panic 钩子拿不到位置，也不为报告去加锁冒险。
    // World state belongs to the world thread alone; the panic hook cannot see
    // positions and never takes a lock just to write a report.
    body.push_str(
        "(not captured: world state is owned by the world thread only; a crash \
         report does not take a lock to read it)\n",
    );
    write_report("crash", &body);
}

fn push_beacons(body: &mut String) {
    for line in crate::island_beacon::snapshot_lines() {
        body.push_str(&line);
        body.push('\n');
    }
}

/// 崩溃报告文件名的时间部分，照官方 `yyyy-MM-dd_HH.mm.ss`。
///
/// The timestamp part of a crash-report filename, matching the official
/// `yyyy-MM-dd_HH.mm.ss`.
fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H.%M.%S").to_string()
}

fn build_info() -> &'static str {
    STATE
        .get()
        .map(|state| state.build_info.as_str())
        .unwrap_or("<not configured>")
}

/// 写一份 `crash-reports/<prefix>-<timestamp>-server.txt`。同一秒里第二份加序号，
/// 免得 tick 线程 panic 和世界线程重新抛出时互相覆盖。
///
/// Write a `crash-reports/<prefix>-<timestamp>-server.txt`. A second report in
/// the same second gets a suffix, so a tick-thread panic and the world thread's
/// re-raise do not overwrite each other.
fn write_report(prefix: &str, body: &str) {
    let Some(state) = STATE.get() else {
        return;
    };
    let stamp = timestamp();
    let mut path = state
        .dir
        .join(format!("{prefix}-{stamp}-server.txt"));
    let mut index = 2usize;
    while path.exists() {
        path = state
            .dir
            .join(format!("{prefix}-{stamp}-{index}-server.txt"));
        index += 1;
    }
    // 写报告失败（磁盘满、权限）也不能再 panic，否则钩子会递归。
    // A failed write must not panic again or the hook would recurse.
    let _ = fs::write(path, body);
}
