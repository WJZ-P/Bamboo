//! 玩家经验值：等级、进度条、总经验以及官方的增减公式。
//!
//! 三个字段和四个方法都照 26.3 的 `Player` / `ServerPlayer` 字节码：进度是 `float`，
//! 每一步只做一次 Java 的 float 运算；总经验是饱和加、夹在 0 和 `i32::MAX` 之间。
//! [`Player`](crate::player::Player) 把状态存在这里，存档和网络包从这里取值。
//!
//! Player experience: level, progress, total and the official arithmetic.
//! The three fields and the formulas follow the 26.3 `Player` / `ServerPlayer`
//! bytecode. Progress is a `float`, each step performs one Java float operation,
//! and the total is clamped between 0 and `i32::MAX`. [`Player`](crate::player::Player)
//! keeps the state here; saves and packets read it from here.

use mc_bamboo_protocol::packet::play::SetExperience;

use crate::region::Region;

/// 官方 `Player` 的 `experienceLevel` / `totalExperience` / `experienceProgress`。
///
/// Vanilla `Player`'s `experienceLevel` / `totalExperience` / `experienceProgress`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Experience {
    /// 等级，官方 `experienceLevel`。
    pub(crate) level: i32,
    /// 累计经验点，官方 `totalExperience`（不是当前级里的点数）。
    pub(crate) total: i32,
    /// 当前级的进度，0 到 1 之间的 `f32`。
    pub(crate) progress: f32,
}

impl Default for Experience {
    fn default() -> Self {
        Self {
            level: 0,
            total: 0,
            progress: 0.0,
        }
    }
}

impl Experience {
    /// 进服时发给客户端的那一包。
    ///
    /// The packet sent to the client when it joins.
    pub(crate) fn packet(self) -> SetExperience {
        SetExperience {
            experience_progress: self.progress,
            experience_level: self.level,
            total_experience: self.total,
        }
    }

    /// 官方 `Player.getXpNeededForNextLevel`。
    pub(crate) fn needed_for_next_level(self) -> i32 {
        xp_needed_for_level(self.level)
    }

    /// 官方 `ExperienceCommand.Type.POINTS.query`：当前级里换算出来的点数。
    ///
    /// Official `ExperienceCommand.Type.POINTS.query`: the points inside the current level.
    pub(crate) fn points_in_level(self) -> i32 {
        mc_bamboo_core::mth::floor_f32(self.progress * self.needed_for_next_level() as f32)
    }

    /// 官方 `Player.giveExperiencePoints`（含 `increaseScore` 的 `score` 字段）。
    ///
    /// `Player.giveExperiencePoints`, including `increaseScore` on `score`.
    pub(crate) fn give_points(&mut self, points: i32, score: &mut i32) {
        *score = score.wrapping_add(points);
        self.progress += points as f32 / self.needed_for_next_level() as f32;
        // Java 的 `totalExperience + points` 先按 int 溢出，再 `Mth.clamp`。
        // Java adds `totalExperience + points` as an int (wrapping) before clamping.
        self.total = self.total.wrapping_add(points).clamp(0, i32::MAX);
        // 负数循环先把这一级退回去，把剩余点数按新的本级需求换算成进度。
        // The negative loop steps back a level and rescales the remaining points
        // with the new level's requirement.
        while self.progress < 0.0 {
            let carry = self.progress * self.needed_for_next_level() as f32;
            if self.level > 0 {
                self.give_levels(-1);
                self.progress = 1.0 + carry / self.needed_for_next_level() as f32;
            } else {
                self.give_levels(-1);
                self.progress = 0.0;
            }
        }
        while self.progress >= 1.0 {
            self.progress = (self.progress - 1.0) * self.needed_for_next_level() as f32;
            self.give_levels(1);
            // 官方把溢出点数换成新一级的进度：`progress = progress / newNeed`。
            // Official converts the overflow into the new level's progress:
            // `progress = progress / newNeed`.
            self.progress /= self.needed_for_next_level() as f32;
        }
    }

    /// 官方 `Player.giveExperienceLevels` 的状态部分（升级音效不在这一层）。
    ///
    /// The state part of `Player.giveExperienceLevels` (the level-up sound
    /// is not part of this layer).
    pub(crate) fn give_levels(&mut self, levels: i32) {
        self.level = self.level.saturating_add(levels);
        if self.level < 0 {
            self.level = 0;
            self.progress = 0.0;
            self.total = 0;
        }
    }

    /// 官方 `ServerPlayer.setExperienceLevels`：等级没变就什么也不做。
    ///
    /// Official `ServerPlayer.setExperienceLevels`: a no-op when the level is unchanged.
    pub(crate) fn set_level(&mut self, level: i32) {
        self.level = level;
    }

    /// 官方 `ServerPlayer.setExperiencePoints`：`Mth.clamp(amount / need, 0, (need-1)/need)`。
    ///
    /// Official `ServerPlayer.setExperiencePoints`:
    /// `Mth.clamp(amount / need, 0, (need-1)/need)`.
    pub(crate) fn set_points(&mut self, points: i32) -> bool {
        let need = self.needed_for_next_level() as f32;
        let max = (need - 1.0) / need;
        let progress = mc_bamboo_core::mth::clamp_f32(points as f32 / need, 0.0, max);
        if progress == self.progress {
            return false;
        }
        self.progress = progress;
        true
    }
}

/// 官方 `ServerPlayer.doTick` 末尾：总经验和上次发包时不同才补发一包。
/// `set_experience` 系列方法会把 `lastSentExp` 改成 `-1` 强制下一刻发。
///
/// At the end of `ServerPlayer.doTick`: a packet goes out when the total differs
/// from the last sent one. The `set_experience` methods set `lastSentExp` to
/// `-1` to force a packet on the next tick.
pub(crate) fn sync_dirty(region: &mut Region) {
    let mut pending = Vec::new();
    for (id, player) in &mut region.players {
        if player.experience.total != player.last_sent_exp {
            player.last_sent_exp = player.experience.total;
            pending.push((*id, player.experience.packet()));
        }
    }
    for (id, packet) in pending {
        region.links.send(id, &packet);
    }
}

/// 官方 `Player.getXpNeededForNextLevel`，三段折线。
///
/// Vanilla `Player.getXpNeededForNextLevel`: a three-piece step function.
pub(crate) fn xp_needed_for_level(level: i32) -> i32 {
    if level >= 30 {
        112_i32.wrapping_add(level.wrapping_sub(30).wrapping_mul(9))
    } else if level >= 15 {
        37_i32.wrapping_add(level.wrapping_sub(15).wrapping_mul(5))
    } else {
        7_i32.wrapping_add(level.wrapping_mul(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_thresholds_match_the_official_piecewise_formula() {
        assert_eq!(xp_needed_for_level(0), 7);
        assert_eq!(xp_needed_for_level(14), 35);
        assert_eq!(xp_needed_for_level(15), 37);
        assert_eq!(xp_needed_for_level(29), 107);
        assert_eq!(xp_needed_for_level(30), 112);
        assert_eq!(xp_needed_for_level(31), 121);
    }

    #[test]
    fn points_level_up_uses_the_official_float_sequence() {
        let mut xp = Experience::default();
        let mut score = 0;
        xp.give_points(7, &mut score);
        assert_eq!((xp.level, xp.total, xp.progress), (1, 7, 0.0));
        // 升级之后本级需求从 7 变成 9，剩余的点数要按 9 重算。
        xp.give_points(10, &mut score);
        assert_eq!(xp.level, 2);
        assert_eq!(xp.total, 17);
        assert!((xp.progress - 1.0 / 11.0).abs() < 1.0e-6);
        assert_eq!(score, 17);
    }

    #[test]
    fn negative_points_walk_back_levels_and_zeroes_at_level_zero() {
        let mut xp = Experience {
            level: 2,
            total: 17,
            progress: 1.0 / 11.0,
        };
        let mut score = 0;
        xp.give_points(-8, &mut score);
        assert_eq!(xp.level, 1);
        assert_eq!(xp.total, 9);
        assert!((xp.progress - 1.0 / 9.0).abs() < 1.0e-6);
        xp.give_points(-100, &mut score);
        assert_eq!(xp.level, 0);
        assert_eq!(xp.total, 0);
        assert_eq!(xp.progress, 0.0);
    }

    #[test]
    fn set_points_clamps_to_just_below_the_next_level() {
        let mut xp = Experience::default();
        assert!(xp.set_points(6));
        assert_eq!(xp.progress, 6.0 / 7.0);
        assert!(!xp.set_points(6));
        let need = xp.needed_for_next_level() as f32;
        assert_eq!(xp.points_in_level(), 6);
        assert!(xp.progress <= (need - 1.0) / need);
    }
}
