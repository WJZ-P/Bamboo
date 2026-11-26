//! 船物品的 `Item.use`，对应官方 `BoatItem.use`。

use mc_bamboo_core::Hand;

use crate::{
    CollisionContext,
    context::Actor,
    entity::Entity,
    interact::InteractionResult,
    item_exchange::ItemExchange,
    world::{ClipFluid, World},
};

/// `BoatItem.use`：用 `ClipContext.Fluid.ANY` 的视线找第一个方块或流体，命中后在
/// 命中点生成船，朝向取玩家的 `yRot`，再做一次 `noCollision` 校验。没命中返回
/// `PASS`，碰撞检查不过返回 `FAIL`；成功时生存模式消耗一个物品。
///
/// Vanilla `BoatItem.use`: clip with `Fluid.ANY`, spawn the boat at the hit
/// location with the player's yaw, then check `noCollision`. A miss returns
/// `PASS`, a blocked spawn `FAIL`; success consumes one item in survival.
pub(crate) fn use_in_air(world: &mut World, actor: &Actor, hand: Hand) -> InteractionResult {
    let item = actor.item_in(hand);
    let Some(entity_type) = crate::entity::boat::entity_type_for_item(item) else {
        return InteractionResult::Pass;
    };
    let (from, to) = actor.view_segment();
    // `getPlayerPOVHitResult(level, player, Fluid.ANY)`：没碰到东西就是 MISS。
    let Some(hit) = world.clip(from, to, ClipFluid::Any, item) else {
        return InteractionResult::Pass;
    };
    let mut boat = Entity::boat(entity_type, hit.location);
    boat.y_rot = actor.yaw;
    // `Level.noCollision(boat, boat.getBoundingBox())`。实体之间的碰撞 Bamboo
    // 不建模；5 格内 `CAN_BE_PICKED` 实体的检查由游戏层负责。
    if !world
        .block_collisions(boat.bounding_box(), CollisionContext::Empty)
        .is_empty()
    {
        return InteractionResult::Fail;
    }
    world.spawn_entity(boat);
    // `ItemStack.consume(1, player)`：生存模式减一个。
    world.set_item_exchange(ItemExchange::Shrink);
    InteractionResult::Success
}
