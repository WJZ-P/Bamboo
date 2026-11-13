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
    self, TravelSegment, enclosed_in_air, no_block_collision, overlaps_new_blocks, sweep_movement,
};
use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockState, CollisionContext, EntityContext, PistonMoving,
    ShulkerBoxEntity, world::CollisionBlocks,
};
use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{blocks, items, registries::attribute_defaults};
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
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
    /// `Entity.fallDistance`。只记录，不结算伤害。
    ///
    /// `Entity.fallDistance`. Recorded, and not turned into damage.
    pub(crate) fall_distance: f64,
    tick_count: i32,
    received_move_packets: i32,
    known_move_packets: i32,
    received_position_this_tick: bool,
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
            client_is_floating: false,
            above_ground_ticks: 0,
            client_loaded_timeout: CLIENT_LOAD_TIMEOUT,
            vertical_collision_below: false,
            stuck_speed: Vec3::default(),
            no_physics: false,
            body_height: STANDING_BODY,
            client_horizontal_collision: false,
            pending_inside: Vec::new(),
        }
    }

    /// 客户端发了 `player_loaded`，或者加载超时已经走完。
    ///
    /// The client sent `player_loaded`, or the load timeout has elapsed.
    pub(crate) fn client_loaded(&self) -> bool {
        self.client_loaded_timeout <= 0
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
    let border = region.world_border.clone();
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
    let reported = position.unwrap_or(player.position());
    let (yaw, pitch) = rotation.unwrap_or((player.yaw, player.pitch));
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
    apply_change(
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
        },
        &sample,
        &border,
    );
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
    let border = region.world_border.clone();
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
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
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
        player.motion.above_ground_ticks > maximum_flying_ticks()
    } else {
        player.motion.client_is_floating = false;
        player.motion.above_ground_ticks = 0;
        false
    }
}

/// `getMaximumFlyingTicks`。重力用属性默认值。
///
/// `getMaximumFlyingTicks`. Gravity is the attribute default.
fn maximum_flying_ticks() -> i32 {
    let gravity = attribute_defaults::GRAVITY;
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
) {
    let change = Change {
        target: clamp_position(attempt.target),
        yaw: attempt.yaw,
        pitch: attempt.pitch,
        on_ground: attempt.on_ground,
        horizontal_collision: attempt.horizontal_collision,
        blocks,
    };
    if reject_speed(player, &change) {
        movement::teleport(player, links, grid, now, player.position());
        return;
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
        record_fall(&mut player.motion.fall_distance, 0.0, change.on_ground);
        return;
    }

    player.set_position(grid, change.target);
    set_rotation(player, change.yaw, change.pitch);
    player.motion.client_is_floating = floating_now(player, from_last.y, stands, change.blocks);
    player.on_ground = change.on_ground;
    player.motion.client_horizontal_collision = change.horizontal_collision;
    let client_delta = change.target.subtract(start);
    record_fall(
        &mut player.motion.fall_distance,
        client_delta.y,
        change.on_ground,
    );
    if moved_up {
        player.motion.fall_distance = 0.0;
    }
    remember_inside(player, start, end, simulated.attempted, change.target);
    player.motion.last_good = player.position();
}

fn reject_speed(player: &mut Player, change: &Change<'_>) -> bool {
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
    let offset = change.target.subtract(player.motion.first_good);
    let moved = offset.length_sqr();
    let expected = player.motion.delta.length_sqr();
    let limit = f64::from(METRES_PER_TICK) * f64::from(packets);
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
        step_height(),
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
        && enclosed_in_air(movement_box(player), &mut |pos| blocks.state(pos))
}

fn jump_from_ground(player: &mut Player) {
    let power = attribute_defaults::JUMP_STRENGTH as f32;
    if power <= 1.0e-5 {
        return;
    }
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
    let max_down = step_height();
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

fn record_fall(fall: &mut f64, ya: f64, on_ground: bool) {
    if ya < 0.0 {
        *fall -= f64::from(ya as f32);
    }
    if on_ground {
        *fall = 0.0;
    }
}

fn block_speed_factor(player: &Player, position: Vec3, blocks: &PlayerBlocks<'_>) -> f32 {
    // 飞行时 `Player.getBlockSpeedFactor` 直接是 1，不读脚下的方块。
    //
    // While flying, `Player.getBlockSpeedFactor` is 1 and ignores the block underfoot.
    if player.sync.flying {
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

fn move_context(player: &Player) -> CollisionContext {
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
fn update_body(player: &mut Player, blocks: &PlayerBlocks<'_>) {
    if !body_fits(player, SWIMMING_BODY, blocks) {
        return;
    }
    let want_crouch = player.sync.shift_key_down && !player.sync.flying;
    let desired = if want_crouch {
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

fn body_fits(player: &Player, height: f64, blocks: &PlayerBlocks<'_>) -> bool {
    let box_ = body_box(player.position(), height).deflated(1.0e-7);
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

fn movement_box(player: &Player) -> Aabb {
    body_box(player.position(), player.motion.body_height)
}

fn body_box(feet: Vec3, height: f64) -> Aabb {
    Aabb::new(
        feet.x - HALF_WIDTH,
        feet.y,
        feet.z - HALF_WIDTH,
        feet.x + HALF_WIDTH,
        feet.y + height,
        feet.z + HALF_WIDTH,
    )
}

fn box_at(player: &Player, feet: Vec3) -> Aabb {
    let delta = feet.subtract(player.position());
    movement_box(player).moved(delta.x, delta.y, delta.z)
}

fn step_height() -> f32 {
    attribute_defaults::STEP_HEIGHT as f32
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
struct PlayerBlocks<'a> {
    chunks: &'a ChunkStore,
    entities: &'a BlockEntities,
}

impl PlayerBlocks<'_> {
    fn new<'a>(chunks: &'a ChunkStore, entities: &'a BlockEntities) -> PlayerBlocks<'a> {
        PlayerBlocks { chunks, entities }
    }

    fn state(&self, pos: BlockPos) -> BlockState {
        block_state(self.chunks, pos)
    }

    fn piston(&self, pos: BlockPos) -> Option<Vec<Aabb>> {
        let entity = self.entities.find_dyn(pos)?;
        entity
            .as_any()
            .downcast_ref::<PistonMoving>()
            .map(PistonMoving::collision_shape)
    }

    fn shulker(&self, pos: BlockPos) -> Option<Aabb> {
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
}

/// 按官方的顺序重放一串移动包：先处理包，服务端 tick 再收尾。
///
/// Replays a move sequence in the official order: packets first, then the server tick epilogue.
pub fn replay_movement(sample: &MoveReplay) -> MoveReplayReport {
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
    player.sync.flying = sample.flying;
    if sample.sprint {
        player.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
    }
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
            MoveReplayPacket::TickEnd => on_client_tick_end(&mut region, id),
            MoveReplayPacket::ServerTick => {
                region.advance_tick();
                finish_tick(&mut region);
            }
        }
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
    fn teleports(&self) -> u32 {
        let bytes = self.0.lock().expect("replay sink").bytes.clone();
        let mut rest = bytes.as_slice();
        let mut count = 0;
        while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).expect("replay frame") {
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            if reader.varint().expect("packet id") == PlayerPosition::ID {
                count += 1;
            }
            rest = &rest[end..];
        }
        count
    }
}
