//! `SkullBlock`、`PlayerHeadBlock`、`WitherSkullBlock`，以及挂墙的 `WallSkullBlock`、
//! `PlayerWallHeadBlock`、`PiglinWallSkullBlock`、`WitherWallSkullBlock`。
//!
//! 官方的凋灵头颅在 `setPlacedBy` 里检查召唤凋灵的图案；`checkSpawn` 复用南瓜图案的
//! 清方块流程并生成一只带无敌期的凋灵。别的头颅放下时什么都不做。
//!
//! Vanilla wither skulls run the summon check in `setPlacedBy`; `checkSpawn` reuses
//! the pumpkin pattern clearing and spawns a wither in its invulnerability phase.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, rotation_segment},
    },
    context::{Actor, PlaceContext},
    state::{BlockState, HORIZONTAL_FACING, POWERED, ROTATION_16},
    world::{UPDATE_CLIENTS, World},
};

/// `AbstractSkullBlock.getStateForPlacement`。
fn base_state(block: &Block, ctx: &PlaceContext, world: &World) -> BlockState {
    default_state(block).with(POWERED, world.has_neighbor_signal(ctx.clicked_pos()))
}

/// `AbstractSkullBlock.neighborChanged`。
fn follow_signal(state: BlockState, world: &mut World, pos: BlockPos) {
    let signal = world.has_neighbor_signal(pos);
    if signal != state.get(POWERED) {
        world.set_block(pos, state.with(POWERED, signal), UPDATE_CLIENTS);
    }
}

/// `SkullBlock`。
pub(super) struct Skull;

impl Behaviour for Skull {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let rotation = rotation_segment(ctx.rotation());
        Some(base_state(block, ctx, world).with(ROTATION_16, rotation))
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        follow_signal(state, world, pos);
    }

    /// `WitherSkullBlock.setPlacedBy`：只有凋灵头颅才走 `checkSpawn`，别的头颅是空操作。
    ///
    /// `WitherSkullBlock.setPlacedBy`: only wither skulls run `checkSpawn`.
    fn set_placed_by(&self, _state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        crate::behaviour::machines::pumpkin::check_spawn_wither(world, pos);
    }
}

/// `WallSkullBlock`。
pub(super) struct WallSkull;

impl Behaviour for WallSkull {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        ctx.nearest_looking_directions()
            .into_iter()
            .filter(|direction| direction.axis().is_horizontal())
            .find(|&direction| {
                !world
                    .block_state(pos.relative(direction))
                    .can_be_replaced(ctx, world)
            })
            .map(|direction| {
                base_state(block, ctx, world).with(HORIZONTAL_FACING, direction.opposite())
            })
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        follow_signal(state, world, pos);
    }

    /// `WitherWallSkullBlock.setPlacedBy`：墙上头颅走同一条 `checkSpawn`。
    ///
    /// `WitherWallSkullBlock.setPlacedBy`: wall skulls use the same `checkSpawn`.
    fn set_placed_by(&self, _state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        crate::behaviour::machines::pumpkin::check_spawn_wither(world, pos);
    }
}
