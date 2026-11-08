//! 生存模式挖方块的进度，官方 `ServerPlayerGameMode` 里破坏方块的那一半。
//!
//! 一个玩家一份状态 [`Mining`]，`tick_mining` 每 tick 推进一次。开始挖、换方向、停下和放弃
//! 这四种动作都走 [`handle_block_break_action`]，和官方同名。
//!
//! 掉落物、经验、工具耐久和统计都还没有：官方的 `destroyBlock` 在 `removeBlock` 之后调用
//! `Block.playerDestroy`（`Block.dropResources`）和 `popExperience`，这里在
//! [`destroy_block`] 里留了注释标出位置。

use mc_bamboo_blocks::{
    Actor, BlockState, InteractionResult, Level,
    mining::{self as block_mining, Miner},
    world::is_within_world_border,
};
use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::packet::play::{
    BlockDestruction, BlockUpdate, GameMode, LevelEvent, PlayerActionKind,
};
use tracing::{debug, warn};

use super::{
    MAX_Y, REACH_BUFFER, actor_of, rules, send_current_block, send_effects, send_level_event,
    within_block_interaction_range,
};
use crate::{inbound::PlayerId, links::encode_frames, region::Region};

/// `LevelEvent.PARTICLES_DESTROY_PROGRESS`：挖到一半的裂纹和声音。
const DESTROY_PROGRESS_EVENT: i32 = 2019;
/// `LevelEvent.PARTICLES_AND_SOUND_DESTROY_PROGRESS`：每四刻响一次。
const DESTROY_PROGRESS_SOUND_EVENT: i32 = 2020;
/// `ServerLevel.destroyBlockProgress` 发包的距离，比较的是平方。
const DESTROY_PROGRESS_RADIUS_SQR: f64 = 1024.0;

/// 官方 `ServerPlayerGameMode` 里和挖掘有关的字段，默认值和官方一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mining {
    /// `isDestroyingBlock`。
    is_destroying: bool,
    /// `destroyProgressStart`：开始挖那一刻的 `gameTicks`。
    progress_start: i64,
    /// `destroyPos = BlockPos.ZERO`。
    pos: BlockPos,
    /// `destroyDirection = Direction.DOWN`。
    direction: Direction,
    /// `hasDelayedDestroy`。
    has_delayed: bool,
    /// `delayedDestroyPos = BlockPos.ZERO`。
    delayed_pos: BlockPos,
    /// `delayedTickStart`。停下但没挖穿时记的是 `destroyProgressStart`，不是当前刻。
    delayed_start: i64,
    /// `lastSentState = -1`。
    last_sent: i32,
    /// `gameTicks`：这个玩家自己的刻数，从进服开始数。
    ticks: i64,
}

impl Default for Mining {
    fn default() -> Self {
        Self {
            is_destroying: false,
            progress_start: 0,
            pos: BlockPos::new(0, 0, 0),
            direction: Direction::Down,
            has_delayed: false,
            delayed_pos: BlockPos::new(0, 0, 0),
            delayed_start: 0,
            last_sent: -1,
            ticks: 0,
        }
    }
}

/// 处理动作时要用到的玩家状态，先拷一份，好让世界的可变借用不受玩家表限制。
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    id: PlayerId,
    /// `player.getId()`：裂纹包里的实体 ID。
    entity_id: i32,
    game_mode: GameMode,
    actor: Actor,
    eye_height: f64,
    on_ground: bool,
    mining: Mining,
}

fn snapshot(region: &Region, id: PlayerId) -> Option<Snapshot> {
    let player = region.players.get(&id)?;
    Some(Snapshot {
        id,
        entity_id: player.id.0,
        game_mode: player.game_mode,
        actor: actor_of(player),
        eye_height: player.eye_height(),
        on_ground: player.on_ground,
        mining: player.mining,
    })
}

fn store(region: &mut Region, snapshot: Snapshot) {
    if let Some(player) = region.players.get_mut(&snapshot.id) {
        player.mining = snapshot.mining;
    }
}

/// 这一格现在的方块，世界外是 `void_air`。
fn block_state(region: &Region, pos: BlockPos) -> BlockState {
    BlockState(region.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
}

/// `player.getDestroySpeed(state)` 要读的几样东西。
fn miner_of(level: &dyn Level, snapshot: &Snapshot) -> Miner {
    Miner {
        main_hand: snapshot.actor.main_hand,
        on_ground: snapshot.on_ground,
        eye_in_water: block_mining::is_eye_in_water(level, snapshot.actor.eye_position()),
    }
}

/// 官方在 `incrementDestroyProgress` 里把进度换成裂纹阶段：`(int)(progress * 10.0F)`。
fn crack_state(progress: f32) -> i32 {
    (progress * 10.0) as i32
}

/// `Player.blockActionRestricted`。这个世界没有权限系统，冒险模式里能用的 `can_break` 组件
/// 也还读不出来，按组件不存在算，所以冒险模式和旁观模式一样，什么都挖不动。
fn block_action_restricted(snapshot: &Snapshot) -> bool {
    !snapshot.actor.may_build
}

/// `ServerPlayerGameMode.handleBlockBreakAction`。四个破坏动作都从这里过。
pub(crate) fn handle_block_break_action(
    region: &mut Region,
    id: PlayerId,
    action: PlayerActionKind,
    pos: BlockPos,
    face: Direction,
) {
    let Some(mut snapshot) = snapshot(region, id) else {
        return;
    };
    if !within_block_interaction_range(
        snapshot.actor.position,
        snapshot.eye_height,
        snapshot.actor.block_interaction_range,
        pos,
        REACH_BUFFER,
    ) {
        if action == PlayerActionKind::AbortDestroyBlock && snapshot.mining.is_destroying {
            abort_destroy_block(region, &mut snapshot, pos);
        }
        debug!(player = id.0, ?pos, "too far to break the block");
        store(region, snapshot);
        return;
    }
    if pos.y > MAX_Y {
        // 官方直接发 `new ClientboundBlockUpdatePacket(pos, getBlockState(pos))`，
        // 世界外读出来是 `void_air`；世界外的格子 `send_current_block` 会跳过，所以这里直接发。
        region.links.send(
            id,
            &BlockUpdate {
                pos,
                state: blocks::VOID_AIR as i32,
            },
        );
        debug!(player = id.0, ?pos, "too high to break the block");
        return;
    }

    match action {
        PlayerActionKind::StartDestroyBlock => {
            start_destroy_block(region, &mut snapshot, pos, face)
        }
        PlayerActionKind::ChangeDestroyDirection => snapshot.mining.direction = face,
        PlayerActionKind::StopDestroyBlock => stop_destroy_block(region, &mut snapshot, pos),
        PlayerActionKind::AbortDestroyBlock => abort_destroy_block(region, &mut snapshot, pos),
        _ => {}
    }
    store(region, snapshot);
}

/// `START_DESTROY_BLOCK`。创造模式一下挖穿，别的模式从此开始攒进度。
fn start_destroy_block(
    region: &mut Region,
    snapshot: &mut Snapshot,
    pos: BlockPos,
    face: Direction,
) {
    // 没有出生点保护，官方的 `isUnderSpawnProtection` 恒假。
    if !is_within_world_border(pos) {
        send_current_block(region, snapshot.id, pos);
        debug!(player = snapshot.id.0, ?pos, "outside the world border");
        return;
    }
    // `abilities.instabuild`：只有创造模式。
    if snapshot.game_mode == GameMode::Creative {
        destroy_and_ack(region, snapshot, pos);
        return;
    }
    if block_action_restricted(snapshot) {
        send_current_block(region, snapshot.id, pos);
        debug!(player = snapshot.id.0, ?pos, "block action restricted");
        return;
    }

    snapshot.mining.progress_start = snapshot.mining.ticks;
    // 官方先去 `attack`，再按攻击前的状态算这一下的进度；龙蛋会当场传送走，但这里用的还是旧状态。
    let (state, progress) = attack_and_progress(region, snapshot, pos);
    if !state.is_air() && progress >= 1.0 {
        destroy_and_ack(region, snapshot, pos);
        return;
    }
    if snapshot.mining.is_destroying {
        // 上一格还没挖完，客户端以为一下就挖穿了：把上一格发回去。
        send_current_block(region, snapshot.id, snapshot.mining.pos);
    }
    snapshot.mining.is_destroying = true;
    snapshot.mining.pos = pos;
    snapshot.mining.direction = face;
    let crack = crack_state(progress);
    send_destroy_progress(region, snapshot.entity_id, pos, crack);
    snapshot.mining.last_sent = crack;
}

/// `START_DESTROY_BLOCK` 里对方块的那一下：`EnchantmentHelper.onHitBlock`（没有附魔，跳过）、
/// `state.attack`，再算这一下的进度。返回攻击前的状态和进度。
fn attack_and_progress(
    region: &mut Region,
    snapshot: &Snapshot,
    pos: BlockPos,
) -> (BlockState, f32) {
    let mut attacked = None;
    let (state, progress) = {
        let (mut level, ticks) = region.block_level();
        let state = level.block_state(pos);
        if state.is_air() {
            (state, 1.0)
        } else {
            attacked = Some(mc_bamboo_blocks::attack(
                &mut level,
                ticks,
                &snapshot.actor,
                pos,
            ));
            (
                state,
                block_mining::destroy_progress(state.0, miner_of(&level, snapshot)),
            )
        }
    };
    if let Some(outcome) = attacked {
        send_effects(region, Some(snapshot.id), &outcome.effects);
    }
    (state, progress)
}

/// `STOP_DESTROY_BLOCK`。挖够了就真挖掉，不够就转成延迟破坏。
fn stop_destroy_block(region: &mut Region, snapshot: &mut Snapshot, pos: BlockPos) {
    if pos != snapshot.mining.pos {
        return;
    }
    let ticks_spent = snapshot.mining.ticks - snapshot.mining.progress_start;
    let progress = {
        let (level, _) = region.block_level();
        let state = level.block_state(pos);
        if state.is_air() {
            return;
        }
        block_mining::destroy_progress(state.0, miner_of(&level, snapshot))
            * (ticks_spent + 1) as f32
    };
    if progress >= 0.7 {
        snapshot.mining.is_destroying = false;
        send_destroy_progress(region, snapshot.entity_id, pos, -1);
        destroy_and_ack(region, snapshot, pos);
        return;
    }
    if !snapshot.mining.has_delayed {
        snapshot.mining.is_destroying = false;
        snapshot.mining.has_delayed = true;
        snapshot.mining.delayed_pos = pos;
        // 官方记的是开始挖的那一刻，不是现在，后面的进度接着攒。
        snapshot.mining.delayed_start = snapshot.mining.progress_start;
    }
}

/// `abortDestroyBlock`。挖到一半放弃了，把两格的裂纹都清掉。
fn abort_destroy_block(region: &mut Region, snapshot: &mut Snapshot, pos: BlockPos) {
    snapshot.mining.is_destroying = false;
    if snapshot.mining.pos != pos {
        warn!(
            player = snapshot.id.0,
            destroying = ?snapshot.mining.pos,
            ?pos,
            "mismatch in destroy block pos"
        );
        send_destroy_progress(region, snapshot.entity_id, snapshot.mining.pos, -1);
    }
    send_destroy_progress(region, snapshot.entity_id, pos, -1);
}

/// `destroyAndAck`：挖掉；没挖掉就把这一格现在的状态发回去，客户端的预测靠它回滚。
fn destroy_and_ack(region: &mut Region, snapshot: &Snapshot, pos: BlockPos) {
    if !destroy_block(region, snapshot, pos) {
        send_current_block(region, snapshot.id, pos);
    }
}

/// `ServerPlayerGameMode.destroyBlock` 的后半段。返回官方那个 `boolean`。
///
/// 挖成功时方块逻辑会留下破坏粒子；挖不动时什么也没发生，调用方负责回包。
fn destroy_block(region: &mut Region, snapshot: &Snapshot, pos: BlockPos) -> bool {
    // `ItemStack.canDestroyBlock`：只有带 `tool` 组件又明确不许在创造模式挖的物品才拦，
    // 而且只拦创造模式。
    if snapshot.game_mode == GameMode::Creative
        && let Some(player) = region.players.get(&snapshot.id)
        && !rules::can_destroy(player.inventory.held(Hand::Main))
    {
        return false;
    }
    // 官方在 `destroyBlock` 里又查一遍，中途切到冒险模式时 STOP 也会被拒。
    if block_action_restricted(snapshot) {
        return false;
    }
    // 世界下面读到的是 `void_air`。破坏空气世界不变；官方还会发一个客户端不看的 2001，
    // 这里和原来的创造模式路径一样省掉。
    if block_state(region, pos).is_air() {
        return true;
    }
    // 方块的 `GameMasterBlock` 检查在这里面：没有权限系统，命令方块这类一律挖不动，
    // 返回 `Fail`，调用方把这一格发回去。
    let outcome = {
        let (mut level, ticks) = region.block_level();
        mc_bamboo_blocks::destroy_block(&mut level, ticks, &snapshot.actor, pos)
    };
    send_effects(region, Some(snapshot.id), &outcome.effects);
    if outcome.result != InteractionResult::Success {
        return false;
    }
    // 官方接下来调用 `Block.playerDestroy`（`Block.dropResources`）和 `popExperience`，
    // 掉落物和经验还没做；工具耐久和统计也没有。`Block.destroy` 也没有对应的移植。
    true
}

/// `ServerPlayerGameMode.tick` 的第一部分，每个玩家每 tick 一次。
pub(crate) fn tick_mining(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(mut snapshot) = snapshot(region, id) else {
            continue;
        };
        snapshot.mining.ticks += 1;
        // 延迟破坏优先：官方在 `hasDelayedDestroy` 为真时不再跑普通的挖掘。
        if snapshot.mining.has_delayed {
            tick_delayed_destroy(region, &mut snapshot);
        } else if snapshot.mining.is_destroying {
            tick_destroying(region, &mut snapshot);
        }
        store(region, snapshot);
    }
}

/// 停下来之后接着攒进度，攒满就挖掉。官方的顺序是先发裂纹，再判断满没满。
fn tick_delayed_destroy(region: &mut Region, snapshot: &mut Snapshot) {
    let pos = snapshot.mining.delayed_pos;
    let state = block_state(region, pos);
    if state.is_air() {
        snapshot.mining.has_delayed = false;
        return;
    }
    let ticks_spent = snapshot.mining.ticks - snapshot.mining.delayed_start;
    let progress = {
        let (level, _) = region.block_level();
        block_mining::destroy_progress(state.0, miner_of(&level, snapshot))
            * (ticks_spent + 1) as f32
    };
    increment_destroy_progress(region, snapshot, pos, progress);
    if progress >= 1.0 {
        snapshot.mining.has_delayed = false;
        // 官方的 `destroyBlock(delayedDestroyPos)` 没有 sequence，被判失败也不回包。
        destroy_block(region, snapshot, pos);
    }
}

/// 正在挖：每 tick 加一次进度，发裂纹和声音；方块没了就清掉裂纹。
fn tick_destroying(region: &mut Region, snapshot: &mut Snapshot) {
    let pos = snapshot.mining.pos;
    let state = block_state(region, pos);
    if state.is_air() {
        send_destroy_progress(region, snapshot.entity_id, pos, -1);
        snapshot.mining.last_sent = -1;
        snapshot.mining.is_destroying = false;
        return;
    }
    let ticks_spent = snapshot.mining.ticks - snapshot.mining.progress_start;
    let event = if ticks_spent % 4 == 0 {
        DESTROY_PROGRESS_SOUND_EVENT
    } else {
        DESTROY_PROGRESS_EVENT
    };
    let progress = {
        let (level, _) = region.block_level();
        block_mining::destroy_progress(state.0, miner_of(&level, snapshot))
            * (ticks_spent + 1) as f32
    };
    increment_destroy_progress(region, snapshot, pos, progress);
    send_level_event(
        region,
        None,
        pos,
        &LevelEvent {
            event,
            pos,
            data: snapshot.mining.direction.id(),
            global: false,
        },
    );
}

/// `incrementDestroyProgress` 里发裂纹的部分：阶段变了才发。
fn increment_destroy_progress(
    region: &mut Region,
    snapshot: &mut Snapshot,
    pos: BlockPos,
    progress: f32,
) {
    let crack = crack_state(progress);
    if crack != snapshot.mining.last_sent {
        send_destroy_progress(region, snapshot.entity_id, pos, crack);
        snapshot.mining.last_sent = crack;
    }
}

/// `ServerLevel.destroyBlockProgress`：给附近的、不是自己的玩家发裂纹。
fn send_destroy_progress(region: &mut Region, breaker: i32, pos: BlockPos, progress: i32) {
    let packet = BlockDestruction {
        id: breaker,
        pos,
        progress: progress as i8,
    };
    let frames = encode_frames(&packet);
    for player in region.players.values() {
        if player.id.0 == breaker {
            continue;
        }
        let Vec3 { x, y, z } = player.position;
        let dx = f64::from(pos.x) - x;
        let dy = f64::from(pos.y) - y;
        let dz = f64::from(pos.z) - z;
        if dx * dx + dy * dy + dz * dz < DESTROY_PROGRESS_RADIUS_SQR {
            region.links.send_frames(player.id, &frames);
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items, packets::play::clientbound};
    use mc_bamboo_protocol::{
        item::ItemStack,
        packet::{
            ClientboundPacket,
            play::{BlockDestruction, GameMode, LevelEvent, PlayRequest, PlayerActionKind},
        },
    };

    use crate::{
        links::encode_frames,
        player::Player,
        world::tests::{Client, Frame, Harness, body_of, frames_with_id},
    };

    use super::*;

    /// 出生点东边一格，站在出生点够得着。
    const GROUND: BlockPos = BlockPos::new(11, -61, 8);
    const ABOVE: BlockPos = BlockPos::new(11, -60, 8);

    fn settled() -> (Harness, Client) {
        let mut server = Harness::new(8);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        (server, client)
    }

    fn two_players() -> (Harness, Client, Client) {
        let mut server = Harness::new(8);
        let miner = server.join_and_settle("Alice");
        let watcher = server.join_and_settle("Bob");
        miner.take_frames();
        watcher.take_frames();
        (server, miner, watcher)
    }

    fn player_mut<'a>(server: &'a mut Harness, client: &Client) -> &'a mut Player {
        server
            .region()
            .players
            .get_mut(&client.id)
            .expect("玩家应该在世界里")
    }

    /// 让这名玩家用生存模式、站在地上、主手拿着 `item`。
    fn survive(server: &mut Harness, client: &Client, item: i32) {
        let player = player_mut(server, client);
        player.game_mode = GameMode::Survival;
        player.on_ground = true;
        player.inventory.slots[36] = ItemStack::new(item, 1);
    }

    fn set_block(server: &mut Harness, pos: BlockPos, state: u32) {
        server.region().chunks.set_block(pos, state);
    }

    fn block_at(server: &mut Harness, pos: BlockPos) -> Option<u32> {
        server.region().chunks.block(pos)
    }

    fn request(
        server: &Harness,
        client: &Client,
        action: PlayerActionKind,
        pos: BlockPos,
        face: Direction,
        sequence: i32,
    ) {
        server.request(
            client,
            PlayRequest::PlayerAction {
                action,
                pos,
                face,
                sequence,
            },
        );
    }

    fn start(server: &Harness, client: &Client, pos: BlockPos, face: Direction, sequence: i32) {
        request(
            server,
            client,
            PlayerActionKind::StartDestroyBlock,
            pos,
            face,
            sequence,
        );
    }

    fn stop(server: &Harness, client: &Client, pos: BlockPos, face: Direction, sequence: i32) {
        request(
            server,
            client,
            PlayerActionKind::StopDestroyBlock,
            pos,
            face,
            sequence,
        );
    }

    fn abort(server: &Harness, client: &Client, pos: BlockPos, sequence: i32) {
        request(
            server,
            client,
            PlayerActionKind::AbortDestroyBlock,
            pos,
            Direction::Up,
            sequence,
        );
    }

    fn frame_of<P: ClientboundPacket>(packet: &P) -> Frame {
        Frame {
            id: P::ID,
            body: body_of(encode_frames(packet)),
        }
    }

    fn crack(breaker: i32, pos: BlockPos, progress: i8) -> Frame {
        frame_of(&BlockDestruction {
            id: breaker,
            pos,
            progress,
        })
    }

    fn level_event(event: i32, pos: BlockPos, data: i32) -> Frame {
        frame_of(&LevelEvent {
            event,
            pos,
            data,
            global: false,
        })
    }

    fn ack(sequence: i32) -> Frame {
        frame_of(&mc_bamboo_protocol::packet::play::BlockChangedAck { sequence })
    }

    fn update(pos: BlockPos, state: u32) -> Frame {
        frame_of(&mc_bamboo_protocol::packet::play::BlockUpdate {
            pos,
            state: state as i32,
        })
    }

    fn cracks(client: &Client) -> Vec<Frame> {
        let frames = client.take_frames();
        frames_with_id(&frames, clientbound::BLOCK_DESTRUCTION)
            .into_iter()
            .cloned()
            .collect()
    }

    fn events(client: &Client) -> Vec<Frame> {
        let frames = client.take_frames();
        frames_with_id(&frames, clientbound::LEVEL_EVENT)
            .into_iter()
            .cloned()
            .collect()
    }

    /// 钻石镐挖石头每刻 8 / 1.5 / 30，四舍五入前是 0.1777778。
    #[test]
    fn survival_needs_ticks_and_then_the_stop_breaks_the_block() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);

        start(&server, &client, GROUND, Direction::Up, 1);
        server.tick();
        // 第 1 刻：0.1778 * 2 = 0.3556，裂纹阶段 3，只有自己不需要裂纹包。
        assert_eq!(
            client.take_frames(),
            [level_event(2019, GROUND, Direction::Up.id()), ack(1)]
        );
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));

        // 再攒两刻到 0.7111，超过 0.7，但普通挖掘不会自己挖穿。
        server.tick();
        server.tick();
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));
        client.take_frames();

        stop(&server, &client, GROUND, Direction::Up, 2);
        server.tick();

        assert_eq!(block_at(&mut server, GROUND), Some(blocks::AIR));
        assert_eq!(client.take_frames(), [update(GROUND, blocks::AIR), ack(2)]);
    }

    #[test]
    fn stopping_early_turns_into_a_delayed_destroy_that_finishes_later() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);

        start(&server, &client, GROUND, Direction::Up, 1);
        server.tick();
        server.tick();
        client.take_frames();

        // 两刻只有 0.5333，停止时转成延迟破坏。
        stop(&server, &client, GROUND, Direction::Up, 2);
        server.tick();
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));

        // 延迟破坏接着攒：0.7111、0.8889，第六刻 1.0667 挖穿。
        server.tick();
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));
        client.take_frames();
        server.tick();

        assert_eq!(block_at(&mut server, GROUND), Some(blocks::AIR));
        // 延迟破坏没有 sequence，只广播方块变化，不发 ack。
        assert_eq!(client.take_frames(), [update(GROUND, blocks::AIR)]);
    }

    #[test]
    fn cracking_sends_every_change_and_abort_clears_it() {
        let (mut server, miner, watcher) = two_players();
        survive(&mut server, &miner, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);

        start(&server, &miner, GROUND, Direction::Up, 1);
        server.tick();

        // 开始挖时发的是每刻的基础进度 0.1778 的裂纹阶段 1，然后每刻才乘上刻数。
        let breaker = miner.id.0;
        assert_eq!(
            cracks(&watcher),
            [crack(breaker, GROUND, 1), crack(breaker, GROUND, 3)]
        );

        abort(&server, &miner, GROUND, 2);
        server.tick();

        assert_eq!(cracks(&watcher), [crack(breaker, GROUND, -1)]);
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));
    }

    /// 旁边的人看到裂纹，方块挖掉时收到 -1 和方块变化。
    #[test]
    fn another_player_sees_the_crack_and_the_block_disappears() {
        let (mut server, miner, watcher) = two_players();
        survive(&mut server, &miner, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);
        let breaker = miner.id.0;

        start(&server, &miner, GROUND, Direction::Up, 1);
        server.tick();
        assert_eq!(
            cracks(&watcher),
            [crack(breaker, GROUND, 1), crack(breaker, GROUND, 3)]
        );

        // 攒到 0.7 以上再松手，方块当场挖掉。
        server.tick();
        server.tick();
        watcher.take_frames();
        miner.take_frames();
        stop(&server, &miner, GROUND, Direction::Up, 2);
        server.tick();

        assert_eq!(block_at(&mut server, GROUND), Some(blocks::AIR));
        assert_eq!(
            watcher.take_frames(),
            [
                crack(breaker, GROUND, -1),
                level_event(2001, GROUND, blocks::STONE as i32),
                update(GROUND, blocks::AIR),
            ]
        );
    }

    #[test]
    fn cracks_do_not_reach_players_further_than_32_blocks() {
        let (mut server, miner, far) = two_players();
        survive(&mut server, &miner, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);
        server.move_to(&far, 2000.0, 2000.0);
        server.tick();
        far.take_frames();
        miner.take_frames();

        start(&server, &miner, GROUND, Direction::Up, 1);
        server.tick();

        assert!(cracks(&far).is_empty());
    }

    #[test]
    fn every_destroying_tick_is_a_2019_and_every_fourth_a_2020() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);

        start(&server, &client, GROUND, Direction::Up, 1);
        for _ in 0..4 {
            server.tick();
        }

        let up = Direction::Up.id();
        assert_eq!(
            events(&client),
            [
                level_event(2019, GROUND, up),
                level_event(2019, GROUND, up),
                level_event(2019, GROUND, up),
                level_event(2020, GROUND, up),
            ]
        );

        // 换方向只改事件里的方向，不影响进度。
        request(
            &server,
            &client,
            PlayerActionKind::ChangeDestroyDirection,
            GROUND,
            Direction::East,
            2,
        );
        server.tick();
        assert_eq!(
            events(&client),
            [level_event(2019, GROUND, Direction::East.id())]
        );
    }

    #[test]
    fn survival_reach_is_half_a_block_shorter_than_creative() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        // 眼睛 (8.25, -58.38, 8.5)，到这一格立方体的距离是 5.75：
        // 生存的 4.5 + 1 格缓冲不够，创造再加 0.5 就够了。
        server.request(
            &client,
            PlayRequest::MovePlayer {
                position: Some(Vec3::new(8.25, -60.0, 8.5)),
                rotation: None,
                on_ground: true,
            },
        );
        let pos = BlockPos::new(14, -59, 8);
        set_block(&mut server, pos, blocks::STONE);
        server.tick();
        client.take_frames();

        start(&server, &client, pos, Direction::Up, 1);
        server.tick();
        assert_eq!(block_at(&mut server, pos), Some(blocks::STONE));
        assert_eq!(client.take_frames(), [ack(1)]);

        player_mut(&mut server, &client).game_mode = GameMode::Creative;
        start(&server, &client, pos, Direction::Up, 2);
        server.tick();
        assert_eq!(block_at(&mut server, pos), Some(blocks::AIR));
    }

    #[test]
    fn adventure_and_spectator_modes_cannot_start_breaking() {
        for game_mode in [GameMode::Adventure, GameMode::Spectator] {
            let (mut server, client) = settled();
            survive(&mut server, &client, items::DIAMOND_PICKAXE);
            player_mut(&mut server, &client).game_mode = game_mode;
            set_block(&mut server, GROUND, blocks::STONE);

            start(&server, &client, GROUND, Direction::Up, 1);
            server.tick();

            assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));
            assert_eq!(
                client.take_frames(),
                [update(GROUND, blocks::STONE), ack(1)]
            );
        }
    }

    #[test]
    fn unbreakable_blocks_never_finish() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::BEDROCK);

        start(&server, &client, GROUND, Direction::Up, 1);
        for _ in 0..10 {
            server.tick();
        }
        stop(&server, &client, GROUND, Direction::Up, 2);
        for _ in 0..10 {
            server.tick();
        }

        assert_eq!(block_at(&mut server, GROUND), Some(blocks::BEDROCK));
        let frames = client.take_frames();
        assert!(
            frames_with_id(&frames, clientbound::BLOCK_UPDATE)
                .iter()
                .all(|frame| frame.body != update(GROUND, blocks::BEDROCK).body),
            "方块一直没变，不该发方块更新"
        );
    }

    #[test]
    fn starting_a_second_block_sends_the_first_one_back() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);
        set_block(&mut server, ABOVE, blocks::STONE);

        start(&server, &client, GROUND, Direction::Up, 1);
        server.tick();
        client.take_frames();

        start(&server, &client, ABOVE, Direction::Up, 2);
        server.tick();

        let frames = client.take_frames();
        assert_eq!(frames[0], update(GROUND, blocks::STONE));
    }

    #[test]
    fn stopping_on_another_block_is_ignored() {
        let (mut server, client) = settled();
        survive(&mut server, &client, items::DIAMOND_PICKAXE);
        set_block(&mut server, GROUND, blocks::STONE);

        start(&server, &client, GROUND, Direction::Up, 1);
        server.tick();
        server.tick();
        client.take_frames();

        stop(&server, &client, ABOVE, Direction::Up, 2);
        server.tick();
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::STONE));

        // 进度还在，原地停下照样能挖掉。
        stop(&server, &client, GROUND, Direction::Up, 3);
        server.tick();
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::AIR));
    }

    #[test]
    fn starting_on_air_shows_a_full_crack_and_clears_it_next_tick() {
        let (mut server, miner, watcher) = two_players();
        survive(&mut server, &miner, items::DIAMOND_PICKAXE);
        set_block(&mut server, ABOVE, blocks::AIR);

        start(&server, &miner, ABOVE, Direction::Up, 1);
        server.tick();
        assert_eq!(
            cracks(&watcher),
            [crack(miner.id.0, ABOVE, 10), crack(miner.id.0, ABOVE, -1)]
        );
        assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    }
}
