//! 羊驼的口水，官方 `LlamaSpit`（`Projectile` 的子类）。
//!
//! 一刻的顺序照 `LlamaSpit.tick`：先 `Projectile.tick`（`hasBeenShot`、`checkLeftOwner`），
//! 再 `Entity.baseTick`（流体、火、掉出世界），然后沿这一刻的速度找命中，按命中前的速度
//! 算新位置；碰到任何非空气方块或者在水里就消失，否则乘 0.99、减 0.06 重力再移过去。
//! 口水不调 `move`，没有方块碰撞；打到实体也不消失。
//!
//! The llama's spit, vanilla `LlamaSpit` (a `Projectile`). The tick follows
//! `LlamaSpit.tick`: `Projectile.tick` (`hasBeenShot`, `checkLeftOwner`), then
//! `Entity.baseTick` (fluids, fire, falling out of the world), then a hit search
//! along this tick's velocity. The new position uses the pre-hit velocity; the
//! spit vanishes when its box touches any non-air block or it is in water,
//! otherwise it slows by 0.99, falls by 0.06 and moves. It never calls `move`,
//! so there is no block collision, and hitting an entity does not remove it.

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};

use super::arrow::{
    clip_entity, clip_including_border, lerp_rotation, player_box, resolve_owner, rotation,
};
use super::ctx::{Command, EntityCtx};
use super::mob::{DamageKind, DamageSource};
use super::{Entity, EntityKind, EntityUid, KindPublic};
use crate::World;

/// `LlamaSpit.getDefaultGravity`。
const GRAVITY: f64 = 0.06;

/// `LlamaSpit.getAirDrag`，官方是 `float`。
///
/// `LlamaSpit.getAirDrag`, a `float` in vanilla.
const AIR_DRAG: f32 = 0.99;

/// 口水只有自己读写的字段。
///
/// The spit's own fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LlamaSpit {
    pub uuid: [u8; 16],
    /// 主人（吐口水的羊驼）的实体 ID；读盘后为空，按 `owner_uuid` 再找。
    ///
    /// The owner's entity id; empty after loading until found by `owner_uuid`.
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `xRot`。
    pub pitch: f32,
    /// `Projectile.leftOwner`。
    pub left_owner: bool,
    /// `Projectile.hasBeenShot`。
    pub has_been_shot: bool,
    /// `Entity.needsSync`：`Projectile.shoot` 置真，到下一刻开头清掉。
    ///
    /// `Entity.needsSync`: set by `Projectile.shoot`, cleared at the start of the next tick.
    pub(crate) needs_sync: bool,
}

/// 口水给别人读的部分：同步要的 UUID、主人、俯仰角和 `needsSync`。
///
/// The public half: UUID, owner, pitch and `needsSync` for the sync layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LlamaSpitPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub needs_sync: bool,
}

impl LlamaSpit {
    pub(crate) const fn public(&self) -> LlamaSpitPublic {
        LlamaSpitPublic {
            uuid: self.uuid,
            owner: self.owner,
            pitch: self.pitch,
            needs_sync: self.needs_sync,
        }
    }
}

/// 一次吐口水的输入：羊驼的位置、身体朝向、宽度、眼高、UUID，和目标的位置。
///
/// One spit's inputs: the llama's position, body yaw, width, eye height and
/// UUID, and the target's aim point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpitShot {
    pub owner_position: Vec3,
    pub owner_body_yaw: f32,
    pub owner_width: f32,
    pub owner_eye_height: f32,
    pub owner_uuid: [u8; 16],
    /// 目标的 `getX()`、`getY(0.3333333333333333)`、`getZ()`。
    ///
    /// The target's `getX()`, `getY(0.3333333333333333)` and `getZ()`.
    pub target: Vec3,
}

impl Entity {
    /// `new LlamaSpit(level, owner)`：位置在羊驼身体前方半个（宽度 + 1），眼高往下 0.1。
    ///
    /// `new LlamaSpit(level, owner)`: half of (width + 1) ahead of the body, 0.1
    /// below the eyes.
    pub fn llama_spit(shot: &SpitShot) -> Self {
        let radians = f64::from(shot.owner_body_yaw * (std::f64::consts::PI / 180.0) as f32);
        let reach = f64::from(shot.owner_width + 1.0) * 0.5;
        let position = Vec3::new(
            shot.owner_position.x - reach * f64::from(mth::sin(radians)),
            shot.owner_position.y + f64::from(shot.owner_eye_height) - f64::from(0.1_f32),
            shot.owner_position.z + reach * f64::from(mth::cos(radians)),
        );
        Self::new(EntityKind::LlamaSpit(Box::default()), position)
    }

    /// `Projectile.shoot`：先归一化，再按 XYZ 顺序抽散布，乘速度，设朝向，置 `needsSync`。
    ///
    /// `Projectile.shoot`: normalize, draw the spread in XYZ order, scale, set
    /// the rotation and raise `needsSync`.
    pub fn shoot_spit(&mut self, direction: Vec3, power: f32, uncertainty: f32) {
        let EntityKind::LlamaSpit(spit) = &mut self.kind else {
            return;
        };
        let spread = 0.0172275 * f64::from(uncertainty);
        self.delta = direction
            .normalize()
            .add(Vec3::new(
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
            ))
            .scale(f64::from(power));
        let (yaw, pitch) = rotation(self.delta);
        self.y_rot = yaw;
        spit.pitch = pitch;
        spit.needs_sync = true;
    }
}

/// `Llama.spit` 里生成口水的那一半：构造（实体自己的随机源取 UUID），瞄准，
/// `spawnProjectileUsingShoot(…, 1.5F, 10.0F)`。羊驼自己的音效和 `didSpit` 在调用方。
///
/// The spawning half of `Llama.spit`: construct (the spit's own random draws
/// the UUID), aim, then `spawnProjectileUsingShoot(..., 1.5F, 10.0F)`. The
/// llama's sound and `didSpit` stay with the caller.
pub(crate) fn spawn(world: &mut World<'_>, owner: EntityUid, shot: SpitShot) {
    let mut entity = world.prepare_entity(Entity::llama_spit(&shot));
    if let EntityKind::LlamaSpit(spit) = &mut entity.kind {
        spit.owner = Some(owner);
        spit.owner_uuid = Some(shot.owner_uuid);
    }
    let dx = shot.target.x - shot.owner_position.x;
    let dy = shot.target.y - entity.position.y;
    let dz = shot.target.z - shot.owner_position.z;
    let lift = (dx * dx + dz * dz).sqrt() * f64::from(0.2_f32);
    entity.shoot_spit(Vec3::new(dx, dy + lift, dz), 1.5, 10.0);
    world.add_prepared_entity(entity);
}

/// 口水一次命中的实体。
///
/// The entity one spit hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpitHit {
    pub target: EntityUid,
    pub player: bool,
}

/// `LlamaSpit.tick`。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    projectile_tick(me, ctx);
    base_tick(me, ctx);
    let movement = me.delta;
    match find_hit(me, ctx, movement) {
        Some(Hit::Entity(hit)) => ctx.emit(Command::SpitHit(hit), me),
        // `onHitBlock`：服务端 `discard()`。`BlockState.onProjectileHit` 没有建模（箭也没有）。
        // `onHitBlock` discards on the server. `BlockState.onProjectileHit` is not modelled.
        Some(Hit::Block) => me.removed = true,
        None => {}
    }
    let next = me.position.add(movement);
    update_rotation(me);
    let blocks = ctx.blocks();
    if touches_non_air(blocks, me.bounding_box()) || me.in_water {
        me.removed = true;
    } else {
        me.delta = movement.scale(f64::from(AIR_DRAG));
        me.delta = me.delta.add(Vec3::new(0.0, -GRAVITY, 0.0));
        me.position = next;
    }
}

/// `Projectile.tick` 在 `super.tick()` 之前的部分：`hasBeenShot` 和 `checkLeftOwner`。
/// `leftOwnerChecked` 每刻只查一次、末尾清掉，口水一刻只调一次，所以不用存。
///
/// The part of `Projectile.tick` before `super.tick()`: `hasBeenShot` and
/// `checkLeftOwner`. The spit checks once per tick, so `leftOwnerChecked`
/// needs no field.
fn projectile_tick(me: &mut Entity, ctx: &dyn EntityCtx) {
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let EntityKind::LlamaSpit(spit) = &mut me.kind else {
        return;
    };
    spit.needs_sync = false;
    spit.has_been_shot = true;
    let owner = owner(spit, ctx);
    spit.owner = owner;
    if !spit.left_owner {
        spit.left_owner = !owner_in_range(ctx, owner, area);
    }
}

/// `Projectile.getOwner`：先用记住的实体 ID，没有就按 UUID 找。
///
/// `Projectile.getOwner`: the remembered entity id, or a lookup by UUID.
fn owner(spit: &LlamaSpit, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    match spit.owner {
        Some(uid) if ctx.other(uid).is_some() => Some(uid),
        _ => spit.owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)),
    }
}

/// `isOutsideOwnerCollisionRange` 取反：主人（`CAN_BE_PICKED`：没被移除）的碰撞箱碰到扩大的范围。
///
/// The negation of `isOutsideOwnerCollisionRange`: the owner (pickable, i.e.
/// not removed) intersects the inflated sweep.
fn owner_in_range(ctx: &dyn EntityCtx, owner: Option<EntityUid>, area: Aabb) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    if let Some(entity) = ctx.other(owner) {
        return !entity.removed && entity.bounding_box().intersects(area);
    }
    let mut hit = false;
    ctx.blocks().for_each_player(&mut |player| {
        hit |= player.uid == owner && !player.spectator && player_box(player).intersects(area);
    });
    hit
}

/// `Entity.baseTick` 里对口水有用的部分：流体状态、着火倒计时、掉出世界。
/// 着火时每 20 刻的 `hurtServer(onFire)` 对口水没有效果，不建模。
///
/// The parts of `Entity.baseTick` that matter for spit: fluid state, the fire
/// countdown and falling out of the world. The 20-tick `hurtServer(onFire)`
/// has no effect on spit and is not modelled.
fn base_tick(me: &mut Entity, ctx: &dyn EntityCtx) {
    let blocks = ctx.blocks();
    me.update_projectile_fluid(blocks);
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
}

enum Hit {
    Entity(SpitHit),
    Block,
}

/// `ProjectileUtil.getHitResultOnMoveVector(this, this::canHitEntity)`：方块按 `COLLIDER`
/// 裁剪（含世界边界），再在裁剪后的线段上找放大 `computeMargin` 的碰撞箱里最近的实体。
///
/// Clip blocks with `COLLIDER` (and the world border), then find the nearest
/// entity whose box, inflated by `computeMargin`, the clipped segment enters.
fn find_hit(me: &Entity, ctx: &dyn EntityCtx, movement: Vec3) -> Option<Hit> {
    let blocks = ctx.blocks();
    let from = me.position;
    let block = clip_including_border(blocks, from, from.add(movement), me.collision_context());
    let to = block.map_or(from.add(movement), |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(movement.x, movement.y, movement.z)
        .inflated(1.0);
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let EntityKind::LlamaSpit(spit) = &me.kind else {
        return None;
    };
    let mut best: Option<(f64, SpitHit)> = None;
    let mut consider = |uid: EntityUid, bb: Aabb, player: bool| {
        if !spit.left_owner && spit.owner == Some(uid) {
            return;
        }
        let Some(location) = clip_entity(bb.inflated(margin), from, to) else {
            return;
        };
        let distance = from.distance_to_sqr(location);
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((
                distance,
                SpitHit {
                    target: uid,
                    player,
                },
            ));
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        if !other.removed && matches!(&other.kind, KindPublic::Mob(mob) if mob.is_alive()) {
            consider(other.uid, other.bounding_box(), false);
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, bb, true);
        }
    });
    match (best, block) {
        (Some((_, hit)), _) => Some(Hit::Entity(hit)),
        (None, Some(_)) => Some(Hit::Block),
        (None, None) => None,
    }
}

/// `Projectile.updateRotation`：朝这一刻速度的方向转 20%。
///
/// `Projectile.updateRotation`: turn 20% towards this tick's velocity.
fn update_rotation(me: &mut Entity) {
    let (yaw, pitch) = rotation(me.delta);
    me.y_rot = lerp_rotation(me.y_rot, yaw);
    if let EntityKind::LlamaSpit(spit) = &mut me.kind {
        spit.pitch = lerp_rotation(spit.pitch, pitch);
    }
}

/// `findBlocksIn(getBoundingBox()).filterState(isAir).noneMatched()` 取反：
/// 碰撞箱两角所在方块围成的范围里有任何非空气方块（不看形状）。
///
/// Whether any block in the cells spanned by the box corners is not air,
/// regardless of its shape.
fn touches_non_air(blocks: &dyn super::ctx::BlockView, bb: Aabb) -> bool {
    let low = BlockPos::containing(Vec3::new(bb.min_x, bb.min_y, bb.min_z));
    let high = BlockPos::containing(Vec3::new(bb.max_x, bb.max_y, bb.max_z));
    (low.x..=high.x).any(|x| {
        (low.y..=high.y)
            .any(|y| (low.z..=high.z).any(|z| !blocks.block_state(BlockPos::new(x, y, z)).is_air()))
    })
}

/// [`Command::SpitHit`] 的落地，`LlamaSpit.onHitEntity`：主人是活体时以
/// `damageSources().spit(this, owner)` 打 1 点。主人找不到时什么也不做。
///
/// Landing of [`Command::SpitHit`], `LlamaSpit.onHitEntity`: with a living
/// owner, deal 1 damage from `damageSources().spit(this, owner)`; nothing otherwise.
pub(crate) fn hit_entity(me: &Entity, world: &mut World<'_>, hit: SpitHit) {
    let EntityKind::LlamaSpit(spit) = &me.kind else {
        return;
    };
    let Some(owner) = spit.owner.and_then(|uid| world.level().entity_public(uid)) else {
        return;
    };
    let KindPublic::Mob(owner_mob) = &owner.kind else {
        return;
    };
    let source = DamageSource {
        attacker: Some(owner.uid),
        direct: Some(me.id),
        position: Some(me.position),
        projectile_motion: Some(me.delta),
        attacker_uuid: Some(owner_mob.uuid),
        scale_when_non_player: true,
        ..DamageSource::environment(DamageKind::Spit)
    };
    if hit.player {
        world.hurt_player_from_mob(hit.target, source, 1.0, Some(&owner));
    } else {
        world.hurt_mob(hit.target, source, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockState, entity::mob::MobType};

    fn still_mob(kind: MobType, x: f64) -> Entity {
        let mut entity = Entity::by_mob_type(kind, Vec3::new(x, 64.0, 0.5));
        entity.with_mob(|_, mob| mob.no_ai = true);
        entity
    }

    fn spit_from(owner: Option<EntityUid>, position: Vec3, delta: Vec3) -> Entity {
        let mut spit = Entity::new(EntityKind::LlamaSpit(Box::default()), position);
        if let EntityKind::LlamaSpit(data) = &mut spit.kind {
            data.owner = owner;
        }
        spit.delta = delta;
        spit
    }

    #[test]
    fn spit_starts_ahead_of_the_body_below_the_eyes() {
        let shot = SpitShot {
            owner_position: Vec3::new(10.0, 64.0, 10.0),
            owner_body_yaw: 0.0,
            owner_width: 0.9,
            owner_eye_height: 1.7765,
            owner_uuid: [0; 16],
            target: Vec3::default(),
        };
        let spit = Entity::llama_spit(&shot);
        // 朝南（yaw 0）时向 +Z 偏 (0.9 + 1) / 2。
        // Facing south (yaw 0) the offset is (0.9 + 1) / 2 along +Z.
        assert_eq!(spit.position.x, 10.0);
        assert_eq!(spit.position.z, 10.0 + f64::from(1.9_f32) * 0.5);
        assert_eq!(
            spit.position.y,
            64.0 + f64::from(1.7765_f32) - f64::from(0.1_f32)
        );
    }

    #[test]
    fn spit_hurts_a_cow_once_and_keeps_flying() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let owner = world.spawn_entity(still_mob(MobType::Llama, -20.0));
        let cow = world.spawn_entity(still_mob(MobType::Cow, 3.0));
        let spit = world.spawn_entity(spit_from(
            Some(owner),
            Vec3::new(0.5, 64.5, 0.5),
            Vec3::new(1.5, 0.0, 0.0),
        ));
        for _ in 0..3 {
            world.tick_entities();
        }
        let health = world.entity(cow).unwrap().kind.mob().unwrap().health;
        assert_eq!(health, 9.0);
        // 打到实体不消失，官方 `onHitEntity` 不 `discard`。
        // Hitting an entity does not discard the spit.
        assert!(world.entity(spit).is_some());
    }

    #[test]
    fn spit_without_a_living_owner_hits_for_nothing() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let cow = world.spawn_entity(still_mob(MobType::Cow, 3.0));
        world.spawn_entity(spit_from(
            None,
            Vec3::new(0.5, 64.5, 0.5),
            Vec3::new(1.5, 0.0, 0.0),
        ));
        for _ in 0..3 {
            world.tick_entities();
        }
        assert_eq!(world.entity(cow).unwrap().kind.mob().unwrap().health, 10.0);
    }

    #[test]
    fn spit_moves_with_the_old_velocity_then_vanishes_on_a_block() {
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(2, 64, 0),
            BlockState(mc_bamboo_data::blocks::STONE),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(spit_from(
            None,
            Vec3::new(0.5, 64.1, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
        ));
        world.tick_entities();
        let spit = world.entity(id).unwrap();
        assert_eq!(spit.position, Vec3::new(1.5, 64.1, 0.5));
        assert_eq!(spit.delta, Vec3::new(f64::from(AIR_DRAG), -GRAVITY, 0.0));
        world.tick_entities();
        assert!(world.entity(id).is_none());
    }

    #[test]
    fn spit_skips_its_owner_until_it_has_left_it() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let owner = world.spawn_entity(still_mob(MobType::Llama, 1.5));
        world.spawn_entity(spit_from(
            Some(owner),
            Vec3::new(0.5, 64.5, 0.5),
            Vec3::new(1.5, 0.0, 0.0),
        ));
        world.tick_entities();
        let llama = world.entity(owner).unwrap();
        assert_eq!(
            llama.kind.mob().unwrap().health,
            llama
                .kind
                .mob()
                .unwrap()
                .attributes
                .value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32
        );
    }
}
