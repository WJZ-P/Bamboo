//! 马科死亡时的掉落：`LivingEntity.dropAllDeathLoot` 的顺序——战利品表、
//! `Mob.dropCustomDeathLoot` 的装备（鞍、马铠、地毯）、`AbstractHorse.dropEquipment`
//! 的箱子物品、`AbstractChestedHorse.dropEquipment` 的箱子方块，最后是经验。
//!
//! Equine death drops in `dropAllDeathLoot` order: the loot table, worn
//! equipment from `dropCustomDeathLoot`, the chest contents and the chest block
//! from `dropEquipment`, then experience.

use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use super::Mob;
use crate::entity::{
    Entity,
    arrow::enchantment_level,
    ctx::{Command, EntityCtx},
    mob::{DamageSource, MobDeathLoot, equipment::SLOTS, skeleton_drops::drop_slot},
};

/// `dropAllDeathLoot`：`shouldDropLoot`（成年且 `MOB_DROPS`）才掉战利品表和装备；
/// 箱子不看这条规则；经验要最近被玩家打过、成年、`MOB_DROPS`，`Animal` 取 `1 + nextInt(3)`。
///
/// `dropAllDeathLoot`: the loot table and equipment need an adult and `MOB_DROPS`;
/// the chest ignores the rule; experience needs a recent player hit, an adult and
/// `MOB_DROPS`, and `Animal` draws `1 + nextInt(3)`.
pub(in crate::entity::mob) fn drop_on_death(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    player_kill: bool,
) {
    let drops = ctx.blocks().block_game_rules().mob_drops;
    let adult = !mob.is_baby();
    if adult && drops {
        ctx.emit(Command::DropMobLoot(MobDeathLoot::new(me, mob, source)), me);
        drop_worn(me, mob, ctx, source, player_kill);
    }
    drop_chest(me, mob, ctx);
    if player_kill && adult && drops {
        let amount = 1 + me.random.next_int_bounded(3);
        ctx.emit(Command::AwardDeathExperience { amount }, me);
    }
}

/// `Mob.dropCustomDeathLoot`：八个槽按 ordinal，抢夺只在玩家打死时加成。
///
/// `Mob.dropCustomDeathLoot`: the eight slots in ordinal order; looting only
/// counts for a player kill.
fn drop_worn(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    player_kill: bool,
) {
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
}

/// `AbstractHorse.dropEquipment` 掉箱子里的东西（带消失诅咒的不掉），
/// `AbstractChestedHorse.dropEquipment` 再掉一个箱子并卸下它。
///
/// Drop the chest contents (except vanishing items), then the chest itself,
/// and take the chest off.
fn drop_chest(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(state) = &mut mob.equine else {
        return;
    };
    for stack in &state.chest_items {
        if stack.is_empty() || enchantment_level(stack, "minecraft:vanishing_curse") > 0 {
            continue;
        }
        ctx.emit(
            Command::DropItem {
                stack: stack.clone(),
            },
            me,
        );
    }
    if state.chested {
        ctx.emit(
            Command::DropItem {
                stack: ItemStack::new(items::CHEST, 1),
            },
            me,
        );
        state.chested = false;
    }
}
