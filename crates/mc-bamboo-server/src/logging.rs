//! 启动时的日志装配：把 `logs/latest.log` 挂成非阻塞写入层，配好级别，装崩溃报告钩子。
//!
//! 命令行版和桌面版都调用 [`setup`] 拿到两个写入器、两条过滤指令和一个守卫，再用
//! [`console_layer`] / [`file_layer`] 把订阅者装起来（桌面版多一层送到界面的
//! `LogLayer`）。写文件和写 stdout 各在一条专用线程上做：发出日志的线程只把格式化好的
//! 一行拷进一块内存、往通道投一次，绝不碰磁盘或控制台 IO；排队的字节数有上限，输出跟不上
//! 时丢行并记下丢了几行，世界线程永远不等。守卫 drop 时、panic 钩子里都会把排队的行刷出去。
//!
//! Startup logging assembly: attach `logs/latest.log` as a non-blocking writer
//! layer, resolve the levels, and install the crash-report hook. Both the CLI
//! and the desktop call [`setup`] for the two writers, the two filter directives
//! and a guard, then build their subscriber from [`console_layer`] /
//! [`file_layer`] (the desktop adds a `LogLayer` for its UI). The file and
//! stdout are each written on a dedicated thread: an emitting thread only
//! copies the formatted line into a buffer and sends it once over a channel,
//! never touching disk or console I/O. Queued bytes are capped: when an output
//! cannot keep up, lines are dropped and the count is recorded, so the world
//! thread never waits. Queued lines are flushed when the guard drops and
//! inside the panic hook.

use std::{
    fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use chrono::{Local, NaiveDate};
use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{self, MakeWriter, format::Writer, time::FormatTime},
    registry::LookupSpan,
};

use crate::config_files::BambooConfig;

/// 排队等落盘的日志字节数上限。超过之后新的行被丢掉（并计数），直到写线程追上来。
///
/// Cap on log bytes queued for the disk. Past it new lines are dropped (and
/// counted) until the writer thread catches up.
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;

/// 刷盘最多等多久：守卫 drop、panic 钩子都用它。磁盘卡死时不能把退出也卡死。
///
/// How long a flush waits at most; used by the guard's drop and the panic
/// hook. A stuck disk must not also hang the exit.
const FLUSH_WAIT: Duration = Duration::from_secs(3);

/// 写线程隔多久看一次本地日期有没有变（跨日滚动）。
///
/// How often the writer thread checks whether the local date changed (the
/// midnight rollover).
const DATE_CHECK: Duration = Duration::from_secs(1);

/// 启动时装配好的东西。
///
/// What startup assembles.
pub struct LogSetup {
    /// 非阻塞的控制台写入器（stdout），交给 [`console_layer`]。
    ///
    /// The non-blocking console writer (stdout), passed to [`console_layer`].
    pub console_writer: NonBlockingWriter,
    /// 非阻塞的文件写入器，交给 [`file_layer`]。
    ///
    /// The non-blocking file writer, passed to [`file_layer`].
    pub file_writer: NonBlockingWriter,
    /// 控制台层（和桌面界面那一层）的 `EnvFilter` 指令。
    ///
    /// The `EnvFilter` directive of the console layer (and the desktop UI layer).
    pub console_directive: String,
    /// 文件层的 `EnvFilter` 指令。
    ///
    /// The `EnvFilter` directive of the file layer.
    pub file_directive: String,
    /// 活到进程结束：drop 时把排队的行刷到磁盘。
    ///
    /// Keep alive until the process ends: dropping it flushes queued lines.
    pub guard: LogGuard,
}

enum Message {
    Line(Vec<u8>),
    Report(PathBuf, String),
    Flush(mpsc::SyncSender<()>),
}

/// 发日志的线程和写线程共用的状态。
///
/// State shared by emitting threads and the writer thread.
struct Shared {
    sender: mpsc::Sender<Message>,
    queued_bytes: AtomicUsize,
    dropped_lines: AtomicU64,
    writer_thread: OnceLock<ThreadId>,
}

impl Shared {
    fn send_line(&self, bytes: Vec<u8>) {
        let len = bytes.len();
        if self.queued_bytes.fetch_add(len, Ordering::Relaxed) > MAX_QUEUED_BYTES {
            self.queued_bytes.fetch_sub(len, Ordering::Relaxed);
            self.dropped_lines.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // 通道本身无界，send 不阻塞；写线程没了就丢掉这一行。
        // The channel itself is unbounded so send never blocks; with the writer
        // thread gone the line is dropped.
        if self.sender.send(Message::Line(bytes)).is_err() {
            self.queued_bytes.fetch_sub(len, Ordering::Relaxed);
        }
    }

    /// 等写线程把此刻已经排队的东西都写完、刷盘，最多等 [`FLUSH_WAIT`]。
    ///
    /// Wait until the writer thread has written and flushed everything queued
    /// so far, for at most [`FLUSH_WAIT`].
    fn flush(&self) {
        if self.writer_thread.get() == Some(&thread::current().id()) {
            return;
        }
        let (ack, done) = mpsc::sync_channel(1);
        if self.sender.send(Message::Flush(ack)).is_ok() {
            let _ = done.recv_timeout(FLUSH_WAIT);
        }
    }
}

/// 日志写线程的守卫。drop 时刷盘；桌面版在应用退出事件里显式调 [`LogGuard::flush`]
/// （`std::process::exit` 不跑析构）。
///
/// Guard of the log writer thread. Dropping it flushes; the desktop calls
/// [`LogGuard::flush`] explicitly on its exit event (`std::process::exit` runs
/// no destructors).
pub struct LogGuard {
    file: Arc<Shared>,
    console: Arc<Shared>,
}

impl LogGuard {
    pub fn flush(&self) {
        self.file.flush();
        self.console.flush();
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        self.flush();
    }
}

/// 一条正在格式化的日志：攒在内存里，drop 时整段送进通道，由写线程落盘。
///
/// One log line being formatted: buffered in memory and sent to the channel as
/// a whole on drop, written to disk by the writer thread.
pub struct LineBuffer<'a> {
    shared: &'a Shared,
    bytes: Vec<u8>,
}

impl Write for LineBuffer<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineBuffer<'_> {
    fn drop(&mut self) {
        if !self.bytes.is_empty() {
            self.shared.send_line(std::mem::take(&mut self.bytes));
        }
    }
}

/// `tracing-subscriber` 的 `MakeWriter` 实现，所有 IO（文件或 stdout）都在后台线程。
///
/// A `MakeWriter` implementation for `tracing-subscriber`; all I/O (the file or
/// stdout) is on a background thread.
pub struct NonBlockingWriter {
    shared: Arc<Shared>,
}

impl<'a> MakeWriter<'a> for NonBlockingWriter {
    type Writer = LineBuffer<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        // 发出日志的线程不取锁，也不动引用计数。
        // The emitting thread takes no lock and touches no reference count.
        LineBuffer {
            shared: &self.shared,
            bytes: Vec::new(),
        }
    }
}

/// 写线程往哪里写：`latest.log` 或 stdout。
///
/// Where a writer thread writes: `latest.log` or stdout.
trait Sink {
    fn write(&mut self, bytes: &[u8]);
    fn flush(&mut self);
}

/// 起一条写线程，`open` 在那条线程上打开输出。线程起不来时通道没有接收端，日志行被
/// 安静丢掉。
///
/// Start a writer thread; `open` opens the output on that thread. If the
/// thread cannot start the channel has no receiver, so log lines are dropped
/// silently.
fn spawn_writer<S: Sink>(name: &str, open: impl FnOnce() -> S + Send + 'static) -> Arc<Shared> {
    let (sender, receiver) = mpsc::channel();
    let shared = Arc::new(Shared {
        sender,
        queued_bytes: AtomicUsize::new(0),
        dropped_lines: AtomicU64::new(0),
        writer_thread: OnceLock::new(),
    });
    let for_thread = Arc::clone(&shared);
    let spawned = thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || run_writer(open(), &receiver, &for_thread));
    match spawned {
        Ok(handle) => {
            let _ = shared.writer_thread.set(handle.thread().id());
        }
        Err(error) => eprintln!("bamboo: cannot start the {name} thread ({error})"),
    }
    shared
}

/// 写线程的主循环：行、报告、刷盘请求按到达顺序处理。
///
/// The writer thread's main loop: lines, reports and flush requests are
/// handled in arrival order.
fn run_writer<S: Sink>(mut sink: S, receiver: &mpsc::Receiver<Message>, shared: &Shared) {
    loop {
        // 队列空了才刷盘：忙的时候一批行合成一次系统调用。
        // Flush only when the queue is empty: under load a batch of lines
        // becomes a single system call.
        let message = match receiver.try_recv() {
            Ok(message) => message,
            Err(mpsc::TryRecvError::Empty) => {
                sink.flush();
                match receiver.recv() {
                    Ok(message) => message,
                    Err(mpsc::RecvError) => return,
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
        };
        match message {
            Message::Line(bytes) => {
                shared
                    .queued_bytes
                    .fetch_sub(bytes.len(), Ordering::Relaxed);
                note_dropped(&mut sink, shared);
                sink.write(&bytes);
            }
            Message::Report(path, body) => {
                if let Err(error) = fs::write(&path, body) {
                    eprintln!("bamboo: cannot write {} ({error})", path.display());
                }
            }
            Message::Flush(ack) => {
                sink.flush();
                let _ = ack.send(());
            }
        }
    }
    sink.flush();
}

fn note_dropped<S: Sink>(sink: &mut S, shared: &Shared) {
    if shared.dropped_lines.load(Ordering::Relaxed) == 0 {
        return;
    }
    let dropped = shared.dropped_lines.swap(0, Ordering::Relaxed);
    let note = format!(
        "[bamboo-log] {dropped} log lines were dropped here: the output fell more than \
         {MAX_QUEUED_BYTES} queued bytes behind\n"
    );
    sink.write(note.as_bytes());
}

/// 控制台写线程手里的 stdout。写不出去（没有控制台、管道断了）就安静丢掉。
///
/// The console writer thread's stdout. Lines that cannot be written (no
/// console, a broken pipe) are dropped silently.
struct Console {
    out: io::Stdout,
}

impl Sink for Console {
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }

    fn flush(&mut self) {
        let _ = self.out.flush();
    }
}

/// 写线程手里的 `latest.log`。
///
/// The writer thread's `latest.log`.
struct LogFile {
    logs_dir: PathBuf,
    file: Option<BufWriter<fs::File>>,
    /// 当前这份文件属于哪一天（本地日期）。
    ///
    /// The local date the current file belongs to.
    date: NaiveDate,
    next_date_check: Instant,
    /// 已经往 stderr 报过一次写失败，不再重复报。
    ///
    /// A write failure was already reported to stderr once; do not repeat it.
    failure_reported: bool,
}

impl LogFile {
    fn open(logs_dir: PathBuf) -> Self {
        let mut log = Self {
            logs_dir,
            file: None,
            date: Local::now().date_naive(),
            next_date_check: Instant::now() + DATE_CHECK,
            failure_reported: false,
        };
        log.reopen();
        log
    }

    /// 以追加方式打开 `latest.log`：归档成功时它是新文件，归档失败时接着旧内容写。
    ///
    /// Open `latest.log` for appending: a fresh file after a successful
    /// archive, and a continuation of the old content after a failed one.
    fn reopen(&mut self) {
        let path = self.logs_dir.join("latest.log");
        match fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => self.file = Some(BufWriter::with_capacity(64 * 1024, file)),
            Err(error) => self.report_failure(&error),
        }
    }

    fn report_failure(&mut self, error: &io::Error) {
        if !self.failure_reported {
            self.failure_reported = true;
            eprintln!("bamboo: cannot write logs/latest.log ({error}); log lines are being lost");
        }
    }

    /// 官方 Log4j 配置里的 `TimeBasedTriggeringPolicy`：本地日期变了，就把写到昨天为止的
    /// `latest.log` 归档成昨天的 `YYYY-MM-DD-N.log.gz`，再开一份新的。每秒最多看一次日期。
    ///
    /// The `TimeBasedTriggeringPolicy` of the official Log4j configuration: once
    /// the local date changes, the `latest.log` written up to yesterday is
    /// archived as yesterday's `YYYY-MM-DD-N.log.gz` and a fresh one is opened.
    /// The date is checked at most once a second.
    fn roll_at_midnight(&mut self) {
        let now = Instant::now();
        if now < self.next_date_check {
            return;
        }
        self.next_date_check = now + DATE_CHECK;
        let today = Local::now().date_naive();
        if today == self.date {
            return;
        }
        self.flush();
        // Windows 上要先关掉文件才能删。
        // On Windows the file must be closed before it can be removed.
        self.file = None;
        mc_bamboo_game::diagnostics::archive_latest_log_dated(&self.logs_dir, self.date);
        self.date = today;
        self.reopen();
    }
}

impl Sink for LogFile {
    fn write(&mut self, bytes: &[u8]) {
        self.roll_at_midnight();
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Err(error) = file.write_all(bytes) {
            self.report_failure(&error);
        }
    }

    fn flush(&mut self) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Err(error) = file.flush() {
            self.report_failure(&error);
        }
    }
}

/// 建目录、归档上一份 `latest.log`、起写线程、装崩溃报告钩子，算出过滤指令。
///
/// Create the directories, archive the previous `latest.log`, start the writer
/// thread, install the crash-report hook, and resolve the filter directives.
pub fn setup(directory: &Path, config: &BambooConfig) -> LogSetup {
    let logs_dir = directory.join("logs");
    let crash_dir = directory.join("crash-reports");
    if let Err(error) = fs::create_dir_all(&logs_dir) {
        eprintln!(
            "bamboo: cannot create {} ({error}); file logging is disabled",
            logs_dir.display()
        );
    }

    mc_bamboo_game::diagnostics::archive_latest_log(&logs_dir);
    let file = spawn_writer("bamboo-log-writer", move || LogFile::open(logs_dir));
    let console = spawn_writer("bamboo-log-console", || Console { out: io::stdout() });
    mc_bamboo_game::diagnostics::configure(&crash_dir, &build_info());
    register_background(&file, &console);

    let (console_directive, file_directive) = directives(
        std::env::var("RUST_LOG").ok().as_deref(),
        &config.log_level,
        &config.log_file_level,
        &config.log_filter,
    );
    LogSetup {
        console_writer: NonBlockingWriter {
            shared: Arc::clone(&console),
        },
        file_writer: NonBlockingWriter {
            shared: Arc::clone(&file),
        },
        console_directive,
        file_directive,
        guard: LogGuard { file, console },
    }
}

/// 把写线程借给 `diagnostics`：判死报告交给文件写线程落盘，panic 钩子靠它们把日志刷出去。
///
/// Lend the writer threads to `diagnostics`: island-death reports are written
/// by the file writer thread, and the panic hook flushes the log through both.
fn register_background(file: &Arc<Shared>, console: &Arc<Shared>) {
    let for_reports = Arc::clone(file);
    let guard = LogGuard {
        file: Arc::clone(file),
        console: Arc::clone(console),
    };
    mc_bamboo_game::diagnostics::set_background(mc_bamboo_game::diagnostics::Background {
        write_report: Box::new(move |path, body| {
            for_reports.writer_thread.get().is_some()
                && for_reports.sender.send(Message::Report(path, body)).is_ok()
        }),
        flush: Box::new(move || guard.flush()),
    });
}

/// 控制台和文件两条过滤指令。`RUST_LOG` 设了就两边都用它；否则控制台用 `log_level`，
/// 文件用 `log_file_level`（留空就跟 `log_level` 一样），再各自拼上 `log_filter`。
///
/// The console and file filter directives. A set `RUST_LOG` is used for both;
/// otherwise the console uses `log_level` and the file `log_file_level` (empty
/// means the same as `log_level`), each with `log_filter` appended.
fn directives(
    rust_log: Option<&str>,
    log_level: &str,
    log_file_level: &str,
    log_filter: &str,
) -> (String, String) {
    if let Some(value) = rust_log.filter(|value| !value.trim().is_empty()) {
        return (value.to_owned(), value.to_owned());
    }
    let file_level = if log_file_level.is_empty() {
        log_level
    } else {
        log_file_level
    };
    (
        compose_directive(log_level, log_filter),
        compose_directive(file_level, log_filter),
    )
}

fn compose_directive(log_level: &str, log_filter: &str) -> String {
    let base = match log_level {
        "error" | "warn" | "info" | "debug" | "trace" => log_level,
        _ => "info",
    };
    let extra = log_filter.trim();
    if extra.is_empty() {
        base.to_owned()
    } else {
        format!("{base},{extra}")
    }
}

/// 日志行开头的时间：本地时间，带日期和毫秒，和崩溃报告文件名用的是同一个时钟。
///
/// The time at the start of a log line: local time with date and milliseconds,
/// the same clock crash-report file names use.
pub struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(writer, "{}", Local::now().format("%Y-%m-%d %H:%M:%S%.3f"))
    }
}

/// 控制台层：经非阻塞写入器输出到 stdout，按 `directive` 过滤。控制台卡住（Windows 上在
/// 窗口里选中文字就会暂停输出）时卡的是写线程，不是发日志的线程。
///
/// The console layer: writes to stdout through the non-blocking writer,
/// filtered by `directive`. When the console stalls (on Windows, selecting
/// text in the window pauses output) it is the writer thread that waits, not
/// the emitting thread.
pub fn console_layer<S>(writer: NonBlockingWriter, directive: &str) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fmt::layer()
        .with_timer(LocalTime)
        .with_writer(writer)
        .with_filter(EnvFilter::new(directive))
}

/// 文件层：经非阻塞写入器写 `logs/latest.log`，不带颜色，按 `directive` 过滤。
///
/// The file layer: writes `logs/latest.log` through the non-blocking writer,
/// without colours, filtered by `directive`.
pub fn file_layer<S>(writer: NonBlockingWriter, directive: &str) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fmt::layer()
        .with_timer(LocalTime)
        .with_writer(writer)
        .with_ansi(false)
        .with_filter(EnvFilter::new(directive))
}

/// 崩溃报告里的 Bamboo 版本、提交号和对应的游戏版本，提交号由 `build.rs` 编进来。
///
/// The Bamboo version, commit and target game version for crash reports; the
/// commit is embedded by `build.rs`.
fn build_info() -> String {
    format!(
        "Bamboo {} (commit {}) for Minecraft {}",
        env!("CARGO_PKG_VERSION"),
        option_env!("BAMBOO_GIT_COMMIT").unwrap_or("unknown"),
        crate::GAME_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_is_a_single_global_directive() {
        assert_eq!(compose_directive("info", ""), "info");
        assert_eq!(compose_directive("warn", "  "), "warn");
        assert_eq!(compose_directive("trace", ""), "trace");
    }

    #[test]
    fn a_module_filter_is_appended_to_the_level() {
        assert_eq!(
            compose_directive("info", "mc_bamboo_game::director=trace"),
            "info,mc_bamboo_game::director=trace"
        );
    }

    #[test]
    fn an_unknown_level_falls_back_to_info() {
        assert_eq!(compose_directive("verbose", ""), "info");
    }

    #[test]
    fn the_file_level_follows_the_console_level_unless_set() {
        assert_eq!(
            directives(None, "info", "", ""),
            ("info".to_owned(), "info".to_owned())
        );
        assert_eq!(
            directives(None, "warn", "debug", "a=trace"),
            ("warn,a=trace".to_owned(), "debug,a=trace".to_owned())
        );
    }

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("bamboo-log-unit-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn detached_shared() -> (Arc<Shared>, mpsc::Receiver<Message>) {
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            sender,
            queued_bytes: AtomicUsize::new(0),
            dropped_lines: AtomicU64::new(0),
            writer_thread: OnceLock::new(),
        });
        (shared, receiver)
    }

    /// 本地日期变了：到昨天为止的内容归档成昨天的 `.log.gz`，新的一行进新文件。
    ///
    /// When the local date changes, what was written up to yesterday is
    /// archived as yesterday's `.log.gz` and the next line starts a new file.
    #[test]
    fn the_log_rolls_over_when_the_date_changes() {
        use std::io::Read;

        let temp = Temp::new();
        let mut log = LogFile::open(temp.0.clone());
        log.write(b"written yesterday\n");
        let yesterday = log.date.pred_opt().unwrap();
        log.date = yesterday;
        log.next_date_check = Instant::now();
        log.write(b"written today\n");
        log.flush();

        assert_eq!(log.date, Local::now().date_naive());
        let archive = temp
            .0
            .join(format!("{}-1.log.gz", yesterday.format("%Y-%m-%d")));
        let mut archived = String::new();
        flate2::read::GzDecoder::new(fs::File::open(&archive).unwrap())
            .read_to_string(&mut archived)
            .unwrap();
        assert_eq!(archived, "written yesterday\n");
        assert_eq!(
            fs::read_to_string(temp.0.join("latest.log")).unwrap(),
            "written today\n"
        );
    }

    /// 同一天里不滚动，每秒最多看一次日期。
    ///
    /// No rollover within a day, and the date is checked at most once a second.
    #[test]
    fn the_log_does_not_roll_within_a_day() {
        let temp = Temp::new();
        let mut log = LogFile::open(temp.0.clone());
        log.write(b"one\n");
        log.next_date_check = Instant::now();
        log.write(b"two\n");
        assert!(log.next_date_check > Instant::now());
        log.flush();
        assert_eq!(
            fs::read_to_string(temp.0.join("latest.log")).unwrap(),
            "one\ntwo\n"
        );
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }

    /// 排队超过上限后新的行被丢掉并计数，发日志的线程不等；写线程追上来时在文件里记下
    /// 丢了几行。
    ///
    /// Past the cap new lines are dropped and counted without the emitting
    /// thread waiting; once the writer catches up the file records how many.
    #[test]
    fn lines_past_the_queue_cap_are_dropped_and_counted() {
        let temp = Temp::new();
        let (shared, receiver) = detached_shared();
        shared
            .queued_bytes
            .store(MAX_QUEUED_BYTES + 1, Ordering::Relaxed);
        shared.send_line(b"lost one\n".to_vec());
        shared.send_line(b"lost two\n".to_vec());
        assert!(receiver.try_recv().is_err(), "nothing was queued");
        assert_eq!(shared.dropped_lines.load(Ordering::Relaxed), 2);
        assert_eq!(
            shared.queued_bytes.load(Ordering::Relaxed),
            MAX_QUEUED_BYTES + 1
        );

        shared.queued_bytes.store(0, Ordering::Relaxed);
        shared.send_line(b"kept\n".to_vec());
        assert_eq!(shared.queued_bytes.load(Ordering::Relaxed), 5);
        let (ack, done) = mpsc::sync_channel(1);
        shared.sender.send(Message::Flush(ack)).unwrap();
        let writer = {
            let shared = Arc::clone(&shared);
            let dir = temp.0.clone();
            thread::spawn(move || run_writer(LogFile::open(dir), &receiver, &shared))
        };
        done.recv_timeout(Duration::from_secs(30)).unwrap();
        let text = fs::read_to_string(temp.0.join("latest.log")).unwrap();
        assert!(text.contains("2 log lines were dropped here"), "{text}");
        assert!(text.ends_with("kept\n"), "{text}");
        assert_eq!(shared.queued_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(shared.dropped_lines.load(Ordering::Relaxed), 0);
        drop(writer);
    }

    /// 写线程自己调刷盘不会等自己。
    ///
    /// A flush from the writer thread itself does not wait for itself.
    #[test]
    fn a_flush_from_the_writer_thread_returns_at_once() {
        let (shared, _receiver) = detached_shared();
        shared.writer_thread.set(thread::current().id()).unwrap();
        let started = Instant::now();
        shared.flush();
        assert!(started.elapsed() < FLUSH_WAIT);
    }

    #[test]
    fn rust_log_overrides_both_levels() {
        assert_eq!(
            directives(Some("trace"), "warn", "debug", "a=info"),
            ("trace".to_owned(), "trace".to_owned())
        );
        assert_eq!(
            directives(Some("  "), "warn", "", ""),
            ("warn".to_owned(), "warn".to_owned())
        );
    }
}
