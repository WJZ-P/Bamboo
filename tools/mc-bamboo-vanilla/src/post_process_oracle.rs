//! 完工后处理对拍：Director 交付的 3×3 区块（`FULL`、还没做后处理）原样交给官方
//! `BlockOracle.java --post-process`，在假世界里对中心区块调真实的
//! `LevelChunk.postProcessGeneration`；游戏侧把同一个中心的加载等级抬进
//! `BLOCK_TICKING`，让岛线程照常做后处理。两边比改了的格子（整个 3×3、全高度）和
//! 处理排上的计划刻（坐标、类型、ID、延迟、优先级），另外检查处理前不可发送、处理后
//! 交付盒子和写盘读回的表都已清空。
//!
//! 游戏侧只开加载票（`PlayerLoading`），模拟等级不到，计划刻不会在比较之前执行。
//!
//! Post-processing oracle: the Director's delivered 3×3 (`FULL`, not yet
//! processed) goes as-is to vanilla `BlockOracle.java --post-process`, which calls
//! the real `LevelChunk.postProcessGeneration` on the center in a fake level; the
//! game raises the same center into `BLOCK_TICKING` so its island processes it.
//! Changed cells (whole 3×3, full height) and scheduled ticks are compared, and
//! the chunk must not be sendable before processing nor keep its table after.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::{
    MailBudget, World, WorldConfig,
    generation_post_process::oracle_break::{PostProcessBreak, set_post_process_break},
    world_options::{LevelType, WorldGenConfig},
};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: post-process <java> [--target SEED X Z STRUCTURES(0|1)]... \
     [--break-skip-fluid | --break-keep-table | --break-send-early] [--show N]";

/// 缺省目标：`(种子, x, z, 是否生成结构)`。沼泽小屋 (42,-17,22)、会记 `SHAPE_CHECK_BLOCKS`
/// 的 (42,-155,96)、诊断里见过发光地衣 / 矮草的试炼密室 (42,17,7)、(42,16,6)，再加
/// 平原村庄、要塞、沙漠神殿；无结构的三块都有含水层边缘的 doFill 标记。
/// Default targets `(seed, x, z, structures)`.
const DEFAULT_TARGETS: &[(i64, i32, i32, bool)] = &[
    (42, -17, 22, true),
    (42, -155, 96, true),
    (42, 17, 7, true),
    (42, 16, 6, true),
    (42, -383, -289, true),
    (42, -121, 96, true),
    (42, -375, -22, true),
    (2026, -2, -2, false),
    (2026, 0, 0, false),
    (2026, 2, 2, false),
];

/// 场景在标记高度上下各留的余量：流体找坡最远读 4 格，气泡柱往上找水面。
/// Scene margins around the marked heights: fluids read up to 4 cells sideways,
/// bubble columns walk up to the water surface.
const BELOW_MARGIN: i32 = 16;
const ABOVE_MARGIN: i32 = 48;

struct Options {
    targets: Vec<(i64, i32, i32, bool)>,
    break_mode: Option<PostProcessBreak>,
    show: usize,
}

fn parse(options: &[&str]) -> Result<Options, String> {
    let mut out = Options {
        targets: Vec::new(),
        break_mode: None,
        show: 20,
    };
    let mut index = 0;
    let number = |text: Option<&&str>| -> Result<i64, String> {
        text.ok_or_else(|| USAGE.to_owned())?
            .parse::<i64>()
            .map_err(|_| USAGE.to_owned())
    };
    while index < options.len() {
        match options[index] {
            "--target" => {
                let seed = number(options.get(index + 1))?;
                let x = number(options.get(index + 2))? as i32;
                let z = number(options.get(index + 3))? as i32;
                let structures = number(options.get(index + 4))? != 0;
                out.targets.push((seed, x, z, structures));
                index += 5;
            }
            "--break-skip-fluid" => {
                out.break_mode = Some(PostProcessBreak::SkipFluid);
                index += 1;
            }
            "--break-keep-table" => {
                out.break_mode = Some(PostProcessBreak::KeepTable);
                index += 1;
            }
            "--break-send-early" => {
                out.break_mode = Some(PostProcessBreak::SendEarly);
                index += 1;
            }
            "--show" => {
                out.show = number(options.get(index + 1))? as usize;
                index += 2;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    if out.targets.is_empty() {
        out.targets = DEFAULT_TARGETS.to_vec();
    }
    Ok(out)
}

/// 游戏侧一个目标的结果。
/// One target's game-side result.
struct GameCase {
    seed: i64,
    pos: ChunkPos,
    /// 交给官方的题（一行 JSON）。
    query: Value,
    /// 3×3 里改了的格子 `(x, y, z) → 新状态`。
    changes: BTreeMap<(i32, i32, i32), u32>,
    /// 处理新排上的计划刻 `(x, y, z, 类型, ID, 延迟, 优先级)`，排好序。
    ticks: Vec<TickRow>,
    /// 处理前条目数、其中带流体的条数。
    entries: usize,
    fluid_entries: usize,
    /// 只在游戏侧能判的不一致（发送时机、清表）。
    problems: Vec<String>,
}

type TickRow = (i32, i32, i32, &'static str, i64, i64, i32);

pub fn run(java_program: Command, work_dir: &Path, options: &[&str]) -> Result<(), String> {
    let options = parse(options)?;
    let mut by_world: BTreeMap<(i64, bool), Vec<ChunkPos>> = BTreeMap::new();
    for &(seed, x, z, structures) in &options.targets {
        by_world
            .entry((seed, structures))
            .or_default()
            .push(ChunkPos::new(x, z));
    }
    set_post_process_break(options.break_mode);
    let mut cases = Vec::new();
    let result = (|| {
        for ((seed, structures), targets) in &by_world {
            cases.extend(game_cases(*seed, *structures, targets)?);
        }
        Ok::<(), String>(())
    })();
    set_post_process_break(None);
    result?;

    let queries_path = work_dir.join("post_process_queries.jsonl");
    let text: String = cases
        .iter()
        .map(|case| format!("{}\n", case.query))
        .collect();
    fs::write(&queries_path, text)
        .map_err(|error| format!("cannot write {}: {error}", queries_path.display()))?;
    let mut command = java_program;
    command.arg("--post-process").arg(&queries_path);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .ok_or("java has no stdout")?
        .read_to_string(&mut stdout)
        .map_err(|error| format!("cannot read java output: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    if !status.success() {
        let tail = log.recv().unwrap_or_default();
        return Err(format!(
            "BlockOracle.java --post-process failed: {status}\n{}",
            tail.join("\n")
        ));
    }
    let mut answers = Vec::new();
    for line in BufReader::new(stdout.as_bytes()).lines() {
        let line = line.map_err(|error| format!("cannot read java output: {error}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
        if value.get("type").and_then(Value::as_str) == Some("postprocess") {
            answers.push(value);
        }
    }
    if answers.len() != cases.len() {
        return Err(format!(
            "post-process: {} answers for {} queries",
            answers.len(),
            cases.len()
        ));
    }

    let mut mismatches = Vec::new();
    let mut total_changes = 0usize;
    let mut total_ticks = 0usize;
    let mut total_entries = 0usize;
    let mut total_fluid = 0usize;
    for (case, answer) in cases.iter().zip(&answers) {
        let (seed, x, z) = (case.seed, case.pos.x, case.pos.z);
        let official_changes = official_changes(answer)?;
        let official_ticks = official_ticks(answer)?;
        let draws = answer.get("draws").and_then(Value::as_i64).unwrap_or(-1);
        let mut differ = Vec::new();
        for (at, state) in &official_changes {
            if case.changes.get(at) != Some(state) {
                differ.push(format!(
                    "{at:?} vanilla {} game {}",
                    name(*state),
                    case.changes
                        .get(at)
                        .map_or("unchanged".to_owned(), |s| name(*s))
                ));
            }
        }
        for (at, state) in &case.changes {
            if !official_changes.contains_key(at) {
                differ.push(format!("{at:?} vanilla unchanged game {}", name(*state)));
            }
        }
        let tick_diff = official_ticks != case.ticks;
        println!(
            "seed {seed} ({x},{z}): {} entries ({} with fluid), vanilla {} changes / {} ticks, \
             game {} changes / {} ticks, random draws {draws}",
            case.entries,
            case.fluid_entries,
            official_changes.len(),
            official_ticks.len(),
            case.changes.len(),
            case.ticks.len()
        );
        for (at, state) in official_changes.iter().take(options.show) {
            println!("    vanilla change {at:?} -> {}", name(*state));
        }
        total_changes += official_changes.len();
        total_ticks += official_ticks.len();
        total_entries += case.entries;
        total_fluid += case.fluid_entries;
        for line in differ.iter().take(options.show) {
            mismatches.push(format!("seed {seed} ({x},{z}) block {line}"));
        }
        if differ.len() > options.show {
            mismatches.push(format!(
                "seed {seed} ({x},{z}): {} more block differences",
                differ.len() - options.show
            ));
        }
        if tick_diff {
            let missing: Vec<_> = official_ticks
                .iter()
                .filter(|tick| !case.ticks.contains(tick))
                .take(5)
                .collect();
            let extra: Vec<_> = case
                .ticks
                .iter()
                .filter(|tick| !official_ticks.contains(tick))
                .take(5)
                .collect();
            mismatches.push(format!(
                "seed {seed} ({x},{z}) scheduled ticks differ: vanilla {} game {}; \
                 missing in game {missing:?}; extra in game {extra:?}",
                official_ticks.len(),
                case.ticks.len()
            ));
        }
        if draws != 0 {
            mismatches.push(format!(
                "seed {seed} ({x},{z}): vanilla drew the world random {draws} times; \
                 the comparison assumes a random-free pass"
            ));
        }
        mismatches.extend(case.problems.iter().cloned());
    }
    for line in &mismatches {
        println!("  MISMATCH {line}");
    }
    println!(
        "post-process: {} targets, {total_entries} entries ({total_fluid} with fluid), \
         {total_changes} vanilla changes, {total_ticks} vanilla ticks, {} mismatches",
        cases.len(),
        mismatches.len()
    );
    if total_changes == 0 || total_fluid == 0 || total_ticks == 0 {
        return Err(
            "post-process: targets exercised no shape change, fluid entry or tick; pick others"
                .to_owned(),
        );
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("post-process: {} mismatches", mismatches.len()))
    }
}

fn name(state: u32) -> String {
    mc_bamboo_worldgen::feature::state::official_name(state)
}

/// 一个世界里的全部目标：先把所有目标的 3×3 抬到 `FULL`，再一个个抬进
/// `BLOCK_TICKING`，每个处理前后各取一次快照。
/// All targets of one world: raise every target's 3×3 to `FULL`, then promote the
/// targets one at a time into `BLOCK_TICKING`, snapshotting before and after.
fn game_cases(seed: i64, structures: bool, targets: &[ChunkPos]) -> Result<Vec<GameCase>, String> {
    let (_inbox, receiver) = mc_bamboo_game::inbound_channel();
    let world_config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            generate_structures: structures,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: 4,
        mail_budget: MailBudget {
            deliveries: 0,
            reclaims: 0,
        },
        // 对拍：显式齐步、一个岛。
        // Oracle: lockstep and one island, explicitly.
        island_threads: 0,
        islands: false,
        ..WorldConfig::default()
    };
    let mut world = World::new(world_config, receiver);
    let mut area = Vec::new();
    for target in targets {
        for pos in ring(*target) {
            if !area.contains(&pos) {
                area.push(pos);
            }
        }
    }
    world.load_chunks_unpaced(&area, mc_bamboo_world::ticket::FULL_CHUNK_LEVEL);
    settle(&mut world, seed, |world| {
        area.iter().all(|&pos| world.is_chunk_loaded(pos))
    })?;
    let mut out = Vec::new();
    for &target in targets {
        out.push(game_case(&mut world, seed, target)?);
    }
    Ok(out)
}

fn game_case(world: &mut World, seed: i64, target: ChunkPos) -> Result<GameCase, String> {
    let mut problems = Vec::new();
    let chunks = ring(target);
    if world.chunk_ready_to_send_unpaced(target) {
        problems.push(format!(
            "seed {seed} ({},{}): sendable at FULL before post-processing",
            target.x, target.z
        ));
    }
    let before = blocks_of(world, &chunks)?;
    let (table, _) = world
        .chunk_post_processing_unpaced(target)
        .ok_or("target table unavailable")?
        .map_err(|error| format!("target table: {error}"))?;
    let ticks_before = world.scheduled_ticks_unpaced(&chunks).map(|(_, rows)| rows);
    let marks: Vec<(usize, u16)> = table
        .iter()
        .enumerate()
        .flat_map(|(section, entries)| entries.iter().map(move |&packed| (section, packed)))
        .collect();
    let mark_y = |&(section, packed): &(usize, u16)| {
        MIN_Y + section as i32 * 16 + i32::from((packed >> 4) & 15)
    };
    let low = marks.iter().map(mark_y).min().unwrap_or(0);
    let high = marks.iter().map(mark_y).max().unwrap_or(0);
    let min_y = (low - BELOW_MARGIN).max(MIN_Y);
    let max_y = (high + ABOVE_MARGIN).min(MIN_Y + WORLD_HEIGHT - 1);
    let fluid_entries = marks
        .iter()
        .filter(|mark| {
            let y = mark_y(mark);
            let (lx, lz) = (i32::from(mark.1 & 15), i32::from((mark.1 >> 8) & 15));
            let state = cell(&before, target, target.x * 16 + lx, y, target.z * 16 + lz);
            !mc_bamboo_blocks::BlockState(state).fluid().is_empty()
        })
        .count();

    let mut states = Vec::with_capacity(((max_y - min_y + 1) * 48 * 48) as usize);
    for y in min_y..=max_y {
        for dx in 0..48 {
            for dz in 0..48 {
                states.push(cell(
                    &before,
                    target,
                    (target.x - 1) * 16 + dx,
                    y,
                    (target.z - 1) * 16 + dz,
                ));
            }
        }
    }
    let query = json!({
        "x": target.x,
        "z": target.z,
        "minY": min_y,
        "maxY": max_y,
        "states": states,
        "post": table,
    });

    // 抬进 BLOCK_TICKING。处理发生在哪一刻，就以那一刻开始前计划刻队列的时钟算延迟。
    // Promote into BLOCK_TICKING; delays count from the tick queue's clock at the
    // start of the tick that processed the chunk.
    world.load_chunks_unpaced(&[target], mc_bamboo_world::ticket::BLOCK_TICKING_LEVEL);
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut clock;
    loop {
        clock = world
            .scheduled_ticks_unpaced(&chunks)
            .map_or(0, |(now, _)| now);
        world.tick_unpaced();
        if world.chunk_post_processed_unpaced(target) {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "seed {seed} ({},{}): never became ticking-ready",
                target.x, target.z
            ));
        }
    }
    let after = blocks_of(world, &chunks)?;
    let mut changes = BTreeMap::new();
    for (index, chunk) in chunks.iter().enumerate() {
        for (cell_index, (&old, &new)) in before[index].iter().zip(&after[index]).enumerate() {
            if old != new {
                let y = MIN_Y + (cell_index / 256) as i32;
                let lx = ((cell_index / 16) % 16) as i32;
                let lz = (cell_index % 16) as i32;
                changes.insert((chunk.x * 16 + lx, y, chunk.z * 16 + lz), new);
            }
        }
    }
    let old_ticks = ticks_before.unwrap_or_default();
    let mut ticks: Vec<TickRow> = world
        .scheduled_ticks_unpaced(&chunks)
        .map(|(_, rows)| rows)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !old_ticks.contains(row))
        .map(|(pos, fluid, id, trigger, priority)| {
            (
                pos.x,
                pos.y,
                pos.z,
                if fluid { "fluid" } else { "block" },
                id,
                trigger - clock,
                priority,
            )
        })
        .collect();
    ticks.sort();
    let (delivered, reloaded) = world
        .chunk_post_processing_unpaced(target)
        .ok_or("target table unavailable after processing")?
        .map_err(|error| format!("target table after processing: {error}"))?;
    let left = |table: &[Vec<u16>]| table.iter().map(Vec::len).sum::<usize>();
    if left(&delivered) + left(&reloaded) > 0 {
        problems.push(format!(
            "seed {seed} ({},{}): table not cleared after post-processing \
             ({} delivered, {} after a save round trip)",
            target.x,
            target.z,
            left(&delivered),
            left(&reloaded)
        ));
    }
    Ok(GameCase {
        seed,
        pos: target,
        query,
        changes,
        ticks,
        entries: marks.len(),
        fluid_entries,
        problems,
    })
}

/// 中心和 8 个邻居，按 `(dz, dx)` 升序。
/// The center and its 8 neighbors in ascending `(dz, dx)` order.
fn ring(center: ChunkPos) -> Vec<ChunkPos> {
    let mut out = Vec::with_capacity(9);
    for dz in -1..=1 {
        for dx in -1..=1 {
            out.push(ChunkPos::new(center.x + dx, center.z + dz));
        }
    }
    out
}

fn blocks_of(world: &World, chunks: &[ChunkPos]) -> Result<Vec<Vec<u32>>, String> {
    chunks
        .iter()
        .map(|&pos| {
            world
                .chunk_blocks_unpaced(pos)
                .ok_or_else(|| format!("{pos:?} not delivered"))
        })
        .collect()
}

/// 3×3 快照里一格的状态；`blocks` 按 [`ring`] 的顺序。
/// One cell of a 3×3 snapshot laid out in [`ring`] order.
fn cell(blocks: &[Vec<u32>], center: ChunkPos, x: i32, y: i32, z: i32) -> u32 {
    let dx = (x >> 4) - center.x;
    let dz = (z >> 4) - center.z;
    if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
        return 0;
    }
    let index = ((dz + 1) * 3 + dx + 1) as usize;
    let cell = ((y - MIN_Y) as usize * 16 + (x & 15) as usize) * 16 + (z & 15) as usize;
    blocks[index].get(cell).copied().unwrap_or(0)
}

/// 推到计算池排空、条件成立后再多跑几刻。
/// Tick until the pool drains and the condition holds, plus a few extra ticks.
fn settle(world: &mut World, seed: i64, ready: impl Fn(&World) -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut quiet = 0;
    loop {
        world.tick_unpaced();
        quiet = if ready(world) && world.compute_in_flight() == 0 {
            quiet + 1
        } else {
            0
        };
        if quiet >= 4 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("seed {seed}: Director did not deliver the targets"));
        }
    }
}

fn official_changes(answer: &Value) -> Result<BTreeMap<(i32, i32, i32), u32>, String> {
    let mut out = BTreeMap::new();
    for row in answer
        .get("changes")
        .and_then(Value::as_array)
        .ok_or("answer missing changes")?
    {
        let row = row.as_array().ok_or("bad change row")?;
        let at = |index: usize| {
            row.get(index)
                .and_then(Value::as_i64)
                .ok_or_else(|| "bad change row".to_owned())
        };
        out.insert((at(0)? as i32, at(1)? as i32, at(2)? as i32), at(3)? as u32);
    }
    Ok(out)
}

fn official_ticks(answer: &Value) -> Result<Vec<TickRow>, String> {
    let mut out = Vec::new();
    for row in answer
        .get("ticks")
        .and_then(Value::as_array)
        .ok_or("answer missing ticks")?
    {
        let row = row.as_array().ok_or("bad tick row")?;
        let at = |index: usize| {
            row.get(index)
                .and_then(Value::as_i64)
                .ok_or_else(|| "bad tick row".to_owned())
        };
        let kind = match row.get(3).and_then(Value::as_str) {
            Some("fluid") => "fluid",
            Some("block") => "block",
            _ => return Err("bad tick kind".to_owned()),
        };
        out.push((
            at(0)? as i32,
            at(1)? as i32,
            at(2)? as i32,
            kind,
            at(4)?,
            at(5)?,
            at(6)? as i32,
        ));
    }
    out.sort();
    Ok(out)
}
