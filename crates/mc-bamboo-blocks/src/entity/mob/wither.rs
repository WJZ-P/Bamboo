//! `WitherBoss`：官方的 `Monster` → `WitherBoss` 专属状态。
//!
//! 三个头的目标、无敌期、Boss 血条、射凋灵之首、半血护甲和破坏方块计时都记在
//! [`Wither`] 上；逐刻推进在别的模块接进主 tick 流程。
//!
//! `WitherBoss`: the vanilla `Monster` → `WitherBoss` state. The three head
//! targets, invulnerability phase, boss bar, wither-skull shots, half-health
//! armour and block-destruction clock all live on [`Wither`]; the per-tick
//! advancement is wired into the main tick elsewhere.

use mc_bamboo_data::registries::attribute;

use super::{attributes::Attributes, goals::GoalSelector};

/// 无敌期长度，`WitherBoss.INVULNERABLE_TICKS`。
///
/// Length of the invulnerability phase, `WitherBoss.INVULNERABLE_TICKS`.
pub const INVULNERABLE_TICKS: i32 = 220;

/// 凋灵的专属状态，只在 `MobType::Wither` 分支读写。
///
/// Wither-only state, read and written only on the `MobType::Wither` branch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wither {
    /// `DATA_TARGET_A/B/C`：三个头各自的目标实体 ID（0 表示没有）。
    ///
    /// `DATA_TARGET_A/B/C`: each head's target entity id (0 means none).
    pub targets: [i32; 3],
    /// `DATA_ID_INV`：剩余无敌刻数。
    ///
    /// `DATA_ID_INV`: the remaining invulnerability ticks.
    pub invulnerable_ticks: i32,
    /// 两个侧头当前的俯仰 / 偏航角（`xRotHeads` / `yRotHeads`）。
    ///
    /// The two side heads' current pitch and yaw (`xRotHeads` / `yRotHeads`).
    pub x_rot_heads: [f32; 2],
    pub y_rot_heads: [f32; 2],
    /// 两个侧头上一刻的俯仰 / 偏航角（`xRotOHeads` / `yRotOHeads`）。
    ///
    /// The two side heads' previous pitch and yaw (`xRotOHeads` / `yRotOHeads`).
    pub x_rot_o_heads: [f32; 2],
    pub y_rot_o_heads: [f32; 2],
    /// `nextHeadUpdate`：两个侧头下次可射击的刻数。
    ///
    /// `nextHeadUpdate`: the tick at which each side head may next fire.
    pub next_head_update: [i32; 2],
    /// `idleHeadUpdates`：两个侧头空闲射击的累计次数。
    ///
    /// `idleHeadUpdates`: each side head's accumulated idle-shot count.
    pub idle_head_updates: [i32; 2],
    /// `destroyBlocksTick`：受伤后破坏方块的倒计时。
    ///
    /// `destroyBlocksTick`: the countdown to the block-destruction sweep after a hit.
    pub destroy_blocks_tick: i32,
    /// `ServerBossEvent` 当前进度。
    ///
    /// The current `ServerBossEvent` progress.
    pub boss_progress: f32,
}

impl Wither {
    /// `makeInvulnerable()`：把无敌刻数置 220、血条清零。血量由调用方按
    /// `getMaxHealth()/3` 设置，因为最大血量活在属性表上。
    ///
    /// `makeInvulnerable()`: set the invulnerability ticks to 220 and clear the
    /// boss bar. Health is set by the caller to `getMaxHealth()/3` because the
    /// maximum lives on the attribute table.
    pub fn make_invulnerable(&mut self) {
        self.invulnerable_ticks = INVULNERABLE_TICKS;
        self.boss_progress = 0.0;
    }
}

/// `WitherBoss.createAttributes()`：`Monster.createMonsterAttributes()` 上覆盖五个值，
/// 并追加 `FLYING_SPEED`。
///
/// `WitherBoss.createAttributes()`: five overrides plus an added `FLYING_SPEED`
/// on `Monster.createMonsterAttributes()`.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, 300.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.6_f32));
    table.set_base(attribute::FOLLOW_RANGE, 40.0);
    table.set_base(attribute::ARMOR, 4.0);
    if table.get(attribute::FLYING_SPEED).is_none() {
        table.entries.push(super::attributes::Attribute::new(
            attribute::FLYING_SPEED,
            f64::from(0.6_f32),
        ));
    }
    table
}

/// `WitherBoss.registerGoals` 的 `goalSelector`。
///
/// The `goalSelector` of `WitherBoss.registerGoals`.
pub(super) fn goals() -> GoalSelector {
    GoalSelector::default()
}

/// `WitherBoss.registerGoals` 的 `targetSelector`。
///
/// The `targetSelector` of `WitherBoss.registerGoals`.
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_match_the_official_table() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 300.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), f64::from(0.6_f32));
        assert_eq!(table.value(attribute::FLYING_SPEED), f64::from(0.6_f32));
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 40.0);
        assert_eq!(table.value(attribute::ARMOR), 4.0);
    }

    #[test]
    fn make_invulnerable_starts_the_220_tick_phase() {
        let mut state = Wither::default();
        state.boss_progress = 0.75;
        state.make_invulnerable();
        assert_eq!(state.invulnerable_ticks, 220);
        assert_eq!(state.boss_progress, 0.0);
    }
}
