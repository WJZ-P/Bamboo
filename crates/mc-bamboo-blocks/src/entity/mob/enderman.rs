/*! 末影人的传送、注视目标和专用存档状态。

Enderman teleportation, gaze targeting and species-specific save data. */

use mc_bamboo_core::{Aabb, Axis, BlockPos, Vec3};
use mc_bamboo_data::{
    entity_types,
    registries::attribute,
    tags::{block_tags, fluid_tags},
};
use mc_bamboo_nbt::Tag;

use crate::archive::tag;
use crate::entity::ctx::{BlockView, EntityCtx, PlayerView};
use crate::entity::{Entity, EntityUid, KindPublic};
use crate::shape::CollisionContext;
use crate::state::BlockState;

use super::attributes::{Attributes, Modifier, Operation};
use super::goals::{Goal, GoalSelector, WrappedGoal, player_by_uid, reduced_tick_delay};
use super::navigation::Navigation;
use super::{DamageKind, DamageSource, Mob, MobEvent, MobType, living};

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct EndermanState {
    pub carried: Option<BlockState>,
    pub creepy: bool,
    pub stared_at: bool,
    target_change_time: i32,
    anger_end_time: i64,
    angry_at: Option<[u8; 16]>,
    pending_block: Option<(BlockPos, BlockPos, bool)>,
}

pub(super) const fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Enderman.entity_type() as usize];
    (facts.width, facts.height, entity_types::ENDERMAN_EYE_HEIGHT)
}

pub(super) fn malus() -> super::path::Malus {
    let mut malus = super::path::Malus::monster();
    malus.set(mc_bamboo_data::PathType::Water, -1.0);
    malus
}

pub(super) fn attributes() -> Attributes {
    let mut attrs = Attributes::skeleton();
    attrs.set_base(attribute::MAX_HEALTH, 40.0);
    attrs.set_base(attribute::ATTACK_DAMAGE, 7.0);
    attrs.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    attrs.set_base(attribute::FOLLOW_RANGE, 64.0);
    attrs.set_base(attribute::STEP_HEIGHT, 1.0);
    attrs
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        goal,
        running: false,
    }
}

pub(super) fn goals(nav: &mut Navigation) -> GoalSelector {
    nav.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(0, Goal::Float),
        wrap(1, Goal::EndermanFreeze),
        wrap(2, Goal::melee(1.0, false)),
        wrap(
            7,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.0,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            8,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
        wrap(10, Goal::EndermanBlock { place: true }),
        wrap(11, Goal::EndermanBlock { place: false }),
    ])
}

pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(1, Goal::EndermanGaze(GazeGoal::default())),
        wrap(2, Goal::HurtByTarget { timestamp: -1 }),
        // 末影螨还不存在，但这个目标仍注册、仍按 `reducedTickDelay(10)` 取数。
        // Endermites are not modelled, but the goal still registers and rolls its interval.
        wrap(
            3,
            Goal::NearestAttackableTarget {
                kind: super::goals::TargetType::Endermite,
                interval: reduced_tick_delay(10),
                unseen_ticks: 0,
                found: None,
                light_gate: false,
            },
        ),
        wrap(4, Goal::Unavailable("ResetUniversalAngerTargetGoal")),
    ])
}

pub(super) fn set_target(me: &Entity, mob: &mut Mob, target: Option<EntityUid>) {
    mob.target = target;
    if mob.mob_type != MobType::Enderman {
        return;
    }
    const ATTACKING: &str = "minecraft:attacking";
    let speed = mob.attributes.instance(attribute::MOVEMENT_SPEED).unwrap();
    mob.enderman.creepy = target.is_some();
    if target.is_some() {
        mob.enderman.target_change_time = me.tick_count;
        if speed.modifier(ATTACKING).is_none() {
            speed.modifiers.push(Modifier {
                id: ATTACKING.to_owned(),
                amount: f64::from(0.15_f32),
                operation: Operation::AddValue,
                permanent: false,
            });
        }
    } else {
        mob.enderman.stared_at = false;
        mob.enderman.target_change_time = 0;
        speed.remove_modifier(ATTACKING);
    }
}

/// `NeutralMob.updatePersistentAnger(level, true)`。
///
/// `NeutralMob.updatePersistentAnger(level, true)`.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    mob.jumping = false;
    let view = ctx.blocks();
    let now = view.game_time();
    let target = mob
        .target
        .filter(|_| super::goals::combat_target(mob, ctx).is_some());
    let angry = mob.enderman.angry_at;
    // 记着的目标已经死掉、而且是个生物：立刻消气。
    // The remembered target died and is a mob: stop being angry at once.
    let angry_mob_died = angry.is_some_and(|uuid| {
        target.is_some_and(|uid| {
            ctx.other(uid).is_some_and(|other| {
                matches!(other.kind, KindPublic::Mob(mob) if !mob.is_alive() && mob.uuid == uuid)
            })
        })
    });
    if angry_mob_died {
        stop_being_angry(me, mob);
        return;
    }
    if let Some(uid) = target {
        let uuid = player_by_uid(view, uid)
            .and_then(|player| player.uuid)
            .or_else(|| match ctx.other(uid)?.kind {
                KindPublic::Mob(mob) => Some(mob.uuid),
                _ => None,
            });
        if angry != uuid {
            mob.enderman.angry_at = uuid;
        }
        // `stayAngryIfTargetPresent`：有目标时每刻都重抽一次怒气计时。
        // `stayAngryIfTargetPresent`: the anger timer is resampled every tick while a target is present.
        mob.enderman.anger_end_time = now + i64::from(400 + me.random.next_int_bounded(381));
        return;
    }
    let expired = now >= mob.enderman.anger_end_time;
    let target_is_mob = mob
        .target
        .is_some_and(|uid| player_by_uid(view, uid).is_none());
    if angry.is_some() && expired && target_is_mob {
        stop_being_angry(me, mob);
        return;
    }
    // 记恨的玩家转成创造 / 旁观，或者难度降到和平：同样消气。
    // The angry player turned creative or spectator, or the difficulty dropped to peaceful.
    if let Some(uuid) = angry {
        let mut invalid = false;
        view.for_each_player(&mut |player| {
            invalid |= player.uuid == Some(uuid)
                && (player.creative || player.spectator || view.difficulty() == 0);
        });
        if invalid {
            stop_being_angry(me, mob);
        }
    }
}

fn stop_being_angry(me: &mut Entity, mob: &mut Mob) {
    mob.enderman.angry_at = None;
    mob.enderman.anger_end_time = -1;
    mob.last_hurt_by_mob = None;
    mob.last_hurt_by_mob_ref = None;
    set_target(me, mob, None);
}

pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    if view.is_bright_outside() && me.tick_count >= mob.enderman.target_change_time + 600 {
        let eye = BlockPos::containing(me.position.add(Vec3::new(
            0.0,
            f64::from(mob.eye_height),
            0.0,
        )));
        let raw = view.max_local_raw_brightness(eye, view.sky_darken()) as f32 / 15.0;
        let brightness = raw / (4.0 - 3.0 * raw);
        if brightness > 0.5
            && view.can_see_sky(me.block_position())
            && me.random.next_float() * 30.0 < (brightness - 0.4) * 2.0
        {
            set_target(me, mob, None);
            teleport(me, mob, ctx);
        }
    }
}

fn is_staring(me: &Entity, mob: &Mob, player: PlayerView, view: &dyn BlockView) -> bool {
    if player.gaze_disguise {
        return false;
    }
    let from = player.eye_position();
    let to = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let offset = to.subtract(from);
    let length = offset.length();
    let direction = offset.normalize();
    let look = player.look_direction.normalize();
    look.x * direction.x + look.y * direction.y + look.z * direction.z > 1.0 - 0.025 / length
        && length <= 128.0
        && view
            .clip_collider(from, to, CollisionContext::Empty)
            .is_none()
}

pub(super) fn freeze_target(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> Option<PlayerView> {
    let player = super::goals::combat_target(mob, ctx)
        .and_then(|target| player_by_uid(ctx.blocks(), target.uid))?;
    (player.position.distance_to_sqr(me.position) <= 256.0
        && is_staring(me, mob, player, ctx.blocks()))
    .then_some(player)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GazeGoal {
    pending: Option<EntityUid>,
    active: Option<EntityUid>,
    aggro_time: i32,
    teleport_time: i32,
}

impl GazeGoal {
    fn anger_inducing(me: &Entity, mob: &Mob, player: PlayerView, view: &dyn BlockView) -> bool {
        is_staring(me, mob, player, view)
            || (player.uuid.is_some() && player.uuid == mob.enderman.angry_at)
    }

    pub(super) fn can_use(&mut self, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        let view = ctx.blocks();
        let mut closest = f64::INFINITY;
        self.pending = None;
        view.for_each_player(&mut |player| {
            let distance = player.position.distance_to_sqr(me.position);
            if distance < closest
                && Self::anger_inducing(me, mob, *player, view)
                && super::goals::can_target_player(me, mob, ctx, *player)
            {
                closest = distance;
                self.pending = Some(player.uid);
            }
        });
        self.pending.is_some()
    }

    pub(super) fn start(&mut self, mob: &mut Mob) {
        self.aggro_time = reduced_tick_delay(5);
        self.teleport_time = 0;
        mob.enderman.stared_at = true;
    }

    pub(super) fn can_continue(&self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        if let Some(uid) = self.pending {
            let Some(player) = player_by_uid(ctx.blocks(), uid) else {
                return false;
            };
            if !Self::anger_inducing(me, mob, player, ctx.blocks()) {
                return false;
            }
            look_at_limited(me, mob, player.eye_position());
            return true;
        }
        self.active
            .and_then(|uid| player_by_uid(ctx.blocks(), uid))
            .is_some_and(|player| {
                player.alive
                    && !player.creative
                    && !player.spectator
                    && ctx.blocks().difficulty() != 0
            })
    }

    pub(super) fn stop(&mut self, me: &Entity, mob: &mut Mob) {
        self.pending = None;
        self.active = None;
        set_target(me, mob, None);
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        if super::goals::combat_target(mob, ctx).is_none() {
            self.active = None;
        }
        if self.pending.is_some() {
            self.aggro_time -= 1;
            if self.aggro_time <= 0 {
                self.active = self.pending.take();
                set_target(me, mob, self.active);
            }
            return;
        }
        if let Some(player) = self.active.and_then(|uid| player_by_uid(ctx.blocks(), uid)) {
            let distance = player.position.distance_to_sqr(me.position);
            if is_staring(me, mob, player, ctx.blocks()) {
                if distance < 16.0 {
                    teleport(me, mob, ctx);
                }
                self.teleport_time = 0;
            } else if distance > 256.0 {
                let elapsed = self.teleport_time;
                self.teleport_time += 1;
                if elapsed >= reduced_tick_delay(30) && teleport_towards(me, mob, ctx, player) {
                    self.teleport_time = 0;
                }
            }
        }
    }
}

fn teleport(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    if !living::is_alive(me, mob) {
        return false;
    }
    let x = me.position.x + (me.random.next_double() - 0.5) * 64.0;
    let y = me.position.y + f64::from(me.random.next_int_bounded(64) - 32);
    let z = me.position.z + (me.random.next_double() - 0.5) * 64.0;
    try_teleport(me, mob, ctx, Vec3::new(x, y, z))
}

fn teleport_towards(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    player: PlayerView,
) -> bool {
    let direction = Vec3::new(me.position.x, me.y_at(0.5), me.position.z)
        .subtract(player.eye_position())
        .normalize();
    let x = me.position.x + (me.random.next_double() - 0.5) * 8.0 - direction.x * 16.0;
    let y = me.position.y + f64::from(me.random.next_int_bounded(16) - 8) - direction.y * 16.0;
    let z = me.position.z + (me.random.next_double() - 0.5) * 8.0 - direction.z * 16.0;
    try_teleport(me, mob, ctx, Vec3::new(x, y, z))
}

fn try_teleport(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx, wanted: Vec3) -> bool {
    let view = ctx.blocks();
    let clamped = view.world_border().clamp_vec3(wanted);
    let mut pos = BlockPos::containing(clamped);
    if !view.is_loaded(pos) {
        return false;
    }
    let mut y = wanted.y;
    while pos.y > view.min_y() {
        pos.y -= 1;
        let state = view.block_state(pos);
        if state.is_tagged(block_tags::ENTITIES_CAN_TELEPORT_TO) {
            let destination = Vec3::new(clamped.x, y, clamped.z);
            if state.is_tagged(block_tags::ENDERMAN_DOES_NOT_TELEPORT_TO)
                || !safe_destination(me, ctx, destination)
            {
                return false;
            }
            if !mob.silent {
                mob.events.push(MobEvent::Sound {
                    sound: "minecraft:entity.enderman.teleport",
                    volume: 1.0,
                    pitch: 1.0,
                    position: me.old_position,
                    hostile: true,
                    seed: None,
                });
            }
            me.position = destination;
            mob.navigation.stop();
            mob.hurt_marked = true;
            mob.events.push(MobEvent::EntityEvent { event: 46 });
            living::play_sound(me, mob, "minecraft:entity.enderman.teleport", 1.0, 1.0);
            return true;
        }
        y -= 1.0;
    }
    false
}

fn safe_destination(me: &Entity, ctx: &dyn EntityCtx, destination: Vec3) -> bool {
    let view = ctx.blocks();
    let bounds = me.make_bounding_box_at(destination);
    if !view
        .block_collisions(bounds, CollisionContext::Empty)
        .is_empty()
    {
        return false;
    }
    let cell = BlockPos::containing(destination);
    let below = BlockPos::new(cell.x, cell.y - 1, cell.z);
    if view.fluid_state(below).is_tagged(fluid_tags::WATER) {
        return false;
    }
    for y in bounds.min(Axis::Y).floor() as i32..bounds.max(Axis::Y).ceil() as i32 {
        for z in bounds.min(Axis::Z).floor() as i32..bounds.max(Axis::Z).ceil() as i32 {
            for x in bounds.min(Axis::X).floor() as i32..bounds.max(Axis::X).ceil() as i32 {
                let cell = BlockPos::new(x, y, z);
                if !view.fluid_state(cell).is_empty()
                    || view
                        .block_state(cell)
                        .is_tagged(block_tags::ENDERMAN_DOES_NOT_TELEPORT_TO)
                {
                    return false;
                }
            }
        }
    }
    true
}

pub(super) fn dodge_projectile(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    for _ in 0..64 {
        if teleport(me, mob, ctx) {
            break;
        }
    }
}

pub(super) fn hurt_if_wet(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `isInRain`：脚所在格，和包围盒顶上的那一格。
    // `isInRain`: the feet block and the block at the top of the bounding box.
    let pos = me.block_position();
    let top = BlockPos::new(pos.x, me.bounding_box().max(Axis::Y).floor() as i32, pos.z);
    if me.in_water || ctx.blocks().is_raining_at(pos) || ctx.blocks().is_raining_at(top) {
        living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Drown),
            1.0,
        );
    }
}

pub(super) fn after_hurt(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    source: DamageSource,
) {
    let from_living = source.attacker_is_player
        || source
            .attacker
            .and_then(|uid| ctx.other(uid))
            .is_some_and(|other| matches!(other.kind, KindPublic::Mob(_)));
    if !from_living && me.random.next_int_bounded(10) != 0 {
        teleport(me, mob, ctx);
    }
}

pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    let state = &mob.enderman;
    if let Some(block) = state.carried {
        tag::put(out, "carriedBlockState", tag::encode_block_state(block));
    }
    tag::put(out, "anger_end_time", Tag::Long(state.anger_end_time));
    if let Some(uuid) = state.angry_at {
        tag::put(out, "angry_at", super::nbt::uuid_tag(&uuid));
    }
}

pub(super) fn read_nbt(mob: &mut Mob, input: &Tag) {
    mob.enderman.carried = tag::field(input, "carriedBlockState")
        .and_then(|value| tag::decode_block_state(value).ok())
        .filter(|block| !block.is_air());
    mob.enderman.anger_end_time = tag::long_or(input, "anger_end_time", -1);
    mob.enderman.angry_at = super::nbt::uuid_field(input, "angry_at");
}

fn look_at_limited(me: &mut Entity, mob: &mut Mob, eye: Vec3) {
    use mc_bamboo_core::mth;
    let dx = eye.x - me.position.x;
    let dy = eye.y - (me.position.y + f64::from(mob.eye_height));
    let dz = eye.z - me.position.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let degrees = |rad: f64| rad * 180.0 / f64::from(std::f32::consts::PI);
    me.y_rot =
        super::control::rotate_towards(me.y_rot, degrees(mth::atan2(dz, dx)) as f32 - 90.0, 10.0);
    mob.x_rot = super::control::rotate_towards(
        mob.x_rot,
        -degrees(mth::atan2(dy, horizontal)) as f32,
        10.0,
    );
}

pub(super) fn block_can_use(me: &mut Entity, mob: &Mob, view: &dyn BlockView, place: bool) -> bool {
    mob.enderman.carried.is_some() == place
        && view.block_game_rules().mob_griefing
        && me
            .random
            .next_int_bounded(reduced_tick_delay(if place { 2000 } else { 20 }))
            == 0
}

pub(super) fn sample_block(me: &mut Entity, mob: &mut Mob, place: bool) {
    let radius = if place { 1.0 } else { 2.0 };
    let x = me.position.x - radius + me.random.next_double() * radius * 2.0;
    let y = me.position.y + me.random.next_double() * if place { 2.0 } else { 3.0 };
    let z = me.position.z - radius + me.random.next_double() * radius * 2.0;
    mob.enderman.pending_block = Some((
        me.block_position(),
        BlockPos::containing(Vec3::new(x, y, z)),
        place,
    ));
}

pub(super) fn flush_block_goal(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    if let Some((origin, pos, place)) = me
        .kind
        .mob_mut()
        .and_then(|mob| mob.enderman.pending_block.take())
    {
        ctx.emit(
            crate::entity::ctx::Command::EndermanBlock { origin, pos, place },
            me,
        );
    }
}

pub(crate) fn apply_block_goal(
    me: &mut Entity,
    world: &mut crate::World<'_>,
    origin: BlockPos,
    pos: BlockPos,
    place: bool,
) {
    let uid = me.id;
    let Some(mob) = me.kind.mob_mut() else {
        return;
    };
    if mob.mob_type != MobType::Enderman || !world.level().block_game_rules().mob_griefing {
        return;
    }
    let state = world.block_state(pos);
    if place {
        let Some(carried) = mob.enderman.carried else {
            return;
        };
        let below = world.block_state(pos.below());
        if !state.is_air()
            || below.is(mc_bamboo_data::blocks::BEDROCK)
            || !below.is_collision_shape_full_block()
        {
            return;
        }
        let cube = Aabb::new(0., 0., 0., 1., 1., 1.).at(pos);
        let mut occupied = false;
        world
            .level()
            .entities_table()
            .for_each_in(cube, &mut |other| occupied |= other.uid != uid);
        world.for_each_player(&mut |player| {
            occupied |= super::goals::CombatTarget::player(*player, true)
                .bounds()
                .intersects(cube);
        });
        if !occupied {
            let carried = world.update_from_neighbour_shapes(carried, pos);
            if carried.can_survive(world, pos) && world.set_block_and_update(pos, carried) {
                mob.enderman.carried = None;
            }
        }
    } else if mob.enderman.carried.is_none() && state.is_tagged(block_tags::ENDERMAN_HOLDABLE) {
        let from = Vec3::new(
            f64::from(origin.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(origin.z) + 0.5,
        );
        let to = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        let reachable = world
            .clip(from, to, crate::world::ClipFluid::None, 0)
            .is_none_or(|hit| hit.pos == pos);
        if reachable && world.remove_block(pos) {
            mob.enderman.carried = Some(state.default_state());
        }
    }
}

impl Mob {
    /// 逐刻对拍使用的末影人状态。
    ///
    /// Enderman state for tick-by-tick comparisons.
    pub fn describe_enderman(&self) -> String {
        format!(
            "carry={},creepy={},stared={}",
            self.enderman.carried.map_or(0, |block| block.0),
            self.enderman.creepy,
            self.enderman.stared_at
        )
    }

    /// 场景 / 存档用：直接设置搬着的方块。
    ///
    /// Scene and save use: set the carried block directly.
    pub fn set_carried_block(&mut self, block: Option<BlockState>) {
        self.enderman.carried = block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, World, testing::TestLevel};
    use mc_bamboo_data::blocks;

    #[test]
    fn carried_block_round_trips_with_properties_and_air_is_not_carried() {
        let mut mob = Mob::new(MobType::Enderman);
        let carried = BlockState(blocks::GRASS_BLOCK).with(crate::state::SNOWY, true);
        mob.enderman.carried = Some(carried);
        mob.enderman.angry_at = Some([42; 16]);
        mob.enderman.anger_end_time = 1234;
        let mut fields = Vec::new();
        write_nbt(&mob, &mut fields);
        let mut loaded = Mob::new(MobType::Enderman);
        read_nbt(&mut loaded, &Tag::Compound(fields));
        assert_eq!(loaded.enderman, mob.enderman);
        read_nbt(
            &mut loaded,
            &Tag::Compound(vec![(
                "carriedBlockState".to_owned(),
                tag::encode_block_state(BlockState(blocks::AIR)),
            )]),
        );
        assert!(loaded.enderman.carried.is_none());
        assert_eq!(loaded.enderman.anger_end_time, -1);
    }

    #[test]
    fn picking_up_and_placing_obeys_griefing_and_support() {
        let mut level = TestLevel::new();
        let pos = BlockPos::new(1, 64, 0);
        let origin = BlockPos::new(0, 64, 0);
        level.set(pos, BlockState(blocks::DIRT));
        level.set(pos.below(), BlockState(blocks::BEDROCK));
        let mut ticks = BlockTicks::default();
        let mut entity = Entity::by_mob_type(MobType::Enderman, Vec3::new(0.5, 64.0, 0.5));
        {
            let mut world = World::new(&mut level, &mut ticks);
            apply_block_goal(&mut entity, &mut world, origin, pos, false);
            assert!(world.block_state(pos).is_air());
            apply_block_goal(&mut entity, &mut world, origin, pos, true);
            assert!(world.block_state(pos).is_air());
        }
        level.set(pos.below(), BlockState(blocks::STONE));
        {
            let mut world = World::new(&mut level, &mut ticks);
            apply_block_goal(&mut entity, &mut world, origin, pos, true);
            assert!(world.block_state(pos).is(blocks::DIRT));
        }
        level.rules.mob_griefing = false;
        let mut world = World::new(&mut level, &mut ticks);
        apply_block_goal(&mut entity, &mut world, origin, pos, false);
        assert!(world.block_state(pos).is(blocks::DIRT));
    }
}
