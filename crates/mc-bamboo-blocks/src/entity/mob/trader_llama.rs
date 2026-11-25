//! 行商羊驼 `TraderLlama`：继承羊驼，随刷新器拴在流浪商人身上，
//! `despawnDelay` 到时和商人一起消失。
//!
//! Trader llama: a llama spawned leashed to a wandering trader, despawning
//! together with it when `despawnDelay` runs out.

use super::{Mob, MobType, goals::GoalSelector, navigation::Navigation};
use crate::entity::Entity;

/// 新建行商羊驼时的 `DespawnDelay` 缺省值（官方 `DEFAULT_DESPAWN_DELAY`）。
///
/// The default `DespawnDelay` for a freshly created trader llama.
pub const DEFAULT_DESPAWN_DELAY: i32 = 47999;

/// `TraderLlama` 的专属状态。
///
/// The trader llama's own state.
#[derive(Debug, Clone, PartialEq)]
pub struct TraderLlama {
    /// `despawnDelay`：被拴在商人身上时跟随商人的倒计时，否则自己递减。
    /// `despawnDelay`: follows the trader's countdown while leashed, otherwise ticks down.
    pub despawn_delay: i32,
}

impl Default for TraderLlama {
    fn default() -> Self {
        Self {
            despawn_delay: DEFAULT_DESPAWN_DELAY,
        }
    }
}

/// `TraderLlama.registerGoals`：先跑 `Llama.registerGoals()` 的公共表，
/// 再加自己的 `PanicGoal` / 保护目标（后两者只在商人受攻击时激活，这里
/// 沿用羊驼表即可覆盖绝大多数刻）。
///
/// `TraderLlama.registerGoals` starts from the llama table; its own panic and
/// defend goals only activate when the trader is attacked.
pub(in crate::entity::mob) fn goals(navigation: &mut Navigation) -> GoalSelector {
    super::equine::goals(MobType::Llama, navigation)
}

/// 行商羊驼的目标选择器沿用羊驼的表。
/// The trader llama keeps the llama target selector.
pub(in crate::entity::mob) fn targets() -> GoalSelector {
    super::equine::targets(MobType::Llama)
}

/// `TraderLlama.canDespawn()`：锁龄、要求持久或有玩家骑乘时不消失。
/// 官方还会挡住「被拴到商人之外的东西」和已驯服两种情形；Bamboo 里行商羊驼
/// 只由刷新器拴在商人身上，玩家重拴与驯服未建模，故不在此判定。
///
/// `TraderLlama.canDespawn()`: no despawn while age-locked, persistence-required
/// or ridden. Vanilla also blocks being leashed to something other than the trader
/// and being tamed; neither path is modelled here (only the spawner leashes them).
fn can_despawn(mob: &Mob) -> bool {
    !mob.persistence_required && !mob.age_locked && mob.passengers.is_empty()
}

/// `TraderLlama.maybeDespawn()`：`aiStep()` 末尾每刻跑一次。
///
/// `TraderLlama.maybeDespawn()`, run once per tick at the end of `aiStep()`.
pub fn maybe_despawn(me: &mut Entity, mob: &mut Mob) {
    if !can_despawn(mob) {
        return;
    }
    mob.trader_llama.despawn_delay -= 1;
    if mob.trader_llama.despawn_delay <= 0 {
        super::leash::remove_leash(mob);
        me.discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llama() -> (crate::entity::Entity, Mob) {
        (
            crate::entity::Entity::by_mob_type(MobType::TraderLlama, Default::default()),
            Mob::new(MobType::TraderLlama),
        )
    }

    #[test]
    fn default_delay_matches_vanilla() {
        assert_eq!(TraderLlama::default().despawn_delay, 47999);
    }

    #[test]
    fn despawn_delay_counts_down_and_discards_at_zero() {
        let (mut me, mut mob) = llama();
        mob.trader_llama.despawn_delay = 2;
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.trader_llama.despawn_delay, 1);
        assert!(!me.removed);
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.trader_llama.despawn_delay, 0);
        assert!(me.removed);
    }

    #[test]
    fn persistence_required_blocks_despawn() {
        let (mut me, mut mob) = llama();
        mob.trader_llama.despawn_delay = 1;
        mob.persistence_required = true;
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.trader_llama.despawn_delay, 1);
        assert!(!me.removed);
    }
}
