//! 凋灵之首，官方 `WitherSkull`（`AbstractHurtingProjectile` 的一支）。
//!
//! 飞行照 `AbstractHurtingProjectile.tick`：每刻先按惯性缩放速度（普通 0.95、
//! 蓝色危险 0.73，加速度 0、无重力），沿本刻速度找方块和实体命中，命中就爆炸
//! 并消失。命中实体先造成 8 点 `wither_skull` 伤害，再按难度挂凋零效果；蓝色的
//! 爆炸把非 `#wither_immune` 方块的抗性压到 0.8，能真的炸掉方块。
//!
//! The wither skull, vanilla `WitherSkull` (an `AbstractHurtingProjectile`).
//! Flight follows `AbstractHurtingProjectile.tick`: scale velocity by inertia
//! each tick (0.95 normal, 0.73 dangerous), no acceleration or gravity, search
//! for a block/entity hit along this tick's movement and explode on contact. An
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

    /// `AbstractHurtingProjectile` 构造：方向归一化就是初速度（`accelerationPower=0`），
    /// 再按速度设朝向，不抽任何散布随机数。
    ///
    /// `AbstractHurtingProjectile` construction: the normalized direction is the
    /// initial velocity (`accelerationPower=0`); set rotation from it, no spread draws.
    pub fn shoot_wither_skull(&mut self, shot: &WitherSkullShot) {
        let movement = shot.direction.normalize();
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

/// `AbstractHurtingProjectile.tick`：`applyInertia` → 命中搜索 → `setPos` → `onHit`。
///
/// `AbstractHurtingProjectile.tick`: `applyInertia` → hit search → `setPos` → `onHit`.
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::WitherSkull(skull) = &mut me.kind else {
        return;
    };
    skull.needs_sync = false;
    skull.sync_velocity = false;
    skull.has_been_shot = true;
    let inertia = if skull.dangerous {
        DANGEROUS_INERTIA
    } else {
        INERTIA
    };
    // `applyInertia()`：非水分支就是 `delta *= getInertia()`（加速度为 0）。
    // `applyInertia()`: the non-water branch is just `delta *= getInertia()`.
    me.delta = me.delta.scale(f64::from(inertia));
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
        let mut damage = DamageSource::environment(DamageKind::WitherSkull);
        damage.direct = Some(me.id);
        damage.attacker = owner;
        damage.position = Some(me.position);
        damage.projectile_motion = Some(me.delta);
        let hurt = if player {
            world.hurt_player_from_mob(target, damage, 8.0, None)
        } else {
            world.hurt_mob(target, damage, 8.0)
        };
        if hurt {
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

    /// `AbstractHurtingProjectile.getInertia()` = 0.95；`WitherSkull` 危险头 = 0.73。
    ///
    /// `AbstractHurtingProjectile.getInertia()` = 0.95; dangerous `WitherSkull` = 0.73.
    #[test]
    fn inertia_values_match_bytecode() {
        assert_eq!(INERTIA, 0.95);
        assert_eq!(DANGEROUS_INERTIA, 0.73);
        assert_eq!(EXPLOSION_RADIUS, 1.0);
        assert_eq!(DANGEROUS_RESISTANCE_CAP, 0.8);
    }

    /// `shoot` 把方向归一化成初速度，不抽散布随机数。
    ///
    /// `shoot` normalizes the direction into the initial velocity, with no spread draw.
    #[test]
    fn shooting_normalizes_the_direction() {
        let shot = sample_shot(true);
        let mut entity = Entity::wither_skull(&shot);
        entity.shoot_wither_skull(&shot);
        assert!(entity.delta.x.abs() < 1e-6);
        assert!(entity.delta.y.abs() < 1e-6);
        assert!((entity.delta.z - 1.0).abs() < 1e-6);
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
