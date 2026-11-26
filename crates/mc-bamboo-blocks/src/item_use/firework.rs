//! 烟花火箭的 `useOn`：`FireworkRocketItem.useOn`。

use mc_bamboo_core::Vec3;
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    context::UseContext,
    entity::firework_rocket::{FireworkShot, FireworkShoot},
    interact::InteractionResult,
    item_exchange::ItemExchange,
    world::World,
};

/// `FireworkRocketItem.useOn`：对着方块右键，在点击位置沿点击面外移 0.15 格生成烟花，
/// 主人是玩家，扣一个物品。`player.isFallFlying()` 时官方返回 `PASS`；Bamboo 还没有
/// 鞘翅滑翔，这个条件恒为假，所以照官方写清楚后直接往下走。
///
/// `FireworkRocketItem.useOn`: spawn a firework 0.15 blocks along the clicked face from
/// the click location, owned by the player, and shrink the stack. Vanilla returns `PASS`
/// while the player is fall flying; Bamboo has no elytra gliding yet, so that condition is
/// always false and this proceeds straight through.
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let (sx, sy, sz) = ctx.clicked_face().normal();
    let click = ctx.click_location();
    let position = Vec3::new(
        click.x + f64::from(sx) * 0.15,
        click.y + f64::from(sy) * 0.15,
        click.z + f64::from(sz) * 0.15,
    );
    let stack = ctx
        .stack
        .cloned()
        .unwrap_or_else(|| ItemStack::new(items::FIREWORK_ROCKET, 1));
    world.spawn_firework(&FireworkShot {
        position,
        owner: Some(ctx.actor.uid),
        owner_uuid: Some(ctx.actor.uuid),
        item: stack,
        shot_at_angle: false,
        shoot: None,
    });
    world.set_item_exchange(ItemExchange::Shrink);
    InteractionResult::Success
}

/// 发射器那一支的 `asProjectile` + `shoot`：斜射（`shotAtAngle`）的烟花。
///
/// The dispenser's `asProjectile` + `shoot`: a firework shot at an angle.
pub(crate) fn spawn_dispensed(
    world: &mut World,
    position: Vec3,
    facing: (i32, i32, i32),
    stack: &ItemStack,
) {
    let (sx, sy, sz) = facing;
    let direction = Vec3::new(f64::from(sx), f64::from(sy), f64::from(sz));
    world.spawn_firework(&FireworkShot {
        position,
        owner: None,
        owner_uuid: None,
        item: crate::container::copy_with_count(stack, 1),
        shot_at_angle: true,
        shoot: Some(FireworkShoot {
            direction,
            power: 0.5,
            uncertainty: 1.0,
        }),
    });
}
