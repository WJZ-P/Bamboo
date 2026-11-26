//! 风弹，官方 `AbstractWindCharge` / `WindCharge` / `BreezeWindCharge`。
//!
//! 飞行照 `AbstractHurtingProjectile.tick`：惯性 1、加速度 0、无重力，沿本刻速度
//! 找方块和实体命中；命中时走统一的爆炸路径（`explosion.rs` 的
//! `ExplosionInteraction.TRIGGER`），不破坏方块但会开门 / 按钮 / 拉杆，并按曝光度
//! 击退范围内的生物和玩家。玩家 / 旋风人 / 不祥物品生成器都用同一份实体结构。

use mc_bamboo_core::{Aabb, BlockPos, Direction, Vec3};
use mc_bamboo_data::{registries::entity_type as types, sounds};

use super::{
    Entity, EntityKind, EntityUid, KindPublic,
    arrow::{clip_entity, clip_including_border, player_box, rotation},
    ctx::{Command, EntityCtx, PlayerView},
};
use crate::World;

thread_local! {
    static NO_MARGIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 由 `mob-oracle` 按 `--break-self-test breeze_margin` 设置，其他驱动不调用。
/// 为真时 `find_hit` 不给实体盒加 `computeMargin` 膨胀（复刻修复前的旧行为）。
///
/// Set by `mob-oracle` from `--break-self-test breeze_margin`; no other driver calls
/// this. When true, `find_hit` does not inflate the entity box by `computeMargin` (the
/// pre-fix behaviour).
pub fn set_oracle_no_margin(value: bool) {
    NO_MARGIN.with(|cell| cell.set(value));
}

/// `WindCharge.RADIUS` / `BreezeWindCharge.RADIUS`。
pub const PLAYER_RADIUS: f32 = 1.2;
pub const BREEZE_RADIUS: f32 = 3.0;
/// `SimpleExplosionDamageCalculator(true, false, 1.22f, ...)` 的击退倍率。
pub const PLAYER_KNOCKBACK: f32 = 1.22;
/// `WindCharge.noDeflectTicks`。
pub const NO_DEFLECT_TICKS: i32 = 5;

/// 两种风弹。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindChargeVariant {
    #[default]
    Player,
    Breeze,
}

impl WindChargeVariant {
    pub const fn entity_type(self) -> i32 {
        match self {
            Self::Player => types::WIND_CHARGE,
            Self::Breeze => types::BREEZE_WIND_CHARGE,
        }
    }

    pub const fn radius(self) -> f32 {
        match self {
            Self::Player => PLAYER_RADIUS,
            Self::Breeze => BREEZE_RADIUS,
        }
    }

    /// `EXPLOSION_DAMAGE_CALCULATOR` 的 `getKnockbackMultiplier`。
    pub const fn knockback(self) -> f32 {
        match self {
            Self::Player => PLAYER_KNOCKBACK,
            Self::Breeze => 1.0,
        }
    }

    pub const fn sound(self) -> &'static str {
        match self {
            Self::Player => "minecraft:entity.wind_charge.wind_burst",
            Self::Breeze => "minecraft:entity.breeze.wind_burst",
        }
    }
}

/// `AbstractWindCharge` / `WindCharge` 自己的状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindCharge {
    pub uuid: [u8; 16],
    pub variant: WindChargeVariant,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `xRot`。
    pub pitch: f32,
    /// `Projectile.leftOwner` / `hasBeenShot`。
    pub left_owner: bool,
    pub has_been_shot: bool,
    /// `WindCharge.noDeflectTicks`。
    pub no_deflect_ticks: i32,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

impl WindCharge {
    pub fn new(variant: WindChargeVariant) -> Self {
        Self {
            variant,
            no_deflect_ticks: if variant == WindChargeVariant::Player {
                NO_DEFLECT_TICKS
            } else {
                0
            },
            ..Self::default()
        }
    }

    pub(crate) fn public(&self, fire: i32) -> WindChargePublic {
        WindChargePublic {
            uuid: self.uuid,
            variant: self.variant,
            owner: self.owner,
            pitch: self.pitch,
            on_fire: fire > 0,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
        }
    }
}

/// 同步层要读的那一半。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindChargePublic {
    pub uuid: [u8; 16],
    pub variant: WindChargeVariant,
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub on_fire: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 一次射出风弹的输入（`Projectile.shoot` / `spawnProjectileUsingShoot`）。
#[derive(Debug, Clone, PartialEq)]
pub struct WindChargeShot {
    pub position: Vec3,
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    pub variant: WindChargeVariant,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
}

impl Entity {
    pub fn wind_charge(shot: &WindChargeShot) -> Self {
        let mut charge = WindCharge::new(shot.variant);
        charge.owner = shot.owner;
        charge.owner_uuid = shot.owner_uuid;
        Self::new(EntityKind::WindCharge(Box::new(charge)), shot.position)
    }

    /// `Projectile.shoot`：归一化 → 按 XYZ 抽散布 → 乘力度 → 设朝向。
    pub fn shoot_wind_charge(&mut self, shot: &WindChargeShot) {
        let spread = 0.0172275 * f64::from(shot.uncertainty);
        let movement = shot
            .direction
            .normalize()
            .add(Vec3::new(
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
            ))
            .scale(f64::from(shot.power));
        let EntityKind::WindCharge(charge) = &mut self.kind else {
            return;
        };
        self.delta = movement;
        charge.needs_sync = true;
        charge.sync_velocity = true;
        let (yaw, pitch) = rotation(movement);
        self.y_rot = yaw;
        charge.pitch = pitch;
    }
}

impl World<'_> {
    /// 生成一枚射出的风弹：先分配 ID / 种子（构造函数里的 UUID），再按 `shoot` 取散布。
    pub fn spawn_wind_charge_shot(&mut self, shot: &WindChargeShot) -> i32 {
        let mut entity = self.prepare_entity(Entity::wind_charge(shot));
        entity.shoot_wind_charge(shot);
        self.add_prepared_entity(entity)
    }
}

/// `AbstractHurtingProjectile.tick`：`applyInertia` → 命中搜索 → `setPos` → `onHit`。
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::WindCharge(charge) = &mut me.kind else {
        return;
    };
    charge.needs_sync = false;
    charge.sync_velocity = false;
    charge.has_been_shot = true;
    if charge.no_deflect_ticks > 0 {
        charge.no_deflect_ticks -= 1;
    }
    // `getInertia() == 1`、`accelerationPower == 0`：水中也不额外减速。
    let movement = me.delta;
    let hit = find_hit(me, ctx, movement);
    let position = hit
        .as_ref()
        .map_or_else(|| me.position.add(movement), |hit| hit.location);
    me.position = position;
    // `ProjectileUtil.rotateTowardsMovement(this, 0.2F)`：先按速度算绝对朝向，
    // 再用 `Entity.xRotO / yRotO`（上一刻的朝向）做 0.2 插值。
    if movement.length_sqr() > 0.0 {
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let deg = |radians: f64| (radians * 57.295_776_367_187_5) as f32;
        let target_yaw = deg(mc_bamboo_core::mth::atan2(movement.z, movement.x)) + 90.0;
        let target_pitch = deg(mc_bamboo_core::mth::atan2(horizontal, movement.y)) - 90.0;
        let EntityKind::WindCharge(charge) = &me.kind else {
            return;
        };
        let old_yaw = me.y_rot;
        let old_pitch = charge.pitch;
        me.y_rot = mc_bamboo_core::mth::lerp_f32(0.2, old_yaw, target_yaw);
        let EntityKind::WindCharge(charge) = &mut me.kind else {
            return;
        };
        charge.pitch = mc_bamboo_core::mth::lerp_f32(0.2, old_pitch, target_pitch);
    }
    me.update_projectile_fluid(ctx.blocks());
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        me.removed = true;
    }
    if me.position.y > f64::from(ctx.blocks().max_y() + 30) {
        if let Some(hit) = hit {
            ctx.emit(Command::WindChargeImpact(hit), me);
        }
        me.removed = true;
    } else if let Some(hit) = hit {
        ctx.emit(Command::WindChargeImpact(hit), me);
        me.removed = true;
    }
}

/// 一次命中。`block` 是方块命中（含方向）；`target` 是被打中的实体。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindChargeHit {
    pub location: Vec3,
    pub face: Option<Direction>,
    pub target: Option<(EntityUid, bool)>,
}

/// `ProjectileUtil.getHitResultOnMoveVector`，`canHitEntity` 排除别的风弹和末影水晶。
///
/// 命中的实体包围盒按 `computeMargin` 膨胀：`max(0, min(0.3, (tickCount - 2) / 20))`，
/// 全 float 运算。飞了几刻以后 margin 到 0.3，命中点（也就是爆炸中心）才和官方一致。
///
/// The hit entity box is inflated by `computeMargin` (`max(0, min(0.3, (tickCount - 2) /
/// 20))`, all float). The margin reaches 0.3 after a few ticks, which keeps the hit point
/// (and therefore the explosion centre) identical to vanilla.
fn find_hit(me: &Entity, ctx: &dyn EntityCtx, movement: Vec3) -> Option<WindChargeHit> {
    let blocks = ctx.blocks();
    let from = me.position;
    let block = clip_including_border(blocks, from, from.add(movement), me.collision_context());
    let to = block.map_or(from.add(movement), |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(movement.x, movement.y, movement.z)
        .inflated(1.0);
    let margin = if NO_MARGIN.with(|cell| cell.get()) {
        0.0
    } else {
        f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3))
    };
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
        if other.removed || matches!(other.kind, KindPublic::WindCharge(_)) {
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
        (Some((_, location, uid, player)), _) => Some(WindChargeHit {
            location,
            face: None,
            target: Some((uid, player)),
        }),
        (None, Some(hit)) => Some(WindChargeHit {
            location: hit.location,
            face: Some(hit.face),
            target: None,
        }),
        (None, None) => None,
    }
}

/// [`Command::WindChargeImpact`] 的落地：1 点 `wind_charge` 伤害 + TRIGGER 爆炸。
pub fn impact(me: &Entity, world: &mut World<'_>, hit: WindChargeHit) {
    let EntityKind::WindCharge(charge) = &me.kind else {
        return;
    };
    let variant = charge.variant;
    let owner = charge.owner;
    if let Some((target, player)) = hit.target {
        let mut damage = super::mob::DamageSource::environment(super::mob::DamageKind::WindCharge);
        damage.direct = Some(me.id);
        damage.attacker = owner;
        damage.position = Some(me.position);
        damage.projectile_motion = Some(me.delta);
        if player {
            world.hurt_player_from_mob(target, damage, 1.0, None);
        } else {
            world.hurt_mob(target, damage, 1.0);
        }
    }
    // `onHitEntity` 用 `position()` 当爆炸中心，`onHitBlock` 按命中面的法线偏移 0.25。
    let center = match (hit.target, hit.face) {
        (None, Some(face)) => {
            let (dx, dy, dz) = face.normal();
            hit.location.add(Vec3::new(
                f64::from(dx) * 0.25,
                f64::from(dy) * 0.25,
                f64::from(dz) * 0.25,
            ))
        }
        _ => me.position,
    };
    // `ServerExplosion.canTriggerBlocks`：玩家风弹恒真，旋风人风弹看 `mob_griefing`。
    let can_trigger_blocks = match variant {
        WindChargeVariant::Player => true,
        WindChargeVariant::Breeze => world.block_game_rules().mob_griefing,
    };
    world.explode_wind_charge(
        center,
        variant.radius(),
        variant.knockback(),
        me.id,
        can_trigger_blocks,
    );
    world.play_sound(
        center,
        sounds::sound_event(variant.sound()),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    let _ = BlockPos::containing(center);
}
