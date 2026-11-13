//! 官方 `generateCarvers` / `WorldCarver` / `CarvingMask` / `applyCarvingMask`。
//!
//! 雕刻器的数据（四条 carver 配置和每个群系的 carver 列表）由 `mc-bamboo-vanilla`
//! 从官方 codec 导出。这里按 `NoiseBasedChunkGenerator.generateCarvers` 的 17×17 邻域
//! 循环造一张 mask，再照 `applyCarvingMask` 从高到低把挖到的方块换回含水层的物质。
//!
//! `WorldgenRandom` 是旧随机源；隧道内部用 `SingleThreadedRandomSource`，和
//! `LegacyRandomSource` 的取数序列完全一致，只是 `setSeed` 的常量不同。

use std::collections::HashMap;

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_data::{block_by_name, block_id_of_state, block_tag, blocks};
use serde_json::Value;

use crate::{
    aquifer::Aquifer,
    biome_manager::{BiomeManager, ClimateCache},
    density::NoiseRegistry,
    material::MaterialRules,
    math::{floor, java_floor_div},
    random::{PositionalFactory, Random, WorldgenRandom},
    sampler::Context,
    settings::{Samplers as RouterSamplers, Settings},
};

/// `WorldCarver.getRange()`：洞穴和峡谷都是 4（17×17 个区块）。
const RANGE: i32 = 4;
/// `SingleThreadedRandomSource` 的种子常量。
/// `Mth.SIN_SCALE` 和查找表长度。
const SIN_SCALE: f64 = 10_430.378_350_470_453;
const SIN_MASK: i64 = 65_535;
use std::sync::OnceLock;

static SIN_TABLE: OnceLock<Vec<f32>> = OnceLock::new();

fn sin_table() -> &'static [f32] {
    SIN_TABLE.get_or_init(|| {
        (0..65_536)
            .map(|index| (index as f64 / SIN_SCALE).sin() as f32)
            .collect()
    })
}

/// 官方 `Mth.sin(double)` 和 `cos(double)`：65536 项的查找表。
fn mth_sin(value: f64) -> f32 {
    let index = ((value * SIN_SCALE) as i64) & SIN_MASK;
    sin_table()[index as usize]
}

fn mth_cos(value: f64) -> f32 {
    // 官方先加 16384.0（double）再 d2i，负数时和「先截断再加」差一位。
    let index = ((value * SIN_SCALE + 16_384.0) as i64) & SIN_MASK;
    sin_table()[index as usize]
}

/// `generateCarvers` 用到的全部配置，按种子无关，可以放进 `Generator`。
#[derive(Debug)]
pub struct Carvers {
    configs: HashMap<String, Carver>,
    /// 群系注册表 ID → carver ID 列表。
    per_biome: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
enum FloatProvider {
    Constant(f32),
    Uniform { min: f32, max: f32 },
    Trapezoid { min: f32, max: f32, plateau: f32 },
}

impl FloatProvider {
    fn parse(value: Option<&Value>) -> Result<Self, String> {
        let value = value.ok_or("missing float provider")?;
        if let Some(number) = value.as_f64() {
            return Ok(Self::Constant(number as f32));
        }
        let object = value
            .as_object()
            .ok_or("float provider must be an object")?;
        match string(object, "type")? {
            "minecraft:constant" => Ok(Self::Constant(float(object, "value")?)),
            "minecraft:uniform" => Ok(Self::Uniform {
                min: float(object, "min_inclusive")?,
                max: float(object, "max_exclusive")?,
            }),
            "minecraft:trapezoid" => Ok(Self::Trapezoid {
                min: float(object, "min")?,
                max: float(object, "max")?,
                plateau: float(object, "plateau")?,
            }),
            other => Err(format!("unknown float provider {other}")),
        }
    }

    fn sample_tunnel(&self, random: &TunnelRandom) -> f32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => min + random.next_float() * (max - min),
            Self::Trapezoid { min, max, plateau } => {
                let span = max - min;
                let slope = (span - plateau) / 2.0;
                let flat = span - slope;
                min + random.next_float() * flat + random.next_float() * slope
            }
        }
    }

    fn sample(&self, random: &Random) -> f32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => min + random.next_float() * (max - min),
            Self::Trapezoid { min, max, plateau } => {
                let span = max - min;
                let slope = (span - plateau) / 2.0;
                let flat = span - slope;
                min + random.next_float() * flat + random.next_float() * slope
            }
        }
    }
}

#[derive(Debug, Clone)]
enum IntProvider {
    Constant(i32),
    Uniform { min: i32, max: i32 },
    VeryBiasedToBottom { min: i32, max: i32 },
}

impl IntProvider {
    fn parse(value: &Value) -> Result<Self, String> {
        if let Some(number) = value.as_i64() {
            return Ok(Self::Constant(
                i32::try_from(number).map_err(|_| "int out of range")?,
            ));
        }
        let object = value.as_object().ok_or("int provider must be an object")?;
        match string(object, "type")? {
            "minecraft:constant" => Ok(Self::Constant(integer(object, "value")?)),
            "minecraft:uniform" => Ok(Self::Uniform {
                min: integer(object, "min_inclusive")?,
                max: integer(object, "max_inclusive")?,
            }),
            "minecraft:very_biased_to_bottom" => Ok(Self::VeryBiasedToBottom {
                min: integer(object, "min_inclusive")?,
                max: integer(object, "max_inclusive")?,
            }),
            other => Err(format!("unknown int provider {other}")),
        }
    }

    fn sample(&self, random: &Random) -> i32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => min + random.next_int(max - min + 1),
            Self::VeryBiasedToBottom { min, max } => {
                min + random.next_int(random.next_int(random.next_int(max - min + 1) + 1) + 1)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Anchor {
    fn parse(value: &Value) -> Result<Self, String> {
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

#[derive(Debug, Clone)]
enum HeightProvider {
    Constant(i32),
    Uniform { min: Anchor, max: Anchor },
}

impl HeightProvider {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("height provider must be an object")?;
        match string(object, "type")? {
            "minecraft:constant" => Ok(Self::Constant(
                Anchor::parse(object.get("value").ok_or("missing value")?)?.resolve(0, 0),
            )),
            "minecraft:uniform" => Ok(Self::Uniform {
                min: Anchor::parse(object.get("min_inclusive").ok_or("missing min_inclusive")?)?,
                max: Anchor::parse(object.get("max_inclusive").ok_or("missing max_inclusive")?)?,
            }),
            other => Err(format!("unknown height provider {other}")),
        }
    }

    fn sample(&self, random: &Random, min_y: i32, height: i32) -> i32 {
        match *self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => {
                let low = min.resolve(min_y, height);
                let high = max.resolve(min_y, height);
                if low > high {
                    low
                } else {
                    low + random.next_int(high - low + 1)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CanyonShape {
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
    y_scale: FloatProvider,
}

#[derive(Debug, Clone)]
enum Carver {
    Cave {
        probability: f32,
        y: HeightProvider,
        count: IntProvider,
        thickness: FloatProvider,
        weird_thickness_bias: bool,
        room_vertical_radius_multiplier: FloatProvider,
        horizontal_radius_multiplier: FloatProvider,
        vertical_radius_multiplier: FloatProvider,
        start_vertical_radius_multiplier: FloatProvider,
        floor_level: FloatProvider,
    },
    Canyon {
        probability: f32,
        y: HeightProvider,
        vertical_rotation: FloatProvider,
        shape: CanyonShape,
    },
}

impl Carver {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("carver must be an object")?;
        let probability = float(object, "probability")?;
        let y = HeightProvider::parse(object.get("y").ok_or("missing y")?)?;
        match string(object, "type")? {
            "minecraft:cave" => Ok(Self::Cave {
                probability,
                y,
                count: IntProvider::parse(object.get("count").ok_or("missing count")?)?,
                thickness: FloatProvider::parse(object.get("thickness"))?,
                weird_thickness_bias: object
                    .get("weird_thickness_bias")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                room_vertical_radius_multiplier: FloatProvider::parse(
                    object.get("room_vertical_radius_multiplier"),
                )?,
                horizontal_radius_multiplier: FloatProvider::parse(
                    object.get("horizontal_radius_multiplier"),
                )?,
                vertical_radius_multiplier: FloatProvider::parse(
                    object.get("vertical_radius_multiplier"),
                )?,
                start_vertical_radius_multiplier: match object
                    .get("start_vertical_radius_multiplier")
                {
                    Some(value) => FloatProvider::parse(Some(value))?,
                    None => FloatProvider::Constant(1.0),
                },
                floor_level: FloatProvider::parse(object.get("floor_level"))?,
            }),
            "minecraft:canyon" => {
                let shape = object
                    .get("shape")
                    .and_then(Value::as_object)
                    .ok_or("missing shape")?;
                Ok(Self::Canyon {
                    probability,
                    y,
                    vertical_rotation: FloatProvider::parse(object.get("vertical_rotation"))?,
                    shape: CanyonShape {
                        distance_factor: FloatProvider::parse(shape.get("distance_factor"))?,
                        thickness: FloatProvider::parse(shape.get("thickness"))?,
                        width_smoothness: integer(shape, "width_smoothness")?,
                        horizontal_radius_factor: FloatProvider::parse(
                            shape.get("horizontal_radius_factor"),
                        )?,
                        vertical_radius_default_factor: float(
                            shape,
                            "vertical_radius_default_factor",
                        )?,
                        vertical_radius_center_factor: float(
                            shape,
                            "vertical_radius_center_factor",
                        )?,
                        y_scale: FloatProvider::parse(shape.get("y_scale"))?,
                    },
                })
            }
            other => Err(format!("unknown carver type {other}")),
        }
    }

    fn probability(&self) -> f32 {
        match self {
            Self::Cave { probability, .. } | Self::Canyon { probability, .. } => *probability,
        }
    }
}

/// `CarvingMask`：位序 `(y - minY) + (z + x*16) * height`。
#[derive(Debug)]
struct CarvingMask {
    min_y: i32,
    height: i32,
    bits: Vec<u64>,
}

impl CarvingMask {
    fn new(min_y: i32, max_y: i32) -> Self {
        let height = max_y - min_y + 1;
        let bits = ((256 * height + 63) / 64) as usize;
        Self {
            min_y,
            height,
            bits: vec![0; bits.max(1)],
        }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let column = (z.wrapping_add(x.wrapping_shl(4))) as i64;
        let row = (y.wrapping_sub(self.min_y)) as i64;
        (row + column * i64::from(self.height)) as usize
    }

    fn carve(&mut self, x: i32, y: i32, z: i32) {
        let index = self.index(x, y, z);
        let word = index / 64;
        if word < self.bits.len() {
            self.bits[word] |= 1_u64 << (index % 64);
        }
    }

    fn is_empty(&self) -> bool {
        self.bits.iter().all(|word| *word == 0)
    }

    /// `CarvingMask.visit`：按位序升序遍历 `(x, z, yFrom, yTo)` 段。
    fn visit(&self, visitor: &mut impl FnMut(i32, i32, i32, i32)) {
        let bits = self.bits.len() * 64;
        let mut at = 0;
        while at < bits {
            if self.bits[at / 64] & (1_u64 << (at % 64)) == 0 {
                at += 1;
                continue;
            }
            let start = at;
            while at < bits && self.bits[at / 64] & (1_u64 << (at % 64)) != 0 {
                at += 1;
            }
            let end = at - 1;
            let height = i64::from(self.height);
            let first_column = (start as i64) / height;
            let last_column = (end as i64) / height;
            for column in first_column..=last_column {
                let x = ((column >> 4) & 15) as i32;
                let z = (column & 15) as i32;
                let base = column * height;
                let low = (start as i64 - base).max(0) + i64::from(self.min_y);
                let high = (end as i64 - base).min(height - 1) + i64::from(self.min_y);
                visitor(x, z, low as i32, high as i32);
            }
        }
    }
}

/// `SingleThreadedRandomSource` 的 `setSeed` 和 `LegacyRandomSource` 一样先异或
/// `MULTIPLIER`，所以取数序列完全一致，直接复用 [`LegacyRandom`]。
#[derive(Debug, Clone)]
struct TunnelRandom {
    inner: LegacyRandom,
}

impl TunnelRandom {
    fn new(seed: i64) -> Self {
        Self {
            inner: LegacyRandom::new(seed),
        }
    }

    fn next_float(&self) -> f32 {
        self.inner.next_float()
    }

    fn next_int(&self, bound: i32) -> i32 {
        self.inner.next_int(bound)
    }

    fn next_long(&self) -> i64 {
        self.inner.next_long()
    }
}

impl Carvers {
    /// 从 `worldgen.json` 的 `carver` 和 `biome_carvers` 解析。
    pub fn compile(root: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let configs = root
            .get("carver")
            .and_then(Value::as_object)
            .ok_or("missing worldgen field carver")?;
        let biomes = root
            .get("biome_carvers")
            .and_then(Value::as_object)
            .ok_or("missing worldgen field biome_carvers")?;
        let mut parsed = HashMap::with_capacity(configs.len());
        for (id, value) in configs {
            parsed.insert(id.clone(), Carver::parse(value)?);
        }
        let names = mc_bamboo_data::registries::biome::NAMES;
        let mut per_biome = vec![Vec::new(); names.len()];
        for (name, value) in biomes {
            let id = names
                .iter()
                .position(|entry| *entry == name)
                .ok_or_else(|| format!("unknown biome {name}"))?;
            let list = value
                .as_array()
                .ok_or("biome carvers must be a list")?
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "carver id must be a string".to_owned())
                })
                .collect::<Result<Vec<_>, String>>()?;
            per_biome[id] = list;
        }
        Ok(Self {
            configs: parsed,
            per_biome,
        })
    }

    /// `generateCarvers` 加 `applyCarvingMask`。
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &self,
        layers: &mut [Vec<u32>],
        chunk_x: i32,
        chunk_z: i32,
        settings: &Settings,
        context: &mut Context,
        router: &RouterSamplers,
        biome_manager: &BiomeManager<'_>,
        aquifer: &mut Aquifer<'_>,
        material: &MaterialRules,
        noises: &NoiseRegistry,
        positional: PositionalFactory,
        seed: i64,
    ) -> Result<(), String> {
        let min_y = settings.min_y;
        let height = settings.height;
        let max_y = min_y + height - 1 - 7;
        let mut mask = CarvingMask::new(min_y + 1, max_y);
        // 邻域只在 y=0 这一层问 quart 群系；17×17 个格子，按 quart 坐标缓存气候参数。
        let mut climate = ClimateCache::new(
            java_floor_div(chunk_x.wrapping_mul(16), 4).wrapping_sub(8),
            0,
            java_floor_div(chunk_z.wrapping_mul(16), 4).wrapping_sub(8),
            17 * 4,
            1,
            17 * 4,
        );
        // 初值不影响结果：每个 (邻域, carver) 都会重新 setLargeFeatureSeed。
        let worldgen_random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
        // `generateCarvers` 的邻域是硬编码的 -8..=8，不是 `getRange()` 的 4。
        for offset_x in -8..=8 {
            for offset_z in -8..=8 {
                let neighbor_x = chunk_x + offset_x;
                let neighbor_z = chunk_z + offset_z;
                // `WorldGenRegion` 为 null 走 `getBiomeGenerationSettingsForCarver`。
                let quart_x = java_floor_div(neighbor_x * 16, 4);
                let quart_z = java_floor_div(neighbor_z * 16, 4);
                let biome = biome_manager.noise_biome_cached(
                    router,
                    &mut Context::uncached(),
                    quart_x,
                    0,
                    quart_z,
                    &mut climate,
                );

                let list = self
                    .per_biome
                    .get(biome as usize)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                for (index, id) in list.iter().enumerate() {
                    let carver = self
                        .configs
                        .get(id)
                        .ok_or_else(|| format!("unknown carver {id}"))?;
                    worldgen_random.set_large_feature_seed(
                        seed.wrapping_add(index as i64),
                        neighbor_x,
                        neighbor_z,
                    );
                    let random = worldgen_random.source().clone();
                    let start_value = random.next_float();
                    if start_value > carver.probability() {
                        continue;
                    }
                    self.carve(
                        carver, &mut mask, chunk_x, chunk_z, neighbor_x, neighbor_z, &random,
                        settings,
                    );
                }
            }
        }
        if mask.is_empty() {
            return Ok(());
        }
        self.apply_mask(
            &mask,
            layers,
            chunk_x,
            chunk_z,
            settings,
            context,
            router,
            biome_manager,
            aquifer,
            material,
            noises,
            positional,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn carve(
        &self,
        carver: &Carver,
        mask: &mut CarvingMask,
        chunk_x: i32,
        chunk_z: i32,
        neighbor_x: i32,
        neighbor_z: i32,
        random: &Random,
        settings: &Settings,
    ) {
        match carver {
            Carver::Cave {
                y,
                count,
                thickness,
                weird_thickness_bias,
                room_vertical_radius_multiplier,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                start_vertical_radius_multiplier,
                floor_level,
                ..
            } => {
                let span = (RANGE * 2 - 1) * 16;
                let count = count.sample(random);
                for _ in 0..count {
                    let x = f64::from(neighbor_x * 16 + random.next_int(16));
                    let y = f64::from(y.sample(random, settings.min_y, settings.height));
                    let z = f64::from(neighbor_z * 16 + random.next_int(16));

                    let horizontal = f64::from(horizontal_radius_multiplier.sample(random));
                    let vertical = f64::from(vertical_radius_multiplier.sample(random));
                    let start_vertical = f64::from(start_vertical_radius_multiplier.sample(random));
                    let floor = f64::from(floor_level.sample(random));
                    let mut tunnels = 1;
                    if random.next_int(4) == 0 {
                        let room_vertical =
                            f64::from(room_vertical_radius_multiplier.sample(random));
                        let radius = 1.0_f32 + random.next_float() * 6.0;
                        create_room(
                            mask,
                            chunk_x,
                            chunk_z,
                            x,
                            y,
                            z,
                            radius,
                            room_vertical,
                            floor,
                        );
                        tunnels += random.next_int(4);
                    }
                    for _ in 0..tunnels {
                        let yaw = random.next_float() * std::f32::consts::TAU;
                        let pitch = (random.next_float() - 0.5) / 4.0;
                        let thickness = get_thickness(thickness, *weird_thickness_bias, random);
                        let length = span - random.next_int(span / 4);
                        create_tunnel(
                            mask,
                            chunk_x,
                            chunk_z,
                            TunnelRandom::new(random.next_long()),
                            x,
                            y,
                            z,
                            horizontal,
                            vertical,
                            thickness,
                            yaw,
                            pitch,
                            0,
                            length,
                            start_vertical,
                            floor,
                        );
                    }
                }
            }
            Carver::Canyon {
                y,
                vertical_rotation,
                shape,
                ..
            } => {
                let span = (RANGE * 2 - 1) * 16;
                let x = f64::from(neighbor_x * 16 + random.next_int(16));
                let y = y.sample(random, settings.min_y, settings.height);
                let z = f64::from(neighbor_z * 16 + random.next_int(16));
                let yaw = random.next_float() * std::f32::consts::TAU;
                let pitch = vertical_rotation.sample(random);
                let y_scale = f64::from(shape.y_scale.sample(random));
                let thickness = shape.thickness.sample(random);
                let distance = (span as f32 * shape.distance_factor.sample(random)) as i32;
                carve_canyon(
                    mask,
                    chunk_x,
                    chunk_z,
                    TunnelRandom::new(random.next_long()),
                    x,
                    f64::from(y),
                    z,
                    thickness,
                    yaw,
                    pitch,
                    0,
                    distance,
                    y_scale,
                    shape,
                    settings,
                );
            }
        }
    }

    /// `applyCarvingMask`：逐列从高到低换回含水层物质，草/菌丝下方的泥土要补地表。
    #[allow(clippy::too_many_arguments)]
    fn apply_mask(
        &self,
        mask: &CarvingMask,
        layers: &mut [Vec<u32>],
        chunk_x: i32,
        chunk_z: i32,
        settings: &Settings,
        context: &mut Context,
        router: &RouterSamplers,
        biome_manager: &BiomeManager<'_>,
        aquifer: &mut Aquifer<'_>,
        material: &MaterialRules,
        noises: &NoiseRegistry,
        positional: PositionalFactory,
    ) -> Result<(), String> {
        let min_x = chunk_x * 16;
        let min_z = chunk_z * 16;
        let grass = block_state("minecraft:grass_block")?;
        let mycelium = block_state("minecraft:mycelium")?;
        let dirt = block_state("minecraft:dirt")?;
        let uncarvable = block_tag("minecraft:uncarvable").unwrap_or(&[]);
        let mut error = None;
        mask.visit(&mut |x, z, from, to| {
            if error.is_some() {
                return;
            }
            let block_x = min_x + x;
            let block_z = min_z + z;
            let mut surface_flag = false;
            for y in (from..=to).rev() {
                let state = layer_get(layers, settings.min_y, x, y, z);
                let block_id = block_id_of_state(state).unwrap_or(0) as i32;
                if uncarvable.contains(&block_id) {
                    continue;
                }
                if state == grass || state == mycelium {
                    surface_flag = true;
                }
                let Some(new_state) = aquifer.compute_substance(context, block_x, y, block_z, 0.0)
                else {
                    continue;
                };
                layer_set(layers, settings.min_y, x, y, z, new_state);
                if surface_flag && y > settings.min_y {
                    let below = layer_get(layers, settings.min_y, x, y - 1, z);
                    if below == dirt {
                        let gradient_x =
                            crate::material::surface_gradient_x(layers, settings.min_y, x, z);
                        let gradient_z =
                            crate::material::surface_gradient_z(layers, settings.min_y, x, z);
                        // 官方传的是下方方块的位置，`updateY` 用 `pos.getY() + 1`，
                        // 正好等于当前被挖的这一格的 y。
                        let water = if crate::biome_manager::fluid_is_empty(new_state) {
                            i32::MIN
                        } else {
                            y
                        };
                        let top = material.top_material(
                            layers,
                            chunk_x,
                            chunk_z,
                            block_x,
                            y - 1,
                            block_z,
                            gradient_x,
                            gradient_z,
                            water,
                            settings,
                            context,
                            router,
                            biome_manager,
                            noises,
                            positional,
                        );
                        let top = match top {
                            Ok(top) => top,
                            Err(failure) => {
                                error = Some(failure);
                                return;
                            }
                        };
                        if let Some(state) = top {
                            layer_set(layers, settings.min_y, x, y - 1, z, state);
                        }
                    }
                }
            }
        });
        match error {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

fn get_thickness(provider: &FloatProvider, weird_bias: bool, random: &Random) -> f32 {
    let mut value = provider.sample(random);
    if weird_bias && random.next_int(10) == 0 {
        value *= 1.0 + random.next_float() * random.next_float() * 3.0;
    }
    value
}

/// `WorldCarver.carveEllipsoid`。
#[allow(clippy::too_many_arguments)]
fn carve_ellipsoid(
    mask: &mut CarvingMask,
    chunk_x: i32,
    chunk_z: i32,
    x: f64,
    y: f64,
    z: f64,
    horizontal: f64,
    vertical: f64,
    skip: impl Fn(f64, f64, f64, i32) -> bool,
) {
    let middle_x = f64::from(chunk_x * 16 + 8);
    let middle_z = f64::from(chunk_z * 16 + 8);
    let reach = 16.0 + horizontal * 2.0;
    if (x - middle_x).abs() > reach || (z - middle_z).abs() > reach {
        return;
    }
    let min_x = chunk_x * 16;
    let min_z = chunk_z * 16;
    let x0 = floor(x - horizontal) - min_x - 1;
    let x1 = floor(x + horizontal) - min_x;
    let y0 = (floor(y - vertical) - 1).max(mask.min_y);
    let y1 = (floor(y + vertical) + 1).min(mask.min_y + mask.height - 1);
    // 官方 `CarverOutput.minY/maxY` 是 mask 的边界，但 `ChunkPos` 是中心区块，
    // 越界（负的 local x）会被 `Math.max(0, ...)` 折回来；这里保持同样的行为。
    let z0 = floor(z - horizontal) - min_z - 1;
    let z1 = floor(z + horizontal) - min_z;
    let from_x = x0.max(0);
    let to_x = x1.min(15);
    let from_z = z0.max(0);
    let to_z = z1.min(15);
    for local_x in from_x..=to_x {
        let dx = (f64::from(min_x + local_x) + 0.5 - x) / horizontal;
        for local_z in from_z..=to_z {
            let dz = (f64::from(min_z + local_z) + 0.5 - z) / horizontal;
            if dx * dx + dz * dz >= 1.0 {
                continue;
            }
            // 官方是 `for (y = y1; y > y0; y--)`，下界是开的。
            for block_y in (y0 + 1..=y1).rev() {
                let dy = (f64::from(block_y) - 0.5 - y) / vertical;
                if !skip(dx, dy, dz, block_y) {
                    mask.carve(local_x, block_y, local_z);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_room(
    mask: &mut CarvingMask,
    chunk_x: i32,
    chunk_z: i32,
    x: f64,
    y: f64,
    z: f64,
    radius: f32,
    vertical_multiplier: f64,
    floor: f64,
) {
    let scale = 1.5 + f64::from(mth_sin(1.570_796_370_506_286_6) * radius);
    carve_ellipsoid(
        mask,
        chunk_x,
        chunk_z,
        x + 1.0,
        y,
        z,
        scale,
        scale * vertical_multiplier,
        |dx, dy, dz, _| dy <= floor || dx * dx + dy * dy + dz * dz >= 1.0,
    );
}

/// `CaveWorldCarver.createTunnel`。
#[allow(clippy::too_many_arguments)]
fn create_tunnel(
    mask: &mut CarvingMask,
    chunk_x: i32,
    chunk_z: i32,
    random: TunnelRandom,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    horizontal: f64,
    vertical: f64,
    thickness: f32,
    mut yaw: f32,
    mut pitch: f32,
    start: i32,
    length: i32,
    start_vertical: f64,
    floor: f64,
) {
    let branch_at = random.next_int(length / 2) + length / 4;
    let thin = random.next_int(6) == 0;
    let mut yaw_delta = 0.0_f32;
    let mut pitch_delta = 0.0_f32;
    for step in start..length {
        let scale = 1.5
            + f64::from(mth_sin(f64::from(
                std::f32::consts::PI * step as f32 / length as f32,
            ))) * f64::from(thickness);
        let vertical_scale = scale * start_vertical;
        let cos_pitch = mth_cos(f64::from(pitch));
        x += f64::from(mth_cos(f64::from(yaw)) * cos_pitch);
        y += f64::from(mth_sin(f64::from(pitch)));
        z += f64::from(mth_sin(f64::from(yaw)) * cos_pitch);
        pitch *= if thin { 0.92 } else { 0.7 };
        pitch += pitch_delta * 0.1;
        yaw += yaw_delta * 0.1;
        pitch_delta *= 0.9;
        yaw_delta *= 0.75;
        pitch_delta += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
        yaw_delta += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
        if step == branch_at && thickness > 1.0 {
            // 官方每条分支先取 nextLong 当种子，再取 nextFloat 当厚度；顺序不能换。
            let first_seed = random.next_long();
            let first_thickness = random.next_float() * 0.5 + 0.5;
            create_tunnel(
                mask,
                chunk_x,
                chunk_z,
                TunnelRandom::new(first_seed),
                x,
                y,
                z,
                horizontal,
                vertical,
                first_thickness,
                yaw - std::f32::consts::FRAC_PI_2,
                pitch / 3.0,
                step,
                length,
                1.0,
                floor,
            );
            let second_seed = random.next_long();
            let second_thickness = random.next_float() * 0.5 + 0.5;
            create_tunnel(
                mask,
                chunk_x,
                chunk_z,
                TunnelRandom::new(second_seed),
                x,
                y,
                z,
                horizontal,
                vertical,
                second_thickness,
                yaw + std::f32::consts::FRAC_PI_2,
                pitch / 3.0,
                step,
                length,
                1.0,
                floor,
            );
            return;
        }
        if random.next_int(4) != 0 {
            if !can_reach(chunk_x, chunk_z, x, z, step, length, thickness) {
                return;
            }
            carve_ellipsoid(
                mask,
                chunk_x,
                chunk_z,
                x,
                y,
                z,
                scale * horizontal,
                vertical_scale * vertical,
                |dx, dy, dz, _| dy <= floor || dx * dx + dy * dy + dz * dz >= 1.0,
            );
        }
    }
}

/// `WorldCarver.canReach`。
fn can_reach(
    chunk_x: i32,
    chunk_z: i32,
    x: f64,
    z: f64,
    step: i32,
    length: i32,
    thickness: f32,
) -> bool {
    let dx = x - f64::from(chunk_x * 16 + 8);
    let dz = z - f64::from(chunk_z * 16 + 8);
    let dy = f64::from(length - step);
    let reach = f64::from((thickness + 2.0) + 16.0);
    dx * dx + dz * dz - dy * dy <= reach * reach
}

/// `CanyonWorldCarver.doCarve`。
#[allow(clippy::too_many_arguments)]
fn carve_canyon(
    mask: &mut CarvingMask,
    chunk_x: i32,
    chunk_z: i32,
    random: TunnelRandom,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    thickness: f32,
    mut yaw: f32,
    mut pitch: f32,
    start: i32,
    length: i32,
    y_scale: f64,
    shape: &CanyonShape,
    settings: &Settings,
) {
    let width_factors = init_width_factors(&random, shape.width_smoothness, settings.height);
    let mut yaw_delta = 0.0_f32;
    let mut pitch_delta = 0.0_f32;
    for step in start..length {
        let scale = 1.5
            + f64::from(mth_sin(f64::from(
                std::f32::consts::PI * step as f32 / length as f32,
            ))) * f64::from(thickness);
        let mut vertical_scale = scale * y_scale;
        let horizontal_scale =
            scale * f64::from(shape.horizontal_radius_factor.sample_tunnel(&random));
        vertical_scale =
            update_vertical_radius(&random, vertical_scale, step as f32, length as f32, shape);
        let cos_pitch = mth_cos(f64::from(pitch));
        let sin_pitch = mth_sin(f64::from(pitch));
        x += f64::from(mth_cos(f64::from(yaw)) * cos_pitch);
        y += f64::from(sin_pitch);
        z += f64::from(mth_sin(f64::from(yaw)) * cos_pitch);
        pitch *= 0.7;
        pitch += pitch_delta * 0.05;
        yaw += yaw_delta * 0.05;
        pitch_delta *= 0.8;
        yaw_delta *= 0.5;
        pitch_delta += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
        yaw_delta += (random.next_float() - random.next_float()) * random.next_float() * 4.0;
        if random.next_int(4) != 0 {
            if !can_reach(chunk_x, chunk_z, x, z, step, length, thickness) {
                return;
            }
            carve_ellipsoid(
                mask,
                chunk_x,
                chunk_z,
                x,
                y,
                z,
                horizontal_scale,
                vertical_scale,
                |dx, dy, dz, block_y| {
                    let index = (block_y - settings.min_y - 1).max(0) as usize;
                    let factor = width_factors.get(index).copied().unwrap_or(1.0);
                    (dx * dx + dz * dz) * f64::from(factor) + dy * dy / 6.0 >= 1.0
                },
            );
        }
    }
}

fn init_width_factors(random: &TunnelRandom, smoothness: i32, height: i32) -> Vec<f32> {
    let mut factors = vec![0.0_f32; height.max(0) as usize];
    let mut current = 1.0_f32;
    for (index, slot) in factors.iter_mut().enumerate() {
        if index == 0 || random.next_int(smoothness) == 0 {
            current = 1.0 + random.next_float() * random.next_float();
        }
        *slot = current * current;
    }
    factors
}

fn update_vertical_radius(
    random: &TunnelRandom,
    radius: f64,
    step: f32,
    length: f32,
    shape: &CanyonShape,
) -> f64 {
    let center = 1.0 - (0.5 - step / length).abs() * 2.0;
    let factor =
        shape.vertical_radius_default_factor + shape.vertical_radius_center_factor * center;
    radius * f64::from(factor) * f64::from(0.75 + random.next_float() * 0.25)
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

fn string<'a>(object: &'a serde_json::Map<String, Value>, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing field {name}"))
}

fn float(object: &serde_json::Map<String, Value>, name: &str) -> Result<f32, String> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| format!("missing field {name}"))
}

fn integer(object: &serde_json::Map<String, Value>, name: &str) -> Result<i32, String> {
    object
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("missing field {name}"))
}
