//! `structure-pipeline`：官方 `createStructures` / `createReferences` 对游戏里同一条
//! `structure_starts` / `structure_references` 纯函数的端到端对拍。
//!
//! Rust 写 queries.txt（每行 `seed cx cz`），Java 侧 `StructurePipelineOracle` 在 JVM 里
//! 造 17×17 个 ProtoChunk，跑官方两阶段，dump 每个中心区块的起点 / 引用 / 结构件清单；
//! Rust 用 `mc_bamboo_game::worldgen_structures` 复算并逐项比。0 不一致才算过，
//! `--break-self-test` 故意改坏一个引用确认比较器真的会报错。
//!
//! End-to-end oracle: vanilla's `createStructures` / `createReferences` against the
//! game's own pure structure-stage functions. Rust writes queries.txt; Java builds a
//! 17x17 ProtoChunk grid, runs both official stages and dumps starts/references/pieces.

use std::{fs, path::Path, process::Command};

use mc_bamboo_data::version;
use mc_bamboo_game::world_options::{LevelType, WorldGenConfig};
use serde_json::Value;

const USAGE: &str =
    "usage: structure-pipeline <java> [--seeds N] [--chunks N] [--show N] [--break-self-test]";

/// 覆盖村庄 / 前哨 / 要塞 / 矿井 / 沉船 / 海底遗迹 / 试炼密室等结构的种子与区块。
/// 种子 42 的 -20..20 扫描已知有矿井 / 试炼密室 / 废弃传送门；其余种子覆盖更多结构。
const QUERIES: &[(i64, i32, i32)] = &[
    // 种子 42 的 -40..40 扫描里确认过的起点，覆盖矿井 / 试炼密室 / 废弃传送门 /
    // 埋藏的宝藏 / 女巫小屋 / 沉船 / 古迹废墟 / 废弃营地 / 村庄等结构集。
    (42, 4, 0),
    (42, -3, -17),
    (42, 0, -17),
    (42, 3, 17),
    (42, 17, 7),
    (42, 19, -15),
    (42, -19, 11),
    (42, -39, -14),
    (42, -32, -16),
    (42, -30, 6),
    (42, -19, -38),
    (42, -17, 22),
    (42, -14, 21),
    (42, -13, 40),
    (42, 2, -34),
    (42, 12, 28),
    (42, 15, 39),
    (42, 16, 22),
    (42, 19, -24),
    (42, 35, 38),
    (42, 38, -25),
    (0, 0, 0),
    (1, 0, 0),
    (1, 34, 34),
    (-1, 100, -100),
    (123456789, 12, -7),
    (123456789, -500, 300),
    (-98765432123, 7, -13),
];

#[derive(Debug, Default)]
struct Options {
    show: usize,
    /// 故意改坏一个引用，确认对拍会报不一致（自检）。
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: 8,
            ..Self::default()
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?,
                "--break-self-test" => options.break_self_test = true,
                // --seeds / --chunks 只影响提示信息，查询集固定，方便逐类覆盖统计。
                "--seeds" | "--chunks" => {
                    let _ = args.next().ok_or(USAGE)?;
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

struct JavaRow {
    starts: Vec<(String, usize)>,
    references: Vec<(String, i64)>,
    pieces: Vec<(String, String, [i32; 6])>,
}

#[derive(Debug, Default)]
struct Stats {
    rows: usize,
    comparisons: usize,
    starts: usize,
    references: usize,
    pieces: usize,
    mismatches: Vec<String>,
    /// 逐方块比过的 terrain 格数。
    terrain_blocks: u64,
    terrain_chunks: usize,
}

pub fn run(mut command: Command, queries_path: &Path, options: &[&str]) -> Result<(), String> {
    let options = Options::parse(options)?;
    let text = QUERIES
        .iter()
        .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
        .collect::<String>();
    fs::write(queries_path, text).map_err(|e| format!("cannot write queries: {e}"))?;
    let output = command
        .arg(queries_path)
        .output()
        .map_err(|e| format!("cannot run StructurePipelineOracle: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructurePipelineOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&String::from_utf8_lossy(&output.stdout), &options)
}

fn compare(stdout: &str, options: &Options) -> Result<(), String> {
    let mut stats = Stats::default();
    let mut header_ok = false;
    for line in stdout.lines() {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("bad oracle line: {e}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        match kind {
            "header" => {
                let reported = value
                    .get("version")
                    .and_then(Value::as_str)
                    .ok_or("header missing version")?;
                let protocol = value
                    .get("protocol")
                    .and_then(Value::as_i64)
                    .ok_or("header missing protocol")?;
                if reported != version::GAME_VERSION
                    || protocol != i64::from(version::PROTOCOL_VERSION)
                {
                    return Err(format!("oracle is {reported}/{protocol}"));
                }
                header_ok = true;
            }
            "structures" => on_row(&value, options, &mut stats)?,
            "terrain" => on_terrain(&value, options, &mut stats)?,
            "end" => {}
            other => return Err(format!("unexpected oracle line {other}")),
        }
    }
    if !header_ok {
        return Err("oracle did not emit a header".to_owned());
    }
    let Stats {
        rows,
        comparisons,
        starts,
        references,
        pieces,
        terrain_blocks,
        terrain_chunks,
        mismatches,
    } = stats;
    println!(
        "structure-pipeline: {rows} rows, {comparisons} comparisons, {starts} starts, \
         {references} references, {pieces} pieces, {terrain_chunks} terrain chunks / \
         {terrain_blocks} blocks, {} mismatches",
        mismatches.len()
    );
    if options.break_self_test {
        if mismatches.is_empty() {
            return Err(
                "self-test broke a reference but the oracle reported 0 mismatches".to_owned(),
            );
        }
        println!(
            "structure-pipeline self-test: detector works ({} mismatches)",
            mismatches.len()
        );
        return Ok(());
    }
    if rows == 0 {
        return Err("structure-pipeline compared no rows".to_owned());
    }
    if !mismatches.is_empty() {
        for message in mismatches.iter().take(options.show) {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!(
            "structure-pipeline found {} mismatches",
            mismatches.len()
        ));
    }
    println!("structure-pipeline: 0 mismatches");
    Ok(())
}

fn on_row(value: &Value, options: &Options, stats: &mut Stats) -> Result<(), String> {
    let seed = value
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or("missing seed")?;
    let x = value.get("x").and_then(Value::as_i64).ok_or("missing x")? as i32;
    let z = value.get("z").and_then(Value::as_i64).ok_or("missing z")? as i32;
    let java = parse_row(value)?;
    let config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    let data = mc_bamboo_game::worldgen_structures::structure_data(&config, x, z)
        .map_err(|e| format!("game structure_data failed for ({seed},{x},{z}): {e}"))?;

    // 起点：`(结构名, 结构件数)` 逐项比。
    let mut rust_starts: Vec<(String, usize)> = data
        .starts
        .by_structure
        .iter()
        .map(|(name, start)| {
            let n = match &start.pieces {
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(p) => p.len(),
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(p) => p.len(),
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(p) => p.len(),
            };
            (name.clone(), n)
        })
        .collect();
    rust_starts.sort();
    let want_starts = {
        let mut v = java.starts.clone();
        v.sort();
        v
    };
    stats.rows += 1;
    stats.comparisons += 1;
    stats.starts += rust_starts.len();
    if rust_starts != want_starts {
        push(
            stats,
            format!("({seed},{x},{z}) starts: ours {rust_starts:?} vs {want_starts:?}"),
        );
    }

    // 引用：`(结构名, packed 键)` 逐项比；`--break-self-test` 改坏一条。
    let mut rust_refs: Vec<(String, i64)> = data
        .references
        .iter()
        .flat_map(|(name, keys)| keys.iter().map(move |&key| (name.clone(), key)))
        .collect();
    rust_refs.sort();
    let mut want_refs = java.references.clone();
    want_refs.sort();
    if options.break_self_test {
        if let Some((_, key)) = rust_refs.first_mut() {
            *key = key.wrapping_add(1);
        } else {
            rust_refs.push(("minecraft:broken".to_owned(), 1));
        }
    }
    stats.comparisons += 1;
    stats.references += rust_refs.len();
    if rust_refs != want_refs {
        push(
            stats,
            format!("({seed},{x},{z}) references: ours {rust_refs:?} vs {want_refs:?}"),
        );
    }

    // 结构件包围盒：逐条比结构名、类型、六点盒。
    let mut rust_pieces: Vec<(String, String, [i32; 6])> = Vec::new();
    for (name, start) in &data.starts.by_structure {
        match &start.pieces {
            mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(pieces) => {
                for p in pieces {
                    rust_pieces.push((name.clone(), p.id.clone(), p.bb));
                }
            }
            mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(pieces) => {
                for p in pieces {
                    rust_pieces.push((
                        name.clone(),
                        "minecraft:jigsaw".to_owned(),
                        [
                            p.bounding_box.min_x,
                            p.bounding_box.min_y,
                            p.bounding_box.min_z,
                            p.bounding_box.max_x,
                            p.bounding_box.max_y,
                            p.bounding_box.max_z,
                        ],
                    ));
                }
            }
            mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(pieces) => {
                for p in pieces {
                    rust_pieces.push((
                        name.clone(),
                        p.piece_type.clone(),
                        [
                            p.bounding_box.min_x,
                            p.bounding_box.min_y,
                            p.bounding_box.min_z,
                            p.bounding_box.max_x,
                            p.bounding_box.max_y,
                            p.bounding_box.max_z,
                        ],
                    ));
                }
            }
        }
    }
    rust_pieces.sort();
    let mut want_pieces = java.pieces.clone();
    want_pieces.sort();
    stats.comparisons += 1;
    stats.pieces += rust_pieces.len();
    // 覆盖统计：跑一遍游戏里的完整流水线，报告这个区块实际放置的结构类型。
    // Coverage: run the full game pipeline and report what was actually placed.
    let pipeline = mc_bamboo_game::worldgen_features::game_pipeline(&config, x, z)
        .map_err(|e| format!("game_pipeline failed for ({seed},{x},{z}): {e}"))?;
    if !pipeline.placed_pieces.is_empty() {
        println!("  PLACED ({seed},{x},{z}): {:?}", pipeline.placed_pieces);
    }
    if rust_pieces != want_pieces {
        let first = rust_pieces
            .iter()
            .zip(want_pieces.iter())
            .find(|(a, b)| a != b);
        push(
            stats,
            format!(
                "({seed},{x},{z}) pieces: ours {} vs {}; first diff {first:?}",
                rust_pieces.len(),
                want_pieces.len()
            ),
        );
    }
    Ok(())
}

/// 官方带真实 Beardifier 的 CARVERS 方块 vs 游戏 `game_pipeline` 的中心方块，逐格比。
/// Compare official Beardifier-terrain blocks against the game pipeline's center.
fn on_terrain(value: &Value, options: &Options, stats: &mut Stats) -> Result<(), String> {
    let seed = value
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or("missing seed")?;
    let x = value.get("x").and_then(Value::as_i64).ok_or("missing x")? as i32;
    let z = value.get("z").and_then(Value::as_i64).ok_or("missing z")? as i32;
    let palette: Vec<String> = value
        .get("palette")
        .and_then(Value::as_array)
        .ok_or("missing palette")?
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_owned())
        .collect();
    let indices: Vec<i64> = value
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or("missing blocks")?
        .iter()
        .map(|v| v.as_i64().ok_or("bad block index"))
        .collect::<Result<Vec<_>, _>>()?;
    let config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    let pipeline = mc_bamboo_game::worldgen_features::game_pipeline(&config, x, z)
        .map_err(|e| format!("game_pipeline failed for ({seed},{x},{z}): {e}"))?;
    // game_pipeline 返回的是跑完 FEATURES 的方块；terrain 阶段对拍只比 CARVERS，
    // 因此这里用同一份 `generate_chunk_with_beardifier` 重新生成 CARVERS 快照。
    let refs = mc_bamboo_game::worldgen_structures::structure_references_stage(&config, x, z)?;
    let beard = mc_bamboo_game::worldgen_structures::beardifier_for_chunk(&config, x, z, &refs)?;
    let ours =
        mc_bamboo_game::worldgen::generate_chunk_with_beardifier(&config, x, z, beard.as_ref())?;
    stats.terrain_chunks += 1;
    let mut differences = 0usize;
    for (y, layer) in ours.blocks.iter().enumerate() {
        for x in 0..16 {
            for z in 0..16 {
                let index = y * 256 + x * 16 + z;
                let state = layer[x * 16 + z];
                let name = mc_bamboo_data::block_of_state(state)
                    .map(|b| b.name)
                    .unwrap_or("minecraft:air");
                let official = palette
                    .get(indices[index] as usize)
                    .map(String::as_str)
                    .unwrap_or("");
                // 官方调色板是 `Block{name[props]}`，地形只放默认状态，按名字比。
                let official_name = official
                    .strip_prefix("Block{")
                    .and_then(|s| s.split_once('}').map(|(n, _)| n))
                    .unwrap_or(official);
                stats.terrain_blocks += 1;
                if options.break_self_test && index == 0 {
                    if official_name != "minecraft:broken" {
                        differences += 1;
                    }
                } else if official_name != name {
                    differences += 1;
                    if stats.mismatches.len() < 8 {
                        stats.mismatches.push(format!(
                            "({seed},{x},{z}) terrain y={} ({x},{z}): game {name} vs {official_name}",
                            y - 64
                        ));
                    }
                }
            }
        }
    }
    let _ = &pipeline;
    if differences > 0 && stats.mismatches.len() < 8 && !options.break_self_test {
        stats.mismatches.push(format!(
            "({seed},{x},{z}) terrain: {differences} block mismatches"
        ));
    }
    Ok(())
}

fn push(stats: &mut Stats, message: String) {
    if stats.mismatches.len() < 256 {
        stats.mismatches.push(message);
    }
}

fn parse_row(value: &Value) -> Result<JavaRow, String> {
    let mut row = JavaRow {
        starts: Vec::new(),
        references: Vec::new(),
        pieces: Vec::new(),
    };
    for entry in value
        .get("starts")
        .and_then(Value::as_array)
        .ok_or("missing starts")?
    {
        let pair = entry.as_array().ok_or("start is not an array")?;
        row.starts.push((
            pair[0].as_str().ok_or("bad start name")?.to_owned(),
            pair[1].as_u64().ok_or("bad start count")? as usize,
        ));
    }
    for entry in value
        .get("references")
        .and_then(Value::as_array)
        .ok_or("missing references")?
    {
        let pair = entry.as_array().ok_or("reference is not an array")?;
        row.references.push((
            pair[0].as_str().ok_or("bad reference name")?.to_owned(),
            pair[1].as_i64().ok_or("bad packed key")?,
        ));
    }
    for entry in value
        .get("pieces")
        .and_then(Value::as_array)
        .ok_or("missing pieces")?
    {
        let item = entry.as_array().ok_or("piece is not an array")?;
        let bb: [i32; 6] = (2..8)
            .map(|i| -> Result<i32, String> { Ok(item[i].as_i64().ok_or("bad bb")? as i32) })
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| "bad bb length".to_owned())?;
        row.pieces.push((
            item[0].as_str().ok_or("bad piece structure")?.to_owned(),
            item[1].as_str().ok_or("bad piece type")?.to_owned(),
            bb,
        ));
    }
    Ok(row)
}
