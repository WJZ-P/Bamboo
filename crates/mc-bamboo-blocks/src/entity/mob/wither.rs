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
        Entity, EntityKind,
        ctx::{Command, EntityCtx},
    },
    state::BlockState,
};

use super::{DamageKind, DamageSource, Mob, attributes::Attributes, goals::GoalSelector};

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

/// `WitherBoss.registerGoals` 的 `goalSelector`。
///
/// The `goalSelector` of `WitherBoss.registerGoals`.
pub(super) fn goals() -> GoalSelector {
    GoalSelector::default()
}

/// `WitherBoss.registerGoals` 的 `targetSelector`。
///
/// The `targetSelector` of `WitherBoss.registerGoals`.
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::default()
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
                ctx.emit(
                    Command::MobLevelEvent {
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
    if is_undead_attacker(source) {
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

/// `minecraft:wither_friends`（等于 `minecraft:undead`）标签里的实体类型 ID。
///
/// The entity-type ids in the `minecraft:wither_friends` (equal to `minecraft:undead`) tag.
const WITHER_FRIENDS: &[i32] = &[
    118, 131, 150, 119, 16, 98, 155, 20, 154, 157, 158, 153, 39, 68, 156, 149, 100,
];

/// `#wither_friends` 就是 `#undead` 标签，用攻击者实体类型判断。
///
/// `#wither_friends` is the `#undead` tag, decided from the attacker entity type.
fn is_undead_attacker(source: &DamageSource) -> bool {
    source
        .attacker_type
        .is_some_and(|kind| WITHER_FRIENDS.contains(&kind))
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
}
