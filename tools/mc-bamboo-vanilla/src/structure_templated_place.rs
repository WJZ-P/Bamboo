//! 四个模板结构 `postProcess` 的逐方块对拍：官方 `StructureTemplatedPlaceOracle` 对
//! `mc-bamboo-worldgen::structure::post_process::{igloo, shipwreck, ocean_ruin,
//! ruined_portal}` + `structure::template_place`。
//!
//! 两边用同一份合成地形（西半陆地、东半海；废弃传送门的查询在陆地上再撒一些熔岩），
//! Java 侧跑官方 `postProcess` 并 dump 写入日志（后写覆盖先写），Rust 侧用生产代码复算，
//! 逐格比较状态。每个区块跑完后两边再各取一个 `nextLong`（`R` 行）：箱子战利品种子这类
//! 不改方块的取数少一次、多一次，都会在这里露出来。
//!
//! Per-block oracle for the four template structures' `postProcess`: vanilla
//! `StructureTemplatedPlaceOracle` against the Rust post-process modules plus
//! `structure::template_place`. Both sides build the same synthetic terrain (land west,
//! ocean east; ruined portal queries also sprinkle lava on the land); Java runs the
//! official `postProcess` and dumps its write log (later writes win), Rust recomputes with
//! production code and compares every cell. After each chunk both sides draw one more
//! `nextLong` (the `R` lines), so a missing or extra draw that changes no block (chest
//! loot seeds) still shows.

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mc_bamboo_worldgen::{
    feature::world::FeatureWorld,
    structure::{
        pieces::{Piece, intersects},
        post_process::{igloo, ocean_ruin, ruined_portal, shipwreck},
        template::StructureTemplate,
        templated::ruined_portal::{PortalPieceSpec, Properties, VerticalPlacement, portal_piece},
    },
};

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 5] = [0, 42, -7, 1_234_567, 99];
/// 覆盖陆地 / 海面边界的区块坐标。
const CHUNKS: [(i32, i32); 4] = [(0, 0), (-1, 1), (2, -2), (5, 5)];
const KINDS: [&str; 5] = [
    "igloo",
    "shipwreck_ocean",
    "shipwreck_beached",
    "ocean_ruin_cold",
    "ocean_ruin_warm",
];
const PORTAL: &str = "ruined_portal";
/// 废弃传送门的 13 个模板。
///
/// The thirteen ruined portal templates.
const PORTAL_TEMPLATES: [&str; 13] = [
    "minecraft:ruined_portal/portal_1",
    "minecraft:ruined_portal/portal_2",
    "minecraft:ruined_portal/portal_3",
    "minecraft:ruined_portal/portal_4",
    "minecraft:ruined_portal/portal_5",
    "minecraft:ruined_portal/portal_6",
    "minecraft:ruined_portal/portal_7",
    "minecraft:ruined_portal/portal_8",
    "minecraft:ruined_portal/portal_9",
    "minecraft:ruined_portal/portal_10",
    "minecraft:ruined_portal/giant_portal_1",
    "minecraft:ruined_portal/giant_portal_2",
    "minecraft:ruined_portal/giant_portal_3",
];
/// `VerticalPlacement` 的六种：Java 枚举名、Rust 值、相对地表的 Y 偏移。
///
/// The six `VerticalPlacement`s: Java enum name, Rust value, Y offset from the surface.
const PORTAL_PLACEMENTS: [(&str, VerticalPlacement, i32); 6] = [
    ("ON_LAND_SURFACE", VerticalPlacement::OnLandSurface, 0),
    ("PARTLY_BURIED", VerticalPlacement::PartlyBuried, -3),
    ("ON_OCEAN_FLOOR", VerticalPlacement::OnOceanFloor, 0),
    ("IN_MOUNTAIN", VerticalPlacement::InMountain, 6),
    ("UNDERGROUND", VerticalPlacement::Underground, -24),
    ("IN_NETHER", VerticalPlacement::InNether, -2),
];
/// 每个（模板，放置方式）组合跑几轮，每轮换旋转、镜像、属性和地形区域。
///
/// Rounds per (template, placement) pair; each round changes rotation, mirror, properties
/// and terrain region.
const PORTAL_ROUNDS: usize = 3;
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

/// 废弃传送门的地形变体里地表是熔岩的列，和 `isLavaColumn` 一致。
///
/// The columns whose surface block is lava in the ruined portal terrain, matching
/// `isLavaColumn`.
pub fn is_lava_column(x: i32, z: i32) -> bool {
    x < 0 && (x * 13 + z * 5).rem_euclid(11) == 0
}

/// 废弃传送门查询的结构件参数。
///
/// The piece parameters of one ruined portal query.
struct Portal {
    template: &'static str,
    rotation: i32,
    mirror: i32,
    placement: VerticalPlacement,
    properties: Properties,
    origin: (i32, i32, i32),
}

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    extra: String,
    portal: Option<Portal>,
}

/// 造废弃传送门的查询：13 个模板 × 6 种放置方式 × `PORTAL_ROUNDS` 轮。结构件中心落在
/// 查询区块里，这样 `spreadNetherrack` 的 ±14 格都在 3×3 写入区内。
///
/// Build the ruined portal queries: 13 templates × 6 placements × `PORTAL_ROUNDS`. The
/// piece centre sits in the query chunk so the ±14 reach of `spreadNetherrack` stays inside
/// the 3×3 write zone.
fn portal_queries() -> Result<Vec<Query>, String> {
    let mut queries = Vec::new();
    let total = PORTAL_TEMPLATES.len() * PORTAL_PLACEMENTS.len() * PORTAL_ROUNDS;
    for index in 0..total {
        let template = PORTAL_TEMPLATES[index % PORTAL_TEMPLATES.len()];
        let (placement_name, placement, y_offset) =
            PORTAL_PLACEMENTS[(index / PORTAL_TEMPLATES.len()) % PORTAL_PLACEMENTS.len()];
        let round = index / (PORTAL_TEMPLATES.len() * PORTAL_PLACEMENTS.len());
        // 一个简单的整数散列，让属性组合和模板 / 放置方式不同步。
        // A small integer hash so the property mix is not in step with template / placement.
        let hash = (index as u32).wrapping_mul(2_654_435_761).rotate_left(7) ^ (round as u32 * 97);
        let rotation = (hash % 4) as i32;
        let mirror = [0, 2, 1][((hash >> 3) % 3) as usize];
        let properties = Properties {
            cold: hash & 0x10 != 0,
            mossiness: [0.2, 0.8, 0.0, 1.0, 0.5][((hash >> 12) % 5) as usize],
            air_pocket: hash & 0x20 != 0,
            overgrown: hash & 0x40 != 0,
            vines: hash & 0x80 != 0,
            replace_with_blackstone: hash & 0x100 != 0,
        };
        // 三种地形区域：纯陆地、陆海交界、纯海。
        // Three terrain regions: land only, the shoreline, ocean only.
        let (cx, base_y) = [(-3, 64), (-1, 64), (2, 49)][(index + round) % 3];
        let cz = (index % 5) as i32 - 2;
        let size = StructureTemplate::load(template)?.size;
        let origin = (
            cx * 16 + 8 - size.0 / 2,
            base_y + y_offset,
            cz * 16 + 8 - size.2 / 2,
        );
        let extra = format!(
            "{template}|{rotation}|{mirror}|{placement_name}|{}:{}:{}:{}:{}:{}|{}|{}|{}",
            u8::from(properties.cold),
            properties.mossiness,
            u8::from(properties.air_pocket),
            u8::from(properties.overgrown),
            u8::from(properties.vines),
            u8::from(properties.replace_with_blackstone),
            origin.0,
            origin.1,
            origin.2,
        );
        queries.push(Query {
            kind: PORTAL,
            seed: index as i64,
            cx,
            cz,
            extra,
            portal: Some(Portal {
                template,
                rotation,
                mirror,
                placement,
                properties,
                origin,
            }),
        });
    }
    Ok(queries)
}

fn build_queries() -> Result<Vec<Query>, String> {
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
                    portal: None,
                });
            }
        }
    }
    queries.extend(portal_queries()?);
    Ok(queries)
}

/// 选项 `--only <kind 前缀>`：只跑名字以它开头的查询（缩小范围时用）。
///
/// Option `--only <kind prefix>`: run only the queries whose kind starts with it (for
/// shrinking a failure).
pub fn run(mut command: Command, queries_path: &Path, options: &[&str]) -> Result<(), String> {
    let mut queries = build_queries()?;
    let mut options = options.iter();
    let mut dump: Option<String> = None;
    while let Some(option) = options.next() {
        match *option {
            "--dump" => {
                // 把官方 oracle 的原始输出留一份，缩小范围时对着看。
                // Keep the raw oracle output for reading while shrinking a failure.
                dump = Some(options.next().ok_or("--dump needs a path")?.to_string());
            }
            "--only" => {
                let prefix = options.next().ok_or("--only needs a kind prefix")?;
                queries.retain(|q| q.kind.starts_with(prefix));
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
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
    if let Some(path) = dump {
        fs::write(&path, &output.stdout)
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    compare(&queries, &String::from_utf8_lossy(&output.stdout))
}

/// 一份合成地形：9 个区块都按同一条公式填。
///
/// Synthetic terrain: nine chunks filled by the same formula.
fn synthetic_chunks(cx: i32, cz: i32, lava: bool) -> mc_bamboo_worldgen::feature::FeatureChunks {
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
                chunk[(surface - MIN_Y) as usize][column] =
                    if lava && is_lava_column(world_x, world_z) {
                        mc_bamboo_data::blocks::LAVA
                    } else {
                        mc_bamboo_data::blocks::SAND
                    };
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
        post_processing: std::array::from_fn(|_| Vec::new()),
    }
}

/// 一个查询的结果：最终方块，加上每个区块跑完后的随机源探针。
///
/// The result of one query: the final blocks plus the random probe after each chunk.
#[derive(Default)]
struct Outcome {
    blocks: BTreeMap<(i32, i32, i32), u32>,
    /// `(dx, dz) -> nextLong`。
    probes: BTreeMap<(i32, i32), i64>,
}

/// Java 输出的一段。
struct Section {
    key: String,
    aborted: usize,
    outcome: Outcome,
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
    let mut traces: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = strip_log_prefix(line);
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let number = |index: usize| -> Result<i64, String> {
            fields
                .get(index)
                .and_then(|field| field.parse().ok())
                .ok_or_else(|| format!("bad oracle line: {line}"))
        };
        match fields[0] {
            "Q" => {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                if fields.len() < 7 {
                    return Err(format!("bad Q line: {line}"));
                }
                current = Some(Section {
                    key: fields[1..5].join(" "),
                    aborted: number(6)? as usize,
                    outcome: Outcome::default(),
                });
            }
            "B" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("B line before any Q: {line}"));
                };
                section.outcome.blocks.insert(
                    (number(1)? as i32, number(2)? as i32, number(3)? as i32),
                    number(4)? as u32,
                );
            }
            "R" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("R line before any Q: {line}"));
                };
                section
                    .outcome
                    .probes
                    .insert((number(1)? as i32, number(2)? as i32), number(3)?);
            }
            // `T` / `U` 行是 `postProcess` 抛出的异常和堆栈；有异常的查询下面会判失败。
            // `T` / `U` lines carry an exception thrown by `postProcess` and its stack; a
            // query with one fails below.
            "T" | "U" => {
                if traces.len() < 40 {
                    traces.push(line.to_owned());
                }
            }
            // 其余是官方日志（例如方块实体读 nbt 时的告警），留几行给人看。
            // Anything else is vanilla logging (block entity nbt warnings and the like); keep
            // a few lines for the reader.
            _ => {
                if traces.len() < 40 {
                    traces.push(line.to_owned());
                }
            }
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    for line in &traces {
        println!("STRUCTURE-TEMPLATED-PLACE JAVA: {line}");
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
    let mut probes = 0usize;
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
        *by_kind.entry(q.kind).or_default() += 1;
        let mut fail = |text: String| {
            mismatch_count += 1;
            *bad_by_kind.entry(q.kind).or_default() += 1;
            // 上限给到 4000 是为了能归类全量不一致；只留前几十条会误判。
            // The 4000 cap keeps enough lines to classify every mismatch.
            if failures.len() < 4000 {
                failures.push(format!("[{key}] {text}"));
            }
        };
        if section.aborted != 0 {
            fail(format!(
                "the official postProcess threw {} time(s)",
                section.aborted
            ));
        }
        let ours = rust_outcome(q)?;
        let positions: std::collections::BTreeSet<(i32, i32, i32)> = section
            .outcome
            .blocks
            .keys()
            .chain(ours.blocks.keys())
            .copied()
            .collect();
        for pos in positions {
            compared += 1;
            let java = section.outcome.blocks.get(&pos).copied();
            let rust = ours.blocks.get(&pos).copied();
            if java != rust {
                fail(format!(
                    "{pos:?}: java {}, rust {}",
                    describe(java),
                    describe(rust)
                ));
            }
        }
        let chunks: std::collections::BTreeSet<(i32, i32)> = section
            .outcome
            .probes
            .keys()
            .chain(ours.probes.keys())
            .copied()
            .collect();
        for chunk in chunks {
            probes += 1;
            let java = section.outcome.probes.get(&chunk).copied();
            let rust = ours.probes.get(&chunk).copied();
            if java != rust {
                fail(format!(
                    "random probe after chunk {chunk:?}: java {java:?}, rust {rust:?}"
                ));
            }
        }
    }
    let kinds = by_kind
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "structure-templated-place: {} queries ({kinds}), {compared} cell comparisons, \
         {probes} random probes",
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

/// 按官方编排造结构件（废弃传送门的参数由查询给出，布局已单独对拍过）。
///
/// Build the pieces the way vanilla does (the ruined portal's parameters come from the
/// query; its layout has its own oracle).
fn rust_pieces(q: &Query) -> Result<Vec<Piece>, String> {
    use mc_bamboo_worldgen::structure::templated;
    Ok(match q.kind {
        "igloo" => templated::igloo::generate(q.seed, q.cx, q.cz)?,
        "shipwreck_ocean" => templated::shipwreck::generate(q.seed, q.cx, q.cz, false)?,
        "shipwreck_beached" => templated::shipwreck::generate(q.seed, q.cx, q.cz, true)?,
        "ocean_ruin_cold" | "ocean_ruin_warm" => {
            let config = templated::ocean_ruin::Config {
                biome_temp: if q.kind == "ocean_ruin_cold" {
                    templated::ocean_ruin::BiomeTemp::Cold
                } else {
                    templated::ocean_ruin::BiomeTemp::Warm
                },
                large_probability: 0.3,
                cluster_probability: 0.9,
            };
            templated::ocean_ruin::generate(q.seed, q.cx, q.cz, config)?
        }
        PORTAL => {
            let portal = q.portal.as_ref().ok_or("portal query without parameters")?;
            let size = StructureTemplate::load(portal.template)?.size;
            vec![portal_piece(PortalPieceSpec {
                template_name: portal.template,
                template_size: size,
                position: portal.origin,
                rotation: portal.rotation,
                mirror: portal.mirror,
                pivot: (size.0 / 2, size.2 / 2),
                placement: portal.placement,
                properties: portal.properties,
            })]
        }
        other => return Err(format!("no Rust layout for kind {other}")),
    })
}

fn rust_outcome(q: &Query) -> Result<Outcome, String> {
    let chunks = synthetic_chunks(q.cx, q.cz, q.kind == PORTAL);
    let mut world = FeatureWorld::new(&chunks, SEA_LEVEL, q.seed)?;
    let mut pieces = rust_pieces(q)?;
    let mut out = Outcome::default();
    // `StructureStart.placeInChunk` 的编排：3×3 每个区块各调一次，世界逐步累积。
    // The `StructureStart.placeInChunk` orchestration: once per chunk of the 3×3 with the
    // world accumulating writes.
    for dz in -1..=1 {
        for dx in -1..=1 {
            let at = (q.cx + dx, q.cz + dz);
            let mut chunk_bb = [
                at.0 * 16,
                MIN_Y + 1,
                at.1 * 16,
                at.0 * 16 + 15,
                MIN_Y + HEIGHT - 1,
                at.1 * 16 + 15,
            ];
            // 官方的 `random` 是 `applyBiomeDecoration` 里 `setFeatureSeed` 过的那一个，一个
            // 区块里所有结构件共用；两侧都用「世界种子」重建。
            // Vanilla's `random` is the one seeded by `setFeatureSeed`, shared by every
            // piece of a chunk; both sides rebuild it from the world seed.
            let random = mc_bamboo_worldgen::random::WorldgenRandom::new(
                mc_bamboo_worldgen::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(
                    q.seed,
                )),
            );
            for piece in &mut pieces {
                // 官方的 `StructurePiece.boundingBox` 是**可变**的：`TemplateStructurePiece`
                // 在 `postProcess` 里重新赋值。`placeInChunk` 每个区块用**当时的**盒子判断
                // 相交，后面的区块看到的是校正后的盒子；Rust 侧的 `postProcess` 也就地改
                // `piece.bb`。
                // Vanilla `StructurePiece.boundingBox` is **mutable**:
                // `TemplateStructurePiece` reassigns it inside `postProcess`, and
                // `placeInChunk` tests `intersects` against the box at that moment. The
                // Rust `postProcess` functions update `piece.bb` in place as well.
                if !intersects(&piece.bb, &chunk_bb) {
                    continue;
                }
                let outcome = match q.kind {
                    "igloo" => igloo::igloo(&mut world, piece, chunk_bb, &random)?,
                    "shipwreck_ocean" | "shipwreck_beached" => {
                        shipwreck::shipwreck(&mut world, piece, chunk_bb, &random)?
                    }
                    "ocean_ruin_cold" | "ocean_ruin_warm" => {
                        ocean_ruin::ocean_ruin(&mut world, piece, chunk_bb, &random)?
                    }
                    PORTAL => {
                        ruined_portal::ruined_portal(&mut world, piece, &mut chunk_bb, &random)?
                    }
                    other => return Err(format!("no Rust postProcess for kind {other}")),
                };
                for (x, y, z, state) in outcome.writes {
                    out.blocks.insert((x, y, z), state);
                }
            }
            out.probes.insert((dx, dz), random.next_long());
        }
    }
    Ok(out)
}

/// 把状态 ID 写成 `名字[属性=值,...]`，失败行里好认。
///
/// Render a state id as `name[property=value,...]` for readable failure lines.
fn describe(state: Option<u32>) -> String {
    let Some(state) = state else {
        return "nothing".to_owned();
    };
    let Some(block) = mc_bamboo_data::block_of_state(state) else {
        return format!("unknown state {state}");
    };
    let mut stride = 1_u32;
    let mut properties = Vec::new();
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        let index = (state - block.first_state) / stride % count;
        properties.push(format!(
            "{}={}",
            property.name, property.values[index as usize]
        ));
        stride *= count;
    }
    properties.reverse();
    format!("{}[{}]", block.name, properties.join(","))
}
