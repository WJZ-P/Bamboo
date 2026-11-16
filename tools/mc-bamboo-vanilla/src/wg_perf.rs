//! `wg-perf`：量区块生成的时间分布，给「地表带先行」的设计提供数据。
//!
//! 只测量、不改生成结果：`mc-bamboo-worldgen` 的 `perf` 特性打开的是一组空操作钩子，
//! 默认构建里没有任何计时和额外状态。这里按群系挑区块，用 release 构建逐个区块跑一遍
//! 完整生成（群系 → 3×3 地形 → 地物 → 光照 → 组装），把各阶段的墙钟、噪声填充按 8 格
//! 高度带的分布、地物候选位置的高度分布和「只生成切割线以上」的对照耗时都算出来，打印
//! Markdown 表格，需要时再打印一份 JSON。
//!
//! 计时噪声：每组跑 `--repeat` 遍取中位数；单线程是主数字，另外给一组多线程的吞吐量。
//!
//! `wg-perf`: measure where chunk generation time goes, to feed the "surface band
//! first" design. It only measures: the `perf` feature of `mc-bamboo-worldgen` is a set
//! of no-op hooks absent from default builds. The subcommand picks chunks by biome, runs
//! a full release-mode generation per chunk (biomes -> 3x3 terrain -> features -> light
//! -> assembly), and reports per-phase wall clock, the noise fill split into 8-block
//! height bands, per-feature candidate heights, and the "only above the cut line"
//! comparison. Every group runs `--repeat` times and the median is reported; the main
//! numbers are single-threaded, plus one multi-thread throughput measurement.

use std::{
    collections::{BTreeMap, HashMap},
    hint::black_box,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::{registries, state_facts};
use mc_bamboo_world::{
    MIN_Y, WORLD_HEIGHT, chunk::Chunk, light::generate::generate_light_from_layers,
};
use mc_bamboo_worldgen::{
    Worldgen,
    chunk::{Generator, Stage},
    feature::{DECORATION_STEPS, world::prime_heightmaps},
    perf::{self, Floors, Phase, Record, Y_BANDS},
};

const USAGE: &str = "usage: wg-perf [--seeds N] [--chunks N] [--biomes a,b,c] [--repeat N] \
                     [--threads N] [--aux N] [--plan-scan N] [--json] [--skip-cut] [--skip-plans] \
                     [--skip-sky] [--skip-threads] [--skip-aux] [--light-compare]";

/// 辅助测量的默认抽样上限（高度分布、切割线对照、多线程）。`--aux N` 可以改。
/// Default cap on the auxiliary measurements (height profile, cut-line comparison,
/// threads); `--aux N` overrides it.
const AUX_LIMIT: usize = 20;

/// 洞穴群系和废弃矿井统计的默认扫描量（每个种子）。矿井 `frequency` 0.004，要 250 个区块
/// 才期望碰到一个，所以默认扫 2500 个。
///
/// Default scan size per seed for the cave-biome and mineshaft counts. Mineshaft `frequency`
/// is 0.004, so 250 chunks are needed to expect one; the default scans 2500.
const PLAN_SCAN: usize = 2500;

/// 默认采样的群系：任务书要求的五类（平原、森林、山地、海洋、丛林），再加上五种常见的
/// 其它类型，让每个种子能凑够 200 个以上区块（窄带群系单种找不到那么多）。
///
/// The default biome sample: the five the brief asks for (plains, forest, hills, ocean,
/// jungle) plus five other common types, so each seed reaches more than two hundred
/// chunks (a single narrow biome cannot supply that many).
const DEFAULT_BIOMES: [&str; 10] = [
    "minecraft:plains",
    "minecraft:forest",
    "minecraft:windswept_hills",
    "minecraft:ocean",
    "minecraft:jungle",
    "minecraft:desert",
    "minecraft:taiga",
    "minecraft:swamp",
    "minecraft:badlands",
    "minecraft:savanna",
];

/// 一个种子下要测的区块。
/// One chunk to measure under one seed.
#[derive(Debug, Clone, Copy)]
struct Target {
    seed: i64,
    x: i32,
    z: i32,
    biome: &'static str,
}

/// 一个区块的一次完整测量。
/// One chunk's full measurement.
#[derive(Clone)]
struct Measured {
    target: Target,
    /// `OCEAN_FLOOR_WG` 的最小值。 The minimum `OCEAN_FLOOR_WG`.
    ocean_floor_min: i32,
    /// 对齐到 16 的切割线（余量 24）。 The cut line aligned to 16 (margin 24).
    cut: i32,
    record: Record,
    /// 整个区块的墙钟（毫秒），由工具自己在外面量，含记录器没覆盖的部分。
    /// The chunk's wall clock in milliseconds, measured outside the record so it also
    /// covers whatever the recorder does not.
    total_ms: f64,
}

/// 选项。
/// Options.
struct Options {
    seeds: usize,
    chunks_per_biome: usize,
    biomes: Vec<String>,
    repeat: usize,
    threads: usize,
    json: bool,
    skip_cut: bool,
    skip_sky: bool,
    skip_threads: bool,
    /// 跳过「水平切线方案 × 玩家情形」两节。
    /// Skip the cut-plan-by-player-situation sections.
    skip_plans: bool,
    /// 洞穴群系和废弃矿井统计每个种子扫多少个区块。
    /// How many chunks per seed the cave-biome and mineshaft scan covers.
    plan_scan: usize,
    /// 辅助测量的抽样上限。 Cap on the auxiliary measurements.
    aux: usize,
    /// 跳过全部辅助测量，只跑主阶段表（改前/改后交替量时用）。
    /// Skip every auxiliary measurement and print only the main phase table
    /// (for alternating before/after measurements).
    skip_aux: bool,
    /// 只跑新旧光照路径的交替对照。 Run only the old/new light path comparison.
    light_compare: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            seeds: 3,
            chunks_per_biome: 8,
            biomes: DEFAULT_BIOMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            repeat: 3,
            threads: 4,
            json: false,
            skip_cut: false,
            skip_sky: false,
            skip_threads: false,
            skip_plans: false,
            plan_scan: PLAN_SCAN,
            aux: AUX_LIMIT,
            skip_aux: false,
            light_compare: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            let mut value = || args.next().copied().ok_or(USAGE.to_owned());
            match arg {
                "--seeds" => options.seeds = value()?.parse().map_err(|_| USAGE.to_owned())?,
                "--chunks" => {
                    options.chunks_per_biome = value()?.parse().map_err(|_| USAGE.to_owned())?;
                }
                "--biomes" => {
                    options.biomes = value()?
                        .split(',')
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                "--repeat" => options.repeat = value()?.parse().map_err(|_| USAGE.to_owned())?,
                "--threads" => options.threads = value()?.parse().map_err(|_| USAGE.to_owned())?,
                "--aux" => options.aux = value()?.parse().map_err(|_| USAGE.to_owned())?,
                "--plan-scan" => {
                    options.plan_scan = value()?.parse().map_err(|_| USAGE.to_owned())?
                }
                "--json" => options.json = true,
                "--skip-cut" => options.skip_cut = true,
                "--skip-sky" => options.skip_sky = true,
                "--skip-threads" => options.skip_threads = true,
                "--skip-plans" => options.skip_plans = true,
                "--skip-aux" => options.skip_aux = true,
                "--light-compare" => options.light_compare = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let worldgen = Worldgen::load()?;
    let seeds: Vec<i64> = (0..options.seeds).map(seed_for).collect();
    let targets = pick_targets(&worldgen, &seeds, &options.biomes, options.chunks_per_biome)?;
    println!(
        "wg-perf: {} chunks ({} seeds x {} biomes x {}), {} repeats each",
        targets.len(),
        seeds.len(),
        options.biomes.len(),
        options.chunks_per_biome,
        options.repeat
    );
    if options.light_compare {
        return measure_light_compare(&worldgen, &targets, options.repeat);
    }

    // 首次调用的初始化开销单独报：建 `Generator`（编译采样器）+ 第一次生成。
    let init_start = Instant::now();
    let generator = Generator::new(&worldgen, seeds[0])?;
    let build_ms = init_start.elapsed().as_secs_f64() * 1000.0;
    let first_start = Instant::now();
    let _ = measure_once(&generator, targets[0], 1)?;
    println!(
        "wg-perf: building the generator {build_ms:.1} ms, first full chunk {:.1} ms \
         (initialisation, excluded from the medians below)",
        first_start.elapsed().as_secs_f64() * 1000.0
    );

    let mut measured = Vec::with_capacity(targets.len());
    let mut generators: HashMap<i64, Generator<'_>> = HashMap::new();
    let mut view_ms = Vec::new();
    for &target in &targets {
        let generator = match generators.entry(target.seed) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Generator::new(&worldgen, target.seed)?)
            }
        };
        let view_start = Instant::now();
        let _ = view_of(generator, target.x, target.z)?;
        view_ms.push(view_start.elapsed().as_secs_f64() * 1000.0);
        let (item, _) = measure_once(generator, target, options.repeat)?;
        measured.push(item);
    }
    let mut sorted_views = view_ms.clone();
    sorted_views.sort_by(f64::total_cmp);
    println!(
        "wg-perf: building the 3x3 view costs {:.1} ms per chunk (median) and is NOT part \
         of the per-chunk numbers below; in the game the neighbours come from the world \
         thread's CARVERS snapshot cache",
        sorted_views[sorted_views.len() / 2]
    );

    print_phases(&measured);
    print_biomes(&measured);
    if options.skip_aux {
        return Ok(());
    }
    // 辅助测量取一部分区块，免得整轮跑太久。
    // The auxiliary measurements use a subsample so a round stays affordable.
    let (aux_targets, aux_measured) = subsample(&targets, &measured, options.aux);
    let bands = measure_bands(&worldgen, &aux_targets, options.repeat)?;
    print_bands(&bands, &aux_measured);
    print_features(&worldgen, &aux_targets, options.repeat, &aux_measured)?;
    if !options.skip_cut {
        measure_cut_lines(&worldgen, &aux_targets, &aux_measured, options.repeat)?;
    }
    if !options.skip_plans {
        measure_cut_plans(&worldgen, &aux_targets, &aux_measured, options.repeat)?;
        measure_special_chunks(&worldgen, &seeds, &aux_measured, options.plan_scan)?;
    }
    if !options.skip_sky {
        measure_sky(&worldgen, &aux_targets, &aux_measured, &bands)?;
    }
    if !options.skip_threads {
        measure_threads(&worldgen, &aux_targets, options.threads, options.repeat)?;
    }
    print_json(&measured, &bands.ms);
    Ok(())
}

fn seed_for(index: usize) -> i64 {
    match index {
        0 => 0,
        1 => 1,
        2 => 123_456_789,
        3 => -987_654_321,
        other => (other as i64).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64),
    }
}

/// 量一个区块 `repeat` 遍，取每项耗时和总时长的中位数。
///
/// 每个区块的 3×3 视图只建一次、不计入「单区块成本」：游戏里邻居的 CARVERS 地形来自
/// 世界线程的快照缓存，不重算。建视图的耗时单独报（见 `print_view_cost`）。
///
/// Measure one chunk `repeat` times and take the median of every field and of the
/// wall clock. The 3x3 view is built once per chunk and excluded from the per-chunk
/// cost: in the game the neighbours' CARVERS terrain comes from the world thread's
/// snapshot cache, not from a recompute. Its cost is reported separately.
fn measure_once(
    generator: &Generator<'_>,
    target: Target,
    repeat: usize,
) -> Result<(Measured, f64), String> {
    let ocean_floor_min = ocean_floor_min(generator, target)?;
    let cut = cut_line(ocean_floor_min);
    let view = view_of(generator, target.x, target.z)?;
    perf::set_floors(Floors::default());
    let mut records = Vec::with_capacity(repeat);
    let mut times = Vec::with_capacity(repeat);
    for _ in 0..repeat {
        let start = Instant::now();
        let record = generate_recorded(generator, target, cut, &view)?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        black_box(&record);
        records.push(record);
    }
    times.sort_by(f64::total_cmp);
    let total = times[times.len() / 2];
    let record = median_record(records);
    Ok((
        Measured {
            target,
            ocean_floor_min,
            cut,
            record,
            total_ms: total,
        },
        times[times.len() - 1],
    ))
}

/// 切割线：本区块 `OCEAN_FLOOR_WG` 的最小值减 24，再向下对齐到 16 的倍数。
///
/// The cut line: the chunk's minimum `OCEAN_FLOOR_WG` minus 24, floored to a multiple
/// of 16.
fn cut_line(ocean_floor_min: i32) -> i32 {
    (ocean_floor_min - 24).div_euclid(16) * 16
}

/// 「水平切线方案 × 玩家情形」一节：量出「必须生成的部分」占完整生成时间的比例。
///
/// **做法：按阶段合成，不靠整链计时相除。** 整链计时有个致命问题：`light_view`（3×3 重打包，
/// 约 27 ms）和 `features`（约 14 ms）几乎不随切割线变化，而真正能省的 `noise_density` +
/// `surface` + `substance` 加起来只有约 10 ms；两者相比，切割线带来的差异被稀释到 15% 以内，
/// 再被这台机器 ±10% 的计时噪声淹没（实测出现过 104% 这种不可能的数字）。
///
/// 所以这里对每个抽样区块量两样东西：
///
/// 1. **各阶段耗时**（`Record::phases_ms`），在完全不裁的情况下量一遍；
/// 2. **几个代表 floor 上的地形耗时**（`doFill` + 地表 + 雕刻器），用来拟合
///    「地形耗时随 floor 变化」的比例。
///
/// 然后按「每个 floor 覆盖多少列」加权，合成出每种情形的总耗时。
///
/// **This section synthesises per-phase costs instead of dividing whole-chain timings.**
/// `light_view` (~27 ms) and `features` (~14 ms) barely move with the cut line, while the
/// parts that do move (`noise_density` + `surface` + `substance`) total about 10 ms; the
/// difference is diluted below fifteen percent and then drowned by this machine's ten-percent
/// timing noise (the first attempt produced impossible 104% figures). So each sampled chunk
/// measures its per-phase costs once, plus the terrain cost at a few representative floors to
/// fit how terrain time scales with the floor, and the situations are combined from those.
fn measure_cut_plans(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 水平切线方案 × 玩家情形\n");
    println!(
        "视距 {VIEW_CHUNKS} 区块（{}×{} 列），3D 距离 ≤ {REQUIRED_RADIUS:.0} 格必须生成，\
         地表带总是生成。每行是「必须生成 / 完整生成」。\n",
        VIEW_CHUNKS * 2 + 1,
        VIEW_CHUNKS * 2 + 1
    );

    print!("| 切法 |");
    for situation in PlayerSituation::ALL {
        print!(" {} |", situation.name());
    }
    println!(" 仅切法（无玩家） |");
    print!("|---|");
    for _ in PlayerSituation::ALL {
        print!("---|");
    }
    println!("---|");

    for plan in CutPlan::ALL {
        let mut required = [0.0f64; 3];
        let mut base_required = 0.0f64;
        let mut full_total = 0.0f64;

        for (target, item) in targets.iter().zip(measured) {
            let generator = Generator::new(worldgen, target.seed)?;
            let full = item.record.total();
            full_total += full;

            // 地形耗时随 floor 的曲线：在几个代表高度上各量一遍。
            // 直接调 `terrain_with_region`，不走整条链——这里只要地形。
            // The terrain cost against the floor, measured at a few representative heights. It
            // calls `terrain_with_region` directly, because only the terrain is wanted here.
            let region = generator.biome_region(target.x, target.z)?;
            let mut terrain_curve: Vec<(i32, f64)> = Vec::new();
            for floor in [MIN_Y, -32, 0, 32, 64, 96] {
                for _ in 0..repeat {
                    perf::set_floors(Floors {
                        noise: Some(floor),
                        surface: Some(floor),
                        carver: Some(floor),
                        feature: Some(floor),
                    });
                    let start = Instant::now();
                    let blocks = generator.terrain_with_region(
                        target.x,
                        target.z,
                        Stage::Carvers,
                        None,
                        &region,
                    )?;
                    terrain_curve.push((floor, start.elapsed().as_secs_f64() * 1000.0));
                    black_box(&blocks);
                }
            }
            perf::set_floors(Floors::default());

            // 按 floor 取中位数，保证单调（floor 越高越快）。
            // Take the median per floor and force monotonicity (higher floor is faster).
            let mut by_floor: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
            for (floor, ms) in terrain_curve {
                by_floor.entry(floor).or_default().push(ms);
            }
            let mut curve: Vec<(i32, f64)> = by_floor
                .into_iter()
                .map(|(floor, mut values)| {
                    values.sort_by(f64::total_cmp);
                    (floor, values[values.len() / 2])
                })
                .collect();
            curve.sort_by_key(|(floor, _)| *floor);
            for index in 1..curve.len() {
                if curve[index].1 > curve[index - 1].1 {
                    curve[index].1 = curve[index - 1].1;
                }
            }

            let terrain_at = |floor: i32| -> f64 {
                // 分段线性插值，floor 超出两端就取端点。
                // Piecewise-linear interpolation; outside the ends, clamp.
                let first = curve[0];
                if floor <= first.0 {
                    return first.1;
                }
                let last = curve[curve.len() - 1];
                if floor >= last.0 {
                    return last.1;
                }
                for pair in curve.windows(2) {
                    let (low, low_ms) = pair[0];
                    let (high, high_ms) = pair[1];
                    if floor >= low && floor <= high {
                        let t = (floor - low) as f64 / (high - low) as f64;
                        return low_ms + (high_ms - low_ms) * t;
                    }
                }
                last.1
            };

            // 整链里除地形以外的部分（群系、地物、光照、组装）不随 floor 变化。
            // Everything but the terrain (biomes, features, light, assembly) does not move
            // with the floor. It is derived from the recorded full time minus the recorded
            // terrain time, so the identity floor reproduces the full time exactly.
            let terrain_full = item.record.terrain();
            let rest = (full - terrain_full).max(0.0);
            // 让 `terrain_at(MIN_Y)` 严格等于 `terrain_full`：曲线是按区块实测的，可能差一点点。
            // Make `terrain_at(MIN_Y)` equal `terrain_full` exactly: the curve is measured per
            // chunk and can drift slightly.
            let curve_min = terrain_at(MIN_Y);
            let scale = if curve_min > 0.0 {
                terrain_full / curve_min
            } else {
                1.0
            };
            let synthesize = |floor: i32| -> f64 { rest + terrain_at(floor) * scale };

            let base_floor = plan.base_floor(item.ocean_floor_min, MIN_Y);
            base_required += synthesize(base_floor);

            for (index, situation) in PlayerSituation::ALL.iter().enumerate() {
                let player_y = situation.player_y(item.ocean_floor_min);
                let mut total_ms = 0.0;
                let mut columns = 0usize;
                for dz in -VIEW_CHUNKS..=VIEW_CHUNKS {
                    for dx in -VIEW_CHUNKS..=VIEW_CHUNKS {
                        let distance = horizontal_distance(dx, dz);
                        let floor = plan.required_floor(
                            item.ocean_floor_min,
                            MIN_Y,
                            Some((distance, player_y)),
                        );
                        total_ms += synthesize(floor);
                        columns += 1;
                    }
                }
                required[index] += total_ms / columns as f64;
            }
        }

        print!("| {} |", plan.name());
        for value in required {
            print!(" {:.1}% |", value / full_total.max(0.001) * 100.0);
        }
        println!(" {:.1}% |", base_required / full_total.max(0.001) * 100.0);

        // 自检：「每段一条」是不裁的恒等切法，四处都该正好 100%。
        // Self-check: "one line per segment" cuts nothing, so all four cells must be 100%.
        if plan == CutPlan::EverySegment {
            let mut worst = (base_required / full_total.max(0.001) * 100.0 - 100.0).abs();
            for value in required {
                worst = worst.max((value / full_total.max(0.001) * 100.0 - 100.0).abs());
            }
            if worst > 2.0 {
                println!(
                    "\n> **自检警告**：恒等切法「每段一条」应正好 100%，实测偏离 {worst:.1} \
                     个百分点。这组数字不可信。\n"
                );
            }
        }
    }
    Ok(())
}

/// 含有洞穴群系的区块比例，以及废弃矿井结构件跨过地表带下边界的区块比例。
///
/// 队长要求：这两类区块按「深层不延迟」计入。
///
/// 洞穴群系四种：繁茂洞穴、滴水石洞穴、硫磺洞穴、深暗之域。
/// 判定方法：`Generator::biomes` 的段调色板里出现其一，就算这一列含有洞穴群系。
///
/// 废弃矿井：用 `pieces::mineshaft::generate` 重建这个区块的矿井结构件，看有没有结构件
/// 的 y 范围跨过地表带下边界（`cut`）。恶地类型（`mineshaft_mesa`）要用真实的
/// `surface_height`，这里用 `first_occupied_height + 1`（官方 `getBaseHeight`）。
///
/// 矿井的 `frequency` 只有 0.004，抽样几十个区块基本碰不到，所以这一节单独扫一**大片**
/// 区块（`scan` 个，按方环从原点铺开），只用轻量的群系查询和矿井布局，不跑地形生成。
///
/// The share of chunks with a cave biome and the share whose abandoned-mineshaft pieces
/// straddle the surface band's lower edge. Mineshaft `frequency` is 0.004, so a few dozen
/// samples almost never contain one; this section therefore scans a larger patch with only
/// the cheap biome query and the piece layout, no terrain generation.
fn measure_special_chunks(
    worldgen: &Worldgen,
    seeds: &[i64],
    measured: &[Measured],
    scan: usize,
) -> Result<(), String> {
    println!("\n## 需要「深层不延迟」的区块\n");

    let cave_biomes = [
        "minecraft:lush_caves",
        "minecraft:dripstone_caves",
        "minecraft:sulfur_caves",
        "minecraft:deep_dark",
    ];
    let cave_ids: Vec<u32> = cave_biomes
        .iter()
        .map(|name| {
            registries::biome::NAMES
                .iter()
                .position(|known| known == name)
                .map(|id| id as u32)
                .ok_or_else(|| format!("unknown biome {name}"))
        })
        .collect::<Result<_, _>>()?;

    // 用抽样区块的切割线中位数做基准，矿井跨线按每个区块自己的切割线算。
    // Use each chunk's own cut line for the mineshaft test.
    let mut cave_chunks = 0usize;
    let mut mine_placement_chunks = 0usize;
    let mut mine_chunks = 0usize;
    let mut mine_24 = 0usize;
    let mut mine_48 = 0usize;
    let mut counted = 0usize;

    // 抽样区块（有完整记录，带切割线）。
    let mut cut_by_seed: BTreeMap<i64, i32> = BTreeMap::new();
    for item in measured {
        cut_by_seed.insert(item.target.seed, item.ocean_floor_min);
    }

    for target in scan_targets(seeds, scan) {
        let Some(&ocean_floor_min) = cut_by_seed.get(&target.seed) else {
            continue;
        };
        counted += 1;
        let generator = Generator::new(worldgen, target.seed)?;
        let sections = generator.biomes(target.x, target.z)?;
        if sections.iter().flatten().any(|id| cave_ids.contains(id)) {
            cave_chunks += 1;
        }
        match mineshaft_pieces(worldgen, &generator, target)? {
            MineShaft::NotPlacement => {}
            MineShaft::NoStart => mine_placement_chunks += 1,
            MineShaft::Pieces(lo, hi) => {
                mine_placement_chunks += 1;
                mine_chunks += 1;
                for (margin, counter) in [(24, &mut mine_24), (48, &mut mine_48)] {
                    let cut = (ocean_floor_min - margin).div_euclid(16) * 16;
                    if lo < cut && hi >= cut {
                        *counter += 1;
                    }
                }
            }
        }
    }

    let percent = |count: usize| count as f64 / counted.max(1) as f64 * 100.0;
    println!(
        "扫描 {counted} 个区块（每个种子从原点按方环铺开；`minecraft:mineshafts` 的 \
         `frequency` 只有 0.004）：\n\
         - 含洞穴群系（繁茂/滴水石/硫磺/深暗）：**{cave_chunks} 个（{:.1}%）**\n\
         - 有矿井起点的区块：{mine_chunks} 个，其中放置区块 {mine_placement_chunks} 个\n\
         - 矿井结构件跨过地表带下边界（余量 24）：**{mine_24} 个（{:.2}%）**\n\
         - 矿井结构件跨过地表带下边界（余量 48）：**{mine_48} 个（{:.2}%）**\n\
         \n> 比例都在**全部扫描区块**上算（不是只在有矿井的区块上），因为「这个区块要不要\
         额外生成深层」是在全部区块上摊的。",
        percent(cave_chunks),
        percent(mine_24),
        percent(mine_48)
    );
    Ok(())
}

/// 扫描用的区块列表：每个种子从原点按方环铺开，共 `count` 个。
/// The chunks to scan: `count` positions per seed, walked as square rings from the origin.
fn scan_targets(seeds: &[i64], count: usize) -> Vec<Target> {
    let mut out = Vec::with_capacity(count * seeds.len());
    for &seed in seeds {
        let mut ring = 0i32;
        let mut found = 0usize;
        while found < count {
            for (x, z) in ring_positions(ring) {
                if found == count {
                    break;
                }
                out.push(Target {
                    seed,
                    x,
                    z,
                    biome: "scan",
                });
                found += 1;
            }
            ring += 1;
        }
    }
    out
}

/// 一个区块的废弃矿井情况。
///
/// The abandoned-mineshaft status of one chunk.
enum MineShaft {
    /// 不是矿井的放置区块。 Not a placement chunk for mineshafts.
    NotPlacement,
    /// 是放置区块，但这次采样没有生成起点（群系或高度不满足）。
    /// A placement chunk whose start did not generate (biome or height gate).
    NoStart,
    /// 有起点，带着结构件的 y 范围。 A start exists, with its pieces' y span.
    Pieces(i32, i32),
}

/// 一个区块里废弃矿井结构件的 y 范围（`None` 表示这个区块没有矿井）。
///
/// 官方由 `structure_starts` 阶段决定哪个区块有矿井起点，这里用同一个布局函数重建：
/// 先按区块的群系决定普通还是恶地类型，再调 `pieces::mineshaft::generate`。
///
/// The y span of a chunk's abandoned-mineshaft pieces (`None` when there is none). Vanilla
/// decides which chunk holds the start during `structure_starts`; this rebuilds it with the
/// same layout function, picking the normal or mesa variant from the chunk's biome.
fn mineshaft_pieces(
    worldgen: &Worldgen,
    generator: &Generator<'_>,
    target: Target,
) -> Result<MineShaft, String> {
    let _ = worldgen;
    // 只有矿井的放置区块才可能有起点；用结构放置判定，和游戏一致。
    // Only a placement chunk can hold a start; use the same placement test as the game.
    let state = mineshaft_state()?;
    if !state.is_structure_chunk("minecraft:mineshafts", target.seed, target.x, target.z) {
        return Ok(MineShaft::NotPlacement);
    }
    // 恶地群系用 `mineshaft_mesa`，否则用 `mineshaft`。
    // Badlands gets `mineshaft_mesa`, everything else `mineshaft`.
    let sections = generator.biomes(target.x, target.z)?;
    let mesa_id = registries::biome::NAMES
        .iter()
        .position(|name| name.contains("badlands"))
        .map(|id| id as u32);
    let mesa = mesa_id.is_some_and(|id| sections.iter().flatten().any(|&cell| cell == id));
    let surface_height = if mesa {
        let cx = target.x * 16 + 8;
        let cz = target.z * 16 + 8;
        generator.first_occupied_height(cx, cz)? + 1
    } else {
        0
    };
    let pieces = mc_bamboo_worldgen::structure::pieces::mineshaft::generate(
        target.seed,
        target.x,
        target.z,
        mesa,
        surface_height,
    );
    if pieces.is_empty() {
        return Ok(MineShaft::NoStart);
    }
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for piece in &pieces {
        lo = lo.min(piece.bb[1]);
        hi = hi.max(piece.bb[4]);
    }
    Ok(MineShaft::Pieces(lo, hi))
}

/// 矿井的结构放置表（只解析一次）。
/// The mineshaft structure-placement table, parsed once.
fn mineshaft_state() -> Result<mc_bamboo_worldgen::structure::StructureState, String> {
    static STATE: std::sync::OnceLock<
        Result<mc_bamboo_worldgen::structure::StructureState, String>,
    > = std::sync::OnceLock::new();
    STATE
        .get_or_init(|| {
            let root: serde_json::Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN)
                .map_err(|error| format!("worldgen json: {error}"))?;
            // `StructureSets::parse` 要的是 `structure_set` 那个对象，不是整个 root。
            // `StructureSets::parse` wants the `structure_set` object, not the whole root.
            let sets_value = root
                .get("structure_set")
                .ok_or_else(|| "worldgen has no structure_set".to_owned())?;
            let sets = mc_bamboo_worldgen::structure::StructureSets::parse(sets_value)
                .map_err(|error| format!("structure sets: {error}"))?;
            Ok(mc_bamboo_worldgen::structure::StructureState::new(sets))
        })
        .clone()
}

/// 一条水平切线方案：用几条 y 线把一列切开。
/// 队长要比较的四种切法。每种都归纳成**一个必须生成的下界**——理由见
/// [`CutPlan::required_floor`]。
///
/// One horizontal cut plan: how many y lines split a column. Each plan reduces to a single
/// required lower bound; see [`CutPlan::required_floor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutPlan {
    /// 一条线：地表带下边界。 One line: the surface band's lower edge.
    SurfaceBand,
    /// 两条线：再加 y=0。 Two lines: plus y=0.
    PlusZero,
    /// 三条线：再加 y=-32。 Three lines: plus y=-32.
    PlusMinus32,
    /// 每 16 格一段各一条线（共 23 条），等于不裁。
    /// One line per 16-block segment (23 of them), i.e. no cutting.
    EverySegment,
}

impl CutPlan {
    const ALL: [Self; 4] = [
        Self::SurfaceBand,
        Self::PlusZero,
        Self::PlusMinus32,
        Self::EverySegment,
    ];

    /// 报告里的名字。 The name used in reports.
    fn name(self) -> &'static str {
        match self {
            Self::SurfaceBand => "1 条线（地表带）",
            Self::PlusZero => "2 条线（+y=0）",
            Self::PlusMinus32 => "3 条线（+y=-32）",
            Self::EverySegment => "每段一条（23 条）",
        }
    }

    /// 这个方案在某一列上必须生成的最低 y。
    ///
    /// `cut` 是地表带的下边界（已对齐 16），`min_y` 是世界下界。
    ///
    /// **为什么只需要一个下界**：切割线以下的段只有在玩家靠近时才生成，而「玩家 128 格
    /// 以内」对一条竖直线来说是一段**连续**区间；地表带又总是生成。两者取并，仍然是从某个
    /// y 到世界顶部的一段连续区间。所以「必须生成」永远是 `[floor, 世界顶部]`。
    ///
    /// `player` 给出玩家相对这一列的水平距离和玩家 y；`None` 表示没有玩家靠近（站着不动
    /// 且这一列在视距外，或纯粹问「完全不靠近时要生成多少」）。
    ///
    /// The lowest y this plan must generate for one column. `cut` is the surface band's lower
    /// edge and `min_y` the world floor. A single bound suffices because the deferred
    /// segments are only generated when a player is close, and "within 128 blocks" meets a
    /// vertical line in one contiguous span; the surface band is always generated, so the
    /// union is still one contiguous run from `floor` to the top. `player` is the horizontal
    /// distance from the player and the player's y; `None` means no player nearby.
    fn required_floor(self, cut: i32, min_y: i32, player: Option<(f64, i32)>) -> i32 {
        let base = self.base_floor(cut, min_y);
        let Some((distance, player_y)) = player else {
            return base;
        };
        // 球体裁剪：水平距离 `distance` 时，垂直方向最多还能覆盖这么远。
        // Sphere clipping: at this horizontal distance the vertical reach is at most this.
        let reach_sq =
            crate::wg_perf::REQUIRED_RADIUS * crate::wg_perf::REQUIRED_RADIUS - distance * distance;
        if reach_sq <= 0.0 {
            return base;
        }
        let reach = reach_sq.sqrt();
        // 这一段段往下看：段 `[lo, lo+16)` 的任意一点离玩家的垂直距离不超过 reach 就必须生成。
        // 用段的上边界判，保守（宁可多生成，不少生成）。
        // Walk the segments downwards; a segment whose top is within `reach` of the player's
        // y must be generated. Using the top edge is conservative.
        let mut floor = base;
        while floor > min_y {
            let next = floor - 16;
            if (floor as f64 - player_y as f64).abs() > reach {
                break;
            }
            floor = next;
        }
        floor
    }

    /// 只看切法本身（玩家不靠近）时必须生成的最低 y。
    ///
    /// The lowest y this plan generates when no player is nearby.
    fn base_floor(self, cut: i32, min_y: i32) -> i32 {
        match self {
            Self::SurfaceBand => cut,
            Self::PlusZero => cut.min(0),
            Self::PlusMinus32 => cut.min(0).min(-32),
            Self::EverySegment => min_y,
        }
    }
}

/// 三种玩家情形。
/// The three player situations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerSituation {
    /// 高空直线飞行，y=200。 Straight flight at y=200.
    Flying,
    /// 地面直线行走（y 取该列的地表高度）。
    /// Walking on the ground (y is the column's surface height).
    Walking,
    /// 站着不动。 Standing still.
    Standing,
}

impl PlayerSituation {
    const ALL: [Self; 3] = [Self::Flying, Self::Walking, Self::Standing];

    /// 报告里的名字。 The name used in reports.
    fn name(self) -> &'static str {
        match self {
            Self::Flying => "高空直线飞行 (y=200)",
            Self::Walking => "地面直线行走",
            Self::Standing => "站着不动",
        }
    }

    /// 玩家 y。行走和站立都用该列的地表高度，由调用方给。
    /// The player's y; walking and standing both use the column's surface height.
    fn player_y(self, surface_y: i32) -> i32 {
        match self {
            Self::Flying => 200,
            Self::Walking | Self::Standing => surface_y,
        }
    }
}

/// 列中心到玩家的水平距离（方块）。
///
/// 列偏移 `dx` 表示它的中心在 `dx * 16 + 8` 处（相对玩家所在列的中心）。
///
/// The horizontal distance from a column's centre to the player. A column at offset `dx`
/// has its centre at `dx * 16 + 8` relative to the player's column centre.
fn horizontal_distance(chunk_dx: i32, chunk_dz: i32) -> f64 {
    let dx = f64::from(chunk_dx * CHUNK_WIDTH + CHUNK_WIDTH / 2);
    let dz = f64::from(chunk_dz * CHUNK_WIDTH + CHUNK_WIDTH / 2);
    (dx * dx + dz * dz).sqrt()
}

/// 一列的水平宽度（方块）。 A column's horizontal width in blocks.
const CHUNK_WIDTH: i32 = 16;

/// 视距（区块）。队长指定的 12。 The view distance in chunks, 12 as the lead specified.
const VIEW_CHUNKS: i32 = 12;

/// 必须生成的最大 3D 距离（方块）。 The largest 3D distance that still forces generation.
const REQUIRED_RADIUS: f64 = 128.0;

/// 一次带记录的生成：中心区块自己走完整条链，3×3 视图由调用方给（游戏里来自快照缓存）。
///
/// One recorded generation: the centre chunk walks the whole chain; the 3x3 view is
/// supplied by the caller (in the game it comes from the snapshot cache).
fn generate_recorded(
    generator: &Generator<'_>,
    target: Target,
    cut: i32,
    view: &[TerrainView; 9],
) -> Result<Record, String> {
    let (cx, cz) = (target.x, target.z);
    let min_y = generator.settings().min_y;
    // 不改调用方设好的实验下界：切割线对照实验靠它切掉深层。
    // Keep the caller's experimental floors: the cut-line comparison relies on them.
    perf::begin(min_y, cut, Phase::Biomes);
    let region = generator.biome_region(cx, cz)?;
    perf::mark(Phase::TerrainSetup);
    let blocks = generator.terrain_with_region(cx, cz, Stage::Carvers, None, &region)?;
    perf::mark(Phase::Features);
    let result = run_features(generator, cx, cz, view)?;
    perf::mark(Phase::LightView);
    let columns = light_columns(cx, cz, view);
    perf::mark(Phase::Light);
    let light = generate_light_from_layers(ChunkPos::new(cx, cz), &columns)?;
    perf::mark(Phase::Assembly);
    let chunk = mc_bamboo_game::worldgen::GeneratedChunk {
        blocks,
        biomes: region.center().sections().to_vec(),
        heightmaps: Default::default(),
    };
    let column = mc_bamboo_game::worldgen::build_region_column(&chunk)?;
    perf::pause();
    black_box(&result);
    black_box(&light);
    black_box(&column);
    Ok(perf::finish().expect("recording was started"))
}

fn view_of(generator: &Generator<'_>, cx: i32, cz: i32) -> Result<[TerrainView; 9], String> {
    let mut view = Vec::with_capacity(9);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let region = generator.biome_region(cx + dx, cz + dz)?;
            let blocks =
                generator.terrain_with_region(cx + dx, cz + dz, Stage::Carvers, None, &region)?;
            view.push(TerrainView {
                blocks,
                biomes: region.center().sections().to_vec(),
            });
        }
    }
    Ok(view.try_into().expect("nine chunks"))
}

fn run_features(
    generator: &Generator<'_>,
    cx: i32,
    cz: i32,
    view: &[TerrainView; 9],
) -> Result<mc_bamboo_worldgen::feature::FeatureResult, String> {
    let input = feature_input(view, cx, cz, generator.settings().min_y);
    generator.features(&input, (1 << DECORATION_STEPS) - 1, None)
}

/// 只算 `features` 那一段（给地物分布测量用），返回耗时和逐地物记录。
///
/// Run just the `features` phase (for the per-feature distribution), returning its
/// wall clock and the per-feature records.
fn measure_features(
    generator: &Generator<'_>,
    target: Target,
    view: &[TerrainView; 9],
    cut: i32,
) -> Result<(f64, Record), String> {
    let min_y = generator.settings().min_y;
    let input = feature_input(view, target.x, target.z, min_y);
    perf::set_floors(Floors::default());
    perf::begin(min_y, cut, Phase::Features);
    let result = generator.features(&input, (1 << DECORATION_STEPS) - 1, None)?;
    let record = perf::finish().expect("recording was started");
    black_box(&result);
    Ok((record.steps_ms.iter().sum(), record))
}

/// 游戏 `light` 阶段现在的输入：3×3 每个区块的位置和逐层方块，只借用不复制。
///
/// The game's `light` stage input now: each 3x3 chunk's position and block
/// layers, borrowed rather than copied.
fn light_columns(cx: i32, cz: i32, view: &[TerrainView; 9]) -> Vec<(ChunkPos, &[Vec<u32>])> {
    view.iter()
        .enumerate()
        .map(|(index, chunk)| {
            let dx = index as i32 % 3 - 1;
            let dz = index as i32 / 3 - 1;
            (ChunkPos::new(cx + dx, cz + dz), chunk.blocks.as_slice())
        })
        .collect()
}

/// 旧的 `light` 阶段输入：复制快照，再逐个走 `build_region_column` 组装完整 `Chunk`。
/// 只给 `--light-compare` 对照用。
///
/// The former `light` stage input: copy each snapshot and assemble a full
/// `Chunk` through `build_region_column`. Only `--light-compare` uses it.
fn assembled_light_view(
    cx: i32,
    cz: i32,
    view: &[TerrainView; 9],
) -> Result<BTreeMap<ChunkPos, Chunk>, String> {
    let mut chunks = BTreeMap::new();
    for (index, chunk) in view.iter().enumerate() {
        let dx = index as i32 % 3 - 1;
        let dz = index as i32 / 3 - 1;
        let generated = chunk.to_generated();
        let column = mc_bamboo_game::worldgen::build_region_column(&generated)?;
        chunks.insert(
            ChunkPos::new(cx + dx, cz + dz),
            column.parts().column.chunk().clone(),
        );
    }
    Ok(chunks)
}

/// `--light-compare`：同一批区块上交替跑旧、新两条光照路径，各取 `repeat` 遍的最小值，
/// 并逐段确认两边的光照完全相同。
///
/// 旧路径 = 复制快照 + `build_region_column` 组装 9 个 `Chunk`（`light_view`）+
/// `generate_light`（`light`）；新路径 = `generate_light_from_layers` 一步（转换和传播都在里面）。
///
/// `--light-compare`: alternate the old and new light paths on the same chunks,
/// keep the minimum of `repeat` runs each, and check that both give identical
/// light. Old = copy snapshots + assemble 9 `Chunk`s via `build_region_column`
/// (`light_view`) + `generate_light` (`light`); new = one
/// `generate_light_from_layers` call (conversion and propagation together).
fn measure_light_compare(
    worldgen: &Worldgen,
    targets: &[Target],
    repeat: usize,
) -> Result<(), String> {
    let mut generators: HashMap<i64, Generator<'_>> = HashMap::new();
    let (mut old_view, mut old_light, mut old_total, mut new_total) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for &target in targets {
        let generator = match generators.entry(target.seed) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Generator::new(worldgen, target.seed)?)
            }
        };
        let view = view_of(generator, target.x, target.z)?;
        let center = ChunkPos::new(target.x, target.z);
        let (mut best_view, mut best_light, mut best_old, mut best_new) =
            (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..repeat.max(1) {
            let start = Instant::now();
            let chunks = assembled_light_view(target.x, target.z, &view)?;
            let middle = Instant::now();
            let old = mc_bamboo_world::light::generate::generate_light(center, &chunks);
            let end = Instant::now();
            best_view = best_view.min((middle - start).as_secs_f64() * 1000.0);
            best_light = best_light.min((end - middle).as_secs_f64() * 1000.0);
            best_old = best_old.min((end - start).as_secs_f64() * 1000.0);

            let start = Instant::now();
            let columns = light_columns(target.x, target.z, &view);
            let new = generate_light_from_layers(center, &columns)?;
            best_new = best_new.min(start.elapsed().as_secs_f64() * 1000.0);
            if old.sky != new.sky || old.block != new.block {
                return Err(format!(
                    "light-compare: old and new light differ at seed {} chunk ({}, {})",
                    target.seed, target.x, target.z
                ));
            }
            black_box((&chunks, &old, &new));
        }
        old_view.push(best_view);
        old_light.push(best_light);
        old_total.push(best_old);
        new_total.push(best_new);
    }
    let stats = |values: &mut Vec<f64>| {
        values.sort_by(f64::total_cmp);
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        (mean, values[values.len() / 2])
    };
    println!(
        "light-compare: {} chunks, min of {} runs each, old and new light identical",
        targets.len(),
        repeat.max(1)
    );
    for (name, values) in [
        ("old light_view", &mut old_view),
        ("old light", &mut old_light),
        ("old light_view + light", &mut old_total),
        ("new light (from layers)", &mut new_total),
    ] {
        let (mean, median) = stats(values);
        println!("  {name:<26} mean {mean:>8.3} ms  median {median:>8.3} ms");
    }
    Ok(())
}

/// 一个区块到 CARVERS 的方块和群系；`features` 和 `light` 都要一份 3×3。
///
/// A chunk's CARVERS blocks and biomes; both `features` and `light` need a 3x3.
#[derive(Clone, Debug)]
struct TerrainView {
    blocks: Vec<Vec<u32>>,
    biomes: Vec<Vec<u32>>,
}

impl TerrainView {
    fn to_generated(&self) -> mc_bamboo_game::worldgen::GeneratedChunk {
        mc_bamboo_game::worldgen::GeneratedChunk {
            blocks: self.blocks.clone(),
            biomes: self.biomes.clone(),
            heightmaps: Default::default(),
        }
    }
}

/// 把 3×3 视图拼成 `FeatureChunks`。
/// Assemble the 3x3 view into `FeatureChunks`.
fn feature_input(
    view: &[TerrainView; 9],
    cx: i32,
    cz: i32,
    min_y: i32,
) -> mc_bamboo_worldgen::feature::FeatureChunks {
    let blocks: [Vec<Vec<u32>>; 9] = std::array::from_fn(|index| view[index].blocks.clone());
    let biomes: [Vec<Vec<u32>>; 9] = std::array::from_fn(|index| view[index].biomes.clone());
    let heights = blocks
        .each_ref()
        .map(|chunk| prime_heightmaps(chunk, min_y, WORLD_HEIGHT));
    mc_bamboo_worldgen::feature::FeatureChunks {
        center_x: cx,
        center_z: cz,
        min_y,
        height: WORLD_HEIGHT,
        blocks,
        biomes,
        heights,
    }
}

/// `OCEAN_FLOOR_WG` 高度图的最小值（`getFirstAvailable`：最高阻挡方块上面一格）。
///
/// The minimum of the `OCEAN_FLOOR_WG` heightmap (`getFirstAvailable`: one above the
/// highest motion-blocking block).
fn ocean_floor_min(generator: &Generator<'_>, target: Target) -> Result<i32, String> {
    let region = generator.biome_region(target.x, target.z)?;
    let blocks =
        generator.terrain_with_region(target.x, target.z, Stage::Carvers, None, &region)?;
    Ok(ocean_floor_min_of(&blocks))
}

fn ocean_floor_min_of(blocks: &[Vec<u32>]) -> i32 {
    let mut min = i32::MAX;
    for column in 0..256 {
        let mut height = MIN_Y;
        for local_y in (0..blocks.len()).rev() {
            if state_facts(blocks[local_y][column]) & mc_bamboo_data::facts::MOTION_BLOCKING != 0 {
                height = MIN_Y + local_y as i32 + 1;
                break;
            }
        }
        min = min.min(height);
    }
    if min == i32::MAX { MIN_Y } else { min }
}

/// 一组记录的逐项中位数。
/// The per-field median of several records.
fn median_record(records: Vec<Record>) -> Record {
    let mut merged = records[0].clone();
    for index in 0..merged.phases_ms.len() {
        let mut values: Vec<f64> = records
            .iter()
            .map(|record| record.phases_ms[index])
            .collect();
        values.sort_by(f64::total_cmp);
        merged.phases_ms[index] = values[values.len() / 2];
    }
    for index in 0..merged.steps_ms.len() {
        let mut values: Vec<f64> = records
            .iter()
            .map(|record| record.steps_ms[index])
            .collect();
        values.sort_by(f64::total_cmp);
        merged.steps_ms[index] = values[values.len() / 2];
    }
    merged
}

/// 辅助测量（高度分布、切割线对照、多线程）只取一部分区块，免得整轮跑太久。
///
/// `limit` 是每轮最多测多少个区块，按等间隔挑。
///
/// The auxiliary measurements (height profile, cut-line comparison, threads) run on a
/// subsample so a full round stays affordable. `limit` is the maximum number of chunks
/// per round, taken at even intervals.
fn subsample(
    targets: &[Target],
    measured: &[Measured],
    limit: usize,
) -> (Vec<Target>, Vec<Measured>) {
    if targets.len() <= limit {
        return (targets.to_vec(), measured.to_vec());
    }
    let step = targets.len() as f64 / limit as f64;
    let mut picked_targets = Vec::with_capacity(limit);
    let mut picked_measured = Vec::with_capacity(limit);
    for index in 0..limit {
        let at = (index as f64 * step) as usize;
        picked_targets.push(targets[at]);
        picked_measured.push(measured[at].clone());
    }
    (picked_targets, picked_measured)
}

/// 按群系找区块：`Generator::biomes` 的段调色板里目标群系过半就算。
///
/// 扫描用螺旋方环（半径逐圈加 1 个区块），一圈里按 `(x, z)` 走完再进下一圈，这样同一圈
/// 能给出足够多的候选；`radius * 6` 的稀疏八方向在群系窄带（山地、丛林）上找不到那么多。
///
/// Find chunks by biome: keep a chunk when the target biome is a majority of its quart
/// cells in the section palettes from `Generator::biomes`. The scan walks square rings
/// one chunk at a time and covers every position of a ring, because the old sparse
/// eight-direction step over `radius * 6` could not find enough chunks of a narrow biome
/// band such as hills or jungle.
fn pick_targets(
    worldgen: &Worldgen,
    seeds: &[i64],
    biomes: &[String],
    per_biome: usize,
) -> Result<Vec<Target>, String> {
    let mut targets = Vec::new();
    for &seed in seeds {
        let generator = Generator::new(worldgen, seed)?;
        for name in biomes {
            let wanted = registries::biome::NAMES
                .iter()
                .position(|known| *known == name)
                .ok_or_else(|| format!("unknown biome {name}"))? as u32;
            let static_name = registries::biome::NAMES[wanted as usize];
            let mut found = 0;
            'rings: for ring in 0..600 {
                for (x, z) in ring_positions(ring) {
                    let sections = generator.biomes(x, z)?;
                    let hits = sections
                        .iter()
                        .flatten()
                        .filter(|&&id| id == wanted)
                        .count();
                    let total: usize = sections.iter().map(Vec::len).sum();
                    if hits * 2 > total {
                        targets.push(Target {
                            seed,
                            x,
                            z,
                            biome: static_name,
                        });
                        found += 1;
                        if found == per_biome {
                            break 'rings;
                        }
                    }
                }
            }
            if found < per_biome {
                return Err(format!(
                    "seed {seed}: only {found} chunks of {name}, wanted {per_biome}"
                ));
            }
        }
    }
    Ok(targets)
}

/// 第 `ring` 圈方环上的全部区块坐标；`ring == 0` 只有原点。
///
/// Every chunk position on square ring `ring`; `ring == 0` is the origin alone.
fn ring_positions(ring: i32) -> Vec<(i32, i32)> {
    if ring == 0 {
        return vec![(0, 0)];
    }
    let mut out = Vec::with_capacity((ring as usize + 1) * 8);
    for x in -ring..=ring {
        out.push((x, -ring));
        out.push((x, ring));
    }
    for z in -ring + 1..ring {
        out.push((-ring, z));
        out.push((ring, z));
    }
    out
}

fn print_phases(measured: &[Measured]) {
    println!("\n## 各阶段耗时（单线程，release，ms/区块）\n");
    println!("| 阶段 | 平均 | 中位数 | p95 |");
    println!("|---|---|---|---|");
    for phase in Phase::ALL {
        let values = column_of(measured, |record| record.phases_ms[phase.index()]);
        let (mean, median, p95) = stats(&values);
        println!("| {} | {mean:.3} | {median:.3} | {p95:.3} |", phase.name());
    }
    let recorded = column_of(measured, Record::total);
    let (mean, median, p95) = stats(&recorded);
    println!("| **记录合计** | {mean:.3} | {median:.3} | {p95:.3} |");
    let wall: Vec<f64> = measured.iter().map(|item| item.total_ms).collect();
    let (mean, median, p95) = stats(&wall);
    println!("| **墙钟合计** | {mean:.3} | {median:.3} | {p95:.3} |");
}

fn print_biomes(measured: &[Measured]) {
    println!("\n## 按群系（中位数 ms/区块）\n");
    println!(
        "| 群系 | 区块数 | 墙钟 | 群系 | 噪声填充 | 地表 | 雕刻器 | 地物 | 光照 | 组装 | 切割线 |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut groups: BTreeMap<&str, Vec<&Measured>> = BTreeMap::new();
    for item in measured {
        groups.entry(item.target.biome).or_default().push(item);
    }
    for (biome, items) in groups {
        let wall = items.iter().map(|item| item.total_ms).collect::<Vec<_>>();
        let biomes = items
            .iter()
            .map(|item| item.record.phases_ms[Phase::Biomes.index()])
            .collect::<Vec<_>>();
        let noise = items
            .iter()
            .map(|item| item.record.do_fill())
            .collect::<Vec<_>>();
        let surface = items
            .iter()
            .map(|item| item.record.phases_ms[Phase::Surface.index()])
            .collect::<Vec<_>>();
        let carver = items
            .iter()
            .map(|item| {
                item.record.phases_ms[Phase::CarverMask.index()]
                    + item.record.phases_ms[Phase::CarverApply.index()]
            })
            .collect::<Vec<_>>();
        let features = items
            .iter()
            .map(|item| {
                item.record.phases_ms[Phase::View.index()]
                    + item.record.phases_ms[Phase::Features.index()]
            })
            .collect::<Vec<_>>();
        let light = items
            .iter()
            .map(|item| {
                item.record.phases_ms[Phase::LightView.index()]
                    + item.record.phases_ms[Phase::Light.index()]
            })
            .collect::<Vec<_>>();
        let assembly = items
            .iter()
            .map(|item| item.record.phases_ms[Phase::Assembly.index()])
            .collect::<Vec<_>>();
        let mut cuts: Vec<f64> = items.iter().map(|item| f64::from(item.cut)).collect();
        cuts.sort_by(f64::total_cmp);
        println!(
            "| {biome} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.0} |",
            items.len(),
            stats(&wall).1,
            stats(&biomes).1,
            stats(&noise).1,
            stats(&surface).1,
            stats(&carver).1,
            stats(&features).1,
            stats(&light).1,
            stats(&assembly).1,
            cuts[cuts.len() / 2]
        );
    }
}

/// 噪声填充按 8 格一层的耗时和「整格空气」计数。
///
/// 耗时用差分的整列求值：从每个带边界一直算到世界顶部，相邻两次的差就是这一带花的
/// 时间。这样和「整块一次求值」的口径一致；每带单采样会重复算插值角点，不能用。
///
/// The noise fill per 8-block band, both timing and the all-air cell count. Timing uses
/// whole-column passes from each band boundary to the top of the world; the difference
/// of consecutive passes is that band's cost. This matches one whole-volume pass, unlike
/// sampling each band alone (which recomputes the interpolation corners).
fn measure_bands(
    worldgen: &Worldgen,
    targets: &[Target],
    repeat: usize,
) -> Result<BandStats, String> {
    let mut per_band: Vec<Vec<f64>> = vec![Vec::new(); Y_BANDS];
    let mut air_bands: Vec<u32> = vec![0; Y_BANDS];
    for target in targets {
        let generator = Generator::new(worldgen, target.seed)?;
        // 每一带取累积耗时的最小值（负载抖动只会把时间推高，最小值最接近干净的数）。
        // Take the minimum cumulative time per boundary: other load only pushes timings
        // up, so the minimum is closest to the clean number.
        let mut cumulative = vec![0.0f64; Y_BANDS + 1];
        for (band, slot) in cumulative.iter_mut().enumerate() {
            let from_y = MIN_Y + band as i32 * 8;
            let mut times = Vec::with_capacity(repeat);
            for _ in 0..repeat {
                times.push(generator.sample_density_from(target.x, target.z, from_y)?);
            }
            *slot = times.iter().copied().fold(f64::MAX, f64::min);
        }
        for band in 0..Y_BANDS {
            per_band[band].push((cumulative[band] - cumulative[band + 1]).max(0.0));
        }
        let air = generator.density_air_bands(target.x, target.z)?;
        for (band, &is_air) in air.iter().enumerate() {
            if is_air {
                air_bands[band] += 1;
            }
        }
    }
    Ok(BandStats {
        ms: per_band
            .into_iter()
            .map(|values| stats(&values).1)
            .collect(),
        air_bands,
        chunks: targets.len(),
    })
}

/// 噪声填充的高度分布结果。
/// The noise fill's height profile.
struct BandStats {
    /// 每个高度带的中位数耗时。 Per-band median milliseconds.
    ms: Vec<f64>,
    /// 每个高度带里「整格空气」的区块数。 Chunks whose cell is entirely air, per band.
    air_bands: Vec<u32>,
    /// 总区块数。 Total chunks.
    chunks: usize,
}

/// 打印噪声填充的高度分布。
/// Print the noise fill's height distribution.
fn print_bands(bands: &BandStats, measured: &[Measured]) {
    println!("\n## 噪声填充按高度（每 8 格一层，中位数 ms/区块）\n");
    println!("| y 范围 | 耗时 | 占整块噪声填充 | 整格空气的区块 | 分类 |");
    println!("|---|---|---|---|---|");
    let total: f64 = bands.ms.iter().sum();
    let mut full: Vec<f64> = measured
        .iter()
        .map(|item| item.record.phases_ms[Phase::NoiseDensity.index()])
        .collect();
    full.sort_by(f64::total_cmp);
    let reference = full[full.len() / 2];
    for (band, &ms) in bands.ms.iter().enumerate() {
        let from_y = MIN_Y + band as i32 * 8;
        let to_y = from_y + 7;
        let share = if reference > 0.0 {
            ms / reference * 100.0
        } else {
            0.0
        };
        println!(
            "| {from_y}..{to_y} | {ms:.3} | {share:.1}% | {}/{} | {} |",
            bands.air_bands[band],
            bands.chunks,
            classify_band(from_y, to_y, measured)
        );
    }
    println!("| **合计** | {total:.3} | — | — | — |");
}

/// 给一个高度带分类：天空层 / 地表附近 / 地下。
/// Classify a band: sky, near the surface, or underground.
fn classify_band(from_y: i32, to_y: i32, measured: &[Measured]) -> &'static str {
    let mut above = 0;
    let mut near = 0;
    let mut below = 0;
    for item in measured {
        if from_y > item.ocean_floor_min {
            above += 1;
        } else if to_y + 24 >= item.ocean_floor_min {
            near += 1;
        } else {
            below += 1;
        }
    }
    if above == measured.len() {
        "天空"
    } else if below == measured.len() {
        "地下"
    } else if near == measured.len() {
        "地表附近"
    } else {
        "混合"
    }
}

fn print_features(
    worldgen: &Worldgen,
    targets: &[Target],
    repeat: usize,
    measured: &[Measured],
) -> Result<(), String> {
    let records = measure_feature_runs(worldgen, targets, measured, repeat)?;
    println!("\n## 地物按 decoration step\n");
    println!("| step | 名称 | 中位数 ms | 候选位置数 |");
    println!("|---|---|---|---|");
    for step in 0..DECORATION_STEPS {
        let values: Vec<f64> = records
            .iter()
            .map(|(_, record)| record.steps_ms[step])
            .collect();
        let (_, median, _) = stats(&values);
        let candidates: u64 = records
            .iter()
            .map(|(_, record)| record.steps_candidates[step])
            .sum();
        println!(
            "| {step} | {} | {median:.3} | {candidates} |",
            step_name(step)
        );
    }

    println!("\n## 地物按 placed feature\n");
    println!(
        "| step | placed feature | 类型 | 候选数 | 高度范围 | 地表带占比 | 中位数 ms | 分类 |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let rows = aggregate_features(&records);
    for row in &rows {
        println!(
            "| {} | {} | {} | {} | {}..{} | {:.0}% | {:.3} | {} |",
            row.step,
            row.name,
            row.kind,
            row.candidates,
            row.min_y,
            row.max_y,
            row.above_share * 100.0,
            row.ms,
            row.band
        );
    }
    let mut surface = 0.0;
    let mut deep = 0.0;
    for row in &rows {
        let above = row.ms * row.above_share;
        surface += above;
        deep += row.ms - above;
    }
    println!(
        "\n地表带合计 {surface:.3} ms/区块，深层带合计 {deep:.3} ms/区块（逐地物中位数 × 高度占比估）"
    );
    Ok(())
}

struct FeatureRow {
    step: usize,
    name: String,
    kind: String,
    candidates: u64,
    above_cut: u64,
    min_y: i32,
    max_y: i32,
    above_share: f64,
    ms: f64,
    band: &'static str,
}

fn aggregate_features(records: &[(f64, Record)]) -> Vec<FeatureRow> {
    let registry = mc_bamboo_worldgen::feature::registry::Registry::parse(
        serde_json::from_str::<serde_json::Value>(mc_bamboo_data::worldgen::WORLDGEN)
            .expect("worldgen json")
            .as_object()
            .expect("worldgen object"),
    )
    .expect("feature registry");
    let mut rows: BTreeMap<u32, FeatureRow> = BTreeMap::new();
    let mut times: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for (_, record) in records {
        for feature in &record.features {
            let row = rows.entry(feature.placed).or_insert_with(|| FeatureRow {
                step: feature.step,
                name: registry
                    .placed(feature.placed)
                    .map(|placed| placed.name.clone())
                    .unwrap_or_else(|| format!("placed#{}", feature.placed)),
                kind: registry
                    .placed(feature.placed)
                    .and_then(|placed| {
                        mc_bamboo_worldgen::feature::run::resolve_feature(
                            &registry,
                            &placed.feature,
                        )
                    })
                    .map(|(kind, _)| kind)
                    .unwrap_or_else(|| "?".to_owned()),
                candidates: 0,
                above_cut: 0,
                min_y: i32::MAX,
                max_y: i32::MIN,
                above_share: 0.0,
                ms: 0.0,
                band: "?",
            });
            row.candidates += feature.candidates;
            row.above_cut += feature.above_cut;
            if feature.candidates > 0 {
                row.min_y = row.min_y.min(feature.min_y);
                row.max_y = row.max_y.max(feature.max_y);
            }
            times
                .entry(feature.placed)
                .or_default()
                .push(feature.place_ms + feature.modifier_ms);
        }
    }
    let mut out = Vec::new();
    for (placed, mut row) in rows {
        let mut values = times.remove(&placed).unwrap_or_default();
        values.sort_by(f64::total_cmp);
        row.ms = if values.is_empty() {
            0.0
        } else {
            values[values.len() / 2]
        };
        row.above_share = if row.candidates == 0 {
            0.0
        } else {
            row.above_cut as f64 / row.candidates as f64
        };
        if row.min_y == i32::MAX {
            row.min_y = 0;
            row.max_y = 0;
        }
        row.band = classify_feature(&row);
        out.push(row);
    }
    out.sort_by(|a, b| (a.step, &a.name).cmp(&(b.step, &b.name)));
    out
}

/// 逐个区块单独跑 `features` 并留记录（整块生成里的记录只留最后一次，不够做统计）。
///
/// Run `features` per chunk on its own and keep the records (the whole-chunk recorder
/// keeps only one record, not enough for statistics).
fn measure_feature_runs(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    repeat: usize,
) -> Result<Vec<(f64, Record)>, String> {
    let mut out = Vec::with_capacity(targets.len());
    for (target, item) in targets.iter().zip(measured) {
        let generator = Generator::new(worldgen, target.seed)?;
        let view = view_of(&generator, target.x, target.z)?;
        let mut records: Vec<(f64, Record)> = Vec::with_capacity(repeat);
        for _ in 0..repeat {
            records.push(measure_features(&generator, *target, &view, item.cut)?);
        }
        records.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.push(records.swap_remove(records.len() / 2));
    }
    Ok(out)
}

/// 按高度分布给地物分类。
///
/// 有一类地物的「候选位置」不是它真正改写的方块位置：`freeze_top_layer` 的候选 y 恒为
/// `minY`，它自己在 `place` 里逐列读 `MOTION_BLOCKING` 再改地表；`monster_room` 的候选是
/// 房间原点，房间本身跨好几层。按候选高度分类对它们是误导，单独标成「扫描型」。
///
/// Classify a feature by its height distribution. Some features' candidates are not the
/// blocks they actually rewrite: `freeze_top_layer`'s candidate y is always `minY` while
/// it walks each column's `MOTION_BLOCKING` height, and `monster_room`'s candidate is the
/// room origin with the room spanning several layers. Classifying those by candidate
/// height would mislead, so they are marked as scan-type instead.
fn classify_feature(row: &FeatureRow) -> &'static str {
    if row.candidates == 0 {
        return "无候选";
    }
    match row.kind.as_str() {
        "minecraft:freeze_top_layer" | "minecraft:monster_room" => return "扫描型",
        _ => {}
    }
    if row.above_share > 0.95 {
        "地表"
    } else if row.above_share < 0.05 {
        "地下"
    } else {
        "跨越"
    }
}

fn step_name(step: usize) -> &'static str {
    mc_bamboo_worldgen::feature::registry::STEP_NAMES
        .get(step)
        .copied()
        .unwrap_or("?")
}

/// 每个余量下、每个阶段的两组耗时（完整生成、半成品）。
/// For each margin and phase, the full and the partial timings.
type CutPhase = BTreeMap<i32, BTreeMap<usize, (Vec<f64>, Vec<f64>)>>;

/// 「只生成切割线以上」的对照：余量 16 / 24 / 32 / 48 各量一遍。
///
/// 完整生成和半成品交替跑（同一次循环里各跑一遍），两边都取**最小值**：机器上有别的
/// 负载，抖动只会把时间推高，交替跑还能让两边看到相近的负载。每个区块各算自己的比例，
/// 再按群系取中位数。除了整块比例，还给出**逐阶段的绝对耗时**，因为光照和组装里有一部分
/// 工作（装 3×3 视图、重打包调色板）不受切割线影响，只看整块比例会把这个掩盖掉。
///
/// The "only above the cut line" comparison at margins 16 / 24 / 32 / 48. The full and
/// the partial generation alternate inside the loop and both use the **minimum**: other
/// load only pushes timings up, and alternating makes both see similar load. Each chunk
/// contributes its own ratio, then the per-biome median is taken. Per-phase absolute
/// times are printed too: part of the light and assembly work (converting the 3x3 view,
/// repacking palettes) is independent of the cut line, and a whole-chunk ratio hides it.
fn measure_cut_lines(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 只生成切割线以上（占完整生成的比例）\n");
    let mut biomes: Vec<&str> = Vec::new();
    for target in targets {
        if !biomes.contains(&target.biome) {
            biomes.push(target.biome);
        }
    }
    let mut ratios: BTreeMap<i32, Vec<(&'static str, f64)>> = BTreeMap::new();
    let mut per_phase: CutPhase = BTreeMap::new();
    let mut totals: BTreeMap<i32, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for margin in [16, 24, 32, 48] {
        for (target, item) in targets.iter().zip(measured) {
            let generator = Generator::new(worldgen, target.seed)?;
            let view = view_of(&generator, target.x, target.z)?;
            let cut = (item.ocean_floor_min - margin).div_euclid(16) * 16;
            let floors = Floors {
                noise: Some(cut),
                surface: Some(cut),
                carver: Some(cut),
                feature: Some(cut),
            };
            let mut full = f64::MAX;
            let mut partial = f64::MAX;
            let mut full_record: Option<Record> = None;
            let mut partial_record: Option<Record> = None;
            for _ in 0..repeat {
                perf::set_floors(Floors::default());
                let start = Instant::now();
                let record = generate_recorded(&generator, *target, item.cut, &view)?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                if elapsed < full {
                    full = elapsed;
                    full_record = Some(record);
                }

                perf::set_floors(floors);
                let start = Instant::now();
                let record = generate_recorded(&generator, *target, cut, &view)?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                if elapsed < partial {
                    partial = elapsed;
                    partial_record = Some(record);
                }
            }
            perf::set_floors(Floors::default());
            ratios
                .entry(margin)
                .or_default()
                .push((target.biome, partial / full.max(0.001) * 100.0));
            let (full_record, partial_record) = match (full_record, partial_record) {
                (Some(full), Some(partial)) => (full, partial),
                _ => continue,
            };
            let phases = per_phase.entry(margin).or_default();
            for phase in Phase::ALL {
                let entry = phases
                    .entry(phase.index())
                    .or_insert_with(|| (Vec::new(), Vec::new()));
                entry.0.push(full_record.phases_ms[phase.index()]);
                entry.1.push(partial_record.phases_ms[phase.index()]);
            }
            let entry = totals
                .entry(margin)
                .or_insert_with(|| (Vec::new(), Vec::new()));
            entry.0.push(full);
            entry.1.push(partial);
        }
    }

    print!("| 余量 |");
    for biome in &biomes {
        print!(" {} |", biome.replace("minecraft:", ""));
    }
    println!(" 总体 |");
    print!("|---|");
    for _ in 0..biomes.len() {
        print!("---|");
    }
    println!("---|");
    for margin in [16, 24, 32, 48] {
        let rows = ratios.get(&margin).cloned().unwrap_or_default();
        print!("| {margin} |");
        for biome in &biomes {
            let values: Vec<f64> = rows
                .iter()
                .filter(|(name, _)| name == biome)
                .map(|(_, ratio)| *ratio)
                .collect();
            print!(" {:.1}% |", stats(&values).1);
        }
        let all: Vec<f64> = rows.iter().map(|(_, ratio)| *ratio).collect();
        println!(" {:.1}% |", stats(&all).1);
    }

    println!("\n### 逐阶段的绝对耗时（中位数 ms/区块，完整 → 半成品）\n");
    print!("| 余量 |");
    for phase in Phase::ALL {
        print!(" {} |", phase.name());
    }
    println!(" 整块 |");
    print!("|---|");
    for _ in Phase::ALL {
        print!("---|");
    }
    println!("---|");
    for margin in [16, 24, 32, 48] {
        let phases = per_phase.get(&margin).cloned().unwrap_or_default();
        print!("| {margin} |");
        for phase in Phase::ALL {
            let entry = phases.get(&phase.index()).cloned().unwrap_or_default();
            print!(" {:.2}→{:.2} |", stats(&entry.0).1, stats(&entry.1).1);
        }
        let entry = totals.get(&margin).cloned().unwrap_or_default();
        println!(" {:.1}→{:.1} |", stats(&entry.0).1, stats(&entry.1).1);
    }
    Ok(())
}

/// 天空层：高于含水层 `skipSamplingAboveY` 的噪声 cell 里有多少个整格空气。
///
/// 「整格空气」按密度判：这一格 8 行 × 256 列里没有任何一个密度 > 0 的点。含水层在
/// 这个高度以上直接返回全局流体（y 高于海平面就是空气），所以密度 <= 0 就是空气。
/// 时间占比用逐带单独成批求值的耗时算；`skipSamplingAboveY` 取各区块记录值的中位数。
///
/// The sky layer: how many of the noise cells above the aquifer's `skipSamplingAboveY`
/// are entirely air. "Entirely air" is judged on the density: no point in the cell's
/// eight rows by 256 columns has density > 0. Above that height the aquifer returns the
/// global fluid directly (air above sea level), so a non-positive density means air.
fn measure_sky(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    bands: &BandStats,
) -> Result<(), String> {
    println!("\n## 天空层\n");
    let mut skips: Vec<f64> = measured
        .iter()
        .filter_map(|item| item.record.skip_above_y)
        .map(f64::from)
        .collect();
    if skips.is_empty() {
        println!("没有记录到 `skipSamplingAboveY`（`perf` 特性没开？）");
        return Ok(());
    }
    skips.sort_by(f64::total_cmp);
    let skip = skips[skips.len() / 2] as i32;
    let mut above_cells = 0u64;
    let mut air_cells = 0u64;
    let mut above_ms = 0.0;
    let mut all_ms = 0.0;
    for (band, &ms) in bands.ms.iter().enumerate() {
        let from_y = MIN_Y + band as i32 * 8;
        all_ms += ms;
        if from_y <= skip {
            continue;
        }
        above_cells += bands.chunks as u64;
        air_cells += u64::from(bands.air_bands[band]);
        above_ms += ms;
    }
    println!(
        "`skipSamplingAboveY` 中位数 {skip}；它之上的噪声 cell {above_cells} 个，其中整格空气 \
         {air_cells} 个（{:.1}%），占噪声填充 {:.1}% 的时间",
        air_cells as f64 / above_cells.max(1) as f64 * 100.0,
        above_ms / all_ms.max(0.001) * 100.0
    );

    // 逐区块数整格空气的 cell，乘逐带耗时，得到「全跳过」能给噪声填充省多少（上界）。
    // Count all-air cells per chunk and multiply by the per-band timings for the upper
    // bound on what skipping every all-air cell saves in the noise fill.
    let mut saved_ms = 0.0;
    let mut noise_ms = 0.0;
    let mut total_ms = 0.0;
    for (target, item) in targets.iter().zip(measured) {
        let generator = Generator::new(worldgen, target.seed)?;
        let air = generator.density_air_bands(target.x, target.z)?;
        for (band, &is_air) in air.iter().enumerate() {
            if is_air {
                saved_ms += bands.ms[band];
            }
        }
        noise_ms += item.record.phases_ms[Phase::NoiseDensity.index()];
        total_ms += item.total_ms;
    }
    println!(
        "若整格空气的 cell 全跳过：噪声填充省 {:.2} ms/区块（{:.1}%），整块生成省 {:.1}%\
         （整块 {:.2} ms/区块）",
        saved_ms / targets.len() as f64,
        saved_ms / noise_ms.max(0.001) * 100.0,
        saved_ms / total_ms.max(0.001) * 100.0,
        total_ms / targets.len() as f64
    );
    Ok(())
}

/// 多线程吞吐量：每线程一个 `Generator` 和一份 `Worldgen`，一起生成同一批区块。
///
/// 每个线程自己建 3×3 视图（和单线程那组一样，视图不算进单区块成本）。
///
/// Multi-thread throughput: one `Generator` and one `Worldgen` per thread, all
/// generating the same batch. Each worker builds its own 3x3 view, the same way the
/// single-threaded group does, and the view stays outside the per-chunk cost.
fn measure_threads(
    worldgen: &Worldgen,
    targets: &[Target],
    threads: usize,
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 多线程吞吐量\n");
    let generator = Generator::new(worldgen, targets[0].seed)?;
    let mut views: Vec<[TerrainView; 9]> = Vec::with_capacity(targets.len());
    for target in targets {
        views.push(view_of(&generator, target.x, target.z)?);
    }
    let mut single = Vec::new();
    perf::set_floors(Floors::default());
    for _ in 0..repeat {
        let start = Instant::now();
        for (target, view) in targets.iter().zip(&views) {
            black_box(generate_recorded(&generator, *target, 0, view)?);
        }
        single.push(targets.len() as f64 / start.elapsed().as_secs_f64());
    }
    single.sort_by(f64::total_cmp);

    let mut parallel = Vec::new();
    for _ in 0..repeat {
        let next = Arc::new(AtomicUsize::new(0));
        let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let start = Instant::now();
        // 复制一份区块列表，线程里不用借外层的切片。
        // Copy the target list so the workers do not borrow the outer slice.
        let batch: Arc<Vec<Target>> = Arc::new(targets.to_vec());
        thread::scope(|scope| {
            for _ in 0..threads {
                let next = Arc::clone(&next);
                let errors = Arc::clone(&errors);
                let batch = Arc::clone(&batch);
                scope.spawn(move || {
                    let worldgen = Worldgen::load().expect("worldgen data");
                    let generator = Generator::new(&worldgen, batch[0].seed).expect("generator");
                    // 下界是按线程存的，每个工作线程自己设一遍。
                    // The floors are thread-local, so every worker sets its own.
                    perf::set_floors(Floors::default());
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(target) = batch.get(index).copied() else {
                            break;
                        };
                        let view = match view_of(&generator, target.x, target.z) {
                            Ok(view) => view,
                            Err(error) => {
                                errors.lock().expect("lock").push(error);
                                break;
                            }
                        };
                        if let Err(error) = generate_recorded(&generator, target, 0, &view) {
                            errors.lock().expect("lock").push(error);
                            break;
                        }
                    }
                });
            }
        });
        if let Some(error) = errors.lock().expect("lock").first() {
            return Err(error.clone());
        }
        parallel.push(targets.len() as f64 / start.elapsed().as_secs_f64());
    }
    parallel.sort_by(f64::total_cmp);
    println!(
        "1 线程 {:.1} 区块/秒，{threads} 线程 {:.1} 区块/秒（{} 个区块）",
        single[single.len() / 2],
        parallel[parallel.len() / 2],
        targets.len()
    );
    Ok(())
}

fn print_json(measured: &[Measured], band_ms: &[f64]) {
    println!("\n```json");
    println!("{{");
    println!("  \"chunks\": {},", measured.len());
    print!("  \"bands_ms\": [");
    for (index, ms) in band_ms.iter().enumerate() {
        if index > 0 {
            print!(", ");
        }
        print!("{ms:.4}");
    }
    println!("],");
    print!("  \"phases\": {{");
    let mut first = true;
    for phase in Phase::ALL {
        let values = column_of(measured, |record| record.phases_ms[phase.index()]);
        if !first {
            print!(", ");
        }
        first = false;
        print!("\"{}\": {:.4}", phase.name(), stats(&values).1);
    }
    println!("}}");
    println!("}}");
    println!("```");
}

fn column_of(measured: &[Measured], get: impl Fn(&Record) -> f64) -> Vec<f64> {
    measured.iter().map(|item| get(&item.record)).collect()
}

/// 平均值、中位数、p95。
/// Mean, median and p95.
fn stats(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)];
    (mean, median, p95)
}

#[cfg(test)]
mod cut_plan_tests {
    use super::*;

    const MIN_Y: i32 = -64;

    /// 没有玩家靠近时，各切法给出的下界就是它自己那条线。
    /// With no player nearby each plan's floor is its own lowest line.
    #[test]
    fn base_floors_follow_the_plan() {
        let cut = 32;
        assert_eq!(CutPlan::SurfaceBand.base_floor(cut, MIN_Y), 32);
        assert_eq!(CutPlan::PlusZero.base_floor(cut, MIN_Y), 0);
        assert_eq!(CutPlan::PlusMinus32.base_floor(cut, MIN_Y), -32);
        assert_eq!(CutPlan::EverySegment.base_floor(cut, MIN_Y), MIN_Y);
        // 地表带下边界在地面以下时，`min` 要取到它。
        // When the surface band's edge is below y=0 the min has to reach it.
        assert_eq!(CutPlan::PlusZero.base_floor(-16, MIN_Y), -16);
    }

    /// 不裁的切法任何时候都返回世界下界。
    /// The uncut plan always returns the world floor.
    #[test]
    fn every_segment_never_cuts() {
        for distance in [0.0, 64.0, 128.0, 400.0] {
            for y in [-60, 0, 100, 200] {
                assert_eq!(
                    CutPlan::EverySegment.required_floor(32, MIN_Y, Some((distance, y))),
                    MIN_Y
                );
            }
        }
    }

    /// 玩家就在脚下（水平距离 0）时，垂直方向能覆盖整个 128 格，所以深层段都会被拉进来。
    /// A player directly below reaches the full 128 blocks vertically, pulling the deep
    /// segments in.
    #[test]
    fn a_player_directly_below_reaches_the_deep_segments() {
        // 地表带下边界 32，玩家在 y=0：128 格垂直覆盖足够拉到 -64。
        // Surface edge 32 with the player at y=0: 128 blocks of vertical reach reaches -64.
        let floor = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((0.0, 0)));
        assert_eq!(floor, MIN_Y);
    }

    /// 玩家很远（超过 128 格）时不影响结果。
    /// A player beyond 128 blocks changes nothing.
    #[test]
    fn a_distant_player_changes_nothing() {
        for distance in [129.0, 200.0, 1000.0] {
            assert_eq!(
                CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((distance, 0))),
                32
            );
        }
    }

    /// 水平距离越大，能下探的垂直距离越小。
    /// The farther away horizontally, the less vertical reach is left.
    #[test]
    fn vertical_reach_shrinks_with_horizontal_distance() {
        let near = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((0.0, 0)));
        let mid = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((64.0, 0)));
        let far = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((120.0, 0)));
        assert!(
            near <= mid,
            "near {near} should reach at least as deep as mid {mid}"
        );
        assert!(
            mid <= far,
            "mid {mid} should reach at least as deep as far {far}"
        );
        // 距离 120 时垂直只够 sqrt(128²-120²) ≈ 44 格，从 32 往下只能到 -16 以下一点。
        // At distance 120 the vertical reach is only sqrt(128^2-120^2) = 44, so from 32 the
        // floor cannot go far below -16.
        assert!(far > MIN_Y);
    }

    /// 玩家的 y 越高，能下探到深层的机会越小（因为垂直差距变大）。
    /// The higher the player, the harder it is to reach the deep segments.
    #[test]
    fn a_higher_player_reaches_less() {
        let low = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((0.0, 0)));
        let high = CutPlan::SurfaceBand.required_floor(32, MIN_Y, Some((0.0, 200)));
        assert!(
            high >= low,
            "high {high} should reach no deeper than low {low}"
        );
        // 玩家在 y=200，地表带下边界 32，垂直差 168 > 128，够不到。
        // A player at y=200 with the surface edge at 32 is 168 blocks up: out of reach.
        assert_eq!(high, 32);
    }

    /// 列中心按 +8 偏移算水平距离。
    /// Column centres are offset by 8 for the horizontal distance.
    #[test]
    fn horizontal_distance_uses_column_centres() {
        // 玩家所在列（dx=dz=0）的中心相距 (8, 8)，即 8√2。
        // The player's own column (dx=dz=0) has its centre at (8, 8), i.e. 8*sqrt(2).
        assert!((horizontal_distance(0, 0) - (128.0_f64).sqrt()).abs() < 1e-9);
        // 东边一个区块：中心在 (24, 8)。
        // One chunk east: the centre is at (24, 8).
        assert!((horizontal_distance(1, 0) - (24.0_f64 * 24.0 + 64.0).sqrt()).abs() < 1e-9);
    }

    /// 三种玩家情形的 y 取值。
    /// The player y of each situation.
    #[test]
    fn situations_pick_the_expected_player_y() {
        assert_eq!(PlayerSituation::Flying.player_y(70), 200);
        assert_eq!(PlayerSituation::Walking.player_y(70), 70);
        assert_eq!(PlayerSituation::Standing.player_y(70), 70);
    }
}
