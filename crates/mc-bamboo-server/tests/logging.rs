//! 文件日志装配的集成测试，单独一个测试二进制：`setup` 会装全局 panic 钩子，而且崩溃
//! 报告目录和后台写入口都只认第一次配置，所以整条流程放在一个测试里。
//!
//! Integration test for file-log setup, in a dedicated test binary: `setup`
//! installs a global panic hook, and the crash-report directory and the
//! background sink only honour the first configuration, so the whole flow lives
//! in one test.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use mc_bamboo_server::{config_files::BambooConfig, logging};
use tracing_subscriber::prelude::*;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-logging-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn files_with(dir: &Path, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix))
        })
        .collect()
}

fn gunzip(path: &Path) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// 整条流程：归档旧日志、按各自级别写文件、刷盘后一行不丢、判死报告经写线程落盘、
/// panic 时报告和日志都当场落盘。
///
/// The whole flow: archive the old log, write the file at its own level, lose
/// no line across a flush, write the island-death report through the writer
/// thread, and have both the report and the log on disk right after a panic.
#[test]
fn file_logging_archives_writes_flushes_and_reports() {
    let temp = Temp::new();
    let logs = temp.path().join("logs");
    let crash = temp.path().join("crash-reports");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("latest.log"), b"old session\n").unwrap();

    let config = BambooConfig {
        log_level: "warn".to_owned(),
        log_file_level: "debug".to_owned(),
        ..BambooConfig::default()
    };
    let setup = logging::setup(temp.path(), &config);
    if std::env::var("RUST_LOG").is_err() {
        assert_eq!(setup.console_directive, "warn");
        assert_eq!(setup.file_directive, "debug");
    }

    let archives = files_with(&logs, "", ".log.gz");
    assert_eq!(archives.len(), 1, "{archives:?}");
    assert_eq!(gunzip(&archives[0]), "old session\n");
    assert!(crash.is_dir(), "the crash-reports directory is created");

    // 只装文件层，级别固定成 debug（不受 RUST_LOG 影响）。
    // Only the file layer, pinned to debug (independent of RUST_LOG).
    let subscriber =
        tracing_subscriber::registry().with(logging::file_layer(setup.file_writer, "debug"));
    let _default = tracing::subscriber::set_default(subscriber);

    const LINES: usize = 20_000;
    for index in 0..LINES {
        tracing::debug!(index, "numbered line");
    }
    tracing::trace!("a trace line the file level filters out");
    setup.guard.flush();
    let text = fs::read_to_string(logs.join("latest.log")).unwrap();
    assert_eq!(text.matches("numbered line").count(), LINES);
    assert!(text.contains(&format!("index={}", LINES - 1)), "last line");
    assert!(
        !text.contains("a trace line"),
        "trace is below the file level"
    );
    assert!(!text.contains("old session"), "the new file starts empty");
    assert!(!text.contains('\u{1b}'), "no colour codes in the file");

    // 判死报告：调用方只拼文本，落盘在写线程上，刷盘之后一定在。
    // The island-death report: the caller only builds the text; the writer
    // thread writes it, and it is there after a flush.
    mc_bamboo_game::diagnostics::write_island_dead_report("island 3 declared dead: test body");
    setup.guard.flush();
    let dead = files_with(&crash, "island-dead-", "-server.txt");
    assert_eq!(dead.len(), 1, "{dead:?}");
    let text = fs::read_to_string(&dead[0]).unwrap();
    assert!(text.contains("island 3 declared dead: test body"), "{text}");
    assert!(text.contains("All island progress beacons"), "{text}");
    assert!(text.contains("Online player positions"), "{text}");
    assert!(text.contains("for Minecraft"), "{text}");

    // panic：不用再显式刷盘，钩子自己把报告写好、把日志刷到磁盘。
    // A panic: no explicit flush; the hook writes the report and flushes the log.
    let result = std::panic::catch_unwind(|| panic!("logging probe panic"));
    assert!(result.is_err());
    let reports = files_with(&crash, "crash-", "-server.txt");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = fs::read_to_string(&reports[0]).unwrap();
    assert!(report.contains("Message: logging probe panic"), "{report}");
    assert!(report.contains("-- Backtrace --"), "{report}");
    let text = fs::read_to_string(logs.join("latest.log")).unwrap();
    assert!(text.contains("panic: logging probe panic"), "{text}");
    let name = reports[0].file_name().unwrap().to_string_lossy();
    assert!(
        text.contains(name.as_ref()),
        "the log names the report file"
    );
}
