//! `CrafterBlock`。合成的计划刻和方块实体的内容不在范围里，世界里没有信号，所以只会被关掉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, update_neighbours_after_destroy},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CRAFTING, FrontAndTop, ORIENTATION, PropertyValue, TRIGGERED},
    world::{UPDATE_CLIENTS, World},
};

/// `CrafterBlock`。
pub(super) struct Crafter;

/// `FrontAndTop.fromFrontAndTop`：枚举的序列化名就是两个方向的名字。
fn front_and_top(front: Direction, top: Direction) -> FrontAndTop {
    FrontAndTop::from_name(&format!("{}_{}", front.name(), top.name()))
        .expect("the crafter only combines directions that FrontAndTop has")
}

impl Behaviour for Crafter {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let front = ctx.nearest_looking_direction().opposite();
        let top = match front {
            Direction::Down => ctx.horizontal_direction().opposite(),
            Direction::Up => ctx.horizontal_direction(),
            _ => Direction::Up,
        };
        Some(
            default_state(block)
                .with(ORIENTATION, front_and_top(front, top))
                .with(TRIGGERED, world.has_neighbor_signal(ctx.clicked_pos())),
        )
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        if !world.has_neighbor_signal(pos) && state.get(TRIGGERED) {
            let released = state.with(TRIGGERED, false).with(CRAFTING, false);
            world.set_block(pos, released, UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}
