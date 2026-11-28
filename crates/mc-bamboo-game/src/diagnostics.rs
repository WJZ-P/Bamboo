//! 日志归档和崩溃报告的文件机制，放在这一层是为了让世界线程的判死点也能直接写报告。
//!
//! panic 钩子在出事的线程上同步写报告（进程可能马上就没了，必须当场落盘）；判死报告
//! 在世界线程上只拼好文本，交给启动时登记的后台写入口（日志写线程）落盘，世界线程不等
//! 磁盘。在线玩家位置由世界线程每秒发布一份快照，钩子只用 `try_lock` 读，拿不到就跳过。
//! 文件日志本身的非阻塞写入在 `mc-bamboo-server::logging`。
//!
//! File mechanics for log archiving and crash reports. This lives at this layer
//! so the world thread's island-death point can write a report directly. The
//! panic hook writes its report synchronously on the failing thread (the process
//! may be gone a moment later, so it must hit the disk at once); the island-death
//! report is only formatted on the world thread and handed to the background
//! sink registered at startup (the log writer thread), so the world thread never
//! waits for the disk. Online player positions are published by the world thread
//! once a second; the hook reads them with `try_lock` only and skips them when
//! it cannot. The non-blocking file *log* writer lives in
//! `mc-bamboo-server::logging`.

use std::{
    backtrace::Backtrace,
    cell::Cell,
    collections::BTreeMap,
    fs,
    io::{self, Write},
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime},
};

use chrono::NaiveDate;

/// 崩溃报告目录和版本信息，启动时由 `mc-bamboo-server::logging` 配置一次。
///
/// The crash-report directory and build info, configured once at startup by
/// `mc-bamboo-server::logging`.
struct CrashState {
    dir: PathBuf,
    build_info: String,
}

static STATE: OnceLock<CrashState> = OnceLock::new();

/// 日志写线程提供的两个入口：后台写一份报告（返回 `false` 表示没接下，调用方自己写），
/// 以及把已经排队的日志行刷到磁盘（最多等一小会儿）。
///
/// The two entry points the log writer thread provides: write a report in the
/// background (`false` means it was not taken and the caller writes it itself),
/// and flush queued log lines to disk (waiting a short while at most).
pub struct Background {
    pub write_report: Box<dyn Fn(PathBuf, String) -> bool + Send + Sync>,
    pub flush: Box<dyn Fn() + Send + Sync>,
}

static BACKGROUND: OnceLock<Background> = OnceLock::new();

/// 登记后台入口，只有第一次生效。
///
/// Register the background entry points; only the first call takes effect.
pub fn set_background(background: Background) {
    let _ = BACKGROUND.set(background);
}

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

thread_local! {
    /// 这条线程正在钩子里：钩子自己再 panic 时不再写第二份，直接交给原来的钩子。
    ///
    /// This thread is inside the hook: a panic raised by the hook itself writes
    /// no second report and goes straight to the previous hook.
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

/// 装 panic 钩子：panic 时先写一份崩溃报告，再往日志里记一条 ERROR 并把日志刷出去，
/// 然后照常展开/退出。日志写线程在时那条 ERROR 已经经非阻塞写入器到了控制台，就不再
/// 调原来的缺省钩子：它同步写 stderr，控制台卡住（Windows 上选中文字）时会把 panic 的
/// 线程一直卡住，岛的 tick 线程因此会被判死。没有写线程、或者钩子自己 panic 时才交给
/// 缺省钩子。用 Once 保证重复调用只装一次，不会串成多层钩子。
///
/// Install the panic hook: a panic first writes a crash report, then logs one
/// ERROR line and flushes the logs, then unwinds/exits as usual. With the log
/// writer threads present that ERROR line already reaches the console through
/// the non-blocking writer, so the previous default hook is not called: it
/// writes stderr synchronously and would hold the panicking thread for as long
/// as the console is stalled (text selected in a Windows console), getting an
/// island tick thread declared dead. Only without writer threads, or when the
/// hook itself panics, does the default hook run. A Once guards against
/// repeated calls chaining multiple hooks.
pub fn install_panic_hook() {
    static HOOK: OnceLock<()> = OnceLock::new();
    HOOK.get_or_init(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if IN_HOOK.replace(true) {
                default(info);
                return;
            }
            report_panic(info);
            IN_HOOK.set(false);
            if BACKGROUND.get().is_none() {
                default(info);
            }
        }));
    });
}

/// 把上一份 `logs/latest.log` 按官方 Log4j 的命名归档成 `logs/YYYY-MM-DD-N.log.gz`。
/// 日期取那份文件最后修改的本地日期（Log4j 用的是旧文件的时间，不是启动时刻），读不到
/// 就用今天。空文件不归档（Log4j `OnStartupTriggeringPolicy` 的 `minSize` 缺省是 1）。
///
/// Archive the previous `logs/latest.log` as `logs/YYYY-MM-DD-N.log.gz`, matching
/// the official Log4j naming. The date is the local date the file was last
/// modified (Log4j names the archive after the old file's time, not the startup
/// time), or today when that cannot be read. An empty file is not archived
/// (Log4j's `OnStartupTriggeringPolicy` has a default `minSize` of 1).
pub fn archive_latest_log(logs_dir: &Path) {
    let latest = logs_dir.join("latest.log");
    let Ok(metadata) = fs::metadata(&latest) else {
        return;
    };
    let date = metadata
        .modified()
        .map(local_date)
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    archive_latest_log_dated(logs_dir, date);
}

fn local_date(time: SystemTime) -> NaiveDate {
    chrono::DateTime::<chrono::Local>::from(time).date_naive()
}

/// 归档的主体，日期由调用方给（跨日滚动时是刚过去的那一天）。N 取那天还没用过的最小
/// 正整数。流式压缩，不把整份日志读进内存。gzip 失败时退一步改名成不压缩的
/// `YYYY-MM-DD-N.log`；连改名也失败就原样留着 `latest.log`，由调用方接着往后追加，
/// 任何情况下都不丢旧日志。调用时 `latest.log` 不能还开着（Windows 上删不掉）。
///
/// The archiving body; the caller supplies the date (the day that just ended
/// for a midnight rollover). N is the smallest positive integer unused that
/// day. Compression is streamed, never reading the whole log into memory. When
/// gzip fails it falls back to renaming to an uncompressed `YYYY-MM-DD-N.log`;
/// when even that fails `latest.log` stays as it is and the caller appends to
/// it, so the old log is never lost. `latest.log` must not be open when this is
/// called (Windows could not remove it).
pub fn archive_latest_log_dated(logs_dir: &Path, date: NaiveDate) {
    let latest = logs_dir.join("latest.log");
    match fs::metadata(&latest) {
        Ok(metadata) if metadata.len() > 0 => {}
        _ => return,
    }
    let date = date.format("%Y-%m-%d").to_string();
    let mut index = 1usize;
    let (compressed, plain) = loop {
        let compressed = logs_dir.join(format!("{date}-{index}.log.gz"));
        let plain = logs_dir.join(format!("{date}-{index}.log"));
        if !compressed.exists() && !plain.exists() {
            break (compressed, plain);
        }
        index += 1;
    };
    if gzip_file(&latest, &compressed).is_ok() {
        let _ = fs::remove_file(&latest);
    } else {
        let _ = fs::remove_file(&compressed);
        let _ = fs::rename(&latest, &plain);
    }
}

fn gzip_file(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let output = io::BufWriter::new(fs::File::create(destination)?);
    let mut encoder = flate2::write::GzEncoder::new(output, flate2::Compression::default());
    io::copy(&mut input, &mut encoder)?;
    encoder.finish()?.flush()
}

/// 岛判死时写一份 `crash-reports/island-dead-*.txt`：`log_body` 是判死日志的内容，
/// 后面附上所有 tick 线程的信标和在线玩家位置。文本在调用线程上拼好，落盘交给后台。
///
/// Write a `crash-reports/island-dead-*.txt` when an island is declared dead:
/// `log_body` is the death log's content, followed by every tick thread's
/// beacon and the online player positions. The text is built on the calling
/// thread; the disk write goes to the background.
pub fn write_island_dead_report(log_body: &str) {
    let mut body = String::from("---- Bamboo Island Death Report ----\n\n");
    body.push_str(&format!("Time: {}\n", timestamp()));
    body.push_str(&format!("Build: {}\n\n", build_info()));
    body.push_str(log_body);
    body.push_str("\n\n-- All island progress beacons --\n");
    push_beacons(&mut body);
    body.push_str("\n-- Online player positions --\n");
    push_players(&mut body);
    let Some(path) = report_path("island-dead") else {
        return;
    };
    let body = match BACKGROUND.get() {
        Some(background) => {
            // 文件名先占住（空文件），同一秒里的下一份报告才会换序号。
            // Claim the name first (an empty file) so the next report in the
            // same second picks another suffix.
            let _ = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path);
            if (background.write_report)(path.clone(), body.clone()) {
                return;
            }
            body
        }
        None => body,
    };
    let _ = fs::write(path, body);
}

/// panic 钩子的主体：写报告、记一条 ERROR、把日志刷盘。
///
/// The panic hook body: write the report, log one ERROR line, flush the log.
fn report_panic(info: &PanicHookInfo<'_>) {
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<unnamed>");
    let message = panic_text(info.payload());
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
    let path = write_panic_report(thread_name, &location, message);
    let report = path
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<not written>".to_owned());
    tracing::error!(
        thread = thread_name,
        location = %location,
        report = %report,
        "panic: {message}"
    );
    flush_log();
}

/// 把已经排队的日志行刷到磁盘，最多等一小会儿。panic 钩子里、`std::process::exit`
/// 之前调用（那条路不跑析构，日志守卫来不及刷）。没装文件日志时什么也不做。
///
/// Flush queued log lines to disk, waiting a short while at most. Called from
/// the panic hook and before `std::process::exit` (which runs no destructors,
/// so the log guard would not get to flush). A no-op without file logging.
pub fn flush_log() {
    if let Some(background) = BACKGROUND.get() {
        (background.flush)();
    }
}

/// panic 载荷里的文字（`&str` 或 `String`），别的类型给一个占位。
///
/// The text of a panic payload (`&str` or `String`), or a placeholder for any
/// other type.
pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

/// 收集消息、位置、线程名、backtrace、版本、信标和玩家位置，同步写报告。
///
/// Gather message, location, thread name, backtrace, build info, beacons and
/// player positions, then write the report synchronously.
fn write_panic_report(thread_name: &str, location: &str, message: &str) -> Option<PathBuf> {
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
    push_players(&mut body);
    let path = report_path("crash")?;
    // 写报告失败（磁盘满、权限）也不能再 panic，否则钩子会递归。
    // A failed write must not panic again or the hook would recurse.
    fs::write(&path, body).ok().map(|()| path)
}

fn push_beacons(body: &mut String) {
    for line in crate::island_beacon::snapshot_lines() {
        body.push_str(&line);
        body.push('\n');
    }
}

/// 一个岛上一名在线玩家的位置。
///
/// One online player's position on an island.
#[derive(Debug, Clone)]
pub(crate) struct PlayerSpot {
    pub(crate) name: String,
    pub(crate) position: [f64; 3],
}

/// 世界线程发布的玩家位置快照：每个岛一份，带发布时刻。
///
/// The player-position snapshot the world thread publishes: one entry per
/// island, with the time it was published.
#[derive(Default)]
struct PlayerBoard {
    islands: Mutex<BTreeMap<u32, (Instant, Vec<PlayerSpot>)>>,
}

static PLAYERS: PlayerBoard = PlayerBoard {
    islands: Mutex::new(BTreeMap::new()),
};

impl PlayerBoard {
    /// `living` 是现存的岛，`parked` 是这一刻停放在主控手里的岛和它们的玩家。不在手里的
    /// 岛保留上一份快照，已经没了的岛删掉。只用 `try_lock`：报告正在读就跳过这一次。
    ///
    /// `living` lists the islands that exist and `parked` the ones in the
    /// master's hands with their players. An island that is away keeps its
    /// previous snapshot; one that is gone is removed. `try_lock` only: when a
    /// report is reading, this round is skipped.
    fn publish(&self, living: &[u32], parked: Vec<(u32, Vec<PlayerSpot>)>) {
        let Ok(mut islands) = self.islands.try_lock() else {
            return;
        };
        islands.retain(|island, _| living.contains(island));
        let now = Instant::now();
        for (island, spots) in parked {
            islands.insert(island, (now, spots));
        }
    }

    fn describe(&self, body: &mut String) {
        let Ok(islands) = self.islands.try_lock() else {
            body.push_str("(player snapshot busy; positions unavailable)\n");
            return;
        };
        let mut any = false;
        for (island, (at, spots)) in islands.iter() {
            for spot in spots {
                any = true;
                let [x, y, z] = spot.position;
                body.push_str(&format!(
                    "{} at ({x:.2}, {y:.2}, {z:.2}) on island {island}, seen {:.1}s ago\n",
                    spot.name,
                    at.elapsed().as_secs_f64()
                ));
            }
        }
        if !any {
            body.push_str(
                "(no players in the last snapshot; the world thread publishes one a second)\n",
            );
        }
    }
}

/// 世界线程每秒调用一次，见 [`PlayerBoard::publish`]。世界线程不等锁。
///
/// Called by the world thread once a second; see [`PlayerBoard::publish`]. The
/// world thread never waits for the lock.
pub(crate) fn publish_players(living: &[u32], parked: Vec<(u32, Vec<PlayerSpot>)>) {
    PLAYERS.publish(living, parked);
}

fn push_players(body: &mut String) {
    PLAYERS.describe(body);
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

/// 选一个还没用过的 `crash-reports/<prefix>-<timestamp>-server.txt`。同一秒里已经有一份
/// 时加序号（几条线程同时 panic、或者一秒里判死几个岛），免得互相覆盖。
///
/// Pick an unused `crash-reports/<prefix>-<timestamp>-server.txt`. When one
/// already exists for the same second (several threads panicking at once, or
/// several islands declared dead within a second) a suffix keeps them from
/// overwriting each other.
fn report_path(prefix: &str) -> Option<PathBuf> {
    let state = STATE.get()?;
    let stamp = timestamp();
    let mut path = state.dir.join(format!("{prefix}-{stamp}-server.txt"));
    let mut index = 2usize;
    while path.exists() {
        path = state
            .dir
            .join(format!("{prefix}-{stamp}-{index}-server.txt"));
        index += 1;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(name: &str, x: f64) -> PlayerSpot {
        PlayerSpot {
            name: name.to_owned(),
            position: [x, 64.0, -3.5],
        }
    }

    /// 停放着的岛换成新快照，不在手里的岛沿用旧的，没了的岛删掉。
    ///
    /// Parked islands get a new snapshot, islands that are away keep the old
    /// one, and islands that are gone are removed.
    #[test]
    fn the_player_board_keeps_absent_islands_and_forgets_dead_ones() {
        let board = PlayerBoard::default();
        let mut text = String::new();
        board.describe(&mut text);
        assert!(text.contains("no players in the last snapshot"), "{text}");

        board.publish(
            &[0, 1, 2],
            vec![
                (0, vec![spot("Alex", 1.0)]),
                (1, vec![spot("Steve", 2.0)]),
                (2, vec![spot("Gone", 3.0)]),
            ],
        );
        // 岛 1 不在手里，岛 2 没了。
        // Island 1 is away and island 2 is gone.
        board.publish(&[0, 1], vec![(0, vec![spot("Alex", 9.25)])]);
        let mut text = String::new();
        board.describe(&mut text);
        assert!(
            text.contains("Alex at (9.25, 64.00, -3.50) on island 0"),
            "{text}"
        );
        assert!(
            text.contains("Steve at (2.00, 64.00, -3.50) on island 1"),
            "{text}"
        );
        assert!(!text.contains("Gone"), "{text}");
    }

    /// 报告正在读的时候，发布直接跳过，不等锁。
    ///
    /// While a report is reading, publishing is skipped instead of waiting.
    #[test]
    fn publishing_never_waits_for_a_reader() {
        let board = PlayerBoard::default();
        let reader = board.islands.lock().unwrap();
        board.publish(&[0], vec![(0, vec![spot("Alex", 1.0)])]);
        assert!(reader.is_empty());
        let mut text = String::new();
        board.describe(&mut text);
        assert!(text.contains("player snapshot busy"), "{text}");
    }
}
