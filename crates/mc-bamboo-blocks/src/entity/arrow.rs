use mc_bamboo_core::{Aabb, Axis, Vec3, mth};
use mc_bamboo_data::{items as item, sounds};
use mc_bamboo_protocol::item::ItemStack;

use super::ctx::{BlockView, Command, EntityCtx, PlayerView};
use super::mob::{DamageKind, DamageSource};
use super::{Entity, EntityKind, EntityUid, KindPublic};
use crate::{BlockHit, BlockState, World};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pickup {
    #[default]
    Disallowed,
    Allowed,
    CreativeOnly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowPublic {
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub critical: bool,
    pub pierce: i8,
    pub in_ground: bool,
    pub shake_time: u8,
    pub pickup: Pickup,
    pub on_fire: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub pitch: f32,
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
    pub has_been_shot: bool,
    pierced: Vec<EntityUid>,
}

impl Default for Arrow {
    fn default() -> Self {
        Self {
            owner: None,
            owner_uuid: None,
            pitch: 0.0,
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
            has_been_shot: false,
            pierced: Vec::new(),
        }
    }
}

impl Arrow {
    pub(crate) fn public(&self, remaining_fire_ticks: i32) -> ArrowPublic {
        ArrowPublic {
            owner: self.owner,
            pitch: self.pitch,
            critical: self.critical,
            pierce: self.pierce,
            in_ground: self.in_ground,
            shake_time: self.shake_time,
            pickup: self.pickup,
            on_fire: remaining_fire_ticks > 0,
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

    /// Projectile.shoot：先归一化，再按 XYZ 顺序抽散布，最后乘速度。
    ///
    /// Projectile.shoot normalizes first, draws spread in XYZ order, then scales.
    pub fn shoot_arrow(&mut self, direction: Vec3, power: f32, uncertainty: f32) {
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
        let (yaw, pitch) = rotation(self.delta);
        self.y_rot = yaw;
        arrow.pitch = pitch;
        arrow.life = 0;
        self.requires_precise_position = true;
    }
}

fn rotation(movement: Vec3) -> (f32, f32) {
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let deg = |r| (r * 180.0 / f64::from(std::f32::consts::PI)) as f32;
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
    let position = me.position;
    let block_pos = me.block_position();
    let blocks = ctx.blocks();
    let current_state = blocks.block_state(block_pos);
    let collision = me.collision_context();
    let surrounding = at_point(position, 0.06);
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return;
    };
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
        arrow.in_ground = true;
        if !me.first_tick && arrow.shake_time == 0 {
            arrow.shake_time = 7;
        }
    }
    arrow.shake_time = arrow.shake_time.saturating_sub(1);
    if me.in_water || blocks.is_raining_at(block_pos) {
        me.remaining_fire_ticks = 0;
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
        me.apply_effects_from_blocks(blocks);
        return;
    }
    if me.in_water {
        me.delta = me.delta.scale(f64::from(0.6_f32));
    }
    let (yaw, pitch) = rotation(movement);
    me.y_rot = lerp_rotation(me.y_rot, yaw);
    arrow.pitch = lerp_rotation(arrow.pitch, pitch);
    let owner = arrow
        .owner
        .or_else(|| arrow.owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)));
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let mut owner_in_range = owner
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|entity| entity.bounding_box().intersects(area));
    let mut players = Vec::new();
    blocks.for_each_player(&mut |p| {
        if p.alive && !p.spectator && player_box(p).intersects(area) {
            owner_in_range |= owner == Some(p.uid);
            players.push(*p);
        }
    });
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        unreachable!()
    };
    arrow.owner = owner;
    arrow.left_owner |= !owner_in_range;
    let end = position.add(movement);
    let block_hit = blocks.clip_collider(position, end, collision);
    let end = block_hit.map_or(end, |hit| hit.location);
    let mut hits = Vec::new();
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let mut consider = |uid, feet, bb: Aabb, player| {
        if (arrow.left_owner || Some(uid) != owner) && !arrow.pierced.contains(&uid) {
            let contact = clip_entity(bb, position, end).or_else(|| {
                if margin == 0.0 {
                    return None;
                }
                let outside = clip_entity(bb.inflated(margin), position, end)?;
                let center = bb.center();
                let inside = blocks
                    .clip_collider(outside, center, collision)
                    .map_or(center, |hit| hit.location);
                clip_entity(bb, outside, inside).map(|_| outside)
            });
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
    for player in players {
        consider(player.uid, player.position, player_box(&player), true);
    }
    if hits.is_empty() && block_hit.is_none() {
        me.position = end;
        me.apply_effects_from_blocks(blocks);
        finish_flight(me, blocks);
    } else {
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        ctx.emit(
            Command::ArrowImpact {
                block: block_hit,
                hits: hits.into_iter().map(|(_, hit)| hit).collect(),
                end,
            },
            me,
        );
    }
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
    hits: Vec<ArrowEntityHit>,
    block: Option<BlockHit>,
    end: Vec3,
) {
    me.requires_precise_position = true;
    let first = hits.first();
    me.position = first.map_or(end, |hit| hit.contact);
    me.apply_effects_from_blocks(world);
    if !me.removed {
        for hit in hits {
            if !hit_entity(me, world, hit) {
                finish_flight(me, world);
                return;
            }
        }
        me.position = end;
        if let Some(hit) = block {
            hit_block(me, world, hit);
        }
    }
    finish_flight(me, world);
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
    let mut damage = (f64::from(speed) * arrow.base_damage)
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
        attacker_uuid: arrow.owner_uuid,
        scale_when_non_player: owner
            .as_ref()
            .is_some_and(|p| matches!(p.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::Arrow)
    };
    let accepted = if hit.player {
        world.hurt_player_from_mob(hit.uid, source, damage as f32, owner.as_ref())
    } else {
        world.hurt_mob(hit.uid, source, damage as f32)
    };
    if accepted {
        play_hit_sound(me, world);
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
    false
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
        world.play_sound(me.position, arrow.sound, sounds::MASTER, 1.0, pitch, false);
    }
}

fn finish_flight(me: &mut Entity, blocks: &dyn BlockView) {
    if !me.in_water {
        me.delta = me.delta.scale(f64::from(0.99_f32));
    }
    if matches!(&me.kind, EntityKind::Arrow(arrow) if !arrow.in_ground) {
        me.delta.y -= 0.05;
    }
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    me.in_water = blocks.fluid_state(me.block_position()).is_water();
    me.first_tick = false;
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.has_been_shot = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::BlockPos;

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
            assert_eq!(row.public.kind, restored.public.kind);
            assert_eq!(row.state.kind, restored.state.kind);
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
}
