//! `gen-world`：官方生成和「游戏里用的那份生成代码」逐区块对拍。
//!
//! `ChunkgenOracle.java --through carvers` 在 JVM 里把一批（种子，区块）跑到
//! `terrain`（CARVERS），输出 quart 群系和方块。Bamboo 侧不直接调 `mc-bamboo-worldgen`，
//! 而是调 `mc-bamboo-game::worldgen::generate_chunk` —— 也就是 `OverworldGenerator`
//! 在游戏里走 `Staged` 流水线时用的同一份纯函数，外加把结果组装成 `RegionColumn`
//! 的那一步。两边逐 quart 格、逐方块比较，0 不一致才算过。
//!
//! 地物、结构、光照还没移植：Java 侧 `--through carvers` 本来就不跑它们，游戏侧
//! 的 `features` / `initialize_light` / `light` / `spawn` 阶段本轮也是透传，两边
//! 用同样的边界（CARVERS 快照）比较，符合 `--through` / `--steps` 的做法。
//! `--level-type` 把 `normal` / `large_biomes` / `amplified` 三种噪声世界逐个对拍。
//!
//! `gen-world`: compare vanilla generation against the SAME code the game runs.
//! `ChunkgenOracle.java --through carvers` emits quart biomes and blocks for a set
//! of (seed, chunk) to `terrain` (CARVERS). Bamboo calls
//! `mc-bamboo-game::worldgen::generate_chunk` — the exact pure function
//! `OverworldGenerator` uses on the `Staged` pipeline — plus the column assembly.
//! Features / structures / light are not ported: the Java side stops at CARVERS
//! and the game's `features` / light / spawn stages are pass-throughs, so both
//! compare the same CARVERS snapshot (the `--through` convention).
//! `--level-type` compares each of `normal` / `large_biomes` / `amplified`.

use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
};

use mc_bamboo_data::{block_by_name, registries, version};
use mc_bamboo_game::{
    world_options::{LevelType, WorldGenConfig},
    worldgen::GeneratedChunk,
};
use mc_bamboo_worldgen::feature::state::state_from_official_name;
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: gen-world <java> [--seeds N] [--chunks N] [--salt S] [--chunk X Z] [--scan N] \
     [--show N] [--level-type normal|large_biomes|amplified] [--break-self-test] \
     [--through features [--structures] [--feature-types IDS | --types IDS | --all-features] \
      [--steps MASK] [--coverage] [--break-type KIND] [--structure-types IDS] \
      [--break-structure KIND] [--structure-coverage]]";
const DEFAULT_SHOW: usize = 8;
/// 全部 11 个 decoration step（和 `chunkgen.rs` / Java 侧的 `ALL_STEPS` 一致）。
const ALL_STEPS: i32 = (1 << 11) - 1;

#[derive(Debug)]
struct Options {
    forwarded: Vec<String>,
    show: usize,
    /// 对拍哪种噪声世界；三种共用 overworld 群系预设，只换 NoiseGeneratorSettings。
    /// Which noise world to compare; all three share the overworld biome preset and
    /// differ only in NoiseGeneratorSettings.
    level_type: LevelType,
    /// 故意把 Bamboo 的第一个方块改坏，确认对拍会报不一致（自检）。
    /// Deliberately corrupt the first Bamboo block to verify the oracle fails (self-test).
    break_self_test: bool,
    /// 跑到 `features`：Java 侧和游戏侧用同一份地物白名单，逐方块比 9 宫格。
    /// 不传 `--feature-types` 时仍默认只放矿石，保持旧命令的语义。
    /// Run through `features`: both sides use the same allow list and compare the
    /// 9-chunk window block by block. Without `--feature-types` only ores run,
    /// preserving the old command's meaning.
    features: bool,
    /// `--feature-types` / `--types` 的白名单；`None` 等价于 `--ore-only`，
    /// `Some(空)` 表示 `--all-features`（不过滤）。
    /// The allow list; `None` means `--ore-only`, `Some(empty)` means
    /// `--all-features` (no filter).
    feature_types: Option<Vec<String>>,
    /// Java 侧的 `--steps` 掩码，逐类型对拍时只跑相关 step。
    /// The Java `--steps` mask; per-type runs narrow this to the relevant steps.
    steps: i32,
    /// 打印每个 Feature 类型实际跑到的候选次数（触发统计）。
    /// Print per-Feature-type candidate counts (coverage report).
    coverage: bool,
    /// 改坏确认：只改这个类型写下的方块。空表示不改。
    /// Break confirmation: corrupt blocks written only by this Feature type.
    break_type: Option<String>,
    /// `--structures`：Java 侧跑官方结构起点 / 引用 / 真 Beardifier / 带结构的
    /// `applyBiomeDecoration`，游戏侧同步打开 `generate_structures`。
    /// Turn vanilla structures on, on both sides of the comparison.
    structures: bool,
    /// 带结构模式下的覆盖统计：打印每种结构放置的结构件数与方块数。
    /// Print per-structure piece / block counts in structures mode.
    structure_coverage: bool,
    /// 带结构模式下的改坏开关：跳过一个特定结构类型的结构件放置。
    /// Break confirmation in structures mode: skip one structure type.
    break_structure: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            // 固定跑到 carvers，并走真实服务端路径（3×3 possibleBiomes、carverBiome、
            // region.getBiomeManager()），Java 侧会额外输出各张高度图。
            // Always go through carvers on the real-server path (3x3 possibleBiomes,
            // carverBiome, region.getBiomeManager()); Java also emits all heightmaps.
            forwarded: vec![
                "--through".to_owned(),
                "carvers".to_owned(),
                "--real-path".to_owned(),
            ],
            show: DEFAULT_SHOW,
            level_type: LevelType::Normal,
            break_self_test: false,
            features: false,
            feature_types: None,
            steps: ALL_STEPS,
            coverage: false,
            break_type: None,
            structures: false,
            structure_coverage: false,
            break_structure: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--seeds" | "--chunks" | "--salt" | "--scan" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                // 指定区块：官方那边每个种子只出这一个区块。
                // A fixed chunk: the oracle emits just this chunk for every seed.
                "--chunk" => {
                    let x = args.next().ok_or(USAGE)?;
                    let z = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*x).to_owned(), (*z).to_owned()]);
                }
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--level-type" => {
                    let value = args.next().ok_or(USAGE)?;
                    let level = LevelType::from_level_id(value)
                        .filter(|l| !l.is_flat())
                        .ok_or(USAGE)?;
                    let settings = level
                        .settings_id()
                        .and_then(|id| id.strip_prefix("minecraft:"))
                        .ok_or(USAGE)?;
                    options.level_type = level;
                    options
                        .forwarded
                        .extend(["--settings".to_owned(), settings.to_owned()]);
                }
                "--break-self-test" => options.break_self_test = true,
                // 地物白名单：转发给 Java，游戏侧用同一组字符串。
                // Feature allow list: forwarded to Java; the game uses the same strings.
                "--types" | "--feature-types" | "--steps" => {
                    let value = args.next().ok_or(USAGE)?;
                    if !options.features {
                        return Err(format!("{arg} requires --through features: {USAGE}"));
                    }
                    if arg == "--steps" {
                        let mut mask = 0_i32;
                        for part in value.split(',') {
                            let step: i32 = part.trim().parse().map_err(|_| USAGE)?;
                            if !(0..11).contains(&step) {
                                return Err(USAGE.to_owned());
                            }
                            mask |= 1 << step;
                        }
                        options.steps = mask;
                    } else {
                        options.feature_types = Some(
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                                .collect(),
                        );
                    }
                    // `--steps` 直接转发；白名单收尾时统一转发一次，避免
                    // `--types` 和 `--feature-types` 同时出现时追加两条。
                    // Forward `--steps` now; the allow list is forwarded once at
                    // the end so repeated / aliased flags cannot produce two.
                    if arg == "--steps" {
                        options
                            .forwarded
                            .extend(["--steps".to_owned(), (*value).to_owned()]);
                    }
                }
                "--coverage" => {
                    if !options.features {
                        return Err(format!("--coverage requires --through features: {USAGE}"));
                    }
                    options.coverage = true;
                }
                "--break-type" => {
                    if !options.features {
                        return Err(format!("--break-type requires --through features: {USAGE}"));
                    }
                    options.break_type = Some((*args.next().ok_or(USAGE)?).to_owned());
                    options.coverage = true;
                }
                "--all-features" => {
                    if !options.features {
                        return Err(format!(
                            "--all-features requires --through features: {USAGE}"
                        ));
                    }
                    options.feature_types = Some(Vec::new());
                }
                // 带结构对拍：默认无结构模式完全不变，只有显式开关才改两边。
                // Structures mode is opt-in; the default no-structure oracle is unchanged.
                "--structures" => {
                    if !options.features {
                        return Err("--structures requires --through features".to_owned());
                    }
                    options.structures = true;
                    options.structure_coverage = true;
                    // 带结构模式下 Java 和游戏都跑全部 11 步、全部地物；白名单
                    // 只用来做改坏确认，不能把结构件之外的地物一起关掉。
                    options.feature_types = Some(Vec::new());
                }
                "--structure-coverage" => {
                    if !options.features {
                        return Err("--structure-coverage requires --through features".to_owned());
                    }
                    options.structure_coverage = true;
                }
                "--break-structure" => {
                    if !options.features {
                        return Err("--break-structure requires --through features".to_owned());
                    }
                    options.break_structure = Some((*args.next().ok_or(USAGE)?).to_owned());
                    options.structure_coverage = true;
                }
                "--through" => {
                    let value = args.next().ok_or(USAGE)?;
                    if *value != "features" {
                        return Err(format!(
                            "gen-world only supports --through features: {USAGE}"
                        ));
                    }
                    options.features = true;
                    // 原地替换头两个 `--through carvers`，不动可能已经追加的
                    // `--seeds/--chunks/--salt`。
                    // Replace the leading `--through carvers` in place, keeping
                    // any size args already appended.
                    options.forwarded[1] = "features".to_owned();
                    // Java 侧 `--real-path` 优先于 `--through features`，地物模式不能带它。
                    // Java's `--real-path` takes precedence over `--through features`,
                    // so features mode must not forward it.
                    options.forwarded.retain(|arg| arg != "--real-path");
                    // 没给白名单时沿用旧行为（只放矿石）。显式白名单要在收尾时
                    // 把这条默认过滤替换掉，Java 侧不能同时收到两个过滤开关。
                    // Without an allow list keep the old ores-only behaviour. An
                    // explicit one replaces this default flag at the end of parsing.
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        if options.structures {
            options.forwarded.push("--structures".to_owned());
        }
        if options.features {
            match &options.feature_types {
                // 显式白名单：原样转发逗号分隔的条目（Java 侧也接 `--feature-types`）。
                // Explicit allow list: forward the comma-separated selectors.
                Some(selectors) if !selectors.is_empty() => {
                    options
                        .forwarded
                        .extend(["--feature-types".to_owned(), selectors.join(",")]);
                }
                // `--all-features` 不转发过滤开关；`None` 是旧的默认矿石模式。
                // `--all-features` forwards nothing; `None` keeps the legacy ores-only mode.
                Some(_) => {}
                None => options.forwarded.push("--ore-only".to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Stats {
    chunks: u64,
    biome_cells: u64,
    blocks: u64,
    /// 比较过的方块实体数量（官方侧与游戏侧都有）。
    /// Block entities compared (present on both the official and game sides).
    block_entities: u64,
    heightmap_columns: u64,
    heightmap_words: u64,
    mismatches: Vec<String>,
    /// 改坏确认里实际被改掉、且确实由目标类型写过的格数。
    /// Cells actually corrupted during break confirmation, all written by the target type.
    break_writes: u64,
    /// 带结构对拍里比较过的结构统计行数。
    /// Structure coverage rows compared in structures mode.
    coverage_rows: u64,
    /// 带结构对拍里 Java 侧报告的结构件总数。
    /// Total structure pieces reported by the Java oracle.
    java_pieces: u64,
}

/// 启动官方进程，一边读一边把收到的线和游戏侧生成结果比较。
/// `command` 是已经指向 `ChunkgenOracle.java` 的命令。
///
/// Launch the official process and compare each line against the in-game generator.
/// `command` already points at `ChunkgenOracle.java`.
pub fn run(command: Command, args: &[&str]) -> Result<(), String> {
    run_with_queries(command, args, None)
}

/// 带结构覆盖脚本用：额外提供一个 `seed cx cz` 查询文件，Java 与游戏侧按同一批
/// 坐标逐个对拍。默认路径（无查询文件）完全走原来的 `--seeds/--chunks` 出题。
///
/// Structures coverage driver: supply a `seed cx cz` query file so both sides
/// compare the same coordinates. Without it the usual seed/chunk generator runs.
pub fn run_with_queries(
    mut command: Command,
    args: &[&str],
    queries: Option<&Path>,
) -> Result<(), String> {
    let options = Options::parse(args)?;
    if let Some(path) = queries {
        command.arg("--chunks-file").arg(path);
    }
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        pending_biomes: HashMap::new(),
        stats: Stats::default(),
        show: options.show,
        level_type: options.level_type,
        break_self_test: options.break_self_test,
        features: options.features,
        feature_types: options.feature_types.clone(),
        steps: options.steps,
        coverage: options.coverage,
        break_type: options.break_type.clone(),
        structures: options.structures,
        structure_coverage: options.structure_coverage,
        break_structure: options.break_structure.clone(),
        structure_totals: BTreeMap::new(),
        pending_coverage: HashMap::new(),
        coverage_totals: BTreeMap::new(),
        feature_kinds: if options.coverage {
            feature_kinds()?
        } else {
            Vec::new()
        },
        header_ok: false,
        official_checks: 0,
    };
    let read = report.read_lines(BufReader::new(stdout));
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

struct Report {
    /// `(seed, x, z)` → 该区块的 quart 群系，等对应的 carvers 线到了再一起比。
    /// `(seed, x, z)` → quart biomes, compared when the matching carvers line arrives.
    pending_biomes: HashMap<(i64, i32, i32), Vec<Vec<String>>>,
    stats: Stats,
    show: usize,
    level_type: LevelType,
    break_self_test: bool,
    /// 跑到 features：读 `features` 线、调 `game_features` 比 9 宫格。
    /// Features mode: read `features` lines, compare the 9-chunk window via `game_features`.
    features: bool,
    /// 地物白名单；`None` 对应 Java 的 `--ore-only`。
    /// Feature allow list; `None` corresponds to Java's `--ore-only`.
    feature_types: Option<Vec<String>>,
    steps: i32,
    coverage: bool,
    /// 改坏确认：只改这个类型写下的方块。
    /// Break confirmation: corrupt blocks written only by this type.
    break_type: Option<String>,
    /// 带结构对拍：Java 侧与游戏侧都开结构。
    /// Structures mode: both sides enable structures.
    structures: bool,
    /// 打印每种结构放置的结构件数与方块数。
    /// Print per-structure piece / block counts.
    structure_coverage: bool,
    /// 带结构改坏确认：跳过这个结构类型的结构件放置。
    /// Structures-mode break confirmation: skip this structure type.
    break_structure: Option<String>,
    /// 结构类型 ID → (结构件数, 结构件写下的方块数)。
    /// Structure type → (placed pieces, blocks written by those pieces).
    structure_totals: BTreeMap<String, (u64, u64)>,
    /// `(seed,x,z)` → 上一条 coverage 行里的起点结构件总数。
    /// `(seed,x,z)` → piece count in the last coverage line.
    pending_coverage: HashMap<(i64, i32, i32), u64>,
    /// 每个 Feature 类型 ID 在这批区块里实际交给 `Feature.place` 的候选次数。
    /// Per-Feature-type candidate count actually handed to `Feature.place`.
    coverage_totals: BTreeMap<String, u64>,
    /// `Registry::placed` 下标 → Feature 类型 ID，按导出表顺序。留空表示不统计。
    /// `Registry::placed` index → Feature type ID; empty when not collecting coverage.
    feature_kinds: Vec<String>,
    header_ok: bool,
    official_checks: u64,
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let value: Value =
                serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
            let kind = value
                .get("type")
                .and_then(Value::as_str)
                .ok_or("oracle line has no type")?;
            match kind {
                "header" => self.check_header(&value)?,
                // features 模式下 biomes 线没有对应的 carvers 线，忽略。
                // In features mode the biomes line has no matching carvers line.
                "biomes" if self.features => {}
                "biomes" => self.on_biomes(&value)?,
                // noise / surface 是 CARVERS 之前的中间快照，游戏侧没有，跳过。
                // noise / surface are pre-CARVERS snapshots the game side lacks; skip.
                "noise" | "surface" => {}
                "carvers" if self.features => {
                    return Err("features-mode oracle emitted a carvers line".to_owned());
                }
                "carvers" => self.on_carvers(&value)?,
                "features" if self.features => self.on_features(&value)?,
                "coverage" if self.features => self.on_coverage(&value)?,
                "end" => {
                    self.official_checks = value.get("checks").and_then(Value::as_u64).unwrap_or(0);
                }
                other => return Err(format!("unexpected oracle line type {other}")),
            }
        }
        if !self.header_ok {
            return Err("oracle did not emit a header".to_owned());
        }
        if !self.features && !self.pending_biomes.is_empty() {
            return Err(format!(
                "{} biomes lines had no matching carvers line",
                self.pending_biomes.len()
            ));
        }
        Ok(())
    }

    fn check_header(&mut self, value: &Value) -> Result<(), String> {
        let get_str = |key: &str| -> Result<&str, String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("header missing {key}"))
        };
        let get_i64 = |key: &str| -> Result<i64, String> {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("header missing {key}"))
        };
        let reported = get_str("version")?;
        let protocol = get_i64("protocol")?;
        if reported != version::GAME_VERSION || protocol != i64::from(version::PROTOCOL_VERSION) {
            return Err(format!(
                "oracle is {reported}/{protocol}, bamboo is {}/{}",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION
            ));
        }
        // 三种噪声世界共用 minecraft:overworld 群系预设，只换 NoiseGeneratorSettings；
        // header 里的 settings 必须等于这次对拍选的世界类型。
        // All three noise worlds share the minecraft:overworld biome preset and differ
        // only in NoiseGeneratorSettings; the header settings must match the level type.
        let settings_id = self
            .level_type
            .settings_id()
            .ok_or("flat has no noise settings")?;
        if get_str("settings")? != settings_id || get_str("preset")? != "minecraft:overworld" {
            return Err(format!(
                "gen-world expects {settings_id} / minecraft:overworld"
            ));
        }
        let min_y = get_i64("minY")?;
        let height = get_i64("height")?;
        let sea_level = get_i64("seaLevel")?;
        let settings = mc_bamboo_worldgen::Worldgen::load()?
            .settings(settings_id)
            .map(|s| (s.min_y, s.height, s.sea_level))
            .ok_or("missing noise settings")?;
        if (min_y, height, sea_level)
            != (
                i64::from(settings.0),
                i64::from(settings.1),
                i64::from(settings.2),
            )
        {
            return Err(format!(
                "settings differ: official {min_y}/{height}/{sea_level}, bamboo {}/{}/{}",
                settings.0, settings.1, settings.2
            ));
        }
        let through = get_i64("through")?;
        // carvers=3, features=4（和 chunkgen.rs 的 header 约定一致）。
        // carvers=3, features=4 (same header convention as chunkgen.rs).
        let expected = if self.features { 4 } else { 3 };
        if through != expected {
            return Err(format!(
                "oracle ran through {through}, gen-world needs {expected}"
            ));
        }
        let structures = value
            .get("structures")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if structures != self.structures {
            return Err(format!(
                "oracle structures={structures}, gen-world structures={}",
                self.structures
            ));
        }
        self.header_ok = true;
        Ok(())
    }

    fn on_biomes(&mut self, value: &Value) -> Result<(), String> {
        let key = line_key(value)?;
        let sections = value
            .get("sections")
            .and_then(Value::as_array)
            .ok_or("biomes line missing sections")?;
        let mut parsed = Vec::with_capacity(sections.len());
        for section in sections {
            let cells = section.as_array().ok_or("biomes section is not an array")?;
            parsed.push(
                cells
                    .iter()
                    .map(|cell| {
                        cell.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "biome cell is not a string".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        self.pending_biomes.insert(key, parsed);
        Ok(())
    }

    fn on_carvers(&mut self, value: &Value) -> Result<(), String> {
        let key = line_key(value)?;
        let (seed, x, z) = key;
        let biomes = self
            .pending_biomes
            .remove(&key)
            .ok_or_else(|| format!("carvers line for ({seed},{x},{z}) has no biomes line"))?;
        let palette = value
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("carvers line missing palette")?;
        let block_indices = value
            .get("blocks")
            .and_then(Value::as_array)
            .ok_or("carvers line missing blocks")?;

        let config = WorldGenConfig {
            seed,
            level_type: self.level_type,
            ..WorldGenConfig::default()
        };
        let mut generated =
            mc_bamboo_game::worldgen::generate_chunk(&config, x, z).map_err(|error| {
                format!("game generation failed for seed {seed} ({x},{z}): {error}")
            })?;
        if self.break_self_test {
            // 自检：改坏一个方块、一张高度图的一列，以及一个高层方块（让从 blocks
            // 重算的 FINAL words 也变化），确认三个比较器都会报不一致。
            // Self-test: corrupt one block, one heightmap column, and one high block
            // (so the FINAL words recomputed from blocks change too); all three
            // comparators must then report mismatches.
            generated.blocks[0][0] = mc_bamboo_data::blocks::LAVA;
            generated.blocks[200][0] = mc_bamboo_data::blocks::STONE;
            if let Some(first) = generated.heightmaps.values_mut().next() {
                first[0] = first[0].wrapping_add(1);
            }
        }
        self.stats.chunks += 1;
        compare_biomes(&mut self.stats, seed, x, z, &biomes, &generated);
        compare_blocks(
            &mut self.stats,
            seed,
            x,
            z,
            palette,
            block_indices,
            &generated,
        )?;
        // 真实路径一定带高度图；缺了就当对拍配置错误，不静默跳过。
        // The real-server path always emits heightmaps; a missing field is an error.
        let columns = value
            .get("heightmaps")
            .and_then(Value::as_object)
            .ok_or("carvers line missing heightmaps")?;
        compare_heightmaps(&mut self.stats, seed, x, z, columns, &generated);
        let words = value
            .get("heightmap_words")
            .and_then(Value::as_object)
            .ok_or("carvers line missing heightmap_words")?;
        let ours = mc_bamboo_game::worldgen::final_heightmap_words(&generated)
            .map_err(|error| format!("cannot assemble column seed {seed} ({x},{z}): {error}"))?;
        compare_heightmap_words(&mut self.stats, seed, x, z, words, &ours);
        Ok(())
    }

    /// `--through features`：官方一行是中心 `(x,z)` 加 3×3 九宫格，每格带自己的
    /// 调色板和方块数组。游戏侧调 `game_features`（同一份 `run_features`），把
    /// 9 宫格逐方块比过去；`--break-self-test` 改坏第一格确认比较器会响。
    ///
    /// `--through features`: one line is the center `(x,z)` plus a 3×3 window,
    /// each cell with its own palette and block array. Call `game_features`
    /// (the same `run_features`) and compare all 9 cells block by block;
    /// `--break-self-test` corrupts one cell to prove the detector works.
    fn on_features(&mut self, value: &Value) -> Result<(), String> {
        let (seed, x, z) = line_key(value)?;
        let steps = value.get("steps").and_then(Value::as_i64).unwrap_or(0);
        if steps != i64::from(self.steps) {
            return Err(format!(
                "features seed {seed} ({x},{z}): oracle ran steps mask {steps}, expected {}",
                self.steps
            ));
        }
        let chunks = value
            .get("chunks")
            .and_then(Value::as_array)
            .ok_or("features line missing chunks")?;
        if chunks.len() != 9 {
            return Err(format!(
                "features seed {seed} ({x},{z}): {} chunks vs 9",
                chunks.len()
            ));
        }

        let mut config = WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        // 默认模式 Java 侧关掉结构分支、terrain 走 `Beardifier.EMPTY`；
        // `--structures` 则两边都跑官方结构起点 / 引用 / 真 Beardifier 与带结构的
        // `applyBiomeDecoration`，游戏侧 `game_features_with_mask` 走同一份
        // `build_view` + `run_features_filtered`。
        // The default mode disables structures and uses Beardifier.EMPTY;
        // `--structures` runs official starts/references/Beardifier on both sides.
        config.generate_structures = self.structures;
        // 和 Java 侧的 `--ore-only` / `--feature-types` 用同一份白名单。
        // Same allow list as Java's `--ore-only` / `--feature-types`.
        let ore_only = ["minecraft:ore".to_owned()];
        let selectors: Option<&[String]> = match &self.feature_types {
            Some(list) if list.is_empty() => None,
            Some(list) => Some(list),
            None => Some(&ore_only),
        };
        mc_bamboo_worldgen::perf::begin(-64, i32::MIN, mc_bamboo_worldgen::perf::Phase::Features);
        let mut result = mc_bamboo_game::worldgen_features::game_features_with_mask_skipping(
            &config,
            x,
            z,
            selectors,
            self.steps,
            self.break_structure.as_deref(),
        )
        .map_err(|error| format!("game features failed for seed {seed} ({x},{z}): {error}"))?;
        if self.coverage {
            let record = mc_bamboo_worldgen::perf::finish().ok_or("missing perf record")?;
            for feature in &record.features {
                let kind = self
                    .feature_kinds
                    .get(feature.placed as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("placed#{}", feature.placed));
                *self.coverage_totals.entry(kind).or_default() += feature.candidates;
            }
        } else {
            let _ = mc_bamboo_worldgen::perf::finish();
        }
        if self.break_self_test {
            // 自检：把九宫格第一格的第一个方块改成岩浆，确认下面会报不一致。
            // Self-test: corrupt one block so the comparison must report a mismatch.
            result.blocks[0][0][0] = mc_bamboo_data::blocks::LAVA;
        }
        if let Some(kind) = &self.break_type {
            // 改坏确认：单独跑这个类型，和「一个地物都不跑」的 CARVERS 基线比，
            // 找到它真正写过的格子，把那一格改坏。随机数按 (index, step) 播种，
            // 白名单只影响跑哪些 index，所以单独跑的结果和它在完整跑里写的位置一致。
            // 只改实际写过的格子，才能抓住「这个类型根本没被比到」的假绿。
            // Break confirmation: run this type alone and diff it against a bare
            // CARVERS run to locate a cell it actually wrote, then corrupt it.
            // Feature seeds depend only on (index, step), so an allow list does
            // not shift the writes of the types it keeps. Corrupting a cell the
            // type really wrote is what exposes a false green.
            let only = [kind.clone()];
            let empty: [String; 0] = [];
            let target = mc_bamboo_game::worldgen_features::game_features_with_mask(
                &config,
                x,
                z,
                Some(&only),
                self.steps,
            )
            .map_err(|error| format!("break run failed for seed {seed} ({x},{z}): {error}"))?;
            let bare = mc_bamboo_game::worldgen_features::game_features_with_mask(
                &config,
                x,
                z,
                Some(&empty),
                self.steps,
            )
            .map_err(|error| format!("bare run failed for seed {seed} ({x},{z}): {error}"))?;
            let marker = mc_bamboo_data::blocks::LAVA;
            'outer: for (chunk, (after, before)) in
                target.blocks.iter().zip(bare.blocks.iter()).enumerate()
            {
                for (layer, (after, before)) in after.iter().zip(before.iter()).enumerate() {
                    for (cell, (&after, &before)) in after.iter().zip(before.iter()).enumerate() {
                        if after != before && after != marker {
                            result.blocks[chunk][layer][cell] = marker;
                            self.stats.break_writes += 1;
                            break 'outer;
                        }
                    }
                }
            }
        }
        // 未移植的分支会让整个区块标 unsupported；此时 0 不一致是假绿，必须报错。
        // An unported branch marks the whole chunk unsupported; 0 mismatches would
        // then be a false green, so fail loudly instead.
        if result.unsupported {
            return Err(format!(
                "features seed {seed} ({x},{z}): bamboo reported unsupported features"
            ));
        }
        self.stats.chunks += 1;
        if self.structure_coverage {
            self.collect_structure_coverage(seed, x, z, &result)?;
        }

        for (index, chunk) in chunks.iter().enumerate() {
            let dz = index as i32 / 3 - 1;
            let dx = index as i32 % 3 - 1;
            let reported_dx = chunk.get("dx").and_then(Value::as_i64).unwrap_or(99);
            let reported_dz = chunk.get("dz").and_then(Value::as_i64).unwrap_or(99);
            if reported_dx != i64::from(dx) || reported_dz != i64::from(dz) {
                return Err(format!(
                    "features seed {seed} ({x},{z}) index {index}: ({reported_dx},{reported_dz}) vs ({dx},{dz})"
                ));
            }
            let palette = chunk
                .get("palette")
                .and_then(Value::as_array)
                .ok_or("features chunk missing palette")?;
            let indices = chunk
                .get("blocks")
                .and_then(Value::as_array)
                .ok_or("features chunk missing blocks")?;
            // 复用 CARVERS 的逐方块比较器，但地物会放带属性的状态
            // （水、原木、楼梯、藤蔓……），必须精确解析调色板而不是取默认状态。
            // Reuse the CARVERS comparator, but features place states with
            // properties, so the palette must be parsed exactly.
            let generated = GeneratedChunk {
                blocks: result.blocks[index].clone(),
                biomes: Vec::new(),
                heightmaps: Default::default(),
            };
            compare_blocks_exact(
                &mut self.stats,
                seed,
                x + dx,
                z + dz,
                palette,
                indices,
                &generated,
            )?;
            compare_block_entities(
                &mut self.stats,
                seed,
                x + dx,
                z + dz,
                chunk.get("block_entities"),
                &result.block_entities,
            )?;
        }
        Ok(())
    }

    /// Java 侧报告中心区块官方生成的起点和引用；逐项与游戏侧同一条
    /// `structure_starts` / `structure_references` 纯函数比，避免「只比方块、
    /// 结构件根本没生成」的假绿。
    ///
    /// Compare vanilla's starts/references with the game's pure structure stages
    /// so a block-only pass cannot hide missing structures.
    fn on_coverage(&mut self, value: &Value) -> Result<(), String> {
        let (seed, x, z) = line_key(value)?;
        let config = WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let data = mc_bamboo_game::worldgen_structures::structure_data(&config, x, z)
            .map_err(|error| format!("game structure_data failed for ({seed},{x},{z}): {error}"))?;
        let mut want_starts: Vec<(String, usize)> = Vec::new();
        let mut total = 0_u64;
        for entry in value
            .get("starts")
            .and_then(Value::as_array)
            .ok_or("coverage line missing starts")?
        {
            let row = entry.as_array().ok_or("start entry is not an array")?;
            let name = row
                .first()
                .and_then(Value::as_str)
                .ok_or("bad start name")?
                .to_owned();
            let count = row
                .get(1)
                .and_then(Value::as_u64)
                .ok_or("bad start count")? as usize;
            total += count as u64;
            want_starts.push((name, count));
        }
        want_starts.sort();
        let mut rust_starts: Vec<(String, usize)> = data
            .starts
            .by_structure
            .iter()
            .map(|(name, start)| {
                let count = match &start.pieces {
                    // 官方海底神殿只有 `MonumentBuilding` 一件；富模型摊开了房间图子件。
                    // Vanilla's ocean monument has only `MonumentBuilding`; the rich
                    // model expands the room-graph children into separate pieces.
                    mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(_)
                        if name == "minecraft:monument" =>
                    {
                        1
                    }
                    mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(p) => p.len(),
                    mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(p) => p.len(),
                    mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(p) => p.len(),
                };
                (name.clone(), count)
            })
            .collect();
        rust_starts.sort();
        if rust_starts != want_starts {
            self.stats.mismatches.push(format!(
                "coverage ({seed},{x},{z}) starts: ours {rust_starts:?} vs {want_starts:?}"
            ));
        }
        let mut want_refs: Vec<(String, i64)> = Vec::new();
        for entry in value
            .get("references")
            .and_then(Value::as_array)
            .ok_or("coverage line missing references")?
        {
            let row = entry.as_array().ok_or("reference entry is not an array")?;
            want_refs.push((
                row.first()
                    .and_then(Value::as_str)
                    .ok_or("bad reference name")?
                    .to_owned(),
                row.get(1)
                    .and_then(Value::as_i64)
                    .ok_or("bad reference key")?,
            ));
        }
        want_refs.sort();
        let mut rust_refs: Vec<(String, i64)> = data
            .references
            .iter()
            .flat_map(|(name, keys)| keys.iter().map(move |&key| (name.clone(), key)))
            .collect();
        rust_refs.sort();
        if rust_refs != want_refs {
            self.stats.mismatches.push(format!(
                "coverage ({seed},{x},{z}) references: ours {rust_refs:?} vs {want_refs:?}"
            ));
        }
        // 结构件清单（结构名、类型、六点盒）逐条比，和 structure-pipeline 同口径。
        let mut want_pieces: Vec<(String, String, [i32; 6])> = Vec::new();
        for entry in value
            .get("pieces")
            .and_then(Value::as_array)
            .ok_or("coverage line missing pieces")?
        {
            let row = entry.as_array().ok_or("piece entry is not an array")?;
            let mut bb = [0_i32; 6];
            for (i, target) in bb.iter_mut().enumerate() {
                *target = row
                    .get(i + 2)
                    .and_then(Value::as_i64)
                    .ok_or("bad piece box")? as i32;
            }
            want_pieces.push((
                row.first()
                    .and_then(Value::as_str)
                    .ok_or("bad piece structure")?
                    .to_owned(),
                row.get(1)
                    .and_then(Value::as_str)
                    .ok_or("bad piece type")?
                    .to_owned(),
                bb,
            ));
        }
        // 模板结构件（雪屋 / 沉船 / 海底遗迹 / 埋藏的宝藏）在 `postProcess` 里
        // 会把 Y 从布局阶段的占位值改到地表 / 海床，所以官方跑完 FEATURES 后的
        // `getBoundingBox` 与布局阶段的盒只有 XZ 一致。覆盖行只比 XZ 与结构件
        // 类型，Y 留给逐方块比较去验。
        // Templated pieces re-anchor Y during `postProcess`, so only XZ and the
        // piece type are comparable here; the blocks comparison covers Y.
        let normalize = |pieces: &mut Vec<(String, String, [i32; 6])>| {
            for (_, _, bb) in pieces.iter_mut() {
                bb[1] = 0;
                bb[4] = 0;
            }
            pieces.sort();
        };
        normalize(&mut want_pieces);
        let mut rust_pieces: Vec<(String, String, [i32; 6])> = Vec::new();
        for (name, start) in &data.starts.by_structure {
            match &start.pieces {
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(pieces) => {
                    if name == "minecraft:monument" {
                        // 官方 `PiecesContainer` 里只有 `MonumentBuilding` 一件，
                        // 房间图子件是它的私有字段；富模型把它们摊成了多件。
                        // 覆盖行按官方口径折回一件（包围盒取并集），逐方块比较
                        // 才真正验证这些子件。
                        // Vanilla's `PiecesContainer` holds only `MonumentBuilding`;
                        // the room-graph children are private. Fold the rich model
                        // back into one piece (union box); the block comparison
                        // verifies the children for real.
                        let mut bb = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
                        for p in pieces {
                            for i in 0..3 {
                                bb[i] = bb[i].min(p.bb[i]);
                                bb[i + 3] = bb[i + 3].max(p.bb[i + 3]);
                            }
                        }
                        rust_pieces.push((name.clone(), "minecraft:omb".to_owned(), bb));
                    } else {
                        for piece in pieces {
                            rust_pieces.push((name.clone(), piece.id.clone(), piece.bb));
                        }
                    }
                }
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(pieces) => {
                    for piece in pieces {
                        rust_pieces.push((
                            name.clone(),
                            "minecraft:jigsaw".to_owned(),
                            [
                                piece.bounding_box.min_x,
                                piece.bounding_box.min_y,
                                piece.bounding_box.min_z,
                                piece.bounding_box.max_x,
                                piece.bounding_box.max_y,
                                piece.bounding_box.max_z,
                            ],
                        ));
                    }
                }
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(pieces) => {
                    for piece in pieces {
                        rust_pieces.push((
                            name.clone(),
                            piece.piece_type.clone(),
                            [
                                piece.bounding_box.min_x,
                                piece.bounding_box.min_y,
                                piece.bounding_box.min_z,
                                piece.bounding_box.max_x,
                                piece.bounding_box.max_y,
                                piece.bounding_box.max_z,
                            ],
                        ));
                    }
                }
            }
        }
        normalize(&mut rust_pieces);
        if rust_pieces != want_pieces {
            let first = rust_pieces
                .iter()
                .zip(want_pieces.iter())
                .find(|(a, b)| a != b);
            self.stats.mismatches.push(format!(
                "coverage ({seed},{x},{z}) pieces: ours {} vs {}; first diff {first:?}",
                rust_pieces.len(),
                want_pieces.len()
            ));
        }
        self.stats.coverage_rows += 1;
        self.stats.java_pieces += total;
        self.pending_coverage.insert((seed, x, z), total);
        Ok(())
    }

    /// 统计本次 `game_features_with_mask` 里每种结构实际放置的结构件数与方块数。
    /// Java 侧的覆盖行保证了这些引用确实来自官方生成。
    ///
    /// Record per-structure placed-piece / written-block counts from the game run.
    fn collect_structure_coverage(
        &mut self,
        seed: i64,
        x: i32,
        z: i32,
        result: &mc_bamboo_worldgen::feature::FeatureResult,
    ) -> Result<(), String> {
        let config = WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let pipeline = mc_bamboo_game::worldgen_features::game_pipeline(&config, x, z)
            .map_err(|error| format!("game_pipeline failed for ({seed},{x},{z}): {error}"))?;
        for (kind, count) in &pipeline.placed_pieces {
            let blocks = pipeline.placed_blocks.get(kind).copied().unwrap_or(0);
            let entry = self.structure_totals.entry(kind.clone()).or_default();
            entry.0 += *count as u64;
            entry.1 += blocks as u64;
        }
        if pipeline.unsupported_pieces > 0 {
            self.stats.mismatches.push(format!(
                "features seed {seed} ({x},{z}): {} unsupported structure pieces",
                pipeline.unsupported_pieces
            ));
        }
        let _ = result;
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let Stats {
            chunks,
            biome_cells,
            blocks,
            block_entities,
            heightmap_columns,
            heightmap_words,
            mismatches,
            break_writes,
            coverage_rows,
            java_pieces,
        } = self.stats;
        println!(
            "gen-world: {chunks} chunks, {biome_cells} quart cells, {blocks} blocks, \
             {block_entities} block entities, {heightmap_columns} heightmap columns, \
             {heightmap_words} heightmap words, \
             {coverage_rows} structure rows / {java_pieces} official pieces, \
             {} mismatches, {} official checks",
            mismatches.len(),
            self.official_checks
        );
        if self.structure_coverage {
            println!("gen-world structure placements (pieces / block writes):");
            if self.structure_totals.is_empty() {
                println!("  (none)");
            }
            for (kind, (pieces, writes)) in &self.structure_totals {
                println!("  {pieces:10} {writes:12} {kind}");
            }
        }
        if self.coverage {
            println!("gen-world feature candidates by type:");
            if self.coverage_totals.is_empty() {
                println!("  (none)");
            }
            for (kind, count) in &self.coverage_totals {
                println!("  {count:10} {kind}");
            }
        }
        if self.break_self_test {
            if mismatches.is_empty() {
                return Err(
                    "self-test broke a block but gen-world reported 0 mismatches; \
                            the detector is broken"
                        .to_owned(),
                );
            }
            println!(
                "gen-world self-test: detector works ({} mismatches from the deliberate break)",
                mismatches.len()
            );
            return Ok(());
        }
        if chunks == 0 {
            return Err("gen-world compared no chunks".to_owned());
        }
        if self.structures && coverage_rows != chunks {
            return Err(format!(
                "structures mode compared {chunks} chunks but {coverage_rows} coverage rows"
            ));
        }
        if let Some(kind) = &self.break_structure {
            // 带结构改坏确认：跳过的那种结构必须真的在这批区块里被比较到，
            // 且逐方块比较器必须报差异；否则是没有覆盖的假绿。
            // Structures-mode break confirmation: the skipped type must actually
            // appear in the clean run and the comparator must report a mismatch.
            let placed = self
                .structure_totals
                .get(kind)
                .copied()
                .unwrap_or_default()
                .0;
            if placed == 0 || mismatches.is_empty() {
                return Err(format!(
                    "break-structure {kind} skipped {placed} pieces but reported {} mismatches",
                    mismatches.len()
                ));
            }
            println!(
                "gen-world break-structure {kind}: detector works ({placed} clean pieces, \
                 {} mismatches)",
                mismatches.len()
            );
            return Ok(());
        }
        if let Some(kind) = &self.break_type {
            // 改坏确认要求这批区块里至少有一个中心真的被目标类型写过，
            // 且改坏被逐方块比较器抓到；否则就是没覆盖到的假绿。
            // Break confirmation needs at least one center where the type really
            // wrote a block, and the corrupted cell must be detected.
            if break_writes == 0 || mismatches.is_empty() {
                return Err(format!(
                    "break-type {kind} corrupted {break_writes} cells but reported {} mismatches",
                    mismatches.len()
                ));
            }
            println!(
                "gen-world break-type {kind}: detector works ({break_writes} corrupted cells, \
                 {} mismatches)",
                mismatches.len()
            );
            return Ok(());
        }
        if !mismatches.is_empty() {
            for message in mismatches.iter().take(self.show) {
                eprintln!("  MISMATCH {message}");
            }
            return Err(format!("gen-world found {} mismatches", mismatches.len()));
        }
        println!("gen-world: 0 mismatches");
        Ok(())
    }
}

/// 按 `placed_feature` 的导出顺序给出每个条目的 Feature 类型 ID。
/// 下标和 `perf::FeatureRecord::placed` 一致（`Registry::placed` 的插入序）。
///
/// Feature type ID per `placed_feature` entry, in exported order; the index
/// matches `perf::FeatureRecord::placed` (`Registry::placed` insertion order).
fn feature_kinds() -> Result<Vec<String>, String> {
    let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN)
        .map_err(|error| format!("bad worldgen data: {error}"))?;
    let placed = root
        .get("placed_feature")
        .and_then(Value::as_object)
        .ok_or("worldgen data has no placed_feature")?;
    let features = root
        .get("feature")
        .and_then(Value::as_object)
        .ok_or("worldgen data has no feature")?;
    let mut kinds = Vec::with_capacity(placed.len());
    for (name, value) in placed {
        let field = value
            .get("feature")
            .ok_or_else(|| format!("placed feature {name} has no feature"))?;
        let kind = match field {
            Value::String(reference) => features
                .get(reference)
                .and_then(|f| f.get("type"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("unknown feature {reference}"))?,
            Value::Object(inline) => inline
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("inline feature of {name} has no type"))?,
            _ => return Err(format!("placed feature {name} has a bad feature field")),
        };
        kinds.push(kind.to_owned());
    }
    Ok(kinds)
}

/// JSON 一行里的 `(seed, x, z)`。
/// `(seed, x, z)` from one JSON line.
fn line_key(value: &Value) -> Result<(i64, i32, i32), String> {
    let get = |key: &str| -> Result<i64, String> {
        value
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("line missing {key}"))
    };
    Ok((get("seed")?, get("x")? as i32, get("z")? as i32))
}

/// 官方调色板是 `BlockState.toString()`；terrain 只放默认状态，忽略属性即可。
/// Official palette entries are `BlockState.toString()`; terrain uses default
/// states only, so properties can be ignored.
fn state_of(name: &str) -> Option<u32> {
    let body = name.strip_prefix("Block{")?;
    let (id, _) = body.split_once('}')?;
    Some(block_by_name(id)?.default_state)
}

fn compare_biomes(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    sections: &[Vec<String>],
    generated: &GeneratedChunk,
) {
    if sections.len() != generated.biomes.len() {
        stats.mismatches.push(format!(
            "biomes seed {seed} ({x},{z}): {} sections vs {}",
            sections.len(),
            generated.biomes.len()
        ));
        return;
    }
    for (section, (ours, expected)) in generated.biomes.iter().zip(sections).enumerate() {
        if ours.len() != expected.len() {
            stats.mismatches.push(format!(
                "biomes seed {seed} ({x},{z}) section {section}: {} cells vs {}",
                ours.len(),
                expected.len()
            ));
            continue;
        }
        for (cell, (&id, name)) in ours.iter().zip(expected).enumerate() {
            let ours_name = registries::biome::NAMES
                .get(id as usize)
                .copied()
                .unwrap_or("<unknown>");
            stats.biome_cells += 1;
            if ours_name != name {
                stats.mismatches.push(format!(
                    "biomes seed {seed} ({x},{z}) section {section} cell {cell}: \
                     official {name}, game {ours_name} ({id})"
                ));
            }
        }
    }
}

fn compare_blocks(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    palette: &[Value],
    indices: &[Value],
    generated: &GeneratedChunk,
) -> Result<(), String> {
    let states = palette
        .iter()
        .map(|name| {
            let text = name
                .as_str()
                .ok_or("carvers palette entry is not a string")?;
            state_of(text).ok_or_else(|| format!("unknown official block state {text}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    compare_indices(stats, seed, x, z, &states, indices, generated)
}

/// 地物阶段的逐方块比较：调色板按 `BlockState.toString()` 精确还原状态 ID。
///
/// Block-by-block comparison for features: resolve each palette entry to its
/// exact state id, properties included.
fn compare_blocks_exact(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    palette: &[Value],
    indices: &[Value],
    generated: &GeneratedChunk,
) -> Result<(), String> {
    let states = palette
        .iter()
        .map(|name| {
            let text = name
                .as_str()
                .ok_or("features palette entry is not a string")?;
            state_from_official_name(text)
        })
        .collect::<Result<Vec<_>, _>>()?;
    compare_indices(stats, seed, x, z, &states, indices, generated)
}

/// 官方方块实体构造器在 `saveAdditional` 里无条件写出的默认值。这些值不是模板
/// 写下的，游戏侧没有对应键；只有官方值恰好等于表里的默认值、且游戏侧缺这个键
/// 时才忽略，非默认值或双方都写了不同值照样报。
///
/// Values unconditionally written by vanilla block-entity constructors in
/// `saveAdditional`. They are not template data, so the game side has no matching key.
/// A mismatch is skipped only when the official value equals the table's default AND
/// the game side omits the key; any other value still fails the comparison.
const BLOCK_ENTITY_DEFAULTS: &[(&str, &str, &str)] = &[
    // `VaultBlockEntity.saveAdditional` 用三个 codec 写 `config` / `shared_data` /
    // `server_data`；新建的宝库里后两个是空复合。
    ("minecraft:vault", "shared_data", "{}"),
    ("minecraft:vault", "server_data", "{}"),
    // `AbstractFurnaceBlockEntity.saveAdditional` 的 `speedMultiplier` 默认 1.0f。
    ("minecraft:furnace", "speed_multiplier", "1.0f"),
    ("minecraft:blast_furnace", "speed_multiplier", "1.0f"),
    ("minecraft:smoker", "speed_multiplier", "1.0f"),
    // `SculkSensorBlockEntity.saveAdditional` 存 `lastVibrationFrequency` 与
    // `VibrationSystem.Data.CODEC` 编码的默认监听器。
    (
        "minecraft:sculk_sensor",
        "last_vibration_frequency",
        "0",
    ),
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
    // `BrewingStandBlockEntity.saveAdditional` 的两个计时总量。
    ("minecraft:brewing_stand", "total_brew_time", "400"),
    ("minecraft:brewing_stand", "total_fuel", "20"),
    ("minecraft:brewing_stand", "speed_multiplier", "1.0f"),
    // `SignBlockEntity.saveAdditional` 无条件写 `putBoolean("is_waxed", false)`。
    ("minecraft:sign", "is_waxed", "0b"),
    ("minecraft:hanging_sign", "is_waxed", "0b"),
];

/// 方块实体类型（`saveWithFullMetadata` 的 `id`）。
///
/// Block-entity type (the `id` from `saveWithFullMetadata`).
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

/// 方块实体的「有意义」键：官方 `saveWithFullMetadata` 会补上 id / x / y / z，
/// 游戏侧写入的 nbt 只带结构件 / 地物真正设置的字段。这些元数据键由位置隐含，
/// 比较时丢掉；空的 `components` 是 `saveWithoutMetadata` 无条件写的，同样丢掉。
///
/// Keys that carry meaning for a block entity: vanilla's `saveWithFullMetadata` adds
/// id / x / y / z, which the game side does not store because they come from the
/// position. The empty `components` compound is written unconditionally by
/// `saveWithoutMetadata`, so it is dropped too.
fn custom_keys(tag: &mc_bamboo_nbt::Tag) -> Vec<(String, String)> {
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
    out
}

/// 官方侧特有的构造器默认值只有在「值等于默认值且游戏侧缺键」时才可以忽略。
///
/// Constructor defaults present only on the official side may be ignored only when
/// their value is the default and the game side omits the key.
fn official_constructor_defaults(
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
            // 游戏侧真的写了这个键：只有值也等于默认值才算一致，否则仍报错。
            // The key is really written by the game: it must also hold the default value.
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

/// 比较一个区块的方块实体：官方导出的是 `[x,y,z, SNBT]`，游戏侧是
/// `FeatureResult::block_entities` 的 `(世界坐标, nbt)`。同位置只比最后写入的那份。
///
/// Compare one chunk's block entities: the official side emits `[x,y,z, SNBT]`,
/// the game side holds `(world position, nbt)` in `FeatureResult::block_entities`.
/// Only the last nbt written at a position counts.
fn compare_block_entities(
    stats: &mut Stats,
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    official: Option<&Value>,
    game: &[((i32, i32, i32), mc_bamboo_nbt::Tag)],
) -> Result<(), String> {
    let Some(official) = official else {
        return Err(format!(
            "features seed {seed} ({chunk_x},{chunk_z}): oracle has no block_entities"
        ));
    };
    let rows = official
        .as_array()
        .ok_or("features block_entities is not an array")?;
    let mut ours: BTreeMap<(i32, i32, i32), Vec<(String, String)>> = BTreeMap::new();
    for ((x, y, z), tag) in game {
        if x >> 4 == chunk_x && z >> 4 == chunk_z {
            ours.insert((*x, *y, *z), custom_keys(tag));
        }
    }
    for row in rows {
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
        let tag = mc_bamboo_nbt::parse_snbt(text)
            .map_err(|error| format!("bad official block entity snbt {text}: {error}"))?;
        let mut want = custom_keys(&tag);
        let ignored = official_constructor_defaults(&tag, &mut want);
        match ours.remove(&pos) {
            None => {
                if want.is_empty() {
                    // 官方只有构造器默认值，游戏侧没有任何真实数据：两边等价。
                    // The official side has only constructor defaults and the game
                    // side has no real data at all: equivalent.
                    stats.block_entities += 1;
                } else {
                    stats.mismatches.push(format!(
                        "block entity seed {seed} ({chunk_x},{chunk_z}) at {pos:?}: official {want:?}, game missing"
                    ));
                }
            }
            Some(mut got) => {
                // 游戏侧写了官方默认键时，比较器已经把「双方都是默认值」的那一对
                // 从两边一起删掉；其它情况必须完全一致。
                // When the game writes a defaulted key, a matching pair is removed
                // from both sides; everything else must match exactly.
                let game_ignored = official_constructor_defaults(&tag, &mut got);
                if got != want || game_ignored != ignored {
                    stats.mismatches.push(format!(
                        "block entity seed {seed} ({chunk_x},{chunk_z}) at {pos:?}: official {want:?}, game {got:?}"
                    ));
                } else {
                    stats.block_entities += 1;
                }
            }
        }
    }
    for (pos, got) in &ours {
        stats.mismatches.push(format!(
            "block entity seed {seed} ({chunk_x},{chunk_z}) at {pos:?}: official missing, game {got:?}"
        ));
    }
    Ok(())
}

fn compare_indices(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    states: &[u32],
    indices: &[Value],
    generated: &GeneratedChunk,
) -> Result<(), String> {
    let height = generated.blocks.len();
    if indices.len() != height * 256 {
        stats.mismatches.push(format!(
            "blocks seed {seed} ({x},{z}): {} blocks vs official {}",
            height * 256,
            indices.len()
        ));
        return Ok(());
    }
    for (index, entry) in indices.iter().enumerate() {
        let id = entry
            .as_u64()
            .ok_or("carvers block index is not a number")? as usize;
        let Some(&official) = states.get(id) else {
            stats.mismatches.push(format!(
                "blocks seed {seed} ({x},{z}) at {index}: palette index {id} out of range"
            ));
            continue;
        };
        let bamboo = generated.blocks[index / 256][index % 256];
        stats.blocks += 1;
        if official != bamboo {
            stats.mismatches.push(format!(
                "blocks seed {seed} ({x},{z}) y {} x {} z {}: official {official}, game {bamboo}",
                index as i32 / 256 - 64,
                (index % 256) / 16,
                index % 16
            ));
        }
    }
    Ok(())
}

/// 逐列比较六张高度图（世界高度 `getFirstAvailable`）。
/// Compare the six heightmaps column by column (world height `getFirstAvailable`).
fn compare_heightmaps(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    columns: &serde_json::Map<String, Value>,
    generated: &GeneratedChunk,
) {
    for key in mc_bamboo_game::worldgen::HEIGHTMAP_KEYS {
        let expected = match columns.get(key).and_then(Value::as_array) {
            Some(array) => array,
            None => {
                stats.mismatches.push(format!(
                    "heightmaps seed {seed} ({x},{z}): official has no {key}"
                ));
                continue;
            }
        };
        let ours = match generated.heightmaps.get(key) {
            Some(map) => map,
            None => {
                stats.mismatches.push(format!(
                    "heightmaps seed {seed} ({x},{z}): game has no {key}"
                ));
                continue;
            }
        };
        if expected.len() != 256 || ours.len() != 256 {
            stats.mismatches.push(format!(
                "heightmaps seed {seed} ({x},{z}) {key}: {} columns vs official {}",
                ours.len(),
                expected.len()
            ));
            continue;
        }
        for (column, (&ours_value, official)) in ours.iter().zip(expected).enumerate() {
            stats.heightmap_columns += 1;
            let official = official.as_i64().unwrap_or(i64::MIN) as i32;
            if ours_value != official {
                stats.mismatches.push(format!(
                    "heightmaps seed {seed} ({x},{z}) {key} column {column}: \
                     official {official}, game {ours_value}"
                ));
            }
        }
    }
}

/// 比较四张 FINAL 高度图的打包位流（存档写出的 `LongArrayTag` 内容）。
/// Compare the packed words of the four FINAL heightmaps (the saved `LongArrayTag`).
fn compare_heightmap_words(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    words: &serde_json::Map<String, Value>,
    ours: &std::collections::BTreeMap<&'static str, Vec<u64>>,
) {
    for (key, values) in ours {
        let expected = match words.get(*key).and_then(Value::as_array) {
            Some(array) => array,
            None => {
                stats.mismatches.push(format!(
                    "heightmap_words seed {seed} ({x},{z}): official has no {key}"
                ));
                continue;
            }
        };
        if expected.len() != values.len() {
            stats.mismatches.push(format!(
                "heightmap_words seed {seed} ({x},{z}) {key}: {} words vs official {}",
                values.len(),
                expected.len()
            ));
            continue;
        }
        for (index, (&ours_word, official)) in values.iter().zip(expected).enumerate() {
            stats.heightmap_words += 1;
            let official = official
                .as_i64()
                .map(|value| value as u64)
                .or_else(|| official.as_u64())
                .unwrap_or(0);
            if ours_word != official {
                stats.mismatches.push(format!(
                    "heightmap_words seed {seed} ({x},{z}) {key} word {index}: \
                     official {official}, game {ours_word}"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 故意改坏一处：确认逐方块比较会报不一致，而不是假绿。
    /// Deliberately break one block: the comparison must report a mismatch, not a false green.
    #[test]
    fn block_comparator_detects_a_single_wrong_block() {
        let generated = GeneratedChunk {
            blocks: vec![vec![mc_bamboo_data::blocks::STONE; 256]; 1],
            biomes: vec![],
            heightmaps: Default::default(),
        };
        let palette = vec![Value::String("Block{minecraft:stone}".to_owned())];
        let indices: Vec<Value> = (0..256).map(|_| Value::from(0u64)).collect();
        let mut clean = Stats::default();
        compare_blocks(&mut clean, 1, 0, 0, &palette, &indices, &generated).unwrap();
        assert!(clean.mismatches.is_empty());
        assert_eq!(clean.blocks, 256);

        // 把官方期望改成另一个方块，比较器必须报 256 处不一致。
        // Change the official expectation; the comparator must report 256 mismatches.
        let wrong_palette = vec![Value::String("Block{minecraft:dirt}".to_owned())];
        let mut broken = Stats::default();
        compare_blocks(&mut broken, 1, 0, 0, &wrong_palette, &indices, &generated).unwrap();
        assert_eq!(broken.mismatches.len(), 256);
    }

    #[test]
    fn biome_comparator_detects_a_wrong_id() {
        let generated = GeneratedChunk {
            blocks: vec![],
            biomes: vec![vec![registries::biome::PLAINS as u32; 64]],
            heightmaps: Default::default(),
        };
        let sections = vec![vec!["minecraft:desert".to_owned(); 64]];
        let mut stats = Stats::default();
        compare_biomes(&mut stats, 7, 2, 3, &sections, &generated);
        assert_eq!(stats.mismatches.len(), 64);
    }

    /// 故意改坏一列：确认六张高度图的逐列比较会报不一致。
    /// Deliberately break one column: the six-heightmap comparison must fail.
    #[test]
    fn heightmap_comparator_detects_a_wrong_column() {
        let mut heightmaps = std::collections::BTreeMap::new();
        for key in mc_bamboo_game::worldgen::HEIGHTMAP_KEYS {
            heightmaps.insert(key, vec![100_i32; 256]);
        }
        let generated = GeneratedChunk {
            blocks: vec![],
            biomes: vec![],
            heightmaps,
        };
        let mut columns = serde_json::Map::new();
        for key in mc_bamboo_game::worldgen::HEIGHTMAP_KEYS {
            columns.insert(
                key.to_owned(),
                Value::Array((0..256).map(|_| Value::from(100_i64)).collect()),
            );
        }
        let mut clean = Stats::default();
        compare_heightmaps(&mut clean, 1, 0, 0, &columns, &generated);
        assert!(clean.mismatches.is_empty());
        assert_eq!(clean.heightmap_columns, 6 * 256);

        columns.insert(
            "WORLD_SURFACE_WG".to_owned(),
            Value::Array((0..256).map(|_| Value::from(101_i64)).collect()),
        );
        let mut broken = Stats::default();
        compare_heightmaps(&mut broken, 1, 0, 0, &columns, &generated);
        assert_eq!(broken.mismatches.len(), 256);
    }

    /// 故意改坏一个 long：确认 FINAL 高度图位流比较会报不一致。
    /// Deliberately break one long: the FINAL heightmap-word comparison must fail.
    #[test]
    fn heightmap_words_comparator_detects_a_wrong_word() {
        let mut ours: std::collections::BTreeMap<&'static str, Vec<u64>> =
            std::collections::BTreeMap::new();
        ours.insert("WORLD_SURFACE", vec![7_u64; 37]);
        let mut words = serde_json::Map::new();
        words.insert(
            "WORLD_SURFACE".to_owned(),
            Value::Array((0..37).map(|_| Value::from(7_i64)).collect()),
        );
        let mut clean = Stats::default();
        compare_heightmap_words(&mut clean, 1, 0, 0, &words, &ours);
        assert!(clean.mismatches.is_empty());
        assert_eq!(clean.heightmap_words, 37);

        words.insert(
            "WORLD_SURFACE".to_owned(),
            Value::Array((0..37).map(|_| Value::from(8_i64)).collect()),
        );
        let mut broken = Stats::default();
        compare_heightmap_words(&mut broken, 1, 0, 0, &words, &ours);
        assert_eq!(broken.mismatches.len(), 37);
    }

    #[test]
    fn options_force_carvers_and_forward_seed_args() {
        let options = Options::parse(&["--seeds", "2", "--chunks", "4", "--salt", "9"]).unwrap();
        assert!(options.forwarded.contains(&"--through".to_owned()));
        assert!(options.forwarded.contains(&"carvers".to_owned()));
        assert!(options.forwarded.contains(&"--real-path".to_owned()));
        assert!(options.forwarded.contains(&"2".to_owned()));
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    #[test]
    fn level_type_forwards_the_settings_id() {
        let normal = Options::parse(&[]).unwrap();
        assert_eq!(normal.level_type, LevelType::Normal);
        assert!(!normal.forwarded.contains(&"--settings".to_owned()));
        let large = Options::parse(&["--level-type", "large_biomes"]).unwrap();
        assert_eq!(large.level_type, LevelType::LargeBiomes);
        let forwarded = large
            .forwarded
            .windows(2)
            .map(|w| (w[0].as_str(), w[1].as_str()));
        assert!(
            forwarded
                .into_iter()
                .any(|(k, v)| k == "--settings" && v == "large_biomes")
        );
        let amplified = Options::parse(&["--level-type", "minecraft:amplified"]).unwrap();
        assert_eq!(amplified.level_type, LevelType::Amplified);
        // flat 不是噪声世界，不接受。/ flat is not a noise world, so it is rejected.
        assert!(Options::parse(&["--level-type", "flat"]).is_err());
    }
}
