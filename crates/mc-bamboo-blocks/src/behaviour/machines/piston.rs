//! `PistonBaseBlock`、`PistonHeadBlock`、`MovingPistonBlock`。
//!
//! 活塞要信号才动：`checkIfExtend` 只会发方块事件，事件什么也不改，这里不写。
//! 伸出的活塞只能靠别的途径写进世界，但是活塞头和底座互相拆的逻辑还是要有。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use super::facing_the_view;
use crate::{
    behaviour::{Behaviour, Plain},
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, EXTENDED, FACING, PISTON_TYPE, PistonType},
    world::{UPDATE_LIMIT, World},
};

/// `PistonBaseBlock`。
pub(super) struct PistonBase;

impl Behaviour for PistonBase {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx).with(EXTENDED, false))
    }
}

/// `PistonHeadBlock`。
pub(super) struct PistonHead;

/// 活塞头背后的那一格。
fn base_pos(arm: BlockState, pos: BlockPos) -> BlockPos {
    pos.relative(arm.get(FACING).opposite())
}

/// `isFittingBase`。
fn is_fitting_base(arm: BlockState, base: BlockState) -> bool {
    let base_block = if arm.get(PISTON_TYPE) == PistonType::Normal {
        blocks::PISTON
    } else {
        blocks::STICKY_PISTON
    };
    base.is(base_block) && base.get(EXTENDED) && base.get(FACING) == arm.get(FACING)
}

impl Behaviour for PistonHead {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let base = world.block_state(base_pos(state, pos));
        is_fitting_base(state, base)
            || base.is(blocks::MOVING_PISTON) && base.get(FACING) == state.get(FACING)
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
        if direction.opposite() == state.get(FACING) && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let base = base_pos(state, pos);
        if is_fitting_base(state, world.block_state(base)) {
            world.destroy_block(base, UPDATE_LIMIT);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        if state.can_survive(world, pos) {
            world.neighbor_changed(base_pos(state, pos), source);
        }
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        let base = base_pos(state, pos);
        if is_fitting_base(state, world.block_state(base)) {
            world.destroy_block(base, UPDATE_LIMIT);
        }
        Plain.player_will_destroy(state, world, pos, player)
    }
}

/// `MovingPistonBlock`。方块实体只在活塞动的时候才有，这里永远没有，右键就把它拿走。
pub(super) struct MovingPiston;

impl Behaviour for MovingPiston {
    fn use_without_item(
        &self,
        _state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        world.remove_block(ctx.clicked_pos());
        InteractionResult::Consume
    }
}
