//! 凋灵之首，官方 `WitherSkull`（`AbstractHurtingProjectile` 的一支）。
//!
//! 飞行照 `AbstractHurtingProjectile.tick`：`applyInertia` 每刻沿速度方向加
//! `accelerationPower=0.1` 再按惯性缩放（普通 0.95、蓝色危险 0.73，水中 0.8 并发
//! 4 个气泡粒子），初速是归一化方向乘 0.1；沿本刻速度找方块和实体命中，命中就
//! 爆炸并消失。命中实体先造成 8 点 `wither_skull` 伤害，再按难度挂凋零效果；
//! 蓝色的爆炸把非 `#wither_immune` 方块的抗性压到 0.8，能真的炸掉方块。
//!
//! The wither skull, vanilla `WitherSkull` (an `AbstractHurtingProjectile`).
//! Flight follows `AbstractHurtingProjectile.tick`: `applyInertia` adds
//! `accelerationPower=0.1` along the velocity then scales by inertia (0.95
//! normal, 0.73 dangerous, 0.8 in water with four bubble particles); the
//! initial velocity is the normalized direction times 0.1. It searches for a
//! block/entity hit along this tick's movement and explodes on contact. An
//! entity hit deals 8 `wither_skull` damage plus a difficulty-scaled wither
//! effect; a dangerous skull caps destroyable blocks' blast resistance at 0.8.

use mc_bamboo_core::{Aabb, Direction, Vec3, mth};
use mc_bamboo_data::registries::{entity_type as types, mob_effect};

use super::{
    Entity, EntityKind, EntityUid, KindPublic,
    arrow::{clip_entity, clip_including_border, player_box, rotation},
    ctx::{Command, EntityCtx, PlayerView},
    mob::{DamageKind, DamageSource, effects::EffectInstance, living},
};
use crate::World;

/// `AbstractHurtingProjectile.getInertia()`（普通头）。
///
/// `AbstractHurtingProjectile.getInertia()` (normal skull).
pub const INERTIA: f32 = 0.95;
/// `WitherSkull.getInertia()`（蓝色危险头）。
///
/// `WitherSkull.getInertia()` (dangerous skull).
pub const DANGEROUS_INERTIA: f32 = 0.73;
/// `AbstractHurtingProjectile.getLiquidInertia()`（水中分支，危险头不覆盖）。
///
/// `AbstractHurtingProjectile.getLiquidInertia()` (water branch, not overridden).
pub const LIQUID_INERTIA: f32 = 0.8;
/// `AbstractHurtingProjectile.accelerationPower`：初速乘数与每刻加速量。
///
/// `AbstractHurtingProjectile.accelerationPower`: initial-speed factor and
/// the per-tick acceleration.
pub const ACCELERATION_POWER: f64 = 0.1;
/// `applyInertia` 水中分支每刻发的气泡粒子数。
///
/// Number of bubble particles `applyInertia` emits per water tick.
pub const WATER_BUBBLES: i32 = 4;
/// `WitherSkull.onHit` 的爆炸强度。
///
/// Blast power of `WitherSkull.onHit`.
pub const EXPLOSION_RADIUS: f32 = 1.0;
/// `WitherSkull.getBlockExplosionResistance` 对蓝色头可破坏方块的上限。
///
/// The resistance cap `WitherSkull.getBlockExplosionResistance` applies to
/// destroyable blocks for a dangerous skull.
pub const DANGEROUS_RESISTANCE_CAP: f32 = 0.8;

/// `WitherSkull` 自己的状态。
///
/// The `WitherSkull`'s own state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WitherSkull {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `DATA_DANGEROUS`：蓝色危险头。
    ///
    /// `DATA_DANGEROUS`: the blue dangerous skull.
    pub dangerous: bool,
    /// `xRot`。
    pub pitch: f32,
    pub left_owner: bool,
    pub has_been_shot: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

impl WitherSkull {
    pub fn new(dangerous: bool) -> Self {
        Self {
            dangerous,
            ..Self::default()
        }
    }

    pub(crate) fn public(&self) -> WitherSkullPublic {
        WitherSkullPublic {
            uuid: self.uuid,
            owner: self.owner,
            dangerous: self.dangerous,
            pitch: self.pitch,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
        }
    }
}

/// 同步层要读的那一半。
///
/// The half the sync layer reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WitherSkullPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub dangerous: bool,
    pub pitch: f32,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 一次射出凋灵之首的输入（`performRangedAttack`）。
///
/// Input for one wither-skull shot (`performRangedAttack`).
#[derive(Debug, Clone, PartialEq)]
pub struct WitherSkullShot {
    pub position: Vec3,
    /// 从头部指向目标的向量，构造时归一化。
    ///
    /// Vector from the head to the target, normalized on construction.
    pub direction: Vec3,
    pub dangerous: bool,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
}

impl Entity {
    pub fn wither_skull(shot: &WitherSkullShot) -> Self {
        let mut skull = WitherSkull::new(shot.dangerous);
        skull.owner = shot.owner;
        skull.owner_uuid = shot.owner_uuid;
        Self::new(EntityKind::WitherSkull(Box::new(skull)), shot.position)
    }

    /// `AbstractHurtingProjectile` 构造：`assignDirectionalMovement` 把归一化方向
    /// 乘 `accelerationPower=0.1` 作初速，再按速度设朝向，不抽任何散布随机数。
    ///
    /// `AbstractHurtingProjectile` construction: `assignDirectionalMovement`
    /// scales the normalized direction by `accelerationPower=0.1` into the
    /// initial velocity; set rotation from it, no spread draws.
    pub fn shoot_wither_skull(&mut self, shot: &WitherSkullShot) {
        let movement = shot.direction.normalize().scale(ACCELERATION_POWER);
        self.delta = movement;
        let EntityKind::WitherSkull(skull) = &mut self.kind else {
            return;
        };
        skull.needs_sync = true;
        skull.sync_velocity = true;
        let (yaw, pitch) = rotation(movement);
        self.y_rot = yaw;
        skull.pitch = pitch;
    }
}

impl World<'_> {
    /// 生成一枚射出的凋灵之首。
    ///
    /// Spawn one fired wither skull.
    pub fn spawn_wither_skull_shot(&mut self, shot: &WitherSkullShot) -> i32 {
        let mut entity = self.prepare_entity(Entity::wither_skull(shot));
        entity.shoot_wither_skull(shot);
        self.add_prepared_entity(entity)
    }
}

/// 一次命中：`block` 是方块命中（含方向），`target` 是被打中的实体。
///
/// One hit: `block` is a block hit (with face); `target` is an entity hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WitherSkullHit {
    pub location: Vec3,
    pub face: Option<Direction>,
    pub target: Option<(EntityUid, bool)>,
}

/// `AbstractHurtingProjectile.applyInertia()`：沿速度方向加 `accelerationPower`
/// 再按惯性缩放；水中用 `getLiquidInertia()`，否则 `getInertia()`（危险头 0.73）。
///
/// `AbstractHurtingProjectile.applyInertia()`: add `accelerationPower` along
/// the velocity then scale by inertia; water uses `getLiquidInertia()`, otherwise
/// `getInertia()` (0.73 for a dangerous skull).
pub fn apply_inertia(delta: Vec3, in_water: bool, dangerous: bool) -> Vec3 {
    let inertia = if in_water {
        LIQUID_INERTIA
    } else if dangerous {
        DANGEROUS_INERTIA
    } else {
        INERTIA
    };
    delta
        .add(delta.normalize().scale(ACCELERATION_POWER))
        .scale(f64::from(inertia))
}

/// `AbstractHurtingProjectile.tick`：`applyInertia` → 命中搜索 → `setPos` → `onHit`。
///
/// `AbstractHurtingProjectile.tick`: `applyInertia` → hit search → `setPos` → `onHit`.
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let dangerous = {
        let EntityKind::WitherSkull(skull) = &mut me.kind else {
            return;
        };
        skull.needs_sync = false;
        skull.sync_velocity = false;
        skull.has_been_shot = true;
        skull.dangerous
    };
    let in_water = me.in_water;
    if in_water {
        // `applyInertia()` 水中分支：4 个 BUBBLE 粒子（位置 = 当前位置 - 速度*0.25，
        // 速度 = 本刻速度），惯性改用 `getLiquidInertia()`。
        // The water branch of `applyInertia()`: four BUBBLE particles at
        // `position - velocity * 0.25` moving with this tick's velocity, and
        // `getLiquidInertia()` replaces `getInertia()`.
        ctx.emit(
            Command::WitherSkullBubbles {
                position: me.position,
                delta: me.delta,
            },
            me,
        );
    }
    me.delta = apply_inertia(me.delta, in_water, dangerous);
    let movement = me.delta;
    let hit = find_hit(me, ctx, movement);
    let position = hit
        .as_ref()
        .map_or_else(|| me.position.add(movement), |hit| hit.location);
    me.position = position;
    // `ProjectileUtil.rotateTowardsMovement(this, 0.2F)`。
    if movement.length_sqr() > 0.0 {
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let deg = |radians: f64| (radians * 57.295_776_367_187_5) as f32;
        let target_yaw = deg(mth::atan2(movement.z, movement.x)) + 90.0;
        let target_pitch = deg(mth::atan2(horizontal, movement.y)) - 90.0;
        let EntityKind::WitherSkull(skull) = &me.kind else {
            return;
        };
        let old_yaw = me.y_rot;
        let old_pitch = skull.pitch;
        me.y_rot = mth::lerp_f32(0.2, old_yaw, target_yaw);
        let EntityKind::WitherSkull(skull) = &mut me.kind else {
            return;
        };
        skull.pitch = mth::lerp_f32(0.2, old_pitch, target_pitch);
    }
    me.update_projectile_fluid(ctx.blocks());
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        me.removed = true;
    }
    if me.position.y > f64::from(ctx.blocks().max_y() + 30) {
        if let Some(hit) = hit {
            ctx.emit(Command::WitherSkullImpact(hit), me);
        }
        me.removed = true;
    } else if let Some(hit) = hit {
        ctx.emit(Command::WitherSkullImpact(hit), me);
        me.removed = true;
    }
}

/// `ProjectileUtil.getHitResultOnMoveVector`，命中实体盒按 `computeMargin` 膨胀。
/// 排除别的凋灵之首和风弹（都是 `noPhysics` 投射物）。
///
/// `ProjectileUtil.getHitResultOnMoveVector`, with the entity box inflated by
/// `computeMargin`. Other wither skulls and wind charges (both `noPhysics`
/// projectiles) are excluded.
fn find_hit(me: &Entity, ctx: &dyn EntityCtx, movement: Vec3) -> Option<WitherSkullHit> {
    let blocks = ctx.blocks();
    let from = me.position;
    let block = clip_including_border(blocks, from, from.add(movement), me.collision_context());
    let to = block.map_or(from.add(movement), |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(movement.x, movement.y, movement.z)
        .inflated(1.0);
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let mut best: Option<(f64, Vec3, EntityUid, bool)> = None;
    let mut consider = |uid: EntityUid, bb: Aabb, player: bool| {
        let Some(location) = clip_entity(bb.inflated(margin), from, to) else {
            return;
        };
        let distance = from.distance_to_sqr(location);
        if best.is_none_or(|(nearest, _, _, _)| distance < nearest) {
            best = Some((distance, location, uid, player));
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        if other.removed
            || matches!(
                other.kind,
                KindPublic::WitherSkull(_) | KindPublic::WindCharge(_)
            )
        {
            return;
        }
        if matches!(&other.kind, KindPublic::Mob(mob) if mob.health > 0.0) {
            consider(other.uid, other.bounding_box(), false);
        }
    });
    blocks.for_each_player(&mut |player: &PlayerView| {
        let bb = player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, bb, true);
        }
    });
    match (best, block) {
        (Some((_, location, uid, player)), _) => Some(WitherSkullHit {
            location,
            face: None,
            target: Some((uid, player)),
        }),
        (None, Some(hit)) => Some(WitherSkullHit {
            location: hit.location,
            face: Some(hit.face),
            target: None,
        }),
        (None, None) => None,
    }
}

/// [`Command::WitherSkullImpact`] 的落地：`WitherSkull.onHitEntity` 的 8 点伤害与
/// 凋零效果、`WitherSkull.onHit` 的 1 格 `MOB` 爆炸（蓝色头降低方块抗性）。
///
/// Landing of [`Command::WitherSkullImpact`]: the 8-point hit and wither effect of
/// `WitherSkull.onHitEntity` and the 1-block `MOB` blast of `WitherSkull.onHit`
/// (a dangerous skull caps block blast resistance).
pub fn impact(me: &Entity, world: &mut World<'_>, hit: WitherSkullHit) {
    let EntityKind::WitherSkull(skull) = &me.kind else {
        return;
    };
    let dangerous = skull.dangerous;
    let owner = skull.owner;
    if let Some((target, player)) = hit.target {
        // `onHitEntity`：owner 是 LivingEntity 时 8 点 `wither_skull`，无 owner 时
        // 5 点 `magic()`。
        // `onHitEntity`: 8 `wither_skull` damage when the owner is a
        // LivingEntity, 5 `magic()` damage otherwise.
        let (kind, amount) = if owner.is_some() {
            (DamageKind::WitherSkull, 8.0_f32)
        } else {
            (DamageKind::Magic, 5.0_f32)
        };
        let mut damage = DamageSource::environment(kind);
        damage.direct = Some(me.id);
        damage.attacker = owner;
        damage.position = Some(me.position);
        damage.projectile_motion = Some(me.delta);
        let hurt = if player {
            world.hurt_player_from_mob(target, damage, amount, None)
        } else {
            world.hurt_mob(target, damage, amount)
        };
        if hurt {
            // 目标还活着时官方跑 `EnchantmentHelper.doPostAttackEffects`（遍历目标
            // 装备的 `POST_ATTACK` 附魔）；凋灵之首命中的生物 / 无装备目标没有这类
            // 附魔，Bamboo 也无通用 POST_ATTACK 框架，按字节码的空结果处理。被打死
            // 则 `owner.heal(5.0)`。
            // While the target is alive vanilla runs
            // `EnchantmentHelper.doPostAttackEffects` (the target's `POST_ATTACK`
            // enchantments); wither-skull mob targets have none and Bamboo has no
            // generic POST_ATTACK framework, so it is a no-op. A killed target
            // makes `owner.heal(5.0)`.
            let target_alive = if player {
                true
            } else {
                let mut alive = false;
                world.with_entity_mut(target, &mut |entity| {
                    alive = entity.kind.mob().is_some_and(|mob| mob.health > 0.0);
                });
                alive
            };
            if !target_alive && let Some(owner_id) = owner {
                world.with_entity_mut(owner_id, &mut |entity| {
                    entity.with_mob(|_, mob| {
                        let max = mob
                            .attributes
                            .value(mc_bamboo_data::registries::attribute::MAX_HEALTH)
                            as f32;
                        mob.health = (mob.health + 5.0).min(max);
                    });
                });
            }
            // `onHitEntity` 结尾：难度 NORMAL 给 200 刻、HARD 给 800 刻的 2 级凋零。
            // `onHitEntity` tail: NORMAL gives 200 ticks, HARD 800 ticks of wither II.
            let ticks = match world.difficulty_id() {
                2 => 20 * 10,
                3 => 20 * 40,
                _ => 0,
            };
            if ticks > 0 && !player {
                world.with_entity_mut(target, &mut |entity| {
                    entity.with_mob(|_, mob| {
                        living::add_effect(
                            mob,
                            EffectInstance::with_amplifier(mob_effect::WITHER, ticks, 1),
                        );
                    });
                });
            }
        }
    }
    // `WitherSkull.onHit`：`level.explode(this, getX(), getY(), getZ(), 1.0F, false, MOB)`。
    // `WitherSkull.onHit`: `level.explode(this, getX(), getY(), getZ(), 1.0F, false, MOB)`.
    let source = Box::new(me.clone().split().public);
    world.explode_mob(
        &source,
        me.position,
        EXPLOSION_RADIUS,
        dangerous.then_some(DANGEROUS_RESISTANCE_CAP),
    );
}

/// `EntityType.WITHER_SKULL`。
pub const ENTITY_TYPE: i32 = types::WITHER_SKULL;

#[cfg(test)]
mod tests {
    use mc_bamboo_nbt::Tag;

    use super::*;

    fn sample_shot(dangerous: bool) -> WitherSkullShot {
        WitherSkullShot {
            position: Vec3::new(1.0, 65.0, 2.0),
            direction: Vec3::new(0.0, 0.0, 10.0),
            dangerous,
            owner: Some(7),
            owner_uuid: None,
        }
    }

    /// `AbstractHurtingProjectile.getInertia()` = 0.95；`WitherSkull` 危险头 = 0.73；
    /// 水中 `getLiquidInertia()` = 0.8；`accelerationPower` = 0.1；水里每刻 4 个气泡。
    ///
    /// `AbstractHurtingProjectile.getInertia()` = 0.95; dangerous `WitherSkull` =
    /// 0.73; `getLiquidInertia()` = 0.8 in water; `accelerationPower` = 0.1;
    /// four bubbles per water tick.
    #[test]
    fn inertia_values_match_bytecode() {
        assert_eq!(INERTIA, 0.95);
        assert_eq!(DANGEROUS_INERTIA, 0.73);
        assert_eq!(LIQUID_INERTIA, 0.8);
        assert_eq!(ACCELERATION_POWER, 0.1);
        assert_eq!(WATER_BUBBLES, 4);
        assert_eq!(EXPLOSION_RADIUS, 1.0);
        assert_eq!(DANGEROUS_RESISTANCE_CAP, 0.8);
    }

    /// `applyInertia`：先沿速度方向加 0.1，再按三种惯性缩放；水中不看 dangerous。
    ///
    /// `applyInertia`: add 0.1 along the velocity then scale by the inertia;
    /// the water branch ignores `dangerous`.
    #[test]
    fn apply_inertia_uses_acceleration_and_inertia() {
        let unit = Vec3::new(0.0, 0.0, 1.0);
        let accel = 1.0 + ACCELERATION_POWER;
        let normal = apply_inertia(unit, false, false);
        assert!((normal.z - accel * f64::from(INERTIA)).abs() < 1e-9);
        let dangerous = apply_inertia(unit, false, true);
        assert!((dangerous.z - accel * f64::from(DANGEROUS_INERTIA)).abs() < 1e-9);
        let water_normal = apply_inertia(unit, true, false);
        let water_dangerous = apply_inertia(unit, true, true);
        assert!((water_normal.z - accel * f64::from(LIQUID_INERTIA)).abs() < 1e-9);
        assert_eq!(water_normal, water_dangerous);
        // 非单位方向：normalize 只取方向，加速量仍是 0.1 而不是长度比例。
        let diagonal = apply_inertia(Vec3::new(0.0, 0.0, 10.0), false, false);
        assert!((diagonal.z - (10.0 + ACCELERATION_POWER) * f64::from(INERTIA)).abs() < 1e-9);
    }

    /// `shoot` 把方向归一化再乘 0.1 作初速，不抽散布随机数。
    ///
    /// `shoot` normalizes the direction and scales it by 0.1 into the initial
    /// velocity, with no spread draw.
    #[test]
    fn shooting_normalizes_the_direction() {
        let shot = sample_shot(true);
        let mut entity = Entity::wither_skull(&shot);
        entity.shoot_wither_skull(&shot);
        assert!(entity.delta.x.abs() < 1e-9);
        assert!(entity.delta.y.abs() < 1e-9);
        assert!((entity.delta.z - ACCELERATION_POWER).abs() < 1e-9);
        let EntityKind::WitherSkull(skull) = &entity.kind else {
            panic!("wither skull");
        };
        assert!(skull.dangerous);
        assert_eq!(skull.owner, Some(7));
        assert!(!skull.has_been_shot);
    }

    /// `WitherSkull.addAdditionalSaveData` 写 `dangerous` 键（蓝色头为 1）。
    ///
    /// `WitherSkull.addAdditionalSaveData` writes the `dangerous` key (1 for blue).
    #[test]
    fn save_writes_the_dangerous_flag() {
        let shot = sample_shot(true);
        let mut entity = Entity::wither_skull(&shot);
        entity.shoot_wither_skull(&shot);
        entity.id = 42;
        let tag = crate::archive::entity::write_entity(&entity.split()).expect("save");
        let Tag::Compound(entries) = &tag else {
            panic!("entity NBT is a compound");
        };
        assert!(
            entries
                .iter()
                .any(|(name, value)| name == "dangerous" && *value == Tag::Byte(1))
        );
    }
}
