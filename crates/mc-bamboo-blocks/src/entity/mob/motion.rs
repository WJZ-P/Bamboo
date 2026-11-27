//! 生物的移动和方块效果：`Entity.move`（带上台阶）、`LivingEntity.checkFallDamage`、
//! 脚步声的随机数、`applyEffectsFromBlocks` 加上分步的效果收集器、流体扫描。
//!
//! Mob movement and block effects: `Entity.move` with step-up, living fall damage,
//! step-sound randomness, `applyEffectsFromBlocks` with the step-based collector, and
//! the fluid scan.

use mc_bamboo_core::{Aabb, Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{
    BlockAncestor, blocks,
    tags::{block_tags, fluid_tags},
};

use super::{DamageKind, DamageSource, Mob, living};
use crate::{
    entity::{
        Entity, Movement, axis_component,
        ctx::{BlockView, Command, EntityCtx},
        item::{apply_current, flow_vector, fluid_box},
        landing,
        physics::{for_each_block_intersected_between, offset_along, sweep_movement, world_boxes},
        with_axis,
    },
    shape,
    state::{AGE_3, BlockState, FluidState, SupportType},
    world,
};

/// `Mth.equal`。
fn nearly_equal(first: f64, second: f64) -> bool {
    (second - first).abs() < f64::from(1.0e-5_f32)
}

/// `Entity.move(MoverType.SELF, movement)`。
pub(crate) fn move_living(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, movement: Vec3) {
    let mut delta = movement;
    // `Entity.noPhysics`（恼鬼）：直接挪位置、清碰撞标志，不做方块碰撞和摔落伤害。
    // `Entity.noPhysics` (the vex): move straight through and clear the collision flags.
    if mob.no_physics {
        me.position = me.position.add(delta);
        me.horizontal_collision = false;
        me.vertical_collision = false;
        me.vertical_collision_below = false;
        me.on_ground = false;
        me.movement_this_tick.push(Movement {
            from: me.position.subtract(delta),
            to: me.position,
            original: Some(delta),
        });
        return;
    }
    if me.stuck_speed_multiplier.length_sqr() > 1.0e-7 {
        delta = delta.multiply(
            me.stuck_speed_multiplier.x,
            me.stuck_speed_multiplier.y,
            me.stuck_speed_multiplier.z,
        );
        me.stuck_speed_multiplier = Vec3::default();
        me.delta = Vec3::default();
    }
    let collided = {
        let view = ctx.blocks();
        sweep_movement(
            delta,
            me.bounding_box(),
            living::max_up_step(mob),
            me.on_ground,
            me.mob_collision_context(mob),
            world::CollisionBlocks {
                block_at: |pos| view.block_state(pos),
                piston_at: |pos| view.moving_piston_shape(pos),
                shulker_at: |pos| view.shulker_box_shape(pos),
            },
            &view.world_border(),
        )
    };
    let length = collided.length_sqr();
    if length > 1.0e-7 || delta.length_sqr() - length < 1.0e-7 {
        if me.fall_distance != 0.0 && length >= 1.0 {
            let check = collided.length().min(8.0);
            let to = me.position.add(collided.normalize().scale(check));
            if ctx.blocks().clip_fall_damage_resetting(me.position, to) {
                me.fall_distance = 0.0;
            }
        }
        let from = me.position;
        let to = from.add(collided);
        if me.movement_this_tick.len() >= 100 {
            let first = me.movement_this_tick.remove(0);
            let second = me.movement_this_tick.remove(0);
            me.movement_this_tick.insert(
                0,
                Movement {
                    from: first.from,
                    to: second.to,
                    original: None,
                },
            );
        }
        me.movement_this_tick.push(Movement {
            from,
            to,
            original: Some(delta),
        });
        me.position = to;
    }
    let x_collision = !nearly_equal(delta.x, collided.x);
    let z_collision = !nearly_equal(delta.z, collided.z);
    me.horizontal_collision = x_collision || z_collision;
    let moved_vertically = delta.y.abs() > 0.0;
    me.vertical_collision = delta.y != collided.y;
    me.vertical_collision_below = me.vertical_collision && delta.y < 0.0;
    me.on_ground = me.vertical_collision_below;
    let on_ground = me.on_ground;
    me.check_supporting_block(ctx.blocks(), on_ground, Some(collided));
    mob.minor_horizontal_collision = false;
    let legacy_pos = me.on_pos(ctx.blocks(), 0.2);
    let legacy_state = ctx.blocks().block_state(legacy_pos);
    check_fall_damage(me, mob, ctx, collided.y, legacy_state, legacy_pos);
    if me.removed {
        return;
    }
    if moved_vertically && me.vertical_collision || me.horizontal_collision {
        restitute(
            me,
            mob,
            ctx,
            legacy_state,
            x_collision,
            z_collision,
            collided,
        );
    }
    movement_emission(me, mob, ctx, collided, legacy_pos, legacy_state);
    let factor = mth::lerp_f32(
        mob.attributes
            .value(mc_bamboo_data::registries::attribute::MOVEMENT_EFFICIENCY) as f32,
        me.block_speed_factor(ctx.blocks()),
        1.0,
    );
    me.delta = me.delta.multiply(f64::from(factor), 1.0, f64::from(factor));
}

/// `LivingEntity.checkFallDamage` 加 `Entity.checkFallDamage`。
fn check_fall_damage(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    ya: f64,
    state: BlockState,
    pos: BlockPos,
) {
    // `Parrot.checkFallDamage`、`Bat.checkFallDamage` 和幻翼的都是空实现：不掉血，
    // 不累计、不清零摔落距离，也不补那次流体更新。
    // The parrot, bat and phantom `checkFallDamage` overrides are empty: no fall
    // damage, no fall distance bookkeeping and no fluid update.
    if mob.mob_type.ignores_fall_damage_check() {
        return;
    }
    if !me.in_water {
        update_fluid_interaction(me, mob, ctx.blocks());
    }
    if me.on_ground
        && me.fall_distance > 0.0
        && let Some((position, count)) = landing::fall_particles(
            me.position,
            pos,
            state,
            me.fall_distance + 1.0e-6
                - mob
                    .attributes
                    .value(mc_bamboo_data::registries::attribute::SAFE_FALL_DISTANCE),
        )
    {
        mob.events.push(super::MobEvent::Particles {
            particle: mc_bamboo_data::registries::particle_type::BLOCK,
            item: Some(state.0 as i32),
            position,
            offset: Vec3::default(),
            speed: 0.15,
            count,
        });
    }
    if !me.in_water && ya < 0.0 {
        me.fall_distance -= f64::from(ya as f32);
    }
    if me.on_ground {
        if me.fall_distance > 0.0 {
            let distance = me.fall_distance;
            fall_on(me, mob, ctx, pos, state, distance);
            ctx.emit(
                Command::HitGround {
                    supporting: me.main_supporting_block,
                    fallback: state,
                },
                me,
            );
        }
        me.fall_distance = 0.0;
        mob.last_climbable_pos = None;
    }
}

/// `Block.fallOn` 按方块分发。
fn fall_on(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    pos: BlockPos,
    state: BlockState,
    distance: f64,
) {
    if state.is(blocks::TURTLE_EGG) {
        if !mob.mob_type.is_zombie() {
            destroy_turtle_egg(me, ctx, pos, 3);
        }
    } else if state.is(blocks::FARMLAND) {
        let roll = ctx.blocks().level_random_float();
        if f64::from(roll) < distance - 0.5
            && ctx.blocks().block_game_rules().mob_griefing
            && landing::can_trample_farmland(me.dimensions.0, me.dimensions.1)
        {
            ctx.emit(Command::TrampleFarmland { pos }, me);
        }
    } else if state.is(blocks::HONEY_BLOCK) {
        living::play_sound(me, mob, "minecraft:block.honey_block.slide", 1.0, 1.0);
        mob.events.push(super::MobEvent::EntityEvent { event: 54 });
    } else if state.is_a(BlockAncestor::PowderSnowBlock) && distance >= 4.0 {
        play_fall_sound(me, mob, distance >= 7.0);
    }
    if let Some((distance, multiplier, kind)) = landing::fall_damage(state, distance, false) {
        let damage = cause_fall_damage(me, mob, ctx, distance, multiplier, kind);
        if damage && state.is(blocks::HONEY_BLOCK) {
            play_block_fall_sound(me, mob, state);
        }
    }
}

fn play_fall_sound(me: &Entity, mob: &mut Mob, big: bool) {
    let sound = match (mob.mob_type.is_monster(), big) {
        (true, true) => "minecraft:entity.hostile.big_fall",
        (true, false) => "minecraft:entity.hostile.small_fall",
        (false, true) => "minecraft:entity.generic.big_fall",
        (false, false) => "minecraft:entity.generic.small_fall",
    };
    living::play_sound(me, mob, sound, 1.0, 1.0);
}

fn play_block_fall_sound(me: &Entity, mob: &mut Mob, state: BlockState) {
    if !state.is_air() {
        let sound = state.sound_type();
        let name = mc_bamboo_data::sounds::SOUND_EVENTS[usize::from(sound.fall_sound)].name;
        living::play_sound(me, mob, name, sound.volume * 0.5, sound.pitch * 0.75);
    }
}

/// `LivingEntity.causeFallDamage`。
fn cause_fall_damage(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    distance: f64,
    multiplier: f32,
    kind: DamageKind,
) -> bool {
    use mc_bamboo_data::registries::attribute;
    if mob.mob_type.is_equine() {
        // 马科覆写的 `causeFallDamage` 先播落地声，伤害结算相同。
        // The equine override plays its landing sound first; damage is unchanged.
        super::equine::land_sound(me, mob, distance);
    }
    // `Breeze.causeFallDamage`：摔落超过 3 格先放 `BREEZE_LAND`（固定音量 / 音高），
    // 再走 `super`。`FALL_DAMAGE_IMMUNE` 标签让 `calculateFallDamage` 恒 0。
    // `Breeze.causeFallDamage`: a fall longer than 3 blocks plays `BREEZE_LAND` at a
    // fixed volume and pitch before the super call; the `FALL_DAMAGE_IMMUNE` tag makes
    // `calculateFallDamage` always return 0.
    if mob.mob_type == super::MobType::Breeze && distance > 3.0 {
        living::play_sound(me, mob, "minecraft:entity.breeze.land", 1.0, 1.0);
    }
    if mob.mob_type.fall_damage_immune() {
        return false;
    }
    let power = distance + 1.0e-6 - mob.attributes.value(attribute::SAFE_FALL_DISTANCE);
    let mut damage = mth::floor(
        power * f64::from(multiplier) * mob.attributes.value(attribute::FALL_DAMAGE_MULTIPLIER),
    );
    if mob.mob_type == super::MobType::Goat {
        // `Goat.calculateFallDamage`：少算 10 点。
        // `Goat.calculateFallDamage`: ten points less.
        damage -= 10;
    }
    if mob.mob_type == super::MobType::Frog {
        // `Frog.calculateFallDamage`：少算 5 点。
        // `Frog.calculateFallDamage`: five points less.
        damage -= 5;
    }
    if damage > 0 {
        play_fall_sound(me, mob, damage > 4);
        let pos = landing::on_pos_legacy(me.position, None, |_| BlockState(blocks::AIR));
        play_block_fall_sound(me, mob, ctx.blocks().block_state(pos));
        living::hurt_server(me, mob, ctx, DamageSource::environment(kind), damage as f32);
    }
    if mob.mob_type == super::MobType::Creeper {
        super::creeper::after_fall(mob, distance);
    }
    damage > 0
}

/// `restituteMovementAfterCollisions`：生物拿方块的整份弹性，弹性属性默认是 0。
fn restitute(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    state: BlockState,
    x_collision: bool,
    z_collision: bool,
    movement: Vec3,
) {
    use mc_bamboo_data::registries::attribute;
    let mut restitution = mob.attributes.value(attribute::BOUNCINESS);
    let current = me.delta;
    let mut after = current;
    if x_collision {
        after = with_axis(after, Axis::X, -current.x * restitution);
    }
    if z_collision {
        after = with_axis(after, Axis::Z, -current.z * restitution);
    }
    let gravity = if mob.no_gravity {
        0.0
    } else {
        mob.attributes.value(attribute::GRAVITY)
    };
    let mut bounced = restitution > 0.0 && (x_collision || z_collision);
    if me.vertical_collision {
        if me.vertical_collision_below {
            restitution = if -current.y <= gravity || state.is_tagged(block_tags::SUPPRESSES_BOUNCE)
            {
                0.0
            } else {
                mth::java_max(restitution, f64::from(state.block().bounce_restitution))
            };
        }
        let (compensation, drag) = if restitution > 0.0 {
            bounced = true;
            let portion = movement.y / current.y;
            let air = mth::clamp_f32(
                1.0 - (1.0 - 0.98) * mob.attributes.value(attribute::AIR_DRAG_MODIFIER) as f32,
                0.0,
                1.0,
            );
            (portion * gravity, mth::lerp(portion, 1.0, f64::from(air)))
        } else {
            (0.0, 1.0)
        };
        after = with_axis(
            after,
            Axis::Y,
            (compensation - current.y) * drag * restitution,
        );
    }
    if bounced {
        if state.is(blocks::SHELF_MUSHROOM) {
            let pos = me.on_pos(ctx.blocks(), 0.500001);
            ctx.emit(
                Command::MobBlockSound {
                    pos,
                    sound: "minecraft:block.shelf_mushroom.bounce",
                    source: mc_bamboo_data::sounds::BLOCKS,
                    volume: 1.0,
                    pitch: 1.0,
                },
                me,
            );
        }
        ctx.emit(
            Command::GameEvent {
                event: crate::game_event::GameEvent::Bounce,
                position: me.position,
                context: crate::game_event::dispatch::EventContext {
                    source_entity: Some(me.id),
                    ..Default::default()
                },
            },
            me,
        );
        mob.needs_sync = true;
    }
    me.delta = after;
}

/// `applyMovementEmissionAndPlaySound`：只有紫水晶脚步和游泳声取随机数。
fn movement_emission(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn super::super::ctx::EntityCtx,
    collided: Vec3,
    legacy_pos: BlockPos,
    legacy_state: BlockState,
) {
    let length = (collided.length() * f64::from(0.6_f32)) as f32;
    let horizontal =
        ((collided.x * collided.x + collided.z * collided.z).sqrt() * f64::from(0.6_f32)) as f32;
    let (on_pos, on_state) = {
        let view = ctx.blocks();
        let on_pos = me.on_pos(view, 1.0e-5);
        (on_pos, view.block_state(on_pos))
    };
    let climbing = on_state.is_tagged(block_tags::CLIMBABLE) || on_state.is(blocks::POWDER_SNOW);
    mob.move_dist += if climbing { length } else { horizontal };
    mob.fly_dist += length;
    let mut step_event = None;
    if mob.move_dist > mob.next_step && !on_state.is_air() {
        let same = on_pos == legacy_pos;
        // `MovementEmission.EVENTS`（鱿鱼）只发游戏事件：没有脚步声，也不取游泳声的随机数。
        // `MovementEmission.EVENTS` (squid) only emits game events: no step sound and no
        // swim-sound draws.
        let sounds = mob.mob_type.emits_movement_sounds();
        let stepped = {
            let view = ctx.blocks();
            let mut stepped =
                step_effects(me, mob, view, legacy_pos, legacy_state, sounds, collided);
            if !same {
                stepped |= step_effects(me, mob, view, on_pos, on_state, false, collided);
            }
            stepped
        };
        if stepped {
            step_event = Some((me.position, legacy_state));
        }
        if stepped {
            mob.next_step = next_step_value(mob);
        } else if me.in_water {
            mob.next_step = next_step_value(mob);
            // `waterSwimSound` → `playSwimSound`：音量按速度算，音高取两个 `nextFloat`。
            // `waterSwimSound` → `playSwimSound`: the volume follows the speed and the
            // pitch draws two `nextFloat`s.
            if sounds {
                let delta = me.delta;
                let speed = (delta.x * delta.x * 0.2 + delta.y * delta.y + delta.z * delta.z * 0.2)
                    .sqrt() as f32
                    * 0.35;
                let mut volume = mth::java_min_f32(1.0, speed);
                // `Turtle.playSwimSound`：音量乘 1.5。
                // `Turtle.playSwimSound` scales the volume by 1.5.
                if super::turtle::is_turtle(mob.mob_type) {
                    volume *= 1.5;
                }
                let first = me.random.next_float();
                let second = me.random.next_float();
                let pitch = 1.0 + (first - second) * 0.4;
                super::living::play_sound(me, mob, mob.mob_type.swim_sound(), volume, pitch);
            }
        }
    } else if on_state.is_air() && mob.mob_type == super::MobType::Parrot {
        // `Parrot.isFlapping()` 加 `onFlap()`：脚下是空气时按飞行距离放翅膀声。
        // `Parrot.isFlapping()` and `onFlap()`: above air, play the wing sound per distance.
        if mob.fly_dist > mob.parrot.next_flap {
            super::living::play_sound(me, mob, "minecraft:entity.parrot.fly", 0.15, 1.0);
            mob.parrot.next_flap = mob.fly_dist + mob.parrot.flap_speed / 2.0;
        }
    }

    if let Some((position, affected_state)) = step_event {
        ctx.emit(
            super::super::ctx::Command::GameEvent {
                event: crate::game_event::GameEvent::Step,
                position,
                context: crate::game_event::dispatch::EventContext {
                    affected_state: Some(affected_state),
                    ..Default::default()
                },
            },
            me,
        );
    }
}

/// `vibrationAndSoundEffectsFromBlock`。
/// `Entity.nextStep()`：海龟覆写成 `moveDist + 0.15F`，其余是 `floor(moveDist) + 1`。
///
/// `Entity.nextStep()`: turtles override it with `moveDist + 0.15F`.
fn next_step_value(mob: &Mob) -> f32 {
    if super::turtle::is_turtle(mob.mob_type) {
        super::turtle::next_step(mob.move_dist)
    } else {
        (mob.move_dist as i32 + 1) as f32
    }
}

fn step_effects(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    pos: BlockPos,
    state: BlockState,
    play_sound: bool,
    collided: Vec3,
) -> bool {
    let _ = collided;
    if state.is_air() {
        return false;
    }
    let climbable = state.is_tagged(block_tags::CLIMBABLE) || state.is(blocks::POWDER_SNOW);
    if !(me.on_ground || climbable) {
        return false;
    }
    if play_sound && mob.mob_type == super::MobType::CopperGolem {
        // `CopperGolem.playStepSound` 用的是自己的脚步声，音量 1.0（不是通用的 0.15）。
        // `CopperGolem.playStepSound` plays its own step sound at volume 1.0, not 0.15.
        super::living::play_sound(
            me,
            mob,
            super::copper_golem::step_sound(mob.copper_golem.weather_state),
            1.0,
            1.0,
        );
    } else if play_sound && mob.mob_type == super::MobType::Slime {
        // 史莱姆没有覆写 `playStepSound`：放脚下方块的脚步声，音量乘 0.15。
        // Slimes do not override `playStepSound`: the block's own step sound at 0.15 volume.
        let sounds = state.sound_type();
        let name = mc_bamboo_data::sounds::SOUND_EVENTS[usize::from(sounds.step_sound)].name;
        super::living::play_sound(me, mob, name, sounds.volume * 0.15, sounds.pitch);
    } else if play_sound && !mob.mob_type.is_water_animal() && mob.mob_type != super::MobType::Bee {
        // `AbstractCow.playStepSound`：`playSound(stepSound, 0.15F, 1.0F)`。
        // 马科按自己的 `playStepSound`（方块声音类型、奔跑步声）走。
        // 苦力怕和末影人走 `Entity.playStepSound`（方块 `SoundType` 的脚步声）。
        // `AbstractFish.playStepSound` 是空实现，但这一步仍算「踩过」。
        // `Wolf.playStepSound` 用叫声品种里的脚步声。
        //
        // `AbstractCow.playStepSound`: `playSound(stepSound, 0.15F, 1.0F)`.
        // Equines go through their own `playStepSound` (block sound type, gallop).
        // Creepers and endermen use `Entity.playStepSound` (the block `SoundType` step sound).
        // `AbstractFish.playStepSound` is empty yet the step still counts.
        // `Wolf.playStepSound` uses the step sound of its sound set.
        if mob.mob_type.is_equine() {
            super::equine::play_step_sound(me, mob, view, pos, state);
        } else {
            let sound = match mob.mob_type {
                super::MobType::Turtle => super::turtle::step_sound(mob.is_baby()),
                super::MobType::Wolf => super::wolf::sound(mob, super::wolf::Sound::Step),
                super::MobType::Pig => super::pig::step_sound(mob.is_baby()),
                super::MobType::Chicken => super::chicken::sounds(mob)[3],
                kind => kind.step_sound().unwrap_or(mob.sound_variant.step()),
            };
            if mob.mob_type.uses_block_step_sound() {
                // `Entity.playStepSound`：方块 `SoundType` 的脚步声，音量乘 0.15。
                // `Entity.playStepSound`: the block `SoundType` step sound at 0.15 volume.
                let kind = state.sound_type();
                let sound = mc_bamboo_data::SOUND_EVENTS[usize::from(kind.step_sound)].name;
                super::living::play_sound(me, mob, sound, kind.volume * 0.15, kind.pitch);
            } else {
                super::living::play_sound(me, mob, sound, 0.15, 1.0);
            }
        }
    }
    if play_sound
        && state.is_tagged(block_tags::CRYSTAL_SOUND_BLOCKS)
        && me.tick_count >= mob.last_crystal_sound_tick + 20
    {
        let elapsed = f64::from(me.tick_count - mob.last_crystal_sound_tick);
        mob.crystal_sound_intensity *= 0.997_f64.powf(elapsed) as f32;
        mob.crystal_sound_intensity = mth::java_min_f32(1.0, mob.crystal_sound_intensity + 0.07);
        me.random.next_float();
        mob.last_crystal_sound_tick = me.tick_count;
    }
    true
}

/// `Entity.isFree(box)`：没有碰撞，也没有液体。
pub(crate) fn is_free(me: &Entity, mob: &Mob, view: &dyn BlockView, bounds: Aabb) -> bool {
    let _ = mob;
    if super::path::has_block_collision(view, bounds, me.mob_collision_context(mob), |pos| {
        view.block_state(pos)
    }) {
        return false;
    }
    // `containsAnyLiquid`。
    let min = BlockPos::new(
        mth::floor(bounds.min_x),
        mth::floor(bounds.min_y),
        mth::floor(bounds.min_z),
    );
    let max = BlockPos::new(
        mth::ceil(bounds.max_x) - 1,
        mth::ceil(bounds.max_y) - 1,
        mth::ceil(bounds.max_z) - 1,
    );
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                if !view.block_state(BlockPos::new(x, y, z)).fluid().is_empty() {
                    return false;
                }
            }
        }
    }
    true
}

// ---- 方块效果 ----

/// `InsideBlockEffectType`，按应用顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectType {
    Freeze = 0,
    ClearFreeze = 1,
    FireIgnite = 2,
    LavaIgnite = 3,
    Extinguish = 4,
}

const EFFECT_ORDER: [EffectType; 5] = [
    EffectType::Freeze,
    EffectType::ClearFreeze,
    EffectType::FireIgnite,
    EffectType::LavaIgnite,
    EffectType::Extinguish,
];

/// 收集器里排着的一个动作。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Effect(EffectType),
    HurtInFire(f32),
    LavaHurt,
}

/// `InsideBlockEffectApplier.StepBasedCollector`。
#[derive(Default)]
struct Collector {
    last_step: i32,
    applied: [bool; 5],
    after: Vec<(EffectType, Action)>,
    finals: Vec<Action>,
}

impl Collector {
    fn new() -> Self {
        Self {
            last_step: -1,
            ..Self::default()
        }
    }

    fn advance_step(&mut self, step: i32) {
        if self.last_step != step {
            self.last_step = step;
            self.flush_step();
        }
    }

    fn flush_step(&mut self) {
        for effect in EFFECT_ORDER {
            if self.applied[effect as usize] {
                self.finals.push(Action::Effect(effect));
            }
            for (owner, action) in &self.after {
                if *owner == effect {
                    self.finals.push(*action);
                }
            }
        }
        self.applied = [false; 5];
        self.after.clear();
    }

    fn apply(&mut self, effect: EffectType) {
        self.applied[effect as usize] = true;
    }

    fn run_after(&mut self, effect: EffectType, action: Action) {
        self.after.push((effect, action));
    }
}

/// `Entity.applyEffectsFromBlocks()` 的整段。
pub(crate) fn apply_effects_from_blocks(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    me.final_movements.clear();
    me.final_movements.append(&mut me.movement_this_tick);
    if me.final_movements.is_empty() {
        me.final_movements.push(Movement {
            from: me.old_position,
            to: me.position,
            original: None,
        });
    } else if let Some(last) = me.final_movements.last()
        && last.to.distance_to_sqr(me.position) > f64::from(9.999_999_4e-11_f32)
    {
        let from = last.to;
        let to = me.position;
        me.final_movements.push(Movement {
            from,
            to,
            original: None,
        });
    }
    if me.removed {
        return;
    }
    if me.on_ground {
        let pos = me.on_pos(ctx.blocks(), 0.2);
        let state = ctx.blocks().block_state(pos);
        step_on(me, mob, ctx, pos, state);
    }
    let was_on_fire = me.remaining_fire_ticks > 0;
    let was_freezing = mob.ticks_frozen > 0;
    let fire_before = me.remaining_fire_ticks;
    let movements = std::mem::take(&mut me.final_movements);
    let mut collector = Collector::new();
    check_inside_blocks(me, mob, ctx, &movements, &mut collector);
    me.final_movements = movements;
    collector.flush_step();
    for action in std::mem::take(&mut collector.finals) {
        if !living::is_alive(me, mob) {
            break;
        }
        run_action(me, mob, ctx, action);
    }
    let raining = {
        let pos = me.block_position();
        let top = BlockPos::containing(Vec3::new(
            f64::from(pos.x),
            me.bounding_box().max_y,
            f64::from(pos.z),
        ));
        ctx.blocks().is_raining_at(pos) || ctx.blocks().is_raining_at(top)
    };
    if raining {
        me.remaining_fire_ticks = me.remaining_fire_ticks.min(0);
    }
    if (was_on_fire && me.remaining_fire_ticks <= 0) || (was_freezing && mob.ticks_frozen <= 0) {
        // `playEntityOnFireExtinguishedSound`：两个 `nextFloat`。
        me.random.next_float();
        me.random.next_float();
    }
    let gained = me.remaining_fire_ticks > fire_before;
    if me.remaining_fire_ticks <= 0 && !gained {
        me.remaining_fire_ticks = 0;
    }
}

/// `Block.stepOn`：岩浆块烫，史莱姆块减速，海龟蛋可能被踩坏。
fn step_on(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    pos: BlockPos,
    state: BlockState,
) {
    if state.is(blocks::TURTLE_EGG) {
        // 生物不会潜行，`isSteppingCarefully` 恒为假。
        destroy_turtle_egg(me, ctx, pos, 100);
    } else if state.is(blocks::MAGMA_BLOCK) {
        living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::HotFloor),
            1.0,
        );
    } else if state.is(blocks::SLIME_BLOCK) {
        let dy = me.delta.y.abs();
        if dy < 0.1 {
            let factor = 0.4 + dy * 0.2;
            me.delta = me.delta.multiply(factor, 1.0, factor);
        }
    }
}

/// `TurtleEggBlock.destroyEgg`：生物（非海龟、蝙蝠、玩家）要 `mobGriefing`；
/// 之后的世界随机数和改方块在命令落地时做。
///
/// `TurtleEggBlock.destroyEgg`: mobs other than turtles, bats and players need
/// `mobGriefing`; the world-random draw and the block change land with the command.
fn destroy_turtle_egg(me: &mut Entity, ctx: &mut dyn EntityCtx, pos: BlockPos, chance: i32) {
    // `canDestroyEgg`：`instanceof Turtle` 和 `instanceof Bat` 都直接返回假。
    // `canDestroyEgg`: `instanceof Turtle` and `instanceof Bat` both return false.
    if me
        .kind
        .mob()
        .is_some_and(|mob| matches!(mob.mob_type, super::MobType::Turtle | super::MobType::Bat))
    {
        return;
    }
    if !ctx.blocks().block_game_rules().mob_griefing {
        return;
    }
    ctx.emit(Command::TrampleTurtleEgg { pos, chance }, me);
}

fn run_action(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, action: Action) {
    match action {
        Action::Effect(EffectType::Freeze) => {
            mob.in_powder_snow = true;
            // `InsideBlockEffectType.FREEZE`：`canFreeze()` 为假时只记「在细雪里」。
            // `InsideBlockEffectType.FREEZE`: without `canFreeze()` only the flag is set.
            if super::living::can_freeze(mob) {
                mob.ticks_frozen = (mob.ticks_frozen + 1).min(140);
            }
        }
        Action::Effect(EffectType::ClearFreeze) => mob.ticks_frozen = 0,
        Action::Effect(EffectType::FireIgnite) => {
            if me.remaining_fire_ticks < 0 {
                me.remaining_fire_ticks += 1;
            }
            if me.remaining_fire_ticks >= 0 {
                ignite_for_ticks(me, mob, mth::floor_f32(8.0 * 20.0));
            }
        }
        Action::Effect(EffectType::LavaIgnite) => {
            ignite_for_ticks(me, mob, mth::floor_f32(15.0 * 20.0));
        }
        Action::Effect(EffectType::Extinguish) => {
            me.remaining_fire_ticks = me.remaining_fire_ticks.min(0);
        }
        Action::HurtInFire(damage) => {
            living::hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::InFire),
                damage,
            );
        }
        Action::LavaHurt => {
            let hurt = living::hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::Lava),
                4.0,
            );
            if hurt && !mob.silent {
                me.random.next_float();
            }
        }
    }
}

/// `Entity.igniteForSeconds(8)` 走 `LivingEntity.igniteForTicks`，再把
/// 燃烧时间属性乘进去。白天燃烧和僵尸着火传染共用这一层。
///
/// `Entity.igniteForSeconds(8)` via `LivingEntity.igniteForTicks`.
pub(crate) fn ignite_for_seconds(me: &mut Entity, mob: &mut Mob, seconds: f32) {
    ignite_for_ticks(me, mob, mth::floor_f32(seconds * 20.0));
}

/// `LivingEntity.igniteForTicks` → `Entity.igniteForTicks`：燃烧时间属性是 1。
fn ignite_for_ticks(me: &mut Entity, mob: &mut Mob, ticks: i32) {
    let burning = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::BURNING_TIME);
    let ticks = mth::ceil(f64::from(ticks) * burning);
    if me.remaining_fire_ticks < ticks {
        me.remaining_fire_ticks = ticks;
    }
    mob.ticks_frozen = 0;
}

/// `checkInsideBlocks(List, collector)`。
fn check_inside_blocks(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    movements: &[Movement],
    collector: &mut Collector,
) {
    let mut visited: Vec<BlockPos> = Vec::new();
    for movement in movements {
        let mut pos = movement.from;
        let delta = movement.to.subtract(movement.from);
        let mut left = 16;
        match movement.original {
            Some(original) if delta.length_sqr() > 0.0 => {
                for axis in Direction::axis_step_order(original) {
                    let amount = axis_component(delta, axis);
                    if amount != 0.0 {
                        let to = offset_along(pos, axis, amount);
                        left -=
                            inside_between(me, mob, ctx, pos, to, &mut visited, left, collector);
                        pos = to;
                    }
                }
            }
            _ => {
                left -= inside_between(
                    me,
                    mob,
                    ctx,
                    movement.from,
                    movement.to,
                    &mut visited,
                    16,
                    collector,
                );
            }
        }
        if left <= 0 {
            inside_between(
                me,
                mob,
                ctx,
                movement.to,
                movement.to,
                &mut visited,
                1,
                collector,
            );
        }
    }
}

/// `checkInsideBlocks(from, to, collector, visited, max)`，返回最后的迭代号加一。
#[expect(clippy::too_many_arguments, reason = "官方方法就是这几个参数")]
fn inside_between(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    from: Vec3,
    to: Vec3,
    visited: &mut Vec<BlockPos>,
    max: i32,
    collector: &mut Collector,
) -> i32 {
    let target = me.make_bounding_box_at(to).deflate(1.0e-5, 1.0e-5, 1.0e-5);
    let long_move =
        from.distance_to_sqr(to) > 0.999_990_000_000_252_6_f64 * 0.999_990_000_000_252_6;
    // 先收集要访问的格子和迭代号，再逐个处理：处理会改实体，扫描本身只读。
    let mut cells: Vec<(BlockPos, i32)> = Vec::new();
    for_each_block_intersected_between(from, to, target, &mut |pos, iteration| {
        if iteration >= max {
            return false;
        }
        cells.push((pos, iteration));
        true
    });
    let mut last = 0;
    for (pos, iteration) in cells {
        if !living::is_alive(me, mob) {
            break;
        }
        last = iteration;
        let state = ctx.blocks().block_state(pos);
        if state.is_air() {
            continue;
        }
        let inside = shape::entity_inside_boxes(state, pos, entity_context(me, mob));
        let from_box = me.make_bounding_box_at(from);
        let in_block = shape::is_unit_cube(&inside)
            || from_box.collided_along_vector(to.subtract(from), &world_boxes(&inside, pos));
        let fluid = state.fluid();
        let in_fluid = !fluid.is_empty()
            && fluid_box(ctx.blocks(), pos, fluid)
                .is_some_and(|bounds| from_box.collided_along_vector(to.subtract(from), &[bounds]));
        if !(in_block || in_fluid) || visited.contains(&pos) {
            continue;
        }
        visited.push(pos);
        if in_block {
            let precise = long_move
                || target.intersects(Aabb::new(
                    f64::from(pos.x),
                    f64::from(pos.y),
                    f64::from(pos.z),
                    f64::from(pos.x + 1),
                    f64::from(pos.y + 1),
                    f64::from(pos.z + 1),
                ));
            let _ = precise;
            collector.advance_step(iteration);
            entity_inside(me, mob, ctx, state, pos, collector);
        }
        if in_fluid {
            collector.advance_step(iteration);
            if fluid.is_tagged(fluid_tags::LAVA) {
                collector.apply(EffectType::ClearFreeze);
                collector.apply(EffectType::LavaIgnite);
                collector.run_after(EffectType::LavaIgnite, Action::LavaHurt);
            } else if fluid.is_tagged(fluid_tags::WATER) {
                collector.apply(EffectType::Extinguish);
            }
        }
    }
    last + 1
}

fn entity_context(me: &Entity, mob: &Mob) -> shape::EntityContext {
    shape::EntityContext {
        fall_distance: me.fall_distance,
        bottom: me.position.y,
        falling_block: false,
        descending: false,
        placement: false,
        powder_snow_walkable: mob.walks_on_powder_snow(),
    }
}

/// 方块的 `entityInside`：牛会碰到的那些。
fn entity_inside(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    state: BlockState,
    _pos: BlockPos,
    collector: &mut Collector,
) {
    if state.is(blocks::CACTUS) {
        living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Cactus),
            1.0,
        );
    } else if state.is(blocks::SWEET_BERRY_BUSH) && mob.mob_type != super::MobType::Fox {
        // 狐狸（和蜜蜂）不被浆果丛卡住也不扎。
        // Foxes (and bees) are neither slowed nor pricked by berry bushes.
        me.fall_distance = 0.0;
        me.stuck_speed_multiplier = Vec3::new(f64::from(0.8_f32), 0.75, f64::from(0.8_f32));
        if state.try_get(AGE_3).unwrap_or(0) != 0 {
            let moved = me.old_position.subtract(me.position);
            if moved.x * moved.x + moved.z * moved.z > 0.0
                && (moved.x.abs() >= f64::from(0.003_f32) || moved.z.abs() >= f64::from(0.003_f32))
            {
                living::hurt_server(
                    me,
                    mob,
                    ctx,
                    DamageSource::environment(DamageKind::SweetBerryBush),
                    1.0,
                );
            }
        }
    } else if state.is_a(BlockAncestor::WebBlock) {
        // `Spider.makeStuckInBlock` 对蜘蛛网直接返回：既不减速也不清摔落距离。
        // `Spider.makeStuckInBlock` returns early for cobwebs: no slowdown, no fall reset.
        if !mob.mob_type.climbs_walls() {
            me.fall_distance = 0.0;
            me.stuck_speed_multiplier = Vec3::new(0.25, f64::from(0.05_f32), 0.25);
        }
    } else if state.is_a(BlockAncestor::PowderSnowBlock) {
        if ctx
            .blocks()
            .block_state(me.block_position())
            .is(blocks::POWDER_SNOW)
        {
            me.fall_distance = 0.0;
            me.stuck_speed_multiplier = Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32));
        }
        if me.delta.x != 0.0 || me.delta.z != 0.0 {
            let random = ctx.blocks();
            if random.level_random_boolean() {
                random.level_random_float();
                random.level_random_float();
            }
        }
        collector.apply(EffectType::Freeze);
        collector.apply(EffectType::Extinguish);
    } else if state.is_a(BlockAncestor::BaseFireBlock) {
        let damage = if state.is(blocks::SOUL_FIRE) {
            2.0
        } else {
            1.0
        };
        collector.apply(EffectType::ClearFreeze);
        collector.apply(EffectType::FireIgnite);
        collector.run_after(EffectType::FireIgnite, Action::HurtInFire(damage));
    } else if state.is_a(BlockAncestor::LavaCauldronBlock) {
        collector.apply(EffectType::ClearFreeze);
        collector.apply(EffectType::LavaIgnite);
        collector.run_after(EffectType::LavaIgnite, Action::LavaHurt);
    } else if state.is_a(BlockAncestor::LayeredCauldronBlock) {
        collector.apply(EffectType::Extinguish);
    }
}

// ---- 流体 ----

/// `Entity.updateFluidInteraction` 加 `EntityFluidInteraction.update`。
pub(crate) fn update_fluid_interaction(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    mob.fluid_trackers.clear();
    let bounds = me.bounding_box().deflated(0.001);
    let x0 = mth::floor(bounds.min_x);
    let y0 = mth::floor(bounds.min_y);
    let z0 = mth::floor(bounds.min_z);
    let x1 = mth::ceil(bounds.max_x) - 1;
    let y1 = mth::ceil(bounds.max_y) - 1;
    let z1 = mth::ceil(bounds.max_z) - 1;
    let mut water = Accumulator::default();
    let mut lava = Accumulator::default();
    if fluid_area_loaded(view, x0 - 1, y0, z0 - 1, x1 + 1, y1, z1 + 1) {
        let entity_min_y = me.position.y;
        let block_x = mth::floor(me.position.x);
        let block_z = mth::floor(me.position.z);
        let eye = me.position.y + f64::from(mob.eye_height);
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let pos = BlockPos::new(x, y, z);
                    let fluid = view.fluid_state(pos);
                    if fluid.is_empty() {
                        continue;
                    }
                    let fy = f64::from(y);
                    let top = fy + f64::from(view.fluid_height(fluid, pos));
                    if top < bounds.min_y {
                        continue;
                    }
                    let index = living::tracker_for(&mut mob.fluid_trackers, fluid.fluid());
                    if x == block_x && z == block_z && eye >= fy {
                        let camera = fy + f64::from(camera_height(view, fluid, pos));
                        if eye <= camera {
                            mob.fluid_trackers[index].eyes_inside = true;
                        }
                    }
                    let tracker = &mut mob.fluid_trackers[index];
                    tracker.height = mth::java_max(top - entity_min_y, tracker.height);
                    let height = tracker.height;
                    let accumulator = if fluid.is_tagged(fluid_tags::WATER) {
                        &mut water
                    } else if fluid.is_tagged(fluid_tags::LAVA) {
                        &mut lava
                    } else {
                        continue;
                    };
                    let mut flow = flow_vector(view, pos, fluid);
                    accumulator.height = mth::java_max(height, accumulator.height);
                    if accumulator.height < 0.4 {
                        flow = flow.scale(accumulator.height);
                    }
                    accumulator.current = accumulator.current.add(flow);
                    accumulator.count += 1;
                }
            }
        }
    }
    let in_water = living::fluid_height(mob, fluid_tags::WATER) > 0.0;
    let in_lava = living::fluid_height(mob, fluid_tags::LAVA) > 0.0;
    if in_water {
        me.fall_distance = 0.0;
        if !me.in_water && !me.first_tick {
            splash(me);
        }
    }
    me.in_water = in_water;
    // `isPushedByFluid()` 为假的（水生生物）不吃水流。
    // Mobs whose `isPushedByFluid()` is false (water creatures) ignore currents.
    if !mob.mob_type.is_pushed_by_fluid() {
        return;
    }
    if in_water && water.count > 0 {
        apply_current(me, &water.current, 0.014);
    }
    if in_lava && lava.count > 0 {
        apply_current(me, &lava.current, 0.002_333_333_333_333_333_5);
    }
}

#[derive(Default)]
struct Accumulator {
    height: f64,
    current: Vec3,
    count: i32,
}

/// `hasFluidAndLoaded` 的加载部分：按区块坐标逐个看。
fn fluid_area_loaded(
    view: &dyn BlockView,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) -> bool {
    for chunk_z in (z0 >> 4)..=(z1 >> 4) {
        for chunk_x in (x0 >> 4)..=(x1 >> 4) {
            let probe = BlockPos::new(
                chunk_x << 4,
                y0.max(view.min_y()).min(view.max_y()),
                chunk_z << 4,
            );
            if !view.is_loaded(probe) {
                return false;
            }
        }
    }
    let _ = y1;
    true
}

/// `FluidState.getHeightForCamera`。
fn camera_height(view: &dyn BlockView, fluid: FluidState, pos: BlockPos) -> f32 {
    if fluid.is_source()
        && view
            .block_state(pos.above())
            .is_face_sturdy(Direction::Down, SupportType::Full)
    {
        1.0
    } else {
        view.fluid_height(fluid, pos)
    }
}

/// `doWaterSplashEffect`：两个音高，再按宽度跑两轮粒子。
fn splash(me: &mut Entity) {
    me.random.next_float();
    me.random.next_float();
    let width = me.dimensions.0;
    let limit = 1.0 + width * 20.0;
    let mut index = 0;
    while (index as f32) < limit {
        me.random.next_double();
        me.random.next_double();
        me.random.next_double();
        index += 1;
    }
    index = 0;
    while (index as f32) < limit {
        me.random.next_double();
        me.random.next_double();
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    //! `TurtleEggBlock.stepOn` / `fallOn` / `canDestroyEgg` 的分派。
    //! Dispatch for `TurtleEggBlock.stepOn`, `fallOn` and `canDestroyEgg`.

    use super::*;
    use crate::{
        BlockTicks, Effect,
        entity::{
            ctx::{Command, EntityCtx, PreciseCtx},
            view::{SourceView, ViewFacts},
        },
        state::EGGS,
        testing::TestLevel,
        world::World,
    };
    use mc_bamboo_data::blocks;

    const EGG_POS: BlockPos = BlockPos::new(0, 63, 0);

    struct EggSource {
        griefing: bool,
    }

    impl crate::entity::view::BlockSource for EggSource {
        fn block_state(&self, pos: BlockPos) -> BlockState {
            if pos.y == EGG_POS.y {
                BlockState(blocks::TURTLE_EGG).with(EGGS, 4)
            } else {
                BlockState(blocks::AIR)
            }
        }

        fn facts(&self) -> ViewFacts {
            let mut facts = ViewFacts::default();
            facts.rules.mob_griefing = self.griefing;
            facts
        }
    }

    fn egg_source(griefing: bool) -> SourceView<EggSource> {
        SourceView::new(EggSource { griefing })
    }

    /// 收集一次 `step_on` 或 `fall_on` 发出的命令，而不是改世界。
    /// Collect the commands emitted by one `step_on` / `fall_on` instead of changing the world.
    struct CollectCtx<T: crate::entity::view::BlockSource> {
        view: SourceView<T>,
        commands: Vec<Command>,
        uid: crate::entity::EntityUid,
    }

    impl<T: crate::entity::view::BlockSource> CollectCtx<T> {
        fn new(view: SourceView<T>, uid: crate::entity::EntityUid) -> Self {
            Self {
                view,
                commands: Vec::new(),
                uid,
            }
        }
    }

    impl<T: crate::entity::view::BlockSource> EntityCtx for CollectCtx<T> {
        fn uid(&self) -> crate::entity::EntityUid {
            self.uid
        }

        fn blocks(&self) -> &dyn BlockView {
            &self.view
        }

        fn other(&self, _: crate::entity::EntityUid) -> Option<crate::entity::EntityPublic> {
            None
        }

        fn entity_uids(&self) -> Vec<crate::entity::EntityUid> {
            Vec::new()
        }

        fn for_each_other_in(&self, _: Aabb, _: &mut dyn FnMut(&crate::entity::EntityPublic)) {}

        fn for_each_other_uid_in(&self, _: Aabb, _: &mut dyn FnMut(crate::entity::EntityUid)) {}

        fn emit(&mut self, command: Command, _me: &mut Entity) {
            self.commands.push(command);
        }

        fn hurt_player(
            &mut self,
            _: crate::entity::EntityUid,
            _: DamageSource,
            _: f32,
            _: &mut Entity,
            _: &crate::entity::EntityPublic,
        ) -> bool {
            false
        }
    }

    /// `step_on` 发出的踩蛋命令，带官方的 1/100。
    /// The emitted trample command from `step_on`, with vanilla's 1 in 100 chance.
    fn stepped(kind: super::super::MobType, griefing: bool) -> Option<i32> {
        let mut me = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
        let mut ctx = CollectCtx::new(egg_source(griefing), me.id);
        let mut mob = me.kind.mob_mut().cloned().expect("a mob");
        step_on(
            &mut me,
            &mut mob,
            &mut ctx,
            EGG_POS,
            BlockState(blocks::TURTLE_EGG).with(EGGS, 4),
        );
        ctx.commands.into_iter().find_map(|command| match command {
            Command::TrampleTurtleEgg { chance, .. } => Some(chance),
            _ => None,
        })
    }

    /// `fall_on` 发出的踩蛋命令，带官方的 1/3。
    /// The emitted trample command from `fall_on`, with vanilla's 1 in 3 chance.
    fn fallen(kind: super::super::MobType, griefing: bool) -> Option<i32> {
        let mut me = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
        let mut ctx = CollectCtx::new(egg_source(griefing), me.id);
        let mut mob = me.kind.mob_mut().cloned().expect("a mob");
        fall_on(
            &mut me,
            &mut mob,
            &mut ctx,
            EGG_POS,
            BlockState(blocks::TURTLE_EGG).with(EGGS, 4),
            0.0,
        );
        ctx.commands.into_iter().find_map(|command| match command {
            Command::TrampleTurtleEgg { chance, .. } => Some(chance),
            _ => None,
        })
    }

    /// `canDestroyEgg`：海龟和蝙蝠都不踩，别的生物按 `mobGriefing`。
    /// `canDestroyEgg`: turtles and bats never trample; other mobs need `mobGriefing`.
    #[test]
    fn turtles_and_bats_never_trample_eggs() {
        for kind in [super::super::MobType::Turtle, super::super::MobType::Bat] {
            assert_eq!(stepped(kind, true), None, "{kind:?} must not step on eggs");
            assert_eq!(fallen(kind, true), None, "{kind:?} must not fall on eggs");
        }
        assert_eq!(stepped(super::super::MobType::Cow, true), Some(100));
        assert_eq!(fallen(super::super::MobType::Cow, true), Some(3));
    }

    /// `fallOn` 的 `instanceof Zombie` 豁免：僵尸一系落到蛋上不踩，走上去照踩。
    /// The `instanceof Zombie` exemption of `fallOn`: zombies do not trample on
    /// landing, but still do when walking.
    #[test]
    fn only_zombies_are_exempt_from_falling_on_eggs() {
        for kind in [
            super::super::MobType::Zombie,
            super::super::MobType::Husk,
            super::super::MobType::Drowned,
        ] {
            assert_eq!(fallen(kind, true), None, "{kind:?} fell on eggs");
            assert_eq!(stepped(kind, true), Some(100));
        }
        assert_eq!(fallen(super::super::MobType::Skeleton, true), Some(3));
    }

    /// `canDestroyEgg` 对非玩家生物要求 `mobGriefing`。
    /// `canDestroyEgg` requires `mobGriefing` for non-player mobs.
    #[test]
    fn mob_griefing_gates_mob_trampling() {
        assert_eq!(stepped(super::super::MobType::Cow, false), None);
        assert_eq!(fallen(super::super::MobType::Cow, false), None);
    }

    /// 命令落地时 `nextInt(chance) == 0` 才减一枚，并发 2001 事件带旧状态。
    /// The command only removes an egg on `nextInt(chance) == 0`, with a 2001 event
    /// carrying the old state.
    #[test]
    fn the_trample_command_decrements_the_clutch() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let egg = BlockState(blocks::TURTLE_EGG).with(EGGS, 4);
        level.set(EGG_POS, egg);
        let mut world = World::new(&mut level, &mut ticks);
        let mut me = Entity::by_mob_type(super::super::MobType::Cow, Vec3::new(0.5, 64.0, 0.5));
        // 测试世界的随机源种子是 0，`nextInt(1)` 恒为 0；chance=1 让必踩。
        // The test world's random seed is 0; chance 1 always tramples.
        crate::entity::ctx::apply(
            Command::TrampleTurtleEgg {
                pos: EGG_POS,
                chance: 1,
            },
            &mut me,
            &mut world,
        );
        let effects = world.drain_effects();
        drop(world);
        assert_eq!(level.get(EGG_POS).get(EGGS), 3);
        assert!(effects.iter().any(|effect| matches!(
            *effect,
            Effect::LevelEvent {
                event: 2001,
                pos,
                data,
                except_initiator: false
            } if pos == EGG_POS && data == egg.0 as i32
        )));
    }

    /// `PreciseCtx` 存在，编译期确认两种驱动都能落地踩蛋命令。
    /// `PreciseCtx` exists so both drivers can apply the trample command.
    #[test]
    fn precise_context_applies_the_same_trample_command() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(EGG_POS, BlockState(blocks::TURTLE_EGG).with(EGGS, 2));
        let mut world = World::new(&mut level, &mut ticks);
        let mut me = Entity::by_mob_type(super::super::MobType::Cow, Vec3::new(0.5, 64.0, 0.5));
        let mut ctx = PreciseCtx::new(&mut world, me.id);
        ctx.emit(
            Command::TrampleTurtleEgg {
                pos: EGG_POS,
                chance: 1,
            },
            &mut me,
        );
        // `ctx` 借着 `world`，两者都在这一段结束时离开作用域。
        // `ctx` borrows `world`; both leave scope at the end of this block.
        assert_eq!(level.get(EGG_POS).get(EGGS), 1);
    }

    /// 摔落免疫照官方 `minecraft:fall_damage_immune` 实体标签：旋风人、鸡、蜜蜂、铁傀儡
    /// 在标签里，僵尸、牛不在。
    ///
    /// Fall immunity follows the vanilla `minecraft:fall_damage_immune` entity tag.
    #[test]
    fn fall_damage_immunity_follows_the_entity_tag() {
        use crate::entity::mob::MobType;
        for kind in [
            MobType::Breeze,
            MobType::Chicken,
            MobType::Bee,
            MobType::IronGolem,
            MobType::CopperGolem,
            MobType::HappyGhast,
        ] {
            assert!(kind.fall_damage_immune(), "{kind:?}");
        }
        for kind in [MobType::Zombie, MobType::Cow, MobType::Villager] {
            assert!(!kind.fall_damage_immune(), "{kind:?}");
        }
    }
}
