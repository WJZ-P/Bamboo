//! 日志归档和崩溃报告的集成测试，单独一个测试二进制：`configure` 装的全局 panic
//! 钩子只在这个进程里生效，不会渗到别的测试二进制。
//!
//! Integration tests for log archiving and crash reports, in a dedicated test
//! binary so the global panic hook installed by `configure` stays isolated.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use mc_bamboo_game::diagnostics;

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("bamboo-diagnostics-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        Self(temp_dir())
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

/// 启动时把上一份 latest.log 按官方命名 gzip 归档，内容逐字节还原。
///
/// Startup gzips the previous latest.log under the official name; the content
/// round-trips byte for byte.
#[test]
fn archiving_gzips_the_previous_latest_log() {
    let temp = Temp::new();
    let logs = temp.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("latest.log"), b"previous session log\nsecond line\n").unwrap();

    diagnostics::archive_latest_log(&logs);

    assert!(
        !logs.join("latest.log").exists(),
        "the old latest.log is moved away"
    );
    let gz = fs::read_dir(&logs)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("gz"))
        .expect("a .log.gz archive");
    let name = gz.file_name().unwrap().to_string_lossy();
    assert!(name.ends_with(".log.gz"), "{name}");
    assert!(
        name.chars()
            .take(10)
            .all(|ch| ch.is_ascii_digit() || ch == '-'),
        "name starts with a YYYY-MM-DD date: {name}"
    );
    let mut decoded = String::new();
    flate2::read::GzDecoder::new(fs::File::open(&gz).unwrap())
        .read_to_string(&mut decoded)
        .unwrap();
    assert_eq!(decoded, "previous session log\nsecond line\n");
}

/// panic 和岛判死各写一份报告，含消息、位置、线程、版本、信标。
///
/// A panic and an island death each write a report with message, location,
/// thread, build info and beacons.
#[test]
fn a_panic_and_an_island_death_each_write_a_report() {
    let temp = Temp::new();
    let crash = temp.path().join("crash-reports");
    diagnostics::configure(&crash, "Bamboo test (commit test)");
    assert!(crash.is_dir());

    // 只在测试里触发 panic，生产路径没有这个开关。
    // The panic is triggered only in this test; there is no such switch in the
    // production path.
    let result = std::panic::catch_unwind(|| panic!("diagnostics probe panic"));
    assert!(result.is_err());
    let report = find(&crash, "crash-");
    let text = fs::read_to_string(&report).unwrap();
    assert!(text.contains("diagnostics probe panic"), "{text}");
    assert!(text.contains("Thread:"), "{text}");
    assert!(text.contains("Location:"), "{text}");
    assert!(text.contains("Bamboo test (commit test)"), "{text}");
    assert!(text.contains("Island progress beacons"), "{text}");
    assert!(
        text.contains("Backtrace") || text.contains("backtrace"),
        "{text}"
    );

    diagnostics::write_island_dead_report("island 3 declared dead: phase entities");
    let dead = find(&crash, "island-dead-");
    let text = fs::read_to_string(&dead).unwrap();
    assert!(text.contains("island 3 declared dead"), "{text}");
    assert!(text.contains("All island progress beacons"), "{text}");
    assert!(text.contains("Bamboo test (commit test)"), "{text}");
}

/// 找出 crash-reports 目录里以 `prefix` 开头、以 -server.txt 结尾的一份报告。
///
/// Find one report in the crash-reports directory with the given prefix.
fn find(dir: &Path, prefix: &str) -> PathBuf {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with("-server.txt"))
        })
        .unwrap_or_else(|| panic!("no {prefix} report in {}", dir.display()))
}
