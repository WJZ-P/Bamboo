//! 四个「代码生成」单件结构的结构件布局对拍：官方 `StructureSingleOracle` 对
//! `mc-bamboo-worldgen::structure::single` 的纯函数实现。
//!
//! 每条查询由 Rust 生成 queries.txt，Java 侧照官方 `generatePieces` 编排用官方构造器
//! 造结构件并按统一格式 dump；Rust 侧用生产代码复算，逐行比 `P` 行（类型、包围盒、
//! O、GD、rotation、mirror、子类参数）。任一字段不同即报不一致。
//!
//! Oracle for the piece layout of the four code-built single-piece structures: vanilla
//! `StructureSingleOracle` vs `mc-bamboo-worldgen::structure::single`. Rust writes
//! queries.txt; Java builds pieces via the official orchestration and dumps them in a
//! canonical format; Rust recomputes with production code and compares every `P` line.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::{pieces::Piece, single};

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
/// 覆盖不同区块坐标（含 <9 与负偏移，喂 BuriedTreasure 的 getBlockX(9)）。
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
const KINDS: [&str; 4] = [
    "buried_treasure",
    "desert_pyramid",
    "jungle_temple",
    "swamp_hut",
];

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries = build_queries();
    write_queries(queries_path, &queries)?;

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureSingleOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureSingleOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&queries, &String::from_utf8_lossy(&output.stdout))
}

fn build_queries() -> Vec<Query> {
    let mut queries = Vec::new();
    for &seed in &SEEDS {
        for &(cx, cz) in &CHUNKS {
            for &kind in &KINDS {
                queries.push(Query { kind, seed, cx, cz });
            }
        }
    }
    queries
}

fn write_queries(path: &Path, queries: &[Query]) -> Result<(), String> {
    let mut text = String::new();
    for q in queries {
        text.push_str(&format!("{} {} {} {}\n", q.kind, q.seed, q.cx, q.cz));
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
                let key = fields[1..5].join(" ");
                let count: usize = fields[5]
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
    format!("{} {} {} {}", q.kind, q.seed, q.cx, q.cz)
}

fn rust_lines(q: &Query) -> Result<Vec<String>, String> {
    let pieces: Vec<Piece> = single::generate(q.kind, q.seed, q.cx, q.cz)?;
    Ok(pieces.iter().map(Piece::plain_line).collect())
}

fn compare(queries: &[Query], stdout: &str) -> Result<(), String> {
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
    // 记下每个 kind 出现过的朝向 O 与包围盒 X 跨度，确认 nextInt(4) 真的被比到四个方向、
    // makeBoundingBox 的方向交换真的被比到两种形状。
    // Track distinct orientation O and box X-spans per kind, to confirm the nextInt(4) draw
    // really hits all four directions and the width/depth swap hits both box shapes.
    let mut orientations: std::collections::BTreeMap<&str, std::collections::BTreeSet<i32>> =
        std::collections::BTreeMap::new();
    let mut x_spans: std::collections::BTreeMap<&str, std::collections::BTreeSet<i32>> =
        std::collections::BTreeMap::new();

    for (q, (key, java_lines)) in queries.iter().zip(sections.iter()) {
        let expected_key = query_key(q);
        if key != &expected_key {
            return Err(format!(
                "query key mismatch: oracle {key:?} vs {expected_key:?}"
            ));
        }
        let ours = rust_lines(q)?;
        let n = java_lines.len().max(ours.len());
        pieces_total += java_lines.len();
        for line in java_lines {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 12 && f[0] == "P" {
                let o: i32 = f[8].parse().unwrap_or(-99);
                let min_x: i32 = f[2].parse().unwrap_or(0);
                let max_x: i32 = f[5].parse().unwrap_or(0);
                orientations.entry(q.kind).or_default().insert(o);
                x_spans.entry(q.kind).or_default().insert(max_x - min_x + 1);
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

    let orientations: Vec<String> = orientations
        .iter()
        .map(|(k, v)| format!("{k}:O{:?}", v.iter().collect::<Vec<_>>()))
        .collect();
    let x_spans: Vec<String> = x_spans
        .iter()
        .map(|(k, v)| format!("{k}:X{:?}", v.iter().collect::<Vec<_>>()))
        .collect();
    println!(
        "structure-single: {} queries ({}), {} pieces, {} line comparisons",
        queries.len(),
        KINDS.join(","),
        pieces_total,
        total
    );
    println!(
        "structure-single branches: orientations [{}], x-spans [{}]",
        orientations.join(", "),
        x_spans.join(", ")
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-SINGLE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-SINGLE FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-SINGLE RESULT: {mismatch_count} mismatches"
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
