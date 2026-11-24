//! 铁傀儡：官方 `AbstractGolem` → `IronGolem`，实现 `NeutralMob`。
//!
//! 村庄相关（村民召唤、送花、回村、村庄巡逻）这一期不做；对应的目标仍按官方优先级
//! 占位，保持注册顺序。`playerCreated` 的傀儡不主动打玩家，所有傀儡都不打苦力怕。
//!
//! Iron golems: vanilla `AbstractGolem` → `IronGolem`, implementing `NeutralMob`.
//! Village behaviour is out of scope; its goals keep their vanilla registration
//! slots. A player-created golem never targets players, and no golem targets creepers.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{
    entity_types,
    registries::{attribute, entity_type},
};

use super::{
    DamageKind, DamageSource, Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    goals::{CombatTarget, Goal, GoalSelector, TargetType, WrappedGoal, reduced_tick_delay},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    neutral,
};
use crate::{
    InteractionResult,
    entity::{Entity, ctx::EntityCtx},
};

/// `IronGolem.DATA_FLAGS_ID` 的第 0 位：`playerCreated`。
const PLAYER_CREATED_BIT: u8 = 1;

/// `IRON_INGOT_HEAL_AMOUNT`。
const IRON_INGOT_HEAL_AMOUNT: f32 = 25.0;

/// `doHurtTarget` 广播的实体事件 4：客户端的挥手动画。
const ATTACK_EVENT: u8 = 4;

/// 铁傀儡自己的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IronGolem {
    /// `DATA_FLAGS_ID`：现在只有 `playerCreated` 一位。
    pub flags: u8,
    /// `attackAnimationTick`。
    pub attack_animation_tick: i32,
}

impl IronGolem {
    pub const fn is_player_created(self) -> bool {
        self.flags & PLAYER_CREATED_BIT != 0
    }

    pub fn set_player_created(&mut self, value: bool) {
        if value {
            self.flags |= PLAYER_CREATED_BIT;
        } else {
            self.flags &= !PLAYER_CREATED_BIT;
        }
    }
}

/// `EntityTypes.IRON_GOLEM`：尺寸取生成器导出的表，眼高单独导出。
pub fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[entity_type::IRON_GOLEM as usize];
    (
        facts.width,
        facts.height,
        entity_types::IRON_GOLEM_EYE_HEIGHT,
    )
}

/// `IronGolem.createAttributes()`：`Mob.createMobAttributes` 的属性表加上五项覆盖。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, entity_types::IRON_GOLEM_MAX_HEALTH);
    table.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::IRON_GOLEM_MOVEMENT_SPEED,
    );
    table.set_base(
        attribute::KNOCKBACK_RESISTANCE,
        entity_types::IRON_GOLEM_KNOCKBACK_RESISTANCE,
    );
    table.set_base(attribute::STEP_HEIGHT, entity_types::IRON_GOLEM_STEP_HEIGHT);
    if let Some(entry) = table
        .entries
        .iter_mut()
        .find(|entry| entry.id == attribute::ATTACK_DAMAGE)
    {
        entry.base = entity_types::IRON_GOLEM_ATTACK_DAMAGE;
    } else {
        table.entries.push(Attribute::new(
            attribute::ATTACK_DAMAGE,
            entity_types::IRON_GOLEM_ATTACK_DAMAGE,
        ));
    }
    // `AbstractCow.createAttributes` 的表带 `TEMPT_RANGE`，官方傀儡的属性表没有这一项。
    table
        .entries
        .retain(|entry| entry.id != attribute::TEMPT_RANGE);
    table
}

/// `IronGolem.registerGoals`：按官方的调用顺序，不按优先级排。
pub(super) fn goals(_navigation: &mut Navigation) -> GoalSelector {
    let wrap = |priority, goal| WrappedGoal {
        priority,
        running: false,
        goal,
    };
    GoalSelector::from_goals(vec![
        wrap(1, Goal::melee(1.0, true)),
        wrap(
            2,
            Goal::MoveTowardsTarget {
                speed: 0.9,
                within: 32.0,
                wanted: Vec3::default(),
                target: None,
            },
        ),
        wrap(
            2,
            Goal::GolemBackToVillage {
                speed: 0.6,
                interval: 10,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            4,
            Goal::GolemStroll {
                speed: 0.6,
                interval: 240,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(5, Goal::OfferFlower { tick: 0 }),
        wrap(
            7,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            8,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `IronGolem` 的 target 选择器：村庄守卫、被打记仇、愤怒的玩家、敌对生物、全体愤怒复位。
pub(super) fn target_goals() -> GoalSelector {
    let nearest = |kind, interval, must_see| Goal::NearestAttackableTarget {
        kind,
        interval,
        unseen_ticks: 0,
        found: None,
        must_see,
        light_gate: false,
    };
    let wrap = |priority, goal| WrappedGoal {
        priority,
        running: false,
        goal,
    };
    GoalSelector::from_goals(vec![
        wrap(1, Goal::DefendVillageTarget),
        wrap(
            2,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        // `NearestAttackableTargetGoal<Player>(10, true, false, NeutralMob.isAngryAt)`。
        wrap(
            3,
            nearest(TargetType::AngerPlayer, reduced_tick_delay(10), true),
        ),
        // `NearestAttackableTargetGoal<Mob>(5, false, false, Enemy && !Creeper)`。
        wrap(
            3,
            nearest(TargetType::IronGolemPrey, reduced_tick_delay(5), false),
        ),
        wrap(4, Goal::Unavailable("ResetUniversalAngerTargetGoal")),
    ])
}

/// `IronGolem.aiStep` 的结尾：挥手计时和持久愤怒。
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    mob.iron_golem.attack_animation_tick = mob.iron_golem.attack_animation_tick.saturating_sub(1);
    neutral::update(me, mob, ctx, true, anger_time);
}

/// `PERSISTENT_ANGER_TIME = TimeUtil.rangeOfSeconds(20, 39)`：`UniformInt(400, 780)`。
fn anger_time(me: &mut Entity) -> i64 {
    i64::from(me.random.next_int_bounded(381) + 400)
}

/// `Crackiness.GOLEM.byFraction(health / maxHealth)`：四档裂纹，档位边界由生成器导出。
pub fn crackiness(mob: &Mob) -> u8 {
    let max = mob.attributes.value(attribute::MAX_HEALTH);
    let fraction = if max > 0.0 {
        f64::from(mob.health) / max
    } else {
        0.0
    } as f32;
    if fraction < entity_types::GOLEM_CRACKINESS_HIGH {
        3
    } else if fraction < entity_types::GOLEM_CRACKINESS_MEDIUM {
        2
    } else if fraction < entity_types::GOLEM_CRACKINESS_LOW {
        1
    } else {
        0
    }
}

/// `IronGolem.canAttack`：玩家造的不打玩家，谁都不打苦力怕。
pub fn can_attack(mob: &Mob, target: MobType, target_is_player: bool) -> bool {
    if mob.iron_golem.is_player_created() && target_is_player {
        return false;
    }
    target != MobType::Creeper
}

/// `IronGolem.doHurtTarget`：伤害 `floor(attackDamage)/2 + nextInt(floor(attackDamage))`，
/// 命中后把目标往上抛 `0.4 * (1 - knockbackResistance)`。
pub(super) fn do_hurt_target(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
) -> bool {
    mob.iron_golem.attack_animation_tick = 10;
    mob.events.push(MobEvent::EntityEvent {
        event: ATTACK_EVENT,
    });
    let amount = attack_amount(me, mob);
    let mut source = DamageSource::environment(DamageKind::MobAttack);
    source.attacker = Some(me.id);
    source.direct = Some(me.id);
    source.attacker_uuid = Some(mob.uuid);
    source.position = Some(me.position);
    let hit = if target.is_player {
        let attacker = me.clone().split().public;
        ctx.hurt_player(target.uid, source, amount, me, &attacker)
    } else {
        ctx.emit(
            crate::entity::ctx::Command::HurtMob {
                target: target.uid,
                source,
                amount,
                on_hit: None,
            },
            me,
        );
        true
    };
    if hit {
        let resistance = target.knockback_resistance;
        let lift = 0.400_000_005_960_464_5_f64 * (1.0 - resistance);
        ctx.emit(
            crate::entity::ctx::Command::LiftMob {
                target: target.uid,
                lift,
            },
            me,
        );
    }
    living::play_sound(me, mob, "minecraft:entity.iron_golem.attack", 1.0, 1.0);
    hit
}

/// `IronGolem.doHurtTarget` 的伤害：`floor(damage)/2 + nextInt(floor(damage))`。
///
/// The damage of `IronGolem.doHurtTarget`.
pub fn attack_amount(me: &mut Entity, mob: &Mob) -> f32 {
    let attack = mob.attributes.value(attribute::ATTACK_DAMAGE) as f32;
    let whole = attack as i32;
    if whole > 0 {
        attack / 2.0 + me.random.next_int_bounded(whole) as f32
    } else {
        attack
    }
}

/// `IronGolem.hurtServer`：裂纹档位变了就多播一声。
pub(super) fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    let before = crackiness(mob);
    let hurt = living::living_hurt_server(me, mob, ctx, source, amount);
    if hurt && crackiness(mob) != before {
        living::play_sound(me, mob, "minecraft:entity.iron_golem.damage", 1.0, 1.0);
    }
    hurt
}

/// `IronGolem.mobInteract`：铁锭回 25 血，血没变返回 PASS，否则取两次 `nextFloat` 定音高。
pub(super) fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if req.held.item != mc_bamboo_data::items::IRON_INGOT {
        return MobInteraction::pass();
    }
    let before = mob.health;
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    mob.health = (mob.health + IRON_INGOT_HEAL_AMOUNT).min(max);
    if mob.health == before {
        return MobInteraction::pass();
    }
    let pitch = 1.0 + (me.random.next_float() - me.random.next_float()) * 0.2_f32;
    living::play_sound(me, mob, "minecraft:entity.iron_golem.repair", 1.0, pitch);
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !req.creative,
        ..MobInteraction::pass()
    }
}

/// `IronGolem.addAdditionalSaveData` / `readAdditionalSaveData`，包含持久愤怒。
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, mc_bamboo_nbt::Tag)>) {
    out.push((
        "PlayerCreated".to_owned(),
        mc_bamboo_nbt::Tag::Byte(i8::from(mob.iron_golem.is_player_created())),
    ));
    neutral::write_nbt(mob, out);
}

pub(super) fn read_nbt(mob: &mut Mob, tag: &mc_bamboo_nbt::Tag) {
    let created = crate::archive::tag::bool_or(tag, "PlayerCreated", false);
    mob.iron_golem.set_player_created(created);
    neutral::read_nbt(mob, tag);
}

/// `IronGolem.doPush`：碰到敌对生物（苦力怕除外）时 1/20 概率把它设成目标。
pub(super) fn do_push(me: &mut Entity, mob: &mut Mob, public: &super::MobPublic, uid: i32) {
    if public.mob_type != MobType::Creeper
        && public.mob_type.is_enemy()
        && me.random.next_int_bounded(20) == 0
    {
        mob.target = Some(uid);
    }
}

/// `spawnGolemInWorld` 里 `setPlayerCreated(true)` 的落地，方块层调用。
pub fn mark_player_created(mob: &mut Mob) {
    mob.iron_golem.set_player_created(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_the_generated_facts() {
        let table = attributes();
        assert_eq!(
            table.value(attribute::MAX_HEALTH),
            entity_types::IRON_GOLEM_MAX_HEALTH
        );
        assert_eq!(
            table.value(attribute::ATTACK_DAMAGE),
            entity_types::IRON_GOLEM_ATTACK_DAMAGE
        );
        assert_eq!(table.value(attribute::KNOCKBACK_RESISTANCE), 1.0);
        assert_eq!(table.value(attribute::STEP_HEIGHT), 1.0);
        assert!(table.get(attribute::TEMPT_RANGE).is_none());
    }

    #[test]
    fn crackiness_has_four_official_bands() {
        let mut mob = Mob::new(MobType::IronGolem);
        assert_eq!(crackiness(&mob), 0);
        mob.health = 74.0;
        assert_eq!(crackiness(&mob), 1);
        mob.health = 49.0;
        assert_eq!(crackiness(&mob), 2);
        mob.health = 24.0;
        assert_eq!(crackiness(&mob), 3);
        // 阈值是严格小于，恰好 75% 还是没有裂纹。
        mob.health = 75.0;
        assert_eq!(crackiness(&mob), 0);
    }

    #[test]
    fn player_created_golems_refuse_players_and_everyone_refuses_creepers() {
        let mut mob = Mob::new(MobType::IronGolem);
        assert!(can_attack(&mob, MobType::Pig, true));
        assert!(!can_attack(&mob, MobType::Creeper, false));
        mark_player_created(&mut mob);
        assert!(!can_attack(&mob, MobType::Pig, true));
    }
}
