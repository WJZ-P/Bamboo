//! 嘎枝的大脑：官方 `CreakingAi` 的传感器表、活动与行为登记表，以及
//! `customServerAiStep` 里的活动切换。
//!
//! The creaking's brain: vanilla `CreakingAi`'s sensor table, activity and behaviour
//! registration tables, and the activity switch in `customServerAiStep`.

use super::{
    Mob,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        memory::{MemoryStatus, MemoryType, MemoryValue, PositionTracker, WalkTarget},
        sensor::{self, SensorKind},
        template_cache,
    },
    creaking, goals,
};
use crate::entity::{Entity, ctx::EntityCtx};

use MemoryStatus::{ValueAbsent, ValuePresent};
use MemoryType::{AttackTarget, AttackCoolingDown, LookTarget, WalkTarget as WalkTargetMemory};

/// `Creaking.makeBrain` 的传感器顺序：`NEAREST_LIVING_ENTITIES` 和 `NEAREST_PLAYERS`。
const SENSORS: &[SensorKind] = &[SensorKind::NearestLivingEntities, SensorKind::NearestPlayers];

/// `Brain.provider(sensors, activities)`：缓存一份模板。
pub(in crate::entity::mob) fn brain() -> Brain {
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            SENSORS,
            vec![core_activity(), idle_activity(), fight_activity()],
        )
    })
}

/// `initCoreActivity`：`Swim(CreakingAi$1, 0.8)` + `LookAtTargetSink(45, 90)` +
/// `MoveToTargetSink`。`CreakingAi$1` 的 `checkExtraStartConditions` 多一层 `canMove`。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Creaking(Behavior::Swim { chance: 0.8 }),
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
        ],
    )
}

/// `initIdleActivity`：`StartAttacking`（激活且看得见可攻击玩家时）+
/// `SetEntityLookTargetSometimes(8.0, 30..60)` + `RunOne[RandomStroll(0.3),
/// SetWalkTargetFromLookTarget(0.3, 3), DoNothing(30, 60)]`。
fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (0, Kind::Creaking(Behavior::StartAttacking)),
            (
                1,
                Kind::LookAtPlayerSometimes {
                    max_dist: 8.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (
                2,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (Kind::RandomStroll { speed: 0.3 }, 2),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::Fixed(0.3),
                                close_enough: 3,
                            },
                            3,
                        ),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
        ],
    )
}

/// `initFightActivity`：`SetWalkTargetFromAttackTargetIfTargetOutOfReach(1.0)` +
/// `MeleeAttack(canMove, 40)` + `StopAttackingIfTargetInvalid(!reachable)`，进入条件是
/// `ATTACK_TARGET` 有值。
fn fight_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Fight,
        vec![
            (0, Kind::Creaking(Behavior::SetWalkTargetFromAttackTarget)),
            (1, Kind::Creaking(Behavior::MeleeAttack)),
            (2, Kind::Creaking(Behavior::StopAttacking)),
        ],
    )
    .when(vec![(AttackTarget, ValuePresent)])
}

/// 嘎枝自己的一次性行为。
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `CreakingAi$1`：`Swim(0.8)`，`checkExtraStartConditions` 多加 `canMove`。
    Swim { chance: f32 },
    /// `StartAttacking.create(isActive, NEAREST_VISIBLE_ATTACKABLE_PLAYER)`。
    StartAttacking,
    /// `SetWalkTargetFromAttackTargetIfTargetOutOfReach.create(1.0)`。
    SetWalkTargetFromAttackTarget,
    /// `MeleeAttack.create(canMove, 40)`。
    MeleeAttack,
    /// `StopAttackingIfTargetInvalid.create(!isAttackTargetStillReachable)`。
    StopAttacking,
}

impl Behavior {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Swim { .. } => "Swim",
            Self::StartAttacking => "StartAttacking",
            Self::SetWalkTargetFromAttackTarget => "SetWalkTargetFromAttackTargetIfTargetOutOfReach",
            Self::MeleeAttack => "MeleeAttack",
            Self::StopAttacking => "StopAttackingIfTargetInvalid",
        }
    }

    pub(super) fn class(&self) -> Class {
        match self {
            Self::Swim { .. } => Class::Timed { min: 60, max: 60 },
            _ => Class::OneShot,
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::Swim { .. } => &[],
            Self::StartAttacking => &[],
            // `SetWalkTargetFromAttackTargetIfTargetOutOfReach` 的 `requires`：
            // `ATTACK_TARGET`、`LOOK_TARGET`（写）、`WALK_TARGET`（写）和
            // `NEAREST_VISIBLE_LIVING_ENTITIES`。
            Self::SetWalkTargetFromAttackTarget => &[
                (AttackTarget, ValuePresent),
                (MemoryType::NearestVisibleLivingEntities, ValuePresent),
            ],
            // `MeleeAttack` 的 `requires`：`LOOK_TARGET`（registered）、`ATTACK_TARGET`、
            // `ATTACK_COOLING_DOWN`（absent）、`NEAREST_VISIBLE_LIVING_ENTITIES`。
            Self::MeleeAttack => &[
                (LookTarget, MemoryStatus::Registered),
                (AttackTarget, ValuePresent),
                (AttackCoolingDown, ValueAbsent),
                (MemoryType::NearestVisibleLivingEntities, ValuePresent),
            ],
            Self::StopAttacking => &[(AttackTarget, ValuePresent)],
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Swim { .. } => {
                cx.mob.creaking.can_move && goals::float_can_use(cx.me, cx.mob)
            }
            _ => true,
        }
    }

    pub(super) fn start(&mut self, _cx: &mut Cx) {}

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Swim { .. } => goals::float_can_use(cx.me, cx.mob),
            _ => false,
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        if let Self::Swim { chance } = self
            && cx.me.random.next_float() < *chance
        {
            cx.mob.jump.jump = true;
        }
    }

    pub(super) fn stop(&mut self, _cx: &mut Cx) {}

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::StartAttacking => start_attacking(cx),
            Self::SetWalkTargetFromAttackTarget => set_walk_target_from_attack_target(cx),
            Self::MeleeAttack => melee_attack(cx),
            Self::StopAttacking => stop_attacking(cx),
            Self::Swim { .. } => false,
        }
    }
}

/// `StartAttacking`：激活时挑 `NEAREST_VISIBLE_ATTACKABLE_PLAYER` 设成攻击目标。
fn start_attacking(cx: &mut Cx) -> bool {
    if !cx.mob.creaking.active {
        return false;
    }
    let Some(uid) = cx
        .memories
        .entity(MemoryType::NearestVisibleAttackablePlayer)
    else {
        return false;
    };
    cx.memories.set(AttackTarget, MemoryValue::Entity(uid));
    true
}

/// `SetWalkTargetFromAttackTargetIfTargetOutOfReach`：目标看得见且在 1 格内就清走路
/// 目标，否则朝目标设 `LOOK_TARGET` / `WALK_TARGET`（速度 1.0）。
fn set_walk_target_from_attack_target(cx: &mut Cx) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return true;
    };
    let view = sensor::living(cx, target);
    let in_range = sensor::visible_contains(cx, target)
        && view.is_some_and(|view| view.position.distance_to_sqr(cx.me.position) <= 1.0);
    if in_range {
        cx.memories.erase(WalkTargetMemory);
    } else {
        cx.memories.set(
            LookTarget,
            MemoryValue::Look(PositionTracker::Entity {
                uid: target,
                track_eye: true,
                target_eye: false,
            }),
        );
        cx.memories.set(
            WalkTargetMemory,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::Entity {
                    uid: target,
                    track_eye: false,
                    target_eye: false,
                },
                speed: 1.0,
                close_enough: 0,
            }),
        );
    }
    true
}

/// `MeleeAttack`：`canMove`、近战范围内、目标可见时挥击并 `doHurtTarget`，冷却 40 刻。
fn melee_attack(cx: &mut Cx) -> bool {
    if !cx.mob.creaking.can_move {
        return false;
    }
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    let Some(combat) = goals::attack_candidate(cx.mob, &*cx.ctx, target) else {
        return false;
    };
    if !goals::within_melee_range(cx.me, cx.mob, combat, &*cx.ctx) {
        return false;
    }
    if !sensor::visible_contains(cx, target) {
        return false;
    }
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::Entity {
            uid: target,
            track_eye: true,
            target_eye: false,
        }),
    );
    // `doHurtTarget`：挥击动画 15 刻 + 事件 4，再走 `Monster.doHurtTarget`。
    creaking::swing_and_hurt(cx.me, cx.mob, cx.ctx, target);
    cx.memories.set_with_ttl(AttackCoolingDown, MemoryValue::Bool(true), 40);
    true
}

/// `StopAttackingIfTargetInvalid`：目标不可攻击 / 死了 / 换了世界 / 不再在
/// `NEAREST_VISIBLE_ATTACKABLE_PLAYERS` 里时擦掉攻击目标。
fn stop_attacking(cx: &mut Cx) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return true;
    };
    let Some(view) = sensor::living(cx, target) else {
        cx.memories.erase(AttackTarget);
        return true;
    };
    let reachable = view.alive
        && cx
            .memories
            .entities(MemoryType::NearestVisibleAttackablePlayers)
            .is_some_and(|list| list.contains(&target));
    if !reachable {
        cx.memories.erase(AttackTarget);
    }
    true
}

/// `Creaking.customServerAiStep`：先跑大脑，再 `CreakingAi.updateActivity`。
pub(in crate::entity::mob) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        update_activity(&mut brain, mob.creaking.can_move);
        mob.brain = Some(brain);
    }
}

/// `CreakingAi.updateActivity`：`!canMove` 时回默认活动，否则 `FIGHT` / `IDLE`。
fn update_activity(brain: &mut Brain, can_move: bool) {
    if !can_move {
        brain.use_default_activity();
    } else {
        brain.set_active_activity_to_first_valid(&[Activity::Fight, Activity::Idle]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 大脑结构表：传感器、活动与行为登记和官方字节码一致。
    #[test]
    fn brain_tables_match_the_vanilla_registration() {
        let brain = brain();
        let sensors = brain.sensor_table();
        assert_eq!(sensors.len(), 2);
        assert_eq!(sensors[0].0, "NearestLivingEntitySensor");
        assert_eq!(sensors[1].0, "PlayerSensor");
        let table = brain.behavior_table();
        assert!(table.iter().any(|(_, _, name)| name == "Swim"));
        assert!(table.iter().any(|(_, _, name)| name == "LookAtTargetSink"));
        assert!(table.iter().any(|(_, _, name)| name == "MoveToTargetSink"));
        assert!(table.iter().any(|(_, _, name)| name == "StartAttacking"));
        assert!(
            table
                .iter()
                .any(|(_, _, name)| name == "SetWalkTargetFromAttackTargetIfTargetOutOfReach")
        );
        assert!(table.iter().any(|(_, _, name)| name == "MeleeAttack"));
        assert!(table.iter().any(|(_, _, name)| name == "StopAttackingIfTargetInvalid"));
        let requirements = brain.requirement_table();
        assert!(requirements.iter().any(|(activity, _)| *activity == "fight"));
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
    }
}
