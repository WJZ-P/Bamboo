//! `NetherPortalBlock` 和官方的 `PortalShape`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{blocks, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain},
    state::{BlockState, HORIZONTAL_AXIS},
    world::{UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, World},
};

/// `NetherPortalBlock`。
pub(super) struct NetherPortal;

/// `MIN_WIDTH`。
const MIN_WIDTH: i32 = 2;
/// `MAX_WIDTH`。
const MAX_WIDTH: i32 = 21;
/// `MIN_HEIGHT`。
const MIN_HEIGHT: i32 = 3;
/// `MAX_HEIGHT`。
const MAX_HEIGHT: i32 = 21;

/// `FRAME`。
pub(super) fn is_frame(state: BlockState) -> bool {
    state.is_tagged(block_tags::NETHER_PORTAL_FRAME)
}

/// `isEmpty`：框架里能有的方块。
fn is_inside(state: BlockState) -> bool {
    state.is_air() || state.is_tagged(block_tags::FIRE) || state.is(blocks::NETHER_PORTAL)
}

/// `getDistanceUntilEdgeAboveFrame`：沿着框架的底边走到竖边，走不到时是 0。
fn distance_to_edge(world: &World, pos: BlockPos, direction: Direction) -> i32 {
    for width in 0..=MAX_WIDTH {
        let at = pos.relative_by(direction, width);
        let state = world.block_state(at);
        if !is_inside(state) {
            return if is_frame(state) { width } else { 0 };
        }
        if !is_frame(world.block_state(at.below())) {
            return 0;
        }
    }
    0
}

/// `calculateBottomLeft`。
fn bottom_left(world: &World, right: Direction, pos: BlockPos) -> Option<BlockPos> {
    let min_y = world.min_y().max(pos.y - MAX_HEIGHT);
    let mut bottom = pos;
    while bottom.y > min_y && is_inside(world.block_state(bottom.below())) {
        bottom = bottom.below();
    }
    let left = right.opposite();
    let edge = distance_to_edge(world, bottom, left) - 1;
    (edge >= 0).then(|| bottom.relative_by(left, edge))
}

/// `calculateWidth`。
fn width_of(world: &World, bottom_left: BlockPos, right: Direction) -> i32 {
    let width = distance_to_edge(world, bottom_left, right);
    if (MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        width
    } else {
        0
    }
}

/// `PortalShape`。
pub(super) struct PortalShape {
    axis: Axis,
    portal_blocks: i32,
    right: Direction,
    bottom_left: BlockPos,
    width: i32,
    height: i32,
}

impl PortalShape {
    /// `findEmptyPortalShape`：完整的框架，里面还没有传送门。
    pub(super) fn find_empty(world: &World, pos: BlockPos, preferred: Axis) -> Option<Self> {
        let other = if preferred == Axis::X {
            Axis::Z
        } else {
            Axis::X
        };
        [preferred, other]
            .into_iter()
            .map(|axis| Self::find_any(world, pos, axis))
            .find(|shape| shape.is_valid() && shape.portal_blocks == 0)
    }

    /// `findAnyShape`。
    fn find_any(world: &World, pos: BlockPos, axis: Axis) -> Self {
        let right = if axis == Axis::X {
            Direction::West
        } else {
            Direction::South
        };
        let mut shape = Self {
            axis,
            portal_blocks: 0,
            right,
            bottom_left: pos,
            width: 0,
            height: 0,
        };
        let Some(bottom_left) = bottom_left(world, right, pos) else {
            return shape;
        };
        shape.bottom_left = bottom_left;
        shape.width = width_of(world, bottom_left, right);
        if shape.width != 0 {
            shape.measure_height(world);
        }
        shape
    }

    /// `calculateHeight`。
    fn measure_height(&mut self, world: &World) {
        let height = self.distance_until_top(world);
        let valid =
            (MIN_HEIGHT..=MAX_HEIGHT).contains(&height) && self.has_top_frame(world, height);
        self.height = if valid { height } else { 0 };
    }

    /// `getDistanceUntilTop`：一层一层往上数，顺便数里面有多少传送门。
    fn distance_until_top(&mut self, world: &World) -> i32 {
        for height in 0..MAX_HEIGHT {
            let row = self.bottom_left.relative_by(Direction::Up, height);
            if !is_frame(world.block_state(row.relative_by(self.right, -1)))
                || !is_frame(world.block_state(row.relative_by(self.right, self.width)))
            {
                return height;
            }
            for offset in 0..self.width {
                let state = world.block_state(row.relative_by(self.right, offset));
                if !is_inside(state) {
                    return height;
                }
                if state.is(blocks::NETHER_PORTAL) {
                    self.portal_blocks += 1;
                }
            }
        }
        MAX_HEIGHT
    }

    /// `hasTopFrame`。
    fn has_top_frame(&self, world: &World, height: i32) -> bool {
        let top = self.bottom_left.relative_by(Direction::Up, height);
        (0..self.width)
            .all(|offset| is_frame(world.block_state(top.relative_by(self.right, offset))))
    }

    /// `isValid`。
    fn is_valid(&self) -> bool {
        (MIN_WIDTH..=MAX_WIDTH).contains(&self.width)
            && (MIN_HEIGHT..=MAX_HEIGHT).contains(&self.height)
    }

    /// `isComplete`。
    fn is_complete(&self) -> bool {
        self.is_valid() && self.portal_blocks == self.width * self.height
    }

    /// `createPortalBlocks`。
    pub(super) fn create_portal_blocks(&self, world: &mut World) {
        let portal = BlockState(blocks::NETHER_PORTAL).with(HORIZONTAL_AXIS, self.axis);
        for height in 0..self.height {
            let row = self.bottom_left.relative_by(Direction::Up, height);
            for offset in 0..self.width {
                let pos = row.relative_by(self.right, offset);
                world.set_block(pos, portal, UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE);
            }
        }
    }
}

impl Behaviour for NetherPortal {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let update_axis = direction.axis();
        let axis = state.get(HORIZONTAL_AXIS);
        let wrong_axis = axis != update_axis && update_axis.is_horizontal();
        if !wrong_axis
            && !neighbor.is_same_block(state)
            && !PortalShape::find_any(world, pos, axis).is_complete()
        {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        behaviour::misc::test_support::set,
        testing::{TestLevel, actor_holding, break_block, top_hit, use_item},
    };

    const INSIDE: [BlockPos; 6] = [
        BlockPos::new(0, 65, 0),
        BlockPos::new(1, 65, 0),
        BlockPos::new(0, 66, 0),
        BlockPos::new(1, 66, 0),
        BlockPos::new(0, 67, 0),
        BlockPos::new(1, 67, 0),
    ];

    /// 里面 2 格宽、3 格高的框架，四个角不用有方块。
    fn frame() -> TestLevel {
        let mut level = TestLevel::new();
        for x in 0..=1 {
            level.set(BlockPos::new(x, 64, 0), BlockState(blocks::OBSIDIAN));
            level.set(BlockPos::new(x, 68, 0), BlockState(blocks::OBSIDIAN));
        }
        for y in 65..=67 {
            level.set(BlockPos::new(-1, y, 0), BlockState(blocks::OBSIDIAN));
            level.set(BlockPos::new(2, y, 0), BlockState(blocks::OBSIDIAN));
        }
        level
    }

    fn lit_portal() -> TestLevel {
        let mut level = frame();
        set(&mut level, INSIDE[0], BlockState(blocks::FIRE));
        level
    }

    #[test]
    fn fire_inside_a_frame_fills_it_with_portal() {
        let level = lit_portal();
        for pos in INSIDE {
            let state = level.get(pos);
            assert!(state.is(blocks::NETHER_PORTAL));
            assert_eq!(state.get(HORIZONTAL_AXIS), Axis::X);
        }
    }

    #[test]
    fn a_frame_along_z_lights_on_the_other_axis() {
        let mut level = TestLevel::new();
        for z in 0..=1 {
            level.set(BlockPos::new(0, 64, z), BlockState(blocks::OBSIDIAN));
            level.set(BlockPos::new(0, 68, z), BlockState(blocks::OBSIDIAN));
        }
        for y in 65..=67 {
            level.set(BlockPos::new(0, y, -1), BlockState(blocks::OBSIDIAN));
            level.set(BlockPos::new(0, y, 2), BlockState(blocks::OBSIDIAN));
        }
        let inside = BlockPos::new(0, 66, 1);
        set(&mut level, inside, BlockState(blocks::FIRE));
        assert_eq!(level.get(inside).get(HORIZONTAL_AXIS), Axis::Z);
    }

    #[test]
    fn fire_in_an_open_frame_stays_fire() {
        let mut level = frame();
        level.set(BlockPos::new(2, 66, 0), BlockState::AIR);
        set(&mut level, INSIDE[0], BlockState(blocks::FIRE));
        assert!(level.get(INSIDE[0]).is(blocks::FIRE));
        assert!(level.get(INSIDE[1]).is_air());
    }

    #[test]
    fn breaking_the_frame_breaks_the_whole_portal() {
        let mut level = lit_portal();
        break_block(&mut level, BlockPos::new(-1, 66, 0));
        assert!(INSIDE.into_iter().all(|pos| level.get(pos).is_air()));
    }

    /// 完整的传送门不用管它前后的方块。
    #[test]
    fn blocks_in_front_of_a_portal_leave_it_alone() {
        let mut level = lit_portal();
        let floor_in_front = BlockPos::new(0, 64, 1);
        level.set(floor_in_front, BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::STONE),
            top_hit(floor_in_front),
        );
        assert!(level.get(floor_in_front.above()).is(blocks::STONE));
        let portal_stands = INSIDE
            .into_iter()
            .all(|pos| level.get(pos).is(blocks::NETHER_PORTAL));
        assert!(portal_stands);
    }
}
