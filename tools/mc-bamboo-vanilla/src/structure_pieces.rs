//! 多件式结构的结构件布局对拍：官方 `StructurePiecesOracle` 对
//! `mc-bamboo-worldgen::structure::pieces` 的纯函数实现（已移植矿井 / 要塞 / 海底神殿）。
//!
//! 每条查询由 Rust 生成 queries.txt，Java 侧照官方编排生成结构件并按统一格式
//! dump；Rust 侧用生产代码复算，逐行比 `P` 行（类型、包围盒、O、GD、rotation、
//! mirror、子类参数）。任一字段不同即报不一致。
//!
//! Oracle for the piece-list layout of the four code-built multi-piece structures:
//! vanilla `StructurePiecesOracle` vs `mc-bamboo-worldgen::structure::pieces`.
//! Rust writes queries.txt; Java builds pieces via the official orchestration and
//! dumps them in a canonical format; Rust recomputes with production code and
//! compares every `P` line field by field.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::pieces::{self, Piece};

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
/// 覆盖不同群系带的区块坐标。
const CHUNKS: [(i32, i32); 8] = [
    (0, 0),
    (1, 1),
    (-1, -1),
    (34, 34),
    (-34, 50),
    (100, -100),
    (1000, 1000),
    (-1000, 1000),
];
/// 恶地类型喂入的 surfaceHeight：40（<= seaLevel，无随机）、100（有随机）。
const MESA_SURFACE_HEIGHTS: [i32; 2] = [40, 100];

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    arg: i32,
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries = build_queries();
    write_queries(queries_path, &queries)?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructurePiecesOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructurePiecesOracle failed: {}\n{}",
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
            queries.push(Query {
                kind: "mineshaft_normal",
                seed,
                cx,
                cz,
                arg: 0,
            });
            for &surface in &MESA_SURFACE_HEIGHTS {
                queries.push(Query {
                    kind: "mineshaft_mesa",
                    seed,
                    cx,
                    cz,
                    arg: surface,
                });
            }
            queries.push(Query {
                kind: "stronghold",
                seed,
                cx,
                cz,
                arg: 0,
            });
            queries.push(Query {
                kind: "ocean_monument",
                seed,
                cx,
                cz,
                arg: 0,
            });
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
fn rust_lines(q: &Query) -> Result<Vec<String>, String> {
    let pieces: Vec<Piece> = match q.kind {
        "mineshaft_normal" => pieces::mineshaft::generate(q.seed, q.cx, q.cz, false, q.arg),
        "mineshaft_mesa" => pieces::mineshaft::generate(q.seed, q.cx, q.cz, true, q.arg),
        "stronghold" => pieces::stronghold::generate(q.seed, q.cx, q.cz),
        "ocean_monument" => pieces::ocean_monument::generate(q.seed, q.cx, q.cz),
        other => return Err(format!("no Rust layout for kind {other}")),
    };
    Ok(pieces.iter().map(Piece::canonical_line).collect())
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
    let mut kind_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();

    for (q, (key, java_lines)) in queries.iter().zip(sections.iter()) {
        let expected_key = query_key(q);
        if key != &expected_key {
            return Err(format!(
                "query key mismatch: oracle {key:?} vs {expected_key:?}"
            ));
        }
        *kind_counts.entry(q.kind).or_default() += 1;
        let ours = rust_lines(q)?;
        let n = java_lines.len().max(ours.len());
        pieces_total += java_lines.len();
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
    println!(
        "structure-pieces: {} queries ({}), {} pieces, {} line comparisons",
        queries.len(),
        kinds,
        pieces_total,
        total
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-PIECES RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-PIECES FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-PIECES RESULT: {} mismatches",
            mismatch_count
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
