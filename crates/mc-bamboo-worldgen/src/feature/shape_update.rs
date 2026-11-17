//! `StructureTemplate.updateShapeAtEdge`：对一块体素形状的每个外表面，让表面两侧的
//! 方块各做一次 `updateShape`。树（`TreeFeature.place` 的最后一步）和结构模板放置都用它。
//!
//! 方块的 `updateShape` 逻辑在 `mc-bamboo-blocks` 里，它们通过 [`Level`] 读世界。
//! [`ShapeLevel`] 把地物阶段的 [`FeatureWorld`] 包成一个 `Level`：只有读方块和写方块
//! 是真的，实体、方块实体、红石记忆这些表都是空的，世界生成阶段也确实没有它们。
//! `updateShape` 里排的计划刻落在一张临时表里，随后丢掉；官方把它们记进
//! `ProtoChunk`，这里只比方块，暂不保存。
//!
//! `StructureTemplate.updateShapeAtEdge`: for every outer face of a voxel shape, the
//! blocks on both sides of the face run one `updateShape`. Trees (the last step of
//! `TreeFeature.place`) and structure template placement use it.
//!
//! The `updateShape` logic lives in `mc-bamboo-blocks` and reads the world through
//! [`Level`]. [`ShapeLevel`] wraps the feature-stage [`FeatureWorld`] as a `Level`:
//! only block reads and writes are real; the entity, block-entity and redstone tables
//! are empty, as they are during world generation. Ticks scheduled by `updateShape`
//! land in a temporary table that is dropped; vanilla records them in the `ProtoChunk`,
//! which is not kept here because only blocks are compared.

use std::collections::HashSet;

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, EntityTables, Level,
    LevelRandom, LevelTables, RedstoneMemory, World,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, sign::SignContent};

use super::world::FeatureWorld;

type Pos = (i32, i32, i32);

/// 形状更新与结构模板放置需要的最小世界接口：读方块、写方块、构建高度。
///
/// The minimal world interface shape updates and template placement need: block reads,
/// block writes and the build height.
pub trait BlockAccess {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32;
    /// `level.setBlock(pos, state, flags)`，返回是否写下。
    ///
    /// `level.setBlock(pos, state, flags)`, returning whether the block was written.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool;
    /// `blockEntity.loadWithComponents(nbt)`：记下该位置的方块实体 nbt。默认丢弃，
    /// 只有真正在收集方块实体的调用方（`FeatureWorld`）需要实现。
    ///
    /// `blockEntity.loadWithComponents(nbt)`: record the block-entity nbt at the
    /// position. The default drops it; only `FeatureWorld` collects it.
    fn set_block_entity(&mut self, _x: i32, _y: i32, _z: i32, _nbt: &mc_bamboo_nbt::Tag) {}
    fn min_y(&self) -> i32;
    fn max_y(&self) -> i32;
}

impl BlockAccess for FeatureWorld<'_> {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        FeatureWorld::get_block(self, x, y, z)
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        self.set_block_with_heightmaps(x, y, z, state)
    }

    fn set_block_entity(&mut self, x: i32, y: i32, z: i32, nbt: &mc_bamboo_nbt::Tag) {
        FeatureWorld::set_block_entity(self, x, y, z, nbt);
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }
}

/// 把 [`BlockAccess`] 包成方块行为能读的 [`Level`]。
///
/// Wraps a [`BlockAccess`] as a [`Level`] block behaviours can read.
struct ShapeLevel<'w> {
    world: &'w mut dyn BlockAccess,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    redstone: RedstoneMemory,
    entities: EntityStore,
    random: LevelRandom,
    /// 这次更新写下的方块，供调用方记写入日志（对拍用）。
    ///
    /// Blocks written by this update, so the caller can log them (oracle only).
    writes: Vec<(i32, i32, i32, u32)>,
}

impl LevelTables for ShapeLevel<'_> {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events_table(&self) -> &BlockEvents {
        &self.block_events
    }

    fn block_events_table_mut(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone_table(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }
}

impl EntityTables for ShapeLevel<'_> {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }
}

impl Level for ShapeLevel<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.world.get_block(pos.x, pos.y, pos.z))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        let written = self.world.set_block(pos.x, pos.y, pos.z, state.0);
        if written {
            self.writes.push((pos.x, pos.y, pos.z, state.0));
        }
        written
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        self.world.min_y()
    }

    fn max_y(&self) -> i32 {
        self.world.max_y()
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        false
    }

    /// FEATURES 阶段的区块还没到 `INITIALIZE_LIGHT`，天空光存储里没有这一列，
    /// `SkyLightSectionStorage.getLightValue` 返回 15；方块光没有数据，返回 0。
    /// 蘑菇的 `canSurvive` 读的就是它。
    ///
    /// A FEATURES-stage chunk has not reached `INITIALIZE_LIGHT`, so the sky light
    /// storage has no entry for the column and `SkyLightSectionStorage.getLightValue`
    /// returns 15; block light has no data and reads 0. Mushroom `canSurvive` reads this.
    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => 15,
            LightLayer::Block => 0,
        }
    }

    fn sign(&self, _pos: BlockPos) -> Option<SignContent> {
        None
    }

    fn set_sign(&mut self, _pos: BlockPos, _content: SignContent) -> bool {
        false
    }

    fn note_block_sound(&self, _pos: BlockPos) -> Option<String> {
        None
    }
}

/// `AxisCycle`：坐标轴的轮换。
///
/// `AxisCycle`: a rotation of the coordinate axes.
#[derive(Clone, Copy)]
enum AxisCycle {
    None,
    Forward,
    Backward,
}

impl AxisCycle {
    fn inverse(self) -> Self {
        match self {
            Self::None => Self::None,
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }

    /// `cycle(x, y, z, axis)` 对 X、Y、Z 三个轴各取一次，得到轮换后的坐标。
    ///
    /// `cycle(x, y, z, axis)` evaluated for X, Y and Z: the cycled coordinates.
    fn coords(self, (x, y, z): Pos) -> Pos {
        match self {
            Self::None => (x, y, z),
            Self::Forward => (z, x, y),
            Self::Backward => (y, z, x),
        }
    }

    /// `cycle(axis)`，轴用 0、1、2 表示 X、Y、Z。
    ///
    /// `cycle(axis)` with axes X, Y, Z as 0, 1, 2.
    fn axis(self, axis: usize) -> usize {
        match self {
            Self::None => axis,
            Self::Forward => (axis + 1) % 3,
            Self::Backward => (axis + 2) % 3,
        }
    }
}

/// `Block.updateFromNeighbourShapes(state, level, pos)`：按 `UPDATE_SHAPE_ORDER`
/// （WEST、EAST、NORTH、SOUTH、DOWN、UP）依次 `updateShape`，返回最终状态。
///
/// 结构模板放置写完所有方块后对每个写入位置调一次；结果与官方相同，因为每一步都只读
/// 前一状态和当前世界。
///
/// `Block.updateFromNeighbourShapes(state, level, pos)`: `updateShape` in
/// `UPDATE_SHAPE_ORDER` (WEST, EAST, NORTH, SOUTH, DOWN, UP), returning the final state.
/// Template placement runs it once per written position after all writes; the result
/// matches vanilla because every step reads only the previous state and the world.
pub fn update_from_neighbour_shapes(world: &mut dyn BlockAccess, pos: Pos) -> u32 {
    const ORDER: [Direction; 6] = [
        Direction::West,
        Direction::East,
        Direction::North,
        Direction::South,
        Direction::Down,
        Direction::Up,
    ];
    let mut level = ShapeLevel {
        world,
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        redstone: RedstoneMemory::default(),
        entities: EntityStore::default(),
        random: LevelRandom::new(0),
        writes: Vec::new(),
    };
    let block_pos = BlockPos::new(pos.0, pos.1, pos.2);
    let mut state = level.block_state(block_pos);
    let mut ticks = BlockTicks::default();
    for direction in ORDER {
        let neighbour_pos = block_pos.relative(direction);
        let neighbour = level.block_state(neighbour_pos);
        let mut block_world = World::new(&mut level, &mut ticks);
        state = state.update_shape(
            &mut block_world,
            block_pos,
            direction,
            neighbour_pos,
            neighbour,
        );
    }
    state.0
}

/// 官方 `StructureTemplate.updateShapeAtEdge(level, 3, shape, minX, minY, minZ)`。
/// `shape` 里是已填充格子的世界坐标，`min` / `max` 是形状的闭区间包围盒。
///
/// Official `StructureTemplate.updateShapeAtEdge(level, 3, shape, minX, minY, minZ)`.
/// `shape` holds the world positions of filled cells; `min` / `max` are the shape's
/// inclusive bounds.
pub fn update_shape_at_edge(
    world: &mut dyn BlockAccess,
    shape: &HashSet<Pos>,
    min: Pos,
    max: Pos,
) -> Vec<(i32, i32, i32, u32)> {
    let size = [max.0 - min.0 + 1, max.1 - min.1 + 1, max.2 - min.2 + 1];
    let mut level = ShapeLevel {
        world,
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        redstone: RedstoneMemory::default(),
        entities: EntityStore::default(),
        random: LevelRandom::new(0),
        writes: Vec::new(),
    };
    // `DiscreteVoxelShape.forAllFaces`：三个轴各扫一遍。
    // `DiscreteVoxelShape.forAllFaces`: one sweep per axis.
    for cycle in [AxisCycle::None, AxisCycle::Forward, AxisCycle::Backward] {
        let inverse = cycle.inverse();
        let c_axis = inverse.axis(2);
        let (a_size, b_size, c_size) = (size[inverse.axis(0)], size[inverse.axis(1)], size[c_axis]);
        let (negative, positive) = match c_axis {
            0 => (Direction::West, Direction::East),
            1 => (Direction::Down, Direction::Up),
            _ => (Direction::North, Direction::South),
        };
        for a in 0..a_size {
            for b in 0..b_size {
                let mut last_full = false;
                for c in 0..=c_size {
                    let cell = |c: i32| {
                        let (x, y, z) = inverse.coords((a, b, c));
                        (min.0 + x, min.1 + y, min.2 + z)
                    };
                    let full = c != c_size && shape.contains(&cell(c));
                    if !last_full && full {
                        update_face(&mut level, cell(c), negative);
                    }
                    if last_full && !full {
                        update_face(&mut level, cell(c - 1), positive);
                    }
                    last_full = full;
                }
            }
        }
    }
    level.writes
}

/// `updateShapeAtEdge` 里对一个面的处理：先更新形状内的这一格，再更新面对面的邻居。
///
/// One face of `updateShapeAtEdge`: update the cell inside the shape, then the
/// neighbour across the face.
fn update_face(level: &mut ShapeLevel<'_>, cell: Pos, direction: Direction) {
    let pos = BlockPos::new(cell.0, cell.1, cell.2);
    let neighbor_pos = pos.relative(direction);
    let state = level.block_state(pos);
    let neighbor = level.block_state(neighbor_pos);
    let mut ticks = BlockTicks::default();
    let new_state = {
        let mut world = World::new(level, &mut ticks);
        let updated = state.update_shape(&mut world, pos, direction, neighbor_pos, neighbor);
        if updated != state {
            level.write_block(pos, updated, 2);
        }
        updated
    };
    let new_neighbor = {
        let mut world = World::new(level, &mut ticks);
        neighbor.update_shape(
            &mut world,
            neighbor_pos,
            direction.opposite(),
            pos,
            new_state,
        )
    };
    if new_neighbor != neighbor {
        level.write_block(neighbor_pos, new_neighbor, 2);
    }
}
