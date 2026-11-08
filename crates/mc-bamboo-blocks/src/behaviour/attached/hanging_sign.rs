//! `CeilingHangingSignBlock`、`WallHangingSignBlock`，都继承 `SignBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{Block, ItemKind, tags::block_tags};

use super::sign::Sign;
use crate::{
    behaviour::{
        Behaviour,
        common::{
            default_state, is_collision_face_full, is_water_source, item_of, rotation_segment,
        },
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{ATTACHED, BlockState, HORIZONTAL_FACING, ROTATION_16, SupportType, WATERLOGGED},
    world::World,
};

/// `itemStack.getItem() instanceof HangingSignItem`。
fn holds_hanging_sign(ctx: &UseContext) -> bool {
    item_of(ctx.item).is_some_and(|item| item.kind == ItemKind::HangingSign)
}

/// `RotationSegment.convertToDirection` 的轴，只有四个正方向才有。
fn axis_of_segment(segment: i32) -> Option<Axis> {
    match segment {
        0 | 8 => Some(Axis::Z),
        4 | 12 => Some(Axis::X),
        _ => None,
    }
}

/// 上面是悬挂告示牌、而且和玩家的朝向同轴，这时新的告示牌接在它下面，不挂在中间。
fn continues_sign_above(above: BlockState, direction: Direction) -> bool {
    if !above.is_tagged(block_tags::ALL_HANGING_SIGNS) {
        return false;
    }
    let axis = match above.try_get(HORIZONTAL_FACING) {
        Some(facing) => Some(facing.axis()),
        None => axis_of_segment(above.get(ROTATION_16)),
    };
    axis == Some(direction.axis())
}

/// `CeilingHangingSignBlock`。
pub(super) struct CeilingHangingSign;

impl Behaviour for CeilingHangingSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let above = world.block_state(pos.above());
        let direction = Direction::from_yaw(f64::from(ctx.rotation()));
        let crouching = ctx.is_secondary_use_active();
        let attached_to_middle = crouching
            || (!is_collision_face_full(world, pos.above(), Direction::Down)
                && !continues_sign_above(above, direction));
        let rotation = if attached_to_middle {
            rotation_segment(ctx.rotation() + 180.0)
        } else {
            rotation_segment(direction.opposite().to_yaw())
        };
        let state = default_state(block)
            .with(ATTACHED, attached_to_middle)
            .with(ROTATION_16, rotation)
            .with(WATERLOGGED, is_water_source(world, pos));
        Some(state)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.above(), Direction::Down, SupportType::Center)
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
        if direction == Direction::Up && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 拿着悬挂告示牌点底面时不交给告示牌，好让物品在下面接一块。
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if holds_hanging_sign(ctx) && ctx.clicked_face() == Direction::Down {
            return InteractionResult::Pass;
        }
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Sign.set_placed_by(state, world, pos, placer);
    }
}

/// `WallHangingSignBlock`。官方没有覆盖 `canSurvive`，放下以后不会掉。
pub(super) struct WallHangingSign;

impl WallHangingSign {
    /// `canPlace`：横杆的两头至少有一头搭在东西上。
    fn can_place(state: BlockState, world: &World, pos: BlockPos) -> bool {
        let facing = state.get(HORIZONTAL_FACING);
        let clockwise = facing.clockwise();
        let counter_clockwise = facing.counter_clockwise();
        Self::can_attach_to(world, state, pos.relative(clockwise), counter_clockwise)
            || Self::can_attach_to(world, state, pos.relative(counter_clockwise), clockwise)
    }

    /// `canAttachTo`。
    fn can_attach_to(
        world: &World,
        state: BlockState,
        attach_pos: BlockPos,
        attach_face: Direction,
    ) -> bool {
        let attach_state = world.block_state(attach_pos);
        if attach_state.is_tagged(block_tags::WALL_HANGING_SIGNS) {
            attach_state.get(HORIZONTAL_FACING).axis() == state.get(HORIZONTAL_FACING).axis()
        } else {
            world.is_face_sturdy(attach_pos, attach_face, SupportType::Full)
        }
    }
}

impl Behaviour for WallHangingSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let clicked_axis = ctx.clicked_face().axis();
        let water = is_water_source(world, pos);
        ctx.nearest_looking_directions()
            .into_iter()
            .filter(|direction| {
                direction.axis().is_horizontal() && direction.axis() != clicked_axis
            })
            .map(|direction| default_state(block).with(HORIZONTAL_FACING, direction.opposite()))
            .find(|&state| state.can_survive(world, pos) && Self::can_place(state, world, pos))
            .map(|state| state.with(WATERLOGGED, water))
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
        let along_the_bar = direction.axis() == state.get(HORIZONTAL_FACING).clockwise().axis();
        if along_the_bar && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 拿着悬挂告示牌点两头时不交给告示牌，点能写字的两面才算。
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let editable_side = ctx.clicked_face().axis() == state.get(HORIZONTAL_FACING).axis();
        if holds_hanging_sign(ctx) && !editable_side {
            return InteractionResult::Pass;
        }
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Sign.set_placed_by(state, world, pos, placer);
    }

    fn can_place_from_hanging_sign_item(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
    ) -> bool {
        Self::can_place(state, world, pos)
    }
}
