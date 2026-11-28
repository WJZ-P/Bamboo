//! 启动时的日志装配：把 `logs/latest.log` 挂成非阻塞写入层，配好级别，装崩溃报告钩子。
//!
//! 命令行版和桌面版都调用 [`setup`] 拿到文件写入器和过滤指令，再各自把订阅者装起来
//! （桌面版多一层送到界面的 `LogLayer`）。写文件在专用线程上做：发出日志的线程只做
//! 内存里格式化和一次无界通道发送，绝不碰磁盘 IO。
//!
//! Startup logging assembly: attach `logs/latest.log` as a non-blocking writer
//! layer, resolve the level, and install the crash-report hook. Both the CLI and
//! the desktop call [`setup`] for the file writer and filter directive, then
//! build their own subscriber (the desktop adds a `LogLayer` for its UI). File
//! I/O runs on a dedicated thread: an emitting thread only formats in memory
//! and sends once over an unbounded channel, never touching disk I/O.

use std::{
    fs, io,
    io::Write,
    path::Path,
    sync::{Mutex, MutexGuard, mpsc},
    thread,
};

use tracing_subscriber::fmt::MakeWriter;

/// 启动时装配好的东西：文件层的写入器和 `EnvFilter` 指令字符串。
///
/// What startup assembles: the file layer's writer and the `EnvFilter` directive
/// string.
pub struct LogSetup {
    /// 非阻塞的文件写入器，交给 `fmt::layer().with_writer(...)`。
    ///
    /// The non-blocking file writer, passed to `fmt::layer().with_writer(...)`.
    pub file_writer: NonBlockingWriter,
    /// 控制台层和文件层共用的 `EnvFilter` 指令。
    ///
    /// The `EnvFilter` directive shared by the console and file layers.
    pub directive: String,
}

/// 一条正在格式化的日志：攒在内存里，drop 时整段送进通道，由写线程落盘。
///
/// One log line being formatted: buffered in memory and sent to the channel as
/// a whole on drop, written to disk by the writer thread.
pub struct LineBuffer {
    sender: mpsc::Sender<Vec<u8>>,
    bytes: Vec<u8>,
}

impl Write for LineBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineBuffer {
    fn drop(&mut self) {
        if !self.bytes.is_empty() {
            // 通道是无界的，send 不阻塞；接收端没了（写线程已退出）就丢掉这一行。
            // The channel is unbounded so send never blocks; if the receiver is
            // gone (writer thread exited) the line is simply dropped.
            let _ = self.sender.send(std::mem::take(&mut self.bytes));
        }
    }
}

/// `tracing-subscriber` 的 `MakeWriter` 实现，所有文件 IO 都在后台线程。
///
/// A `MakeWriter` implementation for `tracing-subscriber`; all file I/O is on a
/// background thread.
pub struct NonBlockingWriter {
    sender: Mutex<mpsc::Sender<Vec<u8>>>,
}

impl NonBlockingWriter {
    /// 起一条专用写线程，喂给它新建的 `latest.log`。
    ///
    /// Start a dedicated writer thread fed by the freshly created `latest.log`.
    fn spawn(file: fs::File) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("bamboo-log-writer".to_owned())
            .spawn(move || write_loop(file, receiver))?;
        Ok(Self {
            sender: Mutex::new(sender),
        })
    }

    /// 文件打不开时的退化：只保留一个没有接收端的通道，日志被安静丢掉，控制台照常。
    ///
    /// Degrade when the file cannot be opened: keep a channel with no receiver,
    /// so log lines are dropped silently and the console keeps working.
    fn discard() -> Self {
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        drop(receiver);
        Self {
            sender: Mutex::new(sender),
        }
    }
}

fn write_loop(mut file: fs::File, receiver: mpsc::Receiver<Vec<u8>>) {
    for bytes in receiver {
        if file.write_all(&bytes).is_err() || file.flush().is_err() {
            break;
        }
    }
}

impl<'a> MakeWriter<'a> for NonBlockingWriter {
    type Writer = LineBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        let guard: MutexGuard<'_, mpsc::Sender<Vec<u8>>> = match self.sender.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        LineBuffer {
            sender: guard.clone(),
            bytes: Vec::new(),
        }
    }
}

/// 建目录、归档上一份 `latest.log`、开新文件、装崩溃报告钩子，算出过滤指令。
///
/// Create the directories, archive the previous `latest.log`, open a fresh
/// file, install the crash-report hook, and resolve the filter directive.
pub fn setup(directory: &Path, log_level: &str, log_filter: &str) -> LogSetup {
    let logs_dir = directory.join("logs");
    let crash_dir = directory.join("crash-reports");
    let _ = fs::create_dir_all(&logs_dir);
    let _ = fs::create_dir_all(&crash_dir);

    mc_bamboo_game::diagnostics::archive_latest_log(&logs_dir);
    mc_bamboo_game::diagnostics::configure(&crash_dir, &build_info());

    let file = match fs::File::create(logs_dir.join("latest.log")) {
        Ok(file) => Some(file),
        Err(error) => {
            eprintln!(
                "bamboo: cannot create logs/latest.log ({error}); file logging is disabled"
            );
            None
        }
    };
    let file_writer = match file {
        Some(file) => NonBlockingWriter::spawn(file).unwrap_or_else(|error| {
            eprintln!(
                "bamboo: cannot start the log writer thread ({error}); file logging is disabled"
            );
            NonBlockingWriter::discard()
        }),
        None => NonBlockingWriter::discard(),
    };

    let directive = std::env::var("RUST_LOG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| compose_directive(log_level, log_filter));
    LogSetup {
        file_writer,
        directive,
    }
}

/// `RUST_LOG` 没设时，用 `log_level` 作全局级别，再拼上按模块细调的 `log_filter`。
///
/// With `RUST_LOG` unset, `log_level` is the global level and `log_filter` adds
/// per-module directives.
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

/// 崩溃报告里的 Bamboo 版本和提交号，提交号由 `build.rs` 编进来。
///
/// The Bamboo version and commit for crash reports; the commit is embedded by
/// `build.rs`.
fn build_info() -> String {
    format!(
        "Bamboo {} (commit {})",
        env!("CARGO_PKG_VERSION"),
        option_env!("BAMBOO_GIT_COMMIT").unwrap_or("unknown")
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
}
