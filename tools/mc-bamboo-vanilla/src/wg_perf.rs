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
use mc_bamboo_data::{blocks, registries, state_facts};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, chunk::Chunk};
use mc_bamboo_worldgen::{
    Worldgen,
    chunk::{Generator, Stage},
    feature::{
        DECORATION_STEPS,
        world::prime_heightmaps,
    },
    perf::{self, Floors, Phase, Record, Y_BANDS},
};

const USAGE: &str = "usage: wg-perf [--seeds N] [--chunks N] [--biomes a,b,c] [--repeat N] \
                     [--threads N] [--json] [--skip-cut] [--skip-sky] [--skip-threads]";

/// 默认按群系采样：平原、森林、山地、海洋、丛林。
/// The default biome sample: plains, forest, hills, ocean, jungle.
const DEFAULT_BIOMES: [&str; 5] = [
    "minecraft:plains",
    "minecraft:forest",
    "minecraft:windswept_hills",
    "minecraft:ocean",
    "minecraft:jungle",
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
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            let mut value = || args.next().map(|value| *value).ok_or(USAGE.to_owned());
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
                "--json" => options.json = true,
                "--skip-cut" => options.skip_cut = true,
                "--skip-sky" => options.skip_sky = true,
                "--skip-threads" => options.skip_threads = true,
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

    // 首次调用的初始化开销单独报：建 `Generator`（编译采样器）+ 第一次生成。
    let init_start = Instant::now();
    let mut generator = Generator::new(&worldgen, seeds[0])?;
    let build_ms = init_start.elapsed().as_secs_f64() * 1000.0;
    let first_start = Instant::now();
    let _ = measure_once(&mut generator, targets[0], 1)?;
    println!(
        "wg-perf: building the generator {build_ms:.1} ms, first full chunk {:.1} ms \
         (initialisation, excluded from the medians below)",
        first_start.elapsed().as_secs_f64() * 1000.0
    );

    let mut measured = Vec::with_capacity(targets.len());
    let mut generators: HashMap<i64, Generator<'_>> = HashMap::new();
    for &target in &targets {
        let generator = match generators.entry(target.seed) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Generator::new(&worldgen, target.seed)?)
            }
        };
        let (item, _) = measure_once(generator, target, options.repeat)?;
        measured.push(item);
    }

    print_phases(&measured);
    print_biomes(&measured);
    let band_ms = measure_bands(&worldgen, &targets, options.repeat)?;
    print_bands(&measured, &band_ms);
    print_features(&measured, &worldgen);
    if !options.skip_cut {
        measure_cut_lines(&worldgen, &targets, &measured, options.repeat)?;
    }
    if !options.skip_sky {
        measure_sky(&worldgen, &targets, &measured, options.repeat)?;
    }
    if !options.skip_threads {
        measure_threads(&worldgen, &targets, options.threads, options.repeat)?;
    }
    if options.json {
        print_json(&measured, &band_ms);
    }
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
/// Measure one chunk `repeat` times and take the median of every field and of the
/// wall clock.
fn measure_once(
    generator: &Generator<'_>,
    target: Target,
    repeat: usize,
) -> Result<(Measured, f64), String> {
    let ocean_floor_min = ocean_floor_min(generator, target)?;
    let cut = cut_line(ocean_floor_min);
    let mut records = Vec::with_capacity(repeat);
    let mut times = Vec::with_capacity(repeat);
    for _ in 0..repeat {
        let start = Instant::now();
        let record = generate_recorded(generator, target, cut)?;
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

/// 一次带记录的完整生成：和游戏 `Staged` 流水线做的事一致，但不起线程。
///
/// One recorded full generation: the same steps the in-game `Staged` pipeline runs,
/// without threads.
fn generate_recorded(
    generator: &Generator<'_>,
    target: Target,
    cut: i32,
) -> Result<Record, String> {
    let (cx, cz) = (target.x, target.z);
    let min_y = generator.settings().min_y;
    perf::set_floors(Floors::default());
    perf::begin(min_y, cut, Phase::Biomes);
    let region = generator.biome_region(cx, cz)?;
    perf::mark(Phase::TerrainSetup);
    let blocks = generator.terrain_with_region(cx, cz, Stage::Carvers, None, &region)?;
    perf::mark(Phase::View);
    let view = view_of(generator, cx, cz)?;
    perf::mark(Phase::Features);
    let result = run_features(generator, cx, cz, &view)?;
    perf::mark(Phase::LightView);
    let chunks = light_view(cx, cz, &view)?;
    perf::mark(Phase::Light);
    let light = mc_bamboo_world::light::generate::generate_light(ChunkPos::new(cx, cz), &chunks);
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

fn light_view(cx: i32, cz: i32, view: &[TerrainView; 9]) -> Result<BTreeMap<ChunkPos, Chunk>, String> {
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

impl std::ops::Index<usize> for TerrainViewList {
    type Output = TerrainView;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
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
        let mut values: Vec<f64> = records.iter().map(|record| record.phases_ms[index]).collect();
        values.sort_by(f64::total_cmp);
        merged.phases_ms[index] = values[values.len() / 2];
    }
    for index in 0..merged.steps_ms.len() {
        let mut values: Vec<f64> = records.iter().map(|record| record.steps_ms[index]).collect();
        values.sort_by(f64::total_cmp);
        merged.steps_ms[index] = values[values.len() / 2];
    }
    merged
}

/// 按群系找区块：`Generator::biomes` 的段调色板里目标群系过半就算。
///
/// Find chunks by biome: keep a chunk when the target biome is a majority of its quart
/// cells in the section palettes from `Generator::biomes`.
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
            'scan: for radius in 0..800 {
                let d = radius * 6;
                for (x, z) in [
                    (d, d),
                    (-d, d),
                    (d, -d),
                    (-d, -d),
                    (d, 0),
                    (0, d),
                    (-d, 0),
                    (0, -d),
                ] {
                    let sections = generator.biomes(x, z)?;
                    let hits = sections.iter().flatten().filter(|&&id| id == wanted).count();
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
                            break 'scan;
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
    println!("| 群系 | 区块数 | 墙钟 | 群系 | 噪声填充 | 地表 | 雕刻器 | 地物 | 光照 | 组装 | 切割线 |");
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

/// 噪声填充按 8 格一层的耗时：y=-64..320 切成 48 段，各单独成批求值。
///
/// The noise fill per 8-block band: slice y=-64..320 into forty-eight spans and sample
/// each alone.
fn measure_bands(
    worldgen: &Worldgen,
    targets: &[Target],
    repeat: usize,
) -> Result<Vec<f64>, String> {
    let mut per_band: Vec<Vec<f64>> = vec![Vec::new(); Y_BANDS];
    for target in targets {
        let generator = Generator::new(worldgen, target.seed)?;
        // 预热一次，免得第一段的计时含初始化。
        generator.sample_density_span(target.x, target.z, MIN_Y, MIN_Y + 7)?;
        for (band, values) in per_band.iter_mut().enumerate() {
            let from_y = MIN_Y + band as i32 * 8;
            let mut times = Vec::with_capacity(repeat);
            for _ in 0..repeat {
                times.push(generator.sample_density_span(target.x, target.z, from_y, from_y + 7)?);
            }
            times.sort_by(f64::total_cmp);
            values.push(times[times.len() / 2]);
        }
    }
    Ok(per_band
        .into_iter()
        .map(|values| stats(&values).1)
        .collect())
}

fn print_bands(measured: &[Measured], band_ms: &[f64]) {
    println!("\n## 噪声填充按高度（每 8 格一层，中位数 ms/区块）\n");
    println!("| y 范围 | 平均耗时 | 占整块噪声填充 | 分类 |");
    println!("|---|---|---|---|");
    let total: f64 = band_ms.iter().sum();
    let mut full: Vec<f64> = measured
        .iter()
        .map(|item| item.record.phases_ms[Phase::NoiseDensity.index()])
        .collect();
    full.sort_by(f64::total_cmp);
    let reference = full[full.len() / 2];
    for (band, &ms) in band_ms.iter().enumerate() {
        let from_y = MIN_Y + band as i32 * 8;
        let to_y = from_y + 7;
        let share = if reference > 0.0 {
            ms / reference * 100.0
        } else {
            0.0
        };
        println!(
            "| {from_y}..{to_y} | {ms:.3} | {share:.1}% | {} |",
            classify_band(from_y, to_y, measured)
        );
    }
    println!("| **合计** | {total:.3} | — | — |");
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

fn print_features(measured: &[Measured], worldgen: &Worldgen) {
    println!("\n## 地物按 decoration step\n");
    println!("| step | 名称 | 中位数 ms | 候选位置数 |");
    println!("|---|---|---|---|");
    for step in 0..DECORATION_STEPS {
        let values = column_of(measured, |record| record.steps_ms[step]);
        let (_, median, _) = stats(&values);
        let candidates: u64 = measured
            .iter()
            .map(|item| item.record.steps_candidates[step])
            .sum();
        println!(
            "| {step} | {} | {median:.3} | {candidates} |",
            step_name(step)
        );
    }

    println!("\n## 地物按 placed feature\n");
    println!(
        "| step | placed feature | 候选数 | 高度范围 | 地表带占比 | 中位数 ms | 地表带 / 深层 |"
    );
    println!("|---|---|---|---|---|---|---|");
    let rows = aggregate_features(measured, worldgen);
    for row in &rows {
        println!(
            "| {} | {} | {} | {}..{} | {:.0}% | {:.3} | {} |",
            row.step,
            row.name,
            row.candidates,
            row.min_y,
            row.max_y,
            row.above_share * 100.0,
            row.ms,
            row.kind
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
        "\n地表带合计 {surface:.3} ms/区块，深层带合计 {deep:.3} ms/区块（按逐地物中位数 × 高度占比估）"
    );
}

struct FeatureRow {
    step: usize,
    name: String,
    candidates: u64,
    min_y: i32,
    max_y: i32,
    above_share: f64,
    ms: f64,
    kind: &'static str,
}

fn aggregate_features(measured: &[Measured], worldgen: &Worldgen) -> Vec<FeatureRow> {
    let _ = worldgen;
    let mut rows: BTreeMap<u32, FeatureRow> = BTreeMap::new();
    let mut times: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for item in measured {
        let cut = item.cut;
        for feature in &item.record.features {
            let row = rows.entry(feature.placed).or_insert_with(|| FeatureRow {
                step: feature.step,
                name: format!("placed#{}", feature.placed),
                candidates: 0,
                min_y: i32::MAX,
                max_y: i32::MIN,
                above_share: 0.0,
                ms: 0.0,
                kind: "?",
            });
            row.candidates += feature.candidates;
            if feature.candidates > 0 {
                row.min_y = row.min_y.min(feature.min_y);
                row.max_y = row.max_y.max(feature.max_y);
                let above: u64 = (0..Y_BANDS)
                    .filter(|&band| item.record.min_y + band as i32 * 8 >= cut)
                    .map(|band| feature.y_counts[band])
                    .sum();
                row.above_share += above as f64 / feature.candidates as f64;
            }
            times
                .entry(feature.placed)
                .or_default()
                .push(feature.place_ms + feature.modifier_ms);
        }
    }
    let count = measured.len() as f64;
    let mut out = Vec::new();
    for (placed, mut row) in rows {
        let mut values = times.remove(&placed).unwrap_or_default();
        values.sort_by(f64::total_cmp);
        row.ms = if values.is_empty() {
            0.0
        } else {
            values[values.len() / 2]
        };
        row.above_share /= count;
        if row.min_y == i32::MAX {
            row.min_y = 0;
            row.max_y = 0;
        }
        row.kind = classify_feature(&row);
        out.push(row);
    }
    out.sort_by(|a, b| (a.step, &a.name).cmp(&(b.step, &b.name)));
    out
}

/// 按高度分布给地物分类。
/// Classify a feature by its height distribution.
fn classify_feature(row: &FeatureRow) -> &'static str {
    if row.candidates == 0 {
        "无候选"
    } else if row.above_share > 0.95 {
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

/// 「只生成切割线以上」的对照：余量 16 / 24 / 32 / 48 各量一遍。
///
/// The "only above the cut line" comparison at margins 16 / 24 / 32 / 48.
fn measure_cut_lines(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 只生成切割线以上（占完整生成的比例）\n");
    println!("| 余量 | 平原 | 森林 | 山地 | 海洋 | 丛林 | 总体 |");
    println!("|---|---|---|---|---|---|---|");
    for margin in [16, 24, 32, 48] {
        let mut shares: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        let mut all = Vec::new();
        for (target, item) in targets.iter().zip(measured) {
            let generator = Generator::new(worldgen, target.seed)?;
            let cut = (item.ocean_floor_min - margin).div_euclid(16) * 16;
            let floors = Floors {
                noise: Some(cut),
                surface: Some(cut),
                carver: Some(cut),
                feature: Some(cut),
            };
            perf::set_floors(floors);
            // 先跑一遍预热，后面的计时不含缓存填充。
            black_box(generate_recorded(&generator, *target, cut)?);
            let mut times = Vec::with_capacity(repeat);
            for _ in 0..repeat {
                let start = Instant::now();
                let record = generate_recorded(&generator, *target, cut)?;
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                black_box(&record);
            }
            perf::set_floors(Floors::default());
            times.sort_by(f64::total_cmp);
            let partial = times[times.len() / 2];
            let full = item.total_ms.max(0.001);
            shares
                .entry(target.biome)
                .or_default()
                .push(partial / full * 100.0);
            all.push(partial / full * 100.0);
        }
        let cells: Vec<String> = DEFAULT_BIOMES
            .iter()
            .map(|biome| {
                let values = shares.get(biome).cloned().unwrap_or_default();
                format!("{:.1}%", stats(&values).1)
            })
            .collect();
        println!("| {margin} | {} | {:.1}% |", cells.join(" | "), stats(&all).1);
    }
    Ok(())
}

/// 天空层：数含水层不再采样的那些噪声 cell 里有多少整格空气，以及它们占噪声填充多少时间。
///
/// The sky layer: count how many of the noise cells above the aquifer's
/// `skipSamplingAboveY` are all air, and how much of the noise fill time they take.
fn measure_sky(
    worldgen: &Worldgen,
    targets: &[Target],
    measured: &[Measured],
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 天空层\n");
    let mut air = 0u64;
    let mut cells = 0u64;
    let mut above_ms = 0.0;
    let mut all_ms = 0.0;
    for (target, item) in targets.iter().zip(measured) {
        let generator = Generator::new(worldgen, target.seed)?;
        let region = generator.biome_region(target.x, target.z)?;
        let blocks =
            generator.terrain_with_region(target.x, target.z, Stage::Noise, None, &region)?;
        let skip = item.record.skip_above_y.unwrap_or(item.ocean_floor_min);
        for band in (0..Y_BANDS).map(|band| MIN_Y + band as i32 * 8) {
            let mut times = Vec::with_capacity(repeat);
            for _ in 0..repeat {
                times.push(generator.sample_density_span(target.x, target.z, band, band + 7)?);
            }
            times.sort_by(f64::total_cmp);
            let ms = times[times.len() / 2];
            all_ms += ms;
            if band <= skip {
                continue;
            }
            cells += 1;
            above_ms += ms;
            let all_air = ((band - MIN_Y)..(band - MIN_Y + 8).min(blocks.len() as i32))
                .all(|y| blocks[y as usize].iter().all(|&state| state == blocks::AIR));
            if all_air {
                air += 1;
            }
        }
    }
    println!(
        "高于 `skipSamplingAboveY` 的噪声 cell：{cells} 个，其中整格空气 {air} 个（{:.1}%）；\
         这些 cell 占噪声填充 {:.1}% 的时间",
        air as f64 / cells.max(1) as f64 * 100.0,
        above_ms / all_ms.max(0.001) * 100.0
    );
    Ok(())
}

/// 多线程吞吐量：每线程一个 `Generator` 和一份 `Worldgen`，一起生成同一批区块。
///
/// Multi-thread throughput: one `Generator` and one `Worldgen` per thread, all
/// generating the same batch.
fn measure_threads(
    worldgen: &Worldgen,
    targets: &[Target],
    threads: usize,
    repeat: usize,
) -> Result<(), String> {
    println!("\n## 多线程吞吐量\n");
    let generator = Generator::new(worldgen, targets[0].seed)?;
    let mut single = Vec::new();
    for _ in 0..repeat {
        let start = Instant::now();
        for target in targets {
            black_box(generate_recorded(&generator, *target, 0)?);
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
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(target) = batch.get(index).copied() else {
                            break;
                        };
                        if let Err(error) = generate_recorded(&generator, target, 0) {
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
