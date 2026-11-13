//! `mc-bamboo-vanilla world-block <目录> <x> <y> <z> <方块名> <java 路径>`：
//! 用官方 `RegionFile` / `SerializableChunkData.parse` 从磁盘上的一个真实世界目录里
//! 读出一格方块，确认它还是期望的方块。给 `tools/crash-test.sh` 检查 `kill -9`
//! 之前改过的那一格重启后还在用；找不到、解不开、方块不对都返回非 0。
//!
//! Read one block out of a real world directory with the official codecs.
//! Used by the crash test to prove a block edited before `kill -9` survived.
//! Missing/unreadable chunks or a mismatched block are errors.

use std::{fs, io::Write, process::Stdio};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{blocks, version};
use mc_bamboo_nbt::{Accounter, Tag, read_named};
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionPos, open_region},
    chunk_nbt::field,
};
use mc_bamboo_world::MIN_Y;
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE: &str = "tools/mc-bamboo-vanilla/java/StorageOracle.java";

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    block: Option<String>,
}

pub fn run(dir: &str, x: &str, y: &str, z: &str, block: &str, java: &str) -> Result<(), String> {
    let at = BlockPos::new(
        x.parse().map_err(|_| format!("bad x coordinate {x:?}"))?,
        y.parse().map_err(|_| format!("bad y coordinate {y:?}"))?,
        z.parse().map_err(|_| format!("bad z coordinate {z:?}"))?,
    );
    let expected = blocks::BLOCKS
        .iter()
        .find(|b| b.name == block)
        .ok_or_else(|| format!("unknown block {block:?}"))?;
    let root = fs::canonicalize(dir)
        .map_err(|error| format!("cannot resolve world dir {dir:?}: {error}"))?;
    let region_path = root.join("dimensions/minecraft/overworld/region");
    if !region_path.is_dir() {
        return Err(format!("no region directory at {}", region_path.display()));
    }
    // 官方目录对拍：逐个 .mca 找那个区块，用官方 codec 解码并读出这一格。
    let mut found: Vec<String> = Vec::new();
    for entry in
        fs::read_dir(&region_path).map_err(|error| format!("cannot read region dir: {error}"))?
    {
        let path = entry
            .map_err(|error| format!("bad region dir entry: {error}"))?
            .path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".mca") || !name.starts_with("r.") {
            continue;
        }
        let inner = &name[2..name.len() - 4];
        let mut parts = inner.split('.');
        let (Some(rx), Some(rz)) = (
            parts.next().and_then(|v| v.parse::<i32>().ok()),
            parts.next().and_then(|v| v.parse::<i32>().ok()),
        ) else {
            continue;
        };
        if parts.next().is_some() {
            continue;
        }
        let request = json!({
            "command": "world-block",
            "dir": path.to_string_lossy(),
            "rx": rx,
            "rz": rz,
            "x": at.x,
            "y": at.y,
            "z": at.z,
        });
        let report = ask_java(java, &request)?;
        if let Some(error) = &report.error {
            return Err(format!(
                "official codec rejected {}: {error}",
                path.display()
            ));
        }
        if let Some(name) = report.block {
            found.push(name);
        }
    }
    if found.len() != 1 {
        return Err(format!(
            "expected the block in exactly one region file, found {} entries",
            found.len()
        ));
    }
    // 官方返回的注册名就是这一格的解出的方块，和期望值逐字比较。
    if found[0] != expected.name {
        return Err(format!(
            "block at {at:?} is {} on disk, expected {}",
            found[0], expected.name
        ));
    }
    // 再用 Bamboo 自己的解码器读一遍，确认两边对同一格的答案一致。
    let chunk = ChunkPos::new(at.x.div_euclid(16), at.z.div_euclid(16));
    let mut region = open_region(&region_path, RegionPos::of(chunk), RegionCompression::Zlib)
        .map_err(|error| format!("cannot open region: {error}"))?;
    let bytes = region
        .read(chunk, 32 << 20)
        .map_err(|error| format!("cannot read {chunk:?}: {error}"))?
        .ok_or_else(|| format!("chunk {chunk:?} is not on disk"))?;
    let (name, entries) = read_named(&bytes, Accounter::uncompressed_quota())
        .map_err(|error| format!("Bamboo cannot read the chunk NBT: {error}"))?;
    if !name.is_empty() {
        return Err(format!("chunk NBT root name is {name:?}"));
    }
    let root_tag = Tag::Compound(entries);
    let data_version = match field(&root_tag, "DataVersion") {
        Some(Tag::Int(v)) => *v,
        _ => return Err("chunk NBT has no DataVersion".to_owned()),
    };
    if data_version != version::DATA_VERSION {
        return Err(format!("chunk DataVersion is {data_version}"));
    }
    let column = mc_bamboo_storage::chunk_nbt::decode_bytes(&bytes)
        .map_err(|error| format!("Bamboo cannot decode the chunk: {error}"))?;
    let local_x = at.x.rem_euclid(16) as usize;
    let local_z = at.z.rem_euclid(16) as usize;
    let state = column.column.chunk().block(local_x, at.y, local_z);
    let got = mc_bamboo_data::block_of_state(state)
        .map(|b| b.name)
        .ok_or_else(|| format!("unknown state {state} at {at:?}"))?;
    if got != expected.name {
        return Err(format!(
            "Bamboo reads {got} at {at:?}, expected {}",
            expected.name
        ));
    }
    if at.y < MIN_Y {
        return Err("marker y is below the world".to_owned());
    }
    println!(
        "WORLD-BLOCK RESULT: {at:?} = {} (official codec and Bamboo agree)",
        expected.name
    );
    Ok(())
}

fn ask_java(java: &str, request: &Value) -> Result<Report, String> {
    let mut child = crate::official_program(java, SOURCE)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
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
        .find(|line| line.contains("\"command\""))
        .ok_or_else(|| {
            format!(
                "no world-block report from java (status {}): {}\n{}",
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
        .map_err(|error| format!("bad world-block JSON {line}: {error}"))
}
