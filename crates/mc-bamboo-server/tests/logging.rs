//! 文件日志装配的集成测试，单独一个测试二进制：`setup` 会装全局 panic 钩子。
//!
//! Integration test for file-log setup, in a dedicated test binary because
//! `setup` installs a global panic hook.

use std::{fs, path::PathBuf};

use mc_bamboo_server::logging;

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("bamboo-logging-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        Self(temp_dir())
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// setup 建出 logs/latest.log 和 crash-reports/，把上一份 latest.log 归档成 .gz。
///
/// setup creates logs/latest.log and crash-reports/ and archives the previous
/// latest.log as .gz.
#[test]
fn setup_creates_the_log_and_crash_dirs_and_archives_the_old_log() {
    let temp = Temp::new();
    let logs = temp.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("latest.log"), b"old session\n").unwrap();

    let setup = logging::setup(temp.path(), "debug", "");

    assert!(
        logs.join("latest.log").exists(),
        "a fresh latest.log is created"
    );
    assert!(
        temp.path().join("crash-reports").is_dir(),
        "the crash-reports directory is created"
    );
    let archived = fs::read_dir(&logs)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("gz"));
    assert!(archived, "the previous latest.log was archived as .log.gz");
    assert!(
        !setup.directive.is_empty(),
        "a filter directive is produced"
    );
    // 写入器活到测试结束，写线程才有机会把还没发的行落盘。
    // Keep the writer alive until the end so the worker can flush pending lines.
    drop(setup);
}
