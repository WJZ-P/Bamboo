//! 守卫者 / 远古守卫者：官方 `Guardian` / `ElderGuardian` 的构造事实、目标表与状态。
//!
//! 移动控制、激光和范围效果在后续阶段接入；这里放属性、路径代价、目标表、音效
//! 分派和实体上需要随存档搬动的自有状态。
//!
//! Guardian / elder guardian: the construction facts, goal table and state of vanilla
//! `Guardian` / `ElderGuardian`. Movement control, the laser and the area effect are
//! wired in later phases; this module holds attributes, pathfinding malus, the goal
//! table, sound dispatch and the state that travels with the entity row.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{PathType, registries::attribute, tags::fluid_tags};

use super::{
    Attributes, DamageKind, DamageSource, Malus, Mob, MobType, control,
    goals::{Flag, Goal, GoalSelector, PLAIN_STROLL, TargetType, WrappedGoal},
    living, motion,
};
use crate::entity::{
    Entity, KindPublic,
    ctx::{BlockView, Command, EntityCtx},
};

/// 是不是守卫者一支。
///
/// Whether this is a guardian (either kind).
pub fn is_guardian(kind: MobType) -> bool {
    matches!(kind, MobType::Guardian | MobType::ElderGuardian)
}

/// `Guardian.getAttackDuration()`：普通 80 刻，远古 60 刻。
///
/// `Guardian.getAttackDuration()`: 80 ticks normally, 60 for the elder guardian.
pub fn attack_duration(kind: MobType) -> i32 {
    if kind == MobType::ElderGuardian {
        60
    } else {
        80
    }
}

/// `GuardianAttackSelector` / `GuardianAttackGoal.canContinueToUse` 的距离平方下限
/// （官方 `> 9.0`，也就是 3 格）。
///
/// The squared-distance floor of `GuardianAttackSelector` /
/// `GuardianAttackGoal.canContinueToUse` (vanilla `> 9.0`, i.e. 3 blocks).
pub const MIN_TARGET_DISTANCE_SQR: f64 = 9.0;

/// 激光的间接魔法伤害：基值 1，困难 +2，远古守卫者 +2。
///
/// The laser's indirect magic damage: base 1, +2 on hard, +2 for the elder guardian.
pub fn laser_damage(elder: bool, hard: bool) -> f32 {
    1.0 + if hard { 2.0 } else { 0.0 } + if elder { 2.0 } else { 0.0 }
}

/// 官方 `Guardian` / `ElderGuardian` 的自有字段。都不是存档字段。
///
/// The own fields of vanilla `Guardian` / `ElderGuardian`; none is saved to disk.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GuardianState {
    /// `clientSideTailAnimation`：构造时抽一次 `nextFloat`。
    ///
    /// `clientSideTailAnimation`: one `nextFloat` drawn by the constructor.
    pub tail_animation: f32,
    /// `clientSideTailAnimationO`。
    pub tail_animation_o: f32,
    /// `clientSideTailAnimationSpeed`。
    pub tail_speed: f32,
    /// `clientSideSpikesAnimation`。
    pub spikes_animation: f32,
    /// `clientSideSpikesAnimationO`。
    pub spikes_animation_o: f32,
    /// `DATA_ID_MOVING`（布尔元数据）：守卫者有没有在动。
    ///
    /// `DATA_ID_MOVING` (boolean metadata): whether the guardian is moving.
    pub moving: bool,
    /// `DATA_ID_ATTACK_TARGET`（整数元数据）：0 表示没有激光目标。
    ///
    /// `DATA_ID_ATTACK_TARGET` (integer metadata): 0 means no laser target.
    pub attack_target: i32,
    /// `GuardianAttackGoal.stop` 调 `randomStrollGoal.trigger()` 的落地标记：
    /// tick 期间目标选择器被暂时取走，标记在下一刻的 `Stroll.canUse` 里转为
    /// `force_trigger`。不存档、不同步。
    ///
    /// Landing flag for `randomStrollGoal.trigger()` from `GuardianAttackGoal.stop`:
    /// the selector is taken out of the mob during its tick, so the flag turns into
    /// `force_trigger` in the next `Stroll.canUse`. Not saved or synced.
    pub stroll_triggered: bool,
    /// `GuardianAttackGoal.attackTime`，`start` 时置 -10。
    ///
    /// `GuardianAttackGoal.attackTime`, set to -10 by `start`.
    pub attack_time: i32,
    /// `clientSideTouchedGround`。
    pub touched_ground: bool,
    /// 场景级改坏确认用：非 0 时覆盖 `Guardian.getAttackDuration()`。
    /// 正常路径恒为 0，只在 `oracle-samples` 下编进结构体。
    ///
    /// Scene-level break-self-test hook: overrides `Guardian.getAttackDuration()`
    /// when nonzero. Always 0 on the normal path; compiled only under `oracle-samples`.
    #[cfg(feature = "oracle-samples")]
    pub attack_duration_override: i32,
    /// 场景级改坏确认用：为真时尖刺忽略「没在动」条件。正常路径恒为假。
    ///
    /// Scene-level break-self-test hook: ignores the "not moving" thorns
    /// condition when true. Always false on the normal path.
    #[cfg(feature = "oracle-samples")]
    pub thorns_ignore_moving: bool,
    /// 激光视线的跨刻缓存。三样键（守卫者眼睛位、目标眼睛位、岛级方块版本戳）
    /// 都没变时复用上一次的射线结果；不存档、不同步。
    ///
    /// Cross-tick laser line-of-sight cache. Reuses the last ray result while the
    /// guardian eye position, target eye position and island block stamp are all
    /// unchanged. Not saved and not synced.
    pub sight_cache: Option<GuardianSightCache>,
}

/// 一次激光视线射线的键和结果。三个坐标用**原始位**比较，避免 `NaN` / `-0.0`
/// 让 `==` 把不同的位置当成相同。
///
/// The key and result of one laser sight ray. Coordinates are compared by raw
/// bits so `NaN` and `-0.0` cannot make two different positions look equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardianSightCache {
    /// 守卫者眼睛位置（x, y, z）的原始位。
    pub from: [u64; 3],
    /// 目标 uid。
    pub target: crate::entity::EntityUid,
    /// 目标眼睛位置（x, y, z）的原始位。
    pub to: [u64; 3],
    /// 岛级的方块变更版本戳。
    pub stamp: u64,
    /// `CollisionContext` 里除眼睛位之外还会影响碰撞形状的两项：官方的碰撞形状
    /// 只按 `fallDistance > 2.5` 分档，所以键里存这一档而不是逐刻变化的原始位。
    ///
    /// The remaining shape-affecting parts of `CollisionContext`: vanilla only
    /// splits on `fallDistance > 2.5`, so the key stores that class instead of
    /// the raw value, which changes every tick while falling.
    pub falling: bool,
    pub powder_snow_walkable: bool,
    /// 上次射线的结果。
    pub seen: bool,
}

fn bits(v: Vec3) -> [u64; 3] {
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

/// `GuardianAttackGoal.tick` 的 `hasLineOfSight`：守卫者攻击时基本不动，三样键
/// 都没变就复用上次结果，否则真做一次射线并更新缓存。
///
/// `hasLineOfSight` for `GuardianAttackGoal.tick`: the guardian barely moves
/// while attacking, so reuse the previous result when all three keys match.
pub(crate) fn laser_line_of_sight(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn crate::entity::ctx::BlockView,
    target: crate::entity::EntityUid,
    target_eye: Vec3,
) -> bool {
    laser_line_of_sight_impl(
        me,
        mob,
        view,
        target,
        target_eye,
        !super::timing::stress_naive(),
    )
}

/// 朴素路径（`cfg(test)` 差分用）：永远走真正的射线。
///
/// Naive path for differential tests: always cast the ray.
#[cfg(test)]
pub(crate) fn laser_line_of_sight_naive(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn crate::entity::ctx::BlockView,
    target: crate::entity::EntityUid,
    target_eye: Vec3,
) -> bool {
    laser_line_of_sight_impl(me, mob, view, target, target_eye, false)
}

fn laser_line_of_sight_impl(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn crate::entity::ctx::BlockView,
    target: crate::entity::EntityUid,
    target_eye: Vec3,
    cached: bool,
) -> bool {
    let from = Vec3::new(me.position.x, super::living::eye_y(me, mob), me.position.z);
    let stamp = view.block_change_stamp();
    let context = match me.mob_collision_context(mob) {
        crate::shape::CollisionContext::Entity(context) => context,
        _ => return super::living::has_line_of_sight(me, mob, view, target, target_eye),
    };
    let key = GuardianSightCache {
        from: bits(from),
        target,
        to: bits(target_eye),
        stamp,
        falling: context.fall_distance > 2.5,
        powder_snow_walkable: context.powder_snow_walkable,
        seen: false,
    };
    if cached
        && let Some(last) = mob.guardian.sight_cache
        && last.from == key.from
        && last.target == key.target
        && last.to == key.to
        && last.stamp == key.stamp
        && last.falling == key.falling
        && last.powder_snow_walkable == key.powder_snow_walkable
    {
        super::timing::record_sight!(true);
        return last.seen;
    }
    let seen = super::living::has_line_of_sight(me, mob, view, target, target_eye);
    super::timing::record_sight!(false);
    if cached {
        mob.guardian.sight_cache = Some(GuardianSightCache { seen, ..key });
    }
    seen
}

/// `Guardian.createAttributes` / `ElderGuardian.createAttributes`：
/// `Monster.createMonsterAttributes()` 上覆盖生命、攻击和移速。
///
/// `Guardian.createAttributes` / `ElderGuardian.createAttributes`: the monster defaults
/// with health, attack damage and movement speed overridden.
pub fn attributes(elder: bool) -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, if elder { 80.0 } else { 30.0 });
    table.set_base(attribute::ATTACK_DAMAGE, if elder { 8.0 } else { 6.0 });
    table.set_base(
        attribute::MOVEMENT_SPEED,
        if elder { 0.30000001192092896 } else { 0.5 },
    );
    table
}

/// `Guardian` 构造函数：`setPathfindingMalus(PathType.WATER, 0.0)`。
///
/// The `Guardian` constructor: `setPathfindingMalus(PathType.WATER, 0.0)`.
pub fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(PathType::Water, 0.0);
    malus
}

/// `Guardian.getWalkTargetValue`：水格给 `10 + 光照路径代价`，其余走 `Monster` 那一支。
///
/// `Guardian.getWalkTargetValue`: water cells get `10 + the light path cost`,
/// otherwise the `Monster` branch.
pub fn walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    if view.fluid_state(pos).is_tagged(fluid_tags::WATER) {
        10.0 + super::goals::light_level_cost(view, pos)
    } else {
        super::goals::monster_walk_target_value(view, pos)
    }
}

/// `Guardian.registerGoals` 的 `goalSelector`，按注册顺序。
///
/// The `goalSelector` half of `Guardian.registerGoals`, in registration order.
pub fn goals(kind: MobType) -> GoalSelector {
    // `ElderGuardian` 构造函数把 `randomStrollGoal.setInterval(400)`。
    // The `ElderGuardian` constructor calls `randomStrollGoal.setInterval(400)`.
    let stroll_interval = if kind == MobType::ElderGuardian {
        400
    } else {
        80
    };
    let mut selector = GoalSelector::from_goals(vec![
        wrap(4, Goal::GuardianAttack),
        wrap(
            5,
            Goal::MoveTowardsRestriction {
                speed: 1.0,
                wanted: Vec3::default(),
            },
        ),
        wrap(
            7,
            Goal::Stroll {
                speed: 1.0,
                interval: stroll_interval,
                probability: PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            8,
            Goal::LookAtMob {
                kind: Some(MobType::Guardian),
                look_distance: 12.0,
                probability: 0.01,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            9,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ]);
    // 官方 `randomStrollGoal.setFlags(EnumSet.of(MOVE, LOOK))`：只覆盖守卫者的
    // 游走目标（下标 2），不动其它生物的 `Goal::Stroll`。
    // Vanilla `randomStrollGoal.setFlags(EnumSet.of(MOVE, LOOK))`: override only the
    // guardian's stroll goal (index 2), leaving other mobs' `Goal::Stroll` alone.
    selector.set_flags(2, &[Flag::Move, Flag::Look]);
    selector
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Guardian.registerGoals` 的 `targetSelector`：优先级 1 的
/// `NearestAttackableTargetGoal(LivingEntity, 10, mustSee=true, mustReach=false,
/// GuardianAttackSelector)`。
///
/// The `targetSelector` half of `Guardian.registerGoals`: the priority-1
/// `NearestAttackableTargetGoal(LivingEntity, 10, mustSee=true, mustReach=false,
/// GuardianAttackSelector)`.
pub fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![wrap(
        1,
        Goal::nearest_target(TargetType::GuardianPrey),
    )])
}

/// `Guardian.getAmbientSound()` / `ElderGuardian.getAmbientSound()`：水中和岸上各一套。
///
/// `Guardian.getAmbientSound()` / `ElderGuardian.getAmbientSound()`: water and land variants.
pub fn ambient_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.ambient",
        (false, false) => "minecraft:entity.guardian.ambient_land",
        (true, true) => "minecraft:entity.elder_guardian.ambient",
        (true, false) => "minecraft:entity.elder_guardian.ambient_land",
    }
}

/// `Guardian.getHurtSound()` / `ElderGuardian.getHurtSound()`：水中和岸上各一套。
///
/// `Guardian.getHurtSound()` / `ElderGuardian.getHurtSound()`: water and land variants.
pub fn hurt_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.hurt",
        (false, false) => "minecraft:entity.guardian.hurt_land",
        (true, true) => "minecraft:entity.elder_guardian.hurt",
        (true, false) => "minecraft:entity.elder_guardian.hurt_land",
    }
}

/// `Guardian.getDeathSound()` / `ElderGuardian.getDeathSound()`：水中和岸上各一套。
///
/// `Guardian.getDeathSound()` / `ElderGuardian.getDeathSound()`: water and land variants.
pub fn death_sound(elder: bool, in_water: bool) -> &'static str {
    match (elder, in_water) {
        (false, true) => "minecraft:entity.guardian.death",
        (false, false) => "minecraft:entity.guardian.death_land",
        (true, true) => "minecraft:entity.elder_guardian.death",
        (true, false) => "minecraft:entity.elder_guardian.death_land",
    }
}

/// `Guardian.getFlopSound()` / `ElderGuardian.getFlopSound()`：岸上扑腾音。
///
/// `Guardian.getFlopSound()` / `ElderGuardian.getFlopSound()`: the land flop sound.
pub fn flop_sound(elder: bool) -> &'static str {
    if elder {
        "minecraft:entity.elder_guardian.flop"
    } else {
        "minecraft:entity.guardian.flop"
    }
}

/// `Guardian.GuardianMoveControl.tick`：转向、速度 lerp、正弦摆动、看向点 lerp。
///
/// `Guardian.GuardianMoveControl.tick`: turning, speed lerp, sinusoidal wobble and
/// look-at lerp.
pub fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if mob.mover.operation != control::Operation::MoveTo || mob.navigation.is_done() {
        living::set_speed(mob, 0.0);
        mob.guardian.moving = false;
        return;
    }
    let dx = mob.mover.wanted_x - me.position.x;
    let dy = mob.mover.wanted_y - me.position.y;
    let dz = mob.mover.wanted_z - me.position.z;
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    let nx = dx / len;
    let ny = dy / len;
    let nz = dz / len;
    let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
    me.y_rot = control::move_rotlerp(me.y_rot, yaw, 90.0);
    mob.y_body_rot = me.y_rot;
    let target_speed =
        (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
    let speed = mth::lerp_f32(0.125, mob.speed, target_speed);
    living::set_speed(mob, speed);

    // 官方这里用 `java.lang.Math.sin/cos`（不是 `Mth` 的查表）。
    // Vanilla uses `java.lang.Math.sin/cos` here, not the `Mth` lookup tables.
    let phase = f64::from(me.tick_count + me.id);
    let a = (phase * 0.5).sin() * 0.05;
    let radians = f64::from(me.y_rot * 0.017_453_292_f32);
    let c = radians.cos();
    let s = radians.sin();
    let b = (phase * 0.75).sin() * 0.05;
    let delta = me.delta;
    me.delta = Vec3::new(
        delta.x + a * c,
        delta.y + b * (s + c) * 0.25 + f64::from(speed) * ny * 0.1,
        delta.z + a * s,
    );

    let look_x = me.position.x + nx * 2.0;
    // 官方怪癖：`ny` 已经除过 `len`，这里又除一次。
    // Vanilla quirk: `ny` is already normalised, yet it is divided by `len` again.
    let look_y = living::eye_y(me, mob) + ny / len;
    let look_z = me.position.z + nz * 2.0;
    // `LookControl.isLookingAtTarget()` = `lookAtCooldown > 0`。
    let (mut wx, mut wy, mut wz) = (mob.look.wanted_x, mob.look.wanted_y, mob.look.wanted_z);
    if mob.look.look_at_cooldown <= 0 {
        wx = look_x;
        wy = look_y;
        wz = look_z;
    }
    mob.look.set_look_at(
        mth::lerp(0.125, wx, look_x),
        mth::lerp(0.125, wy, look_y),
        mth::lerp(0.125, wz, look_z),
        10.0,
        40.0,
    );
    mob.guardian.moving = true;
}

/// `Guardian.travelInWater`：常量 0.1 推进、移动、乘 0.9；没在动且没有目标时下沉 0.005。
///
/// `Guardian.travelInWater`: constant 0.1 thrust, move, scale by 0.9, and sink by
/// 0.005 when not moving and without a target.
pub fn travel_in_water(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    // 官方用常量 0.1F，不是 `getSpeed()`。
    // Vanilla uses the constant 0.1F, not `getSpeed()`.
    living::move_relative(me, 0.1, input);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
    // 官方这里读 `getTarget()`（`asValidTarget()`）：创造 / 旁观 / 死亡玩家算
    // 「没有目标」，照样下沉 0.005。Bamboo 的 `mob.target` 是未过滤的原始字段，
    // 用走同一套过滤的 `combat_target` 判断。
    //
    // Vanilla reads `getTarget()` (`asValidTarget()`): a creative/spectator/dead
    // player counts as "no target" and the guardian still sinks by 0.005. Use
    // `combat_target`, which applies the same filter, instead of the raw field.
    if !mob.guardian.moving && super::goals::combat_target(mob, ctx).is_none() {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y - 0.005, delta.z);
    }
}

/// `Guardian.aiStep` 的服务端部分：水里补满氧气、岸上扑腾、有激光目标时朝向头部转角。
/// `Monster.aiStep` 由调用方随后跑。客户端动画（尾巴 / 尖刺 / 气泡）不在服务端执行。
///
/// The server part of `Guardian.aiStep`: refill air in water, flop on land, and face
/// the head rotation while a laser target is active. The caller runs `Monster.aiStep`
/// afterwards. Client-side animations (tail / spikes / bubbles) do not run here.
pub fn ai_step(me: &mut Entity, mob: &mut Mob) {
    if !living::is_alive(me, mob) {
        return;
    }
    if me.in_water {
        // `Guardian.aiStep` 水里 `setAirSupply(300)`。
        // `Guardian.aiStep` sets `setAirSupply(300)` in water.
        mob.air_supply = 300;
    } else if me.on_ground {
        // 地上扑腾：三次 `nextFloat`（x、z 位移和朝向）。
        // Land flop: three `nextFloat` draws (x/z displacement and yaw).
        let x = (me.random.next_float() * 2.0 - 1.0) * 0.4;
        let z = (me.random.next_float() * 2.0 - 1.0) * 0.4;
        let delta = me.delta;
        me.delta = Vec3::new(
            delta.x + f64::from(x),
            delta.y + 0.5,
            delta.z + f64::from(z),
        );
        me.y_rot = me.random.next_float() * 360.0;
        me.on_ground = false;
        mob.needs_sync = true;
    }
    // `hasActiveAttackTarget()`：`DATA_ID_ATTACK_TARGET != 0`。
    // `hasActiveAttackTarget()`: `DATA_ID_ATTACK_TARGET != 0`.
    if mob.guardian.attack_target != 0 {
        me.y_rot = mob.y_head_rot;
    }
}

/// `ElderGuardian.customServerAiStep` 的相位：`(tickCount + id) % 1200 == 0`。
///
/// The phase of `ElderGuardian.customServerAiStep`: `(tickCount + id) % 1200 == 0`.
pub fn is_fatigue_tick(tick_count: i32, id: i32) -> bool {
    (tick_count + id) % 1200 == 0
}

/// `ElderGuardian.customServerAiStep`：每 1200 刻给半径 50 内的玩家挂挖掘疲劳，
/// 并给挂上的玩家发 `GUARDIAN_ELDER_EFFECT`；每刻都检查有没有家。
///
/// `ElderGuardian.customServerAiStep`: every 1200 ticks apply mining fatigue to
/// eligible players within radius 50 and send them `GUARDIAN_ELDER_EFFECT`; check
/// for a home every tick.
pub fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if is_fatigue_tick(me.tick_count, me.id) {
        let allied_team = ctx.blocks().entity_team(&mob.uuid);
        let mut affected = Vec::new();
        ctx.blocks().for_each_player(&mut |player| {
            if player_gets_fatigue(me.position, allied_team, player) {
                affected.push(player.uid);
            }
        });
        for uid in affected {
            ctx.emit(
                Command::AddPlayerEffect {
                    target: uid,
                    effect: mc_bamboo_data::registries::mob_effect::MINING_FATIGUE,
                    duration: 6000,
                    amplifier: 2,
                },
                me,
            );
            ctx.emit(
                Command::GuardianElderEffect {
                    target: uid,
                    silent: mob.silent,
                },
                me,
            );
        }
    }
    // `!hasHome() → setHomeTo(blockPosition(), 16)`：每刻都检查。
    // `!hasHome() → setHomeTo(blockPosition(), 16)`: checked every tick.
    if mob.home.is_none() {
        mob.home = Some((me.block_position(), 16));
    }
}

/// `MobEffectUtil.addEffectToPlayersAround` 的玩家筛选：生存或冒险、不是盟友、
/// 距离平方严格小于 50²、且（没有挖掘疲劳 / 现有等级 < 2 / 剩余时长 ≤ 1199）。
///
/// `MobEffectUtil.addEffectToPlayersAround`'s player predicate: survival or
/// adventure, not allied, squared distance strictly below 50², and (no mining
/// fatigue / amplifier below 2 / remaining duration at most 1199).
fn player_gets_fatigue(
    guardian_pos: Vec3,
    allied_team: Option<u32>,
    player: &crate::entity::ctx::PlayerView,
) -> bool {
    // `GameType.isSurvival()`：生存（0）和冒险（2）。
    // `GameType.isSurvival()`: survival (0) and adventure (2).
    if player.game_mode != 0 && player.game_mode != 2 {
        return false;
    }
    if allied_team.is_some_and(|team| player.team == Some(team)) {
        return false;
    }
    // `Vec3.closerThan(pos, 50.0)` 是严格小于。
    // `Vec3.closerThan(pos, 50.0)` is a strict less-than.
    if guardian_pos.distance_to_sqr(player.position) >= 2500.0 {
        return false;
    }
    match player.mining_fatigue {
        None => true,
        Some((amplifier, duration)) => amplifier < 2 || duration <= 1199,
    }
}

/// `Guardian.hurtServer` 反伤条件的类型部分：没在动、伤害不在
/// `#avoids_guardian_thorns`、也不是 thorns。「直接来源是活体」需要查实体表，
/// 由 [`hurt_server`] 单独判断。
///
/// The type part of `Guardian.hurtServer`'s reflection condition: not moving, damage
/// outside `#avoids_guardian_thorns` and not thorns. "The direct entity is living"
/// needs the entity table and is checked by [`hurt_server`].
pub fn thorns_reflects(mob: &Mob, kind: DamageKind) -> bool {
    let ignore_moving = thorns_ignore_moving(mob);
    (ignore_moving || !mob.guardian.moving)
        && !super::goals::damage_type_tagged("minecraft:avoids_guardian_thorns", kind)
        && kind != DamageKind::Thorns
}

#[cfg(feature = "oracle-samples")]
fn thorns_ignore_moving(mob: &Mob) -> bool {
    mob.guardian.thorns_ignore_moving
}

#[cfg(not(feature = "oracle-samples"))]
const fn thorns_ignore_moving(_mob: &Mob) -> bool {
    false
}

/// `Guardian.hurtServer`：没在动、伤害不在 `#avoids_guardian_thorns`、不是 thorns、
/// 且直接来源是活体时，对来源造成 2 点 `thorns` 伤害。无论反伤与否都触发游走，
/// 顺序照字节码（会影响随机源）。最后走 `Monster.hurtServer`。
///
/// `Guardian.hurtServer`: when not moving, the damage is outside
/// `#avoids_guardian_thorns`, is not thorns, and the direct entity is living, deal 2
/// points of thorns damage to that entity. The stroll is triggered either way, in
/// bytecode order (which affects the random source). Finally `Monster.hurtServer`.
pub fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    let direct = source.direct;
    let reflects = thorns_reflects(mob, source.kind)
        && direct.is_some_and(|uid| direct_entity_is_living(ctx, uid));
    if let Some(uid) = direct.filter(|_| reflects) {
        // `damageSources().thorns(this)`：来源是守卫者自己。
        // `damageSources().thorns(this)`: the guardian is the source.
        let mut thorns = DamageSource::environment(DamageKind::Thorns);
        thorns.attacker = Some(me.id);
        thorns.direct = Some(me.id);
        thorns.attacker_uuid = Some(mob.uuid);
        thorns.position = Some(me.position);
        thorns.scale_when_non_player = true;
        let is_player = {
            let mut found = false;
            ctx.blocks().for_each_player(&mut |player| {
                if player.uid == uid {
                    found = true;
                }
            });
            found
        };
        if is_player {
            ctx.emit(
                Command::HurtPlayer {
                    target: uid,
                    source: thorns,
                    amount: 2.0,
                },
                me,
            );
        } else {
            ctx.emit(
                Command::HurtMob {
                    target: uid,
                    source: thorns,
                    amount: 2.0,
                    on_hit: None,
                },
                me,
            );
        }
    }
    // `randomStrollGoal.trigger()`：无论反伤与否。
    // `randomStrollGoal.trigger()`: regardless of the thorns hit.
    mob.guardian.stroll_triggered = true;
    living::living_hurt_server(me, mob, ctx, source, amount)
}

/// `DamageSource.getDirectEntity() instanceof LivingEntity`：玩家或生物。
///
/// `DamageSource.getDirectEntity() instanceof LivingEntity`: a player or a mob.
fn direct_entity_is_living(ctx: &dyn EntityCtx, uid: crate::entity::EntityUid) -> bool {
    let mut found = false;
    ctx.blocks().for_each_player(&mut |player| {
        if player.uid == uid {
            found = true;
        }
    });
    found
        || ctx
            .other(uid)
            .is_some_and(|other| matches!(other.kind, KindPublic::Mob(_)))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{PathType, registries::attribute};

    use super::*;
    use crate::{
        BlockState, World,
        entity::mob::{Mob, MobType},
        state::BlockState as State,
        testing::TestLevel,
    };

    /// 激光视线缓存和朴素逐刻射线逐位相同：挡墙放下 / 拿走、版本戳变化、两端眼睛
    /// 位置（含 -0.0 这类原始位不同的值）变化、超 128 格、目标换人都覆盖到。
    /// 三样键都稳定时缓存必须命中。
    ///
    /// The laser sight cache is bit-identical to a naive ray every tick: placing
    /// and removing an occluding wall, stamp changes, both eye positions (including
    /// raw-bit differences like -0.0), the 128-block cutoff and a target swap.
    /// The cache must hit while all three keys are stable.
    #[test]
    fn laser_sight_cache_matches_naive_bit_for_bit() {
        let mut level = TestLevel::new();
        let mut ticks = crate::ticks::BlockTicks::default();
        // 从守卫者眼睛 (0.5, 62.85, 0.5) 到目标眼睛 (8.5, 63.62, 0.5) 的射线
        // 穿过 (4, 63, 0)：这一格放石头就看不见。
        let wall = BlockPos::new(4, 63, 0);
        let target_eye = Vec3::new(8.5, 63.62, 0.5);
        let mut me = Entity::by_mob_type(MobType::Guardian, Vec3::new(0.5, 62.0, 0.5));
        let mut cached = Mob::new(MobType::Guardian);
        let mut naive = Mob::new(MobType::Guardian);
        let hits = std::cell::Cell::new(0_usize);
        let rays = std::cell::Cell::new(0_usize);
        let mut check = |level: &mut TestLevel,
                         ticks: &mut crate::ticks::BlockTicks,
                         me: &Entity,
                         target_eye: Vec3,
                         uid: i32| {
            let world = World::new(level, ticks);
            // 每条路径都从本刻的空 `seen` 出发。
            cached.seen.clear();
            naive.seen.clear();
            let before = cached.guardian.sight_cache;
            let got = laser_line_of_sight(me, &mut cached, &world, uid, target_eye);
            let expected = laser_line_of_sight_naive(me, &mut naive, &world, uid, target_eye);
            assert_eq!(got, expected, "位置 {:?} 目标 {uid}", me.position);
            rays.set(rays.get() + 1);
            if before.is_some() && cached.guardian.sight_cache == before {
                hits.set(hits.get() + 1);
            }
        };
        // 一、空视线连续 20 刻：第一刻真射线，其余 19 刻命中缓存。
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(world.block_state(wall).is_air());
        }
        for _ in 0..20 {
            check(&mut level, &mut ticks, &me, target_eye, 7);
        }
        assert_eq!(hits.get(), 19, "稳定键下应该复用 19 次");
        // 二、放下挡墙：版本戳变了，缓存失效，两条路径都变成看不见。
        level.set(wall, State(mc_bamboo_data::blocks::STONE));
        for _ in 0..5 {
            check(&mut level, &mut ticks, &me, target_eye, 7);
        }
        assert!(!laser_line_of_sight_naive(
            &me,
            &mut Mob::new(MobType::Guardian),
            &World::new(&mut level, &mut ticks),
            7,
            target_eye
        ));
        // 三、拿走挡墙，再换目标 uid / 眼睛位 / 守卫者位置。
        level.set(wall, BlockState(0));
        check(&mut level, &mut ticks, &me, target_eye, 8);
        check(&mut level, &mut ticks, &me, Vec3::new(8.5, 63.62, -0.0), 8);
        check(&mut level, &mut ticks, &me, Vec3::new(8.5, 63.62, 0.0), 8);
        me.position.x = 0.5;
        me.position.y += 0.000_001;
        check(&mut level, &mut ticks, &me, target_eye, 8);
        // 四、超过 128 格：两条路径都直接返回假。
        check(&mut level, &mut ticks, &me, Vec3::new(200.5, 63.62, 0.5), 8);
        assert_eq!(rays.get(), 30);
        assert!(hits.get() > 19, "稳定键下还有额外命中");
    }

    /// 构造事实：属性、水代价、导航、持久化和音效分派都按官方类。
    ///
    /// Construction facts: attributes, water malus, navigation, persistence and the
    /// sound dispatch follow the vanilla classes.
    #[test]
    fn guardian_construction_facts() {
        let guardian = Mob::new(MobType::Guardian);
        assert_eq!(guardian.attributes.base(attribute::MAX_HEALTH), 30.0);
        assert_eq!(guardian.attributes.base(attribute::ATTACK_DAMAGE), 6.0);
        assert_eq!(guardian.attributes.base(attribute::MOVEMENT_SPEED), 0.5);
        assert_eq!(guardian.malus.get(PathType::Water), 0.0);
        assert_eq!(guardian.navigation.flags.swim, Some(false));
        assert!(!guardian.persistence_required);
        assert_eq!(guardian.guardian, GuardianState::default());

        let elder = Mob::new(MobType::ElderGuardian);
        assert_eq!(elder.attributes.base(attribute::MAX_HEALTH), 80.0);
        assert_eq!(elder.attributes.base(attribute::ATTACK_DAMAGE), 8.0);
        assert_eq!(
            elder.attributes.base(attribute::MOVEMENT_SPEED),
            0.30000001192092896
        );
        assert_eq!(elder.malus.get(PathType::Water), 0.0);
        assert_eq!(elder.navigation.flags.swim, Some(false));
        // `ElderGuardian` 构造函数 `setPersistenceRequired()`。
        // The `ElderGuardian` constructor calls `setPersistenceRequired()`.
        assert!(elder.persistence_required);
    }

    /// 目标表：优先级、看守卫者变体、以及远古守卫者的游走间隔 400。
    ///
    /// Goal table: priorities, the "watch guardians" variant and the elder
    /// guardian's stroll interval of 400.
    #[test]
    fn guardian_goal_table() {
        let table = goals(MobType::Guardian).goals;
        let priorities: Vec<i32> = table.iter().map(|wrapped| wrapped.priority).collect();
        assert_eq!(priorities, vec![4, 5, 7, 8, 8, 9]);
        assert!(matches!(table[0].goal, Goal::GuardianAttack));
        assert!(matches!(table[1].goal, Goal::MoveTowardsRestriction { .. }));
        assert!(matches!(
            table[2].goal,
            Goal::Stroll {
                interval: 80,
                probability,
                ..
            } if probability == PLAIN_STROLL
        ));
        assert!(matches!(
            table[3].goal,
            Goal::LookAtPlayer {
                look_distance,
                probability,
                ..
            } if look_distance == 8.0 && probability == 0.02
        ));
        assert!(matches!(
            table[4].goal,
            Goal::LookAtMob {
                kind: Some(MobType::Guardian),
                look_distance,
                probability,
                ..
            } if look_distance == 12.0 && probability == 0.01
        ));
        assert!(matches!(table[5].goal, Goal::RandomLookAround { .. }));

        // `ElderGuardian` 构造函数 `setInterval(400)`。
        // The `ElderGuardian` constructor calls `setInterval(400)`.
        let elder = goals(MobType::ElderGuardian).goals;
        assert!(matches!(elder[2].goal, Goal::Stroll { interval: 400, .. }));

        let targets = target_goals().goals;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].priority, 1);
        assert!(matches!(
            targets[0].goal,
            Goal::NearestAttackableTarget {
                kind: TargetType::GuardianPrey,
                must_see: true,
                ..
            }
        ));
    }

    /// 官方 `setFlags`：优先级 4/5/7 都是 MOVE|LOOK；7 的 `Stroll` 靠选择器层
    /// 覆盖，不改其它生物（下标 3 的 `LookAtPlayer` 仍是 LOOK）。
    ///
    /// Vanilla `setFlags`: priorities 4/5/7 are MOVE|LOOK; the stroll at 7 is
    /// overridden at the selector level, leaving other mobs' `Stroll` alone (index
    /// 3 `LookAtPlayer` is still LOOK).
    #[test]
    fn guardian_goal_flags() {
        let selector = goals(MobType::Guardian);
        assert_eq!(selector.flags(0), &[Flag::Move, Flag::Look]);
        assert_eq!(selector.flags(1), &[Flag::Move, Flag::Look]);
        assert_eq!(selector.flags(2), &[Flag::Move, Flag::Look]);
        assert_eq!(selector.flags(3), &[Flag::Look]);
    }

    /// `Guardian.getAttackDuration()`：普通 80，远古 60。
    ///
    /// `Guardian.getAttackDuration()`: 80 normally, 60 for the elder guardian.
    #[test]
    fn guardian_attack_duration() {
        assert_eq!(attack_duration(MobType::Guardian), 80);
        assert_eq!(attack_duration(MobType::ElderGuardian), 60);
    }

    /// 激光的目标距离下限与伤害：困难 / 远古各 +2。
    ///
    /// The laser's target distance floor and damage: hard / elder each add 2.
    #[test]
    fn guardian_laser_facts() {
        assert_eq!(MIN_TARGET_DISTANCE_SQR, 9.0);
        assert_eq!(laser_damage(false, false), 1.0);
        assert_eq!(laser_damage(false, true), 3.0);
        assert_eq!(laser_damage(true, false), 3.0);
        assert_eq!(laser_damage(true, true), 5.0);
    }

    /// 目标与生成规则按字节码：完整目标规则（从而有视线记忆）、`IN_WATER` 生成、
    /// 未覆写 `isPushedByFluid`（缺省为真）。
    ///
    /// Targeting and placement rules from bytecode: full target rules (hence sight
    /// memory), `IN_WATER` placement, and no `isPushedByFluid` override (default true).
    #[test]
    fn guardian_target_and_placement_rules() {
        assert!(MobType::Guardian.full_target_rules());
        assert!(MobType::Guardian.uses_target_memory());
        assert!(MobType::Guardian.spawns_in_water());
        assert!(MobType::ElderGuardian.spawns_in_water());
        assert!(MobType::Guardian.is_pushed_by_fluid());
    }

    /// `Guardian.aiStep`：水里补满氧气、岸上三次 `nextFloat` 扑腾。
    ///
    /// `Guardian.aiStep`: refill air in water, three `nextFloat` flop draws on land.
    #[test]
    fn guardian_ai_step_water_and_land() {
        let mut entity = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
        let air = entity
            .with_mob(|me, mob| {
                me.in_water = true;
                mob.air_supply = 123;
                ai_step(me, mob);
                mob.air_supply
            })
            .unwrap();
        assert_eq!(air, 300);

        let mut entity = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
        let (y_rot, delta_y, on_ground, needs_sync, random_after) = entity
            .with_mob(|me, mob| {
                me.in_water = false;
                me.on_ground = true;
                me.delta = Vec3::default();
                ai_step(me, mob);
                (
                    me.y_rot,
                    me.delta.y,
                    me.on_ground,
                    mob.needs_sync,
                    me.random.state(),
                )
            })
            .unwrap();
        assert!(!on_ground);
        assert!(needs_sync);
        assert_eq!(delta_y, 0.5);
        assert!((0.0..360.0).contains(&y_rot), "{y_rot}");
        // 三次 `nextFloat` 都取自实体随机源。
        // All three `nextFloat` draws come from the entity's random source.
        let mut reference = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
        reference.with_mob(|me, _| {
            for _ in 0..3 {
                me.random.next_float();
            }
        });
        assert_eq!(random_after, reference.random.state());
    }

    /// 导航结束时 `GuardianMoveControl.tick` 把速度和 `moving` 清零。
    ///
    /// `GuardianMoveControl.tick` zeroes speed and `moving` when navigation is done.
    #[test]
    fn guardian_move_control_idles_when_navigation_done() {
        let mut entity = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
        let (speed, moving) = entity
            .with_mob(|me, mob| {
                mob.mover.operation = control::Operation::MoveTo;
                mob.guardian.moving = true;
                move_control_tick(me, mob);
                (mob.speed, mob.guardian.moving)
            })
            .unwrap();
        assert_eq!(speed, 0.0);
        assert!(!moving);
    }

    /// 环境音经 `living::play_ambient_sound` 按水 / 岸分派，不留空。
    ///
    /// Ambience is dispatched by water / land through `living::play_ambient_sound`.
    #[test]
    fn guardian_ambient_sound_is_wired() {
        for (in_water, expected) in [
            (true, "minecraft:entity.guardian.ambient"),
            (false, "minecraft:entity.guardian.ambient_land"),
        ] {
            let mut entity = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
            entity.in_water = in_water;
            let sound = entity
                .with_mob(|me, mob| {
                    living::play_ambient_sound(me, mob);
                    mob.events.iter().find_map(|event| match event {
                        crate::entity::mob::MobEvent::Sound { sound, .. } => Some(*sound),
                        _ => None,
                    })
                })
                .unwrap();
            assert_eq!(sound, Some(expected));
        }
    }

    /// `MobEffectUtil.addEffectToPlayersAround` 的筛选：生存/冒险、非盟友、严格小于
    /// 50 格、以及挖掘疲劳的等级 / 时长条件。
    ///
    /// `MobEffectUtil.addEffectToPlayersAround`'s filter: survival/adventure, not
    /// allied, strictly within 50 blocks, and the fatigue amplifier / duration rules.
    #[test]
    fn elder_fatigue_player_filter() {
        use crate::entity::mob::throttle::test_player;

        let origin = Vec3::new(0.0, 64.0, 0.0);
        let mut player = test_player(7, origin);
        // 生存、站在守卫者身上、没有挖掘疲劳。
        player.game_mode = 0;
        player.position = origin;
        player.mining_fatigue = None;
        assert!(player_gets_fatigue(origin, None, &player));

        // 冒险也算 `isSurvival()`，创造和旁观不算。
        player.game_mode = 2;
        assert!(player_gets_fatigue(origin, None, &player));
        player.game_mode = 1;
        assert!(!player_gets_fatigue(origin, None, &player));
        player.game_mode = 3;
        assert!(!player_gets_fatigue(origin, None, &player));

        // 盟友（同队）跳过。
        player.game_mode = 0;
        player.team = Some(3);
        assert!(!player_gets_fatigue(origin, Some(3), &player));
        assert!(player_gets_fatigue(origin, Some(4), &player));
        player.team = None;

        // 距离平方严格小于 50² = 2500。
        player.position = Vec3::new(50.0, 64.0, 0.0);
        assert!(!player_gets_fatigue(origin, None, &player));
        player.position = Vec3::new(49.0, 64.0, 0.0);
        assert!(player_gets_fatigue(origin, None, &player));

        // 已有等级 2 且剩余时长 6000：不重挂；等级 < 2 或时长 ≤ 1199 则重挂。
        player.position = origin;
        player.mining_fatigue = Some((2, 6000));
        assert!(!player_gets_fatigue(origin, None, &player));
        player.mining_fatigue = Some((1, 6000));
        assert!(player_gets_fatigue(origin, None, &player));
        player.mining_fatigue = Some((2, 1199));
        assert!(player_gets_fatigue(origin, None, &player));
        player.mining_fatigue = Some((2, 1200));
        assert!(!player_gets_fatigue(origin, None, &player));
    }

    /// `(tickCount + id) % 1200 == 0` 的相位。
    ///
    /// The `(tickCount + id) % 1200 == 0` phase.
    #[test]
    fn elder_fatigue_phase() {
        assert!(is_fatigue_tick(1200, 0));
        assert!(is_fatigue_tick(1199, 1));
        assert!(is_fatigue_tick(0, 1200));
        assert!(!is_fatigue_tick(1199, 0));
        assert!(!is_fatigue_tick(1201, 0));
    }

    /// `Guardian.hurtServer` 的反伤条件：没在动、不在 `#avoids_guardian_thorns`、非 thorns。
    ///
    /// `Guardian.hurtServer`'s reflection condition: not moving, outside
    /// `#avoids_guardian_thorns`, and not thorns.
    #[test]
    fn guardian_thorns_conditions() {
        let mut entity = crate::entity::Entity::by_mob_type(MobType::Guardian, Vec3::default());
        entity.with_mob(|_, mob| {
            assert!(thorns_reflects(mob, DamageKind::MobAttack));
            mob.guardian.moving = true;
            assert!(!thorns_reflects(mob, DamageKind::MobAttack));
            mob.guardian.moving = false;
            assert!(!thorns_reflects(mob, DamageKind::Thorns));
            // `minecraft:magic` 在 `#avoids_guardian_thorns` 里。
            // `minecraft:magic` is in `#avoids_guardian_thorns`.
            assert!(!thorns_reflects(mob, DamageKind::Magic));
        });
    }

    #[test]
    fn guardian_sound_variants() {
        assert_eq!(
            ambient_sound(false, true),
            "minecraft:entity.guardian.ambient"
        );
        assert_eq!(
            ambient_sound(true, false),
            "minecraft:entity.elder_guardian.ambient_land"
        );
        assert_eq!(
            hurt_sound(false, false),
            "minecraft:entity.guardian.hurt_land"
        );
        assert_eq!(
            death_sound(true, true),
            "minecraft:entity.elder_guardian.death"
        );
        assert_eq!(flop_sound(true), "minecraft:entity.elder_guardian.flop");
        assert_eq!(flop_sound(false), "minecraft:entity.guardian.flop");
    }
}
