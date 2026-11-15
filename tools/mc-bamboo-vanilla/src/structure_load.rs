//! 结构存档读回路径的对拍：官方 `StructureLoadOracle` 对 `mc-bamboo-worldgen::structure::save`
//! 的 `load_piece` / `load_pieces` / `regenerate_monument`。
//!
//! Rust 写 queries.txt（`kind seed cx cz`），Java 用官方结构件构造器造件、`createTag` 存档、
//! `PiecesContainer.load` 读回、海底神殿再跑 `regeneratePiecesAfterLoad`；Rust 用生产代码走
//! 「布局 → `SavePiece::from_layout` → `to_tag` → `load_pieces`」同一条路，逐行比读回的
//! 类型 / 包围盒 / 朝向 / genDepth / 子类字段，以及重建后的房间图。
//!
//! Oracle for the read-back path of structure saving: vanilla `StructureLoadOracle`
//! against `structure::save`'s `load_piece` / `load_pieces` / `regenerate_monument`.
//! Rust writes queries.txt; Java builds pieces with the official constructors, saves
//! them via `createTag`, reloads through `PiecesContainer.load`, and reruns
//! `regeneratePiecesAfterLoad` for the ocean monument. Rust walks the same
//! layout → `SavePiece::from_layout` → `to_tag` → `load_pieces` path and compares every
//! reloaded field plus the rebuilt room graph.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::{
    pieces::{self, Piece},
    save::{SavePiece, load_pieces, loaded_piece_as_layout, regenerate_monument},
};

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 5] = [0, 42, -7, 12_345_678_901, -98_765_432_123];
/// 覆盖不同群系带的区块坐标。
const CHUNKS: [(i32, i32); 4] = [(0, 0), (1, -1), (34, 50), (-100, 100)];

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

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureLoadOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureLoadOracle failed: {}\n{}",
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
            for kind in [
                "mineshaft_normal",
                "mineshaft_mesa",
                "stronghold",
                "ocean_monument",
            ] {
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

/// Java 输出按查询分节：`Q <key> <写出件数> <读回件数>`、若干 `L` 行，
/// 海底神殿再有 `S <行数>` 和若干 `R` 行。
struct Section {
    key: String,
    written: usize,
    loaded: usize,
    lines: Vec<String>,
    regenerated: Option<Vec<String>>,
}

fn parse_output(stdout: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    let mut regen: Option<(usize, Vec<String>)> = None;

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "Q" => {
                if let Some(section) = current.take() {
                    sections.push(finish_section(section, regen.take())?);
                }
                let written: usize = fields[5]
                    .parse()
                    .map_err(|_| format!("bad written count: {line}"))?;
                let loaded: usize = fields[6]
                    .parse()
                    .map_err(|_| format!("bad loaded count: {line}"))?;
                current = Some(Section {
                    key: fields[1..5].join(" "),
                    written,
                    loaded,
                    lines: Vec::new(),
                    regenerated: None,
                });
            }
            "L" => match &mut current {
                Some(section) => section.lines.push(line[2..].to_owned()),
                None => return Err(format!("L line before any Q: {line}")),
            },
            "S" => {
                let count: usize = fields[1]
                    .parse()
                    .map_err(|_| format!("bad regenerated count: {line}"))?;
                regen = Some((count, Vec::new()));
            }
            "R" => match &mut regen {
                Some((_, lines)) => lines.push(line[2..].to_owned()),
                None => return Err(format!("R line before any S: {line}")),
            },
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    if let Some(section) = current.take() {
        sections.push(finish_section(section, regen.take())?);
    }
    Ok(sections)
}

/// 收尾一节：核对 `L` / `R` 行数和声明的数量。
///
/// Finish one section, checking the `L` / `R` line counts against the declared ones.
fn finish_section(
    mut section: Section,
    regen: Option<(usize, Vec<String>)>,
) -> Result<Section, String> {
    if section.lines.len() != section.loaded {
        return Err(format!(
            "Q {}: claimed {} loaded pieces, got {} L lines",
            section.key,
            section.loaded,
            section.lines.len()
        ));
    }
    if let Some((count, lines)) = regen {
        if lines.len() != count {
            return Err(format!(
                "Q {}: claimed {count} regenerated lines, got {}",
                section.key,
                lines.len()
            ));
        }
        section.regenerated = Some(lines);
    }
    Ok(section)
}

/// 按 kind 用生产代码产出**装进 `PiecesContainer` 的结构件**。
///
/// 海底神殿的 `generatePieces` 只 `addPiece(createTopPiece(...))`，房间图是
/// `MonumentBuilding` 的私有 `childPieces`，不进容器；其余三个结构每个结构件都进容器。
/// 存档对拍只需容器里的这部分，房间图单独在 `R` 段比。
///
/// Build the pieces of one kind that end up **in the `PiecesContainer`**.
///
/// The ocean monument's `generatePieces` only adds the top piece: the room graph
/// lives in `MonumentBuilding.childPieces` and never enters the container, while the
/// other three structures add every piece. The save oracle only needs the container
/// contents; the room graph is compared separately in the `R` section.
fn layout(q: &Query) -> Result<Vec<Piece>, String> {
    match q.kind {
        // 对拍读回路径时 surfaceHeight 固定成海平面，只要求两边同一输入。
        // The read-back oracle fixes surfaceHeight at sea level; both sides share it.
        "mineshaft_normal" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, false, 0)),
        "mineshaft_mesa" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, true, 63)),
        "stronghold" => Ok(pieces::stronghold::generate(q.seed, q.cx, q.cz)),
        "ocean_monument" => {
            let mut all = pieces::ocean_monument::generate(q.seed, q.cx, q.cz);
            all.truncate(1);
            Ok(all)
        }
        other => Err(format!("no Rust layout for kind {other}")),
    }
}

/// `L` 行的规范串：`id bb6 O GD extras`（数字带 NBT 类型前缀）。
///
/// Canonical `L` line: `id bb6 O GD extras`, numbers carrying their NBT type.
fn loaded_line(piece: &mc_bamboo_worldgen::structure::save::LoadedPiece) -> String {
    let base = [
        piece.id.clone(),
        piece.bb.min_x.to_string(),
        piece.bb.min_y.to_string(),
        piece.bb.min_z.to_string(),
        piece.bb.max_x.to_string(),
        piece.bb.max_y.to_string(),
        piece.bb.max_z.to_string(),
        piece.orientation.to_string(),
        piece.gen_depth.to_string(),
    ];
    let mut extras: Vec<String> = piece
        .extras
        .iter()
        .map(|(key, tag)| format!("{key}={}", extra_token(tag)))
        .collect();
    extras.sort();
    let extras_token = if extras.is_empty() {
        "-".to_owned()
    } else {
        extras.join(",")
    };
    format!("{} {}", base.join(" "), extras_token)
}

/// 子类字段值：Byte/Int → `B:`/`I:` 前缀，String 原样，列表用 `/` 连接。
///
/// Subclass value: Byte/Int get a `B:`/`I:` prefix, strings stay verbatim, lists join
/// with `/`.
fn extra_token(tag: &mc_bamboo_nbt::Tag) -> String {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Byte(v) => format!("B:{v}"),
        Tag::Short(v) => format!("S:{v}"),
        Tag::Int(v) => format!("I:{v}"),
        Tag::Long(v) => format!("L:{v}"),
        Tag::Float(v) => format!("F:{v}"),
        Tag::Double(v) => format!("D:{v}"),
        Tag::String(v) => v.clone(),
        Tag::IntArray(values) => values
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join("_"),
        Tag::List(values) => values.iter().map(extra_token).collect::<Vec<_>>().join("/"),
        other => format!("{other:?}"),
    }
}

/// `R` 行的规范串（和 `L` 行同格式，只多一个 rot/mirror 不参与，因为读回不带）。
///
/// Canonical `R` line, same shape as `L`: the rebuilt pieces carry only save fields.
fn compare(queries: &[Query], stdout: &str) -> Result<(), String> {
    let sections = parse_output(stdout)?;
    if sections.len() != queries.len() {
        return Err(format!(
            "oracle emitted {} Q sections, expected {}",
            sections.len(),
            queries.len()
        ));
    }

    let mut failures: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut comparisons = 0usize;
    let mut loaded_total = 0usize;
    let mut regen_total = 0usize;

    for (q, section) in queries.iter().zip(sections.iter()) {
        let key = format!("{} {} {} {}", q.kind, q.seed, q.cx, q.cz);
        if section.key != key {
            return Err(format!(
                "query key mismatch: oracle {:?} vs {key:?}",
                section.key
            ));
        }
        let pieces = layout(q)?;
        // 存档标签：走生产 `SavePiece::from_layout` + `to_tag`。
        let tags: Vec<mc_bamboo_nbt::Tag> = pieces
            .iter()
            .map(|piece| SavePiece::from_layout(piece).to_tag())
            .collect();
        if tags.len() != section.written {
            mismatch_count += 1;
            push(
                &mut failures,
                format!(
                    "[{key}] wrote {} pieces, oracle wrote {}",
                    tags.len(),
                    section.written
                ),
            );
        }

        let loaded = load_pieces(&tags);
        loaded_total += loaded.len();
        let ours: Vec<String> = loaded.iter().map(loaded_line).collect();
        report_lines(
            &mut failures,
            &mut mismatch_count,
            &mut comparisons,
            &key,
            "L",
            &section.lines,
            &ours,
        );

        if q.kind == "ocean_monument" {
            let Some(theirs) = &section.regenerated else {
                mismatch_count += 1;
                push(
                    &mut failures,
                    format!("[{key}] oracle did not emit the R section"),
                );
                continue;
            };
            let saved_layout: Vec<Piece> = loaded.iter().map(loaded_piece_as_layout).collect();
            let rebuilt = regenerate_monument(q.cx, q.cz, q.seed, &saved_layout);
            // 重建后的比较只到 `MonumentBuilding` 本身：官方 `regeneratePiecesAfterLoad`
            // 返回的 `PiecesContainer` 只含这一个结构件，房间图在它的私有 childPieces 里。
            let ours_r: Vec<String> = rebuilt
                .first()
                .map(|piece| vec![layout_piece_line(piece)])
                .unwrap_or_default();
            let theirs_head: Vec<String> = theirs.iter().take(1).cloned().collect();
            regen_total += theirs.len();
            report_lines(
                &mut failures,
                &mut mismatch_count,
                &mut comparisons,
                &key,
                "R(head)",
                &theirs_head,
                &ours_r,
            );

            // 房间图逐件比：Rust 布局的第 1 项起就是 childPieces。
            let ours_children: Vec<String> =
                rebuilt.iter().skip(1).map(layout_piece_line).collect();
            let theirs_children: Vec<String> = theirs.iter().skip(1).cloned().collect();
            report_lines(
                &mut failures,
                &mut mismatch_count,
                &mut comparisons,
                &key,
                "R(children)",
                &theirs_children,
                &ours_children,
            );
        }
    }

    println!(
        "structure-load: {} queries, {} pieces written, {} reloaded, {} regenerated lines, {} comparisons",
        queries.len(),
        queries.len(),
        loaded_total,
        regen_total,
        comparisons
    );
    if failures.is_empty() {
        println!("STRUCTURE-LOAD RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in failures.iter().take(24) {
            println!("STRUCTURE-LOAD FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-LOAD RESULT: {mismatch_count} mismatches"
        ))
    }
}

/// 读回的布局件转成规范串：读回侧没有 rot/mirror，只比存档字段。
///
/// Canonical line of a rebuilt layout piece: the read-back side has no rot/mirror, so
/// only the save fields are compared.
fn layout_piece_line(piece: &Piece) -> String {
    let mut extras: Vec<String> = piece
        .extras
        .iter()
        .map(|(key, value)| format!("{key}={}", value.token()))
        .collect();
    extras.sort();
    let extras_token = if extras.is_empty() {
        "-".to_owned()
    } else {
        extras.join(",")
    };
    format!(
        "{} {} {} {} {} {} {} {} {} {}",
        piece.id,
        piece.bb[0],
        piece.bb[1],
        piece.bb[2],
        piece.bb[3],
        piece.bb[4],
        piece.bb[5],
        piece.o,
        piece.gd,
        extras_token
    )
}

/// 逐行比 Java 与 Rust，记录不一致。
///
/// Compare Java and Rust line by line, recording mismatches.
fn report_lines(
    failures: &mut Vec<String>,
    mismatch_count: &mut usize,
    comparisons: &mut usize,
    key: &str,
    label: &str,
    theirs: &[String],
    ours: &[String],
) {
    let n = theirs.len().max(ours.len());
    for i in 0..n {
        *comparisons += 1;
        let t = theirs.get(i).map(String::as_str).unwrap_or("<missing>");
        let o = ours.get(i).map(String::as_str).unwrap_or("<missing>");
        if t != o {
            *mismatch_count += 1;
            push(
                failures,
                format!("[{key}] {label} {i}:\n  java: {t}\n  rust: {o}"),
            );
        }
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
