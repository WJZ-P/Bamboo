//! 北极熊：站起来警告再扑咬的近战、幼崽挨打时叫大熊、带崽的大熊主动打玩家、持久愤怒。
//!
//! 目标的调度在 [`goals`](super::goals) 里，这里放北极熊覆写的那几段和它自己的状态。
//! 成年熊会选狐狸为目标；`universalAnger` 规则没有建模，`ResetUniversalAngerTargetGoal` 只占位置。
//!
//! Polar bears: the stand-up warning before a bite, cubs calling adults when hurt, adults
//! with a cub nearby attacking players, and persistent anger. Scheduling lives in
//! [`goals`](super::goals); this module holds the polar bear overrides and its own state.
//! Adults target foxes. The `universalAnger` rule is not modelled, so
//! `ResetUniversalAngerTargetGoal` is a placeholder.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{entity_types, registries::attribute, tags::block_tags};
use mc_bamboo_nbt::Tag;

use super::{
    Mob, MobType,
    attributes::{Attribute, Attributes},
    goals::{self, CombatTarget, Goal, GoalSelector, TargetType, WrappedGoal},
    living,
    navigation::Navigation,
    neutral,
};
use crate::{
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{Command, EntityCtx},
    },
    shape::Aabb,
    world::World,
};

/// `PolarBear.DATA_STANDING_ID` 和 `warningSoundTicks`。
///
/// `PolarBear.DATA_STANDING_ID` and `warningSoundTicks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PolarBear {
    pub standing: bool,
    pub warning_sound_ticks: i32,
}

/// 成年取 `EntityTypes.POLAR_BEAR`（眼高是缺省的 0.85 倍身高），幼年是
/// `PolarBear.BABY_DIMENSIONS`。站立时的加高只在客户端。
///
/// Adults use `EntityTypes.POLAR_BEAR` (default eye height, 0.85 of the height); babies
/// use `PolarBear.BABY_DIMENSIONS`. The standing stretch is client-side only.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.7, 0.7, 0.34375)
    } else {
        let facts = &entity_types::FACTS[MobType::PolarBear.entity_type() as usize];
        (facts.width, facts.height, facts.height * 0.85)
    }
}

/// `PolarBear.createAttributes`：动物属性上生命 30、跟随距离 20、速度 0.25、攻击 6。
///
/// `PolarBear.createAttributes`: animal attributes with health 30, follow range 20,
/// speed 0.25 and attack damage 6.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 30.0);
    table.set_base(attribute::FOLLOW_RANGE, 20.0);
    table.set_base(attribute::MOVEMENT_SPEED, 0.25);
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 6.0));
    table
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `PolarBear.registerGoals` 的 `goalSelector`，按注册顺序。`RandomStrollGoal` 不是避水的
/// 那个子类，用负的 `probability` 表示（见 [`Goal::Stroll`]）。
///
/// The `goalSelector` of `PolarBear.registerGoals` in registration order. The plain
/// `RandomStrollGoal` (not the water-avoiding subclass) is marked by a negative
/// `probability` (see [`Goal::Stroll`]).
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(0, Goal::Float),
        wrap(
            1,
            goals::melee_attack(MELEE_SPEED, true, "PolarBearMeleeAttackGoal"),
        ),
        wrap(
            1,
            Goal::Panic {
                speed: 2.0,
                target: Vec3::default(),
            },
        ),
        wrap(
            4,
            Goal::FollowParent {
                speed: 1.25,
                parent: None,
                time_to_recalc_path: 0,
            },
        ),
        wrap(
            5,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: goals::PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            6,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            7,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `PolarBearMeleeAttackGoal` 的速度；[`Goal::name`] 也靠它认出这个子类。
///
/// The `PolarBearMeleeAttackGoal` speed; [`Goal::name`] recognises the subclass by it.
pub(super) const MELEE_SPEED: f64 = 1.25;

/// `PolarBear.registerGoals` 的 `targetSelector`。带崽打玩家的间隔是 20（折半成 10），
/// 另外两个 `NearestAttackableTargetGoal` 是 10（折半成 5）。
///
/// The `targetSelector` of `PolarBear.registerGoals`. The cub-guarding goal uses interval
/// 20 (halved to 10); the other two `NearestAttackableTargetGoal`s use 10 (halved to 5).
pub(super) fn target_goals() -> GoalSelector {
    let nearest = |kind, interval| Goal::NearestAttackableTarget {
        kind,
        interval,
        unseen_ticks: 0,
        found: None,
        // 三条目标都是 `mustSee=true`（狐狸目标另有未建模的 `mustReach=true`）。
        // All three goals have `mustSee=true` (the fox goal also has an unmodelled
        // `mustReach=true`).
        must_see: true,
        light_gate: false,
    };
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::HurtByTarget {
                timestamp: 0,
                unseen_ticks: 0,
            },
        ),
        wrap(2, nearest(TargetType::GuardedPlayer, 10)),
        wrap(3, nearest(TargetType::AngerPlayer, 5)),
        wrap(4, nearest(TargetType::Fox, 5)),
        wrap(5, Goal::Unavailable("ResetUniversalAngerTargetGoal")),
    ])
}

/// `PolarBearMeleeAttackGoal.checkAndPerformAttack`：够得着就咬并放下身子；离目标不到
/// 「目标宽度 + 3」格时，冷却转完先重置，剩 10 刻以内就站起来、吼一声；再远就一直重置冷却。
///
/// `PolarBearMeleeAttackGoal.checkAndPerformAttack`: bite when in reach and drop down;
/// within "target width + 3" blocks reset an elapsed cooldown, then stand up and growl
/// during its last 10 ticks; farther away keep resetting the cooldown.
pub(super) fn check_and_perform_attack(
    ticks_until_next_attack: &mut i32,
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
) {
    const ATTACK_INTERVAL: i32 = 20;
    let time_to_attack = *ticks_until_next_attack <= 0;
    if time_to_attack
        && goals::within_melee_range(me, mob, target)
        && living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position())
    {
        *ticks_until_next_attack = ATTACK_INTERVAL;
        living::do_hurt_target(me, mob, ctx, target.uid);
        mob.polar_bear.standing = false;
        return;
    }
    let reach = target.width + 3.0_f32;
    if me.position.distance_to_sqr(target.position) >= f64::from(reach * reach) {
        *ticks_until_next_attack = ATTACK_INTERVAL;
        mob.polar_bear.standing = false;
        return;
    }
    if time_to_attack {
        mob.polar_bear.standing = false;
        *ticks_until_next_attack = ATTACK_INTERVAL;
    }
    if *ticks_until_next_attack <= 10 {
        mob.polar_bear.standing = true;
        play_warning_sound(me, mob);
    }
}

/// `PolarBear.playWarningSound`：40 刻内只吼一次，`makeSound` 取两个随机数算音高。
///
/// `PolarBear.playWarningSound`: one growl per 40 ticks; `makeSound` draws the pitch.
fn play_warning_sound(me: &mut Entity, mob: &mut Mob) {
    if mob.polar_bear.warning_sound_ticks > 0 {
        return;
    }
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(
        me,
        mob,
        "minecraft:entity.polar_bear.warning",
        mob.mob_type.sound_volume(),
        pitch,
    );
    mob.polar_bear.warning_sound_ticks = 40;
}

/// `PolarBearAttackPlayersGoal.canUse` 的后半段：自己周围 `inflate(8, 4, 8)` 里有幼崽。
///
/// The tail of `PolarBearAttackPlayersGoal.canUse`: a cub within `inflate(8, 4, 8)`.
pub(super) fn cub_nearby(me: &Entity, ctx: &dyn EntityCtx) -> bool {
    let mut found = false;
    ctx.for_each_other_in(me.bounding_box().inflate(8.0, 4.0, 8.0), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type == MobType::PolarBear
            && public.baby
        {
            found = true;
        }
    });
    found
}

/// `PolarBearHurtByTargetGoal.start` 里幼崽的那一段：`alertOthers()` 只叫成年北极熊
/// （`alertOther` 的覆写），搜索盒是脚下的单位方块 `inflate(跟随距离, 10, 跟随距离)`。
///
/// The cub part of `PolarBearHurtByTargetGoal.start`: `alertOthers()` reaches adult polar
/// bears only (the `alertOther` override) inside the unit cube at the feet inflated by
/// (follow range, 10, follow range).
pub(super) fn alert_adults(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let Some(enemy) = mob.last_hurt_by_mob else {
        return;
    };
    let within = mob.attributes.value(attribute::FOLLOW_RANGE);
    let at = me.position;
    let area = Aabb::new(at.x, at.y, at.z, at.x + 1.0, at.y + 1.0, at.z + 1.0)
        .inflate(within, 10.0, within);
    let mut others = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type == MobType::PolarBear
            && !public.baby
        {
            others.push(other.uid);
        }
    });
    if !others.is_empty() {
        ctx.emit(Command::AlertMobs { others, enemy }, me);
    }
}

/// 落地 [`Command::AlertMobs`]：`HurtByTargetGoal.alertOthers` 里对每个同伴的
/// `other.getTarget() == null` 判断和 `alertOther` 的 `setTarget(hurtByMob)`。
///
/// Applies [`Command::AlertMobs`]: the `other.getTarget() == null` test of
/// `HurtByTargetGoal.alertOthers` and `alertOther`'s `setTarget(hurtByMob)`.
pub(crate) fn apply_alert(world: &mut World, others: &[EntityUid], enemy: EntityUid) {
    if !valid_enemy(world, enemy) {
        return;
    }
    for &other in others {
        let mut current = None;
        world.with_entity_mut(other, &mut |entity| {
            current = entity.kind.mob().and_then(|mob| mob.target);
        });
        if current.is_some_and(|target| valid_enemy(world, target)) {
            continue;
        }
        world.with_entity_mut(other, &mut |entity| {
            if let Some(mob) = entity.kind.mob_mut() {
                mob.target = Some(enemy);
            }
        });
    }
}

/// `Mob.asValidTarget`：创造 / 旁观的玩家、和平难度下的玩家、死了的活物都不算目标。
///
/// `Mob.asValidTarget`: creative or spectator players, players in peaceful and dead
/// things are not targets.
fn valid_enemy(world: &mut World, uid: EntityUid) -> bool {
    if let Some(player) = goals::player_by_uid(&*world, uid) {
        return player.alive
            && !player.creative
            && !player.spectator
            && world.level().difficulty_id() != 0;
    }
    let mut alive = false;
    world.with_entity_mut(uid, &mut |entity| {
        alive = !entity.removed && entity.kind.mob().is_some_and(|mob| !mob.is_dead_or_dying());
    });
    alive
}

/// `PolarBear.tick` 在 `super.tick()` 之后的部分：吼叫冷却减一，然后
/// `updatePersistentAnger(level, true)`。
///
/// The part of `PolarBear.tick` after `super.tick()`: the growl cooldown, then
/// `updatePersistentAnger(level, true)`.
pub(super) fn tick_end(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.polar_bear.warning_sound_ticks > 0 {
        mob.polar_bear.warning_sound_ticks -= 1;
    }
    neutral::update(me, mob, ctx, true, anger_time);
}

/// `PERSISTENT_ANGER_TIME = TimeUtil.rangeOfSeconds(20, 39)`：`UniformInt(400, 780)` 的一次取样。
///
/// `PERSISTENT_ANGER_TIME = TimeUtil.rangeOfSeconds(20, 39)`: one `UniformInt(400, 780)` sample.
fn anger_time(me: &mut Entity) -> i64 {
    i64::from(me.random.next_int_bounded(780 - 400 + 1) + 400)
}

/// `PolarBear.checkPolarBearSpawnRules`：群系在 `polar_bears_spawn_on_alternate_blocks` 里时
/// 看亮度和脚下的 `polar_bears_spawnable_on_alternate`，否则是普通动物的规则。
///
/// `PolarBear.checkPolarBearSpawnRules`: in `polar_bears_spawn_on_alternate_blocks` biomes
/// the light and the `polar_bears_spawnable_on_alternate` block below decide; elsewhere the
/// plain animal rule does.
pub(super) fn spawn_rules(world: &World, pos: BlockPos) -> bool {
    let biome = world.biome_id(pos);
    if !super::spawn::biome_tag_contains("minecraft:polar_bears_spawn_on_alternate_blocks", biome) {
        return super::natural::animal_spawn_rules(world, pos);
    }
    world.raw_brightness(pos) > 8
        && world
            .block_state(pos.below())
            .is_tagged(block_tags::POLAR_BEARS_SPAWNABLE_ON_ALTERNATE)
}

/// `PolarBear.getAmbientSound`：幼崽有自己的叫声。
///
/// `PolarBear.getAmbientSound`: cubs have their own sound.
pub(super) const fn ambient_sound(baby: bool) -> &'static str {
    if baby {
        "minecraft:entity.polar_bear.ambient_baby"
    } else {
        "minecraft:entity.polar_bear.ambient"
    }
}

/// `PolarBear.addAdditionalSaveData`：只多写持久愤怒。
///
/// `PolarBear.addAdditionalSaveData`: only the persistent anger is added.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    neutral::write_nbt(mob, out);
}

/// `PolarBear.readAdditionalSaveData`。
///
/// `PolarBear.readAdditionalSaveData`.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    neutral::read_nbt(mob, tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_and_goals_match_the_official_registration() {
        let bear = Mob::new(MobType::PolarBear);
        assert_eq!(bear.attributes.value(attribute::MAX_HEALTH), 30.0);
        assert_eq!(bear.attributes.value(attribute::FOLLOW_RANGE), 20.0);
        assert_eq!(bear.attributes.value(attribute::MOVEMENT_SPEED), 0.25);
        assert_eq!(bear.attributes.value(attribute::ATTACK_DAMAGE), 6.0);
        assert_eq!(bear.health, 30.0);
        let priorities: Vec<i32> = bear.goals.goals.iter().map(|goal| goal.priority).collect();
        assert_eq!(priorities, [0, 1, 1, 4, 5, 6, 7]);
        let names: Vec<&str> = bear
            .goals
            .goals
            .iter()
            .map(|goal| goal.goal.name())
            .collect();
        assert_eq!(names[1], "PolarBearMeleeAttackGoal");
        assert_eq!(names[4], "RandomStrollGoal");
        let targets: Vec<i32> = bear
            .target_goals
            .goals
            .iter()
            .map(|goal| goal.priority)
            .collect();
        assert_eq!(targets, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn cub_dimensions_follow_the_baby_table() {
        assert_eq!(MobType::PolarBear.dimensions(true), (0.7, 0.7, 0.34375));
        let (width, height, eye) = MobType::PolarBear.dimensions(false);
        assert_eq!((width, height), (1.4, 1.4));
        assert_eq!(eye, 1.4_f32 * 0.85);
    }

    #[test]
    fn anger_time_stays_in_the_official_range() {
        let mut entity = Entity::by_mob_type(MobType::PolarBear, Vec3::default());
        for _ in 0..200 {
            let ticks = anger_time(&mut entity);
            assert!((400..=780).contains(&ticks));
        }
    }

    #[test]
    fn the_growl_waits_for_its_cooldown() {
        let mut entity = Entity::by_mob_type(MobType::PolarBear, Vec3::default());
        let mut mob = Mob::new(MobType::PolarBear);
        play_warning_sound(&mut entity, &mut mob);
        play_warning_sound(&mut entity, &mut mob);
        assert_eq!(mob.events.len(), 1);
        assert_eq!(mob.polar_bear.warning_sound_ticks, 40);
    }
}
