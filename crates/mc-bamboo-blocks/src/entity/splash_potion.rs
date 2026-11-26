//! 扔出的喷溅药水，官方 `AbstractThrownPotion` → `ThrownSplashPotion`。飞行和鸡蛋同一条
//! `ThrowableProjectile` 物理，只是重力是 0.05；命中时把药水的效果按距离衰减分给周围的
//! 生物和玩家，广播 2002 / 2007 号世界事件后消失。
//!
//! A thrown splash potion, vanilla `AbstractThrownPotion` → `ThrownSplashPotion`. The
//! flight is the same `ThrowableProjectile` physics as an egg with gravity 0.05. On impact
//! the potion's effects are handed to nearby mobs and players with distance falloff, then
//! level event 2002 or 2007 plays and the potion disappears.

use mc_bamboo_core::{Aabb, Vec3};
use mc_bamboo_data::registries::mob_effect;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Entity, EntityKind, EntityUid, KindPublic,
    ctx::{BlockView, Command, EntityCtx},
    mob::{DamageKind, DamageSource, effects::EffectInstance, living},
};
use crate::{Effect, World};

/// `AbstractThrownPotion.SPLASH_RANGE`。
///
/// `AbstractThrownPotion.SPLASH_RANGE`.
pub const SPLASH_RANGE: f64 = 4.0;

/// `AbstractThrownPotion.getDefaultGravity()`。
///
/// `AbstractThrownPotion.getDefaultGravity()`.
const GRAVITY: f64 = 0.05;

/// 喷溅药水自己的字段，和 `ThrownEgg` 一样，只多了药水物品本身。
///
/// The splash potion fields; like `ThrownEgg` plus the potion item.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashPotion {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub item: ItemStack,
    pub x_rot: f32,
    pub no_gravity: bool,
    pub left_owner: bool,
    pub has_been_shot: bool,
    pub(crate) needs_sync: bool,
    sync_velocity: bool,
}

impl SplashPotion {
    /// `ThrowableItemProjectile.setItem(stack)`。
    pub fn new(stack: &ItemStack) -> Self {
        Self {
            uuid: [0; 16],
            owner: None,
            owner_uuid: None,
            item: crate::container::copy_with_count(stack, 1),
            x_rot: 0.0,
            no_gravity: false,
            left_owner: false,
            has_been_shot: false,
            needs_sync: false,
            sync_velocity: false,
        }
    }

    pub(crate) fn public(&self, remaining_fire_ticks: i32) -> SplashPotionPublic {
        SplashPotionPublic {
            uuid: self.uuid,
            owner: self.owner,
            x_rot: self.x_rot,
            item: self.item.clone(),
            on_fire: remaining_fire_ticks > 0,
            no_gravity: self.no_gravity,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
        }
    }
}

/// 同步层要读的那一半。
///
/// The half the sync layer reads.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashPotionPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub x_rot: f32,
    pub item: ItemStack,
    pub on_fire: bool,
    pub no_gravity: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 一次投掷的输入：`Projectile.spawnProjectileUsingShoot` 的参数。
///
/// One throw's inputs: the arguments of `Projectile.spawnProjectileUsingShoot`.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashShot {
    pub position: Vec3,
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    pub source_movement: Option<Vec3>,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub item: ItemStack,
}

impl Entity {
    /// `new ThrownSplashPotion(level, owner, stack)` 加上主人。
    ///
    /// `new ThrownSplashPotion(level, owner, stack)` plus the owner.
    pub fn thrown_splash_potion(shot: &SplashShot) -> Self {
        let mut potion = SplashPotion::new(&shot.item);
        potion.owner = shot.owner;
        potion.owner_uuid = shot.owner_uuid;
        Self::new(EntityKind::SplashPotion(Box::new(potion)), shot.position)
    }

    /// `Projectile.shoot`。
    ///
    /// `Projectile.shoot`.
    pub fn shoot_splash_potion(&mut self, shot: &SplashShot) {
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
        let EntityKind::SplashPotion(potion) = &mut self.kind else {
            return;
        };
        self.delta = movement;
        potion.needs_sync = true;
        let (yaw, pitch) = super::thrown_egg::rotation(movement);
        self.y_rot = yaw;
        potion.x_rot = pitch;
        if let Some(source) = shot.source_movement {
            self.delta = self.delta.add(source);
        }
    }
}

impl World<'_> {
    /// 生成一瓶扔出的药水：先分配 ID 和种子，再按 `shoot` 取散布。
    ///
    /// Spawns one thrown potion: the id and seed first, then the spread of `shoot`.
    pub fn spawn_splash_shot(&mut self, shot: &SplashShot) -> i32 {
        let mut entity = self.prepare_entity(Entity::thrown_splash_potion(shot));
        entity.shoot_splash_potion(shot);
        self.add_prepared_entity(entity)
    }
}

/// `ThrowableProjectile.tick`，在 `commonTick` 之后。
///
/// `ThrowableProjectile.tick`, after `commonTick`.
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let blocks = ctx.blocks();
    let EntityKind::SplashPotion(potion) = &mut me.kind else {
        return;
    };
    potion.needs_sync = false;
    potion.sync_velocity = false;
    if !potion.no_gravity {
        me.delta = me.delta.add(Vec3::new(0.0, -GRAVITY, 0.0));
    }
    let inertia = if me.in_water { 0.8_f32 } else { 0.99_f32 };
    me.delta = me.delta.scale(f64::from(inertia));
    let owner = potion.owner.or_else(|| {
        potion
            .owner_uuid
            .and_then(|uuid| super::arrow::resolve_owner(ctx, uuid))
    });
    let left_owner = potion.left_owner;
    if let EntityKind::SplashPotion(potion) = &mut me.kind {
        potion.owner = owner;
    }
    let hit = find_hit(me, ctx, owner, left_owner);
    me.position = hit.map_or_else(|| me.position.add(me.delta), |hit| hit.location);
    update_rotation(me);
    me.apply_effects_along(blocks, me.old_position);
    check_left_owner(me, ctx, owner);
    base_tick(me, blocks);
    if let Some(hit) = hit
        && !me.removed
    {
        ctx.emit(Command::SplashPotionImpact { hit }, me);
    }
}

/// `Projectile.updateRotation`。
///
/// `Projectile.updateRotation`.
fn update_rotation(me: &mut Entity) {
    let (yaw, pitch) = super::thrown_egg::rotation(me.delta);
    let EntityKind::SplashPotion(potion) = &mut me.kind else {
        return;
    };
    potion.x_rot = super::thrown_egg::lerp_rotation(potion.x_rot, pitch);
    me.y_rot = super::thrown_egg::lerp_rotation(me.y_rot, yaw);
}

/// 一次命中。和鸡蛋的 `EggHit` 一样，只是这个类型由 `ctx` 的命令带走。
///
/// One hit, as for eggs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplashHit {
    target: SplashTarget,
    location: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplashTarget {
    Block,
    Mob(EntityUid),
    Player(EntityUid),
    Other,
}

/// `ProjectileUtil.getHitResultOnMoveVector`。
///
/// `ProjectileUtil.getHitResultOnMoveVector`.
fn find_hit(
    me: &Entity,
    ctx: &dyn EntityCtx,
    owner: Option<EntityUid>,
    left_owner: bool,
) -> Option<SplashHit> {
    let blocks = ctx.blocks();
    let from = me.position;
    let block = super::arrow::clip_including_border(
        blocks,
        from,
        from.add(me.delta),
        me.collision_context(),
    );
    let end = block.map_or_else(|| from.add(me.delta), |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let mut nearest = f64::MAX;
    let mut best = None;
    let mut consider = |uid: EntityUid, bb: Aabb, target: SplashTarget| {
        if !left_owner && Some(uid) == owner {
            return;
        }
        if let Some(location) = super::arrow::clip_entity(bb.inflated(margin), from, end) {
            let distance = from.distance_to_sqr(location);
            if distance < nearest {
                nearest = distance;
                best = Some(SplashHit { target, location });
            }
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        let bb = other.bounding_box();
        if other.removed || !bb.intersects(area) {
            return;
        }
        match &other.kind {
            KindPublic::Mob(mob) if mob.health > 0.0 => {
                consider(other.uid, bb, SplashTarget::Mob(other.uid));
            }
            KindPublic::Tnt { .. } | KindPublic::FallingBlock { .. } => {
                consider(other.uid, bb, SplashTarget::Other);
            }
            _ => {}
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = super::arrow::player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, bb, SplashTarget::Player(player.uid));
        }
    });
    best.or_else(|| {
        block.map(|hit| SplashHit {
            target: SplashTarget::Block,
            location: hit.location,
        })
    })
}

/// `Projectile.checkLeftOwner`。
///
/// `Projectile.checkLeftOwner`.
fn check_left_owner(me: &mut Entity, ctx: &dyn EntityCtx, owner: Option<EntityUid>) {
    if matches!(&me.kind, EntityKind::SplashPotion(potion) if potion.left_owner) {
        return;
    }
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let mut in_range = owner
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|entity| entity.bounding_box().intersects(area));
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive && !player.spectator && super::arrow::player_box(player).intersects(area) {
            in_range |= owner == Some(player.uid);
        }
    });
    if let EntityKind::SplashPotion(potion) = &mut me.kind {
        potion.left_owner = !in_range;
    }
}

/// `Projectile.tick` 的其余部分和 `Entity.baseTick`。
///
/// The rest of `Projectile.tick` and `Entity.baseTick`.
fn base_tick(me: &mut Entity, blocks: &dyn BlockView) {
    let in_lava = me.update_projectile_fluid(blocks);
    let sync_velocity =
        me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 && !in_lava;
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
    if let EntityKind::SplashPotion(potion) = &mut me.kind {
        potion.has_been_shot = true;
        potion.sync_velocity = sync_velocity;
    }
}

/// `AbstractThrownPotion.onHit`：先按药水组件把效果分给周围，再按有没有瞬间效果选
/// 2007 / 2002 和 1054 / 1053 号世界事件，最后消失。官方 `AbstractThrownPotion` 不覆写
/// `onHitEntity`，命中实体时不打额外的 0 点投掷伤害。
///
/// `AbstractThrownPotion.onHit`: hand the potion effects to the surroundings, emit level
/// events 2007/1054 or 2002/1053 depending on instant effects, then discard. Vanilla
/// `AbstractThrownPotion` does not override `onHitEntity`, so there is no extra 0-damage hit.
pub(super) fn impact(me: &mut Entity, world: &mut World<'_>, hit: SplashHit) {
    // `hitTargetOrDeflectSelf`：目标是会弹回投射物的生物（旋风人）时先弹回，不造成
    // 伤害、继续飞行；`SplashPotionImpact` 命令在这刻 tick 末尾处理，随机源状态一致。
    if let SplashTarget::Mob(uid) = hit.target
        && super::arrow::deflect_off_target(me, world, uid)
    {
        return;
    }
    let EntityKind::SplashPotion(potion) = &me.kind else {
        return;
    };
    let stack = potion.item.clone();
    let (effects, duration_scale) = super::arrow::potion_raw_effects_and_scale(&stack);
    let instant = super::arrow::potion_has_instant(&stack);
    let color = super::arrow::potion_contents_color(&stack);
    let silent = false;
    let box_now = me.bounding_box();
    let dx = hit.location.x - me.position.x;
    let dy = hit.location.y - me.position.y;
    let dz = hit.location.z - me.position.z;
    let moved = Aabb::new(
        box_now.min_x + dx,
        box_now.min_y + dy,
        box_now.min_z + dz,
        box_now.max_x + dx,
        box_now.max_y + dy,
        box_now.max_z + dz,
    );
    if !effects.is_empty() {
        apply_splash(me, world, moved, &effects, duration_scale);
    }
    let pos = me.block_position();
    world.push_effect(Effect::LevelEvent {
        event: if instant { 2007 } else { 2002 },
        pos,
        data: color,
        except_initiator: false,
    });
    if !silent {
        world.push_effect(Effect::LevelEvent {
            event: if instant { 1054 } else { 1053 },
            pos,
            data: 0,
            except_initiator: false,
        });
    }
    me.removed = true;
}

/// `DamageSources.indirectMagic(this, getOwner())`：击退方向用 `position`（`AbstractThrownPotion`
/// 覆写 `calculateHorizontalHurtKnockbackDirection` 为目标位置减药水位置，不取速度向量）。
///
/// `DamageSources.indirectMagic(this, getOwner())`: knockback uses `position` (matching
/// `AbstractThrownPotion.calculateHorizontalHurtKnockbackDirection`, not projectile motion).
fn indirect_magic_source(me: &Entity, world: &World<'_>) -> DamageSource {
    let EntityKind::SplashPotion(potion) = &me.kind else {
        return DamageSource::environment(DamageKind::IndirectMagic);
    };
    let owner_mob = potion
        .owner
        .and_then(|uid| world.level().entity_public(uid));
    let mut owner_player = None;
    if let Some(uid) = potion.owner {
        world.level().for_each_player(&mut |player| {
            if player.uid == uid {
                owner_player = Some(*player);
            }
        });
    }
    DamageSource {
        attacker: owner_mob
            .as_ref()
            .map(|owner| owner.uid)
            .or(owner_player.map(|player| player.uid)),
        direct: Some(me.id),
        attacker_is_player: owner_player.is_some(),
        creative_player: owner_player.is_some_and(|player| player.creative),
        position: Some(me.position),
        projectile_motion: None,
        attacker_uuid: potion.owner_uuid,
        attacker_may_build: true,
        scale_when_non_player: owner_mob
            .as_ref()
            .is_some_and(|owner| matches!(owner.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::IndirectMagic)
    }
}

/// `ThrownSplashPotion.onHitAsPotion`：把药水效果按距离分给周围生物和玩家。
///
/// `ThrownSplashPotion.onHitAsPotion`: hand the potion effects to nearby mobs and players
/// with distance falloff.
fn apply_splash(
    me: &Entity,
    world: &mut World<'_>,
    potion_box: Aabb,
    effects: &[EffectInstance],
    duration_scale: f32,
) {
    let area = Aabb::new(
        potion_box.min_x - 4.0,
        potion_box.min_y - 2.0,
        potion_box.min_z - 4.0,
        potion_box.max_x + 4.0,
        potion_box.max_y + 2.0,
        potion_box.max_z + 4.0,
    );
    // `ProjectileUtil.computeMargin(this)`：`clamp((tickCount - 2) / 20.0F, 0.0F, 0.3F)`。
    //
    // `ProjectileUtil.computeMargin(this)`: `clamp((tickCount - 2) / 20.0F, 0.0F, 0.3F)`.
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let mut mobs = Vec::new();
    world.level().for_each_entity_in(area, &mut |other| {
        if let KindPublic::Mob(mob) = &other.kind
            && !other.removed
            && mob.health > 0.0
        {
            mobs.push((
                other.uid,
                other.bounding_box(),
                mob.mob_type.is_inverted_heal_and_harm(),
            ));
        }
    });
    let mut players = Vec::new();
    world.level().for_each_player(&mut |player| {
        let bounds = super::arrow::player_box(player);
        if player.alive && !player.spectator && bounds.intersects(area) {
            players.push((player.uid, bounds));
        }
    });
    if mobs.is_empty() && players.is_empty() {
        return;
    }
    let source = indirect_magic_source(me, world);
    let owner = source
        .attacker
        .and_then(|uid| world.level().entity_public(uid));
    for (uid, bounds, inverted) in mobs {
        if let Some(scale) = splash_scale(potion_box, bounds, margin) {
            for effect in effects {
                apply_to_mob(world, uid, inverted, effect, scale, duration_scale, source);
            }
        }
    }
    for (uid, bounds) in players {
        if let Some(scale) = splash_scale(potion_box, bounds, margin) {
            for effect in effects {
                apply_to_player(
                    world,
                    uid,
                    effect,
                    scale,
                    duration_scale,
                    source,
                    owner.as_ref(),
                );
            }
        }
    }
}

/// `potionAabb.distanceToSqr(entity.getBoundingBox().inflate(margin)) < 16` 时的
/// `1.0 - sqrt(dist) / 4.0`。
///
/// `1.0 - sqrt(dist) / 4.0` when the squared distance is below sixteen.
fn splash_scale(potion_box: Aabb, bounds: Aabb, margin: f64) -> Option<f64> {
    let bounds = bounds.inflated(margin);
    let axis = |min_a: f64, max_a: f64, min_b: f64, max_b: f64| {
        (min_a - max_b).max(min_b - max_a).max(0.0)
    };
    let dx = axis(
        potion_box.min_x,
        potion_box.max_x,
        bounds.min_x,
        bounds.max_x,
    );
    let dy = axis(
        potion_box.min_y,
        potion_box.max_y,
        bounds.min_y,
        bounds.max_y,
    );
    let dz = axis(
        potion_box.min_z,
        potion_box.max_z,
        bounds.min_z,
        bounds.max_z,
    );
    let distance = dx * dx + dy * dy + dz * dz;
    (distance < SPLASH_RANGE * SPLASH_RANGE).then(|| 1.0 - distance.sqrt() / SPLASH_RANGE)
}

/// `ThrownSplashPotion.onHitAsPotion` 的生物分支：`HealOrHarmMobEffect.applyInstantaneousEffect`
/// 按 `(int)(scale * base + 0.5)` 取整并反转亡灵，其余效果按 `scale * duration * durationScale + 0.5`
/// 截断，20 刻以内（`endsWithin(20)`）不挂。
///
/// The mob branch of `ThrownSplashPotion.onHitAsPotion`: `HealOrHarmMobEffect` rounds
/// `(int)(scale * base + 0.5)` and inverts undead; other effects scale duration with
/// `scale * duration * durationScale + 0.5` and skip durations within twenty ticks.
fn apply_to_mob(
    world: &mut World<'_>,
    uid: EntityUid,
    inverted: bool,
    effect: &EffectInstance,
    scale: f64,
    duration_scale: f32,
    source: DamageSource,
) {
    if effect.effect == mob_effect::INSTANT_HEALTH || effect.effect == mob_effect::INSTANT_DAMAGE {
        let is_harm = effect.effect == mob_effect::INSTANT_DAMAGE;
        if is_harm == inverted {
            let amount = (scale * f64::from(4i32 << effect.amplifier) + 0.5) as i32 as f32;
            world.with_entity_mut(uid, &mut |entity| {
                if let Some(mob) = entity.kind.mob_mut()
                    && mob.health > 0.0
                {
                    let max = mob
                        .attributes
                        .value(mc_bamboo_data::registries::attribute::MAX_HEALTH)
                        as f32;
                    mob.health = (mob.health + amount).min(max);
                }
            });
        } else {
            let amount = (scale * f64::from(6i32 << effect.amplifier) + 0.5) as i32 as f32;
            world.hurt_mob(uid, source, amount);
        }
        return;
    }
    let duration = scaled_splash_duration(effect, scale, duration_scale);
    if !effect.is_infinite() && duration <= 20 {
        return;
    }
    let instance = EffectInstance::with_flags(
        effect.effect,
        duration,
        effect.amplifier,
        effect.ambient,
        effect.visible,
    );
    world.with_entity_mut(uid, &mut |entity| {
        if let Some(mob) = entity.kind.mob_mut() {
            living::add_effect(mob, instance.clone());
        }
    });
}

/// 玩家身上的同一套结算：瞬时治疗走 `heal_player`，瞬时伤害按 `indirectMagic` 走
/// `hurt_player_from_mob`，其余效果按距离缩放后挂上。
///
/// The same resolution for players: instant health heals by the scaled integer amount,
/// instant damage hurts with `indirectMagic`, and non-instant effects scale duration.
fn apply_to_player(
    world: &mut World<'_>,
    uid: EntityUid,
    effect: &EffectInstance,
    scale: f64,
    duration_scale: f32,
    source: DamageSource,
    owner: Option<&super::EntityPublic>,
) {
    if effect.effect == mob_effect::INSTANT_HEALTH {
        let amount = (scale * f64::from(4i32 << effect.amplifier) + 0.5) as i32 as f32;
        world.heal_player(uid, amount);
        return;
    }
    if effect.effect == mob_effect::INSTANT_DAMAGE {
        let amount = (scale * f64::from(6i32 << effect.amplifier) + 0.5) as i32 as f32;
        world.hurt_player_from_mob(uid, source, amount, owner);
        return;
    }
    let duration = scaled_splash_duration(effect, scale, duration_scale);
    if !effect.is_infinite() && duration <= 20 {
        return;
    }
    world.give_player_effect(
        uid,
        EffectInstance::with_flags(
            effect.effect,
            duration,
            effect.amplifier,
            effect.ambient,
            effect.visible,
        ),
    );
}

/// `MobEffectInstance.mapDuration(d -> (int)(scale * d * durationScale + 0.5))`。
///
/// `MobEffectInstance.mapDuration(d -> (int)(scale * d * durationScale + 0.5))`.
fn scaled_splash_duration(effect: &EffectInstance, scale: f64, duration_scale: f32) -> i32 {
    if effect.is_infinite() {
        crate::entity::mob::effects::INFINITE
    } else {
        (scale * f64::from(effect.duration) * f64::from(duration_scale) + 0.5) as i32
    }
}
