//! `ItemCooldowns`：官方 `Player.getCooldowns()` 里那张按物品 / 冷却组记结束时刻的表。
//!
//! `ServerPlayerGameMode.useItem` 开头先问 `isOnCooldown(stack)`；26.3 的末影珍珠
//! 带 `use_cooldown` 组件（1 秒 = 20 刻），`ItemStack.use` 成功后由
//! `UseCooldown.apply` 挂上。`tick()` 每刻把 `tickCount` 加一，到点的条目删掉。
//!
//! Vanilla `ItemCooldowns`: the map of cooldown end times keyed by item or
//! cooldown group. `ServerPlayerGameMode.useItem` refuses while a stack is on
//! cooldown; a used ender pearl gets a 20-tick cooldown from its
//! `use_cooldown` component. `tick()` advances the clock and drops expired entries.

use std::collections::BTreeMap;

/// `UseCooldown.ticks()` 的键：显式冷却组优先，否则用物品注册表 ID。
///
/// `UseCooldown.ticks()` key: an explicit cooldown group, else the item id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CooldownKey {
    Item(i32),
    Group(String),
}

/// 一个玩家身上的物品冷却。所有倒计时都是相对刻数，跟着玩家整行搬岛。
///
/// One player's item cooldowns. All times are relative ticks, so the whole row
/// moves between islands without rebasing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemCooldowns {
    tick_count: i32,
    cooldowns: BTreeMap<CooldownKey, i32>,
}

impl ItemCooldowns {
    /// `isOnCooldown(stack)`：`getCooldownPercent(stack, 0) > 0`。
    pub fn is_on_cooldown(&self, item: i32, group: Option<&str>) -> bool {
        self.end_time(item, group)
            .is_some_and(|end| end > self.tick_count)
    }

    /// `addCooldown(stack, ticks)`：`endTime = tickCount + ticks`。
    pub fn add(&mut self, item: i32, group: Option<&str>, ticks: i32) {
        let key = group.map_or(CooldownKey::Item(item), |name| {
            CooldownKey::Group(name.to_owned())
        });
        self.cooldowns
            .insert(key, self.tick_count.saturating_add(ticks));
    }

    /// `tick()`：时钟加一，到点的条目移除。
    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.saturating_add(1);
        self.cooldowns.retain(|_, end| *end > self.tick_count);
    }

    fn end_time(&self, item: i32, group: Option<&str>) -> Option<i32> {
        let key = match group {
            Some(name) => CooldownKey::Group(name.to_owned()),
            None => CooldownKey::Item(item),
        };
        self.cooldowns.get(&key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_twenty_tick_cooldown_expires_on_the_twentieth_tick() {
        let mut cooldowns = ItemCooldowns::default();
        cooldowns.add(1265, None, 20);
        assert!(cooldowns.is_on_cooldown(1265, None));
        for _ in 0..19 {
            cooldowns.tick();
            assert!(cooldowns.is_on_cooldown(1265, None));
        }
        cooldowns.tick();
        assert!(!cooldowns.is_on_cooldown(1265, None));
    }

    #[test]
    fn a_reuse_restarts_the_cooldown_and_groups_are_shared() {
        let mut cooldowns = ItemCooldowns::default();
        cooldowns.add(1265, None, 20);
        for _ in 0..10 {
            cooldowns.tick();
        }
        cooldowns.add(1265, None, 20);
        for _ in 0..19 {
            cooldowns.tick();
        }
        assert!(cooldowns.is_on_cooldown(1265, None));
        cooldowns.add(7, Some("minecraft:ender_pearl"), 5);
        assert!(cooldowns.is_on_cooldown(9, Some("minecraft:ender_pearl")));
        assert!(!cooldowns.is_on_cooldown(9, None));
    }
}
