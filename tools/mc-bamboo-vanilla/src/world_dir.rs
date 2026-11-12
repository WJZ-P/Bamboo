//! `mc-bamboo-vanilla world-dir <目录> <java 路径>`：用官方
//! `RegionFile` + `SerializableChunkData.parse` + `NbtIo.readCompressed` 逐项检查
//! Bamboo 写出的真实世界目录，失败返回非 0。先用一个故意写坏的 `level.dat`
//! 自检，确认 oracle 真的会报错，再相信主目录的结果。
//!
//! Verify a real Bamboo world directory with the official codecs; exit non-zero
//! on failure. A built-in negative check (a deliberately broken `level.dat`)
//! proves the oracle actually reports errors before we trust the main result.

use std::{fs, io::Write, path::Path, process::Stdio};

use serde::Deserialize;
use serde_json::json;

const SOURCE: &str = "tools/mc-bamboo-vanilla/java/StorageOracle.java";

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    chunks: u64,
    #[serde(default)]
    #[serde(rename = "entityChunks")]
    entity_chunks: u64,
    #[serde(default)]
    players: u64,
    #[serde(default)]
    #[serde(rename = "savedData")]
    saved_data: u64,
    #[serde(default)]
    errors: Vec<String>,
}

/// 跑一次官方 `world-dir`，返回逐项报告。
/// Run the official `world-dir` command once and return its per-item report.
fn ask(java: &str, dir: &Path) -> Result<Report, String> {
    let mut child = crate::official_program(java, SOURCE)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
        let request = json!({ "command": "world-dir", "dir": dir.to_string_lossy() });
        stdin
            .write_all(request.to_string().as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("cannot write java stdin: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.contains("\"chunks\""))
        .ok_or_else(|| {
            format!(
                "no world-dir report from java (status {}): {}\n{}",
                output.status,
                text,
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "StorageOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_str::<Report>(line)
        .map_err(|error| format!("bad world-dir JSON {line}: {error}"))
}

/// 造一个只有坏 `level.dat` 的最小世界目录，用来做「故意弄坏一处」的自检。
/// Build a minimal world dir with a broken `level.dat` for the negative check.
fn make_broken_dir() -> Result<std::path::PathBuf, String> {
    let dir = crate::workspace_root().join("target/vanilla/world-dir-broken");
    let _ = fs::remove_dir_all(&dir);
    for sub in [
        "dimensions/minecraft/overworld/region",
        "dimensions/minecraft/overworld/entities",
        "dimensions/minecraft/overworld/data/minecraft",
        "players/data",
        "data/minecraft",
    ] {
        fs::create_dir_all(dir.join(sub))
            .map_err(|error| format!("cannot create broken dir: {error}"))?;
    }
    fs::write(dir.join("level.dat"), b"not a gzip NBT file")
        .map_err(|error| format!("cannot write broken level.dat: {error}"))?;
    Ok(dir)
}

pub fn run(dir: &str, java: &str) -> Result<(), String> {
    // Java 的工作目录是 target/vanilla，相对路径要转成绝对路径。
    // Java runs with cwd target/vanilla, so resolve the path first.
    let absolute = fs::canonicalize(dir)
        .map_err(|error| format!("cannot resolve world dir {dir:?}: {error}"))?;

    // 1. 自检：故意写坏一处，oracle 必须报错，否则不能相信后面的「通过」。
    // Negative check: a broken file must be reported, otherwise a "pass" is meaningless.
    let broken = make_broken_dir()?;
    let negative = ask(java, &broken)?;
    if negative.errors.is_empty() {
        return Err("official oracle accepted a world dir with a broken level.dat".to_owned());
    }
    let _ = fs::remove_dir_all(&broken);

    // 2. 真正的目录：逐项报告，任何错误都返回非 0。
    // The real directory: report each item; any error is a non-zero failure.
    let report = ask(java, &absolute)?;
    println!(
        "world-dir: chunks={} entity_chunks={} players={} saved_data={}",
        report.chunks, report.entity_chunks, report.players, report.saved_data
    );
    for error in &report.errors {
        eprintln!("world-dir error: {error}");
    }
    if !report.errors.is_empty() {
        return Err(format!(
            "official world-dir verification failed with {} error(s)",
            report.errors.len()
        ));
    }
    if report.chunks == 0 {
        return Err("official world-dir verification found no chunks".to_owned());
    }
    println!("WORLD-DIR RESULT: official codecs read every item; broken-file self-check passed");
    Ok(())
}
