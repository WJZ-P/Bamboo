//! 豹猫：信任玩家（`DATA_TRUSTING`）、潜行 / 冲刺姿势与疾跑速度加成、
//! `OcelotAttackGoal` 扑杀鸡和陆地幼年海龟、远离玩家、丛林自然生成。
//!
//! Ocelots: trusting players (`DATA_TRUSTING`), crouching / sprinting poses with the
//! sprinting speed modifier, `OcelotAttackGoal` hunting chickens and baby turtles on land,
//! avoiding untrusted players, and jungle natural spawning.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{
    blocks, entity_types,
    registries::attribute,
    tags::{block_tags, item_tags},
};
use mc_bamboo_nbt::Tag;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    avoid::{AvoidClass, AvoidEntity},
    goals::{Goal, GoalSelector, TargetType, TemptScare, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
};
use crate::{
    InteractionResult,
    entity::{Entity, EntityUid, ctx::EntityCtx},
    random::RandomSource,
    world::World,
};

/// `Ocelot.CROUCH_SPEED_MOD`。
pub(super) const CROUCH_SPEED_MOD: f64 = 0.6;
/// `Ocelot.WALK_SPEED_MOD`。
pub(super) const WALK_SPEED_MOD: f64 = 0.8;
/// `Ocelot.SPRINT_SPEED_MOD`。
pub(super) const SPRINT_SPEED_MOD: f64 = 1.33;

/// 豹猫自己的状态：`DATA_TRUSTING` 和 `customServerAiStep` 设的潜行姿势。
///
/// An ocelot's own state: `DATA_TRUSTING` and the crouching pose set by
/// `customServerAiStep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ocelot {
    /// `Ocelot.DATA_TRUSTING`：喂鱼有 1/3 概率置真，存盘键 `"Trusting"`。
    ///
    /// `Ocelot.DATA_TRUSTING`: 1/3 chance to set on feeding fish; saved as `"Trusting"`.
    pub trusting: bool,
    /// `Pose.CROUCHING`：移动速度修饰符为 `0.6` 时置真，不存盘。
    ///
    /// `Pose.CROUCHING`: true while move control speed modifier is `0.6`; not saved.
    pub crouching: bool,
}

/// 成年取 `EntityTypes.OCELOT`（0.6×0.7，眼高 0.7×0.85），幼年取 `Ocelot.BABY_DIMENSIONS`。
///
/// Adults use `EntityTypes.OCELOT` (0.6×0.7, eye height 0.7×0.85); babies use
/// `Ocelot.BABY_DIMENSIONS`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.3, 0.35, 0.34375)
    } else {
        let facts = &entity_types::FACTS[MobType::Ocelot.entity_type() as usize];
        (facts.width, facts.height, facts.height * 0.85)
    }
}

/// `Ocelot.createAttributes`：`MAX_HEALTH` 10、`MOVEMENT_SPEED` 0.3F、`ATTACK_DAMAGE` 3。
///
/// `Ocelot.createAttributes`: `MAX_HEALTH` 10, `MOVEMENT_SPEED` 0.3F, `ATTACK_DAMAGE` 3.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 10.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 3.0));
    table
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Ocelot.registerGoals` 加上构造函数末尾的 `reassessTrustingGoals`。
/// `OcelotAvoidEntityGoal` 由后者追加在末尾，保持官方 `ObjectLinkedOpenHashSet` 的迭代顺序。
///
/// `Ocelot.registerGoals` followed by the constructor's `reassessTrustingGoals`.
/// `OcelotAvoidEntityGoal` is appended last to preserve vanilla's iteration order.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(
            3,
            Goal::Tempt {
                steering_item: false,
                speed: CROUCH_SPEED_MOD,
                player: None,
                calm_down: 0,
                scare: Some(TemptScare::default()),
            },
        ),
        wrap(7, Goal::LeapAtTarget { yd: 0.3 }),
        wrap(
            8,
            Goal::OcelotAttack {
                target: None,
                attack_time: 0,
            },
        ),
        wrap(
            9,
            Goal::Breed {
                speed: WALK_SPEED_MOD,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(
            10,
            Goal::Stroll {
                speed: WALK_SPEED_MOD,
                interval: 120,
                probability: 1.000_000_1e-5,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            11,
            Goal::LookAtPlayer {
                look_distance: 10.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        avoid_players_goal(),
    ])
}

fn avoid_players_goal() -> WrappedGoal {
    wrap(
        4,
        Goal::Avoid(AvoidEntity::new(
            AvoidClass::Player,
            16.0,
            WALK_SPEED_MOD,
            SPRINT_SPEED_MOD,
        )),
    )
}

/// `Ocelot.registerGoals` 的 `targetSelector`：鸡和陆地上的幼年海龟（`mustSee = false`）。
///
/// The `targetSelector` of `Ocelot.registerGoals`: chickens and baby turtles on land
/// (`mustSee = false`).
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(1, Goal::nearest_target(TargetType::Chicken)),
        wrap(1, Goal::nearest_target(TargetType::BabyTurtleOnLand)),
    ])
}

/// `Ocelot.setTrusting`：写标志并重算逃离玩家目标。
///
/// `Ocelot.setTrusting`: update the flag and reassess the avoid-players goal.
pub fn set_trusting(mob: &mut Mob, trusting: bool) {
    mob.ocelot.trusting = trusting;
    reassess_trusting_goals(mob);
}

/// `Ocelot.reassessTrustingGoals`：先移除旧的躲玩家目标，未信任时再追加到末尾。
///
/// `Ocelot.reassessTrustingGoals`: remove any existing avoid-players goal and re-append
/// it when untrusted.
pub(super) fn reassess_trusting_goals(mob: &mut Mob) {
    mob.goals
        .remove_goals(|goal| matches!(goal, Goal::Avoid(_)));
    if !mob.ocelot.trusting {
        mob.goals.goals.push(avoid_players_goal());
    }
}

/// `Ocelot.customServerAiStep`：按 `MoveControl` 的速度修饰符切换潜行、疾跑或站立。
///
/// `Ocelot.customServerAiStep`: switch between crouching, sprinting and standing from
/// the `MoveControl` speed modifier.
pub(super) fn custom_server_ai_step(mob: &mut Mob) {
    if mob.mover.has_wanted() {
        let speed = mob.mover.speed_modifier;
        if speed == CROUCH_SPEED_MOD {
            mob.ocelot.crouching = true;
            living::set_sprinting(mob, false);
        } else if speed == SPRINT_SPEED_MOD {
            mob.ocelot.crouching = false;
            living::set_sprinting(mob, true);
        } else {
            mob.ocelot.crouching = false;
            living::set_sprinting(mob, false);
        }
    } else {
        mob.ocelot.crouching = false;
        living::set_sprinting(mob, false);
    }
}

/// `Ocelot.mobInteract`：吸引目标正在跑、尚未信任、手里是鱼、距离小于 3 格时试着建立信任；
/// 否则返回 `None` 交给 `Animal.mobInteract`（求爱 / 催熟）。
///
/// `Ocelot.mobInteract`: when the tempt goal is running, the ocelot is untrusted, the
/// held item is ocelot food and the player is within 3 blocks, roll 1/3 to trust;
/// otherwise return `None` to fall through to `Animal.mobInteract`.
pub(super) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> Option<MobInteraction> {
    let tempt_running = mob
        .goals
        .goals
        .iter()
        .any(|wrapped| wrapped.running && matches!(wrapped.goal, Goal::Tempt { .. }));
    if tempt_running
        && !mob.ocelot.trusting
        && !req.held.is_empty()
        && crate::state::item_is_tagged(req.held.item, item_tags::OCELOT_FOOD)
        && req.position.distance_to_sqr(me.position) < 9.0
    {
        if me.random.next_int_bounded(3) == 0 {
            set_trusting(mob, true);
            spawn_trusting_particles(me);
            mob.events.push(MobEvent::EntityEvent { event: 41 });
        } else {
            spawn_trusting_particles(me);
            mob.events.push(MobEvent::EntityEvent { event: 40 });
        }
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            ..MobInteraction::pass()
        });
    }
    None
}

/// `Ocelot.spawnTrustingParticles`：服务端 `ServerLevel.addParticle` 是空实现，但官方
/// 仍在服务端调用这个方法，七次循环各取三个 `nextGaussian` 和三个 `nextDouble`
/// （`getRandomX` / `getRandomY` / `getRandomZ`），这里保住随机源同步。
///
/// `Ocelot.spawnTrustingParticles`: `ServerLevel.addParticle` is a no-op, yet vanilla
/// calls this on the server too, drawing three `nextGaussian` and three `nextDouble`
/// values per particle across seven iterations; preserve the draws to stay in sync.
fn spawn_trusting_particles(me: &mut Entity) {
    for _ in 0..7 {
        let _xa = me.random.next_gaussian();
        let _ya = me.random.next_gaussian();
        let _za = me.random.next_gaussian();
        let _rx = me.random.next_double();
        let _ry = me.random.next_double();
        let _rz = me.random.next_double();
    }
}

/// `Ocelot.removeWhenFarAway`：未信任且存活超过 2400 刻才允许远离消失。
///
/// `Ocelot.removeWhenFarAway`: only untrusted ocelots older than 2400 ticks despawn.
pub(super) const fn removes_when_far_away(mob: &Mob, tick_count: i32) -> bool {
    !mob.ocelot.trusting && tick_count > 2400
}

/// `Ocelot.checkOcelotSpawnRules`：`random.nextInt(3) != 0`。
///
/// `Ocelot.checkOcelotSpawnRules`: `random.nextInt(3) != 0`.
pub(super) fn spawn_rules(random: &dyn RandomSource) -> bool {
    random.next_int(3) != 0
}

/// `Ocelot.checkSpawnObstruction`：无实体遮挡、无液体、不低于海平面、脚下是草方块或树叶。
///
/// `Ocelot.checkSpawnObstruction`: unobstructed, no liquid, at or above sea level, and
/// standing on a grass block or leaves.
pub(super) fn check_spawn_obstruction(world: &World, entity: &Entity, sea_level: i32) -> bool {
    let bounds = entity.bounding_box();
    if !super::natural::is_unobstructed_by_entities(world, bounds)
        || super::natural::contains_any_liquid(world, bounds)
    {
        return false;
    }
    let pos = entity.block_position();
    if pos.y < sea_level {
        return false;
    }
    let below = world.block_state(pos.below());
    below.is(blocks::GRASS_BLOCK) || below.is_tagged(block_tags::LEAVES)
}

/// `Ocelot.finalizeSpawn`：同组从第二只起 100% 是幼崽（`AgeableMobGroupData(1.0F)`）。
///
/// `Ocelot.finalizeSpawn`: later members of a group are always kittens
/// (`AgeableMobGroupData(1.0F)`).
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<super::spawn::SpawnGroup>,
) {
    super::spawn::finalize_ageable(entity, random, group, 1.0);
}

/// `OcelotAttackGoal.tick`：看目标、按距离选速度（0.6 潜行、1.33 扑击、0.8 步行）、
/// 导航并按 20 刻冷却攻击。
///
/// `OcelotAttackGoal.tick`: look at the target, pick speed by distance (0.6 crouch,
/// 1.33 sprint, 0.8 walk), navigate and strike on a 20-tick cooldown.
pub(super) fn attack_tick(
    target: Option<EntityUid>,
    attack_time: &mut i32,
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    let Some((uid, pos, eye)) = target.and_then(|uid| super::goals::entity_look_target(ctx, uid))
    else {
        return;
    };
    mob.look.set_look_at(pos.x, eye.y, pos.z, 30.0, 30.0);
    // 官方在 float 里算 `bbWidth * 2.0F` 的平方，再拓成 double。
    // Vanilla squares `bbWidth * 2.0F` in float before widening to double.
    let reach = me.dimensions.0 * 2.0_f32;
    let melee_radius_sqr = f64::from(reach * reach);
    let dist_sqr = me.position.distance_to_sqr(pos);
    let speed = if dist_sqr > melee_radius_sqr && dist_sqr < 16.0 {
        SPRINT_SPEED_MOD
    } else if dist_sqr < 225.0 {
        CROUCH_SPEED_MOD
    } else {
        WALK_SPEED_MOD
    };
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation
        .move_to_entity(ctx.blocks(), &nav, pos, speed, 1);
    *attack_time = (*attack_time - 1).max(0);
    if dist_sqr <= melee_radius_sqr && *attack_time <= 0 {
        *attack_time = 20;
        living::do_hurt_target(me, mob, ctx, uid);
    }
}

/// `Ocelot.addAdditionalSaveData`：写 `"Trusting"` 布尔。
///
/// `Ocelot.addAdditionalSaveData`: write the `"Trusting"` boolean.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "Trusting".to_owned(),
        Tag::Byte(i8::from(mob.ocelot.trusting)),
    ));
}

/// `Ocelot.readAdditionalSaveData`：读 `"Trusting"`（缺省假）并重算目标。
///
/// `Ocelot.readAdditionalSaveData`: read `"Trusting"` (default false) and reassess goals.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    set_trusting(mob, crate::archive::tag::bool_or(tag, "Trusting", false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_and_attributes_match_vanilla() {
        assert_eq!(dimensions(true), (0.3, 0.35, 0.34375));
        let (w, h, eye) = dimensions(false);
        assert_eq!((w, h), (0.6, 0.7));
        assert!((eye - 0.595).abs() < 1.0e-6);

        let attrs = attributes();
        assert_eq!(attrs.value(attribute::MAX_HEALTH), 10.0);
        assert_eq!(attrs.value(attribute::MOVEMENT_SPEED), f64::from(0.3_f32));
        assert_eq!(attrs.value(attribute::ATTACK_DAMAGE), 3.0);
    }

    #[test]
    fn trusting_removes_and_restores_avoid_players_goal() {
        let mut mob = Mob::new(MobType::Ocelot);
        assert!(
            mob.goals
                .goals
                .iter()
                .any(|w| matches!(w.goal, Goal::Avoid(_)))
        );
        set_trusting(&mut mob, true);
        assert!(mob.ocelot.trusting);
        assert!(
            !mob.goals
                .goals
                .iter()
                .any(|w| matches!(w.goal, Goal::Avoid(_)))
        );
        set_trusting(&mut mob, false);
        assert!(!mob.ocelot.trusting);
        assert!(matches!(
            mob.goals.goals.last().map(|w| &w.goal),
            Some(Goal::Avoid(_))
        ));
    }

    #[test]
    fn save_data_round_trips_trusting() {
        let mut mob = Mob::new(MobType::Ocelot);
        set_trusting(&mut mob, true);
        let mut entries = Vec::new();
        write_nbt(&mob, &mut entries);
        let mut loaded = Mob::new(MobType::Ocelot);
        read_nbt(&mut loaded, &Tag::Compound(entries));
        assert!(loaded.ocelot.trusting);
        assert!(
            !loaded
                .goals
                .goals
                .iter()
                .any(|w| matches!(w.goal, Goal::Avoid(_)))
        );
    }

    #[test]
    fn custom_server_ai_step_toggles_crouch_and_sprint_modifier() {
        let mut mob = Mob::new(MobType::Ocelot);
        mob.mover
            .set_wanted_position(1.0, 0.0, 0.0, CROUCH_SPEED_MOD);
        custom_server_ai_step(&mut mob);
        assert!(mob.ocelot.crouching);
        assert!(!mob.sprinting);

        mob.mover
            .set_wanted_position(1.0, 0.0, 0.0, SPRINT_SPEED_MOD);
        custom_server_ai_step(&mut mob);
        assert!(!mob.ocelot.crouching);
        assert!(mob.sprinting);
        let expected = f64::from(0.3_f32) * (1.0 + f64::from(0.3_f32));
        assert!((mob.attributes.value(attribute::MOVEMENT_SPEED) - expected).abs() < 1.0e-12);

        mob.mover.operation = super::super::control::Operation::Wait;
        custom_server_ai_step(&mut mob);
        assert!(!mob.ocelot.crouching);
        assert!(!mob.sprinting);
        assert_eq!(
            mob.attributes.value(attribute::MOVEMENT_SPEED),
            f64::from(0.3_f32)
        );
    }
}
