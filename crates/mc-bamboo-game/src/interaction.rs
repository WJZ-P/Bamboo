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
use mc_bamboo_data::{BlockAncestor, SOUND_EVENTS, items, registries::attribute, sounds};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        BlockChangedAck, BlockEvent, BlockUpdate, DEFAULT_EXPLOSION_BLOCK_PARTICLES, Explode,
        GameMode, LevelEvent, OpenSignEditor, PlayerActionKind, Sound, SystemChat,
    },
    text::{Color, Component},
};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use tracing::debug;

use crate::{
    block_items, book, container, durability, entity_sync,
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
///
/// 计划刻队列的时钟由岛的时钟驱动（岛的实施计划第 6 节第 3 步），不再读世界时间；
/// 岛建立时两者相同、每刻同步加一，所以精确模式下取到的数和原来逐位相同。
///
/// The scheduled-tick clock is driven by the island clock, not the world time;
/// they start equal and advance together, so exact mode sees the same values.
pub(crate) fn run_block_ticks(region: &mut Region) {
    let now = region.tick.game_time();
    if region.redstone_cache.config().enabled {
        let (mut level, ticks, cache) = region.block_level_with_cache();
        let outcome =
            mc_bamboo_blocks::run_due_ticks_with_cache(&mut level, ticks, now, Some(cache));
        send_effects(region, None, &outcome.effects);
        return;
    }
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_due_ticks(&mut level, ticks, now);
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
    if region.redstone_cache.config().enabled {
        let now = region.tick.game_time();
        let (mut level, ticks, cache) = region.block_level_with_cache();
        let outcome =
            mc_bamboo_blocks::run_block_entity_tick_with_cache(&mut level, ticks, now, Some(cache));
        send_effects(region, None, &outcome.effects);
        return;
    }
    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::run_block_entity_tick(&mut level, ticks);
    send_effects(region, None, &outcome.effects);
}

/// `EntityTickList.forEach`。计划刻里生成的实体这一步才第一次 tick。
///
/// 精确驱动（默认）整个阶段算完立刻落地，剖析器记为一段；并行驱动（`bamboo.toml` 里 `entity_threads ≥ 1`）
/// 拆成只读算阶段和落地阶段，剖析器分别记两段。开关存在 `World` 上，创建时读一次。
/// `pool` 是启动时建好、按刻派活的常驻线程池，所有岛共用一份。
///
/// `EntityTickList.forEach`. Entities spawned by scheduled ticks tick here for the first time.
pub(crate) fn run_entity_ticks(
    entity_threads: usize,
    region: &mut Region,
    pool: Option<&mc_bamboo_blocks::entity::parallel::WorkerPool>,
) -> Vec<mc_bamboo_blocks::entity::minecart::CommandActivation> {
    // 玩家不在实体列表里。官方玩家通常先于后生成的物品 tick，捡起看到的是移动前的位置。
    // Players are not in the entity list. Vanilla players usually tick before later items,
    // so pickup sees the item where it was before this tick's movement.
    crate::drops::pickup_before_entities(region);
    // 把骑在矿车里的第一玩家乘客的方向按键同步进矿车，供 `OldMinecartBehavior.moveAlongTrack` 读。
    // Sync each riding player's `getLastClientMoveIntent` onto its minecart before entity ticks.
    sync_minecart_rider_intents(region);
    if entity_threads == 0 {
        let (mut level, ticks) = region.entity_level();
        let outcome = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        send_entity_effects(region, &outcome.effects)
    } else {
        parallel_entity_ticks(entity_threads, region, pool)
    }
}

/// 把在线骑着矿车且是第一位乘客的玩家的 `getLastClientMoveIntent()` 写到矿车的
/// `rider_intent` 里。矿车自己没有玩家控制者，由服务端 `OldMinecartBehavior` 驱动。
///
/// Push each online first-passenger player's `getLastClientMoveIntent()` onto
/// their minecart so server-side `OldMinecartBehavior` can apply it.
fn sync_minecart_rider_intents(region: &mut Region) {
    let riders: Vec<(i32, mc_bamboo_blocks::entity::EntityUid, Vec3)> = region
        .players
        .iter()
        .filter_map(|(&id, player)| {
            let vehicle = player.riding.vehicle?;
            let intent = crate::entity_sync::last_client_move_intent(
                player.sync.last_input_flags,
                player.yaw,
            );
            Some((id.0, vehicle, intent))
        })
        .collect();
    for (player_uid, vehicle, intent) in riders {
        region.entities.with_mut(vehicle, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Minecart(cart) = &mut entity.kind {
                cart.rider_intent = if cart.passengers.first() == Some(&player_uid) {
                    intent
                } else {
                    Vec3::default()
                };
            }
        });
    }
}

/// 并行驱动的两段：算阶段只读区块表和实体表，落地阶段重新构造 `RegionLevel` 和 `World`。
///
/// 正式服务端传启动时建好的常驻池；只有直接调这段的历史测试没有池，那里临时建一个
/// （`threads = 1` 时池里一条后台线程也不起）。
pub(crate) fn parallel_entity_ticks(
    threads: usize,
    region: &mut Region,
    pool: Option<&mc_bamboo_blocks::entity::parallel::WorkerPool>,
) -> Vec<mc_bamboo_blocks::entity::minecart::CommandActivation> {
    let game_time = region.tick.game_time();
    let entity_rules = region.entity_game_rules();
    use mc_bamboo_blocks::entity::{
        parallel::{WorkerPool, commit, compute},
        view::ViewFacts,
    };

    let computed = {
        let Region {
            chunks,
            poi,
            entities,
            block_entities,
            containers,
            global,
            tick_levels,
            profiler,
            players,
            block_change_stamp,
            ..
        } = &mut *region;
        let player_views = crate::player::player_views(&*players, &global.scoreboard);
        let block_entity_shapes = crate::entity_view::block_entity_shapes(chunks, block_entities);
        // `pickup_before_entities` 已经跑过；把实体表切进「这一刻正在 tick」的状态。
        // 这里只用实体表，不需要完整的 `RegionLevel`：区块表接下来要借给只读视图。
        entities.begin_tick();
        let hopper_positions = crate::entity_view::hopper_positions(chunks);
        // 算阶段开始的容器槽位快照（按位置升序），铜傀儡只读查询用它；
        // 精确驱动读的是当前容器表。
        let container_snapshot: Vec<_> = containers
            .iter()
            .map(|(pos, entry)| (*pos, entry.slots.clone()))
            .collect();
        let container_open_counts: Vec<_> = containers
            .iter()
            .map(|(pos, entry)| (*pos, entry.open_count))
            .collect();
        let facts = ViewFacts {
            min_y: mc_bamboo_world::MIN_Y,
            max_y: mc_bamboo_world::MIN_Y + mc_bamboo_world::WORLD_HEIGHT - 1,
            rules: entity_rules,
            difficulty: global.difficulty as i32,
            sky_darken: global.sky_darken,
            villager_activity: global.villager_activity,
            baby_villager_activity: global.baby_villager_activity,
            game_time,
            weather: global.block_weather,
            clock_ticks: global.clock.total_ticks,
            monsters_burn: mc_bamboo_blocks::sky::monsters_burn(global.clock.total_ticks),
            sea_level: crate::natural_spawn::sea_level_of(global),
            ..ViewFacts::default()
        };
        let view = crate::entity_view::EntityBlockView::new(crate::entity_view::ReadOnlyWorld {
            chunks,
            poi,
            tick_levels,
            facts,
            open_shulker_boxes: block_entity_shapes.open_shulker_boxes,
            moving_piston_shapes: block_entity_shapes.moving_piston_shapes,
            shulker_box_shapes: block_entity_shapes.shulker_box_shapes,
            hopper_positions,
            container_snapshot,
            container_open_counts,
            block_change_stamp: block_change_stamp.get(),
            world_border: global.world_border.clone(),
            players: player_views,
            scoreboard: &global.scoreboard,
        });
        let owned_pool = match pool {
            Some(_) => None,
            None => Some(
                WorkerPool::new("bamboo-entity", threads).unwrap_or_else(|error| {
                    panic!("could not start the entity worker pool: {error}")
                }),
            ),
        };
        let executor: &dyn mc_bamboo_blocks::entity::parallel::Executor =
            pool.or(owned_pool.as_ref()).expect("上面二选一必有执行器");
        let computed = compute(&view, entities, executor);
        profiler.lap(crate::profiler::Phase::EntityCompute);
        computed
    };

    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        commit(computed, &mut world);
        world.drain_effects()
    };
    let activations = send_entity_effects(region, &effects);
    region.profiler.lap(crate::profiler::Phase::EntityCommit);
    activations
}

fn send_entity_effects(
    region: &mut Region,
    effects: &[Effect],
) -> Vec<mc_bamboo_blocks::entity::minecart::CommandActivation> {
    send_effects(region, None, effects);
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::RunMinecartCommand(activation) => Some(*activation),
            _ => None,
        })
        .collect()
}

/// 计划刻之后：先对全部区块跑官方的 `tickThunder`，再对全部区块跑 `tickChunk`。
///
/// 官方 `ServerChunkCache.tickChunks` 的 `tickSpawningChunk`（里面是 `ServerLevel.tickThunder`）
/// 和 `forEachBlockTickingChunk`（里面是 `tickChunk`）是两个独立的循环。Bamboo 按坐标顺序
/// 遍历，官方按哈希顺序并打乱前者，这是已有的差异。
pub(crate) fn run_random_ticks(region: &mut Region) {
    // `tickSpawningChunk` 第一句是 `incrementInhabitedTime`，在 `tickThunder` 之前。
    // 官方只在 `TickRateManager.runsNormally()` 时走到这里；Bamboo 没有暂停 / 冻结刻，恒为真。
    // 累加只写 ChunkStore 的单独一张表，不换区块版本、不进存盘集合。
    for chunk in crate::natural_spawn::inhabited_time_chunks(region) {
        region.chunks.increment_inhabited_time(chunk);
    }
    let chunks = random_ticking_chunks(region);
    // 官方 `tickSpawningChunk` 只在下雨且打雷时调 `tickThunder`；晴天一个随机数都不取。
    // 这里先按天气快照短路，避免每 tick 为每个区块建一次方块世界。
    if region.global.block_weather.is_raining() && region.global.block_weather.is_thundering() {
        for chunk in &chunks {
            let (mut level, ticks) = region.block_level();
            let outcome = mc_bamboo_blocks::tick_thunder(&mut level, ticks, *chunk);
            send_effects(region, None, &outcome.effects);
        }
    }
    for chunk in chunks {
        let speed = region.global.block_rules.random_tick_speed;
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
pub(crate) fn actor_of(player: &Player, id: PlayerId) -> Actor {
    let held = |hand| {
        let stack = player.inventory.held(hand);
        if stack.is_empty() {
            items::AIR
        } else {
            stack.item
        }
    };
    Actor {
        uid: id.0,
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
        block_interaction_range: player.attributes.value(attribute::BLOCK_INTERACTION_RANGE),
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

/// 玩家能不能够到这个方块。`blocks-inventory.md` 6.1。
pub(crate) fn within_reach(player: &Player, pos: BlockPos) -> bool {
    within_block_interaction_range(
        player.position(),
        player.eye_height(),
        player.attributes.value(attribute::BLOCK_INTERACTION_RANGE),
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
        PlayerActionKind::ReleaseUseItem => {
            crate::player_consume::release(region, id);
            crate::player_weapon::release(region, id);
        }
        PlayerActionKind::Stab => crate::player_attack::on_stab(region, id),
    }
}

/// 把方块逻辑留下的结果发出去。`initiator` 是发起动作的玩家，计划刻和容器开合没有发起者。
pub(crate) fn send_effects(region: &mut Region, initiator: Option<PlayerId>, effects: &[Effect]) {
    for effect in effects {
        match *effect {
            Effect::RunMinecartCommand(_) => {}
            Effect::EntityEvent { entity, event } => {
                let packet = mc_bamboo_protocol::packet::play::EntityEvent {
                    entity_id: entity,
                    event,
                };
                for id in region.entity_tracking.viewers(entity) {
                    region.links.send(id, &packet);
                }
                let player = PlayerId(entity);
                if region.players.contains_key(&player) {
                    region.links.send(player, &packet);
                }
            }
            Effect::BoatBubbleEvent {
                entity,
                event,
                ref ejected,
            } => {
                // `Level.broadcastEntityEvent(boat, 71/72)`：只发给跟踪这艘船的
                // 玩家，再把 down 气泡柱甩下来的乘客按完整 `stopRiding` 落位。
                let packet = mc_bamboo_protocol::packet::play::EntityEvent {
                    entity_id: entity,
                    event,
                };
                let viewers = region.entity_tracking.viewers(entity);
                for id in viewers {
                    region.links.send(id, &packet);
                }
                // `ejectPassengers()` 从后往前对每个乘客调 `stopRiding()`；载具名单
                // 由方块层清空，这里补乘客一侧的完整落位（摘关系、冷却、下船位置）。
                let vehicle_public = region.entities.public_at(entity).cloned();
                for &uid in ejected {
                    if uid < mc_bamboo_blocks::entity::FIRST_ENTITY_ID {
                        region.entities.enqueue_ejected_player(uid);
                    } else if let Some(vehicle) = &vehicle_public {
                        // 船载具：`AbstractBoat.getDismountLocationForPassenger` 用
                        // `getCollisionHorizontalEscapeVector` + `DismountHelper`，
                        // 生物乘客也走这一套，不能用 blocks 层的缺省落位。
                        //
                        // A boat uses `AbstractBoat.getDismountLocationForPassenger`
                        // (escape vector + DismountHelper) for mob passengers too,
                        // not the default block-layer dismount location.
                        let location = if matches!(
                            &vehicle.kind,
                            mc_bamboo_blocks::entity::KindPublic::Boat(_)
                        ) {
                            let mut params = None;
                            region.entities.with_mut(uid, |rider| {
                                params = Some((
                                    rider.dimensions,
                                    rider.y_rot,
                                    rider.fall_distance,
                                    rider.position.y,
                                ));
                            });
                            params.and_then(|(dimensions, yaw, fall_distance, bottom)| {
                                crate::riding::dismount::boat_dismount_location_for_mob(
                                    &region.chunks,
                                    &region.global.world_border,
                                    f64::from(dimensions.0),
                                    f64::from(dimensions.1),
                                    yaw,
                                    fall_distance,
                                    bottom,
                                    vehicle.position,
                                    1.375,
                                    vehicle.bounding_box().max_y,
                                )
                            })
                        } else {
                            None
                        };
                        region.entities.with_mut(uid, |rider| {
                            mc_bamboo_blocks::entity::riding::stop_riding_at(
                                rider, vehicle, None, location,
                            );
                        });
                    }
                }
            }
            Effect::PlayerShoulder { player, ref tag } => {
                // `ShoulderRidingEntity.setEntityOnShoulder`：把鹦鹉 NBT 存进玩家肩膀。
                let id = crate::inbound::PlayerId(player);
                if let Some(shouldered) = region.players.get_mut(&id) {
                    crate::player_shoulder::set_entity_on_shoulder(shouldered, tag.clone());
                }
            }
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
                    // `BucketItem.use` 拿到满桶之后，服务端触发 FILLED_BUCKET。
                    crate::advancement_triggers::record(
                        region,
                        id,
                        "minecraft:filled_bucket",
                        serde_json::json!({"item": item_name_of(item)}),
                    );
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
                region.sound_effects.push(effect.clone());
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
            Effect::Explode {
                center,
                radius,
                block_count,
                ref hit_players,
                is_small,
                play_sound,
            } => send_explode(
                region,
                center,
                radius,
                block_count,
                hit_players,
                is_small,
                play_sound,
            ),
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
            Effect::GlobalLevelEvent { event, pos, data } => {
                // `ServerLevel.globalLevelEvent`：不看距离、不排除发起者，发给所有在线
                // 玩家，协议包带 `global=true`。
                //
                // `ServerLevel.globalLevelEvent`: no distance check and no initiator
                // exclusion, sent to every online player with `global=true`.
                let packet = LevelEvent {
                    event,
                    pos,
                    data,
                    global: true,
                };
                let frames = encode_frames(&packet);
                let Region { players, links, .. } = region;
                for id in players.keys() {
                    links.send_frames(*id, &frames);
                }
            }
            Effect::Particles {
                particle,
                position,
                offset,
                speed,
                count,
            } => {
                let mut options = Vec::new();
                mc_bamboo_protocol::codec::write_varint(&mut options, particle);
                let packet = mc_bamboo_protocol::packet::play::LevelParticles {
                    options: &options,
                    override_limiter: false,
                    always_show: false,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    x_dist: offset.x as f32,
                    y_dist: offset.y as f32,
                    z_dist: offset.z as f32,
                    max_speed: speed,
                    count,
                    randomization: mc_bamboo_protocol::packet::play::Randomization::Default,
                };
                let frames = encode_frames(&packet);
                for player in region.players.values() {
                    let delta = player.position().subtract(position);
                    if delta.length_sqr() < 32.0 * 32.0 {
                        region.links.send_frames(player.id, &frames);
                    }
                }
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
            Effect::ConduitChanged { pos } => {
                crate::block_items::refresh_update_tag(region, pos);
                // `sendBlockUpdated`：方块状态没变也要重发这一格和它的方块实体数据。
                region.block_changes.changed.push(pos);
            }
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
            Effect::PushPlayerUp { target, distance } => {
                crate::movement::push_player_up(region, PlayerId(target), distance);
            }
            Effect::EnderPearlTeleport { target, position } => {
                crate::ender_pearl::teleport(region, target, position);
            }
            Effect::HurtPlayer {
                target,
                source,
                amount,
            } => {
                // 只有并行驱动走到这里：精确驱动在生物自己的 tick 里当场落地
                // （`RegionLevel::hurt_player_now`）。并行驱动按实体顺序把意图收进
                // effects 逐个走正式入口，算阶段已经按「打中」取过数（性能模式差异）。
                let id = crate::inbound::PlayerId(target);
                let (_, drops) = crate::player_hurt::hurt_player_by_mob(
                    &mut region.hurt_parts(),
                    id,
                    source,
                    amount,
                );
                if let Some(drops) = drops {
                    let (mut level, ticks) = region.block_level();
                    let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
                    world.spawn_player_death_drops(&drops);
                }
            }
            Effect::DestroyProgress {
                breaker,
                pos,
                progress,
            } => mining::send_destroy_progress(region, breaker, pos, progress),
            Effect::IgnitePlayer { target, ticks } => {
                crate::player_environment::ignite_for_ticks(
                    region,
                    crate::inbound::PlayerId(target),
                    ticks,
                );
            }
            Effect::PlayerEffect {
                target,
                effect,
                duration,
                amplifier,
                ambient,
                visible,
                show_icon,
            } => {
                let id = crate::inbound::PlayerId(target);
                if mc_bamboo_blocks::entity::mob::effects::is_instantaneous(effect) {
                    crate::player_effects::apply_instantaneous(region, id, effect, amplifier);
                } else {
                    let mut instance =
                        mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_flags(
                            effect, duration, amplifier, ambient, visible,
                        );
                    instance.show_icon = show_icon;
                    crate::player_effects::give(region, id, instance);
                }
            }
            Effect::TrialSpawnerTransformBadOmen { player, duration } => {
                use mc_bamboo_data::registries::mob_effect;
                let Some(id) = region
                    .players
                    .iter()
                    .find(|(_, p)| p.uuid.as_bytes() == &player)
                    .map(|(id, _)| *id)
                else {
                    return;
                };
                crate::player_effects::clear(region, id, Some(mob_effect::BAD_OMEN));
                let instance = mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_flags(
                    mob_effect::TRIAL_OMEN,
                    duration,
                    0,
                    false,
                    true,
                );
                crate::player_effects::give(region, id, instance);
            }
            Effect::HealPlayer { target, amount } => {
                let id = crate::inbound::PlayerId(target);
                if let Some(player) = region.players.get_mut(&id)
                    && player.health.health > 0.0
                {
                    let max = crate::player_effects::max_health(player);
                    player.health.health = (player.health.health + amount).min(max);
                }
            }
            Effect::GuardianElderEffect { target, silent } => {
                use mc_bamboo_protocol::packet::play::GameEvent;
                let packet = GameEvent {
                    event: GameEvent::GUARDIAN_ELDER_EFFECT,
                    param: if silent { 0.0 } else { 1.0 },
                };
                region.links.send(crate::inbound::PlayerId(target), &packet);
            }
            Effect::SleepInBed {
                pos,
                set_spawn,
                problem,
            } => {
                if let Some(id) = initiator {
                    crate::sleep::on_sleep_in_bed(region, id, pos, set_spawn, problem);
                }
            }
            Effect::RaiderDied {
                uid,
                position,
                killer,
            } => {
                crate::raid::raider_died(region, uid, position, killer);
            }
            Effect::RaidSetLeader { id, wave, leader } => {
                crate::raid::set_leader(region, id, wave, leader);
            }
            Effect::BredAnimals {
                father,
                mother,
                child,
                lover,
                ..
            } => {
                // `Animal.finalizeSpawnChildFromBreeding`：官方把判据发给求爱者。
                if let Some(uuid) = lover {
                    let ids: Vec<PlayerId> = region
                        .players
                        .iter()
                        .filter(|(_, player)| *player.uuid.as_bytes() == uuid)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in ids {
                        crate::advancement_triggers::record(
                            region,
                            id,
                            "minecraft:bred_animals",
                            serde_json::json!({
                                "parent": {"type": father},
                                "partner": {"type": mother},
                                "child": {"type": child},
                            }),
                        );
                    }
                }
            }
            Effect::SummonedEntity {
                entity,
                position,
                radius,
            } => {
                // 官方按召唤实体的包围盒膨胀后的 AABB 找附近的 ServerPlayer。
                let ids: Vec<PlayerId> = region
                    .players
                    .iter()
                    .filter(|(_, player)| {
                        let p = player.position();
                        (p.x - position.x).abs() <= radius
                            && (p.y - position.y).abs() <= radius
                            && (p.z - position.z).abs() <= radius
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    crate::advancement_triggers::record(
                        region,
                        id,
                        "minecraft:summoned_entity",
                        serde_json::json!({"entity": {"type": entity}}),
                    );
                }
            }
            Effect::KilledByArrow {
                shooter,
                victims,
                weapon,
            } => {
                let victims: Vec<&str> =
                    victims.iter().copied().filter(|v| !v.is_empty()).collect();
                let unique = victims
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                // `AbstractArrow.onHitEntity`：射手是玩家时给这名玩家触发。
                if let Some(uuid) = shooter {
                    let ids: Vec<PlayerId> = region
                        .players
                        .iter()
                        .filter(|(_, player)| *player.uuid.as_bytes() == uuid)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in ids {
                        crate::advancement_triggers::record(
                            region,
                            id,
                            "minecraft:killed_by_arrow",
                            serde_json::json!({
                                "victims": victims.iter().map(|v| serde_json::json!({"type": v})).collect::<Vec<_>>(),
                                "unique_entity_types": unique,
                                "fired_from_weapon": weapon,
                            }),
                        );
                    }
                }
            }
            Effect::VibrationParticle {
                pos,
                x,
                y,
                z,
                travel,
            } => {
                // VibrationParticleOption.STREAM_CODEC:
                // particle type id, position-source type id 0 (minecraft:block),
                // BlockPos(long), arrivalInTicks(VarInt).
                let mut options = Vec::new();
                mc_bamboo_protocol::codec::write_varint(
                    &mut options,
                    mc_bamboo_data::registries::particle_type::VIBRATION,
                );
                mc_bamboo_protocol::codec::write_varint(&mut options, 0);
                mc_bamboo_protocol::codec::write_block_pos(&mut options, pos);
                mc_bamboo_protocol::codec::write_varint(&mut options, travel);
                let body = mc_bamboo_protocol::frame::packet_body(
                    &mc_bamboo_protocol::packet::play::LevelParticles {
                        options: &options,
                        override_limiter: false,
                        always_show: false,
                        x,
                        y,
                        z,
                        x_dist: 0.0,
                        y_dist: 0.0,
                        z_dist: 0.0,
                        max_speed: 0.0,
                        count: 1,
                        randomization: mc_bamboo_protocol::packet::play::Randomization::Default,
                    },
                );
                crate::entity_tracking::send_vibration_particle(
                    region,
                    &body,
                    mc_bamboo_core::Vec3::new(x, y, z),
                );
            }
            Effect::CrafterCrafted { recipe, position } => {
                // `CrafterBlock.dispenseFrom`：17x17x17 的盒子里每名玩家。
                let ids: Vec<PlayerId> = region
                    .players
                    .iter()
                    .filter(|(_, player)| {
                        let p = player.position();
                        (p.x - position.x).abs() <= 8.5
                            && (p.y - position.y).abs() <= 8.5
                            && (p.z - position.z).abs() <= 8.5
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    crate::advancement_triggers::record(
                        region,
                        id,
                        "minecraft:crafter_recipe_crafted",
                        serde_json::json!({"recipes": recipe}),
                    );
                }
            }
            Effect::ConstructBeacon { target, levels } => {
                crate::advancement_triggers::record(
                    region,
                    PlayerId(target),
                    "minecraft:construct_beacon",
                    serde_json::json!({"level": levels}),
                );
            }
            // 烟花火箭只记录、不落地实体（烟花火箭实体另开任务），游戏层不处理。
            // A firework rocket is only recorded, not spawned (the entity is a separate
            // task), so the game layer ignores it.
            Effect::Firework { .. } => {}
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
pub(crate) fn send_sound(region: &mut Region, except: Option<PlayerId>, sound: &Sound) {
    send_sound_to(&region.players, &mut region.links, except, sound);
}

/// [`send_sound`] 只用玩家表和连接的那一半。
pub(crate) fn send_sound_to(
    players: &std::collections::BTreeMap<PlayerId, Player>,
    links: &mut crate::links::Links,
    except: Option<PlayerId>,
    sound: &Sound,
) {
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
    for player in players.values() {
        if Some(player.id) == except {
            continue;
        }
        let Vec3 { x, y, z } = player.position();
        let dx = sound.x - x;
        let dy = sound.y - y;
        let dz = sound.z - z;
        if dx * dx + dy * dy + dz * dz < range * range {
            links.send_frames(player.id, &frames);
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

/// `ServerLevel.explode` 把 `ClientboundExplodePacket` 发给 64 格内的每名玩家，
/// 各带 `getHitPlayers` 里自己那份击退向量。
///
/// `ServerLevel.explode` sends `ClientboundExplodePacket` to every player within
/// 64 blocks, each carrying their own `getHitPlayers` knockback.
pub(crate) fn send_explode(
    region: &mut Region,
    center: Vec3,
    radius: f32,
    block_count: i32,
    hit_players: &[(i32, Vec3)],
    is_small: bool,
    play_sound: bool,
) {
    let particle = if is_small {
        mc_bamboo_data::registries::particle_type::EXPLOSION
    } else {
        mc_bamboo_data::registries::particle_type::EXPLOSION_EMITTER
    };
    let sound = i32::from(sounds::sound_event("minecraft:entity.generic.explode"));
    for player in region.players.values() {
        let Vec3 { x, y, z } = player.position();
        let dx = center.x - x;
        let dy = center.y - y;
        let dz = center.z - z;
        if dx * dx + dy * dy + dz * dz >= 64.0 * 64.0 {
            continue;
        }
        let knockback = hit_players
            .iter()
            .find(|(uid, _)| *uid == player.id.0)
            .map(|(_, knock)| *knock);
        let packet = Explode {
            center,
            radius,
            block_count,
            player_knockback: knockback,
            explosion_particle: particle,
            explosion_sound: sound,
            block_particles: DEFAULT_EXPLOSION_BLOCK_PARTICLES,
            play_sound,
        };
        region.links.send(player.id, &packet);
    }
}

fn block_update(pos: BlockPos, state: u32) -> BlockUpdate {
    BlockUpdate {
        pos,
        state: state as i32,
    }
}

/// `use_item_on`，官方 `handleUseItemOn` 的顺序。`blocks-inventory.md` 3.1、第 6 章。
/// 物品注册表 ID 的完整名字，方块层只给出 ID。
pub(crate) fn item_name_of(item: i32) -> String {
    mc_bamboo_data::items::ITEMS
        .get(item as usize)
        .map(|item| item.name.to_owned())
        .unwrap_or_default()
}

fn item_name(region: &Region, id: PlayerId, hand: mc_bamboo_core::Hand) -> String {
    region
        .players
        .get(&id)
        .and_then(|player| {
            mc_bamboo_data::items::ITEMS.get(player.inventory.held(hand).item as usize)
        })
        .map(|item| item.name.to_owned())
        .unwrap_or_default()
}

fn block_name(region: &Region, pos: mc_bamboo_core::BlockPos) -> String {
    region
        .chunks
        .block(pos)
        .and_then(|state| mc_bamboo_data::blocks::BLOCKS.get(state as usize))
        .map(|block| block.name.to_owned())
        .unwrap_or_default()
}

pub(crate) fn on_use_item_on(region: &mut Region, id: PlayerId, packet: UseItemOn) {
    on_use_item_on_with_maps(region, None, id, packet);
}

/// 同 [`on_use_item_on`]，由主控传入权威 [`MapStore`](crate::map::MapStore) 以支持多岛旗帜标记同步。
///
/// Like [`on_use_item_on`], passing the master's authoritative `MapStore` so
/// banner markers toggle directly on the global map store.
pub(crate) fn on_use_item_on_with_maps(
    region: &mut Region,
    maps: Option<&mut crate::map::MapStore>,
    id: PlayerId,
    packet: UseItemOn,
) {
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
    if !region.global.world_border.is_within_bounds_pos(pos) {
        send_build_limit(&mut region.links, id, TOO_HIGH, MAX_Y);
        send_current_block(region, id, pos);
        send_current_block(region, id, pos.relative(face));
        return;
    }

    // 客户端可能还没收到传送，这时的点击不能信。
    if !player.teleport.is_awaiting() {
        use_held_item(region, maps, id, &packet);
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
fn use_held_item(
    region: &mut Region,
    maps: Option<&mut crate::map::MapStore>,
    id: PlayerId,
    packet: &UseItemOn,
) {
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
    let actor = actor_of(player, id);
    // `can_place_on` 要在借出 `level` 之前从物品栏读出来。
    let held = player.inventory.held(packet.hand).clone();
    // `FireworkRocketItem.useOn`：`player.isFallFlying()` 时官方直接 `PASS`，不生成
    // 烟花也不扣物品。滑翔时的加速交给 `use`（对着空气右键）那条路径。
    //
    // `FireworkRocketItem.useOn`: vanilla returns `PASS` directly while the player is
    // fall flying, spawning and consuming nothing. Gliding boosts go through `use`.
    if player.sync.is_fall_flying() && held.item == items::FIREWORK_ROCKET {
        return;
    }
    let is_placement_attempt = rules::is_placement_attempt(&held);
    let armed_book = lectern::arm_book(region, id, packet.pos, packet.hand);

    let mut outcome = durability::run(region, id, packet.hand, |region| {
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

    // 官方 `MapItem.useOn`：在方块交互放行且未冷却、允许交互（`mayBuild || canPlaceOn`）时，
    // 手持已填充地图右键旗帜切换旗帜标记。
    //
    // Official `MapItem.useOn`: when block interaction passes, item is not on
    // cooldown, and `mayBuild || canPlaceOn` holds, right-clicking a banner with
    // a `filled_map` toggles the banner marker.
    if outcome.result == InteractionResult::Pass
        && held.item == items::FILLED_MAP
        && let Some(clicked_state) = region.chunks.block(packet.pos)
        && (actor.may_build || adventure::can_place_on(&held, BlockState(clicked_state)))
        && !region
            .players
            .get(&id)
            .is_some_and(|p| crate::ender_pearl::on_cooldown(p, &held))
        && let Some(map_result) = crate::map::use_map_on_banner(region, maps, packet.pos, &held)
    {
        outcome.result = map_result;
        if map_result == InteractionResult::Success {
            crate::item_use_stats::award(region, id, items::FILLED_MAP);
            if let Some(player) = region.players.get_mut(&id) {
                crate::ender_pearl::apply_cooldown(player, &held);
            }
            let block = block_name(region, packet.pos);
            let payload = serde_json::json!({
                "block": block,
                "item": item_name(region, id, packet.hand),
                "x": packet.pos.x, "y": packet.pos.y, "z": packet.pos.z,
            });
            crate::advancement_triggers::record(
                region,
                id,
                "minecraft:item_used_on_block",
                payload,
            );
        }
    }

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
        // 官方 `PlacedBlockTrigger` / `ItemUsedOnLocationTrigger`：放置成功时按
        // 放下的方块、位置与手持物品触发。
        let block = region
            .chunks
            .block(placed)
            .and_then(|state| mc_bamboo_data::blocks::BLOCKS.get(state as usize))
            .map(|block| block.name.to_owned())
            .unwrap_or_default();
        let payload = serde_json::json!({
            "block": block,
            "item": item_name(region, id, packet.hand),
            "x": placed.x, "y": placed.y, "z": placed.z,
        });
        crate::advancement_triggers::record(region, id, "minecraft:placed_block", payload.clone());
        crate::advancement_triggers::record(region, id, "minecraft:item_used_on_block", payload);
    }
    // `ServerGamePacketListenerImpl.handleUseItemOn` 在结果 consumesAction 之后
    // 给这次右键触发 ANY_BLOCK_USE；BLOCK_USE loot context 带方块与手持物品。
    let clicked_block = block_name(region, packet.pos);
    if outcome.result.consumes_action() {
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:any_block_use",
            serde_json::json!({"block": clicked_block.clone()}),
        );
    }
    // `DefaultBlockInteractionTrigger`：`useWithoutItem` 成功且用的是主手。
    if outcome.used_without_item && packet.hand == Hand::Main && outcome.result.consumes_action() {
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:default_block_use",
            serde_json::json!({"block": clicked_block}),
        );
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
    rotation: (f32, f32),
) {
    on_use_item_with_maps(region, None, id, hand, sequence, rotation);
}

/// 同 [`on_use_item`]，由主控传入权威 [`MapStore`](crate::map::MapStore) 以避免多岛地图 ID 撞号。
///
/// Like [`on_use_item`], passing the master's authoritative `MapStore` so map
/// ID allocation is globally unique across islands.
pub(crate) fn on_use_item_with_maps(
    region: &mut Region,
    maps: Option<&mut crate::map::MapStore>,
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
    // 鱼桶要从这一堆上的组件还原鱼，先抄下来。
    // A fish bucket restores its fish from this stack's components, so copy it first.
    let held = player.inventory.held(hand).clone();
    // `Item.use` → `Consumable.startConsuming`：可消耗物品先进入「正在使用」，
    // 不往下走放置 / 方块交互链。先把 `actor` 抄出来，`start` 要独占借 `region`。
    let actor = actor_of(player, id);
    // `BundleItem.use`：只 `startUsingItem`，逐件倒出在 `player_consume::tick`。
    if mc_bamboo_protocol::components::bundle::is_bundle_item(held.item)
        && crate::player_consume::start_bundle(player, hand, held.item)
    {
        return;
    }
    // `BoatItem.use`：`getPlayerPOVHitResult` 之前先查 5 格内 `CAN_BE_PICKED`
    // 实体；玩家眼睛落在某个实体的盒子里就 `PASS`，不放船。
    if mc_bamboo_blocks::entity::boat::entity_type_for_item(held.item).is_some() {
        let view = mc_bamboo_core::Vec3::view_vector(yaw, pitch).scale(5.0);
        let bb = player.bounding_box();
        let area = mc_bamboo_core::Aabb::new(
            bb.min_x + view.x.min(0.0),
            bb.min_y + view.y.min(0.0),
            bb.min_z + view.z.min(0.0),
            bb.max_x + view.x.max(0.0),
            bb.max_y + view.y.max(0.0),
            bb.max_z + view.z.max(0.0),
        )
        .inflate(1.0, 1.0, 1.0);
        let eye = mc_bamboo_core::Vec3::new(
            player.position().x,
            player.position().y + player.eye_height(),
            player.position().z,
        );
        let mut blocked = false;
        region.entities.for_each_in(area, &mut |public| {
            // `EntitySelector.CAN_BE_PICKED` 就是 `Entity.isPickable()`，不是所有活实体。
            if !public.is_pickable() {
                return;
            }
            // `getPickRadius()` 对船 / 生物 / 矿车都是 0，直接用实体盒子判眼睛是否在内。
            if public.bounding_box().contains(eye.x, eye.y, eye.z) {
                blocked = true;
            }
        });
        if blocked {
            return;
        }
    }
    if crate::player_consume::start(region, id, hand) {
        return;
    }
    // `BowItem.use` / `CrossbowItem.use`：拉弓、装填，或者射出手里已经装好的弩。
    //
    // `BowItem.use` / `CrossbowItem.use`: draw, load, or fire a charged crossbow.
    if crate::player_weapon::start(region, id, hand) {
        return;
    }
    // `EggItem.use`：扔出鸡蛋，返回 `SUCCESS`。
    // `EggItem.use`: throw the egg and return `SUCCESS`.
    if crate::player_egg::throw(region, id, hand) {
        swing(region, id, hand, InteractionResult::Success);
        return;
    }
    // `WindChargeItem.use`：扔出一枚风弹，返回 `SUCCESS`。
    if crate::player_wind_charge::throw(region, id, hand) {
        swing(region, id, hand, InteractionResult::Success);
        return;
    }
    // `FishingRodItem.use`：抛竿或收竿，返回 `SUCCESS`。
    // `FishingRodItem.use`: cast or retrieve the fishing bobber, returning `SUCCESS`.
    if crate::player_fishing::use_rod(region, id, hand) {
        swing(region, id, hand, InteractionResult::Success);
        return;
    }
    // `EnderEyeItem.use`：瞄着框架返回 `PASS`，否则找要塞扔出 `EyeOfEnder`（`SUCCESS_SERVER`）或 `CONSUME`。
    // `EnderEyeItem.use`: `PASS` on a frame, otherwise throw an `EyeOfEnder` (`SUCCESS_SERVER`) or `CONSUME`.
    if let Some(result) = crate::player_ender_eye::use_eye(region, id, hand, &actor) {
        swing(region, id, hand, result);
        return;
    }
    // `EmptyMapItem.use`：空地图右键创建已填充地图，返回 `SUCCESS`。
    // `EmptyMapItem.use`: create a filled map from an empty map, returning `SUCCESS`.
    if crate::map::use_empty_map(region, maps, id, hand) {
        swing(region, id, hand, InteractionResult::Success);
        return;
    }
    // `FireworkRocketItem.use`：仅在玩家 `isFallFlying()`（鞘翅滑翔）时生成附着烟花加速。
    // 滑翔时生成附着在玩家身上的烟花、扣物品（创造模式不扣）、记使用统计并返回 `SUCCESS`；
    // 不滑翔官方返回 `PASS`，什么都不做。
    //
    // `FireworkRocketItem.use`: only spawns an attached boost rocket while fall flying.
    // While gliding it spawns a rocket attached to the player, consumes one (not in
    // creative), awards the use stat and returns `SUCCESS`; vanilla returns `PASS`
    // and does nothing otherwise.
    if held.item == items::FIREWORK_ROCKET {
        let Some((position, owner_uuid, fall_flying)) = region.players.get(&id).map(|player| {
            (
                player.position(),
                *player.uuid.as_bytes(),
                player.sync.is_fall_flying(),
            )
        }) else {
            return;
        };
        if !fall_flying {
            return;
        }
        let effects = {
            let (mut level, ticks) = region.block_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            // 官方顺序：`dropAllLeashConnections` → SHEAR 游戏事件 → 播 `LEAD_BREAK`
            // → 生成烟花 → `consume` → `awardStat`。剪刀剪结 / 剪生物复用同一套解绳。
            //
            // Official order: `dropAllLeashConnections` → SHEAR game event →
            // `LEAD_BREAK` sound → spawn the rocket → `consume` → `awardStat`.
            // The shears paths reuse the same leash-drop helper.
            //
            // 字节码里 `dropAllLeashConnections(null)` 仍发 SHEAR（`Context(null, null)`），
            // 和任务书「传 null 就不发」不同；按字节码，这里发一条空源的 SHEAR。
            if mc_bamboo_blocks::entity::leash_knot::drop_all_leashed_to(&mut world, id.0) {
                mc_bamboo_blocks::entity::leash_knot::shear_game_event(
                    &mut world, position, None, false,
                );
                world.play_sound(
                    position,
                    mc_bamboo_data::sound_event("minecraft:item.lead.break"),
                    mc_bamboo_data::sounds::NEUTRAL,
                    1.0,
                    1.0,
                    false,
                );
            }
            world.spawn_firework(&mc_bamboo_blocks::entity::firework_rocket::FireworkShot {
                position,
                owner: Some(id.0),
                owner_uuid: Some(owner_uuid),
                item: held.clone(),
                shot_at_angle: false,
                attached_to_target: Some(id.0),
                shoot: None,
            });
            world.drain_effects()
        };
        send_effects(region, Some(id), &effects);
        crate::item_use_stats::award(region, id, held.item);
        crate::inventory::shrink_held(region, id, hand);
        swing(region, id, hand, InteractionResult::Success);
        return;
    }
    // `FoodOnAStickItem.use`：拿着胡萝卜钓竿、骑着由自己控制的猪时，右键
    // 让猪加速并扣 7 点耐久；没加速成则只记一次使用统计。
    //
    // `FoodOnAStickItem.use`: with a carrot on a stick while riding a
    // player-controlled pig, right-click boosts the pig and spends seven
    // durability points; a failed use only awards the item-use stat.
    if held.item == items::CARROT_ON_A_STICK {
        let vehicle = crate::riding::player_vehicles(region).get(&id).copied();
        let boosted = vehicle.is_some_and(|vehicle| {
            region
                .entities
                .with_mut(vehicle, |entity| {
                    entity.with_mob(|me, mob| {
                        if mob.mob_type != mc_bamboo_blocks::entity::mob::MobType::Pig
                            || mob.controlling_passenger() != Some(id.0)
                        {
                            return false;
                        }
                        let random = mc_bamboo_blocks::entity::EntityRandom(
                            std::cell::RefCell::new(me.random),
                        );
                        let boosted = mob.pig_steering.boost(&random);
                        me.random = random.0.into_inner();
                        if boosted {
                            mob.needs_sync = true;
                        }
                        boosted
                    })
                })
                .flatten()
                .unwrap_or(false)
        });
        if boosted {
            durability::hurt_and_convert_on_break(region, id, hand);
            swing(region, id, hand, InteractionResult::SuccessServer);
        } else {
            crate::item_use_stats::award(region, id, held.item);
        }
        return;
    }

    let (mut level, ticks) = region.block_level();
    let outcome = mc_bamboo_blocks::use_item_with_stack(&mut level, ticks, &actor, hand, &held);

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
    // 船放置成功时 `ItemStack.consume(1, player)`：生存模式减一个。方块层的
    // `Item.use` 路径只有船会记 `item_exchange`。
    //
    // A successful boat placement consumes one item in survival. The boat is
    // the only `Item.use` path that records an `item_exchange` in the block layer.
    if let Some(exchange) = &outcome.item_exchange {
        crate::inventory::apply_item_exchange(region, id, hand, exchange);
    }
    let mut result = outcome.result;
    if !result.consumes_action() && book::use_in_hand(region, id, hand) {
        result = InteractionResult::Success;
    }
    swing(region, id, hand, result);
}

#[cfg(test)]
mod tests;

/// `ServerGamePacketListenerImpl.handleInteract`：官方先校验实体存在、世界边界和够距，
/// 再按 `Player.interactOn` 的顺序用请求的手试一次，没成功再试另一只手。
///
/// Vanilla's `handleInteract`: validate the entity and the reach, then let
/// `Player.interactOn` try the requested hand and fall back to the other one.
#[cfg(test)]
pub(crate) fn on_interact_entity(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    hand: Hand,
    using_secondary_action: bool,
) {
    // 菜单 / 测试路径没有点击位置，按官方 `INTERACT`（非 `INTERACT_AT`）给零向量。
    // Menu / test paths carry no hit point; use the zero vector of a plain `INTERACT`.
    on_interact_entity_at(
        region,
        id,
        entity_id,
        hand,
        using_secondary_action,
        Vec3::default(),
    );
}

/// `ServerGamePacketListenerImpl.handleInteract`：带 `INTERACT_AT` 的点击位置。
///
/// `ServerGamePacketListenerImpl.handleInteract`: with the `INTERACT_AT` hit point.
pub(crate) fn on_interact_entity_at(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    hand: Hand,
    using_secondary_action: bool,
    location: Vec3,
) {
    // 官方把 `usingSecondaryAction` 记成潜行键；马科用它判断开物品栏。
    // Vanilla stores `usingSecondaryAction` as the sneak key; equines read it.
    // 先把玩家和目标的只读数据抄出来，`block_level()` 要独占借 `region`。
    let Some((
        game_mode,
        position,
        eye_height,
        entity_reach,
        player_uuid,
        main_held,
        off_held,
        player_height,
    )) = region.players.get(&id).map(|player| {
        (
            player.game_mode,
            player.position(),
            player.eye_height(),
            player.attributes.value(attribute::ENTITY_INTERACTION_RANGE),
            *player.uuid.as_bytes(),
            player.inventory.held(Hand::Main).clone(),
            player.inventory.held(Hand::Off).clone(),
            player.height(),
        )
    })
    else {
        return;
    };
    if game_mode == GameMode::Spectator {
        return;
    }
    let Some(target) = region.entities.public_at(entity_id) else {
        return;
    };
    let is_knot = matches!(
        target.kind,
        mc_bamboo_blocks::entity::KindPublic::LeashKnot { .. }
    );
    let is_boat = matches!(target.kind, mc_bamboo_blocks::entity::KindPublic::Boat(_));
    let is_item_frame = matches!(
        target.kind,
        mc_bamboo_blocks::entity::KindPublic::ItemFrame(_)
    );
    // `ItemFrame.interact` 只在空框放物品时查地图追踪上限，先记下框是否为空。
    // `ItemFrame.interact` checks the tracked-decoration limit only when inserting
    // into an empty frame; remember whether the frame is empty now.
    let frame_empty = matches!(
        &target.kind,
        mc_bamboo_blocks::entity::KindPublic::ItemFrame(frame) if frame.item.is_empty()
    );
    let is_minecart = matches!(
        target.kind,
        mc_bamboo_blocks::entity::KindPublic::Minecart(_)
    );
    let minecart_kind = match &target.kind {
        mc_bamboo_blocks::entity::KindPublic::Minecart(cart) => {
            mc_bamboo_blocks::entity::minecart::MinecartKind::from_entity_type(cart.entity_type)
        }
        _ => None,
    };
    let target_position = target.position;
    let target_removed = target.removed;
    let target_is_mob = matches!(target.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_));
    let boat_is_chest = matches!(
        &target.kind,
        mc_bamboo_blocks::entity::KindPublic::Boat(boat)
            if mc_bamboo_blocks::entity::boat::is_chest(boat.entity_type)
    );
    if target_removed || !(target_is_mob || is_knot || is_boat || is_item_frame || is_minecart) {
        return;
    }
    // 够距：官方 `ServerGamePacketListenerImpl.handleInteract` 用 `ENTITY_INTERACTION_RANGE` + 3.0 缓冲。
    if !within_block_interaction_range(
        position,
        eye_height,
        entity_reach,
        mc_bamboo_core::BlockPos::containing(target_position),
        3.0,
    ) {
        return;
    }
    let creative = game_mode == GameMode::Creative;
    let other = match hand {
        Hand::Main => Hand::Off,
        Hand::Off => Hand::Main,
    };
    if let Some(kind) = minecart_kind {
        use mc_bamboo_blocks::entity::minecart::MinecartKind;
        match kind {
            // `MinecartFurnace.interact`：手上那一件是燃料就 `addFuel`，成功扣一个；
            // 不管成没成，官方都返回 SUCCESS（挥手）。
            // `MinecartFurnace.interact`: add fuel from the held hand, shrink on
            // success, and always report SUCCESS (the swing).
            MinecartKind::Furnace => {
                let held = match hand {
                    Hand::Main => main_held.clone(),
                    Hand::Off => off_held.clone(),
                };
                let fuel_item = mc_bamboo_blocks::state::item_is_tagged(
                    held.item,
                    mc_bamboo_data::tags::item_tags::FURNACE_MINECART_FUEL,
                );
                let cart_position = target_position;
                let mut added = false;
                region.entities.with_mut(entity_id, |entity| {
                    if let mc_bamboo_blocks::entity::EntityKind::Minecart(cart) = &mut entity.kind {
                        added = cart.add_fuel(position, cart_position, fuel_item);
                    }
                });
                if added {
                    inventory::apply_item_exchange(
                        region,
                        id,
                        hand,
                        &mc_bamboo_blocks::ItemExchange::Shrink,
                    );
                }
                swing(region, id, hand, InteractionResult::Success);
                return;
            }
            // `Minecart.interact`：非潜行且车里没人时 `player.startRiding(this)`。
            MinecartKind::Rideable => {
                if !using_secondary_action {
                    crate::riding::mount(region, id, entity_id);
                    if crate::riding::passengers(region, entity_id).contains(&id.0) {
                        swing(region, id, hand, InteractionResult::Success);
                    }
                }
                return;
            }
            // `MinecartChest` / `MinecartHopper.interact`：潜行或没上成车时
            // `interactWithContainerVehicle` 打开实体容器菜单（和船共用一套）。
            // `MinecartChest` / `MinecartHopper.interact`: opening the entity
            // container menu shares the chest-boat path.
            MinecartKind::Chest | MinecartKind::Hopper => {
                crate::menu::open_entity_container(region, id, entity_id);
                return;
            }
            MinecartKind::CommandBlock
                if region
                    .players
                    .get(&id)
                    .is_some_and(crate::command_block::player_can_use) =>
            {
                swing(region, id, hand, InteractionResult::Success);
            }
            _ => {}
        }
        return;
    }
    if is_boat {
        let is_chest = boat_is_chest;
        // `AbstractBoat.interact`：`super.interact` 放行后，非潜行且
        // `outOfControlTicks < 60` 时 `player.startRiding(this)`。
        if !using_secondary_action {
            crate::riding::mount(region, id, entity_id);
            if crate::riding::passengers(region, entity_id).contains(&id.0) {
                swing(region, id, hand, InteractionResult::Success);
                return;
            }
        }
        // `AbstractChestBoat.interact`：super 返回 PASS（没上成船或潜行）时
        // `interactWithContainerVehicle(player)` 打开 27 格菜单。
        if is_chest {
            crate::menu::open_entity_container(region, id, entity_id);
        }
        return;
    }
    if is_item_frame {
        // 物品展示框：空框放手上物品，已有物品则顺时针旋转 45 度。
        // Item frame: place held item when empty, otherwise rotate by 45 degrees.
        for try_hand in [hand, other] {
            let held = match try_hand {
                Hand::Main => main_held.clone(),
                Hand::Off => off_held.clone(),
            };
            // `ItemFrame.interact`：空框放带 `map_id` 的地图前，若地图数据
            // `isTrackedCountOverLimit(256)` 则返 `FAIL`，不放入。
            // `ItemFrame.interact`: inserting a map into an empty frame fails when
            // the saved data is over the tracked-decoration limit (256).
            if frame_empty
                && let Some(map_id) = crate::map::stack_map_id(&held)
                && region
                    .global
                    .maps
                    .get(map_id)
                    .is_some_and(|data| data.is_tracked_count_over_limit(256))
            {
                swing(region, id, try_hand, InteractionResult::Fail);
                return;
            }
            let (outcome, effects) = {
                let (mut level, ticks) = region.block_level();
                let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
                let out = world.interact_item_frame(entity_id, &held, creative);
                (out, world.into_effects())
            };
            send_effects(region, Some(id), &effects);
            if outcome.consume_held {
                inventory::apply_item_exchange(
                    region,
                    id,
                    try_hand,
                    &mc_bamboo_blocks::ItemExchange::Shrink,
                );
            }
            if !outcome.result.consumes_action() {
                continue;
            }
            swing(region, id, try_hand, outcome.result);
            return;
        }
        return;
    }
    if is_knot {
        // 拴绳结：剪刀剪断绳子，或把玩家 / 结之间的生物转来转去。
        for try_hand in [hand, other] {
            let held = match try_hand {
                Hand::Main => main_held.clone(),
                Hand::Off => off_held.clone(),
            };
            let result = crate::durability::run(region, id, try_hand, |region| {
                let (mut level, ticks) = region.block_level();
                let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
                world.interact_knot(
                    entity_id,
                    id.0,
                    player_uuid,
                    &held,
                    try_hand,
                    using_secondary_action,
                )
            });
            if !result.consumes_action() {
                continue;
            }
            swing(region, id, try_hand, result);
            return;
        }
        return;
    }
    for try_hand in [hand, other] {
        let held = match try_hand {
            Hand::Main => main_held.clone(),
            Hand::Off => off_held.clone(),
        };
        let request = mc_bamboo_blocks::entity::mob::interact::MobInteractRequest {
            player_uid: id.0,
            player_uuid,
            hand: try_hand,
            held,
            creative,
            position,
            player_height,
            sneaking: using_secondary_action,
            difficulty: region.global.difficulty as i32,
            hit_pos: Some(location),
            spectator: game_mode == GameMode::Spectator,
        };
        // 剪刀在交互里扣耐久，所以交互期间把玩家的手交给方块逻辑。
        // Shears lose durability inside the interaction, so lend the hands to block logic.
        let outcome = crate::durability::run(region, id, try_hand, |region| {
            let (mut level, ticks) = region.block_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            world.interact_mob(entity_id, &request)
        });
        // 村民不由 `mobInteract` 处理：右键按 `Villager.mobInteract` 分支走
        // （幼年摇头、`NONE` 职业摇头 + 消耗、有职业进交易菜单）。
        // Bamboo's mobInteract does not implement villagers; handle `Villager.mobInteract`
        // here before the generic "pass" filter.
        if !using_secondary_action
            && let Some(target) = region.entities.public_at(entity_id)
            && matches!(target.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_))
            && let Some(result) =
                crate::merchant::interact(region, id, entity_id, try_hand == Hand::Main)
        {
            swing(region, id, try_hand, result);
            return;
        }
        if !outcome.result.consumes_action() {
            continue;
        }
        // `TamableAnimal.tame` / `AbstractHorse.tameWithName`：交互里当场驯服成功。
        if outcome.tamed {
            let kind = region
                .entities
                .public_at(entity_id)
                .and_then(|public| match &public.kind {
                    mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.mob_type.name()),
                    _ => None,
                })
                .unwrap_or("");
            crate::advancement_triggers::record(
                region,
                id,
                "minecraft:tame_animal",
                serde_json::json!({"entity": {"type": kind}}),
            );
        }
        // `Bucketable.bucketMobPickup`：鱼桶 / 美西螈桶 / 蝌蚪桶也是 FILLED_BUCKET。
        if outcome.filled_bucket {
            let item = outcome
                .replacement
                .as_ref()
                .map(|stack| item_name_of(stack.item))
                .unwrap_or_default();
            crate::advancement_triggers::record(
                region,
                id,
                "minecraft:filled_bucket",
                serde_json::json!({"item": item}),
            );
        }
        // `createFilledResult` 自己会消耗手里的一个，有交回物品时不再另扣。
        // `createFilledResult` consumes the held item itself, so no extra shrink then.
        if outcome.shrink_held && outcome.replacement.is_none() {
            // 狼铠修复走官方 `ItemStack.shrink`：创造模式也扣一个鳞甲。
            if outcome.shrink_even_creative {
                inventory::shrink_held_force(region, id, try_hand);
            } else {
                inventory::shrink_held(region, id, try_hand);
            }
        }
        if let Some(replacement) = outcome.replacement {
            inventory::give_filled_result(
                region,
                id,
                try_hand,
                replacement,
                outcome.limit_creative,
            );
        }
        // 马科 `openCustomInventoryScreen` 在 `mobInteract` 里当场开菜单，先于挥手。
        // Equine `openCustomInventoryScreen` opens the menu inside `mobInteract`, before the swing.
        if outcome.open_inventory {
            crate::menu::station::open_mount(region, id, entity_id);
        }
        // `doPlayerRide` 的 `startRiding`：骑手和载具两边一起改，排在开菜单之后、挥手之前。
        //
        // `startRiding` from `doPlayerRide` touches both sides; it goes after the
        // menu and before the swing.
        if outcome.ride {
            crate::riding::mount(region, id, entity_id);
        }
        // 生物事件（爱心、挤奶音效）留在实体表里，同步阶段按官方收件人发出去。
        swing(region, id, try_hand, outcome.result);
        return;
    }
}

#[cfg(test)]
mod boat_dismount_tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::{blocks, registries::entity_type as ids};
    use mc_bamboo_world::flat;

    use super::*;
    use mc_bamboo_blocks::entity::{Entity, FIRST_ENTITY_ID, mob::MobType};

    /// 气泡柱甩下来的生物乘客用船专用 `getDismountLocationForPassenger`
    /// （`getCollisionHorizontalEscapeVector` + `DismountHelper`），不是 blocks 层
    /// 缺省的 `(x, bb.maxY, z)`。
    #[test]
    fn an_ejected_mob_passenger_uses_the_boat_dismount_location() {
        let mut region = Region::new(2, 20, false);
        let position = Vec3::new(0.5, f64::from(flat::surface_y()) + 1.0, 0.5);
        let boat = region.entities.spawn(Entity::boat(ids::OAK_BOAT, position));

        let mut rider = Entity::by_mob_type(MobType::Pig, position);
        rider.vehicle = Some(boat);
        let rider_id = region.entities.spawn(rider);
        assert!(rider_id >= FIRST_ENTITY_ID);

        // 缺省落位：改回 `stop_riding` 时 mob 就会落在这里。
        let vehicle = region.entities.public_at(boat).unwrap().clone();
        let default = mc_bamboo_blocks::entity::riding::dismount_location(&vehicle);

        send_effects(
            &mut region,
            None,
            &[Effect::BoatBubbleEvent {
                entity: boat,
                event: 71,
                ejected: vec![rider_id],
            }],
        );

        let landed = region.entities.public_at(rider_id).unwrap().position;
        assert_ne!(landed, default, "生物乘客不该用缺省 (x, bb.maxY, z) 落位");
        assert!(
            (landed.x - position.x).abs() > 0.1 || (landed.z - position.z).abs() > 0.1,
            "船专用落位应沿 escape vector 水平偏移: {landed:?}"
        );
        let _ = blocks::STONE;
    }
}
