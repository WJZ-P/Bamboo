//! 四个模板结构 `postProcess` 的逐方块对拍：官方 `StructureTemplatedPlaceOracle` 对
//! `mc-bamboo-worldgen::structure::post_process::{igloo, shipwreck, ocean_ruin,
//! ruined_portal}` + `structure::template_place`。
//!
//! 两边用同一份合成地形（西半陆地、东半海），Java 侧跑官方 `postProcess` 并 dump
//! 写入日志（后写覆盖先写），Rust 侧用生产代码复算，逐格比较状态。
//!
//! Per-block oracle for the four template structures' `postProcess`: vanilla
//! `StructureTemplatedPlaceOracle` against the Rust post-process modules plus
//! `structure::template_place`. Both sides build the same synthetic terrain (land west,
//! ocean east); Java runs the official `postProcess` and dumps its write log (later writes
//! win), Rust recomputes with production code and compares every cell.

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mc_bamboo_worldgen::{
    feature::world::FeatureWorld,
    structure::{
        pieces::{Piece, intersects},
        post_process::{igloo, ocean_ruin, shipwreck},
    },
};

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 3] = [0, 42, -7];
/// 覆盖陆地 / 海面边界的区块坐标。
const CHUNKS: [(i32, i32); 4] = [(0, 0), (-1, 1), (2, -2), (5, 5)];
const KINDS: [&str; 4] = [
    "igloo",
    "shipwreck_ocean",
    "ocean_ruin_cold",
    "ocean_ruin_warm",
];
/// 假世界的 Y 范围，和 Java 侧一致。
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;
const SEA_LEVEL: i32 = 63;

/// 合成地形的地表高度，和 `StructureTemplatedPlaceOracle.surfaceY` 完全一致。
///
/// The synthetic surface height, identical to `StructureTemplatedPlaceOracle.surfaceY`.
pub fn surface_y(x: i32, z: i32) -> i32 {
    if x >= 0 {
        48 + (x * 7 + z * 3).rem_euclid(3)
    } else {
        64 + (x * 31 + z * 17).rem_euclid(5) - 2
    }
}

/// 东半是海，和 `isOcean` 一致。
///
/// The east half is ocean, matching `isOcean`.
pub fn is_ocean(x: i32) -> bool {
    x >= 0
}

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    extra: String,
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
                    extra: "0".to_owned(),
                });
            }
        }
    }
    queries
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries = build_queries();
    let mut text = String::new();
    for q in &queries {
        text.push_str(&format!(
            "{} {} {} {} {}\n",
            q.kind, q.seed, q.cx, q.cz, q.extra
        ));
    }
    fs::write(queries_path, text).map_err(|error| format!("cannot write queries file: {error}"))?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureTemplatedPlaceOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureTemplatedPlaceOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&queries, &String::from_utf8_lossy(&output.stdout))
}

/// 一份合成地形：9 个区块都按同一条公式填。
///
/// Synthetic terrain: nine chunks filled by the same formula.
fn synthetic_chunks(cx: i32, cz: i32) -> mc_bamboo_worldgen::feature::FeatureChunks {
    use mc_bamboo_worldgen::feature::{ChunkBlocks, FeatureChunks, Heightmaps};
    let blocks: [ChunkBlocks; 9] = std::array::from_fn(|index| {
        let dx = index as i32 % 3 - 1;
        let dz = index as i32 / 3 - 1;
        let base_x = (cx + dx) * 16;
        let base_z = (cz + dz) * 16;
        let mut chunk: ChunkBlocks = vec![vec![mc_bamboo_data::blocks::AIR; 256]; HEIGHT as usize];
        for x in 0..16 {
            for z in 0..16 {
                let world_x = base_x + x;
                let world_z = base_z + z;
                let surface = surface_y(world_x, world_z);
                let column = x as usize * 16 + z as usize;
                for y in MIN_Y..surface - 1 {
                    chunk[(y - MIN_Y) as usize][column] = mc_bamboo_data::blocks::STONE;
                }
                chunk[(surface - 1 - MIN_Y) as usize][column] = mc_bamboo_data::blocks::SANDSTONE;
                chunk[(surface - MIN_Y) as usize][column] = mc_bamboo_data::blocks::SAND;
                if is_ocean(world_x) {
                    for y in surface + 1..=SEA_LEVEL {
                        chunk[(y - MIN_Y) as usize][column] = mc_bamboo_data::blocks::WATER;
                    }
                }
            }
        }
        chunk
    });
    let heights: [Heightmaps; 9] = std::array::from_fn(|index| {
        mc_bamboo_worldgen::feature::world::prime_heightmaps(&blocks[index], MIN_Y, HEIGHT)
    });
    let biomes = std::array::from_fn(|_| vec![vec![0u32; 64]; 4]);
    FeatureChunks {
        center_x: cx,
        center_z: cz,
        min_y: MIN_Y,
        height: HEIGHT,
        blocks,
        biomes,
        heights,
    }
}

/// Java 输出的一段。
struct Section {
    key: String,
    blocks: BTreeMap<(i32, i32, i32), u32>,
}

fn strip_log_prefix(line: &str) -> &str {
    match line.find("]: [STDOUT]: ") {
        Some(index) => &line[index + "]: [STDOUT]: ".len()..],
        None => line,
    }
}

fn parse_output(stdout: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    for line in stdout.lines() {
        let line = strip_log_prefix(line);
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "Q" => {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                current = Some(Section {
                    key: fields[1..5].join(" "),
                    blocks: BTreeMap::new(),
                });
            }
            "B" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("B line before any Q: {line}"));
                };
                let x: i32 = fields[1].parse().map_err(|_| format!("bad x: {line}"))?;
                let y: i32 = fields[2].parse().map_err(|_| format!("bad y: {line}"))?;
                let z: i32 = fields[3].parse().map_err(|_| format!("bad z: {line}"))?;
                let state: u32 = fields[4]
                    .parse()
                    .map_err(|_| format!("bad state: {line}"))?;
                section.blocks.insert((x, y, z), state);
            }
            // `T` 行是 postProcess 抛出的异常类型，不参与比较。
            // `T` lines name the exception thrown by `postProcess`; not compared.
            "T" => {}
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    Ok(sections)
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
    let mut failures: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut compared = 0usize;
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let mut bad_by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for (q, section) in queries.iter().zip(sections.iter()) {
        let key = format!("{} {} {} {}", q.kind, q.seed, q.cx, q.cz);
        if section.key != key {
            return Err(format!(
                "query key mismatch: oracle {:?} vs {key:?}",
                section.key
            ));
        }
        let ours = rust_writes(q)?;
        *by_kind.entry(q.kind).or_default() += 1;
        let positions: std::collections::BTreeSet<(i32, i32, i32)> =
            section.blocks.keys().chain(ours.keys()).copied().collect();
        for pos in positions {
            compared += 1;
            let java = section.blocks.get(&pos).copied();
            let rust = ours.get(&pos).copied();
            if java != rust {
                mismatch_count += 1;
                *bad_by_kind.entry(q.kind).or_default() += 1;
                // 上限给到 4000 是为了能用 `statename.js` 归类全量不一致；24 条只能看到
                // 前几个区块，上一轮就因此误判过「0 不一致」。
                // The 4000 cap exists so `statename.js` can classify every mismatch; 24
                // only showed the first chunks and caused a wrong "0 mismatches" claim.
                if failures.len() < 4000 {
                    failures.push(format!("[{key}] {pos:?}: java {java:?}, rust {rust:?}"));
                }
            }
        }
    }
    let kinds = by_kind
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "structure-templated-place: {} queries ({kinds}), {compared} cell comparisons",
        queries.len()
    );
    let bad = bad_by_kind
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    if !bad.is_empty() {
        println!("structure-templated-place: mismatches by kind: {bad}");
    }
    if failures.is_empty() {
        println!("STRUCTURE-TEMPLATED-PLACE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &failures {
            println!("STRUCTURE-TEMPLATED-PLACE FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-TEMPLATED-PLACE RESULT: {mismatch_count} mismatches"
        ))
    }
}

fn rust_writes(q: &Query) -> Result<BTreeMap<(i32, i32, i32), u32>, String> {
    let chunks = synthetic_chunks(q.cx, q.cz);
    let mut world = FeatureWorld::new(&chunks, 63, q.seed)?;
    let pieces: Vec<Piece> = match q.kind {
        "igloo" => mc_bamboo_worldgen::structure::templated::igloo::generate(q.seed, q.cx, q.cz)?,
        "shipwreck_ocean" => mc_bamboo_worldgen::structure::templated::shipwreck::generate(
            q.seed, q.cx, q.cz, false,
        )?,
        "ocean_ruin_cold" | "ocean_ruin_warm" => {
            let config = mc_bamboo_worldgen::structure::templated::ocean_ruin::Config {
                biome_temp: if q.kind == "ocean_ruin_cold" {
                    mc_bamboo_worldgen::structure::templated::ocean_ruin::BiomeTemp::Cold
                } else {
                    mc_bamboo_worldgen::structure::templated::ocean_ruin::BiomeTemp::Warm
                },
                large_probability: 0.3,
                cluster_probability: 0.9,
            };
            mc_bamboo_worldgen::structure::templated::ocean_ruin::generate(
                q.seed, q.cx, q.cz, config,
            )?
        }
        other => return Err(format!("no Rust layout for kind {other}")),
    };

    let mut out: BTreeMap<(i32, i32, i32), u32> = BTreeMap::new();
    let mut boxes: Vec<[i32; 6]> = pieces.iter().map(|piece| piece.bb).collect();
    // `StructureStart.placeInChunk` 的编排：3×3 每个区块各调一次，世界逐步累积。
    // The `StructureStart.placeInChunk` orchestration: once per chunk of the 3×3 with the
    // world accumulating writes.
    for dz in -1..=1 {
        for dx in -1..=1 {
            let at = (q.cx + dx, q.cz + dz);
            let chunk_bb = [
                at.0 * 16,
                MIN_Y + 1,
                at.1 * 16,
                at.0 * 16 + 15,
                MIN_Y + HEIGHT - 1,
                at.1 * 16 + 15,
            ];
            // 官方的 `random` 是 `applyBiomeDecoration` 里 `setFeatureSeed` 过的那一个；两侧
            // 都用「世界种子」重建，保证 `calculateBeachedPosition` 的取数位置一致。
            // Vanilla's `random` is the one seeded by `setFeatureSeed`; both sides rebuild it
            // from the world seed so `calculateBeachedPosition` draws from the same position.
            let random = mc_bamboo_worldgen::random::WorldgenRandom::new(
                mc_bamboo_worldgen::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(
                    q.seed,
                )),
            );
            for (index, piece) in pieces.iter().enumerate() {
                // 官方的 `StructurePiece.boundingBox` 是**可变**的：`TemplateStructurePiece`
                // 在 `postProcess` 里重新赋值（雪屋 / 沉船 / 海底遗迹都先移动了
                // `templatePosition`）。`placeInChunk` 每个区块用**当时的**盒子判断相交，
                // 后面的区块看到的是校正后的盒子。
                // Vanilla `StructurePiece.boundingBox` is **mutable**:
                // `TemplateStructurePiece` reassigns it inside `postProcess` after moving
                // `templatePosition`. `placeInChunk` tests `intersects` against the box at
                // that moment, so later chunks see the corrected box.
                if !intersects(&boxes[index], &chunk_bb) {
                    continue;
                }
                let mut piece = piece.clone();
                piece.bb = boxes[index];
                let writes: Vec<(i32, i32, i32, u32)> = match q.kind {
                    "igloo" => {
                        let outcome = igloo::igloo(&mut world, &piece, chunk_bb, true)?;
                        let mut writes = outcome
                            .placement
                            .placed
                            .as_ref()
                            .and_then(|p| p.writes.clone())
                            .unwrap_or_default();
                        writes.extend(igloo::marker_blocks(&outcome));
                        if let Some(snow) = outcome.snow {
                            writes.push((snow.0, snow.1, snow.2, snow_state()));
                        }
                        boxes[index] = outcome.placement.bounding_box;
                        writes
                    }
                    "shipwreck_ocean" => {
                        let outcome =
                            shipwreck::shipwreck(&mut world, &piece, chunk_bb, &random, true)?;
                        boxes[index] = outcome.bounding_box;
                        outcome.placed.and_then(|p| p.writes).unwrap_or_default()
                    }
                    "ocean_ruin_cold" | "ocean_ruin_warm" => {
                        let outcome =
                            ocean_ruin::ocean_ruin(&mut world, &piece, chunk_bb, q.seed, true)?;
                        let mut writes = outcome
                            .placed
                            .as_ref()
                            .and_then(|p| p.writes.clone())
                            .unwrap_or_default();
                        writes.extend(ocean_ruin::marker_blocks(&outcome, &world));
                        boxes[index] = outcome.bounding_box;
                        writes
                    }
                    other => return Err(format!("no Rust postProcess for kind {other}")),
                };
                for (x, y, z, state) in writes {
                    out.insert((x, y, z), state);
                }
            }
        }
    }
    Ok(out)
}

fn snow_state() -> u32 {
    mc_bamboo_data::block_by_name("minecraft:snow_block")
        .map(|block| block.default_state)
        .unwrap_or(mc_bamboo_data::blocks::AIR)
}
