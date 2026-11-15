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

/// 假世界的 Y 范围，和 Java 侧一致。
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    fill: &'static str,
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

fn build_queries() -> Vec<Query> {
    let mut queries = Vec::new();
    for &seed in &SEEDS {
        for &(cx, cz) in &CHUNKS {
            for kind in ["mineshaft_normal", "mineshaft_mesa", "stronghold"] {
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
    // `STRUCTURE_PLACE_LIMIT=n` 只跑前 n 个查询，定位单个 case 时省时间。
    // `STRUCTURE_PLACE_LIMIT=n` keeps only the first n queries to shorten a
    // single-case investigation.
    if let Ok(limit) = std::env::var("STRUCTURE_PLACE_LIMIT")
        && let Ok(limit) = limit.parse::<usize>()
    {
        queries.truncate(limit);
    }
    queries
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

/// 一个查询的输出：结构件清单（`L` 行）、每件写入次数（`P` 行）和方块快照（`B` 行）。
struct Section {
    key: String,
    pieces: Vec<String>,
    /// 每个结构件的写入次数，按 `(chunk_x, chunk_z, count)` 记录。
    ///
    /// Per-piece write counts, as `(chunk_x, chunk_z, count)`.
    writes: Vec<(i32, i32, usize)>,
    /// 每次调用的写入位置（只在设置了 `STRUCTURE_PLACE_TRACE` 时非空）。
    ///
    /// Write positions per invocation (only when `STRUCTURE_PLACE_TRACE` is set).
    piece_positions: Vec<(usize, i32, i32, i32, i32, i32)>,
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
                    writes: Vec::new(),
                    piece_positions: Vec::new(),
                    blocks: Vec::new(),
                });
                expected_blocks = None;
            }
            "L" => match &mut current {
                Some(section) => section.pieces.push(line[2..].to_owned()),
                None => return Err(format!("L line before any Q: {line}")),
            },
            "P" => match &mut current {
                Some(section) => {
                    let x: i32 = fields[1]
                        .parse()
                        .map_err(|_| format!("bad chunk x: {line}"))?;
                    let z: i32 = fields[2]
                        .parse()
                        .map_err(|_| format!("bad chunk z: {line}"))?;
                    let count: usize = fields[3]
                        .parse()
                        .map_err(|_| format!("bad write count: {line}"))?;
                    section.writes.push((x, z, count));
                }
                None => return Err(format!("P line before any Q: {line}")),
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
            "D" => match &mut current {
                Some(section) => {
                    // `D <invocation> <chunk_x> <chunk_z> <x> <y> <z>`。
                    // `D <invocation> <chunk_x> <chunk_z> <x> <y> <z>`.
                    let invocation: usize = fields[1]
                        .parse()
                        .map_err(|_| format!("bad invocation: {line}"))?;
                    let chunk_x: i32 = fields[2]
                        .parse()
                        .map_err(|_| format!("bad chunk x: {line}"))?;
                    let chunk_z: i32 = fields[3]
                        .parse()
                        .map_err(|_| format!("bad chunk z: {line}"))?;
                    let x: i32 = fields[4].parse().map_err(|_| format!("bad x: {line}"))?;
                    let y: i32 = fields[5].parse().map_err(|_| format!("bad y: {line}"))?;
                    let z: i32 = fields[6].parse().map_err(|_| format!("bad z: {line}"))?;
                    section
                        .piece_positions
                        .push((invocation, chunk_x, chunk_z, x, y, z));
                }
                None => return Err(format!("D line before any Q: {line}")),
            },
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
    writes: Vec<(i32, i32, usize)>,
    piece_writes: Vec<Vec<(i32, i32, i32)>>,
    blocks: Vec<String>,
}

fn rust_result(q: &Query) -> Result<RustResult, String> {
    let layout = layout(q)?;
    let center = (q.cx, q.cz);
    let mut level = FlatLevel::new(q.fill, center);

    // 和 Java 侧一样逐区块调：`chunkBB` 是该区块整列盒，`random` 每区块重播。
    // Once per chunk like Java: the chunk's full column box, random reseeded per chunk.
    let mut writes: Vec<(i32, i32, usize)> = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            let at = (q.cx + dx, q.cz + dz);
            let chunk_bb = chunk_box(at);
            let random = pieces::make_random(q.seed ^ 0x5DEECE66D, at.0, at.1);
            for (i, piece) in layout.iter().enumerate() {
                // `StructureStart.placeInChunk` 只调与本区块盒相交的结构件。
                // `StructureStart.placeInChunk` only runs pieces intersecting the chunk box.
                if !pieces::intersects(&piece.bb, &chunk_bb) {
                    continue;
                }
                let before = level.written.len();
                level.begin_piece();
                if place_piece(
                    q.kind, piece, i, &layout, &mut level, chunk_bb, center, &random,
                )?
                .is_some()
                {
                    // 命中 1% 矿车箱子：Java 侧同一位置截断，这条结构件在这里结束。
                    // Hit the 1% minecart chest: Java truncates at the same point, ending
                    // this piece here.
                    writes.push((at.0, at.1, level.written.len() - before));
                    break;
                }
                writes.push((at.0, at.1, level.written.len() - before));
            }
        }
    }

    Ok(RustResult {
        pieces: layout.iter().map(piece_line).collect(),
        writes,
        piece_writes: level.piece_writes.clone(),
        blocks: level.snapshot(),
    })
}

fn layout(q: &Query) -> Result<Vec<Piece>, String> {
    match q.kind {
        "mineshaft_normal" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, false, 0)),
        "mineshaft_mesa" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, true, 0)),
        "stronghold" => Ok(pieces::stronghold::generate(q.seed, q.cx, q.cz)),
        other => Err(format!("no Rust layout for kind {other}")),
    }
}

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
) -> Result<Option<()>, String> {
    use mc_bamboo_worldgen::structure::pieces::mineshaft_place as place;

    let _ = layout;
    if kind == "stronghold" {
        return place_stronghold_piece(piece, index, level, chunk_bb, random);
    }

    let shaft = if kind == "mineshaft_mesa" {
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
    /// 下一次结构件调用的编号。
    ///
    /// Index of the next piece invocation.
    next_invocation: usize,
    /// 当前正在记录的结构件调用编号。
    ///
    /// The invocation whose writes are being recorded.
    current_piece: usize,
    /// 每次调用写过的位置，用于逐件定位差异。
    ///
    /// Positions written per invocation, for per-piece diffing.
    piece_writes: Vec<Vec<(i32, i32, i32)>>,
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
            next_invocation: 0,
            current_piece: 0,
            piece_writes: Vec::new(),
            section_non_air: BTreeMap::new(),
        }
    }

    /// 开始一次结构件调用，返回它的编号。
    ///
    /// Start one piece invocation and return its index.
    /// 开始一次结构件调用，返回它的编号；后续写入记到这个编号下。
    ///
    /// Start one piece invocation and return its index; later writes go into that slot.
    fn begin_piece(&mut self) -> usize {
        let index = self.next_invocation;
        self.next_invocation += 1;
        self.current_piece = index;
        while self.piece_writes.len() <= index {
            self.piece_writes.push(Vec::new());
        }
        index
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

    /// `WorldGenRegion.isWithinWriteZone`：写半径 1。
    ///
    /// `WorldGenRegion.isWithinWriteZone`: write radius 1.
    fn in_write_zone(&self, x: i32, z: i32) -> bool {
        let dx = (x >> 4) - self.center.0;
        let dz = (z >> 4) - self.center.1;
        (-1..=1).contains(&dx) && (-1..=1).contains(&dz)
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
        if let Some(list) = self.piece_writes.get_mut(self.current_piece) {
            list.push((x, y, z));
        }
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
        // 高度图不受 `WorldGenRegion` 写区限制：它按整列求值，所以这里直接用
        // `initial_block` + 写入表，不走 `get_block`（那个对写区外返回空气）。
        //
        // The heightmap is not limited by the `WorldGenRegion` write area: it evaluates the
        // whole column, so read the write table plus the initial fill instead of going
        // through `get_block` (which returns air outside the write area).
        let mut y = MIN_Y + HEIGHT - 1;
        while y > MIN_Y {
            let state = self
                .blocks
                .get(&(x, y, z))
                .copied()
                .unwrap_or_else(|| self.initial_block(y));
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

    let mut failures: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut one_sided = 0usize;
    let mut pieces_total = 0usize;
    let mut block_total = 0usize;
    let mut comparisons = 0usize;
    let mut java_only_by_block: std::collections::BTreeMap<String, (usize, String)> =
        std::collections::BTreeMap::new();
    let mut rust_only_by_block: std::collections::BTreeMap<String, (usize, String)> =
        std::collections::BTreeMap::new();
    let mut all_diffs: Vec<String> = Vec::new();
    let mut write_mismatches = 0usize;

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
        // 先比每个结构件的写入次数：第一次出现数量不同就说明取数或分支分叉，
        // 比按方块名猜要精确得多。
        // Compare per-piece write counts first: the first piece whose count differs marks
        // where the draws or a branch diverge, which is far more precise than guessing by
        // block name.
        let n = section.writes.len().min(ours.writes.len());
        for i in 0..n {
            let (jx, jz, jcount) = section.writes[i];
            let (rx, rz, rcount) = ours.writes[i];
            if jcount != rcount {
                write_mismatches += 1;
                let at = if (jx, jz) == (rx, rz) {
                    format!("chunk {jx} {jz}")
                } else {
                    format!("java chunk {jx} {jz} vs rust chunk {rx} {rz}")
                };
                push(
                    &mut failures,
                    format!("[{key}] piece #{i} ({at}): java wrote {jcount}, rust wrote {rcount}"),
                );
                // 有 `D` 行时把两侧的位置集合差出来，直接看差哪几格。
                // With `D` lines available, diff the two position sets for this piece.
                if !section.piece_positions.is_empty() {
                    let theirs: std::collections::BTreeSet<(i32, i32, i32)> = section
                        .piece_positions
                        .iter()
                        .filter(|(inv, _, _, _, _, _)| *inv == i)
                        .map(|(_, _, _, x, y, z)| (*x, *y, *z))
                        .collect();
                    // 这条调用属于哪个 chunk，用来判断 `chunkBB` 的作用域。
                    // Which chunk this invocation belongs to, to check `chunkBB`'s scope.
                    let chunk = section
                        .piece_positions
                        .iter()
                        .find(|(inv, _, _, _, _, _)| *inv == i)
                        .map(|(_, cx, cz, _, _, _)| format!("chunk {cx} {cz}"))
                        .unwrap_or_else(|| "<none>".to_owned());
                    let mine: std::collections::BTreeSet<(i32, i32, i32)> = ours
                        .piece_writes
                        .get(i)
                        .map(|list| list.iter().copied().collect())
                        .unwrap_or_default();
                    for pos in theirs.difference(&mine).take(6) {
                        push(
                            &mut failures,
                            format!(
                                "[{key}]   piece #{i} ({chunk}) java wrote {pos:?}, rust did not"
                            ),
                        );
                    }
                    for pos in mine.difference(&theirs).take(6) {
                        push(
                            &mut failures,
                            format!("[{key}]   piece #{i} rust wrote {pos:?}, java did not"),
                        );
                    }
                }
            }
        }
        if section.writes.len() != ours.writes.len() {
            write_mismatches += 1;
            push(
                &mut failures,
                format!(
                    "[{key}] placed {} pieces in java, {} in rust",
                    section.writes.len(),
                    ours.writes.len()
                ),
            );
        }

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
        let java_only: Vec<&str> = theirs.difference(&mine).copied().collect();
        let rust_only: Vec<&str> = mine.difference(&theirs).copied().collect();
        mismatch_count += java_only.len() + rust_only.len();
        // 按方块名聚合，样例只留前 3 条，明细全写进 `all_diffs`。
        // Aggregate by block name with at most three samples each; the full list goes into
        // `all_diffs` for the report file.
        tally(&mut java_only_by_block, &java_only);
        tally(&mut rust_only_by_block, &rust_only);
        samples(&mut failures, &key, "java", &java_only);
        samples(&mut failures, &key, "rust", &rust_only);
        for line in &java_only {
            all_diffs.push(format!("[{key}] only in java: {line}"));
        }
        for line in &rust_only {
            all_diffs.push(format!("[{key}] only in rust: {line}"));
        }
        // 只在一侧出现的方块单独统计（不算逐块不一致），共有部分仍严格比较。
        // Positions present on only one side are counted separately (not as per-block
        // mismatches); the shared part is still compared strictly.
        one_sided += java_only.len() + rust_only.len();
    }

    if !java_only_by_block.is_empty() || !rust_only_by_block.is_empty() {
        println!("-- blocks only in java (by block) --");
        for (block, (count, sample)) in &java_only_by_block {
            println!("  {count:5} {block}   e.g. {sample}");
        }
        println!("-- blocks only in rust (by block) --");
        for (block, (count, sample)) in &rust_only_by_block {
            println!("  {count:5} {block}   e.g. {sample}");
        }
    }
    if let Ok(path) = std::env::var("STRUCTURE_PLACE_DIFFS") {
        let _ = std::fs::write(&path, all_diffs.join("\n"));
        println!("wrote {} diff lines to {path}", all_diffs.len());
    }

    println!(
        "structure-place: {} queries, {} pieces, {} blocks compared, {} comparisons, {} one-sided positions, {} piece write-count differences",
        queries.len(),
        pieces_total,
        block_total,
        comparisons,
        one_sided,
        write_mismatches
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
    if list.len() < 60 {
        list.push(line);
    }
}

/// 按方块名（`Block{minecraft:x}[...]` 里的 `minecraft:x`）累加，并记一条样例位置。
///
/// Accumulate per block name (the `minecraft:x` inside `Block{minecraft:x}[...]`) with
/// one sample position each.
fn tally(into: &mut std::collections::BTreeMap<String, (usize, String)>, lines: &[&str]) {
    for line in lines {
        // 行格式：`x y z Block{...}`。
        // Line format: `x y z Block{...}`.
        let mut parts = line.splitn(4, ' ');
        let _ = (parts.next(), parts.next(), parts.next());
        let state = parts.next().unwrap_or("");
        let name = state
            .strip_prefix("Block{")
            .and_then(|s| s.split(['}', '[']).next())
            .unwrap_or(state)
            .to_owned();
        let entry = into.entry(name).or_insert_with(|| (0, (*line).to_owned()));
        entry.0 += 1;
    }
}

/// 往失败列表里加一条按方块名聚合的摘要，附最多三条样例。
///
/// Add one aggregate line per block name to the failure list with up to three samples.
fn samples(failures: &mut Vec<String>, key: &str, side: &str, lines: &[&str]) {
    let mut by_block: std::collections::BTreeMap<String, Vec<&str>> =
        std::collections::BTreeMap::new();
    for line in lines {
        let mut parts = line.splitn(4, ' ');
        let _ = (parts.next(), parts.next(), parts.next());
        let state = parts.next().unwrap_or("");
        let name = state
            .strip_prefix("Block{")
            .and_then(|s| s.split(['}', '[']).next())
            .unwrap_or(state)
            .to_owned();
        by_block.entry(name).or_default().push(line);
    }
    for (block, list) in by_block {
        for line in list.iter().take(3) {
            push(failures, format!("[{key}] {side} only: {block} @ {line}"));
        }
    }
}
