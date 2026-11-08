//! `SlabBlock`、`WeatheringCopperSlabBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::clicked_upper_half;
use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, FluidState, SLAB_TYPE, SlabType, WATERLOGGED},
    world::World,
};

/// `SlabBlock`。
pub(super) struct Slab;

impl Behaviour for Slab {
    /// `WeatheringCopperSlabBlock` 在随机刻里氧化，别的类不会被随机刻选中。
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
        let default = default_state(block);
        let replaced = world.block_state(pos);
        // 放在同种台阶上时合成双层，这时水会被挤出去。
        if replaced.is_same_block(default) {
            let merged = replaced
                .with(SLAB_TYPE, SlabType::Double)
                .with(WATERLOGGED, false);
            return Some(merged);
        }

        let slab = default
            .with(SLAB_TYPE, SlabType::Bottom)
            .with(WATERLOGGED, is_water_source(world, pos));
        let face = ctx.clicked_face();
        let bottom = face != Direction::Down && (face == Direction::Up || !clicked_upper_half(ctx));
        Some(if bottom {
            slab
        } else {
            slab.with(SLAB_TYPE, SlabType::Top)
        })
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        let slab_type = state.get(SLAB_TYPE);
        if slab_type == SlabType::Double || ctx.item != state.block().item {
            return false;
        }
        if !ctx.replacing_clicked_on_block() {
            return true;
        }
        let upper = clicked_upper_half(ctx);
        let face = ctx.clicked_face();
        if slab_type == SlabType::Bottom {
            face == Direction::Up || upper && face.axis().is_horizontal()
        } else {
            face == Direction::Down || !upper && face.axis().is_horizontal()
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
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 双层台阶是整格，装不了水。
    fn can_place_liquid(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        fluid: i32,
        by_player: bool,
    ) -> bool {
        state.get(SLAB_TYPE) != SlabType::Double
            && Plain.can_place_liquid(state, world, pos, fluid, by_player)
    }

    fn place_liquid(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        fluid: FluidState,
    ) -> bool {
        state.get(SLAB_TYPE) != SlabType::Double && Plain.place_liquid(state, world, pos, fluid)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, hit, level_with_floor, side_of, top_of},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn slab_half_follows_the_click() {
        let cases = [
            (top_of(FLOOR), Direction::Up, SlabType::Bottom),
            (
                hit(FLOOR, Direction::Down, 0.5, 0.0, 0.5),
                Direction::Down,
                SlabType::Top,
            ),
            (
                side_of(FLOOR, Direction::East, 0.75),
                Direction::East,
                SlabType::Top,
            ),
            (
                side_of(FLOOR, Direction::East, 0.25),
                Direction::East,
                SlabType::Bottom,
            ),
        ];
        for (click, face, slab_type) in cases {
            let mut level = level_with_floor();
            use_item(&mut level, &actor_holding(items::OAK_SLAB), click);
            let placed = level.get(FLOOR.relative(face));
            assert!(placed.is(blocks::OAK_SLAB));
            assert_eq!(placed.get(SLAB_TYPE), slab_type);
        }
    }

    #[test]
    fn slab_merges_when_clicked_on_the_open_half() {
        let actor = actor_holding(items::OAK_SLAB);
        let above = FLOOR.above();

        // 下半台阶的顶面。
        let mut level = level_with_floor();
        level.set(above, BlockState(blocks::OAK_SLAB));
        use_item(&mut level, &actor, hit(above, Direction::Up, 0.5, 0.5, 0.5));
        assert_eq!(level.get(above).get(SLAB_TYPE), SlabType::Double);

        // 下半台阶的侧面上半部分：点击落在空着的那一半。
        let mut level = level_with_floor();
        level.set(above, BlockState(blocks::OAK_SLAB));
        use_item(&mut level, &actor, side_of(above, Direction::East, 0.75));
        assert_eq!(level.get(above).get(SLAB_TYPE), SlabType::Double);

        // 上半台阶的底面。
        let mut level = level_with_floor();
        let top = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, SlabType::Top);
        level.set(above, top);
        use_item(
            &mut level,
            &actor,
            hit(above, Direction::Down, 0.5, 0.5, 0.5),
        );
        assert_eq!(level.get(above).get(SLAB_TYPE), SlabType::Double);
    }

    #[test]
    fn slab_does_not_merge_with_a_different_click() {
        let above = FLOOR.above();
        let actor = actor_holding(items::OAK_SLAB);

        // 下半台阶的侧面下半部分：不合成，放在旁边。
        let mut level = level_with_floor();
        level.set(above, BlockState(blocks::OAK_SLAB));
        use_item(&mut level, &actor, side_of(above, Direction::East, 0.25));
        assert_eq!(level.get(above).get(SLAB_TYPE), SlabType::Bottom);
        assert!(level.get(above.east()).is(blocks::OAK_SLAB));

        // 别的台阶不合成，而是放在上面。
        let mut level = level_with_floor();
        level.set(above, BlockState(blocks::STONE_SLAB));
        use_item(&mut level, &actor, hit(above, Direction::Up, 0.5, 0.5, 0.5));
        assert!(level.get(above).is(blocks::STONE_SLAB));
        assert!(level.get(above.above()).is(blocks::OAK_SLAB));
    }

    #[test]
    fn slab_merges_from_the_neighbouring_block() {
        // 点在下面的石头上，放置位置已经有下半台阶：`can_place` 走不点中的分支。
        let mut level = level_with_floor();
        level.set(FLOOR.above(), BlockState(blocks::OAK_SLAB));
        use_item(&mut level, &actor_holding(items::OAK_SLAB), top_of(FLOOR));
        assert_eq!(level.get(FLOOR.above()).get(SLAB_TYPE), SlabType::Double);
    }

    #[test]
    fn slab_in_water_is_waterlogged_until_it_merges() {
        let above = FLOOR.above();
        let actor = actor_holding(items::OAK_SLAB);

        let mut level = level_with_floor();
        level.set(above, BlockState::WATER);
        use_item(&mut level, &actor, top_of(FLOOR));
        let placed = level.get(above);
        assert!(placed.get(WATERLOGGED));
        assert_eq!(placed.get(SLAB_TYPE), SlabType::Bottom);

        use_item(&mut level, &actor, hit(above, Direction::Up, 0.5, 0.5, 0.5));
        let merged = level.get(above);
        assert_eq!(merged.get(SLAB_TYPE), SlabType::Double);
        assert!(!merged.get(WATERLOGGED));
    }

    #[test]
    fn double_slab_is_never_replaced() {
        let above = FLOOR.above();
        let mut level = level_with_floor();
        let double = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, SlabType::Double);
        level.set(above, double);
        use_item(
            &mut level,
            &actor_holding(items::OAK_SLAB),
            hit(above, Direction::Up, 0.5, 1.0, 0.5),
        );
        assert_eq!(level.get(above), double);
        assert!(level.get(above.above()).is(blocks::OAK_SLAB));
    }
}
