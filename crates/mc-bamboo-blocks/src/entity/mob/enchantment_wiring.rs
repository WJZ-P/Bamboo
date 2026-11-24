//! 生物侧把 [`super::enchantment_effects`] 接进一刻：跨格、换装备、每刻 tick，
//! 以及把动作落到属性表、世界方块和命令上。
//!
//! Mob-side wiring of [`super::enchantment_effects`] into a tick: block changes, equipment
//! changes and per-tick effects, plus applying actions to attributes, world blocks and
//! commands. Without relevant enchantments every entry point is a no-op (no RNG draws,
//! no state or packet changes), so existing scenes keep their random streams.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, tags::block_tags};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use crate::{
    entity::{
        Entity,
        ctx::{BlockView, Command, EntityCtx},
    },
    shape::Aabb,
};

use super::{Mob, MobEvent, attributes::Modifier, enchantment_effects as fx, equipment};

/// 构造官方 `ENCHANTED_LOCATION` / `ENCHANTED_ENTITY` 上下文里要用的实体快照。
///
/// Build the entity snapshot used by the official loot contexts.
pub fn entity_state(me: &Entity, view: &dyn BlockView) -> fx::EntityState {
    let below = me.on_pos(view, 0.500001_f32);
    fx::EntityState {
        position: me.position,
        on_ground: me.on_ground,
        // 生物没有创造模式飞行标志。
        // Mobs have no creative-flight flag.
        is_flying: false,
        has_vehicle: me.vehicle.is_some(),
        horizontal_speed: (me.delta.x * me.delta.x + me.delta.z * me.delta.z).sqrt(),
        tick_count: me.tick_count,
        on_soul_speed_block: view
            .block_state(below)
            .is_tagged(block_tags::SOUL_SPEED_BLOCKS),
    }
}

/// 按官方 `EquipmentSlot.VALUES` 顺序整理当前装备上本框架认识的附魔。
///
/// Collect recognised enchantments on current equipment in vanilla slot order.
pub fn equipped(mob: &Mob) -> Vec<(EquipmentSlot, &'static str, i32)> {
    let stacks: Vec<(EquipmentSlot, &ItemStack)> = equipment::SLOTS
        .iter()
        .map(|&slot| (slot, mob.equipment.get(slot)))
        .collect();
    fx::equipped_enchantments(&stacks)
}

/// `LivingEntity.onChangedBlock` → `EnchantmentHelper.runLocationChangedEffects`（全装备）。
///
/// `LivingEntity.onChangedBlock` → `EnchantmentHelper.runLocationChangedEffects`.
pub fn run_location(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let enchants = equipped(mob);
    if enchants.is_empty() && mob.active_location_effects == fx::ActiveLocationEffects::default() {
        return;
    }
    let state = entity_state(me, ctx.blocks());
    let actions = {
        // `random_chance` 用官方 `level.getRandom()`；只有精确模式有世界随机源。
        // `random_chance` uses vanilla `level.getRandom()`; only precise mode has it.
        let mut rng = || ctx.blocks().level_random_float();
        fx::run_location_changed(
            &state,
            &enchants,
            &mut mob.active_location_effects,
            &mut rng,
        )
    };
    apply_actions(me, mob, ctx, actions);
}

/// `LivingEntity.baseTick` 开头的 `EnchantmentHelper.tickEffects`（TICK 组件）。
///
/// `EnchantmentHelper.tickEffects` at the head of `LivingEntity.baseTick`.
pub fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let enchants = equipped(mob);
    if enchants.is_empty() {
        return;
    }
    let state = entity_state(me, ctx.blocks());
    let actions = {
        let mut rng = || ctx.blocks().level_random_float();
        fx::tick_effects(&state, &enchants, &mut rng)
    };
    apply_actions(me, mob, ctx, actions);
}

/// 换靴子时：先撤掉旧装备的 location 效果，再对当前装备立即重跑一次
/// （官方 `collectEquipmentChanges` 后立即 `runLocationChangedEffects`，不用等跨格）。
///
/// On a boot change: deactivate the old location effects, then immediately re-run on
/// current equipment (vanilla does this right after `collectEquipmentChanges`).
pub fn reequip(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let actions = fx::stop_location_based(&mut mob.active_location_effects, EquipmentSlot::Feet);
    apply_actions(me, mob, ctx, actions);
    run_location(me, mob, ctx);
}

/// 把动作落地：改属性表、写方块、扣耐久、发粒子音效。
///
/// Apply actions: edit attributes, write blocks, damage items, emit particle/sound commands.
fn apply_actions(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    actions: Vec<fx::Action>,
) {
    for action in actions {
        match action {
            fx::Action::AddModifier {
                attribute,
                id,
                amount,
                operation,
            } => {
                if let Some(instance) = mob.attributes.instance(attribute) {
                    instance.modifiers.push(Modifier {
                        id,
                        amount,
                        operation,
                        permanent: false,
                    });
                    instance.touch_sync();
                }
            }
            fx::Action::RemoveModifier { attribute, id } => {
                if let Some(instance) = mob.attributes.instance(attribute) {
                    instance.remove_modifier(&id);
                    instance.touch_sync();
                }
            }
            fx::Action::Freeze(req) => {
                let cells = freeze_cells_for(&*ctx, &req);
                if !cells.is_empty() {
                    ctx.emit(Command::FreezeWater { cells }, me);
                }
            }
            fx::Action::DamageItem { slot, amount } => {
                damage_item(me, mob, ctx, slot, amount);
            }
            fx::Action::SoulParticles { position } => {
                ctx.emit(Command::SoulSpeedParticles { position }, me);
            }
            fx::Action::SoulSound { position } => {
                ctx.emit(Command::SoulSpeedSound { position }, me);
            }
        }
    }
}

/// 逐格跑 `replace_disk` 的四个方块谓词：
/// `matching_blocks(water)`、`matching_fluids(water)`（water 方块的流体即 water）、
/// `matching_block_tag(offset [0,1,0], air)`、`unobstructed`（该格无实体碰撞箱）。
///
/// Run the four `replace_disk` block predicates on each cell.
fn freeze_cells_for(ctx: &dyn EntityCtx, req: &fx::FreezeRequest) -> Vec<BlockPos> {
    let view = ctx.blocks();
    fx::freeze_cells(req, |pos| {
        let state = view.block_state(pos);
        let is_water = state.is(blocks::WATER);
        let air_above = view.block_state(pos.above()).is_tagged(block_tags::AIR);
        let cell = Aabb::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
            f64::from(pos.x.saturating_add(1)),
            f64::from(pos.y.saturating_add(1)),
            f64::from(pos.z.saturating_add(1)),
        );
        let mut blocked = false;
        ctx.for_each_other_in(cell, &mut |public| {
            if public.bounding_box().intersects(cell) {
                blocked = true;
            }
        });
        fx::FreezeCell {
            is_water,
            air_above,
            unobstructed: !blocked,
        }
    })
}

/// `change_item_damage` 的 `hurtAndBreak`；破损时当场撤掉这件的属性与 location 效果。
///
/// `change_item_damage`'s `hurtAndBreak`; on break, deactivate this item's effects.
fn damage_item(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    slot: EquipmentSlot,
    amount: i32,
) {
    let before = mob.equipment.get(slot).clone();
    let mut broke = false;
    {
        let view = ctx.blocks();
        mob.equipment.edit(slot, |stack| {
            broke = view.hurt_mob_equipment(stack, amount);
        });
    }
    if broke {
        mob.events.push(MobEvent::EntityEvent {
            event: equipment::break_event(slot),
        });
        equipment::remove_stack_modifiers(&before, slot, &mut mob.attributes);
        let actions = fx::stop_location_based(&mut mob.active_location_effects, slot);
        apply_actions(me, mob, ctx, actions);
    }
}
