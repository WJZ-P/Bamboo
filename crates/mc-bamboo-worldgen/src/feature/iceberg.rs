//! `IcebergFeature`（冰山）：从海平面起在水面上下堆出一座冰山，再抹平边角、挖洞。
//!
//! 逐段照 `IcebergFeature.place` 及它的私有辅助方法：所有 `nextDouble/nextFloat/
//! nextInt/nextBoolean` 的出现顺序、`Math.pow(x, 2.0)`（JDK 26 里与 `x*x` 逐位相同）、
//! `Mth.ceil(float)` 和 f32/f64 的宽度都不化简。读方块用 `WorldGenLevel.getBlockState`，
//! 写方块一律走 `Feature.setBlock` → `setBlockAndUpdate`（flags 3）→ 高度图增量更新。
//!
//! `IcebergFeature` (iceberg): build a mound above and below sea level from the
//! `IcebergFeature.place` bytecode, then smooth and carve it. Every random draw order,
//! `Math.pow(x, 2.0)` (bit-equal to `x*x` on JDK 26), `Mth.ceil(float)` and f32/f64
//! width are preserved. Writes go through `Feature.setBlock` → `setBlockAndUpdate`
//! (flags 3), so they update heightmaps.

use mc_bamboo_data::{block_of_state, blocks, facts, state_facts};
use serde_json::Value;

use crate::{math::clamp, random::WorldgenRandom};

use super::{state::parse_state, world::FeatureWorld};

/// `IcebergFeature` 的配置：主体方块（浮冰 / 蓝冰）。
///
/// Configuration of `IcebergFeature`: the main block (packed ice / blue ice).
#[derive(Debug, Clone)]
pub struct IcebergFeature {
    pub state: u32,
}

impl IcebergFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("iceberg feature must be an object")?;
        Ok(Self {
            state: parse_state(object.get("state").ok_or("iceberg has no state")?)?,
        })
    }
}

/// `state.is(Blocks.X)`：按方块注册名比较。
///
/// `state.is(Blocks.X)`: compare by block registry name.
fn is(state: u32, name: &str) -> bool {
    block_of_state(state).is_some_and(|block| block.name == name)
}

/// `BlockState.isAir()`。
///
/// `BlockState.isAir()`.
fn is_air_state(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `Mth.ceil(float v)` = `(int)Math.ceil(v)`。
///
/// `Mth.ceil(float v)` = `(int)Math.ceil(v)`.
fn ceil_f32(value: f32) -> i32 {
    value.ceil() as i32
}

/// `Math.pow(x, 2.0)`：JDK 26 的 C2 把它折成 `x*x`，逐位一致。
///
/// `Math.pow(x, 2.0)`: JDK 26 C2 folds it to `x*x`, bit-identical.
fn pow2(x: f64) -> f64 {
    x * x
}

/// 一次 `place` 的状态，避免给每个私有辅助方法传十几参数。
///
/// State for one `place` call, so the private helpers avoid a dozen arguments.
struct IcebergGen<'w, 'a> {
    world: &'w mut FeatureWorld<'a>,
    random: &'w WorldgenRandom,
    state: u32,
    snow_on_top: bool,
    shape_angle: f64,
    shape_ellipse_a: i32,
    shape_ellipse_c: i32,
    is_ellipse: bool,
    over_water_height: i32,
    under_water_height: i32,
    width: i32,
    a: i32,
}

/// `signedDistanceEllipse` 的椭圆参数。
///
/// The ellipse parameters of `signedDistanceEllipse`.
struct EllipseAxes {
    origin_x: i32,
    origin_z: i32,
    a: i32,
    c: i32,
    angle: f64,
}

impl IcebergGen<'_, '_> {
    /// `IcebergFeature.place`。返回恒为 true。
    ///
    /// `IcebergFeature.place`. Always returns true.
    fn place(&mut self, origin: (i32, i32, i32)) -> bool {
        let (ox, _, oz) = origin;
        // origin = new BlockPos(origin.x, chunkGenerator.getSeaLevel(), origin.z)
        let oy = self.world.sea_level;
        let origin = (ox, oy, oz);

        self.snow_on_top = self.random.next_double() > 0.7;
        self.shape_angle = self.random.next_double() * 2.0 * std::f64::consts::PI;
        self.shape_ellipse_a = 11 - self.random.next_int(5);
        self.shape_ellipse_c = 3 + self.random.next_int(3);
        self.is_ellipse = self.random.next_double() > 0.7;

        self.over_water_height = if self.is_ellipse {
            self.random.next_int(6) + 6
        } else {
            self.random.next_int(15) + 3
        };
        if !self.is_ellipse && self.random.next_double() > 0.9 {
            self.over_water_height += self.random.next_int(19) + 7;
        }
        self.under_water_height = (self.over_water_height + self.random.next_int(11)).min(18);
        self.width =
            (self.over_water_height + self.random.next_int(7) - self.random.next_int(5)).min(11);
        self.a = if self.is_ellipse {
            self.shape_ellipse_a
        } else {
            11
        };

        // 水面上：xo / zo / yOff 三层循环，非椭圆时每层都调 heightDependentRadiusRound。
        // Above water: xo / zo / yOff loops; the round case draws radius randomness each layer.
        let a = self.a;
        for xo in -a..a {
            for zo in -a..a {
                for y_off in 0..self.over_water_height {
                    let radius = if self.is_ellipse {
                        self.height_dependent_radius_ellipse(y_off, self.over_water_height)
                    } else {
                        self.height_dependent_radius_round(y_off, self.over_water_height)
                    };
                    if self.is_ellipse || xo < radius {
                        self.generate_iceberg_block(
                            origin,
                            self.over_water_height,
                            xo,
                            y_off,
                            zo,
                            radius,
                            a,
                        );
                    }
                }
            }
        }

        self.smooth(origin, self.over_water_height);

        // 水面下：yOff 从 -1 递减到 -underWaterHeight（不含）。
        // Below water: yOff from -1 down to -underWaterHeight (exclusive).
        for xo in -a..a {
            for zo in -a..a {
                let mut y_off = -1;
                while y_off > -self.under_water_height {
                    let new_a = if self.is_ellipse {
                        ceil_f32(
                            a as f32
                                * (1.0_f32
                                    - ((y_off * y_off) as f32)
                                        / (self.under_water_height as f32 * 8.0_f32)),
                        )
                    } else {
                        a
                    };
                    let radius =
                        self.height_dependent_radius_steep(-y_off, self.under_water_height);
                    if xo < radius {
                        self.generate_iceberg_block(
                            origin,
                            self.under_water_height,
                            xo,
                            y_off,
                            zo,
                            radius,
                            new_a,
                        );
                    }
                    y_off -= 1;
                }
            }
        }

        let do_cut_out = if self.is_ellipse {
            self.random.next_double() > 0.1
        } else {
            self.random.next_double() > 0.7
        };
        if do_cut_out {
            self.generate_cut_out(origin, self.over_water_height);
        }
        true
    }

    /// `heightDependentRadiusEllipse`：不消费随机数。
    ///
    /// `heightDependentRadiusEllipse`: consumes no randomness.
    fn height_dependent_radius_ellipse(&self, y_off: i32, height: i32) -> i32 {
        let scale =
            (1.0_f32 - ((y_off * y_off) as f32) / (height as f32 * 1.0_f32)) * self.width as f32;
        ceil_f32(scale / 2.0_f32)
    }

    /// `heightDependentRadiusRound`：`nextFloat` 打头，随后 `nextInt(5)`，条件成立再 `nextInt(6)`。
    ///
    /// `heightDependentRadiusRound`: `nextFloat` first, then `nextInt(5)`, and `nextInt(6)`
    /// only inside the branch.
    fn height_dependent_radius_round(&self, y_off: i32, height: i32) -> i32 {
        let k = 3.5_f32 - self.random.next_float();
        let mut scale =
            (1.0_f32 - ((y_off * y_off) as f32) / (height as f32 * k)) * self.width as f32;
        if height > 15 + self.random.next_int(5) {
            let temp = if y_off < 3 + self.random.next_int(6) {
                y_off / 2
            } else {
                y_off
            };
            scale = (1.0_f32 - temp as f32 / (height as f32 * k * 0.4_f32)) * self.width as f32;
        }
        ceil_f32(scale / 2.0_f32)
    }

    /// `heightDependentRadiusSteep`：一次 `nextFloat()`。
    ///
    /// `heightDependentRadiusSteep`: one `nextFloat()`.
    fn height_dependent_radius_steep(&self, y_off: i32, height: i32) -> i32 {
        let k = 1.0_f32 + self.random.next_float() / 2.0_f32;
        let scale = (1.0_f32 - y_off as f32 / (height as f32 * k)) * self.width as f32;
        ceil_f32(scale / 2.0_f32)
    }

    /// `getEllipseC`。
    ///
    /// `getEllipseC`.
    fn get_ellipse_c(&self, y_off: i32, height: i32) -> i32 {
        let mut c = self.shape_ellipse_c;
        if y_off > 0 && height - y_off <= 3 {
            c -= 4 - (height - y_off);
        }
        c
    }

    /// `signedDistanceCircle`：`nextFloat` 经 `Mth.clamp`。
    ///
    /// `signedDistanceCircle`: `nextFloat` via `Mth.clamp`.
    fn signed_distance_circle(&self, xo: i32, zo: i32, radius: i32) -> f64 {
        let off = 10.0_f32 * clamp(self.random.next_float(), 0.2_f32, 0.8_f32) / radius as f32;
        off as f64 + pow2(xo as f64) + pow2(zo as f64) - pow2(radius as f64)
    }

    /// `signedDistanceEllipse`（origin 为 BlockPos 传入，分母为 int → double）。
    ///
    /// `signedDistanceEllipse` (origin is a BlockPos; int denominators widen to double).
    fn signed_distance_ellipse(&self, xo: i32, zo: i32, ellipse: EllipseAxes) -> f64 {
        let dx = (xo - ellipse.origin_x) as f64;
        let dz = (zo - ellipse.origin_z) as f64;
        let t1 = (dx * ellipse.angle.cos() - dz * ellipse.angle.sin()) / ellipse.a as f64;
        let t2 = (dx * ellipse.angle.sin() + dz * ellipse.angle.cos()) / ellipse.c as f64;
        pow2(t1) + pow2(t2) - 1.0
    }

    /// `isIcebergState`。
    ///
    /// `isIcebergState`.
    fn is_iceberg_state(state: u32) -> bool {
        is(state, "minecraft:packed_ice")
            || is(state, "minecraft:snow_block")
            || is(state, "minecraft:blue_ice")
    }

    /// `generateIcebergBlock`。
    ///
    /// `generateIcebergBlock`.
    #[allow(clippy::too_many_arguments)]
    fn generate_iceberg_block(
        &mut self,
        origin: (i32, i32, i32),
        height: i32,
        xo: i32,
        y_off: i32,
        zo: i32,
        radius: i32,
        a: i32,
    ) {
        let signed_dist = if self.is_ellipse {
            let c = self.get_ellipse_c(y_off, height);
            self.signed_distance_ellipse(
                xo,
                zo,
                EllipseAxes {
                    origin_x: 0,
                    origin_z: 0,
                    a,
                    c,
                    angle: self.shape_angle,
                },
            )
        } else {
            self.signed_distance_circle(xo, zo, radius)
        };
        if signed_dist < 0.0 {
            let compare_val = if self.is_ellipse {
                -0.5
            } else {
                -6.0 - self.random.next_int(3) as f64
            };
            if signed_dist > compare_val && self.random.next_double() > 0.9 {
                return;
            }
            self.set_iceberg_block(origin, xo, y_off, zo, height, height - y_off);
        }
    }

    /// `setIcebergBlock`：先算 randomness（椭圆才取 `nextDouble`），条件里再取 `nextInt`。
    ///
    /// `setIcebergBlock`: compute randomness (ellipse only draws `nextDouble`) first,
    /// then the condition may draw `nextInt`.
    fn set_iceberg_block(
        &mut self,
        origin: (i32, i32, i32),
        xo: i32,
        y_off: i32,
        zo: i32,
        height: i32,
        h_diff: i32,
    ) {
        let (x, y, z) = (origin.0 + xo, origin.1 + y_off, origin.2 + zo);
        let state = self.world.get_block(x, y, z);
        if is_air_state(state)
            || is(state, "minecraft:snow_block")
            || is(state, "minecraft:ice")
            || is(state, "minecraft:water")
        {
            let randomness = !self.is_ellipse || self.random.next_double() > 0.05;
            let divisor = if self.is_ellipse { 3 } else { 2 };
            let snow = self.snow_on_top
                && !is(state, "minecraft:water")
                && (h_diff as f64
                    <= self.random.next_int((height / divisor).max(1)) as f64
                        + height as f64 * 0.6)
                && randomness;
            let placed = if snow { blocks::SNOW_BLOCK } else { self.state };
            self.world.set_block_with_heightmaps(x, y, z, placed);
        }
    }

    /// `smooth`：抹掉悬空和孤立的冰山方块。
    ///
    /// `smooth`: remove floating and isolated iceberg blocks.
    fn smooth(&mut self, origin: (i32, i32, i32), height: i32) {
        let a = if self.is_ellipse {
            self.shape_ellipse_a
        } else {
            self.width / 2
        };
        for x in -a..=a {
            for z in -a..=a {
                for y_off in 0..=height {
                    let (px, py, pz) = (origin.0 + x, origin.1 + y_off, origin.2 + z);
                    let state = self.world.get_block(px, py, pz);
                    if Self::is_iceberg_state(state) || is(state, "minecraft:snow") {
                        let below = self.world.get_block(px, py - 1, pz);
                        if is_air_state(below) {
                            self.world
                                .set_block_with_heightmaps(px, py, pz, blocks::AIR);
                            self.world
                                .set_block_with_heightmaps(px, py + 1, pz, blocks::AIR);
                        } else if Self::is_iceberg_state(state) {
                            let neighbours = [
                                self.world.get_block(px - 1, py, pz),
                                self.world.get_block(px + 1, py, pz),
                                self.world.get_block(px, py, pz - 1),
                                self.world.get_block(px, py, pz + 1),
                            ];
                            let counter = neighbours
                                .iter()
                                .filter(|&&side| !Self::is_iceberg_state(side))
                                .count();
                            if counter >= 3 {
                                self.world
                                    .set_block_with_heightmaps(px, py, pz, blocks::AIR);
                            }
                        }
                    }
                }
            }
        }
    }

    /// `generateCutOut`。
    ///
    /// `generateCutOut`.
    fn generate_cut_out(&mut self, global_origin: (i32, i32, i32), height: i32) {
        let sign_x = if self.random.next_boolean() { -1 } else { 1 };
        let sign_z = if self.random.next_boolean() { -1 } else { 1 };
        let first = (self.width / 2 - 2).max(1);
        let second = (self.width - self.width / 2 - 1).max(1);
        let mut x_off = self.random.next_int(first);
        if self.random.next_boolean() {
            x_off = self.width / 2 + 1 - self.random.next_int(second);
        }
        let mut z_off = self.random.next_int(first);
        if self.random.next_boolean() {
            z_off = self.width / 2 + 1 - self.random.next_int(second);
        }
        if self.is_ellipse {
            x_off = self.random.next_int((self.shape_ellipse_a - 5).max(1));
            z_off = x_off;
        }
        let local_origin = (sign_x * x_off, sign_z * z_off);
        let angle = if self.is_ellipse {
            self.shape_angle + std::f64::consts::PI / 2.0
        } else {
            self.random.next_double() * 2.0 * std::f64::consts::PI
        };
        for y_off in 0..height - 3 {
            let radius = self.height_dependent_radius_round(y_off, height);
            self.carve(radius, y_off, global_origin, false, angle, local_origin);
        }
        // 条件里的 nextInt(5) 每轮都要取一次。
        // The nextInt(5) in the condition is drawn on every iteration.
        let mut y_off = -1;
        loop {
            if y_off <= -height + self.random.next_int(5) {
                break;
            }
            let radius = self.height_dependent_radius_steep(-y_off, height);
            self.carve(radius, y_off, global_origin, true, angle, local_origin);
            y_off -= 1;
        }
    }

    /// `carve`。
    ///
    /// `carve`.
    #[allow(clippy::too_many_arguments)]
    fn carve(
        &mut self,
        radius: i32,
        y_off: i32,
        global_origin: (i32, i32, i32),
        under_water: bool,
        angle: f64,
        local_origin: (i32, i32),
    ) {
        let a = radius + 1 + self.shape_ellipse_a / 3;
        let c = (radius - 3).min(3) + self.shape_ellipse_c / 2 - 1;
        for xo in -a..a {
            for zo in -a..a {
                let signed_dist = self.signed_distance_ellipse(
                    xo,
                    zo,
                    EllipseAxes {
                        origin_x: local_origin.0,
                        origin_z: local_origin.1,
                        a,
                        c,
                        angle,
                    },
                );
                if signed_dist < 0.0 {
                    let (px, py, pz) = (
                        global_origin.0 + xo,
                        global_origin.1 + y_off,
                        global_origin.2 + zo,
                    );
                    let state = self.world.get_block(px, py, pz);
                    if Self::is_iceberg_state(state) || is(state, "minecraft:snow_block") {
                        if under_water {
                            self.world
                                .set_block_with_heightmaps(px, py, pz, blocks::WATER);
                        } else {
                            self.world
                                .set_block_with_heightmaps(px, py, pz, blocks::AIR);
                            // removeFloatingSnowLayer：上方雪层换成空气。
                            // removeFloatingSnowLayer: replace the snow layer above with air.
                            let above = self.world.get_block(px, py + 1, pz);
                            if is(above, "minecraft:snow") {
                                self.world
                                    .set_block_with_heightmaps(px, py + 1, pz, blocks::AIR);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `IcebergFeature.place`。
///
/// `IcebergFeature.place`.
pub fn place(
    feature: &IcebergFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let mut builder = IcebergGen {
        world,
        random,
        state: feature.state,
        snow_on_top: false,
        shape_angle: 0.0,
        shape_ellipse_a: 0,
        shape_ellipse_c: 0,
        is_ellipse: false,
        over_water_height: 0,
        under_water_height: 0,
        width: 0,
        a: 0,
    };
    builder.place(origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_iceberg_states() {
        let packed = IcebergFeature::parse(
            &serde_json::from_str(r#"{"state":"minecraft:packed_ice"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(packed.state, mc_bamboo_data::blocks::PACKED_ICE);
        let blue = IcebergFeature::parse(
            &serde_json::from_str(r#"{"state":"minecraft:blue_ice"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(blue.state, mc_bamboo_data::blocks::BLUE_ICE);
    }

    #[test]
    fn pow2_matches_square() {
        for value in [-3.5_f64, -1.0, 0.0, 0.25, 7.0, 123.456] {
            assert_eq!(pow2(value), value * value);
        }
    }
}
