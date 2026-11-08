//! `IronBarsBlock`、`StainedGlassPaneBlock`、`WeatheringCopperBarsBlock`：
//! 铁栏杆、玻璃板和铜栏杆，这些方块互相连接。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, tags::block_tags};

use super::has_full_face;
use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_exception_for_connection, is_water_source, pipe_property,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BlockState, WATERLOGGED},
    world::World,
};

/// `IronBarsBlock`。
pub(super) struct IronBars;

/// `attachsTo`。`face` 是 `other` 朝着栏杆的面。
fn attaches_to(other: BlockState, face: Direction) -> bool {
    !is_exception_for_connection(other) && has_full_face(other, face)
        || other.is_a(BlockAncestor::IronBarsBlock)
        || other.is_tagged(block_tags::WALLS)
}

impl Behaviour for IronBars {
    /// `WeatheringCopperBarsBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let state = Direction::HORIZONTAL
            .into_iter()
            .fold(default_state(block), |state, side| {
                let neighbor = world.block_state(pos.relative(side));
                state.with(pipe_property(side), attaches_to(neighbor, side.opposite()))
            });
        Some(state.with(WATERLOGGED, is_water_source(world, pos)))
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
        if direction.axis().is_horizontal() {
            state.with(
                pipe_property(direction),
                attaches_to(neighbor, direction.opposite()),
            )
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        state::{NORTH, SOUTH},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn full_faces_on_the_exception_list_do_not_attach() {
        let mut level = level_with_floor();
        let pane = FLOOR.above();
        level.set(pane.north(), BlockState(blocks::PUMPKIN));
        level.set(pane.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_holding(items::GLASS_PANE), top_of(FLOOR));
        let placed = level.get(pane);
        assert!(!placed.get(NORTH));
        assert!(placed.get(SOUTH));
    }
}
