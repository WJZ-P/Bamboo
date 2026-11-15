//! 区块生成到 `terrain` 步里 `doFill` 跑完为止的那一部分。
//!
//! 官方 26.3 把噪声、地表规则和雕刻器合在同一个 `terrain` 状态里；这里把噪声那一段单独
//! 拿出来，叫 `NOISE` 阶段，方便对拍和以后接进区块流程。两个入口都是纯函数：
//! 种子 + 设置 + 区块坐标进，段数组和群系出，不碰世界状态。
//!
//! 一个区块有两步：
//!
//! * [`Generator::biomes`]：`ChunkStatus.BIOMES`，六个气候函数在 quart 网格上成批求值，
//!   再查群系参数表填进 4×4×4 的格子里；
//! * [`Generator::noise`]：`doFill`，最终密度在整个 16×高度×16 的体积上成批求值，
//!   含水层自顶向下把每个方块换成石头、水、岩浆或空气。
//!
//! 两步各自用一个 [`Context`]，和官方 `ChunkGenerator.doCreateBiomes` / `NoiseChunk` 一样：
//! `cache` 的记忆不跨步，但 `biomes` 那一步的成批结果会留在 R 树的 `lastResult` 里，
//! 所以查询顺序和平局时选哪条都跟着官方。

use std::collections::HashMap;

use mc_bamboo_data::{block_by_name, blocks};
use serde_json::Value;

use crate::{
    Worldgen,
    aquifer::Aquifer,
    beardifier::Beardifier,
    biome::MultiNoiseBiomeSource,
    biome_manager::BiomeManager,
    biome_region::{BiomeGetter, ChunkBiomes, RegionBiomes},
    carver::Carvers,
    climate::TargetPoint,
    density::{self, FunctionRegistry, NoiseRegistry, Ref},
    material::MaterialRules,
    math::{java_floor_div, quart_to_block},
    sampler::{Compiler, Context, Sampler},
    settings::Settings,
    volume::Volume,
};

/// `terrain` 状态里依次做的三件事。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// `doFill` 跑完那一刻。
    Noise,
    /// `MaterialSystem.buildSurface` 跑完那一刻。
    Surface,
    /// `generateCarvers` 跑完那一刻。
    Carvers,
}

/// 一个区块 16×16 个柱子。
pub const CHUNK_WIDTH: usize = 16;
/// 群系容器里一个 section 每边 4 个 quart 格。
pub const BIOME_CELLS: usize = 4;

/// `minecraft:overworld` 的设置和群系预设。
pub const OVERWORLD_SETTINGS: &str = "minecraft:overworld";
pub const OVERWORLD_PRESET: &str = "minecraft:overworld";

/// 一个维度的生成参数，按种子建一次，可以反复生成区块。
///
/// 对应官方一个 `NoiseGeneratorSettings` 加一个 `RandomState`：八个路由采样器、含水层的
/// 六条密度函数和所有噪声实例共用一个 [`Compiler`]，所以 `cache` 编号也共用。
#[derive(Debug)]
pub struct Generator<'a> {
    worldgen: &'a Worldgen,
    settings: &'a Settings,
    biomes: &'a MultiNoiseBiomeSource,
    router: crate::settings::Samplers,
    aquifer: Option<AquiferFunctions>,
    /// `RandomState.getOrCreateRandomFactory("minecraft:aquifer")`。
    aquifer_random: crate::random::PositionalFactory,
    /// `RandomState.getOrCreateRandomFactory("minecraft:worldgen_region_random")`。
    /// `WorldGenRegion.random` 用它按中心区块原点建一个随机源，`MossyCarpetBlock.placeAt`
    /// 之类的 `level.getRandom()` 调用读的是它，不是地物的 `random` 参数。
    ///
    /// `RandomState.getOrCreateRandomFactory("minecraft:worldgen_region_random")`; the
    /// source behind `WorldGenRegion.random`. `level.getRandom()` calls such as
    /// `MossyCarpetBlock.placeAt` read it instead of the feature's own `random`.
    region_random: crate::random::PositionalFactory,
    cache_count: usize,
    material: MaterialRules,
    carvers: Carvers,
    /// `RandomState.random`，`MaterialSystem` 用同一个 positional factory 造噪声。
    positional: crate::random::PositionalFactory,
    seed: i64,
    default_block: u32,
    default_fluid: u32,
    empty: u32,
    /// 地物注册表，按 `Worldgen` 建一次。
    features: crate::feature::registry::Registry,
}

/// 含水层用的六条密度函数，编译顺序和 `NoiseBasedAquifer` 的构造器一样。
#[derive(Debug)]
pub struct AquiferFunctions {
    pub barrier: Sampler,
    pub fluid_level_floodedness: Sampler,
    pub fluid_level_spread: Sampler,
    pub lava: Sampler,
    pub exclusion: Sampler,
    pub surface_level: Sampler,
}

impl<'a> Generator<'a> {
    /// 从官方数据和种子建一个主世界生成器。
    ///
    /// 官方的 `RandomState` 只按维度建一次；这里也一样，`generate` 不会重新编译采样器。
    pub fn new(worldgen: &'a Worldgen, seed: i64) -> Result<Self, String> {
        Self::with_settings(worldgen, OVERWORLD_SETTINGS, OVERWORLD_PRESET, seed)
    }

    /// 按 ID 取一套设置。这一步只支持有 `aquifers` 的维度（主世界和放大化），
    /// 下界和末地还没做。
    pub fn with_settings(
        worldgen: &'a Worldgen,
        settings_id: &str,
        preset: &str,
        seed: i64,
    ) -> Result<Self, String> {
        let settings = worldgen
            .settings(settings_id)
            .ok_or_else(|| format!("unknown noise settings {settings_id}"))?;
        let biomes = worldgen
            .preset(preset)
            .ok_or_else(|| format!("unknown biome preset {preset}"))?;
        let raw = parse_settings(worldgen, settings_id)?;
        let mut compiler = Compiler::new(seed, settings.legacy_random_source);

        // 编译顺序要和官方一样，编译顺序决定 cache 的编号，不过编号只要自洽就不影响结果。
        let router = crate::settings::Samplers {
            temperature: compiler.compile(&settings.router.temperature),
            vegetation: compiler.compile(&settings.router.vegetation),
            continents: compiler.compile(&settings.router.continents),
            erosion: compiler.compile(&settings.router.erosion),
            depth: compiler.compile(&settings.router.depth),
            ridges: compiler.compile(&settings.router.ridges),
            chunk_surface_level: compiler.compile(&settings.router.chunk_surface_level),
            final_density: compiler.compile(&settings.router.final_density),
        };

        let noises = worldgen.noises();
        let functions = worldgen.functions();
        let aquifer = raw
            .get("aquifers")
            .map(|value| parse_aquifer(value, noises, functions, &mut compiler))
            .transpose()?;
        let aquifer_random = compiler.random_factory("minecraft:aquifer");
        let region_random = compiler.random_factory("minecraft:worldgen_region_random");
        let raw_functions = worldgen
            .raw()
            .get("density_function")
            .and_then(Value::as_object)
            .ok_or("missing density_function")?;
        let material_rule = raw
            .get("material_rule")
            .and_then(Value::as_str)
            .ok_or("noise settings has no material_rule")?
            .to_owned();
        let material = MaterialRules::compile(
            worldgen.raw(),
            raw_functions,
            worldgen.noises(),
            worldgen.functions(),
            &mut compiler,
            &material_rule,
        )
        .map_err(|error| format!("material rules: {error}"))?;
        let carvers =
            Carvers::compile(worldgen.raw()).map_err(|error| format!("carvers: {error}"))?;
        let features = crate::feature::registry::Registry::parse(worldgen.raw())
            .map_err(|error| format!("features: {error}"))?;
        let cache_count = compiler.cache_count();
        let positional = compiler.positional_factory();
        let mut material = material;
        material.prepare(worldgen.noises(), positional, settings)?;
        let default_block = block_state(raw.get("default_block"))?;
        let default_fluid = block_state(raw.get("default_fluid"))?;
        Ok(Self {
            worldgen,
            settings,
            biomes,
            router,
            aquifer,
            aquifer_random,
            region_random,
            cache_count,
            material,
            carvers,
            positional,
            seed,
            default_block,
            default_fluid,
            empty: blocks::AIR,
            features,
        })
    }

    pub fn settings(&self) -> &Settings {
        self.settings
    }

    /// 供诊断和对拍用的群系来源：等价于 `BiomeManager::new(preset, seed)`。
    ///
    /// A biome source for diagnostics and oracles; equals `BiomeManager::new(preset, seed)`.
    pub fn biome_source(&self) -> &MultiNoiseBiomeSource {
        self.biomes
    }

    /// 路由采样器（六条气候函数在这里）。
    /// The router samplers (the six climate functions live here).
    pub fn samplers(&self) -> &crate::settings::Samplers {
        &self.router
    }

    /// 世界种子。
    /// The world seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// 建一个区块的 quart 群系格（`ChunkStatus.BIOMES` 的产物），用来拼 3×3 region。
    ///
    /// Build one chunk's quart biome cells (the `ChunkStatus.BIOMES` output), for
    /// assembling the 3×3 region.
    pub fn chunk_biomes(&self, chunk_x: i32, chunk_z: i32) -> Result<ChunkBiomes, String> {
        let sections = self.biomes(chunk_x, chunk_z)?;
        Ok(ChunkBiomes::new(
            chunk_x,
            chunk_z,
            self.settings.min_y,
            sections,
        ))
    }

    /// 造中心加 8 个邻居的 3×3 群系视图，和官方 `buildTerrain` 的 `WorldGenRegion` 一样。
    ///
    /// 邻居的 `biomes()` 一定先于中心算：官方 `buildTerrain` 的 `collectPossibleBiomes`
    /// 和 `region.getBiomeManager()` 都要读它们的 quart 格。查询顺序只影响 R 树的
    /// `lastResult`，而这里每个区块的气候值都是成批 `sampleVolume` 出来的，和官方一致。
    ///
    /// Build the centre plus eight neighbours' 3×3 biome view, like vanilla
    /// `buildTerrain`'s `WorldGenRegion`. Neighbours are filled first because
    /// `collectPossibleBiomes` and `region.getBiomeManager()` read their quart cells.
    /// Query order only affects the R-tree `lastResult`, and every chunk here samples
    /// the climate in one batch, matching vanilla.
    pub fn biome_region(&self, chunk_x: i32, chunk_z: i32) -> Result<RegionBiomes, String> {
        let center = self.chunk_biomes(chunk_x, chunk_z)?;
        let mut neighbors = Vec::with_capacity(8);
        // 顺序照 `ChunkgenOracle.buildRealTerrain`：`dx` 外层、`dz` 内层、跳过中心。
        // 顺序决定 `ParameterList` 的 `lastResult` 链，进而决定平局时选哪条。
        //
        // Same order as `ChunkgenOracle.buildRealTerrain`: `dx` outer, `dz` inner,
        // skipping the centre. The order drives the `ParameterList` `lastResult` chain
        // and therefore which entry wins a tie.
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                neighbors.push(self.chunk_biomes(chunk_x + dx, chunk_z + dz)?);
            }
        }
        Ok(RegionBiomes::new(
            crate::biome_manager::obfuscate_seed(self.seed),
            center,
            neighbors,
        ))
    }

    /// `ChunkStatus.BIOMES`：`ChunkGenerator.doCreateBiomes` 加 `fillBiomesFromNoise`。
    ///
    /// 返回每个 section 的 64 个 quart 格（下标 `y*16 + z*4 + x`）的群系注册表 ID。
    pub fn biomes(&self, chunk_x: i32, chunk_z: i32) -> Result<Vec<Vec<u32>>, String> {
        crate::perf::mark(crate::perf::Phase::Biomes);
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let sections = (height / 16) as usize;
        let quart_y = java_floor_div(min_y, 4);
        let quart_height = height / 4;
        // 官方 `createResolverForChunk` 用 (4, height/4, 4)、步长都是 4 的一块体积。
        let volume = Volume::new(
            (4, quart_height, 4),
            (
                quart_to_block(java_floor_div(chunk_x * 16, 4)),
                quart_to_block(quart_y),
                quart_to_block(java_floor_div(chunk_z * 16, 4)),
            ),
            (4, 4, 4),
        );
        let mut context = Context::new(self.cache_count);
        let mut columns: Vec<Vec<f32>> = Vec::with_capacity(6);
        for sampler in [
            &self.router.temperature,
            &self.router.vegetation,
            &self.router.continents,
            &self.router.erosion,
            &self.router.depth,
            &self.router.ridges,
        ] {
            let mut buffer = context.acquire(volume.len());
            sampler.sample_volume(&mut context, &mut buffer, &volume);
            columns.push(buffer);
        }

        let mut result = Vec::with_capacity(sections);
        for section_y in 0..sections {
            // section 从下到上；x 外、y 中、z 内，和 `LevelChunkSection.fillBiomesFromNoise` 一样。
            let mut cells = Vec::with_capacity(64);
            for x in 0..4 {
                for y in 0..4 {
                    for z in 0..4 {
                        let quart_y = quart_y + section_y as i32 * 4 + y;
                        let index = volume.index(x, quart_y - java_floor_div(min_y, 4), z);
                        let target = TargetPoint::of([
                            columns[0][index],
                            columns[1][index],
                            columns[2][index],
                            columns[3][index],
                            columns[4][index],
                            columns[5][index],
                        ]);
                        cells.push(biome_id(self.biomes.biome(target))?);
                    }
                }
            }
            result.push(cells);
        }
        for column in columns {
            context.release(column);
        }
        crate::perf::pause();
        Ok(result)
    }

    /// `NoiseBasedChunkGenerator.doFill`：生成到 NOISE 阶段的方块。
    pub fn noise(&self, chunk_x: i32, chunk_z: i32) -> Result<Vec<Vec<u32>>, String> {
        self.terrain(chunk_x, chunk_z, Stage::Noise)
    }

    /// 生成到 `terrain` 状态里的某一个阶段。
    ///
    /// 返回 `height` 个方块层，每层的 256 个默认状态 ID，下标 `x*16 + z`。
    /// 三个步骤共用一个 [`Context`] 和一个 [`Aquifer`]，和官方一个 `NoiseChunk`
    /// 一样；只跑到 `stage` 指定的那一刻就返回。
    pub fn terrain(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        stage: Stage,
    ) -> Result<Vec<Vec<u32>>, String> {
        self.terrain_with_beardifier(chunk_x, chunk_z, stage, None)
    }

    /// `terrain` 加上一个本区块的 `Beardifier`。`structure_starts` / `structure_references`
    /// 接进游戏后由调用方把真实结构喂进来；`None` 等价于官方 `Beardifier.EMPTY`，和
    /// [`Generator::terrain`] 逐位相同。`first_occupied_height` 等基线路径永远走 `None`。
    ///
    /// `terrain` with a per-chunk `Beardifier`. Once the structure stages are wired into the
    /// game the caller feeds real structures here; `None` equals vanilla `Beardifier.EMPTY` and
    /// is bit-identical to [`Generator::terrain`]. Baseline paths such as
    /// `first_occupied_height` always stay on `None`.
    pub fn terrain_with_beardifier(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        stage: Stage,
        beardifier: Option<&Beardifier>,
    ) -> Result<Vec<Vec<u32>>, String> {
        self.terrain_inner(chunk_x, chunk_z, stage, beardifier, None)
    }

    /// 建一个区块用的含水层。有 `aquifers` 配置时是噪声版，没有时（下界）是
    /// `Aquifer.createDisabled` 的简化版。
    ///
    /// Build the aquifer for one volume: the noise-based one when `aquifers` is
    /// configured, and the `Aquifer.createDisabled` simplification otherwise (the
    /// nether).
    fn make_aquifer(
        &self,
        context: &mut Context,
        volume: &Volume,
        lava: u32,
    ) -> Result<Aquifer<'_>, String> {
        Ok(match self.aquifer.as_ref() {
            Some(functions) => Aquifer::new(
                functions,
                context,
                self.aquifer_random,
                volume,
                self.settings.sea_level,
                self.default_fluid,
                lava,
                self.empty,
            ),
            // 没有 `aquifers` 的维度（下界）走 `Aquifer.createDisabled`。
            // A dimension without `aquifers` (the nether) takes `Aquifer.createDisabled`.
            None => Aquifer::new_disabled(
                self.aquifer_random,
                self.settings.sea_level,
                self.default_fluid,
                lava,
                self.empty,
            ),
        })
    }

    /// `terrain` 再带上周围 3×3 区块已填好的 quart 群系格。
    ///
    /// 官方 `buildTerrain` 的地表阶段用 `region.getBiomeManager()`，它的 `BiomeResolver`
    /// 就是 `WorldGenRegion`：先读 `ChunkStatus.BIOMES` 成批填好的格子，读不到才回退
    /// 单点重算。成批 `sampleVolume` 和单点 `sampleValue` 的浮点顺序不同，气候参数落在
    /// R 树最近邻的临界点上时会选到不同群系，所以地表材质必须照真实路径读格子。
    ///
    /// `terrain` plus the quart biome cells already stored in the surrounding 3×3 chunks.
    /// Vanilla's terrain surface pass uses `region.getBiomeManager()`, whose
    /// `BiomeResolver` is the `WorldGenRegion` itself: it reads the cells the
    /// `ChunkStatus.BIOMES` batch fill wrote and only falls back to single-point
    /// sampling when none are there. Batch `sampleVolume` and single-point
    /// `sampleValue` differ in floating-point order, so climate values on an R-tree
    /// nearest-neighbour tie pick different biomes; surface material must therefore
    /// read the cells like the real path.
    pub fn terrain_with_region(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        stage: Stage,
        beardifier: Option<&Beardifier>,
        region: &RegionBiomes,
    ) -> Result<Vec<Vec<u32>>, String> {
        self.terrain_inner(chunk_x, chunk_z, stage, beardifier, Some(region))
    }

    fn terrain_inner(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        stage: Stage,
        beardifier: Option<&Beardifier>,
        region: Option<&RegionBiomes>,
    ) -> Result<Vec<Vec<u32>>, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        crate::perf::mark(crate::perf::Phase::TerrainSetup);
        // 测量时可能只算切割线以上的密度（见 `perf::Floors`），默认就是整个高度。
        // Measurement may only fill the density above a cut line (see `perf::Floors`);
        // by default the whole column is generated.
        let floor = crate::perf::floors()
            .noise
            .unwrap_or(min_y)
            .clamp(min_y, min_y + height - 1);
        let density_min_y = if floor > min_y { floor } else { min_y };
        let density_height = min_y + height - density_min_y;
        let volume = Volume::blocks(
            (16, density_height, 16),
            (chunk_x * 16, density_min_y, chunk_z * 16),
        );
        let mut context = Context::new(self.cache_count);
        if let Some(beardifier) = beardifier {
            context.set_beardifier(std::sync::Arc::new(beardifier.clone()));
        }
        let mut density = context.acquire(volume.len());
        let lava = lava_state()?;
        crate::perf::note_density_floor(density_min_y);
        crate::perf::mark(crate::perf::Phase::AquiferInit);
        // 官方先建 NoiseChunk（含水层的构造器也在这里求值），再算 finalDensity。
        // Rust 借检查不让两件事同时抱着 context，所以先建含水层，再算密度。
        // Build the aquifer before the density, because Rust's borrow checker will not
        // let both hold the context at once.
        let mut aquifer = self.make_aquifer(&mut context, &volume, lava)?;
        crate::perf::note_skip_above(aquifer.skip_sampling_above_y());
        crate::perf::mark(crate::perf::Phase::NoiseDensity);
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);

        crate::perf::mark(crate::perf::Phase::Substance);
        let mut layers = vec![vec![self.empty; 256]; height as usize];
        // 切割线以下不生成：留成空气。下游阶段（地表规则、雕刻器、地物、光照）都按"这里
        // 什么都没有"处理，和"深层还没生成"的语义一致；这也是测量对照实验想要的成本。
        // Below the cut line nothing is generated: the layers stay air, so later stages
        // (surface rules, carvers, features, light) treat that volume as empty, matching
        // "the deep part has not been generated yet". That is the cost the measurement
        // experiment is after.
        for z in 0..16 {
            let block_z = chunk_z * 16 + z;
            for x in 0..16 {
                let block_x = chunk_x * 16 + x;
                for local_y in (0..density_height).rev() {
                    let block_y = density_min_y + local_y;
                    let value = f64::from(density[volume.index(x, local_y, z)]);
                    let state = aquifer
                        .compute_substance(&mut context, block_x, block_y, block_z, value)
                        .unwrap_or(self.default_block);
                    if state != self.empty {
                        layers[(block_y - min_y) as usize][x as usize * 16 + z as usize] = state;
                    }
                }
            }
        }
        context.release(density);
        let biome_manager = BiomeManager::new(self.biomes, self.seed);
        let biome_getter = match region {
            Some(region) => BiomeGetter::Region {
                manager: &biome_manager,
                region,
            },
            None => BiomeGetter::Uncached(&biome_manager),
        };
        let possible_biomes = region.map(RegionBiomes::possible_biomes);
        if stage >= Stage::Surface {
            crate::perf::mark(crate::perf::Phase::Surface);
            self.material.build_surface(
                &mut layers,
                chunk_x,
                chunk_z,
                self.settings,
                &mut context,
                &self.router,
                &biome_getter,
                possible_biomes.as_deref(),
                self.worldgen_noises(),
                self.positional,
            )?;
        }
        if stage >= Stage::Carvers {
            crate::perf::mark(crate::perf::Phase::CarverMask);
            self.carvers.generate(
                &mut layers,
                chunk_x,
                chunk_z,
                self.settings,
                &mut context,
                &self.router,
                &biome_manager,
                &mut aquifer,
                &self.material,
                self.worldgen.noises(),
                self.positional,
                self.seed,
            )?;
        }
        crate::perf::pause();
        Ok(layers)
    }

    /// 从某个 y 到世界顶部成批求值 `finalDensity`，返回耗时（毫秒）。
    ///
    /// 测量用：把 y 逐个高度带边界抬高，相邻两次整列求值的差就是这一带的边际耗时，
    /// 和整块一次求值的口径一致（每一带单采样会重复算插值角点，不能用）。
    ///
    /// Sample `finalDensity` from `from_y` to the top of the world and return the elapsed
    /// milliseconds. Measurement only: raising `from_y` band by band and taking the
    /// difference of consecutive whole-column passes gives a per-band cost on the same
    /// footing as one whole-volume pass (sampling each band alone would recompute the
    /// interpolation corners). The aquifer is not involved.
    #[cfg(feature = "perf")]
    pub fn sample_density_from(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        from_y: i32,
    ) -> Result<f64, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let from_y = from_y.clamp(min_y, min_y + height - 1);
        let volume = Volume::blocks(
            (16, min_y + height - from_y, 16),
            (chunk_x * 16, from_y, chunk_z * 16),
        );
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        let start = std::time::Instant::now();
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        context.release(density);
        Ok(elapsed)
    }

    /// 每个 8 格高度带是不是「整格空气」：一次整列求值，逐带看有没有 `density > 0`。
    ///
    /// 含水层在 `skipSamplingAboveY` 之上直接返回全局流体（海平面以上是空气），所以那里
    /// `density <= 0` 就等于空气；更低的地方这个判定也会把含水的空腔算成「非空气」，
    /// 用它的地方要注意。
    ///
    /// Whether each 8-block band is entirely air: one whole-column pass, then check each
    /// band for any point with `density > 0`. Above the aquifer's `skipSamplingAboveY`
    /// `density <= 0` is exactly air (the aquifer returns the global fluid, air above sea
    /// level); lower down this also counts fluid-filled pockets as non-air.
    #[cfg(feature = "perf")]
    pub fn density_air_bands(&self, chunk_x: i32, chunk_z: i32) -> Result<Vec<bool>, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let volume = Volume::blocks((16, height, 16), (chunk_x * 16, min_y, chunk_z * 16));
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);
        let mut bands = vec![true; (height / 8) as usize];
        for local_y in 0..height {
            let band = (local_y / 8) as usize;
            if !bands[band] {
                continue;
            }
            for z in 0..16 {
                for x in 0..16 {
                    if density[volume.index(x, local_y, z)] > 0.0 {
                        bands[band] = false;
                        break;
                    }
                }
                if !bands[band] {
                    break;
                }
            }
        }
        context.release(density);
        Ok(bands)
    }

    /// 按 8 格高度带给出「这一格整格是实心」的掩码，供测量「跳过整格空气」的对照。
    ///
    /// 返回 `(bands, solid)`：`bands` 是每个高度带里 256 列的 `density > 0` 掩码。
    ///
    /// Per-band solid mask at 8-block granularity, for the "skip all-air cells"
    /// experiment. Returns each band's 256-column `density > 0` mask.
    #[cfg(feature = "perf")]
    pub fn density_solid_mask(
        &self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Result<Vec<[bool; 256]>, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let volume = Volume::blocks((16, height, 16), (chunk_x * 16, min_y, chunk_z * 16));
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);
        let mut bands = vec![[false; 256]; (height / 8) as usize];
        for local_y in 0..height {
            let band = &mut bands[(local_y / 8) as usize];
            for z in 0..16 {
                for x in 0..16 {
                    if density[volume.index(x, local_y, z)] > 0.0 {
                        band[x as usize * 16 + z as usize] = true;
                    }
                }
            }
        }
        context.release(density);
        Ok(bands)
    }

    /// `NoiseBasedChunkGenerator.getFirstOccupiedHeight(x, z, WORLD_SURFACE_WG, ...)`。
    ///
    /// 照 `iterateNoiseColumn` 用一块 1×height×1、步长 1 的体积和全新的含水层，自顶向下
    /// 找第一个非空气方块。`getBaseHeight` 返回 blockY+1，`getFirstOccupiedHeight` 再减 1，
    /// 所以这里直接返回该 blockY；整列都是空气时返回 minY-1。
    ///
    /// `NoiseBasedChunkGenerator.getFirstOccupiedHeight(x, z, WORLD_SURFACE_WG, ...)`.
    /// Mirrors `iterateNoiseColumn`: a fresh 1xheightx1 step-1 volume and a fresh aquifer,
    /// scanning top-down for the first non-air block. `getBaseHeight` returns blockY+1 and
    /// `getFirstOccupiedHeight` subtracts 1, so we return that blockY directly; an all-air
    /// column yields minY-1.
    ///
    /// 官方 `iterateNoiseColumn` 用 `Beardifier = null`（EMPTY）：`getBaseHeight` 永远不受
    /// 结构地基影响，所以这里固定走 EMPTY 分支，绝不调用 `terrain_with_beardifier`。接入真实
    /// Beardifier 时这一支必须保持 EMPTY，否则结构生成期的高度判定会算进结构地基，和原版不一致。
    ///
    /// Vanilla `iterateNoiseColumn` passes a `null` (EMPTY) beardifier: `getBaseHeight` never
    /// accounts for structure terrain, so this stays on the EMPTY branch and must never call
    /// `terrain_with_beardifier`. Keep it EMPTY when a real Beardifier is wired in, or structure
    /// height checks would wrongly include structure terrain and diverge from vanilla.
    pub fn first_occupied_height(&self, block_x: i32, block_z: i32) -> Result<i32, String> {
        self.first_occupied_height_impl(block_x, block_z, false)
    }

    /// `getFirstOccupiedHeight(x, z, OCEAN_FLOOR_WG, ...)`：自顶向下找第一个 `BLOCKS_MOTION_IN_HEIGHTMAP`
    /// 方块（WORLD_SURFACE_WG 用 `NOT_AIR`；OCEAN_FLOOR_WG 用 `MATERIAL_MOTION_BLOCKING`）。
    ///
    /// `getFirstOccupiedHeight(x, z, OCEAN_FLOOR_WG, ...)`: top-down first motion-blocking block
    /// (WORLD_SURFACE_WG uses `NOT_AIR`; OCEAN_FLOOR_WG uses `MATERIAL_MOTION_BLOCKING`).
    pub fn first_occupied_height_ocean_floor(
        &self,
        block_x: i32,
        block_z: i32,
    ) -> Result<i32, String> {
        self.first_occupied_height_impl(block_x, block_z, true)
    }

    fn first_occupied_height_impl(
        &self,
        block_x: i32,
        block_z: i32,
        motion_blocking: bool,
    ) -> Result<i32, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let volume = Volume::blocks((1, height, 1), (block_x, min_y, block_z));
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        let mut aquifer = self.make_aquifer(&mut context, &volume, lava_state()?)?;
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);
        for local_y in (0..height).rev() {
            let block_y = min_y + local_y;
            let value = f64::from(density[volume.index(0, local_y, 0)]);
            let state = aquifer
                .compute_substance(&mut context, block_x, block_y, block_z, value)
                .unwrap_or(self.default_block);
            let opaque = if motion_blocking {
                mc_bamboo_blocks::BlockState(state)
                    .is_tagged(mc_bamboo_data::tags::block_tags::BLOCKS_MOTION_IN_HEIGHTMAP)
            } else {
                state != self.empty
            };
            if opaque {
                context.release(density);
                return Ok(block_y);
            }
        }
        context.release(density);
        Ok(min_y - 1)
    }

    /// `NoiseBasedChunkGenerator.getBaseColumn(x, z, ...)`：一列的方块状态，下标
    /// `blockY - minY`。
    ///
    /// 照 `iterateNoiseColumn` 的 `columnReference` 分支：同一个 1×height×1、步长 1 的体积、
    /// 全新含水层、`Beardifier = null`，把每一层的 `computeSubstance` 结果按 Y 升序收全。
    /// `NoiseColumn.getBlock(y)` 越界返回空气，所以调用方直接按 `y - minY` 取值即可。
    ///
    /// `NoiseBasedChunkGenerator.getBaseColumn(x, z, ...)`: one column of block states indexed
    /// by `blockY - minY`. This is the `columnReference` branch of `iterateNoiseColumn`: the
    /// same 1xheightx1 step-1 volume, a fresh aquifer and a null (EMPTY) beardifier, collecting
    /// every `computeSubstance` result in ascending Y. `NoiseColumn.getBlock(y)` yields air out
    /// of range, so callers index with `y - minY` directly.
    pub fn base_column(&self, block_x: i32, block_z: i32) -> Result<Vec<u32>, String> {
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let volume = Volume::blocks((1, height, 1), (block_x, min_y, block_z));
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        let mut aquifer = self.make_aquifer(&mut context, &volume, lava_state()?)?;
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);
        let mut column = vec![self.empty; height as usize];
        for local_y in 0..height {
            let block_y = min_y + local_y;
            let value = f64::from(density[volume.index(0, local_y, 0)]);
            column[local_y as usize] = aquifer
                .compute_substance(&mut context, block_x, block_y, block_z, value)
                .unwrap_or(self.default_block);
        }
        context.release(density);
        Ok(column)
    }

    /// `MultiNoiseBiomeSource.createResolverForChunk` 的成批气候采样，返回一列 quart Y 的群系名。
    ///
    /// `couldStructureExistInColumn` 用 1×N×1、步长 4 的体积，对六个气候函数各跑一次
    /// `sampleVolume`，再对每格查群系参数表。和 `biomes()` 同一条成批路径。
    ///
    /// Batch climate sampling of `MultiNoiseBiomeSource.createResolverForChunk`: returns the
    /// biome name for each quart Y of one column. `couldStructureExistInColumn` uses a
    /// 1xNx1 step-4 volume, runs `sampleVolume` for each of the six climate functions, then
    /// looks up the parameter list per cell. Same batch path as `biomes()`.
    pub fn column_biome_names(
        &self,
        quart_x: i32,
        quart_z: i32,
        min_quart_y: i32,
        max_quart_y: i32,
    ) -> Result<Vec<String>, String> {
        let count = max_quart_y - min_quart_y + 1;
        let volume = Volume::new(
            (1, count, 1),
            (
                quart_to_block(quart_x),
                quart_to_block(min_quart_y),
                quart_to_block(quart_z),
            ),
            (4, 4, 4),
        );
        let mut context = Context::new(self.cache_count);
        let mut columns: Vec<Vec<f32>> = Vec::with_capacity(6);
        for sampler in [
            &self.router.temperature,
            &self.router.vegetation,
            &self.router.continents,
            &self.router.erosion,
            &self.router.depth,
            &self.router.ridges,
        ] {
            let mut buffer = context.acquire(volume.len());
            sampler.sample_volume(&mut context, &mut buffer, &volume);
            columns.push(buffer);
        }
        let mut names = Vec::with_capacity(count as usize);
        for quart_y in min_quart_y..=max_quart_y {
            let index = volume.index(0, quart_y - min_quart_y, 0);
            let target = TargetPoint::of([
                columns[0][index],
                columns[1][index],
                columns[2][index],
                columns[3][index],
                columns[4][index],
                columns[5][index],
            ]);
            names.push(self.biomes.biome(target).to_owned());
        }
        for column in columns {
            context.release(column);
        }
        Ok(names)
    }

    /// `ChunkStatus.FEATURES`：`ChunkGenerator.applyBiomeDecoration`。
    ///
    /// 输入是中心加 8 个邻居到 CARVERS 阶段的结果；输出 9 个区块改成 FEATURES
    /// 阶段之后的样子，以及给 8 个邻居的写增量。见
    /// [`crate::feature::run::FeatureContext`] 的两个过渡开关。
    pub fn features(
        &self,
        input: &crate::feature::FeatureChunks,
        steps_mask: i32,
        allowed_kinds: Option<&[String]>,
    ) -> Result<crate::feature::FeatureResult, String> {
        let biomes = BiomeManager::new(self.biomes, self.seed);
        // `WorldGenRegion.random`：`getOrCreateRandomFactory("worldgen_region_random")`
        // 在中心区块的**世界原点**（`center.getWorldPosition()`）上取一个随机源。
        // 整个 `features` 调用共用这一个。
        //
        // 注意它是**裸 `RandomSource`**，不是 `WorldgenRandom`：官方
        // `WorldGenRegion.random` 的静态类型就是 `RandomSource`，
        // `MossyCarpetBlock.placeAt` 调 `nextBoolean()` 走的是
        // `XoroshiroRandomSource.nextBoolean() = (nextLong() & 1) != 0`（long 的低位）。
        // 包成 `WorldgenRandom` 会走 `next(1)`（高位），序列完全不同。
        //
        // `WorldGenRegion.random`: the `worldgen_region_random` factory sampled at
        // `center.getPos().getWorldPosition()`, which is
        // `new BlockPos(minBlockX, 0, minBlockZ)` — **y is always 0**, not the dimension's
        // minimum Y. It is a **bare `RandomSource`**, not a `WorldgenRandom`:
        // `MossyCarpetBlock.placeAt`'s `nextBoolean()` is
        // `XoroshiroRandomSource.nextBoolean() = (nextLong() & 1) != 0` (the low bit).
        // Wrapping it in `WorldgenRandom` would use `next(1)` (the high bit).
        let region_random = self
            .region_random
            .at(input.center_x * 16, 0, input.center_z * 16);
        let context = crate::feature::run::FeatureContext {
            registry: &self.features,
            samplers: &self.router,
            biomes: &biomes,
            settings: self.settings,
            seed: self.seed,
            steps_mask,
            allowed_kinds,
            region_random: &region_random,
        };
        crate::feature::run::features(input, &context)
    }

    /// 地物注册表，对拍报告统计用。
    pub fn feature_registry(&self) -> &crate::feature::registry::Registry {
        &self.features
    }

    fn worldgen_noises(&self) -> &'a NoiseRegistry {
        self.worldgen.noises()
    }
}

/// `Blocks.LAVA.defaultBlockState()`。
fn lava_state() -> Result<u32, String> {
    Ok(block_by_name("minecraft:lava")
        .ok_or("missing minecraft:lava")?
        .default_state)
}

/// `NoiseGeneratorSettings` 的原始 JSON 里，含水层配置、默认方块和默认流体都还没进
/// [`Settings`]，这里再读一次。
fn parse_settings(
    worldgen: &Worldgen,
    settings_id: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN)
        .map_err(|error| format!("bad worldgen data: {error}"))?;
    let _ = worldgen;
    root.get("noise_settings")
        .and_then(|value| value.get(settings_id))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| format!("unknown noise settings {settings_id}"))
}

fn block_state(value: Option<&Value>) -> Result<u32, String> {
    let name = value
        .and_then(Value::as_str)
        .ok_or("missing block in noise settings")?;
    Ok(block_by_name(name)
        .ok_or_else(|| format!("unknown block {name}"))?
        .default_state)
}

fn biome_id(name: &str) -> Result<u32, String> {
    mc_bamboo_data::registries::biome::NAMES
        .iter()
        .position(|id| *id == name)
        .map(|id| id as u32)
        .ok_or_else(|| format!("unknown biome {name}"))
}

/// 按官方 `Aquifer.Config.CODEC` 的字段名解析，用同一个编译器编译六条函数。
fn parse_aquifer(
    value: &Value,
    noises: &NoiseRegistry,
    registry: &FunctionRegistry,
    compiler: &mut Compiler,
) -> Result<AquiferFunctions, String> {
    let object = value
        .as_object()
        .ok_or("aquifers must be an object")?
        .clone();
    let raw_functions: serde_json::Map<String, Value> = object.clone().into_iter().collect();
    let mut entries: HashMap<String, Ref> = registry
        .ids()
        .map(|id| (id.to_owned(), Ref::clone(registry.get(id).expect("刚问过"))))
        .collect();
    let mut resolving = std::collections::HashSet::new();
    let mut field = |name: &str| -> Result<Sampler, String> {
        let value = object
            .get(name)
            .ok_or_else(|| format!("missing aquifer field {name}"))?;
        let function = density::parse(value, &raw_functions, noises, &mut entries, &mut resolving)
            .map_err(|error| format!("aquifer field {name}: {error}"))?;
        Ok(compiler.compile(&function))
    };
    Ok(AquiferFunctions {
        barrier: field("barrier")?,
        fluid_level_floodedness: field("fluid_level_floodedness")?,
        fluid_level_spread: field("fluid_level_spread")?,
        lava: field("lava")?,
        exclusion: field("exclusion")?,
        surface_level: field("surface_level")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overworld_biomes_are_a_whole_chunk() {
        let worldgen = Worldgen::load().expect("能读数据");
        let generator = Generator::new(&worldgen, 1).expect("能建生成器");
        let sections = generator.biomes(0, 0).expect("能生成群系");
        assert_eq!(sections.len(), 24);
        for section in sections {
            assert_eq!(section.len(), 64);
            assert!(section.iter().all(|id| (*id as usize) < 67));
        }
    }

    /// 回归：种子 -1、区块 (52,-26) 在岩浆层上边界（y=-54）应给出水，而不是深板岩。
    /// 官方 `Aquifer.NoiseBasedAquifer.computeSubstance` 在最近格子给的是水、且正下方
    /// 全局流体是岩浆时，跳过压力计算直接返回水；缺失这段会误判成实心方块。
    /// Regression: seed -1, chunk (52,-26) at the lava-layer top (y=-54) must be water,
    /// not deepslate. Vanilla `computeSubstance` short-circuits to water when the nearest
    /// cell is water and the global fluid one block below is lava; without it a pressure
    /// check wrongly returns a solid block.
    #[test]
    fn aquifer_water_over_lava_boundary() {
        let worldgen = Worldgen::load().expect("能读数据");
        let generator = Generator::new(&worldgen, -1).expect("能建生成器");
        let layers = generator
            .terrain(52, -26, Stage::Noise)
            .expect("能生成地形");
        let water = block_by_name("minecraft:water")
            .expect("有水方块")
            .default_state;
        let min_y = worldgen
            .settings("minecraft:overworld")
            .expect("有主世界设置")
            .min_y;
        let local_y = (-54 - min_y) as usize;
        for z in 1..=3 {
            assert_eq!(layers[local_y][14 * 16 + z], water, "y=-54 x=14 z={z}");
        }
    }
}
