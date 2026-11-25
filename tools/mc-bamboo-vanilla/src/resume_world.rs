//! `resume-world`：多阶段往返对拍（官方写到一半的区块，读回后接着生成）。
//!
//! Java 侧的 `ResumeWorldOracle.java` 用官方 `SerializableChunkData` codec 按每个
//! `ChunkStatus` 写真实存档 NBT，并用官方 `LevelLightEngine` 在 FEATURES 后的 3×3
//! 方块上算生成期光照。这里把 `ChunkgenOracle.java --through features` 已逐格对拍
//! 过的官方 FULL 数据作为题目，交给 Java 写成半成品存档，再用游戏侧的
//! `resume_replay` 读回、从盘上 `Status` 续跑到 `full`，逐格比：
//! 方块、quart 群系、六张高度图、方块实体、天空光 / 方块光、`isLightOn`、
//! 结构 `starts` / `References`，以及下一次写盘的 `Status`。
//!
//! `Multi-stage resume comparison`: Java writes real chunk NBT at each
//! `ChunkStatus` with the official codec and computes official generation-time
//! light over post-FEATURES blocks; Rust decodes it, resumes generation from the
//! on-disk status and compares every cell against vanilla's straight-to-FULL run.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    process::Stdio,
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::{registries, version};
use mc_bamboo_game::{
    resume_replay::{ResumeBreak, ResumeCase, clear_resume_overrides, resume_chunk},
    world_options::{LevelType, WorldGenConfig},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::ticket::ChunkStatus;
use mc_bamboo_worldgen::feature::state::state_from_official_name;
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: resume-world <java> [--seeds N] [--chunks N] [--salt S] \
     [--structures] [--break-self-test] [--break-neighbor] [--break-light] \
     [--break-post-processing] [--show N] [--seed S] [--chunk X Z]";
const SOURCE: &str = "tools/mc-bamboo-vanilla/java/ResumeWorldOracle.java";
const CHUNKGEN_SOURCE: &str = "tools/mc-bamboo-vanilla/java/ChunkgenOracle.java";
/// 官方 `Heightmap.Types.sendToClient()` 的四张图，也是存档会写出的图。
const FINAL_HEIGHTMAP_KEYS: [&str; 4] = [
    "WORLD_SURFACE",
    "OCEAN_FLOOR",
    "MOTION_BLOCKING",
    "MOTION_BLOCKING_NO_LEAVES",
];

/// 所有能存盘的阶段（含 empty 与 full）。
const STAGES: &[ChunkStatus] = &ChunkStatus::ALL;
/// 邻居盘上状态组合：没有存档、同阶段、已 full、更早期阶段。
const NEIGHBOR_MODES: &[&str] = &["none", "same", "full", "earlier"];

#[derive(Debug)]
struct Options {
    seeds: usize,
    chunks: usize,
    salt: i64,
    structures: bool,
    break_self_test: bool,
    break_neighbor: bool,
    break_light: bool,
    break_post_processing: bool,
    show: usize,
    /// 固定种子 / 固定区块（`--seed S --chunk X Z`），用于结构起点坐标。
    seed: Option<i64>,
    chunk: Option<(i32, i32)>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            seeds: 1,
            chunks: 1,
            salt: 7,
            structures: false,
            break_self_test: false,
            break_neighbor: false,
            break_light: false,
            break_post_processing: false,
            show: 8,
            seed: None,
            chunk: None,
        };
        let mut rest = args.iter();
        while let Some(&arg) = rest.next() {
            let mut next = || rest.next().ok_or(USAGE);
            match arg {
                "--seeds" => options.seeds = next()?.parse().map_err(|_| USAGE)?,
                "--chunks" => options.chunks = next()?.parse().map_err(|_| USAGE)?,
                "--salt" => options.salt = next()?.parse().map_err(|_| USAGE)?,
                "--show" => options.show = next()?.parse().map_err(|_| USAGE)?,
                "--structures" => options.structures = true,
                "--break-self-test" => options.break_self_test = true,
                "--break-neighbor" => options.break_neighbor = true,
                "--break-light" => options.break_light = true,
                "--break-post-processing" => options.break_post_processing = true,
                "--seed" => options.seed = Some(next()?.parse().map_err(|_| USAGE)?),
                "--chunk" => {
                    let x = next()?.parse().map_err(|_| USAGE)?;
                    let z = next()?.parse().map_err(|_| USAGE)?;
                    options.chunk = Some((x, z));
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        if options.seeds == 0 || options.chunks == 0 {
            return Err(USAGE.to_owned());
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Stats {
    cases: u64,
    blocks: u64,
    biomes: u64,
    heightmaps: u64,
    light_cells: u64,
    block_entities: u64,
    /// 比较过的 `PostProcessing` 条目数。
    post_processing: u64,
    mismatches: Vec<String>,
    breaks: u64,
    /// 「替换代替追加」改坏确认报出来的用例数。
    replace_breaks: u64,
    /// 真正比到非空结构起点 / 引用的用例数。
    structure_cases: u64,
}

/// 官方一口气生成到 FULL 的逐格数据（来自 `ChunkgenOracle --through features`）。
#[derive(Debug, Clone)]
struct FullChunk {
    seed: i64,
    pos: ChunkPos,
    /// 3×3 视图，每格是 `(调色板名数组, 方块下标数组)`。
    view: Vec<(Vec<String>, Vec<u32>)>,
    biomes: Vec<Vec<String>>,
    heightmaps: BTreeMap<String, Vec<i64>>,
    heightmap_words: BTreeMap<String, Vec<i64>>,
    block_entities: Vec<(i32, i32, i32, String)>,
    structures: BTreeMap<String, String>,
    /// 中心区块官方 FULL 的 `PostProcessing`（24 段 packed short）：地形标记 +
    /// 9 个中心各自记进来的标记，按约定顺序（中心先、slot 升序）拼成。
    post_processing: Vec<Vec<u16>>,
    /// 中心区块官方 TERRAIN 阶段的 `PostProcessing`（doFill 含水层标记）。
    post_processing_terrain: Vec<Vec<u16>>,
}

pub fn run(options: &[&str], java: &str) -> Result<(), String> {
    let options = Options::parse(options)?;
    // 1. 官方一口气生成到 FEATURES（3×3 视图 + 方块实体 + 结构）。
    let carvers = official_carvers(java, &options)?;
    let mut official = official_full_chunks(java, &options, &carvers)?;
    // 官方 9 个中心各跑一遍地物，再用游戏固定先后合规则得到中心最终方块。
    // `feature-merge --orders isolated` 已证明这条线和官方逐中心输出逐格一致；
    // 单中心的 `features` 行只覆盖中心自己的地物，少了邻居溢进来的写。
    // Official FULL is the 9-center merge, not the single-center features line.
    let scenes = official_light_scenes(java, &options)?;
    let merged = official_merged_blocks(java, &options)?;
    for full in &mut official {
        let key = (full.seed, full.pos.x, full.pos.z);
        let Some(baseline) = merged.get(&key) else {
            return Err(format!("no isolated merge line for {key:?}"));
        };
        // 官方 FULL 的 `PostProcessing` 也取 9 中心那条线：单中心的 `features` 行
        // 只有中心自己记的标记，少了邻居中心记进来的。两条线的地形标记必须相同。
        // Official FULL `PostProcessing` also comes from the 9-center line; the
        // single-center line lacks neighbors' marks. Terrain marks must agree.
        if baseline.post_processing.terrain != full.post_processing_terrain {
            return Err(format!(
                "{key:?}: isolated terrain PostProcessing differs from the features line"
            ));
        }
        full.post_processing = baseline.post_processing.full.clone();
        let blocks = &baseline.blocks;
        let names = blocks
            .iter()
            .map(|&state| mc_bamboo_worldgen::feature::state::official_name(state))
            .collect::<Vec<String>>();
        let mut palette = Vec::new();
        let mut indices = Vec::with_capacity(blocks.len());
        for name in names {
            let index = palette
                .iter()
                .position(|entry| entry == &name)
                .unwrap_or_else(|| {
                    palette.push(name);
                    palette.len() - 1
                });
            indices.push(index as u32);
        }
        full.view[4] = (palette, indices.clone());
        // 官方 CARVERS 高度图是地物之前的；FULL 的四张 FINAL 图要按
        // FEATURES 之后的方块重算（`Heightmap.update` 增量维护的终值等价于
        // 从最终方块 prime）。`build_region_column` + `final_heightmap_words`
        // 是游戏写盘走的那条路径，和官方逐字对拍过。
        // Recompute the four FINAL heightmaps from the post-FEATURES blocks,
        // matching official `Heightmap.update` (equals primeHeightmaps).
        let state_ids = final_states_from_view(&full.view[4]);
        let generated = mc_bamboo_game::worldgen::GeneratedChunk {
            blocks: state_ids.chunks(256).map(<[u32]>::to_vec).collect(),
            biomes: vec![vec![0_u32; 64]; mc_bamboo_world::SECTION_COUNT],
            heightmaps: Default::default(),
            post_processing: Vec::new(),
            block_entities: Vec::new(),
            entities: Vec::new(),
        };
        let words = mc_bamboo_game::worldgen::final_heightmap_words(&generated)
            .map_err(|e| format!("final heightmaps {key:?}: {e}"))?;
        for (name, values) in words {
            let mut columns = Vec::with_capacity(256);
            for word in &values {
                for slot in 0..7 {
                    columns.push(((word >> (slot * 9)) & 0x1ff) as i64 - 64);
                }
            }
            columns.truncate(256);
            full.heightmaps.insert(name.to_owned(), columns);
            full.heightmap_words
                .insert(name.to_owned(), values.iter().map(|&v| v as i64).collect());
        }
    }
    // 2. 把每一题按 10 个阶段 × 4 种邻居组合交给官方 codec 写成半成品存档。
    let mut child = crate::official_program(java, SOURCE)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    // 一问一答：Java 的答案含整块 NBT / 光照字节，先把所有请求写完再读会
    // 填满管道死锁。逐条写、逐条读，顺便把答案按请求顺序收好。
    // Request/response one at a time: Java answers with full chunk NBT and
    // light bytes, so writing every request before reading fills the pipe.
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("java has no stdout")?);
    let mut requests = Vec::new();
    for full in &official {
        for stage in STAGES {
            for mode in NEIGHBOR_MODES {
                let key = (full.seed, full.pos.x, full.pos.z);
                let scene = scenes
                    .get(&key)
                    .ok_or_else(|| format!("no official lightscene for {key:?}"))?;
                requests.push(make_request(full, *stage, mode, options.structures, scene));
            }
        }
    }
    let mut read_line = || -> Result<String, String> {
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .map_err(|error| format!("cannot read java: {error}"))?;
        if line.is_empty() {
            return Err("java closed stdout before answering".to_owned());
        }
        Ok(line)
    };
    let header: Value = serde_json::from_str(&read_line()?)
        .map_err(|error| format!("bad oracle header: {error}"))?;
    let reported = header
        .get("version")
        .and_then(Value::as_str)
        .ok_or("header missing version")?;
    let protocol = header
        .get("protocol")
        .and_then(Value::as_i64)
        .ok_or("header missing protocol")?;
    if reported != version::GAME_VERSION || protocol != i64::from(version::PROTOCOL_VERSION) {
        return Err(format!(
            "oracle is {reported}/{protocol}, bamboo is {}/{}",
            version::GAME_VERSION,
            version::PROTOCOL_VERSION
        ));
    }

    let mut stats = Stats::default();
    let mut cases = Vec::new();
    for request in &requests {
        writeln!(stdin, "{request}")
            .map_err(|error| format!("cannot write java stdin: {error}"))?;
        stdin
            .flush()
            .map_err(|error| format!("cannot flush java stdin: {error}"))?;
        let line = read_line()?;
        let value: Value =
            serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        if kind != "case" {
            return Err(format!("unexpected oracle line type {kind}"));
        }
        cases.push(value);
    }
    drop(stdin);
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("ResumeWorldOracle.java failed: {status}\n{log}"));
    }
    if cases.len() != requests.len() {
        return Err(format!(
            "oracle wrote {} cases for {} requests",
            cases.len(),
            requests.len()
        ));
    }

    // 3. 每个半成品存档用游戏侧同一批阶段纯函数续跑，和官方 FULL 逐格比。
    for (case, full) in cases.iter().zip(official.iter().flat_map(|full| {
        STAGES
            .iter()
            .flat_map(move |stage| NEIGHBOR_MODES.iter().map(move |mode| (full, *stage, *mode)))
    })) {
        let (full, stage, mode) = full;
        compare_case(&mut stats, case, full, stage, mode, &options)?;
    }
    clear_resume_overrides();
    finish(stats, &options, requests.len())
}

fn finish(stats: Stats, options: &Options, requests: usize) -> Result<(), String> {
    let Stats {
        cases,
        blocks,
        biomes,
        heightmaps,
        light_cells,
        block_entities,
        post_processing,
        mismatches,
        breaks,
        replace_breaks,
        structure_cases,
    } = stats;
    println!(
        "resume-world: {requests} requests, {cases} resumed cases, {blocks} blocks, \
         {biomes} quart cells, {heightmaps} heightmap columns, {light_cells} light cells, \
         {block_entities} block entities, {post_processing} post-processing entries, \
         {structure_cases} structured cases, {} mismatches, {breaks} break checks",
        mismatches.len(),
    );
    if options.break_self_test
        || options.break_neighbor
        || options.break_light
        || options.break_post_processing
    {
        if breaks == 0 {
            return Err("resume-world break check did not run".to_owned());
        }
        if options.break_post_processing && replace_breaks == 0 {
            return Err("resume-world: the replace-instead-of-append break never ran".to_owned());
        }
        let undetected: Vec<&String> = mismatches
            .iter()
            .filter(|message| message.contains("was not detected"))
            .collect();
        if !undetected.is_empty() {
            for message in &undetected {
                println!("  {message}");
            }
            return Err(format!(
                "resume-world: {} break checks were not detected",
                undetected.len()
            ));
        }
        println!(
            "resume-world break checks: {breaks} confirmed mismatches \
             ({replace_breaks} replace-instead-of-append)"
        );
        return Ok(());
    }
    if cases != requests as u64 || blocks == 0 {
        return Err("resume-world compared no chunks".to_owned());
    }
    if !mismatches.is_empty() {
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for message in &mismatches {
            let kind = message
                .split_once(": ")
                .map(|(_, rest)| rest.split_whitespace().next().unwrap_or("?"))
                .unwrap_or("?");
            *by_kind.entry(kind.to_owned()).or_default() += 1;
        }
        println!("resume-world mismatch kinds: {by_kind:?}");
        let mut by_stage: BTreeMap<String, usize> = BTreeMap::new();
        for message in &mismatches {
            if let Some(stage) = message
                .split("stage ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
            {
                *by_stage.entry(stage.to_owned()).or_default() += 1;
            }
        }
        println!("resume-world mismatches by stage: {by_stage:?}");
        let mut by_pair: BTreeMap<(String, String), usize> = BTreeMap::new();
        for message in &mismatches {
            let stage = message
                .split("stage ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("?");
            let kind = message
                .split_once(": ")
                .map(|(_, rest)| rest.split_whitespace().next().unwrap_or("?"))
                .unwrap_or("?");
            *by_pair
                .entry((stage.to_owned(), kind.to_owned()))
                .or_default() += 1;
        }
        println!("resume-world mismatches by (stage, kind): {by_pair:?}");
        let mut examples: BTreeMap<String, &String> = BTreeMap::new();
        for message in &mismatches {
            let kind = message
                .split_once(": ")
                .map(|(_, rest)| rest.split_whitespace().next().unwrap_or("?"))
                .unwrap_or("?");
            examples.entry(kind.to_owned()).or_insert(message);
        }
        for (kind, message) in examples {
            eprintln!("  EXAMPLE {kind}: {message}");
        }
        for message in mismatches.iter().take(options.show) {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!(
            "resume-world found {} mismatches",
            mismatches.len()
        ));
    }
    println!("resume-world: 0 mismatches");
    Ok(())
}

/// 把 `--seed` / `--chunk` 追加给官方出题器（固定结构起点坐标用）。
fn forwarded(options: &Options, args: &mut Vec<String>) {
    if let Some(seed) = options.seed {
        args.extend(["--seed".to_owned(), seed.to_string()]);
    }
    if let Some((x, z)) = options.chunk {
        args.extend(["--chunk".to_owned(), x.to_string(), z.to_string()]);
    }
}

/// 官方 `ChunkgenOracle --light-pipeline` 的一条题：FEATURES 后的 3×3 方块
/// 和官方 `LevelLightEngine` 算出的中心柱逐格天空光 / 方块光。
#[derive(Debug, Clone)]
struct LightScene {
    /// 3×3 每格的方块状态 ID（`CELLS` 个）。
    blocks: Vec<Vec<u32>>,
    /// 中心柱逐格天空光（`CELLS` 个 4 位值）。
    sky: Vec<u8>,
    /// 中心柱逐格方块光。
    block: Vec<u8>,
}

/// 官方 `--light-pipeline` 给出 FEATURES 后 3×3 的真实方块和官方光照。
///
/// `resume-world` 的 Java 出题器用这批方块写存档、跑官方光引擎；
/// 单中心的 `features` 行少了邻居溢出的写，不能当 LIGHT 的输入。
fn official_light_scenes(
    java: &str,
    options: &Options,
) -> Result<BTreeMap<(i64, i32, i32), LightScene>, String> {
    let mut command = crate::official_program(java, CHUNKGEN_SOURCE)?;
    let mut args = vec![
        "--light-pipeline".to_owned(),
        "--seeds".to_owned(),
        options.seeds.to_string(),
        "--chunks".to_owned(),
        options.chunks.to_string(),
        "--salt".to_owned(),
        options.salt.to_string(),
    ];
    forwarded(options, &mut args);
    if options.structures {
        args.push("--structures".to_owned());
    }
    command.args(&args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("run light-pipeline: {e}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut out = BTreeMap::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("read light-pipeline: {e}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad light-pipeline line: {e}"))?;
        if value.get("type").and_then(Value::as_str) != Some("lightscene") {
            continue;
        }
        let (seed, x, z) = line_key(&value)?;
        let palette = value
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("lightscene missing palette")?;
        let states = palette
            .iter()
            .map(|entry| {
                entry
                    .as_u64()
                    .map(|id| id as u32)
                    .ok_or("bad lightscene state id".to_owned())
            })
            .collect::<Result<Vec<u32>, String>>()?;
        let chunks = value
            .get("chunks")
            .and_then(Value::as_array)
            .ok_or("lightscene missing chunks")?;
        if chunks.len() != 9 {
            return Err(format!("lightscene has {} chunks", chunks.len()));
        }
        let mut blocks = Vec::with_capacity(9);
        for chunk in chunks {
            let indices = chunk
                .get("blocks")
                .and_then(Value::as_array)
                .ok_or("chunk missing blocks")?;
            blocks.push(
                indices
                    .iter()
                    .map(|entry| {
                        let id = entry
                            .as_u64()
                            .ok_or("bad lightscene block index".to_owned())?
                            as usize;
                        states
                            .get(id)
                            .copied()
                            .ok_or("lightscene state index out of range".to_owned())
                    })
                    .collect::<Result<Vec<u32>, String>>()?,
            );
        }
        let sky = byte_values(value.get("sky").ok_or("lightscene missing sky")?)?;
        let block = byte_values(value.get("block").ok_or("lightscene missing block")?)?;
        out.insert((seed, x, z), LightScene { blocks, sky, block });
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait light-pipeline: {e}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("light-pipeline failed: {status}\n{log}"));
    }
    Ok(out)
}

fn byte_values(value: &Value) -> Result<Vec<u8>, String> {
    value
        .as_array()
        .ok_or("light bytes must be an array")?
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .map(|v| v as u8)
                .ok_or("bad light byte".to_owned())
        })
        .collect()
}

/// 官方一口气生成到 FULL 的一个区块：逐格方块状态和 `PostProcessing`。
struct Baseline {
    blocks: Vec<u32>,
    post_processing: crate::feature_merge::IsolatedPostProcessing,
}

/// `(种子, x, z) → 官方 FULL 基准`。
type BlockBaselines = BTreeMap<(i64, i32, i32), Baseline>;

/// 官方 `ChunkgenOracle --merge-neighbors isolated` 的 9 中心写增量，
/// 按游戏同一份 `merge_feature_writes` 合成中心最终方块。
/// 这条线来自官方每个中心真实跑过的 `applyBiomeDecoration`，
/// `feature-merge` 已逐格证明过它等于 Director 交付的中心方块。
fn official_merged_blocks(java: &str, options: &Options) -> Result<BlockBaselines, String> {
    let queries = crate::workspace_root().join("target/vanilla/resume_queries.txt");
    let text = official_full_chunk_keys(java, options)?
        .iter()
        .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
        .collect::<String>();
    std::fs::write(&queries, text).map_err(|e| format!("write queries: {e}"))?;
    let mut command = crate::official_program(java, CHUNKGEN_SOURCE)?;
    let mut args = vec![
        "--through".to_owned(),
        "features".to_owned(),
        "--merge-neighbors".to_owned(),
        "isolated".to_owned(),
        "--chunks-file".to_owned(),
        queries.to_string_lossy().into_owned(),
    ];
    forwarded(options, &mut args);
    command.args(&args);
    if options.structures {
        command.arg("--structures");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("run merge oracle: {e}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut out = BTreeMap::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("read merge oracle: {e}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad merge oracle line: {e}"))?;
        if value.get("type").and_then(Value::as_str) != Some("isolated") {
            continue;
        }
        let (seed, x, z) = line_key(&value)?;
        let runs = value.get("runs").and_then(Value::as_i64).ok_or("runs")?;
        if runs != 9 {
            return Err(format!("isolated ran {runs} centers"));
        }
        let baseline = value
            .get("blocks")
            .and_then(Value::as_array)
            .ok_or("isolated missing blocks")?;
        let names = value
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("isolated missing palette")?;
        let states = names
            .iter()
            .map(|n| {
                mc_bamboo_worldgen::feature::state::state_from_official_name(
                    n.as_str().unwrap_or(""),
                )
            })
            .collect::<Result<Vec<u32>, String>>()?;
        let terrain = baseline
            .iter()
            .map(|v| {
                states
                    .get(v.as_u64().unwrap_or(0) as usize)
                    .copied()
                    .ok_or("state index out of range".to_owned())
            })
            .collect::<Result<Vec<u32>, String>>()?;
        // 9 个来源的写用同一份游戏合并规则；cell 下标是 (y*16+x)*16+z。
        let write_palette = value
            .get("writePalette")
            .and_then(Value::as_array)
            .ok_or("isolated missing writePalette")?;
        let write_states = write_palette
            .iter()
            .map(|n| {
                mc_bamboo_worldgen::feature::state::state_from_official_name(
                    n.as_str().unwrap_or(""),
                )
            })
            .collect::<Result<Vec<u32>, String>>()?;
        let sources = value
            .get("sources")
            .and_then(Value::as_array)
            .ok_or("isolated missing sources")?;
        if sources.len() != 9 {
            return Err(format!("isolated has {} sources", sources.len()));
        }
        let mut groups = Vec::new();
        for (slot, writes) in sources.iter().enumerate() {
            let mut deltas = Vec::new();
            for write in writes.as_array().ok_or("source is not an array")? {
                let pair = write.as_array().ok_or("write is not a pair")?;
                let cell = pair.first().and_then(Value::as_u64).ok_or("bad cell")? as usize;
                let id = pair.get(1).and_then(Value::as_u64).ok_or("bad state")? as usize;
                let state = *write_states.get(id).ok_or("write state out of palette")?;
                let y = mc_bamboo_world::MIN_Y + (cell / 256) as i32;
                let lx = ((cell / 16) % 16) as i32;
                let lz = (cell % 16) as i32;
                deltas.push((x * 16 + lx, y, z * 16 + lz, state));
            }
            groups.push(mc_bamboo_game::worldgen_features::FeatureWrites {
                slot: slot as u8,
                deltas,
                marks: Vec::new(),
                block_entities: Vec::new(),
                entities: Vec::new(),
            });
        }
        let mut chunk = mc_bamboo_game::worldgen::GeneratedChunk {
            blocks: terrain.chunks(256).map(<[u32]>::to_vec).collect(),
            biomes: Vec::new(),
            heightmaps: Default::default(),
            post_processing: Vec::new(),
            block_entities: Vec::new(),
            entities: Vec::new(),
        };
        mc_bamboo_game::worldgen_features::merge_feature_writes(&mut chunk, groups);
        let post_processing = crate::feature_merge::isolated_post_processing(&value)
            .map_err(|error| format!("isolated ({seed},{x},{z}): {error}"))?;
        out.insert(
            (seed, x, z),
            Baseline {
                blocks: chunk.blocks.into_iter().flatten().collect(),
                post_processing,
            },
        );
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait merge oracle: {e}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("merge oracle failed: {status}\n{log}"));
    }
    Ok(out)
}

/// 官方 `features` 行的 `(seed, x, z)` 键，用来给 merge oracle 写查询文件。
fn official_full_chunk_keys(java: &str, options: &Options) -> Result<Vec<(i64, i32, i32)>, String> {
    let mut command = crate::official_program(java, CHUNKGEN_SOURCE)?;
    let mut args = vec![
        "--through".to_owned(),
        "biomes".to_owned(),
        "--seeds".to_owned(),
        options.seeds.to_string(),
        "--chunks".to_owned(),
        options.chunks.to_string(),
        "--salt".to_owned(),
        options.salt.to_string(),
    ];
    forwarded(options, &mut args);
    command.args(&args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("run biomes oracle: {e}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut keys = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("read biomes oracle: {e}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad biomes oracle line: {e}"))?;
        if value.get("type").and_then(Value::as_str) == Some("biomes") {
            keys.push(line_key(&value)?);
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait biomes oracle: {e}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("biomes oracle failed: {status}\n{log}"));
    }
    Ok(keys)
}

/// `--real-path --through carvers` 的官方 quart 群系与高度图。
#[derive(Debug, Clone)]
struct CarversData {
    biomes: Vec<Vec<String>>,
    heightmaps: BTreeMap<String, Vec<i64>>,
    heightmap_words: BTreeMap<String, Vec<i64>>,
}

/// 官方 `--through carvers --real-path` 给同一批坐标的 quart 群系和六张高度图。
///
/// `ChunkgenOracle` 的 features 行只带 3×3 方块，群系 / 高度图由这条独立的
/// CARVERS 路径提供（和 `gen-world` 用的是同一份官方输出）。
fn official_carvers(
    java: &str,
    options: &Options,
) -> Result<BTreeMap<(i64, i32, i32), CarversData>, String> {
    let mut command = crate::official_program(java, CHUNKGEN_SOURCE)?;
    let mut args = vec![
        "--through".to_owned(),
        "carvers".to_owned(),
        "--real-path".to_owned(),
        "--seeds".to_owned(),
        options.seeds.to_string(),
        "--chunks".to_owned(),
        options.chunks.to_string(),
        "--salt".to_owned(),
        options.salt.to_string(),
    ];
    forwarded(options, &mut args);
    command.args(&args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run ChunkgenOracle carvers: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut biomes: BTreeMap<(i64, i32, i32), Vec<Vec<String>>> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("cannot read java: {error}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        match kind {
            "biomes" => {
                let key = line_key(&value)?;
                let sections = value
                    .get("sections")
                    .and_then(Value::as_array)
                    .ok_or("biomes line missing sections")?;
                let parsed = sections
                    .iter()
                    .map(|cells| {
                        cells
                            .as_array()
                            .ok_or("bad biome section")?
                            .iter()
                            .map(|name| {
                                name.as_str()
                                    .map(str::to_owned)
                                    .ok_or("bad biome name".to_owned())
                            })
                            .collect::<Result<Vec<String>, String>>()
                    })
                    .collect::<Result<Vec<Vec<String>>, String>>()?;
                biomes.insert(key, parsed);
            }
            "carvers" => {
                let key = line_key(&value)?;
                let names = biomes
                    .remove(&key)
                    .ok_or_else(|| format!("no biomes line for {key:?}"))?;
                let columns = value
                    .get("heightmaps")
                    .and_then(Value::as_object)
                    .ok_or("carvers line missing heightmaps")?;
                let mut heightmaps = BTreeMap::new();
                for (name, values) in columns {
                    // `SerializableChunkData.write` 只写 `sendToClient()` 的四张 FINAL 图；
                    // 两张 WG 图不进存档，不能拿来比。
                    // Only the four FINAL maps are persisted; the two WG maps are not.
                    if !FINAL_HEIGHTMAP_KEYS.contains(&name.as_str()) {
                        continue;
                    }
                    let parsed = values
                        .as_array()
                        .ok_or("heightmap columns must be an array")?
                        .iter()
                        .map(|entry| entry.as_i64().ok_or("bad heightmap column".to_owned()))
                        .collect::<Result<Vec<i64>, String>>()?;
                    heightmaps.insert(name.clone(), parsed);
                }
                let words = parse_words(
                    value
                        .get("heightmap_words")
                        .ok_or("carvers line missing heightmap_words")?,
                )?;
                out.insert(
                    key,
                    CarversData {
                        biomes: names,
                        heightmaps,
                        heightmap_words: words,
                    },
                );
            }
            "noise" | "surface" | "header" | "end" => {}
            other => return Err(format!("unexpected carvers oracle line {other}")),
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("ChunkgenOracle carvers failed: {status}\n{log}"));
    }
    Ok(out)
}

/// 调 `ChunkgenOracle --through features` 收集官方 FEATURES 后的 3×3 视图。
fn official_full_chunks(
    java: &str,
    options: &Options,
    carvers: &BTreeMap<(i64, i32, i32), CarversData>,
) -> Result<Vec<FullChunk>, String> {
    let mut command = crate::official_program(java, CHUNKGEN_SOURCE)?;
    let mut args = vec![
        "--through".to_owned(),
        "features".to_owned(),
        "--seeds".to_owned(),
        options.seeds.to_string(),
        "--chunks".to_owned(),
        options.chunks.to_string(),
        "--salt".to_owned(),
        options.salt.to_string(),
    ];
    forwarded(options, &mut args);
    command.args(&args);
    if options.structures {
        command.arg("--structures");
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run ChunkgenOracle: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut out = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("cannot read java: {error}"))?;
        let value: Value =
            serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        match kind {
            "header" => {}
            "biomes" => {}
            "features" => {
                let key = line_key(&value)?;
                let (seed, x, z) = key;
                let chunks = value
                    .get("chunks")
                    .and_then(Value::as_array)
                    .ok_or("features line missing chunks")?;
                if chunks.len() != 9 {
                    return Err(format!("features line has {} chunks", chunks.len()));
                }
                let mut view = Vec::new();
                for chunk in chunks {
                    let names = chunk
                        .get("palette")
                        .and_then(Value::as_array)
                        .ok_or("chunk missing palette")?
                        .iter()
                        .map(|entry| {
                            entry
                                .as_str()
                                .map(str::to_owned)
                                .ok_or("bad palette entry".to_owned())
                        })
                        .collect::<Result<Vec<String>, String>>()?;
                    let indices = chunk
                        .get("blocks")
                        .and_then(Value::as_array)
                        .ok_or("chunk missing blocks")?
                        .iter()
                        .map(|entry| {
                            entry
                                .as_u64()
                                .map(|value| value as u32)
                                .ok_or("bad block index".to_owned())
                        })
                        .collect::<Result<Vec<u32>, String>>()?;
                    view.push((names, indices));
                }
                let carvers_data = carvers
                    .get(&key)
                    .ok_or_else(|| format!("no carvers line for ({seed},{x},{z})"))?;
                let biomes = carvers_data.biomes.clone();
                let heightmap_words = carvers_data.heightmap_words.clone();
                let heightmaps = carvers_data.heightmaps.clone();
                let mut block_entities = Vec::new();
                if let Some(entries) = chunks[4].get("block_entities").and_then(Value::as_array) {
                    for entry in entries {
                        let row = entry.as_array().ok_or("bad block entity row")?;
                        if row.len() != 4 {
                            return Err("block entity row must have 4 fields".to_owned());
                        }
                        block_entities.push((
                            row[0].as_i64().ok_or("bad x")? as i32,
                            row[1].as_i64().ok_or("bad y")? as i32,
                            row[2].as_i64().ok_or("bad z")? as i32,
                            row[3].as_str().ok_or("bad block entity SNBT")?.to_owned(),
                        ));
                    }
                }
                // `--structures` 时 Java 侧另外发 coverage 行；features 行本身没有
                // structures 标签，用游戏侧同一份纯函数补出官方形状的标签。
                let structures = if options.structures {
                    structures_for(seed, x, z, options.structures)
                } else {
                    BTreeMap::new()
                };
                let post_processing = packed_sections(chunks[4].get("postProcessing"))?;
                let post_processing_terrain =
                    packed_sections(chunks[4].get("postProcessingTerrain"))?;
                out.push(FullChunk {
                    seed,
                    pos: ChunkPos::new(x, z),
                    view,
                    biomes,
                    heightmaps,
                    heightmap_words,
                    block_entities,
                    structures,
                    post_processing,
                    post_processing_terrain,
                });
            }
            "coverage" | "end" | "noise" | "surface" | "carvers" => {}
            other => return Err(format!("unexpected ChunkgenOracle line {other}")),
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("ChunkgenOracle.java failed: {status}\n{log}"));
    }
    if out.is_empty() {
        return Err("ChunkgenOracle emitted no features lines".to_owned());
    }
    Ok(out)
}

fn line_key(value: &Value) -> Result<(i64, i32, i32), String> {
    let get = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("line missing {key}"))
    };
    Ok((get("seed")?, get("x")? as i32, get("z")? as i32))
}

fn parse_words(value: &Value) -> Result<BTreeMap<String, Vec<i64>>, String> {
    let object = value
        .as_object()
        .ok_or("heightmap words must be an object")?;
    let mut out = BTreeMap::new();
    for (name, words) in object {
        if !FINAL_HEIGHTMAP_KEYS.contains(&name.as_str()) {
            continue;
        }
        let parsed = words
            .as_array()
            .ok_or("heightmap words entry must be an array")?
            .iter()
            .map(|word| word.as_i64().ok_or("bad heightmap word".to_owned()))
            .collect::<Result<Vec<i64>, String>>()?;
        out.insert(name.clone(), parsed);
    }
    Ok(out)
}

fn structures_for(seed: i64, x: i32, z: i32, structures: bool) -> BTreeMap<String, String> {
    let config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        generate_structures: structures,
        ..WorldGenConfig::default()
    };
    let Ok(tag) = mc_bamboo_game::worldgen_structures::encode_structures(
        &config,
        &mc_bamboo_game::worldgen_structures::structure_starts(&config, x, z).unwrap_or_default(),
        &mc_bamboo_game::worldgen_structures::structure_references_stage(&config, x, z)
            .unwrap_or_default(),
    ) else {
        return BTreeMap::new();
    };
    let Tag::Compound(entries) = tag else {
        return BTreeMap::new();
    };
    entries
        .into_iter()
        .map(|(key, value)| (key, value.to_snbt()))
        .collect()
}

fn final_states_from_view(view: &(Vec<String>, Vec<u32>)) -> Vec<u32> {
    let states = view
        .0
        .iter()
        .map(|name| state_from_official_name(name).unwrap_or(0))
        .collect::<Vec<u32>>();
    view.1
        .iter()
        .map(|&index| states.get(index as usize).copied().unwrap_or(0))
        .collect()
}

fn make_request(
    full: &FullChunk,
    stage: ChunkStatus,
    mode: &str,
    structures: bool,
    scene: &LightScene,
) -> Value {
    // 官方 `--light-pipeline` 的 3×3 post-FEATURES 方块（状态 ID），
    // 是 LIGHT 阶段真实读到的视图；单中心 features 行少了邻居溢出。
    let view: Vec<Value> = scene
        .blocks
        .iter()
        .map(|blocks| json!({ "blocks": blocks }))
        .collect();
    let biomes: Vec<Value> = full
        .biomes
        .iter()
        .map(|cells| Value::Array(cells.iter().cloned().map(Value::String).collect()))
        .collect();
    let block_entities: Vec<Value> = full
        .block_entities
        .iter()
        .map(|(x, y, z, snbt)| json!([x, y, z, snbt]))
        .collect();
    let mut heightmaps = serde_json::Map::new();
    for (key, columns) in &full.heightmaps {
        heightmaps.insert(
            key.clone(),
            Value::Array(columns.iter().map(|&value| json!(value)).collect()),
        );
    }
    let mut words = serde_json::Map::new();
    for (key, values) in &full.heightmap_words {
        words.insert(
            key.clone(),
            Value::Array(values.iter().map(|&value| json!(value)).collect()),
        );
    }
    let mut neighbor_tags = Vec::new();
    if mode != "none" {
        // 邻居的 structures 由同一份纯函数按坐标算出，覆盖起点 / 引用跨区块恢复。
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let tags = structures_for(full.seed, full.pos.x + dx, full.pos.z + dz, structures);
                neighbor_tags.push(json!({
                    "x": full.pos.x + dx,
                    "z": full.pos.z + dz,
                    "structures": tags,
                }));
            }
        }
    }
    json!({
        "command": "write",
        "seed": full.seed,
        "x": full.pos.x,
        "z": full.pos.z,
        "stage": stage.name(),
        "neighbor": mode,
        "view": view,
        "sky": scene.sky,
        "block": scene.block,
        "neighbors": neighbor_tags,
        "full": {
            "palette": full.view[4].0,
            "blocks": full.view[4].1,
            "biomes": biomes,
            "heightmaps": heightmaps,
            "heightmap_words": words,
            "block_entities": block_entities,
            "structures": full.structures,
            "post_processing": full.post_processing,
            "post_processing_terrain": full.post_processing_terrain,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn compare_case(
    stats: &mut Stats,
    case: &Value,
    full: &FullChunk,
    stage: ChunkStatus,
    mode: &str,
    options: &Options,
) -> Result<(), String> {
    let seed = case
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or("case missing seed")?;
    let x = case
        .get("x")
        .and_then(Value::as_i64)
        .ok_or("case missing x")? as i32;
    let z = case
        .get("z")
        .and_then(Value::as_i64)
        .ok_or("case missing z")? as i32;
    let pos = ChunkPos::new(x, z);
    let nbt = decode_base64(case.get("chunk_nbt").ok_or("case missing chunk_nbt")?)?;
    let full_nbt = decode_base64(case.get("full_nbt").ok_or("case missing full_nbt")?)?;
    let config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        generate_structures: options.structures,
        ..WorldGenConfig::default()
    };
    let neighbor_tags = parse_neighbor_tags(case)?;
    let clean = resume_chunk(
        &config,
        &ResumeCase {
            pos,
            chunk_nbt: nbt.clone(),
            stage,
        },
        &neighbor_tags,
        None,
    )
    .map_err(|error| format!("resume seed {seed} ({x},{z}) stage {stage:?}: {error}"))?;
    stats.cases += 1;
    let label = format!(
        "seed {seed} ({x},{z}) stage {} neighbor {mode}",
        stage.name()
    );

    // 方块 / 群系 / 高度图 / 方块实体 / 结构 / Status：和官方一口气到 FULL 逐格比。
    let expected_states = full.view[4]
        .0
        .iter()
        .map(|name| state_from_official_name(name))
        .collect::<Result<Vec<u32>, String>>()?;
    compare_blocks(
        stats,
        &label,
        &expected_states,
        &full.view[4].0,
        &full.view[4].1,
        &clean,
    );
    compare_biomes(stats, &label, &full.biomes, &clean);
    compare_heightmaps(stats, &label, &full.heightmaps, &clean);
    // 生成期方块实体由 `game_features_with_mask_skipping`（同一份 run_features）
    // 交回，和 `gen-world` / `structure-e2e` 的方块实体比较器用同一个来源；
    // 交付列里的读盘条目是另一条路径，不参与这项比较。
    // Feature-phase block entities come from the same pure feature run as
    // `gen-world`/`structure-e2e`; decoded disk entries are a separate path.
    let feature_result = mc_bamboo_game::worldgen_features::game_features_with_mask_skipping(
        &config,
        x,
        z,
        None,
        (1 << 11) - 1,
        None,
    )
    .map_err(|error| format!("{label}: game features for block entities: {error}"))?;
    compare_block_entities(
        stats,
        &label,
        &full.block_entities,
        &feature_result.block_entities,
    );
    compare_structures(stats, &label, &full_nbt, &clean);
    compare_post_processing(stats, &label, &full.post_processing, &clean.post_processing);
    if clean.saved_status != ChunkStatus::Full {
        stats.mismatches.push(format!(
            "{label}: saved status {:?}, expected full",
            clean.saved_status
        ));
    }

    // 光照：官方 FULL 存档里的两层光照是基线。Status >= light 时官方读盘直接
    // 复用盘上光照；Status < light 时游戏续跑的 LIGHT 输入现在是 CARVERS，
    // 差异按主代理要求单独分类列出，不放宽、合并不隐藏。
    let expected = mc_bamboo_storage::chunk_nbt::decode_bytes(&full_nbt)
        .map_err(|error| format!("{label}: official full NBT: {error}"))?;
    compare_light(stats, &label, &expected.column, &clean);

    // 改坏确认：把读回状态当 full、丢掉邻居写进中心的增量，两种都必须报不一致。
    if options.break_self_test && stage == ChunkStatus::StructureStarts && mode == "none" {
        let broken = resume_chunk(
            &config,
            &ResumeCase {
                pos,
                chunk_nbt: nbt.clone(),
                stage,
            },
            &neighbor_tags,
            Some(ResumeBreak::TreatAsFull),
        )
        .map_err(|error| format!("{label}: break-as-full failed: {error}"))?;
        if broken.blocks == clean.blocks && broken.saved_status == clean.saved_status {
            stats
                .mismatches
                .push(format!("{label}: break-as-full was not detected"));
        } else {
            stats.breaks += 1;
        }
    }
    // 光照改坏确认：故意把生成期光照抹成 0，逐格比较器必须报不一致。
    // 只在 Status < LIGHT（会重算光照）的一个用例上做，省一次生成。
    if options.break_light && stage == ChunkStatus::Empty && mode == "none" {
        let broken = resume_chunk(
            &config,
            &ResumeCase {
                pos,
                chunk_nbt: nbt.clone(),
                stage,
            },
            &neighbor_tags,
            Some(ResumeBreak::ClearGeneratedLight),
        )
        .map_err(|error| format!("{label}: break-light failed: {error}"))?;
        let official = mc_bamboo_storage::chunk_nbt::decode_bytes(&full_nbt)
            .map_err(|error| format!("{label}: official full NBT: {error}"))?;
        let mut probe = Stats::default();
        compare_light(&mut probe, &label, &official.column, &broken);
        if probe.mismatches.is_empty() {
            stats
                .mismatches
                .push(format!("{label}: break-light was not detected"));
        } else {
            stats.breaks += 1;
        }
    }
    // `PostProcessing` 改坏确认，两处：
    // * 读回时丢掉盘上的表（FEATURES 存档，表里已是完工标记）；
    // * 续跑 `features` 时用新标记替换而不是追加（TERRAIN 存档，盘上只有地形
    //   标记；地形标记为空的区块替换和追加没有区别，跳过）。
    // Two `PostProcessing` break checks: drop the disk table on read (FEATURES
    // save), and replace instead of append when resuming `features` (TERRAIN
    // save; skipped when the terrain table is empty, where both agree).
    let post_breaks = [
        (
            ChunkStatus::Features,
            ResumeBreak::DiscardPostProcessing,
            true,
        ),
        (
            ChunkStatus::Terrain,
            ResumeBreak::ReplacePostProcessing,
            full.post_processing_terrain
                .iter()
                .any(|section| !section.is_empty()),
        ),
    ];
    for (at, kind, applicable) in post_breaks {
        if !(options.break_post_processing && stage == at && mode == "none" && applicable) {
            continue;
        }
        let broken = resume_chunk(
            &config,
            &ResumeCase {
                pos,
                chunk_nbt: nbt.clone(),
                stage,
            },
            &neighbor_tags,
            Some(kind),
        )
        .map_err(|error| format!("{label}: break-post-processing {kind:?} failed: {error}"))?;
        let mut probe = Stats::default();
        compare_post_processing(
            &mut probe,
            &label,
            &full.post_processing,
            &broken.post_processing,
        );
        if probe.mismatches.is_empty() {
            stats.mismatches.push(format!(
                "{label}: break-post-processing {kind:?} was not detected"
            ));
        } else {
            stats.breaks += 1;
            if kind == ResumeBreak::ReplacePostProcessing {
                stats.replace_breaks += 1;
            }
        }
    }
    if options.break_neighbor && stage == ChunkStatus::StructureStarts && mode == "same" {
        let broken = resume_chunk(
            &config,
            &ResumeCase {
                pos,
                chunk_nbt: nbt,
                stage,
            },
            &neighbor_tags,
            Some(ResumeBreak::DropNeighborWrites),
        )
        .map_err(|error| format!("{label}: break-neighbor failed: {error}"))?;
        if broken.blocks == clean.blocks {
            stats
                .mismatches
                .push(format!("{label}: break-neighbor was not detected"));
        } else {
            stats.breaks += 1;
        }
    }
    Ok(())
}

fn compare_blocks(
    stats: &mut Stats,
    label: &str,
    states: &[u32],
    names: &[String],
    indices: &[u32],
    clean: &mc_bamboo_game::resume_replay::ResumedChunk,
) {
    if indices.len() != clean.blocks.len() * 256 {
        stats.mismatches.push(format!(
            "{label}: official {} blocks vs game {}",
            indices.len(),
            clean.blocks.len() * 256
        ));
        return;
    }
    for (index, &id) in indices.iter().enumerate() {
        let Some(&official) = states.get(id as usize) else {
            stats
                .mismatches
                .push(format!("{label}: palette index {id} out of range"));
            continue;
        };
        let game = clean.blocks[index / 256][index % 256];
        stats.blocks += 1;
        if official != game {
            stats.mismatches.push(format!(
                "{label}: block {index}: official {official} ({}), game {game} ({})",
                names.get(id as usize).map(String::as_str).unwrap_or("?"),
                mc_bamboo_worldgen::feature::state::official_name(game)
            ));
        }
    }
}

fn compare_biomes(
    stats: &mut Stats,
    label: &str,
    expected: &[Vec<String>],
    clean: &mc_bamboo_game::resume_replay::ResumedChunk,
) {
    if expected.len() != clean.biomes.len() {
        stats.mismatches.push(format!(
            "{label}: official {} biome sections vs game {}",
            expected.len(),
            clean.biomes.len()
        ));
        return;
    }
    for (section, (names, cells)) in expected.iter().zip(&clean.biomes).enumerate() {
        for (cell, (&id, name)) in cells.iter().zip(names).enumerate() {
            let game = registries::biome::NAMES
                .get(id as usize)
                .copied()
                .unwrap_or("<unknown>");
            stats.biomes += 1;
            if game != name {
                stats.mismatches.push(format!(
                    "{label}: biome section {section} cell {cell}: official {name}, game {game}"
                ));
            }
        }
    }
}

fn compare_heightmaps(
    stats: &mut Stats,
    label: &str,
    expected: &BTreeMap<String, Vec<i64>>,
    clean: &mc_bamboo_game::resume_replay::ResumedChunk,
) {
    for (name, columns) in expected {
        let key = name.as_str();
        let Some(game) = clean.heightmaps.get(key) else {
            stats
                .mismatches
                .push(format!("{label}: game has no {name} heightmap"));
            continue;
        };
        for (column, (&official, &ours)) in columns.iter().zip(game).enumerate() {
            stats.heightmaps += 1;
            if official != i64::from(ours) {
                stats.mismatches.push(format!(
                    "{label}: {name} column {column}: official {official}, game {ours}"
                ));
            }
        }
    }
}

/// 官方 `saveAdditional` 无条件写出的构造器默认值：只有「官方值恰好是默认值、
/// 且游戏侧缺这个键」时才可以忽略。和 `gen-world` 的表同源。
const BLOCK_ENTITY_DEFAULTS: &[(&str, &str, &str)] = &[
    ("minecraft:vault", "shared_data", "{}"),
    ("minecraft:vault", "server_data", "{}"),
    ("minecraft:furnace", "speed_multiplier", "1.0f"),
    ("minecraft:blast_furnace", "speed_multiplier", "1.0f"),
    ("minecraft:smoker", "speed_multiplier", "1.0f"),
    ("minecraft:sculk_sensor", "last_vibration_frequency", "0"),
    (
        "minecraft:sculk_sensor",
        "listener",
        "{event_delay:0,selector:{tick:-1L}}",
    ),
    (
        "minecraft:calibrated_sculk_sensor",
        "last_vibration_frequency",
        "0",
    ),
    (
        "minecraft:calibrated_sculk_sensor",
        "listener",
        "{event_delay:0,selector:{tick:-1L}}",
    ),
    ("minecraft:brewing_stand", "total_brew_time", "400"),
    ("minecraft:brewing_stand", "total_fuel", "20"),
    ("minecraft:brewing_stand", "speed_multiplier", "1.0f"),
    ("minecraft:sign", "is_waxed", "0b"),
    ("minecraft:hanging_sign", "is_waxed", "0b"),
];

fn block_entity_id(tag: &mc_bamboo_nbt::Tag) -> &str {
    let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
        return "";
    };
    entries
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("id", mc_bamboo_nbt::Tag::String(text)) => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("")
}

fn strip_official_defaults(
    official: &mc_bamboo_nbt::Tag,
    ours: &mut Vec<(String, String)>,
) -> Vec<(String, String)> {
    let id = block_entity_id(official);
    let mc_bamboo_nbt::Tag::Compound(entries) = official else {
        return Vec::new();
    };
    let mut ignored = Vec::new();
    for (key, value) in entries {
        let text = value.to_snbt();
        let default = BLOCK_ENTITY_DEFAULTS
            .iter()
            .any(|(kind, name, expected)| *kind == id && *name == key && *expected == text);
        if !default {
            continue;
        }
        if let Some(index) = ours.iter().position(|(name, _)| name == key) {
            if ours[index].1 == text {
                ours.remove(index);
                ignored.push((key.clone(), text));
            }
        } else {
            ignored.push((key.clone(), text));
        }
    }
    ignored
}

fn compare_block_entities(
    stats: &mut Stats,
    label: &str,
    expected: &[(i32, i32, i32, String)],
    game: &[((i32, i32, i32), mc_bamboo_nbt::Tag)],
) {
    // 官方 SNBT 里有 `id/x/y/z` 与构造器默认值；按 `gen-world` 的办法
    // 剥掉元数据键和「双方都有的默认值」再逐项比。
    let custom = |tag: &mc_bamboo_nbt::Tag| -> Vec<(String, String)> {
        let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
            return vec![("#tag".to_owned(), tag.to_snbt())];
        };
        let mut out: Vec<(String, String)> = entries
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "id" | "x" | "y" | "z"))
            .filter(|(key, value)| {
                *key != "components"
                    || !matches!(value, mc_bamboo_nbt::Tag::Compound(items) if items.is_empty())
            })
            .map(|(key, value)| (key.clone(), value.to_snbt()))
            .collect();
        out.sort();
        out.dedup();
        out
    };
    let mut official: Vec<(String, Vec<(String, String)>)> = expected
        .iter()
        .filter_map(|(x, y, z, snbt)| {
            let tag = mc_bamboo_nbt::parse_snbt(snbt).ok()?;
            let mut keys = custom(&tag);
            strip_official_defaults(&tag, &mut keys);
            Some((format!("{x},{y},{z}"), keys))
        })
        .collect();
    official.sort();
    let mut ours: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for ((x, y, z), tag) in game {
        let mut keys = custom(tag);
        // 游戏侧写了默认值时也一起删掉，避免「一方删一方留」的假差异。
        strip_official_defaults(tag, &mut keys);
        ours.insert(format!("{x},{y},{z}"), keys);
    }
    let ours: Vec<(String, Vec<(String, String)>)> = ours.into_iter().collect();
    stats.block_entities += official.len() as u64;
    if official != ours {
        stats.mismatches.push(format!(
            "{label}: block entities official {official:?}, game {ours:?}"
        ));
    }
}

fn compare_structures(
    stats: &mut Stats,
    label: &str,
    full_nbt: &[u8],
    clean: &mc_bamboo_game::resume_replay::ResumedChunk,
) {
    let Ok(decoded) = mc_bamboo_storage::chunk_nbt::decode_bytes(full_nbt) else {
        stats
            .mismatches
            .push(format!("{label}: official full NBT cannot be decoded"));
        return;
    };
    let official = decoded.structures.to_snbt();
    if official != clean.structures {
        stats.mismatches.push(format!(
            "{label}: structures official {official}, game {}",
            clean.structures
        ));
    }
    // 带结构一组必须真的比到非空 starts / References，避免「结构没生成」的假绿。
    if official.contains("minecraft:") || clean.structures.contains("minecraft:") {
        stats.structure_cases += 1;
    }
}

/// 官方 features 行里的 `PostProcessing` 是 24 段 packed short；解析成同样形状。
///
/// Parse the official features line's 24 packed-short `PostProcessing` sections.
fn packed_sections(value: Option<&Value>) -> Result<Vec<Vec<u16>>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let sections = value
        .as_array()
        .ok_or("PostProcessing is not an array".to_owned())?;
    sections
        .iter()
        .map(|section| {
            section
                .as_array()
                .ok_or("PostProcessing section is not an array".to_owned())?
                .iter()
                .map(|entry| {
                    entry
                        .as_u64()
                        .map(|packed| packed as u16)
                        .ok_or("bad packed PostProcessing short".to_owned())
                })
                .collect()
        })
        .collect()
}

/// 逐段比较完工后的 `PostProcessing`，段内按标记顺序逐项比。
///
/// Compare the finished `PostProcessing` section by section, entry order included.
fn compare_post_processing(
    stats: &mut Stats,
    label: &str,
    official: &[Vec<u16>],
    game: &[Vec<u16>],
) {
    if official.len() != mc_bamboo_world::SECTION_COUNT {
        stats.mismatches.push(format!(
            "{label}: official PostProcessing has {} sections, expected {}",
            official.len(),
            mc_bamboo_world::SECTION_COUNT
        ));
        return;
    }
    if game.len() != official.len() {
        stats.mismatches.push(format!(
            "{label}: game PostProcessing has {} sections, expected {}",
            game.len(),
            official.len()
        ));
        return;
    }
    for (section, (want, have)) in official.iter().zip(game.iter()).enumerate() {
        stats.post_processing += want.len() as u64;
        if want == have {
            continue;
        }
        let first = want
            .iter()
            .zip(have.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(want.len().min(have.len()));
        stats.mismatches.push(format!(
            "{label}: PostProcessing section {section}: official {} entries vs game {}, \
             first diff at {first}: official {:?}, game {:?}",
            want.len(),
            have.len(),
            want.get(first),
            have.get(first)
        ));
    }
}

fn compare_light(
    stats: &mut Stats,
    label: &str,
    expected: &mc_bamboo_world::column::ChunkColumn,
    clean: &mc_bamboo_game::resume_replay::ResumedChunk,
) {
    use mc_bamboo_world::light::{LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos};
    let mut differences = Vec::new();
    for slot in 0..LIGHT_SECTION_COUNT {
        let y = MIN_LIGHT_SECTION + slot as i32;
        for (layer, game_layers) in [
            (mc_bamboo_core::LightLayer::Sky, &clean.sky),
            (mc_bamboo_core::LightLayer::Block, &clean.block_light),
        ] {
            let official = expected
                .section(layer, SectionPos::new(0, y, 0))
                .map(|data| data.bytes().to_vec());
            let game = game_layers.get(slot).cloned().unwrap_or_default();
            match (official, game) {
                (None, None) => {}
                (Some(official), Some(game)) => {
                    for (index, (&a, &b)) in official.iter().zip(&game).enumerate() {
                        stats.light_cells += 1;
                        if a != b {
                            differences.push(format!(
                                "{label}: {layer:?} y {y} byte {index}: official {a}, game {b}"
                            ));
                        }
                    }
                }
                (official, game) => {
                    // 官方 `SerializableChunkData.copyOf` 跳过
                    // `DataLayer.isEmpty()`（没分配数组、默认值 0）的段；
                    // `read` 用 `retainData` 把缺省段恢复成默认 0。Bamboo 为了
                    // 原样往返把存着的 `Filled(0)` 也写成 2048 字节
                    // （见 `docs/m3-storage.md` §5.2）。两边语义相同：逐格都是 0。
                    // 只有「一边没有、另一边有非零光」才是真不一致。
                    // Vanilla copyOf omits empty (all-zero) DataLayers; read()
                    // restores them as default 0. Bamboo persists stored Filled(0)
                    // layers verbatim. Only a non-empty vs absent difference fails.
                    let official_nonzero = official
                        .as_ref()
                        .is_some_and(|bytes| bytes.iter().any(|&b| b != 0));
                    let game_nonzero = game
                        .as_ref()
                        .is_some_and(|bytes| bytes.iter().any(|&b| b != 0));
                    if official_nonzero != game_nonzero {
                        differences.push(format!(
                            "{label}: {layer:?} y {y}: official present {}, game present {} \
                             (official nonzero {official_nonzero}, game nonzero {game_nonzero})",
                            official.is_some(),
                            game.is_some(),
                        ));
                    }
                }
            }
        }
    }
    if expected.light_on() != clean.light_on {
        stats.mismatches.push(format!(
            "{label}: isLightOn official {}, game {}",
            expected.light_on(),
            clean.light_on
        ));
    }
    // 光照修正合入后，所有阶段都必须逐格等于官方；任何差异都是真不一致。
    stats.mismatches.extend(differences);
}

fn parse_neighbor_tags(case: &Value) -> Result<BTreeMap<ChunkPos, Tag>, String> {
    let mut out = BTreeMap::new();
    let Some(entries) = case.get("neighbor_tags").and_then(Value::as_array) else {
        return Ok(out);
    };
    for entry in entries {
        let x = entry.get("x").and_then(Value::as_i64).ok_or("neighbor x")? as i32;
        let z = entry.get("z").and_then(Value::as_i64).ok_or("neighbor z")? as i32;
        let structures = entry
            .get("structures")
            .and_then(Value::as_object)
            .ok_or("neighbor structures")?;
        let mut fields = Vec::new();
        for (key, value) in structures {
            let snbt = value.as_str().ok_or("neighbor structure SNBT")?;
            let tag = mc_bamboo_nbt::parse_snbt(snbt).map_err(|e| e.to_string())?;
            fields.push((key.clone(), tag));
        }
        out.insert(ChunkPos::new(x, z), Tag::Compound(fields));
    }
    Ok(out)
}

fn decode_base64(value: &Value) -> Result<Vec<u8>, String> {
    let text = value.as_str().ok_or("base64 value must be a string")?;
    crate::storage_oracle::decode_b64(text)
}
