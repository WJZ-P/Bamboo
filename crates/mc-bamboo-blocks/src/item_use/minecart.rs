//! 矿车物品的 `useOn`：官方 `MinecartItem.useOn`。
//!
//! 只有点在铁轨上才放得下，斜坡再抬高半格；生成一辆对应种类的矿车并扣一个物品。

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{BlockAncestor, tags::block_tags};

use crate::{
    context::UseContext,
    entity::{Entity, minecart::MinecartKind},
    game_event::{GameEvent, dispatch::EventContext},
    interact::InteractionResult,
    item_exchange::ItemExchange,
    state::{RAIL_SHAPE, RailShape},
    world::World,
};

/// `RailShape.isSlope`。
const fn is_slope(shape: RailShape) -> bool {
    matches!(
        shape,
        RailShape::AscendingNorth
            | RailShape::AscendingEast
            | RailShape::AscendingSouth
            | RailShape::AscendingWest
    )
}

/// `MinecartItem.useOn`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    let state = world.block_state(pos);
    if !state.is_tagged(block_tags::RAILS) {
        return InteractionResult::Fail;
    }
    let shape = if state.is_a(BlockAncestor::BaseRailBlock) {
        state.try_get(RAIL_SHAPE).unwrap_or(RailShape::NorthSouth)
    } else {
        RailShape::NorthSouth
    };
    let rail_height = if is_slope(shape) { 0.5 } else { 0.0 };
    let position = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.0625 + rail_height,
        f64::from(pos.z) + 0.5,
    );
    let Some(kind) = MinecartKind::from_item(ctx.item) else {
        return InteractionResult::Fail;
    };
    let entity = world.prepare_entity(Entity::minecart(kind, position));
    world.add_prepared_entity(entity);
    world.game_event(
        GameEvent::EntityPlace,
        position,
        EventContext {
            source_player: Some(ctx.actor.uid),
            ..Default::default()
        },
    );
    world.set_item_exchange(ItemExchange::Shrink);
    InteractionResult::Success
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{Actor, BlockState, BlockTicks, context::BlockHit, testing::TestLevel};

    fn actor(item: i32) -> Actor {
        Actor {
            uid: 1,
            uuid: [0; 16],
            position: Vec3::new(8.5, 64.0, 8.5),
            yaw: 0.0,
            head_yaw: 0.0,
            pitch: 0.0,
            secondary_use: false,
            main_hand: item,
            off_hand: items::AIR,
            eye_height: 1.62,
            block_interaction_range: 5.0,
            may_build: true,
            prevents_block_drops: false,
            can_use_game_master_blocks: false,
        }
    }

    fn hit(pos: BlockPos) -> BlockHit {
        BlockHit {
            pos,
            face: Direction::Up,
            location: Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ),
            inside: false,
        }
    }

    fn rail(shape: RailShape) -> BlockState {
        BlockState(blocks::RAIL).with(RAIL_SHAPE, shape)
    }

    #[test]
    fn a_flat_rail_places_at_the_rail_height() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, rail(RailShape::NorthSouth));
        let a = actor(items::MINECART);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1);
        let public = level.entities().public_at(ids[0]).unwrap();
        assert_eq!(public.position, Vec3::new(0.5, 64.0625, 0.5));
    }

    #[test]
    fn a_sloped_rail_adds_half_a_block() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, rail(RailShape::AscendingEast));
        let a = actor(items::CHEST_MINECART);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1);
        let public = level.entities().public_at(ids[0]).unwrap();
        assert_eq!(public.position, Vec3::new(0.5, 64.5625, 0.5));
    }

    #[test]
    fn a_non_rail_fails_without_spawning() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, BlockState(blocks::STONE));
        let a = actor(items::MINECART);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Fail);
        let _ = world.into_effects();
        assert!(level.entities().ids().is_empty());
    }

    #[test]
    fn all_six_items_place_their_own_kind() {
        let cases = [
            items::MINECART,
            items::CHEST_MINECART,
            items::FURNACE_MINECART,
            items::TNT_MINECART,
            items::HOPPER_MINECART,
            items::COMMAND_BLOCK_MINECART,
        ];
        for item in cases {
            let mut level = TestLevel::new();
            let mut ticks = BlockTicks::default();
            let pos = BlockPos::new(0, 64, 0);
            level.set(pos, rail(RailShape::NorthSouth));
            let a = actor(item);
            let mut world = World::new(&mut level, &mut ticks);
            let ctx = UseContext::new(&a, Hand::Main, hit(pos));
            assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
            let _ = world.into_effects();
            let ids = level.entities().ids();
            assert_eq!(ids.len(), 1, "one cart for item {item}");
            let public = level.entities().public_at(ids[0]).unwrap();
            assert_eq!(
                public.kind.entity_type(),
                MinecartKind::from_item(item).unwrap().entity_type(),
            );
        }
    }
}
