//! `feature-merge`：邻居溢出合并的整块对拍。
//!
//! 一个区块 C 交付时的方块是 9 个中心（C 自己和 8 个邻居）各跑一遍地物、写进 C
//! 的结果合起来。`gen-world` / `structure-e2e` 只比「以 C 为中心跑一遍」的 3×3，
//! 经过不了游戏里把邻居溢出合进 C 的那条路径（`cached_feature_run` + 固定先后合并）。
//! 这里两边都走完整路径再比整块：
//!
//! * 官方：`ChunkgenOracle.java --merge-neighbors ORDERS` 造 5×5 个跑到 CARVERS 的
//!   ProtoChunk，以 9 个中心各跑一次官方 `applyBiomeDecoration`（`runs` 必须是 9，
//!   8 个邻居都真的跑过）。两种用法：
//!   - `isolated`：每个中心都在干净的 3×3 上跑（跑完还原），交回每个中心写进 C 的
//!     格子；这里调游戏同一个 `merge_feature_writes`（按中心固定先后、同格先写的赢）
//!     合成整块。比的是「官方每个中心的输出」+「游戏的合并路径」，**必须 0 不一致**，
//!     另外逐个来源比写集。
//!   - `center-first` / `center-last`：9 个中心按先后写进同一批共享 ProtoChunk（官方
//!     `WorldGenRegion` 的语义，后跑的看得到先跑的写），交回 C 的最终方块。这和游戏
//!     「每个中心看干净视图」的模型本来就不同，不一致只列出来，不算失败。
//! * 游戏：真实 Director 流水线（`World::new` + 计算池 + 强加载票）交付 C，读出方块。
//!
//! 逐方块比较；不一致按「游戏侧谁写了这一格」分类列出。官方线里的 setBlock 次数
//! 不含矿石（`BulkSectionAccess` 不经过 setBlock），只作参考。
//!
//! `isolated` 线还比完工的 `PostProcessing`：官方交回目标的干净地形标记和每个中心
//! 新记进目标的标记，这里按约定顺序（地形标记在前，然后中心自己、其余 slot 升序）
//! 逐段拼成基准，和 Director 交付盒子里的表、以及它写盘再读回的表逐段逐项比，
//! **必须 0 不一致**。官方跨中心的先后取决于调度，本身不确定；按约定顺序比同时
//! 保证了每段的多重集合相等和每个中心内部的调用顺序。
//!
//! `--post-process-diagnostic` 另外让官方把 9 个中心 center-first 写进共享 ProtoChunk 后，
//! 按目标的 `PostProcessing` 表走一遍 `LevelChunk.postProcessGeneration`（形状更新照
//! 官方算，流体 / `LiquidBlock` 的 tick 只计数），列出完工后会变的格子，并对照
//! Director 交付的方块。Bamboo 还没有这一步，这组差异是已知的，只打印不计入失败。
//!
//! `feature-merge`: whole-chunk oracle for merging neighbor feature spills. Vanilla
//! runs `applyBiomeDecoration` on all 9 centers around C, either each on a clean
//! view (`isolated`, merged here with the game's own `merge_feature_writes`; must
//! match) or in order over shared ProtoChunks (`center-first` / `center-last`;
//! model differences are listed only). The game delivers C through the real
//! Director pipeline.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::{
    MailBudget, World, WorldConfig,
    world_options::{LevelType, WorldGenConfig},
    worldgen::{GeneratedBlockEntity, GeneratedChunk},
    worldgen_features::{
        FeatureWrites, merge_feature_writes,
        oracle_merge::{MergeBreak, feature_marks_into, feature_writes_into, set_merge_break},
    },
};
use mc_bamboo_worldgen::feature::state::{official_name, state_from_official_name};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: feature-merge <java> [--structures] [--target SEED X Z]... \
     [--orders isolated,center-first,center-last] [--threads N] [--show N] \
     [--break-drop-neighbor DX DZ | --break-drop-neighbor-marks DX DZ | --break-largest-state | --break-drop-block-entities] \
     [--post-process-diagnostic] | --scan SEED X0 Z0 SIDE [--structures]";

/// 默认目标（`--scan` 挑的）：(-2,-2) 收到邻居的流体（湖）和树冠溢出，(0,0)、(2,2)
/// 收到树冠溢出；三块都收到 8 个方向里大部分邻居的溢出。
/// Default targets picked with `--scan`: (-2,-2) receives fluid (lake) and canopy
/// spills, (0,0) and (2,2) canopy spills.
const DEFAULT_TARGETS: &[(i64, i32, i32)] = &[(2026, -2, -2), (2026, 0, 0), (2026, 2, 2)];
/// `--structures` 的默认目标：试炼密室起点和它的两个邻居，加上上面两块；再加两块
/// 结构件 `placeBlock` 会记 `SHAPE_CHECK_BLOCKS`（栅栏 / 火把）的：沼泽小屋
/// `(-17,22)` 和 `(-155,96)`。
/// Default `--structures` targets: the trial-chambers start, two neighbors, two
/// from above, plus two whose `placeBlock` pieces mark `SHAPE_CHECK_BLOCKS`
/// (fences / torches): the swamp hut `(-17,22)` and `(-155,96)`.
const DEFAULT_STRUCTURE_TARGETS: &[(i64, i32, i32)] = &[
    (42, 17, 7),
    (42, 16, 6),
    (42, 18, 8),
    (2026, -2, -2),
    (2026, 2, 2),
    (42, -17, 22),
    (42, -155, 96),
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
    /// 打印官方完工后处理（`postProcessGeneration`）会改的格子；只是诊断。
    /// Print the cells vanilla's `postProcessGeneration` would change; diagnostic only.
    post_process_diagnostic: bool,
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
            post_process_diagnostic: false,
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
                "--break-drop-neighbor-marks" => {
                    let dx = number(next(&mut index)?)? as i32;
                    let dz = number(next(&mut index)?)? as i32;
                    if dx.abs() > 1 || dz.abs() > 1 || (dx == 0 && dz == 0) {
                        return Err(
                            "--break-drop-neighbor-marks needs a neighbor offset".to_owned()
                        );
                    }
                    options.break_mode = Some(MergeBreak::DropNeighborMarks { dx, dz });
                }
                "--break-largest-state" => options.break_mode = Some(MergeBreak::LargestStateWins),
                "--break-drop-block-entities" => {
                    options.break_mode = Some(MergeBreak::DropBlockEntities);
                }
                "--post-process-diagnostic" => options.post_process_diagnostic = true,
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
    let (bamboo, bamboo_post, bamboo_entities) = director_blocks(&options)?;

    // 2. 官方：9 个中心都跑过的整块。
    let mut command = command;
    command.args(["--through", "features", "--merge-neighbors"]);
    let mut java_orders = options.orders.clone();
    if options.post_process_diagnostic {
        java_orders.push("post-process".to_owned());
    }
    command.arg(java_orders.join(","));
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
        bamboo_post: &bamboo_post,
        bamboo_entities: &bamboo_entities,
        entity_checks: 0,
        entity_mismatches: Vec::new(),
        post_entries: 0,
        post_mismatches: Vec::new(),
        post_lines: 0,
        post_marks_by_source: [0; 9],
        post_process_lines: Vec::new(),
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
/// `(种子, x, z)` → Director 交付盒子里的 `PostProcessing`，以及写盘再读回的同一张表。
/// `(seed, x, z)` → the delivered box's `PostProcessing` and the same table after
/// a save encode / decode round trip.
type GamePost = HashMap<(i64, i32, i32), (Vec<Vec<u16>>, Vec<Vec<u16>>)>;
/// `(种子, x, z)` → Director 交付的区块现在写盘会写出的 `block_entities` 条目。
/// `(seed, x, z)` → the `block_entities` entries a save of the delivered chunk writes.
type GameEntities = HashMap<(i64, i32, i32), Vec<mc_bamboo_nbt::Tag>>;

/// 每个种子起一个 `World`，强加载这个种子的全部目标，跑到全部交付、计算池排空，
/// 读出方块。改坏开关只对这个种子生效，跑完清掉。
/// One `World` per seed: force-load its targets, run until delivered and the pool
/// drains, read the blocks. The break switch is scoped to the seed and cleared.
fn director_blocks(options: &Options) -> Result<(GameBlocks, GamePost, GameEntities), String> {
    let mut by_seed: BTreeMap<i64, Vec<ChunkPos>> = BTreeMap::new();
    for &(seed, x, z) in &options.targets {
        by_seed.entry(seed).or_default().push(ChunkPos::new(x, z));
    }
    let mut out = HashMap::new();
    let mut post = HashMap::new();
    let mut entities = HashMap::new();
    for (seed, positions) in by_seed {
        set_merge_break(options.break_mode.map(|mode| (seed, mode)));
        let (_inbox, receiver) = mc_bamboo_game::inbound_channel();
        let world_config = WorldConfig {
            view_distance: 3,
            world_gen: config(seed, options.structures),
            chunk_worker_threads: options.threads,
            mail_budget: MailBudget {
                deliveries: 0,
                reclaims: 0,
            },
            // 对拍：显式选齐步驱动和一个岛（`island_threads = 0`、`islands = false`），
            // 不靠缺省值（岛的实施计划第 9 节第 7 条）。
            // Oracle: choose the lockstep drive and one island explicitly, not by default.
            island_threads: 0,
            islands: false,
            ..WorldConfig::default()
        };
        let mut world = World::new(world_config, receiver);
        world.load_chunks_unpaced(&positions, mc_bamboo_world::ticket::FULL_CHUNK_LEVEL);
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
            let tables = world
                .chunk_post_processing_unpaced(pos)
                .ok_or_else(|| format!("seed {seed}: {pos:?} PostProcessing unavailable"))?
                .map_err(|error| format!("seed {seed}: {pos:?} save round trip: {error}"))?;
            post.insert((seed, pos.x, pos.z), tables);
            let nbt = world
                .chunk_nbt_unpaced(pos)
                .ok_or_else(|| format!("seed {seed}: {pos:?} chunk NBT unavailable"))?
                .map_err(|error| format!("seed {seed}: {pos:?} chunk NBT: {error}"))?;
            let list = match mc_bamboo_storage::chunk_nbt::field(&nbt, "block_entities") {
                Some(mc_bamboo_nbt::Tag::List(list)) => list.clone(),
                _ => Vec::new(),
            };
            entities.insert((seed, pos.x, pos.z), list);
        }
        set_merge_break(None);
    }
    Ok((out, post, entities))
}

/// 官方一个来源中心的方块实体行 `[x,y,z,SNBT|"DUMMY"]` 转成合并用的条目；DUMMY 用
/// 只有 `id: "DUMMY"` 的标签表示。
/// One source's vanilla block entity rows `[x,y,z,SNBT|"DUMMY"]` as merge entries;
/// DUMMY becomes a tag holding only `id: "DUMMY"`.
fn official_block_entities(rows: &Value) -> Result<Vec<GeneratedBlockEntity>, String> {
    let mut out = Vec::new();
    for row in rows
        .as_array()
        .ok_or("block entity source is not an array")?
    {
        let row = row.as_array().ok_or("bad block entity row")?;
        let at = |index: usize| -> Result<i32, String> {
            row.get(index)
                .and_then(Value::as_i64)
                .map(|value| value as i32)
                .ok_or_else(|| "bad block entity coordinate".to_owned())
        };
        let pos = (at(0)?, at(1)?, at(2)?);
        let text = row
            .get(3)
            .and_then(Value::as_str)
            .ok_or("bad block entity snbt")?;
        let tag = if text == "DUMMY" {
            mc_bamboo_nbt::Tag::Compound(vec![(
                "id".to_owned(),
                mc_bamboo_nbt::Tag::String("DUMMY".to_owned()),
            )])
        } else {
            mc_bamboo_nbt::parse_snbt(text)
                .map_err(|error| format!("bad official block entity snbt {text}: {error}"))?
        };
        out.push((pos, tag));
    }
    Ok(out)
}

fn is_dummy(tag: &mc_bamboo_nbt::Tag) -> bool {
    matches!(
        mc_bamboo_storage::chunk_nbt::field(tag, "id"),
        Some(mc_bamboo_nbt::Tag::String(id)) if id == "DUMMY"
    )
}

/// 复合标签的全部键值（值写成 SNBT），按键排序，比较时不看键的先后。
/// All entries of a compound (values as SNBT), sorted by key so order is ignored.
fn sorted_entries(tag: &mc_bamboo_nbt::Tag) -> Vec<(String, String)> {
    let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
        return vec![("#tag".to_owned(), tag.to_snbt())];
    };
    let mut out: Vec<(String, String)> = entries
        .iter()
        .map(|(key, value)| (key.clone(), value.to_snbt()))
        .collect();
    out.sort();
    out
}

struct Report<'a> {
    options: &'a Options,
    bamboo: &'a GameBlocks,
    bamboo_post: &'a GamePost,
    bamboo_entities: &'a GameEntities,
    /// 比过的方块实体条数。
    entity_checks: u64,
    /// 方块实体不一致：必须为空。
    entity_mismatches: Vec<String>,
    /// 比过的官方 `PostProcessing` 条目数（交付和读回各算一次）。
    post_entries: u64,
    /// `PostProcessing` 不一致：必须为空。
    post_mismatches: Vec<String>,
    /// 比过 `PostProcessing` 的 isolated 线数。
    post_lines: usize,
    /// 官方各来源中心记进目标的新标记条数之和（下标 (dz+1)*3+(dx+1)），
    /// 用来确认邻居记进来的标记真的被覆盖到。
    post_marks_by_source: [u64; 9],
    /// `--post-process-diagnostic` 的逐目标报告（不计入失败）。
    post_process_lines: Vec<String>,
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
                Some("postprocess") => self.on_post_process(&value)?,
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

    /// `isolated` 线：官方 9 个中心各自在干净视图上写进目标的格子，调游戏同一个
    /// `merge_feature_writes`（按中心固定先后、同格先写的赢）合成整块，再和 Director
    /// 交付的方块比；另外逐个来源比「写了哪些格、写成什么」。
    /// `isolated` line: merge the 9 vanilla per-center write sets with the game's
    /// own `merge_feature_writes` (fixed center order, first writer wins), compare
    /// with the Director's blocks, and diff each source's writes against the game's.
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
        let baseline = official_blocks(value)?;
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
        let entity_sources = value
            .get("blockEntities")
            .and_then(Value::as_array)
            .ok_or("isolated line missing blockEntities")?;
        if entity_sources.len() != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}) isolated: {} block entity sources",
                entity_sources.len()
            ));
        }
        let mut groups = Vec::with_capacity(sources.len());
        let mut source_diffs = Vec::new();
        for (slot, writes) in sources.iter().enumerate() {
            let mut official_writes: BTreeMap<usize, u32> = BTreeMap::new();
            let mut deltas = Vec::new();
            for write in writes.as_array().ok_or("source is not an array")? {
                let pair = write.as_array().ok_or("write is not a pair")?;
                let cell = pair.first().and_then(Value::as_u64).ok_or("bad cell")? as usize;
                let id = pair.get(1).and_then(Value::as_u64).ok_or("bad state")? as usize;
                let state = *palette.get(id).ok_or("write state out of palette")?;
                official_writes.insert(cell, state);
                // 官方的 cell 下标是 (y*16+x)*16+z；转回世界坐标，交给游戏的
                // 合并函数（它按来源下标排先后）。
                // The oracle's cell index is (y*16+x)*16+z; turn it back into
                // world coordinates and let the game's merge order the sources.
                let y = mc_bamboo_world::MIN_Y + (cell / 256) as i32;
                let lx = ((cell / 16) % 16) as i32;
                let lz = (cell % 16) as i32;
                deltas.push((x * 16 + lx, y, z * 16 + lz, state));
            }
            groups.push(FeatureWrites {
                slot: slot as u8,
                deltas,
                marks: Vec::new(),
                block_entities: official_block_entities(&entity_sources[slot])?,
            });
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
        // 官方整块线给的是干净 CARVERS 方块（9 个中心跑完还原后的目标区块），
        // 逐格用它当地形，再调游戏同一份合并函数写进 9 个中心的写。
        // `merge_feature_writes` 只读写 `blocks`，群系 / 高度图留空即可。
        // The oracle's `blocks` array is the restored clean CARVERS target; use
        // it as the terrain and apply the 9 centers' writes with the game's own
        // merge function. Only `blocks` is read/written, so biomes and
        // heightmaps can stay empty.
        let mut merged = GeneratedChunk {
            blocks: baseline
                .chunks(256)
                .map(<[u32]>::to_vec)
                .collect::<Vec<Vec<u32>>>(),
            biomes: Vec::new(),
            heightmaps: Default::default(),
            post_processing: Vec::new(),
            block_entities: Vec::new(),
        };
        merge_feature_writes(&mut merged, groups);
        self.compare_block_entities(seed, x, z, &merged.block_entities)?;
        let merged = merged.blocks.into_iter().flatten().collect::<Vec<u32>>();
        self.compare_post_processing(seed, x, z, value)?;
        let note = if source_diffs.is_empty() {
            "per-source writes identical".to_owned()
        } else {
            format!("per-source differences: {}", source_diffs.join("; "))
        };
        self.compare(seed, x, z, "isolated", &merged, note)
    }

    /// 官方完工后处理诊断线：官方会改的格子，对照 Director 交付的方块（Bamboo 没有
    /// 这一步，交付的仍是处理前的状态）。只记报告，不计入失败。
    /// Vanilla post-processing diagnostic line: cells vanilla would change, checked
    /// against the Director's delivered blocks (Bamboo still delivers the
    /// pre-processing state). Reported only, never a failure.
    fn on_post_process(&mut self, value: &Value) -> Result<(), String> {
        let get = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("postprocess line missing {key}"))
        };
        let (seed, x, z) = (get("seed")?, get("x")? as i32, get("z")? as i32);
        if get("runs")? != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}) postprocess: not all 9 centers ran"
            ));
        }
        let ours = self
            .bamboo
            .get(&(seed, x, z))
            .ok_or_else(|| format!("no game blocks for seed {seed} ({x},{z})"))?;
        let cells = value
            .get("cells")
            .and_then(Value::as_array)
            .ok_or("postprocess line missing cells")?;
        let mut differ = 0usize;
        let mut samples = Vec::new();
        for cell in cells {
            let cell = cell.as_array().ok_or("postprocess cell is not an array")?;
            let coord = |index: usize| {
                cell.get(index)
                    .and_then(Value::as_i64)
                    .map(|v| v as i32)
                    .ok_or("bad postprocess cell")
            };
            let (wx, wy, wz) = (coord(0)?, coord(1)?, coord(2)?);
            let after = cell.get(4).and_then(Value::as_str).unwrap_or("");
            let index = ((wy - mc_bamboo_world::MIN_Y) as usize * 16 + (wx - x * 16) as usize) * 16
                + (wz - z * 16) as usize;
            let game = ours
                .get(index)
                .copied()
                .map(official_name)
                .unwrap_or_default();
            if game != after {
                differ += 1;
                if samples.len() < 3 {
                    samples.push(format!("({wx},{wy},{wz}) vanilla {after}, game {game}"));
                }
            }
        }
        self.post_process_lines.push(format!(
            "seed {seed} ({x},{z}) postProcessGeneration: {} entries, {} fluid, {} liquid-block \
             (ticks not simulated), {} shape updates {}; {differ} of them differ from the \
             delivered blocks{}{}",
            get("entries")?,
            get("fluid")?,
            get("liquid")?,
            get("changed")?,
            value
                .get("byBlock")
                .map(Value::to_string)
                .unwrap_or_default(),
            if samples.is_empty() { "" } else { ": " },
            samples.join("; ")
        ));
        Ok(())
    }

    /// 交付区块写盘的 `block_entities` 和官方 9 个中心的方块实体（按游戏同一条合并
    /// 规则合好）逐条比：官方有真实数据的位置整条标签逐键相同；官方只有
    /// `WorldGenRegion.setBlock` 的 DUMMY 占位的位置，交付侧要有一条（构造器默认值，
    /// 官方升成 `LevelChunk` 时同样补建）；交付侧多出来的位置报错。
    ///
    /// Compare the delivered chunk's saved `block_entities` with vanilla's nine
    /// centers' block entities merged by the game's own rule: real entries must
    /// match key by key; DUMMY placeholders need some delivered entry (vanilla
    /// creates the default one at `LevelChunk` promotion); extra delivered
    /// positions are mismatches.
    fn compare_block_entities(
        &mut self,
        seed: i64,
        x: i32,
        z: i32,
        official: &[GeneratedBlockEntity],
    ) -> Result<(), String> {
        let delivered = self
            .bamboo_entities
            .get(&(seed, x, z))
            .ok_or_else(|| format!("no delivered block entities for seed {seed} ({x},{z})"))?;
        let mut ours: BTreeMap<(i32, i32, i32), Vec<(String, String)>> = BTreeMap::new();
        for tag in delivered {
            let int = |key: &str| match mc_bamboo_storage::chunk_nbt::field(tag, key) {
                Some(mc_bamboo_nbt::Tag::Int(value)) => Ok(*value),
                _ => Err(format!("delivered block entity without {key}")),
            };
            ours.insert((int("x")?, int("y")?, int("z")?), sorted_entries(tag));
        }
        for (at, tag) in official {
            self.entity_checks += 1;
            let got = ours.remove(at);
            if is_dummy(tag) {
                if got.is_none() {
                    self.entity_mismatches.push(format!(
                        "seed {seed} ({x},{z}) block entity at {at:?}: vanilla has a pending \
                         block entity, game has none"
                    ));
                }
                continue;
            }
            // 官方 `LevelChunk.getBlockEntityNbtForSaving` 存盘时补 `keepPacked: false`；
            // `ProtoChunk` 的那份没有。交付的是完工区块，按前者比。
            // Vanilla `LevelChunk.getBlockEntityNbtForSaving` adds `keepPacked: false`
            // (the `ProtoChunk` one does not); the delivered chunk is full.
            let mut want = sorted_entries(tag);
            if !want.iter().any(|(key, _)| key == "keepPacked") {
                want.push(("keepPacked".to_owned(), "0b".to_owned()));
                want.sort();
            }
            match got {
                Some(got) if got == want => {}
                Some(got) => self.entity_mismatches.push(format!(
                    "seed {seed} ({x},{z}) block entity at {at:?}: vanilla {want:?}, game {got:?}"
                )),
                None => self.entity_mismatches.push(format!(
                    "seed {seed} ({x},{z}) block entity at {at:?}: vanilla {want:?}, game missing"
                )),
            }
        }
        for (at, got) in ours {
            self.entity_mismatches.push(format!(
                "seed {seed} ({x},{z}) block entity at {at:?}: vanilla none, game {got:?}"
            ));
        }
        Ok(())
    }

    /// isolated 线的 `PostProcessing`：官方干净地形标记 + 9 个中心各自新记进目标的
    /// 标记，按约定顺序（中心自己先、其余 slot 升序）逐段拼成基准；和 Director
    /// 交付盒子的表、写盘读回的表逐段逐项比。另外逐个来源比标记，便于定位。
    /// The isolated line's `PostProcessing`: vanilla's clean terrain marks plus the
    /// 9 centers' new marks, concatenated per section in the agreed order (center
    /// first, then ascending slot), compared entry by entry with the Director's
    /// delivered table and its save round trip; per-source marks are diffed too.
    fn compare_post_processing(
        &mut self,
        seed: i64,
        x: i32,
        z: i32,
        value: &Value,
    ) -> Result<(), String> {
        let tables = isolated_post_processing(value)
            .map_err(|error| format!("seed {seed} ({x},{z}): {error}"))?;
        let (marks, want) = (tables.marks, tables.full);
        for (total, source) in self.post_marks_by_source.iter_mut().zip(&marks) {
            *total += source.iter().map(Vec::len).sum::<usize>() as u64;
        }
        // 逐来源：游戏的标记（世界坐标）按段打包后与官方同一来源比。
        // Per source: pack the game's world-space marks and compare with vanilla's.
        let game_marks = feature_marks_into(&config(seed, self.options.structures), x, z)?;
        for (slot, game) in game_marks.iter().enumerate() {
            let mut packed = vec![Vec::new(); mc_bamboo_world::SECTION_COUNT];
            for &(wx, wy, wz) in game {
                let section = ((wy - mc_bamboo_world::MIN_Y) >> 4) as usize;
                if let Some(list) = packed.get_mut(section) {
                    list.push(((wx & 15) | ((wy & 15) << 4) | ((wz & 15) << 8)) as u16);
                }
            }
            if packed != marks[slot] {
                let official: usize = marks[slot].iter().map(Vec::len).sum();
                self.post_mismatches.push(format!(
                    "seed {seed} ({x},{z}) source {}: official {official} marks vs game {} \
                     (per-source marks before assembly)",
                    offset_name(slot),
                    game.len()
                ));
            }
        }
        let (delivered, reloaded) = self
            .bamboo_post
            .get(&(seed, x, z))
            .ok_or_else(|| format!("no game PostProcessing for seed {seed} ({x},{z})"))?;
        for (label, have) in [("delivered", delivered), ("save round trip", reloaded)] {
            self.post_entries += want.iter().map(Vec::len).sum::<usize>() as u64;
            if have.len() != want.len() {
                self.post_mismatches.push(format!(
                    "seed {seed} ({x},{z}) {label}: {} sections, expected {}",
                    have.len(),
                    want.len()
                ));
                continue;
            }
            for (section, (want, have)) in want.iter().zip(have).enumerate() {
                if want == have {
                    continue;
                }
                let mut a = want.clone();
                let mut b = have.clone();
                a.sort_unstable();
                b.sort_unstable();
                let kind = if a == b {
                    "same multiset, different order"
                } else {
                    "different multiset"
                };
                let first = want
                    .iter()
                    .zip(have)
                    .position(|(a, b)| a != b)
                    .unwrap_or(want.len().min(have.len()));
                self.post_mismatches.push(format!(
                    "seed {seed} ({x},{z}) {label} PostProcessing section {section} ({kind}): \
                     official {} vs game {}, first diff at {first}: official {:?}, game {:?}",
                    want.len(),
                    have.len(),
                    want.get(first),
                    have.get(first)
                ));
            }
        }
        self.post_lines += 1;
        Ok(())
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
        for line in &self.post_mismatches {
            println!("  MISMATCH {line}");
        }
        for line in self
            .entity_mismatches
            .iter()
            .take(self.options.show.max(20))
        {
            println!("  MISMATCH {line}");
        }
        println!(
            "feature-merge block entities: {} vanilla entries compared with the delivered \
             save, {} mismatches",
            self.entity_checks,
            self.entity_mismatches.len()
        );
        if self.options.post_process_diagnostic {
            // 已知差异：Bamboo 还没有完工后处理，这里只打印，不计入失败。
            // Known gap: Bamboo has no post-processing pass yet; print only.
            println!(
                "post-process diagnostic (known gap, not counted): {} targets",
                self.post_process_lines.len()
            );
            for line in &self.post_process_lines {
                println!("  {line}");
            }
            if self.post_process_lines.len() != self.options.targets.len() {
                return Err(format!(
                    "feature-merge: {} post-process lines for {} targets",
                    self.post_process_lines.len(),
                    self.options.targets.len()
                ));
            }
        }
        println!(
            "feature-merge: {} target×order lines, {} cells; isolated (fixed-order \
             first-writer merge of vanilla per-center writes) {} differing; shared ProtoChunk \
             orders {} differing; game spill writes by source {:?}; PostProcessing {} isolated \
             lines, {} entries (delivered + save round trip), official new marks by source {:?}, \
             {} mismatches",
            self.compared,
            self.cells,
            self.isolated_differing,
            self.shared_differing,
            self.spill_writes,
            self.post_lines,
            self.post_entries,
            self.post_marks_by_source,
            self.post_mismatches.len()
        );
        if self.isolated_differing > 0 {
            return Err(format!(
                "feature-merge: {} differing cells against the isolated merge",
                self.isolated_differing
            ));
        }
        let isolated_lines = if self.options.orders.iter().any(|order| order == "isolated") {
            self.options.targets.len()
        } else {
            0
        };
        if self.post_lines != isolated_lines {
            return Err(format!(
                "feature-merge: compared PostProcessing on {} isolated lines, expected {isolated_lines}",
                self.post_lines
            ));
        }
        if !self.entity_mismatches.is_empty() {
            return Err(format!(
                "feature-merge: {} block entity mismatches",
                self.entity_mismatches.len()
            ));
        }
        if !self.post_mismatches.is_empty() {
            return Err(format!(
                "feature-merge: {} PostProcessing mismatches",
                self.post_mismatches.len()
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

/// 一条官方 isolated 线里的 `PostProcessing`：目标的干净地形标记、9 个来源各自新记的
/// 标记，以及按约定顺序（地形在前，然后中心自己、其余 slot 升序）逐段拼出的完工表。
/// A vanilla isolated line's `PostProcessing`: the target's clean terrain marks,
/// each source's new marks, and the finished table concatenated per section in
/// the agreed order (terrain, then the center, then ascending slots).
pub(crate) struct IsolatedPostProcessing {
    pub(crate) terrain: Vec<Vec<u16>>,
    pub(crate) marks: Vec<Vec<Vec<u16>>>,
    pub(crate) full: Vec<Vec<u16>>,
}

/// 解析 isolated 线的 `postProcessing`（还原后的地形标记）和 `marks`（9 × 24 段）。
/// Parse an isolated line's `postProcessing` (restored terrain marks) and `marks`.
pub(crate) fn isolated_post_processing(value: &Value) -> Result<IsolatedPostProcessing, String> {
    let terrain = packed_sections(value.get("postProcessing"))
        .map_err(|error| format!("terrain PostProcessing: {error}"))?;
    let marks = value
        .get("marks")
        .and_then(Value::as_array)
        .ok_or("isolated line missing marks")?
        .iter()
        .map(|source| packed_sections(Some(source)))
        .collect::<Result<Vec<_>, String>>()?;
    if terrain.len() != mc_bamboo_world::SECTION_COUNT || marks.len() != 9 {
        return Err(format!(
            "{} terrain sections, {} mark sources",
            terrain.len(),
            marks.len()
        ));
    }
    let mut order: Vec<usize> = vec![4];
    order.extend((0..9).filter(|&slot| slot != 4));
    let mut full = terrain.clone();
    for &slot in &order {
        if marks[slot].len() != mc_bamboo_world::SECTION_COUNT {
            return Err(format!(
                "source {slot}: {} mark sections",
                marks[slot].len()
            ));
        }
        for (section, entries) in marks[slot].iter().enumerate() {
            full[section].extend_from_slice(entries);
        }
    }
    Ok(IsolatedPostProcessing {
        terrain,
        marks,
        full,
    })
}

/// 官方线里 24 段 packed short 的 `PostProcessing`。
/// A vanilla line's 24 packed-short `PostProcessing` sections.
fn packed_sections(value: Option<&Value>) -> Result<Vec<Vec<u16>>, String> {
    value
        .and_then(Value::as_array)
        .ok_or("PostProcessing is not an array")?
        .iter()
        .map(|section| {
            section
                .as_array()
                .ok_or("PostProcessing section is not an array")?
                .iter()
                .map(|entry| {
                    entry
                        .as_u64()
                        .and_then(|packed| u16::try_from(packed).ok())
                        .ok_or_else(|| "bad packed PostProcessing short".to_owned())
                })
                .collect()
        })
        .collect()
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
