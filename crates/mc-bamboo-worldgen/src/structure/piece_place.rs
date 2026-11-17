//! 官方 `StructurePiece` 的放置辅助函数（`placeBlock` / `generateBox` / `getWorldPos` 等）。
//!
//! 这一层只做「把相对坐标换成世界坐标、按写入范围过滤、写方块」，把世界读写抽象成
//! [`PieceLevel`]：官方对应 `WorldGenLevel` / `WorldGenRegion`。地物那边已有
//! `feature::FeatureWorld` 这样的 3×3 视图，但结构件的写入范围由 `BoundingBox chunkBB`
//! 决定（`StructureStart.placeInChunk` 传的是当前区块的盒），和地物的写半径不同，
//! 所以这里单独定义 trait，由调用方给实现。
//!
//! 逐条照官方翻译（`StructurePiece.java` 200–500 行）：
//! * `getWorldPos` / `getWorldX` / `getWorldY` / `getWorldZ`：朝向变换；
//! * `getBlock`：`chunkBB` 外返回空气；
//! * `placeBlock`：`chunkBB.isInside` + `canBeReplaced` + 写方块（不处理镜像 / 旋转，
//!   那两种状态变换留给调用方按方块类型自己算）；
//! * `generateBox` 三个重载、`generateAirBox`、`generateMaybeBox`、`maybeGenerateBlock`、
//!   `generateUpperHalfSphere`、`fillColumnDown`、`isInterior`、
//!   `isReplaceableByStructures`。
//!
//! Vanilla `StructurePiece` placement helpers (`placeBlock` / `generateBox` /
//! `getWorldPos` ...). The layer only turns relative coordinates into world ones,
//! filters by the write area and writes blocks; the world is abstracted by
//! [`PieceLevel`] (vanilla `WorldGenLevel` / `WorldGenRegion`). `feature::FeatureWorld`
//! is a 3×3 view for features, but a structure piece's write area comes from the
//! `chunkBB` passed to `placeInChunk`, so the world is a trait here and the caller
//! supplies it.

use mc_bamboo_data::{block_by_name, facts, state_facts};

use super::pieces::{Box, HDir};

/// 官方 `Direction` 的六个方向，顺序就是 `get3DDataValue()`（DOWN=0、UP=1、NORTH=2、
/// SOUTH=3、WEST=4、EAST=5）。方块事实表的 `isFaceSturdy` 位按这个顺序排。
///
/// The six vanilla `Direction` values in `get3DDataValue()` order (DOWN=0, UP=1,
/// NORTH=2, SOUTH=3, WEST=4, EAST=5). The block fact table orders its
/// `isFaceSturdy` bits this way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    /// `Direction.get3DDataValue()`。
    pub fn id(self) -> i32 {
        match self {
            Self::Down => 0,
            Self::Up => 1,
            Self::North => 2,
            Self::South => 3,
            Self::West => 4,
            Self::East => 5,
        }
    }

    /// `Direction.getOpposite()`。
    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

/// 结构件放方块需要的世界读写，对应官方 `WorldGenLevel`（实际是 `WorldGenRegion`）。
///
/// World access needed by piece placement; vanilla `WorldGenLevel` (really
/// `WorldGenRegion`).
pub trait PieceLevel {
    /// `LevelReader.getBlockState`。构建高度之外返回空气；水平方向上**不受写区限制**，
    /// `WorldGenRegion` 读写区外的区块照样返回真实方块（`reorient`、支撑柱都会读到邻区块）。
    ///
    /// `LevelReader.getBlockState`. Air outside the build height; horizontally it is
    /// **not limited to the write zone**: `WorldGenRegion` returns real blocks for chunks
    /// outside it (`reorient` and the support pillars do read neighbouring chunks).
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32;
    /// `LevelWriter.setBlock(pos, state, flags)`。
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool;
    /// `WorldGenRegion.getBlockEntity(...)` 之后写进方块实体的 nbt。默认丢弃；只有
    /// 收集方块实体的调用方（`FeaturePieceLevel`）需要实现。
    ///
    /// The nbt written to a block entity after `WorldGenRegion.getBlockEntity(...)`.
    /// The default drops it; only `FeaturePieceLevel` collects it.
    fn set_block_entity(&mut self, _x: i32, _y: i32, _z: i32, _nbt: &mc_bamboo_nbt::Tag) {}
    /// `LevelReader.getMinY()`。
    fn min_y(&self) -> i32;
    /// `LevelReader.getMaxY()`。
    fn max_y(&self) -> i32;
    /// `LevelReader.getHeight(OCEAN_FLOOR_WG, x, z)`。
    ///
    /// 必须是地形阶段结束时**冻结**的那张高度图：26.3 里 FEATURES 阶段区块的持久状态是
    /// TERRAIN，它的 `heightmapsAfter` 只有 FINAL 那一组，所以结构件和地物写方块时
    /// `OCEAN_FLOOR_WG` 不再更新。实现方不能按当前方块现算。
    ///
    /// `LevelReader.getHeight(OCEAN_FLOOR_WG, x, z)`. This must be the heightmap as
    /// **frozen** at the end of the terrain stage: in 26.3 a chunk's persisted status
    /// during FEATURES is TERRAIN, whose `heightmapsAfter` holds only the FINAL set, so
    /// `OCEAN_FLOOR_WG` is no longer updated when pieces and features write blocks.
    /// Implementations must not recompute it from the current blocks.
    fn ocean_floor_height(&self, x: i32, z: i32) -> i32;
    /// `LevelAccessor.getRandom()`：`WorldGenRegion` 自己的位置随机源，和传给
    /// `postProcess` 的 `random` 是两回事（林地府邸的悦灵数量从这里取）。
    ///
    /// `LevelAccessor.getRandom()`: the `WorldGenRegion`'s own positional source, separate
    /// from the `random` passed to `postProcess` (the mansion's allay count draws from it).
    fn level_random(&self) -> &crate::random::WorldgenRandom;
}

/// `BaseSpawner` 的默认记录（`Delay` / `SpawnPotentials` / 各计时字段）加上
/// `setEntityId` 写入的 `SpawnData.entity.id`。`WeightedList.of()` 是空表，
/// `getOrCreateNextSpawnData` 在没有 potential 时造一份空的 `SpawnData`。
///
/// The default `BaseSpawner` record plus the `SpawnData.entity.id` written by
/// `setEntityId`. `WeightedList.of()` is empty, so `getOrCreateNextSpawnData`
/// creates an empty `SpawnData` when no potential exists.
pub fn spawner_nbt(entity: &str) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    Tag::Compound(vec![
        ("Delay".to_owned(), Tag::Short(20)),
        ("MaxNearbyEntities".to_owned(), Tag::Short(6)),
        ("MaxSpawnDelay".to_owned(), Tag::Short(800)),
        ("MinSpawnDelay".to_owned(), Tag::Short(200)),
        ("RequiredPlayerRange".to_owned(), Tag::Short(16)),
        ("SpawnCount".to_owned(), Tag::Short(4)),
        (
            "SpawnData".to_owned(),
            Tag::Compound(vec![(
                "entity".to_owned(),
                Tag::Compound(vec![("id".to_owned(), Tag::String(entity.to_owned()))]),
            )]),
        ),
        ("SpawnPotentials".to_owned(), Tag::List(Vec::new())),
        ("SpawnRange".to_owned(), Tag::Short(4)),
    ])
}

/// `BoundingBox.isInside(pos)`：三轴都含端点。
///
/// `BoundingBox.isInside(pos)`: all three axes inclusive.
pub fn box_is_inside(bb: &Box, x: i32, y: i32, z: i32) -> bool {
    x >= bb[0] && x <= bb[3] && y >= bb[1] && y <= bb[4] && z >= bb[2] && z <= bb[5]
}

/// 结构件放置的上下文：自己那一条 `StructurePiece` 的状态。
///
/// Placement context of one `StructurePiece`: its own state.
pub struct PieceCtx {
    /// 结构件自己的包围盒（`StructurePiece.boundingBox`）。
    pub bb: Box,
    /// `StructurePiece.getOrientation()`；`None` 对应官方 null（NBT `O = -1`）。
    pub orientation: Option<HDir>,
    /// `StructurePiece.mirror` / `rotation`，由 `setOrientation` 推出。
    ///
    /// `StructurePiece.mirror` / `rotation`, derived by `setOrientation`.
    pub mirror: crate::structure::state_transform::Mirror,
    pub rotation: crate::structure::state_transform::Rotation,
    /// `StructurePiece.canBeReplaced(...)` 子类覆写；`None` 用基类默认（恒 true）。
    ///
    /// The subclass `canBeReplaced` override; `None` is the base default (always true).
    pub can_replace: Option<fn(u32) -> bool>,
}

impl PieceCtx {
    pub fn new(bb: Box, orientation: Option<HDir>) -> Self {
        let (mirror, rotation) = crate::structure::state_transform::from_orientation(orientation);
        Self {
            bb,
            orientation,
            mirror,
            rotation,
            can_replace: None,
        }
    }

    /// 带上子类的 `canBeReplaced` 覆写。
    ///
    /// Attach a subclass `canBeReplaced` override.
    pub fn with_can_replace(mut self, can_replace: fn(u32) -> bool) -> Self {
        self.can_replace = Some(can_replace);
        self
    }

    /// `StructurePiece.getWorldX(x, z)`。
    ///
    /// `StructurePiece.getWorldX(x, z)`.
    pub fn world_x(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => x,
            Some(HDir::North | HDir::South) => self.bb[0] + x,
            Some(HDir::West) => self.bb[3] - z,
            Some(HDir::East) => self.bb[0] + z,
        }
    }

    /// `StructurePiece.getWorldY(y)`。
    ///
    /// `StructurePiece.getWorldY(y)`.
    pub fn world_y(&self, y: i32) -> i32 {
        match self.orientation {
            None => y,
            Some(_) => y + self.bb[1],
        }
    }

    /// `StructurePiece.getWorldZ(x, z)`。
    ///
    /// `StructurePiece.getWorldZ(x, z)`.
    pub fn world_z(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => z,
            Some(HDir::North) => self.bb[5] - z,
            Some(HDir::South) => self.bb[2] + z,
            Some(HDir::West | HDir::East) => self.bb[2] + x,
        }
    }

    /// `StructurePiece.getWorldPos(x, y, z)`。
    ///
    /// `StructurePiece.getWorldPos(x, y, z)`.
    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        (self.world_x(x, z), self.world_y(y), self.world_z(x, z))
    }

    /// `StructurePiece.getBlock(level, x, y, z, chunkBB)`：`chunkBB` 外返回空气。
    ///
    /// `StructurePiece.getBlock(level, x, y, z, chunkBB)`: air outside `chunkBB`.
    pub fn get_block(&self, level: &dyn PieceLevel, chunk_bb: &Box, x: i32, y: i32, z: i32) -> u32 {
        let (wx, wy, wz) = self.world_pos(x, y, z);
        if !box_is_inside(chunk_bb, wx, wy, wz) {
            return mc_bamboo_data::blocks::AIR;
        }
        level.get_block(wx, wy, wz)
    }

    /// `StructurePiece.placeBlock(level, state, x, y, z, chunkBB)`。
    ///
    /// 顺序照官方：`isInside` → `canBeReplaced` → `mirror` → `rotate` → `setBlock`。
    ///
    /// `StructurePiece.placeBlock(level, state, x, y, z, chunkBB)`. Order follows
    /// vanilla: `isInside` → `canBeReplaced` → `mirror` → `rotate` → `setBlock`.
    pub fn place_block(
        &self,
        level: &mut dyn PieceLevel,
        state: u32,
        x: i32,
        y: i32,
        z: i32,
        chunk_bb: &Box,
    ) {
        let (wx, wy, wz) = self.world_pos(x, y, z);
        if !box_is_inside(chunk_bb, wx, wy, wz) {
            return;
        }
        if let Some(can_replace) = self.can_replace
            && !can_replace(level.get_block(wx, wy, wz))
        {
            return;
        }
        let state = crate::structure::state_transform::transform(state, self.mirror, self.rotation);
        level.set_block(wx, wy, wz, state);
    }
}

/// 一个 `BlockSelector`：官方 `StructurePiece.BlockSelector`，按「是不是边缘」选方块。
///
/// A `BlockSelector`: vanilla `StructurePiece.BlockSelector`, picking a block from
/// whether the position is on the box edge.
pub trait BlockSelector {
    /// `BlockSelector.next(random, x, y, z, edge)`。
    fn next(&mut self, random: &crate::random::WorldgenRandom, x: i32, y: i32, z: i32, edge: bool);
    /// `BlockSelector.getNext()`。
    fn get_next(&self) -> u32;
}

/// `StructurePiece.generateAirBox`。
///
/// `StructurePiece.generateAirBox`.
#[allow(clippy::too_many_arguments)]
pub fn generate_air_box(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) {
    let air = mc_bamboo_data::blocks::AIR;
    generate_box(
        ctx,
        level,
        chunk_bb,
        [x0, y0, z0, x1, y1, z1],
        air,
        air,
        false,
    );
}

/// `StructurePiece.generateBox(level, chunkBB, x0, y0, z0, x1, y1, z1, edge, fill, skipAir)`。
///
/// 循环顺序必须是 `y` 外层、`x`、`z` 内层（官方就是这三重 for 的顺序）；
/// `skipAir` 会让 `getBlock` 参与判定，因此也影响后续 `getBlock` 的观察结果。
///
/// `StructurePiece.generateBox(...)`. The loop order must be `y` outer, then `x`,
/// then `z` (vanilla's three nested loops); `skipAir` consults `getBlock`, which
/// therefore depends on earlier writes.
#[allow(clippy::too_many_arguments)]
pub fn generate_box(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    range: Box,
    edge_block: u32,
    fill_block: u32,
    skip_air: bool,
) {
    let [x0, y0, z0, x1, y1, z1] = range;
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if skip_air && is_air(ctx.get_block(level, chunk_bb, x, y, z)) {
                    continue;
                }
                let interior = y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1;
                let block = if interior { fill_block } else { edge_block };
                ctx.place_block(level, block, x, y, z, chunk_bb);
            }
        }
    }
}

/// `StructurePiece.generateBox(..., skipAir, random, selector)`。
///
/// `StructurePiece.generateBox(..., skipAir, random, selector)`.
pub fn generate_box_selected(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    range: Box,
    skip_air: bool,
    random: &crate::random::WorldgenRandom,
    selector: &mut dyn BlockSelector,
) {
    let [x0, y0, z0, x1, y1, z1] = range;
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if skip_air && is_air(ctx.get_block(level, chunk_bb, x, y, z)) {
                    continue;
                }
                let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                selector.next(random, x, y, z, edge);
                let block = selector.get_next();
                ctx.place_block(level, block, x, y, z, chunk_bb);
            }
        }
    }
}

/// `StructurePiece.isInterior(level, x, y, z, chunkBB)`。
///
/// `StructurePiece.isInterior(level, x, y, z, chunkBB)`.
pub fn is_interior(
    ctx: &PieceCtx,
    level: &dyn PieceLevel,
    x: i32,
    y: i32,
    z: i32,
    chunk_bb: &Box,
) -> bool {
    let (wx, wy, wz) = ctx.world_pos(x, y + 1, z);
    if !box_is_inside(chunk_bb, wx, wy, wz) {
        return false;
    }
    wy < level.ocean_floor_height(wx, wz)
}

/// `StructurePiece.generateMaybeBox`：每个位置先取一次 `nextFloat()`。
///
/// `StructurePiece.generateMaybeBox`: one `nextFloat()` per position.
#[allow(clippy::too_many_arguments)]
pub fn generate_maybe_box(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    random: &crate::random::WorldgenRandom,
    probability: f32,
    range: Box,
    edge_block: u32,
    fill_block: u32,
    skip_air: bool,
    has_to_be_inside: bool,
) {
    let [x0, y0, z0, x1, y1, z1] = range;
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if random.next_float() > probability {
                    continue;
                }
                if skip_air && is_air(ctx.get_block(level, chunk_bb, x, y, z)) {
                    continue;
                }
                if has_to_be_inside && !is_interior(ctx, level, x, y, z, chunk_bb) {
                    continue;
                }
                let interior = y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1;
                let block = if interior { fill_block } else { edge_block };
                ctx.place_block(level, block, x, y, z, chunk_bb);
            }
        }
    }
}

/// `StructurePiece.maybeGenerateBlock`。
///
/// `StructurePiece.maybeGenerateBlock`.
#[allow(clippy::too_many_arguments)]
pub fn maybe_generate_block(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    random: &crate::random::WorldgenRandom,
    probability: f32,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
) {
    if random.next_float() < probability {
        ctx.place_block(level, state, x, y, z, chunk_bb);
    }
}

/// `StructurePiece.generateUpperHalfSphere`。
///
/// 浮点运算顺序照官方：先算 `diagX/diagY/diagZ` 与中心 `cx/cz`，三重循环 `y/x/z`，
/// 距离 `<= 1.05F` 才放。`normalizedYDistance` 用 `diagY`，X/Z 用各自的半宽。
///
/// `StructurePiece.generateUpperHalfSphere`. Float order follows vanilla: compute
/// `diagX/diagY/diagZ` and the `cx/cz` centres, loop `y/x/z`, place when the
/// squared distance is `<= 1.05F`.
#[allow(clippy::too_many_arguments)]
pub fn generate_upper_half_sphere(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    range: Box,
    fill_block: u32,
    skip_air: bool,
) {
    let [x0, y0, z0, x1, y1, z1] = range;
    let diag_x = (x1 - x0 + 1) as f32;
    let diag_y = (y1 - y0 + 1) as f32;
    let diag_z = (z1 - z0 + 1) as f32;
    let cx = x0 as f32 + diag_x / 2.0;
    let cz = z0 as f32 + diag_z / 2.0;

    for y in y0..=y1 {
        let normalized_y = (y - y0) as f32 / diag_y;
        for x in x0..=x1 {
            let normalized_x = (x as f32 - cx) / (diag_x * 0.5);
            for z in z0..=z1 {
                let normalized_z = (z as f32 - cz) / (diag_z * 0.5);
                if skip_air && is_air(ctx.get_block(level, chunk_bb, x, y, z)) {
                    continue;
                }
                let distance = normalized_x * normalized_x
                    + normalized_y * normalized_y
                    + normalized_z * normalized_z;
                if distance <= 1.05 {
                    ctx.place_block(level, fill_block, x, y, z, chunk_bb);
                }
            }
        }
    }
}

/// `StructurePiece.fillColumnDown`：自 `startY` 往下，只要「结构可替换」就一直写。
///
/// `StructurePiece.fillColumnDown`: from `startY` downwards, keep writing while the
/// block is replaceable by structures.
pub fn fill_column_down(
    ctx: &PieceCtx,
    level: &mut dyn PieceLevel,
    chunk_bb: &Box,
    state: u32,
    x: i32,
    start_y: i32,
    z: i32,
) {
    let (wx, _, wz) = ctx.world_pos(x, start_y, z);
    let mut wy = ctx.world_y(start_y);
    if !box_is_inside(chunk_bb, wx, wy, wz) {
        return;
    }
    while is_replaceable_by_structures(level.get_block(wx, wy, wz)) && wy > level.min_y() + 1 {
        level.set_block(wx, wy, wz, state);
        wy -= 1;
    }
}

/// `StructurePiece.isReplaceableByStructures`。
///
/// `StructurePiece.isReplaceableByStructures`.
pub fn is_replaceable_by_structures(state: u32) -> bool {
    let facts = state_facts(state);
    if facts & facts::AIR != 0 || facts & facts::LIQUID != 0 {
        return true;
    }
    matches!(
        block_name_of(state),
        Some("minecraft:glow_lichen" | "minecraft:seagrass" | "minecraft:tall_seagrass")
    )
}

/// 状态 ID 的方块名；查不到返回 `None`。
///
/// Block name of a state id; `None` when unknown.
fn block_name_of(state: u32) -> Option<&'static str> {
    let id = mc_bamboo_data::block_id_of_state(state)?;
    Some(mc_bamboo_data::blocks::BLOCKS[id].name)
}

/// `BlockState.isAir()`。
///
/// `BlockState.isAir()`.
pub fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `BlockState.liquid()`：方块属性里的标记，不是「含有流体」。
///
/// `BlockState.liquid()`: the block property, not "contains fluid".
pub fn is_liquid(state: u32) -> bool {
    state_facts(state) & facts::LIQUID != 0
}

/// `BlockState.is(Blocks.LAVA)`。
///
/// `BlockState.is(Blocks.LAVA)`.
pub fn is_lava(state: u32) -> bool {
    is_block(state, "minecraft:lava")
}

/// `BlockState.isSolidRender()`。
///
/// `BlockState.isSolidRender()`.
pub fn is_solid_render(state: u32) -> bool {
    state_facts(state) & facts::SOLID_RENDER != 0
}

/// 方块是不是给定的那个（不看属性）。
///
/// Whether the state belongs to the named block, ignoring properties.
pub fn is_block(state: u32, name: &str) -> bool {
    let Some(want) = block_by_name(name) else {
        return false;
    };
    mc_bamboo_data::block_id_of_state(state)
        == mc_bamboo_data::block_id_of_state(want.default_state)
}

/// `BlockState.isFaceSturdy(level, pos, direction)`：`FULL` 那一组位。
///
/// `BlockState.isFaceSturdy(level, pos, direction)`: the `FULL` bits.
pub fn is_face_sturdy_full(state: u32, dir: Dir) -> bool {
    let bit = 1_u64 << (facts::STURDY_FULL + dir.id() as u32);
    state_facts(state) & bit != 0
}

/// `BlockState.isFaceSturdy(level, pos, direction, SupportType.CENTER)`：`CENTER` 那一组位。
///
/// `BlockState.isFaceSturdy(level, pos, direction, SupportType.CENTER)`: the `CENTER` bits.
pub fn is_face_sturdy_center(state: u32, dir: Dir) -> bool {
    let bit = 1_u64 << (facts::STURDY_CENTER + dir.id() as u32);
    state_facts(state) & bit != 0
}

/// `Block.canSupportCenter(level, posAbove, DOWN) && !(block instanceof FallingBlock)`：
/// 上方能挂住铁链。
///
/// `canSupportCenter` 对 DOWN 先排除 `UNSTABLE_BOTTOM_CENTER` 标签，再看
/// `isFaceSturdy(DOWN, SupportType.CENTER)`（不是 `FULL`：栅栏、墙、铁栏杆这类非满方块也
/// 撑得住中心）。`FallingBlock` 按方块类的祖先表判定，包括沙、沙砾、混凝土粉末、铁砧、龙蛋。
///
/// `Block.canSupportCenter(level, posAbove, DOWN) && !(block instanceof FallingBlock)`:
/// a chain can hang below. For DOWN `canSupportCenter` first rejects the
/// `UNSTABLE_BOTTOM_CENTER` tag and then tests `isFaceSturdy(DOWN, SupportType.CENTER)`
/// (not `FULL`: fences, walls and iron bars support the centre too). `FallingBlock` is
/// decided from the block class ancestor table, covering sand, gravel, concrete powder,
/// anvils and the dragon egg.
pub fn can_hang_chain_below(state: u32) -> bool {
    if mc_bamboo_blocks::BlockState(state)
        .is_tagged(mc_bamboo_data::tags::block_tags::UNSTABLE_BOTTOM_CENTER)
    {
        return false;
    }
    if !is_face_sturdy_center(state, Dir::Down) {
        return false;
    }
    !mc_bamboo_data::block_of_state(state).is_some_and(|block| {
        block
            .class
            .ancestors()
            .contains(&mc_bamboo_data::BlockAncestor::FallingBlock)
    })
}

/// 按名字取方块的默认状态，找不到就 panic（对拍里立刻暴露）。
///
/// Default state of a block by name; panics when the name is unknown (surfaces
/// immediately in the oracle).
pub fn block(name: &str) -> u32 {
    block_by_name(name)
        .unwrap_or_else(|| panic!("unknown block {name}"))
        .default_state
}

/// 返回把某个属性改成给定值的状态 ID，属性名或值不存在时返回 `None`。
///
/// A state with one property set to the given value, or `None` if absent.
pub fn with_property(state: u32, name: &str, value: &str) -> Option<u32> {
    crate::feature::state::with_property(state, name, value)
}

/// 读一个方块状态上的属性值，找不到返回 `None`。
///
/// Read one property of a block state; `None` when it is absent.
pub fn property_of_state(state: u32, key: &str) -> Option<&'static str> {
    crate::structure::state_transform::property_of(state, key)
}

/// `StructurePiece.reorient(level, pos, chestDefault)`：按四周的实体方块决定箱子的朝向。
///
/// 逐条照官方（`StructurePiece.java` 401 行）：
/// 1. 沿 `Direction.Plane.HORIZONTAL`（NORTH、EAST、SOUTH、WEST）数邻居；
///    邻居已经是 `CHEST` 就原样返回；邻居 `isSolidRender` 时记成唯一实体邻居，
///    出现第二个就作废；
/// 2. 有唯一实体邻居时 `facing = 该邻居的反方向`；
/// 3. 否则从当前 `facing` 出发，依次看正前方、反方向、顺时针、反方向，
///    第一个 `isSolidRender` 的方向就是 `facing`。
///
/// `StructurePiece.reorient(level, pos, chestDefault)`: pick a chest's facing from the
/// solid blocks around it. Translated from `StructurePiece.java:401`: walk
/// `Direction.Plane.HORIZONTAL` (NORTH, EAST, SOUTH, WEST) and return the default state
/// when a neighbour is already a `CHEST`; record a single solid neighbour and drop it when
/// a second appears. With a unique solid neighbour, `facing` points away from it;
/// otherwise the current `facing` is probed forward, backward, clockwise, backward and the
/// first solid direction wins.
pub fn reorient(level: &dyn PieceLevel, pos: (i32, i32, i32), chest_default: u32) -> u32 {
    reorient_with(|x, y, z| level.get_block(x, y, z), pos, chest_default)
}

/// `reorient` 的纯函数版本：邻居方块由闭包给出，不要求实现完整的 [`PieceLevel`]。
///
/// Pure form of `reorient`: neighbours come from a closure, no full [`PieceLevel`] needed.
pub fn reorient_with(
    block_at: impl Fn(i32, i32, i32) -> u32,
    pos: (i32, i32, i32),
    chest_default: u32,
) -> u32 {
    const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];
    let mut solid: Option<&'static str> = None;
    for name in HORIZONTAL {
        let neighbor = neighbor_towards(&block_at, pos, name);
        if is_block(neighbor, "minecraft:chest") {
            return chest_default;
        }
        if is_solid_render(neighbor) {
            if solid.is_some() {
                solid = None;
                break;
            }
            solid = Some(name);
        }
    }

    if let Some(name) = solid {
        return with_property(chest_default, "facing", opposite_facing(name))
            .unwrap_or(chest_default);
    }

    // 官方后半段是三段**顺序**探测，每段只在上一段命中实心块时改变方向：
    // 正前命中则反向，再命中则顺时针，再命中则反向（每次都用改变后的方向读邻居）。
    // Vanilla's second half is a sequential chain: each probe only changes the direction
    // when its neighbour is solid, and the next probe reads the neighbour of the new
    // direction.
    let mut facing =
        crate::structure::state_transform::property_of(chest_default, "facing").unwrap_or("north");
    if is_solid_render(neighbor_towards(&block_at, pos, facing)) {
        facing = opposite_facing(facing);
    }
    if is_solid_render(neighbor_towards(&block_at, pos, facing)) {
        facing = clock_wise_facing(facing);
    }
    if is_solid_render(neighbor_towards(&block_at, pos, facing)) {
        facing = opposite_facing(facing);
    }
    with_property(chest_default, "facing", facing).unwrap_or(chest_default)
}

/// 水平四方向的反方向。
///
/// The opposite of a horizontal facing.
fn opposite_facing(facing: &str) -> &'static str {
    match facing {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        _ => "east",
    }
}

/// 水平四方向的顺时针下一个。
///
/// The clockwise neighbour of a horizontal facing.
fn clock_wise_facing(facing: &str) -> &'static str {
    match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

/// 朝 `facing` 走一格后那格的方块。
///
/// The block one step along `facing`.
fn neighbor_towards(
    block_at: &impl Fn(i32, i32, i32) -> u32,
    pos: (i32, i32, i32),
    facing: &str,
) -> u32 {
    let (dx, dz) = match facing {
        "north" => (0, -1),
        "south" => (0, 1),
        "east" => (1, 0),
        _ => (-1, 0),
    };
    block_at(pos.0 + dx, pos.1, pos.2 + dz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 一个测试用的一格世界。
    ///
    /// A one-block test world.
    struct TestLevel {
        blocks: BTreeMap<(i32, i32, i32), u32>,
        random: crate::random::WorldgenRandom,
    }

    impl TestLevel {
        fn new() -> Self {
            Self {
                blocks: BTreeMap::new(),
                random: crate::structure::pieces::make_random(0, 0, 0),
            }
        }
    }

    impl PieceLevel for TestLevel {
        fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
            self.blocks
                .get(&(x, y, z))
                .copied()
                .unwrap_or(mc_bamboo_data::blocks::AIR)
        }

        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
            self.blocks.insert((x, y, z), state);
            true
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn max_y(&self) -> i32 {
            319
        }

        fn ocean_floor_height(&self, _x: i32, _z: i32) -> i32 {
            0
        }

        fn level_random(&self) -> &crate::random::WorldgenRandom {
            &self.random
        }
    }

    #[test]
    fn world_pos_follows_the_orientation() {
        let bb = [10, 50, 20, 18, 55, 27];
        let north = PieceCtx::new(bb, Some(HDir::North));
        assert_eq!(north.world_pos(1, 2, 3), (11, 52, 24));
        let south = PieceCtx::new(bb, Some(HDir::South));
        assert_eq!(south.world_pos(1, 2, 3), (11, 52, 23));
        let west = PieceCtx::new(bb, Some(HDir::West));
        assert_eq!(west.world_pos(1, 2, 3), (15, 52, 21));
        let east = PieceCtx::new(bb, Some(HDir::East));
        assert_eq!(east.world_pos(1, 2, 3), (13, 52, 21));
        // `None` 就是官方 null 朝向：坐标原样返回。
        // `None` is vanilla's null orientation: coordinates pass through.
        let none = PieceCtx::new(bb, None);
        assert_eq!(none.world_pos(1, 2, 3), (1, 2, 3));
    }

    #[test]
    fn place_block_respects_the_chunk_box() {
        let mut level = TestLevel::new();
        let ctx = PieceCtx::new([0, 0, 0, 2, 2, 2], None);
        let stone = block("minecraft:stone");
        // 盒外不写。
        // Nothing outside the box.
        ctx.place_block(&mut level, stone, 3, 0, 0, &[0, 0, 0, 2, 2, 2]);
        assert_eq!(level.get_block(3, 0, 0), mc_bamboo_data::blocks::AIR);
        ctx.place_block(&mut level, stone, 1, 1, 1, &[0, 0, 0, 2, 2, 2]);
        assert_eq!(level.get_block(1, 1, 1), stone);
    }

    #[test]
    fn generate_box_uses_edge_and_fill() {
        let mut level = TestLevel::new();
        let ctx = PieceCtx::new([0, 0, 0, 2, 2, 2], None);
        let stone = block("minecraft:stone");
        let dirt = block("minecraft:dirt");
        generate_box(
            &ctx,
            &mut level,
            &[-10, -10, -10, 10, 10, 10],
            [0, 0, 0, 2, 2, 2],
            stone,
            dirt,
            false,
        );
        assert_eq!(level.get_block(1, 1, 1), dirt);
        assert_eq!(level.get_block(0, 1, 1), stone);
        assert_eq!(level.get_block(1, 0, 1), stone);
    }

    #[test]
    fn generate_box_skip_air_only_writes_onto_solid() {
        let mut level = TestLevel::new();
        let ctx = PieceCtx::new([0, 0, 0, 1, 0, 1], None);
        let stone = block("minecraft:stone");
        generate_box(
            &ctx,
            &mut level,
            &[-10, -10, -10, 10, 10, 10],
            [0, 0, 0, 1, 0, 1],
            stone,
            stone,
            true,
        );
        // 全是空气，一个都不写。
        // All air: nothing is written.
        assert!(level.blocks.is_empty());
    }

    #[test]
    fn is_replaceable_by_structures_covers_air_liquid_and_plants() {
        assert!(is_replaceable_by_structures(mc_bamboo_data::blocks::AIR));
        assert!(is_replaceable_by_structures(block("minecraft:water")));
        assert!(is_replaceable_by_structures(block("minecraft:glow_lichen")));
        assert!(is_replaceable_by_structures(block("minecraft:seagrass")));
        assert!(!is_replaceable_by_structures(block("minecraft:stone")));
        assert!(!is_replaceable_by_structures(block("minecraft:oak_planks")));
    }

    #[test]
    fn reorient_picks_the_facing_away_from_the_only_solid_neighbour() {
        let mut level = TestLevel::new();
        let stone = block("minecraft:stone");
        let chest = block("minecraft:chest");
        // 只有北边是实体方块 → 箱子朝南。
        // Only the north side is solid, so the chest faces south.
        level.blocks.insert((0, 0, -1), stone);
        assert_eq!(
            property_of_state(reorient(&level, (0, 0, 0), chest), "facing"),
            Some("south")
        );

        // 四面都是实体方块 → 超过一个实体邻居，走官方的顺序探测链：
        // north 前(实心)→ south，south 前(实心)→ west，west 前(实心)→ east。
        // Solid on all four sides: more than one solid neighbour, so the sequential probe
        // chain runs north → south → west → east.
        let mut all = TestLevel::new();
        for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            all.blocks.insert((dx, 0, dz), stone);
        }
        assert_eq!(
            property_of_state(reorient(&all, (0, 0, 0), chest), "facing"),
            Some("east")
        );
    }

    #[test]
    fn reorient_keeps_the_default_when_a_neighbour_is_a_chest() {
        let mut level = TestLevel::new();
        let chest = block("minecraft:chest");
        level.blocks.insert((0, 0, -1), chest);
        assert_eq!(reorient(&level, (0, 0, 0), chest), chest);
    }
}
