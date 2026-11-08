//! 方块交互的网络一侧：够距、点击点、序列号和确认、高度提示、把当前的方块发回去。
//!
//! 方块的逻辑都在 `mc_bamboo_blocks` 里，这里只把玩家的动作交给它，再把结果发给客户端。
//! M2 只有创造模式：破坏一步完成，放置不扣物品。
//! 客户端在本地预测这些操作，结果靠 `block_update` 和 `block_changed_ack` 收敛到服务端。

pub(crate) mod level;
pub(crate) mod rules;

use std::mem;

use mc_bamboo_blocks::{Actor, BlockHit, Effect, InteractionResult, Level};
use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::{
    packet::play::{BlockChangedAck, BlockUpdate, LevelEvent, PlayerActionKind, SystemChat},
    text::{Color, Component},
};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use tracing::debug;

use crate::{
    entity_sync,
    global::GameTime,
    inbound::PlayerId,
    inventory,
    links::{Links, encode_frames},
    movement,
    player::Player,
    region::Region,
};

const MAX_Y: i32 = MIN_Y + WORLD_HEIGHT - 1;
/// 创造模式的 `block_interaction_range`：基础值 4.5 加修饰符 0.5。
const BLOCK_INTERACTION_RANGE: f64 = 5.0;
/// 官方判定时多给的一格，服务端记录的位置可能比客户端晚一点。
const REACH_BUFFER: f64 = 1.0;
/// 点击点离方块中心的最大偏差，官方的比较是严格小于。
const CURSOR_LIMIT: f64 = 1.000_000_1;
/// `level_event` 能传多远，比较的是方块的整数坐标和玩家的位置。
const LEVEL_EVENT_RADIUS: f64 = 64.0;

const NO_SEQUENCE: i32 = -1;

const TOO_HIGH: &str = "build.tooHigh";
const TOO_LOW: &str = "build.tooLow";
/// 客户端语言文件里的“网络协议错误”。`multiplayer.disconnect.invalid_packet` 写的是服务端发错了包。
const PACKET_ERROR: &str = "disconnect.packetError";

/// 一名玩家等着确认的预测。`blocks-inventory.md` 4.3。
#[derive(Debug)]
pub(crate) struct BlockPrediction {
    /// 这一 tick 见过的最大 sequence，-1 表示没有。
    pub(crate) pending_ack: i32,
}

impl Default for BlockPrediction {
    fn default() -> Self {
        Self {
            pending_ack: NO_SEQUENCE,
        }
    }
}

/// 这一 tick 改过的方块，第 5 阶段广播后清空。`blocks-inventory.md` 第 11 章。
#[derive(Debug, Default)]
pub(crate) struct BlockChanges {
    pub(crate) changed: Vec<BlockPos>,
}

/// `use_item_on` 的内容，字段的含义见 [`PlayRequest::UseItemOn`](mc_bamboo_protocol::packet::play::PlayRequest::UseItemOn)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UseItemOn {
    pub(crate) hand: Hand,
    pub(crate) pos: BlockPos,
    pub(crate) face: Direction,
    pub(crate) cursor: (f32, f32, f32),
    pub(crate) inside: bool,
    pub(crate) sequence: i32,
}

/// tick 第 2 阶段之后：到期的计划刻。官方 `ServerLevel.tick` 在时间加一之后、区块广播之前跑它们。
pub(crate) fn run_block_ticks(region: &mut Region, time: GameTime) {
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_scheduled_ticks(&mut level, ticks, time.0);
    send_effects(region, None, &outcome.effects);
}

/// tick 第 5 阶段：把改过的方块发给 `is_chunk_tracked` 的玩家。`blocks-inventory.md` 第 11 章。
///
/// 区块还在 `pending` 里的玩家不用发，它们收到的区块就是最新内容。
pub(crate) fn broadcast_block_changes(region: &mut Region) {
    let changed = &mut region.block_changes.changed;
    // 同一格这一 tick 改了几次时，只有最后的状态要发。
    changed.sort_unstable();
    changed.dedup();
    for pos in changed.drain(..) {
        let Some(state) = region.chunks.block(pos) else {
            continue;
        };
        let frames = encode_frames(&block_update(pos, state));
        let chunk = pos.chunk();
        for player in region.players.values() {
            if player.chunks.is_chunk_tracked(chunk) {
                region.links.send_frames(player.id, &frames);
            }
        }
    }
}

/// tick 第 8 阶段：每名玩家发 `block_changed_ack`。必须晚于第 5 阶段，`blocks-inventory.md` 4.3。
///
/// 一个包确认到最大的 sequence 为止的所有预测，所以每 tick 最多发一个。
pub(crate) fn send_block_changed_acks(region: &mut Region) {
    for player in region.players.values_mut() {
        let sequence = mem::replace(&mut player.prediction.pending_ack, NO_SEQUENCE);
        if sequence != NO_SEQUENCE {
            region.links.send(player.id, &BlockChangedAck { sequence });
        }
    }
}

/// 记下要确认的 sequence。拒绝操作时也要记，客户端的预测靠它结束。
///
/// 负数是畸形包，客户端不会发：踢掉发送者并返回 `false`，调用方要丢弃这个包。
fn record_sequence(region: &mut Region, id: PlayerId, sequence: i32) -> bool {
    if sequence < 0 {
        region
            .links
            .kick(id, &Component::translate(PACKET_ERROR, []));
        return false;
    }
    if let Some(player) = region.players.get_mut(&id) {
        player.prediction.pending_ack = player.prediction.pending_ack.max(sequence);
    }
    true
}

/// 方块逻辑看到的这名玩家。
fn actor_of(player: &Player) -> Actor {
    let held = |hand| {
        let stack = player.inventory.held(hand);
        if stack.is_empty() {
            items::AIR
        } else {
            stack.item
        }
    };
    Actor {
        position: player.position,
        yaw: player.yaw,
        head_yaw: player.head_yaw,
        pitch: player.pitch,
        // 官方看的是按键，不是姿势，所以飞行时按着潜行键也算。
        secondary_use: player.sync.shift_key_down,
        main_hand: held(Hand::Main),
        off_hand: held(Hand::Off),
        eye_height: player.eye_height(),
        block_interaction_range: BLOCK_INTERACTION_RANGE,
    }
}

/// 玩家能不能够到这个方块。`blocks-inventory.md` 6.1。
pub(crate) fn within_reach(player: &Player, pos: BlockPos) -> bool {
    let Vec3 { x, y, z } = player.position;
    // 眼睛到方块单位立方体的距离。
    let axis = |low: i32, eye: f64| {
        let low = f64::from(low);
        (low - eye).max(eye - (low + 1.0)).max(0.0)
    };
    let dx = axis(pos.x, x);
    let dy = axis(pos.y, y + player.eye_height());
    let dz = axis(pos.z, z);
    let range = BLOCK_INTERACTION_RANGE + REACH_BUFFER;
    dx * dx + dy * dy + dz * dz < range * range
}

/// `player_action`：破坏方块、丢物品、交换主副手。`blocks-inventory.md` 3.1、第 5 章、8.3。
///
/// 只有四个破坏动作带真实的 sequence，其余动作不记 ack。
pub(crate) fn on_player_action(
    region: &mut Region,
    id: PlayerId,
    action: PlayerActionKind,
    pos: BlockPos,
    sequence: i32,
) {
    match action {
        PlayerActionKind::StartDestroyBlock => {
            if record_sequence(region, id, sequence) {
                destroy_block(region, id, pos);
            }
        }
        // 创造模式不发这三个，收到了也只结束预测。
        PlayerActionKind::ChangeDestroyDirection
        | PlayerActionKind::AbortDestroyBlock
        | PlayerActionKind::StopDestroyBlock => {
            record_sequence(region, id, sequence);
        }
        PlayerActionKind::DropAllItems => inventory::drop_held_item(region, id, true),
        PlayerActionKind::DropItem => inventory::drop_held_item(region, id, false),
        PlayerActionKind::SwapItemWithOffhand => inventory::swap_hands(region, id),
        PlayerActionKind::ReleaseUseItem | PlayerActionKind::Stab => {}
    }
}

/// 创造模式破坏方块，官方 `handleBlockBreakAction` 和 `destroyBlock` 的顺序。
/// `blocks-inventory.md` 5.2。
///
/// 够不到时什么也不发，靠 ack 回滚客户端的预测。
fn destroy_block(region: &mut Region, id: PlayerId, pos: BlockPos) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if !within_reach(player, pos) {
        return;
    }
    if pos.y > MAX_Y {
        region.links.send(id, &block_update(pos, blocks::VOID_AIR));
        return;
    }
    if !rules::can_destroy(player.inventory.held(Hand::Main)) {
        send_current_block(region, id, pos);
        return;
    }

    let actor = actor_of(player);
    let (mut level, ticks) = region.block_level();
    // 世界下面读到的是 `void_air`。破坏空气世界不变，官方还会发一个客户端不看的 2001，这里省掉。
    if level.block_state(pos).is_air() {
        return;
    }
    let outcome = mc_bamboo_blocks::destroy_block(&mut level, ticks, &actor, pos);
    send_effects(region, Some(id), &outcome.effects);
    if outcome.result == InteractionResult::Fail {
        send_current_block(region, id, pos);
    }
}

/// 把方块逻辑留下的结果发出去。`initiator` 是发起动作的玩家，计划刻没有发起者。
fn send_effects(region: &mut Region, initiator: Option<PlayerId>, effects: &[Effect]) {
    for effect in effects {
        match *effect {
            Effect::Destroyed {
                pos,
                state,
                except_initiator,
            } => {
                let except = initiator.filter(|_| except_initiator);
                let event = LevelEvent::destroy_block(pos, state.0 as i32);
                send_level_event(region, except, &event);
            }
            Effect::TorchBurnout { pos } => {
                send_level_event(region, None, &LevelEvent::redstone_torch_burnout(pos));
            }
            Effect::FilledBucket { item } => {
                if let Some(id) = initiator {
                    inventory::add_filled_bucket(region, id, item);
                }
            }
        }
    }
}

/// 世界事件的粒子和声音发给附近的人。`except` 是不用发的玩家，它自己已经在本地预测过了。
fn send_level_event(region: &mut Region, except: Option<PlayerId>, event: &LevelEvent) {
    let frames = encode_frames(event);
    for player in region.players.values() {
        if Some(player.id) != except && hears_level_event(player, event.pos) {
            region.links.send_frames(player.id, &frames);
        }
    }
}

fn hears_level_event(player: &Player, pos: BlockPos) -> bool {
    let Vec3 { x, y, z } = player.position;
    let dx = f64::from(pos.x) - x;
    let dy = f64::from(pos.y) - y;
    let dz = f64::from(pos.z) - z;
    dx * dx + dy * dy + dz * dz < LEVEL_EVENT_RADIUS * LEVEL_EVENT_RADIUS
}

fn block_update(pos: BlockPos, state: u32) -> BlockUpdate {
    BlockUpdate {
        pos,
        state: state as i32,
    }
}

/// `use_item_on`，官方 `handleUseItemOn` 的顺序。`blocks-inventory.md` 3.1、第 6 章。
pub(crate) fn on_use_item_on(region: &mut Region, id: PlayerId, packet: UseItemOn) {
    if !record_sequence(region, id, packet.sequence) {
        return;
    }
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let UseItemOn {
        pos, face, cursor, ..
    } = packet;
    if !within_reach(player, pos) {
        return;
    }
    if !cursor_is_valid(cursor) {
        debug!(
            player = id.0,
            ?pos,
            ?cursor,
            "the click is too far from the block"
        );
        return;
    }
    if pos.y > MAX_Y {
        send_build_limit(&mut region.links, id, TOO_HIGH, MAX_Y);
        return;
    }
    if pos.y < MIN_Y {
        send_build_limit(&mut region.links, id, TOO_LOW, MIN_Y);
        return;
    }

    // 客户端可能还没收到传送，这时的点击不能信。
    if !player.teleport.is_awaiting() {
        use_held_item(region, id, &packet);
    }

    // 无论结果如何都把这两格发回去，放置失败时客户端靠它们和 ack 回滚。
    send_current_block(region, id, pos);
    send_current_block(region, id, pos.relative(face));
}

/// 三个轴的偏差都在范围内才算。写成“全部小于”，所以 NaN 也不通过。
fn cursor_is_valid((x, y, z): (f32, f32, f32)) -> bool {
    [x, y, z]
        .into_iter()
        .all(|value| (f64::from(value) - 0.5).abs() < CURSOR_LIMIT)
}

/// 世界外的位置不用发：官方发的是 `void_air`，客户端也写不进去。
fn send_current_block(region: &mut Region, id: PlayerId, pos: BlockPos) {
    if let Some(state) = region.chunks.block(pos) {
        region.links.send(id, &block_update(pos, state));
    }
}

/// 快捷栏上方的红字提示。`blocks-inventory.md` 6.6。
fn send_build_limit(links: &mut Links, id: PlayerId, key: &str, limit: i32) {
    let content = Component::translate(key, [limit.into()]).color(Color::Red);
    let packet = SystemChat {
        content: &content,
        overlay: true,
    };
    links.send(id, &packet);
}

/// 把右键交给方块逻辑，再按结果提示和挥手。
fn use_held_item(region: &mut Region, id: PlayerId, packet: &UseItemOn) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let actor = actor_of(player);
    let is_placement_attempt = rules::is_placement_attempt(player.inventory.held(packet.hand));

    let (mut level, ticks) = region.block_level();
    let hit = block_hit(packet);
    let outcome = mc_bamboo_blocks::use_item_on(&mut level, ticks, &actor, packet.hand, &hit);

    send_effects(region, Some(id), &outcome.effects);
    if is_placement_attempt && !outcome.result.consumes_action() {
        send_limit_hint(&mut region.links, id, packet);
    }
    swing(region, id, packet.hand, outcome.result);
}

/// `BlockHitResult`。点击点和官方解码时一样，是整数坐标加上 `f32` 的偏移。
fn block_hit(packet: &UseItemOn) -> BlockHit {
    let UseItemOn {
        pos,
        face,
        cursor: (x, y, z),
        inside,
        ..
    } = *packet;
    let location = Vec3::new(
        f64::from(pos.x) + f64::from(x),
        f64::from(pos.y) + f64::from(y),
        f64::from(pos.z) + f64::from(z),
    );
    BlockHit {
        pos,
        face,
        location,
        inside,
    }
}

/// 放置尝试没有成功，又正好点在世界的边界上。`blocks-inventory.md` 6.6。
fn send_limit_hint(links: &mut Links, id: PlayerId, packet: &UseItemOn) {
    match packet.face {
        Direction::Up if packet.pos.y >= MAX_Y => send_build_limit(links, id, TOO_HIGH, MAX_Y),
        Direction::Down if packet.pos.y <= MIN_Y => send_build_limit(links, id, TOO_LOW, MIN_Y),
        _ => {}
    }
}

/// 官方看结果的 `swingSource`：`SUCCESS` 客户端已经挥过了，`SUCCESS_SERVER` 没有，`CONSUME` 不挥手。
/// 官方在两个分支里各调用一次，客户端的动画不分一次两次，这里只发一次。
fn swing(region: &mut Region, id: PlayerId, hand: Hand, result: InteractionResult) {
    match result {
        InteractionResult::Success => entity_sync::broadcast_swing(region, id, hand),
        InteractionResult::SuccessServer => {
            entity_sync::broadcast_swing_with_self(region, id, hand);
        }
        _ => {}
    }
}

/// `use_item`：对着空气或者水用手里的物品，官方 `handleUseItem` 的顺序。
pub(crate) fn on_use_item(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    sequence: i32,
    (yaw, pitch): (f32, f32),
) {
    if !record_sequence(region, id, sequence) {
        return;
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.inventory.held(hand).is_empty() {
        return;
    }
    // 视线找水面用的是包里的朝向，不是上一次移动包的。
    movement::snap_rotation(player, yaw, pitch);
    let actor = actor_of(player);

    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::use_item(&mut level, ticks, &actor, hand);

    send_effects(region, Some(id), &outcome.effects);
    swing(region, id, hand, outcome.result);
}

#[cfg(test)]
mod tests;
