//! 玩家移动校验，对应官方 `ServerGamePacketListenerImpl.handleMovePlayer`。
//!
//! 位移先用玩家自己的碰撞盒走一遍 `Entity.move`，再和客户端报的位置比。
//! 差得太多就拉回，不断开。悬空太久才按飞行踢掉。
//!
//! Player movement checks, matching `ServerGamePacketListenerImpl.handleMovePlayer`.
//!
//! The displacement is run through `Entity.move` with the player's own box, then compared
//! with the position the client reported. A large disagreement rubber-bands without a disconnect.
//! Only staying airborne too long disconnects for flying.

use mc_bamboo_blocks::entity::{
    self, TravelSegment, enclosed_in_air,
    mob::{DamageKind, DamageSource},
    no_block_collision, overlaps_new_blocks, sweep_movement,
};
use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockState, CollisionContext, EntityContext, PistonMoving,
    ShulkerBoxEntity, world::CollisionBlocks,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3, mth};
use mc_bamboo_data::{
    FluidTag, blocks, gamerules, items,
    registries::{attribute, custom_stat, mob_effect},
    tags::fluid_tags,
};
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    item::ItemStack,
    packet::{
        ClientboundPacket,
        play::{ClientInformation, ENTITY_FLAG_SPRINTING, EquipmentSlot, GameMode, PlayerPosition},
    },
};
use mc_bamboo_world::store::ChunkStore;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    inbound::{PlayerId, PlayerSink},
    movement::{self, set_rotation},
    outgoing::Outgoing,
    player::Player,
    player_environment::Landing,
    region::Region,
};

const MAX_HORIZONTAL: f64 = 3.0e7;
const MAX_VERTICAL: f64 = 2.0e7;
/// `Avatar.POSES` 里站立、蹲下、游泳的高度。宽度都是 0.6。
///
/// Standing, crouching and swimming heights from `Avatar.POSES`. The width is 0.6 for all three.
const STANDING_BODY: f64 = 1.8;
const CROUCHING_BODY: f64 = 1.5;
const SWIMMING_BODY: f64 = 0.6;
/// 滑翔姿势的碰撞盒高度，对应 `Avatar.POSES`。
/// The glide bounding-box height from `Avatar.POSES`.
pub(crate) const FALL_FLYING_BODY: f64 = 0.6_f32 as f64;
const HALF_WIDTH: f64 = 0.3;
/// 专用服务器 `allow-flight` 的默认值。配置里没有这一项。
///
/// Dedicated `allow-flight`. The config has no field for it, so this is the default.
const ALLOW_FLIGHT: bool = false;
/// 客户端还没发 `player_loaded` 时，最多再等这么多 tick。
///
/// How many ticks to keep ignoring movement before `player_loaded` arrives.
const CLIENT_LOAD_TIMEOUT: i32 = 60;
const MOVED_WRONGLY: f64 = 0.0625;
const PACKETS_BEFORE_PENALTY: i32 = 5;
const METRES_PER_TICK: f32 = 100.0;

/// `ServerGamePacketListenerImpl` 里和这一名玩家的移动有关的字段。
///
/// The movement fields `ServerGamePacketListenerImpl` keeps for one player.
#[derive(Debug, Clone)]
pub(crate) struct Motion {
    first_good: Vec3,
    last_good: Vec3,
    /// `Entity.deltaMovement`。速度检查用它的长度平方。
    ///
    /// `Entity.deltaMovement`. The speed check uses its squared length.
    pub(crate) delta: Vec3,
    /// `Entity.fallDistance`，落地结算后清零。
    /// `Entity.fallDistance`, cleared after settling a landing.
    pub(crate) fall_distance: f64,
    tick_count: i32,
    received_move_packets: i32,
    known_move_packets: i32,
    received_position_this_tick: bool,
    /// 位移箭用的是最近一次被接受的客户端位移，不是物理速度。
    ///
    /// The last accepted client displacement, not the physics velocity.
    pub(crate) known_movement: Vec3,
    received_movement_this_tick: bool,
    /// 上一个被接受的移动把这一位置成了真。
    ///
    /// Set by the last accepted move.
    pub(crate) client_is_floating: bool,
    /// 连续悬空的 tick 数。
    ///
    /// How many ticks the floating flag has stayed set.
    pub(crate) above_ground_ticks: i32,
    client_loaded_timeout: i32,
    /// 上一次 `Entity.move` 留下的 `verticalCollisionBelow`。
    ///
    /// `verticalCollisionBelow` left by the previous `Entity.move`.
    vertical_collision_below: bool,
    stuck_speed: Vec3,
    /// `noPhysics`。旁观模式在 tick 里设上，和官方 `Player.tick` 一样。
    ///
    /// `noPhysics`. Spectator mode sets it during the tick, as `Player.tick` does.
    no_physics: bool,
    /// `Avatar` 姿势对应的碰撞盒高度。默认站立。`updatePlayerPose` 在 tick 末尾才改它。
    ///
    /// Collision-box height for the `Avatar` pose. Standing until `updatePlayerPose` runs at the end of a tick.
    body_height: f64,
    /// 客户端移动包里的水平碰撞位。`setOnGroundWithMovement` 会记下它。
    ///
    /// The horizontal-collision bit from the move packet. `setOnGroundWithMovement` stores it.
    pub(crate) client_horizontal_collision: bool,
    /// `Entity.isSwimming()`（共享标志位 4）。海豚跟着游泳的玩家游时读它。
    ///
    /// `Entity.isSwimming()` (shared flag 4); dolphins read it to swim with players.
    pub(crate) swimming: bool,
    pending_inside: Vec<TravelSegment>,
}

impl Motion {
    pub(crate) fn new(position: Vec3) -> Self {
        Self {
            first_good: position,
            last_good: position,
            delta: Vec3::default(),
            fall_distance: 0.0,
            tick_count: 0,
            received_move_packets: 0,
            known_move_packets: 0,
            received_position_this_tick: false,
            known_movement: Vec3::default(),
            received_movement_this_tick: false,
            client_is_floating: false,
            above_ground_ticks: 0,
            client_loaded_timeout: CLIENT_LOAD_TIMEOUT,
            vertical_collision_below: false,
            stuck_speed: Vec3::default(),
            no_physics: false,
            body_height: STANDING_BODY,
            client_horizontal_collision: false,
            swimming: false,
            pending_inside: Vec::new(),
        }
    }

    /// 客户端发了 `player_loaded`，或者加载超时已经走完。
    ///
    /// The client sent `player_loaded`, or the load timeout has elapsed.
    pub(crate) fn client_loaded(&self) -> bool {
        self.client_loaded_timeout <= 0
    }

    /// 当前的碰撞盒使用滑翔姿势。
    /// Whether the collision box is in the fall-flying pose.
    pub(crate) fn is_fall_flying_pose(&self) -> bool {
        self.body_height == FALL_FLYING_BODY
    }

    pub(crate) fn mark_loaded(&mut self) {
        self.client_loaded_timeout = 0;
    }

    /// 把本 tick 的起点和上一个接受的位置都设成 `position`。
    ///
    /// Sets both the start-of-tick position and the last accepted position to `position`.
    pub(crate) fn sync_goods(&mut self, position: Vec3) {
        self.first_good = position;
        self.last_good = position;
    }
}

/// 处理四种移动包。
///
/// Handles the four player move packets.
pub(crate) fn on_move_player(
    region: &mut Region,
    id: PlayerId,
    position: Option<Vec3>,
    rotation: Option<(f32, f32)>,
    on_ground: bool,
    horizontal_collision: bool,
) {
    // 边界快照在借出玩家表之前取：两种借用互不相交。
    //
    // Take the border snapshot before borrowing the player map: the borrows are disjoint.
    let border = region.global.world_border.clone();
    let Region {
        players,
        player_grid,
        links,
        chunks,
        block_entities,
        tick,
        ..
    } = region;
    let Some(player) = players.get_mut(&id) else {
        return;
    };
    // 等出生点区块就绪时不处理移动包：官方 `PrepareSpawnTask` 在进服前
    // 把区块准备好，玩家这时还没进场。
    // Ignore movement while the spawn chunks are pending: vanilla
    // `PrepareSpawnTask` prepares them before the player enters.
    if player.spawn_pending.is_some() {
        return;
    }
    let reported = position.unwrap_or(player.position());
    let (yaw, pitch) = rotation.unwrap_or((player.yaw, player.pitch));
    // 骑在载具上时位置由 `positionRider` 决定，移动包只用来更新朝向：官方的客户端
    // 这时发的是 `move_vehicle`，这里不接受 `move_player` 带来的位置。
    //
    // While riding, `positionRider` owns the position: a move packet only updates
    // the rotation. Vanilla clients send `move_vehicle` then, so the position a
    // `move_player` carries is ignored.
    if player.riding.is_passenger() {
        if rotation.is_some() {
            set_rotation(player, yaw, pitch);
        }
        return;
    }
    if movement::invalid_position(reported, (yaw, pitch)) {
        links.kick(id, &movement::invalid_movement());
        return;
    }
    if position.is_some() {
        if player.motion.received_position_this_tick {
            links.kick(id, &movement::invalid_movement());
            return;
        }
        player.motion.received_position_this_tick = true;
    }
    if player.motion.tick_count == 0 {
        let here = player.position();
        player.motion.sync_goods(here);
    }
    if !player.motion.client_loaded() {
        return;
    }
    if let Some(awaiting) = player.teleport.awaiting_position {
        if tick.since(player.teleport.sent_at) > movement::TELEPORT_RESEND_TICKS {
            movement::teleport(player, links, player_grid, *tick, awaiting);
        }
        if rotation.is_some() {
            set_rotation(player, yaw, pitch);
        }
        return;
    }

    let sample = PlayerBlocks::new(chunks, block_entities);
    let landing = apply_change(
        player,
        player_grid,
        links,
        *tick,
        Attempt {
            target: reported,
            yaw,
            pitch,
            on_ground,
            horizontal_collision,
            check_speed: should_check_speed(player, &region.global.rules),
        },
        &sample,
        &border,
    );
    if let Some(landing) = landing {
        crate::player_environment::on_land(region, id, landing);
    }
}

/// 处理 `accept_teleportation`。
///
/// Handles `accept_teleportation`.
pub(crate) fn on_accept_teleportation(
    region: &mut Region,
    id: PlayerId,
    teleport_id: i32,
    position: Vec3,
    (yaw, pitch): (f32, f32),
) {
    // 边界快照在借出玩家表之前取：两种借用互不相交。
    //
    // Take the border snapshot before borrowing the player map: the borrows are disjoint.
    let border = region.global.world_border.clone();
    let Region {
        players,
        player_grid,
        links,
        chunks,
        block_entities,
        tick,
        ..
    } = region;
    let Some(player) = players.get_mut(&id) else {
        return;
    };
    if teleport_id != player.teleport.latest_id {
        return;
    }
    if player.teleport.awaiting_position.is_none()
        || movement::invalid_position(position, (yaw, pitch))
    {
        links.kick(id, &movement::invalid_movement());
        return;
    }
    let awaiting = player
        .teleport
        .awaiting_position
        .take()
        .expect("上面已经确认过");
    player.set_position(player_grid, awaiting);
    player.motion.last_good = awaiting;
    let sample = PlayerBlocks::new(chunks, block_entities);
    apply_change(
        player,
        player_grid,
        links,
        *tick,
        Attempt {
            target: position,
            yaw,
            pitch,
            on_ground: false,
            horizontal_collision: false,
            check_speed: should_check_speed(player, &region.global.rules),
        },
        &sample,
        &border,
    );
}

/// `handleClientTickEnd`：清掉“这一客户端 tick 已经收过位置”的标记。
///
/// `handleClientTickEnd`: clears the flag that a position already arrived this client tick.
pub(crate) fn on_client_tick_end(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        player.motion.received_position_this_tick = false;
        if !std::mem::take(&mut player.motion.received_movement_this_tick) {
            player.motion.known_movement = Vec3::default();
        }
    }
}

/// `handleAcceptPlayerLoad`。
///
/// `handleAcceptPlayerLoad`.
pub(crate) fn on_player_loaded(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        player.motion.mark_loaded();
    }
}

/// 这一服务端 tick 的收尾：加载超时、方块内效果、飞行，以及下一 tick 的起点。
///
/// End of one server tick: the load timeout, inside-block effects, flying, and the next tick's origin.
pub(crate) fn finish_tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.spawn_pending.is_none())
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        finish_player(region, id);
    }
}

/// 测试里把玩家直接放到 `position`，当作人已经在那儿了。
///
/// Test helper: puts the player at `position`, as if they were already there.
#[cfg(test)]
pub(crate) fn place_player(region: &mut Region, id: PlayerId, position: Vec3) {
    let Region {
        players,
        player_grid,
        ..
    } = region;
    let Some(player) = players.get_mut(&id) else {
        return;
    };
    let position = clamp_position(position);
    player.set_position(player_grid, position);
    player.motion.sync_goods(position);
    player.motion.mark_loaded();
    player.motion.received_position_this_tick = false;
}

fn finish_player(region: &mut Region, id: PlayerId) {
    {
        let Region {
            players,
            chunks,
            block_entities,
            ..
        } = region;
        if let Some(player) = players.get_mut(&id) {
            let sample = PlayerBlocks::new(chunks, block_entities);
            update_body(player, &sample);
            update_swimming(player, &sample);
        }
    }
    let pending = {
        let Some(player) = region.players.get(&id) else {
            return;
        };
        player.motion.pending_inside.clone()
    };
    let stuck = inside_stuck(region, id, &pending);
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    player.motion.pending_inside.clear();
    if let Some(multiplier) = stuck {
        player.motion.stuck_speed = multiplier;
        player.motion.fall_distance = 0.0;
    }
    if player.motion.client_loaded_timeout > 0 {
        player.motion.client_loaded_timeout -= 1;
    }
    player.motion.no_physics = player.game_mode == GameMode::Spectator;
    // `LivingEntity.aiStep`：缓降 / 漂浮时每刻把 `fallDistance` 清零。
    // 官方的服务端 tick 收尾在 `Entity.move` 之后做这件事，所以放在这里。
    // `LivingEntity.aiStep` resets fall distance while slow falling or
    // levitating; vanilla does this at the end of the server tick.
    if player.effects.has(mob_effect::SLOW_FALLING) || player.effects.has(mob_effect::LEVITATION) {
        player.motion.fall_distance = 0.0;
    }
    let here = player.position();
    player.motion.sync_goods(here);
    player.motion.tick_count = player.motion.tick_count.wrapping_add(1);
    player.motion.known_move_packets = player.motion.received_move_packets;
    let kick = flying_tick(player);
    let name = player.name.clone();
    if kick {
        warn!("{name} was kicked for floating too long!");
        region
            .links
            .kick(id, &movement::translated("multiplayer.disconnect.flying"));
    }
}

/// 推进服务端的滑翔运动，并在速度因撞墙损失时结算伤害。
/// Advance server-side gliding and apply damage for speed lost to a wall collision.
pub(crate) fn travel_fall_flying(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let prior = player.motion.delta;
    let position = player.position();
    player.motion.delta = glide_velocity(player, prior);
    let blocks = PlayerBlocks::new(&region.chunks, &region.block_entities);
    let simulated = simulate_move(
        player,
        player.motion.delta,
        &blocks,
        &region.global.world_border,
    );
    let end = position.add(simulated.movement);
    player.set_position(&mut region.player_grid, end);
    player.motion.delta = simulated.velocity;
    player.motion.stuck_speed = simulated.stuck_speed;
    if let Some(below) = simulated.vertical_collision_below {
        player.motion.vertical_collision_below = below;
        player.on_ground = below;
    }
    remember_inside(player, position, end, simulated.attempted, end);
    let horizontal_collision = (simulated.attempted.x - simulated.movement.x).abs() >= 1.0e-5
        || (simulated.attempted.z - simulated.movement.z).abs() >= 1.0e-5;
    if horizontal_collision {
        let loss = horizontal_speed(prior) - horizontal_speed(simulated.velocity);
        let damage = (loss * 10.0 - 3.0) as f32;
        if damage > 0.0 {
            crate::player_hurt::play_fall_sound(region, id, damage as i32, end);
            crate::player_hurt::hurt_player(
                region,
                id,
                DamageSource::environment(DamageKind::FlyIntoWall),
                damage,
                &mut None,
            );
        }
    }
}

fn horizontal_speed(velocity: Vec3) -> f64 {
    (velocity.x * velocity.x + velocity.z * velocity.z).sqrt()
}

fn glide_velocity(player: &Player, prior: Vec3) -> Vec3 {
    let look = Vec3::view_vector(player.yaw, player.pitch);
    let pitch = f64::from(player.pitch * 0.017453292_f32);
    let look_speed = horizontal_speed(look);
    let prior_speed = horizontal_speed(prior);
    let mut gravity = player.attributes.value(attribute::GRAVITY);
    if prior.y <= 0.0 && player.effects.has(mob_effect::SLOW_FALLING) {
        gravity = gravity.min(0.01);
    }
    let cos_pitch = pitch.cos();
    let lift = cos_pitch * cos_pitch;
    let mut velocity = prior.add(Vec3::new(0.0, gravity * (-1.0 + lift * 0.75), 0.0));
    if look_speed > 0.0 {
        if velocity.y < 0.0 {
            let lift = velocity.y * -0.1 * lift;
            velocity = velocity.add(Vec3::new(
                look.x * lift / look_speed,
                lift,
                look.z * lift / look_speed,
            ));
        }
        if pitch < 0.0 {
            let climb = prior_speed * f64::from(-mth::sin(pitch)) * 0.04;
            velocity = velocity.add(Vec3::new(
                -look.x * climb / look_speed,
                climb * 3.2,
                -look.z * climb / look_speed,
            ));
        }
        velocity = velocity.add(Vec3::new(
            (look.x / look_speed * prior_speed - velocity.x) * 0.1,
            0.0,
            (look.z / look_speed * prior_speed - velocity.z) * 0.1,
        ));
    }
    velocity.multiply(
        f64::from(0.99_f32),
        f64::from(0.98_f32),
        f64::from(0.99_f32),
    )
}

fn inside_stuck(region: &Region, id: PlayerId, pending: &[TravelSegment]) -> Option<Vec3> {
    if pending.is_empty() {
        return None;
    }
    let player = region.players.get(&id)?;
    let template = movement_box(player);
    let origin = player.position();
    let context = EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom: origin.y,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement: false,
        powder_snow_walkable: wears_leather_boots(player),
    };
    entity::stuck_speed(
        pending,
        |feet| {
            let delta = feet.subtract(origin);
            template.moved(delta.x, delta.y, delta.z)
        },
        context,
        &mut |pos| block_state(&region.chunks, pos),
    )
}

fn flying_tick(player: &mut Player) -> bool {
    if player.motion.client_is_floating {
        player.motion.above_ground_ticks = player.motion.above_ground_ticks.wrapping_add(1);
        player.motion.above_ground_ticks > maximum_flying_ticks(player)
    } else {
        player.motion.client_is_floating = false;
        player.motion.above_ground_ticks = 0;
        false
    }
}

/// `getMaximumFlyingTicks`：从玩家属性表读 `Attributes.GRAVITY`。
///
/// `getMaximumFlyingTicks`: reads `Attributes.GRAVITY` from the player's
/// attribute map.
pub(crate) fn maximum_flying_ticks(player: &Player) -> i32 {
    let gravity = player.attributes.value(attribute::GRAVITY);
    if gravity < f64::from(1.0e-5_f32) {
        return i32::MAX;
    }
    let modifier = 0.08 / gravity;
    mth::ceil(80.0 * modifier.max(1.0))
}

struct Attempt {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    horizontal_collision: bool,
    check_speed: bool,
}

struct Change<'a> {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    horizontal_collision: bool,
    blocks: &'a PlayerBlocks<'a>,
}

fn apply_change(
    player: &mut Player,
    grid: &mut crate::player_grid::PlayerGrid,
    links: &mut crate::links::Links,
    now: crate::region::RegionTick,
    attempt: Attempt,
    blocks: &PlayerBlocks<'_>,
    border: &mc_bamboo_blocks::WorldBorder,
) -> Option<Landing> {
    let change = Change {
        target: clamp_position(attempt.target),
        yaw: attempt.yaw,
        pitch: attempt.pitch,
        on_ground: attempt.on_ground,
        horizontal_collision: attempt.horizontal_collision,
        blocks,
    };
    if reject_speed(player, &change, attempt.check_speed) {
        movement::teleport(player, links, grid, now, player.position());
        return None;
    }
    let start = player.position();
    let from_last = change.target.subtract(player.motion.last_good);
    let moved_up = from_last.y > 0.0;
    if player.on_ground && !change.on_ground && moved_up {
        jump_from_ground(player);
    }
    let stands = player.motion.vertical_collision_below;
    let simulated = simulate_move(player, from_last, change.blocks, border);
    let end = start.add(simulated.movement);
    player.motion.delta = simulated.velocity;
    player.motion.stuck_speed = simulated.stuck_speed;
    // 玩家在服务端不是 `isLocalInstanceAuthoritative`，所以只有竖直位移才改落地标记。
    // `move` 里的摔落记录被跳过，只留下后面那次 `doCheckFallDamage`。
    //
    // A server player is not `isLocalInstanceAuthoritative`, so only a vertical move updates the
    // ground flags. The fall record inside `move` is skipped; the later `doCheckFallDamage` remains.
    if let Some(below) = simulated.vertical_collision_below {
        player.motion.vertical_collision_below = below;
        if simulated.sets_on_ground {
            player.on_ground = below;
        }
    }

    let mut x_error = change.target.x - end.x;
    let mut y_error = change.target.y - end.y;
    // 官方这个条件恒为真，竖直误差不计入 moved wrongly。
    //
    // This condition is always true in vanilla, so vertical error never counts as moved wrongly.
    if y_error > -0.5 || y_error < 0.5 {
        y_error = 0.0;
    }
    let z_error = change.target.z - end.z;
    let moved_wrongly = x_error * x_error + y_error * y_error + z_error * z_error;
    x_error = moved_wrongly;
    let creative = player.game_mode == GameMode::Creative;
    let spectator = player.game_mode == GameMode::Spectator;
    let fail = x_error > MOVED_WRONGLY && !creative && !spectator;
    if fail {
        warn!("{} moved wrongly!", player.name);
    }
    let old_box = movement_box(player);
    // 官方 `level.noCollision(player, oldAABB)` 是方块、实体、边界三样都不碰。
    // 方块和实体在 `no_block_collision` 里，边界单独补上。
    //
    // Vanilla `level.noCollision(player, oldAABB)` means no block, entity or border
    // collision. Blocks and entities live in `no_block_collision`; the border is added here.
    let old_clear =
        no_block_collision(
            old_box,
            context_at(player, end.y, false),
            CollisionBlocks {
                block_at: |pos: BlockPos| change.blocks.state(pos),
                piston_at: |pos| change.blocks.piston(pos),
                shulker_at: |pos| change.blocks.shulker(pos),
            },
        ) && border.no_border_collision(player.position().x, player.position().z, old_box);
    let newly = overlaps_new_blocks(
        old_box,
        box_at(player, end),
        end,
        change.target,
        context_at(player, old_box.min_y, true),
        CollisionBlocks {
            block_at: |pos: BlockPos| change.blocks.state(pos),
            piston_at: |pos| change.blocks.piston(pos),
            shulker_at: |pos| change.blocks.shulker(pos),
        },
    );
    let accept = player.motion.no_physics || (!fail || !old_clear) && !newly;
    if !accept {
        set_rotation(player, change.yaw, change.pitch);
        movement::teleport(player, links, grid, now, start);
        return check_fall(player, change.blocks, Vec3::default(), change.on_ground);
    }

    player.set_position(grid, change.target);
    set_rotation(player, change.yaw, change.pitch);
    player.motion.client_is_floating = floating_now(player, from_last.y, stands, change.blocks);
    player.on_ground = change.on_ground;
    player.motion.client_horizontal_collision = change.horizontal_collision;
    let client_delta = change.target.subtract(start);
    player.motion.known_movement = client_delta;
    player.motion.received_movement_this_tick = true;
    // `ServerPlayer.checkMovementStatistics`：按客户端报告的位移算消耗的厘米数。
    // 水 / 梯子 / 游泳的判定直接从玩家当前所在格和脚下格读方块状态，
    // 和官方 `isEyeInFluid` / `isInWater` / `onClimbable` 的方块查询一致。
    let sprinting = player.sync.is_sprinting();
    let feet = BlockPos::containing(change.target);
    let eye = BlockPos::containing(Vec3::new(
        change.target.x,
        change.target.y + player.eye_height(),
        change.target.z,
    ));
    let eye_state = change.blocks.state(eye);
    let feet_state = change.blocks.state(feet);
    let eye_in_water = eye_state
        .fluid()
        .is_tagged(mc_bamboo_data::tags::fluid_tags::WATER);
    let fall_flying = player.sync.is_fall_flying();
    let in_water = if fall_flying {
        touches_fluid(player, change.blocks, fluid_tags::WATER)
    } else {
        feet_state.fluid().is_water()
    };
    let on_climbable = crate::player_glide::on_climbable(player, change.blocks);
    let swimming = !fall_flying && player.motion.body_height == SWIMMING_BODY && !player.on_ground;
    if fall_flying && !eye_in_water && !in_water && !on_climbable && !player.on_ground {
        let centimeters = (client_delta.length_sqr().sqrt() as f32 * 100.0).round() as i32;
        if centimeters > 0 {
            player
                .item_use_stats
                .award_custom(custom_stat::AVIATE_ONE_CM, centimeters);
        }
    }
    if let Some(amount) = crate::player_exhaustion::movement(
        client_delta.x,
        client_delta.y,
        client_delta.z,
        swimming,
        eye_in_water,
        in_water,
        on_climbable,
        player.on_ground,
        sprinting,
        fall_flying,
    ) {
        crate::player_exhaustion::add(player, amount);
    }
    let landing = check_fall(player, change.blocks, client_delta, change.on_ground);
    if moved_up && landing.is_none() {
        player.motion.fall_distance = 0.0;
    }
    remember_inside(player, start, end, simulated.attempted, change.target);
    player.motion.last_good = player.position();
    landing
}

fn check_fall(
    player: &mut Player,
    blocks: &PlayerBlocks<'_>,
    movement: Vec3,
    on_ground: bool,
) -> Option<Landing> {
    let bounds = movement_box(player).inflated(1.0);
    let min = ChunkPos::containing(bounds.min_x.floor(), bounds.min_z.floor());
    let max = ChunkPos::containing(bounds.max_x.ceil(), bounds.max_z.ceil());
    for x in min.x..=max.x {
        for z in min.z..=max.z {
            if !blocks.chunks.is_loaded(ChunkPos::new(x, z)) {
                return None;
            }
        }
    }
    if touches_fluid(player, blocks, fluid_tags::WATER) {
        player.motion.fall_distance = 0.0;
        return None;
    }
    let before = player.motion.fall_distance;
    if movement.y < 0.0 {
        player.motion.fall_distance -= f64::from(movement.y as f32);
    }
    if on_ground {
        if player.motion.fall_distance > 0.0 {
            return Some(Landing { movement, before });
        }
        player.motion.fall_distance = 0.0;
    }
    None
}

fn should_check_speed(player: &Player, rules: &crate::gamerules::GameRuleSet) -> bool {
    rules.bool(gamerules::PLAYER_MOVEMENT_CHECK)
        && (!player.sync.is_fall_flying() || rules.bool(gamerules::ELYTRA_MOVEMENT_CHECK))
}

fn reject_speed(player: &mut Player, change: &Change<'_>, check_speed: bool) -> bool {
    player.motion.received_move_packets = player.motion.received_move_packets.wrapping_add(1);
    let mut packets = player
        .motion
        .received_move_packets
        .wrapping_sub(player.motion.known_move_packets);
    if packets > PACKETS_BEFORE_PENALTY {
        debug!(
            "{} is sending move packets too frequently ({packets} packets since last tick)",
            player.name
        );
        packets = 1;
    }
    if !check_speed {
        return false;
    }
    let offset = change.target.subtract(player.motion.first_good);
    let moved = offset.length_sqr();
    let expected = player.motion.delta.length_sqr();
    let limit = if player.sync.is_fall_flying() {
        300.0
    } else {
        METRES_PER_TICK
    };
    let limit = f64::from(limit * packets as f32);
    if moved - expected > limit {
        warn!(
            "{} moved too quickly! {},{},{}",
            player.name, offset.x, offset.y, offset.z
        );
        return true;
    }
    false
}

struct Simulated {
    movement: Vec3,
    attempted: Vec3,
    velocity: Vec3,
    stuck_speed: Vec3,
    /// `None` 表示这一步没有改 `verticalCollisionBelow`。
    ///
    /// `None` means this step left `verticalCollisionBelow` alone.
    vertical_collision_below: Option<bool>,
    sets_on_ground: bool,
}

fn simulate_move(
    player: &Player,
    displacement: Vec3,
    blocks: &PlayerBlocks<'_>,
    border: &mc_bamboo_blocks::WorldBorder,
) -> Simulated {
    let mut velocity = player.motion.delta;
    let mut stuck_speed = player.motion.stuck_speed;
    let mut delta = displacement;
    if stuck_speed.length_sqr() > 1.0e-7 {
        delta = delta.multiply(stuck_speed.x, stuck_speed.y, stuck_speed.z);
        stuck_speed = Vec3::default();
        velocity = Vec3::default();
    }
    if player.motion.no_physics {
        return Simulated {
            movement: delta,
            attempted: delta,
            velocity,
            stuck_speed,
            vertical_collision_below: Some(false),
            sets_on_ground: false,
        };
    }
    delta = back_off_from_edge(player, delta, blocks);
    let resolved = sweep_movement(
        delta,
        movement_box(player),
        step_height(player),
        player.on_ground,
        move_context(player),
        CollisionBlocks {
            block_at: |pos| blocks.state(pos),
            piston_at: |pos| blocks.piston(pos),
            shulker_at: |pos| blocks.shulker(pos),
        },
        border,
    );
    let moved_vertically = delta.y.abs() > 0.0;
    let end = player.position().add(resolved);
    let factor = f64::from(block_speed_factor(player, end, blocks));
    if player.sync.is_fall_flying() {
        let restitution = if player.sync.shift_key_down {
            0.0
        } else {
            player.attributes.value(attribute::BOUNCINESS)
        };
        if (delta.x - resolved.x).abs() >= 1.0e-5 {
            velocity.x = -velocity.x * restitution;
        }
        if delta.y != resolved.y {
            velocity.y = 0.0;
        }
        if (delta.z - resolved.z).abs() >= 1.0e-5 {
            velocity.z = -velocity.z * restitution;
        }
    }
    velocity = velocity.multiply(factor, 1.0, factor);
    Simulated {
        movement: resolved,
        attempted: delta,
        velocity,
        stuck_speed,
        vertical_collision_below: moved_vertically
            .then_some(delta.y != resolved.y && delta.y < 0.0),
        sets_on_ground: moved_vertically,
    }
}

fn remember_inside(player: &mut Player, start: Vec3, end: Vec3, attempted: Vec3, target: Vec3) {
    let movement_length = end.subtract(start).length_sqr();
    let attempted_length = attempted.length_sqr();
    if movement_length > 1.0e-7 || attempted_length - movement_length < 1.0e-7 {
        player.motion.pending_inside.push(TravelSegment {
            from: start,
            to: end,
            original: Some(attempted),
        });
    }
    if end.distance_to_sqr(target) > f64::from(9.9999994e-11_f32) {
        player.motion.pending_inside.push(TravelSegment {
            from: end,
            to: target,
            original: None,
        });
    }
}

fn floating_now(
    player: &Player,
    oy: f64,
    stands_on_something: bool,
    blocks: &PlayerBlocks<'_>,
) -> bool {
    oy >= -0.03125
        && !stands_on_something
        && player.game_mode != GameMode::Spectator
        && !ALLOW_FLIGHT
        && !may_fly(player.game_mode)
        // `ServerGamePacketListenerImpl.handleMovePlayer`：漂浮效果期间
        // 不把悬空记成飞行，80 刻的飞行踢出因此不会误伤。
        // `handleMovePlayer`: levitation exempts the player from the
        // floating check, so the 80-tick flying kick cannot fire.
        && !player.effects.has(mob_effect::LEVITATION)
        && !player.sync.is_fall_flying()
        && enclosed_in_air(movement_box(player), &mut |pos| blocks.state(pos))
}

pub(crate) fn jump_from_ground(player: &mut Player) {
    // `LivingEntity.getJumpPower` = JUMP_STRENGTH * block factor + getJumpBoostPower()。
    // 跳跃提升每级 +0.1；移动校验要用同一份高度，否则跳跃时会被判成悬空。
    // `getJumpPower`: JUMP_STRENGTH times the block factor plus the jump-boost
    // power (0.1 per level). The movement check must use the same height.
    let boost = player
        .effects
        .amplifier(mob_effect::JUMP_BOOST)
        .map_or(0.0, |amplifier| 0.1 * (amplifier.max(0) as f32 + 1.0));
    let power = player.attributes.value(attribute::JUMP_STRENGTH) as f32 + boost;
    if power <= 1.0e-5 {
        return;
    }
    // `ServerPlayer.jumpFromGround`：疾跑 0.2 / 走路 0.05 点饥饿消耗。
    let sprinting = player.sync.shared_flags & ENTITY_FLAG_SPRINTING != 0;
    crate::player_exhaustion::add(player, crate::player_exhaustion::jump(sprinting));
    let mut delta = player.motion.delta;
    delta.y = f64::from(power).max(delta.y);
    if player.sync.shared_flags & ENTITY_FLAG_SPRINTING != 0 {
        // `(float)(Math.PI / 180)` 再乘 yaw，`Mth.sin` 的参数是这个 float 提升成的 double。
        //
        // `(float)(Math.PI / 180)` times yaw. `Mth.sin` takes that float promoted to double.
        let angle = f64::from(player.yaw * ((std::f64::consts::PI / 180.0) as f32));
        delta.x += f64::from(-mth::sin(angle)) * 0.2;
        delta.z += f64::from(mth::cos(angle)) * 0.2;
    }
    player.motion.delta = delta;
}

fn back_off_from_edge(player: &Player, delta: Vec3, blocks: &PlayerBlocks<'_>) -> Vec3 {
    let max_down = step_height(player);
    if player.sync.flying
        || delta.y > 0.0
        || !player.sync.shift_key_down
        || !is_above_ground(player, max_down, blocks)
    {
        return delta;
    }
    let mut x = delta.x;
    let mut z = delta.z;
    let step_x = x.signum() * 0.05;
    let step_z = z.signum() * 0.05;
    while x != 0.0 && can_fall_at_least(player, x, 0.0, f64::from(max_down), blocks) {
        if x.abs() <= 0.05 {
            x = 0.0;
            break;
        }
        x -= step_x;
    }
    while z != 0.0 && can_fall_at_least(player, 0.0, z, f64::from(max_down), blocks) {
        if z.abs() <= 0.05 {
            z = 0.0;
            break;
        }
        z -= step_z;
    }
    while x != 0.0 && z != 0.0 && can_fall_at_least(player, x, z, f64::from(max_down), blocks) {
        if x.abs() <= 0.05 {
            x = 0.0;
        } else {
            x -= step_x;
        }
        if z.abs() <= 0.05 {
            z = 0.0;
        } else {
            z -= step_z;
        }
    }
    Vec3::new(x, delta.y, z)
}

fn is_above_ground(player: &Player, max_down: f32, blocks: &PlayerBlocks<'_>) -> bool {
    player.on_ground
        || player.motion.fall_distance < f64::from(max_down)
            && !can_fall_at_least(
                player,
                0.0,
                0.0,
                f64::from(max_down) - player.motion.fall_distance,
                blocks,
            )
}

fn can_fall_at_least(
    player: &Player,
    delta_x: f64,
    delta_z: f64,
    min_height: f64,
    blocks: &PlayerBlocks<'_>,
) -> bool {
    let bounds = movement_box(player);
    let probe = Aabb::new(
        bounds.min_x + 1.0e-7 + delta_x,
        bounds.min_y - min_height - 1.0e-7,
        bounds.min_z + 1.0e-7 + delta_z,
        bounds.max_x - 1.0e-7 + delta_x,
        bounds.min_y,
        bounds.max_z - 1.0e-7 + delta_z,
    );
    no_block_collision(
        probe,
        move_context(player),
        CollisionBlocks {
            block_at: |pos| blocks.state(pos),
            piston_at: |pos| blocks.piston(pos),
            shulker_at: |pos| blocks.shulker(pos),
        },
    )
}

fn block_speed_factor(player: &Player, position: Vec3, blocks: &PlayerBlocks<'_>) -> f32 {
    // `Player.getBlockSpeedFactor` 在能力飞行或滑翔中直接返回 1。
    // `Player.getBlockSpeedFactor` returns 1 during ability flight or gliding.
    if player.sync.flying || player.sync.is_fall_flying() {
        return 1.0;
    }
    let here_pos = BlockPos::containing(position);
    let state = blocks.state(here_pos);
    let here = state.block().speed_factor;
    if !state.is(blocks::WATER) && !state.is(blocks::BUBBLE_COLUMN) && here == 1.0 {
        let below = BlockPos::new(
            mth::floor(position.x),
            mth::floor(position.y - 0.500001),
            mth::floor(position.z),
        );
        blocks.state(below).block().speed_factor
    } else {
        here
    }
}

/// `canEntityWalkOnPowderSnow` 的 LivingEntity 分支：脚上穿皮革靴。
fn wears_leather_boots(player: &Player) -> bool {
    player
        .inventory
        .equipment(EquipmentSlot::Feet)
        .is_some_and(|boots| boots.item == items::LEATHER_BOOTS && !boots.is_empty())
}

/// 移动与落地支撑查询共用的实体碰撞上下文。
pub(crate) fn move_context(player: &Player) -> CollisionContext {
    CollisionContext::Entity(EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom: player.position().y,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement: false,
        powder_snow_walkable: wears_leather_boots(player),
    })
}

fn context_at(player: &Player, bottom: f64, placement: bool) -> CollisionContext {
    CollisionContext::Entity(EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement,
        powder_snow_walkable: wears_leather_boots(player),
    })
}

/// `Player.updatePlayerPose`。游泳姿势塞不进去就保持原样。
///
/// `Player.updatePlayerPose`. The pose stays put when the swimming box does not fit.
/// 按当前位置和姿势更新移动碰撞盒。
/// Resolve the movement box for the player's current position and pose.
pub(crate) fn update_body(player: &mut Player, blocks: &PlayerBlocks<'_>) {
    let gliding = player.sync.is_fall_flying();
    let low_body = if gliding {
        FALL_FLYING_BODY
    } else {
        SWIMMING_BODY
    };
    if !body_fits(player, low_body, blocks) {
        return;
    }
    let want_crouch = player.sync.shift_key_down && !player.sync.flying;
    let desired = if gliding {
        FALL_FLYING_BODY
    } else if want_crouch {
        CROUCHING_BODY
    } else {
        STANDING_BODY
    };
    player.motion.body_height = if body_fits(player, desired, blocks) {
        desired
    } else if body_fits(player, CROUCHING_BODY, blocks) {
        CROUCHING_BODY
    } else {
        SWIMMING_BODY
    };
}

/// `Player.updateSwimming`：飞行时直接清掉，否则走 `Entity.updateSwimming` 的判定。
/// 已经在游泳的只要还在水里就保持，进入游泳要求眼睛也浸在水里。游泳状态按这一 tick
/// 结束时（也就是下一 tick 开始前）的位置算，实体阶段看到的就是官方 `baseTick`
/// 那一刻的值。
///
/// `Player.updateSwimming`: cleared while flying, otherwise the
/// `Entity.updateSwimming` test. A swimming player stays swimming while still in
/// water; entering requires the eye under water too. The flag is computed from the
/// position at the end of this tick (the start of the next one), which is what the
/// entity phase sees at vanilla's `baseTick` moment.
fn update_swimming(player: &mut Player, blocks: &PlayerBlocks<'_>) {
    let swimming = if player.sync.flying {
        false
    } else {
        let in_water = player_in_water(player, blocks);
        let sprinting = player.sync.is_sprinting();
        let passenger = player.riding.is_passenger();
        if player.motion.swimming {
            sprinting && in_water && !passenger
        } else {
            let feet = player.position();
            let eye = BlockPos::containing(Vec3::new(feet.x, feet.y + player.eye_height(), feet.z));
            sprinting && !passenger && in_water && fluid_is_water(blocks, eye)
        }
    };
    player.motion.swimming = swimming;
}

/// 玩家脚下方块（`blockPosition`）的流体是水。
///
/// The fluid of the player's feet block (`blockPosition`) is water.
fn player_in_water(player: &Player, blocks: &PlayerBlocks<'_>) -> bool {
    let feet = BlockPos::containing(player.position());
    fluid_is_water(blocks, feet)
}

/// 这一格的流体在 `#water` 里。
///
/// Whether the fluid in this cell is water.
fn fluid_is_water(blocks: &PlayerBlocks<'_>, pos: BlockPos) -> bool {
    blocks
        .state(pos)
        .fluid()
        .is_tagged(mc_bamboo_data::tags::fluid_tags::WATER)
}

/// 判断玩家的流体交互盒是否接触该类型的流体。
/// Whether the player's fluid interaction box touches fluid of this type.
pub(crate) fn touches_fluid(player: &Player, blocks: &PlayerBlocks<'_>, tag: FluidTag) -> bool {
    let aabb = movement_box(player).deflated(0.001);
    for x in mth::floor(aabb.min_x)..(aabb.max_x.ceil() as i32) {
        for z in mth::floor(aabb.min_z)..(aabb.max_z.ceil() as i32) {
            for y in mth::floor(aabb.min_y)..(aabb.max_y.ceil() as i32) {
                let pos = BlockPos::new(x, y, z);
                let fluid = blocks.state(pos).fluid();
                if !fluid.is_tagged(tag) {
                    continue;
                }
                let height = if blocks.state(pos.above()).fluid().is_tagged(tag) {
                    1.0
                } else {
                    f64::from(fluid.own_height())
                };
                if f64::from(y) + height >= aabb.min_y {
                    return true;
                }
            }
        }
    }
    false
}

fn body_fits(player: &Player, height: f64, blocks: &PlayerBlocks<'_>) -> bool {
    let box_ = body_box(player, height).deflated(1.0e-7);
    no_block_collision(
        box_,
        move_context(player),
        CollisionBlocks {
            block_at: |pos| blocks.state(pos),
            piston_at: |pos| blocks.piston(pos),
            shulker_at: |pos| blocks.shulker(pos),
        },
    )
}

/// 移动校验使用的当前姿势碰撞盒。
pub(crate) fn movement_box(player: &Player) -> Aabb {
    body_box(player, player.motion.body_height)
}

fn body_box(player: &Player, height: f64) -> Aabb {
    let feet = player.position();
    let half_width = if player.sync.is_fall_flying() || player.motion.is_fall_flying_pose() {
        f64::from(0.3_f32)
    } else {
        HALF_WIDTH
    };
    Aabb::new(
        feet.x - half_width,
        feet.y,
        feet.z - half_width,
        feet.x + half_width,
        feet.y + height,
        feet.z + half_width,
    )
}

fn box_at(player: &Player, feet: Vec3) -> Aabb {
    let delta = feet.subtract(player.position());
    movement_box(player).moved(delta.x, delta.y, delta.z)
}

pub(crate) fn step_height(player: &Player) -> f32 {
    player.attributes.value(attribute::STEP_HEIGHT) as f32
}

fn may_fly(mode: GameMode) -> bool {
    matches!(mode, GameMode::Creative | GameMode::Spectator)
}

fn clamp_position(position: Vec3) -> Vec3 {
    Vec3::new(
        position.x.clamp(-MAX_HORIZONTAL, MAX_HORIZONTAL),
        position.y.clamp(-MAX_VERTICAL, MAX_VERTICAL),
        position.z.clamp(-MAX_HORIZONTAL, MAX_HORIZONTAL),
    )
}

fn block_state(chunks: &ChunkStore, pos: BlockPos) -> BlockState {
    BlockState(chunks.block(pos).unwrap_or(blocks::VOID_AIR))
}

/// 玩家移动校验用到的只读方块视图。除了区块表还要带方块实体表：`moving_piston`
/// 是 `dynamicShape`，碰撞形状要从这一格的 [`PistonMoving`] 现算。
///
/// The read-only block view for player movement checks. Besides the chunk store it
/// carries the block-entity table: `moving_piston` is a dynamic shape whose collision
/// shape comes from the [`PistonMoving`] at that cell.
pub(crate) struct PlayerBlocks<'a> {
    chunks: &'a ChunkStore,
    entities: &'a BlockEntities,
}

impl PlayerBlocks<'_> {
    pub(crate) fn new<'a>(chunks: &'a ChunkStore, entities: &'a BlockEntities) -> PlayerBlocks<'a> {
        PlayerBlocks { chunks, entities }
    }

    pub(crate) fn state(&self, pos: BlockPos) -> BlockState {
        block_state(self.chunks, pos)
    }

    pub(crate) fn piston(&self, pos: BlockPos) -> Option<Vec<Aabb>> {
        let entity = self.entities.find_dyn(pos)?;
        entity
            .as_any()
            .downcast_ref::<PistonMoving>()
            .map(PistonMoving::collision_shape)
    }

    pub(crate) fn shulker(&self, pos: BlockPos) -> Option<Aabb> {
        let entity = self.entities.find_dyn(pos)?;
        entity
            .as_any()
            .downcast_ref::<ShulkerBoxEntity>()
            .map(|shulker| shulker.bounding_box(self.state(pos)))
    }
}

/// 一次和官方 `handleMovePlayer` 对照的样本。调用方给出和官方相同的起点、方块和移动包。
///
/// One sample compared with official `handleMovePlayer`. The caller supplies the same start,
/// blocks, and move packets the official listener saw.
pub struct MoveReplay {
    pub mode: GameMode,
    pub shift: bool,
    pub sprint: bool,
    pub flying: bool,
    pub loaded: bool,
    pub blocks: Vec<(BlockPos, u32)>,
    pub start: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    /// 脚上穿着皮革靴，细雪的碰撞形状要用。
    pub boots: bool,
    /// 场景开始时挂着的 (效果注册表 ID, 等级)；移动校验的漂浮 / 摔落 /
    /// 跳跃判定按官方读这些效果。
    ///
    /// The (effect id, amplifier) pairs active at the start. The movement
    /// check reads these for levitation, slow falling and jump boost.
    pub effects: Vec<(i32, i32)>,
    pub packets: Vec<MoveReplayPacket>,
}

/// 样本里的一个包，或者一个没有包的服务端 tick。
///
/// One packet in the sample, or a server tick with no packet.
pub enum MoveReplayPacket {
    Move {
        position: Option<Vec3>,
        rotation: Option<(f32, f32)>,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// 装备鞘翅，再发送开始滑翔命令。
    /// Equip elytra and send the start-gliding command.
    StartGlide,
    /// 对拍样本预置服务端速度。
    /// Seed the server's velocity in the replay sample.
    Velocity(Vec3),
    TickEnd,
    ServerTick,
}

/// 整串包跑完之后的结果。`kick` 优先于中途的拉回。
///
/// The result after the whole sequence. `kick` wins over an earlier rubber-band.
pub struct MoveReplayReport {
    pub outcome: &'static str,
    pub position: Vec3,
    pub kicked: bool,
    pub teleports: u32,
    pub fall_distance: f64,
    /// 样本结束时的共享滑翔状态。
    /// Shared glide state at the end of the sample.
    pub fall_flying: bool,
    /// 样本结束时的生命值。
    /// Health at the end of the sample.
    pub health: f32,
    /// 这一串包累计的饥饿消耗（`causeFoodExhaustion`，创造模式为 0）。
    pub exhaustion: f32,
    /// 落地场景的方块状态和每个收件人的原始出站包。
    pub landing: Option<MoveLandingReport>,
}

/// 落地场景的实测方块与包体。
/// Observed block states and packet bodies for a landing case.
pub struct MoveLandingReport {
    pub blocks: Vec<u32>,
    pub player_packets: Vec<Vec<u8>>,
    pub observer_packets: Vec<Vec<u8>>,
}

/// 按官方的顺序重放一串移动包：先处理包，服务端 tick 再收尾。
///
/// Replays a move sequence in the official order: packets first, then the server tick epilogue.
pub fn replay_movement(sample: &MoveReplay) -> MoveReplayReport {
    replay_moves(sample, None)
}

/// 重放落地场景，并记录落地玩家和观察者实际收到的包。
/// Replays a landing scene and captures packets received by the falling player and observer.
pub fn replay_landing(sample: &MoveReplay, fall_damage: bool) -> MoveReplayReport {
    replay_moves(sample, Some(fall_damage))
}

fn replay_moves(sample: &MoveReplay, landing_fall_damage: Option<bool>) -> MoveReplayReport {
    let mut region = Region::new(8, 20, false);
    let sink = ReplaySink::default();
    let id = PlayerId(1);
    region.links.add(id, Box::new(sink.clone()));
    let mut player = Player::new(
        id,
        Uuid::nil(),
        "Oracle".to_owned(),
        sample.mode,
        ClientInformation::default(),
        sample.start,
    );
    player.yaw = sample.yaw;
    player.pitch = sample.pitch;
    player.on_ground = sample.on_ground;
    player.sync.shift_key_down = sample.shift;
    if sample.boots {
        player.inventory.slots[8] = ItemStack::new(items::LEATHER_BOOTS, 1);
    }
    player.sync.flying = sample.flying;
    if sample.sprint {
        player.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
    }
    for &(effect, amplifier) in &sample.effects {
        player.effects.add(
            mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_amplifier(
                effect,
                i32::MAX / 2,
                amplifier,
            ),
        );
    }
    crate::player_attributes::rebuild_player(&mut player, false);
    region.insert_player(player);
    for (pos, state) in &sample.blocks {
        region.chunks.set_block(*pos, *state);
    }
    if sample.loaded {
        on_player_loaded(&mut region, id);
    }
    // 官方在第一个 `tickPlayer` 之后 `tickCount` 才离开 0。样本从这一刻开始。
    //
    // Official `tickCount` leaves 0 after the first `tickPlayer`. The sample starts there.
    region.advance_tick();
    finish_tick(&mut region);

    let landing_observer = landing_fall_damage.map(|fall_damage| {
        std::sync::Arc::make_mut(&mut region.global.rules).set_saved(
            gamerules::FALL_DAMAGE,
            crate::gamerules::Stored::Bool(fall_damage),
        );
        let observer_id = PlayerId(2);
        let observer_sink = ReplaySink::default();
        let mut observer = Player::new(
            observer_id,
            Uuid::from_u128(2),
            "Observer".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            sample.start.add(Vec3::new(2.0, 0.0, 0.0)),
        );
        let center = observer.chunk_pos();
        observer.chunks.set_view_for_test(center, 8);
        if let Some(player) = region.players.get_mut(&id) {
            player.chunks.set_view_for_test(player.chunk_pos(), 8);
        }
        region
            .links
            .add(observer_id, Box::new(observer_sink.clone()));
        region.insert_player(observer);
        for player in region.players.values_mut() {
            player.health_sync = crate::player_hurt::HealthSync::of(player);
        }
        crate::entity_sync::sync_entities(&mut region);
        region.links.flush();
        sink.clear();
        observer_sink.clear();
        region.random = mc_bamboo_blocks::LevelRandom::new(1);
        region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);
        observer_sink
    });

    for packet in &sample.packets {
        if !region.links.is_open(id) {
            break;
        }
        match packet {
            MoveReplayPacket::Move {
                position,
                rotation,
                on_ground,
                horizontal_collision,
            } => on_move_player(
                &mut region,
                id,
                *position,
                *rotation,
                *on_ground,
                *horizontal_collision,
            ),
            MoveReplayPacket::StartGlide => {
                if let Some(player) = region.players.get_mut(&id)
                    && let Some(chest) = player.inventory.equipment_mut(EquipmentSlot::Chest)
                {
                    *chest = ItemStack::new(items::ELYTRA, 1);
                }
                crate::entity_sync::on_player_command(
                    &mut region,
                    id,
                    mc_bamboo_protocol::packet::play::PlayerCommandKind::StartFallFlying,
                    0,
                );
            }
            MoveReplayPacket::Velocity(velocity) => {
                if let Some(player) = region.players.get_mut(&id) {
                    player.motion.delta = *velocity;
                }
            }
            MoveReplayPacket::TickEnd => on_client_tick_end(&mut region, id),
            MoveReplayPacket::ServerTick => {
                region.advance_tick();
                crate::player_glide::tick(&mut region);
                finish_tick(&mut region);
            }
        }
    }
    if landing_observer.is_some() {
        crate::player_hurt::tick(&mut region);
        crate::chunk_broadcast::broadcast_changes(&mut region);
    }
    region.links.flush();
    let teleports = sink.teleports();
    let kicked = !region.links.is_open(id);
    let player = region.players.get(&id).expect("replay player");
    MoveReplayReport {
        outcome: if kicked {
            "kick"
        } else if teleports > 0 {
            "rubber"
        } else {
            "accept"
        },
        position: player.position(),
        kicked,
        teleports,
        fall_distance: player.motion.fall_distance,
        fall_flying: player.sync.is_fall_flying(),
        health: player.health.health,
        exhaustion: player.pending_exhaustion,
        landing: landing_observer.map(|observer| MoveLandingReport {
            blocks: sample
                .blocks
                .iter()
                .map(|(pos, _)| region.chunks.block(*pos).unwrap_or(blocks::VOID_AIR))
                .collect(),
            player_packets: sink.packets(),
            observer_packets: observer.packets(),
        }),
    }
}

#[derive(Default)]
struct ReplayBytes {
    bytes: Vec<u8>,
}

#[derive(Clone, Default)]
struct ReplaySink(std::sync::Arc<std::sync::Mutex<ReplayBytes>>);

impl PlayerSink for ReplaySink {
    fn send(&mut self, frames: Outgoing) -> bool {
        self.0
            .lock()
            .expect("replay sink")
            .bytes
            .extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {}
}

impl ReplaySink {
    fn clear(&self) {
        self.0.lock().expect("replay sink").bytes.clear();
    }

    fn packets(&self) -> Vec<Vec<u8>> {
        let locked = self.0.lock().expect("replay sink");
        let mut rest = locked.bytes.as_slice();
        let mut packets = Vec::new();
        while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).expect("replay frame") {
            let end = bounds.header_len + bounds.payload_len;
            packets.push(rest[bounds.header_len..end].to_vec());
            rest = &rest[end..];
        }
        packets
    }

    fn teleports(&self) -> u32 {
        self.packets()
            .iter()
            .filter(|packet| Reader::new(packet).varint().expect("packet id") == PlayerPosition::ID)
            .count() as u32
    }
}

#[cfg(test)]
mod exhaustion_tests {
    use super::*;
    use mc_bamboo_data::blocks;

    /// 用 `replay_movement` 走一段位移，方块表只放脚下 / 眼睛那一格。
    fn exhaust(state: u32, at: BlockPos, sprint: bool, creative: bool) -> f32 {
        let sample = MoveReplay {
            mode: if creative {
                GameMode::Creative
            } else {
                GameMode::Survival
            },
            shift: false,
            sprint,
            flying: false,
            loaded: true,
            blocks: vec![(at, state)],
            start: Vec3::new(0.5, 80.0, 0.5),
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            boots: false,
            effects: Vec::new(),
            packets: vec![MoveReplayPacket::Move {
                position: Some(Vec3::new(1.5, 80.0, 0.5)),
                rotation: None,
                on_ground: false,
                horizontal_collision: false,
            }],
        };
        replay_movement(&sample).exhaustion
    }

    #[test]
    fn water_and_climbable_movements_use_the_official_coefficients() {
        let feet = BlockPos::new(1, 80, 0);
        // 眼睛在水里：1 格 = 100 cm -> 0.01 * 100 * 0.01 = 0.01。
        let eye = BlockPos::new(1, 81, 0);
        let wet = exhaust(blocks::WATER, eye, false, false);
        assert!((wet - 0.01).abs() < 1.0e-6, "eye in water: {wet}");
        // 身体在水里同样按水平位移算。
        let body = exhaust(blocks::WATER, feet, false, false);
        assert!((body - 0.01).abs() < 1.0e-6, "in water: {body}");
        // 梯子上只发统计、不加饥饿消耗。
        let ladder = exhaust(blocks::LADDER, feet, false, false);
        assert_eq!(ladder, 0.0);
        // 创造模式整段跳过。
        assert_eq!(exhaust(blocks::WATER, eye, false, true), 0.0);
        // 改坏确认：眼睛在水里时按普通地面疾跑算会变成 0.1。
        assert_ne!(exhaust(blocks::WATER, eye, true, false), 0.1);
    }
}
