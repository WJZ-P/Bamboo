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

use mc_bamboo_blocks::{Actor, BlockHit, BlockState, Effect, InteractionResult, LevelSideState};
use mc_bamboo_command::permission;
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, Vec3};
use mc_bamboo_data::{BlockAncestor, SOUND_EVENTS, items, registries::attribute_defaults, sounds};
use mc_bamboo_protocol::{
    item::ItemStack,
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
pub(crate) const SIMULATION_DISTANCE: i32 = 10;
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
    if region.redstone_cache.config().enabled {
        let (mut level, ticks, cache) = region.block_level_with_cache();
        let outcome =
            mc_bamboo_blocks::run_due_ticks_with_cache(&mut level, ticks, time.0, Some(cache));
        send_effects(region, None, &outcome.effects);
        return;
    }
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_due_ticks(&mut level, ticks, time.0);
    send_effects(region, None, &outcome.effects);
}

/// `ServerLevel.runBlockEvents`：随机刻之后、实体之前。
///
/// `ServerLevel.runBlockEvents`: after random ticks, before entities.
pub(crate) fn run_block_events(region: &mut Region) {
    if region.redstone_cache.config().enabled {
        let now = region.tick.game_time();
        let (mut level, ticks, cache) = region.block_level_with_cache();
        let outcome =
            mc_bamboo_blocks::run_block_event_phase_with_cache(&mut level, ticks, Some(cache), now);
        send_effects(region, None, &outcome.effects);
        return;
    }
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_block_event_phase(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// `Level.tickBlockEntities`：实体之后。
///
/// `Level.tickBlockEntities`: after entities.
pub(crate) fn run_block_entity_ticks(region: &mut Region) {
    // 区块能不能跑方块逻辑变了：睡着的熔炉按公式推进到这一刻并结束睡眠，
    // 下一刻从原进度重新算日程。官方在不能 tick 的刻根本不跑，所以这是暂停，
    // 不是把冻结期间的事件攒起来补跑。
    let changes = region.tick_levels.take_block_tick_changes();
    if !changes.is_empty() {
        let (mut level, _) = region.block_level();
        for (chunk, ticks) in changes {
            level.set_chunk_block_entity_ticking(chunk, ticks);
        }
    }
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_block_entity_tick(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// `EntityTickList.forEach`。计划刻里生成的实体这一步才第一次 tick。
///
/// 精确驱动（默认）整个阶段算完立刻落地，剖析器记为一段；并行驱动（`bamboo.toml` 里 `entity_threads ≥ 1`）
/// 拆成只读算阶段和落地阶段，剖析器分别记两段。开关存在 `World` 上，创建时读一次。
///
/// `EntityTickList.forEach`. Entities spawned by scheduled ticks tick here for the first time.
pub(crate) fn run_entity_ticks(entity_threads: usize, region: &mut Region) {
    // 玩家不在实体列表里。官方玩家通常先于后生成的物品 tick，捡起看到的是移动前的位置。
    // Players are not in the entity list. Vanilla players usually tick before later items,
    // so pickup sees the item where it was before this tick's movement.
    crate::drops::pickup_before_entities(region);
    if entity_threads == 0 {
        let (mut level, ticks) = region.block_level();
        let outcome = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        send_effects(region, None, &outcome.effects);
    } else {
        parallel_entity_ticks(entity_threads, region);
    }
}

/// 并行驱动的两段：算阶段只读区块表和实体表，落地阶段重新构造 `RegionLevel` 和 `World`。
///
/// 每刻起线程的 [`ScopedThreads`] 开销大，只用于实验；常驻线程池留给任务系统
/// （设计稿第 12 节）。
fn parallel_entity_ticks(threads: usize, region: &mut Region) {
    use mc_bamboo_blocks::entity::{
        parallel::{ScopedThreads, commit, compute},
        view::ViewFacts,
    };

    let computed = {
        let Region {
            chunks,
            entities,
            block_entities,
            block_rules,
            world_border,
            tick_levels,
            profiler,
            ..
        } = &mut *region;
        let open_shulker_boxes = crate::entity_view::open_shulker_boxes(block_entities);
        // `pickup_before_entities` 已经跑过；把实体表切进「这一刻正在 tick」的状态。
        // 这里只用实体表，不需要完整的 `RegionLevel`：区块表接下来要借给只读视图。
        entities.begin_tick();
        let hopper_positions = crate::entity_view::hopper_positions(chunks);
        let facts = ViewFacts {
            min_y: mc_bamboo_world::MIN_Y,
            max_y: mc_bamboo_world::MIN_Y + mc_bamboo_world::WORLD_HEIGHT - 1,
            rules: *block_rules,
            ..ViewFacts::default()
        };
        let view = crate::entity_view::EntityBlockView::new(crate::entity_view::ReadOnlyWorld {
            chunks,
            tick_levels,
            facts,
            open_shulker_boxes,
            hopper_positions,
            world_border: world_border.clone(),
        });
        let executor = ScopedThreads { threads };
        let computed = compute(&view, entities, &executor);
        profiler.lap(crate::profiler::Phase::EntityCompute);
        computed
    };

    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        commit(computed, &mut world);
        world.drain_effects()
    };
    send_effects(region, None, &effects);
    region.profiler.lap(crate::profiler::Phase::EntityCommit);
}

/// 计划刻之后：先对全部区块跑官方的 `tickThunder`，再对全部区块跑 `tickChunk`。
///
/// 官方 `ServerChunkCache.tickChunks` 的 `tickSpawningChunk`（里面是 `ServerLevel.tickThunder`）
/// 和 `forEachBlockTickingChunk`（里面是 `tickChunk`）是两个独立的循环。Bamboo 按坐标顺序
/// 遍历，官方按哈希顺序并打乱前者，这是已有的差异。
pub(crate) fn run_random_ticks(region: &mut Region) {
    let chunks = random_ticking_chunks(region);
    // 官方 `tickSpawningChunk` 只在下雨且打雷时调 `tickThunder`；晴天一个随机数都不取。
    // 这里先按天气快照短路，避免每 tick 为每个区块建一次方块世界。
    if region.block_weather.is_raining() && region.block_weather.is_thundering() {
        for chunk in &chunks {
            let (mut level, ticks) = region.block_level();
            let outcome = mc_bamboo_blocks::tick_thunder(&mut level, ticks, *chunk);
            send_effects(region, None, &outcome.effects);
        }
    }
    for chunk in chunks {
        let speed = region.block_rules.random_tick_speed;
        let (mut level, ticks) = region.block_level();
        // `ServerChunkCache.tickChunks` 把 `GameRules.RANDOM_TICK_SPEED` 传给 `tickChunk`。
        //
        // `ServerChunkCache.tickChunks` passes `GameRules.RANDOM_TICK_SPEED` to `tickChunk`.
        let outcome = mc_bamboo_blocks::tick_chunk(&mut level, ticks, chunk, speed);
        send_effects(region, None, &outcome.effects);
    }
}

/// 已加载模式跑逐区块 tick 的区块：能跑方块逻辑、且模拟等级 ≤31（`random` 集合）。
/// 无限超平坦模式仍按每名玩家视距内的方形（每个位置都隐式存在，没有等级可用）。
///
/// 官方按 `Long2ByteOpenHashMap` 的哈希顺序遍历，这里按坐标排序。所有区块共用一个随机源，
/// 所以和官方逐位相同的只有单个区块，几个区块一起跑时每个区块拿到的随机数不一样，概率相同。
fn random_ticking_chunks(region: &Region) -> Vec<ChunkPos> {
    if region.tick_levels.enabled() {
        // 拷一份坐标：`block_level()` 后面要可变借用整个 region。
        return region.tick_levels.random_chunks().iter().copied().collect();
    }
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
    chunks.into_iter().collect()
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
        // `ServerPlayer.canUseGameMasterBlocks`：`instabuild` 并且至少是游戏管理员。
        // 旁观模式的 `instant_build` 是假，所以旁观的 OP 也不能用调试棒和命令方块。
        //
        // `ServerPlayer.canUseGameMasterBlocks`: `instabuild` and at least gamemaster.
        // Spectator `instant_build` is false, so a spectator op cannot use the debug stick
        // or command blocks either.
        can_use_game_master_blocks: matches!(player.game_mode, GameMode::Creative)
            && player.permission_level >= permission::GAMEMASTERS,
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
        PlayerActionKind::DropAllItems => crate::drops::drop_held(region, id, true),
        PlayerActionKind::DropItem => crate::drops::drop_held(region, id, false),
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
                // 方块实体已经按 `dropContents` 掉过物品，这里只收掉打开着的菜单留下的记录。
                container::on_block_destroyed(region, pos);
                let except = initiator.filter(|_| except_initiator);
                let event = LevelEvent::destroy_block(pos, state.0 as i32);
                send_level_event(region, except, pos, &event);
            }
            Effect::TorchBurnout { pos } => {
                let event = LevelEvent::redstone_torch_burnout(pos);
                send_level_event(region, None, pos, &event);
            }
            Effect::FilledBucket { item, hand } => {
                if let Some(id) = initiator {
                    inventory::give_filled_result(region, id, hand, ItemStack::new(item, 1), true);
                }
            }
            Effect::EmptiedBucket { hand } => {
                let Some(id) = initiator else {
                    continue;
                };
                // 创造模式交还手里原来的那一堆，`contains` 为真，什么也不加。
                // Creative mode hands back the stack already held. `contains` is true, so nothing is added.
                let replacement = {
                    let Some(player) = region.players.get(&id) else {
                        continue;
                    };
                    if player.game_mode == GameMode::Creative {
                        player.inventory.held(hand).clone()
                    } else {
                        ItemStack::new(items::BUCKET, 1)
                    }
                };
                inventory::give_filled_result(region, id, hand, replacement, true);
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
            // 穿上的格子已经写进物品栏。本人收 `container_set_slot`，附近的人靠之后的装备同步。
            //
            // The slot is already written. This player gets `container_set_slot`. Nearby viewers
            // see it on the later equipment sync.
            Effect::DispenserEquipped {
                entity_id,
                menu_slot,
            } => {
                let players = &mut region.players;
                let links = &mut region.links;
                if let Some(player) = players.get_mut(&PlayerId(entity_id)) {
                    inventory::send_slot(player, links, usize::from(menu_slot));
                }
            }
            Effect::CampfireChanged { pos } => crate::campfire::write_update_tag(region, pos),
            // `ServerPlayer.sendOverlayMessage` → `sendSystemMessage(message, true)`，
            // overlay 恒发（隐藏聊天也收）。只发给发起动作的那名玩家。
            //
            // `ServerPlayer.sendOverlayMessage` → `sendSystemMessage(message, true)`;
            // overlay messages always arrive, even for hidden chat. Only the acting player.
            Effect::Overlay { key, first, second } => {
                let Some(id) = initiator else {
                    continue;
                };
                let mut args = Vec::new();
                if let Some(first) = first {
                    args.push(first.into());
                }
                if let Some(second) = second {
                    args.push(second.into());
                }
                let content = Component::translate(key, args);
                crate::chat::send_system_message(region, id, &content, true);
            }
            Effect::OpenCommandBlock { pos } => {
                if let Some(id) = initiator {
                    crate::command_block::open(region, id, pos);
                }
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

    // 官方 `handleUseItemOn` 的 `level.mayInteract(player, pos)`：边界外不能交互，
    // 走 `sendBuildLimitMessage(true, maxY)` 那条路，但仍把两格方块发回去。
    //
    // Official `handleUseItemOn`'s `level.mayInteract(player, pos)`: outside the
    // border the click is refused with `sendBuildLimitMessage(true, maxY)`, yet both
    // block updates are still sent.
    if !region.world_border.is_within_bounds_pos(pos) {
        send_build_limit(&mut region.links, id, TOO_HIGH, MAX_Y);
        send_current_block(region, id, pos);
        send_current_block(region, id, pos.relative(face));
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
    // 官方 `sendBuildLimitMessage` → `sendOverlayMessage` → `sendSystemMessage(message, true)`，
    // overlay 恒发（隐藏聊天也收），所以直接 `links.send` 与走
    // `send_system_message(..., true)` 等价。这是只拿 `Links` 的网络层小助手，
    // 保持直接发，不去改动交互里的高度判断。
    //
    // Official `sendBuildLimitMessage` → `sendOverlayMessage` → `sendSystemMessage(message, true)`;
    // overlay always arrives (even for hidden chat), so a direct `links.send` is equivalent to
    // `send_system_message(..., true)`. This network-layer helper only holds `Links`; it stays a
    // direct send to avoid touching the interaction height checks.
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
        // `CommandBlock.setPlacedBy`。`/setblock` 不走这里。
        //
        // `CommandBlock.setPlacedBy`. `/setblock` does not come through here.
        crate::command_block::on_placed(region, placed);
    }
    send_effects(region, Some(id), &outcome.effects);
    // 炼药锅的 `createFilledResult` 和洗染料。方块逻辑只记账，物品栏在这里改。
    // A cauldron's `createFilledResult` or dye wash. Block behaviour records it; the inventory changes here.
    if let Some(exchange) = &outcome.item_exchange {
        inventory::apply_item_exchange(region, id, packet.hand, exchange);
    }
    // 雕纹书架取出的书。整本进背包，生存模式放不下的按 `Player.drop` 丢在地上，
    // 创造模式放不下的那一份官方直接清掉。
    //
    // A book taken from a chiseled bookshelf. The whole stack goes into the inventory;
    // survival drops what does not fit and creative discards it as vanilla does.
    if let Some(gained) = outcome.gained.clone() {
        inventory::give_or_drop(region, id, gained);
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
        // 官方 `sendOverlayMessage` → `sendSystemMessage(message, true)`，overlay 恒发。
        //
        // Official `sendOverlayMessage` → `sendSystemMessage(message, true)`; overlay always arrives.
        crate::chat::send_system_message(region, id, &content, true);
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
        crate::command_block::on_placed(region, placed);
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
