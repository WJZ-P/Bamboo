//! 需要模板的单件结构件布局对拍：官方 `StructureTemplatedOracle` 对
//! `mc-bamboo-worldgen::structure::templated` 的纯函数实现（已移植雪屋）。
//!
//! 每条查询由 Rust 生成 queries.txt，Java 侧照官方 `generatePieces` 编排用官方构造器
//! 造结构件并按统一格式 dump；Rust 侧用生产代码复算，逐行比 `P` 行（类型、包围盒、
//! O、GD、rotation、mirror、子类参数）。任一字段不同即报不一致。
//!
//! Oracle for the piece layout of templated single-piece structures: vanilla
//! `StructureTemplatedOracle` vs `mc-bamboo-worldgen::structure::templated`. Rust writes
//! queries.txt; Java builds pieces via the official orchestration and dumps them in a
//! canonical format; Rust recomputes with production code and compares every `P` line.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::{
    Structures,
    pieces::Piece,
    templated::{self, ocean_ruin::Config},
};
use serde_json::Value;

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 8] = [
    0,
    1,
    -1,
    42,
    12_345_678_901,
    -98_765_432_123,
    2_147_483_647,
    -2_147_483_648,
];
/// 覆盖不同区块坐标（含负偏移）。
const CHUNKS: [(i32, i32); 12] = [
    (0, 0),
    (1, 1),
    (-1, -1),
    (34, 34),
    (-34, 50),
    (100, -100),
    (1000, 1000),
    (-1000, 1000),
    (12, -7),
    (250, 250),
    (-500, 300),
    (7, -13),
];
const KINDS: [&str; 5] = [
    "igloo",
    "ocean_ruin_cold",
    "ocean_ruin_warm",
    "shipwreck_beached",
    "shipwreck_ocean",
];

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    arg: i32,
}

pub fn run(
    mut command: Command,
    worldgen_path: &Path,
    queries_path: &Path,
    _options: &[&str],
) -> Result<(), String> {
    let text = fs::read_to_string(worldgen_path)
        .map_err(|error| format!("cannot read worldgen.json: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let raw = value.as_object().ok_or("worldgen.json must be an object")?;
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;
    // 海底遗迹的两个概率从 `worldgen.json` 读，不再手写常量。
    // The two ocean-ruin probabilities come from `worldgen.json` instead of hand-written
    // constants.
    let mut configs = std::collections::HashMap::new();
    for id in ["minecraft:ocean_ruin_warm", "minecraft:ocean_ruin_cold"] {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("missing structure {id}"))?;
        configs.insert(id, Config::parse(&def.raw)?);
    }

    let queries = build_queries();
    write_queries(queries_path, &queries)?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureTemplatedOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureTemplatedOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&configs, &queries, &String::from_utf8_lossy(&output.stdout))
}

fn build_queries() -> Vec<Query> {
    let mut queries = Vec::new();
    for &seed in &SEEDS {
        for &(cx, cz) in &CHUNKS {
            for &kind in &KINDS {
                queries.push(Query {
                    kind,
                    seed,
                    cx,
                    cz,
                    arg: 0,
                });
            }
        }
    }
    queries
}

fn write_queries(path: &Path, queries: &[Query]) -> Result<(), String> {
    let mut text = String::new();
    for q in queries {
        text.push_str(&format!(
            "{} {} {} {} {}\n",
            q.kind, q.seed, q.cx, q.cz, q.arg
        ));
    }
    fs::write(path, text).map_err(|error| format!("cannot write queries file: {error}"))
}

/// 把 Java 输出解析成「查询键 → P 行列表」。
fn parse_output(stdout: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<(String, usize, Vec<String>)> = None;
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "Q" => {
                if let Some((key, count, pieces)) = current.take() {
                    if pieces.len() != count {
                        return Err(format!(
                            "Q {key}: expected {count} P lines, got {}",
                            pieces.len()
                        ));
                    }
                    sections.push((key, pieces));
                }
                // 键 = "kind seed cx cz arg"（去掉数量字段）。
                let key = fields[1..6].join(" ");
                let count: usize = fields[6]
                    .parse()
                    .map_err(|_| format!("bad count in line: {line}"))?;
                current = Some((key, count, Vec::new()));
            }
            "P" => match &mut current {
                Some((_, _, pieces)) => pieces.push(line.to_owned()),
                None => return Err(format!("P line before any Q: {line}")),
            },
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    if let Some((key, count, pieces)) = current {
        if pieces.len() != count {
            return Err(format!(
                "Q {key}: expected {count} P lines, got {}",
                pieces.len()
            ));
        }
        sections.push((key, pieces));
    }
    Ok(sections)
}

fn query_key(q: &Query) -> String {
    format!("{} {} {} {} {}", q.kind, q.seed, q.cx, q.cz, q.arg)
}

/// 按 kind 调生产代码，产出规范化的 `P` 行。
fn rust_lines(
    configs: &std::collections::HashMap<&str, Config>,
    q: &Query,
) -> Result<Vec<String>, String> {
    let ocean = |id: &str| -> Result<Config, String> {
        configs
            .get(id)
            .copied()
            .ok_or_else(|| format!("missing ocean ruin config {id}"))
    };
    let pieces: Vec<Piece> = match q.kind {
        "igloo" => templated::igloo::generate(q.seed, q.cx, q.cz)?,
        "ocean_ruin_warm" => templated::ocean_ruin::generate(
            q.seed,
            q.cx,
            q.cz,
            ocean("minecraft:ocean_ruin_warm")?,
        )?,
        "ocean_ruin_cold" => templated::ocean_ruin::generate(
            q.seed,
            q.cx,
            q.cz,
            ocean("minecraft:ocean_ruin_cold")?,
        )?,
        "shipwreck_ocean" => templated::shipwreck::generate(q.seed, q.cx, q.cz, false)?,
        "shipwreck_beached" => templated::shipwreck::generate(q.seed, q.cx, q.cz, true)?,
        other => return Err(format!("no Rust layout for kind {other}")),
    };
    Ok(pieces.iter().map(Piece::canonical_line).collect())
}

fn compare(
    configs: &std::collections::HashMap<&str, Config>,
    queries: &[Query],
    stdout: &str,
) -> Result<(), String> {
    let sections = parse_output(stdout)?;
    if sections.len() != queries.len() {
        return Err(format!(
            "oracle emitted {} Q sections, expected {}",
            sections.len(),
            queries.len()
        ));
    }

    let mut mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut total = 0usize;
    let mut pieces_total = 0usize;
    let mut kind_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    // 记下每个 kind 出现过的模板名与朝向，确认 rotation 的四个值和有 / 无地下室两条分支
    // 真的被比到。
    // Track the templates and rotations seen per kind to confirm all four rotation values
    // and both basement branches are really compared.
    let mut templates: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut rotations: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut depth_counts: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    for (q, (key, java_lines)) in queries.iter().zip(sections.iter()) {
        let expected_key = query_key(q);
        if key != &expected_key {
            return Err(format!(
                "query key mismatch: oracle {key:?} vs {expected_key:?}"
            ));
        }
        *kind_counts.entry(q.kind).or_default() += 1;
        let ours = rust_lines(configs, q)?;
        let n = java_lines.len().max(ours.len());
        pieces_total += java_lines.len();
        depth_counts.insert(java_lines.len());
        for line in java_lines {
            for field in line.split_whitespace().flat_map(|f| f.split(',')) {
                if let Some(value) = field.strip_prefix("Template=") {
                    templates.insert(value.to_owned());
                }
                if let Some(value) = field.strip_prefix("Rot=") {
                    rotations.insert(value.to_owned());
                }
            }
        }
        for i in 0..n {
            total += 1;
            let j = java_lines.get(i).map(String::as_str).unwrap_or("<missing>");
            let o = ours.get(i).map(String::as_str).unwrap_or("<missing>");
            if j != o {
                mismatch_count += 1;
                push(
                    &mut mismatches,
                    format!("[{key}] piece {i}:\n  java: {j}\n  rust: {o}"),
                );
            }
        }
    }

    let kinds = kind_counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    let templates = templates.into_iter().collect::<Vec<_>>().join(",");
    let rotations = rotations.into_iter().collect::<Vec<_>>().join(",");
    let depths = depth_counts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "structure-templated: {} queries ({}), {} pieces, {} line comparisons",
        queries.len(),
        kinds,
        pieces_total,
        total
    );
    println!(
        "structure-templated branches: templates [{templates}], rotations [{rotations}], \
         piece counts [{depths}]"
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-TEMPLATED RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-TEMPLATED FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-TEMPLATED RESULT: {mismatch_count} mismatches"
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
