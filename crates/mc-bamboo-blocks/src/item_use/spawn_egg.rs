//! 刷怪蛋的 `useOn`：`EntityType.spawn` 走 [`crate::entity::mob::spawn`]。

use crate::{
    context::UseContext,
    entity::mob::{MobType, spawn},
    interact::InteractionResult,
    world::World,
};

/// 牛的刷怪蛋。官方 `SpawnEggItem.useOn` 在 `mayUseItemAt` 通过之后调
/// `spawnMob`，成功时消耗一个物品；`EntityType.spawn` 的 `offsetMore` 是 false。
///
/// A cow spawn egg. Vanilla's `SpawnEggItem.useOn` calls `spawnMob` after
/// `mayUseItemAt` passes and consumes one item on success; `EntityType.spawn`
/// passes `offsetMore = false`.
pub(super) fn cow(world: &mut World, ctx: &UseContext) -> InteractionResult {
    use_egg(world, ctx, MobType::Cow)
}

/// 僵尸 / 尸壳刷怪蛋走同一个 `useOn` 入口。
pub(super) fn monster(world: &mut World, ctx: &UseContext, kind: MobType) -> InteractionResult {
    use_egg(world, ctx, kind)
}

fn use_egg(world: &mut World, ctx: &UseContext, kind: MobType) -> InteractionResult {
    let pos = ctx.clicked_pos();
    // `SpawnEggItem.useOn` 在流体方块上直接放过。
    // `SpawnEggItem.useOn` refuses to spawn on a fluid block.
    if !world.block_state(pos).fluid().is_empty() {
        return InteractionResult::Pass;
    }
    spawn::spawn_egg(world, kind, pos, false, None);
    // `SpawnEggItem.spawnMob` 的 `itemStack.consume(1, null)`：生存模式减一个。
    // `SpawnEggItem.spawnMob`'s `itemStack.consume(1, null)`: shrink in survival.
    world.set_item_exchange(crate::item_exchange::ItemExchange::Shrink);
    InteractionResult::Success
}
