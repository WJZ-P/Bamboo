//! `WardenSpawnTracker`：玩家触发的监守者警告等级，存在玩家档里。
//!
//! 26.3 里它每刻随玩家 `doTick` 推进，幽匿尖啸体 `tryShriek` 时按
//! [`WardenSpawnTracker::try_warn`] 累积；达到 4 级才尝试召唤监守者（监守者本体
//! 这一轮还没有，召唤留成接口）。警告等级每 12000 刻没有新警告就掉一级，两次
//! 累积之间至少隔 200 刻。

use mc_bamboo_nbt::Tag;

/// 幽匿尖啸体 `tryToWarn`：给这名玩家加一级警告，返回警告后的等级。
/// 世界不存玩家警告等级时返回 `None`。
pub fn warn_player(world: &mut crate::World<'_>, player: crate::entity::EntityUid) -> Option<i32> {
    world.warn_player(player)
}

/// `WardenSpawnTracker.MAX_WARNING_LEVEL`。
pub const MAX_WARNING_LEVEL: i32 = 4;
/// `DECREASE_WARNING_LEVEL_EVERY_INTERVAL`：这么久没新警告就掉一级。
const DECREASE_EVERY_INTERVAL: i32 = 12_000;
/// `WARNING_LEVEL_INCREASE_COOLDOWN`：两次累积之间的最短间隔。
const INCREASE_COOLDOWN: i32 = 200;
/// `PLAYER_SEARCH_RADIUS`：尖啸体响时离得多近的玩家会被警告。
pub const PLAYER_SEARCH_RADIUS: f64 = 16.0;

/// 一名玩家的监守者警告状态。`Default` 是官方新建玩家时的值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WardenSpawnTracker {
    ticks_since_last_warning: i32,
    warning_level: i32,
    cooldown_ticks: i32,
}

impl WardenSpawnTracker {
    /// `tick()`：随玩家 tick 推进衰减与冷却。
    pub fn tick(&mut self) {
        if self.ticks_since_last_warning >= DECREASE_EVERY_INTERVAL {
            self.decrease_warning_level();
            self.ticks_since_last_warning = 0;
        } else {
            self.ticks_since_last_warning += 1;
        }
        if self.cooldown_ticks > 0 {
            self.cooldown_ticks -= 1;
        }
    }

    /// `reset()`：把三格状态都归零。
    pub fn reset(&mut self) {
        self.ticks_since_last_warning = 0;
        self.warning_level = 0;
        self.cooldown_ticks = 0;
    }

    /// `getWarningLevel()`。
    pub fn warning_level(&self) -> i32 {
        self.warning_level
    }

    /// `onCooldown()`：两次累积之间的冷却还没过。
    pub fn on_cooldown(&self) -> bool {
        self.cooldown_ticks > 0
    }

    /// `setWarningLevel(level)`：夹到 0..=4。
    pub fn set_warning_level(&mut self, level: i32) {
        self.warning_level = level.clamp(0, MAX_WARNING_LEVEL);
    }

    /// `increaseWarningLevel()`：不在冷却里才加一级，并把冷却设成 200 刻。
    fn increase_warning_level(&mut self) {
        if self.on_cooldown() {
            return;
        }
        self.ticks_since_last_warning = 0;
        self.cooldown_ticks = INCREASE_COOLDOWN;
        self.set_warning_level(self.warning_level + 1);
    }

    /// `decreaseWarningLevel()`。
    fn decrease_warning_level(&mut self) {
        self.set_warning_level(self.warning_level - 1);
    }

    /// `tryWarn` 的单玩家路径：不在冷却里就加一级，返回警告后的等级。
    /// 冷却没过、或附近已经有监守者（这一轮还没有监守者，见 README 限制）时返回 `None`。
    pub fn try_warn(&mut self) -> Option<i32> {
        if self.on_cooldown() {
            return None;
        }
        self.increase_warning_level();
        Some(self.warning_level)
    }

    /// `WardenSpawnTracker.CODEC` 的三个键，键序照官方。
    pub fn write_codec(self) -> Tag {
        let mut out = Vec::new();
        crate::archive::tag::put(
            &mut out,
            "ticks_since_last_warning",
            Tag::Int(self.ticks_since_last_warning),
        );
        crate::archive::tag::put(&mut out, "warning_level", Tag::Int(self.warning_level));
        crate::archive::tag::put(&mut out, "cooldown_ticks", Tag::Int(self.cooldown_ticks));
        Tag::Compound(out)
    }

    /// 从玩家档的 `warden_spawn_tracker` 复合标签还原；坏数据回到默认值。
    pub fn read_codec(tag: &Tag) -> Self {
        let mut tracker = Self::default();
        let Tag::Compound(values) = tag else {
            return tracker;
        };
        for (key, value) in values {
            let Ok(int) = crate::archive::tag::java_i32(value) else {
                continue;
            };
            match key.as_str() {
                "ticks_since_last_warning" => {
                    tracker.ticks_since_last_warning = int.max(0);
                }
                "warning_level" => tracker.set_warning_level(int),
                "cooldown_ticks" => tracker.cooldown_ticks = int.max(0),
                _ => {}
            }
        }
        tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_level_accumulates_with_cooldown_then_decays() {
        let mut tracker = WardenSpawnTracker::default();
        assert_eq!(tracker.try_warn(), Some(1));
        // 冷却没过，再触发不加。
        assert_eq!(tracker.try_warn(), None);
        // 过完 200 刻冷却又能加一级。
        for _ in 0..200 {
            tracker.tick();
        }
        assert_eq!(tracker.try_warn(), Some(2));
        assert_eq!(tracker.warning_level(), 2);
        // 12000 刻没有新警告后，下一次 tick 掉回一级。
        for _ in 0..12_001 {
            tracker.tick();
        }
        assert_eq!(tracker.warning_level(), 1);
    }

    #[test]
    fn warning_level_clamps_at_four() {
        let mut tracker = WardenSpawnTracker::default();
        for _ in 0..4 {
            assert!(tracker.try_warn().is_some());
            for _ in 0..200 {
                tracker.tick();
            }
        }
        assert_eq!(tracker.warning_level(), 4);
        // 再累积被夹住。
        tracker.set_warning_level(99);
        assert_eq!(tracker.warning_level(), 4);
    }

    #[test]
    fn codec_round_trip() {
        let tracker = WardenSpawnTracker {
            ticks_since_last_warning: 42,
            warning_level: 3,
            cooldown_ticks: 7,
        };
        let tag = tracker.write_codec();
        let read = WardenSpawnTracker::read_codec(&tag);
        assert_eq!(read, tracker);
        // 坏数据回到默认值。
        assert_eq!(
            WardenSpawnTracker::read_codec(&Tag::Int(1)),
            WardenSpawnTracker::default()
        );
    }
}
