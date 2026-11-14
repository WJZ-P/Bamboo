//! 官方 `net.minecraft.world.level.levelgen.Beardifier` 的移植：结构件对地形最终密度的贡献。
//!
//! 它是一个 `DensitySampler`：terrain 的 `finalDensity` 里那个 `beardifier` 标记节点在求值时
//! 从 `SamplerContext` 里取出本对象取值（见 `sampler.rs`）。构造它只需要结构件的包围盒、
//! 地形调整类型、`groundLevelDelta` 和 jigsaw 连接点，不需要生成结构件本身，所以能直接
//! 接收 `structure_starts` 阶段的产物，也能吃对拍工具导出的官方数据。
//!
//! Port of vanilla `net.minecraft.world.level.levelgen.Beardifier`: the contribution of
//! structure pieces to the terrain final density. It is a `DensitySampler`; the `beardifier`
//! marker node inside `finalDensity` reads it from the `SamplerContext` at sample time (see
//! `sampler.rs`). Building it needs only piece boxes, terrain adjustments, `groundLevelDelta`
//! and jigsaw junctions, not the piece-generation itself, so it can consume both the output of
//! the `structure_starts` stage and the official data exported by the oracle.

use std::sync::OnceLock;

use crate::{
    math::{java_floor_div, sqrt_f32},
    volume::Volume,
};

/// `StructureTemplatePool.Projection`：只有 `RIGID` 的 pool 结构件会变成 `Rigid`。
///
/// `StructureTemplatePool.Projection`: only `RIGID` pool pieces become a `Rigid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolProjection {
    Rigid,
    TerrainMatching,
}

/// `net.minecraft.world.level.levelgen.structure.TerrainAdjustment`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainAdjustment {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

impl TerrainAdjustment {
    /// 按官方 `getSerializedName()` 的名字解析。
    ///
    /// Parse by the official `getSerializedName()` string.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "bury" => Self::Bury,
            "beard_thin" => Self::BeardThin,
            "beard_box" => Self::BeardBox,
            "encapsulate" => Self::Encapsulate,
            _ => return None,
        })
    }
}

/// 官方 `BoundingBox`（六个 int）。字段名和 `structure::start::BoundingBox` 一致。
///
/// Vanilla `BoundingBox` (six ints). Field names match `structure::start::BoundingBox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Box3 {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl Box3 {
    /// `new BoundingBox(new BlockPos(x, y, z))`：单点包围盒。
    ///
    /// `new BoundingBox(new BlockPos(x, y, z))`: a single-point box.
    pub const fn point(x: i32, y: i32, z: i32) -> Self {
        Self {
            min_x: x,
            min_y: y,
            min_z: z,
            max_x: x,
            max_y: y,
            max_z: z,
        }
    }

    /// `BoundingBox.encapsulating(a, b)`：取两盒的并集。
    ///
    /// `BoundingBox.encapsulating(a, b)`: union of two boxes.
    pub fn encapsulating(a: Self, b: Self) -> Self {
        Self {
            min_x: a.min_x.min(b.min_x),
            min_y: a.min_y.min(b.min_y),
            min_z: a.min_z.min(b.min_z),
            max_x: a.max_x.max(b.max_x),
            max_y: a.max_y.max(b.max_y),
            max_z: a.max_z.max(b.max_z),
        }
    }

    /// `BoundingBox.inflatedBy(n)`（三轴都外扩）。
    ///
    /// `BoundingBox.inflatedBy(n)` (inflate all three axes).
    pub fn inflated_by(&self, n: i32) -> Self {
        Self {
            min_x: self.min_x - n,
            min_y: self.min_y - n,
            min_z: self.min_z - n,
            max_x: self.max_x + n,
            max_y: self.max_y + n,
            max_z: self.max_z + n,
        }
    }

    /// `BoundingBox.intersects(minX, minZ, maxX, maxZ)`：只比 XZ 两轴，`isCloseToChunk` 用。
    ///
    /// `BoundingBox.intersects(minX, minZ, maxX, maxZ)`: XZ-only, used by `isCloseToChunk`.
    pub fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max_x >= min_x && self.min_x <= max_x && self.max_z >= min_z && self.min_z <= max_z
    }

    /// `BoundingBox.isInside(x, y, z)`（三轴都含端点）。
    ///
    /// `BoundingBox.isInside(x, y, z)` (all three axes, inclusive).
    pub fn is_inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && z >= self.min_z
            && z <= self.max_z
            && y >= self.min_y
            && y <= self.max_y
    }

    /// `DensityVolume.intersects(box)`：`box.intersects(volume.min, volume.max)`。
    ///
    /// `DensityVolume.intersects(box)`: `box.intersects(volume.min, volume.max)`.
    pub fn intersects_volume(&self, volume: &Volume) -> bool {
        self.max_x >= volume.min_x
            && self.min_x <= volume.max_x()
            && self.max_z >= volume.min_z
            && self.min_z <= volume.max_z()
            && self.max_y >= volume.min_y
            && self.min_y <= volume.max_y()
    }
}

/// 官方 `JigsawJunction` 里 Beardifier 用到的三个字段。
///
/// The three `JigsawJunction` fields the Beardifier uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}

/// 官方 `Beardifier.Rigid` 记录。
///
/// Vanilla `Beardifier.Rigid` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rigid {
    pub box_: Box3,
    pub terrain_adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

/// 一个结构件在 Beardifier 眼里的样子。**合并接口**：结构 / jigsaw 队员把真实
/// `StructureStart` 的 `StructurePiece` 翻成这些值喂给 `for_structures_in_chunk`。
///
/// A structure piece as the Beardifier sees it. **Merge interface**: the structure/jigsaw
/// teammates translate real `StructureStart` `StructurePiece`s into these values and feed
/// them to `for_structures_in_chunk`.
#[derive(Debug, Clone)]
pub struct PieceInput {
    /// `StructurePiece.getBoundingBox()`。
    ///
    /// `StructurePiece.getBoundingBox()`.
    pub bounding_box: Box3,
    /// `StructurePiece.getGroundLevelDelta()`；非 pool 结构件按官方固定传 0。
    ///
    /// `StructurePiece.getGroundLevelDelta()`; official passes 0 for non-pool pieces.
    pub ground_level_delta: i32,
    /// pool 还是非 pool 结构件；pool 时带 projection 和全部 junctions。
    ///
    /// Pool or plain piece; a pool piece carries its projection and all junctions.
    pub kind: PieceKind,
}

/// 结构件的种类，决定 `forStructuresInChunk` 走哪条分支。
///
/// Piece kind, deciding which branch of `forStructuresInChunk` applies.
#[derive(Debug, Clone)]
pub enum PieceKind {
    /// 非 `PoolElementStructurePiece`：无条件收成一个 `Rigid(..., 0)`。
    ///
    /// Not a `PoolElementStructurePiece`: always collected as a `Rigid(..., 0)`.
    Plain,
    /// `PoolElementStructurePiece`：只有 `Rigid` projection 收成 `Rigid`，junctions 单独过滤。
    ///
    /// `PoolElementStructurePiece`: only `Rigid` projection becomes a `Rigid`; junctions filtered.
    Pool {
        projection: PoolProjection,
        junctions: Vec<Junction>,
    },
}

/// 一个带地形调整的结构起点（`terrainAdaptation() != NONE` 的 `StructureStart`）。
///
/// A structure start with terrain adjustment (a `StructureStart` whose
/// `terrainAdaptation() != NONE`).
#[derive(Debug, Clone)]
pub struct StructureStartInput {
    pub terrain_adjustment: TerrainAdjustment,
    pub pieces: Vec<PieceInput>,
}

const KERNEL_RADIUS: i32 = 12;
const KERNEL_SIZE: usize = 24 * 24 * 24;
static BEARD_KERNEL: OnceLock<[f32; KERNEL_SIZE]> = OnceLock::new();

/// `BEARD_KERNEL`：24³ 个 `e^(-dist²/16)`，构造期按官方 `computeBeardContribution` 算好。
///
/// `BEARD_KERNEL`: 24³ values of `e^(-dist²/16)`, precomputed by the official
/// `computeBeardContribution` at construction time.
fn beard_kernel() -> &'static [f32; KERNEL_SIZE] {
    BEARD_KERNEL.get_or_init(|| {
        let mut kernel = [0.0_f32; KERNEL_SIZE];
        for zi in 0..24_i32 {
            for xi in 0..24_i32 {
                for yi in 0..24_i32 {
                    let index = (zi * 24 * 24 + xi * 24 + yi) as usize;
                    kernel[index] = compute_beard_contribution(
                        xi - KERNEL_RADIUS,
                        yi - KERNEL_RADIUS,
                        zi - KERNEL_RADIUS,
                    ) as f32;
                }
            }
        }
        kernel
    })
}

/// `computeBeardContribution(int dx, int dy, int dz)` → `computeBeardContribution(dx, dy+0.5, dz)`。
///
/// `computeBeardContribution(int dx, int dy, int dz)` → `computeBeardContribution(dx, dy+0.5, dz)`.
fn compute_beard_contribution(dx: i32, dy: i32, dz: i32) -> f64 {
    compute_beard_contribution_y(dx, f64::from(dy) + 0.5, dz)
}

/// `computeBeardContribution(int dx, double dy, int dz)`：`Math.pow(Math.E, -lengthSquared/16)`。
///
/// `computeBeardContribution(int dx, double dy, int dz)`: `Math.pow(Math.E, -lengthSquared/16)`.
fn compute_beard_contribution_y(dx: i32, dy: f64, dz: i32) -> f64 {
    let dx = f64::from(dx);
    let dz = f64::from(dz);
    let distance_sqr = dx * dx + dy * dy + dz * dz;
    (-distance_sqr / 16.0_f64).exp()
}

/// `Mth.fastInvSqrt(double)`：Quake 的魔数反平方根，逐位照 26.3 字节码。
///
/// `Mth.fastInvSqrt(double)`: the Quake magic inverse square root, bit-for-bit per 26.3 bytecode.
fn fast_inv_sqrt(x: f64) -> f64 {
    let xhalf = 0.5 * x;
    let mut bits = x.to_bits() as i64;
    bits = 6_910_469_410_427_058_090_i64.wrapping_sub(bits >> 1);
    let y = f64::from_bits(bits as u64);
    y * (1.5 - xhalf * y * y)
}

/// `getBuryContribution(float dx, float dy, float dz)`。
///
/// `getBuryContribution(float dx, float dy, float dz)`.
fn get_bury_contribution(dx: f32, dy: f32, dz: f32) -> f32 {
    let distance_sq = dx * dx + dy * dy + dz * dz;
    if distance_sq >= 36.0_f32 {
        0.0_f32
    } else {
        1.0_f32 - sqrt_f32(distance_sq) / 6.0_f32
    }
}

/// `getBeardContribution(int dx, int dy, int dz, int yToGround)`。
///
/// `getBeardContribution(int dx, int dy, int dz, int yToGround)`.
fn get_beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f32 {
    let xi = dx + KERNEL_RADIUS;
    let yi = dy + KERNEL_RADIUS;
    let zi = dz + KERNEL_RADIUS;
    let in_range = |v: i32| (0..24).contains(&v);
    if in_range(xi) && in_range(yi) && in_range(zi) {
        let dy_with_offset = y_to_ground as f32 + 0.5_f32;
        let dxf = dx as f32;
        let dzf = dz as f32;
        let distance_sqr = dxf * dxf + dy_with_offset * dy_with_offset + dzf * dzf;
        let value = -dy_with_offset * (fast_inv_sqrt(f64::from(distance_sqr / 2.0_f32)) as f32)
            / 2.0_f32;
        value * beard_kernel()[(zi * 24 * 24 + xi * 24 + yi) as usize]
    } else {
        0.0_f32
    }
}

fn include_box(encompassing: Option<Box3>, new_box: Box3) -> Box3 {
    match encompassing {
        None => new_box,
        Some(current) => Box3::encapsulating(current, new_box),
    }
}

/// 官方 `Beardifier` 的等价物。
///
/// The Rust counterpart of vanilla `Beardifier`.
#[derive(Debug, Clone)]
pub struct Beardifier {
    pieces: Vec<Rigid>,
    junctions: Vec<Junction>,
    affected_box: Option<Box3>,
}

impl Beardifier {
    /// `Beardifier.EMPTY`：没有结构件，`sampleValue` 恒 0。
    ///
    /// `Beardifier.EMPTY`: no pieces, `sampleValue` is always 0.
    pub const fn empty() -> Self {
        Self {
            pieces: Vec::new(),
            junctions: Vec::new(),
            affected_box: None,
        }
    }

    /// 用已经收好的 `Rigid` 和 junction 构造；照 `forStructuresInChunk` 的尾部算 `affectedBox`。
    ///
    /// Build from already-collected `Rigid`s and junctions; computes `affectedBox` like the
    /// tail of `forStructuresInChunk`.
    pub fn from_parts(pieces: Vec<Rigid>, junctions: Vec<Junction>) -> Self {
        let mut any: Option<Box3> = None;
        for rigid in &pieces {
            any = Some(include_box(any, rigid.box_));
        }
        for junction in &junctions {
            any = Some(include_box(
                any,
                Box3::point(
                    junction.source_x,
                    junction.source_ground_y,
                    junction.source_z,
                ),
            ));
        }
        let affected_box = any.map(|b| b.inflated_by(24));
        Self {
            pieces,
            junctions,
            affected_box,
        }
    }

    /// 官方 `Beardifier.forStructuresInChunk` 的逐行移植。
    ///
    /// 入参是与区块相交、且 `terrainAdaptation() != NONE` 的结构起点视图；按官方顺序做
    /// `isCloseToChunk(12)`、RIGID projection、junctions 的 `chunk±12` 过滤，并集再 `inflatedBy(24)`。
    ///
    /// Line-by-line port of vanilla `Beardifier.forStructuresInChunk`. The input is the view of
    /// structure starts intersecting the chunk with `terrainAdaptation() != NONE`; it applies
    /// `isCloseToChunk(12)`, RIGID projection, the `chunk±12` junction filter, unions the boxes
    /// and `inflatedBy(24)`, all in the official order.
    pub fn for_structures_in_chunk(chunk_x: i32, chunk_z: i32, starts: &[StructureStartInput]) -> Self {
        let min_x = chunk_x * 16;
        let min_z = chunk_z * 16;
        let mut rigids: Vec<Rigid> = Vec::new();
        let mut junctions: Vec<Junction> = Vec::new();
        for start in starts {
            for piece in &start.pieces {
                if !piece
                    .bounding_box
                    .intersects_xz(min_x - 12, min_z - 12, min_x + 27, min_z + 27)
                {
                    continue;
                }
                match &piece.kind {
                    PieceKind::Pool {
                        projection,
                        junctions: piece_junctions,
                    } => {
                        if *projection == PoolProjection::Rigid {
                            rigids.push(Rigid {
                                box_: piece.bounding_box,
                                terrain_adjustment: start.terrain_adjustment,
                                ground_level_delta: piece.ground_level_delta,
                            });
                        }
                        for junction in piece_junctions {
                            if junction.source_x > min_x - 12
                                && junction.source_z > min_z - 12
                                && junction.source_x < min_x + 27
                                && junction.source_z < min_z + 27
                            {
                                junctions.push(*junction);
                            }
                        }
                    }
                    PieceKind::Plain => rigids.push(Rigid {
                        box_: piece.bounding_box,
                        terrain_adjustment: start.terrain_adjustment,
                        ground_level_delta: 0,
                    }),
                }
            }
        }
        Self::from_parts(rigids, junctions)
    }

    /// 官方 `Beardifier.sampleValue`：在 `affectedBox` 内逐点算，否则 0。
    ///
    /// Vanilla `Beardifier.sampleValue`: computed inside `affectedBox`, else 0.
    pub fn sample_value(&self, x: i32, y: i32, z: i32) -> f32 {
        match &self.affected_box {
            Some(b) if b.is_inside(x, y, z) => self.sample_value_unchecked(x, y, z),
            _ => 0.0_f32,
        }
    }

    /// 官方 `Beardifier.sampleVolume`（`fillArray`）：先整块填 0，再只算与 `affectedBox`
    /// 相交的子区域，循环顺序 z → x → y。
    ///
    /// Vanilla `Beardifier.sampleVolume` (`fillArray`): fill 0, then compute only the sub-region
    /// intersecting `affectedBox`, iterating z → x → y.
    pub fn sample_volume(&self, out: &mut [f32], volume: &Volume) {
        out.fill(0.0_f32);
        let Some(affected) = &self.affected_box else {
            return;
        };
        if !affected.intersects_volume(volume) {
            return;
        }
        let min_x = java_floor_div((affected.min_x.wrapping_sub(volume.min_x)).max(0), volume.step_x);
        let min_y = java_floor_div((affected.min_y.wrapping_sub(volume.min_y)).max(0), volume.step_y);
        let min_z = java_floor_div((affected.min_z.wrapping_sub(volume.min_z)).max(0), volume.step_z);
        let max_x = (volume.size_x - 1).min(java_floor_div(
            affected.max_x.wrapping_sub(volume.min_x),
            volume.step_x,
        ));
        let max_y = (volume.size_y - 1).min(java_floor_div(
            affected.max_y.wrapping_sub(volume.min_y),
            volume.step_y,
        ));
        let max_z = (volume.size_z - 1).min(java_floor_div(
            affected.max_z.wrapping_sub(volume.min_z),
            volume.step_z,
        ));
        for z in min_z..=max_z {
            let block_z = volume.block_z(z);
            for x in min_x..=max_x {
                let block_x = volume.block_x(x);
                for y in min_y..=max_y {
                    let index = volume.index(x, y, z);
                    let block_y = volume.block_y(y);
                    out[index] = self.sample_value_unchecked(block_x, block_y, block_z);
                }
            }
        }
    }

    /// 官方 `Beardifier.sampleValueUnchecked`。
    ///
    /// Vanilla `Beardifier.sampleValueUnchecked`.
    fn sample_value_unchecked(&self, x: i32, y: i32, z: i32) -> f32 {
        let mut noise = 0.0_f32;
        for rigid in &self.pieces {
            let b = rigid.box_;
            let dx = (b.min_x.wrapping_sub(x))
                .max(x.wrapping_sub(b.max_x))
                .max(0);
            let dz = (b.min_z.wrapping_sub(z))
                .max(z.wrapping_sub(b.max_z))
                .max(0);
            let ground_y = b.min_y + rigid.ground_level_delta;
            let dy_to_ground = y - ground_y;
            let dy = match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => dy_to_ground,
                TerrainAdjustment::BeardBox => (ground_y - y).max(y - b.max_y).max(0),
                TerrainAdjustment::Encapsulate => (b.min_y - y).max(y - b.max_y).max(0),
            };
            noise += match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0.0_f32,
                TerrainAdjustment::Bury => get_bury_contribution(
                    dx as f32,
                    dy as f32 / 2.0_f32,
                    dz as f32,
                ),
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    get_beard_contribution(dx, dy, dz, dy_to_ground) * 0.8_f32
                }
                TerrainAdjustment::Encapsulate => {
                    get_bury_contribution(
                        dx as f32 / 2.0_f32,
                        dy as f32 / 2.0_f32,
                        dz as f32 / 2.0_f32,
                    ) * 0.8_f32
                }
            };
        }
        for junction in &self.junctions {
            let dx = x - junction.source_x;
            let dy = y - junction.source_ground_y;
            let dz = z - junction.source_z;
            noise += get_beard_contribution(dx, dy, dz, dy) * 0.4_f32;
        }
        noise
    }
}
