//! 流浪商人 `WanderingTrader` 的专属状态、`despawnDelay` 和饮水状态机。
//!
//! 交易层（`offers`、`inventory`、`trading_player`、`unhappy`）是 `AbstractVillager`
//! 的字段，和村民共用 [`super::Mob::villager`]；这里只放流浪商人自己的部分。
//!
//! Wandering-trader-specific state, `despawnDelay` and the drinking state machine.
//! The trading layer is shared with villagers through `Mob::villager`.

use mc_bamboo_core::BlockPos;

use super::{Mob, goals::GoalSelector, navigation::Navigation};
use crate::entity::{Entity, ctx::EntityCtx};

/// 刷新器生成流浪商人时设的 `despawnDelay`（`WanderingTraderSpawner.spawn`）。
///
/// The `despawnDelay` set by `WanderingTraderSpawner.spawn`.
pub const SPAWNED_DESPAWN_DELAY: i32 = 48000;

/// 一次 `UseItemGoal` 的饮水时长：隐身药水和牛奶桶都是 32 刻。
///
/// The `UseItemGoal` duration: both the invisibility potion and the milk bucket take 32 ticks.
const DRINK_DURATION: i32 = 32;

/// 当前正在使用的消耗品（官方 `UseItemGoal` 的 `startUsingItem` 状态）。
///
/// The consumable currently in use (`UseItemGoal`'s `startUsingItem` state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Drink {
    #[default]
    None,
    /// 夜里喝隐身药水，喝完隐身。
    /// Drinking an invisibility potion at night; invisible once finished.
    Invisibility { remaining: i32 },
    /// 白天喝牛奶桶，喝完显形。
    /// Drinking a milk bucket by day; visible once finished.
    Milk { remaining: i32 },
}

/// `WanderingTrader` 的专属状态。
///
/// The wandering trader's own state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WanderingTrader {
    /// `despawnDelay`：每刻 -1，到 0 时消失；交易中暂停递减。
    /// `despawnDelay`: ticked down each tick, despawning at zero; paused while trading.
    pub despawn_delay: i32,
    /// `wanderTarget`：`WanderToPositionGoal` 的目的地，刷新器设为村庄钟位置。
    /// `wanderTarget`: the goal's destination, set to the village bell by the spawner.
    pub wander_target: Option<BlockPos>,
    /// `Entity.isInvisible()` 的共享标志位：夜里隐身、白天显形。
    /// The shared `isInvisible()` flag: invisible at night, visible by day.
    pub invisible: bool,
    /// 当前饮水状态。
    /// The current drinking state.
    pub drink: Drink,
}

/// `WanderingTrader.registerGoals`：官方用 `GoalSelector` 注册一串目标
/// （`UseItemGoal` 喝隐身药水 / 牛奶、`WanderToPositionGoal`、躲避僵尸等）。
/// 这里保留一张空表：饮水、游走目标和消失都由 [`custom_ai_step`] 与
/// [`maybe_despawn`] 按官方时序落地，不经过目标调度取随机数。
///
/// `WanderingTrader.registerGoals` uses a `GoalSelector`; drinking, wandering and
/// despawning are handled here by the per-tick step instead of goal scheduling.
pub(in crate::entity::mob) fn goals(_navigation: &mut Navigation) -> GoalSelector {
    GoalSelector::default()
}

/// `WanderingTrader.maybeDespawn()`：`aiStep()` 末尾每刻跑一次。
///
/// `WanderingTrader.maybeDespawn()`, run once per tick at the end of `aiStep()`.
pub fn maybe_despawn(me: &mut Entity, mob: &mut Mob) {
    if mob.wandering_trader.despawn_delay > 0 && !mob.villager.is_trading() {
        mob.wandering_trader.despawn_delay -= 1;
        if mob.wandering_trader.despawn_delay == 0 {
            me.discard();
        }
    }
}

/// 夜里喝隐身药水、白天喝牛奶：照 `UseItemGoal` 的两个谓词推进饮水状态。
/// 官方谓词用 `isDarkOutside()` / `isBrightOutside()`；这里把「暗」取成
/// `!is_bright_outside()`，与 `Level.isDarkOutside()` 一致。
///
/// Advances the drinking state from the two `UseItemGoal` predicates: invisibility
/// potion at night, milk by day. Darkness is `!is_bright_outside()`.
pub fn custom_ai_step(mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let bright = ctx.blocks().is_bright_outside();
    let state = &mut mob.wandering_trader;
    match state.drink {
        Drink::Invisibility { ref mut remaining } => {
            *remaining -= 1;
            if *remaining <= 0 {
                state.invisible = true;
                state.drink = Drink::None;
            }
        }
        Drink::Milk { ref mut remaining } => {
            *remaining -= 1;
            if *remaining <= 0 {
                state.invisible = false;
                state.drink = Drink::None;
            }
        }
        Drink::None => {
            if !bright && !state.invisible {
                state.drink = Drink::Invisibility {
                    remaining: DRINK_DURATION,
                };
            } else if bright && state.invisible {
                state.drink = Drink::Milk {
                    remaining: DRINK_DURATION,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::MobType;

    fn trader() -> (crate::entity::Entity, Mob) {
        let kind = MobType::WanderingTrader;
        let mob = Mob::new(kind);

        (crate::entity::Entity::by_mob_type(kind, Default::default()), mob)
    }

    #[test]
    fn despawn_delay_counts_down_and_discards_at_zero() {
        let (mut me, mut mob) = trader();
        mob.wandering_trader.despawn_delay = 3;
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 2);
        assert!(!me.removed);
        maybe_despawn(&mut me, &mut mob);
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 0);
        assert!(me.removed);
    }

    #[test]
    fn trading_pauses_the_despawn_delay() {
        let (mut me, mut mob) = trader();
        mob.wandering_trader.despawn_delay = 1;
        mob.villager.set_trading_player(Some(7));
        maybe_despawn(&mut me, &mut mob);
        assert_eq!(mob.wandering_trader.despawn_delay, 1);
        assert!(!me.removed);
    }
}
