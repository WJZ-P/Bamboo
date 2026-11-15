//! 结构件 `postProcess` 的逐方块对拍：官方 `StructurePlaceOracle` 对
//! `mc-bamboo-worldgen::structure::pieces::*_place`。
//!
//! Rust 写 queries.txt（`kind seed cx cz fill`），Java 在假 `ProtoChunk` 世界里按
//! `StructureStart.placeInChunk` 的编排，对整个结构碰到的每个区块调官方 `postProcess`，输出结构件清单（`L` 行）、
//! 每次调用的写入次数（`P` 行）与每个写过的位置的最终状态（`B` 之后的行）；Rust 侧用生产
//! 代码走「布局 → `postProcess`」，逐项比较。
//!
//! 定位用的环境变量：
//! * `STRUCTURE_PLACE_KINDS=a,b`：只跑这些类型；
//! * `STRUCTURE_PLACE_ONLY="kind seed cx cz fill"`：只跑这一条查询；
//! * `STRUCTURE_PLACE_TRACE=1`：两侧把每次调用的写入**按顺序**输出，逐次调用找出第一处
//!   分叉（配合 `STRUCTURE_PLACE_ONLY` 用，否则输出很大）；
//! * `STRUCTURE_PLACE_PER_QUERY=1`：逐查询打印差异数。
//!
//! Per-block oracle for piece `postProcess`: vanilla `StructurePlaceOracle` against the
//! Rust `*_place` modules. Rust writes queries.txt; Java builds pieces with the official
//! orchestration, runs `postProcess` over a fake `ProtoChunk` world for every chunk the
//! whole structure touches, the way `StructureStart.placeInChunk` does, and dumps the piece list (`L`), the write count of
//! every invocation (`P`) and the final state of every written position. Rust runs the
//! production layout plus placement and compares all of it.
//!
//! Environment switches for narrowing down: `STRUCTURE_PLACE_KINDS=a,b` keeps the named
//! kinds; `STRUCTURE_PLACE_ONLY="kind seed cx cz fill"` keeps one query;
//! `STRUCTURE_PLACE_TRACE=1` makes both sides dump every invocation's writes in order and
//! reports the first divergence per invocation (use with `STRUCTURE_PLACE_ONLY`, the
//! output is large otherwise); `STRUCTURE_PLACE_PER_QUERY=1` prints per-query counts.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
    process::Command,
};

use mc_bamboo_data::block_id_of_state;
use mc_bamboo_worldgen::structure::{
    piece_place::{PieceCtx, PieceLevel, block},
    pieces::{self, HDir, Piece},
};

/// 世界种子。
///
/// World seeds.
const SEEDS: [i64; 4] = [0, 42, -7, 12_345_678_901];
/// 结构起点所在的区块：原点附近和远处、正负坐标都有。整个结构碰到的区块都会放置，
/// 所以起点不用多。
///
/// Start chunks: near the origin and far away, positive and negative. Every chunk the
/// whole structure touches is placed, so a few starts are enough.
const CHUNKS: [(i32, i32); 2] = [(0, 0), (34, -50)];
/// 四种初始世界（定义见 [`FlatLevel::initial_block`]）：全空、地表一层石头、实心石头、
/// 带空气洞和水洞的石头。覆盖「内部 / 外部」、支撑、柱子 / 铁链、`isInInvalidLocation`。
///
/// Four initial worlds (see [`FlatLevel::initial_block`]): empty, a surface slab, solid
/// stone, and stone with air and water pockets. They cover interior versus exterior,
/// supports, pillars and chains, and `isInInvalidLocation`.
const FILLS: [&str; 4] = ["air", "band", "stone", "caves"];
/// 海底神殿的初始世界：把 `band` 换成 `ocean`（有海床、海水和冰面，见
/// [`FlatLevel::initial_block`]），`generateWaterBox` / `generateBoxOnFillOnly` /
/// `fillColumnDown` 的分支才走得到。
///
/// Initial worlds for the ocean monument: `band` is swapped for `ocean` (a sea floor,
/// water and surface ice, see [`FlatLevel::initial_block`]) so the branches of
/// `generateWaterBox`, `generateBoxOnFillOnly` and `fillColumnDown` are reached.
const MONUMENT_FILLS: [&str; 4] = ["air", "ocean", "stone", "caves"];
/// 林地府邸的初始世界：不用 `air`（`afterPlace` 会把每一列一路填到世界底，输出太大），
/// 换成 `ocean`，让「往下填圆石」穿过水、停在海床上。
///
/// Initial worlds for the woodland mansion: `air` is left out (`afterPlace` would fill
/// every column down to the world floor, a huge output) in favour of `ocean`, where the
/// cobblestone fill passes through water and stops on the sea floor.
const MANSION_FILLS: [&str; 4] = ["ocean", "band", "stone", "caves"];
/// 林地府邸喂入的 `startY`（官方是 `getLowestYIn5by5Box` 的结果，必须 >= 60），和 Java
/// 侧的 `MANSION_START_Y` 一致。
///
/// The `startY` fed to the woodland mansion (vanilla takes it from
/// `getLowestYIn5by5Box`, always >= 60); the same value as Java's `MANSION_START_Y`.
const MANSION_START_Y: i32 = 64;

/// 假世界的 Y 范围，和 Java 侧一致。
const MIN_Y: i32 = -64;
const HEIGHT: i32 = 384;
/// `level.getSeaLevel()`，和 Java 侧一致。
///
/// `level.getSeaLevel()`, the same as on the Java side.
const SEA_LEVEL: i32 = 63;

/// 一条查询。
struct Query {
    kind: &'static str,
    seed: i64,
    cx: i32,
    cz: i32,
    fill: &'static str,
}

impl Query {
    fn key(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.kind, self.seed, self.cx, self.cz, self.fill
        )
    }
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries = build_queries();
    if queries.is_empty() {
        return Err("no structure-place query selected".to_owned());
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

fn build_queries() -> Vec<Query> {
    let mut queries = Vec::new();
    for &seed in &SEEDS {
        for &(cx, cz) in &CHUNKS {
            for (kind, fills) in [
                ("mineshaft_normal", &FILLS),
                ("mineshaft_mesa", &FILLS),
                ("stronghold", &FILLS),
                ("ocean_monument", &MONUMENT_FILLS),
                ("woodland_mansion", &MANSION_FILLS),
            ] {
                for &fill in fills {
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
    if let Ok(kinds) = std::env::var("STRUCTURE_PLACE_KINDS") {
        let wanted: Vec<&str> = kinds.split(',').map(str::trim).collect();
        queries.retain(|q| wanted.contains(&q.kind));
    }
    if let Ok(only) = std::env::var("STRUCTURE_PLACE_ONLY") {
        let only = only.split_whitespace().collect::<Vec<_>>().join(" ");
        queries.retain(|q| q.key() == only);
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

/// 一次 `postProcess` 调用：第几个结构件、在哪个区块、写了几次，以及（开了 trace 时）
/// 按顺序的每一次写入 `x y z state`。
///
/// One `postProcess` invocation: which piece, in which chunk, how many writes, and (with
/// tracing on) every write in order as `x y z state`.
#[derive(Default)]
struct Invocation {
    piece: usize,
    chunk: (i32, i32),
    count: usize,
    trace: Vec<String>,
}

/// 一个查询的输出：结构件清单（`L` 行）、逐次调用（`P` / `D` 行）和最终方块。
///
/// One query's output: the piece list (`L`), the invocations (`P` / `D`) and the final
/// blocks.
struct Section {
    key: String,
    pieces: Vec<String>,
    invocations: Vec<Invocation>,
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
        if fields[0] == "Q" {
            if let Some(section) = current.take() {
                check_block_count(&section, expected_blocks)?;
                sections.push(section);
            }
            current = Some(Section {
                key: fields[1..6].join(" "),
                pieces: Vec::new(),
                invocations: Vec::new(),
                blocks: Vec::new(),
            });
            expected_blocks = None;
            continue;
        }
        // 第一个 `Q` 之前只会是官方日志（读府邸模板时箱子物品的反序列化警告），跳过。
        // Anything before the first `Q` is vanilla logging (the item decoding warnings
        // when the mansion templates load), skipped.
        let Some(section) = &mut current else {
            continue;
        };
        let number = |index: usize| -> Result<i64, String> {
            fields
                .get(index)
                .and_then(|field| field.parse().ok())
                .ok_or_else(|| format!("bad oracle line: {line}"))
        };
        match fields[0] {
            "L" => section.pieces.push(line[2..].to_owned()),
            // `P <invocation> <piece> <chunk_x> <chunk_z> <count>`。
            // `P <invocation> <piece> <chunk_x> <chunk_z> <count>`.
            "P" => {
                if number(1)? as usize != section.invocations.len() {
                    return Err(format!("P line out of order: {line}"));
                }
                section.invocations.push(Invocation {
                    piece: number(2)? as usize,
                    chunk: (number(3)? as i32, number(4)? as i32),
                    count: number(5)? as usize,
                    trace: Vec::new(),
                });
            }
            // `D <invocation> <x> <y> <z> <state>`，紧跟在它的 `P` 行后面。
            // `D <invocation> <x> <y> <z> <state>`, right after its `P` line.
            "D" => {
                let invocation = number(1)? as usize;
                let rest = line.splitn(3, ' ').nth(2).unwrap_or("").to_owned();
                section
                    .invocations
                    .get_mut(invocation)
                    .ok_or_else(|| format!("D line without P: {line}"))?
                    .trace
                    .push(rest);
            }
            "B" => expected_blocks = Some(number(1)? as usize),
            // 方块行以坐标开头（数字），其余才是关键字。
            // Block lines start with a coordinate; anything else is a keyword.
            other if other.parse::<i32>().is_ok() => section.blocks.push(line.to_owned()),
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
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
///
/// The Rust result of one query.
struct RustResult {
    pieces: Vec<String>,
    invocations: Vec<Invocation>,
    blocks: Vec<String>,
}

fn rust_result(q: &Query) -> Result<RustResult, String> {
    let layout = layout(q)?;
    let mut level = FlatLevel::new(q.fill, (q.cx, q.cz));
    // 结构件上跨区块调用保留的可变字段（`hasPlacedSpider` / `hasPlacedChest` /
    // `hasPlacedSpawner`），按 layout 下标保存。
    // Mutable piece fields that survive the per-chunk invocations (`hasPlacedSpider`,
    // `hasPlacedChest`, `hasPlacedSpawner`), kept by layout index.
    let mut piece_flags: Vec<bool> = vec![false; layout.len()];
    // 海底神殿的清单里只有一个 `MonumentBuilding`，房间图是它的私有字段；放置要用完整的
    // 布局（房间图 + 子件）。
    // The monument's list holds a single `MonumentBuilding` whose room graph is a private
    // field; placement needs the full layout (room graph plus children).
    let monument = (q.kind == "ocean_monument").then(|| monument_layout(q));

    // 和 Java 侧一样：对结构包围盒碰到的每个区块各调一次（先 z 后 x），`chunkBB` 是该
    // 区块整列盒，`random` 每区块重播，写区是这个区块的 ±1。
    // Like Java: once per chunk the structure's bounding box touches (z outer, x inner),
    // with the chunk's full column box, the random reseeded per chunk and the write zone
    // being that chunk plus or minus one.
    let mut invocations: Vec<Invocation> = Vec::new();
    let chunks = |low: usize, high: usize| {
        let min = layout.iter().map(|piece| piece.bb[low]).min().unwrap_or(0);
        let max = layout.iter().map(|piece| piece.bb[high]).max().unwrap_or(0);
        (min >> 4)..=(max >> 4)
    };
    for chunk_z in chunks(2, 5) {
        for chunk_x in chunks(0, 3) {
            let at = (chunk_x, chunk_z);
            level.center = at;
            let chunk_bb = chunk_box(at);
            let random = pieces::make_random(q.seed ^ 0x5DEECE66D, at.0, at.1);
            for (i, piece) in layout.iter().enumerate() {
                // `StructureStart.placeInChunk` 只调与本区块盒相交的结构件。
                // `StructureStart.placeInChunk` only runs pieces intersecting the chunk box.
                if !pieces::intersects(&piece.bb, &chunk_bb) {
                    continue;
                }
                level.piece_writes.clear();
                if let Some(monument) = &monument {
                    pieces::ocean_monument_place::place_in_chunk(
                        monument, &mut level, chunk_bb, SEA_LEVEL, &random,
                    );
                } else {
                    place_piece(
                        q.kind,
                        piece,
                        i,
                        &mut level,
                        chunk_bb,
                        &random,
                        &mut piece_flags,
                    )?;
                }
                invocations.push(Invocation {
                    piece: i,
                    chunk: at,
                    count: level.piece_writes.len(),
                    trace: std::mem::take(&mut level.piece_writes),
                });
            }
            if q.kind == "woodland_mansion" {
                // `StructureStart.placeInChunk` 末尾的 `structure.afterPlace(...)`，和 Java 侧
                // 一样记成一次结构件下标为 `layout.len()` 的调用。
                // The `structure.afterPlace(...)` at the end of `StructureStart.placeInChunk`,
                // recorded like Java as one invocation whose piece index is `layout.len()`.
                level.piece_writes.clear();
                pieces::woodland_mansion::after_place(&layout, &mut level, &chunk_bb);
                invocations.push(Invocation {
                    piece: layout.len(),
                    chunk: at,
                    count: level.piece_writes.len(),
                    trace: std::mem::take(&mut level.piece_writes),
                });
            }
        }
    }

    Ok(RustResult {
        pieces: layout.iter().map(piece_line).collect(),
        invocations,
        blocks: level.snapshot(),
    })
}

fn layout(q: &Query) -> Result<Vec<Piece>, String> {
    match q.kind {
        "mineshaft_normal" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, false, 0)),
        "mineshaft_mesa" => Ok(pieces::mineshaft::generate(q.seed, q.cx, q.cz, true, 0)),
        "stronghold" => Ok(pieces::stronghold::generate(q.seed, q.cx, q.cz)),
        // `Children` 里只存 `MonumentBuilding` 一件（清单的第 0 项）。
        // Only the `MonumentBuilding` is stored in `Children` (item 0 of the list).
        "ocean_monument" => Ok(monument_layout(q).pieces().into_iter().take(1).collect()),
        "woodland_mansion" => {
            pieces::woodland_mansion::generate(q.seed, q.cx, q.cz, MANSION_START_Y)
        }
        other => Err(format!("no Rust layout for kind {other}")),
    }
}

/// `OceanMonumentStructure.createTopPiece`：先取朝向，再造 `MonumentBuilding`（房间图在
/// 它的构造器里取数）。
///
/// `OceanMonumentStructure.createTopPiece`: draw the orientation, then build the
/// `MonumentBuilding` (its constructor draws the room graph).
fn monument_layout(q: &Query) -> pieces::ocean_monument::MonumentLayout {
    let random = pieces::make_random(q.seed, q.cx, q.cz);
    let dir = HDir::random(&random);
    pieces::ocean_monument::monument_layout(&random, q.cx * 16 - 29, q.cz * 16 - 29, dir)
}

/// 按结构件类型调对应的 `postProcess`。
///
/// Dispatch one piece to its `postProcess`.
fn place_piece(
    kind: &str,
    piece: &Piece,
    index: usize,
    level: &mut FlatLevel,
    chunk_bb: [i32; 6],
    random: &mc_bamboo_worldgen::random::WorldgenRandom,
    piece_flags: &mut [bool],
) -> Result<(), String> {
    use mc_bamboo_worldgen::structure::pieces::mineshaft_place as place;

    if kind == "stronghold" {
        return place_stronghold_piece(piece, index, level, chunk_bb, random, piece_flags);
    }
    if kind == "woodland_mansion" {
        return pieces::woodland_mansion::place_piece(piece, level, &chunk_bb, random);
    }

    let shaft = if kind == "mineshaft_mesa" {
        place::ShaftType::Mesa
    } else {
        place::ShaftType::Normal
    };
    let orientation = (piece.o >= 0).then(|| HDir::from_2d(piece.o));
    let ctx = PieceCtx::new(piece.bb, orientation).with_can_replace(match shaft {
        place::ShaftType::Normal => mine_shaft_can_replace_normal,
        place::ShaftType::Mesa => mine_shaft_can_replace_mesa,
    });
    let mut sp = place::ShaftPiece {
        ctx,
        shaft,
        level,
        chunk_bb,
        // 假世界的群系恒为平原，不在 `MINESHAFT_BLOCKING` 里。
        // The fake world is all plains, which is not in `MINESHAFT_BLOCKING`.
        biome_blocking: false,
    };

    match piece.id.as_str() {
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
                // `hasPlacedSpider` 是结构件自身的字段，跨区块的多次调用会保留。
                // `hasPlacedSpider` lives on the piece and persists across invocations.
                has_placed_spider: piece_flags[index],
            };
            place::corridor_post_process(&mut sp, random, &mut state);
            piece_flags[index] = state.has_placed_spider;
        }
        "minecraft:mscrossing" => {
            let two_floored = piece.extras.get("tf").is_some_and(|v| v.token() == "B:1");
            place::crossing_post_process(&mut sp, two_floored);
        }
        "minecraft:msstairs" => place::stairs_post_process(&mut sp),
        "minecraft:msroom" => place::room_post_process(&mut sp, &piece.entrances),
        other => return Err(format!("no Rust postProcess for {other}")),
    }
    Ok(())
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
    piece_flags: &mut [bool],
) -> Result<(), String> {
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
            // `hasPlacedChest` 同样是结构件字段，跨 3×3 调用保留。
            // `hasPlacedChest` is also a piece field that survives the 3x3 invocations.
            let mut chest = piece_flags[index];
            sp::chest_corridor_post_process(&mut stronghold, random, door, &mut chest);
            piece_flags[index] = chest;
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
            // `hasPlacedSpawner` 同上。
            // `hasPlacedSpawner` likewise.
            let mut spawner = piece_flags[index];
            sp::portal_room_post_process(&mut stronghold, random, &mut spawner);
            piece_flags[index] = spawner;
        }
        other => return Err(format!("no stronghold postProcess for {other} ({index})")),
    }
    Ok(())
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

/// 假世界：初始方块由 [`FlatLevel::initial_block`] 算出，写入记在稀疏表里。
///
/// 语义照 Java 侧的假 `WorldGenRegion`：写只落在正在生成的区块 ±1 内，读不受限制；
/// 构建高度之外读到空气、写了不生效。
///
/// Fake world: initial blocks come from [`FlatLevel::initial_block`], writes go into a
/// sparse map. Semantics follow the Java side's fake `WorldGenRegion`: writes land only
/// within one chunk of the chunk being generated, reads are unrestricted, and outside the
/// build height reads yield air and writes have no effect.
struct FlatLevel {
    blocks: HashMap<(i32, i32, i32), u32>,
    /// 所有写过的位置。
    ///
    /// Every written position.
    written: BTreeSet<(i32, i32, i32)>,
    fill: &'static str,
    /// 正在生成的区块：`WorldGenRegion` 的中心，写区是它的 ±1。
    ///
    /// The chunk being generated: the `WorldGenRegion` centre, whose write zone is that
    /// chunk plus or minus one.
    center: (i32, i32),
    /// 当前这次调用的写入，按顺序：`x y z state`。
    ///
    /// The current invocation's writes in order, as `x y z state`.
    piece_writes: Vec<String>,
    /// 每列 `OCEAN_FLOOR_WG` 高度的缓存（只看初始地形，见 `ocean_floor_height`）。
    ///
    /// Cached `OCEAN_FLOOR_WG` height per column (initial terrain only, see
    /// `ocean_floor_height`).
    heights: RefCell<HashMap<(i32, i32), i32>>,
    /// `WorldGenRegion.getRandom()` 的替身：和 Java 侧一样，每条查询一个 `RandomSource.create(0)`。
    ///
    /// Stand-in for `WorldGenRegion.getRandom()`: like Java, one `RandomSource.create(0)`
    /// per query.
    level_random: mc_bamboo_worldgen::random::WorldgenRandom,
}

/// 初始填充的最高一层（`stone` / `caves`）。
///
/// Topmost layer of the initial fill (`stone` / `caves`).
const FILL_TOP: i32 = 100;

impl FlatLevel {
    fn new(fill: &'static str, center: (i32, i32)) -> Self {
        Self {
            blocks: HashMap::new(),
            written: BTreeSet::new(),
            fill,
            center,
            piece_writes: Vec::new(),
            heights: RefCell::new(HashMap::new()),
            level_random: pieces::legacy_random(0),
        }
    }

    /// `WorldGenRegion.isWithinWriteZone`：写半径 1。
    ///
    /// `WorldGenRegion.isWithinWriteZone`: write radius 1.
    fn in_write_zone(&self, x: i32, z: i32) -> bool {
        let dx = (x >> 4) - self.center.0;
        let dz = (z >> 4) - self.center.1;
        (-1..=1).contains(&dx) && (-1..=1).contains(&dz)
    }

    /// 初始世界，和 Java 侧 `Level.initial` 是同一个函数：`air` 全空；`band` 只有
    /// y 56..=63 一层石头；`stone` 是 y <= 100 的实心石头；`caves` 在实心石头里按 4×4×4
    /// 的格子挖空气洞和少量水洞；`ocean` 是海床（按 8×8 的格子在 y 30..=45 之间变化）
    /// 加上到 y 62 的水，部分 4×4 格子的水面是冰。
    ///
    /// The initial world, the same function as Java's `Level.initial`: `air` is empty;
    /// `band` is a stone slab at y 56..=63; `stone` is solid stone up to y 100; `caves`
    /// carves air pockets and a few water pockets out of that stone on a 4x4x4 grid;
    /// `ocean` is a sea floor (varying over y 30..=45 on an 8x8 grid) under water up to
    /// y 62, with ice on the surface of some 4x4 cells.
    fn initial_block(&self, x: i32, y: i32, z: i32) -> u32 {
        let air = mc_bamboo_data::blocks::AIR;
        match self.fill {
            "band" if (56..=63).contains(&y) => block("minecraft:stone"),
            "stone" if y <= FILL_TOP => block("minecraft:stone"),
            "caves" if y <= FILL_TOP => {
                let hash = x.div_euclid(4).wrapping_mul(73_856_093)
                    ^ y.div_euclid(4).wrapping_mul(19_349_663)
                    ^ z.div_euclid(4).wrapping_mul(83_492_791);
                match hash.rem_euclid(61) {
                    0..12 => air,
                    12 => block("minecraft:water"),
                    _ => block("minecraft:stone"),
                }
            }
            "ocean" if y <= 62 => {
                let floor = 30
                    + (x.div_euclid(8).wrapping_mul(73_856_093)
                        ^ z.div_euclid(8).wrapping_mul(83_492_791))
                    .rem_euclid(16);
                let ice = y == 62
                    && (x.div_euclid(4).wrapping_mul(19_349_663)
                        ^ z.div_euclid(4).wrapping_mul(73_856_093))
                    .rem_euclid(5)
                        == 0;
                if y <= floor {
                    block("minecraft:stone")
                } else if ice {
                    block("minecraft:ice")
                } else {
                    block("minecraft:water")
                }
            }
            _ => air,
        }
    }

    /// 每个写过的位置的最终状态，按 x、y、z 排序；空气家族统一记成 `AIR`
    /// （`ProtoChunk.getBlockState` 对全空气的段直接返回 `Blocks.AIR`，读不出 cave_air）。
    ///
    /// Final state of every written position, sorted by x, y, z; the air family is
    /// normalised to `AIR` (`ProtoChunk.getBlockState` returns `Blocks.AIR` for an all-air
    /// section, so cave_air cannot be read back reliably).
    fn snapshot(&self) -> Vec<String> {
        self.written
            .iter()
            .map(|&(x, y, z)| format!("{x} {y} {z} {}", state_name(self.get_block(x, y, z))))
            .collect()
    }
}

impl PieceLevel for FlatLevel {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        if !(MIN_Y..MIN_Y + HEIGHT).contains(&y) {
            return mc_bamboo_data::blocks::AIR;
        }
        self.blocks
            .get(&(x, y, z))
            .copied()
            .unwrap_or_else(|| self.initial_block(x, y, z))
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        if !self.in_write_zone(x, z) {
            return false;
        }
        self.written.insert((x, y, z));
        // 逐次写入记原状态，cave_air 和 air 不合并。
        // The ordered trace keeps the exact state; cave_air and air stay distinct here.
        self.piece_writes
            .push(format!("{x} {y} {z} {}", exact_state_name(state)));
        if (MIN_Y..MIN_Y + HEIGHT).contains(&y) {
            self.blocks.insert((x, y, z), state);
        }
        true
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MIN_Y + HEIGHT - 1
    }

    /// `OCEAN_FLOOR_WG`：**初始地形**里整列自顶向下第一个 `blocksMotion()` 方块之上一格。
    /// 官方在 FEATURES 阶段不再更新这张高度图（区块状态 TERRAIN 的 `heightmapsAfter` 是
    /// FINAL 那一组），所以结构件自己写的方块不算。
    ///
    /// `OCEAN_FLOOR_WG`: one above the first `blocksMotion()` block from the top of the
    /// column **in the initial terrain**. Vanilla stops updating this heightmap during
    /// FEATURES (the TERRAIN status' `heightmapsAfter` is the FINAL set), so blocks written
    /// by the pieces themselves do not count.
    fn ocean_floor_height(&self, x: i32, z: i32) -> i32 {
        if let Some(&height) = self.heights.borrow().get(&(x, z)) {
            return height;
        }
        let mut height = MIN_Y;
        let mut y = MIN_Y + HEIGHT - 1;
        while y >= MIN_Y {
            let facts = mc_bamboo_data::state_facts(self.initial_block(x, y, z));
            if facts & mc_bamboo_data::facts::MOTION_BLOCKING != 0 {
                height = y + 1;
                break;
            }
            y -= 1;
        }
        self.heights.borrow_mut().insert((x, z), height);
        height
    }

    fn level_random(&self) -> &mc_bamboo_worldgen::random::WorldgenRandom {
        &self.level_random
    }
}

/// 状态的 `Block{name[props]}` 名字，和 Java `BlockState.toString()` 一致；
/// 空气家族统一记成 `AIR`。
///
/// The `Block{name[props]}` name, matching Java's `BlockState.toString()`; the air family
/// is normalised to `AIR`.
fn state_name(state: u32) -> String {
    if is_air_block(state) {
        return "AIR".to_owned();
    }
    exact_state_name(state)
}

/// 状态的 `Block{name}[props]` 名字，和 Java `BlockState.toString()` 一致，空气家族不合并。
///
/// The `Block{name}[props]` name, matching Java's `BlockState.toString()`, with the air
/// family kept distinct.
fn exact_state_name(state: u32) -> String {
    let Some(id) = block_id_of_state(state) else {
        return format!("<state {state}>");
    };
    let block = &mc_bamboo_data::blocks::BLOCKS[id];
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

/// 一种结构（`kind`）的累计数字。
///
/// Running totals of one structure kind.
#[derive(Default)]
struct KindTotals {
    queries: usize,
    pieces: usize,
    invocations: usize,
    blocks: usize,
    list_mismatches: usize,
    block_mismatches: usize,
    write_mismatches: usize,
    /// 每种结构件有多少次调用真的写了方块（按 `id` 与朝向 `O`）。
    ///
    /// Invocations that actually wrote blocks, per piece `id` and orientation `O`.
    placed: BTreeMap<String, usize>,
    /// 官方那一侧最终方块按方块名的计数：看覆盖到了哪些分支。
    ///
    /// Vanilla's final blocks counted by block name, to show which branches were covered.
    coverage: BTreeMap<String, usize>,
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
    let mut totals: BTreeMap<&str, KindTotals> = BTreeMap::new();
    let mut java_only_by_block: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut rust_only_by_block: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut all_diffs: Vec<String> = Vec::new();
    let mut per_query: Vec<(String, usize, usize)> = Vec::new();

    for (q, section) in queries.iter().zip(sections.iter()) {
        let key = q.key();
        if section.key != key {
            return Err(format!(
                "query key mismatch: oracle {:?} vs {key:?}",
                section.key
            ));
        }
        let ours = rust_result(q)?;
        let total = totals.entry(q.kind).or_default();
        total.queries += 1;
        total.pieces += section.pieces.len();
        total.invocations += section.invocations.len();
        total.blocks += section.blocks.len();
        for line in &section.blocks {
            *total.coverage.entry(block_name(line)).or_insert(0) += 1;
        }
        for invocation in &section.invocations {
            if invocation.count == 0 {
                continue;
            }
            // `L` 行：`id minX minY minZ maxX maxY maxZ O GD extras`。
            // `L` line: `id minX minY minZ maxX maxY maxZ O GD extras`.
            // 下标等于结构件数的是林地府邸的 `afterPlace`。
            // An index equal to the piece count is the woodland mansion's `afterPlace`.
            let label = match section.pieces.get(invocation.piece) {
                Some(line) => {
                    let fields: Vec<&str> = line.split(' ').collect();
                    format!("{}/O{}", fields[0], fields[7])
                }
                None => "afterPlace".to_owned(),
            };
            *total.placed.entry(label).or_insert(0) += 1;
        }

        let mut list_mismatches = 0usize;
        let mut ignored = 0usize;
        report_lines(
            &mut failures,
            &mut list_mismatches,
            &mut ignored,
            &key,
            "L",
            &section.pieces,
            &ours.pieces,
        );
        total.list_mismatches += list_mismatches;

        // 逐次调用比：第一处写入次数不同、或（开了 trace 时）第一处写入内容不同的调用，
        // 就是取数或分支分叉的地方。
        // Compare invocation by invocation: the first one whose write count differs, or
        // (with tracing) whose writes differ, is where the draws or a branch diverge.
        let mut write_mismatches = 0usize;
        if section.invocations.len() != ours.invocations.len() {
            write_mismatches += 1;
            push(
                &mut failures,
                format!(
                    "[{key}] {} invocations in java, {} in rust",
                    section.invocations.len(),
                    ours.invocations.len()
                ),
            );
        }
        for (i, (theirs, mine)) in section
            .invocations
            .iter()
            .zip(ours.invocations.iter())
            .enumerate()
        {
            let label = format!(
                "[{key}] invocation #{i} piece #{} chunk {} {} ({})",
                theirs.piece,
                theirs.chunk.0,
                theirs.chunk.1,
                section
                    .pieces
                    .get(theirs.piece)
                    .map_or("<missing>", String::as_str)
            );
            if (theirs.piece, theirs.chunk) != (mine.piece, mine.chunk) {
                write_mismatches += 1;
                push(
                    &mut failures,
                    format!(
                        "{label}: rust ran piece #{} chunk {} {}",
                        mine.piece, mine.chunk.0, mine.chunk.1
                    ),
                );
                continue;
            }
            let traced = !theirs.trace.is_empty();
            let first_difference = if traced {
                (0..theirs.trace.len().max(mine.trace.len()))
                    .find(|&n| theirs.trace.get(n) != mine.trace.get(n))
            } else {
                None
            };
            if theirs.count == mine.count && first_difference.is_none() {
                continue;
            }
            write_mismatches += 1;
            push(
                &mut failures,
                format!(
                    "{label}: java wrote {}, rust wrote {}",
                    theirs.count, mine.count
                ),
            );
            if let Some(n) = first_difference {
                let show = |list: &[String], n: usize| {
                    list.get(n).cloned().unwrap_or_else(|| "<end>".to_owned())
                };
                for k in n.saturating_sub(2)..n {
                    push(
                        &mut failures,
                        format!("    write {k}: both {}", show(&theirs.trace, k)),
                    );
                }
                for k in n..n + 3 {
                    push(
                        &mut failures,
                        format!(
                            "    write {k}: java {} | rust {}",
                            show(&theirs.trace, k),
                            show(&mine.trace, k)
                        ),
                    );
                }
            }
        }
        total.write_mismatches += write_mismatches;

        // 方块按集合比：只在一侧出现（或两侧状态不同）的行都算差异。
        // Blocks compare as sets: a line present on one side only (or differing in state)
        // counts as a difference.
        let theirs: BTreeSet<&str> = section.blocks.iter().map(String::as_str).collect();
        let mine: BTreeSet<&str> = ours.blocks.iter().map(String::as_str).collect();
        let java_only: Vec<&str> = theirs.difference(&mine).copied().collect();
        let rust_only: Vec<&str> = mine.difference(&theirs).copied().collect();
        let block_mismatches = java_only.len() + rust_only.len();
        total.block_mismatches += block_mismatches;
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
        per_query.push((key, list_mismatches + block_mismatches, write_mismatches));
    }

    if std::env::var("STRUCTURE_PLACE_PER_QUERY").is_ok() {
        println!("-- differences per query (only non-zero): blocks+list / invocations --");
        for (key, diff, writes) in &per_query {
            if *diff != 0 || *writes != 0 {
                println!("  {diff:5} {writes:5} {key}");
            }
        }
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

    let mut mismatch_count = 0usize;
    let mut write_mismatch_count = 0usize;
    for (kind, total) in &totals {
        let mismatches = total.list_mismatches + total.block_mismatches;
        mismatch_count += mismatches;
        write_mismatch_count += total.write_mismatches;
        println!(
            "structure-place {kind}: {} queries, {} pieces, {} invocations, {} written positions compared, {} mismatches ({} piece list, {} blocks), {} invocations with differing writes",
            total.queries,
            total.pieces,
            total.invocations,
            total.blocks,
            mismatches,
            total.list_mismatches,
            total.block_mismatches,
            total.write_mismatches
        );
        let coverage: Vec<String> = total
            .coverage
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect();
        println!("  vanilla blocks: {}", coverage.join(" "));
        let placed: Vec<String> = total
            .placed
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect();
        println!("  invocations that wrote blocks: {}", placed.join(" "));
    }
    if mismatch_count == 0 && write_mismatch_count == 0 {
        println!("STRUCTURE-PLACE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in failures.iter().take(40) {
            println!("STRUCTURE-PLACE FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-PLACE RESULT: {mismatch_count} mismatches, {write_mismatch_count} invocations with differing writes"
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

/// 一条方块行 `x y z Block{minecraft:x}[...]` 里的方块名 `minecraft:x`（空气是 `AIR`）。
///
/// The block name `minecraft:x` of a block line `x y z Block{minecraft:x}[...]` (air is
/// `AIR`).
fn block_name(line: &str) -> String {
    let state = line.splitn(4, ' ').nth(3).unwrap_or("");
    state
        .strip_prefix("Block{")
        .and_then(|s| s.split(['}', '[']).next())
        .unwrap_or(state)
        .to_owned()
}

/// 按方块名（`Block{minecraft:x}[...]` 里的 `minecraft:x`）累加，并记一条样例位置。
///
/// Accumulate per block name (the `minecraft:x` inside `Block{minecraft:x}[...]`) with
/// one sample position each.
fn tally(into: &mut BTreeMap<String, (usize, String)>, lines: &[&str]) {
    for line in lines {
        let name = block_name(line);
        let entry = into.entry(name).or_insert_with(|| (0, (*line).to_owned()));
        entry.0 += 1;
    }
}

/// 往失败列表里加一条按方块名聚合的摘要，附最多三条样例。
///
/// Add one aggregate line per block name to the failure list with up to three samples.
fn samples(failures: &mut Vec<String>, key: &str, side: &str, lines: &[&str]) {
    let mut by_block: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for line in lines {
        by_block.entry(block_name(line)).or_default().push(line);
    }
    for (block, list) in by_block {
        for line in list.iter().take(3) {
            push(failures, format!("[{key}] {side} only: {block} @ {line}"));
        }
    }
}
