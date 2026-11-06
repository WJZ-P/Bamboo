//! 运行官方数据生成器和 `Extractor.java`，读回它们的输出。

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

/// 官方报告里一个连接状态的数据包：方向 → 包名 → ID。
pub type PacketReport = BTreeMap<String, BTreeMap<String, PacketEntry>>;

#[derive(Debug, Deserialize)]
pub struct PacketEntry {
    pub protocol_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub name: String,
    pub protocol: i32,
    pub data_version: i32,
}

#[derive(Debug, Deserialize)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncedRegistry {
    pub name: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TagGroup {
    pub registry: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub name: String,
    pub ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BlockStates {
    pub total: u32,
    pub defaults: BTreeMap<String, u32>,
}

/// 生成代码需要的全部输入。
pub struct VersionData {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
    /// 连接状态 → 方向 → 包名 → ID。
    pub packets: BTreeMap<String, PacketReport>,
}

pub fn run(
    java: &str,
    server_jar: &Path,
    work_dir: &Path,
    workspace_root: &Path,
) -> Result<VersionData, String> {
    run_reports(java, server_jar, work_dir)?;
    let extracted_path = work_dir.join("extracted.json");
    run_extractor(java, work_dir, workspace_root, &extracted_path)?;

    let extracted: Extracted = read_json(&extracted_path)?;
    let packets = read_packet_report(&work_dir.join("generated/reports/packets.json"))?;
    Ok(VersionData {
        version: extracted.version,
        known_packs: extracted.known_packs,
        enabled_features: extracted.enabled_features,
        synced_registries: extracted.synced_registries,
        tags: extracted.tags,
        block_states: extracted.block_states,
        packets,
    })
}

/// 运行官方的数据生成器。它同时会把依赖库解压到工作目录里。
fn run_reports(java: &str, server_jar: &Path, work_dir: &Path) -> Result<(), String> {
    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-DbundlerMainClass=net.minecraft.data.Main")
        .arg("-jar")
        .arg(server_jar)
        .args(["--reports", "--output", "generated"])
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("官方数据生成器退出失败：{status}"))
    }
}

fn run_extractor(
    java: &str,
    work_dir: &Path,
    workspace_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let source = workspace_root.join("tools/mc-bamboo-vanilla/java/Extractor.java");
    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-cp")
        .arg(classpath(work_dir)?)
        .arg(source)
        .arg(output)
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Extractor.java 退出失败：{status}"))
    }
}

/// 官方数据生成器解压出来的全部 jar，运行 Java 工具时用作 classpath。
pub fn classpath(work_dir: &Path) -> Result<OsString, String> {
    let mut jars = Vec::new();
    collect_jars(&work_dir.join("libraries"), &mut jars)?;
    collect_jars(&work_dir.join("versions"), &mut jars)?;
    env::join_paths(jars).map_err(|error| error.to_string())
}

fn collect_jars(directory: &Path, jars: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("无法读取 {}：{error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_jars(&path, jars)?;
        } else if path.extension().is_some_and(|extension| extension == "jar") {
            jars.push(path);
        }
    }
    Ok(())
}

fn read_packet_report(path: &Path) -> Result<BTreeMap<String, PacketReport>, String> {
    read_json(path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} 格式不对：{error}", path.display()))
}
