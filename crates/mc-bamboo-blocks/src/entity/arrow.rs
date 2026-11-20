use mc_bamboo_core::{Aabb, Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{items as item, sounds};
use mc_bamboo_protocol::item::ItemStack;

use super::ctx::{BlockView, Command, EntityCtx, PlayerView};
use super::mob::{DamageKind, DamageSource};
use super::{Entity, EntityKind, EntityUid, KindPublic};
use crate::{BlockHit, BlockState, CollisionContext, World};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pickup {
    #[default]
    Disallowed,
    Allowed,
    CreativeOnly,
}

/// 箭的两种官方类：`Arrow` 和 `SpectralArrow`。
///
/// 两者共用 `AbstractArrow` 的飞行、命中和持久化，只有命中后的效果和实体类型不同。
///
/// The two official classes: `Arrow` and `SpectralArrow`. They share
/// `AbstractArrow` flight, impacts and persistence; only the post-hit effect
/// and the entity type differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrowVariant {
    #[default]
    Normal,
    /// `SpectralArrow`，`duration` 是发光刻数（默认 200）。
    ///
    /// `SpectralArrow`; `duration` is the glowing tick count (default 200).
    Spectral { duration: i32 },
}

impl ArrowVariant {
    /// 注册表里的实体类型。
    ///
    /// The registry entity type.
    pub const fn entity_type(self) -> i32 {
        match self {
            Self::Normal => mc_bamboo_data::registries::entity_type::ARROW,
            Self::Spectral { .. } => mc_bamboo_data::registries::entity_type::SPECTRAL_ARROW,
        }
    }

    /// 按弹药物品决定箭的种类。`SpectralArrowItem.createArrow` 造 `SpectralArrow`。
    ///
    /// The variant chosen by the ammo item; `SpectralArrowItem.createArrow`
    /// builds a `SpectralArrow`.
    pub fn of_ammo(ammo: &ItemStack) -> Self {
        if ammo.item == item::SPECTRAL_ARROW {
            Self::Spectral { duration: 200 }
        } else {
            Self::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub critical: bool,
    pub pierce: i8,
    pub in_ground: bool,
    pub shake_time: u8,
    pub pickup: Pickup,
    pub on_fire: bool,
    pub no_gravity: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
    pub variant: ArrowVariant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub pitch: f32,
    pub no_gravity: bool,
    pub base_damage: f64,
    pub critical: bool,
    pub pierce: i8,
    pub in_ground: bool,
    pub shake_time: u8,
    pub life: i32,
    pub pickup: Pickup,
    pub pickup_item: ItemStack,
    pub weapon: Option<ItemStack>,
    pub last_state: Option<BlockState>,
    pub sound: u16,
    pub left_owner: bool,
    pub variant: ArrowVariant,
    /// `Projectile.leftOwnerChecked`：同一刻内只查一次，每刻末尾由 `Projectile.tick` 清掉。
    ///
    /// `Projectile.leftOwnerChecked`: at most one check per tick, cleared by
    /// `Projectile.tick` at the end of every tick.
    left_owner_checked: bool,
    pub has_been_shot: bool,
    needs_sync: bool,
    sync_velocity: bool,
    pierced: Vec<EntityUid>,
}

impl Default for Arrow {
    fn default() -> Self {
        Self {
            uuid: [0; 16],
            owner: None,
            owner_uuid: None,
            pitch: 0.0,
            no_gravity: false,
            base_damage: 2.0,
            critical: false,
            pierce: 0,
            in_ground: false,
            shake_time: 0,
            life: 0,
            pickup: Pickup::Disallowed,
            pickup_item: ItemStack::new(item::ARROW, 1),
            weapon: None,
            last_state: None,
            sound: arrow_hit_sound(),
            left_owner: false,
            left_owner_checked: false,
            has_been_shot: false,
            needs_sync: false,
            sync_velocity: false,
            pierced: Vec::new(),
            variant: ArrowVariant::Normal,
        }
    }
}

impl Arrow {
    pub(crate) fn public(&self, remaining_fire_ticks: i32) -> ArrowPublic {
        ArrowPublic {
            uuid: self.uuid,
            owner: self.owner,
            pitch: self.pitch,
            critical: self.critical,
            pierce: self.pierce,
            in_ground: self.in_ground,
            shake_time: self.shake_time,
            pickup: self.pickup,
            on_fire: remaining_fire_ticks > 0,
            no_gravity: self.no_gravity,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
            variant: self.variant,
        }
    }
}

pub(crate) fn arrow_hit_sound() -> u16 {
    sounds::sound_event("minecraft:entity.arrow.hit")
}

impl Entity {
    pub fn arrow(position: Vec3) -> Self {
        Self::new(EntityKind::Arrow(Box::default()), position)
    }

    /// 一次射箭的输入。玩家弓、弩和发射器都走这里，区别只在弹药、武器和拾取方式。
    ///
    /// One shot's inputs. The player bow, crossbow and dispenser all use this;
    /// only ammo, weapon and pickup mode differ.
    pub fn arrow_from_shot(shot: ArrowShot) -> Self {
        let mut entity = Self::configured_arrow(&shot);
        entity.shoot_arrow_with_movement(
            shot.direction,
            shot.power,
            shot.uncertainty,
            shot.source_movement,
        );
        entity
    }

    /// 只设字段、不射。调用方先用 [`EntityStore::prepare`] 拿种子和 UUID，再射。
    ///
    /// Configure only. The caller lets [`EntityStore::prepare`] draw the seed and
    /// UUID first, then shoots.
    pub fn configured_arrow(shot: &ArrowShot) -> Self {
        let mut entity = Self::arrow(shot.position);
        let EntityKind::Arrow(arrow) = &mut entity.kind else {
            unreachable!()
        };
        arrow.variant = ArrowVariant::of_ammo(&shot.ammo);
        arrow.pickup = shot.pickup;
        arrow.pickup_item = shot.ammo.clone();
        arrow.owner = shot.owner;
        arrow.owner_uuid = shot.owner_uuid;
        arrow.weapon = shot.weapon.clone();
        arrow.base_damage = 2.0;
        if let Some(weapon) = &arrow.weapon {
            if weapon.item == mc_bamboo_data::items::CROSSBOW {
                arrow.sound = mc_bamboo_data::sounds::sound_event("minecraft:item.crossbow.hit");
            }
            arrow.pierce = enchantment_level(weapon, "minecraft:piercing") as i8;
            if enchantment_level(weapon, "minecraft:flame") > 0 {
                entity.remaining_fire_ticks = 2000;
            }
        }
        arrow.critical = shot.critical;
        entity
    }

    /// Projectile.shoot：先归一化，再按 XYZ 顺序抽散布，最后乘速度。
    ///
    /// Projectile.shoot normalizes first, draws spread in XYZ order, then scales.
    pub fn shoot_arrow(&mut self, direction: Vec3, power: f32, uncertainty: f32) {
        self.shoot_arrow_with_movement(direction, power, uncertainty, None);
    }

    /// `Projectile.shootFromRotation` 的最后一步：再加上射手的已知移动。
    ///
    /// The last step of `Projectile.shootFromRotation`: add the shooter's known movement.
    pub fn shoot_arrow_with_movement(
        &mut self,
        direction: Vec3,
        power: f32,
        uncertainty: f32,
        source_movement: Option<Vec3>,
    ) {
        let EntityKind::Arrow(arrow) = &mut self.kind else {
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
        if let Some(movement) = source_movement {
            let y = if self.on_ground { 0.0 } else { movement.y };
            self.delta = self.delta.add(Vec3::new(movement.x, y, movement.z));
        }
        let (yaw, pitch) = rotation(self.delta);
        self.y_rot = yaw;
        arrow.pitch = pitch;
        arrow.life = 0;
        arrow.needs_sync = true;
    }
}

/// [`Entity::arrow_from_shot`] 的输入。
///
/// The inputs to [`Entity::arrow_from_shot`].
#[derive(Debug, Clone)]
pub struct ArrowShot {
    pub position: Vec3,
    /// 射手视线方向，`shootFromRotation` 的 `(x, y, z)`；发射器传朝向的步长。
    ///
    /// The shooter's view vector; the dispenser passes the facing step.
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    /// `ServerPlayer.lastKnownClientMovement`；发射器为 `None`。
    ///
    /// `ServerPlayer.lastKnownClientMovement`; `None` for a dispenser.
    pub source_movement: Option<Vec3>,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// 弹药物品，原样写进 `pickup`（含药水组件）。
    ///
    /// The ammo stack, stored as-is for pickup (including potion components).
    pub ammo: ItemStack,
    /// 弓或弩；发射器为 `None`。
    ///
    /// The bow or crossbow; `None` for a dispenser.
    pub weapon: Option<ItemStack>,
    pub critical: bool,
    pub pickup: Pickup,
}

fn rotation(movement: Vec3) -> (f32, f32) {
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let deg = |r| (r * f64::from(180.0_f32 / std::f32::consts::PI)) as f32;
    (
        deg(mth::atan2(movement.x, movement.z)),
        deg(mth::atan2(movement.y, horizontal)),
    )
}

fn lerp_rotation(from: f32, to: f32) -> f32 {
    let mut from = from;
    while to - from < -180.0 {
        from -= 360.0;
    }
    while to - from >= 180.0 {
        from += 360.0;
    }
    from + 0.2 * (to - from)
}

fn at_point(pos: Vec3, radius: f64) -> Aabb {
    Aabb::new(pos.x, pos.y, pos.z, pos.x, pos.y, pos.z).inflated(radius)
}

pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let movement = me.delta;
    if !tick_or_check_ground(me, ctx) {
        fly(me, ctx, movement);
    }
}

fn tick_or_check_ground(me: &mut Entity, ctx: &mut dyn EntityCtx) -> bool {
    let blocks = ctx.blocks();
    let position = me.position;
    let block_pos = me.block_position();
    let current_state = blocks.block_state(block_pos);
    let collision = me.collision_context();
    let surrounding = at_point(position, 0.06);
    let top = BlockPos::containing(Vec3::new(position.x, me.bounding_box().max_y, position.z));
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return true;
    };
    arrow.needs_sync = false;
    arrow.sync_velocity = false;
    let nearby_blocks = blocks.block_collisions(surrounding, collision);
    if nearby_blocks
        .iter()
        .filter(|(pos, _)| *pos == block_pos)
        .any(|(_, shapes)| {
            shapes
                .iter()
                .any(|bb| bb.contains(position.x, position.y, position.z))
        })
    {
        me.delta = Vec3::default();
        // `setInGround(true)` 只在值变化时触发 `onSyncedDataUpdated`，那里才设震动。
        // `setInGround(true)` only fires `onSyncedDataUpdated` on a real change, which
        // is where the shake is set.
        let was_in_ground = arrow.in_ground;
        arrow.in_ground = true;
        if !was_in_ground && !me.first_tick && arrow.shake_time == 0 {
            arrow.shake_time = 7;
        }
    }
    arrow.shake_time = arrow.shake_time.saturating_sub(1);
    if me.in_water || blocks.is_raining_at(block_pos) || blocks.is_raining_at(top) {
        me.remaining_fire_ticks = me.remaining_fire_ticks.min(0);
    }
    if arrow.in_ground {
        if arrow.last_state != Some(current_state) && nearby_blocks.is_empty() {
            arrow.in_ground = false;
            me.delta = Vec3::new(
                me.delta.x * f64::from(me.random.next_float() * 0.2),
                me.delta.y * f64::from(me.random.next_float() * 0.2),
                me.delta.z * f64::from(me.random.next_float() * 0.2),
            );
            arrow.life = 0;
        } else {
            arrow.life += 1;
            me.removed = arrow.life >= 1200;
        }
        if !me.removed
            && let Some(pitch) = travel_effects(me, blocks, position)
        {
            ctx.emit(Command::ArrowExtinguished { pitch }, me);
        }
        true
    } else {
        false
    }
}

fn fly(me: &mut Entity, ctx: &mut dyn EntityCtx, movement: Vec3) {
    let blocks = ctx.blocks();
    let position = me.position;
    let collision = me.collision_context();
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return;
    };
    if me.in_water {
        me.delta = me.delta.scale(f64::from(0.6_f32));
    }
    let (yaw, pitch) = rotation(movement);
    me.y_rot = lerp_rotation(me.y_rot, yaw);
    arrow.pitch = lerp_rotation(arrow.pitch, pitch);
    let owner = arrow
        .owner
        .or_else(|| arrow.owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)));
    let area = sweep_area(me);
    let mut owner_in_range = owner
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|entity| entity.bounding_box().intersects(area));
    blocks.for_each_player(&mut |p| {
        if p.alive && !p.spectator && player_box(p).intersects(area) {
            owner_in_range |= owner == Some(p.uid);
        }
    });
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        unreachable!()
    };
    arrow.owner = owner;
    // `Projectile.checkLeftOwner` 只在第一次调用时更新，之后 `leftOwnerChecked` 挡住。
    // `Projectile.checkLeftOwner` only updates on its first call; `leftOwnerChecked`
    // blocks every later one within the same tick.
    if !arrow.left_owner && !arrow.left_owner_checked {
        arrow.left_owner = !owner_in_range;
        arrow.left_owner_checked = true;
    }
    let end = position.add(movement);
    let block_hit = clip_including_border(blocks, position, end, collision);
    let end = block_hit.map_or(end, |hit| hit.location);
    let hits = find_targets(me, ctx, end);
    if hits.is_empty() && block_hit.is_none() {
        me.position = end;
        if let Some(pitch) = travel_effects(me, blocks, position) {
            ctx.emit(Command::ArrowExtinguished { pitch }, me);
        }
        finish_flight(me, ctx.blocks());
    } else {
        ctx.emit(
            Command::ArrowImpact {
                block: block_hit,
                hits,
                end,
            },
            me,
        );
    }
}

fn sweep_area(me: &Entity) -> Aabb {
    me.bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0)
}

fn find_targets(me: &Entity, ctx: &dyn EntityCtx, end: Vec3) -> Vec<ArrowEntityHit> {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return Vec::new();
    };
    let blocks = ctx.blocks();
    let area = sweep_area(me);
    let position = me.position;
    let collision = me.collision_context();
    let mut hits = Vec::new();
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let mut consider = |uid, feet, bb: Aabb, player| {
        if (arrow.left_owner || Some(uid) != arrow.owner) && !arrow.pierced.contains(&uid) {
            let contact = clip_entity_with_margin(blocks, bb, position, end, collision, margin);
            if let Some(contact) = contact {
                hits.push((
                    position.distance_to_sqr(feet),
                    ArrowEntityHit {
                        uid,
                        contact,
                        player,
                    },
                ));
            }
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        if !other.removed && matches!(&other.kind, KindPublic::Mob(mob) if mob.health > 0.0) {
            consider(other.uid, other.position, other.bounding_box(), false);
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, player.position, bb, true);
        }
    });
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.into_iter().map(|(_, hit)| hit).collect()
}

fn clip_entity_with_margin(
    view: &dyn BlockView,
    bb: Aabb,
    from: Vec3,
    to: Vec3,
    context: CollisionContext,
    margin: f64,
) -> Option<Vec3> {
    clip_entity(bb, from, to).or_else(|| {
        if margin == 0.0 {
            return None;
        }
        let outside = clip_entity(bb.inflated(margin), from, to)?;
        let center = bb.center();
        let inside = clip_including_border(view, outside, center, context)
            .map_or(center, |hit| hit.location);
        clip_entity(bb, outside, inside).map(|_| outside)
    })
}

fn clip_including_border(
    view: &dyn BlockView,
    from: Vec3,
    to: Vec3,
    context: CollisionContext,
) -> Option<BlockHit> {
    let hit = view.clip_collider(from, to, context);
    let end = hit.map_or(to, |hit| hit.location);
    let border = view.world_border();
    if !border.is_within_bounds(from.x, from.z, 0.0) || border.is_within_bounds(end.x, end.z, 0.0) {
        return hit;
    }
    let delta = end.subtract(from);
    let mut face = Direction::North;
    let mut highest = f32::from_bits(1);
    for direction in Direction::ALL {
        let (x, y, z) = direction.normal();
        let dot = delta.x as f32 * x as f32 + delta.y as f32 * y as f32 + delta.z as f32 * z as f32;
        if dot > highest {
            highest = dot;
            face = direction;
        }
    }
    let location = border.clamp_vec3(end);
    Some(BlockHit {
        pos: BlockPos::containing(location),
        face,
        location,
        inside: false,
    })
}

fn resolve_owner(ctx: &dyn EntityCtx, uuid: [u8; 16]) -> Option<EntityUid> {
    let mut player = None;
    ctx.blocks().for_each_player(&mut |view| {
        if view.uuid == Some(uuid) {
            player = Some(view.uid);
        }
    });
    player.or_else(|| {
        ctx.entity_uids().into_iter().find(|&uid| {
            ctx.other(uid).is_some_and(
                |entity| matches!(entity.kind, KindPublic::Mob(mob) if mob.uuid == uuid),
            )
        })
    })
}

fn player_box(player: &PlayerView) -> Aabb {
    let p = player.position;
    let half = f64::from(0.6_f32) / 2.0;
    Aabb::new(
        p.x - half,
        p.y,
        p.z - half,
        p.x + half,
        p.y + f64::from(if player.sneaking { 1.5_f32 } else { 1.8_f32 }),
        p.z + half,
    )
}

// AABB.clip 只接受线段内部的入射面交点，不把盒内起点当成命中。
// AABB.clip only accepts entering faces strictly within the segment.
fn clip_entity(bb: Aabb, from: Vec3, to: Vec3) -> Option<Vec3> {
    let start = [from.x, from.y, from.z];
    let d = to.subtract(from);
    let steps = [d.x, d.y, d.z];
    let axes = [Axis::X, Axis::Y, Axis::Z];
    let mut best = 1.0;
    for (i, axis) in axes.into_iter().enumerate() {
        let plane = if steps[i] > 1.0e-7 {
            bb.min(axis)
        } else if steps[i] < -1.0e-7 {
            bb.max(axis)
        } else {
            continue;
        };
        let t = (plane - start[i]) / steps[i];
        if t > 0.0
            && t < best
            && (1..=2).all(|n| {
                let j = (i + n) % 3;
                let value = start[j] + t * steps[j];
                bb.min(axes[j]) - 1.0e-7 < value && value < bb.max(axes[j]) + 1.0e-7
            })
        {
            best = t;
        }
    }
    (best < 1.0).then(|| from.add(d.scale(best)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowEntityHit {
    uid: EntityUid,
    contact: Vec3,
    player: bool,
}

pub(super) fn impact(
    me: &mut Entity,
    world: &mut World<'_>,
    mut hits: Vec<ArrowEntityHit>,
    block: Option<BlockHit>,
    end: Vec3,
) {
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.needs_sync = true;
    }
    while !me.removed {
        let from = me.position;
        me.position = hits.first().map_or(end, |hit| hit.contact);
        if let Some(pitch) = travel_effects(me, world, from) {
            play_extinguished_sound(me, world, pitch);
        }
        if me.removed {
            break;
        }
        if hits.is_empty() {
            if let Some(hit) = block {
                hit_block(me, world, hit);
            }
            break;
        }
        if !hits.into_iter().all(|hit| hit_entity(me, world, hit)) {
            break;
        }
        // 这一批命中后，沿原先的方块裁剪终点重查剩余线段。
        // After this batch, query the remaining segment against the original block-clip endpoint.
        let ctx = super::ctx::PreciseCtx::new(world, me.id);
        hits = find_targets(me, &ctx, end);
    }
    finish_flight(me, world);
}

fn travel_effects(me: &mut Entity, blocks: &dyn BlockView, from: Vec3) -> Option<f32> {
    let burning = me.remaining_fire_ticks > 0;
    me.apply_effects_along(blocks, from);
    // Entity.playEntityOnFireExtinguishedSound 使用本实体的随机源。
    // Entity.playEntityOnFireExtinguishedSound uses the entity's own random stream.
    (burning && me.remaining_fire_ticks <= 0)
        .then(|| 1.6 + (me.random.next_float() - me.random.next_float()) * 0.4)
}

pub(super) fn play_extinguished_sound(me: &Entity, world: &mut World<'_>, pitch: f32) {
    world.play_sound(
        me.position,
        sounds::sound_event("minecraft:entity.generic.extinguish_fire"),
        sounds::NEUTRAL,
        0.7,
        pitch,
        false,
    );
}

fn hit_entity(me: &mut Entity, world: &mut World<'_>, hit: ArrowEntityHit) -> bool {
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return false;
    };
    if arrow.pierce > 0 {
        if arrow.pierced.len() > arrow.pierce as usize {
            me.removed = true;
            return false;
        }
        arrow.pierced.push(hit.uid);
    }
    let speed = me.delta.length() as f32;
    let mut damage = (f64::from(speed) * impact_damage(arrow))
        .clamp(0.0, f64::from(i32::MAX))
        .ceil() as i32;
    if arrow.critical {
        damage = damage.saturating_add(me.random.next_int_bounded(damage / 2 + 2));
    }
    let owner = arrow.owner.and_then(|uid| world.level().entity_public(uid));
    let owner_player = arrow.owner.and_then(|uid| {
        let mut owner = None;
        world.level().for_each_player(&mut |p| {
            if p.uid == uid {
                owner = Some(*p);
            }
        });
        owner
    });
    let cause = owner
        .as_ref()
        .map(|p| p.uid)
        .or(owner_player.map(|p| p.uid));
    let source = DamageSource {
        attacker: cause.or(Some(me.id)),
        direct: Some(me.id),
        attacker_is_player: owner_player.is_some(),
        creative_player: owner_player.is_some_and(|p| p.creative),
        position: Some(me.position),
        projectile_motion: Some(me.delta),
        arrow_ignite_ticks: if me.remaining_fire_ticks > 0 { 100 } else { 0 },
        attacker_uuid: arrow.owner_uuid,
        scale_when_non_player: owner
            .as_ref()
            .is_some_and(|p| matches!(p.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::Arrow)
    };
    let accepted = if hit.player {
        world.hurt_player_from_mob(hit.uid, source, damage as f32, owner.as_ref())
    } else {
        hurt_mob(world, hit.uid, source, damage as f32)
    };
    if accepted {
        play_hit_sound(me, world);
        // 官方 `LivingEntity` 目标上的 `doKnockback` 和 `doPostHurtEffects` 都在
        // 受伤成功之后；玩家目标只做药水效果（玩家击退不走实体推力）。
        //
        // Vanilla runs `doKnockback` and `doPostHurtEffects` on a living
        // target after a successful hurt. Player targets only get the potion
        // effects; player knockback does not go through an entity push.
        if !hit.player {
            apply_punch_knockback(me, world, hit.uid);
        }
        apply_post_hurt_effects(me, world, hit.uid, hit.player);
        if arrow_pierces(me) {
            return true;
        }
        me.removed = true;
    } else {
        me.delta = me.delta.scale(-0.2 * 0.5);
        me.y_rot += 170.0 + me.random.next_float() * 20.0;
        if me.delta.length_sqr() < 1.0e-7 {
            if let EntityKind::Arrow(arrow) = &me.kind
                && arrow.pickup == Pickup::Allowed
            {
                world.spawn_at_location(
                    me.position.add(Vec3::new(0.0, f64::from(0.1_f32), 0.0)),
                    arrow.pickup_item.clone(),
                );
            }
            me.removed = true;
        }
    }
    // 拒伤造成的反弹发生在 onHitEntity 内，不是目标主动偏转。
    // Reversal after rejected damage happens within onHitEntity, not as a target deflection.
    arrow_pierces(me) && !me.removed
}

/// `AbstractArrow.doKnockback`：冲击附魔在受伤成功后再推一次。
///
/// `AbstractArrow.doKnockback`: the Punch enchantment pushes again after a
/// successful hurt.
fn apply_punch_knockback(me: &Entity, world: &mut World<'_>, target: EntityUid) {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return;
    };
    let Some(weapon) = &arrow.weapon else {
        return;
    };
    let level = enchantment_level(weapon, "minecraft:punch");
    if level <= 0 {
        return;
    }
    let horizontal = Vec3::new(me.delta.x, 0.0, me.delta.z);
    if horizontal.length_sqr() == 0.0 {
        return;
    }
    let direction = horizontal.normalize();
    let knockback = f64::from(level as f32);
    world.with_entity_mut(target, &mut |entity| {
        let Some(mob) = entity.kind.mob() else {
            return;
        };
        let resistance = mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::KNOCKBACK_RESISTANCE)
            .max(0.0);
        let scale = knockback * 0.6 * (1.0 - resistance).max(0.0);
        let movement = direction.scale(scale);
        if movement.length_sqr() > 0.0 {
            // `Entity.push`：直接加到速度上。
            //
            // `Entity.push`: added straight to the velocity.
            entity.delta = entity.delta.add(Vec3::new(movement.x, 0.1, movement.z));
        }
    });
}

/// `Arrow.doPostHurtEffects` 和 `SpectralArrow.doPostHurtEffects`。
///
/// Vanilla's `Arrow.doPostHurtEffects` and `SpectralArrow.doPostHurtEffects`.
fn apply_post_hurt_effects(me: &Entity, world: &mut World<'_>, target: EntityUid, player: bool) {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return;
    };
    let effects = match arrow.variant {
        ArrowVariant::Spectral { duration } => {
            vec![crate::entity::mob::effects::EffectInstance::new(
                mc_bamboo_data::registries::mob_effect::GLOWING,
                duration,
            )]
        }
        ArrowVariant::Normal => potion_effect_instances(arrow),
    };
    for effect in effects {
        if crate::entity::mob::effects::is_instantaneous(effect.effect) {
            // 官方 `LivingEntity.addEffect` 对瞬间效果当场结算。
            //
            // Vanilla `LivingEntity.addEffect` settles instantaneous effects at once.
            if player {
                world.instant_player_effect(target, effect.effect, effect.amplifier);
            } else {
                world.apply_mob_instant_effect(target, effect.effect, effect.amplifier);
            }
        } else if player {
            world.give_player_effect(target, effect);
        } else {
            world.add_mob_effect(target, effect);
        }
    }
}

/// `PotionContents.forEachEffect`：先基础药水，再自定义效果，时长按组件缩放。
///
/// `PotionContents.forEachEffect`: the base potion first, then the custom
/// effects, with durations scaled by the component.
fn potion_effect_instances(arrow: &Arrow) -> Vec<crate::entity::mob::effects::EffectInstance> {
    use crate::entity::mob::effects::EffectInstance;
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = arrow.pickup_item.patch();
    let Some(ComponentValue::PotionContents {
        potion,
        custom_effects,
        ..
    }) = defaults::resolve(arrow.pickup_item.item, &patch, kinds::POTION_CONTENTS)
    else {
        return Vec::new();
    };
    let scale =
        match defaults::resolve(arrow.pickup_item.item, &patch, kinds::POTION_DURATION_SCALE) {
            Some(ComponentValue::PotionDurationScale(scale)) => *scale,
            _ => 1.0,
        };
    let mut out = Vec::new();
    if let Some(potion) = potion
        && let Some(effects) = mc_bamboo_data::potions::effects(*potion)
    {
        for &(effect, duration, amplifier) in effects {
            out.push(EffectInstance::with_amplifier(
                effect,
                scaled_duration(duration, scale),
                amplifier,
            ));
        }
    }
    for instance in custom_effects {
        let details = &instance.details;
        let mut effect = EffectInstance::with_flags(
            instance.effect,
            scaled_duration(details.duration, scale),
            details.amplifier,
            details.ambient,
            details.show_particles,
        );
        effect.show_icon = details.show_icon;
        out.push(effect);
    }
    out
}

/// `MobEffectInstance.withScaledDuration`：无限时长和 0 不缩放，其余至少 1 刻。
///
/// `MobEffectInstance.withScaledDuration`: infinite and zero durations stay,
/// everything else gets at least one tick.
fn scaled_duration(duration: i32, scale: f32) -> i32 {
    if duration <= 0 {
        duration
    } else {
        (((duration as f32) * scale) as i32).max(1)
    }
}

fn impact_damage(arrow: &Arrow) -> f64 {
    let Some(weapon) = &arrow.weapon else {
        return arrow.base_damage;
    };
    let mut damage = arrow.base_damage as f32;
    let level = enchantment_level(weapon, "minecraft:power");
    if level > 0 {
        damage += 1.0 + (level - 1) as f32 * 0.5;
    }
    f64::from(damage)
}

/// 武器上某个附魔的等级；没有 `enchantments` 组件时是 0。
///
/// One enchantment's level on a stack; 0 without an `enchantments` component.
pub fn enchantment_level(stack: &ItemStack, enchantment: &str) -> i32 {
    use mc_bamboo_data::registries::{SYNCED_REGISTRIES, data_component_type::ENCHANTMENTS};
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::Enchantments(levels)) =
        defaults::resolve(stack.item, &patch, ENCHANTMENTS)
    else {
        return 0;
    };
    let Some(id) = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
        .and_then(|registry| {
            registry
                .entries
                .iter()
                .position(|name| *name == enchantment)
        })
    else {
        return 0;
    };
    levels
        .iter()
        .find_map(|&(key, level)| (key == id as i32).then_some(level))
        .unwrap_or(0)
}

fn hurt_mob(world: &mut World<'_>, target: EntityUid, source: DamageSource, damage: f32) -> bool {
    let burning = source.arrow_ignite_ticks > 0;
    let mut previous_fire = 0;
    if burning {
        world.with_entity_mut(target, &mut |entity| {
            previous_fire = entity.remaining_fire_ticks;
            entity.remaining_fire_ticks = previous_fire.max(source.arrow_ignite_ticks);
            if let Some(mob) = entity.kind.mob_mut() {
                mob.ticks_frozen = 0;
            }
        });
    }
    let accepted = world.hurt_mob(target, source, damage);
    if burning && !accepted {
        world.with_entity_mut(target, &mut |entity| {
            entity.remaining_fire_ticks = previous_fire
        });
    }
    accepted
}

fn arrow_pierces(me: &Entity) -> bool {
    matches!(&me.kind, EntityKind::Arrow(arrow) if arrow.pierce > 0)
}

fn hit_block(me: &mut Entity, world: &mut World<'_>, hit: BlockHit) {
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return;
    };
    arrow.last_state = Some(world.block_state(hit.pos));
    let sign = |x: f64| if x == 0.0 { x } else { x.signum() };
    let offset = Vec3::new(sign(me.delta.x), sign(me.delta.y), sign(me.delta.z));
    me.position = me.position.subtract(offset.scale(f64::from(0.05_f32)));
    me.delta = Vec3::default();
    arrow.in_ground = true;
    arrow.shake_time = 7;
    arrow.critical = false;
    arrow.pierce = 0;
    arrow.pierced.clear();
    play_hit_sound(me, world);
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.sound = arrow_hit_sound();
    }
}

fn play_hit_sound(me: &mut Entity, world: &mut World<'_>) {
    if let EntityKind::Arrow(arrow) = &me.kind {
        let pitch = 1.2 / (me.random.next_float() * 0.2 + 0.9);
        world.play_sound(me.position, arrow.sound, sounds::NEUTRAL, 1.0, pitch, false);
    }
}

fn finish_flight(me: &mut Entity, blocks: &dyn BlockView) {
    if !me.in_water {
        me.delta = me.delta.scale(f64::from(0.99_f32));
    }
    if matches!(&me.kind, EntityKind::Arrow(arrow) if !arrow.in_ground && !arrow.no_gravity) {
        // Entity.applyGravity 把整支向量加上 (0, -g, 0)，也会规范化反弹留下的 -0。
        // Entity.applyGravity adds (0, -g, 0) to the whole vector, normalizing -0 after a reversal.
        me.delta = me.delta.add(Vec3::new(0.0, -0.05, 0.0));
    }
    let in_lava = me.update_projectile_fluid(blocks);
    // Entity.baseTick 的 on_fire 伤害会让 Projectile.markHurt，即使没有损血。
    // Entity.baseTick's on-fire pulse calls Projectile.markHurt even without health damage.
    let sync_velocity =
        me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 && !in_lava;
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.has_been_shot = true;
        arrow.sync_velocity = sync_velocity;
        // `Projectile.tick` 在每刻末尾把 `leftOwnerChecked` 清回 false，
        // 所以下一飞行的刻还会再判一次，直到真的离开主人。
        // `Projectile.tick` clears `leftOwnerChecked` at the end of each tick,
        // so the next flying tick checks again until the arrow really left its owner.
        arrow.left_owner_checked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_crossing_water_extinguishes_before_ending_in_dry_air() {
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(2, 65, 0),
            BlockState(mc_bamboo_data::blocks::WATER),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.25, 0.5));
        arrow.delta = Vec3::new(5.0, 0.0, 0.0);
        arrow.remaining_fire_ticks = 200;
        let id = world.spawn_entity(arrow);
        let mut expected = world.entity(id).unwrap().random;
        expected.next_float();
        expected.next_float();
        world.tick_entities();
        let arrow = world.entity(id).unwrap();
        assert_eq!(arrow.position, Vec3::new(5.5, 65.25, 0.5));
        assert_eq!(arrow.remaining_fire_ticks, 0);
        assert_eq!(arrow.random.state(), expected.state());
        assert!(!arrow.in_water);
    }

    #[test]
    fn piercing_rescans_and_applies_blocks_in_the_remaining_segment() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(5, 65, 0),
            BlockState(mc_bamboo_data::blocks::COBWEB),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let targets = [2.0, 4.0].map(|x| {
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5)))
        });
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.pierce = 1;
        data.base_damage = 0.5;
        arrow.delta = Vec3::new(6.0, 0.0, 0.0);
        let end = arrow.position.add(arrow.delta);
        // 首批候选只给第一头牛；后续扫描应发现第二头，并穿过其后的蛛网。
        // Supply only the first cow in the initial batch. The rescan should find
        // the second one and apply the web behind it.
        impact(
            &mut arrow,
            &mut world,
            vec![ArrowEntityHit {
                uid: targets[0],
                contact: Vec3::new(1.7, 65.0, 0.5),
                player: false,
            }],
            None,
            end,
        );
        assert_eq!(
            targets.map(|id| world.entity(id).unwrap().kind.mob().unwrap().health),
            [7.0, 7.0]
        );
        assert_eq!(arrow.position, end);
        assert_eq!(arrow.stuck_speed_multiplier.x, 0.25);
    }

    #[test]
    fn burning_arrow_forces_velocity_sync_on_the_fire_damage_pulse() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        arrow.remaining_fire_ticks = 20;
        let id = world.spawn_entity(arrow);
        for expected in [true, false] {
            world.tick_entities();
            let entity = world.entity(id).unwrap();
            assert!(
                matches!(&entity.kind, EntityKind::Arrow(data) if data.sync_velocity == expected)
            );
        }
    }

    #[test]
    fn piercing_arrow_keeps_hitting_after_rejection_until_it_reaches_the_limit() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(data) = &mut arrow.kind {
            data.pierce = 1;
            data.base_damage = 0.5;
        }
        arrow.shoot_arrow(Vec3::new(1.0, 0.0, 0.0), 6.0, 0.0);
        let projectile = world.spawn_entity(arrow);
        let targets = [2.0, 3.5, 5.5].map(|x| {
            let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5));
            target.with_mob(|_, mob| {
                mob.no_ai = true;
                mob.invulnerable = x == 2.0;
            });
            world.spawn_entity(target)
        });
        world.tick_entities();
        let health = targets.map(|id| world.entity(id).unwrap().kind.mob().unwrap().health);
        assert_eq!(health, [10.0, 9.0, 10.0]);
        assert!(world.entity(projectile).is_none());
    }

    #[test]
    fn no_gravity_keeps_air_drag_and_does_not_add_vertical_speed() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.no_gravity = true;
        arrow.delta = Vec3::new(1.0, 0.0, 0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(arrow);
        world.tick_entities();
        let arrow = world.entity(uid).unwrap();
        assert_eq!(arrow.position, Vec3::new(1.5, 64.5, 0.5));
        assert_eq!(arrow.delta, Vec3::new(f64::from(0.99_f32), 0.0, 0.0));
    }

    #[test]
    fn embedded_arrow_is_removed_at_age_1200() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let pos = BlockPos::new(0, 64, 0);
        let state = BlockState(mc_bamboo_data::blocks::STONE);
        level.set(pos, state);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.in_ground = true;
        data.last_state = Some(state);
        data.life = 1198;
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(arrow);
        world.tick_entities();
        assert!(
            matches!(&world.entity(uid).unwrap().kind, EntityKind::Arrow(data) if data.life == 1199)
        );
        world.tick_entities();
        assert!(world.entity(uid).is_none());
    }

    #[test]
    fn spectral_arrows_glow_the_target() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo: ItemStack::new(item::SPECTRAL_ARROW, 1),
            weapon: None,
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        let mob = entity.kind.mob().unwrap();
        assert!(
            mob.effects
                .has(mc_bamboo_data::registries::mob_effect::GLOWING)
        );
        assert_eq!(
            mob.effects
                .get(mc_bamboo_data::registries::mob_effect::GLOWING)
                .map(|effect| effect.duration),
            Some(200)
        );
    }

    #[test]
    fn a_tipped_arrow_applies_its_potion_effects() {
        use crate::entity::mob::MobType;
        use mc_bamboo_data::registries::{data_component_type as kinds, mob_effect};
        use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let mut ammo = ItemStack::new(item::TIPPED_ARROW, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::POTION_CONTENTS,
            PatchEntry::Set(ComponentValue::PotionContents {
                potion: Some(mc_bamboo_data::registries::potion::POISON),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            }),
        );
        let (added, removed) = patch.to_opaque();
        ammo.added = added;
        ammo.removed = removed;
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo,
            weapon: None,
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        let mob = entity.kind.mob().unwrap();
        assert!(mob.effects.has(mob_effect::POISON));
        assert_eq!(
            mob.effects
                .get(mob_effect::POISON)
                .map(|effect| effect.duration),
            Some(112),
            "药水箭的 `potion_duration_scale` 是 0.125，45 秒的毒变成 112 刻"
        );
    }

    #[test]
    fn punch_pushes_the_target_along_the_arrow() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let mut weapon = ItemStack::new(item::BOW, 1);
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            mc_bamboo_protocol::components::PatchEntry::Set(
                mc_bamboo_protocol::components::ComponentValue::Enchantments(vec![(
                    enchantment_id("minecraft:punch"),
                    1,
                )]),
            ),
        );
        let (added, removed) = patch.to_opaque();
        weapon.added = added;
        weapon.removed = removed;
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo: ItemStack::new(item::ARROW, 1),
            weapon: Some(weapon),
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        assert!(
            entity.delta.x > 0.0,
            "冲击附魔把目标沿箭的方向推出去，got {:?}",
            entity.delta
        );
    }

    fn enchantment_id(name: &str) -> i32 {
        mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
            .expect("enchantment is registered") as i32
    }

    #[test]
    fn entity_clip_excludes_inside_starts_and_endpoint_contacts() {
        let bb = Aabb::new(0., 0., 0., 1., 1., 1.);
        let start = Vec3::new(-1., 0.5, 0.5);
        let face = Vec3::new(0., 0.5, 0.5);
        assert_eq!(clip_entity(bb, start, Vec3::new(2., 0.5, 0.5)), Some(face));
        assert_eq!(clip_entity(bb, start, face), None);
        assert_eq!(clip_entity(bb, bb.center(), start), None);
    }

    #[test]
    fn arrow_embeds_and_detaches_after_its_block_is_removed() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let block = BlockPos::new(2, 64, 0);
        level.set(block, BlockState(mc_bamboo_data::blocks::STONE));
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        arrow.delta = Vec3::new(3.0, 0.0, 0.0);
        let uid;
        {
            let mut world = World::new(&mut level, &mut ticks);
            uid = world.spawn_entity(arrow);
            world.tick_entities();
            let arrow = world.entity(uid).unwrap();
            assert_eq!(arrow.position.x, 2.0 - f64::from(0.05_f32));
            assert_eq!(arrow.delta, Vec3::default());
            assert!(
                matches!(&arrow.kind, EntityKind::Arrow(a) if a.in_ground && a.shake_time == 7)
            );

            let row = arrow.split();
            let tag = crate::archive::entity::write_entity(&row).unwrap();
            let restored = crate::archive::entity::read_entity(&tag, uid)
                .unwrap()
                .unwrap();
            let KindPublic::Arrow(mut original) = row.public.kind else {
                panic!("expected arrow")
            };
            original.needs_sync = false;
            assert_eq!(KindPublic::Arrow(original), restored.public.kind);
            let super::super::KindState::Arrow(mut saved) = row.state.kind else {
                panic!("expected arrow state")
            };
            saved.needs_sync = false;
            // `leftOwnerChecked` 不写进存档，读回来从 false 开始。
            // `leftOwnerChecked` is not saved, so a reloaded arrow starts from false.
            saved.left_owner_checked = false;
            assert_eq!(super::super::KindState::Arrow(saved), restored.state.kind);
        }
        level.set(block, BlockState(mc_bamboo_data::blocks::AIR));
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_entities();
        let detached = world.entity(uid).unwrap();
        assert!(matches!(&detached.kind, EntityKind::Arrow(a) if !a.in_ground && a.life == 0));
        world.tick_entities();
        assert_eq!(world.entity(uid).unwrap().delta.y, -0.05);
    }

    #[test]
    fn a_block_placed_around_an_arrow_only_starts_shaking_once() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(Entity::arrow(Vec3::new(0.5, 64.5, 0.5)));
        world.tick_entities();
        world.set_block(
            BlockPos::new(0, 64, 0),
            BlockState(mc_bamboo_data::blocks::STONE),
            2,
        );
        for expected in [6, 5, 4, 3, 2, 1, 0, 0, 0] {
            world.tick_entities();
            let arrow = world.entity(uid).unwrap();
            assert!(
                matches!(&arrow.kind, EntityKind::Arrow(a) if a.in_ground && a.shake_time == expected)
            );
        }
    }

    #[test]
    fn arrow_shoot_uses_xyz_triangular_spread() {
        let mut arrow = Entity::arrow(Vec3::default());
        let mut random = arrow.random;
        let spread = 0.0172275 * 6.0;
        let expected = Vec3::new(
            random.triangle(0., spread),
            random.triangle(0., spread),
            1. + random.triangle(0., spread),
        )
        .scale(f64::from(1.6_f32));
        arrow.shoot_arrow(Vec3::new(0., 0., 2.), 1.6, 6.);
        assert_eq!(arrow.delta, expected);
        assert_eq!(arrow.random, random);
    }

    #[test]
    fn left_owner_is_rechecked_on_every_flying_tick() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let owner =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(0.5, 64.0, 0.5)));
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(data) = &mut arrow.kind {
            data.owner = Some(owner);
        }
        arrow.delta = Vec3::new(5.0, 0.0, 0.0);
        let uid = world.spawn_entity(arrow);
        let left_owner = |world: &World<'_>| {
            let entity = world.entity(uid).unwrap();
            let EntityKind::Arrow(data) = &entity.kind else {
                panic!("expected arrow")
            };
            data.left_owner
        };
        world.tick_entities();
        assert!(!left_owner(&world));
        world.tick_entities();
        assert!(left_owner(&world));
    }
}
