//! `RotatedPillarBlock`、`HayBlock`、`InfestedRotatedPillarBlock`、`ChainBlock`、
//! `WeatheringCopperChainBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{AXIS, BlockState, WATERLOGGED},
    world::World,
};

/// `RotatedPillarBlock`：木头这类方块，轴跟着点击的面走。
pub(super) struct RotatedPillar;

impl Behaviour for RotatedPillar {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block).with(AXIS, ctx.clicked_face().axis()))
    }
}

/// `ChainBlock`：能含水的柱子。
pub(super) struct Chain;

impl Behaviour for Chain {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pillar = RotatedPillar.state_for_placement(block, ctx, world)?;
        Some(pillar.with(WATERLOGGED, is_water_source(world, ctx.clicked_pos())))
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Axis;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockHit, InteractionResult,
        behaviour::building::test_support::{FLOOR, hit, level_with_floor, side_of, top_of},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn pillar_axis_follows_the_clicked_face() {
        for (face, axis) in [
            (Direction::Up, Axis::Y),
            (Direction::Down, Axis::Y),
            (Direction::North, Axis::Z),
            (Direction::South, Axis::Z),
            (Direction::West, Axis::X),
            (Direction::East, Axis::X),
        ] {
            let mut level = level_with_floor();
            let actor = actor_holding(items::OAK_LOG);
            let outcome = use_item(&mut level, &actor, side_or_top(face));
            assert_eq!(outcome.result, InteractionResult::Success);
            let placed = level.get(FLOOR.relative(face));
            assert!(placed.is(blocks::OAK_LOG));
            assert_eq!(placed.get(AXIS), axis);
        }
    }

    fn side_or_top(face: Direction) -> BlockHit {
        match face {
            Direction::Up => top_of(FLOOR),
            Direction::Down => hit(FLOOR, Direction::Down, 0.5, 0.0, 0.5),
            side => side_of(FLOOR, side, 0.5),
        }
    }

    #[test]
    fn pillar_subclasses_share_the_placement() {
        for (item, block) in [
            (items::HAY_BLOCK, blocks::HAY_BLOCK),
            (items::INFESTED_DEEPSLATE, blocks::INFESTED_DEEPSLATE),
        ] {
            let mut level = level_with_floor();
            use_item(
                &mut level,
                &actor_holding(item),
                side_of(FLOOR, Direction::East, 0.5),
            );
            let placed = level.get(FLOOR.east());
            assert!(placed.is(block));
            assert_eq!(placed.get(AXIS), Axis::X);
        }
    }

    #[test]
    fn chain_is_a_pillar_that_keeps_the_water() {
        let mut level = level_with_floor();
        level.set(FLOOR.east(), BlockState::WATER);
        use_item(
            &mut level,
            &actor_holding(items::IRON_CHAIN),
            side_of(FLOOR, Direction::East, 0.5),
        );
        let placed = level.get(FLOOR.east());
        assert!(placed.is(blocks::IRON_CHAIN));
        assert_eq!(placed.get(AXIS), Axis::X);
        assert!(placed.get(WATERLOGGED));
    }
}
