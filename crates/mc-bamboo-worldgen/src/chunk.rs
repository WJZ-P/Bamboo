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
    biome::MultiNoiseBiomeSource,
    biome_manager::BiomeManager,
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
        let raw_functions = worldgen
            .raw()
            .get("density_function")
            .and_then(Value::as_object)
            .ok_or("missing density_function")?;
        let material = MaterialRules::compile(
            worldgen.raw(),
            raw_functions,
            worldgen.noises(),
            worldgen.functions(),
            &mut compiler,
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

    /// `ChunkStatus.BIOMES`：`ChunkGenerator.doCreateBiomes` 加 `fillBiomesFromNoise`。
    ///
    /// 返回每个 section 的 64 个 quart 格（下标 `y*16 + z*4 + x`）的群系注册表 ID。
    pub fn biomes(&self, chunk_x: i32, chunk_z: i32) -> Result<Vec<Vec<u32>>, String> {
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
        let min_y = self.settings.min_y;
        let height = self.settings.height;
        let volume = Volume::blocks((16, height, 16), (chunk_x * 16, min_y, chunk_z * 16));
        let mut context = Context::new(self.cache_count);
        let mut density = context.acquire(volume.len());
        // 官方先建 NoiseChunk（含水层的构造器也在这里求值），再算 finalDensity。
        // Rust 借检查不让两件事同时抱着 context，所以先建含水层，再算密度。
        let aquifer = self.aquifer.as_ref().ok_or("dimension has no aquifers")?;
        let mut aquifer = Aquifer::new(
            aquifer,
            &mut context,
            self.aquifer_random,
            &volume,
            self.settings.sea_level,
            self.default_fluid,
            lava_state()?,
            self.empty,
        );
        self.router
            .final_density
            .sample_volume(&mut context, &mut density, &volume);

        let mut layers = vec![vec![self.empty; 256]; height as usize];
        for z in 0..16 {
            let block_z = chunk_z * 16 + z;
            for x in 0..16 {
                let block_x = chunk_x * 16 + x;
                for local_y in (0..height).rev() {
                    let block_y = min_y + local_y;
                    let value = f64::from(density[volume.index(x, local_y, z)]);
                    let state = aquifer
                        .compute_substance(&mut context, block_x, block_y, block_z, value)
                        .unwrap_or(self.default_block);
                    if state != self.empty {
                        layers[local_y as usize][x as usize * 16 + z as usize] = state;
                    }
                }
            }
        }
        context.release(density);
        if stage >= Stage::Surface {
            let biomes = BiomeManager::new(self.biomes, self.seed);
            self.material.build_surface(
                &mut layers,
                chunk_x,
                chunk_z,
                self.settings,
                &mut context,
                &self.router,
                &biomes,
                self.worldgen_noises(),
                self.positional,
            )?;
        }
        if stage >= Stage::Carvers {
            let biomes = BiomeManager::new(self.biomes, self.seed);
            self.carvers.generate(
                &mut layers,
                chunk_x,
                chunk_z,
                self.settings,
                &mut context,
                &self.router,
                &biomes,
                &mut aquifer,
                &self.material,
                self.worldgen.noises(),
                self.positional,
                self.seed,
            )?;
        }
        Ok(layers)
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
        allowed_kinds: Option<&'static [&'static str]>,
    ) -> Result<crate::feature::FeatureResult, String> {
        let biomes = BiomeManager::new(self.biomes, self.seed);
        let context = crate::feature::run::FeatureContext {
            registry: &self.features,
            samplers: &self.router,
            biomes: &biomes,
            settings: self.settings,
            seed: self.seed,
            cache_count: self.cache_count,
            steps_mask,
            allowed_kinds,
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
}
