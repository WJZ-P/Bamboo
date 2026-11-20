use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_protocol::{
    components::{ComponentValue, PatchEntry, defaults},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::{
    DamageSource, Mob,
    equipment::{MobEquipment, SLOTS},
};
use crate::entity::{
    Entity, JavaRandom,
    arrow::enchantment_level,
    ctx::{Command, EntityCtx},
};

pub(super) fn drop_on_death(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    player_kill: bool,
) {
    if !ctx.blocks().block_game_rules().mob_drops {
        return;
    }
    ctx.emit(
        Command::DropMobLoot {
            table: mob.mob_type.loot_table(),
            on_fire: me.remaining_fire_ticks > 0,
            killer: source.attacker,
            experience: 0,
        },
        me,
    );
    // Mob.dropCustomDeathLoot 在计算经验以前移除掉落装备。
    // Mob.dropCustomDeathLoot clears successfully dropped slots before computing XP.
    let looting = if source.attacker_is_player {
        source.looting_level
    } else {
        0
    };
    for slot in SLOTS {
        if let Some(stack) = drop_slot(
            &mut mob.equipment,
            slot,
            player_kill,
            looting,
            &mut me.random,
        ) {
            ctx.emit(Command::DropItem { stack }, me);
        }
    }
    if player_kill {
        let experience = experience_reward(mob, &mut me.random);
        ctx.emit(
            Command::DropMobLoot {
                table: "",
                on_fire: false,
                killer: None,
                experience,
            },
            me,
        );
    }
}

fn drop_slot(
    equipment: &mut MobEquipment,
    slot: EquipmentSlot,
    player_kill: bool,
    looting: i32,
    random: &mut JavaRandom,
) -> Option<ItemStack> {
    let base = equipment.drop_chance(slot);
    let stack = equipment.get(slot);
    let preserve = equipment.is_preserved(slot);
    if base == 0.0
        || stack.is_empty()
        || enchantment_level(stack, "minecraft:vanishing_curse") > 0
        || (!player_kill && !preserve)
        || random.next_float() >= base + 0.01 * looting as f32
    {
        return None;
    }
    let mut stack = stack.clone();
    if !preserve {
        damage_drop(&mut stack, random);
    }
    equipment.set(slot, ItemStack::empty());
    Some(stack)
}

fn damage_drop(stack: &mut ItemStack, random: &mut JavaRandom) {
    let mut patch = stack.patch();
    let resolve = |kind| defaults::resolve(stack.item, &patch, kind);
    let Some(ComponentValue::MaxDamage(max)) = resolve(kinds::MAX_DAMAGE) else {
        return;
    };
    if resolve(kinds::UNBREAKABLE).is_some() || resolve(kinds::DAMAGE).is_none() {
        return;
    }
    let max = *max;
    let bound = 1 + random.next_int_bounded((max - 3).max(1));
    let damage = (max - random.next_int_bounded(bound)).clamp(0, max);
    patch.put(
        kinds::DAMAGE,
        PatchEntry::Set(ComponentValue::Damage(damage)),
    );
    (stack.added, stack.removed) = patch.to_opaque();
}

fn experience_reward(mob: &Mob, random: &mut JavaRandom) -> i32 {
    let equipment = &mob.equipment;
    let mut reward = mob.mob_type.xp_reward();
    for slot in SLOTS {
        if !equipment.get(slot).is_empty() && equipment.drop_chance(slot) <= 1.0 {
            reward += 1 + random.next_int_bounded(3);
        }
    }
    reward
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items::BOW;

    #[test]
    fn preserved_gear_drops_unchanged_without_a_player_kill() {
        let slot = EquipmentSlot::MainHand;
        let mut equipment = MobEquipment::default();
        let bow = ItemStack::new(BOW, 1);
        equipment.set(slot, bow.clone());
        let mut rng = JavaRandom::new(12);
        let seed = rng.state();
        assert!(drop_slot(&mut equipment, slot, false, 0, &mut rng).is_none());
        assert_eq!(rng.state(), seed);
        equipment.set_drop_chance(slot, 2.0);
        assert_eq!(
            drop_slot(&mut equipment, slot, false, 0, &mut rng),
            Some(bow)
        );
        assert!(equipment.get(slot).is_empty());
        let mut expected = JavaRandom::new(12);
        expected.next_float();
        assert_eq!(rng.state(), expected.state());
    }
}
