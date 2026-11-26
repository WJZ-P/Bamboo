//! 劫掠兽：官方 `Raider` → `Ravager`。100 血、0.3 移速、0.75 击退抗性、12 攻击、
//! 1.5 攻击击退、32 跟随距离、1 格台阶高度；三个同步计时字段 `AttackTick` /
//! `StunTick` / `RoarTick` 存档。
//!
//! 咆哮、撞击、踩坏树叶 / 作物和骑手生成属于袭击波次交互，先按官方注册顺序
//! 占位；属性、计时字段和存档按 26.3 字节码。
//!
//! Ravager: vanilla `Raider` → `Ravager`. 100 health, 0.3 speed, 0.75 knockback
//! resistance, 12 attack damage, 1.5 attack knockback, 32 follow range and a
//! 1-block step height; the three synced timers `AttackTick` / `StunTick` /
//! `RoarTick` are saved. Roaring, charging, breaking leaves / crops and riders
//! are raid-wave interactions and keep their registration slots for now.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{BlockAncestor, registries::attribute};
use mc_bamboo_nbt::Tag;

use super::{
    DamageKind, DamageSource, Mob, MobEvent,
    attributes::Attributes,
    goals::{self, Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
    raider::RaiderGoal,
};
use crate::{
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{Command, EntityCtx},
    },
    random::RandomSource,
};

/// `Ravager` 的三个同步计时字段，按官方存档键 `AttackTick` / `StunTick` / `RoarTick`。
///
/// Ravager's three synced timers, saved as `AttackTick` / `StunTick` / `RoarTick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RavagerState {
    pub attack_tick: i32,
    pub stunned_tick: i32,
    pub roar_tick: i32,
}

/// `Ravager` 构造函数最后一句 `setPathfindingMalus(PathType.LEAVES, 0.0F)`：
/// 劫掠兽能直接穿过树叶寻路（它会踩坏树叶）。怪物缺省的树叶代价是 -1（走不通）。
///
/// The `Ravager` constructor ends with `setPathfindingMalus(PathType.LEAVES, 0.0F)`:
/// ravagers path straight through leaves (which they trample). The monster
/// default leaf malus is -1 (impassable).
pub(super) fn malus() -> super::path::Malus {
    let mut malus = super::path::Malus::monster();
    malus.set(mc_bamboo_data::PathType::Leaves, 0.0);
    malus
}

/// `Ravager.createAttributes()`。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MAX_HEALTH, 100.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    table.set_base(attribute::KNOCKBACK_RESISTANCE, 0.75);
    table.set_base(attribute::ATTACK_DAMAGE, 12.0);
    table.set_base(attribute::ATTACK_KNOCKBACK, f64::from(1.5_f32));
    table.set_base(attribute::FOLLOW_RANGE, 32.0);
    table.set_base(attribute::STEP_HEIGHT, 1.0);
    table
}

/// `Ravager.registerGoals` 的注册顺序：父类四个袭击目标、漂浮、近战（不松目标）、
/// 避水闲逛、看玩家 / 看生物。劫掠兽没有 `HoldGroundAttackGoal`。
///
/// The `Ravager.registerGoals` order: the four superclass raid goals, float,
/// melee (`followingTargetEvenIfNotSeen`), water-avoiding stroll and the two
/// look goals. Ravagers have no `HoldGroundAttackGoal`.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        // `MeleeAttackGoal(mob, 1.0, true)`：看不见目标也继续追。
        // `MeleeAttackGoal(mob, 1.0, true)`: keep following even out of sight.
        (4, goals::melee_attack(1.0, true, "MeleeAttackGoal")),
        // `WaterAvoidingRandomStrollGoal(mob, 0.4)`。
        (
            5,
            Goal::Stroll {
                speed: 0.4,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            6,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            10,
            Goal::Raider(RaiderGoal::LookAtMob {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            }),
        ),
    ]);
    navigation.flags.can_float = true;
    GoalSelector::from_ordered_pairs(goals)
}

/// `Ravager` 的目标选择器：受伤反击（2）、玩家（3）、村民（4，只打成年）、
/// 铁傀儡（4），都 `mustSee=true`。
///
/// The ravager's target selector: hurt by (2), players (3), villagers (4,
/// adults only) and iron golems (4), all with `mustSee=true`.
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (
            2,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        (3, Goal::nearest_target(TargetType::Player)),
        (4, Goal::nearest_target(TargetType::Villager)),
        (4, Goal::nearest_target(TargetType::IronGolem)),
    ])
}

/// `Ravager.finalizeSpawn`：`Raider` 的 `canJoinRaid`、PatrollingMonster 的队长 /
/// 巡逻状态，以及 `Mob` 的初始化，按官方顺序取随机数。
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn RandomSource) {
    entity.with_mob(|_, mob| {
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, false, false);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Ravager.addAdditionalSaveData` 的三个计时字段。
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push(("AttackTick".to_owned(), Tag::Int(mob.ravager.attack_tick)));
    out.push(("StunTick".to_owned(), Tag::Int(mob.ravager.stunned_tick)));
    out.push(("RoarTick".to_owned(), Tag::Int(mob.ravager.roar_tick)));
}

/// `Ravager.readAdditionalSaveData` 的三个计时字段。
pub(super) fn load(tag: &Tag) -> RavagerState {
    use crate::archive::tag;
    RavagerState {
        attack_tick: tag::int_or(tag, "AttackTick", 0),
        stunned_tick: tag::int_or(tag, "StunTick", 0),
        roar_tick: tag::int_or(tag, "RoarTick", 0),
    }
}

/// `Ravager.isImmobile`：`super.isImmobile()` 或三个计时器里还有正数。
///
/// `Ravager.isImmobile`: the superclass check, or any of the three timers active.
pub(super) fn is_immobile(mob: &Mob) -> bool {
    super::equine::is_immobile(mob)
        || mob.ravager.attack_tick > 0
        || mob.ravager.stunned_tick > 0
        || mob.ravager.roar_tick > 0
}

/// `Ravager.aiStep` 在 `super.aiStep()` 之后的那一段：速度往目标值渐进、水平撞墙时
/// 踩坏树叶、三个计时器递减（`roarTick` 到 10 咆哮，`stunnedTick` 到 0 后补一声
/// 咆哮再开 20 刻咆哮窗口）。
///
/// The part of `Ravager.aiStep` after `super.aiStep()`: ease speed to its target,
/// break leaves on horizontal collision, and run the three timers (`roarTick`
/// roars at 10; a `stunnedTick` reaching 0 plays the roar and opens a 20-tick
/// roar window).
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    if is_immobile(mob) {
        mob.attributes.set_base(attribute::MOVEMENT_SPEED, 0.0);
    } else {
        let target = if mob.target.is_some() { 0.35 } else { 0.3 };
        let current = mob.attributes.base(attribute::MOVEMENT_SPEED);
        mob.attributes
            .set_base(attribute::MOVEMENT_SPEED, mth::lerp(0.1, current, target));
    }
    // `horizontalCollision && mobGriefing`：扫自己包围盒 inflate(0.2) 里的树叶，
    // 逐格破坏；一片都没破而且站在地上时跳一下。
    if me.horizontal_collision && ctx.blocks().block_game_rules().mob_griefing {
        let box_ = me.bounding_box().inflate(0.2, 0.2, 0.2);
        let mut broke = false;
        for x in mth::floor(box_.min_x)..=mth::floor(box_.max_x) {
            for y in mth::floor(box_.min_y)..=mth::floor(box_.max_y) {
                for z in mth::floor(box_.min_z)..=mth::floor(box_.max_z) {
                    let pos = BlockPos::new(x, y, z);
                    if ctx
                        .blocks()
                        .block_state(pos)
                        .is_a(BlockAncestor::LeavesBlock)
                    {
                        ctx.emit(Command::DestroyBlock { pos, drop: true }, me);
                        broke = true;
                    }
                }
            }
        }
        if !broke && me.on_ground {
            living::jump_from_ground(me, mob, ctx.blocks());
        }
    }
    if mob.ravager.roar_tick > 0 {
        mob.ravager.roar_tick -= 1;
        if mob.ravager.roar_tick == 10 {
            roar(me, mob, ctx);
        }
    }
    if mob.ravager.attack_tick > 0 {
        mob.ravager.attack_tick -= 1;
    }
    if mob.ravager.stunned_tick > 0 {
        mob.ravager.stunned_tick -= 1;
        if mob.ravager.stunned_tick == 0 {
            living::play_sound(me, mob, "minecraft:entity.ravager.roar", 1.0, 1.0);
            mob.ravager.roar_tick = 20;
        }
    }
}

/// `Ravager.roar`：4 格外（`mobGriefing` 开着时排除盔甲架）的非劫掠兽活体，不是
/// 灾厄村民就打 6 点近战伤害，不是玩家就强撞击退；最后播 69 号实体事件。
///
/// `Ravager.roar`: living non-ravagers within 4 blocks (armour stands excluded
/// under `mobGriefing`) take 6 mob-attack damage unless they are illagers, and
/// strong knockback unless they are players; entity event 69 follows.
fn roar(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    let box_ = me.bounding_box().inflate(4.0, 4.0, 4.0);
    let mut mobs = Vec::new();
    ctx.for_each_other_in(box_, &mut |other| {
        if other.removed || other.uid == me.id {
            return;
        }
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if public.mob_type == super::MobType::Ravager || !public.is_alive() {
            return;
        }
        mobs.push(other.uid);
    });
    let mut source = DamageSource::environment(DamageKind::MobAttack);
    source.attacker = Some(me.id);
    source.direct = Some(me.id);
    source.attacker_uuid = Some(mob.uuid);
    source.attacker_type = Some(mob.mob_type.entity_type());
    source.position = Some(me.position);
    for uid in &mobs {
        let illager = ctx.other(*uid).is_some_and(|public| {
            matches!(
                public.kind,
                KindPublic::Mob(m) if matches!(
                    m.mob_type,
                    super::MobType::Pillager | super::MobType::Vindicator
                )
            )
        });
        if !illager {
            ctx.emit(
                Command::HurtMob {
                    target: *uid,
                    source,
                    amount: 6.0,
                    on_hit: None,
                },
                me,
            );
        }
        strong_knockback(me, mob, ctx, *uid);
    }
    // 玩家只挨 6 点伤害、不被撞退。
    let mut players = Vec::new();
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive && !player.creative && !player.spectator {
            players.push(*player);
        }
    });
    for player in players {
        let attacker = me.clone().split().public;
        ctx.hurt_player(player.uid, source, 6.0, me, &attacker);
    }
    mob.events.push(MobEvent::EntityEvent { event: 69 });
}

/// `Ravager.strongKnockback`：`push(dx / d * 4, 0.2, dz / d * 4)`，`d = max(dx²+dz², 0.001)`。
///
/// `Ravager.strongKnockback`: `push(dx / d * 4, 0.2, dz / d * 4)` with
/// `d = max(dx²+dz², 0.001)`.
fn strong_knockback(me: &mut Entity, _mob: &Mob, ctx: &mut dyn EntityCtx, target: EntityUid) {
    let Some(other) = ctx.other(target) else {
        return;
    };
    let dx = other.position.x - me.position.x;
    let dz = other.position.z - me.position.z;
    let d = (dx * dx + dz * dz).max(0.001);
    ctx.emit(
        Command::AddMobDelta {
            target,
            delta: Vec3::new(dx / d * 4.0, 0.2, dz / d * 4.0),
        },
        me,
    );
}

/// `Ravager.doHurtTarget` 在 `super.doHurtTarget()` 之前的那段：`attackTick = 10`、
/// 4 号实体事件和攻击声。
///
/// The part of `Ravager.doHurtTarget` before `super.doHurtTarget()`:
/// `attackTick = 10`, entity event 4 and the attack sound.
pub(super) fn start_attack(me: &mut Entity, mob: &mut Mob) {
    mob.ravager.attack_tick = 10;
    mob.events.push(MobEvent::EntityEvent { event: 4 });
    living::play_sound(me, mob, "minecraft:entity.ravager.attack", 1.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_nbt::Tag;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 100.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED) as f32, 0.3);
        assert_eq!(table.value(attribute::KNOCKBACK_RESISTANCE), 0.75);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 12.0);
        assert_eq!(table.value(attribute::ATTACK_KNOCKBACK) as f32, 1.5);
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 32.0);
        assert_eq!(table.value(attribute::STEP_HEIGHT), 1.0);
    }

    #[test]
    fn leaves_are_passable() {
        // 官方 `Ravager.<init>` 把树叶代价设成 0；其它怪物保持缺省的 -1。
        let ravager = malus();
        assert_eq!(ravager.get(mc_bamboo_data::PathType::Leaves), 0.0);
        assert_eq!(
            crate::entity::mob::path::Malus::monster().get(mc_bamboo_data::PathType::Leaves),
            -1.0
        );
    }

    #[test]
    fn timers_round_trip() {
        let mut mob = Mob::new(super::super::MobType::Ravager);
        mob.ravager = RavagerState {
            attack_tick: 7,
            stunned_tick: 11,
            roar_tick: 20,
        };
        let mut out = Vec::new();
        save(&mob, &mut out);
        let tag = Tag::Compound(out);
        let loaded = load(&tag);
        assert_eq!(loaded.attack_tick, 7);
        assert_eq!(loaded.stunned_tick, 11);
        assert_eq!(loaded.roar_tick, 20);
    }
}
