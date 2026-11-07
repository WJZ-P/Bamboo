//! `FireBlock`、`SoulFireBlock` 和父类 `BaseFireBlock`。
//!
//! 火的蔓延和熄灭都在 `FireBlock.tick` 里，不在范围里，这里不排它的计划刻。
//! 火没有方块物品，打火石和火焰弹用 [`can_be_placed_at`] 和 [`state_at`] 生火，见 `crate::item_use`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{block_maps::FIRE_ODDS, blocks, tags::block_tags};

use super::portal::{PortalShape, is_frame};
use crate::{
    behaviour::Behaviour,
    state::{AGE_15, BlockState, EAST, NORTH, Property, SOUTH, SupportType, UP, WATERLOGGED, WEST},
    world::World,
};

/// `FireBlock`。
pub(super) struct Fire;

/// `SoulFireBlock`。
pub(super) struct SoulFire;

/// `PROPERTY_BY_DIRECTION`：火没有朝下的面。
const fn side_property(side: Direction) -> Option<Property<bool>> {
    match side {
        Direction::North => Some(NORTH),
        Direction::East => Some(EAST),
        Direction::South => Some(SOUTH),
        Direction::West => Some(WEST),
        Direction::Up => Some(UP),
        Direction::Down => None,
    }
}

/// `getIgniteOdds(state)`。
fn ignite_odds(state: BlockState) -> i32 {
    if state.try_get(WATERLOGGED) == Some(true) {
        return 0;
    }
    let block = state.block_id() as i32;
    FIRE_ODDS
        .iter()
        .find(|&&(id, _, _)| id == block)
        .map_or(0, |&(_, ignite, _)| ignite)
}

/// `FireBlock.canBurn`。
fn can_burn(state: BlockState) -> bool {
    ignite_odds(state) > 0
}

/// `isValidFireLocation`：旁边有能烧的方块。
fn is_valid_location(world: &World, pos: BlockPos) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| can_burn(world.block_state(pos.relative(side))))
}

/// `FireBlock.getStateForPlacement(level, pos)`：悬空的火贴在能烧的方块上。
fn fire_state(world: &World, pos: BlockPos) -> BlockState {
    let below_pos = pos.below();
    let default = BlockState(blocks::FIRE);
    if can_burn(world.block_state(below_pos))
        || world.is_face_sturdy(below_pos, Direction::Up, SupportType::Full)
    {
        return default;
    }
    Direction::ALL.into_iter().fold(default, |state, side| {
        side_property(side).map_or(state, |property| {
            state.with(property, can_burn(world.block_state(pos.relative(side))))
        })
    })
}

/// `SoulFireBlock.canSurviveOnBlock`。
fn is_soul_fire_base(state: BlockState) -> bool {
    state.is_tagged(block_tags::SOUL_FIRE_BASE_BLOCKS)
}

/// `BaseFireBlock.getState`。
pub(crate) fn state_at(world: &World, pos: BlockPos) -> BlockState {
    if is_soul_fire_base(world.block_state(pos.below())) {
        BlockState(blocks::SOUL_FIRE)
    } else {
        fire_state(world, pos)
    }
}

/// `BaseFireBlock.canBePlacedAt`：空气里放得住火，或者这里能点亮一个空的传送门框架。
pub(crate) fn can_be_placed_at(world: &World, pos: BlockPos, forward: Direction) -> bool {
    world.block_state(pos).is_air()
        && (state_at(world, pos).can_survive(world, pos) || is_portal(world, pos, forward))
}

/// `BaseFireBlock.isPortal`：旁边有框架方块时，按玩家朝向的左手轴优先找空框架。
/// 官方只在朝向是竖直的时候取随机的轴，玩家的水平朝向永远是水平的。
fn is_portal(world: &World, pos: BlockPos, forward: Direction) -> bool {
    let beside_frame = Direction::ALL
        .into_iter()
        .any(|side| is_frame(world.block_state(pos.relative(side))));
    beside_frame
        && PortalShape::find_empty(world, pos, forward.counter_clockwise().axis()).is_some()
}

/// `getStateWithAge`。
fn state_with_age(world: &World, pos: BlockPos, age: i32) -> BlockState {
    let state = state_at(world, pos);
    if state.is(blocks::FIRE) {
        state.with(AGE_15, age)
    } else {
        state
    }
}

/// `BaseFireBlock.onPlace`：点燃传送门框架，或者放不住时灭掉。
fn on_place(state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
    if old.is_same_block(state) {
        return;
    }
    if let Some(shape) = PortalShape::find_empty(world, pos, Axis::X) {
        shape.create_portal_blocks(world);
    } else if !state.can_survive(world, pos) {
        world.remove_block(pos);
    }
}

impl Behaviour for Fire {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full)
            || is_valid_location(world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if state.can_survive(world, pos) {
            state_with_age(world, pos, state.get(AGE_15))
        } else {
            BlockState::AIR
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        on_place(state, world, pos, old);
    }

    fn spawn_destroy_by_entity_particles(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _by_player: bool,
    ) {
    }
}

impl Behaviour for SoulFire {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        is_soul_fire_base(world.block_state(pos.below()))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if state.can_survive(world, pos) {
            state.default_state()
        } else {
            BlockState::AIR
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        on_place(state, world, pos, old);
    }

    fn spawn_destroy_by_entity_particles(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _by_player: bool,
    ) {
    }
}
