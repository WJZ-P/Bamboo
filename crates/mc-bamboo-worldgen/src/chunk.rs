#![allow(dead_code)]
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
    biome::MultiNoiseBiomeSource,
    climate::TargetPoint,
    density::{self, FunctionRegistry, NoiseRegistry, Ref},
    math::{java_floor_div, quart_to_block},
    sampler::{Compiler, Context, Sampler},
    settings::Settings,
    volume::Volume,
};

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
    settings: &'a Settings,
    biomes: &'a MultiNoiseBiomeSource,
    router: Vec<Sampler>,
    aquifer: Option<AquiferFunctions>,
    /// `RandomState.getOrCreateRandomFactory("minecraft:aquifer")`。
    aquifer_random: crate::random::PositionalFactory,
    cache_count: usize,
    default_block: u32,
    default_fluid: u32,
    empty: u32,
}

/// 含水层用的六条密度函数，编译顺序和 `NoiseBasedAquifer` 的构造器一样。
#[derive(Debug)]
struct AquiferFunctions {
    barrier: Sampler,
    fluid_level_floodedness: Sampler,
    fluid_level_spread: Sampler,
    lava: Sampler,
    exclusion: Sampler,
    surface_level: Sampler,
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
        let router: Vec<Sampler> = [
            &settings.router.temperature,
            &settings.router.vegetation,
            &settings.router.continents,
            &settings.router.erosion,
            &settings.router.depth,
            &settings.router.ridges,
            &settings.router.chunk_surface_level,
            &settings.router.final_density,
        ]
        .iter()
        .map(|function| compiler.compile(function))
        .collect();

        let noises = worldgen.noises();
        let functions = worldgen.functions();
        let aquifer = raw
            .get("aquifers")
            .map(|value| parse_aquifer(value, noises, functions, &mut compiler))
            .transpose()?;
        let aquifer_random = compiler.random_factory("minecraft:aquifer");
        let default_block = block_state(raw.get("default_block"))?;
        let default_fluid = block_state(raw.get("default_fluid"))?;
        Ok(Self {
            settings,
            biomes,
            router,
            aquifer,
            aquifer_random,
            cache_count: compiler.cache_count(),
            default_block,
            default_fluid,
            empty: blocks::AIR,
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
        for field in [0usize, 1, 2, 3, 4, 5] {
            let mut buffer = context.acquire(volume.len());
            self.router[field].sample_volume(&mut context, &mut buffer, &volume);
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
