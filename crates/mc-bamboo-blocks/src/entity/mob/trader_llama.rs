//! 行商羊驼 `TraderLlama`：继承羊驼，随刷新器拴在流浪商人身上，
//! `despawnDelay` 到时和商人一起消失。
//!
//! Trader llama: a llama spawned leashed to a wandering trader, despawning
//! together with it when `despawnDelay` runs out.

use super::{Mob, MobType, goals::GoalSelector, leash, navigation::Navigation};
use crate::entity::{Entity, ctx::EntityCtx};

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
/// 被拴在流浪商人身上时按官方 `holder.getDespawnDelay() - 1` 取牵引者的倒计时；
/// 否则自己递减。`getLeashHolder()` 的公开行里带着商人的 `despawnDelay`。
///
/// `TraderLlama.maybeDespawn()`, run once per tick at the end of `aiStep()`.
/// While leashed to a wandering trader it follows the holder's
/// `getDespawnDelay() - 1`; otherwise it ticks down its own counter.
pub fn maybe_despawn(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if !can_despawn(mob) {
        return;
    }
    let holder_delay = leash::leash_holder(mob).and_then(|uid| {
        let mut delay = None;
        ctx.with_other(uid, &mut |public| {
            if let crate::entity::KindPublic::Mob(holder) = &public.kind
                && holder.mob_type == MobType::WanderingTrader
            {
                delay = Some(holder.trader_despawn_delay);
            }
        });
        delay
    });
    if tick_despawn_delay(mob, holder_delay) {
        leash::remove_leash(mob);
        me.discard();
    }
}

/// 官方 `maybeDespawn()` 的赋值那一半：`holder.getDespawnDelay() - 1` 或
/// `despawnDelay - 1`；返回是否已经到 `<= 0`（调用方解绳并 `discard()`）。
///
/// The assignment half of official `maybeDespawn()`: either the holder's
/// `getDespawnDelay() - 1` or its own `despawnDelay - 1`.
fn tick_despawn_delay(mob: &mut Mob, holder_delay: Option<i32>) -> bool {
    mob.trader_llama.despawn_delay = match holder_delay {
        Some(delay) => delay - 1,
        None => mob.trader_llama.despawn_delay - 1,
    };
    mob.trader_llama.despawn_delay <= 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llama() -> Mob {
        Mob::new(MobType::TraderLlama)
    }

    #[test]
    fn default_delay_matches_vanilla() {
        assert_eq!(TraderLlama::default().despawn_delay, 47999);
    }

    #[test]
    fn unleased_delay_counts_down_to_zero() {
        let mut mob = llama();
        mob.trader_llama.despawn_delay = 2;
        assert!(!tick_despawn_delay(&mut mob, None));
        assert_eq!(mob.trader_llama.despawn_delay, 1);
        assert!(tick_despawn_delay(&mut mob, None));
        assert_eq!(mob.trader_llama.despawn_delay, 0);
    }

    #[test]
    fn leashed_delay_follows_the_trader() {
        let mut mob = llama();
        mob.trader_llama.despawn_delay = 47999;
        // 官方 `holder.getDespawnDelay() - 1`：商人剩 123 刻，羊驼就是 122。
        assert!(!tick_despawn_delay(&mut mob, Some(123)));
        assert_eq!(mob.trader_llama.despawn_delay, 122);
        // 商人的倒计时到 0 时羊驼也一起消失。
        assert!(tick_despawn_delay(&mut mob, Some(1)));
        assert_eq!(mob.trader_llama.despawn_delay, 0);
    }

    #[test]
    fn persistence_required_blocks_despawn() {
        let mut mob = llama();
        mob.persistence_required = true;
        assert!(!can_despawn(&mob));
    }
}
