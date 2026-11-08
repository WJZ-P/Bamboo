//! `CopperBulbBlock`、`WeatheringCopperBulbBlock`、`CopperGolemStatueBlock`、
//! `WeatheringCopperGolemStatueBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, items, tags::item_tags};

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour,
        common::{
            default_state, field_name, is_water_source, tick_water_if_waterlogged,
            update_neighbours_after_destroy,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, COPPER_GOLEM_POSE, CopperGolemPose, HORIZONTAL_FACING, LIT, POWERED,
        WATERLOGGED, item_is_tagged,
    },
    world::World,
};

/// `CopperBulbBlock`。
pub(super) struct CopperBulb;

/// `checkAndFlip`。
fn check_and_flip(state: BlockState, world: &mut World, pos: BlockPos) {
    let signal = world.has_neighbor_signal(pos);
    if signal == state.get(POWERED) {
        return;
    }
    let flipped = if state.get(POWERED) {
        state
    } else {
        state.cycle(LIT)
    };
    world.set_block_and_update(pos, flipped.with(POWERED, signal));
}

/// `WeatheringCopperFullBlock`：只有氧化。
pub(super) struct WeatheringCopperFull;

impl Behaviour for WeatheringCopperFull {
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }
}

impl Behaviour for CopperBulb {
    /// `WeatheringCopperBulbBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            check_and_flip(state, world, pos);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_and_flip(state, world, pos);
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(LIT) { 15 } else { 0 }
    }
}

/// `CopperGolemStatueBlock`。
pub(super) struct CopperGolemStatue;

/// `updatePose`。
fn update_pose(state: BlockState, world: &mut World, pos: BlockPos) {
    world.set_block_and_update(pos, state.cycle(COPPER_GOLEM_POSE));
}

/// `getAnalogOutputSignal`：姿势的序号加一，顺序和官方的 `Pose` 一样。
fn pose_signal(state: BlockState) -> i32 {
    match state.get(COPPER_GOLEM_POSE) {
        CopperGolemPose::Standing => 1,
        CopperGolemPose::Sitting => 2,
        CopperGolemPose::Running => 3,
        CopperGolemPose::Star => 4,
    }
}

impl Behaviour for CopperGolemStatue {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite())
                .with(WATERLOGGED, waterlogged),
        )
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
        tick_water_if_waterlogged(state, world, pos);
        state
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if item_is_tagged(ctx.item, item_tags::AXES) {
            return InteractionResult::Pass;
        }
        update_pose(state, world, ctx.clicked_pos());
        InteractionResult::Success
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        pose_signal(state)
    }
}

/// `WeatheringCopperGolemStatueBlock`：会氧化的雕像。
pub(super) struct WeatheringCopperGolemStatue;

/// 没有氧化的雕像被斧头点中时变回铜傀儡。实体不在范围里，方块和官方成功路径一样消失。
///
/// An unaffected statue hit with an axe turns back into a copper golem. The entity is out of scope; the block disappears, matching vanilla's success path.
fn revive_statue(state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult {
    if field_name(state.block(), "getAge()") != "unaffected" {
        return InteractionResult::Pass;
    }
    // 官方先 `hurtAndBreak(1)`，傀儡生成成功才 `removeBlock`。这里没有方块实体，方块总会消失。
    //
    // Vanilla calls `hurtAndBreak(1)` first and `removeBlock` only when the golem exists. There is no block entity here, so the block always disappears.
    crate::durability::hurt_hand(world, ctx.hand, 1);
    world.remove_block(ctx.clicked_pos());
    InteractionResult::Success
}

impl Behaviour for WeatheringCopperGolemStatue {
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        CopperGolemStatue.state_for_placement(block, ctx, world)
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
        CopperGolemStatue.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        CopperGolemStatue.affect_neighbors_after_removal(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        if item_is_tagged(ctx.item, item_tags::AXES) {
            return revive_statue(state, world, ctx);
        }
        if ctx.item == items::HONEYCOMB {
            return InteractionResult::Pass;
        }
        update_pose(state, world, pos);
        InteractionResult::Success
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        pose_signal(state)
    }
}
