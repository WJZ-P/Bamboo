//! `WitherBoss`：官方的 `Monster` → `WitherBoss` 专属状态与逐刻行为。
//!
//! 三个头的目标、无敌期、Boss 血条进度、半血护甲和破坏方块计时都记在 [`Wither`] 上。
//! `aiStep` 的飞行速度和侧头朝向、以及 `customServerAiStep` 的无敌期 / 爆炸 /
//! 破坏方块 / 回血，都在 [`living`] 的主 tick 里挂进来。射凋灵之首和目标选择器
//! 在别的模块接。
//!
//! `WitherBoss`: the vanilla `Monster` → `WitherBoss` state and per-tick behaviour.
//! The three head targets, invulnerability phase, boss-bar progress, half-health
//! armour and block-destruction clock live on [`Wither`]. The flight speed and side
//! head rotations of `aiStep` and the invulnerability / explosion / block destruction /
//! healing of `customServerAiStep` are wired into the main tick from [`living`].

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{registries::attribute, tags::block_tags};

use crate::{
    entity::{
        Entity, EntityKind, KindPublic,
        arrow::player_box,
        ctx::{Command, EntityCtx},
        wither_skull::WitherSkullShot,
    },
    state::BlockState,
};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::Attributes,
    goals::{CombatTarget, GoalSelector},
    living,
};

/// 无敌期长度，`WitherBoss.INVULNERABLE_TICKS`。
///
/// Length of the invulnerability phase, `WitherBoss.INVULNERABLE_TICKS`.
pub const INVULNERABLE_TICKS: i32 = 220;

/// 无敌期结束的爆炸半径（`explode(..., 7.0F, ..., MOB)`）。
///
/// Radius of the explosion when the invulnerability phase ends.
pub const SPAWN_EXPLOSION_RADIUS: f32 = 7.0;

/// 凋灵的专属状态，只在 `MobType::Wither` 分支读写。
///
/// Wither-only state, read and written only on the `MobType::Wither` branch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wither {
    /// `DATA_TARGET_A/B/C`：三个头各自的目标实体 ID（0 表示没有）。
    ///
    /// `DATA_TARGET_A/B/C`: each head's target entity id (0 means none).
    pub targets: [i32; 3],
    /// `DATA_ID_INV`：剩余无敌刻数。
    ///
    /// `DATA_ID_INV`: the remaining invulnerability ticks.
    pub invulnerable_ticks: i32,
    /// 两个侧头当前的俯仰 / 偏航角（`xRotHeads` / `yRotHeads`）。
    ///
    /// The two side heads' current pitch and yaw (`xRotHeads` / `yRotHeads`).
    pub x_rot_heads: [f32; 2],
    pub y_rot_heads: [f32; 2],
    /// 两个侧头上一刻的俯仰 / 偏航角（`xRotOHeads` / `yRotOHeads`）。
    ///
    /// The two side heads' previous pitch and yaw (`xRotOHeads` / `yRotOHeads`).
    pub x_rot_o_heads: [f32; 2],
    pub y_rot_o_heads: [f32; 2],
    /// `nextHeadUpdate`：两个侧头下次可射击的刻数。
    ///
    /// `nextHeadUpdate`: the tick at which each side head may next fire.
    pub next_head_update: [i32; 2],
    /// `idleHeadUpdates`：两个侧头空闲射击的累计次数。
    ///
    /// `idleHeadUpdates`: each side head's accumulated idle-shot count.
    pub idle_head_updates: [i32; 2],
    /// `destroyBlocksTick`：受伤后破坏方块的倒计时。
    ///
    /// `destroyBlocksTick`: the countdown to the block-destruction sweep after a hit.
    pub destroy_blocks_tick: i32,
    /// `ServerBossEvent` 当前进度。
    ///
    /// The current `ServerBossEvent` progress.
    pub boss_progress: f32,
}

impl Wither {
    /// `makeInvulnerable()`：把无敌刻数置 220、血条清零。血量由调用方按
    /// `getMaxHealth()/3` 设置，因为最大血量活在属性表上。
    ///
    /// `makeInvulnerable()`: set the invulnerability ticks to 220 and clear the
    /// boss bar. Health is set by the caller to `getMaxHealth()/3` because the
    /// maximum lives on the attribute table.
    pub fn make_invulnerable(&mut self) {
        self.invulnerable_ticks = INVULNERABLE_TICKS;
        self.boss_progress = 0.0;
    }
}

/// `isPowered()`：`getHealth() <= getMaxHealth()/2`。
///
/// `isPowered()`: `getHealth() <= getMaxHealth()/2`.
pub fn is_powered(health: f32, max_health: f32) -> bool {
    health <= max_health * 0.5
}

/// `WitherBoss.canDestroy`：`!isAir() && !is(WITHER_IMMUNE)`。
///
/// `WitherBoss.canDestroy`: `!isAir() && !is(WITHER_IMMUNE)`.
pub fn can_destroy(state: BlockState) -> bool {
    !state.is_air() && !state.is_tagged(block_tags::WITHER_IMMUNE)
}

/// `rotlerp(current, target, limit)`：折角差的绝对值封顶，再加回当前值。
///
/// `rotlerp(current, target, limit)`: clamp the wrapped difference, then add it back.
pub fn rotlerp(current: f32, target: f32, limit: f32) -> f32 {
    let mut delta = mth::wrap_degrees(target - current);
    if delta > limit {
        delta = limit;
    }
    if delta < -limit {
        delta = -limit;
    }
    current + delta
}

/// `getHeadX(head)`：中间头在身体正中，两侧头按身体朝向偏 1.3 格。
///
/// `getHeadX(head)`: the middle head is on the body axis; side heads are offset
/// by 1.3 blocks along the body rotation.
pub fn head_x(head: i32, x: f64, y_body_rot: f32, scale: f32) -> f64 {
    if head <= 0 {
        return x;
    }
    let radians = (y_body_rot + 180.0 * (head - 1) as f32) * 0.017_453_292_f32;
    x + f64::from(mth::cos(f64::from(radians)) * 1.3_f32) * f64::from(scale)
}

/// `getHeadY(head)`：中间头高 3 格，两侧头高 2.2 格。
///
/// `getHeadY(head)`: 3 blocks for the middle head, 2.2 for the side heads.
pub fn head_y(head: i32, y: f64, scale: f32) -> f64 {
    let offset = if head <= 0 { 3.0_f32 } else { 2.2_f32 };
    y + f64::from(offset) * f64::from(scale)
}

/// `getHeadZ(head)`。
pub fn head_z(head: i32, z: f64, y_body_rot: f32, scale: f32) -> f64 {
    if head <= 0 {
        return z;
    }
    let radians = (y_body_rot + 180.0 * (head - 1) as f32) * 0.017_453_292_f32;
    z + f64::from(mth::sin(f64::from(radians)) * 1.3_f32) * f64::from(scale)
}

/// 无敌期的 Boss 血条进度：`1 - invul/220`。
///
/// Boss-bar progress during the invulnerability phase: `1 - invul/220`.
pub fn invulnerable_progress(invulnerable_ticks: i32) -> f32 {
    1.0 - (invulnerable_ticks as f32) / (INVULNERABLE_TICKS as f32)
}

fn max_health(mob: &Mob) -> f32 {
    mob.attributes.value(attribute::MAX_HEALTH) as f32
}

fn heal(mob: &mut Mob, amount: f32) {
    mob.health = (mob.health + amount).min(max_health(mob));
}

/// `WitherBoss.createAttributes()`：`Monster.createMonsterAttributes()` 上覆盖五个值，
/// 并追加 `FLYING_SPEED`。
///
/// `WitherBoss.createAttributes()`: five overrides plus an added `FLYING_SPEED`
/// on `Monster.createMonsterAttributes()`.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, 300.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.6_f32));
    table.set_base(attribute::FOLLOW_RANGE, 40.0);
    table.set_base(attribute::ARMOR, 4.0);
    if table.get(attribute::FLYING_SPEED).is_none() {
        table.entries.push(super::attributes::Attribute::new(
            attribute::FLYING_SPEED,
            f64::from(0.6_f32),
        ));
    }
    table
}

/// `#wither_friends`（等于 `#undead`）标签里的实体类型 ID。
///
/// The entity-type ids in the `minecraft:wither_friends` (equal to `minecraft:undead`) tag.
const WITHER_FRIENDS: &[i32] = &[
    118, 131, 150, 119, 16, 98, 155, 20, 154, 157, 158, 153, 39, 68, 156, 149, 100,
];

/// `LIVING_ENTITY_SELECTOR` / `hurtServer` 的 `#wither_friends` 判定：
/// 用攻击者 / 候选目标的实体类型 ID 查标签。
///
/// The `#wither_friends` test of `LIVING_ENTITY_SELECTOR` / `hurtServer`: look the
/// entity type up in the tag.
pub(crate) fn is_wither_friend(kind: MobType) -> bool {
    WITHER_FRIENDS.contains(&kind.entity_type())
}

/// `WitherBoss.registerGoals` 的 `goalSelector`：
/// `0 WitherDoNothingGoal`、`2 RangedAttackGoal`、`5 WaterAvoidingRandomFlyingGoal`、
/// `6 LookAtPlayerGoal`、`7 RandomLookAroundGoal`。
///
/// The `goalSelector` of `WitherBoss.registerGoals`.
pub(super) fn goals() -> GoalSelector {
    use super::goals::Goal;
    GoalSelector::from_ordered_pairs(vec![
        (0, Goal::WitherDoNothing),
        (
            2,
            Goal::WitherRangedAttack {
                target: None,
                attack_time: -1,
                see_time: 0,
            },
        ),
        (
            5,
            Goal::WitherRandomFlying {
                speed: 1.0,
                interval: 120,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            6,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            7,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `WitherBoss.registerGoals` 的 `targetSelector`：
/// `1 HurtByTargetGoal`、`2 NearestAttackableTargetGoal<LivingEntity>`（不打亡灵）。
/// 官方 `randomInterval` 传 0，所以 `interval` 为 0、每刻都搜目标。
///
/// The `targetSelector` of `WitherBoss.registerGoals`; vanilla passes 0 for
/// `randomInterval`, so the interval is 0 and the search runs every tick.
pub(super) fn target_goals() -> GoalSelector {
    use super::goals::{Goal, TargetType};
    GoalSelector::from_ordered_pairs(vec![
        (
            1,
            Goal::HurtByTarget {
                timestamp: 0,
                unseen_ticks: 0,
            },
        ),
        (
            2,
            Goal::NearestAttackableTarget {
                kind: TargetType::WitherPrey,
                interval: 0,
                unseen_ticks: 0,
                found: None,
                must_see: false,
                light_gate: false,
            },
        ),
    ])
}

/// `WitherBoss.aiStep` 的前半：追主目标（头 0）时的飞行速度，在 `super.aiStep()` 之前。
///
/// The first half of `WitherBoss.aiStep`: the flight speed while chasing the main
/// target (head 0), ahead of `super.aiStep()`.
pub(super) fn ai_step_flight(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) {
    let mut delta = me.delta;
    delta.y *= 0.6;
    let powered = is_powered(mob.health, max_health(mob));
    let target = mob.wither.targets[0];
    if target > 0
        && let Some(other) = ctx.other(target)
    {
        let mut dy = delta.y;
        if me.position.y < other.position.y || (!powered && me.position.y < other.position.y + 5.0)
        {
            dy = dy.max(0.0) + 0.3 - dy * 0.6;
        }
        delta.y = dy;
        let dx = other.position.x - me.position.x;
        let dz = other.position.z - me.position.z;
        let horizontal_sqr = dx * dx + dz * dz;
        if horizontal_sqr > 9.0 {
            let length = horizontal_sqr.sqrt();
            delta.x += dx / length * 0.3 - delta.x * 0.6;
            delta.z += dz / length * 0.3 - delta.z * 0.6;
        }
    }
    me.delta = delta;
    if delta.x * delta.x + delta.z * delta.z > 0.05 {
        me.y_rot = (mth::atan2(delta.z, delta.x) * 57.295_776) as f32 - 90.0;
    }
}

/// `WitherBoss.aiStep` 的后半：两个侧头朝各自目标转，`super.aiStep()` 之后。
/// 官方在这里播的烟雾 / 实体效果粒子只影响客户端表现和实体随机源，这里不取数。
///
/// The tail of `WitherBoss.aiStep`: the two side heads turn towards their targets,
/// after `super.aiStep()`. Vanilla's smoke / entity-effect particles here only
/// affect the client and the entity random source; no draws are made here.
pub(super) fn ai_step_heads(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    for head in 0..2 {
        mob.wither.y_rot_o_heads[head] = mob.wither.y_rot_heads[head];
        mob.wither.x_rot_o_heads[head] = mob.wither.x_rot_heads[head];
    }
    for head in 0..2 {
        let target = mob.wither.targets[head + 1];
        if target > 0
            && let Some(other) = ctx.other(target)
        {
            let hx = head_x(head as i32 + 1, me.position.x, mob.y_body_rot, 1.0);
            let hy = head_y(head as i32 + 1, me.position.y, 1.0);
            let hz = head_z(head as i32 + 1, me.position.z, mob.y_body_rot, 1.0);
            let dx = other.position.x - hx;
            let dz = other.position.z - hz;
            let eye_y = match &other.kind {
                crate::entity::KindPublic::Mob(public) => {
                    other.position.y + f64::from(public.mob_type.dimensions(false).2)
                }
                _ => other.position.y + 1.0,
            };
            let dy = eye_y - hy;
            let horizontal = (dx * dx + dz * dz).sqrt();
            let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
            let pitch = -(mth::atan2(dy, horizontal) * 57.295_776_367_187_5) as f32;
            mob.wither.x_rot_heads[head] = rotlerp(mob.wither.x_rot_heads[head], pitch, 40.0);
            mob.wither.y_rot_heads[head] = rotlerp(mob.wither.y_rot_heads[head], yaw, 10.0);
        } else {
            mob.wither.y_rot_heads[head] =
                rotlerp(mob.wither.y_rot_heads[head], mob.y_body_rot, 10.0);
        }
    }
}

/// `WitherBoss.aiStep` 的粒子段：服务端不发包，但参数里的实体随机源照取不误；
/// 半血时每头还要取一次世界随机源 `level.getRandom().nextInt(4)`。
///
/// The particle block of `WitherBoss.aiStep`: the server sends no packets but still
/// draws the entity's random source; at half health each head also draws the world
/// random's `level.getRandom().nextInt(4)`.
pub(super) fn ai_step_particles(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) {
    let powered = is_powered(mob.health, max_health(mob));
    for _ in 0..3 {
        // 每个头的 SMOKE 粒子：三次 `random.nextGaussian()`。
        // Each head's SMOKE particle: three `random.nextGaussian()` draws.
        for _ in 0..3 {
            let _ = me.random.next_gaussian();
        }
        // 半血时每头一次 `level.getRandom().nextInt(4)`，为 0 再发 ENTITY_EFFECT
        // 粒子（又三次高斯）。
        // At half health each head draws `level.getRandom().nextInt(4)`; a zero adds
        // an ENTITY_EFFECT particle (three more gaussians).
        if powered && ctx.blocks().level_random_int(4) == 0 {
            for _ in 0..3 {
                let _ = me.random.next_gaussian();
            }
        }
    }
    // 无敌期：三个 ENTITY_EFFECT 粒子，每个按
    // `nextGaussian / nextFloat / nextGaussian` 取数。
    // Invulnerability phase: three ENTITY_EFFECT particles, each drawing
    // `nextGaussian / nextFloat / nextGaussian`.
    if mob.wither.invulnerable_ticks > 0 {
        for _ in 0..3 {
            let _ = me.random.next_gaussian();
            let _ = me.random.next_float();
            let _ = me.random.next_gaussian();
        }
    }
}

/// `RangedAttackGoal(wither, 1.0, 40, 20.0F)` 的参数：`speedModifier=1.0`、
/// `attackIntervalMin=attackIntervalMax=40`、`attackRadius=20.0`。
///
/// Parameters of `RangedAttackGoal(wither, 1.0, 40, 20.0F)`.
const RANGED_SPEED: f64 = 1.0;
const RANGED_RADIUS: f32 = 20.0;
const RANGED_INTERVAL: i32 = 40;

/// `RangedAttackGoal.tick`（凋灵）：算距离和视线，近且看见够 5 刻就停下导航，
/// 否则以 `speedModifier` 追；盯住目标；`attackTime` 到 0 且看得见时射主头
/// （第 3 步接 `performRangedAttack`），随后重设为 40。
///
/// `RangedAttackGoal.tick` (wither): distance and sight; stop when close and seen
/// for 5 ticks, otherwise chase; look at the target; fire the main head when the
/// timer reaches 0 (wired to `performRangedAttack` in step 3), then reset to 40.
pub(super) fn ranged_attack_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
    attack_time: &mut i32,
    see_time: &mut i32,
) {
    let distance_sqr = me.position.distance_to_sqr(target.position);
    let seen = living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position());
    *see_time = if seen { *see_time + 1 } else { 0 };
    let radius_sqr = f64::from(RANGED_RADIUS * RANGED_RADIUS);
    if distance_sqr <= radius_sqr && *see_time >= 5 {
        mob.navigation.stop();
    } else {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation
            .move_to_entity(ctx.blocks(), &nav, target.position, RANGED_SPEED, 1);
    }
    mob.look.set_look_at(
        target.position.x,
        target.eye_y(),
        target.position.z,
        30.0,
        30.0,
    );
    *attack_time -= 1;
    if *attack_time == 0 {
        if !seen {
            return;
        }
        // `RangedAttackMob.performRangedAttack(target, f)` → 主头（head 0）。
        // `RangedAttackMob.performRangedAttack(target, f)` → the main head (head 0).
        perform_ranged_attack_entity(me, mob, ctx, 0, &target);
        *attack_time = RANGED_INTERVAL;
    } else if *attack_time < 0 {
        *attack_time = RANGED_INTERVAL;
    }
}

/// `WitherBoss.performRangedAttack(head, x, y, z, dangerous)`：非静音时发 1024 号
/// 世界事件，从该头的位置朝目标点生成一枚凋灵之首。
///
/// `WitherBoss.performRangedAttack(head, x, y, z, dangerous)`: send world event
/// 1024 when not silent, then spawn a wither skull from the head towards the point.
#[expect(
    clippy::too_many_arguments,
    reason = "官方 performRangedAttack 就是这几个参数"
)]
fn perform_ranged_attack(
    me: &mut Entity,
    mob: &Mob,
    ctx: &mut dyn EntityCtx,
    head: i32,
    tx: f64,
    ty: f64,
    tz: f64,
    dangerous: bool,
) {
    if !mob.silent {
        ctx.emit(
            Command::MobLevelEvent {
                event: 1024,
                pos: me.block_position(),
                data: 0,
            },
            me,
        );
    }
    let hx = head_x(head, me.position.x, mob.y_body_rot, 1.0);
    let hy = head_y(head, me.position.y, 1.0);
    let hz = head_z(head, me.position.z, mob.y_body_rot, 1.0);
    ctx.emit(
        Command::ShootWitherSkull(Box::new(WitherSkullShot {
            position: Vec3::new(hx, hy, hz),
            direction: Vec3::new(tx - hx, ty - hy, tz - hz),
            dangerous,
            owner: Some(me.id),
            owner_uuid: Some(mob.uuid),
        })),
        me,
    );
}

/// `WitherBoss.performRangedAttack(head, LivingEntity)`：瞄准 `getY() + eyeHeight*0.5`，
/// 主头（head 0）另有 0.1% 概率抽成蓝色危险头。
///
/// `WitherBoss.performRangedAttack(head, LivingEntity)`: aim at
/// `getY() + eyeHeight*0.5`; the main head (head 0) has a 0.1% dangerous roll.
fn perform_ranged_attack_entity(
    me: &mut Entity,
    mob: &Mob,
    ctx: &mut dyn EntityCtx,
    head: i32,
    target: &CombatTarget,
) {
    let eye_height = target.eye_y() - target.position.y;
    let ty = target.position.y + eye_height * 0.5;
    let dangerous = head == 0 && me.random.next_float() < 0.001;
    perform_ranged_attack(
        me,
        mob,
        ctx,
        head,
        target.position.x,
        ty,
        target.position.z,
        dangerous,
    );
}

/// `WitherBoss.customServerAiStep` 里两个侧头（head 1 / 2）的射击循环：
/// `nextHeadUpdate` 到点就抽 10 刻内的下次更新、NORMAL/HARD 下累计空闲射击到 16 次
/// 朝随机点发蓝色头；有目标且可攻击 / 看得见就朝它射、清空闲计数；没目标就按
/// `TARGETING_CONDITIONS` 找附近非亡灵活体随机设一个目标。
///
/// The two side-head firing loop of `WitherBoss.customServerAiStep`.
fn side_head_attacks(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let difficulty = ctx.blocks().difficulty();
    for i in 1..3usize {
        if me.tick_count < mob.wither.next_head_update[i - 1] {
            continue;
        }
        mob.wither.next_head_update[i - 1] = me.tick_count + 10 + me.random.next_int_bounded(10);
        if difficulty == 2 || difficulty == 3 {
            mob.wither.idle_head_updates[i - 1] += 1;
            if mob.wither.idle_head_updates[i - 1] > 15 {
                // `Mth.nextDouble(random, x-10, x+10)` 三连。
                // Three `Mth.nextDouble(random, x-10, x+10)` draws.
                let x = me.position.x - 10.0 + me.random.next_double() * 20.0;
                let y = me.position.y - 5.0 + me.random.next_double() * 10.0;
                let z = me.position.z - 10.0 + me.random.next_double() * 20.0;
                perform_ranged_attack(me, mob, ctx, (i + 1) as i32, x, y, z, true);
                mob.wither.idle_head_updates[i - 1] = 0;
            }
        }
        let head = (i + 1) as i32;
        let target_id = mob.wither.targets[i];
        if target_id > 0 {
            let found = super::goals::target_view(ctx, target_id);
            let valid = found.as_ref().is_some_and(|(target, _)| {
                me.position.distance_to_sqr(target.position) <= 900.0
                    && living::has_line_of_sight(
                        me,
                        mob,
                        ctx.blocks(),
                        target_id,
                        target.eye_position(),
                    )
            });
            if valid {
                let (target, _) = found.expect("checked above");
                perform_ranged_attack_entity(me, mob, ctx, head, &target);
                mob.wither.next_head_update[i - 1] =
                    me.tick_count + 40 + me.random.next_int_bounded(20);
                mob.wither.idle_head_updates[i - 1] = 0;
            } else {
                mob.wither.targets[i] = 0;
            }
        } else {
            // `getNearbyEntities(LivingEntity, ..., AABB.inflate(20, 8, 20))`：官方收所有
            // 符合 `LIVING_ENTITY_SELECTOR` 的 `LivingEntity`，玩家也算（侧头会自己打玩家）。
            // `getNearbyEntities(LivingEntity, ..., AABB.inflate(20, 8, 20))`: vanilla
            // collects every `LivingEntity` matching `LIVING_ENTITY_SELECTOR`, players
            // included (side heads target players on their own).
            let area = me.bounding_box().inflate(20.0, 8.0, 20.0);
            let mut candidates = Vec::new();
            ctx.for_each_other_in(area, &mut |other| {
                if let KindPublic::Mob(public) = &other.kind
                    && other.uid != me.id
                    && !other.removed
                    && public.is_alive()
                    && !is_wither_friend(public.mob_type)
                {
                    candidates.push(other.uid);
                }
            });
            ctx.blocks().for_each_player(&mut |player| {
                // 玩家不是 `#wither_friends`；`attackable()` 排除旁观 / 创造模式，
                // 包围盒要落在扫描范围内。
                // A player is not `#wither_friends`; `attackable()` drops spectators
                // and creative players, and the box must lie in the scanned area.
                if player.alive
                    && !player.spectator
                    && !player.creative
                    && player_box(player).intersects(area)
                {
                    candidates.push(player.uid);
                }
            });
            if !candidates.is_empty() {
                let pick = me.random.next_int_bounded(candidates.len() as i32) as usize;
                mob.wither.targets[i] = candidates[pick];
            }
        }
    }
}

/// `WitherBoss.customServerAiStep`：无敌期、爆炸、破坏方块、每刻回血和 Boss 血条进度。
/// 无敌期直接 `return`，不跑 `super.customServerAiStep` 的那一支。
///
/// `WitherBoss.customServerAiStep`: invulnerability phase, explosion, block
/// destruction, periodic healing and boss-bar progress. The invulnerable branch
/// returns early and skips the `super.customServerAiStep` part.
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.wither.invulnerable_ticks > 0 {
        let next = mob.wither.invulnerable_ticks - 1;
        mob.wither.boss_progress = invulnerable_progress(next);
        if next <= 0 {
            spawn_explosion(me, mob, ctx);
            if !mob.silent {
                // 官方 `globalLevelEvent(1023, blockPosition(), 0)`：全服广播。
                // Vanilla `globalLevelEvent(1023, blockPosition(), 0)`: server-wide broadcast.
                ctx.emit(
                    Command::GlobalLevelEvent {
                        event: 1023,
                        pos: me.block_position(),
                        data: 0,
                    },
                    me,
                );
            }
        }
        mob.wither.invulnerable_ticks = next;
        if me.tick_count % 10 == 0 {
            heal(mob, 10.0);
        }
        return;
    }

    // 两个侧头的射击循环（官方在 `setAlternativeTarget(0, ...)` 之前）。
    // The two side-head firing loop (vanilla runs it before `setAlternativeTarget(0, ...)`).
    side_head_attacks(me, mob, ctx);

    // `setAlternativeTarget(0, getTarget() == null ? 0 : getTarget().getId())`。
    // `setAlternativeTarget(0, getTarget() == null ? 0 : getTarget().getId())`.
    mob.wither.targets[0] = mob.target.unwrap_or(0);

    // `destroyBlocksTick` 到 0 且开着 `mobGriefing` 时扫周围方块。
    // When `destroyBlocksTick` reaches 0 and `mobGriefing` is on, sweep nearby blocks.
    if mob.wither.destroy_blocks_tick > 0 {
        mob.wither.destroy_blocks_tick -= 1;
        if mob.wither.destroy_blocks_tick == 0 && ctx.blocks().block_game_rules().mob_griefing {
            destroy_blocks(me, mob, ctx);
        }
    }
    if me.tick_count % 20 == 0 {
        heal(mob, 1.0);
    }
    mob.wither.boss_progress = (mob.health / max_health(mob)).clamp(0.0, 1.0);
}

/// 无敌期结束的 `explode(this, getX(), getEyeY(), getZ(), 7.0F, false, MOB)`。
///
/// The `explode(this, getX(), getEyeY(), getZ(), 7.0F, false, MOB)` at the end of
/// the invulnerability phase.
fn spawn_explosion(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let source = {
        let mut whole = me.clone();
        whole.kind = EntityKind::Mob(Box::new(mob.clone()));
        Box::new(whole.split().public)
    };
    let eye_y = me.position.y + f64::from(mob.eye_height);
    ctx.emit(
        Command::ExplodeMob {
            center: Vec3::new(me.position.x, eye_y, me.position.z),
            radius: SPAWN_EXPLOSION_RADIUS,
            source,
        },
        me,
    );
}

/// 每 20 刻受伤后的一次 `BlockPos.betweenClosed` 扫描，逐格发 `destroyBlock(pos, true, this)`。
///
/// The `BlockPos.betweenClosed` sweep after a hit: emit `destroyBlock(pos, true, this)`
/// for every destroyable cell.
fn destroy_blocks(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let (width, height) = mob.dimensions();
    let half = mth::floor(f64::from(width) / 2.0 + 1.0);
    let top = mth::floor(f64::from(height));
    let origin = me.block_position();
    let view = ctx.blocks();
    let mut cells = Vec::new();
    for x in (origin.x - half)..=(origin.x + half) {
        for y in origin.y..=(origin.y + top) {
            for z in (origin.z - half)..=(origin.z + half) {
                let pos = BlockPos::new(x, y, z);
                if can_destroy(view.block_state(pos)) {
                    cells.push(pos);
                }
            }
        }
    }
    if !cells.is_empty() {
        for pos in cells {
            ctx.emit(Command::DestroyBlock { pos, drop: true }, me);
        }
        ctx.emit(
            Command::MobLevelEvent {
                event: 1022,
                pos: origin,
                data: 0,
            },
            me,
        );
    }
}

/// `WitherBoss.hurtServer` 的专属闸门：返回 `Some(false)` 表示这一击被挡下，
/// `None` 表示继续走 `super.hurtServer`。挡下时不写 `destroyBlocksTick` /
/// `idleHeadUpdates`；放行前先按要求把它们推进。
///
/// The `WitherBoss.hurtServer` gate: `Some(false)` rejects the hit, `None` lets it
/// continue into `super.hurtServer`. A rejected hit skips the `destroyBlocksTick` /
/// `idleHeadUpdates` updates; an accepted one advances them first.
pub(super) fn hurt_server_guard(mob: &mut Mob, source: &DamageSource) -> Option<bool> {
    // `#wither_immune_to` 只有 `drown`；另一个凋灵造成的伤害也无效。
    // `#wither_immune_to` is only `drown`; another wither's damage is also void.
    if source.kind == DamageKind::Drown
        || source.attacker_type == Some(mc_bamboo_data::registries::entity_type::WITHER)
    {
        return Some(false);
    }
    // 无敌期，除非伤害能穿透无敌。
    // Invulnerability phase, unless the damage bypasses invulnerability.
    if mob.wither.invulnerable_ticks > 0 && !source.kind.bypasses_invulnerability() {
        return Some(false);
    }
    // 半血护甲：箭和风弹被弹开。
    // Half-health armour: arrows and wind charges are deflected.
    if is_powered(mob.health, max_health(mob))
        && matches!(source.kind, DamageKind::Arrow | DamageKind::WindCharge)
    {
        return Some(false);
    }
    // 亡灵（`#wither_friends`）打不动凋灵。
    // Undead (`#wither_friends`) cannot hurt the wither.
    if source
        .attacker_type
        .is_some_and(|kind| WITHER_FRIENDS.contains(&kind))
    {
        return Some(false);
    }
    if mob.wither.destroy_blocks_tick <= 0 {
        mob.wither.destroy_blocks_tick = 20;
    }
    for idle in &mut mob.wither.idle_head_updates {
        *idle += 3;
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::entity::mob::MobType;

    use super::*;

    #[test]
    fn attributes_match_the_official_table() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 300.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), f64::from(0.6_f32));
        assert_eq!(table.value(attribute::FLYING_SPEED), f64::from(0.6_f32));
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 40.0);
        assert_eq!(table.value(attribute::ARMOR), 4.0);
    }

    #[test]
    fn make_invulnerable_starts_the_220_tick_phase() {
        let mut state = Wither {
            boss_progress: 0.75,
            ..Default::default()
        };
        state.make_invulnerable();
        assert_eq!(state.invulnerable_ticks, 220);
        assert_eq!(state.boss_progress, 0.0);
    }

    #[test]
    fn is_powered_at_half_health() {
        assert!(is_powered(150.0, 300.0));
        assert!(is_powered(100.0, 300.0));
        assert!(!is_powered(151.0, 300.0));
    }

    #[test]
    fn can_destroy_rejects_air_and_wither_immune() {
        assert!(!can_destroy(BlockState::AIR));
        assert!(!can_destroy(BlockState(mc_bamboo_data::blocks::BEDROCK)));
        assert!(can_destroy(BlockState(mc_bamboo_data::blocks::STONE)));
    }

    #[test]
    fn rotlerp_clamps_to_the_limit() {
        assert_eq!(rotlerp(0.0, 90.0, 10.0), 10.0);
        assert_eq!(rotlerp(0.0, -90.0, 10.0), -10.0);
        assert_eq!(rotlerp(100.0, 105.0, 10.0), 105.0);
    }

    #[test]
    fn head_positions_match_the_official_offsets() {
        assert_eq!(head_x(0, 5.0, 0.0, 1.0), 5.0);
        assert_eq!(head_z(0, 5.0, 0.0, 1.0), 5.0);
        assert!((head_y(0, 10.0, 1.0) - 13.0).abs() < 1e-6);
        assert!((head_y(1, 10.0, 1.0) - 12.2).abs() < 1e-6);
        assert!((head_x(1, 0.0, 0.0, 1.0) - 1.3).abs() < 1e-5);
    }

    #[test]
    fn invulnerable_progress_counts_up_to_one() {
        assert_eq!(invulnerable_progress(220), 0.0);
        assert_eq!(invulnerable_progress(0), 1.0);
        assert!((invulnerable_progress(110) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn half_health_armour_deflects_arrows() {
        let mut mob = Mob::new(MobType::Wither);
        mob.health = 150.0;
        let arrow = DamageSource::environment(DamageKind::Arrow);
        assert_eq!(hurt_server_guard(&mut mob, &arrow), Some(false));
        assert_eq!(mob.wither.destroy_blocks_tick, 0);
        // 未进入护甲状态时箭能打中，并启动破坏方块倒计时。
        mob.health = 300.0;
        assert_eq!(hurt_server_guard(&mut mob, &arrow), None);
        assert_eq!(mob.wither.destroy_blocks_tick, 20);
        assert_eq!(mob.wither.idle_head_updates, [3, 3]);
    }

    #[test]
    fn invulnerable_phase_rejects_normal_damage() {
        let mut mob = Mob::new(MobType::Wither);
        mob.wither.make_invulnerable();
        let source = DamageSource::environment(DamageKind::MobAttack);
        assert_eq!(hurt_server_guard(&mut mob, &source), Some(false));
        assert_eq!(mob.wither.destroy_blocks_tick, 0);
    }

    /// `WitherBoss.registerGoals` 的注册顺序、优先级和运行名照官方。
    ///
    /// The registration order, priorities and running names of `WitherBoss.registerGoals`.
    #[test]
    fn goal_tables_match_the_official_registration() {
        use crate::entity::mob::goals::{Flag, Goal};

        let selector = goals();
        let pairs: Vec<_> = selector
            .goals
            .iter()
            .map(|wrapped| (wrapped.priority, wrapped.goal.name()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                (0, "WitherDoNothingGoal"),
                (2, "RangedAttackGoal"),
                (5, "WaterAvoidingRandomFlyingGoal"),
                (6, "LookAtPlayerGoal"),
                (7, "RandomLookAroundGoal"),
            ]
        );
        assert_eq!(
            Goal::WitherDoNothing.flags(),
            &[Flag::Move, Flag::Look, Flag::Jump]
        );
        assert!(selector.goals[1].goal.every_tick());

        let targets = target_goals();
        let names: Vec<_> = targets
            .goals
            .iter()
            .map(|wrapped| wrapped.goal.name())
            .collect();
        assert_eq!(
            names,
            vec!["HurtByTargetGoal", "NearestAttackableTargetGoal"]
        );
    }

    /// `LIVING_ENTITY_SELECTOR` 排除 `#wither_friends`（亡灵），牛不是亡灵。
    ///
    /// `LIVING_ENTITY_SELECTOR` excludes `#wither_friends` (undead); cows are not undead.
    #[test]
    fn wither_friends_are_undead_only() {
        assert!(!is_wither_friend(MobType::Cow));
        assert!(is_wither_friend(MobType::Wither)); // 凋灵也在 #undead / #wither_friends 里
        assert!(is_wither_friend(MobType::Skeleton));
        assert!(is_wither_friend(MobType::Zombie));
        assert!(is_wither_friend(MobType::Drowned));
    }
}
