//! `StructureTemplate.placeInWorld` 末尾的两步形状更新，跑在一条
//! [`crate::structure::piece_place::PieceLevel`] 上。
//!
//! 官方顺序（`StructureTemplate.java` 的 `placeInWorld`，只要 `settings.getKnownShape()`
//! 为 false 就跑，林地府邸就属于这种）：
//! 1. 用实际写成功的格子填一个 `BitSetDiscreteVoxelShape`，再调
//!    `updateShapeAtEdge(level, updateMode, shape, minX, minY, minZ)`；
//! 2. 对每个写成功的格子，`Block.updateFromNeighbourShapes(state, level, pos)`，变了就
//!    `setBlock(pos, newState, updateMode & -2 | 16)`。
//!
//! `updateMode` 是 2（`TemplateStructurePiece.postProcess` 传的值）。两步都只写方块，
//! 排的计划刻和方块事件都丢弃：对拍只比方块。
//!
//! The two shape-update steps at the end of `StructureTemplate.placeInWorld`, running on a
//! [`crate::structure::piece_place::PieceLevel`]. Vanilla's order (taken whenever
//! `settings.getKnownShape()` is false, which is the woodland mansion's case): fill a
//! `BitSetDiscreteVoxelShape` with the positions actually written and call
//! `updateShapeAtEdge(level, updateMode, shape, minX, minY, minZ)`; then for every written
//! position run `Block.updateFromNeighbourShapes(state, level, pos)`, writing the result
//! with `updateMode & -2 | 16` when it changed. `updateMode` is 2 here. Both steps only
//! write blocks; scheduled ticks and block events are dropped because only blocks are
//! compared.

use std::collections::HashSet;

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, EntityTables, Level,
    LevelRandom, LevelTables, RedstoneMemory, World,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, sign::SignContent};

use crate::structure::piece_place::PieceLevel;

/// `Block.UPDATE_SHAPE_ORDER`。
///
/// `Block.UPDATE_SHAPE_ORDER`.
const UPDATE_SHAPE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::North,
    Direction::South,
    Direction::Down,
    Direction::Up,
];

/// 跑完 `placeInWorld` 的两步形状更新。
///
/// `placed` 是实际写成功的世界坐标，顺序就是写入顺序。
///
/// Run the two shape-update steps of `placeInWorld`. `placed` holds the world positions
/// actually written, in write order.
pub fn update_shapes(level: &mut dyn PieceLevel, placed: &[(i32, i32, i32)]) {
    if placed.is_empty() {
        return;
    }
    let mut shape: HashSet<(i32, i32, i32)> = HashSet::new();
    let (mut min_x, mut min_y, mut min_z) = (i32::MAX, i32::MAX, i32::MAX);
    let (mut max_x, mut max_y, mut max_z) = (i32::MIN, i32::MIN, i32::MIN);
    for &pos in placed {
        shape.insert(pos);
        min_x = min_x.min(pos.0);
        min_y = min_y.min(pos.1);
        min_z = min_z.min(pos.2);
        max_x = max_x.max(pos.0);
        max_y = max_y.max(pos.1);
        max_z = max_z.max(pos.2);
    }

    let mut wrapper = ShapeLevel {
        level,
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        redstone: RedstoneMemory::default(),
        entities: EntityStore::default(),
        random: LevelRandom::new(0),
    };

    // `updateShapeAtEdge`：三个轴各扫一遍，形状面的两侧各跑一次 `updateShape`。
    // `updateShapeAtEdge`: one sweep per axis, running `updateShape` on both sides of every
    // shape face.
    let size = [max_x - min_x + 1, max_y - min_y + 1, max_z - min_z + 1];
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
                        (min_x + x, min_y + y, min_z + z)
                    };
                    let full = c != c_size && shape.contains(&cell(c));
                    if !last_full && full {
                        update_face(&mut wrapper, cell(c), negative);
                    }
                    if last_full && !full {
                        update_face(&mut wrapper, cell(c - 1), positive);
                    }
                    last_full = full;
                }
            }
        }
    }

    // `Block.updateFromNeighbourShapes`：按 `UPDATE_SHAPE_ORDER` 逐方向 `updateShape`。
    // `Block.updateFromNeighbourShapes`: one `updateShape` per direction in
    // `UPDATE_SHAPE_ORDER`.
    for &cell in placed {
        let pos = BlockPos::new(cell.0, cell.1, cell.2);
        let state = wrapper.block_state(pos);
        let mut new_state = state;
        for direction in UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor = wrapper.block_state(neighbor_pos);
            let mut ticks = BlockTicks::default();
            let mut world = World::new(&mut wrapper, &mut ticks);
            new_state = new_state.update_shape(&mut world, pos, direction, neighbor_pos, neighbor);
        }
        if new_state != state {
            wrapper.write_block(pos, new_state, 2);
        }
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
    /// 反方向的轮换。
    ///
    /// The inverse rotation.
    fn inverse(self) -> Self {
        match self {
            Self::None => Self::None,
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }

    /// `cycle(x, y, z)`：轮换后的坐标。
    ///
    /// `cycle(x, y, z)`: the cycled coordinates.
    fn coords(self, (x, y, z): (i32, i32, i32)) -> (i32, i32, i32) {
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

/// `updateShapeAtEdge` 里对一个面的处理：先更新形状内的这一格，再更新对面的邻居。
///
/// One face of `updateShapeAtEdge`: update the cell inside the shape, then the neighbour
/// across the face.
fn update_face(level: &mut ShapeLevel<'_>, cell: (i32, i32, i32), direction: Direction) {
    let pos = BlockPos::new(cell.0, cell.1, cell.2);
    let neighbor_pos = pos.relative(direction);
    let state = level.block_state(pos);
    let neighbor = level.block_state(neighbor_pos);
    let mut ticks = BlockTicks::default();
    let new_state = {
        let mut world = World::new(level, &mut ticks);
        state.update_shape(&mut world, pos, direction, neighbor_pos, neighbor)
    };
    if new_state != state {
        level.write_block(pos, new_state, 2);
    }
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

/// 把 [`PieceLevel`] 包成方块行为能读的 [`Level`]。
///
/// Wraps a [`PieceLevel`] as a [`Level`] block behaviours can read.
struct ShapeLevel<'w> {
    level: &'w mut dyn PieceLevel,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    redstone: RedstoneMemory,
    entities: EntityStore,
    random: LevelRandom,
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
        BlockState(self.level.get_block(pos.x, pos.y, pos.z))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        self.level.set_block(pos.x, pos.y, pos.z, state.0)
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        self.level.min_y()
    }

    fn max_y(&self) -> i32 {
        self.level.max_y()
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

    fn brightness(&self, _layer: LightLayer, _pos: BlockPos) -> i32 {
        0
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
