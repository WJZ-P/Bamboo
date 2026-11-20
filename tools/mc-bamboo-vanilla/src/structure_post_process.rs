//! 单件结构 `postProcess` 的逐方块对拍：官方 `StructurePlaceOracle` 对
//! `mc-bamboo-worldgen::structure::post_process`（子命令 `structure-post-process`）。
//!
//! 两边用同一份合成地形（`synthetic_terrain`）和一个只含该区块的 3×3 世界，然后跑
//! `postProcess` 的方块部分，逐格比「写前 vs 写后」的差异。只比方块状态：
//! 箱子战利品表要取随机数、女巫 / 猫是实体，都不在本轮比较范围（oracle 会标记
//! `entityAbort`）。
//!
//! Oracle for the single-piece `postProcess` block writes (subcommand
//! `structure-post-process`): vanilla `StructurePlaceOracle` against
//! `mc-bamboo-worldgen::structure::post_process`. Both sides build the same synthetic
//! terrain (`synthetic_terrain`) and a 3×3 world containing only that chunk, run the block part
//! of `postProcess`, and compare the write-before/write-after diff cell by cell. Only block
//! states are compared: chest loot tables draw randomness and the witch / cat are entities, both
//! out of scope this round (the oracle flags `entityAbort`).

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mc_bamboo_worldgen::{feature::world::FeatureWorld, structure::post_process};

/// 合成地形的地表公式，和 `StructurePlaceOracle.fillTerrain` 完全一致。
///
/// The synthetic terrain surface formula, identical to
/// `StructurePlaceOracle.fillTerrain`.
pub fn surface_y(x: i32, z: i32) -> i32 {
    64 + (x * 31 + z * 17).rem_euclid(5) - 2
}

/// 覆盖四种结构的种子与区块。
///
/// Seeds and chunks covering the four structures.
const SEEDS: [i64; 4] = [0, 42, -7, 12_345];
const CHUNKS: [(i32, i32); 4] = [(0, 0), (1, -1), (-3, 2), (100, -100)];
/// 每格要比较的方块范围：`minY..minY + height`。
///
/// The block range to compare: `minY..minY + height`.
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;
const KINDS: [&str; 4] = [
    "buried_treasure",
    "desert_pyramid",
    "swamp_hut",
    "jungle_temple",
];
/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
}

/// 一份合成地形：9 个区块都按同一条公式填，`postProcess` 写半径内的读写都能拿到正确的
/// 地形（官方 `WorldGenRegion` 的写半径就是 1，同样能读到邻居）。
///
/// Synthetic terrain: all nine chunks follow the same formula so `postProcess` reads and
/// writes the correct terrain within its write radius (vanilla's `WorldGenRegion` write
/// radius is 1 and can read the neighbours too).
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
                let surface = surface_y(base_x + x, base_z + z);
                let column = x as usize * 16 + z as usize;
                for y in MIN_Y..surface - 1 {
                    chunk[(y - MIN_Y) as usize][column] = mc_bamboo_data::blocks::STONE;
                }
                chunk[(surface - 1 - MIN_Y) as usize][column] = mc_bamboo_data::blocks::SANDSTONE;
                chunk[(surface - MIN_Y) as usize][column] = mc_bamboo_data::blocks::SAND;
            }
        }
        chunk
    });
    let heights: [Heightmaps; 9] = std::array::from_fn(|index| prime(&blocks[index]));
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

/// 用 Rust 侧的高度图 prime 代替官方 `Heightmap.primeHeightmaps`。
///
/// Rust-side heightmap prime, standing in for vanilla `Heightmap.primeHeightmaps`.
fn prime(
    blocks: &mc_bamboo_worldgen::feature::ChunkBlocks,
) -> mc_bamboo_worldgen::feature::Heightmaps {
    mc_bamboo_worldgen::feature::world::prime_heightmaps(blocks, MIN_Y, HEIGHT)
}

/// 一个查询的差异：`(x, y, z) -> 状态 ID`。
///
/// One query's diff: `(x, y, z) -> state id`.
type Diff = BTreeMap<(i32, i32, i32), u32>;

/// 方块实体的战利品表：(位置, 表名, 种子)。
///
/// Loot table of a block entity: `(position, table, seed)`.
type Entities = BTreeMap<(i32, i32, i32), (String, i64)>;

/// 一次查询的 Rust 结果：写入日志、随机探针和方块实体。
///
/// Rust result of one query: the write log, the random probe and the block entities.
struct RustOutcome {
    diff: Diff,
    probe: i64,
    entities: Entities,
    entity_abort: bool,
}

/// 跑一个查询的 Rust 侧，返回结构写下的方块。
///
/// 比的是「写入日志」而不是写前写后的状态差异：结构写在地形上的方块（沙漠神殿的塔身几乎
/// 全落在沙 / 砂岩上）不是状态变化，但官方确实写了。Java 侧的 `PieceLevelHandler.written`
/// 是同一套语义（后写覆盖先写）。
///
/// Run the Rust side of one query, returning the blocks the structure wrote. Compares write
/// logs rather than before/after state diffs: a block landing on existing terrain (most of the
/// desert pyramid) is not a state change yet vanilla wrote it. The Java
/// `PieceLevelHandler.written` has the same semantics (later writes win).
fn rust_diff(q: &Query) -> Result<RustOutcome, String> {
    let chunks = synthetic_chunks(q.cx, q.cz);
    let mut world = FeatureWorld::new(&chunks, 63, 0)?;
    let chunk_bb = [
        q.cx * 16,
        MIN_Y + 1,
        q.cz * 16,
        q.cx * 16 + 15,
        MIN_Y + HEIGHT - 1,
        q.cz * 16 + 15,
    ];
    let mut writes: Vec<(i32, i32, i32, u32)> = Vec::new();
    // 整条查询共用一个 `postProcess` 随机源，和 Java 侧一致。
    // One postProcess random for the whole query, matching the Java side.
    let random = mc_bamboo_worldgen::random::WorldgenRandom::new(
        mc_bamboo_worldgen::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(q.seed)),
    );
    let entity_abort = match q.kind {
        "buried_treasure" => {
            let x = q.cx * 16 + 9;
            let z = q.cz * 16 + 9;
            if let Some((chest, _)) =
                post_process::buried_treasure(&mut world, &[x, 90, z, x, 90, z], Some(&mut writes))
                && let Some(state) = post_process::create_chest(
                    &mut world,
                    &chunk_bb,
                    chest,
                    None,
                    &random,
                    Some("minecraft:chests/buried_treasure"),
                )
            {
                writes.push((chest.0, chest.1, chest.2, state));
            }
            // 埋藏的宝藏不生成实体，不会 abort。
            // The buried treasure spawns no entity, so it never aborts.
            false
        }
        "swamp_hut" => {
            // 结构件的包围盒和朝向由 `structure::single` 给出（已对拍过）。
            // The piece box and orientation come from `structure::single` (already verified).
            let pieces =
                mc_bamboo_worldgen::structure::single::generate(q.kind, q.seed, q.cx, q.cz)?;
            let piece = pieces.first().ok_or("no piece")?;
            let (_, log) = post_process::swamp_hut::swamp_hut(
                &mut world,
                &piece.bb,
                &chunk_bb,
                Some(mc_bamboo_worldgen::structure::pieces::HDir::from_2d(
                    piece.o,
                )),
                piece.rot,
                piece.mirror,
                Some(Vec::new()),
            )?;
            writes.extend(log.unwrap_or_default());
            // 女巫和猫是实体，Java 侧在这一步 abort；方块部分两边都写完了。
            // The witch and cat are entities; the Java side aborts there after writing blocks.
            true
        }
        "jungle_temple" => {
            let pieces =
                mc_bamboo_worldgen::structure::single::generate(q.kind, q.seed, q.cx, q.cz)?;
            let piece = pieces.first().ok_or("no piece")?;
            let (_, log) = post_process::jungle_temple::jungle_temple(
                &mut world,
                &piece.bb,
                &chunk_bb,
                Some(mc_bamboo_worldgen::structure::pieces::HDir::from_2d(
                    piece.o,
                )),
                piece.rot,
                piece.mirror,
                &random,
                Some(Vec::new()),
            );
            writes.extend(log.unwrap_or_default());
            false
        }
        "desert_pyramid" => {
            let pieces =
                mc_bamboo_worldgen::structure::single::generate(q.kind, q.seed, q.cx, q.cz)?;
            let piece = pieces.first().ok_or("no piece")?;
            let (_, log) = post_process::pyramid::desert_pyramid(
                &mut world,
                &piece.bb,
                &chunk_bb,
                Some(mc_bamboo_worldgen::structure::pieces::HDir::from_2d(
                    piece.o,
                )),
                piece.rot,
                piece.mirror,
                q.seed,
                false,
                q.cx,
                q.cz,
                &random,
                Some(Vec::new()),
                &mut Default::default(),
            );
            writes.extend(log.unwrap_or_default());
            false
        }
        other => return Err(format!("no postProcess for {other}")),
    };
    let probe = if entity_abort { 0 } else { random.next_long() };
    // 后写的覆盖先写的，和 Java 的 `TreeMap` 一致。
    // Later writes win, matching the Java `TreeMap`.
    let mut diff = Diff::new();
    for (x, y, z, state) in writes {
        diff.insert((x, y, z), state);
    }
    let mut entities = Entities::new();
    for (pos, nbt) in &world.block_entities {
        let mc_bamboo_nbt::Tag::Compound(entries) = nbt else {
            continue;
        };
        let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let table = match get("LootTable") {
            Some(mc_bamboo_nbt::Tag::String(name)) => name.clone(),
            _ => String::new(),
        };
        let seed = match get("LootTableSeed") {
            Some(mc_bamboo_nbt::Tag::Long(seed)) => *seed,
            _ => 0,
        };
        entities.insert(*pos, (table, seed));
    }
    Ok(RustOutcome {
        diff,
        probe,
        entities,
        entity_abort,
    })
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries: Vec<Query> = SEEDS
        .iter()
        .flat_map(|&seed| {
            CHUNKS.iter().flat_map(move |&(cx, cz)| {
                KINDS.iter().map(move |&kind| Query { kind, seed, cx, cz })
            })
        })
        .collect();
    let mut text = String::new();
    for q in &queries {
        text.push_str(&format!("{} {} {} {}\n", q.kind, q.seed, q.cx, q.cz));
    }
    fs::write(queries_path, text).map_err(|error| format!("cannot write queries file: {error}"))?;

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructurePlaceOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructurePlaceOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&queries, &String::from_utf8_lossy(&output.stdout))
}

/// 剥掉 Java 日志框架给 `System.out` 加的前缀。
///
/// Strip the prefix the Java logging layer adds to `System.out` lines.
fn strip_log_prefix(line: &str) -> &str {
    match line.find("]: [STDOUT]: ") {
        Some(index) => &line[index + "]: [STDOUT]: ".len()..],
        None => line,
    }
}

/// Java 输出的一段：`Q` 头 + `B` 方块 + `R` 随机探针 + `E` 方块实体。
///
/// One Java section: the `Q` header, `B` blocks, the `R` random probe and `E` entities.
struct Section {
    key: String,
    count: usize,
    abort: bool,
    blocks: Diff,
    probe: i64,
    entities: Entities,
}

/// 解析 Java 输出。
///
/// Parse the Java output.
fn parse_output(stdout: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    for line in stdout.lines() {
        // Java 的日志框架给 System.out 的每一行加了 `[时间] [main/INFO]: [STDOUT]: ` 前缀，
        // 先剥掉再解析。
        // The Java logging layer prefixes every System.out line with
        // `[time] [main/INFO]: [STDOUT]: `; strip it before parsing.
        let line = strip_log_prefix(line);
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        match fields[0] {
            "Q" => {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                let key = fields[1..5].join(" ");
                let count: usize = fields[5]
                    .parse()
                    .map_err(|_| format!("bad count: {line}"))?;
                let abort = fields[6] == "1";
                current = Some(Section {
                    key,
                    count,
                    abort,
                    blocks: Diff::new(),
                    probe: 0,
                    entities: Entities::new(),
                });
            }
            "B" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("B line before any Q: {line}"));
                };
                let blocks = &mut section.blocks;
                let x: i32 = fields[1].parse().map_err(|_| format!("bad x: {line}"))?;
                let y: i32 = fields[2].parse().map_err(|_| format!("bad y: {line}"))?;
                let z: i32 = fields[3].parse().map_err(|_| format!("bad z: {line}"))?;
                let state: u32 = fields[4]
                    .parse()
                    .map_err(|_| format!("bad state: {line}"))?;
                blocks.insert((x, y, z), state);
            }
            "R" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("R line before any Q: {line}"));
                };
                section.probe = fields[1]
                    .parse()
                    .map_err(|_| format!("bad probe: {line}"))?;
            }
            "E" => {
                let Some(section) = current.as_mut() else {
                    return Err(format!("E line before any Q: {line}"));
                };
                let x: i32 = fields[1].parse().map_err(|_| format!("bad x: {line}"))?;
                let y: i32 = fields[2].parse().map_err(|_| format!("bad y: {line}"))?;
                let z: i32 = fields[3].parse().map_err(|_| format!("bad z: {line}"))?;
                let table = fields[4].to_owned();
                let seed: i64 = fields[5].parse().map_err(|_| format!("bad seed: {line}"))?;
                section.entities.insert((x, y, z), (table, seed));
            }
            // `T` 行是 Java 侧 postProcess 抛异常的类型（实体路径），`D` 行是诊断用的包围盒，
            // 都不参与比较。
            // `T` lines name the exception type from the Java postProcess (entity path) and
            // `D` lines are diagnostic boxes; neither takes part in the comparison.
            "T" | "D" => {}
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
    let mut mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut total = 0usize;
    let mut cells_total = 0usize;
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let mut aborts = 0usize;
    for (q, section) in queries.iter().zip(sections.iter()) {
        let Section {
            key,
            count,
            abort: java_abort,
            blocks: java_blocks,
            probe: java_probe,
            entities: java_entities,
        } = section;
        let expected_key = format!("{} {} {} {}", q.kind, q.seed, q.cx, q.cz);
        if key != &expected_key {
            return Err(format!("query key mismatch: {key:?} vs {expected_key:?}"));
        }
        if *count != java_blocks.len() {
            return Err(format!(
                "Q {key}: header says {count} blocks, got {}",
                java_blocks.len()
            ));
        }
        if *java_abort {
            aborts += 1;
        }
        *by_kind.entry(q.kind).or_default() += 1;
        let ours = rust_diff(q)?;
        cells_total += java_blocks.len();
        let positions: std::collections::BTreeSet<(i32, i32, i32)> = java_blocks
            .keys()
            .chain(ours.diff.keys())
            .copied()
            .collect();
        for pos in positions {
            total += 1;
            let j = java_blocks.get(&pos).copied();
            let o = ours.diff.get(&pos).copied();
            if j != o {
                mismatch_count += 1;
                let describe = |state: Option<u32>| match state {
                    Some(state) => format!(
                        "{state} ({})",
                        mc_bamboo_blocks::BlockState(state).block().name
                    ),
                    None => "unchanged".to_owned(),
                };
                push(
                    &mut mismatches,
                    format!(
                        "[{key}] ({}, {}, {}):\n  java: {}\n  rust: {}",
                        pos.0,
                        pos.1,
                        pos.2,
                        describe(j),
                        describe(o)
                    ),
                );
            }
        }
        // 战利品表 / 种子与随机探针：箱子写对了但少取一次 `nextLong` 也要报。
        // Loot tables, seeds and the random probe: a missing `nextLong` must fail even when
        // the chest block itself is right.
        if !*java_abort && !ours.entity_abort && java_probe != &ours.probe {
            mismatch_count += 1;
            push(
                &mut mismatches,
                format!(
                    "[{key}] random probe: java {java_probe}, rust {}",
                    ours.probe
                ),
            );
        }
        let all_entities: std::collections::BTreeSet<(i32, i32, i32)> = java_entities
            .keys()
            .chain(ours.entities.keys())
            .copied()
            .collect();
        for pos in all_entities {
            total += 1;
            let j = java_entities.get(&pos);
            let o = ours.entities.get(&pos);
            if j != o {
                mismatch_count += 1;
                push(
                    &mut mismatches,
                    format!("[{key}] entity {pos:?}: java {j:?}, rust {o:?}"),
                );
            }
        }
    }
    let kinds = by_kind
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "structure-place: {} queries ({kinds}), {cells_total} changed blocks, {total} cell comparisons, {aborts} entity aborts",
        queries.len()
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-PLACE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-PLACE FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-PLACE RESULT: {mismatch_count} mismatches"
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
