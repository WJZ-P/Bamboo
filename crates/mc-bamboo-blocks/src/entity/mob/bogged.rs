//! 沼骸：`AbstractSkeleton` 的目标和装备原样复用；生命 16、射击间隔 70 / 50（困难）、
//! 箭带 100 刻中毒，可以用剪刀剪下头上的蘑菇（`Shearable`）。
//!
//! Bogged reuse the `AbstractSkeleton` goals and equipment. They have 16 health, shoot
//! every 70 ticks (50 on hard), tip their arrows with 100 ticks of poison and can be
//! sheared for their mushrooms (`Shearable`).

use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{
    items,
    registries::{attribute, mob_effect},
};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    EntityLootRequest, Mob, MobEvent,
    attributes::Attributes,
    interact::{MobInteractRequest, MobInteraction},
};
use crate::{InteractionResult, entity::Entity, world::World};

/// `Bogged.getHardAttackInterval()` / `getAttackInterval()`。
pub(super) const HARD_ATTACK_INTERVAL: i32 = 50;
pub(super) const ATTACK_INTERVAL: i32 = 70;

/// `Bogged.getArrow`：`new MobEffectInstance(MobEffects.POISON, 100)`。
pub(super) const ARROW_EFFECT: (i32, i32) = (mob_effect::POISON, 100);

/// `Bogged.createAttributes`：骷髅的属性，生命改成 16。
///
/// `Bogged.createAttributes`: the skeleton attributes with 16 max health.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::skeleton();
    attributes.set_base(attribute::MAX_HEALTH, 16.0);
    attributes
}

/// `Bogged.mobInteract` 的剪刀分支：手里是剪刀且 `readyForShearing()`（还没剪过）时返回
/// `SUCCESS`，由调用方落地 [`apply_shear`]；否则交回 `Mob.mobInteract`（`PASS`）。
///
/// The shears branch of `Bogged.mobInteract`: shears on an unsheared bogged return
/// `SUCCESS` and the caller lands [`apply_shear`]; anything else falls through to
/// `Mob.mobInteract` (`PASS`).
pub(super) fn interact(mob: &Mob, req: &MobInteractRequest) -> MobInteraction {
    if req.held.is_empty() || req.held.item != items::SHEARS || mob.bogged_sheared {
        return MobInteraction::pass();
    }
    MobInteraction {
        result: InteractionResult::Success,
        shear: true,
        ..MobInteraction::pass()
    }
}

/// `Bogged.shear` 加上 `mobInteract` 之后的 `hurtAndBreak(1)`：音效、`shearing/bogged` 表、
/// 每堆掉落在 `getBbHeight()` 高处 `spawnAtLocation`（不另加速度），最后标成已剪。
///
/// `Bogged.shear` plus the later `hurtAndBreak(1)`: the sound, the `shearing/bogged` table,
/// one `spawnAtLocation` per stack at `getBbHeight()` (no extra velocity), then sheared.
pub(crate) fn apply_shear(world: &mut World, entity: &mut Entity, hand: Hand, tool: &ItemStack) {
    let Some(request) = entity.with_mob(|me, mob| {
        mob.events.push(MobEvent::PlayerSound {
            sound: "minecraft:entity.bogged.shear",
            position: me.position,
        });
        EntityLootRequest::new("minecraft:shearing/bogged", me, mob, Some(tool.clone()))
    }) else {
        return;
    };
    let height = f64::from(entity.dimensions.1);
    let at = request.origin.add(Vec3::new(0.0, height, 0.0));
    for drop in world.entity_loot_drops(request) {
        world.spawn_at_location(at, drop);
    }
    entity.with_mob(|_, mob| mob.bogged_sheared = true);
    crate::durability::hurt_hand(world, hand, 1);
}
