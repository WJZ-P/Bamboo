//! 废弃传送门的结构件布局对拍：官方 `StructureRuinedPortalOracle` 对
//! `mc-bamboo-worldgen::structure::templated::ruined_portal`。
//!
//! Java 侧用官方 `RuinedPortalStructure.findValidGenerationPoint`（内部跑
//! `findSuitableY` 的整列方块搜索），Rust 侧用生产代码复算，逐行比 `P` 行。
//! 查询集覆盖 `ruined_portal`（两个 setup：地下 + 地表，各 0.5 权重）、
//! `ruined_portal_desert`（`partly_buried`）、`ruined_portal_jungle`（`on_land_surface` +
//! 藤蔓）、`ruined_portal_ocean`（`on_ocean_floor`，用 `OCEAN_FLOOR_WG` 的整列判定，
//! 且 `Mirror` 分支被 `nextFloat() < 0.5` 覆盖）。
//!
//! Oracle for the ruined portal piece layout: vanilla `StructureRuinedPortalOracle` against
//! `mc-bamboo-worldgen::structure::templated::ruined_portal`. Java runs the official
//! `RuinedPortalStructure.findValidGenerationPoint` (which performs `findSuitableY`'s
//! full-column scan); Rust recomputes with production code and compares every `P` line.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        Structures, pieces::Piece, rings::BiomeResolver, templated::ruined_portal,
        validity::is_valid_biome_at,
    },
};
use serde_json::Value;
/// 六个废弃传送门变体：覆盖五种 `VerticalPlacement`（缺 `in_nether`，主世界不做下界）
/// 与 `can_be_cold` 的真 / 假。
///
/// Six ruined-portal variants covering five `VerticalPlacement` kinds (`in_nether` is
/// skipped because this round stays in the overworld) and both `can_be_cold` branches.
const STRUCTURES: [&str; 6] = [
    "minecraft:ruined_portal",
    "minecraft:ruined_portal_desert",
    "minecraft:ruined_portal_jungle",
    "minecraft:ruined_portal_mountain",
    "minecraft:ruined_portal_ocean",
    "minecraft:ruined_portal_swamp",
];
/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
/// 覆盖多种地形（平原、深海、山地、丛林、沙漠、恶地、雪原）的区块坐标。
/// 后六个是特意找出来的群系样本（种子 0）：沙漠 / 恶地 / 雪原 / 丛林 / 深海 / 石岸，
/// 让 `partly_buried`、`in_mountain`、`on_ocean_floor` 三条 `findSuitableY` 分支都被比到。
///
/// Chunk coordinates spanning many terrains (plains, deep ocean, mountains, jungle, desert,
/// badlands, snow). The last six are biome samples found for seed 0 — desert, badlands, snowy
/// plains, jungle, deep ocean and stony shore — so all three `findSuitableY` branches
/// (`partly_buried`, `in_mountain`, `on_ocean_floor`) are compared.
const CHUNKS: [(i32, i32); 18] = [
    (0, 0),
    (1, 1),
    (-1, -1),
    (34, 34),
    (100, -100),
    (1000, 1000),
    (-450, 0),
    (-300, -450),
    (-150, -300),
    (0, 150),
    (-600, 150),
    (300, -300),
    (32, -286),
    (31, -285),
    (257, -400),
    (-352, -382),
    (-155, -395),
    (-211, -400),
];

/// 一条查询。
struct Query {
    structure: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
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
    let worldgen = Worldgen::parse(&text)?;

    let queries: Vec<Query> = SEEDS
        .iter()
        .flat_map(|&seed| {
            CHUNKS.iter().flat_map(move |&(cx, cz)| {
                STRUCTURES.iter().map(move |&structure| Query {
                    structure,
                    seed,
                    cx,
                    cz,
                })
            })
        })
        .collect();
    write_queries(queries_path, &queries)?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureRuinedPortalOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureRuinedPortalOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(
        &worldgen,
        structures,
        &queries,
        &String::from_utf8_lossy(&output.stdout),
    )
    .map_err(|error| error.to_string())
}

fn write_queries(path: &Path, queries: &[Query]) -> Result<(), String> {
    let mut text = String::new();
    for q in queries {
        text.push_str(&format!("{} {} {} {}\n", q.structure, q.seed, q.cx, q.cz));
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
    format!("{} {} {} {}", q.structure, q.seed, q.cx, q.cz)
}

#[cfg(feature = "oracle")]
fn compare(
    worldgen: &Worldgen,
    structures: Structures,
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
    let mut with_pieces = 0usize;
    let mut by_structure: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    let mut mirrors: std::collections::BTreeMap<&str, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut placements: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut cold_flags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let preset = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing overworld preset")?;

    for (q, (key, java_lines)) in queries.iter().zip(sections.iter()) {
        let expected_key = query_key(q);
        if key != &expected_key {
            return Err(format!(
                "query key mismatch: oracle {key:?} vs {expected_key:?}"
            ));
        }
        let def = structures
            .get(q.structure)
            .ok_or_else(|| format!("Rust is missing structure {}", q.structure))?;
        let allowed = mc_bamboo_worldgen::structure::allowed_biome_names(def)?;
        let generator = Generator::new(worldgen, q.seed)?;
        let mut router = SeededRouter::new(settings, q.seed);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source: preset,
        };

        // `findValidGenerationPoint` 先做整列群系短路、再对起点做 `isValidBiome`。
        // 这里照做：整列没有合法群系时结构件列表为空，不做昂贵的高度计算。
        // `findValidGenerationPoint` short-circuits on the column biome check and then filters
        // with `isValidBiome` at the start. Do the same: an empty piece list without touching
        // the expensive height work when the column has no allowed biome.
        let dims = (settings.min_y, settings.height);
        // 废弃传送门的 `findGenerationPoint` **不做** `couldValidBiomeExistOnTopOfChunkCenter`
        // 整列短路（它自己算完起点再交给 `isValidBiome`），所以这里也不做，只在起点判一次。
        // The ruined portal's `findGenerationPoint` does **not** run the
        // `couldValidBiomeExistOnTopOfChunkCenter` column short-circuit (it computes the start and
        // lets `isValidBiome` decide), so this side does not either; only the start is tested.
        let _ = dims;
        let start =
            ruined_portal::generate(&generator, &mut resolver, q.seed, q.cx, q.cz, &def.raw)?;
        let valid = is_valid_biome_at(
            &mut resolver,
            start.origin.0,
            start.origin.1,
            start.origin.2,
            &allowed,
        );
        let ours: Vec<Piece> = if valid { start.pieces } else { Vec::new() };

        *by_structure.entry(q.structure).or_default() += 1;
        if !java_lines.is_empty() {
            with_pieces += 1;
            for line in java_lines {
                for token in line.split_whitespace().flat_map(|f| f.split(',')) {
                    if let Some(value) = token.strip_prefix("Mirror=") {
                        mirrors
                            .entry(q.structure)
                            .or_default()
                            .insert(value.to_owned());
                    }
                    if let Some(value) = token.strip_prefix("VerticalPlacement=") {
                        placements.insert(value.to_owned());
                    }
                    if let Some(value) = token.strip_prefix("Properties=")
                        && let Some(cold) = value
                            .trim_start_matches('{')
                            .split(';')
                            .find_map(|field| field.strip_prefix("cold="))
                    {
                        cold_flags.insert(cold.to_owned());
                    }
                }
            }
        }

        let ours: Vec<String> = ours.iter().map(Piece::canonical_line).collect();
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

    let kinds = by_structure
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mirrors = mirrors
        .iter()
        .map(|(k, v)| format!("{k}:{:?}", v.iter().collect::<Vec<_>>()))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "structure-ruined-portal: {} queries ({}), {} with pieces, {} pieces, {} line comparisons",
        queries.len(),
        kinds,
        with_pieces,
        pieces_total,
        total
    );
    println!(
        "structure-ruined-portal branches: mirrors [{mirrors}], placements [{:?}], cold [{:?}]",
        placements.iter().collect::<Vec<_>>(),
        cold_flags.iter().collect::<Vec<_>>()
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-RUINED-PORTAL RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-RUINED-PORTAL FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-RUINED-PORTAL RESULT: {mismatch_count} mismatches"
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
