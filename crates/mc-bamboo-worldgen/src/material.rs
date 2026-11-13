//! 官方 `MaterialSystem` / `MaterialRuleContext` / `MaterialRules`：地表规则（含矿脉）。
//!
//! 规则树和条件树是数据包 codec 的 JSON，由 `mc-bamboo-vanilla` 从官方注册表导出，
//! 存在 `mc-bamboo-data` 的 `worldgen.json` 里。这里按官方 `compile` 的顺序把树编译成
//! 求值节点，再照 `MaterialSystem.buildSurface` 的逐列扫描应用到 `doFill` 的方块上。
//!
//! 只做纯函数：不接进游戏，不碰 `ProtoChunk` 的高度图/后期处理表以外的字段。
//! 官方 `getMinSurfaceLevel`、矿脉密度、条件噪声都走 `NoiseChunk` 的同一个
//! `cachingSamplers`，所以这里也用外部传进来的同一个 [`Context`]。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use mc_bamboo_data::{block_by_name, blocks};
use serde_json::Value;

use crate::{
    biome_manager::{self, BiomeManager, ClimateCache},
    density::{self, FunctionRegistry, NoiseRegistry, Ref},
    math::{floor_f32, java_floor_div, java_round},
    noise::Noise as NoiseInstance,
    random::{PositionalFactory, Random},
    sampler::{Compiler, Context, Sampler},
    settings::{Samplers as RouterSamplers, Settings},
    volume::Volume,
};

/// 官方 `null` 的 `BlockState`：规则没有给出方块。
const NO_STATE: u32 = u32::MAX;
/// `HOW_FAR_BELOW_PRELIMINARY_SURFACE_LEVEL_TO_BUILD_SURFACE`。
const BELOW_PRELIMINARY_SURFACE: i32 = 8;
/// `DimensionType.WAY_BELOW_MIN_Y`。
const WAY_BELOW_MIN_Y: i32 = -32512;
const I32_MIN: i32 = i32::MIN;
const I32_MAX: i32 = i32::MAX;
const BAND_COUNT: usize = 192;

/// 规则树的编译产物，按种子建一次。
#[derive(Debug)]
pub struct MaterialRules {
    root: RuleNode,
    /// `MaterialSystem` 的九条内置噪声只跟种子有关，按 `Generator` 缓存一次。
    system: Option<System>,
    /// 条件噪声实例，按 `noise_ids` 的顺序。
    condition_noises: Vec<Arc<NoiseInstance>>,
    /// 192 格陶土色带，按种子算一次。
    clay_bands: Vec<u32>,
    veins: Vec<Vein>,
    /// 条件噪声的 ID，按首次出现顺序编号。
    noise_ids: Vec<String>,
    /// `minecraft:ore` 的 positional random factory。
    ore_random: PositionalFactory,
}

#[derive(Debug)]
struct Vein {
    ore: u32,
    raw_ore: u32,
    filler: u32,
    raw_ore_chance: f32,
    density: Sampler,
    richness: Sampler,
    filler_gap: Sampler,
}

/// 一个区块里两条矿脉缓冲的求值状态。
struct VeinBuffers {
    density: Vec<f32>,
    richness: Vec<f32>,
}

/// 条件节点。官方的 `LazyYCondition` / `LazyXZCondition` 每个编译实例只在同一格上被问一次，
/// 求值直接用 `RuleContext` 当前状态即可，缓存只影响速度。
#[derive(Debug)]
enum CondNode {
    Biome(Vec<u32>),
    Not(Box<CondNode>),
    Hole,
    Steep,
    StoneDepth {
        ceiling: bool,
        add_surface_depth: bool,
        offset: i32,
        secondary_depth_range: i32,
    },
    YAbove {
        anchor: Anchor,
        add_stone_depth: bool,
        surface_depth_multiplier: i32,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    NoiseThreshold {
        noise: usize,
        is_3d: bool,
        min: f64,
        max: f64,
    },
    VerticalGradient {
        true_at_and_below: Anchor,
        false_at_and_above: Anchor,
        random: PositionalFactory,
    },
    AbovePreliminarySurface,
    Temperature,
}

#[derive(Debug)]
enum RuleNode {
    Block(u32),
    Bandlands,
    Sequence(Vec<RuleNode>),
    Condition(Box<CondNode>, Box<RuleNode>),
    Vein(usize),
}

/// 官方 `VerticalAnchor`。
#[derive(Debug, Clone, Copy)]
enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Anchor {
    fn from_json(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("anchor must be an object")?;
        for (name, kind) in [("absolute", 0), ("above_bottom", 1), ("below_top", 2)] {
            if let Some(offset) = object.get(name).and_then(Value::as_i64) {
                let offset = i32::try_from(offset).map_err(|_| "anchor out of range")?;
                return Ok(match kind {
                    0 => Self::Absolute(offset),
                    1 => Self::AboveBottom(offset),
                    _ => Self::BelowTop(offset),
                });
            }
        }
        Err("unknown vertical anchor".to_owned())
    }

    fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => min_y.wrapping_add(offset),
            Self::BelowTop(offset) => height
                .wrapping_sub(1)
                .wrapping_add(min_y)
                .wrapping_sub(offset),
        }
    }
}

/// `MaterialSystem` 构造器和 `MaterialRuleContext` 合在一起：一次地表扫描的全部状态。
struct Scan<'a> {
    surface_noise: &'a NoiseInstance,
    surface_secondary_noise: &'a NoiseInstance,
    clay_bands_offset_noise: &'a NoiseInstance,
    /// `MaterialSystem.noiseRandom`，`getSurfaceDepth` 和冰山扩展用它。
    positional: PositionalFactory,
    noises: Vec<Arc<NoiseInstance>>,
    clay_bands: Vec<u32>,
    sea_level: i32,
    min_y: i32,
    height: i32,
    /// 矿脉成批求值的体积。
    expected: Volume,
    preliminary: Volume,
    preliminary_buffer: Vec<f32>,
    preliminary_ready: bool,
    router: &'a RouterSamplers,
    biome_manager: &'a BiomeManager<'a>,
    /// 一次扫描里反复问的气候参数；`topMaterial` 单独建一张 1×1×1 的。
    climate: &'a mut ClimateCache,
    block_x: i32,
    block_z: i32,
    surface_gradient_x: i32,
    surface_gradient_z: i32,
    surface_depth: i32,
    surface_secondary: Option<f64>,
    min_surface_level: Option<i32>,
    block_y: i32,
    water_height: i32,
    stone_depth_below: i32,
    stone_depth_above: i32,
    biome: Option<u32>,
    /// `MaterialRuleContext.createNoiseSampler2d/3d` 的 `DoubleSupplier`：按代数惰性求值，
    /// 每列（2d）或每格（3d）第一次被问到才算一次。
    noise_2d: Vec<Option<(usize, f64)>>,
    noise_3d: Vec<Option<(usize, f64)>>,
    /// `MaterialRuleContext.lastUpdateXZ` / `lastUpdateY`。
    update_xz: usize,
    update_y: usize,
    veins: Vec<VeinBuffers>,
}

impl MaterialRules {
    /// 从 `worldgen.json` 的注册表编译主世界的规则树。
    ///
    /// `compiler` 必须是 `Generator` 用来编译路由和含水层的那个，官方一个 `RandomState`
    /// 只有一个 `DensityFunctionCompiler`，`cache` 编号因此共用。
    pub fn compile(
        root: &serde_json::Map<String, Value>,
        raw_functions: &serde_json::Map<String, Value>,
        noises: &NoiseRegistry,
        functions: &FunctionRegistry,
        compiler: &mut Compiler,
    ) -> Result<Self, String> {
        let rules = object(root, "material_rule")?;
        let conditions = object(root, "material_condition")?;
        let mut entries: HashMap<String, Ref> = functions
            .ids()
            .map(|id| {
                (
                    id.to_owned(),
                    Ref::clone(functions.get(id).expect("刚问过")),
                )
            })
            .collect();
        let mut resolving = HashSet::new();
        let mut veins = Vec::new();
        let mut noise_ids = Vec::new();
        let mut noise_map = HashMap::new();
        let mut visited = HashSet::new();
        let tree = parse_rule(
            &Value::String("minecraft:overworld".to_owned()),
            rules,
            conditions,
            raw_functions,
            noises,
            &mut entries,
            &mut resolving,
            compiler,
            &mut veins,
            &mut NoiseIds {
                names: &mut noise_ids,
                map: &mut noise_map,
            },
            &mut visited,
        )?;
        Ok(Self {
            root: tree,
            system: None,
            condition_noises: Vec::new(),
            clay_bands: Vec::new(),
            veins,
            noise_ids,
            ore_random: compiler.random_factory("minecraft:ore"),
        })
    }

    /// 按种子造一次九条内置噪声、条件噪声和陶土色带。
    pub fn prepare(
        &mut self,
        noises: &NoiseRegistry,
        positional: PositionalFactory,
        settings: &Settings,
    ) -> Result<(), String> {
        if self.system.is_some() {
            return Ok(());
        }
        let system = System::new(noises, positional, settings)?;
        self.condition_noises = self
            .noise_ids
            .iter()
            .map(|id| {
                let parameters = noises
                    .get(id)
                    .ok_or_else(|| format!("missing noise {id}"))?;
                Ok(Arc::new(parameters.create(&positional.from_hash_of(id))))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.clay_bands = generate_bands(&system.bands_random);
        self.system = Some(system);
        Ok(())
    }

    /// `MaterialSystem.buildSurface`：把规则应用到 `layers` 上。
    #[allow(clippy::too_many_arguments)]
    pub fn build_surface(
        &self,
        layers: &mut [Vec<u32>],
        chunk_x: i32,
        chunk_z: i32,
        settings: &Settings,
        context: &mut Context,
        router: &RouterSamplers,
        biome_manager: &BiomeManager<'_>,
        noises: &NoiseRegistry,
        positional: PositionalFactory,
    ) -> Result<(), String> {
        let system = self
            .system
            .as_ref()
            .ok_or("material system was not prepared")?;
        let highest = highest_filled_section(layers);
        let top_y = (highest + settings.min_y / 16) * 16 + 15;
        let min_x = chunk_x.wrapping_mul(16);
        let min_z = chunk_z.wrapping_mul(16);
        // `getBiome` 和规则条件用的是 (blockX-2, blockY-2, blockZ-2) 的 quart 格，
        // 加上八角插值可能选到的 +1 角；y 从 minY 到最高非空段。
        let mut climate = ClimateCache::new(
            java_floor_div(min_x.wrapping_sub(2), 4).wrapping_sub(1),
            java_floor_div(settings.min_y.wrapping_sub(2), 4).wrapping_sub(1),
            java_floor_div(min_z.wrapping_sub(2), 4).wrapping_sub(1),
            8,
            java_floor_div(settings.height, 4).wrapping_add(4),
            8,
        );
        let mut scan = Scan::new(
            self,
            system,
            layers,
            chunk_x,
            chunk_z,
            settings,
            context,
            router,
            biome_manager,
            &mut climate,
            noises,
            positional,
            (16, (top_y - settings.min_y + 1).max(1), 16),
            (min_x, settings.min_y, min_z),
            (16, 1, 16),
            (min_x, 0, min_z),
        )?;

        for x in 0..16 {
            for z in 0..16 {
                let block_x = min_x + x;
                let block_z = min_z + z;
                let start = world_surface_height(layers, settings.min_y, x, z) + 1;
                let biome = scan.biome_manager.biome_cached(
                    router,
                    &mut Context::uncached(),
                    block_x,
                    start,
                    block_z,
                    Some(scan.climate),
                );
                if biome_manager::biome_name(biome) == "minecraft:eroded_badlands" {
                    eroded_badlands_extension(
                        layers, system, settings, x, z, block_x, block_z, start,
                    );
                }
                let start = world_surface_height(layers, settings.min_y, x, z) + 1;
                scan.update_xz(
                    block_x,
                    block_z,
                    world_surface_height(layers, settings.min_y, (x + 1).min(15), z)
                        - world_surface_height(layers, settings.min_y, (x - 1).max(0), z),
                    world_surface_height(layers, settings.min_y, x, (z + 1).min(15))
                        - world_surface_height(layers, settings.min_y, x, (z - 1).max(0)),
                );
                let mut stone_depth_above: i32 = 0;
                let mut min_y_stone = I32_MAX;
                let mut water_height = I32_MIN;
                for y in (settings.min_y..=start).rev() {
                    let state = layer_get(layers, settings.min_y, x, y, z);
                    if biome_manager::is_air(state) {
                        stone_depth_above = 0;
                        water_height = I32_MIN;
                        continue;
                    }
                    if !biome_manager::fluid_is_empty(state) {
                        if water_height == I32_MIN {
                            water_height = y.wrapping_add(1);
                        }
                        continue;
                    }
                    if min_y_stone >= y {
                        min_y_stone = WAY_BELOW_MIN_Y;
                        let mut below = y.wrapping_sub(1);
                        while below >= settings.min_y.wrapping_sub(1) {
                            if !is_stone(layer_get(layers, settings.min_y, x, below, z)) {
                                min_y_stone = below.wrapping_add(1);
                                break;
                            }
                            below = below.wrapping_sub(1);
                        }
                    }
                    stone_depth_above = stone_depth_above.wrapping_add(1);
                    let stone_depth_below = y.wrapping_sub(min_y_stone).wrapping_add(1);
                    scan.update_y(stone_depth_above, stone_depth_below, water_height, y);
                    if y >= settings.min_y && y < settings.min_y + settings.height {
                        let state = self.apply(&self.root, &mut scan, context, block_x, y, block_z);
                        if state != NO_STATE {
                            layer_set(layers, settings.min_y, x, y, z, state);
                        }
                    }
                }
                if matches!(
                    biome_manager::biome_name(biome),
                    "minecraft:frozen_ocean" | "minecraft:deep_frozen_ocean"
                ) {
                    let min_surface = scan.min_surface_level(context);
                    frozen_ocean_extension(
                        layers,
                        system,
                        settings,
                        x,
                        z,
                        block_x,
                        block_z,
                        start,
                        min_surface,
                        biome,
                    );
                }
            }
        }
        for buffer in scan.veins {
            context.release(buffer.density);
            context.release(buffer.richness);
        }
        context.release(scan.preliminary_buffer);
        Ok(())
    }

    /// `MaterialSystem.topMaterial`：在 1×1×1 的体积上重新编译并应用一次规则树。
    ///
    /// 雕刻器把草方块挖掉后，用它决定下面那格泥土该变成什么。
    #[allow(clippy::too_many_arguments)]
    pub fn top_material(
        &self,
        layers: &[Vec<u32>],
        chunk_x: i32,
        chunk_z: i32,
        x: i32,
        y: i32,
        z: i32,
        surface_gradient_x: i32,
        surface_gradient_z: i32,
        water_height: i32,
        settings: &Settings,
        context: &mut Context,
        router: &RouterSamplers,
        biome_manager: &BiomeManager<'_>,
        noises: &NoiseRegistry,
        positional: PositionalFactory,
    ) -> Result<Option<u32>, String> {
        let system = self
            .system
            .as_ref()
            .ok_or("material system was not prepared")?;
        let mut climate = ClimateCache::new(
            java_floor_div(x.wrapping_sub(2), 4),
            java_floor_div(y.wrapping_sub(2), 4),
            java_floor_div(z.wrapping_sub(2), 4),
            2,
            2,
            2,
        );
        let mut scan = Scan::new(
            self,
            system,
            layers,
            chunk_x,
            chunk_z,
            settings,
            context,
            router,
            biome_manager,
            &mut climate,
            noises,
            positional,
            (1, 1, 1),
            (x, y, z),
            (1, 1, 1),
            (x, 0, z),
        )?;
        scan.update_xz(x, z, surface_gradient_x, surface_gradient_z);
        scan.update_y(1, 1, water_height, y);
        let state = self.apply(&self.root, &mut scan, context, x, y, z);
        for buffer in scan.veins {
            context.release(buffer.density);
            context.release(buffer.richness);
        }
        context.release(scan.preliminary_buffer);
        Ok((state != NO_STATE).then_some(state))
    }

    /// `RuleEvaluator.tryApply`。
    fn apply(
        &self,
        node: &RuleNode,
        scan: &mut Scan<'_>,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
    ) -> u32 {
        match node {
            RuleNode::Block(state) => *state,
            RuleNode::Bandlands => scan.get_band(x, y, z),
            RuleNode::Sequence(sequence) => {
                for child in sequence {
                    let state = self.apply(child, scan, context, x, y, z);
                    if state != NO_STATE {
                        return state;
                    }
                }
                NO_STATE
            }
            RuleNode::Condition(condition, then) => {
                if self.test(condition, scan, context) {
                    self.apply(then, scan, context, x, y, z)
                } else {
                    NO_STATE
                }
            }
            RuleNode::Vein(index) => {
                let vein = &self.veins[*index];
                let buffers = &scan.veins[*index];
                let density = density_get(
                    context,
                    &vein.density,
                    &buffers.density,
                    scan.expected,
                    x,
                    y,
                    z,
                );
                if density <= 0.0 {
                    return NO_STATE;
                }
                let random = self.ore_random.at(x, y, z);
                if random.next_float() > density {
                    return NO_STATE;
                }
                let richness = density_get(
                    context,
                    &vein.richness,
                    &buffers.richness,
                    scan.expected,
                    x,
                    y,
                    z,
                );
                if random.next_float() < richness
                    && vein.filler_gap.sample_value(context, x, y, z) < 0.0
                {
                    if random.next_float() < vein.raw_ore_chance {
                        return vein.raw_ore;
                    }
                    return vein.ore;
                }
                vein.filler
            }
        }
    }

    fn test(&self, condition: &CondNode, scan: &mut Scan<'_>, context: &mut Context) -> bool {
        match condition {
            CondNode::Biome(biomes) => biomes.contains(&scan.get_biome()),
            CondNode::Not(child) => !self.test(child, scan, context),
            CondNode::Hole => scan.surface_depth <= 0,
            CondNode::Steep => scan.surface_gradient_x <= -4 || scan.surface_gradient_z >= 4,
            CondNode::StoneDepth {
                ceiling,
                add_surface_depth,
                offset,
                secondary_depth_range,
            } => {
                let depth = if *ceiling {
                    scan.stone_depth_below
                } else {
                    scan.stone_depth_above
                };
                let surface = if *add_surface_depth {
                    scan.surface_depth
                } else {
                    0
                };
                let secondary = if *secondary_depth_range == 0 {
                    0
                } else {
                    map(
                        scan.surface_secondary(),
                        -1.0,
                        1.0,
                        0.0,
                        f64::from(*secondary_depth_range),
                    ) as i32
                };
                depth
                    <= 1_i32
                        .wrapping_add(*offset)
                        .wrapping_add(surface)
                        .wrapping_add(secondary)
            }
            CondNode::YAbove {
                anchor,
                add_stone_depth,
                surface_depth_multiplier,
            } => {
                let extra = if *add_stone_depth {
                    scan.stone_depth_above
                } else {
                    0
                };
                let left = scan.block_y.wrapping_add(extra);
                let right = anchor
                    .resolve(scan.min_y, scan.height)
                    .wrapping_add(scan.surface_depth.wrapping_mul(*surface_depth_multiplier));
                left >= right
            }
            CondNode::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let extra = if *add_stone_depth {
                    scan.stone_depth_above
                } else {
                    0
                };
                let left = scan.block_y.wrapping_add(extra);
                let right = scan
                    .water_height
                    .wrapping_add(*offset)
                    .wrapping_add(scan.surface_depth.wrapping_mul(*surface_depth_multiplier));
                scan.water_height == I32_MIN || left >= right
            }
            CondNode::NoiseThreshold {
                noise,
                is_3d,
                min,
                max,
            } => {
                let value = scan.noise_get(*noise, *is_3d);
                value >= *min && value <= *max
            }
            CondNode::VerticalGradient {
                true_at_and_below,
                false_at_and_above,
                random,
            } => {
                let y = scan.block_y;
                let low = true_at_and_below.resolve(scan.min_y, scan.height);
                let high = false_at_and_above.resolve(scan.min_y, scan.height);
                if y <= low {
                    return true;
                }
                if y >= high {
                    return false;
                }
                let probability = map(f64::from(y), f64::from(low), f64::from(high), 1.0, 0.0);
                let source = random.at(scan.block_x, y, scan.block_z);
                f64::from(source.next_float()) < probability
            }
            CondNode::AbovePreliminarySurface => scan.block_y >= scan.min_surface_level(context),
            CondNode::Temperature => biome_manager::cold_enough_to_snow(
                scan.get_biome(),
                scan.block_x,
                scan.block_y,
                scan.block_z,
                scan.sea_level,
            ),
        }
    }
}

impl<'a> Scan<'a> {
    /// 造一次地表扫描的状态；`topMaterial` 也走这里，只是体积是 1×1×1。
    #[allow(clippy::too_many_arguments)]
    fn new(
        rules: &'a MaterialRules,
        system: &'a System,
        _layers: &[Vec<u32>],
        chunk_x: i32,
        chunk_z: i32,
        settings: &Settings,
        context: &mut Context,
        router: &'a RouterSamplers,
        biome_manager: &'a BiomeManager<'a>,
        climate: &'a mut ClimateCache,
        _noises: &NoiseRegistry,
        positional: PositionalFactory,
        expected_size: (i32, i32, i32),
        expected_min: (i32, i32, i32),
        preliminary_size: (i32, i32, i32),
        preliminary_min: (i32, i32, i32),
    ) -> Result<Scan<'a>, String> {
        let min_x = chunk_x.wrapping_mul(16);
        let min_z = chunk_z.wrapping_mul(16);
        let expected = Volume::blocks(expected_size, expected_min);
        // 官方 `MaterialRule.compile` 的顺序：两条矿脉各对 density/richness 成批求值，
        // fillerGap 不求值（编译时只是 `getDensitiesInChunk(fn, false)`）。
        let mut vein_buffers = Vec::with_capacity(rules.veins.len());
        for vein in &rules.veins {
            let mut density = context.acquire(expected.len());
            vein.density.sample_volume(context, &mut density, &expected);
            let mut richness = context.acquire(expected.len());
            vein.richness
                .sample_volume(context, &mut richness, &expected);
            vein_buffers.push(VeinBuffers { density, richness });
        }
        let preliminary = Volume::blocks(preliminary_size, preliminary_min);
        let condition_noises = rules.condition_noises.clone();

        Ok(Scan {
            surface_noise: &system.surface,
            surface_secondary_noise: &system.surface_secondary,
            clay_bands_offset_noise: &system.clay_bands_offset,
            positional,
            noises: condition_noises,
            clay_bands: rules.clay_bands.clone(),
            sea_level: settings.sea_level,
            min_y: settings.min_y,
            height: settings.height,
            expected,
            preliminary,
            preliminary_buffer: context.acquire(preliminary.len()),
            preliminary_ready: false,
            router,
            biome_manager,
            climate,
            block_x: min_x,
            block_z: min_z,
            surface_gradient_x: 0,
            surface_gradient_z: 0,
            surface_depth: 0,
            surface_secondary: None,
            min_surface_level: None,
            block_y: 0,
            water_height: I32_MIN,
            stone_depth_below: 0,
            stone_depth_above: 0,
            biome: None,
            noise_2d: vec![None; rules.noise_ids.len()],
            noise_3d: vec![None; rules.noise_ids.len()],
            update_xz: 0,
            update_y: 0,
            veins: vein_buffers,
        })
    }
}

impl Scan<'_> {
    fn update_xz(&mut self, x: i32, z: i32, gradient_x: i32, gradient_z: i32) {
        // 官方 `updateXZ` 同时把 lastUpdateXZ 和 lastUpdateY 各加一。
        self.update_xz = self.update_xz.wrapping_add(1);
        self.update_y = self.update_y.wrapping_add(1);
        self.block_x = x;
        self.block_z = z;
        self.surface_gradient_x = gradient_x;
        self.surface_gradient_z = gradient_z;
        self.surface_depth = surface_depth(self.surface_noise, self.positional, x, z);
        self.surface_secondary = None;
        self.min_surface_level = None;
        self.biome = None;
    }

    fn update_y(&mut self, above: i32, below: i32, water: i32, y: i32) {
        self.update_y = self.update_y.wrapping_add(1);
        self.biome = None;
        self.block_y = y;
        self.water_height = water;
        self.stone_depth_below = below;
        self.stone_depth_above = above;
    }

    fn get_biome(&mut self) -> u32 {
        if let Some(id) = self.biome {
            return id;
        }
        let id = self.biome_manager.biome_cached(
            self.router,
            &mut Context::uncached(),
            self.block_x,
            self.block_y,
            self.block_z,
            Some(self.climate),
        );
        self.biome = Some(id);
        id
    }

    fn surface_secondary(&mut self) -> f64 {
        if let Some(value) = self.surface_secondary {
            return value;
        }
        let value = f64::from(self.surface_secondary_noise.get(
            f64::from(self.block_x),
            0.0,
            f64::from(self.block_z),
        ));
        self.surface_secondary = Some(value);
        value
    }

    fn min_surface_level(&mut self, context: &mut Context) -> i32 {
        if let Some(value) = self.min_surface_level {
            return value;
        }
        // 官方只在这里成批求一次，缓冲留在 context 的 cache 里给之后的单点求值用。
        if !self.preliminary_ready {
            self.router.chunk_surface_level.sample_volume(
                context,
                &mut self.preliminary_buffer,
                &self.preliminary,
            );
            self.preliminary_ready = true;
        }
        let value = match self
            .preliminary
            .index_of_block(self.block_x, 0, self.block_z)
        {
            Some(index) => self.preliminary_buffer[index],
            None => {
                self.router
                    .chunk_surface_level
                    .sample_value(context, self.block_x, 0, self.block_z)
            }
        };
        let level = floor_f32(value) + self.surface_depth - BELOW_PRELIMINARY_SURFACE;
        self.min_surface_level = Some(level);
        level
    }

    /// `MaterialRuleContext$1/$2.getAsDouble`：代数没变就返回上次的值，变了才调噪声。
    fn noise_get(&mut self, index: usize, is_3d: bool) -> f64 {
        let (slot, generation) = if is_3d {
            (&mut self.noise_3d[index], self.update_y)
        } else {
            (&mut self.noise_2d[index], self.update_xz)
        };
        if let Some((seen, value)) = *slot
            && seen == generation
        {
            return value;
        }
        let value = f64::from(self.noises[index].get(
            f64::from(self.block_x),
            if is_3d { f64::from(self.block_y) } else { 0.0 },
            f64::from(self.block_z),
        ));
        *slot = Some((generation, value));
        value
    }

    fn get_band(&self, x: i32, y: i32, z: i32) -> u32 {
        let offset = java_round(
            self.clay_bands_offset_noise
                .get(f64::from(x), 0.0, f64::from(z))
                * 4.0,
        );
        let index = (y.wrapping_add(offset).wrapping_add(BAND_COUNT as i32))
            .rem_euclid(BAND_COUNT as i32) as usize;
        self.clay_bands[index]
    }
}

/// `MaterialSystem` 的九条内置噪声，用 `RandomState.random` 实例化。
#[derive(Debug)]
struct System {
    surface: NoiseInstance,
    surface_secondary: NoiseInstance,
    clay_bands_offset: NoiseInstance,
    badlands_pillar: NoiseInstance,
    badlands_pillar_roof: NoiseInstance,
    badlands_surface: NoiseInstance,
    iceberg_pillar: NoiseInstance,
    iceberg_pillar_roof: NoiseInstance,
    iceberg_surface: NoiseInstance,
    positional: PositionalFactory,
    bands_random: Random,
}

impl System {
    fn new(
        noises: &NoiseRegistry,
        positional: PositionalFactory,
        _settings: &Settings,
    ) -> Result<Self, String> {
        let make = |name: &str| -> Result<NoiseInstance, String> {
            let parameters = noises
                .get(name)
                .ok_or_else(|| format!("missing noise {name}"))?;
            Ok(parameters.create(&positional.from_hash_of(name)))
        };
        Ok(Self {
            surface: make("minecraft:surface")?,
            surface_secondary: make("minecraft:surface_secondary")?,
            clay_bands_offset: make("minecraft:clay_bands_offset")?,
            badlands_pillar: make("minecraft:badlands_pillar")?,
            badlands_pillar_roof: make("minecraft:badlands_pillar_roof")?,
            badlands_surface: make("minecraft:badlands_surface")?,
            iceberg_pillar: make("minecraft:iceberg_pillar")?,
            iceberg_pillar_roof: make("minecraft:iceberg_pillar_roof")?,
            iceberg_surface: make("minecraft:iceberg_surface")?,
            positional,
            bands_random: positional.from_hash_of("minecraft:clay_bands"),
        })
    }
}

/// 编译期按首次出现顺序给条件噪声编号。
struct NoiseIds<'a> {
    names: &'a mut Vec<String>,
    map: &'a mut HashMap<String, usize>,
}

impl NoiseIds<'_> {
    fn id(&mut self, name: &str) -> usize {
        if let Some(id) = self.map.get(name) {
            return *id;
        }
        let id = self.names.len();
        self.names.push(name.to_owned());
        self.map.insert(name.to_owned(), id);
        id
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_rule(
    value: &Value,
    rules: &serde_json::Map<String, Value>,
    conditions: &serde_json::Map<String, Value>,
    raw_functions: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
    compiler: &mut Compiler,
    veins: &mut Vec<Vein>,
    noise_ids: &mut NoiseIds<'_>,
    visited: &mut HashSet<String>,
) -> Result<RuleNode, String> {
    if let Value::String(id) = value {
        if !visited.insert(id.clone()) {
            return Err(format!("recursive material rule {id}"));
        }
        let found = rules
            .get(id)
            .ok_or_else(|| format!("unknown material rule {id}"))?;
        let parsed = parse_rule(
            found,
            rules,
            conditions,
            raw_functions,
            noises,
            entries,
            resolving,
            compiler,
            veins,
            noise_ids,
            visited,
        )?;
        visited.remove(id);
        return Ok(parsed);
    }
    let object = value.as_object().ok_or("material rule must be an object")?;
    match string_field(object, "type")? {
        "minecraft:block" => Ok(RuleNode::Block(block_state(string_field(
            object,
            "result_state",
        )?)?)),
        "minecraft:bandlands" => Ok(RuleNode::Bandlands),
        "minecraft:sequence" => {
            let mut sequence = Vec::new();
            for child in array_field(object, "sequence")? {
                sequence.push(parse_rule(
                    child,
                    rules,
                    conditions,
                    raw_functions,
                    noises,
                    entries,
                    resolving,
                    compiler,
                    veins,
                    noise_ids,
                    visited,
                )?);
            }
            Ok(RuleNode::Sequence(sequence))
        }
        "minecraft:condition" => {
            let condition = parse_condition(
                object.get("if_true").ok_or("missing if_true")?,
                conditions,
                compiler,
                noise_ids,
            )?;
            let then = parse_rule(
                object.get("then_run").ok_or("missing then_run")?,
                rules,
                conditions,
                raw_functions,
                noises,
                entries,
                resolving,
                compiler,
                veins,
                noise_ids,
                visited,
            )?;
            Ok(RuleNode::Condition(Box::new(condition), Box::new(then)))
        }
        "minecraft:ore_vein" => {
            let density = compile_density(
                object.get("density").ok_or("missing density")?,
                raw_functions,
                noises,
                entries,
                resolving,
                compiler,
            )?;
            let richness = compile_density(
                object.get("richness").ok_or("missing richness")?,
                raw_functions,
                noises,
                entries,
                resolving,
                compiler,
            )?;
            let filler_gap = compile_density(
                object.get("filler_gap").ok_or("missing filler_gap")?,
                raw_functions,
                noises,
                entries,
                resolving,
                compiler,
            )?;
            let index = veins.len();
            veins.push(Vein {
                ore: block_state(string_field(object, "ore_block")?)?,
                raw_ore: block_state(string_field(object, "raw_ore_block")?)?,
                filler: block_state(string_field(object, "filler_block")?)?,
                raw_ore_chance: float_field(object, "raw_ore_chance")?,
                density,
                richness,
                filler_gap,
            });
            Ok(RuleNode::Vein(index))
        }
        other => Err(format!("unknown material rule type {other}")),
    }
}

fn parse_condition(
    value: &Value,
    conditions: &serde_json::Map<String, Value>,
    compiler: &mut Compiler,
    noise_ids: &mut NoiseIds<'_>,
) -> Result<CondNode, String> {
    if let Value::String(id) = value {
        let found = conditions
            .get(id)
            .ok_or_else(|| format!("unknown material condition {id}"))?;
        return parse_condition(found, conditions, compiler, noise_ids);
    }
    let object = value.as_object().ok_or("condition must be an object")?;
    match string_field(object, "type")? {
        "minecraft:biome" => {
            let names: Vec<u32> = match object.get("biome_is") {
                Some(Value::String(name)) => vec![biome_manager::biome_id(name)],
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(biome_manager::biome_id)
                            .ok_or_else(|| "biome_is must be a string".to_owned())
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err("missing biome_is".to_owned()),
            };
            Ok(CondNode::Biome(names))
        }
        "minecraft:not" => Ok(CondNode::Not(Box::new(parse_condition(
            object.get("invert").ok_or("missing invert")?,
            conditions,
            compiler,
            noise_ids,
        )?))),
        "minecraft:hole" => Ok(CondNode::Hole),
        "minecraft:steep" => Ok(CondNode::Steep),
        "minecraft:stone_depth" => Ok(CondNode::StoneDepth {
            ceiling: string_field(object, "surface_type")? == "ceiling",
            add_surface_depth: bool_field(object, "add_surface_depth")?,
            offset: int_field(object, "offset")?,
            secondary_depth_range: int_field(object, "secondary_depth_range")?,
        }),
        "minecraft:y_above" => Ok(CondNode::YAbove {
            anchor: Anchor::from_json(object.get("anchor").ok_or("missing anchor")?)?,
            add_stone_depth: bool_field(object, "add_stone_depth")?,
            surface_depth_multiplier: int_field(object, "surface_depth_multiplier")?,
        }),
        "minecraft:water" => Ok(CondNode::Water {
            offset: int_field(object, "offset")?,
            surface_depth_multiplier: int_field(object, "surface_depth_multiplier")?,
            add_stone_depth: bool_field(object, "add_stone_depth")?,
        }),
        "minecraft:noise_threshold" => Ok(CondNode::NoiseThreshold {
            noise: noise_ids.id(string_field(object, "noise")?),
            is_3d: bool_field(object, "is_3d").unwrap_or(false),
            min: double_field(object, "min_threshold")?,
            max: double_field(object, "max_threshold")?,
        }),
        "minecraft:vertical_gradient" => {
            let name = string_field(object, "random_name")?.to_owned();
            let random = compiler.random_factory(&name);
            Ok(CondNode::VerticalGradient {
                true_at_and_below: Anchor::from_json(
                    object
                        .get("true_at_and_below")
                        .ok_or("missing true_at_and_below")?,
                )?,
                false_at_and_above: Anchor::from_json(
                    object
                        .get("false_at_and_above")
                        .ok_or("missing false_at_and_above")?,
                )?,
                random,
            })
        }
        "minecraft:above_preliminary_surface" => Ok(CondNode::AbovePreliminarySurface),
        "minecraft:temperature" => Ok(CondNode::Temperature),
        other => Err(format!("unknown material condition type {other}")),
    }
}

fn compile_density(
    value: &Value,
    raw_functions: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
    compiler: &mut Compiler,
) -> Result<Sampler, String> {
    let function = density::parse(value, raw_functions, noises, entries, resolving)?;
    Ok(compiler.compile(&function))
}

/// `Mth.map(value, inMin, inMax, outMin, outMax)`：先 `inverseLerp` 再 `lerp`。
fn map(value: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    let ratio = (value - in_min) / (in_max - in_min);
    out_min + ratio * (out_max - out_min)
}

/// `MaterialSystem.getSurfaceDepth`。
fn surface_depth(noise: &NoiseInstance, positional: PositionalFactory, x: i32, z: i32) -> i32 {
    let value = f64::from(noise.get(f64::from(x), 0.0, f64::from(z)));
    let with_constant = value * 2.75 + 3.0;
    let jitter = positional.at(x, 0, z).next_double() * 0.25;
    (with_constant + jitter) as i32
}

/// `MaterialRules.DensityGetter`：落在 expectedVolume 的格点就读成批缓冲，否则单点求值。
fn density_get(
    context: &mut Context,
    sampler: &Sampler,
    buffer: &[f32],
    volume: Volume,
    x: i32,
    y: i32,
    z: i32,
) -> f32 {
    match volume.index_of_block(x, y, z) {
        Some(index) => buffer[index],
        None => sampler.sample_value(context, x, y, z),
    }
}

/// `MaterialSystem.erodedBadlandsExtension`。
#[allow(clippy::too_many_arguments)]
fn eroded_badlands_extension(
    layers: &mut [Vec<u32>],
    system: &System,
    settings: &Settings,
    x: i32,
    z: i32,
    block_x: i32,
    block_z: i32,
    start: i32,
) {
    let surface = f64::from(system.badlands_surface.get(
        f64::from(block_x),
        0.0,
        f64::from(block_z),
    )) * 8.25;
    let pillar =
        system
            .badlands_pillar
            .get(f64::from(block_x) * 0.2, 0.0, f64::from(block_z) * 0.2)
            * 15.0;
    let height = surface.abs().min(f64::from(pillar));
    if height <= 0.0 {
        return;
    }
    let roof = (f64::from(system.badlands_pillar_roof.get(
        f64::from(block_x) * 0.75,
        0.0,
        f64::from(block_z) * 0.75,
    )) * 1.5)
        .abs();
    let top = 64.0 + (height * height * 2.5).min((roof * 50.0).ceil() + 24.0);
    let y = top.floor() as i32;
    if start > y {
        return;
    }
    let default = block_state("minecraft:stone").unwrap_or(blocks::AIR);
    let water = block_state("minecraft:water").unwrap_or(blocks::AIR);
    let mut level = y;
    while level >= settings.min_y {
        let state = layer_get(layers, settings.min_y, x, level, z);
        if state == default {
            break;
        }
        if state == water {
            return;
        }
        level -= 1;
    }
    let mut level = y;
    while level >= settings.min_y {
        let state = layer_get(layers, settings.min_y, x, level, z);
        if !biome_manager::is_air(state) {
            return;
        }
        layer_set(layers, settings.min_y, x, level, z, default);
        level -= 1;
    }
}

/// `MaterialSystem.frozenOceanExtension`。
#[allow(clippy::too_many_arguments)]
fn frozen_ocean_extension(
    layers: &mut [Vec<u32>],
    system: &System,
    settings: &Settings,
    x: i32,
    z: i32,
    block_x: i32,
    block_z: i32,
    start: i32,
    min_surface_level: i32,
    biome: u32,
) {
    let surface = f64::from(system.iceberg_surface.get(
        f64::from(block_x),
        0.0,
        f64::from(block_z),
    )) * 8.25;
    let pillar =
        system
            .iceberg_pillar
            .get(f64::from(block_x) * 1.28, 0.0, f64::from(block_z) * 1.28)
            * 15.0;
    let height = surface.abs().min(f64::from(pillar));
    if height <= 1.8 {
        return;
    }
    let roof = (f64::from(system.iceberg_pillar_roof.get(
        f64::from(block_x) * 1.17,
        0.0,
        f64::from(block_z) * 1.17,
    )) * 1.5)
        .abs();
    let mut size = (height * height * 1.2).min((roof * 40.0).ceil() + 14.0);
    if biome_manager::biome_temperature(
        biome,
        block_x,
        settings.sea_level,
        block_z,
        settings.sea_level,
    ) > 0.1
    {
        size -= 2.0;
    }
    if size <= 2.0 {
        return;
    }
    let lower = f64::from(settings.sea_level) - size - 7.0;
    let top = f64::from(settings.sea_level) + size;
    let random = system.positional.at(block_x, 0, block_z);
    let snow = 2 + random.next_int(4);
    let snow_line = settings.sea_level + 18 + random.next_int(10);
    let mut snow_count = 0;
    let mut level = start.max(top as i32 + 1);
    let snow_block = block_state("minecraft:snow_block").unwrap_or(blocks::AIR);
    let packed_ice = block_state("minecraft:packed_ice").unwrap_or(blocks::AIR);
    while level >= min_surface_level {
        let state = layer_get(layers, settings.min_y, x, level, z);
        let melt =
            biome_manager::is_air(state) && level < top as i32 && random.next_double() > 0.01;
        let water = state == block_state("minecraft:water").unwrap_or(blocks::AIR)
            && level > lower as i32
            && level < settings.sea_level
            && random.next_double() > 0.15;
        if melt || water {
            if snow_count <= snow && level > snow_line {
                layer_set(layers, settings.min_y, x, level, z, snow_block);
                snow_count += 1;
            } else {
                layer_set(layers, settings.min_y, x, level, z, packed_ice);
            }
        }
        level -= 1;
    }
}

/// `MaterialSystem.generateBands`。
fn generate_bands(random: &Random) -> Vec<u32> {
    let terracotta = block_state("minecraft:terracotta").unwrap_or(blocks::AIR);
    let orange = block_state("minecraft:orange_terracotta").unwrap_or(terracotta);
    let yellow = block_state("minecraft:yellow_terracotta").unwrap_or(terracotta);
    let brown = block_state("minecraft:brown_terracotta").unwrap_or(terracotta);
    let red = block_state("minecraft:red_terracotta").unwrap_or(terracotta);
    let white = block_state("minecraft:white_terracotta").unwrap_or(terracotta);
    let light_gray = block_state("minecraft:light_gray_terracotta").unwrap_or(terracotta);
    let mut bands = vec![terracotta; BAND_COUNT];
    let mut i = 0;
    while i < bands.len() {
        i += random.next_int(5) as usize + 1;
        if i < bands.len() {
            bands[i] = orange;
        }
        i += 1;
    }
    make_bands(random, &mut bands, 1, yellow);
    make_bands(random, &mut bands, 2, brown);
    make_bands(random, &mut bands, 1, red);
    let count = (random.next_int(7) + 9) as usize;
    let mut made = 0;
    let mut at = 0;
    while made < count && at < bands.len() {
        bands[at] = white;
        if at > 0 && random.next_boolean() {
            bands[at - 1] = light_gray;
        }
        if at + 1 < bands.len() && random.next_boolean() {
            bands[at + 1] = light_gray;
        }
        made += 1;
        at += random.next_int(16) as usize + 4;
    }
    bands
}

fn make_bands(random: &Random, bands: &mut [u32], base: i32, state: u32) {
    let count = random.next_int(10) + 6;
    for _ in 0..count {
        let size = base + random.next_int(3);
        let start = random.next_int(bands.len() as i32) as usize;
        for offset in 0..size.max(0) as usize {
            if start + offset >= bands.len() {
                break;
            }
            bands[start + offset] = state;
        }
    }
}

/// `MaterialSystem.getSurfaceGradientX`：给 `topMaterial` 用。
pub fn surface_gradient_x(layers: &[Vec<u32>], min_y: i32, x: i32, z: i32) -> i32 {
    world_surface_height(layers, min_y, (x + 1).min(15), z)
        - world_surface_height(layers, min_y, (x - 1).max(0), z)
}

/// `MaterialSystem.getSurfaceGradientZ`：给 `topMaterial` 用。
pub fn surface_gradient_z(layers: &[Vec<u32>], min_y: i32, x: i32, z: i32) -> i32 {
    world_surface_height(layers, min_y, x, (z + 1).min(15))
        - world_surface_height(layers, min_y, x, (z - 1).max(0))
}

/// `WORLD_SURFACE_WG`：从顶往下第一个非空方块。
fn world_surface_height(layers: &[Vec<u32>], min_y: i32, x: i32, z: i32) -> i32 {
    for (index, layer) in layers.iter().enumerate().rev() {
        if !biome_manager::is_air(layer[x as usize * 16 + z as usize]) {
            return min_y + index as i32;
        }
    }
    min_y - 1
}

/// `getHighestFilledSectionIndex`：最上面那个非全空的 16 格段（相对 minY 的段号）。
fn highest_filled_section(layers: &[Vec<u32>]) -> i32 {
    let mut top = -1;
    for (index, layer) in layers.iter().enumerate() {
        if layer.iter().any(|state| !biome_manager::is_air(*state)) {
            top = index as i32 / 16;
        }
    }
    top
}

fn is_stone(state: u32) -> bool {
    !biome_manager::is_air(state) && biome_manager::fluid_is_empty(state)
}

fn layer_get(layers: &[Vec<u32>], min_y: i32, x: i32, y: i32, z: i32) -> u32 {
    let index = y.wrapping_sub(min_y);
    if index < 0 || index as usize >= layers.len() {
        return blocks::AIR;
    }
    layers[index as usize][x as usize * 16 + z as usize]
}

fn layer_set(layers: &mut [Vec<u32>], min_y: i32, x: i32, y: i32, z: i32, state: u32) {
    let index = y.wrapping_sub(min_y);
    if index < 0 || index as usize >= layers.len() {
        return;
    }
    layers[index as usize][x as usize * 16 + z as usize] = state;
}

fn block_state(name: &str) -> Result<u32, String> {
    Ok(block_by_name(name)
        .ok_or_else(|| format!("unknown block {name}"))?
        .default_state)
}

fn object<'a>(
    root: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    root.get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing worldgen field {name}"))
}

fn string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing field {name}"))
}

fn int_field(object: &serde_json::Map<String, Value>, name: &str) -> Result<i32, String> {
    object
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("missing field {name}"))
}

fn float_field(object: &serde_json::Map<String, Value>, name: &str) -> Result<f32, String> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| format!("missing field {name}"))
}

fn double_field(object: &serde_json::Map<String, Value>, name: &str) -> Result<f64, String> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("missing field {name}"))
}

fn bool_field(object: &serde_json::Map<String, Value>, name: &str) -> Result<bool, String> {
    object
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing field {name}"))
}

fn array_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a Vec<Value>, String> {
    object
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing field {name}"))
}
