//! 扔出的鸡蛋，官方 `ThrownEgg`（`ThrowableItemProjectile` → `ThrowableProjectile` →
//! `Projectile`）。飞行每刻先重力后阻力，再沿速度找方块和实体命中；命中时打目标 0 点
//! `thrown` 伤害，按鸡蛋自己的随机源 1/8 孵小鸡（其中 1/32 孵四只），广播实体事件 3 后消失。
//!
//! A thrown egg, vanilla `ThrownEgg`. Each tick applies gravity then drag, looks for
//! block and entity hits along the velocity, and on impact deals 0 `thrown` damage,
//! hatches chicks with the egg's own random (1/8, and 1/32 of those hatch four), then
//! broadcasts entity event 3 and disappears.

use mc_bamboo_core::{Aabb, Vec3, mth};
use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_protocol::components::{ComponentValue, defaults};
use mc_bamboo_protocol::item::ItemStack;

use super::ctx::{BlockView, Command, EntityCtx};
use super::mob::{CowVariant, DamageKind, DamageSource, MobType};
use super::{Entity, EntityKind, EntityUid, KindPublic};
use crate::World;

/// `ThrownEgg` 自己的字段，加上 `Projectile` 里的主人和标志。
///
/// The `ThrownEgg` fields plus the owner and flags from `Projectile`.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrownEgg {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `DATA_ITEM_STACK`：`setItem` 存的是数量为 1 的拷贝。
    ///
    /// `DATA_ITEM_STACK`: `setItem` stores a copy with count 1.
    pub item: ItemStack,
    pub x_rot: f32,
    pub no_gravity: bool,
    pub left_owner: bool,
    pub has_been_shot: bool,
    needs_sync: bool,
    sync_velocity: bool,
    /// 这一刻命中后广播了实体事件 3（碎蛋粒子），等同步层取走。
    ///
    /// Entity event 3 (the shell particles) was broadcast on impact this tick.
    cracked: bool,
}

impl ThrownEgg {
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
            cracked: false,
        }
    }

    /// 命中后广播过实体事件 3，还没被同步层取走。
    ///
    /// Entity event 3 was broadcast on impact and not yet taken.
    pub(crate) const fn is_cracked(&self) -> bool {
        self.cracked
    }

    pub(crate) fn public(&self, remaining_fire_ticks: i32) -> EggPublic {
        EggPublic {
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
pub struct EggPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub x_rot: f32,
    pub item: ItemStack,
    pub on_fire: bool,
    pub no_gravity: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 一次扔蛋的输入：玩家 `EggItem.use` 和发射器 `ProjectileDispenseBehavior` 共用。
///
/// One throw's inputs, shared by the player's `EggItem.use` and the dispenser's
/// `ProjectileDispenseBehavior`.
#[derive(Debug, Clone)]
pub struct EggShot {
    pub position: Vec3,
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    /// `shootFromRotation` 叠加的射手已知位移；发射器为 `None`。
    ///
    /// The shooter's known movement added by `shootFromRotation`; `None` for a dispenser.
    pub source_movement: Option<Vec3>,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub item: ItemStack,
}

impl Entity {
    /// `new ThrownEgg(level, x, y, z, stack)` 加上主人。
    ///
    /// `new ThrownEgg(level, x, y, z, stack)` plus the owner.
    pub fn thrown_egg(shot: &EggShot) -> Self {
        let mut egg = ThrownEgg::new(&shot.item);
        egg.owner = shot.owner;
        egg.owner_uuid = shot.owner_uuid;
        Self::new(EntityKind::Egg(Box::new(egg)), shot.position)
    }

    /// `Projectile.shoot`，再按 `shootFromRotation` 加上射手的已知位移。
    ///
    /// `Projectile.shoot`, then the shooter's known movement as `shootFromRotation` adds it.
    pub fn shoot_egg(&mut self, shot: &EggShot) {
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
        let EntityKind::Egg(egg) = &mut self.kind else {
            return;
        };
        self.delta = movement;
        egg.needs_sync = true;
        let (yaw, pitch) = rotation(movement);
        self.y_rot = yaw;
        egg.x_rot = pitch;
        if let Some(source) = shot.source_movement {
            self.delta = self.delta.add(source);
        }
    }
}

impl World<'_> {
    /// 生成一枚扔出的蛋：先分配 ID 和种子（构造函数的 UUID 取数），再按 `shoot` 取散布。
    ///
    /// Spawns one thrown egg: the id and seed first (the constructor's UUID draw), then
    /// the spread of `shoot`.
    pub fn spawn_egg_shot(&mut self, shot: &EggShot) -> i32 {
        let mut entity = self.prepare_entity(Entity::thrown_egg(shot));
        entity.shoot_egg(shot);
        self.add_prepared_entity(entity)
    }
}

/// `Projectile.shoot` / `updateRotation` 的目标朝向。反编译出来的 `180.0F / (float) PI`
/// 是编译期折叠好的 float 常量 `57.295776F`，所以是 double 乘这个常量再截成 float。
///
/// The target angles of `Projectile.shoot` / `updateRotation`. The decompiled
/// `180.0F / (float) PI` is the folded float constant `57.295776F`, so the double is
/// multiplied by that constant and then narrowed.
pub(super) fn rotation(movement: Vec3) -> (f32, f32) {
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let degrees = |radians: f64| (radians * f64::from(180.0_f32 / std::f32::consts::PI)) as f32;
    (
        degrees(mth::atan2(movement.x, movement.z)),
        degrees(mth::atan2(movement.y, horizontal)),
    )
}

/// `Projectile.lerpRotation`。
pub(super) fn lerp_rotation(from: f32, to: f32) -> f32 {
    let mut from = from;
    while to - from < -180.0 {
        from -= 360.0;
    }
    while to - from >= 180.0 {
        from += 360.0;
    }
    from + 0.2 * (to - from)
}

/// 一次命中。`location` 是 `HitResult.getLocation()`。
///
/// One hit; `location` is `HitResult.getLocation()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EggHit {
    target: EggTarget,
    location: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EggTarget {
    /// 方块或世界边界（鸡蛋不在边界上反弹，照样 `onHit`）。
    ///
    /// A block or the world border (eggs do not bounce off the border).
    Block,
    Mob(EntityUid),
    Player(EntityUid),
    /// TNT 和下落方块：`isPickable` 为真，但 `hurt` 不改任何状态。
    ///
    /// Primed TNT and falling blocks: pickable, but `hurt` changes nothing here.
    Other,
}

/// `ThrowableProjectile.tick`，在 `commonTick` 之后。
///
/// `ThrowableProjectile.tick`, after `commonTick`.
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let blocks = ctx.blocks();
    let EntityKind::Egg(egg) = &mut me.kind else {
        return;
    };
    egg.needs_sync = false;
    egg.sync_velocity = false;
    // `handleFirstTickBubbleColumn` 没有建模，见 README 已知限制。
    // `handleFirstTickBubbleColumn` is not modelled; see the README limitations.
    if !egg.no_gravity {
        me.delta = me.delta.add(Vec3::new(0.0, -0.03, 0.0));
    }
    let inertia = if me.in_water { 0.8_f32 } else { 0.99_f32 };
    me.delta = me.delta.scale(f64::from(inertia));
    let owner = egg.owner.or_else(|| {
        egg.owner_uuid
            .and_then(|uuid| super::arrow::resolve_owner(ctx, uuid))
    });
    let left_owner = egg.left_owner;
    if let EntityKind::Egg(egg) = &mut me.kind {
        egg.owner = owner;
    }
    let hit = find_hit(me, ctx, owner, left_owner);
    me.position = hit.map_or_else(|| me.position.add(me.delta), |hit| hit.location);
    update_rotation(me);
    let extinguish_pitch = super::arrow::projectile_travel_effects(me, blocks, me.old_position);
    check_left_owner(me, ctx, owner);
    base_tick(me, blocks);
    if let Some(pitch) = extinguish_pitch {
        ctx.emit(Command::ProjectileExtinguished { pitch }, me);
    }
    if let Some(hit) = hit
        && !me.removed
    {
        ctx.emit(Command::EggImpact { hit }, me);
    }
}

/// `Projectile.updateRotation`：从当前朝向往速度方向插值五分之一。
///
/// `Projectile.updateRotation`: one fifth of the way towards the velocity.
fn update_rotation(me: &mut Entity) {
    let (yaw, pitch) = rotation(me.delta);
    let EntityKind::Egg(egg) = &mut me.kind else {
        return;
    };
    egg.x_rot = lerp_rotation(egg.x_rot, pitch);
    me.y_rot = lerp_rotation(me.y_rot, yaw);
}

/// `ProjectileUtil.getHitResultOnMoveVector`：方块裁剪（含世界边界）之后，在缩短的线段上
/// 找最近的实体；有实体就取实体。
///
/// `ProjectileUtil.getHitResultOnMoveVector`: clip blocks (and the world border), then
/// find the nearest entity on the shortened segment, which wins when present.
fn find_hit(
    me: &Entity,
    ctx: &dyn EntityCtx,
    owner: Option<EntityUid>,
    left_owner: bool,
) -> Option<EggHit> {
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
    let mut consider = |uid: EntityUid, bb: Aabb, target: EggTarget| {
        // `canHitEntity`：没离开主人之前不打主人自己（没有载具，同载具就是它本身）。
        // `canHitEntity`: the owner is skipped until the egg leaves it.
        if !left_owner && Some(uid) == owner {
            return;
        }
        if let Some(location) = super::arrow::clip_entity(bb.inflated(margin), from, end) {
            let distance = from.distance_to_sqr(location);
            if distance < nearest {
                nearest = distance;
                best = Some(EggHit { target, location });
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
                consider(other.uid, bb, EggTarget::Mob(other.uid));
            }
            KindPublic::Tnt { .. } | KindPublic::FallingBlock { .. } => {
                consider(other.uid, bb, EggTarget::Other);
            }
            _ => {}
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = super::arrow::player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, bb, EggTarget::Player(player.uid));
        }
    });
    best.or_else(|| {
        block.map(|hit| EggHit {
            target: EggTarget::Block,
            location: hit.location,
        })
    })
}

/// `Projectile.checkLeftOwner`：主人（含玩家）不再碰到「当前碰撞盒沿速度展开再外扩 1」时离开。
///
/// `Projectile.checkLeftOwner`: the egg has left once its owner no longer touches the
/// bounding box expanded along the velocity and inflated by 1.
fn check_left_owner(me: &mut Entity, ctx: &dyn EntityCtx, owner: Option<EntityUid>) {
    if matches!(&me.kind, EntityKind::Egg(egg) if egg.left_owner) {
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
    if let EntityKind::Egg(egg) = &mut me.kind {
        egg.left_owner = !in_range;
    }
}

/// `Projectile.tick` 的其余部分和 `Entity.baseTick`：流体、着火、掉出世界。
///
/// The rest of `Projectile.tick` and `Entity.baseTick`: fluids, fire and the void.
fn base_tick(me: &mut Entity, blocks: &dyn BlockView) {
    let in_lava = me.update_projectile_fluid(blocks);
    // 着火伤害让 `Projectile.markHurt`，下一次同步要发速度。
    // The burning pulse calls `Projectile.markHurt`, so velocity is synced.
    let sync_velocity =
        me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 && !in_lava;
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
    if let EntityKind::Egg(egg) = &mut me.kind {
        egg.has_been_shot = true;
        egg.sync_velocity = sync_velocity;
    }
}

/// `ThrownEgg.onHit`：先 `onHitEntity` 打 0 点伤害，再用鸡蛋自己的随机源决定孵几只。
///
/// `ThrownEgg.onHit`: `onHitEntity` deals 0 damage first, then the egg's own random
/// decides how many chicks hatch.
pub(super) fn impact(me: &mut Entity, world: &mut World<'_>, hit: EggHit) {
    match hit.target {
        EggTarget::Mob(uid) => {
            world.hurt_mob(uid, damage_source(me, world), 0.0);
        }
        EggTarget::Player(uid) => {
            let source = damage_source(me, world);
            let owner = source
                .attacker
                .and_then(|uid| world.level().entity_public(uid));
            world.hurt_player_from_mob(uid, source, 0.0, owner.as_ref());
        }
        EggTarget::Block | EggTarget::Other => {}
    }
    if me.random.next_int_bounded(8) == 0 {
        let count = if me.random.next_int_bounded(32) == 0 {
            4
        } else {
            1
        };
        for _ in 0..count {
            if !hatch(me, world) {
                break;
            }
        }
    }
    if let EntityKind::Egg(egg) = &mut me.kind {
        egg.cracked = true;
    }
    me.removed = true;
}

/// `damageSources().thrown(this, getOwner())`：没有主人时造成者为空，直接实体是蛋。
///
/// `damageSources().thrown(this, getOwner())`: no cause without an owner; the egg is
/// the direct entity.
fn damage_source(me: &Entity, world: &World<'_>) -> DamageSource {
    let EntityKind::Egg(egg) = &me.kind else {
        return DamageSource::environment(DamageKind::Thrown);
    };
    let owner_mob = egg.owner.and_then(|uid| world.level().entity_public(uid));
    let mut owner_player = None;
    if let Some(uid) = egg.owner {
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
        projectile_motion: Some(me.delta),
        attacker_uuid: egg.owner_uuid,
        scale_when_non_player: owner_mob
            .as_ref()
            .is_some_and(|owner| matches!(owner.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::Thrown)
    }
}

/// 孵一只小鸡：`EntityType.CHICKEN.create`、`setAge(-24000)`、`snapTo` 到蛋的位置和
/// 朝向、按蛋的 `chicken/variant` 组件定花纹，再 `fudgePositionAfterSizeChange`。
/// 找不到空位时返回假，调用方停止孵剩下的。
///
/// Hatch one chick: create, `setAge(-24000)`, `snapTo` the egg's position and yaw,
/// take the variant from the egg's `chicken/variant` component, then fudge the
/// position. False when no free spot exists, which stops the remaining chicks.
fn hatch(me: &Entity, world: &mut World<'_>) -> bool {
    let EntityKind::Egg(egg) = &me.kind else {
        return false;
    };
    let variant = item_variant(&egg.item);
    let yaw = me.y_rot;
    let mut chick = world.prepare_entity(Entity::by_mob_type(MobType::Chicken, me.position));
    chick.with_mob(|entity, mob| {
        super::mob::living::set_age(entity, mob, -24000);
        // `snapTo` 只改 yRot / xRot 和旧值；构造时的 yHeadRot 保留，身体朝向仍是 0。
        // `snapTo` sets yRot / xRot and their old values; the constructor's head yaw stays.
        entity.y_rot = yaw;
        mob.y_rot_o = yaw;
        mob.x_rot = 0.0;
        mob.x_rot_o = 0.0;
        if let Some(variant) = variant {
            mob.variant = variant;
        }
    });
    chick.old_position = me.position;
    let Some(position) = super::resize::fudge_position(world, &chick, (0.0, 0.0)) else {
        return false;
    };
    chick.position = position;
    world.add_prepared_entity(chick);
    true
}

/// 蛋上的 `minecraft:chicken/variant` 组件（含物品默认值）。
///
/// The egg's `minecraft:chicken/variant` component, item defaults included.
fn item_variant(stack: &ItemStack) -> Option<CowVariant> {
    let patch = stack.patch();
    let Some(ComponentValue::ChickenVariant(id)) =
        defaults::resolve(stack.item, &patch, kinds::CHICKEN_VARIANT)
    else {
        return None;
    };
    let names = mc_bamboo_data::component_registries::registry_names::CHICKEN_VARIANT;
    names
        .get(usize::try_from(*id).ok()?)
        .and_then(|name| CowVariant::from_name(name))
}

/// 取走这一刻的碎蛋事件。
///
/// Take this tick's shell-crack event.
pub(crate) fn take_cracked(egg: &mut ThrownEgg) -> bool {
    std::mem::take(&mut egg.cracked)
}

#[cfg(test)]
mod tests;
