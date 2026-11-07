//! 珊瑚。`BaseCoralPlantBlock`、`BaseCoralFanBlock`、`BaseCoralWallFanBlock` 是死的，
//! `CoralPlantBlock`、`CoralFanBlock`、`CoralWallFanBlock`、`CoralBlock` 是活的。
//!
//! 活珊瑚离开水后过一会儿变成死珊瑚。官方的延迟是 60 到 99 刻的随机数，这里固定取 [`DIE_DELAY`]。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, field_block, has_sturdy_wall, is_full_water, lost_wall_support,
            tick_water_if_waterlogged, wall_state_for_placement,
        },
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING, SupportType, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// 活珊瑚离开水到死去的计划刻延迟。
const DIE_DELAY: i32 = 80;

/// `CoralBlock.scanForWater`：六个邻居里有水。
fn has_water_nearby(world: &World, pos: BlockPos) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| world.fluid_state(pos.relative(side)).is_water())
}

/// `BaseCoralPlantTypeBlock.scanForWater`。
fn scan_for_water(state: BlockState, world: &World, pos: BlockPos) -> bool {
    state.get(WATERLOGGED) || has_water_nearby(world, pos)
}

/// `tryScheduleDieTick`。
fn try_schedule_die_tick(state: BlockState, world: &mut World, pos: BlockPos) {
    if !scan_for_water(state, world, pos) {
        world.schedule_tick(pos, state.block(), DIE_DELAY);
    }
}

/// `deadBlock` 的默认状态。
fn dead_block_of(state: BlockState) -> BlockState {
    default_state(field_block(state.block(), "deadBlock"))
}

/// `BaseCoralPlantTypeBlock.getStateForPlacement`。
fn placement_state(block: &'static Block, ctx: &PlaceContext, world: &World) -> BlockState {
    let full_water = is_full_water(world.fluid_state(ctx.clicked_pos()));
    default_state(block).with(WATERLOGGED, full_water)
}

/// `BaseCoralPlantTypeBlock`、`BaseCoralPlantBlock`、`BaseCoralFanBlock`：长在地上的死珊瑚。
pub(super) struct BaseCoralPlant;

impl Behaviour for BaseCoralPlant {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(placement_state(block, ctx, world))
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full)
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
        tick_water_if_waterlogged(state, world, pos);
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `CoralPlantBlock`、`CoralFanBlock`：长在地上的活珊瑚。
pub(super) struct CoralPlant;

impl Behaviour for CoralPlant {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        BaseCoralPlant.state_for_placement(block, ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        BaseCoralPlant.can_survive(state, world, pos)
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
        if direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        try_schedule_die_tick(state, world, pos);
        BaseCoralPlant.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        try_schedule_die_tick(state, world, pos);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !scan_for_water(state, world, pos) {
            let dead = dead_block_of(state).with(WATERLOGGED, false);
            world.set_block(pos, dead, UPDATE_CLIENTS);
        }
    }
}

/// `BaseCoralWallFanBlock`：贴在墙上的死珊瑚扇。
pub(super) struct BaseCoralWallFan;

impl Behaviour for BaseCoralWallFan {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        wall_state_for_placement(placement_state(block, ctx, world), ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sturdy_wall(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        if lost_wall_support(state, world, pos, direction) {
            BlockState::AIR
        } else {
            state
        }
    }
}

/// `CoralWallFanBlock`：贴在墙上的活珊瑚扇。
pub(super) struct CoralWallFan;

impl Behaviour for CoralWallFan {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        BaseCoralWallFan.state_for_placement(block, ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        BaseCoralWallFan.can_survive(state, world, pos)
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
        if lost_wall_support(state, world, pos, direction) {
            return BlockState::AIR;
        }
        try_schedule_die_tick(state, world, pos);
        BaseCoralWallFan.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        try_schedule_die_tick(state, world, pos);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !scan_for_water(state, world, pos) {
            let dead = dead_block_of(state)
                .with(WATERLOGGED, false)
                .with(HORIZONTAL_FACING, state.get(HORIZONTAL_FACING));
            world.set_block(pos, dead, UPDATE_CLIENTS);
        }
    }
}

/// `CoralBlock`：完整的珊瑚块。
pub(super) struct Coral;

impl Behaviour for Coral {
    fn before_placement(&self, block: &'static Block, ctx: &PlaceContext, world: &mut World) {
        let pos = ctx.clicked_pos();
        if !has_water_nearby(world, pos) {
            world.schedule_tick(pos, block, DIE_DELAY);
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
        if !has_water_nearby(world, pos) {
            world.schedule_tick(pos, state.block(), DIE_DELAY);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !has_water_nearby(world, pos) {
            world.set_block(pos, dead_block_of(state), UPDATE_CLIENTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockTicks, run_scheduled_ticks,
        testing::{TestLevel, actor_holding, top_hit},
        use_item_on,
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);
    const CORAL: BlockPos = FLOOR.above();

    /// 把 `item` 放在干地上。
    fn placed_on_dry_land(item: i32) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        let mut ticks = BlockTicks::new();
        let actor = actor_holding(item);
        use_item_on(&mut level, &mut ticks, &actor, Hand::Main, &top_hit(FLOOR));
        (level, ticks)
    }

    /// 官方的延迟是随机的，oracle 只比位置和类型，这里把延迟固定下来。
    #[test]
    fn a_coral_block_on_dry_land_dies_after_the_fixed_delay() {
        let (mut level, mut ticks) = placed_on_dry_land(items::TUBE_CORAL_BLOCK);
        run_scheduled_ticks(&mut level, &mut ticks, i64::from(DIE_DELAY) - 1);
        assert!(level.get(CORAL).is(blocks::TUBE_CORAL_BLOCK));
        run_scheduled_ticks(&mut level, &mut ticks, i64::from(DIE_DELAY));
        assert!(level.get(CORAL).is(blocks::DEAD_TUBE_CORAL_BLOCK));
    }

    #[test]
    fn a_coral_plant_on_dry_land_dies_after_the_fixed_delay() {
        let (mut level, mut ticks) = placed_on_dry_land(items::TUBE_CORAL);
        assert_eq!(level.get(CORAL).try_get(WATERLOGGED), Some(false));
        run_scheduled_ticks(&mut level, &mut ticks, i64::from(DIE_DELAY));
        let dead = BlockState(blocks::DEAD_TUBE_CORAL).with(WATERLOGGED, false);
        assert_eq!(level.get(CORAL), dead);
    }
}
