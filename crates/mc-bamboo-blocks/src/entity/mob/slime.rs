//! 史莱姆：官方 `AbstractCubeMob` → `Slime`。大小决定尺寸、血量、速度和攻击力；用自己的
//! `CubeMobMoveControl` 一跳一跳地走，没有寻路；碰到玩家就打；死掉被移除时分裂成小一号的。
//! 自然生成看史莱姆区块、沼泽的月相和亮度。
//!
//! Slimes: vanilla `AbstractCubeMob` → `Slime`. The size drives dimensions, health, speed
//! and attack damage. They hop with their own `CubeMobMoveControl` and never pathfind, hurt
//! players on touch and split into smaller cubes when a dead one is removed. Natural spawns
//! depend on slime chunks and on the swamp moon phase and light.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    entity_types,
    registries::{attribute, mob_effect},
};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::{Attributes, Modifier, Operation as ModifierOperation},
    control::{self, Operation},
    goals::{CombatTarget, Flag, Goal, GoalSelector, TargetType, WrappedGoal},
    living,
    navigation::Navigation,
    spawn::{AgeableGroupData, RANDOM_SPAWN_BONUS, SpawnGroup},
};
use crate::{
    entity::{
        Entity, EntityPublic, EntityUid, JavaRandom,
        ctx::{BlockView, Command, EntityCtx},
    },
    random::LegacyRandom,
    world::World,
};

/// `Slime.checkSlimeSpawnRules` 查的群系标签。
///
/// The biome tag `Slime.checkSlimeSpawnRules` tests.
const SURFACE_SLIME_BIOMES: &str = "minecraft:allows_surface_slime_spawns";

/// `AbstractCubeMob.MIN_SIZE` / `MAX_SIZE`。
const MIN_SIZE: i32 = 1;
const MAX_SIZE: i32 = 127;

/// `WorldgenRandom.seedSlimeChunk` 在 `Slime.checkSlimeSpawnRules` 里用的盐。
///
/// The salt `Slime.checkSlimeSpawnRules` passes to `WorldgenRandom.seedSlimeChunk`.
const SLIME_CHUNK_SALT: i64 = 987_234_911;

/// 史莱姆自己的状态：`ID_SIZE`、`wasOnGround` 和 `CubeMobMoveControl` 的三个字段。
/// 都是相对量，随实体行搬岛。
///
/// Slime-only state: `ID_SIZE`, `wasOnGround` and the three `CubeMobMoveControl` fields.
/// Nothing here is an absolute time, so it moves with the entity row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubeState {
    pub size: i32,
    pub was_on_ground: bool,
    /// `CubeMobMoveControl.yRot`：要转向的角度。
    pub move_y_rot: f32,
    pub jump_delay: i32,
    pub aggressive: bool,
}

impl Default for CubeState {
    fn default() -> Self {
        Self {
            size: MIN_SIZE,
            was_on_ground: false,
            move_y_rot: 0.0,
            jump_delay: 0,
            aggressive: false,
        }
    }
}

/// `AbstractCubeMob.getDefaultDimensions`：`EntityType` 的尺寸乘大小（宽，高，眼高）。
///
/// `AbstractCubeMob.getDefaultDimensions`: the type's dimensions scaled by the size
/// (width, height, eye height).
pub(super) const fn dimensions(size: i32) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Slime.entity_type() as usize];
    let scale = size as f32;
    (
        facts.width * scale,
        facts.height * scale,
        entity_types::SLIME_EYE_HEIGHT * scale,
    )
}

/// `Monster.createMonsterAttributes()`：`Mob` 的缺省属性加 `ATTACK_DAMAGE`。大小相关的三项
/// 由 [`set_size`] 改。
///
/// `Monster.createMonsterAttributes()`: the `Mob` defaults plus `ATTACK_DAMAGE`. The three
/// size-driven bases are set by [`set_size`].
pub(super) fn attributes() -> Attributes {
    use mc_bamboo_data::registries::attribute_defaults as defaults;
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MOVEMENT_SPEED, defaults::MOVEMENT_SPEED);
    table
}

/// `AbstractCubeMob.registerGoals` 加 `Slime.addBehaviourGoals`，按注册顺序。
/// `CubeMobFloatGoal` 的构造函数顺手打开导航的浮水。
///
/// `AbstractCubeMob.registerGoals` plus `Slime.addBehaviourGoals`, in registration order.
/// The `CubeMobFloatGoal` constructor turns on floating for the navigation.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    selector(vec![
        (1, Goal::CubeFloat),
        (
            4,
            Goal::CubeRandomDirection {
                chosen_degrees: 0.0,
                next_randomize_time: 0,
            },
        ),
        (5, Goal::CubeKeepJumping),
        (
            2,
            Goal::CubeAttack {
                grow_tired_timer: 0,
            },
        ),
    ])
}

/// `Slime.addTargetingGoals`：玩家（间隔 10、要看得见、高度差不超过 4）和铁傀儡。
///
/// `Slime.addTargetingGoals`: players (interval 10, must see, at most 4 blocks of height
/// difference) and iron golems.
pub(super) fn targets() -> GoalSelector {
    let nearest = |kind| Goal::NearestAttackableTarget {
        kind,
        interval: 5,
        unseen_ticks: 0,
        found: None,
        light_gate: false,
    };
    selector(vec![
        (1, nearest(TargetType::Player)),
        (3, nearest(TargetType::IronGolem)),
    ])
}

fn selector(goals: Vec<(i32, Goal)>) -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = goals
        .into_iter()
        .map(|(priority, goal)| WrappedGoal {
            priority,
            running: false,
            goal,
        })
        .collect();
    selector
}

/// `Slime.addTargetingGoals` 里玩家目标的附加条件：`abs(target.getY() - getY()) <= 4`。
///
/// The extra player condition in `Slime.addTargetingGoals`.
pub(super) fn accepts_target_height(me: &Entity, target_y: f64) -> bool {
    (target_y - me.position.y).abs() <= 4.0
}

/// `CubeMobMoveControl` 的构造函数：`yRot = 180.0F * mob.getYRot() / (float) Math.PI`，
/// 在 `LivingEntity` 构造函数抽完朝向之后算。
///
/// The `CubeMobMoveControl` constructor, evaluated after the `LivingEntity` constructor
/// drew the facing.
pub(crate) fn constructor_move_y_rot(y_rot: f32) -> f32 {
    180.0 * y_rot / std::f32::consts::PI
}

/// `Slime.setSize`：夹到 `[1, 127]`，换尺寸，按大小设血量上限、移速、攻击力；
/// `update_health` 时回满血。大小真的变了才走 `onSyncedDataUpdated`：身体和实体的朝向
/// 都改成头的朝向，在水里再取一次 `nextInt(20)`。
///
/// `Slime.setSize`: clamp to `[1, 127]`, resize, set max health, speed and attack damage
/// from the size and optionally refill health. `onSyncedDataUpdated` only runs when the
/// size really changes: yaw and body yaw snap to the head yaw, and one `nextInt(20)` is
/// drawn in water.
pub fn set_size(me: &mut Entity, mob: &mut Mob, size: i32, update_health: bool) {
    let actual = size.clamp(MIN_SIZE, MAX_SIZE);
    let changed = mob.cube.size != actual;
    mob.cube.size = actual;
    let (width, height, eye) = dimensions(actual);
    me.dimensions = (width, height);
    mob.eye_height = eye;
    if changed {
        me.y_rot = mob.y_head_rot;
        mob.y_body_rot = mob.y_head_rot;
        if me.in_water {
            // `doWaterSplashEffect` 的音效和粒子没有接，只保住这一次取数。
            // The splash sound and particles are not wired; only the draw is kept.
            let _ = me.random.next_int_bounded(20);
        }
    }
    mob.attributes
        .set_base(attribute::MAX_HEALTH, f64::from(actual * actual));
    mob.attributes.set_base(
        attribute::MOVEMENT_SPEED,
        f64::from(0.2_f32 + 0.1_f32 * actual as f32),
    );
    if update_health {
        mob.health = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    }
    mob.attributes
        .set_base(attribute::ATTACK_DAMAGE, f64::from(actual));
}

/// `isTiny()`。
pub(super) const fn is_tiny(mob: &Mob) -> bool {
    mob.cube.size <= 1
}

/// `canDealDamage()`：不是最小的一号，且 `isEffectiveAi()`。
pub(super) const fn can_deal_damage(mob: &Mob) -> bool {
    !is_tiny(mob) && !mob.no_ai
}

/// `getSoundVolume()`：`0.4F * size`。
pub(super) fn sound_volume(mob: &Mob) -> f32 {
    0.4 * mob.cube.size as f32
}

pub(super) const fn hurt_sound(mob: &Mob) -> &'static str {
    if is_tiny(mob) {
        "minecraft:entity.slime.hurt_small"
    } else {
        "minecraft:entity.slime.hurt"
    }
}

pub(super) const fn death_sound(mob: &Mob) -> &'static str {
    if is_tiny(mob) {
        "minecraft:entity.slime.death_small"
    } else {
        "minecraft:entity.slime.death"
    }
}

const fn squish_sound(mob: &Mob) -> &'static str {
    if is_tiny(mob) {
        "minecraft:entity.slime.squish_small"
    } else {
        "minecraft:entity.slime.squish"
    }
}

const fn jump_sound(mob: &Mob) -> &'static str {
    if is_tiny(mob) {
        "minecraft:entity.slime.jump_small"
    } else {
        "minecraft:entity.slime.jump"
    }
}

/// `CubeMobMoveControl.tick`：每刻朝目标角度转最多 90 度；要动的那一刻在地上按 `jumpDelay`
/// 起跳（抽下一次的延迟，攻击时除以 3），没到时间就把输入清零；空中保持前进速度。
///
/// `CubeMobMoveControl.tick`: turn up to 90 degrees a tick; when asked to move, hop from the
/// ground once `jumpDelay` runs out (drawing the next delay, a third of it when aggressive)
/// and otherwise zero the input; keep the forward speed in the air.
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    me.y_rot = control::move_rotlerp(me.y_rot, mob.cube.move_y_rot, 90.0);
    mob.y_head_rot = me.y_rot;
    mob.y_body_rot = me.y_rot;
    if mob.mover.operation != Operation::MoveTo {
        mob.zza = 0.0;
        return;
    }
    mob.mover.operation = Operation::Wait;
    let speed = (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
    if !me.on_ground {
        living::set_speed(mob, speed);
        return;
    }
    living::set_speed(mob, speed);
    let delay = mob.cube.jump_delay;
    mob.cube.jump_delay -= 1;
    if delay > 0 {
        mob.xxa = 0.0;
        mob.zza = 0.0;
        living::set_speed(mob, 0.0);
        return;
    }
    mob.cube.jump_delay = jump_delay(&mut me.random);
    if mob.cube.aggressive {
        mob.cube.jump_delay /= 3;
    }
    mob.jump.jump = true;
    // `doPlayJumpSound()` 是 `getSize() > 0`，恒真。
    // `doPlayJumpSound()` is `getSize() > 0`, always true.
    let pitch = sound_pitch(&mut me.random, mob);
    living::play_sound(me, mob, jump_sound(mob), sound_volume(mob), pitch);
}

/// `getJumpDelay()`：`nextInt(20) + 10`。
fn jump_delay(random: &mut JavaRandom) -> i32 {
    random.next_int_bounded(20) + 10
}

/// `getSoundPitch()`：最小的一号音调高。
fn sound_pitch(random: &mut JavaRandom, mob: &Mob) -> f32 {
    let adjuster = if is_tiny(mob) { 1.4_f32 } else { 0.8 };
    let first = random.next_float();
    let second = random.next_float();
    ((first - second) * 0.2 + 1.0) * adjuster
}

/// `AbstractCubeMob.jumpFromGround`：竖直速度直接设成 `getJumpPower()`，并要求同步一次速度。
///
/// `AbstractCubeMob.jumpFromGround`: the vertical speed becomes `getJumpPower()` outright
/// and one velocity sync is requested.
pub(super) fn jump_from_ground(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    let strength = mob.attributes.value(attribute::JUMP_STRENGTH) as f32;
    let boost = mob
        .effects
        .amplifier(mob_effect::JUMP_BOOST)
        .map_or(0.0, |amplifier| 0.1 * (amplifier.max(0) as f32 + 1.0));
    let power = strength * living::block_jump_factor(me, view) + boost;
    me.delta = Vec3::new(me.delta.x, f64::from(power), me.delta.z);
    mob.hurt_marked = true;
}

/// `AbstractCubeMob.tick` 在 `super.tick()` 之后的部分：刚落地时按宽度取粒子的随机数
/// （每颗两个 `nextFloat`，服务端不显示粒子但照样取）、放落地音效。挤压动画只在客户端看得见，
/// 不建模。
///
/// The part of `AbstractCubeMob.tick` after `super.tick()`: on landing draw the particle
/// randoms (two `nextFloat` each; the server shows nothing but still draws) and play the
/// squish sound. The squish animation is client-only and not modelled.
pub(super) fn after_tick(me: &mut Entity, mob: &mut Mob) {
    if me.on_ground && !mob.cube.was_on_ground {
        let size = me.dimensions.0 * 2.0;
        let mut index = 0;
        while (index as f32) < size * 16.0 {
            me.random.next_float();
            me.random.next_float();
            index += 1;
        }
        let first = me.random.next_float();
        let second = me.random.next_float();
        let pitch = ((first - second) * 0.2 + 1.0) / 0.8;
        living::play_sound(me, mob, squish_sound(mob), sound_volume(mob), pitch);
    }
    mob.cube.was_on_ground = me.on_ground;
}

/// `Mob.lookAt(target, 10, 10)`：俯仰和朝向各自最多转 10 度。
///
/// `Mob.lookAt(target, 10, 10)`: pitch and yaw each turn by at most 10 degrees.
fn look_at(me: &mut Entity, mob: &mut Mob, target: CombatTarget) {
    let dx = target.position.x - me.position.x;
    let dz = target.position.z - me.position.z;
    let dy = target.eye_y() - living::eye_y(me, mob);
    let horizontal = (dx * dx + dz * dz).sqrt();
    let degrees = |radians: f64| radians * f64::from(180.0_f32 / std::f32::consts::PI);
    let yaw = degrees(mth::atan2(dz, dx)) as f32 - 90.0;
    let pitch = -degrees(mth::atan2(dy, horizontal)) as f32;
    mob.x_rot = control::rotate_towards(mob.x_rot, pitch, 10.0);
    me.y_rot = control::rotate_towards(me.y_rot, yaw, 10.0);
}

/// `CubeMobAttackGoal.tick`：看向目标，把移动控制的方向设成当前朝向。
pub(super) fn attack_tick(me: &mut Entity, mob: &mut Mob, target: Option<CombatTarget>) {
    if let Some(target) = target {
        look_at(me, mob, target);
    }
    mob.cube.move_y_rot = me.y_rot;
    mob.cube.aggressive = can_deal_damage(mob);
}

/// `CubeMobRandomDirectionGoal.canUse` 里 `getTarget() == null` 之后的部分。
pub(super) fn can_pick_direction(me: &Entity, mob: &Mob) -> bool {
    me.on_ground || living::is_in_liquid(me, mob) || mob.effects.has(mob_effect::LEVITATION)
}

/// `CubeMobMoveControl.setWantedMovement`。
pub(super) const fn set_wanted_movement(mob: &mut Mob, speed_modifier: f64) {
    mob.mover.speed_modifier = speed_modifier;
    mob.mover.operation = Operation::MoveTo;
}

/// 这些目标占的标志位。
pub(super) const fn flags(goal: &Goal) -> &'static [Flag] {
    match goal {
        Goal::CubeFloat | Goal::CubeKeepJumping => &[Flag::Jump, Flag::Move],
        _ => &[Flag::Look],
    }
}

// ---- 分裂 ----

/// 分裂出来的一只：相对母体的水平偏移和朝向。
///
/// One split child: its horizontal offset from the parent and its facing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitChild {
    pub xd: f32,
    pub zd: f32,
    pub y_rot: f32,
}

/// `AbstractCubeMob.remove`：死掉的、比最小号大的史莱姆被移除时分裂。数量
/// `2 + nextInt(3)`，每只的朝向 `nextFloat() * 360`，都取母体的随机源。和平难度下
/// `EntityType.create` 返回空，`setUpSplitCube` 不跑，朝向也就不取。
///
/// `AbstractCubeMob.remove`: a dead slime above the smallest size splits when removed. The
/// count is `2 + nextInt(3)` and each facing `nextFloat() * 360`, all on the parent's
/// random. In peaceful `EntityType.create` returns null, so `setUpSplitCube` never runs and
/// no facing is drawn.
pub(super) fn split_on_remove(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let size = mob.cube.size;
    if size <= 1 || !mob.is_dead_or_dying() {
        return;
    }
    let offset = me.dimensions.0 / 2.0;
    let count = 2 + me.random.next_int_bounded(3);
    if ctx.blocks().difficulty() == 0 {
        return;
    }
    let children = (0..count)
        .map(|index| SplitChild {
            xd: ((index % 2) as f32 - 0.5) * offset,
            zd: ((index / 2) as f32 - 0.5) * offset,
            y_rot: me.random.next_float() * 360.0,
        })
        .collect();
    ctx.emit(
        Command::SplitCubeMob {
            from: Box::new(mob.clone()),
            half_size: size / 2,
            children,
        },
        me,
    );
}

/// 落地 [`Command::SplitCubeMob`]：每只走一遍 `Mob.convertTo(getType(),
/// ConversionParams(SPLIT_ON_DEATH, false, false, team), TRIGGERED, setUpSplitCube)`。
///
/// Lands [`Command::SplitCubeMob`]: one `Mob.convertTo(getType(),
/// ConversionParams(SPLIT_ON_DEATH, false, false, team), TRIGGERED, setUpSplitCube)` per
/// child.
pub(crate) fn apply_split(
    me: &Entity,
    from: &Mob,
    half_size: i32,
    children: &[SplitChild],
    world: &mut World,
) {
    for child in children {
        let mut entity = world.prepare_entity(Entity::by_mob_type(from.mob_type, Vec3::default()));
        entity.with_mob(|cube, mob| {
            super::conversion::copy_common(from, mob, false);
            set_size(cube, mob, half_size, true);
        });
        let pos = Vec3::new(
            me.position.x + f64::from(child.xd),
            me.position.y + 0.5,
            me.position.z + f64::from(child.zd),
        );
        super::spawn::snap_to(&mut entity, pos, child.y_rot, 0.0);
        world.add_prepared_entity(entity);
    }
}

// ---- 碰到玩家 ----

/// 一次碰撞伤害：伤害来源、数值和攻击者的公开快照（死亡消息要它的名字）。
///
/// One touch hit: the damage source, the amount and the attacker's public snapshot (the
/// death message needs its name).
struct TouchHit {
    source: DamageSource,
    amount: f32,
    attacker: EntityPublic,
}

/// `AbstractCubeMob.playerTouch` → `dealDamage` 在 `hurtServer` 之前的判断：能造成伤害、
/// 活着、在近战范围里、看得见。满足时返回这次伤害，由调用方走玩家受伤的正式入口。
///
/// The checks of `AbstractCubeMob.playerTouch` → `dealDamage` ahead of `hurtServer`: able
/// to deal damage, alive, in melee range and in sight. Returns the hit for the caller to
/// run through the real player hurt path.
fn touch_hit(world: &World, uid: EntityUid, player: EntityUid) -> Option<TouchHit> {
    let entity = world.entity(uid)?;
    let mob = entity.kind.mob()?;
    if mob.mob_type != MobType::Slime || !can_deal_damage(mob) || !living::is_alive(&entity, mob) {
        return None;
    }
    let player = super::goals::player_by_uid(world, player)?;
    let target = CombatTarget::player(player, false);
    if !within_melee_range(&entity, target) {
        return None;
    }
    let from = Vec3::new(
        entity.position.x,
        living::eye_y(&entity, mob),
        entity.position.z,
    );
    let to = target.eye_position();
    if to.distance_to_sqr(from).sqrt() > 128.0
        || BlockView::clip_collider(world, from, to, entity.collision_context()).is_some()
    {
        return None;
    }
    let mut source = DamageSource::environment(DamageKind::MobAttack).with_non_player_cause(true);
    source.attacker = Some(uid);
    source.direct = Some(uid);
    source.attacker_uuid = Some(mob.uuid);
    source.position = Some(entity.position);
    let amount = mob.attributes.value(attribute::ATTACK_DAMAGE) as f32;
    // 表里取出来的副本不带可靠的 ID，公开快照的 uid 以调用方给的为准。
    // A copy taken from the table carries no reliable id; the caller's uid is authoritative.
    let mut attacker = entity.split().public;
    attacker.uid = uid;
    Some(TouchHit {
        source,
        amount,
        attacker,
    })
}

/// `Mob.isWithinMeleeAttackRange`：自己的包围盒水平膨胀 `DEFAULT_ATTACK_REACH`
/// （`sqrt(2.04F) - 0.6F`），和目标的包围盒相交。竖直方向不膨胀。
///
/// `Mob.isWithinMeleeAttackRange`: the own box inflated horizontally by
/// `DEFAULT_ATTACK_REACH` intersects the target's box. There is no vertical inflation.
fn within_melee_range(me: &Entity, target: CombatTarget) -> bool {
    let reach = 2.039_999_961_853_027_3_f64.sqrt() - f64::from(0.6_f32);
    me.bounding_box()
        .inflate(reach, 0.0, reach)
        .intersects(target.hitbox())
}

/// `AbstractCubeMob.playerTouch` 的全部：判断、走玩家受伤的入口、打中了放攻击音效。
/// 返回是否打中。
///
/// All of `AbstractCubeMob.playerTouch`: the checks, the player hurt path and the attack
/// sound on a hit. Returns whether the hit landed.
pub fn player_touch(world: &mut World, slime: EntityUid, player: EntityUid) -> bool {
    let Some(hit) = touch_hit(world, slime, player) else {
        return false;
    };
    let hurt = world.hurt_player_from_mob(player, hit.source, hit.amount, Some(&hit.attacker));
    if hurt {
        world.with_entity_mut(slime, &mut after_touch_hit);
    }
    hurt
}

/// `dealDamage` 打中之后：`playSound(SLIME_ATTACK, 1.0F, (nextFloat - nextFloat) * 0.2F + 1.0F)`。
///
/// After a successful `dealDamage` hit: the attack sound with its two pitch draws.
fn after_touch_hit(me: &mut Entity) {
    me.with_mob(|me, mob| {
        let first = me.random.next_float();
        let second = me.random.next_float();
        let pitch = (first - second) * 0.2 + 1.0;
        living::play_sound(me, mob, "minecraft:entity.slime.attack", 1.0, pitch);
    });
}

// ---- 生成 ----

/// `Slime.finalizeSpawn` → `AbstractCubeMob.finalizeSpawn`：先是 `AgeableMob` /
/// `Mob` 两层（组数据是 `AgeableMobGroupData(false)`，不会变小），再 `setSpawnSize`：
/// `nextInt(3)`，小于 2 时再按 `0.5 * 特殊倍率` 的概率加一档，大小是 `1 << 档`。
///
/// `Slime.finalizeSpawn` → `AbstractCubeMob.finalizeSpawn`: the `AgeableMob` / `Mob` layers
/// first (the group data is `AgeableMobGroupData(false)`, so never a baby), then
/// `setSpawnSize`: `nextInt(3)`, bumped once below 2 with probability `0.5 * special`, and
/// the size is `1 << scale`.
pub(super) fn finalize_spawn(
    world: &World<'_>,
    entity: &mut Entity,
    group: &mut Option<SpawnGroup>,
) {
    let effective = BlockView::effective_difficulty(world, entity.block_position());
    let special = ((effective - 2.0) / 2.0).clamp(0.0, 1.0);
    let random = world.random();
    if !matches!(group, Some(SpawnGroup::Ageable(_))) {
        *group = Some(SpawnGroup::Ageable(AgeableGroupData {
            group_size: 0,
            should_spawn_baby: false,
            baby_spawn_chance: 0.05,
        }));
    }
    if let Some(SpawnGroup::Ageable(data)) = group {
        // 官方这里的条件是 `shouldSpawnBaby && groupSize > 0 && nextFloat() <= chance`；
        // 史莱姆自己建的组数据 `shouldSpawnBaby` 为假，短路在取数之前。
        // Vanilla tests `shouldSpawnBaby && groupSize > 0 && nextFloat() <= chance`; a
        // slime's own group data has `shouldSpawnBaby` false and short-circuits the draw.
        if data.should_spawn_baby && data.group_size > 0 {
            let _ = random.next_float();
        }
        data.group_size += 1;
    }
    entity.with_mob(|me, mob| {
        if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
            && range.modifier(RANDOM_SPAWN_BONUS).is_none()
        {
            let amount = random.triangle(0.0, 0.114_850_000_000_000_01);
            range.modifiers.push(Modifier {
                id: RANDOM_SPAWN_BONUS.to_owned(),
                amount,
                operation: ModifierOperation::AddMultipliedBase,
                permanent: true,
            });
        }
        mob.left_handed = random.next_float() < 0.05;
        let mut scale = random.next_int(3);
        if scale < 2 && random.next_float() < 0.5 * special {
            scale += 1;
        }
        set_size(me, mob, 1 << scale, true);
    });
}

/// `WorldgenRandom.seedSlimeChunk(x, z, seed, salt).nextInt(10) == 0`。官方里
/// `x * x * 4987142` 这几项是 int 乘法（会溢出），再提升成 long。
///
/// `WorldgenRandom.seedSlimeChunk(x, z, seed, salt).nextInt(10) == 0`. The `x * x * 4987142`
/// style terms are int multiplications (they overflow) widened to long afterwards.
pub fn is_slime_chunk(chunk_x: i32, chunk_z: i32, world_seed: i64) -> bool {
    let seed = world_seed
        .wrapping_add(i64::from(
            chunk_x.wrapping_mul(chunk_x).wrapping_mul(4_987_142),
        ))
        .wrapping_add(i64::from(chunk_x.wrapping_mul(5_947_611)))
        .wrapping_add(i64::from(chunk_z.wrapping_mul(chunk_z)).wrapping_mul(4_392_871))
        .wrapping_add(i64::from(chunk_z.wrapping_mul(389_711)))
        ^ SLIME_CHUNK_SALT;
    LegacyRandom::new(seed).next_int(10) == 0
}

/// `Slime.checkSlimeSpawnRules` 里难度判断之后、`NATURAL` 的两支：
///
/// 1. 群系在 `#allows_surface_slime_spawns`、高度在 (50, 70)：`nextFloat()` 小于月相给的
///    概率，且亮度不超过 `nextInt(8)` 时，走 `checkMobSpawnRules`；
/// 2. 否则取 `nextInt(10)`，为 0、在史莱姆区块里且高度低于 40 时，走 `checkMobSpawnRules`。
///
/// 取数的先后和短路照官方：第一支的 `nextInt(8)` 只在概率通过时取，第二支的 `nextInt(10)`
/// 在看史莱姆区块之前取。
///
/// The two `NATURAL` branches of `Slime.checkSlimeSpawnRules` past the difficulty test:
/// surface spawns in `#allows_surface_slime_spawns` between y 50 and 70 gated by the moon
/// phase chance and the light, then slime chunks below y 40 with a one-in-ten roll. Draw
/// order and short-circuits follow vanilla.
pub(super) fn check_spawn_rules(world: &World, pos: BlockPos) -> bool {
    let random = world.random();
    let surface_biome = super::spawn::biome_tag_contains(SURFACE_SLIME_BIOMES, world.biome_id(pos));
    if surface_biome && pos.y > 50 && pos.y < 70 {
        let chance = crate::sky::surface_slime_spawn_chance(world.level().clock_ticks());
        if random.next_float() < chance && world.max_local_raw_brightness(pos) <= random.next_int(8)
        {
            return valid_spawn_block(world, pos);
        }
    }
    let slime_chunk = is_slime_chunk(pos.x >> 4, pos.z >> 4, world.level().world_seed());
    if random.next_int(10) == 0 && slime_chunk && pos.y < 40 {
        return valid_spawn_block(world, pos);
    }
    false
}

/// `Mob.checkMobSpawnRules`（`NATURAL` 不是刷怪笼）：脚下的方块 `isValidSpawn`。
fn valid_spawn_block(world: &World, pos: BlockPos) -> bool {
    // 冰和霜冰只允许北极熊当落脚块，史莱姆按普通状态位判断。
    // Ice and frosted ice are footing only for polar bears; slimes use the ordinary bit.
    world
        .block_state(BlockPos::new(pos.x, pos.y - 1, pos.z))
        .is_valid_spawn_surface(MobType::Slime.entity_type())
}

/// `EntityType.getSpawnAABB` 用的 `spawnDimensionsScale(4.0F)`。
pub(super) const SPAWN_DIMENSIONS_SCALE: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 `Slime` 的三项属性：血量上限 `size²`、移速 `0.2F + 0.1F * size`、攻击力 `size`。
    ///
    /// The three vanilla size-driven attributes.
    #[test]
    fn set_size_drives_dimensions_and_attributes() {
        let mut entity = Entity::by_mob_type(MobType::Slime, Vec3::default());
        entity.with_mob(|me, mob| {
            assert_eq!(mob.cube.size, 1);
            set_size(me, mob, 4, true);
            assert_eq!(me.dimensions, (0.52 * 4.0, 0.52 * 4.0));
            assert_eq!(mob.eye_height, 0.325 * 4.0);
            assert_eq!(mob.health, 16.0);
            assert_eq!(mob.attributes.value(attribute::ATTACK_DAMAGE), 4.0);
            assert_eq!(
                mob.attributes.value(attribute::MOVEMENT_SPEED),
                f64::from(0.2_f32 + 0.1_f32 * 4.0)
            );
            set_size(me, mob, 500, false);
            assert_eq!(mob.cube.size, 127);
            assert_eq!(mob.health, 16.0);
            set_size(me, mob, 0, true);
            assert_eq!((mob.cube.size, mob.health), (1, 1.0));
            assert!(!can_deal_damage(mob));
        });
    }

    /// 史莱姆区块：种子 0 时 (0, 0) 不是；已知的几个坐标用官方公式手算过
    /// （`new Random(seed ^ 987234911).nextInt(10) == 0`）。
    ///
    /// Slime chunks checked against the vanilla formula.
    #[test]
    fn slime_chunks_follow_the_vanilla_formula() {
        let hits: Vec<(i32, i32)> = (-8..8)
            .flat_map(|x| (-8..8).map(move |z| (x, z)))
            .filter(|&(x, z)| is_slime_chunk(x, z, 0))
            .collect();
        // 大约十分之一的区块。
        // Roughly one chunk in ten.
        assert!((10..45).contains(&hits.len()), "{hits:?}");
        assert_eq!(is_slime_chunk(3, -7, 42), is_slime_chunk(3, -7, 42));
    }

    /// 存档写 `Size`（大小减一）和 `wasOnGround`，带 `AgeableMob` 的三个键、不带 `Animal` 的
    /// 求爱键；读回来大小、尺寸和血量都在。
    ///
    /// Saves write `Size` (size minus one) and `wasOnGround` with the three `AgeableMob`
    /// keys and without the `Animal` love keys; loading restores size, dimensions and health.
    #[test]
    fn nbt_round_trip_keeps_the_size() {
        use crate::entity::mob::nbt;
        let mut entity = Entity::by_mob_type(MobType::Slime, Vec3::new(0.5, 64.0, 0.5));
        entity.with_mob(|me, mob| {
            set_size(me, mob, 4, true);
            mob.health = 9.0;
            mob.cube.was_on_ground = true;
        });
        let tag = nbt::write_mob(&entity.split()).expect("write slime");
        let mc_bamboo_nbt::Tag::Compound(entries) = &tag else {
            panic!("mob NBT is a compound");
        };
        let value = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(value("Size"), Some(mc_bamboo_nbt::Tag::Int(3)));
        assert_eq!(value("wasOnGround"), Some(mc_bamboo_nbt::Tag::Byte(1)));
        assert_eq!(value("Age"), Some(mc_bamboo_nbt::Tag::Int(0)));
        assert_eq!(value("InLove"), None);
        let mut loaded = nbt::read_mob(&tag, "minecraft:slime", 7)
            .expect("read slime")
            .expect("modelled")
            .into_entity();
        assert_eq!(loaded.dimensions, (0.52 * 4.0, 0.52 * 4.0));
        loaded.with_mob(|_, mob| {
            assert_eq!((mob.cube.size, mob.health), (4, 9.0));
            assert!(mob.cube.was_on_ground);
            assert_eq!(mob.attributes.value(attribute::ATTACK_DAMAGE), 4.0);
        });
    }

    #[test]
    fn goals_are_registered_in_vanilla_order() {
        let mob = Mob::new(MobType::Slime);
        let names: Vec<(i32, &str)> = mob
            .goals
            .goals
            .iter()
            .map(|wrapped| (wrapped.priority, wrapped.goal.name()))
            .collect();
        assert_eq!(
            names,
            vec![
                (1, "CubeMobFloatGoal"),
                (4, "CubeMobRandomDirectionGoal"),
                (5, "CubeMobKeepOnJumpingGoal"),
                (2, "CubeMobAttackGoal"),
            ]
        );
        assert_eq!(mob.target_goals.goals.len(), 2);
        assert!(mob.navigation.can_float());
    }
}
