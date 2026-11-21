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
            me.collision_context(),
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
        restitute(me, mob, legacy_state, x_collision, z_collision, collided);
    }
    movement_emission(me, mob, ctx.blocks(), collided, legacy_pos, legacy_state);
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
    if !me.in_water {
        update_fluid_interaction(me, mob, ctx.blocks());
    }
    if !me.in_water && ya < 0.0 {
        me.fall_distance -= f64::from(ya as f32);
    }
    if me.on_ground {
        if me.fall_distance > 0.0 {
            let distance = me.fall_distance;
            fall_on(me, mob, ctx, pos, state, distance);
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
        // `TurtleEggBlock.fallOn`：僵尸一系（含尸壳）落上去不踩蛋，其它生物 1/3。
        if !mob.mob_type.is_monster() {
            destroy_turtle_egg(me, ctx, pos, 3);
        }
        cause_fall_damage(me, mob, ctx, distance, 1.0);
    } else if state.is(blocks::HAY_BLOCK) || state.is(blocks::HONEY_BLOCK) {
        cause_fall_damage(me, mob, ctx, distance, 0.2);
    } else if state.is(blocks::SLIME_BLOCK) {
        cause_fall_damage(me, mob, ctx, distance, 0.0);
    } else if state.is_a(BlockAncestor::PowderSnowBlock) {
        // 细雪不调 `super.fallOn`，没有伤害。
    } else if state.is(blocks::FARMLAND) {
        // `FarmlandBlock.fallOn`：先无条件取一次世界随机数；踩坏耕地这一轮不做。
        let _ = ctx.blocks().level_random_float();
        cause_fall_damage(me, mob, ctx, distance, 1.0);
    } else if state.is_a(BlockAncestor::BedBlock) {
        cause_fall_damage(me, mob, ctx, distance * f64::from(1.0_f32 - 0.5), 1.0);
    } else {
        cause_fall_damage(me, mob, ctx, distance, 1.0);
    }
}

/// `LivingEntity.causeFallDamage`。
fn cause_fall_damage(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    distance: f64,
    multiplier: f32,
) {
    use mc_bamboo_data::registries::attribute;
    if mob.mob_type.is_equine() {
        // 马科覆写的 `causeFallDamage` 先播落地声，伤害结算相同。
        // The equine override plays its landing sound first; damage is unchanged.
        super::equine::land_sound(me, mob, distance);
    }
    let power = distance + 1.0e-6 - mob.attributes.value(attribute::SAFE_FALL_DISTANCE);
    let damage = mth::floor(
        power * f64::from(multiplier) * mob.attributes.value(attribute::FALL_DAMAGE_MULTIPLIER),
    );
    if damage > 0 {
        living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Fall),
            damage as f32,
        );
    }
}

/// `restituteMovementAfterCollisions`：生物拿方块的整份弹性，弹性属性默认是 0。
fn restitute(
    me: &mut Entity,
    mob: &Mob,
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
    me.delta = after;
}

/// `applyMovementEmissionAndPlaySound`：只有紫水晶脚步和游泳声取随机数。
fn movement_emission(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    collided: Vec3,
    legacy_pos: BlockPos,
    legacy_state: BlockState,
) {
    let length = (collided.length() * f64::from(0.6_f32)) as f32;
    let horizontal =
        ((collided.x * collided.x + collided.z * collided.z).sqrt() * f64::from(0.6_f32)) as f32;
    let on_pos = me.on_pos(view, 1.0e-5);
    let on_state = view.block_state(on_pos);
    let climbing = on_state.is_tagged(block_tags::CLIMBABLE) || on_state.is(blocks::POWDER_SNOW);
    mob.move_dist += if climbing { length } else { horizontal };
    mob.fly_dist += length;
    if mob.move_dist > mob.next_step && !on_state.is_air() {
        let same = on_pos == legacy_pos;
        // `MovementEmission.EVENTS`（鱿鱼）只发游戏事件：没有脚步声，也不取游泳声的随机数。
        // `MovementEmission.EVENTS` (squid) only emits game events: no step sound and no
        // swim-sound draws.
        let sounds = mob.mob_type.emits_movement_sounds();
        let mut stepped = step_effects(me, mob, legacy_state, sounds, collided);
        if !same {
            stepped |= step_effects(me, mob, on_state, false, collided);
        }
        if stepped {
            mob.next_step = (mob.move_dist as i32 + 1) as f32;
        } else if me.in_water {
            mob.next_step = (mob.move_dist as i32 + 1) as f32;
            // `waterSwimSound` → `playSwimSound`：音量按速度算，音高取两个 `nextFloat`。
            // `waterSwimSound` → `playSwimSound`: the volume follows the speed and the
            // pitch draws two `nextFloat`s.
            if sounds {
                let delta = me.delta;
                let speed = (delta.x * delta.x * 0.2 + delta.y * delta.y + delta.z * delta.z * 0.2)
                    .sqrt() as f32
                    * 0.35;
                let volume = mth::java_min_f32(1.0, speed);
                let first = me.random.next_float();
                let second = me.random.next_float();
                let pitch = 1.0 + (first - second) * 0.4;
                super::living::play_sound(me, mob, mob.mob_type.swim_sound(), volume, pitch);
            }
        }
    }
}

/// `vibrationAndSoundEffectsFromBlock`。
fn step_effects(
    me: &mut Entity,
    mob: &mut Mob,
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
    if play_sound && !mob.mob_type.is_water_animal() {
        // `AbstractCow.playStepSound`：`playSound(stepSound, 0.15F, 1.0F)`。
        // `AbstractFish.playStepSound` 是空实现，但这一步仍算「踩过」。
        // `AbstractCow.playStepSound` plays; `AbstractFish.playStepSound` is empty yet
        // the step still counts.
        let sound = match mob.mob_type {
            super::MobType::Pig => super::pig::step_sound(mob.is_baby()),
            super::MobType::Chicken => super::chicken::sounds(mob)[3],
            kind => kind.step_sound().unwrap_or(mob.sound_variant.step()),
        };
        super::living::play_sound(me, mob, sound, 0.15, 1.0);
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
    if super::path::has_block_collision(view, bounds, me.collision_context(), |pos| {
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
fn destroy_turtle_egg(me: &mut Entity, ctx: &mut dyn EntityCtx, pos: BlockPos, chance: i32) {
    if !ctx.blocks().block_game_rules().mob_griefing {
        return;
    }
    ctx.emit(Command::TrampleTurtleEgg { pos, chance }, me);
}

fn run_action(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, action: Action) {
    match action {
        Action::Effect(EffectType::Freeze) => {
            mob.in_powder_snow = true;
            mob.ticks_frozen = (mob.ticks_frozen + 1).min(140);
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
        let inside = shape::entity_inside_boxes(state, pos, entity_context(me));
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

fn entity_context(me: &Entity) -> shape::EntityContext {
    shape::EntityContext {
        fall_distance: me.fall_distance,
        bottom: me.position.y,
        falling_block: false,
        descending: false,
        placement: false,
        powder_snow_walkable: false,
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
    } else if state.is(blocks::SWEET_BERRY_BUSH) {
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
        me.fall_distance = 0.0;
        me.stuck_speed_multiplier = Vec3::new(0.25, f64::from(0.05_f32), 0.25);
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
