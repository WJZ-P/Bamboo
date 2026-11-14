//! 结构放置的对拍：用官方 `RandomSpreadStructurePlacement` 比 `mc-bamboo-worldgen` 的
//! 纯函数实现。
//!
//! 大范围区块（多种子、坐标跨 ±100 万）逐格比 `getPotentialStructureChunk` 和
//! `applyAdditionalChunkRestrictions`。Java 侧 `StructurePlacementOracle.java` 用 Gson 从
//! 同一份 `worldgen.json` 独立读 `structure_set` 参数并造官方 placement；这里用
//! `mc-bamboo-worldgen::structure::StructureSets` 解析同一份文件再复算，两边解析、随机源
//! 都独立，任一侧错了都要出不一致。concentric_rings 需要群系吸附，本轮跳过。
//!
//! Placement oracle: compare `mc-bamboo-worldgen`'s pure functions against vanilla's
//! `RandomSpreadStructurePlacement`. Over a large chunk grid (multiple seeds, coordinates
//! spanning +/-1,000,000) it compares `getPotentialStructureChunk` and
//! `applyAdditionalChunkRestrictions` per query. Java reads the same `worldgen.json` via
//! Gson and builds the official placements; Rust parses it with `StructureSets` and
//! recomputes. Both parsing and random sources are independent, so either side being wrong
//! shows up as a mismatch. `concentric_rings` needs biome snapping and is skipped here.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::{StructureSets, selection_sequence};
use serde_json::Value;

/// 覆盖原点、固定点和很大正负数的种子，含 0、±1、大数和 int 边界。
///
/// Seeds covering the origin, fixed points and large positive/negative values, including
/// 0, +/-1, big numbers and int boundaries.
const SEEDS: [i64; 8] = [
    0,
    1,
    -1,
    42,
    12_345_678_901,
    -98_765_432_123,
    i32::MAX as i64,
    i32::MIN as i64,
];

pub fn run(
    mut command: Command,
    worldgen_path: &Path,
    queries_path: &Path,
    _options: &[&str],
) -> Result<(), String> {
    // 两边都从磁盘上的同一份 worldgen.json 读；Rust 侧解析用生产代码 StructureSets。
    // Both sides read the same worldgen.json from disk; Rust parses with the production
    // `StructureSets` so the oracle also exercises the shipping parser.
    let text = fs::read_to_string(worldgen_path)
        .map_err(|error| format!("cannot read worldgen.json: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let sets_value = value
        .get("structure_set")
        .ok_or("worldgen.json missing structure_set")?;
    let sets = StructureSets::parse(sets_value)?;

    let queries = build_queries();
    write_queries(queries_path, &queries)?;

    let output = command
        .arg(worldgen_path)
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructurePlacementOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructurePlacementOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(
        &sets,
        &String::from_utf8_lossy(&output.stdout),
        queries.len(),
    )
}

/// 生成 `(seed, x, z)` 查询：每个种子几个固定区块，再加一串确定性 LCG 的大范围坐标。
///
/// Build `(seed, x, z)` queries: a few fixed chunks per seed plus deterministic LCG
/// coordinates spanning a wide range.
fn build_queries() -> Vec<(i64, i32, i32)> {
    const FIXED: [(i32, i32); 10] = [
        (0, 0),
        (1, 1),
        (-1, -1),
        (34, 34),
        (-34, 50),
        (100, -100),
        (1000, 1000),
        (-1_000_000, 1_000_000),
        (999_999, -999_999),
        (100, 100),
    ];
    let mut out = Vec::new();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for &seed in &SEEDS {
        for &(x, z) in &FIXED {
            out.push((seed, x, z));
        }
        for _ in 0..48 {
            state = lcg(state);
            let x = (state >> 33) as i32;
            state = lcg(state);
            let z = (state >> 33) as i32;
            out.push((seed, x, z));
        }
    }
    out
}

fn lcg(state: u64) -> u64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
}

fn write_queries(path: &Path, queries: &[(i64, i32, i32)]) -> Result<(), String> {
    let mut text = String::new();
    for (seed, x, z) in queries {
        text.push_str(&format!("{seed} {x} {z}\n"));
    }
    fs::write(path, text).map_err(|error| format!("cannot write queries file: {error}"))
}

/// 失败信息里的序列最多留 120 个字符。
///
/// Keep at most 120 characters of a sequence in a failure line.
fn truncate(text: &str) -> &str {
    text.get(..text.len().min(120)).unwrap_or(text)
}

fn compare(sets: &StructureSets, stdout: &str, query_count: usize) -> Result<(), String> {
    let mut lines = stdout.lines();

    // 第一行 `SETS n`，接着 n 行结构集名字。两边集合必须一致（顺序无关）。
    // First line `SETS n`, then n set names. Both sides must see the same set (order-free).
    let header = lines.next().ok_or("empty oracle output")?;
    let count: usize = header
        .strip_prefix("SETS ")
        .ok_or_else(|| format!("bad header {header}"))?
        .parse()
        .map_err(|_| format!("bad header count {header}"))?;
    let mut java_names = Vec::with_capacity(count);
    for _ in 0..count {
        java_names.push(lines.next().ok_or("missing set name")?.to_owned());
    }
    let mut expected = sets.random_spread_names();
    let mut actual = java_names.clone();
    expected.sort();
    actual.sort();
    if expected != actual {
        return Err(format!(
            "random_spread set names differ: Rust {expected:?} vs Java {actual:?}"
        ));
    }

    let mut mismatches = Vec::new();
    let mut total_mismatches = 0usize;
    let mut comparisons = 0usize;
    // 记一下频率判定为 false 的条数，确认频率削减的分支真的被比到，不是恒 true。
    // Count false frequency restrictions to confirm the reducers are actually exercised.
    let mut false_restrictions = 0usize;
    let mut selection_comparisons = 0usize;
    let mut in_selection = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        // R 段之后是 "SELECT n"，再往后是加权威选序列。
        // After the R section comes "SELECT n", then the weighted-selection lines.
        if let Some(select_count) = line.strip_prefix("SELECT ") {
            let select_count: usize = select_count
                .parse()
                .map_err(|_| format!("bad SELECT count: {line}"))?;
            if select_count != sets.len() {
                return Err(format!(
                    "SELECT set count {select_count} != Rust {} ",
                    sets.len()
                ));
            }
            in_selection = true;
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let name = *fields
            .get(1)
            .ok_or_else(|| format!("bad oracle line: {line}"))?;
        let seed: i64 = fields[2].parse().map_err(|_| format!("bad seed: {line}"))?;
        let x: i32 = fields[3].parse().map_err(|_| format!("bad x: {line}"))?;
        let z: i32 = fields[4].parse().map_err(|_| format!("bad z: {line}"))?;
        let set = sets
            .get(name)
            .ok_or_else(|| format!("unknown set in oracle output: {name}"))?;

        if in_selection {
            // `S name seed x z a,b,c`：官方 createStructures 的加权尝试序列。
            // `S name seed x z a,b,c`: official createStructures attempt sequence.
            if fields.len() != 6 || fields[0] != "S" {
                return Err(format!("bad selection line: {line}"));
            }
            let ours = selection_sequence(&set.structures, seed, x, z).join(",");
            selection_comparisons += 1;
            if ours != fields[5] {
                total_mismatches += 1;
                if mismatches.len() < 24 {
                    // 多结构集的序列很长，失败信息截断，避免刷屏。
                    // Multi-structure sequences are long; truncate failure lines.
                    mismatches.push(format!(
                        "selection {name} seed={seed} ({x},{z}): ours [{}] vs [{}]",
                        truncate(&ours),
                        truncate(fields[5])
                    ));
                }
            }
            continue;
        }

        if fields.len() != 8 || fields[0] != "R" {
            return Err(format!("bad oracle line: {line}"));
        }
        let px: i32 = fields[5].parse().map_err(|_| format!("bad px: {line}"))?;
        let pz: i32 = fields[6].parse().map_err(|_| format!("bad pz: {line}"))?;
        let restrict = fields[7] == "1";
        if !restrict {
            false_restrictions += 1;
        }
        let spread = set
            .placement
            .as_random_spread()
            .ok_or_else(|| format!("set {name} is not random_spread"))?;
        let (our_px, our_pz) = spread.get_potential_structure_chunk(seed, x, z);
        let our_restrict = set
            .placement
            .apply_additional_chunk_restrictions(seed, x, z);

        comparisons += 1;
        if our_px != px || our_pz != pz || our_restrict != restrict {
            total_mismatches += 1;
            if mismatches.len() < 24 {
                mismatches.push(format!(
                    "{name} seed={seed} ({x},{z}): potential ({our_px},{our_pz}) vs ({px},{pz}), restrict {our_restrict} vs {restrict}"
                ));
            }
        }
    }

    println!(
        "structures: {} sets, {} queries, {} placement comparisons ({} false frequency restrictions), {} selection comparisons",
        count, query_count, comparisons, false_restrictions, selection_comparisons
    );
    if total_mismatches == 0 {
        println!("STRUCTURES RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURES FAIL: {line}");
        }
        Err(format!("STRUCTURES RESULT: {total_mismatches} mismatches"))
    }
}
