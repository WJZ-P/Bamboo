//! 生物的一刻：`Entity.commonTick` → `LivingEntity.tick` → `Mob.tick`，以及受伤、死亡、
//! 跳跃、行走（`travel`）和推挤。
//!
//! 顺序照 `ServerLevel` 的实体循环：先 `checkDespawn`，再 `commonTick`，再 `tick`。
//! 移动和方块效果在 [`super::motion`]。
//!
//! One mob tick, in the order of the `ServerLevel` entity loop. Movement and block
//! effects live in [`super::motion`].

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    FluidTag,
    registries::attribute,
    tags::{block_tags, fluid_tags},
};

use super::{
    DamageKind, DamageSource, FluidTracker, Mob, MobEvent,
    control::{
        self, MAX_HEAD_Y_ROT, Operation, clamp_x_rot, look_x_rot, look_y_rot, move_rotlerp,
        rotate_towards,
    },
    goals::Flag,
    motion,
    navigation::NavMob,
    path::PathMob,
    timing,
};
use crate::{
    entity::{
        Entity, EntityKind, KindPublic, PrimedTnt,
        ctx::{BlockView, Command, EntityCtx},
    },
    shape::{self, CollisionContext},
};

/// 生物的一刻，`Entity.tick` 交到这里。`kind` 在这期间被换成一个占位。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let mut kind = std::mem::replace(&mut me.kind, EntityKind::Tnt(PrimedTnt { fuse: 0 }));
    if let EntityKind::Mob(mob) = &mut kind {
        tick_mob(me, mob, ctx);
    }
    me.kind = kind;
}

/// 对一个生物跑一段外部逻辑（受伤、命令），期间 `kind` 也换成占位。
pub(crate) fn with_mob<R>(
    me: &mut Entity,
    f: impl FnOnce(&mut Entity, &mut Mob) -> R,
) -> Option<R> {
    let mut kind = std::mem::replace(&mut me.kind, EntityKind::Tnt(PrimedTnt { fuse: 0 }));
    let result = match &mut kind {
        EntityKind::Mob(mob) => Some(f(me, mob)),
        _ => None,
    };
    me.kind = kind;
    result
}

fn tick_mob(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    timing::span!(Other);
    check_despawn(me, mob, ctx);
    if me.removed {
        return;
    }
    // `Entity.commonTick`。
    if mob.invulnerable_time > 0 {
        mob.invulnerable_time -= 1;
    }
    me.old_position = me.position;
    mob.y_rot_o = me.y_rot;
    mob.x_rot_o = mob.x_rot;
    me.tick_count += 1;
    // `LivingEntity.tick` → `Entity.tick` → `Mob.baseTick`。
    base_tick(me, mob, ctx);
    if !me.removed {
        ai_step(me, mob, ctx);
    }
    // `tickHeadTurn` → `Mob` 改成 `BodyRotationControl.clientTick`。
    body_rotation_tick(me, mob);
    range_checks(me, mob);
    // `Mob.tick`：每 5 刻按乘客和载具重设控制标志；牛没有乘客，三个都打开。
    if me.tick_count % 5 == 0 {
        mob.goals.set_control_flag(Flag::Move, true);
        mob.goals.set_control_flag(Flag::Jump, true);
        mob.goals.set_control_flag(Flag::Look, true);
    }
}

/// `LivingEntity.tick` 的 `rangeChecks`。
fn range_checks(me: &Entity, mob: &mut Mob) {
    fn fold(current: f32, old: &mut f32) {
        while current - *old < -180.0 {
            *old -= 360.0;
        }
        while current - *old >= 180.0 {
            *old += 360.0;
        }
    }
    fold(me.y_rot, &mut mob.y_rot_o);
    fold(mob.y_body_rot, &mut mob.y_body_rot_o);
    fold(mob.x_rot, &mut mob.x_rot_o);
    fold(mob.y_head_rot, &mut mob.y_head_rot_o);
}

/// `isAlive()`。
pub(crate) fn is_alive(me: &Entity, mob: &Mob) -> bool {
    !me.removed && mob.health > 0.0
}

/// `Mob.checkDespawn`。牛不会被远离玩家删掉，但最近的玩家决定 `noActionTime` 归不归零。
fn check_despawn(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // 难度不是和平，`isAllowedInPeaceful` 也为真，这一支不删。
    let persistent = mob.persistence_required;
    let mut nearest: Option<f64> = None;
    ctx.blocks().for_each_player(&mut |player| {
        if player.spectator {
            return;
        }
        let distance = player.position.distance_to_sqr(me.position);
        if nearest.is_none_or(|best| distance < best) {
            nearest = Some(distance);
        }
    });
    let Some(distance) = nearest else {
        return;
    };
    // `CREATURE` 的 `despawnDistance` 是 128，`Animal.removeWhenFarAway` 为假。
    const NO_DESPAWN: f64 = 32.0 * 32.0;
    if !persistent
        && mob.no_action_time > 600
        && me.random.next_int_bounded(800) == 0
        && distance > NO_DESPAWN
    {
        // `removeWhenFarAway` 为假，不删。
    } else if distance < NO_DESPAWN {
        mob.no_action_time = 0;
    }
}

// ---- 流体 ----

/// `getFluidHeight(tag)`。
pub(crate) fn fluid_height(mob: &Mob, tag: FluidTag) -> f64 {
    let mut height = 0.0;
    for tracker in &mob.fluid_trackers {
        if tracker.height > height && fluid_is_tagged(tracker.fluid, tag) {
            height = tracker.height;
        }
    }
    height
}

fn fluid_is_tagged(id: i32, tag: FluidTag) -> bool {
    crate::state::fluid_type_is_tagged(id, tag)
}

/// `isEyeInFluid(tag)`。
pub(crate) fn eye_in_fluid(mob: &Mob, tag: FluidTag) -> bool {
    mob.fluid_trackers
        .iter()
        .any(|tracker| tracker.eyes_inside && fluid_is_tagged(tracker.fluid, tag))
}

/// `getFluidJumpThreshold`。
pub(crate) fn fluid_jump_threshold(mob: &Mob) -> f64 {
    if f64::from(mob.eye_height) < 0.4 {
        0.0
    } else {
        0.4
    }
}

/// `isInLava()`。
pub(crate) fn is_in_lava(me: &Entity, mob: &Mob) -> bool {
    !me.first_tick && fluid_height(mob, fluid_tags::LAVA) > 0.0
}

fn is_in_liquid(me: &Entity, mob: &Mob) -> bool {
    me.in_water || is_in_lava(me, mob)
}

/// `getEyeY()`。
fn eye_y(me: &Entity, mob: &Mob) -> f64 {
    me.position.y + f64::from(mob.eye_height)
}

/// `getLastDamageSource()`：40 刻以内的那一次。
pub(crate) fn last_damage_source(mob: &Mob, now: i64) -> Option<DamageSource> {
    if now - mob.last_damage_stamp > 40 {
        None
    } else {
        mob.last_damage_source
    }
}

/// 导航要的生物快照。
pub(crate) fn nav_mob(me: &Entity, mob: &Mob, view: &dyn BlockView) -> NavMob {
    let (width, height) = mob.dimensions();
    NavMob {
        path: PathMob {
            position: me.position,
            on_ground: me.on_ground,
            in_floatable_fluid: fluid_height(mob, fluid_tags::ENTITY_FLOATABLE) > 0.0,
            width,
            height,
            max_up_step: max_up_step(mob),
            max_fall_distance: 3,
            malus: mob.malus,
            context: me.collision_context(),
        },
        speed: mob.speed,
        in_liquid: is_in_liquid(me, mob),
        follow_range: mob.attributes.value(attribute::FOLLOW_RANGE),
        game_time: view.game_time(),
    }
}

/// `LivingEntity.maxUpStep()`。
pub(crate) fn max_up_step(mob: &Mob) -> f32 {
    mob.attributes.value(attribute::STEP_HEIGHT) as f32
}

/// `Mob.baseTick` → `LivingEntity.baseTick` → `Entity.baseTick`。
fn base_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `Entity.baseTick`。
    mob.was_in_powder_snow = mob.in_powder_snow;
    mob.in_powder_snow = false;
    mob.was_eye_in_water = eye_in_fluid(mob, fluid_tags::WATER);
    motion::update_fluid_interaction(me, mob, ctx.blocks());
    if me.remaining_fire_ticks > 0 {
        if me.remaining_fire_ticks % 20 == 0 && !is_in_lava(me, mob) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::OnFire),
                1.0,
            );
        }
        me.remaining_fire_ticks -= 1;
    }
    if is_in_lava(me, mob) {
        me.fall_distance *= 0.5;
    }
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::OutOfWorld),
            4.0,
        );
    }
    me.first_tick = false;
    // `LivingEntity.baseTick` 自己的部分。
    if is_alive(me, mob) {
        if is_in_wall(me, mob, ctx.blocks()) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::InWall),
                1.0,
            );
        }
        let eye = BlockPos::containing(Vec3::new(me.position.x, eye_y(me, mob), me.position.z));
        if eye_in_fluid(mob, fluid_tags::WATER)
            && !ctx
                .blocks()
                .block_state(eye)
                .is(mc_bamboo_data::blocks::BUBBLE_COLUMN)
        {
            // 牛不能在水下呼吸，没有效果；`OXYGEN_BONUS` 是 0，不取随机数。
            mob.air_supply -= 1;
            if mob.air_supply <= -20 {
                mob.air_supply = 0;
                hurt_server(
                    me,
                    mob,
                    ctx,
                    DamageSource::environment(DamageKind::Drown),
                    2.0,
                );
            }
        } else if mob.air_supply < 300 {
            mob.air_supply = (mob.air_supply + 4).min(300);
        }
        mob.last_pos = Some(me.block_position());
    }
    if mob.hurt_time > 0 {
        mob.hurt_time -= 1;
    }
    if mob.damage_cooldown > 0 {
        mob.damage_cooldown -= 1;
    }
    if mob.is_dead_or_dying() {
        tick_death(me, mob);
    }
    if mob.last_hurt_by_player_memory_time > 0 {
        mob.last_hurt_by_player_memory_time -= 1;
    } else {
        mob.last_hurt_by_player = None;
        mob.last_hurt_by_player_ref = None;
    }
    if mob.last_hurt_by_mob.is_some() || mob.last_hurt_by_mob_ref.is_some() {
        // `getLastHurtByMob()` 按引用找实体：玩家不在实体表里，从玩家表找；读盘后只剩
        // UUID 的引用找不到实体，当作没了。
        // Players are not in the entity table; a reference left with only a UUID
        // after a load resolves to nothing.
        let alive = match mob.last_hurt_by_mob {
            Some(attacker) => match ctx.other(attacker) {
                Some(other) => match &other.kind {
                    KindPublic::Mob(public) => public.is_alive() && !other.removed,
                    _ => !other.removed,
                },
                None => {
                    let mut alive = false;
                    ctx.blocks().for_each_player(&mut |player| {
                        if player.uid == attacker {
                            alive = player.alive;
                        }
                    });
                    alive
                }
            },
            None => false,
        };
        if !alive || me.tick_count - mob.last_hurt_by_mob_timestamp > 100 {
            mob.last_hurt_by_mob = None;
            mob.last_hurt_by_mob_ref = None;
        }
    }
    mob.y_head_rot_o = mob.y_head_rot;
    mob.y_body_rot_o = mob.y_body_rot;
    // `Mob.baseTick`：先取随机数，再拿自增前的值比较。
    if is_alive(me, mob) {
        let roll = me.random.next_int_bounded(1000);
        let time = mob.ambient_sound_time;
        mob.ambient_sound_time += 1;
        if roll < time {
            mob.ambient_sound_time = -120;
            let pitch = voice_pitch(me, mob);
            play_sound(me, mob, mob.sound_variant.ambient(), 1.0, pitch);
        }
    }
}

/// `Entity.playSound`：静音的生物不出声（音高这些参数调用方已经取过）。
///
/// `Entity.playSound`: a silent mob makes no sound (the caller already drew the pitch).
pub(crate) fn play_sound(me: &Entity, mob: &mut Mob, sound: &'static str, volume: f32, pitch: f32) {
    if mob.silent {
        return;
    }
    mob.events.push(MobEvent::Sound {
        sound,
        volume,
        pitch,
        position: me.position,
    });
}

/// `Mob.playAmbientSound`：`makeSound(getAmbientSound())`。
pub(crate) fn play_ambient_sound(me: &mut Entity, mob: &mut Mob) {
    let pitch = voice_pitch(me, mob);
    play_sound(me, mob, mob.sound_variant.ambient(), 1.0, pitch);
}

/// `getVoicePitch()`：两个 `nextFloat`，牛的叫声从不为空，静音时也照取。
fn voice_pitch(me: &mut Entity, mob: &Mob) -> f32 {
    let first = me.random.next_float();
    let second = me.random.next_float();
    let base = if mob.is_baby() { 1.5 } else { 1.0 };
    (first - second) * 0.2 + base
}

/// `Entity.isInWall`：眼睛那一小片和会让人窒息的方块重叠。
fn is_in_wall(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    let width = f64::from(me.dimensions.0 * 0.8);
    let eye = eye_y(me, mob);
    let bounds = mc_bamboo_core::Aabb::new(
        me.position.x - width / 2.0,
        eye - 1.0e-6 / 2.0,
        me.position.z - width / 2.0,
        me.position.x + width / 2.0,
        eye + 1.0e-6 / 2.0,
        me.position.z + width / 2.0,
    );
    let min = BlockPos::containing(Vec3::new(bounds.min_x, bounds.min_y, bounds.min_z));
    let max = BlockPos::containing(Vec3::new(bounds.max_x, bounds.max_y, bounds.max_z));
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let pos = BlockPos::new(x, y, z);
                let state = view.block_state(pos);
                if state.is_air() || !state.is_suffocating() {
                    continue;
                }
                let boxes = shape::collision_boxes(state, pos, CollisionContext::Empty);
                if boxes.iter().any(|part| part.at(pos).intersects(bounds)) {
                    return true;
                }
            }
        }
    }
    false
}

/// `tickDeath`：第 20 刻移除。
fn tick_death(me: &mut Entity, mob: &mut Mob) {
    mob.death_time += 1;
    if mob.death_time >= 20 && !me.removed {
        mob.events.push(MobEvent::Poof);
        me.discard();
    }
}

// ---- aiStep ----

/// `Animal.aiStep` → `AgeableMob.aiStep` → `Mob.aiStep` → `LivingEntity.aiStep`。
fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.no_jump_delay > 0 {
        mob.no_jump_delay -= 1;
    }
    let delta = me.delta;
    let mut x = delta.x;
    let mut y = delta.y;
    let mut z = delta.z;
    if delta.x.abs() < 0.003 {
        x = 0.0;
    }
    if delta.z.abs() < 0.003 {
        z = 0.0;
    }
    if delta.y.abs() < 0.003 {
        y = 0.0;
    }
    me.delta = Vec3::new(x, y, z);
    // `applyInput`。
    mob.xxa *= 0.98;
    mob.zza *= 0.98;
    if mob.is_dead_or_dying() {
        mob.jumping = false;
        mob.xxa = 0.0;
        mob.zza = 0.0;
    } else if !mob.no_ai {
        server_ai_step(me, mob, ctx);
    }
    // 跳。
    if mob.jumping {
        let fluid = if is_in_lava(me, mob) {
            fluid_height(mob, fluid_tags::LAVA)
        } else {
            fluid_height(mob, fluid_tags::WATER)
        };
        let in_water = me.in_water && fluid > 0.0;
        let threshold = fluid_jump_threshold(mob);
        // 岩浆那一支是 `!isInShallowFluid(LAVA)`，NaN 时为真，不能写成 `>`。
        // The lava branch is `!isInShallowFluid(LAVA)`, true on NaN, so it is not `>`.
        let deep_lava = !matches!(
            fluid_height(mob, fluid_tags::LAVA).partial_cmp(&fluid_jump_threshold(mob)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        );
        if (in_water && (!me.on_ground || fluid > threshold))
            || (is_in_lava(me, mob) && (!me.on_ground || deep_lava))
        {
            jump_in_liquid(me, mob);
        } else if (me.on_ground || (in_water && fluid <= threshold)) && mob.no_jump_delay == 0 {
            jump_from_ground(me, mob, ctx.blocks());
            mob.no_jump_delay = 10;
        }
    } else {
        mob.no_jump_delay = 0;
    }
    // 走。
    let input = Vec3::new(f64::from(mob.xxa), f64::from(mob.yya), f64::from(mob.zza));
    if !mob.no_ai {
        timing::span!(Travel);
        travel(me, mob, ctx, input);
    }
    {
        timing::span!(BlockEffects);
        motion::apply_effects_from_blocks(me, mob, ctx);
    }
    // 冻结。
    if !mob.in_powder_snow {
        mob.ticks_frozen = (mob.ticks_frozen - 2).max(0);
    }
    if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
        speed.remove_modifier(POWDER_SNOW_SLOW);
    }
    let legacy = me.on_pos(ctx.blocks(), 0.2);
    if !ctx.blocks().block_state(legacy).is_air() && mob.ticks_frozen > 0 {
        let percent = mob.ticks_frozen.min(140) as f32 / 140.0;
        let amount = -0.05_f32 * percent;
        if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
            speed.modifiers.push(super::attributes::Modifier {
                id: POWDER_SNOW_SLOW.to_owned(),
                amount: f64::from(amount),
                operation: super::attributes::Operation::AddValue,
                permanent: false,
            });
        }
    }
    if me.tick_count % 40 == 0 && mob.ticks_frozen >= 140 {
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Freeze),
            1.0,
        );
    }
    {
        timing::span!(Push);
        push_entities(me, mob, ctx);
    }
    // `AgeableMob.aiStep`。
    if mob.forced_age_timer > 0 {
        if mob.forced_age_timer % 4 == 0 {
            for _ in 0..3 {
                me.random.next_double();
            }
        }
        mob.forced_age_timer -= 1;
    }
    if is_alive(me, mob) {
        if mob.is_baby() && !mob.age_locked {
            set_age(me, mob, mob.age + 1);
        } else if mob.age > 0 {
            set_age(me, mob, mob.age - 1);
        }
    }
    if mob.age_lock_particle_timer > 0 {
        if mob.age_lock_particle_timer % 2 == 0 {
            for _ in 0..3 {
                me.random.next_double();
            }
        }
        mob.age_lock_particle_timer -= 1;
    }
    // `Animal.aiStep`：心形粒子在服务端不显示，但参数照算。
    if mob.age != 0 {
        mob.in_love = 0;
    }
    if mob.in_love > 0 {
        mob.in_love -= 1;
        if mob.in_love % 10 == 0 {
            for _ in 0..3 {
                me.random.next_gaussian();
            }
            for _ in 0..3 {
                me.random.next_double();
            }
        }
    }
}

/// `LivingEntity.SPEED_MODIFIER_POWDER_SNOW_ID`。
const POWDER_SNOW_SLOW: &str = "minecraft:powder_snow";

/// `AgeableMob.setAge`：跨过 0 时换尺寸。
pub(crate) fn set_age(me: &mut Entity, mob: &mut Mob, age: i32) {
    let old = mob.age;
    mob.age = age;
    if (old < 0) != (age < 0) {
        let (width, height, eye) = mob.mob_type.dimensions(age < 0);
        me.dimensions = (width, height);
        mob.eye_height = eye;
    }
}

/// `Mob.serverAiStep`。
fn server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    mob.no_action_time += 1;
    mob.seen.clear();
    {
        timing::span!(Goals);
        let mut goals = std::mem::take(&mut mob.goals);
        let phase = me.tick_count.wrapping_add(me.id);
        if phase % 2 != 0 && me.tick_count > 1 {
            goals.tick_running(false, me, mob, ctx);
        } else {
            goals.tick(me, mob, ctx);
        }
        mob.goals = goals;
    }
    timing::span!(Navigation);
    let nav = nav_mob(me, mob, ctx.blocks());
    if let Some((wanted, speed)) = mob.navigation.tick(ctx.blocks(), &nav) {
        mob.mover
            .set_wanted_position(wanted.x, wanted.y, wanted.z, speed);
    }
    // `Animal.customServerAiStep`。
    if mob.age != 0 {
        mob.in_love = 0;
    }
    move_control_tick(me, mob, ctx.blocks());
    look_control_tick(me, mob);
    // `JumpControl.tick`。
    mob.jumping = mob.jump.jump;
    mob.jump.jump = false;
}

/// `Mob.setSpeed`：同时设 `zza`。
fn set_speed(mob: &mut Mob, speed: f32) {
    mob.speed = speed;
    mob.zza = speed;
}

/// `MoveControl.tick`（牛不横移，`STRAFE` 走不到）。
fn move_control_tick(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    match mob.mover.operation {
        Operation::MoveTo => {
            mob.mover.operation = Operation::Wait;
            let dx = mob.mover.wanted_x - me.position.x;
            let dz = mob.mover.wanted_z - me.position.z;
            let dy = mob.mover.wanted_y - me.position.y;
            let distance = dx * dx + dy * dy + dz * dz;
            if distance < 2.500_000_277_905_201e-7 {
                mob.zza = 0.0;
                return;
            }
            let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
            me.y_rot = move_rotlerp(me.y_rot, yaw, 90.0);
            let speed =
                (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
            set_speed(mob, speed);
            let pos = me.block_position();
            let state = view.block_state(pos);
            let piston = state
                .is(mc_bamboo_data::blocks::MOVING_PISTON)
                .then(|| view.moving_piston_shape(pos))
                .flatten();
            let boxes = shape::block_collision_boxes(
                state,
                pos,
                CollisionContext::Empty,
                piston.as_deref(),
                view.shulker_box_shape(pos),
            );
            let width = mth::java_max_f32(1.0, me.dimensions.0);
            let step_over =
                dy > f64::from(max_up_step(mob)) && dx * dx + dz * dz < f64::from(width);
            let inside = !boxes.is_empty()
                && me.position.y < super::path::max_y(&boxes) + f64::from(pos.y)
                && control::blocks_jump(state);
            if step_over || inside {
                mob.jump.jump = true;
                mob.mover.operation = Operation::Jumping;
            }
        }
        Operation::Jumping => {
            let speed =
                (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
            set_speed(mob, speed);
            if me.on_ground || is_in_liquid(me, mob) {
                mob.mover.operation = Operation::Wait;
            }
        }
        Operation::Wait | Operation::Strafe => mob.zza = 0.0,
    }
}

/// `LookControl.tick`。
fn look_control_tick(me: &mut Entity, mob: &mut Mob) {
    mob.x_rot = clamp_x_rot(0.0, mob.x_rot);
    if mob.look.look_at_cooldown > 0 {
        mob.look.look_at_cooldown -= 1;
        if let Some(target) = look_y_rot(&mob.look, me.position.x, me.position.z) {
            mob.y_head_rot = rotate_towards(mob.y_head_rot, target, mob.look.y_max_rot_speed);
        }
        if let Some(target) = look_x_rot(&mob.look, me.position.x, eye_y(me, mob), me.position.z) {
            let rotated = rotate_towards(mob.x_rot, target, mob.look.x_max_rot_angle);
            mob.x_rot = clamp_x_rot(rotated, mob.x_rot);
        }
    } else {
        mob.y_head_rot = rotate_towards(mob.y_head_rot, mob.y_body_rot, 10.0);
    }
    if !mob.navigation.is_done() {
        mob.y_head_rot =
            mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, MAX_HEAD_Y_ROT as f32);
    }
}

/// `BodyRotationControl.clientTick`。
fn body_rotation_tick(me: &Entity, mob: &mut Mob) {
    let dx = me.position.x - me.old_position.x;
    let dz = me.position.z - me.old_position.z;
    let max = MAX_HEAD_Y_ROT as f32;
    if dx * dx + dz * dz > 2.500_000_277_905_201e-7 {
        mob.y_body_rot = me.y_rot;
        mob.y_head_rot = mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, max);
        mob.body.last_stable_y_head_rot = mob.y_head_rot;
        mob.body.head_stable_time = 0;
        return;
    }
    if (mob.y_head_rot - mob.body.last_stable_y_head_rot).abs() > 15.0 {
        mob.body.head_stable_time = 0;
        mob.body.last_stable_y_head_rot = mob.y_head_rot;
        mob.y_body_rot = mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, max);
    } else {
        mob.body.head_stable_time += 1;
        if mob.body.head_stable_time > 10 {
            let since = mob.body.head_stable_time - 10;
            let progress = mth::clamp_f32(since as f32 / 10.0, 0.0, 1.0);
            let limit = max * (1.0 - progress);
            mob.y_body_rot = mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, limit);
        }
    }
}

// ---- 跳跃 ----

/// `Mob.jumpInLiquid`：会浮水时加 `(double) 0.04F`。
fn jump_in_liquid(me: &mut Entity, mob: &Mob) {
    let lift = if mob.navigation.can_float() {
        f64::from(0.04_f32)
    } else {
        0.3
    };
    me.delta = me.delta.add(Vec3::new(0.0, lift, 0.0));
}

/// `getBlockJumpFactor`：只有蜂蜜块是 0.5。
fn block_jump_factor(me: &Entity, view: &dyn BlockView) -> f32 {
    let factor = |state: crate::state::BlockState| {
        if state.is(mc_bamboo_data::blocks::HONEY_BLOCK) {
            0.5_f32
        } else {
            1.0
        }
    };
    let here = factor(view.block_state(me.block_position()));
    let below = factor(view.block_state(me.on_pos(view, 0.500_001)));
    if f64::from(here) == 1.0 { below } else { here }
}

/// `jumpFromGround`。
fn jump_from_ground(me: &mut Entity, mob: &Mob, view: &dyn BlockView) {
    let strength = mob.attributes.value(attribute::JUMP_STRENGTH) as f32;
    let power = strength * 1.0 * block_jump_factor(me, view) + 0.0;
    if power <= 1.0e-5 {
        return;
    }
    me.delta = Vec3::new(
        me.delta.x,
        mth::java_max(f64::from(power), me.delta.y),
        me.delta.z,
    );
}

// ---- travel ----

/// `LivingEntity.travel`。
fn travel(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    let in_liquid = is_in_liquid(me, mob);
    if in_liquid {
        travel_in_fluid(me, mob, ctx, input);
    } else {
        travel_in_air(me, mob, ctx, input);
    }
}

/// `computeModifiedFriction`。
fn modified_friction(friction: f32, modifier: f32) -> f32 {
    mth::clamp_f32(1.0 - (1.0 - friction) * modifier, 0.0, 1.0)
}

/// `getEffectiveGravity`：读调用时的速度。
fn effective_gravity(_me: &Entity, mob: &Mob) -> f64 {
    if mob.no_gravity {
        0.0
    } else {
        mob.attributes.value(attribute::GRAVITY)
    }
}

/// `travelInAir`。
fn travel_in_air(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    let below = me.on_pos(ctx.blocks(), 0.500_001);
    let block_friction = if me.on_ground {
        modified_friction(
            crate::entity::item::friction(ctx.blocks().block_state(below)),
            mob.attributes.value(attribute::FRICTION_MODIFIER) as f32,
        )
    } else {
        1.0
    };
    let movement = relative_friction_movement(me, mob, ctx, input, block_friction);
    let y = movement.y - effective_gravity(me, mob);
    let air_drag = mob.attributes.value(attribute::AIR_DRAG_MODIFIER) as f32;
    let horizontal_air = modified_friction(0.91, air_drag);
    let horizontal = block_friction * horizontal_air;
    let vertical = modified_friction(0.98, air_drag);
    me.delta = Vec3::new(
        movement.x * f64::from(horizontal),
        y * f64::from(vertical),
        movement.z * f64::from(horizontal),
    );
}

/// `handleRelativeFrictionAndCalculateMovement`。
fn relative_friction_movement(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
    friction: f32,
) -> Vec3 {
    let speed = if me.on_ground {
        if f64::from(friction) > 0.6 {
            mob.speed * (0.216_000_02 / (friction * friction * friction))
        } else {
            mob.speed
        }
    } else {
        0.02
    };
    move_relative(me, speed, input);
    me.delta = handle_on_climbable(me, mob, ctx.blocks(), me.delta);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    let mut delta = me.delta;
    if (me.horizontal_collision || mob.jumping) && on_climbable(me, mob, ctx.blocks()) {
        delta = Vec3::new(delta.x, 0.2, delta.z);
    }
    delta
}

/// `Entity.moveRelative`。
fn move_relative(me: &mut Entity, speed: f32, input: Vec3) {
    let length = input.length_sqr();
    if length < 1.0e-7 {
        return;
    }
    let scaled = if length > 1.0 {
        input.normalize()
    } else {
        input
    }
    .scale(f64::from(speed));
    let angle = f64::from(me.y_rot * 0.017_453_292);
    let sin = f64::from(mth::sin(angle));
    let cos = f64::from(mth::cos(angle));
    let push = Vec3::new(
        scaled.x * cos - scaled.z * sin,
        scaled.y,
        scaled.z * cos + scaled.x * sin,
    );
    me.delta = me.delta.add(push);
}

/// `LivingEntity.onClimbable`：可攀爬的方块，或下面是同向梯子的开着的活板门。
pub(crate) fn on_climbable(me: &Entity, mob: &mut Mob, view: &dyn BlockView) -> bool {
    let pos = me.block_position();
    let state = view.block_state(pos);
    if state.is_tagged(block_tags::CLIMBABLE) {
        mob.last_climbable_pos = Some(pos);
        return true;
    }
    if state.is_a(mc_bamboo_data::BlockAncestor::TrapDoorBlock)
        && state.try_get(crate::state::OPEN) == Some(true)
    {
        let below = view.block_state(pos.below());
        if below.is(mc_bamboo_data::blocks::LADDER)
            && below.try_get(crate::state::HORIZONTAL_FACING)
                == state.try_get(crate::state::HORIZONTAL_FACING)
        {
            mob.last_climbable_pos = Some(pos);
            return true;
        }
    }
    false
}

fn handle_on_climbable(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView, delta: Vec3) -> Vec3 {
    if !on_climbable(me, mob, view) {
        return delta;
    }
    me.fall_distance = 0.0;
    let limit = f64::from(0.15_f32);
    let x = mth::clamp(delta.x, -limit, limit);
    let z = mth::clamp(delta.z, -limit, limit);
    let y = mth::java_max(delta.y, -limit);
    Vec3::new(x, y, z)
}

/// `travelInFluid`。
fn travel_in_fluid(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    let falling = me.delta.y <= 0.0;
    let old_y = me.position.y;
    let gravity = effective_gravity(me, mob);
    if me.in_water {
        let slow = 0.8_f32;
        let speed = 0.02_f32;
        let mut efficiency = mob.attributes.value(attribute::WATER_MOVEMENT_EFFICIENCY) as f32;
        if !me.on_ground {
            efficiency *= 0.5;
        }
        let (slow, speed) = if efficiency > 0.0 {
            (
                slow + (0.546_000_06 - slow) * efficiency,
                speed + (mob.speed - speed) * efficiency,
            )
        } else {
            (slow, speed)
        };
        move_relative(me, speed, input);
        let movement = me.delta;
        motion::move_living(me, mob, ctx, movement);
        let mut delta = me.delta;
        if me.horizontal_collision && on_climbable(me, mob, ctx.blocks()) {
            delta = Vec3::new(delta.x, 0.2, delta.z);
        }
        delta = delta.multiply(f64::from(slow), f64::from(0.8_f32), f64::from(slow));
        me.delta = fluid_falling_adjusted(gravity, falling, delta);
    } else {
        move_relative(me, 0.02, input);
        let movement = me.delta;
        motion::move_living(me, mob, ctx, movement);
        if fluid_height(mob, fluid_tags::LAVA) <= fluid_jump_threshold(mob) {
            me.delta = me.delta.multiply(0.5, f64::from(0.8_f32), 0.5);
            me.delta = fluid_falling_adjusted(gravity, falling, me.delta);
        } else {
            me.delta = me.delta.scale(0.5);
        }
        if gravity != 0.0 {
            me.delta = me.delta.add(Vec3::new(0.0, -gravity / 4.0, 0.0));
        }
    }
    jump_out_of_fluid(me, mob, ctx.blocks(), old_y);
}

/// `getFluidFallingAdjustedMovement`。
fn fluid_falling_adjusted(gravity: f64, falling: bool, delta: Vec3) -> Vec3 {
    if gravity == 0.0 {
        return delta;
    }
    let y = if falling
        && (delta.y - 0.005).abs() >= 0.003
        && (delta.y - gravity / 16.0).abs() < 0.003
    {
        -0.003
    } else {
        delta.y - gravity / 16.0
    };
    Vec3::new(delta.x, y, delta.z)
}

/// `jumpOutOfFluid`。
fn jump_out_of_fluid(me: &mut Entity, mob: &Mob, view: &dyn BlockView, old_y: f64) {
    let delta = me.delta;
    if !me.horizontal_collision {
        return;
    }
    let dy = delta.y + f64::from(0.6_f32) - me.position.y + old_y;
    let moved = me.bounding_box().moved(delta.x, dy, delta.z);
    if motion::is_free(me, mob, view, moved) {
        me.delta = Vec3::new(delta.x, f64::from(0.3_f32), delta.z);
    }
}

// ---- 推挤 ----

/// `LivingEntity.pushEntities`：身边的生物互相推开。
fn push_entities(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let bounds = me.bounding_box();
    let mut others = Vec::new();
    ctx.for_each_other_in(bounds, &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && !other.removed
            && public.is_alive()
        {
            others.push((other.uid, other.position));
        }
    });
    if others.is_empty() {
        return;
    }
    let cramming = ctx.blocks().block_game_rules().max_entity_cramming;
    if cramming > 0 && others.len() as i32 > cramming - 1 && me.random.next_int_bounded(4) == 0 {
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Cramming),
            6.0,
        );
    }
    let pushable_self = is_alive(me, mob) && !on_climbable(me, mob, ctx.blocks());
    for (uid, position) in others {
        ctx.emit(
            Command::PushMob {
                other: uid,
                from: position,
                self_pushable: pushable_self,
            },
            me,
        );
    }
}

/// `Entity.push(Entity)` 的数学部分：返回推开的水平量。
pub(crate) fn push_amount(from: Vec3, to: Vec3) -> Option<(f64, f64)> {
    let mut dx = to.x - from.x;
    let mut dz = to.z - from.z;
    let mut max = dx.abs().max(dz.abs());
    if max < f64::from(0.01_f32) {
        return None;
    }
    max = max.sqrt();
    dx /= max;
    dz /= max;
    let mut inverse = 1.0 / max;
    if inverse > 1.0 {
        inverse = 1.0;
    }
    dx *= inverse;
    dz *= inverse;
    dx *= f64::from(0.05_f32);
    dz *= f64::from(0.05_f32);
    Some((dx, dz))
}

// ---- 视线 ----

/// `Sensing.hasLineOfSight` 加 `LivingEntity.hasLineOfSight`：从眼睛到目标眼睛，碰撞形状挡住就看不见。
pub(crate) fn has_line_of_sight(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    uid: crate::entity::EntityUid,
    target_eye: Vec3,
) -> bool {
    if let Some(&(_, seen)) = mob.seen.iter().find(|(seen_uid, _)| *seen_uid == uid) {
        return seen;
    }
    let from = Vec3::new(me.position.x, eye_y(me, mob), me.position.z);
    let seen = if target_eye.distance_to_sqr(from).sqrt() > 128.0 {
        false
    } else {
        view.clip_collider(from, target_eye, me.collision_context())
            .is_none()
    };
    mob.seen.push((uid, seen));
    seen
}

// ---- 受伤和死亡 ----

/// `LivingEntity.hurtServer`。
pub(crate) fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    // `isInvulnerableTo`。
    if me.removed
        || ((mob.invulnerable || mob.invulnerable_time > 0)
            && !source.kind.bypasses_invulnerability()
            && !source.creative_player)
    {
        return false;
    }
    if mob.is_dead_or_dying() {
        return false;
    }
    mob.no_action_time = 0;
    let mut amount = if amount < 0.0 { 0.0 } else { amount };
    if amount.is_nan() || amount.is_infinite() {
        amount = f32::MAX;
    }
    let full_hit = if mob.damage_cooldown as f32 > 10.0 {
        if amount <= mob.last_hurt {
            return false;
        }
        actually_hurt(me, mob, source, amount - mob.last_hurt);
        mob.last_hurt = amount;
        false
    } else {
        mob.last_hurt = amount;
        mob.damage_cooldown = 20;
        actually_hurt(me, mob, source, amount);
        mob.hurt_duration = 10;
        mob.hurt_time = mob.hurt_duration;
        true
    };
    if let Some(attacker) = source.attacker {
        mob.last_hurt_by_mob = Some(attacker);
        mob.last_hurt_by_mob_ref = source.attacker_uuid;
        mob.last_hurt_by_mob_timestamp = me.tick_count;
        if source.attacker_is_player {
            mob.last_hurt_by_player = Some(attacker);
            mob.last_hurt_by_player_ref = source.attacker_uuid;
            mob.last_hurt_by_player_memory_time = 100;
        }
    }
    if full_hit {
        mob.events.push(MobEvent::Damage {
            kind: source.kind,
            attacker: source.attacker,
        });
        if !source.kind.no_impact() {
            mob.hurt_marked = true;
        }
        if !source.kind.no_knockback() {
            let (dx, dz) = source.position.map_or((0.0, 0.0), |position| {
                (position.x - me.position.x, position.z - me.position.z)
            });
            knockback(me, mob, f64::from(0.4_f32), dx, dz);
        }
    }
    if mob.is_dead_or_dying() {
        if full_hit {
            let pitch = voice_pitch(me, mob);
            play_sound(me, mob, mob.sound_variant.death(), 1.0, pitch);
        }
        die(me, mob, ctx, source);
    } else if full_hit {
        // `Mob.playHurtSound`：先重置叫声计时，再取音高。
        mob.ambient_sound_time = -120;
        let pitch = voice_pitch(me, mob);
        play_sound(me, mob, mob.sound_variant.hurt(), 1.0, pitch);
    }
    mob.last_damage_source = Some(source);
    mob.last_damage_stamp = ctx.blocks().game_time();
    true
}

/// `Animal.actuallyHurt` → `LivingEntity.actuallyHurt`。没有护甲、效果和吸收。
fn actually_hurt(me: &mut Entity, mob: &mut Mob, source: DamageSource, amount: f32) {
    mob.in_love = 0;
    let _ = me;
    let mut amount = amount;
    if !source.kind.bypasses_armor() {
        let armor = mth::floor(mob.attributes.value(attribute::ARMOR)) as f32;
        let toughness = mob.attributes.value(attribute::ARMOR_TOUGHNESS) as f32;
        let factor = 2.0 + toughness / 4.0;
        let effective = mth::clamp_f32(armor - amount / factor, armor * 0.2, 20.0);
        amount *= 1.0 - effective / 25.0;
    }
    if amount <= 0.0 {
        amount = 0.0;
    }
    let before = amount;
    amount = mth::java_max_f32(amount - mob.absorption, 0.0);
    mob.absorption = (mob.absorption - (before - amount)).max(0.0);
    if amount == 0.0 {
        return;
    }
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    mob.health = mth::clamp_f32(mob.health - amount, 0.0, max);
}

/// `LivingEntity.knockback`。
pub(crate) fn knockback(me: &mut Entity, mob: &Mob, strength: f64, dx: f64, dz: f64) {
    let strength = strength * (1.0 - mob.attributes.value(attribute::KNOCKBACK_RESISTANCE));
    if strength <= 0.0 {
        return;
    }
    let (mut dx, mut dz) = (dx, dz);
    let delta = me.delta;
    while dx * dx + dz * dz < f64::from(1.0e-5_f32) {
        dx = (me.random.next_double() - me.random.next_double()) * 0.01;
        dz = (me.random.next_double() - me.random.next_double()) * 0.01;
    }
    let push = Vec3::new(dx, 0.0, dz).normalize().scale(strength);
    me.delta = Vec3::new(
        delta.x / 2.0 - push.x,
        if me.on_ground {
            mth::java_min(0.4, delta.y / 2.0 + strength)
        } else {
            delta.y
        },
        delta.z / 2.0 - push.z,
    );
}

/// `LivingEntity.die`：掉落交给 [`Command::DropMobLoot`]，经验只在被玩家打过时掉。
fn die(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, source: DamageSource) {
    if me.removed || mob.dead {
        return;
    }
    mob.dead = true;
    let by_player = mob.last_hurt_by_player_memory_time > 0;
    let adult = !mob.is_baby();
    let drops = ctx.blocks().block_game_rules().mob_drops;
    let experience = if by_player && adult && drops {
        // `Animal.getBaseExperienceReward`：`1 + random.nextInt(3)`。
        1 + me.random.next_int_bounded(3)
    } else {
        0
    };
    mob.events.push(MobEvent::Death);
    if adult && drops {
        ctx.emit(
            Command::DropMobLoot {
                table: "minecraft:entities/cow",
                on_fire: me.remaining_fire_ticks > 0,
                killer: source.attacker,
                experience,
            },
            me,
        );
    } else if experience > 0 {
        ctx.emit(
            Command::DropMobLoot {
                table: "",
                on_fire: false,
                killer: source.attacker,
                experience,
            },
            me,
        );
    }
}

/// `FluidTracker` 的构造，给 [`super::motion`] 用。
pub(crate) fn tracker_for(trackers: &mut Vec<FluidTracker>, fluid: i32) -> usize {
    if let Some(index) = trackers.iter().position(|tracker| tracker.fluid == fluid) {
        return index;
    }
    trackers.push(FluidTracker {
        fluid,
        height: 0.0,
        eyes_inside: false,
    });
    trackers.len() - 1
}

/// [`Command::PushMob`] 的落地：`other.push(me)`，对方先被推开，自己再被推回来。
pub(crate) fn apply_push(
    me: &mut Entity,
    other: crate::entity::EntityUid,
    from: Vec3,
    self_pushable: bool,
    world: &mut crate::world::World,
) {
    let _ = from;
    let Some(target) = world.entity(other) else {
        return;
    };
    let Some((dx, dz)) = push_amount(target.position, me.position) else {
        return;
    };
    let other_pushable =
        matches!(&target.kind, EntityKind::Mob(mob) if mob.health > 0.0) && !target.removed;
    if other_pushable {
        world.with_entity_mut(other, &mut |entity| {
            entity.delta = entity.delta.add(Vec3::new(-dx, 0.0, -dz));
        });
    }
    if self_pushable {
        me.delta = me.delta.add(Vec3::new(dx, 0.0, dz));
    }
}

/// [`Command::DropMobLoot`] 的落地：战利品表交给世界，经验球按 `ExperienceOrb.award`
/// 拆成几个，每个先取一次 `level.random.nextInt(40)`（`tryMergeToExisting`）。
pub(crate) fn apply_death_drops(
    me: &mut Entity,
    table: &'static str,
    on_fire: bool,
    killer: Option<crate::entity::EntityUid>,
    experience: i32,
    world: &mut crate::world::World,
) {
    if !table.is_empty() {
        for stack in world.mob_loot_drops(table, on_fire, killer) {
            if !stack.is_empty() {
                world.spawn_at_location(me.position, stack);
            }
        }
    }
    let mut left = experience;
    while left > 0 {
        let value = experience_value(left);
        left -= value;
        let _ = world.random().next_int(40);
        // 现在没有经验球实体：取数照官方，球不生成。
        // There is no experience-orb entity yet: the draw happens, no orb spawns.
    }
}

/// `ExperienceOrb.getExperienceValue`。
fn experience_value(amount: i32) -> i32 {
    [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3]
        .into_iter()
        .find(|&value| amount >= value)
        .unwrap_or(1)
}
