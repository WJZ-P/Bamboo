//! `BellBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{FaceAttached, can_support_center, default_state},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BELL_ATTACHMENT, BellAttachType, BlockState, HORIZONTAL_FACING, POWERED, SupportType},
    world::World,
};

/// 点在钟的这个高度以上时敲不响，那里是挂钟的横梁。
const MAX_RING_HEIGHT: f32 = 0.8124;

/// `BellBlock`。敲钟的动画和声音都在方块实体里，这里只决定敲不敲得响。
pub(super) struct Bell;

/// `getConnectedDirection`：钟朝外的那个方向，支撑在它的反方向。
fn connected_direction(state: BlockState) -> Direction {
    match state.get(BELL_ATTACHMENT) {
        BellAttachType::Floor => Direction::Up,
        BellAttachType::Ceiling => Direction::Down,
        BellAttachType::SingleWall | BellAttachType::DoubleWall => {
            state.get(HORIZONTAL_FACING).opposite()
        }
    }
}

/// `isProperHit`：钟的侧面才敲得响，挂法决定了是哪两面。
fn is_proper_hit(state: BlockState, clicked: Direction, click_y: f64) -> bool {
    if clicked.axis() == Axis::Y || click_y > f64::from(MAX_RING_HEIGHT) {
        return false;
    }
    let along_facing = state.get(HORIZONTAL_FACING).axis() == clicked.axis();
    match state.get(BELL_ATTACHMENT) {
        BellAttachType::Floor => along_facing,
        BellAttachType::SingleWall | BellAttachType::DoubleWall => !along_facing,
        BellAttachType::Ceiling => true,
    }
}

/// 点在顶面或者底面：立在地上或者挂在天花板上。
fn floor_or_ceiling_state(
    default: BlockState,
    ctx: &PlaceContext,
    world: &World,
) -> Option<BlockState> {
    let attachment = if ctx.clicked_face() == Direction::Down {
        BellAttachType::Ceiling
    } else {
        BellAttachType::Floor
    };
    let state = default
        .with(BELL_ATTACHMENT, attachment)
        .with(HORIZONTAL_FACING, ctx.horizontal_direction());
    state.can_survive(world, ctx.clicked_pos()).then_some(state)
}

/// 这条轴上两边都是完整的面。
fn has_walls_on_both_sides(world: &World, pos: BlockPos, axis: Axis) -> bool {
    let sturdy = |side: Direction| {
        world.is_face_sturdy(pos.relative(side), side.opposite(), SupportType::Full)
    };
    sturdy(axis.negative()) && sturdy(axis.positive())
}

/// 点在侧面：挂在墙上，挂不住时退回地上或者天花板，朝向不变。
fn wall_state(default: BlockState, ctx: &PlaceContext, world: &World) -> Option<BlockState> {
    let pos = ctx.clicked_pos();
    let face = ctx.clicked_face();
    let attachment = if has_walls_on_both_sides(world, pos, face.axis()) {
        BellAttachType::DoubleWall
    } else {
        BellAttachType::SingleWall
    };
    let state = default
        .with(HORIZONTAL_FACING, face.opposite())
        .with(BELL_ATTACHMENT, attachment);
    if state.can_survive(world, pos) {
        return Some(state);
    }
    let attachment = if world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full) {
        BellAttachType::Floor
    } else {
        BellAttachType::Ceiling
    };
    let state = state.with(BELL_ATTACHMENT, attachment);
    state.can_survive(world, pos).then_some(state)
}

/// `updateShape` 里挂在两面墙之间的钟和单面墙上的钟互相转换的部分。
///
/// 这里看的是传进来的邻居，不是世界里的；邻居是 `moving_piston` 时形状来自它的方块实体。
fn wall_shape_change(
    state: BlockState,
    world: &World,
    direction: Direction,
    neighbor_pos: BlockPos,
    neighbor: BlockState,
) -> Option<BlockState> {
    let sturdy = |face| world.is_state_face_sturdy(neighbor, neighbor_pos, face, SupportType::Full);
    let attachment = state.get(BELL_ATTACHMENT);
    let facing = state.get(HORIZONTAL_FACING);
    if direction.axis() != facing.axis() {
        return None;
    }
    if attachment == BellAttachType::DoubleWall && !sturdy(direction) {
        let single = state
            .with(BELL_ATTACHMENT, BellAttachType::SingleWall)
            .with(HORIZONTAL_FACING, direction.opposite());
        return Some(single);
    }
    let behind = connected_direction(state);
    if attachment == BellAttachType::SingleWall && behind == direction && sturdy(facing) {
        return Some(state.with(BELL_ATTACHMENT, BellAttachType::DoubleWall));
    }
    None
}

impl Behaviour for Bell {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        if ctx.clicked_face().axis() == Axis::Y {
            floor_or_ceiling_state(default_state(block), ctx, world)
        } else {
            wall_state(default_state(block), ctx, world)
        }
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let support = connected_direction(state).opposite();
        if support == Direction::Up {
            can_support_center(world, pos.above(), Direction::Down)
        } else {
            FaceAttached::can_attach(world, pos, support)
        }
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let support = connected_direction(state).opposite();
        if support == direction
            && !state.can_survive(world, pos)
            && state.get(BELL_ATTACHMENT) != BellAttachType::DoubleWall
        {
            return BlockState::AIR;
        }
        wall_shape_change(state, world, direction, neighbor_pos, neighbor).unwrap_or_else(|| {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        })
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let signal = world.has_neighbor_signal(pos);
        if signal != state.get(POWERED) {
            world.set_block_and_update(pos, state.with(POWERED, signal));
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        _world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let click_y = ctx.click_location().y - f64::from(ctx.clicked_pos().y);
        if is_proper_hit(state, ctx.clicked_face(), click_y) {
            InteractionResult::Success
        } else {
            InteractionResult::Pass
        }
    }
}
