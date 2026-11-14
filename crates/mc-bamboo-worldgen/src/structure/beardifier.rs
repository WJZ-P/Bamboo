//! `net.minecraft.world.level.levelgen.Beardifier`：结构件对 terrain 密度的贡献。
//!
//! `final_density` 的顶层是 `add(base, beardifier)`：beardifier 是一个不含噪声取数的纯函数，
//! 由本区块 `structure_starts` / `structure_references` 里 `terrainAdaptation != NONE` 的结构件
//! 组装出 `Rigid` / `JigsawJunction` / `affectedBox`，再逐点算密度贡献。官方
//! `SampleVolume` 只在 `affectedBox ∩ volume` 内算 `sampleValueUnchecked`，外面填 0。
//!
//! 这里只移植纯计算（`for_structures_in_chunk` 的组装、`sample_value`、`sample_volume`）；
//! 不做结构件 `postProcess`，也不读世界。接入方式：terrain 的 `final_density` 把 beardifier
//! 标记编译成常量 0（EMPTY 基线），再由 `chunk::Generator` 把真实 beardifier 的贡献逐格加到
//! 密度缓冲上——因为标记在 `final_density` 顶层且无取数，`base + 0 + beard` 与官方
//! `add(base, beard)` 逐位相同，且不改噪声取数次数与顺序。`first_occupied_height` 永远走
//! EMPTY 分支（官方 `iterateNoiseColumn` 用 `null` beardifier），见 `chunk.rs`。
//!
//! `net.minecraft.world.level.levelgen.Beardifier`: structure pieces' contribution to terrain
//! density. `final_density` tops out at `add(base, beardifier)`; the beardifier is a pure,
//! noise-free function built from the chunk's structure pieces whose `terrainAdaptation != NONE`.
//! This module ports only the pure computation (assembly, `sample_value`, `sample_volume`), not
//! `postProcess`. Wiring: the compiled `final_density` keeps the marker as constant 0 (the EMPTY
//! baseline) and `chunk::Generator` adds the real beardifier's contribution cell by cell; since
//! the marker sits at the top and draws no randomness, `base + 0 + beard` is bit-identical to
//! vanilla `add(base, beard)` and does not change noise draw order. `first_occupied_height`
//! always uses the EMPTY branch (vanilla `iterateNoiseColumn` passes a `null` beardifier).

use std::sync::OnceLock;

use crate::{math::java_floor_div, structure::pieces::Box, volume::Volume};

/// `TerrainAdjustment` 的五个值，`ordinal()` 顺序。
///
/// The five `TerrainAdjustment` values in `ordinal()` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

impl TerrainAdjustment {
    /// 官方 `values()[ordinal]`，`minecraft:...` 名字只用于解析，这里按数字输入。
    ///
    /// Vanilla `values()[ordinal]`; names are only for parsing, input here is numeric.
    pub fn from_ordinal(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::Bury,
            2 => Self::BeardThin,
            3 => Self::BeardBox,
            4 => Self::Encapsulate,
            _ => return None,
        })
    }

    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Bury,
        Self::BeardThin,
        Self::BeardBox,
        Self::Encapsulate,
    ];
}

/// `Beardifier.Rigid(BoundingBox box, TerrainAdjustment, int groundLevelDelta)`。
///
/// `Beardifier.Rigid(BoundingBox box, TerrainAdjustment, int groundLevelDelta)`.
#[derive(Debug, Clone, Copy)]
pub struct Rigid {
    pub box_: Box,
    pub terrain_adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

/// `JigsawJunction` 里 beardifier 只用到这三个来源坐标。
///
/// The three source coords of a `JigsawJunction` that the beardifier reads.
#[derive(Debug, Clone, Copy)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}

/// 一条 beardifier 的输入：`Rigid` 列表、junction 列表、`affectedBox`（可为 `None` 表示 EMPTY）。
///
/// A beardifier's input: `Rigid`s, junctions, and `affectedBox` (`None` means EMPTY).
#[derive(Debug, Clone, Default)]
pub struct Beardifier {
    pub pieces: Vec<Rigid>,
    pub junctions: Vec<Junction>,
    /// `anyPieceBoundingBox.inflatedBy(24)`；`None` 对应官方 `Beardifier.EMPTY`。
    ///
    /// `anyPieceBoundingBox.inflatedBy(24)`; `None` corresponds to vanilla `Beardifier.EMPTY`.
    pub affected_box: Option<Box>,
}

/// `BEARD_KERNEL`：24³ 个 `e^(-dist²/16)`，官方构造期用 `Math.pow(Math.E, -dist²/16)` 算出，
/// 这里用等价的 `exp`——已验证 13824 个 `(float)` 值逐位相同。索引 `zi*576 + xi*24 + yi`。
///
/// `BEARD_KERNEL`: 24³ `e^(-dist²/16)` values. Vanilla builds them with `Math.pow(Math.E, ...)`;
/// we use the equivalent `exp` — all 13824 `(float)` values were verified bit-identical.
fn kernel() -> &'static [f32; 13824] {
    static KERNEL: OnceLock<[f32; 13824]> = OnceLock::new();
    KERNEL.get_or_init(|| {
        let mut kernel = [0.0_f32; 13824];
        for zi in 0..24_i32 {
            for xi in 0..24_i32 {
                for yi in 0..24_i32 {
                    let dx = f64::from(xi - 12);
                    let dy = f64::from(yi - 12) + 0.5;
                    let dz = f64::from(zi - 12);
                    let distance_sqr = dx * dx + dy * dy + dz * dz;
                    kernel[(zi * 576 + xi * 24 + yi) as usize] =
                        (-distance_sqr / 16.0).exp() as f32;
                }
            }
        }
        kernel
    })
}

/// `Mth.fastInvSqrt(double)`：Quake 式 double 位技巧 + 一次牛顿迭代。
///
/// `Mth.fastInvSqrt(double)`: the Quake-style double bit trick plus one Newton iteration.
fn fast_inv_sqrt(x: f64) -> f64 {
    let xhalf = 0.5 * x;
    let bits = x.to_bits() as i64;
    let i = 6_910_469_410_427_058_090_i64.wrapping_sub(bits >> 1);
    let mut y = f64::from_bits(i as u64);
    y *= 1.5 - xhalf * y * y;
    y
}

/// `Mth.sqrt(float x)` = `(float)Math.sqrt((double)x)`。
///
/// `Mth.sqrt(float x)` = `(float)Math.sqrt((double)x)`.
fn sqrt_f32(x: f32) -> f32 {
    (f64::from(x)).sqrt() as f32
}

/// `Mth.lengthSquared(float x, y, z)` = `x*x + y*y + z*z`（float 运算）。
///
/// `Mth.lengthSquared(float x, y, z)` = `x*x + y*y + z*z` (float arithmetic).
fn length_squared_f32(x: f32, y: f32, z: f32) -> f32 {
    x * x + y * y + z * z
}

/// `Beardifier.getBuryContribution(dx, dy, dz)`。
///
/// `Beardifier.getBuryContribution(dx, dy, dz)`.
fn get_bury_contribution(dx: f32, dy: f32, dz: f32) -> f32 {
    let distance_sq = length_squared_f32(dx, dy, dz);
    if distance_sq >= 36.0 {
        0.0
    } else {
        1.0 - sqrt_f32(distance_sq) / 6.0
    }
}

/// `Beardifier.getBeardContribution(dx, dy, dz, yToGround)`。
///
/// `Beardifier.getBeardContribution(dx, dy, dz, yToGround)`.
fn get_beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f32 {
    let xi = dx + 12;
    let yi = dy + 12;
    let zi = dz + 12;
    let in_range = |v: i32| (0..24).contains(&v);
    if in_range(xi) && in_range(yi) && in_range(zi) {
        let dy_with_offset = y_to_ground as f32 + 0.5;
        let distance_sqr = length_squared_f32(dx as f32, dy_with_offset, dz as f32);
        // 官方 `distanceSqr / 2.0F` 先在 float 里除，再拓宽到 double 喂 `fastInvSqrt(double)`。
        // Vanilla divides `distanceSqr / 2.0F` in float, then widens to double for `fastInvSqrt`.
        let inv = fast_inv_sqrt(f64::from(distance_sqr / 2.0_f32)) as f32;
        let value = -dy_with_offset * inv / 2.0_f32;
        value * kernel()[(zi * 576 + xi * 24 + yi) as usize]
    } else {
        0.0
    }
}

/// `BoundingBox.inflatedBy(n)`，六个轴都外扩。
///
/// `BoundingBox.inflatedBy(n)`, expanded on all six sides.
fn inflated_by(box_: Box, n: i32) -> Box {
    [
        box_[0] - n,
        box_[1] - n,
        box_[2] - n,
        box_[3] + n,
        box_[4] + n,
        box_[5] + n,
    ]
}

/// `BoundingBox.encapsulating(a, b)`：逐轴取 min / max。
///
/// `BoundingBox.encapsulating(a, b)`: min/max on each axis.
fn encapsulating(a: Box, b: Box) -> Box {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].min(b[2]),
        a[3].max(b[3]),
        a[4].max(b[4]),
        a[5].max(b[5]),
    ]
}

fn inside(box_: &Box, x: i32, y: i32, z: i32) -> bool {
    x >= box_[0] && x <= box_[3] && z >= box_[2] && z <= box_[5] && y >= box_[1] && y <= box_[4]
}

impl Beardifier {
    /// 官方 `Beardifier.EMPTY`：没有任何结构件，`affectedBox` 为 null，处处返回 0。
    ///
    /// Vanilla `Beardifier.EMPTY`: no pieces, null `affectedBox`, zero everywhere.
    pub const EMPTY: Self = Self {
        pieces: Vec::new(),
        junctions: Vec::new(),
        affected_box: None,
    };

    /// 从一组 `(piece box, terrainAdjustment, groundLevelDelta, junctions, isRigid)` 组装
    /// `Beardifier`，照 `Beardifier.forStructuresInChunk` 过滤 `isCloseToChunk(chunkPos, 12)`、
    /// junction 的 XZ 范围，并 `inflatedBy(24)` 得到 `affectedBox`。
    ///
    /// `StructurePiece` 的 `postProcess` 与 `projection` 细节不在本函数内，由调用方按官方
    /// `PoolElementStructurePiece`（`RIGID` 才加入 `Rigid`）判好 `is_rigid` 再传入。
    ///
    /// Assembles a `Beardifier` from `(piece box, terrainAdjustment, groundLevelDelta,
    /// junctions, isRigid)` tuples, mirroring `Beardifier.forStructuresInChunk`: keeps
    /// `isCloseToChunk(chunkPos, 12)` pieces, filters junction XZ range, and
    /// `inflatedBy(24)` to get `affectedBox`. `postProcess` / `projection` handling is the
    /// caller's job: pass `is_rigid` per official `PoolElementStructurePiece` (RIGID only).
    pub fn for_structures_in_chunk(
        chunk_x: i32,
        chunk_z: i32,
        inputs: &[StructureBeardInput],
    ) -> Self {
        let min_x = chunk_x * 16;
        let min_z = chunk_z * 16;
        let mut pieces = Vec::new();
        let mut junctions = Vec::new();
        let mut any_box: Option<Box> = None;

        for input in inputs {
            let b = input.box_;
            // `StructurePiece.isCloseToChunk(pos, 12)`：只比 XZ。
            // `StructurePiece.isCloseToChunk(pos, 12)`: XZ-only.
            let close = b[3] >= min_x - 12
                && b[0] <= min_x + 15 + 12
                && b[5] >= min_z - 12
                && b[2] <= min_z + 15 + 12;
            if !close {
                continue;
            }
            if input.is_rigid {
                pieces.push(Rigid {
                    box_: b,
                    terrain_adjustment: input.terrain_adjustment,
                    ground_level_delta: input.ground_level_delta,
                });
                any_box = Some(match any_box {
                    None => b,
                    Some(existing) => encapsulating(existing, b),
                });
            }
            for j in input.junctions {
                if j.source_x > min_x - 12
                    && j.source_z > min_z - 12
                    && j.source_x < min_x + 15 + 12
                    && j.source_z < min_z + 15 + 12
                {
                    junctions.push(*j);
                    let jb = [
                        j.source_x,
                        j.source_ground_y,
                        j.source_z,
                        j.source_x,
                        j.source_ground_y,
                        j.source_z,
                    ];
                    any_box = Some(match any_box {
                        None => jb,
                        Some(existing) => encapsulating(existing, jb),
                    });
                }
            }
        }

        let affected_box = any_box.map(|b| inflated_by(b, 24));
        Self {
            pieces,
            junctions,
            affected_box,
        }
    }

    /// 官方 `sampleValue(context, x, y, z)`：`affectedBox` 内含点才算，否则 0。
    ///
    /// Vanilla `sampleValue(context, x, y, z)`: computed only inside `affectedBox`, else 0.
    pub fn sample_value(&self, x: i32, y: i32, z: i32) -> f32 {
        match self.affected_box {
            Some(ref b) if inside(b, x, y, z) => self.sample_value_unchecked(x, y, z),
            _ => 0.0,
        }
    }

    /// 官方 `sampleValueUnchecked`：累加所有 `Rigid` 与 junction 的贡献。
    ///
    /// Vanilla `sampleValueUnchecked`: sum all `Rigid` and junction contributions.
    fn sample_value_unchecked(&self, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        let mut noise_value = 0.0_f32;
        for rigid in &self.pieces {
            let b = rigid.box_;
            let dx = 0.max((b[0] - block_x).max(block_x - b[3]));
            let dz = 0.max((b[2] - block_z).max(block_z - b[5]));
            let ground_y = b[1] + rigid.ground_level_delta;
            let dy_to_ground = block_y - ground_y;
            let dy = match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => dy_to_ground,
                TerrainAdjustment::BeardBox => 0.max((ground_y - block_y).max(block_y - b[4])),
                TerrainAdjustment::Encapsulate => 0.max((b[1] - block_y).max(block_y - b[4])),
            };
            noise_value += match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0.0,
                TerrainAdjustment::Bury => {
                    get_bury_contribution(dx as f32, dy as f32 / 2.0, dz as f32)
                }
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    get_beard_contribution(dx, dy, dz, dy_to_ground) * 0.8
                }
                TerrainAdjustment::Encapsulate => {
                    get_bury_contribution(dx as f32 / 2.0, dy as f32 / 2.0, dz as f32 / 2.0) * 0.8
                }
            };
        }
        for j in &self.junctions {
            let dx = block_x - j.source_x;
            let dy = block_y - j.source_ground_y;
            let dz = block_z - j.source_z;
            noise_value += get_beard_contribution(dx, dy, dz, dy) * 0.4;
        }
        noise_value
    }

    /// 官方 `sampleVolume`：`affectedBox ∩ volume` 内填 `sampleValueUnchecked`，其余 0。
    /// 这里把贡献**加**到 `out` 上（对应 `final_density` 顶层的 `add`）。
    ///
    /// Vanilla `sampleVolume`: `sampleValueUnchecked` inside `affectedBox ∩ volume`, 0 elsewhere.
    /// Here the contribution is **added** to `out` (the top-level `add` in `final_density`).
    pub fn add_to_volume(&self, out: &mut [f32], volume: &Volume) {
        let Some(b) = self.affected_box else {
            return;
        };
        // `volume.intersects(affectedBox)`：三轴都相交才算。
        // `volume.intersects(affectedBox)`: all three axes overlap.
        if volume.max_x() < b[0]
            || volume.min_x > b[3]
            || volume.max_y() < b[1]
            || volume.min_y > b[4]
            || volume.max_z() < b[2]
            || volume.min_z > b[5]
        {
            return;
        }
        let min_x = java_floor_div(0.max(b[0] - volume.min_x), volume.step_x);
        let min_y = java_floor_div(0.max(b[1] - volume.min_y), volume.step_y);
        let min_z = java_floor_div(0.max(b[2] - volume.min_z), volume.step_z);
        let max_x = (volume.size_x - 1).min(java_floor_div(b[3] - volume.min_x, volume.step_x));
        let max_y = (volume.size_y - 1).min(java_floor_div(b[4] - volume.min_y, volume.step_y));
        let max_z = (volume.size_z - 1).min(java_floor_div(b[5] - volume.min_z, volume.step_z));
        for z in min_z..=max_z {
            let bz = volume.block_z(z);
            for x in min_x..=max_x {
                let bx = volume.block_x(x);
                for y in min_y..=max_y {
                    let by = volume.block_y(y);
                    let index = volume.index(x, y, z);
                    out[index] += self.sample_value_unchecked(bx, by, bz);
                }
            }
        }
    }

    /// `for_structures_in_chunk` 的 `Beardifier.EMPTY` 判定与官方一致。
    ///
    /// `Beardifier.EMPTY` check, matching `for_structures_in_chunk`.
    pub fn is_empty(&self) -> bool {
        self.affected_box.is_none()
    }
}

/// `for_structures_in_chunk` 的单条输入：一个结构件（或其 junction）的 beardifier 数据。
///
/// One input to `for_structures_in_chunk`: a structure piece's beardifier data.
#[derive(Debug, Clone, Copy)]
pub struct StructureBeardInput {
    pub box_: Box,
    pub terrain_adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
    /// 官方 `PoolElementStructurePiece` 且 `projection == RIGID` 才为 true。
    ///
    /// True only for a `PoolElementStructurePiece` with `projection == RIGID`.
    pub is_rigid: bool,
    pub junctions: &'static [Junction],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero_everywhere() {
        let b = Beardifier::EMPTY;
        assert!(b.is_empty());
        for x in -40..=40 {
            assert_eq!(b.sample_value(x, 64, x), 0.0);
        }
    }

    #[test]
    fn kernel_matches_official_bits() {
        // 锚点取自官方 `BEARD_KERNEL` 反射读出的前几个值（PowProbe）。
        // Anchors read from the official `BEARD_KERNEL` via reflection (PowProbe).
        let k = kernel();
        assert_eq!(k[0].to_bits(), 747_230_124);
        assert_eq!(k[1].to_bits(), 763_905_896);
        assert_eq!(k[600].to_bits(), 781_758_625);
    }

    #[test]
    fn affected_box_is_union_inflated() {
        let inputs = vec![StructureBeardInput {
            box_: [0, 60, 0, 10, 70, 10],
            terrain_adjustment: TerrainAdjustment::BeardThin,
            ground_level_delta: 0,
            is_rigid: true,
            junctions: &[],
        }];
        let b = Beardifier::for_structures_in_chunk(0, 0, &inputs);
        assert_eq!(b.affected_box, Some([-24, 36, -24, 34, 94, 34]));
    }
}
