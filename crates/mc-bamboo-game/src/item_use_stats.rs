//! 当前会话的 `Stats.ITEM_USED` 累计值。
//!
//! Session-local cumulative `Stats.ITEM_USED` values.

use std::collections::BTreeMap;

use crate::{inbound::PlayerId, region::Region};

#[derive(Debug, Default)]
pub(crate) struct ItemUseStats {
    items: BTreeMap<i32, (i32, bool)>,
}

impl ItemUseStats {
    /// 和 `ServerStatsCounter.sendStats` 一样，只发送变过的累计计数。
    ///
    /// Like `ServerStatsCounter.sendStats`, sends only changed cumulative counts.
    pub(crate) fn take_dirty(&mut self) -> Vec<(i32, i32)> {
        self.items
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
