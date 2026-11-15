//! 结构件 `postProcess` 的逐方块对拍：官方 `StructurePlaceOracle` 对
//! `mc-bamboo-worldgen::structure::pieces::*_place`。
//!
//! Rust 写 queries.txt（`kind seed cx cz fill`），Java 在 3×3 假 `ProtoChunk` 世界里按
//! `StructureStart.placeInChunk` 的编排逐区块调官方 `postProcess`，输出结构件清单（`L` 行）
//! 与全部方块快照（`B` 行）；Rust 侧用生产代码走「布局 → `postProcess` → 同一个方块快照」，
//! 逐行比结构件清单与每个方块状态。
//!
//! Per-block oracle for piece `postProcess`: vanilla `StructurePlaceOracle` against the
//! Rust `*_place` modules. Rust writes queries.txt; Java builds pieces with the official
//! orchestration, runs `postProcess` over a fake 3×3 `ProtoChunk` world the way
//! `StructureStart.placeInChunk` does, and dumps the piece list plus every block. Rust
//! runs the production layout plus placement and compares both.

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mc_bamboo_data::{block_id_of_state, block_of_state};
use mc_bamboo_worldgen::structure::{
    piece_place::{PieceCtx, PieceLevel, block},
    pieces::{self, HDir, Piece},
};

/// 覆盖原点、负坐标、远处坐标的种子。
const SEEDS: [i64; 4] = [0, 42, -7, 12_345_678_901];
/// 覆盖不同群系带的区块坐标。
const CHUNKS: [(i32, i32); 3] = [(0, 0), (1, -1), (34, 50)];
/// 三种初始世界，覆盖「内部 / 外部」与「顶上有无方块」的分支。
const FILLS: [&str; 3] = ["stone", "air", "stone_with_ceiling"];
/// 写半径：`WorldGenRegion` 用 1；海底神殿横跨 4×4 区块，边缘区块的写入要 2 才覆盖。
///
/// Write radius: `WorldGenRegion` uses 1; the ocean monument spans 4×4 chunks, so 2
/// is needed to reach the writes of its edge chunks.
const SIDE: i32 = 2;

/// 假世界的 Y 范围，和 Java 侧一致。
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;

/// 查询类型名字（同时是 `--feature-types` 的取值）。
///
/// Query kind names, also the values accepted by `--feature-types`.
const KIND_MINESHAFT: &str = "mineshaft_normal";
const KIND_MINESHAFT_MESA: &str = "mineshaft_mesa";
const KIND_STRONGHOLD: &str = "stronghold";
const KIND_OCEAN_MONUMENT: &str = "ocean_monument";
const KIND_WOODLAND_MANSION: &str = "woodland_mansion";

/// 一条查询。
#[derive(Debug, Clone)]
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    fill: &'static str,
}

pub fn run(mut command: Command, queries_path: &Path, options: &[&str]) -> Result<(), String> {
    let filter = option_value(options, "--feature-types");
    let chunk_filter = option_value(options, "--chunk").map(|raw| {
        let mut parts = raw.split_whitespace();
        let x: i32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let z: i32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        (x, z)
    });
    let queries = build_queries(filter.as_deref(), chunk_filter);
    if queries.is_empty() {
        return Err("no queries selected".to_owned());
    }
    write_queries(queries_path, &queries)?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

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

fn build_queries(filter: Option<&str>, chunk_filter: Option<(i32, i32)>) -> Vec<Query> {
    let mut queries = Vec::new();

    // `--feature-types <name>` 只保留那一个类型；没给就用全部。
    // `--feature-types <name>` keeps only that type; all of them otherwise.
    let wants = |kind: &str| filter.is_none_or(|name| name == kind);
    let chunks: Vec<(i32, i32)> = match chunk_filter {
        Some(one) => vec![one],
        None => CHUNKS.to_vec(),
    };

    for &seed in &SEEDS {
        for &(cx, cz) in &chunks {
            for kind in [KIND_MINESHAFT, KIND_MINESHAFT_MESA, KIND_STRONGHOLD] {
                if !wants(kind) {
                    continue;
                }
                for &fill in &FILLS {
                    queries.push(Query {
                        kind,
                        seed,
                        cx,
                        cz,
                        fill,
                    });
                }
            }
            for kind in [KIND_OCEAN_MONUMENT, KIND_WOODLAND_MANSION] {
                if !wants(kind) {
                    continue;
                }
                // 海底神殿 / 林地府邸只在有水的 / 有地面的填充下有意义：
                // `air` 全空世界里所有写入都落在「原本空气」上，仍然照放。
                // The monument and the mansion are still placed in the all-air fill; the
                // writes just land on existing air.
                for &fill in &FILLS {
                    queries.push(Query {
                        kind,
                        seed,
                        cx,
                        cz,
                        fill,
                    });
                }
            }
        }
    }
    queries
}

/// `--opt value` 的取值。
///
/// Value of `--opt value`.
fn option_value(options: &[&str], name: &str) -> Option<String> {
    let index = options.iter().position(|option| *option == name)?;
    options.get(index + 1).map(|value| (*value).to_owned())
}

fn write_queries(path: &Path, queries: &[Query]) -> Result<(), String> {
    let mut text = String::new();
    for q in queries {
        text.push_str(&format!(
            "{} {} {} {} {}\n",
            q.kind, q.seed, q.cx, q.cz, q.fill
        ));
    }
    fs::write(path, text).map_err(|error| format!("cannot write queries file: {error}"))
}

/// 一个查询的输出：结构件清单（`L` 行）和方块快照（`B` 行）。
struct Section {
    key: String,
    pieces: Vec<String>,
    blocks: Vec<String>,
}

fn parse_output(stdout: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    let mut expected_blocks: Option<usize> = None;

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "Q" => {
                if let Some(section) = current.take() {
                    check_block_count(&section, expected_blocks)?;
                    sections.push(section);
                }
                current = Some(Section {
                    key: fields[1..6].join(" "),
                    pieces: Vec::new(),
                    blocks: Vec::new(),
                });
                expected_blocks = None;
            }
            "L" => match &mut current {
                Some(section) => section.pieces.push(line[2..].to_owned()),
                None => return Err(format!("L line before any Q: {line}")),
            },
            "A" => {
                // 中断计数：命中 1% 矿车箱子、需要真实 ServerLevel 的结构件次数。
                // Abort count: pieces that hit the 1% minecart chest and need a real
                // ServerLevel.
            }
            "B" => {
                expected_blocks = Some(
                    fields[1]
                        .parse()
                        .map_err(|_| format!("bad block count: {line}"))?,
                );
            }
            other => {
                // 方块行以坐标开头（数字），其余才是关键字。
                if other.parse::<i32>().is_ok() {
                    match &mut current {
                        Some(section) => section.blocks.push(line.to_owned()),
                        None => return Err(format!("block line before any Q: {line}")),
                    }
                } else {
                    return Err(format!("unknown oracle line kind {other}: {line}"));
                }
            }
        }
    }
    if let Some(section) = current.take() {
        check_block_count(&section, expected_blocks)?;
        sections.push(section);
    }
    Ok(sections)
}

fn check_block_count(section: &Section, expected: Option<usize>) -> Result<(), String> {
    match expected {
        Some(count) if count != section.blocks.len() => Err(format!(
            "Q {}: claimed {count} blocks, got {}",
            section.key,
            section.blocks.len()
        )),
        None => Err(format!("Q {}: no B header", section.key)),
        _ => Ok(()),
    }
}

/// 一个查询对应的 Rust 结果。
struct RustResult {
    pieces: Vec<String>,
    blocks: Vec<String>,
}

fn rust_result(q: &Query) -> Result<RustResult, String> {
    let center = (q.cx, q.cz);
    let mut level = FlatLevel::new(q.fill, center);
    let mut minecraft_random = 0;

    // 海底神殿的 `Children` 只有一个 `MonumentBuilding`，房间图是它的私有字段：
    // `L` 行只比那一个结构件，方块靠 `MonumentBuilding.postProcess` 自己放下整个房间图。
    // The ocean monument's `Children` holds a single `MonumentBuilding` whose room graph
    // is a private field: the `L` lines only cover that piece, and blocks come from
    // `MonumentBuilding.postProcess` placing the entire room graph itself.
    if q.kind == KIND_OCEAN_MONUMENT {
        let random = pieces::make_random(q.seed, q.cx, q.cz);
        let dir = HDir::random(&random);
        let west = q.cx * 16 - 29;
        let north = q.cz * 16 - 29;
        let monument = pieces::ocean_monument::monument_layout(&random, west, north, dir);
        for dz in -SIDE..=SIDE {
            for dx in -SIDE..=SIDE {
                let at = (q.cx + dx, q.cz + dz);
                let chunk_bb = chunk_box(at);
                let chunk_random = pieces::make_random(q.seed ^ 0x5DEECE66D, at.0, at.1);
                pieces::ocean_monument_place::place_in_chunk(
                    &monument,
                    &mut level,
                    chunk_bb,
                    63,
                    &chunk_random,
                );
            }
        }
        return Ok(RustResult {
            pieces: monument.pieces().iter().take(1).map(piece_line).collect(),
            blocks: level.snapshot(),
        });
    }

    let layout = layout(q)?;

    // 和 Java 侧一样逐区块调：`chunkBB` 是该区块整列盒，`random` 每区块重播。
    // Once per chunk like Java: the chunk's full column box, random reseeded per chunk.
    for dz in -SIDE..=SIDE {
        for dx in -SIDE..=SIDE {
            let at = (q.cx + dx, q.cz + dz);
            let chunk_bb = chunk_box(at);
            let random = pieces::make_random(q.seed ^ 0x5DEECE66D, at.0, at.1);
            for (i, piece) in layout.iter().enumerate() {
                // `StructureStart.placeInChunk` 只调与本区块盒相交的结构件。
                // `StructureStart.placeInChunk` only runs pieces intersecting the chunk box.
                if !pieces::intersects(&piece.bb, &chunk_bb) {
                    continue;
                }
                if place_piece(
                    q.kind,
                    piece,
                    i,
                    &layout,
                    &mut level,
                    chunk_bb,
                    center,
                    &random,
                    &mut minecraft_random,
                )?
                .is_some()
                {
                    // 命中 1% 矿车箱子：Java 侧同一位置截断，这条结构件在这里结束。
                    // Hit the 1% minecart chest: Java truncates at the same point, ending
                    // this piece here.
                    break;
                }
            }
            // `StructureStart.placeInChunk` 在所有结构件之后调 `structure.afterPlace`；
            // 林地府邸的那一份往下填圆石。
            // `StructureStart.placeInChunk` runs `structure.afterPlace` after all pieces;
            // the mansion's fills cobblestone downwards.
            if q.kind == KIND_WOODLAND_MANSION {
                mc_bamboo_worldgen::structure::pieces::woodland_mansion::after_place(
                    &layout, &mut level, &chunk_bb,
                );
            }
        }
    }

    Ok(RustResult {
        pieces: layout.iter().map(piece_line).collect(),
        blocks: level.snapshot(),
    })
}

fn layout(q: &Query) -> Result<Vec<Piece>, String> {
    match q.kind {
        KIND_MINESHAFT => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, false, 0)),
        KIND_MINESHAFT_MESA => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, true, 0)),
        KIND_STRONGHOLD => Ok(pieces::stronghold::generate(q.seed, q.cx, q.cz)),
        KIND_OCEAN_MONUMENT => Ok(pieces::ocean_monument::generate(q.seed, q.cx, q.cz)),
        KIND_WOODLAND_MANSION => Ok(pieces::woodland_mansion::generate(
            q.seed,
            q.cx,
            q.cz,
            MANSION_START_Y,
        )?),
        other => Err(format!("no Rust layout for kind {other}")),
    }
}

/// 林地府邸的喂入 `startY`（官方 `getLowestYIn5by5Box` 的结果，必须 >= 60）。
///
/// The `startY` fed to the woodland mansion layout (vanilla
/// `getLowestYIn5by5Box`, always >= 60).
const MANSION_START_Y: i32 = 63;

/// 按结构件类型调对应的 `postProcess`。
#[allow(clippy::too_many_arguments)]
fn place_piece(
    kind: &str,
    piece: &Piece,
    index: usize,
    layout: &[Piece],
    level: &mut FlatLevel,
    chunk_bb: [i32; 6],
    _center: (i32, i32),
    random: &mc_bamboo_worldgen::random::WorldgenRandom,
    _minecraft_random: &mut i64,
) -> Result<Option<()>, String> {
    use mc_bamboo_worldgen::structure::pieces::mineshaft_place as place;

    let _ = layout;
    if kind == KIND_STRONGHOLD {
        return place_stronghold_piece(piece, index, level, chunk_bb, random);
    }
    if kind == KIND_WOODLAND_MANSION {
        mc_bamboo_worldgen::structure::pieces::woodland_mansion::place_piece(
            piece, level, &chunk_bb, random,
        )?;
        return Ok(None);
    }

    let shaft = if kind == KIND_MINESHAFT_MESA {
        place::ShaftType::Mesa
    } else {
        place::ShaftType::Normal
    };
    let orientation = (piece.o >= 0).then(|| HDir::from_2d(piece.o));
    let ctx = PieceCtx::new(piece.bb, orientation);
    let ctx = PieceCtx {
        can_replace: Some(match shaft {
            place::ShaftType::Normal => mine_shaft_can_replace_normal,
            place::ShaftType::Mesa => mine_shaft_can_replace_mesa,
        }),
        ..ctx
    };
    let mut sp = place::ShaftPiece {
        ctx,
        shaft,
        level,
        chunk_bb,
    };

    let id = piece.id.as_str();
    match id {
        "minecraft:mscorridor" => {
            let num_sections = piece
                .extras
                .get("Num")
                .map(|v| v.token())
                .and_then(|v| v.strip_prefix("I:").and_then(|v| v.parse().ok()))
                .ok_or_else(|| format!("corridor {index} has no Num"))?;
            let has_rails = piece.extras.get("hr").is_some_and(|v| v.token() == "B:1");
            let spider_corridor = piece.extras.get("sc").is_some_and(|v| v.token() == "B:1");
            let mut state = place::CorridorPlace {
                num_sections,
                has_rails,
                spider_corridor,
                has_placed_spider: false,
            };
            let truncated = place::corridor_post_process(&mut sp, random, &mut state);
            // 返回 true 表示命中 1% 矿车箱子，Java 侧也在同一步截断。
            // Returning true means the 1% minecart chest hit; Java truncates at the same
            // point.
            return Ok(if truncated { Some(()) } else { None });
        }
        "minecraft:mscrossing" => {
            let two_floored = piece.extras.get("tf").is_some_and(|v| v.token() == "B:1");
            place::crossing_post_process(&mut sp, two_floored);
        }
        "minecraft:msstairs" => place::stairs_post_process(&mut sp),
        "minecraft:msroom" => {
            let entrances: Vec<[i32; 6]> = piece.entrances.clone();
            place::room_post_process(&mut sp, &entrances);
        }
        other => return Err(format!("no Rust postProcess for {other}")),
    }
    let _ = layout;
    Ok(None)
}

/// 要塞结构件的 `postProcess` 分发：按注册名选十二个子类之一。
///
/// Dispatch a stronghold piece to one of the twelve `postProcess` bodies.
fn place_stronghold_piece(
    piece: &Piece,
    index: usize,
    level: &mut FlatLevel,
    chunk_bb: [i32; 6],
    random: &mc_bamboo_worldgen::random::WorldgenRandom,
) -> Result<Option<()>, String> {
    use mc_bamboo_worldgen::structure::pieces::stronghold_place as sp;

    let orientation = (piece.o >= 0).then(|| HDir::from_2d(piece.o));
    let mut stronghold = sp::StrongholdPiece {
        ctx: PieceCtx::new(piece.bb, orientation),
        level,
        chunk_bb,
    };

    // 存档里的布尔取 `B:1`，整数取 `I:n`。
    // Saved booleans are `B:1`, integers `I:n`.
    let flag = |key: &str| piece.extras.get(key).is_some_and(|v| v.token() == "B:1");
    let integer = |key: &str| {
        piece
            .extras
            .get(key)
            .map(|v| v.token())
            .and_then(|v| v.strip_prefix("I:").and_then(|v| v.parse::<i32>().ok()))
            .unwrap_or(0)
    };
    let door = piece
        .extras
        .get("EntryDoor")
        .map(|v| sp::door_type_from_name(&v.token()))
        .unwrap_or(sp::SmallDoorType::Opening);

    match piece.id.as_str() {
        "minecraft:shcc" => {
            let mut chest = false;
            sp::chest_corridor_post_process(&mut stronghold, random, door, &mut chest);
        }
        "minecraft:shfc" => sp::filler_corridor_post_process(&mut stronghold, integer("Steps")),
        "minecraft:shs" => {
            sp::straight_post_process(&mut stronghold, random, door, flag("Left"), flag("Right"))
        }
        "minecraft:shph" => sp::prison_hall_post_process(&mut stronghold, random, door),
        "minecraft:shrt" => sp::right_turn_post_process(&mut stronghold, random, door, orientation),
        "minecraft:shlt" => sp::left_turn_post_process(&mut stronghold, random, door, orientation),
        "minecraft:shrc" => {
            sp::room_crossing_post_process(&mut stronghold, random, door, integer("Type"))
        }
        "minecraft:shsd" => sp::stairs_down_post_process(&mut stronghold, random, door),
        "minecraft:shstart" => sp::stairs_down_post_process(&mut stronghold, random, door),
        "minecraft:shssd" => sp::straight_stairs_down_post_process(&mut stronghold, random, door),
        "minecraft:sh5c" => sp::five_crossing_post_process(
            &mut stronghold,
            random,
            door,
            (
                flag("leftLow"),
                flag("leftHigh"),
                flag("rightLow"),
                flag("rightHigh"),
            ),
        ),
        "minecraft:shli" => sp::library_post_process(&mut stronghold, random, door, flag("Tall")),
        "minecraft:shpr" => {
            let mut spawner = false;
            sp::portal_room_post_process(&mut stronghold, random, &mut spawner);
        }
        other => return Err(format!("no stronghold postProcess for {other} ({index})")),
    }
    Ok(None)
}

/// 是不是 `Blocks.AIR`（**只**算普通空气，cave_air / void_air 不算）。/// `ProtoChunk.setBlockState` 的 `state.is(Blocks.AIR)` 是方块身份比较，不是 `isAir()`。
///
/// Whether the state is `Blocks.AIR` (**only** plain air; cave_air and void_air do not
/// count). `ProtoChunk.setBlockState` uses `state.is(Blocks.AIR)`, a block identity
/// check, not `isAir()`.
fn is_plain_air(state: u32) -> bool {
    let Some(id) = mc_bamboo_data::block_id_of_state(state) else {
        return false;
    };
    mc_bamboo_data::blocks::BLOCKS[id].name == "minecraft:air"
}

/// 是不是 `AirBlock`（空气、洞穴空气、虚空空气）。
///
/// Whether the state is an `AirBlock` (air, cave air, void air).
fn is_air_block(state: u32) -> bool {
    mc_bamboo_data::state_facts(state) & mc_bamboo_data::facts::AIR != 0
}

fn chunk_box(pos: (i32, i32)) -> [i32; 6] {
    [
        pos.0 * 16,
        MIN_Y,
        pos.1 * 16,
        pos.0 * 16 + 15,
        MIN_Y + HEIGHT - 1,
        pos.1 * 16 + 15,
    ]
}

/// `MineShaftPiece.canBeReplaced`（普通矿井材质）。
///
/// `MineShaftPiece.canBeReplaced` for the normal mineshaft wood.
fn mine_shaft_can_replace_normal(state: u32) -> bool {
    use mc_bamboo_worldgen::structure::pieces::mineshaft_place as place;
    place::can_replace(place::ShaftType::Normal, state)
}

/// `MineShaftPiece.canBeReplaced`（恶地矿井材质）。
///
/// `MineShaftPiece.canBeReplaced` for the mesa mineshaft wood.
fn mine_shaft_can_replace_mesa(state: u32) -> bool {
    use mc_bamboo_worldgen::structure::pieces::mineshaft_place as place;
    place::can_replace(place::ShaftType::Mesa, state)
}

fn piece_line(piece: &Piece) -> String {
    let mut extras: Vec<String> = piece
        .extras
        .iter()
        .map(|(key, value)| format!("{key}={}", value.token()))
        .collect();
    if !piece.entrances.is_empty() {
        let parts: Vec<String> = piece
            .entrances
            .iter()
            .map(|b| format!("{}_{}_{}_{}_{}_{}", b[0], b[1], b[2], b[3], b[4], b[5]))
            .collect();
        extras.push(format!("Entrances={}", parts.join("/")));
    }
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

/// 假世界：无限的稀疏方块表，只记写过的方块。
///
/// 写范围照 `WorldGenRegion`（半径 1，即中心区块 ±1）与 y 轴构建高度；
/// 范围外的读写分别记为「丢弃」和空气。
///
/// Fake world: an unbounded sparse block map that only records written positions.
/// The write area follows `WorldGenRegion` (radius 1 around the centre chunk) and
/// the build height; reads and writes outside it yield air / are dropped.
struct FlatLevel {
    blocks: BTreeMap<(i32, i32, i32), u32>,
    written: Vec<(i32, i32, i32)>,
    fill: &'static str,
    center: (i32, i32),
    /// 每个 16³ 段里非空气方块的个数（`LevelChunkSection.hasOnlyAir` 的等价物）。
    /// 初始填充先算入，之后每次写入即时维护。
    ///
    /// Non-air block count per 16³ section (the `LevelChunkSection.hasOnlyAir`
    /// equivalent). The initial fill seeds it; each write updates it.
    section_non_air: BTreeMap<(i32, i32, i32), usize>,
}

impl FlatLevel {
    fn new(fill: &'static str, center: (i32, i32)) -> Self {
        Self {
            blocks: BTreeMap::new(),
            written: Vec::new(),
            fill,
            center,
            section_non_air: BTreeMap::new(),
        }
    }

    /// 段坐标：`(chunk_x, section_y, chunk_z)`。
    ///
    /// Section key: `(chunk_x, section_y, chunk_z)`.
    fn section_key(x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        (x >> 4, y >> 4, z >> 4)
    }

    /// `LevelChunkSection.hasOnlyAir()`。
    ///
    /// `LevelChunkSection.hasOnlyAir()`.
    fn section_is_empty(&self, x: i32, y: i32, z: i32) -> bool {
        self.section_non_air
            .get(&Self::section_key(x, y, z))
            .map_or_else(
                // 没有计数记录时按初始填充推：这一段的 16 层里只要有一层在填充范围
                // 内、且填充不是空气，就算非空。
                // Without a counter entry, derive from the initial fill: any of the 16
                // layers inside the fill range (and the fill is not air) makes it non-empty.
                || {
                    let base_y = (y >> 4) << 4;
                    (0..16).any(|dy| !is_air_block(self.initial_block(base_y + dy)))
                },
                |&count| count == 0,
            )
    }

    /// `WorldGenRegion.isWithinWriteZone`：写半径。这里取 `SIDE`（比官方的 1 宽），
    /// 因为对拍刻意把 5×5 区块全部当中心跑一遍，好让横跨 4×4 区块的海底神殿的每个
    /// 边缘区块都被走到。Java 侧同样用 `SIDE`，语义一致。
    ///
    /// `WorldGenRegion.isWithinWriteZone`: the write radius. `SIDE` is wider than
    /// vanilla's radius because the oracle deliberately runs all 5×5 chunks as the
    /// centre, so every edge chunk of the 4×4-chunk ocean monument is exercised. The Java
    /// side uses the same `SIDE`.
    fn in_write_zone(&self, x: i32, z: i32) -> bool {
        let dx = (x >> 4) - self.center.0;
        let dz = (z >> 4) - self.center.1;
        (-SIDE..=SIDE).contains(&dx) && (-SIDE..=SIDE).contains(&dz)
    }

    /// 初始状态下这一格的方块（`stone` 与 `stone_with_ceiling` 在 y 56..=62 / 63）。
    /// 所有区块都一样填，和 Java 侧 `fillChunk` 的惰性填充等价。
    ///
    /// The initial state of one position (`stone` and `stone_with_ceiling` fill y 56..=62
    /// / 63). Every chunk is filled the same way, matching the Java side's lazy fill.
    fn initial_block(&self, y: i32) -> u32 {
        if self.fill == "air" {
            return mc_bamboo_data::blocks::AIR;
        }
        let last = if self.fill == "stone_with_ceiling" {
            63
        } else {
            62
        };
        if (56..=last).contains(&y) {
            block("minecraft:stone")
        } else {
            mc_bamboo_data::blocks::AIR
        }
    }

    /// 只输出「和初始填充不同的非空气方块」，按 x、y、z 排序。
    ///
    /// air 家族之间的区分在官方的 `ProtoChunk` 里不可靠（`PalettedContainer` 按
    /// `state.isAir()` 判定，cave_air / void_air 都算空气，单值调色板不会真的落盘），
    /// 所以两侧都只比非空气方块。结构件写空气只为挖空。
    ///
    /// Only positions that differ from the initial fill and are not air, sorted by x, y,
    /// z. Air-family distinctions are unreliable in vanilla's `ProtoChunk`
    /// (`PalettedContainer` tests `state.isAir()`), so both sides compare non-air blocks.
    fn snapshot(&self) -> Vec<String> {
        let mut out: BTreeMap<(i32, i32, i32), String> = BTreeMap::new();
        for &(x, y, z) in &self.written {
            let state = self.get_block(x, y, z);
            if is_air_block(state) {
                out.remove(&(x, y, z));
                continue;
            }
            if state != self.initial_block(y) {
                out.insert((x, y, z), format!("{x} {y} {z} {}", state_name(state)));
            } else {
                out.remove(&(x, y, z));
            }
        }
        out.into_values().collect()
    }
}

impl PieceLevel for FlatLevel {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        if !self.in_write_zone(x, z) || !(MIN_Y..MIN_Y + HEIGHT).contains(&y) {
            return mc_bamboo_data::blocks::AIR;
        }
        self.blocks
            .get(&(x, y, z))
            .copied()
            .unwrap_or_else(|| self.initial_block(y))
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        if !self.in_write_zone(x, z) || !(MIN_Y..MIN_Y + HEIGHT).contains(&y) {
            return false;
        }
        // `ProtoChunk.setBlockState`：section 原本全空、写入的又恰好是 `Blocks.AIR`
        // （普通空气，**不含** cave_air / void_air）时直接返回，方块不落盘：
        // `if (wasEmpty && state.is(Blocks.AIR)) return state;`。
        // `ProtoChunk.setBlockState` returns early for plain `Blocks.AIR` (not cave_air or
        // void_air) written into an otherwise empty section.
        if is_plain_air(state) && self.section_is_empty(x, y, z) {
            return true;
        }
        let key = Self::section_key(x, y, z);
        let previous = self.get_block(x, y, z);
        let was_air = is_air_block(previous);
        let is_air = is_air_block(state);
        if was_air && !is_air {
            *self.section_non_air.entry(key).or_insert(0) += 1;
        } else if !was_air && is_air {
            let count = self.section_non_air.entry(key).or_insert(0);
            *count = count.saturating_sub(1);
        }
        self.written.push((x, y, z));
        self.blocks.insert((x, y, z), state);
        true
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MIN_Y + HEIGHT - 1
    }

    /// 假世界的高度图按「方块列自顶向下第一个 `BLOCKS_MOTION_IN_HEIGHTMAP` 方块之上」
    /// 求值（`OCEAN_FLOOR_WG`，`Heightmap.Types.isOpaque` = `MATERIAL_MOTION_BLOCKING`）；
    /// 这正是 `ProtoChunk` 里那张真高度图的定义。
    ///
    /// The fake world's heightmap is the first `BLOCKS_MOTION_IN_HEIGHTMAP` block from the
    /// top plus one (`OCEAN_FLOOR_WG`, `Heightmap.Types.isOpaque`), which is exactly the
    /// real heightmap `ProtoChunk` keeps.
    fn ocean_floor_height(&self, x: i32, z: i32) -> i32 {
        let mut y = MIN_Y + HEIGHT - 1;
        while y > MIN_Y {
            let state = self.get_block(x, y, z);
            if mc_bamboo_data::state_facts(state) & mc_bamboo_data::facts::MOTION_BLOCKING != 0 {
                return y + 1;
            }
            y -= 1;
        }
        MIN_Y
    }
}

/// 状态的 `Block{name[props]}` 名字，和 Java `BlockState.toString()` 一致。
///
/// The `Block{name[props]}` name, matching Java's `BlockState.toString()`.
fn state_name(state: u32) -> String {
    let Some(id) = block_id_of_state(state) else {
        return format!("<state {state}>");
    };
    let block = &mc_bamboo_data::blocks::BLOCKS[id];
    let _ = block_of_state(state);
    let mut out = format!("Block{{{}}}", block.name);
    if !block.properties.is_empty() {
        out.push('[');
        let mut stride = 1_u32;
        let mut parts = Vec::with_capacity(block.properties.len());
        for property in block.properties.iter().rev() {
            let count = property.values.len() as u32;
            let index = (state - block.first_state) / stride % count;
            parts.push(format!(
                "{}={}",
                property.name, property.values[index as usize]
            ));
            stride *= count;
        }
        parts.reverse();
        out.push_str(&parts.join(","));
        out.push(']');
    }
    out
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
    // 调试用：把 Rust 的方块快照写到文件，方便和 Java 的输出直接 diff。
    // Debug aid: dump the Rust block snapshots so they can be diffed against Java's.
    if std::env::var_os("BAMBOO_STRUCTURE_PLACE_DUMP_RUST").is_some() {
        let mut dump = String::new();
        for q in queries {
            let ours = rust_result(q)?;
            dump.push_str(&format!(
                "Q {} {} {} {} {}\n",
                q.kind, q.seed, q.cx, q.cz, q.fill
            ));
            for line in &ours.pieces {
                dump.push_str(line);
                dump.push('\n');
            }
            dump.push_str(&format!("B {}\n", ours.blocks.len()));
            for line in &ours.blocks {
                dump.push_str(line);
                dump.push('\n');
            }
        }
        std::fs::write("structure_place_rust.txt", dump)
            .map_err(|error| format!("cannot write the rust dump: {error}"))?;
    }

    let mut failures: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut one_sided = 0usize;
    let mut pieces_total = 0usize;
    let mut block_total = 0usize;
    let mut comparisons = 0usize;

    for (q, section) in queries.iter().zip(sections.iter()) {
        let key = format!("{} {} {} {} {}", q.kind, q.seed, q.cx, q.cz, q.fill);
        if section.key != key {
            return Err(format!(
                "query key mismatch: oracle {:?} vs {key:?}",
                section.key
            ));
        }
        let ours = rust_result(q)?;
        pieces_total += section.pieces.len();
        block_total += section.blocks.len();
        report_lines(
            &mut failures,
            &mut mismatch_count,
            &mut comparisons,
            &key,
            "L",
            &section.pieces,
            &ours.pieces,
        );
        // 方块用集合比：先各自去重，再比「只在一边出现」的位置，索引不会因为一侧多写而错位。
        // Compare blocks as sets: dedupe first, then report only-in-one-side positions, so a
        // side that writes extra blocks does not shift every later index.
        let theirs: std::collections::BTreeSet<&str> =
            section.blocks.iter().map(String::as_str).collect();
        let mine: std::collections::BTreeSet<&str> =
            ours.blocks.iter().map(String::as_str).collect();
        comparisons += theirs.len().max(mine.len());
        for line in theirs.difference(&mine).take(8) {
            mismatch_count += 1;
            push(&mut failures, format!("[{key}] B only in java: {line}"));
        }
        for line in mine.difference(&theirs).take(8) {
            mismatch_count += 1;
            push(&mut failures, format!("[{key}] B only in rust: {line}"));
        }
        // 只在一侧出现的方块单独统计（不算逐块不一致），共有部分仍严格比较。
        // Positions present on only one side are counted separately (not as per-block
        // mismatches); the shared part is still compared strictly.
        one_sided += theirs.symmetric_difference(&mine).count();
        if !theirs.is_subset(&mine) || !mine.is_subset(&theirs) {
            println!(
                "  case {key}: java {} blocks, rust {} blocks, java-only {}, rust-only {}",
                theirs.len(),
                mine.len(),
                theirs.difference(&mine).count(),
                mine.difference(&theirs).count()
            );
        }
    }

    println!(
        "structure-place: {} queries, {} pieces, {} blocks compared, {} comparisons, {} one-sided positions",
        queries.len(),
        pieces_total,
        block_total,
        comparisons,
        one_sided
    );
    if failures.is_empty() {
        println!("STRUCTURE-PLACE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in failures.iter().take(24) {
            println!("STRUCTURE-PLACE FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-PLACE RESULT: {mismatch_count} mismatches"
        ))
    }
}

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
