//! 守卫者 / 远古守卫者：官方 `Guardian` / `ElderGuardian` 的构造事实、目标表与状态。
//!
//! 移动控制、激光和范围效果在后续阶段接入；这里放属性、路径代价、目标表、音效
//! 分派和实体上需要随存档搬动的自有状态。
//!
//! Guardian / elder guardian: the construction facts, goal table and state of vanilla
//! `Guardian` / `ElderGuardian`. Movement control, the laser and the area effect are
//! wired in later phases; this module holds attributes, pathfinding malus, the goal
//! table, sound dispatch and the state that travels with the entity row.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{PathType, registries::attribute, tags::fluid_tags};

use super::{
    Attributes, Malus, MobType,
    goals::{Goal, GoalSelector, PLAIN_STROLL, WrappedGoal},
};
use crate::entity::ctx::BlockView;

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

/// `Guardian.getWalkTargetValue`：水格给 `10 + 光照路径代价`，其余走 `Monster` 那一支。
///
/// `Guardian.getWalkTargetValue`: water cells get `10 + the light path cost`,
/// otherwise the `Monster` branch.
pub fn walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    if view.fluid_state(pos).is_tagged(fluid_tags::WATER) {
        10.0 + super::goals::light_level_cost(view, pos)
    } else {
        super::goals::monster_walk_target_value(view, pos)
    }
}

/// `Guardian.registerGoals` 的 `goalSelector`，按注册顺序。激光和回家目标先用
/// [`Goal::Unavailable`] 占住优先级，逻辑在后续阶段接入。
///
/// The `goalSelector` half of `Guardian.registerGoals`, in registration order. The
/// laser and go-home goals keep their priority slots as [`Goal::Unavailable`] until
/// their behaviour is wired in a later phase.
pub fn goals(kind: MobType) -> GoalSelector {
    // `ElderGuardian` 构造函数把 `randomStrollGoal.setInterval(400)`。
    // The `ElderGuardian` constructor calls `randomStrollGoal.setInterval(400)`.
    let stroll_interval = if kind == MobType::ElderGuardian {
        400
    } else {
        80
    };
    GoalSelector::from_goals(vec![
        wrap(4, Goal::Unavailable("GuardianAttackGoal")),
        wrap(5, Goal::Unavailable("MoveTowardsRestrictionGoal")),
        wrap(
            7,
            Goal::Stroll {
                speed: 1.0,
                interval: stroll_interval,
                probability: PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            8,
            Goal::LookAtMob {
                kind: MobType::Guardian,
                look_distance: 12.0,
                probability: 0.01,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            9,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Guardian.registerGoals` 的 `targetSelector`：优先级 1 的
/// `NearestAttackableTargetGoal`。选择器和激光一起在后续阶段接入，先占位。
///
/// The `targetSelector` half of `Guardian.registerGoals`: the priority-1
/// `NearestAttackableTargetGoal`. Its selector arrives with the laser in a later
/// phase; kept as a placeholder for now.
pub fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![wrap(
        1,
        Goal::Unavailable("NearestAttackableTargetGoal"),
    )])
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

    /// 目标表：优先级、看守卫者变体、以及远古守卫者的游走间隔 400。
    ///
    /// Goal table: priorities, the "watch guardians" variant and the elder
    /// guardian's stroll interval of 400.
    #[test]
    fn guardian_goal_table() {
        let table = goals(MobType::Guardian).goals;
        let priorities: Vec<i32> = table.iter().map(|wrapped| wrapped.priority).collect();
        assert_eq!(priorities, vec![4, 5, 7, 8, 8, 9]);
        assert!(matches!(
            table[2].goal,
            Goal::Stroll {
                interval: 80,
                probability,
                ..
            } if probability == PLAIN_STROLL
        ));
        assert!(matches!(
            table[3].goal,
            Goal::LookAtPlayer {
                look_distance,
                probability,
                ..
            } if look_distance == 8.0 && probability == 0.02
        ));
        assert!(matches!(
            table[4].goal,
            Goal::LookAtMob {
                kind: MobType::Guardian,
                look_distance,
                probability,
                ..
            } if look_distance == 12.0 && probability == 0.01
        ));
        assert!(matches!(table[5].goal, Goal::RandomLookAround { .. }));

        // `ElderGuardian` 构造函数 `setInterval(400)`。
        // The `ElderGuardian` constructor calls `setInterval(400)`.
        let elder = goals(MobType::ElderGuardian).goals;
        assert!(matches!(elder[2].goal, Goal::Stroll { interval: 400, .. }));

        let targets = target_goals().goals;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].priority, 1);
    }

    /// 目标与生成规则按字节码：完整目标规则（从而有视线记忆）、`IN_WATER` 生成、
    /// 未覆写 `isPushedByFluid`（缺省为真）。
    ///
    /// Targeting and placement rules from bytecode: full target rules (hence sight
    /// memory), `IN_WATER` placement, and no `isPushedByFluid` override (default true).
    #[test]
    fn guardian_target_and_placement_rules() {
        assert!(MobType::Guardian.full_target_rules());
        assert!(MobType::Guardian.uses_target_memory());
        assert!(MobType::Guardian.spawns_in_water());
        assert!(MobType::ElderGuardian.spawns_in_water());
        assert!(MobType::Guardian.is_pushed_by_fluid());
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
