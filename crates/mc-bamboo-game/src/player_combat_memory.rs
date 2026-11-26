//! 玩家的战斗记忆：官方 `LivingEntity` 的 `lastHurtByMob`、`lastHurtMob` 和
//! `getLastDamageSource(100)`。被驯服的狼靠它们保护主人（`OwnerHurtByTargetGoal`、
//! `OwnerHurtTargetGoal`、`SitWhenOrderedToGoal`）。时间戳用玩家自己的 tick 数，
//! 不是岛时钟，拆岛 / 并岛不用平移。
//!
//! A player's combat memory: vanilla `lastHurtByMob`, `lastHurtMob` and
//! `getLastDamageSource(100)`, read by tamed wolves guarding their owner.
//! Timestamps are the player's own tick count, so islands need no rebasing.

use mc_bamboo_blocks::entity::{EntityUid, ctx::PlayerCombat, mob::DamageSource};

/// `LivingEntity.baseTick`：`lastHurtByMob` 超过 100 刻就清掉。
///
/// `LivingEntity.baseTick` forgets `lastHurtByMob` after 100 ticks.
const HURT_BY_MEMORY: i32 = 100;

/// `OwnerHurtByTargetGoal` 读的 `getLastDamageSource(100)`。
const DAMAGE_MEMORY: i32 = 100;

/// 一名玩家的战斗记忆。
///
/// One player's combat memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CombatMemory {
    last_hurt_by_mob: Option<EntityUid>,
    last_hurt_by_mob_timestamp: i32,
    last_hurt_mob: Option<EntityUid>,
    last_hurt_mob_timestamp: i32,
    /// 上一次受伤的 tick 数和那次伤害能不能让狼还手。
    ///
    /// The tick of the last damage and whether wolves may retaliate for it.
    last_damage: Option<(i32, bool)>,
}

impl CombatMemory {
    /// `LivingEntity.hurtServer` 受伤成立之后：`resolveMobResponsibleForDamage`
    /// （有生物造成者、不在 `#no_anger` 里）和 `lastDamageSource`。
    ///
    /// After `LivingEntity.hurtServer` accepts the damage.
    pub(crate) fn record_hurt(&mut self, source: &DamageSource, now: i32) {
        let tagged = |tag: &str| {
            mc_bamboo_data::damage_type_id(source.kind.name())
                .is_some_and(|id| damage_type_tagged(tag, id))
        };
        if let Some(attacker) = source.attacker
            && !tagged("minecraft:no_anger")
        {
            self.last_hurt_by_mob = Some(attacker);
            self.last_hurt_by_mob_timestamp = now;
        }
        self.last_damage = Some((now, !tagged("minecraft:no_wolf_retaliation")));
    }

    /// `Player.attack` 打中之后的 `setLastHurtMob(target)`。
    ///
    /// `setLastHurtMob(target)` after `Player.attack` lands.
    pub(crate) fn record_attack(&mut self, target: EntityUid, now: i32) {
        self.last_hurt_mob = Some(target);
        self.last_hurt_mob_timestamp = now;
    }

    /// 实体逻辑读到的那一份，按玩家现在的 tick 数过期。
    ///
    /// The view seen by entity logic, expired against the player's tick count.
    pub(crate) fn view(&self, now: i32) -> PlayerCombat {
        let hurt_by_fresh = now.wrapping_sub(self.last_hurt_by_mob_timestamp) <= HURT_BY_MEMORY;
        PlayerCombat {
            last_hurt_by_mob: self.last_hurt_by_mob.filter(|_| hurt_by_fresh),
            last_hurt_by_mob_timestamp: self.last_hurt_by_mob_timestamp,
            last_hurt_mob: self.last_hurt_mob,
            last_hurt_mob_timestamp: self.last_hurt_mob_timestamp,
            retaliation_damage: self.last_damage.is_some_and(|(at, retaliable)| {
                retaliable && now.wrapping_sub(at) <= DAMAGE_MEMORY
            }),
        }
    }
}

fn damage_type_tagged(tag: &str, id: i32) -> bool {
    mc_bamboo_data::in_tag("minecraft:damage_type", tag, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::entity::mob::DamageKind;

    #[test]
    fn hurt_by_memory_expires_after_a_hundred_ticks() {
        let mut memory = CombatMemory::default();
        let mut source = DamageSource::environment(DamageKind::MobAttack);
        source.attacker = Some(9);
        memory.record_hurt(&source, 10);
        assert_eq!(memory.view(110).last_hurt_by_mob, Some(9));
        assert!(memory.view(110).retaliation_damage);
        assert_eq!(memory.view(111).last_hurt_by_mob, None);
        assert!(!memory.view(111).retaliation_damage);
    }

    #[test]
    fn attacks_keep_their_own_timestamp() {
        let mut memory = CombatMemory::default();
        memory.record_attack(4, 7);
        let view = memory.view(500);
        assert_eq!(view.last_hurt_mob, Some(4));
        assert_eq!(view.last_hurt_mob_timestamp, 7);
    }
}
