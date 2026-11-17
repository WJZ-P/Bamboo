//! `feature-merge`：邻居溢出合并的整块对拍。
//!
//! 一个区块 C 交付时的方块是 9 个中心（C 自己和 8 个邻居）各跑一遍地物、写进 C
//! 的结果合起来。`gen-world` / `structure-e2e` 只比「以 C 为中心跑一遍」的 3×3，
//! 经过不了游戏里把邻居溢出合进 C 的那条路径（`cached_feature_run` + 规范序合并）。
//! 这里两边都走完整路径再比整块：
//!
//! * 官方：`ChunkgenOracle.java --merge-neighbors ORDERS` 造 5×5 个跑到 CARVERS 的
//!   ProtoChunk，以 9 个中心各跑一次官方 `applyBiomeDecoration`（`runs` 必须是 9，
//!   8 个邻居都真的跑过）。两种用法：
//!   - `isolated`：每个中心都在干净的 3×3 上跑（跑完还原），交回每个中心写进 C 的
//!     格子；这里按游戏的规范序规则（同一格取状态 ID 最大的）合成整块。比的是「官方
//!     每个中心的输出」+「游戏的合并路径」，**必须 0 不一致**，另外逐个来源比写集。
//!   - `center-first` / `center-last`：9 个中心按先后写进同一批共享 ProtoChunk（官方
//!     `WorldGenRegion` 的语义，后跑的看得到先跑的写），交回 C 的最终方块。这和游戏
//!     「每个中心看干净视图」的模型本来就不同，不一致只列出来，不算失败。
//! * 游戏：真实 Director 流水线（`World::new` + 计算池 + 强加载票）交付 C，读出方块。
//!
//! 逐方块比较；不一致按「游戏侧谁写了这一格」分类列出。官方线里的 setBlock 次数
//! 不含矿石（`BulkSectionAccess` 不经过 setBlock），只作参考。
//!
//! `feature-merge`: whole-chunk oracle for merging neighbor feature spills. Vanilla
//! runs `applyBiomeDecoration` on all 9 centers around C, either each on a clean
//! view (`isolated`, merged here with the game's canonical rule; must match) or in
//! order over shared ProtoChunks (`center-first` / `center-last`; model differences
//! are listed only). The game delivers C through the real Director pipeline.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::{
    MailBudget, World, WorldConfig,
    world_options::{LevelType, WorldGenConfig},
    worldgen_features::oracle_merge::{MergeBreak, feature_writes_into, set_merge_break},
};
use mc_bamboo_worldgen::feature::state::{official_name, state_from_official_name};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: feature-merge <java> [--structures] [--target SEED X Z]... \
     [--orders center-first,center-last] [--threads N] [--show N] \
     [--break-drop-neighbor DX DZ | --break-first-writer] | --scan SEED X0 Z0 SIDE [--structures]";

/// 默认目标（`--scan` 挑的）：(-2,-2) 收到邻居的流体（湖）和树冠溢出，(0,0)、(2,2)
/// 收到树冠溢出；三块都收到 8 个方向里大部分邻居的溢出。
/// Default targets picked with `--scan`: (-2,-2) receives fluid (lake) and canopy
/// spills, (0,0) and (2,2) canopy spills.
const DEFAULT_TARGETS: &[(i64, i32, i32)] = &[(2026, -2, -2), (2026, 0, 0), (2026, 2, 2)];
/// `--structures` 的默认目标：试炼密室起点和它的两个邻居，加上上面两块。
/// Default `--structures` targets: the trial-chambers start, two neighbors, plus two above.
const DEFAULT_STRUCTURE_TARGETS: &[(i64, i32, i32)] = &[
    (42, 17, 7),
    (42, 16, 6),
    (42, 18, 8),
    (2026, -2, -2),
    (2026, 2, 2),
];

#[derive(Debug)]
struct Options {
    structures: bool,
    targets: Vec<(i64, i32, i32)>,
    orders: Vec<String>,
    threads: usize,
    show: usize,
    break_mode: Option<MergeBreak>,
    scan: Option<(i64, i32, i32, i32)>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            structures: false,
            targets: Vec::new(),
            orders: vec![
                "isolated".to_owned(),
                "center-first".to_owned(),
                "center-last".to_owned(),
            ],
            threads: 2,
            show: 12,
            break_mode: None,
            scan: None,
        };
        let mut index = 0;
        let next = |index: &mut usize| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .map(|value| (*value).to_owned())
                .ok_or_else(|| USAGE.to_owned())
        };
        let number = |text: String| -> Result<i64, String> {
            text.parse::<i64>()
                .map_err(|error| format!("bad number {text}: {error}"))
        };
        while index < args.len() {
            match args[index] {
                "--structures" => options.structures = true,
                "--target" => {
                    let seed = number(next(&mut index)?)?;
                    let x = number(next(&mut index)?)? as i32;
                    let z = number(next(&mut index)?)? as i32;
                    options.targets.push((seed, x, z));
                }
                "--orders" => {
                    options.orders = next(&mut index)?
                        .split(',')
                        .map(|order| order.trim().to_owned())
                        .collect();
                }
                "--threads" => options.threads = number(next(&mut index)?)? as usize,
                "--show" => options.show = number(next(&mut index)?)? as usize,
                "--break-drop-neighbor" => {
                    let dx = number(next(&mut index)?)? as i32;
                    let dz = number(next(&mut index)?)? as i32;
                    if dx.abs() > 1 || dz.abs() > 1 || (dx == 0 && dz == 0) {
                        return Err("--break-drop-neighbor needs a neighbor offset".to_owned());
                    }
                    options.break_mode = Some(MergeBreak::DropNeighbor { dx, dz });
                }
                "--break-first-writer" => options.break_mode = Some(MergeBreak::FirstWriterWins),
                "--scan" => {
                    let seed = number(next(&mut index)?)?;
                    let x = number(next(&mut index)?)? as i32;
                    let z = number(next(&mut index)?)? as i32;
                    let side = number(next(&mut index)?)? as i32;
                    options.scan = Some((seed, x, z, side));
                }
                other => return Err(format!("unknown option {other}\n{USAGE}")),
            }
            index += 1;
        }
        if options.targets.is_empty() {
            options.targets = if options.structures {
                DEFAULT_STRUCTURE_TARGETS.to_vec()
            } else {
                DEFAULT_TARGETS.to_vec()
            };
        }
        Ok(options)
    }
}

fn config(seed: i64, structures: bool) -> WorldGenConfig {
    WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        generate_structures: structures,
        ..WorldGenConfig::default()
    }
}

/// 一格状态属于哪类地物，用来挑目标和给不一致分类。
/// Rough feature category of a state, for target picking and mismatch reports.
fn category(state: u32) -> &'static str {
    let name = mc_bamboo_data::block_of_state(state).map_or("?", |block| block.name);
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    if name.ends_with("_leaves") || name.ends_with("_log") || name.ends_with("_wood") {
        "tree"
    } else if name == "lava" || name == "water" {
        "fluid"
    } else if name.contains("ore") {
        "ore"
    } else {
        "other"
    }
}

pub fn run(command: Command, queries_path: &Path, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    if let Some(scan) = options.scan {
        return scan_targets(scan, options.structures);
    }
    let mut text = String::new();
    for (seed, x, z) in &options.targets {
        text.push_str(&format!("{seed} {x} {z}\n"));
    }
    fs::write(queries_path, text)
        .map_err(|error| format!("cannot write {}: {error}", queries_path.display()))?;

    // 1. 游戏侧：真实 Director 流水线交付目标区块。
    let bamboo = director_blocks(&options)?;

    // 2. 官方：9 个中心都跑过的整块。
    let mut command = command;
    command.args(["--through", "features", "--merge-neighbors"]);
    command.arg(options.orders.join(","));
    command.arg("--chunks-file").arg(queries_path);
    if options.structures {
        command.arg("--structures");
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        options: &options,
        bamboo: &bamboo,
        compared: 0,
        cells: 0,
        mismatches: Vec::new(),
        per_line: Vec::new(),
        spill_writes: [0; 9],
        isolated_differing: 0,
        shared_differing: 0,
    };
    let read = report.read(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("ChunkgenOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

/// `(种子, x, z)` → Director 交付的整块方块。
type GameBlocks = HashMap<(i64, i32, i32), Vec<u32>>;

/// 每个种子起一个 `World`，强加载这个种子的全部目标，跑到全部交付、计算池排空，
/// 读出方块。改坏开关只对这个种子生效，跑完清掉。
/// One `World` per seed: force-load its targets, run until delivered and the pool
/// drains, read the blocks. The break switch is scoped to the seed and cleared.
fn director_blocks(options: &Options) -> Result<GameBlocks, String> {
    let mut by_seed: BTreeMap<i64, Vec<ChunkPos>> = BTreeMap::new();
    for &(seed, x, z) in &options.targets {
        by_seed.entry(seed).or_default().push(ChunkPos::new(x, z));
    }
    let mut out = HashMap::new();
    for (seed, positions) in by_seed {
        set_merge_break(options.break_mode.map(|mode| (seed, mode)));
        let (_inbox, receiver) = mpsc::channel();
        let world_config = WorldConfig {
            view_distance: 3,
            world_gen: config(seed, options.structures),
            chunk_worker_threads: options.threads,
            mail_budget: MailBudget {
                deliveries: 0,
                reclaims: 0,
            },
            ..WorldConfig::default()
        };
        let mut world = World::new(world_config, receiver);
        world.force_chunks_unpaced(&positions);
        let deadline = Instant::now() + Duration::from_secs(1800);
        let mut quiet = 0;
        loop {
            world.tick_unpaced();
            let ready = positions.iter().all(|&pos| world.is_chunk_loaded(pos))
                && world.compute_in_flight() == 0;
            quiet = if ready { quiet + 1 } else { 0 };
            if quiet >= 4 {
                break;
            }
            if Instant::now() >= deadline {
                set_merge_break(None);
                return Err(format!("seed {seed}: Director did not deliver the targets"));
            }
        }
        for pos in positions {
            let blocks = world
                .chunk_blocks_unpaced(pos)
                .ok_or_else(|| format!("seed {seed}: {pos:?} not delivered"))?;
            out.insert((seed, pos.x, pos.z), blocks);
        }
        set_merge_break(None);
    }
    Ok(out)
}

struct Report<'a> {
    options: &'a Options,
    bamboo: &'a GameBlocks,
    compared: usize,
    cells: u64,
    mismatches: Vec<String>,
    per_line: Vec<String>,
    /// 游戏侧各来源中心写进目标的条数之和（下标 (dz+1)*3+(dx+1)）。
    spill_writes: [u64; 9],
    /// `isolated`（官方逐中心输出 + 游戏的合并规则）的不一致格数：必须为 0。
    isolated_differing: usize,
    /// 官方共享 ProtoChunk 先后跑的不一致格数：模型差异，只列出来。
    shared_differing: usize,
}

impl Report<'_> {
    fn read(&mut self, source: BufReader<impl std::io::Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let value: Value =
                serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
            match value.get("type").and_then(Value::as_str) {
                Some("merged") => self.on_merged(&value)?,
                Some("isolated") => self.on_isolated(&value)?,
                Some("header" | "biomes" | "end") => {}
                other => return Err(format!("unexpected oracle line {other:?}")),
            }
        }
        Ok(())
    }

    fn on_merged(&mut self, value: &Value) -> Result<(), String> {
        let get = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("merged line missing {key}"))
        };
        let (seed, x, z) = (get("seed")?, get("x")? as i32, get("z")? as i32);
        let order = value.get("order").and_then(Value::as_str).unwrap_or("?");
        let runs = get("runs")?;
        let writes: Vec<i64> = value
            .get("writes")
            .and_then(Value::as_array)
            .ok_or("merged line missing writes")?
            .iter()
            .map(|count| count.as_i64().unwrap_or(-1))
            .collect();
        // 官方那边 9 个中心都要真的跑过：8 个邻居 + 自己。
        // All 9 centers must really have run on the vanilla side.
        if runs != 9 || writes.len() != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}) {order}: oracle ran {runs} centers, writes {writes:?}"
            ));
        }
        let official = official_blocks(value)?;
        let note = format!("official setBlock writes into target by center {writes:?}");
        self.compare(seed, x, z, order, &official, note)
    }

    /// `isolated` 线：官方 9 个中心各自在干净视图上写进目标的格子，用游戏的规范序
    /// 规则合（同一格取状态 ID 最大的那条，即 `(x,y,z,state)` 排序后最后一条），
    /// 再和 Director 交付的方块比；另外逐个来源比「写了哪些格、写成什么」。
    /// `isolated` line: merge the 9 vanilla per-center write sets with the game's
    /// canonical rule (largest state id per cell), compare with the Director's
    /// blocks, and diff each source's writes against the game's.
    fn on_isolated(&mut self, value: &Value) -> Result<(), String> {
        let get = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("isolated line missing {key}"))
        };
        let (seed, x, z) = (get("seed")?, get("x")? as i32, get("z")? as i32);
        if get("runs")? != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}) isolated: not all 9 centers ran"
            ));
        }
        let mut merged = official_blocks(value)?;
        let palette = value
            .get("writePalette")
            .and_then(Value::as_array)
            .ok_or("isolated line missing writePalette")?
            .iter()
            .map(|name| state_from_official_name(name.as_str().unwrap_or("")))
            .collect::<Result<Vec<u32>, String>>()?;
        let sources = value
            .get("sources")
            .and_then(Value::as_array)
            .ok_or("isolated line missing sources")?;
        if sources.len() != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}) isolated: {} sources",
                sources.len()
            ));
        }
        let game = feature_writes_into(&config(seed, self.options.structures), x, z)?;
        let mut best: HashMap<usize, u32> = HashMap::new();
        let mut source_diffs = Vec::new();
        for (slot, writes) in sources.iter().enumerate() {
            let mut official_writes: BTreeMap<usize, u32> = BTreeMap::new();
            for write in writes.as_array().ok_or("source is not an array")? {
                let pair = write.as_array().ok_or("write is not a pair")?;
                let cell = pair.first().and_then(Value::as_u64).ok_or("bad cell")? as usize;
                let id = pair.get(1).and_then(Value::as_u64).ok_or("bad state")? as usize;
                let state = *palette.get(id).ok_or("write state out of palette")?;
                official_writes.insert(cell, state);
                let entry = best.entry(cell).or_insert(state);
                *entry = (*entry).max(state);
            }
            let mut game_writes: BTreeMap<usize, u32> = BTreeMap::new();
            for &(wx, wy, wz, state) in &game[slot] {
                let cell = ((wy - mc_bamboo_world::MIN_Y) as usize * 16 + (wx - x * 16) as usize)
                    * 16
                    + (wz - z * 16) as usize;
                game_writes.insert(cell, state);
            }
            let missing = official_writes
                .iter()
                .filter(|(cell, state)| game_writes.get(cell) != Some(state))
                .count();
            let extra = game_writes
                .iter()
                .filter(|(cell, _)| !official_writes.contains_key(cell))
                .count();
            if missing + extra > 0 {
                source_diffs.push(format!(
                    "{}: {missing} official writes absent/different in game, {extra} game-only",
                    offset_name(slot)
                ));
            }
        }
        for (cell, state) in best {
            merged[cell] = state;
        }
        let note = if source_diffs.is_empty() {
            "per-source writes identical".to_owned()
        } else {
            format!("per-source differences: {}", source_diffs.join("; "))
        };
        self.compare(seed, x, z, "isolated", &merged, note)
    }

    fn compare(
        &mut self,
        seed: i64,
        x: i32,
        z: i32,
        order: &str,
        official: &[u32],
        note: String,
    ) -> Result<(), String> {
        let ours = self
            .bamboo
            .get(&(seed, x, z))
            .ok_or_else(|| format!("no game blocks for seed {seed} ({x},{z})"))?;
        if ours.len() != official.len() {
            return Err(format!(
                "seed {seed} ({x},{z}): {} game cells vs {}",
                ours.len(),
                official.len()
            ));
        }
        // 游戏侧每格的写来源：(来源中心下标, 状态)。
        let sources = feature_writes_into(&config(seed, self.options.structures), x, z)?;
        let mut writers: HashMap<(i32, i32, i32), Vec<(usize, u32)>> = HashMap::new();
        let mut game_writes = [0u64; 9];
        for (slot, deltas) in sources.iter().enumerate() {
            game_writes[slot] = deltas.len() as u64;
            for &(wx, wy, wz, state) in deltas {
                writers.entry((wx, wy, wz)).or_default().push((slot, state));
            }
        }
        if order == self.options.orders[0] {
            for (total, count) in self.spill_writes.iter_mut().zip(game_writes) {
                *total += count;
            }
        }
        let mut differing = 0usize;
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for (cell, (&official, &game)) in official.iter().zip(ours).enumerate() {
            self.cells += 1;
            if official == game {
                continue;
            }
            differing += 1;
            let y = mc_bamboo_world::MIN_Y + (cell / 256) as i32;
            let lx = ((cell / 16) % 16) as i32;
            let lz = (cell % 16) as i32;
            let (wx, wz) = (x * 16 + lx, z * 16 + lz);
            let who = writers.get(&(wx, y, wz)).cloned().unwrap_or_default();
            let class = match who.len() {
                0 => "no game writer".to_owned(),
                1 => format!("one game writer {}", offset_name(who[0].0)),
                _ => {
                    let states: std::collections::BTreeSet<u32> =
                        who.iter().map(|(_, state)| *state).collect();
                    if states.len() > 1 {
                        "same-cell conflict".to_owned()
                    } else {
                        "several game writers, same state".to_owned()
                    }
                }
            };
            let kind = format!(
                "{class}; official {} vs game {}",
                category(official),
                category(game)
            );
            *kinds.entry(kind).or_default() += 1;
            if self.mismatches.len() < self.options.show {
                let writers_text: Vec<String> = who
                    .iter()
                    .map(|(slot, state)| {
                        format!("{}={}", offset_name(*slot), official_name(*state))
                    })
                    .collect();
                self.mismatches.push(format!(
                    "seed {seed} ({x},{z}) {order} at ({wx},{y},{wz}): official {}, game {}; \
                     game writers [{}]",
                    official_name(official),
                    official_name(game),
                    writers_text.join(", ")
                ));
            }
        }
        self.compared += 1;
        if order == "isolated" {
            self.isolated_differing += differing;
        } else {
            self.shared_differing += differing;
        }
        let kinds_text: Vec<String> = kinds
            .iter()
            .map(|(kind, count)| format!("{count} × {kind}"))
            .collect();
        self.per_line.push(format!(
            "seed {seed} ({x},{z}) {order}: {differing} differing cells; {note}; \
             game writes into target by source {game_writes:?}{}{}",
            if kinds_text.is_empty() { "" } else { "\n    " },
            kinds_text.join("\n    ")
        ));
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        for line in &self.per_line {
            println!("{line}");
        }
        for line in &self.mismatches {
            println!("  {line}");
        }
        let expected = self.options.targets.len() * self.options.orders.len();
        if self.compared != expected {
            return Err(format!(
                "oracle returned {} merged lines, expected {expected}",
                self.compared
            ));
        }
        // 游戏侧 8 个邻居都要有溢出写进过目标，不然这批目标没覆盖到合并。
        // Every neighbor must have spilled into some target on the game side.
        for (slot, count) in self.spill_writes.iter().enumerate() {
            if slot != 4 && *count == 0 {
                return Err(format!(
                    "no target received a spill from neighbor {}; pick other targets",
                    offset_name(slot)
                ));
            }
        }
        println!(
            "feature-merge: {} target×order lines, {} cells; isolated (canonical merge of \
             vanilla per-center writes) {} differing; shared ProtoChunk orders {} differing; \
             game spill writes by source {:?}",
            self.compared,
            self.cells,
            self.isolated_differing,
            self.shared_differing,
            self.spill_writes
        );
        if self.isolated_differing > 0 {
            return Err(format!(
                "feature-merge: {} differing cells against the isolated merge",
                self.isolated_differing
            ));
        }
        Ok(())
    }
}

/// 来源中心下标 → `(dx,dz)` 文字。
fn offset_name(slot: usize) -> String {
    let dz = slot as i32 / 3 - 1;
    let dx = slot as i32 % 3 - 1;
    if dx == 0 && dz == 0 {
        "self".to_owned()
    } else {
        format!("({dx:+},{dz:+})")
    }
}

/// `--scan`：只跑游戏侧纯函数，列出一片区块里每个区块收到的邻居溢出（条数、
/// 有没有树 / 流体），用来挑对拍目标。
/// `--scan`: game-side only; list each chunk's incoming neighbor spills (count,
/// trees / fluids) to pick oracle targets.
fn scan_targets(
    (seed, x0, z0, side): (i64, i32, i32, i32),
    structures: bool,
) -> Result<(), String> {
    let config = config(seed, structures);
    for z in z0..z0 + side {
        for x in x0..x0 + side {
            let sources = feature_writes_into(&config, x, z)?;
            let mut neighbors = 0usize;
            let mut senders = 0usize;
            let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
            let mut cells: HashMap<(i32, i32, i32), std::collections::BTreeSet<u32>> =
                HashMap::new();
            for (slot, deltas) in sources.iter().enumerate() {
                for &(wx, wy, wz, state) in deltas {
                    cells.entry((wx, wy, wz)).or_default().insert(state);
                }
                if slot == 4 {
                    continue;
                }
                if !deltas.is_empty() {
                    senders += 1;
                }
                neighbors += deltas.len();
                for &(_, _, _, state) in deltas {
                    *kinds.entry(category(state)).or_default() += 1;
                }
            }
            let conflicts = cells.values().filter(|states| states.len() > 1).count();
            println!(
                "seed {seed} ({x},{z}): own {}, neighbor spills {neighbors} from {senders} \
                 neighbors {kinds:?}, same-cell conflicts {conflicts}",
                sources[4].len()
            );
        }
    }
    Ok(())
}

/// 一条官方线里的整块方块（`palette` + `blocks`，y、x、z 顺序）。
fn official_blocks(value: &Value) -> Result<Vec<u32>, String> {
    let palette = value
        .get("palette")
        .and_then(Value::as_array)
        .ok_or("oracle line missing palette")?
        .iter()
        .map(|name| state_from_official_name(name.as_str().unwrap_or("")))
        .collect::<Result<Vec<u32>, String>>()?;
    value
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or("oracle line missing blocks")?
        .iter()
        .map(|index| {
            let index = index.as_u64().ok_or("block index is not a number")? as usize;
            palette
                .get(index)
                .copied()
                .ok_or_else(|| "block index out of palette".to_owned())
        })
        .collect()
}
