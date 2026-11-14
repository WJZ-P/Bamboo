//! `OreFeature` / `AbstractOreFeature`：`UNDERGROUND_ORES` 步骤的主力。
//!
//! 逐段照 26.3 字节码（`OreFeature.place` / `doPlace`，
//! `AbstractOreFeature.canPlaceOre` / `isAdjacentToAir`）：
//! `place` 先取 `nextFloat()*π` 和两个 `nextInt(3)`，算椭球的中心与 `-8..8`
//! 的边界；`doPlace` 用 `BitSet` 形状 + 逐点 `nextDouble()`，最后按
//! `targets` 顺序找第一个 `RuleTest` 命中的状态写进区块。

use crate::random::WorldgenRandom;

use super::{
    placement::OreFeature,
    rule_test::RuleTest,
    world::{FeatureWorld, HeightmapKind},
};

/// `Mth.sin(double)`：65536 项查找表，`OreFeature.doPlace` 用。
fn mth_sin(value: f64) -> f32 {
    const SCALE: f64 = 10_430.378_350_470_453;
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..65_536)
            .map(|i| (i as f64 / SCALE).sin() as f32)
            .collect()
    });
    let index = ((value * SCALE) as i64) & 65_535;
    table[index as usize]
}

/// `OreFeature.place`。
pub fn place(
    ore: &OreFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    // 字节码 0-38：f = nextFloat()*π，f1 = size/8，i = ceil((2*(size/16)+1)/2)。
    let f = random.next_float() * std::f32::consts::PI;
    let f1 = ore.size as f32 / 8.0;
    let i = ((2.0 * (ore.size as f32 / 16.0) + 1.0) / 2.0).ceil() as i32;

    // 40-114：Math.sin/cos（double），中心 x/z 各加减 f1。
    let sin = (f as f64).sin();
    let cos = (f as f64).cos();
    let d0 = ox as f64 + sin * f1 as f64;
    let d1 = ox as f64 - sin * f1 as f64;
    let d2 = oz as f64 + cos * f1 as f64;
    let d3 = oz as f64 - cos * f1 as f64;

    // 116-153：两个 nextInt(3) 决定 y 的上下端。
    let d4 = (oy + random.next_int(3) - 2) as f64;
    let d5 = (oy + random.next_int(3) - 2) as f64;

    // 155-211：包围盒。
    let ceil_f1 = f1.ceil() as i32;
    let j = ox - ceil_f1 - i;
    let k = oy - 2 - i;
    let l = oz - ceil_f1 - i;
    let i1 = 2 * (ceil_f1 + i);
    let j1 = 2 * (2 + i);

    // 219-306：逐列用 OCEAN_FLOOR_WG 高度图剪枝，第一个满足的就 doPlace 并返回。
    for x in j..=j + i1 {
        for z in l..=l + i1 {
            let height = world.get_height(HeightmapKind::OceanFloorWg, x, z);
            if k <= height {
                return do_place(ore, world, random, d0, d1, d2, d3, d4, d5, j, k, l, i1, j1);
            }
        }
    }
    false
}

/// `OreFeature.doPlace`，参数顺序照方法签名。
#[allow(clippy::too_many_arguments)]
fn do_place(
    ore: &OreFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    d0: f64,
    d1: f64,
    d2: f64,
    d3: f64,
    d4: f64,
    d5: f64,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    size_xz: i32,
    size_y: i32,
) -> bool {
    let size = ore.size.max(0) as usize;
    // 字节码 3-18：BitSet(sizeXZ * sizeY * sizeXZ)；下标公式见 713-738。
    let bit_len = (size_xz.max(0) as usize)
        .saturating_mul(size_y.max(0) as usize)
        .saturating_mul(size_xz.max(0) as usize);
    let mut mask = vec![false; bit_len];

    // 29-185：每条椭球半径。
    let mut spheres = vec![[0.0_f64; 4]; size];
    for (i, sphere) in spheres.iter_mut().enumerate() {
        let f = i as f32 / ore.size as f32;
        let cx = lerp(f as f64, d0, d1);
        let cy = lerp(f as f64, d4, d5);
        let cz = lerp(f as f64, d2, d3);
        // nextDouble() * size / 16
        let d = random.next_double() * ore.size as f64 / 16.0;
        // 字节码 115-136：π(f32)*f(f32) 先做 f32 乘法再 f2d；Mth.sin 返回 float，
        // +1f 是 f32 加法再 f2d；之后是 double 的 *d、+1、/2（不是 FMA）。
        let angle = (std::f32::consts::PI * f) as f64;
        let radius = (f64::from(mth_sin(angle) + 1.0_f32) * d + 1.0) / 2.0;
        *sphere = [cx, cy, cz, radius];
    }

    // 188-406：去掉被别的球完全包住的球（半径置 -1）。
    for i in 0..size.saturating_sub(1) {
        if spheres[i][3] <= 0.0 {
            continue;
        }
        for j in (i + 1)..size {
            if spheres[j][3] <= 0.0 {
                continue;
            }
            let dx = spheres[i][0] - spheres[j][0];
            let dy = spheres[i][1] - spheres[j][1];
            let dz = spheres[i][2] - spheres[j][2];
            let dr = spheres[i][3] - spheres[j][3];
            if dr * dr > dx * dx + dy * dy + dz * dz {
                if dr > 0.0 {
                    spheres[j][3] = -1.0;
                } else {
                    spheres[i][3] = -1.0;
                }
            }
        }
    }

    let mut placed = 0usize;
    for sphere in spheres.iter().take(size) {
        let radius = sphere[3];
        if radius < 0.0 {
            continue;
        }
        let cx = sphere[0];
        let cy = sphere[1];
        let cz = sphere[2];
        // 字节码 483-571：六个边界，Mth.floor 后和包围盒取 max。
        let x0 = (cx - radius).floor() as i32;
        let y0 = (cy - radius).floor() as i32;
        let z0 = (cz - radius).floor() as i32;
        let x1 = (cx + radius).floor() as i32;
        let y1 = (cy + radius).floor() as i32;
        let z1 = (cz + radius).floor() as i32;
        let x0 = x0.max(min_x);
        let y0 = y0.max(min_y);
        let z0 = z0.max(min_z);
        let x1 = x1.max(x0);
        let y1 = y1.max(y0);
        let z1 = z1.max(z0);

        for x in x0..=x1 {
            // 字节码 584-606：dx = ((x+0.5)-cx)/radius，dx² >= 1 就跳过整列。
            let dx = (x as f64 + 0.5 - cx) / radius;
            if dx * dx >= 1.0 {
                continue;
            }
            for y in y0..=y1 {
                let dy = (y as f64 + 0.5 - cy) / radius;
                if dx * dx + dy * dy >= 1.0 {
                    continue;
                }
                for z in z0..=z1 {
                    let dz = (z as f64 + 0.5 - cz) / radius;
                    if dx * dx + dy * dy + dz * dz >= 1.0 {
                        continue;
                    }
                    // 699：isOutsideBuildHeight(y)。
                    if y < world.min_y || y >= world.min_y + world.height {
                        continue;
                    }
                    // 713-758：BitSet 下标；用 saturating 计算防止异常尺寸 panic。
                    let index = (x - min_x) as i64
                        + (y - min_y) as i64 * size_xz as i64
                        + (z - min_z) as i64 * size_xz as i64 * size_y as i64;
                    if index < 0 || index as usize >= mask.len() {
                        continue;
                    }
                    let index = index as usize;
                    if mask[index] {
                        continue;
                    }
                    mask[index] = true;
                    // 773-786：ensureCanWrite（3×3 之外）。
                    if !world.can_write(x, y, z) {
                        continue;
                    }
                    let state = world.get_block(x, y, z);
                    for (rule, replacement) in &ore.targets {
                        if can_place_ore(ore, rule, world, random, state, x, y, z) {
                            if world.set_block(x, y, z, *replacement) {
                                placed += 1;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    placed > 0
}

/// `Mth.lerp(alpha, p0, p1)` 的 double 版本：`p0 + alpha*(p1-p0)`。
fn lerp(alpha: f64, p0: f64, p1: f64) -> f64 {
    p0 + alpha * (p1 - p0)
}

/// `AbstractOreFeature.canPlaceOre`。
#[allow(clippy::too_many_arguments)]
fn can_place_ore(
    ore: &OreFeature,
    rule: &RuleTest,
    world: &FeatureWorld<'_>,
    random: &WorldgenRandom,
    state: u32,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    if !rule.test(state, x, y, z, random) {
        return false;
    }
    // shouldSkipAirCheck：chance <= 0 直接 true；>= 1 直接 false；
    // 否则 nextFloat() >= chance 时跳过检查（即 true）。
    let chance = ore.discard_chance_on_air_exposure;
    if chance <= 0.0 {
        return true;
    }
    if chance >= 1.0 {
        return !is_adjacent_to_air(world, x, y, z);
    }
    if random.next_float() >= chance {
        return true;
    }
    !is_adjacent_to_air(world, x, y, z)
}

/// `AbstractOreFeature.isAdjacentToAir`：六个方向，`Direction.values()` 顺序。
/// 26.3 是 DOWN, UP, NORTH, SOUTH, WEST, EAST。
fn is_adjacent_to_air(world: &FeatureWorld<'_>, x: i32, y: i32, z: i32) -> bool {
    const DIRECTIONS: [(i32, i32, i32); 6] = [
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
        (-1, 0, 0),
        (1, 0, 0),
    ];
    for (dx, dy, dz) in DIRECTIONS {
        if world.is_air(x + dx, y + dy, z + dz) {
            return true;
        }
    }
    false
}
