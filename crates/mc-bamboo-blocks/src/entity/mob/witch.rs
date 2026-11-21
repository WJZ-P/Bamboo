//! 女巫：官方 `Raider` → `Witch`。Bamboo 没有袭击系统，所以 `Raider` 的四个目标和
//! 「治疗袭击者」目标只保留注册顺序和官方那一次 `nextBoolean` 取数。
//!
//! 自己的行为：主手拿药水喝（水下呼吸 / 抗火 / 治疗 / 速度），喝的时候移速 -0.25；
//! 每 60 刻朝目标扔一瓶喷溅药水（伤害 / 缓慢 / 中毒 / 虚弱）；火焰和魔法伤害只吃 15%。
//!
//! Witch: vanilla `Raider` → `Witch`. Bamboo has no raids, so the four `Raider` goals and
//! the heal-raiders goal only keep their registration order and vanilla's `nextBoolean`
//! draw. Its own behaviour: drink a potion (water breathing, fire resistance, healing,
//! swiftness) with a -0.25 speed modifier while drinking; throw a splash potion at the
//! target every 60 ticks; fire and magic damage is cut to 15%.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{
    registries::{attribute, mob_effect, potion},
    tags::fluid_tags,
};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    attributes::Attributes,
    effects::EffectInstance,
    goals::{Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
};
use crate::entity::{Entity, ctx::EntityCtx};

/// `Witch.SPEED_MODIFIER_DRINKING_ID`：喝药时移速 -0.25。
///
/// `Witch.SPEED_MODIFIER_DRINKING_ID`: movement speed -0.25 while drinking.
const DRINKING_SPEED: &str = "minecraft:drinking";

/// 一瓶药水的使用时长（`Items.POTION` 的 `useDuration`）。
///
/// The use duration of a potion.
const POTION_USE_DURATION: i32 = 32;

/// 女巫自己的状态：`usingTime`。喝不喝药另有一个同步布尔。
///
/// The witch's own state: `usingTime`. Whether it is drinking is a separate synced boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WitchState {
    pub using_time: i32,
}

/// `Witch.createAttributes()`：`Monster` 的缺省加 26 血、0.25 移速。
///
/// `Witch.createAttributes()`: the monster defaults plus 26 health and 0.25 speed.
pub(super) fn attributes() -> Attributes {
    use mc_bamboo_data::registries::attribute_defaults as defaults;
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MAX_HEALTH, 26.0);
    table.set_base(attribute::MOVEMENT_SPEED, 0.25);
    // `Mob.createMobAttributes` 的跟随距离是 16；`attribute_defaults` 里的 32 是属性自己的
    // 缺省值，不是生物注册的值。
    //
    // `Mob.createMobAttributes` registers a follow range of 16; the 32 in
    // `attribute_defaults` is the attribute's own default, not the mob's.
    table.set_base(attribute::FOLLOW_RANGE, 16.0);
    table.set_base(attribute::ATTACK_DAMAGE, defaults::ATTACK_DAMAGE);
    table
}

/// `Witch.registerGoals`：`Raider` 的四个目标按注册顺序占位，然后是漂浮、远程、
/// 闲逛、看玩家、随便看看。
///
/// `Witch.registerGoals`: the four `Raider` goals keep their slots, then float, ranged,
/// stroll, look at player and random look.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let goals = vec![
        (1, Goal::Unavailable("ObtainRaidLeaderBannerGoal")),
        (3, Goal::Unavailable("PathfindToRaidGoal")),
        (4, Goal::Unavailable("RaiderMoveThroughVillageGoal")),
        (5, Goal::Unavailable("RaiderCelebration")),
        (1, Goal::Float),
        (
            2,
            Goal::WitchRangedAttack {
                target: None,
                attack_time: -1,
                see_time: 0,
            },
        ),
        (
            2,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            3,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            3,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ];
    let _ = navigation;
    GoalSelector::from_goals(goals)
}

/// `Witch.registerGoals` 的 `targetSelector`：受伤反击、治疗袭击者（占位）、打玩家。
///
/// The `targetSelector` of `Witch.registerGoals`: hurt by, heal raiders (a slot), attack
/// players.
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_goals(vec![
        (1, Goal::HurtByTarget { timestamp: -1 }),
        (2, Goal::WitchHealRaiders { cooldown: 0 }),
        (
            3,
            Goal::NearestAttackableTarget {
                kind: TargetType::Player,
                interval: super::goals::reduced_tick_delay(10),
                unseen_ticks: 0,
                found: None,
            },
        ),
    ])
}

/// `Witch.aiStep` 里 `super.aiStep()` 之前的那一段：治疗袭击者的冷却、喝药、偶尔冒烟。
///
/// The part of `Witch.aiStep` before `super.aiStep()`: the heal cooldown, drinking and the
/// occasional smoke event.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.no_ai || !living::is_alive(me, mob) {
        return;
    }
    // `healRaidersGoal.decrementCooldown()`，冷却到 0 之后允许攻击玩家。没有袭击时
    // 这个冷却从 0 开始一直减，攻击玩家始终可用。
    if let Some(Goal::WitchHealRaiders { cooldown }) = mob
        .target_goals
        .goals
        .iter_mut()
        .find(|wrapped| matches!(wrapped.goal, Goal::WitchHealRaiders { .. }))
        .map(|wrapped| &mut wrapped.goal)
    {
        *cooldown -= 1;
    }
    if mob.witch.using_time > 0 || mob.witch_drinking {
        drink_tick(mob);
    } else if let Some(potion) = choose_potion(me, mob, ctx) {
        start_drinking(me, mob, potion);
    }
    if me.random.next_float() < 7.5e-4 {
        mob.events.push(super::MobEvent::EntityEvent { event: 15 });
    }
}

/// `Witch.aiStep` 的喝药分支：`usingTime--`，到点把主手的药水效果吃下去、清空主手、
/// 记一次喝水游戏事件、去掉减速。
///
/// The drinking branch of `Witch.aiStep`: tick `usingTime` down, then apply the potion's
/// effects, clear the hand, record the drink game event and drop the speed modifier.
fn drink_tick(mob: &mut Mob) {
    let old = mob.witch.using_time;
    mob.witch.using_time = old - 1;
    if old > 0 {
        return;
    }
    mob.witch_drinking = false;
    let stack = mob.equipment.get(EquipmentSlot::MainHand).clone();
    mob.equipment
        .set(EquipmentSlot::MainHand, ItemStack::empty());
    if stack.item == mc_bamboo_data::items::POTION {
        apply_potion(mob, &stack);
    }
    // `gameEvent(GameEvent.DRINK)`：没有监听方，只保留语义。
    // `gameEvent(GameEvent.DRINK)`: nothing listens, so this is only semantic.
    if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
        speed.remove_modifier(DRINKING_SPEED);
    }
}

/// `PotionContents.forEachEffect(this::addEffect, durationScale)`：瞬时效果立刻结算，
/// 其余挂上去。
///
/// `PotionContents.forEachEffect(this::addEffect, durationScale)`: instantaneous effects
/// resolve at once, the rest are added.
fn apply_potion(mob: &mut Mob, stack: &ItemStack) {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::PotionContents {
        potion: Some(potion),
        ..
    }) = defaults::resolve(stack.item, &patch, kinds::POTION_CONTENTS)
    else {
        return;
    };
    let Some(effects) = mc_bamboo_data::potions::effects(*potion) else {
        return;
    };
    for &(effect, duration, amplifier) in effects {
        add_or_apply(mob, effect, duration, amplifier);
    }
}

/// `MobEffect.isInstantaneous()` 的效果立刻结算，其余按 `EffectInstance` 挂上。
///
/// An instantaneous effect resolves now; anything else is added as an `EffectInstance`.
fn add_or_apply(mob: &mut Mob, effect: i32, duration: i32, amplifier: i32) {
    if effect == mob_effect::INSTANT_HEALTH {
        let amount = 4.0 * (amplifier as f32 + 1.0);
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        mob.health = (mob.health + amount).min(max);
        return;
    }
    if effect == mob_effect::INSTANT_DAMAGE {
        // 女巫不会给自己扔伤害药水，这一支只为完整性保留。
        //
        // A witch never drinks harming potions; kept for completeness.
        return;
    }
    living::add_effect(
        mob,
        EffectInstance::with_amplifier(effect, duration, amplifier),
    );
}

/// `Witch.aiStep` 选药的四支：水下呼吸、抗火、治疗、速度。四次 `nextFloat` 按短路顺序取。
///
/// The four potion choices of `Witch.aiStep`, with the `nextFloat` draws in short-circuit
/// order.
fn choose_potion(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> Option<i32> {
    let eye_water = living::eye_in_fluid(mob, fluid_tags::WATER);
    if me.random.next_float() < 0.15 && eye_water && !mob.effects.has(mob_effect::WATER_BREATHING) {
        return Some(potion::WATER_BREATHING);
    }
    let fire_damage = mob
        .last_damage_source
        .is_some_and(|source| source.kind.is_fire());
    if me.random.next_float() < 0.15
        && (me.remaining_fire_ticks > 0 || fire_damage)
        && !mob.effects.has(mob_effect::FIRE_RESISTANCE)
    {
        return Some(potion::FIRE_RESISTANCE);
    }
    if me.random.next_float() < 0.05
        && mob.health < mob.attributes.value(attribute::MAX_HEALTH) as f32
    {
        return Some(potion::HEALING);
    }
    let far_target = super::goals::current_combat_target(mob, ctx)
        .is_some_and(|target| me.position.distance_to_sqr(target.position) > 121.0);
    if me.random.next_float() < 0.5 && far_target && !mob.effects.has(mob_effect::SPEED) {
        return Some(potion::SWIFTNESS);
    }
    None
}

/// `Witch.aiStep` 拿到药水之后：放进主手、设 `usingTime`、同步喝药标志、放喝药音效
/// （音高两次取数）、加 -0.25 移速。
///
/// After `Witch.aiStep` picks a potion: put it in the main hand, set `usingTime`, sync the
/// drinking flag, play the drink sound (two pitch draws) and add the -0.25 speed modifier.
fn start_drinking(me: &mut Entity, mob: &mut Mob, potion: i32) {
    mob.equipment.set(
        EquipmentSlot::MainHand,
        potion_stack(mc_bamboo_data::items::POTION, potion),
    );
    mob.witch.using_time = POTION_USE_DURATION;
    mob.witch_drinking = true;
    if !mob.silent {
        let first = me.random.next_float();
        let pitch = 0.8 + first * 0.4;
        living::play_sound(me, mob, "minecraft:entity.witch.drink", 1.0, pitch);
    }
    if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
        speed.remove_modifier(DRINKING_SPEED);
        speed.modifiers.push(super::attributes::Modifier {
            id: DRINKING_SPEED.to_owned(),
            amount: -0.25,
            operation: super::attributes::Operation::AddValue,
            permanent: false,
        });
    }
}

/// `PotionContents.createItemStack(Items.POTION, potion)`。
///
/// `PotionContents.createItemStack(Items.POTION, potion)`.
pub(super) fn potion_stack(item: i32, potion: i32) -> ItemStack {
    let mut stack = ItemStack::new(item, 1);
    crate::item_exchange::set_component(
        &mut stack,
        mc_bamboo_data::registries::data_component_type::POTION_CONTENTS,
        mc_bamboo_protocol::components::ComponentValue::PotionContents {
            potion: Some(potion),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        },
    );
    stack
}

/// `Witch.performRangedAttack`：按目标的位移提前量算方向，选药水，扔出去，再放投掷音效。
///
/// `Witch.performRangedAttack`: lead the target, pick a potion, throw it and play the
/// throw sound.
pub(super) fn perform_ranged_attack(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: super::goals::CombatTarget,
    _power: f32,
) {
    if mob.witch_drinking {
        return;
    }
    // `target.getDeltaMovement()` 这一期没有进 `CombatTarget`，提前量按零算。
    //
    // `target.getDeltaMovement()` is not in `CombatTarget` yet, so the lead is zero.
    let xd = target.position.x - me.position.x;
    let yd = target.eye_position().y - 1.1 - me.position.y;
    let zd = target.position.z - me.position.z;
    let dist = (xd * xd + zd * zd).sqrt();
    let potion = choose_throw_potion(me, dist);
    let speed = if dist <= 2.0 { 0.45 } else { 0.75 };
    let stack = potion_stack(mc_bamboo_data::items::SPLASH_POTION, potion);
    let shot = crate::entity::splash_potion::SplashShot {
        owner: Some(me.id),
        owner_uuid: Some(mob.uuid),
        item: stack,
        direction: Vec3::new(xd, yd + dist * 0.2, zd),
        power: speed,
        uncertainty: 8.0,
        position: Vec3::new(me.position.x, living::eye_y(me, mob) - 0.1, me.position.z),
        source_movement: None,
    };
    ctx.emit(
        crate::entity::ctx::Command::ThrowSplashPotion {
            shot: Box::new(shot),
        },
        me,
    );
    if !mob.silent {
        let first = me.random.next_float();
        let pitch = 0.8 + first * 0.4;
        living::play_sound(me, mob, "minecraft:entity.witch.throw", 1.0, pitch);
    }
}

/// `Witch.performRangedAttack` 选药：8 格外缓慢、3 格内 25% 虚弱，否则伤害。官方还有
/// 「目标血量」「目标已有药水效果」「袭击者」三支，`CombatTarget` 里没有这些字段，
/// 见汇报的疑点。
///
/// The potion choice of `Witch.performRangedAttack`: slowness past eight blocks, weakness
/// within three blocks at 25%, otherwise harming. Vanilla also tests the target's health,
/// effects and whether it is a raider, none of which `CombatTarget` carries.
fn choose_throw_potion(me: &mut Entity, dist: f64) -> i32 {
    if dist >= 8.0 {
        return potion::SLOWNESS;
    }
    if me.random.next_float() < 0.25 && dist <= 3.0 {
        return potion::WEAKNESS;
    }
    potion::HARMING
}

/// `Witch.getDamageAfterMagicAbsorb`：自己打自己为 0，`#witch_resistant_to`（魔法、
/// 荆棘）乘 0.15。
///
/// `Witch.getDamageAfterMagicAbsorb`: self-inflicted damage is zero and
/// `#witch_resistant_to` (magic, thorns) is multiplied by 0.15.
pub(super) fn damage_after_magic_absorb(
    source: super::DamageSource,
    damage: f32,
    victim: i32,
) -> f32 {
    let mut damage = damage;
    if source.attacker == Some(victim) {
        damage = 0.0;
    }
    if matches!(
        source.kind,
        super::DamageKind::Magic | super::DamageKind::Thorns
    ) {
        damage *= 0.15;
    }
    damage
}

#[cfg(test)]
mod tests {
    use super::super::MobType;
    use super::*;

    /// 属性是 `Monster` 的缺省加 26 血、0.25 移速。
    ///
    /// The monster defaults plus 26 health and 0.25 speed.
    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 26.0);
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), 0.25);
        assert_eq!(table.base(attribute::FOLLOW_RANGE), 16.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 2.0);
    }

    /// 目标和目标列表按官方顺序注册。
    ///
    /// Goals and targets keep the vanilla order.
    #[test]
    fn goals_and_targets_follow_vanilla() {
        let mob = Mob::new(MobType::Witch);
        let names: Vec<(i32, &str)> = mob
            .goals
            .goals
            .iter()
            .map(|wrapped| (wrapped.priority, wrapped.goal.name()))
            .collect();
        assert_eq!(
            names,
            vec![
                (1, "ObtainRaidLeaderBannerGoal"),
                (3, "PathfindToRaidGoal"),
                (4, "RaiderMoveThroughVillageGoal"),
                (5, "RaiderCelebration"),
                (1, "FloatGoal"),
                (2, "RangedAttackGoal"),
                (2, "WaterAvoidingRandomStrollGoal"),
                (3, "LookAtPlayerGoal"),
                (3, "RandomLookAroundGoal"),
            ]
        );
        assert_eq!(mob.target_goals.goals.len(), 3);
    }
}
