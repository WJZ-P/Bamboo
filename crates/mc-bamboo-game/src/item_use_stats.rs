//! 当前会话的 `Stats.ITEM_USED` 与 `Stats.DAMAGE_DEALT` 累计值。
//!
//! Session-local cumulative `Stats.ITEM_USED` and `Stats.DAMAGE_DEALT` values.

use std::collections::BTreeMap;

use crate::{inbound::PlayerId, region::Region};

#[derive(Debug, Default)]
pub(crate) struct ItemUseStats {
    items: BTreeMap<i32, (i32, bool)>,
    /// 会话累计的 `Stats.DAMAGE_DEALT`，官方 `ServerStatsCounter` 发累计值而非增量。
    ///
    /// Session cumulative `Stats.DAMAGE_DEALT`; vanilla `ServerStatsCounter`
    /// sends the cumulative value, not the increment.
    damage_dealt: i32,
    damage_dealt_dirty: bool,
    /// 会话累计的其它 `minecraft:custom` 统计（`TALKED_TO_VILLAGER` /
    /// `TRADED_WITH_VILLAGER` 等），键是 `custom_stat` 的注册表 ID。
    ///
    /// Other session-cumulative `minecraft:custom` stats (e.g. `TALKED_TO_VILLAGER`
    /// and `TRADED_WITH_VILLAGER`), keyed by their `custom_stat` registry id.
    custom: BTreeMap<i32, (i32, bool)>,
}

impl ItemUseStats {
    #[cfg(feature = "oracle-samples")]
    /// 当前累计值（物品 ID 升序），对拍里比较 `ITEM_USED` 用；不清脏标记。
    ///
    /// Current cumulative values (item id order) for oracle comparison; does
    /// not clear the dirty flags.
    pub(crate) fn values(&self) -> Vec<(i32, i32)> {
        self.items
            .iter()
            .map(|(&item, &(value, _))| (item, value))
            .collect()
    }

    /// 和 `ServerStatsCounter.sendStats` 一样，只发送变过的累计计数。
    ///
    /// Like `ServerStatsCounter.sendStats`, sends only changed cumulative counts.
    pub(crate) fn take_dirty(&mut self) -> Vec<(i32, i32)> {
        self.items
            .iter_mut()
            .filter_map(|(&id, (value, dirty))| std::mem::take(dirty).then_some((id, *value)))
            .collect()
    }

    /// `getDirty()` 之后清掉脏标记，返回要发的 `DAMAGE_DEALT` 累计值。
    ///
    /// Clear the dirty flag like `getDirty()` and return the cumulative
    /// `DAMAGE_DEALT` value to send.
    pub(crate) fn take_dirty_damage_dealt(&mut self) -> Option<i32> {
        std::mem::take(&mut self.damage_dealt_dirty).then_some(self.damage_dealt)
    }

    /// 其它自定义统计里变过的项（ID 升序），清掉脏标记。
    ///
    /// Other changed custom stats (id order), clearing the dirty flags.
    pub(crate) fn take_dirty_custom(&mut self) -> Vec<(i32, i32)> {
        self.custom
            .iter_mut()
            .filter_map(|(&id, (value, dirty))| std::mem::take(dirty).then_some((id, *value)))
            .collect()
    }
}

pub(crate) fn award(region: &mut Region, id: PlayerId, item: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        let (value, dirty) = player.item_use_stats.items.entry(item).or_default();
        *value = value.saturating_add(1);
        *dirty = true;
    }
}

/// `Player.damageStatsAndHearts` 的
/// `awardStat(Stats.DAMAGE_DEALT, Math.round(actualDamage * 10.0F))`：值可能是 0，
/// 但官方照样把这一项标脏并发出去。
///
/// `Player.damageStatsAndHearts`'s
/// `awardStat(Stats.DAMAGE_DEALT, Math.round(actualDamage * 10.0F))`: the value
/// may be 0, yet vanilla still marks the stat dirty and sends it.
pub(crate) fn award_damage_dealt(region: &mut Region, id: PlayerId, count: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        let stats = &mut player.item_use_stats;
        stats.damage_dealt = stats.damage_dealt.saturating_add(count);
        stats.damage_dealt_dirty = true;
    }
}

/// `Player.awardStat(Stats.TALKED_TO_VILLAGER)` / `Stats.TRADED_WITH_VILLAGER`：
/// 自定义统计，`count` 是增量（官方 `incrementStat` 加 1）。
///
/// `Player.awardStat(Stats.TALKED_TO_VILLAGER)` / `Stats.TRADED_WITH_VILLAGER`:
/// custom stats; `count` is the increment (`incrementStat` adds 1).
pub(crate) fn award_custom(region: &mut Region, id: PlayerId, stat: i32, count: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        let (value, dirty) = player.item_use_stats.custom.entry(stat).or_default();
        *value = value.saturating_add(count);
        *dirty = true;
    }
}
