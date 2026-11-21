//! 鱿鱼和发光鱿鱼：官方 `Squid` / `GlowSquid`（`AgeableWaterCreature` 一支）。
//!
//! 鱿鱼不走寻路：两个目标只改 `movementVector`，`aiStep` 尾部按触手节拍把它写成速度，
//! `travel` 只是 `move(SELF, deltaMovement)`。离水后掉氧气、窒息；被生物打到会喷墨。
//!
//! Squid and glow squid: vanilla `Squid` / `GlowSquid` on the `AgeableWaterCreature`
//! branch. They never path-find: the two goals only write `movementVector`, the tail of
//! `aiStep` turns it into velocity on the tentacle beat, and `travel` is a bare
//! `move(SELF, deltaMovement)`. Out of water they lose air; hit by a mob they squirt ink.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    PathType, blocks, entity_types,
    registries::{attribute, mob_effect, particle_type},
    tags::fluid_tags,
};

use super::{
    DamageSource, Mob, MobEvent, MobType,
    attributes::Attributes,
    goals::{Goal, GoalSelector, WrappedGoal},
    living,
    path::Malus,
    spawn::SpawnGroup,
    water,
};
use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
    world::World,
};

/// `(float) Math.PI`。
const PI: f32 = std::f32::consts::PI;
/// `(float) (Math.PI * 2)`。
const TAU: f32 = std::f32::consts::TAU;
/// `180.0F / (float) Math.PI`。
const DEGREES: f32 = 180.0 / PI;

/// 鱿鱼专属的状态：身体倾角、触手节拍和目标写下的移动向量；发光鱿鱼再加变暗计时。
///
/// Squid-only state: body tilt, the tentacle beat and the movement vector the goals
/// write; glow squid add the dark countdown.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SquidState {
    pub x_body_rot: f32,
    pub x_body_rot_o: f32,
    pub z_body_rot: f32,
    pub z_body_rot_o: f32,
    pub tentacle_movement: f32,
    pub old_tentacle_movement: f32,
    pub tentacle_angle: f32,
    pub old_tentacle_angle: f32,
    pub tentacle_speed: f32,
    pub rotate_speed: f32,
    /// `Squid.movementVector`。
    pub movement: Vec3,
    /// `GlowSquid.DATA_DARK_TICKS_REMAINING`。
    pub dark_ticks: i32,
}

pub(crate) const fn is_squid(kind: MobType) -> bool {
    matches!(kind, MobType::Squid | MobType::GlowSquid)
}

/// 成年用实体类型表的尺寸（眼高 `eyeHeight(0.4F)`），幼年是 `Squid.BABY_DIMENSIONS`。
///
/// Adults use the entity type's size (`eyeHeight(0.4F)`); babies `Squid.BABY_DIMENSIONS`.
pub(super) const fn dimensions(kind: MobType, baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.5, 0.5, 0.37)
    } else {
        let facts = &entity_types::FACTS[kind.entity_type() as usize];
        (facts.width, facts.height, 0.4)
    }
}

/// `Squid.createAttributes`：`Mob.createMobAttributes()` 加 10 点血。
///
/// `Squid.createAttributes`: the mob defaults with ten health.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::mob();
    attributes.set_base(attribute::MAX_HEALTH, 10.0);
    attributes
}

/// `AgeableWaterCreature` 构造函数：`setPathfindingMalus(PathType.WATER, 0.0F)`。
///
/// The `AgeableWaterCreature` constructor: `setPathfindingMalus(PathType.WATER, 0.0F)`.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(PathType::Water, 0.0);
    malus
}

/// `Squid.registerGoals`：0 随机游动，1 逃离攻击者；都不占标志位。
///
/// `Squid.registerGoals`: random movement at 0, fleeing at 1; neither takes a flag.
pub(super) fn goals() -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = vec![
        WrappedGoal {
            priority: 0,
            running: false,
            goal: Goal::SquidRandomMovement,
        },
        WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::SquidFlee { flee_ticks: 0 },
        },
    ];
    selector
}

/// `Squid` 构造函数尾部：`tentacleSpeed = 1 / (nextFloat + 1) * 0.2`。
///
/// The tail of the `Squid` constructor: one `nextFloat` for the tentacle speed.
pub(crate) fn roll_tentacle_speed(random: &mut crate::entity::JavaRandom) -> f32 {
    1.0 / (random.next_float() + 1.0) * 0.2
}

pub(super) const fn ambient_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::GlowSquid => "minecraft:entity.glow_squid.ambient",
        _ => "minecraft:entity.squid.ambient",
    }
}

pub(super) const fn hurt_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::GlowSquid => "minecraft:entity.glow_squid.hurt",
        _ => "minecraft:entity.squid.hurt",
    }
}

pub(super) const fn death_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::GlowSquid => "minecraft:entity.glow_squid.death",
        _ => "minecraft:entity.squid.death",
    }
}

const fn squirt_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::GlowSquid => "minecraft:entity.glow_squid.squirt",
        _ => "minecraft:entity.squid.squirt",
    }
}

const fn ink_particle(kind: MobType) -> i32 {
    match kind {
        MobType::GlowSquid => particle_type::GLOW_SQUID_INK,
        _ => particle_type::SQUID_INK,
    }
}

/// `Squid.hurtServer` 加 `GlowSquid.hurtServer`：父类成功且 `getLastHurtByMob() != null`
/// 才算成功并喷墨；发光鱿鱼成功时变暗 100 刻。
///
/// `Squid.hurtServer` with `GlowSquid.hurtServer` on top: only a hit that leaves a
/// `lastHurtByMob` counts and squirts ink; a glow squid then goes dark for 100 ticks.
pub(super) fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    if !living::living_hurt_server(me, mob, ctx, source, amount) {
        return false;
    }
    if last_hurt_by_mob_position(mob, ctx).is_none() {
        return false;
    }
    spawn_ink(me, mob);
    if mob.mob_type == MobType::GlowSquid {
        mob.squid.dark_ticks = 100;
    }
    true
}

/// `getLastHurtByMob()` 的位置：实体表里的生物或玩家表里的玩家，找不到就是 `null`。
///
/// The position of `getLastHurtByMob()`: a mob in the entity table or a player; `None`
/// when the reference resolves to nothing.
fn last_hurt_by_mob_position(mob: &Mob, ctx: &dyn EntityCtx) -> Option<Vec3> {
    let attacker: EntityUid = mob.last_hurt_by_mob?;
    if let Some(other) = ctx.other(attacker) {
        return (!other.removed).then_some(other.position);
    }
    super::goals::player_by_uid(ctx.blocks(), attacker).map(|player| player.position)
}

/// `Squid.rotateVector`：先绕 X 转上一刻的 `xBodyRot`，再绕 Y 转上一刻的 `yBodyRot`。
///
/// `Squid.rotateVector`: rotate about X by last tick's `xBodyRot`, then about Y by last
/// tick's `yBodyRot`.
fn rotate_vector(mob: &Mob, vector: Vec3) -> Vec3 {
    let rotated = x_rot(vector, mob.squid.x_body_rot_o * (PI / 180.0));
    y_rot(rotated, -mob.y_body_rot_o * (PI / 180.0))
}

/// `Vec3.xRot(float)`。
fn x_rot(vector: Vec3, radians: f32) -> Vec3 {
    let cos = f64::from(mth::cos(f64::from(radians)));
    let sin = f64::from(mth::sin(f64::from(radians)));
    Vec3::new(
        vector.x,
        vector.y * cos + vector.z * sin,
        vector.z * cos - vector.y * sin,
    )
}

/// `Vec3.yRot(float)`。
fn y_rot(vector: Vec3, radians: f32) -> Vec3 {
    let cos = f64::from(mth::cos(f64::from(radians)));
    let sin = f64::from(mth::sin(f64::from(radians)));
    Vec3::new(
        vector.x * cos + vector.z * sin,
        vector.y,
        vector.z * cos - vector.x * sin,
    )
}

/// `Squid.spawnInk`：喷墨声（音高两次 `nextFloat`），再发 30 个数量为 0 的粒子包，
/// 每个取三次 `nextFloat`。
///
/// `Squid.spawnInk`: the squirt sound (two `nextFloat` for the pitch), then thirty
/// count-0 particle packets drawing three `nextFloat` each.
fn spawn_ink(me: &mut Entity, mob: &mut Mob) {
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(
        me,
        mob,
        squirt_sound(mob.mob_type),
        mob.mob_type.sound_volume(),
        pitch,
    );
    let origin = rotate_vector(mob, Vec3::new(0.0, -1.0, 0.0)).add(me.position);
    let particle = ink_particle(mob.mob_type);
    for _ in 0..30 {
        let x = f64::from(me.random.next_float()) * 0.6 - 0.3;
        let z = f64::from(me.random.next_float()) * 0.6 - 0.3;
        let direction = rotate_vector(mob, Vec3::new(x, -1.0, z));
        let base: f32 = if mob.is_baby() { 0.1 } else { 0.3 };
        let offset = direction.scale(f64::from(base + me.random.next_float() * 2.0));
        mob.events.push(MobEvent::Particles {
            particle,
            position: Vec3::new(origin.x, origin.y + 0.5, origin.z),
            offset,
            speed: 0.1,
            count: 0,
        });
    }
}

/// `Squid.aiStep` 在 `super.aiStep()` 之后的部分，发光鱿鱼再接 `GlowSquid.aiStep` 的尾部。
///
/// The part of `Squid.aiStep` after `super.aiStep()`, followed by the tail of
/// `GlowSquid.aiStep` for glow squid.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob) {
    let state = &mut mob.squid;
    state.x_body_rot_o = state.x_body_rot;
    state.z_body_rot_o = state.z_body_rot;
    state.old_tentacle_movement = state.tentacle_movement;
    state.old_tentacle_angle = state.tentacle_angle;
    state.tentacle_movement += state.tentacle_speed;
    if f64::from(state.tentacle_movement) > std::f64::consts::TAU {
        state.tentacle_movement -= TAU;
        if me.random.next_int_bounded(10) == 0 {
            state.tentacle_speed = roll_tentacle_speed(&mut me.random);
        }
        mob.events.push(MobEvent::EntityEvent { event: 19 });
    }
    if me.in_water {
        swim_step(me, mob);
    } else {
        beached_step(me, mob);
    }
    if mob.mob_type == MobType::GlowSquid {
        glow_step(me, mob);
    }
}

/// 在水里：触手节拍的前半段末尾把 `movementVector` 写成速度，后半段每刻乘 0.9；
/// 身体朝速度方向转。
///
/// In water: late in the first half of the beat the movement vector becomes the
/// velocity, the second half decays it by 0.9; the body turns towards the velocity.
fn swim_step(me: &mut Entity, mob: &mut Mob) {
    let state = &mut mob.squid;
    if state.tentacle_movement < PI {
        let scale = state.tentacle_movement / PI;
        state.tentacle_angle = mth::sin(f64::from(scale * scale * PI)) * PI * 0.25;
        if f64::from(scale) > 0.75 {
            me.delta = state.movement;
            state.rotate_speed = 1.0;
        } else {
            state.rotate_speed *= 0.8;
        }
    } else {
        state.tentacle_angle = 0.0;
        me.delta = me.delta.scale(0.9);
        state.rotate_speed *= 0.99;
    }
    let movement = me.delta;
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let yaw = -(mth::atan2(movement.x, movement.z) as f32) * DEGREES;
    mob.y_body_rot += (yaw - mob.y_body_rot) * 0.1;
    me.y_rot = mob.y_body_rot;
    state.z_body_rot += PI * state.rotate_speed * 1.5;
    let pitch = -(mth::atan2(horizontal, movement.y) as f32) * DEGREES;
    state.x_body_rot += (pitch - state.x_body_rot) * 0.1;
}

/// 离水：水平速度清零，竖直方向自己算重力（或漂浮），乘 `getAirDrag()`；身体慢慢竖起来。
///
/// Out of water: horizontal velocity is zeroed, gravity (or levitation) is applied by
/// hand and scaled by `getAirDrag()`; the body slowly tips upright.
fn beached_step(me: &mut Entity, mob: &mut Mob) {
    let y = match mob.effects.amplifier(mob_effect::LEVITATION) {
        Some(amplifier) => 0.05 * f64::from(amplifier + 1),
        None => me.delta.y - gravity(mob),
    };
    me.delta = Vec3::new(0.0, y * f64::from(air_drag(mob)), 0.0);
    let state = &mut mob.squid;
    state.tentacle_angle = mth::sin(f64::from(state.tentacle_movement)).abs() * PI * 0.25;
    state.x_body_rot += (-90.0 - state.x_body_rot) * 0.02;
}

/// `Entity.getGravity()`，`Squid.getDefaultGravity()` 固定 0.08（不读属性）。
///
/// `Entity.getGravity()`; `Squid.getDefaultGravity()` is a fixed 0.08, not the attribute.
fn gravity(mob: &Mob) -> f64 {
    if mob.no_gravity { 0.0 } else { 0.08 }
}

/// `LivingEntity.getAirDrag()`：`computeModifiedFriction(0.98F, AIR_DRAG_MODIFIER)`。
fn air_drag(mob: &Mob) -> f32 {
    let modifier = mob.attributes.value(attribute::AIR_DRAG_MODIFIER) as f32;
    mth::clamp_f32(1.0 - (1.0 - 0.98) * modifier, 0.0, 1.0)
}

/// `GlowSquid.aiStep` 的尾部：变暗计时减一；服务端的 `addParticle` 不发包，但参数里的
/// `getRandomX` / `getRandomY` / `getRandomZ` 照样各取一次 `nextDouble`。
///
/// The tail of `GlowSquid.aiStep`: count the dark ticks down. `addParticle` sends nothing
/// on the server, yet its `getRandomX` / `getRandomY` / `getRandomZ` arguments still draw
/// one `nextDouble` each.
fn glow_step(me: &mut Entity, mob: &mut Mob) {
    if mob.squid.dark_ticks > 0 {
        mob.squid.dark_ticks -= 1;
    }
    for _ in 0..3 {
        me.random.next_double();
    }
}

/// `Squid.hasMovementVector`：`lengthSqr() > 1.0E-5F`。
fn has_movement_vector(mob: &Mob) -> bool {
    mob.squid.movement.length_sqr() > f64::from(1.0e-5_f32)
}

/// `SquidRandomMovementGoal.tick`：闲置超过 100 刻就停；否则 1/25 的概率、或不在水里、
/// 或没有移动向量时重抽一个方向。
///
/// `SquidRandomMovementGoal.tick`: stop after 100 idle ticks; otherwise pick a new
/// direction on a 1-in-25 roll, when out of water, or when there is no vector.
pub(super) fn random_movement_tick(me: &mut Entity, mob: &mut Mob) {
    if mob.no_action_time > 100 {
        mob.squid.movement = Vec3::default();
        return;
    }
    // `reducedTickDelay(50)` 是 25。短路求值：抽中 0 就不再看后两个条件。
    // `reducedTickDelay(50)` is 25; the roll short-circuits the two other conditions.
    if me.random.next_int_bounded(25) == 0 || !me.in_water || !has_movement_vector(mob) {
        let angle = me.random.next_float() * TAU;
        let x = mth::cos(f64::from(angle)) * 0.2;
        let y = -0.1 + me.random.next_float() * 0.2;
        let z = mth::sin(f64::from(angle)) * 0.2;
        mob.squid.movement = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    }
}

/// `SquidFleeGoal.canUse`：在水里，且离 `getLastHurtByMob()` 不到 10 格。
///
/// `SquidFleeGoal.canUse`: in water and within ten blocks of `getLastHurtByMob()`.
pub(super) fn flee_can_use(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    if !me.in_water {
        return false;
    }
    last_hurt_by_mob_position(mob, ctx)
        .is_some_and(|attacker| me.position.distance_to_sqr(attacker) < 100.0)
}

/// `SquidFleeGoal.tick`：朝远离攻击者的方向游。官方的 `fleeTo.normalize()` 丢弃了返回值，
/// 所以向量没有归一化，这里照样不归一化。水泡粒子只在客户端有，不取随机数。
///
/// `SquidFleeGoal.tick`: swim away from the attacker. Vanilla discards the result of
/// `fleeTo.normalize()`, so the vector stays unnormalised here as well. The bubble
/// particle is client-only and draws nothing.
pub(super) fn flee_tick(flee_ticks: &mut i32, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    *flee_ticks += 1;
    let Some(attacker) = last_hurt_by_mob_position(mob, ctx) else {
        return;
    };
    let mut flee = Vec3::new(
        me.position.x - attacker.x,
        me.position.y - attacker.y,
        me.position.z - attacker.z,
    );
    let view: &dyn BlockView = ctx.blocks();
    let target = BlockPos::containing(me.position.add(flee));
    let air = view.block_state(target).is_air();
    if !view.fluid_state(target).is_tagged(fluid_tags::WATER) && !air {
        return;
    }
    let length = flee.length();
    if length > 0.0 {
        let mut speed = 3.0;
        if length > 5.0 {
            speed -= (length - 5.0) / 5.0;
        }
        if speed > 0.0 {
            flee = flee.scale(speed);
        }
    }
    if air {
        flee = Vec3::new(flee.x, 0.0, flee.z);
    }
    mob.squid.movement = Vec3::new(flee.x / 20.0, flee.y / 20.0, flee.z / 20.0);
}

/// `SpawnPlacements` 给两种鱿鱼登记的规则：鱿鱼用
/// `checkSurfaceAgeableWaterCreatureSpawnRules`，发光鱿鱼用 `checkGlowSquidSpawnRules`
/// （海平面下 33 格以下、原始亮度 0、这一格是水方块）。
///
/// The `SpawnPlacements` predicates: squid use
/// `checkSurfaceAgeableWaterCreatureSpawnRules`, glow squid `checkGlowSquidSpawnRules`
/// (at least 33 below sea level, raw brightness 0 and a water block at the position).
pub(super) fn check_spawn_rules(
    world: &World,
    kind: MobType,
    pos: BlockPos,
    sea_level: i32,
) -> bool {
    if kind == MobType::GlowSquid {
        return pos.y <= sea_level - 33
            && world.raw_brightness(pos) == 0
            && world.block_state(pos).is(blocks::WATER);
    }
    water::surface_spawn_rules(world, sea_level, pos)
}

/// `Squid.finalizeSpawn`：没有组数据时新建 `AgeableMobGroupData(0.05F)`，其余是
/// `AgeableMob` / `Mob` 两层。
///
/// `Squid.finalizeSpawn`: a fresh `AgeableMobGroupData(0.05F)` when there is no group,
/// then the `AgeableMob` and `Mob` layers.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    water::finalize_ageable(entity, random, group, 0.05);
}
