//! 守卫者 / 远古守卫者：官方 `Guardian` / `ElderGuardian` 的构造事实与状态。
//!
//! 目标表、移动控制、激光和范围效果在后续阶段接入；这里先放属性、路径代价、
//! 音效分派和实体上需要随存档搬动的自有状态。
//!
//! Guardian / elder guardian: the construction facts and state of vanilla `Guardian` /
//! `ElderGuardian`. Goals, movement control, the laser and the area effect are wired in
//! later phases; this module holds attributes, pathfinding malus, sound dispatch and the
//! state that travels with the entity row.

use mc_bamboo_data::{PathType, registries::attribute};

use super::{Attributes, Malus, MobType, goals::GoalSelector};

/// 是不是守卫者一支。
///
/// Whether this is a guardian (either kind).
pub fn is_guardian(kind: MobType) -> bool {
    matches!(kind, MobType::Guardian | MobType::ElderGuardian)
}

/// 官方 `Guardian` / `ElderGuardian` 的自有字段。都不是存档字段。
///
/// The own fields of vanilla `Guardian` / `ElderGuardian`; none is saved to disk.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GuardianState {
    /// `clientSideTailAnimation`：构造时抽一次 `nextFloat`。
    ///
    /// `clientSideTailAnimation`: one `nextFloat` drawn by the constructor.
    pub tail_animation: f32,
    /// `clientSideTailAnimationO`。
    pub tail_animation_o: f32,
    /// `clientSideTailAnimationSpeed`。
    pub tail_speed: f32,
    /// `clientSideSpikesAnimation`。
    pub spikes_animation: f32,
    /// `clientSideSpikesAnimationO`。
    pub spikes_animation_o: f32,
    /// `DATA_ID_MOVING`（布尔元数据）：守卫者有没有在动。
    ///
    /// `DATA_ID_MOVING` (boolean metadata): whether the guardian is moving.
    pub moving: bool,
    /// `DATA_ID_ATTACK_TARGET`（整数元数据）：0 表示没有激光目标。
    ///
    /// `DATA_ID_ATTACK_TARGET` (integer metadata): 0 means no laser target.
    pub attack_target: i32,
    /// `GuardianAttackGoal.attackTime`，`start` 时置 -10。
    ///
    /// `GuardianAttackGoal.attackTime`, set to -10 by `start`.
    pub attack_time: i32,
    /// `clientSideTouchedGround`。
    pub touched_ground: bool,
}

/// `Guardian.createAttributes` / `ElderGuardian.createAttributes`：
/// `Monster.createMonsterAttributes()` 上覆盖生命、攻击和移速。
///
/// `Guardian.createAttributes` / `ElderGuardian.createAttributes`: the monster defaults
/// with health, attack damage and movement speed overridden.
pub fn attributes(elder: bool) -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, if elder { 80.0 } else { 30.0 });
    table.set_base(attribute::ATTACK_DAMAGE, if elder { 8.0 } else { 6.0 });
    table.set_base(
        attribute::MOVEMENT_SPEED,
        if elder { 0.30000001192092896 } else { 0.5 },
    );
    table
}

/// `Guardian` 构造函数：`setPathfindingMalus(PathType.WATER, 0.0)`。
///
/// The `Guardian` constructor: `setPathfindingMalus(PathType.WATER, 0.0)`.
pub fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(PathType::Water, 0.0);
    malus
}

/// 守卫者的目标表在后续阶段接入；目前两个选择器都为空。
///
/// The guardian goal tables are wired in a later phase; both selectors are empty for now.
pub fn goals() -> GoalSelector {
    GoalSelector::default()
}

/// 守卫者的目标选择器在后续阶段接入；目前为空。
///
/// The guardian target selector is wired in a later phase; empty for now.
pub fn target_goals() -> GoalSelector {
    GoalSelector::default()
}

/// `Guardian.getAmbientSound()` / `ElderGuardian.getAmbientSound()`：水中和岸上各一套。
///
/// `Guardian.getAmbientSound()` / `ElderGuardian.getAmbientSound()`: water and land variants.
pub fn ambient_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.ambient",
        (false, false) => "minecraft:entity.guardian.ambient_land",
        (true, true) => "minecraft:entity.elder_guardian.ambient",
        (true, false) => "minecraft:entity.elder_guardian.ambient_land",
    }
}

/// `Guardian.getHurtSound()` / `ElderGuardian.getHurtSound()`：水中和岸上各一套。
///
/// `Guardian.getHurtSound()` / `ElderGuardian.getHurtSound()`: water and land variants.
pub fn hurt_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.hurt",
        (false, false) => "minecraft:entity.guardian.hurt_land",
        (true, true) => "minecraft:entity.elder_guardian.hurt",
        (true, false) => "minecraft:entity.elder_guardian.hurt_land",
    }
}

/// `Guardian.getDeathSound()` / `ElderGuardian.getDeathSound()`：水中和岸上各一套。
///
/// `Guardian.getDeathSound()` / `ElderGuardian.getDeathSound()`: water and land variants.
pub fn death_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.death",
        (false, false) => "minecraft:entity.guardian.death_land",
        (true, true) => "minecraft:entity.elder_guardian.death",
        (true, false) => "minecraft:entity.elder_guardian.death_land",
    }
}

/// `Guardian.getFlopSound()` / `ElderGuardian.getFlopSound()`：岸上扑腾音。
///
/// `Guardian.getFlopSound()` / `ElderGuardian.getFlopSound()`: the land flop sound.
pub fn flop_sound(elder: bool) -> &'static str {
    if elder {
        "minecraft:entity.elder_guardian.flop"
    } else {
        "minecraft:entity.guardian.flop"
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{PathType, registries::attribute};

    use super::*;
    use crate::entity::mob::{Mob, MobType};

    /// 构造事实：属性、水代价、导航、持久化和音效分派都按官方类。
    ///
    /// Construction facts: attributes, water malus, navigation, persistence and the
    /// sound dispatch follow the vanilla classes.
    #[test]
    fn guardian_construction_facts() {
        let guardian = Mob::new(MobType::Guardian);
        assert_eq!(guardian.attributes.base(attribute::MAX_HEALTH), 30.0);
        assert_eq!(guardian.attributes.base(attribute::ATTACK_DAMAGE), 6.0);
        assert_eq!(guardian.attributes.base(attribute::MOVEMENT_SPEED), 0.5);
        assert_eq!(guardian.malus.get(PathType::Water), 0.0);
        assert_eq!(guardian.navigation.flags.swim, Some(false));
        assert!(!guardian.persistence_required);
        assert_eq!(guardian.guardian, GuardianState::default());

        let elder = Mob::new(MobType::ElderGuardian);
        assert_eq!(elder.attributes.base(attribute::MAX_HEALTH), 80.0);
        assert_eq!(elder.attributes.base(attribute::ATTACK_DAMAGE), 8.0);
        assert_eq!(
            elder.attributes.base(attribute::MOVEMENT_SPEED),
            0.30000001192092896
        );
        assert_eq!(elder.malus.get(PathType::Water), 0.0);
        assert_eq!(elder.navigation.flags.swim, Some(false));
        // `ElderGuardian` 构造函数 `setPersistenceRequired()`。
        // The `ElderGuardian` constructor calls `setPersistenceRequired()`.
        assert!(elder.persistence_required);
    }

    #[test]
    fn guardian_sound_variants() {
        assert_eq!(
            ambient_sound(false, true),
            "minecraft:entity.guardian.ambient"
        );
        assert_eq!(
            ambient_sound(true, false),
            "minecraft:entity.elder_guardian.ambient_land"
        );
        assert_eq!(
            hurt_sound(false, false),
            "minecraft:entity.guardian.hurt_land"
        );
        assert_eq!(
            death_sound(true, true),
            "minecraft:entity.elder_guardian.death"
        );
        assert_eq!(flop_sound(true), "minecraft:entity.elder_guardian.flop");
        assert_eq!(flop_sound(false), "minecraft:entity.guardian.flop");
    }
}
