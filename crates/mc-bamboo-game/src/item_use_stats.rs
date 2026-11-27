//! 当前会话的 `Stats.ITEM_USED` 与 `Stats.DAMAGE_DEALT` 累计值。
//!
//! Session-local cumulative `Stats.ITEM_USED` and `Stats.DAMAGE_DEALT` values.
//!
//! 每次加统计还要照官方 `ServerPlayer.awardStat` / `resetStat` 改计分板上用这项统计
//! 当准则的目标。计分板在主控上，所以这里先按顺序记下来，玩家这一刻的末尾由
//! [`tick_criteria`](crate::scoreboard::tick_criteria) 一起交给主控。
//!
//! Every award also has to change the scoreboard objectives that use the stat as their
//! criterion, as official `ServerPlayer.awardStat` / `resetStat` do. The scoreboard lives
//! on the master, so the changes are recorded here in order and handed over at the end of
//! the player's tick by [`tick_criteria`](crate::scoreboard::tick_criteria).

use std::collections::BTreeMap;

use crate::{inbound::PlayerId, region::Region};

/// 一项计分板准则，名字到交给主控时才拼出来。
///
/// One scoreboard criterion. The name is built only when the change is handed to the master.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScoreKey {
    /// `minecraft.used:<item>`，物品注册表 ID。
    ///
    /// `minecraft.used:<item>` by item registry id.
    Used(i32),
    /// `minecraft.custom:<stat>`，`custom_stat` 注册表 ID。
    ///
    /// `minecraft.custom:<stat>` by `custom_stat` registry id.
    Custom(i32),
    /// `minecraft.killed:<entity>`，实体类型注册表 ID。
    ///
    /// `minecraft.killed:<entity>` by entity type registry id.
    Killed(i32),
    /// `minecraft.killed_by:<entity>`。
    KilledBy(i32),
    /// `deathCount`、`playerKillCount`、`totalKillCount`。
    Named(&'static str),
    /// 官方 `ServerPlayer.handleTeamKill`：准则是 `prefix` 加上 `of` 所在队伍的颜色名。
    /// 队伍在主控上查，`of` 没有队伍或者队伍没有颜色时什么都不做。
    ///
    /// Official `ServerPlayer.handleTeamKill`: the criterion is `prefix` plus the colour
    /// of the team `of` belongs to. The master looks the team up; nothing happens when
    /// `of` has no team or the team has no colour.
    Team { prefix: &'static str, of: String },
}

impl ScoreKey {
    /// 官方 `Stat.buildName`：统计类型和条目的名字里 `:` 换成 `.`，中间用 `:` 连起来。
    ///
    /// Official `Stat.buildName`: `:` becomes `.` inside the stat type and the entry
    /// name, and the two are joined with `:`.
    pub(crate) fn criterion(&self, team_color: impl Fn(&str) -> Option<u8>) -> Option<String> {
        let stat = |kind: &str, name: Option<&&str>| {
            name.map(|name| format!("minecraft.{kind}:{}", name.replace(':', ".")))
        };
        let entity =
            |id: i32| mc_bamboo_data::registries::entity_type::NAMES.get(usize::try_from(id).ok()?);
        match self {
            Self::Used(item) => {
                let item = mc_bamboo_data::items::ITEMS.get(usize::try_from(*item).ok()?)?;
                stat("used", Some(&item.name))
            }
            Self::Custom(id) => stat(
                "custom",
                mc_bamboo_data::registries::custom_stat::NAMES.get(usize::try_from(*id).ok()?),
            ),
            Self::Killed(id) => stat("killed", entity(*id)),
            Self::KilledBy(id) => stat("killed_by", entity(*id)),
            Self::Named(name) => Some((*name).to_owned()),
            Self::Team { prefix, of } => {
                let color = mc_bamboo_command::TeamColorId(team_color(of)?);
                Some(format!("{prefix}{}", color.name()))
            }
        }
    }
}

/// 官方 `ScoreAccess.add` 或 `ScoreAccess.reset`（后者是 `set(0)`）。
///
/// Official `ScoreAccess.add` or `ScoreAccess.reset` (the latter is `set(0)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScoreChange {
    Add(i32),
    Reset,
}

#[derive(Debug, Default)]
pub(crate) struct ItemUseStats {
    /// 这一刻还没交给主控的计分板变化，按发生顺序。
    ///
    /// This tick's scoreboard changes not yet handed to the master, in order.
    scores: Vec<(ScoreKey, ScoreChange)>,
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
    /// 增加自定义统计。
    /// Increment a custom statistic.
    pub(crate) fn award_custom(&mut self, stat: i32, count: i32) {
        let (value, dirty) = self.custom.entry(stat).or_default();
        *value = value.saturating_add(count);
        *dirty = true;
        self.score(ScoreKey::Custom(stat), ScoreChange::Add(count));
    }

    /// 官方 `ServerPlayer.resetStat`：统计清零，计分板上对应的分数也 `reset`。
    ///
    /// Official `ServerPlayer.resetStat`: zero the stat and `reset` the matching scores.
    pub(crate) fn reset_custom(&mut self, stat: i32) {
        if let Some((value, dirty)) = self.custom.get_mut(&stat) {
            *value = 0;
            *dirty = true;
        }
        self.score(ScoreKey::Custom(stat), ScoreChange::Reset);
    }

    /// 记一条计分板变化，不动统计本身。
    ///
    /// Record one scoreboard change without touching the stat itself.
    pub(crate) fn score(&mut self, key: ScoreKey, change: ScoreChange) {
        self.scores.push((key, change));
    }

    /// 取走这一刻攒下的计分板变化。
    ///
    /// Take this tick's recorded scoreboard changes.
    pub(crate) fn take_scores(&mut self) -> Vec<(ScoreKey, ScoreChange)> {
        std::mem::take(&mut self.scores)
    }

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
        player
            .item_use_stats
            .score(ScoreKey::Used(item), ScoreChange::Add(1));
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
        stats.score(
            ScoreKey::Custom(mc_bamboo_data::registries::custom_stat::DAMAGE_DEALT),
            ScoreChange::Add(count),
        );
    }
}

/// `Player.awardStat(Stats.TALKED_TO_VILLAGER)` / `Stats.TRADED_WITH_VILLAGER`：
/// 自定义统计，`count` 是增量（官方 `incrementStat` 加 1）。
///
/// `Player.awardStat(Stats.TALKED_TO_VILLAGER)` / `Stats.TRADED_WITH_VILLAGER`:
/// custom stats; `count` is the increment (`incrementStat` adds 1).
pub(crate) fn award_custom(region: &mut Region, id: PlayerId, stat: i32, count: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        player.item_use_stats.award_custom(stat, count);
    }
}
