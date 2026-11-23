//! 刷怪蛋的 `useOn`：`EntityType.spawn` 走 [`crate::entity::mob::spawn`]。

use crate::{
    context::UseContext,
    entity::mob::{MobType, spawn},
    interact::InteractionResult,
    world::World,
};

/// 牛的刷怪蛋。官方 `SpawnEggItem.useOn` 在 `mayUseItemAt` 通过之后调
/// `spawnMob`，成功时消耗一个物品。
///
/// A cow spawn egg. Vanilla's `SpawnEggItem.useOn` calls `spawnMob` after
/// `mayUseItemAt` passes and consumes one item on success.
pub(super) fn cow(world: &mut World, ctx: &UseContext) -> InteractionResult {
    use_egg(world, ctx, MobType::Cow)
}

/// 僵尸 / 尸壳刷怪蛋走同一个 `useOn` 入口。
pub(super) fn monster(world: &mut World, ctx: &UseContext, kind: MobType) -> InteractionResult {
    use_egg(world, ctx, kind)
}

pub(super) fn use_egg(world: &mut World, ctx: &UseContext, kind: MobType) -> InteractionResult {
    let clicked = ctx.clicked_pos();
    let face = ctx.clicked_face();
    let state = world.block_state(clicked);
    // `SpawnEggItem.useOn` 在流体方块上直接放过。
    // `SpawnEggItem.useOn` refuses to spawn on a fluid block.
    if !state.fluid().is_empty() {
        return InteractionResult::Pass;
    }
    // `getCollisionShape(level, clickedPos).isEmpty()`：碰撞形状为空才在被点格生成，
    // 否则沿点击面挪一格。`offsetMore` 只在真的挪了、而且点的是顶面时为真，
    // 让 `getYOffset` 往下多扫一格（点在实心方块顶面时给实体留出落脚的地方）。
    //
    // Vanilla generates in the clicked cell only when its collision shape is empty;
    // otherwise it shifts one cell along the clicked face. `shouldOffsetYMore` is true
    // only for a shift off an `UP` face, making `getYOffset` scan one cell further down.
    let empty = crate::shape::block_collision_boxes(
        state,
        clicked,
        crate::CollisionContext::Empty,
        world.moving_piston_shape(clicked).as_deref(),
        world.shulker_box_shape(clicked),
    )
    .is_empty();
    let spawn_pos = if empty {
        clicked
    } else {
        clicked.relative(face)
    };
    let offset_more = spawn_pos != clicked && face == mc_bamboo_core::Direction::Up;
    spawn::spawn_egg(world, kind, spawn_pos, true, offset_more, None);
    // `SpawnEggItem.spawnMob` 的 `itemStack.consume(1, null)`：生存模式减一个。
    // `SpawnEggItem.spawnMob`'s `itemStack.consume(1, null)`: shrink in survival.
    world.set_item_exchange(crate::item_exchange::ItemExchange::Shrink);
    InteractionResult::Success
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockState, BlockTicks,
        context::BlockHit,
        testing::{TestLevel, actor_holding, top_hit},
    };

    /// 点中 `pos` 的某一面，返回新生成实体的位置。
    fn spawn_at(
        pos: BlockPos,
        face: Direction,
        state: BlockState,
        extra: &[(BlockPos, BlockState)],
    ) -> Vec3 {
        let mut level = TestLevel::new();
        level.set(pos, state);
        for &(at, what) in extra {
            level.set(at, what);
        }
        let actor = actor_holding(items::COW_SPAWN_EGG);
        let mut ticks = BlockTicks::default();
        let outcome = {
            let mut world = World::new(&mut level, &mut ticks);
            let hit = BlockHit {
                pos,
                face,
                location: Vec3::new(
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y) + 0.5,
                    f64::from(pos.z) + 0.5,
                ),
                inside: false,
            };
            let ctx = UseContext::new(&actor, Hand::Main, hit);
            let result = use_egg(&mut world, &ctx, MobType::Cow);
            assert_eq!(result, InteractionResult::Success);
            let _ = world.into_effects();
            result
        };
        assert_eq!(outcome, InteractionResult::Success);
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1, "只生成一头牛");
        level.entities().public_at(ids[0]).unwrap().position
    }

    /// 被点方块有碰撞形状时沿点击面挪一格。相邻格是空气，`getYOffset` 让牛落到格底。
    #[test]
    fn clicking_a_solid_side_spawns_in_the_adjacent_cell() {
        let clicked = BlockPos::new(3, 64, 5);
        let position = spawn_at(clicked, Direction::West, BlockState(blocks::STONE), &[]);
        assert_eq!(position, Vec3::new(2.5, 64.0, 5.5));
    }

    /// 点实心方块顶面并挪一格时 `shouldOffsetYMore` 为真：被点格是半砖时，
    /// 往下多扫的一格让牛落在半砖顶面（纵坐标比生成格还低半格）。
    #[test]
    fn clicking_a_solid_top_spawns_above_with_the_extra_drop() {
        let clicked = BlockPos::new(3, 64, 5);
        let slab = BlockState(blocks::STONE_SLAB).default_state();
        let position = spawn_at(clicked, Direction::Up, slab, &[]);
        assert_eq!(position, Vec3::new(3.5, 64.5, 5.5));
    }

    /// 碰撞形状为空的方块：生成在被点格自己。
    #[test]
    fn clicking_a_block_without_collision_shape_spawns_in_that_cell() {
        let clicked = BlockPos::new(3, 64, 5);
        let grass = BlockState(blocks::SHORT_GRASS).default_state();
        assert!(mc_bamboo_data::collision_shape(grass.0).is_empty());
        let position = spawn_at(clicked, Direction::Up, grass, &[]);
        assert_eq!(position, Vec3::new(3.5, 64.0, 5.5));
    }

    /// 台阶不满一格：挪过去的那一格是台阶时，牛落在台阶顶面而不是格底。
    #[test]
    fn a_slab_in_the_spawn_cell_holds_the_mob_at_its_height() {
        let clicked = BlockPos::new(3, 64, 5);
        let spawn_pos = clicked.relative(Direction::West);
        let slab = BlockState(blocks::STONE_SLAB).default_state();
        let position = spawn_at(
            clicked,
            Direction::West,
            BlockState(blocks::STONE),
            &[(spawn_pos, slab)],
        );
        assert_eq!(position, Vec3::new(2.5, 64.5, 5.5));
    }

    /// 流体方块直接放过，不生成实体。
    #[test]
    fn a_fluid_click_does_not_spawn() {
        let pos = BlockPos::new(0, 64, 0);
        let mut level = TestLevel::new();
        level.set(pos, BlockState(blocks::WATER).default_state());
        let actor = actor_holding(items::COW_SPAWN_EGG);
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&actor, Hand::Main, top_hit(pos));
        assert_eq!(
            use_egg(&mut world, &ctx, MobType::Cow),
            InteractionResult::Pass
        );
        assert!(level.entities().ids().is_empty());
    }
}
