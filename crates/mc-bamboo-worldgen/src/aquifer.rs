//! 官方 `Aquifer.NoiseBasedAquifer`：把密度值换成石头、水、岩浆或空气。
//!
//! 26.3 的含水层配置在 `noise_settings` 的 `aquifers` 字段里，六条密度函数都由
//! `RandomState` 的同一个编译器编译。`computeSubstance` 只在点路径上求值这些函数，
//! 唯一的成批求值是构造器里 `maxSurfaceLevel` 对 `surfaceLevel` 的一次扫描；
//! 那次扫描会留在 `cache` 的记忆里，所以顺序不能省。
//!
//! 只实现 `NoiseBasedChunkGenerator.doFill` 会用得到的部分：方块状态用默认状态 ID，
//! `shouldScheduleFluidUpdate` 影响的是后期处理的位置表，对拍不比它。

use std::collections::HashMap;

use crate::{
    math::{floor_f32, java_floor_div},
    sampler::Context,
    volume::Volume,
};

/// 官方 `BlockPos.asLong` 打包的坐标，用来当 `aquiferLocationCache` 的哨兵和值。
const LOCATION_EMPTY: i64 = i64::MAX;
/// `DimensionType.WAY_BELOW_MIN_Y`。
const WAY_BELOW_MIN_Y: i32 = -32512;
/// `FluidStatus` 里 `at()` 的三种流体：默认流体（水）、岩浆、空气。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fluid {
    Water,
    Lava,
}

/// `Aquifer.FluidStatus`：一个液面高度和液面以下的流体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FluidStatus {
    level: i32,
    fluid: Fluid,
    water: u32,
    lava: u32,
    air: u32,
}

impl FluidStatus {
    fn at(self, y: i32) -> u32 {
        if y < self.level {
            match self.fluid {
                Fluid::Water => self.water,
                Fluid::Lava => self.lava,
            }
        } else {
            self.air
        }
    }
}

/// `SurfaceSamplingOffsets` 的 13 个二维偏移，顺序和官方静态初始化一样。
const SURFACE_OFFSETS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// `Aquifer.NoiseBasedAquifer` 的区块级状态。
pub struct Aquifer<'a> {
    functions: &'a super::chunk::AquiferFunctions,
    /// `RandomState.getOrCreateRandomFactory("minecraft:aquifer")`。
    positional: crate::random::PositionalFactory,
    sea_level: i32,
    default_water: u32,
    default_lava: u32,
    air: u32,
    /// 网格范围，从区块体积扩出来 5 个方块和 12 个 y。
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,
    /// 计算过的格子：`aquiferCache` 和 `aquiferLocationCache`。
    status: Vec<Option<FluidStatus>>,
    locations: Vec<i64>,
    /// `surfaceLevel` 的 (x, z) → 高度，按 quart 格缓存。
    surface_levels: HashMap<i64, i32>,
    /// 高于这个 y 就不采样含水层，直接用全局流体规则。
    skip_sampling_above_y: i32,
    should_schedule_fluid_update: bool,
}

/// `Aquifer.Config` 编译出来的六条函数。`chunk` 模块解析官方 JSON 后交给这里。
impl<'a> Aquifer<'a> {
    /// `new NoiseBasedAquifer(...)`：构造器里会成批求一次 `surfaceLevel`。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        functions: &'a super::chunk::AquiferFunctions,
        context: &mut Context,
        positional: crate::random::PositionalFactory,
        volume: &Volume,
        sea_level: i32,
        default_water: u32,
        default_lava: u32,
        air: u32,
    ) -> Self {
        let min_grid_x = grid_x(volume.min_x - 5);
        let min_grid_z = grid_z(volume.min_z - 5);
        let min_grid_y = grid_y(volume.min_y + 1) - 1;
        let max_grid_x = grid_x(volume.max_x() - 5) + 1;
        let max_grid_z = grid_z(volume.max_z() - 5) + 1;
        let max_grid_y = grid_y(volume.max_y() + 1) + 1;
        let grid_size_x = max_grid_x - min_grid_x + 1;
        let grid_size_z = max_grid_z - min_grid_z + 1;
        let grid_size_y = max_grid_y - min_grid_y + 1;
        let cells = (grid_size_x * grid_size_y * grid_size_z).max(0) as usize;
        let mut aquifer = Self {
            functions,
            positional,
            sea_level,
            default_water,
            default_lava,
            air,
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            status: vec![None; cells],
            locations: vec![LOCATION_EMPTY; cells],
            surface_levels: HashMap::new(),
            skip_sampling_above_y: 0,
            should_schedule_fluid_update: false,
        };
        // maxSurfaceLevel 覆盖从 minGridX 的第一个方块到 maxGridX 的第十个方块。
        let from_x = from_grid_x(min_grid_x, 0);
        let from_z = from_grid_z(min_grid_z, 0);
        let to_x = from_grid_x(max_grid_x, 9);
        let to_z = from_grid_z(max_grid_z, 9);
        let maximum = aquifer.max_surface_level(context, from_x, to_x, from_z, to_z);
        let skip_grid_y = grid_y(maximum + 8 + 12) + 1;
        aquifer.skip_sampling_above_y = from_grid_y(skip_grid_y, 11) - 1;
        aquifer
    }

    /// `computeSubstance(x, y, z, density)`：`null` 表示用维度的默认方块（石头）。
    pub fn compute_substance(
        &mut self,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        density: f64,
    ) -> Option<u32> {
        if density > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }
        let global = self.global_fluid(y);
        if y > self.skip_sampling_above_y {
            self.should_schedule_fluid_update = false;
            return Some(global.at(y));
        }
        if global.fluid == Fluid::Lava {
            self.should_schedule_fluid_update = false;
            return Some(self.default_lava);
        }

        let grid_x = grid_x(x - 5);
        let grid_y = grid_y(y + 1);
        let grid_z = grid_z(z - 5);
        // 四个最近的格子，按到 (x,y,z) 的平方距离排序；等距时按官方的插入顺序。
        let mut distances = [i32::MAX; 4];
        let mut indices = [0usize; 4];
        const NO_STATUS: [FluidStatus; 4] = [FluidStatus {
            level: 0,
            fluid: Fluid::Water,
            water: 0,
            lava: 0,
            air: 0,
        }; 4];
        let mut fluids = NO_STATUS;
        // 官方是 x: 0..1、y: -1..1、z: 0..1 一共 12 个候选格子，不是 27 个。
        for dx in 0..=1 {
            for dy in -1..=1 {
                for dz in 0..=1 {
                    let (cx, cy, cz) = (grid_x + dx, grid_y + dy, grid_z + dz);
                    let index = self.index(cx, cy, cz);
                    let location = self.locations[index];
                    let (px, py, pz) = if location == LOCATION_EMPTY {
                        let random = self.positional.at(cx, cy, cz);
                        let px = from_grid_x(cx, random.next_int(10));
                        let py = from_grid_y(cy, random.next_int(9));
                        let pz = from_grid_z(cz, random.next_int(10));
                        let location = crate::sampler::block_pos_as_long(px, py, pz);
                        self.locations[index] = location;
                        (px, py, pz)
                    } else {
                        (
                            block_pos_x(location),
                            block_pos_y(location),
                            block_pos_z(location),
                        )
                    };
                    let (ox, oy, oz) = (px - x, py - y, pz - z);
                    let distance = ox * ox + oy * oy + oz * oz;
                    if distance <= distances[0] {
                        shift4(
                            &mut indices,
                            &mut distances,
                            &mut fluids,
                            0,
                            index,
                            distance,
                        );
                    } else if distance <= distances[1] {
                        shift4(
                            &mut indices,
                            &mut distances,
                            &mut fluids,
                            1,
                            index,
                            distance,
                        );
                    } else if distance <= distances[2] {
                        shift4(
                            &mut indices,
                            &mut distances,
                            &mut fluids,
                            2,
                            index,
                            distance,
                        );
                    } else if distance <= distances[3] {
                        indices[3] = index;
                        distances[3] = distance;
                    }
                }
            }
        }
        let first = self.status(context, indices[0]);
        let nearest = similarity(distances[0], distances[1]);
        let fluid = first.at(y);
        self.should_schedule_fluid_update = if nearest <= 0.0 {
            if nearest >= flowing_update_similarity() {
                self.status(context, indices[1]) != first
            } else {
                false
            }
        } else {
            let mut barrier = f64::NAN;
            let second = self.status(context, indices[1]);
            let raw_pressure =
                self.calculate_pressure(context, x, y, z, &mut barrier, first, second);
            let pressure = nearest * raw_pressure;
            if density + pressure > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
            let third = self.status(context, indices[2]);
            let second_similarity = similarity(distances[0], distances[2]);
            if second_similarity > 0.0 {
                let extra = nearest
                    * second_similarity
                    * self.calculate_pressure(context, x, y, z, &mut barrier, first, third);
                if density + extra > 0.0 {
                    self.should_schedule_fluid_update = false;
                    return None;
                }
            }
            let third_similarity = similarity(distances[1], distances[2]);
            if third_similarity > 0.0 {
                let extra = nearest
                    * third_similarity
                    * self.calculate_pressure(context, x, y, z, &mut barrier, second, third);
                if density + extra > 0.0 {
                    self.should_schedule_fluid_update = false;
                    return None;
                }
            }
            let changed = first != second
                || (second_similarity >= flowing_update_similarity() && second != third)
                || (third_similarity >= flowing_update_similarity() && first != third);
            if changed {
                true
            } else if third_similarity >= flowing_update_similarity()
                && similarity(distances[0], distances[3]) >= flowing_update_similarity()
            {
                let fourth = self.status(context, indices[3]);
                first != fourth
            } else {
                false
            }
        };
        Some(fluid)
    }

    pub fn should_schedule_fluid_update(&self) -> bool {
        self.should_schedule_fluid_update
    }

    fn global_fluid(&self, y: i32) -> FluidStatus {
        if y < (-54).min(self.sea_level) {
            FluidStatus::lava(-54, self.default_water, self.default_lava, self.air)
        } else {
            FluidStatus::water(
                self.sea_level,
                self.default_water,
                self.default_lava,
                self.air,
            )
        }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((y - self.min_grid_y) * self.grid_size_z + (z - self.min_grid_z))
            .wrapping_mul(self.grid_size_x)
            .wrapping_add(x - self.min_grid_x) as usize
    }

    /// `getAquiferStatus(index)`：格子算过就直接读。
    fn status(&mut self, context: &mut Context, index: usize) -> FluidStatus {
        if let Some(status) = self.status[index] {
            return status;
        }
        let location = self.locations[index];
        let status = self.compute_fluid(
            context,
            block_pos_x(location),
            block_pos_y(location),
            block_pos_z(location),
        );
        self.status[index] = Some(status);
        status
    }

    /// `maxSurfaceLevel`：在 quart 网格上成批求一次 `surfaceLevel`，顺便填缓存。
    fn max_surface_level(
        &mut self,
        context: &mut Context,
        min_x: i32,
        max_x: i32,
        min_z: i32,
        max_z: i32,
    ) -> i32 {
        let quart_x = java_floor_div(min_x, 4);
        let quart_z = java_floor_div(min_z, 4);
        let end_x = java_floor_div(max_x, 4);
        let end_z = java_floor_div(max_z, 4);
        let volume = Volume::new(
            (end_x - quart_x + 1, 1, end_z - quart_z + 1),
            (quart_to_block(quart_x), 0, quart_to_block(quart_z)),
            (4, 1, 4),
        );
        let mut buffer = context.acquire(volume.len());
        self.functions
            .surface_level
            .sample_volume(context, &mut buffer, &volume);
        let mut maximum = i32::MIN;
        for z in 0..volume.size_z {
            for x in 0..volume.size_x {
                let level = floor_f32(buffer[volume.index(x, 0, z)]);
                self.surface_levels
                    .insert(chunk_pos_pack(volume.block_x(x), volume.block_z(z)), level);
                maximum = maximum.max(level);
            }
        }
        context.release(buffer);
        maximum
    }

    /// `surfaceLevel(x, z)`：按 quart 格缓存，用 `sampleValue(x, 0, z)` 算。
    fn surface_level(&mut self, context: &mut Context, x: i32, z: i32) -> i32 {
        let quart_x = quart_to_block(java_floor_div(x, 4));
        let quart_z = quart_to_block(java_floor_div(z, 4));
        let key = chunk_pos_pack(quart_x, quart_z);
        if let Some(level) = self.surface_levels.get(&key) {
            return *level;
        }
        let sampler = &self.functions.surface_level;
        let level = floor_f32(sampler.sample_value(context, quart_x, 0, quart_z));
        self.surface_levels.insert(key, level);
        level
    }

    /// `computeFluid(x, y, z)`：扫 13 个邻近区块的地面高度，算出这一个格子的液面。
    fn compute_fluid(&mut self, context: &mut Context, x: i32, y: i32, z: i32) -> FluidStatus {
        let global = self.global_fluid(y);
        let mut minimum = i32::MAX;
        let above = y + 12;
        let below = y - 12;
        let mut flooded = false;
        for [dx, dz] in SURFACE_OFFSETS {
            let sample_x = x + section_to_block(dx);
            let sample_z = z + section_to_block(dz);
            let raw = self.surface_level(context, sample_x, sample_z);
            let adjusted = raw + 8;
            let center = dx == 0 && dz == 0;
            if center && below > adjusted {
                return global;
            }
            let below_level = above > adjusted;
            if below_level || center {
                let neighbour = self.global_fluid(adjusted);
                if !is_air(neighbour.at(adjusted)) {
                    if center {
                        flooded = true;
                    }
                    if below_level {
                        return neighbour;
                    }
                }
            }
            minimum = minimum.min(raw);
        }
        let level = self.compute_surface_level(context, x, y, z, global, minimum, flooded);
        let fluid = self.compute_fluid_type(context, x, y, z, global, level);
        FluidStatus {
            level,
            fluid,
            water: global.water,
            lava: global.lava,
            air: global.air,
        }
    }

    /// `computeSurfaceLevel(x, y, z, fluidStatus, minSurfaceLevel, flooded)`。
    #[allow(clippy::too_many_arguments)]
    fn compute_surface_level(
        &mut self,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        global: FluidStatus,
        minimum: i32,
        flooded: bool,
    ) -> i32 {
        let exclusion = &self.functions.exclusion;
        let (random_level, global_level) =
            if f64::from(exclusion.sample_value(context, x, y, z)) > 0.0 {
                (-1.0, -1.0)
            } else {
                let delta = minimum + 8 - y;
                let spread = if flooded {
                    clamped_map(f64::from(delta), 0.0, 64.0, 1.0, 0.0)
                } else {
                    0.0
                };
                let floodedness = java_clamp(
                    f64::from(
                        self.functions
                            .fluid_level_floodedness
                            .sample_value(context, x, y, z),
                    ),
                    -1.0,
                    1.0,
                );
                let lower = mapped(spread, 1.0, 0.0, -0.8, 0.4);
                let upper = mapped(spread, 1.0, 0.0, -0.3, 0.8);
                (floodedness - lower, floodedness - upper)
            };
        if global_level > 0.0 {
            global.level
        } else if random_level > 0.0 {
            self.compute_randomized_fluid_surface_level(context, x, y, z, minimum)
        } else {
            WAY_BELOW_MIN_Y
        }
    }

    /// `computeRandomizedFluidSurfaceLevel(x, y, z, fluidLevel)`。
    fn compute_randomized_fluid_surface_level(
        &mut self,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        fluid_level: i32,
    ) -> i32 {
        let grid_x = java_floor_div(x, 16);
        let grid_y = java_floor_div(y, 40);
        let grid_z = java_floor_div(z, 16);
        let base = grid_y * 40 + 20;
        let spread = f64::from(
            self.functions
                .fluid_level_spread
                .sample_value(context, grid_x, grid_y, grid_z)
                * 10.0,
        );
        let quantized = java_floor(spread / 3.0) * 3;
        fluid_level.min(base + quantized)
    }

    /// `computeFluidType(x, y, z, fluidStatus, level)`。
    fn compute_fluid_type(
        &mut self,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        global: FluidStatus,
        level: i32,
    ) -> Fluid {
        if level <= -10 && level != WAY_BELOW_MIN_Y && global.fluid != Fluid::Lava {
            let grid_x = java_floor_div(x, 64);
            let grid_y = java_floor_div(y, 40);
            let grid_z = java_floor_div(z, 64);
            let lava = f64::from(
                self.functions
                    .lava
                    .sample_value(context, grid_x, grid_y, grid_z),
            );
            if lava.abs() > 0.3 {
                return Fluid::Lava;
            }
        }
        global.fluid
    }

    /// `calculatePressure(x, y, z, barrierNoise, first, second)`。
    #[allow(clippy::too_many_arguments)]
    fn calculate_pressure(
        &mut self,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        barrier: &mut f64,
        first: FluidStatus,
        second: FluidStatus,
    ) -> f64 {
        let first_fluid = first.at(y);
        let second_fluid = second.at(y);
        if (first_fluid == self.default_lava && second_fluid == self.default_water)
            || (first_fluid == self.default_water && second_fluid == self.default_lava)
        {
            return 2.0;
        }
        let difference = (first.level - second.level).abs();
        if difference == 0 {
            return 0.0;
        }
        let midpoint = 0.5 * f64::from(first.level + second.level);
        let relative = f64::from(y) + 0.5 - midpoint;
        let half = f64::from(difference) / 2.0;
        let distance = half - relative.abs();
        let scaled = if relative > 0.0 {
            let value = distance;
            if value > 0.0 {
                value / 1.5
            } else {
                value / 2.5
            }
        } else {
            let value = 3.0 + distance;
            if value > 0.0 {
                value / 3.0
            } else {
                value / 10.0
            }
        };
        // 超出 [-2, 2] 时官方把 barrier 当 0 用，不是把整项压力归零。
        let noise = if !(-2.0..=2.0).contains(&scaled) {
            0.0
        } else if barrier.is_nan() {
            let value = f64::from(self.functions.barrier.sample_value(context, x, y, z));
            *barrier = value;
            value
        } else {
            *barrier
        };
        2.0 * (noise + scaled)
    }
}

impl FluidStatus {
    fn water(level: i32, water: u32, lava: u32, air: u32) -> Self {
        Self {
            level,
            fluid: Fluid::Water,
            water,
            lava,
            air,
        }
    }

    fn lava(level: i32, water: u32, lava: u32, air: u32) -> Self {
        Self {
            level,
            fluid: Fluid::Lava,
            water,
            lava,
            air,
        }
    }
}

/// 官方的 `similarity(a, b) = 1 - (b - a) / 25`。
fn similarity(a: i32, b: i32) -> f64 {
    1.0 - f64::from(b - a) / 25.0
}

/// `FLOWING_UPDATE_SIMULARITY = similarity(square(10), square(12))`。
fn flowing_update_similarity() -> f64 {
    similarity(10 * 10, 12 * 12)
}

/// `Mth.clampedMap(value, from, to, mappedFrom, mappedTo)`。
fn clamped_map(value: f64, from: f64, to: f64, mapped_from: f64, mapped_to: f64) -> f64 {
    let inverse = (value - from) / (to - from);
    if inverse < 0.0 {
        mapped_from
    } else if inverse > 1.0 {
        mapped_to
    } else {
        mapped_from + (mapped_to - mapped_from) * inverse
    }
}

/// `Mth.map(value, from, to, mappedFrom, mappedTo)`。
fn mapped(value: f64, from: f64, to: f64, mapped_from: f64, mapped_to: f64) -> f64 {
    let inverse = (value - from) / (to - from);
    mapped_from + (mapped_to - mapped_from) * inverse
}

/// `Mth.clamp(double, min, max)`：NaN 走 `Math.min`，Rust 的 `clamp` 遇到 NaN 会 panic。
fn java_clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min { min } else { value.min(max) }
}

fn java_floor(value: f64) -> i32 {
    value.floor() as i32
}

fn grid_x(x: i32) -> i32 {
    x >> 4
}

fn grid_z(z: i32) -> i32 {
    z >> 4
}

fn grid_y(y: i32) -> i32 {
    y.div_euclid(12)
}

fn from_grid_x(grid: i32, offset: i32) -> i32 {
    grid * 16 + offset
}

fn from_grid_y(grid: i32, offset: i32) -> i32 {
    grid * 12 + offset
}

fn from_grid_z(grid: i32, offset: i32) -> i32 {
    grid * 16 + offset
}

fn quart_to_block(quart: i32) -> i32 {
    quart << 2
}

fn section_to_block(section: i32) -> i32 {
    section << 4
}

fn chunk_pos_pack(x: i32, z: i32) -> i64 {
    (i64::from(x) & 0xFFFF_FFFF) | ((i64::from(z) & 0xFFFF_FFFF) << 32)
}

/// `BlockPos` 打包坐标的三个 getter。
fn block_pos_x(value: i64) -> i32 {
    (value >> 38) as i32
}

fn block_pos_y(value: i64) -> i32 {
    (value << (64 - 12) >> (64 - 12)) as i32
}

fn block_pos_z(value: i64) -> i32 {
    (value << (64 - 12 - 26) >> (64 - 26)) as i32
}

fn is_air(state: u32) -> bool {
    state == 0
}

/// 把第四个格子往后挤一格；`fluids` 是占位，值到用的时候才从缓存取。
fn shift4(
    indices: &mut [usize; 4],
    distances: &mut [i32; 4],
    fluids: &mut [FluidStatus; 4],
    at: usize,
    index: usize,
    distance: i32,
) {
    for slot in (at + 1..4).rev() {
        indices[slot] = indices[slot - 1];
        distances[slot] = distances[slot - 1];
        fluids[slot] = fluids[slot - 1];
    }
    indices[at] = index;
    distances[at] = distance;
}
