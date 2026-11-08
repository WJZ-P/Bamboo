//! 方块交互的网络一侧：够距、点击点、序列号和确认、高度提示、把当前的方块发回去。
//!
//! 方块的逻辑都在 `mc_bamboo_blocks` 里，这里只把玩家的动作交给它，再把结果发给客户端。
//! 创造模式破坏一步完成，也不扣物品数量。生存模式按挖掘进度破坏，工具会掉耐久。
//! 客户端在本地预测这些操作，结果靠 `block_update` 和 `block_changed_ack` 收敛到服务端。

pub(crate) mod adventure;
pub(crate) mod level;
pub(crate) mod mining;
pub(crate) mod rules;

pub(crate) use mining::tick_mining;

use std::{collections::BTreeSet, mem};

use mc_bamboo_blocks::{
    Actor, BlockHit, BlockState, DEFAULT_RANDOM_TICK_SPEED, Effect, InteractionResult,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, Vec3};
use mc_bamboo_data::{BlockAncestor, SOUND_EVENTS, items, registries::attribute_defaults, sounds};
use mc_bamboo_protocol::{
    packet::play::{
        BlockChangedAck, BlockEvent, BlockUpdate, GameMode, LevelEvent, OpenSignEditor,
        PlayerActionKind, Sound, SystemChat,
    },
    text::{Color, Component},
};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use tracing::debug;

use crate::{
    block_items, book, container, durability, entity_sync,
    global::GameTime,
    inbound::PlayerId,
    inventory, lectern,
    links::{Links, encode_frames},
    menu, movement,
    player::Player,
    region::Region,
};

const MAX_Y: i32 = MIN_Y + WORLD_HEIGHT - 1;
/// `server.properties` 的 `simulation-distance` 默认值，单位是区块。
const SIMULATION_DISTANCE: i32 = 10;
/// `LevelEvent.PARTICLES_AND_SOUND_PLANT_GROWTH`：骨粉的粒子。
const BONE_MEAL_EVENT: i32 = 1505;
/// 讲台翻页，`LecternBlock.signalPageChange` 的 `levelEvent(1043, pos, 0)`。
///
/// Page turn, `levelEvent(1043, pos, 0)` from `LecternBlock.signalPageChange`.
const LECTERN_PAGE_TURN_EVENT: i32 = 1043;
/// `LevelEvent.COMPOSTER_FILL`：堆肥桶收物品的粒子和声音。
const COMPOSTER_FILL_EVENT: i32 = 1500;
/// 骨粉事件的数据：粒子的数量。
const BONE_MEAL_PARTICLES: i32 = 15;
/// 创造模式的 `block_interaction_range`：基础值 4.5 加修饰符 0.5。
pub(crate) const BLOCK_INTERACTION_RANGE: f64 = 5.0;
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
    let outcome = mc_bamboo_blocks::run_due_ticks(&mut level, ticks, time.0);
    send_effects(region, None, &outcome.effects);
}

/// `ServerLevel.runBlockEvents`：随机刻之后、实体之前。
///
/// `ServerLevel.runBlockEvents`: after random ticks, before entities.
pub(crate) fn run_block_events(region: &mut Region) {
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_block_event_phase(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// `Level.tickBlockEntities`：实体之后。
///
/// `Level.tickBlockEntities`: after entities.
pub(crate) fn run_block_entity_ticks(region: &mut Region) {
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_block_entity_tick(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// `EntityTickList.forEach`。计划刻里生成的实体这一步才第一次 tick。
///
/// `EntityTickList.forEach`. Entities spawned by scheduled ticks tick here for the first time.
pub(crate) fn run_entity_ticks(region: &mut Region) {
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// 计划刻之后：玩家周围的区块各跑一次随机刻，官方 `ServerChunkCache.tickChunks` 也排在计划刻后面。
pub(crate) fn run_random_ticks(region: &mut Region) {
    for chunk in random_ticking_chunks(region) {
        let (mut level, ticks) = region.block_level();
        let outcome =
            mc_bamboo_blocks::tick_chunk(&mut level, ticks, chunk, DEFAULT_RANDOM_TICK_SPEED);
        send_effects(region, None, &outcome.effects);
    }
}

/// 官方跑随机刻的区块：离某名玩家所在的区块（切比雪夫距离）不超过模拟距离，而且加载到了能跑方块刻的程度，
/// 也就是不超过视距加一：视距以内有 `PLAYER_LOADING` 票，再往外一圈是 `BLOCK_TICKING`。
///
/// 官方按 `Long2ByteOpenHashMap` 的哈希顺序遍历，这里按坐标排序。所有区块共用一个随机源，
/// 所以和官方逐位相同的只有单个区块，几个区块一起跑时每个区块拿到的随机数不一样，概率相同。
fn random_ticking_chunks(region: &Region) -> BTreeSet<ChunkPos> {
    let radius = SIMULATION_DISTANCE.min(region.view_distance + 1);
    let mut chunks = BTreeSet::new();
    for player in region.players.values() {
        let center = player.chunk_pos();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                chunks.insert(ChunkPos::new(center.x + dx, center.z + dz));
            }
        }
    }
    chunks
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
        uuid: *player.uuid.as_bytes(),
        position: player.position(),
        yaw: player.yaw,
        head_yaw: player.head_yaw,
        pitch: player.pitch,
        // 官方看的是按键，不是姿势，所以飞行时按着潜行键也算。
        secondary_use: player.sync.shift_key_down,
        main_hand: held(Hand::Main),
        off_hand: held(Hand::Off),
        eye_height: player.eye_height(),
        block_interaction_range: interaction_range(player.game_mode),
        // `GameType.applyAbilities`：冒险模式和旁观模式不能建造。
        may_build: !matches!(player.game_mode, GameMode::Adventure | GameMode::Spectator),
        // `Player.preventsBlockDrops()` 看的是 `instabuild`，只有创造模式有。
        prevents_block_drops: matches!(player.game_mode, GameMode::Creative),
    }
}

/// `Player.blockInteractionRange()`：属性的基础值 4.5，
/// 创造模式的 `ServerPlayer.updatePlayerAttributes` 再加 0.5 的修饰符。
pub(crate) fn interaction_range(game_mode: GameMode) -> f64 {
    let base = attribute_defaults::BLOCK_INTERACTION_RANGE;
    if game_mode == GameMode::Creative {
        base + 0.5
    } else {
        base
    }
}

/// 玩家能不能够到这个方块。`blocks-inventory.md` 6.1。
pub(crate) fn within_reach(player: &Player, pos: BlockPos) -> bool {
    within_block_interaction_range(
        player.position(),
        player.eye_height(),
        interaction_range(player.game_mode),
        pos,
        REACH_BUFFER,
    )
}

/// `Player.isWithinBlockInteractionRange(pos, buffer)`：眼睛到方块单位立方体的距离。
pub(crate) fn within_block_interaction_range(
    position: Vec3,
    eye_height: f64,
    range: f64,
    pos: BlockPos,
    buffer: f64,
) -> bool {
    let Vec3 { x, y, z } = position;
    let axis = |low: i32, eye: f64| {
        let low = f64::from(low);
        (low - eye).max(eye - (low + 1.0)).max(0.0)
    };
    let dx = axis(pos.x, x);
    let dy = axis(pos.y, y + eye_height);
    let dz = axis(pos.z, z);
    let range = range + buffer;
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
    face: Direction,
    sequence: i32,
) {
    match action {
        PlayerActionKind::StartDestroyBlock
        | PlayerActionKind::ChangeDestroyDirection
        | PlayerActionKind::AbortDestroyBlock
        | PlayerActionKind::StopDestroyBlock => {
            if record_sequence(region, id, sequence) {
                mining::handle_block_break_action(region, id, action, pos, face);
            }
        }
        PlayerActionKind::DropAllItems => inventory::drop_held_item(region, id, true),
        PlayerActionKind::DropItem => inventory::drop_held_item(region, id, false),
        PlayerActionKind::SwapItemWithOffhand => inventory::swap_hands(region, id),
        PlayerActionKind::ReleaseUseItem | PlayerActionKind::Stab => {}
    }
}

/// 把方块逻辑留下的结果发出去。`initiator` 是发起动作的玩家，计划刻和容器开合没有发起者。
pub(crate) fn send_effects(region: &mut Region, initiator: Option<PlayerId>, effects: &[Effect]) {
    for effect in effects {
        match *effect {
            Effect::Destroyed {
                pos,
                state,
                except_initiator,
            } => {
                // 容器里的东西没有掉落物，等打开着的菜单关掉后一起丢掉。
                container::on_block_destroyed(region, pos);
                let except = initiator.filter(|_| except_initiator);
                let event = LevelEvent::destroy_block(pos, state.0 as i32);
                send_level_event(region, except, pos, &event);
            }
            Effect::TorchBurnout { pos } => {
                let event = LevelEvent::redstone_torch_burnout(pos);
                send_level_event(region, None, pos, &event);
            }
            Effect::FilledBucket { item } => {
                if let Some(id) = initiator {
                    inventory::add_filled_bucket(region, id, item);
                }
            }
            Effect::BoneMealUsed { pos } => {
                let event = LevelEvent {
                    event: BONE_MEAL_EVENT,
                    pos,
                    data: BONE_MEAL_PARTICLES,
                    global: false,
                };
                send_level_event(region, None, pos, &event);
            }
            // `markUpdated` 的 `sendBlockUpdated`：方块没变，广播阶段照样重发这一格。
            Effect::SignChanged { pos } => region.block_changes.changed.push(pos),
            Effect::SignWaxed { pos } => {
                let event = LevelEvent::wax_on_sign(pos);
                send_level_event(region, None, pos, &event);
            }
            // 活塞伸出、收回的动画。官方广播给 64 格内的所有玩家，发起者也要。
            Effect::BlockEvent {
                pos,
                block,
                param_a,
                param_b,
            } => {
                let block = i32::try_from(block).expect("方块注册表 ID 放得进 i32");
                send_level_event(
                    region,
                    None,
                    pos,
                    &BlockEvent {
                        pos,
                        block,
                        param_a,
                        param_b,
                    },
                );
            }
            // `openTextEdit` 只给登记的编辑者发，它一定就是发起者。
            Effect::OpenSignEditor { pos, slot } => {
                if let Some(id) = initiator {
                    region.links.send(id, &OpenSignEditor { pos, slot });
                }
            }
            Effect::Sound {
                sound,
                source,
                x,
                y,
                z,
                volume,
                pitch,
                except_initiator,
                seed,
            } => {
                let except = initiator.filter(|_| except_initiator);
                let packet = Sound {
                    sound,
                    inline: None,
                    source,
                    x,
                    y,
                    z,
                    volume,
                    pitch,
                    seed: seed.unwrap_or_else(|| region.sound_seed_generator.source().next_long()),
                };
                send_sound(region, except, &packet);
            }
            Effect::LevelEvent {
                event,
                pos,
                data,
                except_initiator,
            } => {
                let except = initiator.filter(|_| except_initiator);
                let packet = LevelEvent {
                    event,
                    pos,
                    data,
                    global: false,
                };
                send_level_event(region, except, pos, &packet);
            }
            // 音符盒的声音在方块事件里算好了，这里换成音效包。粒子仍靠方块事件。
            Effect::NoteBlockPlayed {
                pos,
                instrument,
                pitch,
                seed,
                custom,
                ..
            } => {
                let inline = custom.map(mc_bamboo_blocks::custom_sound_name);
                let packet = Sound {
                    sound: if inline.is_some() {
                        0
                    } else {
                        i32::from(instrument.sound())
                    },
                    inline,
                    source: sounds::RECORDS,
                    x: f64::from(pos.x) + 0.5,
                    y: f64::from(pos.y) + 0.5,
                    z: f64::from(pos.z) + 0.5,
                    volume: 3.0,
                    pitch,
                    seed,
                };
                send_sound(region, None, &packet);
            }
            // `levelEvent(1500)`：客户端播堆肥的粒子和声音。升级了数据是 1，没升级是 0。
            Effect::ComposterFill { pos, success } => {
                let event = LevelEvent {
                    event: COMPOSTER_FILL_EVENT,
                    pos,
                    data: i32::from(success),
                    global: false,
                };
                send_level_event(region, None, pos, &event);
            }
            // 翻页的声音发给附近所有人，包括翻页的那名玩家。
            // The page-turn sound goes to everyone nearby, including the player who turned it.
            Effect::PageTurned { pos } => {
                let event = LevelEvent {
                    event: LECTERN_PAGE_TURN_EVENT,
                    pos,
                    data: 0,
                    global: false,
                };
                send_level_event(region, None, pos, &event);
            }
        }
    }
}

/// 世界事件和方块事件的粒子和动画发给附近的人。`except` 是不用发的玩家，它自己已经在本地预测过了。
fn send_level_event(
    region: &mut Region,
    except: Option<PlayerId>,
    pos: BlockPos,
    event: &impl mc_bamboo_protocol::packet::ClientboundPacket,
) {
    let frames = encode_frames(event);
    // 先从网格挑出附近的候选，再做和官方一样的距离判断。
    // Pick nearby candidates from the grid, then apply the same distance check as vanilla.
    let Region {
        players,
        player_grid,
        links,
        ..
    } = region;
    let x = f64::from(pos.x);
    let z = f64::from(pos.z);
    let mut sent = 0;
    for id in player_grid.near(x, z, LEVEL_EVENT_RADIUS) {
        if Some(id) != except
            && players
                .get(&id)
                .is_some_and(|player| hears_level_event(player, pos))
        {
            links.send_frames(id, &frames);
            sent += 1;
        }
    }
    // 网格漏记了谁，这名玩家就会悄悄收不到包。测试构建里每次都和全量扫描对一遍。
    // A player missing from the grid would silently miss packets, so debug builds compare against a full scan.
    debug_assert_eq!(
        sent,
        players
            .values()
            .filter(|player| Some(player.id) != except && hears_level_event(player, pos))
            .count(),
        "player grid is out of sync with player positions"
    );
}

/// `playSeededSound` 的广播。范围是音效的固定范围，没有就是音量大于 1 时的 16 × 音量，否则 16。
/// 内联的音效是 `createVariableRangeEvent`，没有固定范围。
fn send_sound(region: &mut Region, except: Option<PlayerId>, sound: &Sound) {
    let range = if sound.inline.is_some() {
        if sound.volume > 1.0 {
            16.0 * sound.volume
        } else {
            16.0
        }
    } else {
        SOUND_EVENTS[sound.sound as usize]
            .fixed_range
            .unwrap_or(if sound.volume > 1.0 {
                16.0 * sound.volume
            } else {
                16.0
            })
    };
    let range = f64::from(range);
    let frames = encode_frames(sound);
    for player in region.players.values() {
        if Some(player.id) == except {
            continue;
        }
        let Vec3 { x, y, z } = player.position();
        let dx = sound.x - x;
        let dy = sound.y - y;
        let dz = sound.z - z;
        if dx * dx + dy * dy + dz * dz < range * range {
            region.links.send_frames(player.id, &frames);
        }
    }
}

pub(crate) fn hears_level_event(player: &Player, pos: BlockPos) -> bool {
    let Vec3 { x, y, z } = player.position();
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
    // 官方 `ServerPlayerGameMode.useItemOn` 在功能开关之后、物品 `useOn` 之前单独处理旁观者。
    // 能打开的容器打开，传送门走目的地（这里没有别的维度和折跃出口，一律提示），其余直接放过，
    // 不会因为 `can_place_on` 走到放置。
    if player.game_mode == GameMode::Spectator {
        spectator_use_item_on(region, id, packet);
        return;
    }
    let actor = actor_of(player);
    // `can_place_on` 要在借出 `level` 之前从物品栏读出来。
    let held = player.inventory.held(packet.hand).clone();
    let is_placement_attempt = rules::is_placement_attempt(&held);
    let armed_book = lectern::arm_book(region, id, packet.pos, packet.hand);

    let outcome = durability::run(region, id, packet.hand, |region| {
        let (mut level, ticks) = region.block_level();
        let hit = block_hit(packet);
        mc_bamboo_blocks::use_item_on_checked_with_stack(
            &mut level,
            ticks,
            &actor,
            packet.hand,
            &hit,
            Some(&held),
            |state| actor.may_build || adventure::can_place_on(&held, state),
        )
    });

    // `BlockItem.place` 里的 `updateBlockEntityComponents`：放下去的物品把组件留给方块实体。
    if let Some(placed) = outcome.placed_at {
        let stack = region
            .players
            .get(&id)
            .map(|player| player.inventory.held(packet.hand).clone());
        if let Some(stack) = stack {
            block_items::apply(region, placed, id, &stack);
        }
    }
    send_effects(region, Some(id), &outcome.effects);
    // 炼药锅的 `createFilledResult` 和洗染料。方块逻辑只记账，物品栏在这里改。
    // A cauldron's `createFilledResult` or dye wash. Block behaviour records it; the inventory changes here.
    if let Some(exchange) = &outcome.item_exchange {
        inventory::apply_item_exchange(region, id, packet.hand, exchange);
    }
    // 官方在 `useWithoutItem` 里直接调 `player.openMenu`，菜单包先于挥手。
    if outcome.used_without_item && !menu::open_block(region, id, packet.pos) {
        lectern::open(region, id, packet.pos);
    }
    lectern::complete_use(region, id, packet.pos, packet.hand, armed_book);
    if is_placement_attempt && !outcome.result.consumes_action() {
        send_limit_hint(&mut region.links, id, packet);
    }
    swing(region, id, packet.hand, outcome.result);
}

/// 旁观者右键方块。`getMenuProvider` 非空就打开并 `CONSUME`；传送门无论找不找得到出口都 `CONSUME`
/// （找不到时发 `spectator.cannot_teleport`）；其余 `PASS`，不调用物品的 `useOn`。
fn spectator_use_item_on(region: &mut Region, id: PlayerId, packet: &UseItemOn) {
    if menu::open_block(region, id, packet.pos) || lectern::open(region, id, packet.pos) {
        swing(region, id, packet.hand, InteractionResult::Consume);
        return;
    }
    let portal = region
        .chunks
        .block(packet.pos)
        .is_some_and(|state| BlockState(state).is_a(BlockAncestor::Portal));
    if portal {
        let content = Component::translate("spectator.cannot_teleport", []).color(Color::Red);
        region.links.send(
            id,
            &SystemChat {
                content: &content,
                overlay: true,
            },
        );
        swing(region, id, packet.hand, InteractionResult::Consume);
        return;
    }
    swing(region, id, packet.hand, InteractionResult::Pass);
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
    // 官方 `useItem` 对旁观者直接 `PASS`，对着空气也不会把睡莲、桶这类放出去。
    if player.game_mode == GameMode::Spectator {
        return;
    }
    let actor = actor_of(player);

    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::use_item(&mut level, ticks, &actor, hand);

    // 睡莲这类 `PlaceOnWaterBlockItem` 的放置走这条路，一样要把组件留给方块实体。
    if let Some(placed) = outcome.placed_at {
        let stack = region
            .players
            .get(&id)
            .map(|player| player.inventory.held(hand).clone());
        if let Some(stack) = stack {
            block_items::apply(region, placed, id, &stack);
        }
    }
    send_effects(region, Some(id), &outcome.effects);
    let mut result = outcome.result;
    if !result.consumes_action() && book::use_in_hand(region, id, hand) {
        result = InteractionResult::Success;
    }
    swing(region, id, hand, result);
}

#[cfg(test)]
mod tests;
