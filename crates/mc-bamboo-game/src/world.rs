//! 世界线程的主循环：20 TPS，每 tick 先处理队列里的消息，再按固定顺序推进各个系统。

use std::{
    ops::ControlFlow,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::packets::play::serverbound;
use mc_bamboo_protocol::{
    packet::play::{ClientInformation, GameMode, PlayRequest},
    text::Component,
};
use mc_bamboo_storage::{
    chunk_io::ChunkIo,
    player_data::PlayerData,
    world_dir::{encode_player_data, player_uuid_string},
};
use mc_bamboo_world::director::TicketOp;

use crate::director_host::{DirectorHost, MailBudget};
use tracing::{error, info, warn};

use crate::{
    book, chat, chunk_broadcast,
    chunk_tracking::{self, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE},
    command, container, entity_sync, entity_tracking,
    global::{Global, RosterEntry},
    inbound::{Inbound, JoinRequest, PlayerId},
    interaction::{self, UseItemOn},
    inventory, lectern, light_updates, menu, movement,
    pacing::{Pacer, WallTiming},
    player::shows_hat,
    player_save::{game_mode_from_id, restore_inventory, runtime_player_data},
    profiler::{Metric, Phase},
    region::{JoinSettings, Region},
    save_session::{SaveCommand, SaveSession},
    secure_chat::{self, ServiceProfileKeys},
    sign, weather,
    world_state::RespawnData,
};

const TICK: Duration = Duration::from_millis(50);
/// 落后超过这个时长就不追了，直接跳过去。短暂的卡顿靠连续跑 tick 追上。
const MAX_CATCH_UP: Duration = Duration::from_secs(2);
/// 信箱预算和实体线程数的上限，防住配置文件里的巨值。
pub const MAX_MAIL_BUDGET: usize = 1 << 16;
pub const MAX_ENTITY_THREADS: usize = 256;

const SERVER_SHUTDOWN: &str = "multiplayer.disconnect.server_shutdown";

/// 世界的启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldConfig {
    /// 服务端视距，合法范围是 2 到 32。
    pub view_distance: i32,
    /// 写进 `login` 包的玩家上限。满员由连接层判断，世界线程不看它。
    pub max_players: i32,
    /// 服务端有没有开正版验证。只写进 `login` 包，验证本身在连接层做。
    pub online_mode: bool,
    /// 官方 `enforce-secure-profile`，默认打开。没开正版验证、或者公钥还没取到时不生效。
    ///
    /// Official `enforce-secure-profile`, on by default. It does nothing until online mode is on
    /// and the profile keys have been fetched.
    pub enforce_secure_profile: bool,
    /// 启动时读到的管理名单，成为世界线程里的权威副本。
    pub admin: crate::admin::AdminSnapshot,
    /// 实体阶段的线程数。0 = 精确驱动（和官方逐位一致）；≥ 1 = 并行驱动。
    pub entity_threads: usize,
    /// Director 信箱每刻交付/收回的区块数上限，0 表示不限。
    pub mail_budget: MailBudget,
    /// 世界目录。`None` 表示不存档（测试、oracle、压测都走这条路）。
    ///
    /// The world directory. `None` disables saving (tests, oracle, load tests).
    pub world_dir: Option<PathBuf>,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            view_distance: 8,
            max_players: 20,
            online_mode: false,
            enforce_secure_profile: true,
            admin: crate::admin::AdminSnapshot::default(),
            entity_threads: 0,
            mail_budget: MailBudget::default(),
            world_dir: None,
        }
    }
}

/// 所有游戏状态的唯一所有者。
pub struct World {
    pub(crate) global: Global,
    pub(crate) region: Region,
    inbox: Receiver<Inbound>,
    /// 睡到下一刻用的节拍器。
    pacer: Pacer,
    /// 只有连接层起的世界才用权威 OP 名单重算进服权限。
    /// 直接嵌 `World::new` 的测试/工具按 `JoinRequest` 传入的等级走。
    authoritative_admin: bool,
    /// 实体阶段的线程数。0 = 精确驱动；≥ 1 = 并行驱动。
    pub(crate) entity_threads: usize,
    /// 区块平面的 Director 和它的宿主（内联执行器、停放表、region 消息）。
    host: DirectorHost,
    /// 上一刻每名玩家所在的区块，用来只在跨区块时发 `PlayerMoved`。
    ticket_centers: std::collections::BTreeMap<PlayerId, ChunkPos>,
    /// Director 的逻辑刻数，每次 `tick_ticket_timeouts` 加一。
    director_tick: u64,
    /// 存档会话。`None` 表示这一局不存档。
    save: Option<SaveSession>,
    /// 启动阶段打开存档失败的原因；非空时 `run` 直接拒绝启动。
    startup_error: Option<String>,
    /// 等着某道 IO 栅栏回执再发成功消息的 `save-all flush`：`(栅栏序号, 玩家)`。
    /// `save-all flush` success messages waiting on an IO barrier: `(seq, player)`.
    pending_save_success: Vec<(u64, PlayerId)>,
}

impl World {
    /// 创建一个空世界。`config` 的视距超出合法范围时取最接近的值。
    pub fn new(config: WorldConfig, inbox: Receiver<Inbound>) -> Self {
        Self::with_service_keys(config, inbox, secure_chat::empty_keys())
    }

    /// `keys` 和连接任务共用。世界线程只读，不去取。
    ///
    /// `keys` is shared with the connection tasks. The world thread only reads them.
    pub fn with_service_keys(
        config: WorldConfig,
        inbox: Receiver<Inbound>,
        keys: Arc<ServiceProfileKeys>,
    ) -> Self {
        Self::build(config, inbox, keys, None, false)
    }

    /// 连接层把启动时读到的名单和发布变更的通道交给世界线程。
    /// 名单变更作为 owned 消息发给连接层，世界线程不会等它。
    pub fn with_admin(
        config: WorldConfig,
        inbox: Receiver<Inbound>,
        keys: Arc<ServiceProfileKeys>,
        admin: crate::admin::AdminInit,
    ) -> Self {
        Self::build(config, inbox, keys, Some(admin), false)
    }

    fn build(
        config: WorldConfig,
        inbox: Receiver<Inbound>,
        keys: Arc<ServiceProfileKeys>,
        admin: Option<crate::admin::AdminInit>,
        infinite_flat: bool,
    ) -> Self {
        let view_distance = config
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
        // 启动阶段同步打开存档：建目录、拿锁、读 level.dat。失败就拒绝启动，
        // 不让世界线程带着一个没锁住的世界目录继续跑。
        // Open the save synchronously at startup: create the layout, take the lock,
        // read level.dat. On failure, refuse to start instead of running unlocked.
        let (save, startup_error) = match (&config.world_dir, infinite_flat) {
            (Some(root), false) => match SaveSession::open(root) {
                Ok((session, _source)) => (Some(session), None),
                Err(error) => {
                    let message = format!("could not open the world directory: {error}");
                    error!(%message, "refusing to start");
                    (None, Some(message))
                }
            },
            _ => (None, None),
        };
        let mut global = Global::default();
        let authoritative_admin = admin.is_some();
        if let Some(init) = admin {
            global.admin = init.snapshot.into_state();
            global.admin_messages = init.messages;
        }
        let mut region = Region::with_keys(
            view_distance,
            config.max_players,
            config.online_mode,
            config.enforce_secure_profile,
            keys,
        );
        // 启动时把读到的 `level.dat` 世界设置和 `data/` 的游戏规则 / 世界边界
        // 套回运行时；边界的 `restore` 用存档里的游戏时间。
        // Apply the loaded `level.dat` world settings and the `data/` game rules /
        // world border back onto runtime; `restore` uses the saved game time.
        if let Some(save) = &save {
            crate::save_data::apply_level_data(global.state_mut(), save.level());
            save.startup()
                .apply(&mut global, &mut region, save.level().time);
        }
        let mut world = Self {
            authoritative_admin,
            entity_threads: config.entity_threads.min(MAX_ENTITY_THREADS),
            global,
            region,
            inbox,
            pacer: Pacer,
            host: if infinite_flat {
                DirectorHost::new_infinite_flat(Default::default())
            } else {
                DirectorHost::new(
                    Default::default(),
                    MailBudget {
                        deliveries: config.mail_budget.deliveries.min(MAX_MAIL_BUDGET),
                        reclaims: config.mail_budget.reclaims.min(MAX_MAIL_BUDGET),
                    },
                )
            },
            ticket_centers: std::collections::BTreeMap::new(),
            director_tick: 0,
            save,
            startup_error,
            pending_save_success: Vec::new(),
        };
        world
            .host
            .director_mut()
            .tickets_mut()
            .set_view_distance(view_distance);
        world
            .host
            .director_mut()
            .tickets_mut()
            .set_simulation_distance(crate::interaction::SIMULATION_DISTANCE);
        if infinite_flat {
            world.region.use_infinite_flat_chunks();
        }
        // 启用存档时起一条 IO 线程并接进 Director 宿主；起不来也拒绝启动。
        // With saving enabled, start one IO thread and attach it to the host.
        if let Some(save) = &world.save {
            match ChunkIo::open(save.dir()) {
                Ok(io) => world.host.set_io(io),
                Err(error) => {
                    let message = format!("could not start the chunk IO thread: {error}");
                    error!(%message, "refusing to start");
                    world.startup_error = Some(message);
                }
            }
        }
        world.region.profiler.on_world_start();
        world
    }

    /// **测试和 oracle 专用**：创建一个「无限超平坦」世界，每个区块位置都隐式存在。
    ///
    /// 正式入口（[`new`](Self::new)、[`with_admin`](Self::with_admin)）一律用已加载模式，
    /// 区块由 Director 按 ticket 交付。这个退化实现只给不需要加载语义的单元测试、
    /// 集成测试和 oracle 用；不要在生产代码里调用。
    pub fn new_infinite_flat_for_tests(config: WorldConfig, inbox: Receiver<Inbound>) -> Self {
        Self::build(config, inbox, secure_chat::empty_keys(), None, true)
    }

    /// 票表的只读视图，给测试核对玩家票和等级用。
    #[cfg(test)]
    pub(crate) fn tickets(&self) -> &mc_bamboo_world::ticket::TicketTable {
        self.host.director().tickets()
    }

    /// 测试专用：直接改票表，跳过 `TicketOp` 的事件路径。
    #[cfg(test)]
    pub(crate) fn tickets_mut(&mut self) -> &mut mc_bamboo_world::ticket::TicketTable {
        self.host.director_mut().tickets_mut()
    }

    /// 测试专用：走一批 `TicketOp`。
    #[cfg(test)]
    pub(crate) fn apply_ticket(&mut self, op: TicketOp) {
        self.apply_ticket_ops([op]);
    }

    /// 一批票的操作。已加载模式交给 Director（会派加载/卸载任务），
    /// 无限超平坦模式只记票、不派任务。
    fn apply_ticket_ops(&mut self, ops: impl IntoIterator<Item = TicketOp>) {
        let ops: Vec<TicketOp> = ops.into_iter().collect();
        if ops.is_empty() {
            return;
        }
        for op in &ops {
            if let TicketOp::PlayerMoved { id, to } = op {
                let player = PlayerId(*id as i32);
                match to {
                    Some(pos) => self.ticket_centers.insert(player, *pos),
                    None => self.ticket_centers.remove(&player),
                };
            }
        }
        self.host.tickets(&mut self.region, ops);
    }

    /// 每刻在玩家跟踪阶段之前调用：只在玩家跨区块时发 `PlayerMoved`。
    fn sync_player_tickets(&mut self) {
        let mut moves = Vec::new();
        for (&id, player) in &self.region.players {
            let at = player.chunk_pos();
            if self.ticket_centers.get(&id) == Some(&at) {
                continue;
            }
            self.ticket_centers.insert(id, at);
            moves.push(TicketOp::PlayerMoved {
                id: u64::from(id.0 as u32),
                to: Some(at),
            });
        }
        let mut left = Vec::new();
        for &id in self.ticket_centers.keys() {
            if !self.region.players.contains_key(&id) {
                left.push(id);
            }
        }
        for id in left {
            self.ticket_centers.remove(&id);
            moves.push(TicketOp::PlayerMoved {
                id: u64::from(id.0 as u32),
                to: None,
            });
        }
        if !moves.is_empty() {
            self.apply_ticket_ops(moves);
        }
    }

    /// 超时票每刻递减。`is_busy` 现在恒为假（区块都隐式存在），
    /// 提交 5 接 Director 后按「有在途任务/未交付」填。
    fn tick_ticket_timeouts(&mut self) {
        self.director_tick += 1;
        self.host.tick(&mut self.region, self.director_tick);
    }

    /// 阻塞运行，直到收到 [`Inbound::Shutdown`] 或者所有发送端都没了。
    pub fn run(mut self) {
        if let Some(message) = &self.startup_error {
            error!(%message, "world startup failed; not running");
            return;
        }
        let mut timing = WallTiming;
        let mut schedule = Schedule::starting_at(self.region.profiler.now());
        loop {
            let expected = schedule.expected_start();
            let actual = self.region.profiler.now();
            let skipped = schedule.skipped_catch_up();
            let catch_up = schedule.catching_up();
            self.region
                .profiler
                .begin_scheduled_tick(expected, actual, skipped, catch_up);
            if self.tick().is_break() {
                break;
            }
            schedule.after_tick(self.region.profiler.now());
            self.pacer.wait_until(schedule.deadline(), &mut timing);
        }
    }

    /// 推进一 tick。顺序有约束：光照和官方一样在处理完请求之后、下一 tick 之前算，计划刻、随机刻和
    /// 方块实体的 tick 紧跟在时间增长之后（官方 `ServerLevel.tick` 的顺序），新区块排在区块中心和
    /// forget 后面，每个区块的光照变化紧挨在它的方块变化前面，方块变化排在 ack 前面，
    /// 区块批次放在最后，这样发出去的内容是这一 tick 结束时的世界。
    fn tick(&mut self) -> ControlFlow<()> {
        self.region.profiler.begin_tick();
        self.record_tick_counts();
        let flow = self.tick_phases();
        self.region.profiler.end_tick();
        flow
    }

    /// 每刻开始时的数量指标，趁状态还没变先记下来。到期的计划刻数是
    /// 队列里 `trigger_tick <= now + 1` 的那些（`Global::advance_time` 在计划刻之前加一）。
    /// 队列按到期时刻排好了，所以只看开头到期的那一段，不扫整条队列。
    fn record_tick_counts(&mut self) {
        let next_time = self.global.time().0 + 1;
        let due = self
            .region
            .block_ticks
            .pending()
            .take_while(move |tick| tick.trigger_tick <= next_time)
            .count()
            + self
                .region
                .block_ticks
                .pending_fluids()
                .take_while(move |tick| tick.trigger_tick <= next_time)
                .count();
        let profiler = &mut self.region.profiler;
        profiler.record(Metric::OnlinePlayers, self.region.players.len() as u64);
        profiler.record(Metric::Entities, self.region.entities.len() as u64);
        profiler.record(Metric::DueScheduledTicks, due as u64);
        profiler.record(
            Metric::LoadedChunks,
            self.region.chunks.loaded_count().unwrap_or(0) as u64,
        );
    }

    /// 拆成 `tick` 和这一段只是为了保证提前返回时也能收尾；顺序仍然是原来的顺序。
    fn tick_phases(&mut self) -> ControlFlow<()> {
        self.host.begin_tick();
        // 上一刻发出去的读盘请求，结果在这一刻开头经信箱收回。
        // Read requests dispatched last tick are collected at the start of this tick.
        self.host.poll_io(&mut self.region);
        // 收回已经到达的 IO 栅栏回执，发等在上面的 `save-all flush` 成功消息。
        // Release `save-all flush` success messages whose IO barrier acked.
        let flushed = self.host.take_flushed_barriers();
        self.release_flushed_save_success(flushed);
        self.process_inbox()?;
        self.consume_save_commands();
        self.sync_player_tickets();
        self.region.profiler.lap(Phase::Inbox);
        self.tick_ticket_timeouts();
        let (delivered, reclaimed) = self.host.counters_this_tick();
        let profiler = &mut self.region.profiler;
        profiler.record(Metric::ChunksDelivered, delivered as u64);
        profiler.record(Metric::ChunksReclaimed, reclaimed as u64);
        profiler.record(Metric::ParkedChunks, self.host.parked_count() as u64);
        profiler.lap(Phase::Delivery);
        light_updates::run_light_updates(&mut self.region);
        self.region.profiler.lap(Phase::LightUpdates);

        // 官方 `ServerLevel.tick` 开头的 "world border" 阶段在天气和时间之前。
        // 渐变推进后把新快照发布给 region，这一刻的方块逻辑和实体碰撞都读它。
        //
        // Vanilla `ServerLevel.tick`'s "world border" phase comes before weather and time.
        // After the lerp step, publish the new snapshot; this tick's block logic and
        // entity collisions read it.
        self.global.tick_world_border();
        self.global.publish_border(&mut self.region);
        // 官方 `ServerLevel.tick` 的天气阶段在 `tickTime` 之前；广播帧排在时间包之前发出。
        let weather = weather::advance(&mut self.global, &self.region);
        if !weather.is_empty() {
            self.region.links.broadcast(&weather);
        }
        weather::sync(&self.global, &mut self.region);
        self.global.advance_time();
        self.region.advance_tick();
        self.region.profiler.lap(Phase::WeatherAndTime);
        // 官方 `ServerLevel.tick` 的顺序：方块和流体的计划刻、`chunkSource.tick` 里的随机刻、
        // 方块事件、实体、方块实体。计划刻和随机刻里生成的下落方块和 TNT 同一 tick 就动。
        //
        // Vanilla `ServerLevel.tick` order: block and fluid ticks, random ticks inside
        // `chunkSource.tick`, block events, entities, then block entities. Falling blocks and
        // primed TNT spawned by block or random ticks move in the same game tick.
        interaction::run_block_ticks(&mut self.region, self.global.time());
        self.region.profiler.lap(Phase::ScheduledTicks);
        interaction::run_random_ticks(&mut self.region);
        self.region.profiler.lap(Phase::RandomTicks);
        interaction::run_block_events(&mut self.region);
        self.region.profiler.lap(Phase::BlockEvents);
        interaction::run_entity_ticks(self.entity_threads, &mut self.region);
        if self.entity_threads == 0 {
            self.region.profiler.lap(Phase::Entities);
        }
        interaction::run_block_entity_ticks(&mut self.region);
        self.region.profiler.lap(Phase::BlockEntities);
        // 玩家自己每 tick 的挖掘进度。官方在实体 tick 里跑 `ServerPlayerGameMode.tick`，
        // 这里跟着玩家 tick 放在随机刻之后。
        interaction::tick_mining(&mut self.region);
        // 方块实体的 tick：告示牌在这里清掉走远的编辑者。
        sign::tick_editors(&mut self.region);
        // 容器开合人数的重查和菜单同步。
        container::tick(&mut self.region);
        menu::tick(&mut self.region);
        lectern::tick(&mut self.region);
        chat::tick_spam_throttlers(&mut self.region);
        crate::drops::tick_drop_throttle(&mut self.region);
        crate::command::admin::tick_idle(&mut self.global, &mut self.region);
        // 官方 `ServerPlayer.doTick` 的六个只读准则。Bamboo 用玩家初值，进服后第一刻各写一次。
        //
        // The six read-only criteria from official `ServerPlayer.doTick`. Bamboo uses the
        // player defaults and writes each once after join.
        self.global.scoreboard_mut().tick_criteria(&mut self.region);
        self.region.profiler.lap(Phase::PlayerMisc);

        entity_sync::update_derived_state(&mut self.region);
        chunk_tracking::update_views(&mut self.region);
        self.region.profiler.lap(Phase::Views);
        chunk_broadcast::broadcast_changes(&mut self.region);
        self.region.profiler.lap(Phase::BlockChangeBroadcast);
        entity_sync::sync_entities(&mut self.region);
        entity_tracking::sync(&mut self.region);
        entity_sync::send_equipment_changes(&mut self.region);
        interaction::send_block_changed_acks(&mut self.region);
        if let Some(frames) = self.global.tick_latency_broadcast() {
            self.region.links.broadcast(&frames);
        }
        self.region.profiler.lap(Phase::EntitySyncAndTracking);
        chunk_tracking::send_chunks(&mut self.region);
        movement::finish_tick(&mut self.region);
        self.region.profiler.lap(Phase::ChunkSending);
        let flush_bytes = self.region.links.flush();
        self.region.profiler.record(Metric::FlushBytes, flush_bytes);
        self.region.profiler.lap(Phase::Flush);
        // 运行中每 6000 tick 自动保存一次（官方 `autoSave`，`save-on` 控制开关）。
        // Run-time autosave every 6000 ticks (official `autoSave`, gated by save-on).
        if self.director_tick > 0
            && self.director_tick.is_multiple_of(6_000)
            && self.save.as_ref().is_some_and(|save| save.auto_save())
        {
            self.save_all_players();
            let saved = self
                .save
                .as_mut()
                .map(|save| save.save_all(&self.global, crate::admin::now_millis()));
            if let Some(Err(error)) = saved {
                warn!(%error, "could not auto-save level data");
            }
        }
        // 运行中匀速给仍加载的脏区块拍快照写盘（D6），`save-off` 时停。
        // 区块平时由这条平摊的 IO 保存，不再等 6000 刻集中刷，世界线程也不等待。
        // Paced run-time snapshots of still-loaded dirty chunks (D6), off during
        // save-off. Chunks are saved by this flat IO stream rather than a
        // 6000-tick burst; the world thread never waits.
        let save_enabled = self.save.as_ref().is_some_and(|save| save.auto_save());
        self.host
            .save_loaded_dirty(&mut self.region, self.director_tick, save_enabled);
        if self.global.admin.stop_requested {
            let flow = self.shut_down();
            self.region.profiler.lap(Phase::TickTail);
            return flow;
        }
        // 名单变更在这一 tick 末尾统一发布：一条命令改多个目标只写一次盘。
        let changes = crate::command::admin::take_changes(&mut self.region);
        if changes.any() {
            self.publish_admin(changes);
        }
        self.region.profiler.lap(Phase::TickTail);
        ControlFlow::Continue(())
    }

    /// 把名单快照交给连接层写盘。通道不阻塞，满了只记日志。
    fn publish_admin(&self, changes: crate::admin::AdminChanges) {
        let Some(sender) = &self.global.admin_messages else {
            return;
        };
        let update = crate::admin::AdminUpdate {
            snapshot: self.global.admin.snapshot(),
            changes,
        };
        if sender
            .send(crate::admin::AdminMessage::Update(update))
            .is_err()
        {
            warn!("admin list update could not reach the connection layer");
        }
    }

    /// 按到达顺序处理这一 tick 之前收到的所有消息。
    fn process_inbox(&mut self) -> ControlFlow<()> {
        // 先全部取出再处理：配额要处理完才归还，连接任务补不进新消息，所以这一步一定会结束。
        let mut messages = Vec::new();
        let disconnected = loop {
            match self.inbox.try_recv() {
                Ok(message) => messages.push(message),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };

        self.region
            .profiler
            .record(Metric::InboundMessages, messages.len() as u64);
        for message in messages {
            match message {
                Inbound::Join(request) => self.on_join(*request),
                Inbound::Request {
                    player,
                    request,
                    in_flight,
                } => {
                    self.on_request(player, request);
                    drop(in_flight);
                }
                Inbound::Latency { player, millis } => self.global.set_latency(player, millis),
                Inbound::Leave { player } => self.on_leave(player),
                Inbound::WhitelistReload(reload) => {
                    self.global.admin.whitelist = reload.entries;
                    crate::command::admin::after_reload(&mut self.global, &mut self.region);
                }
                Inbound::Shutdown => return self.shut_down(),
            }
        }
        if disconnected {
            return self.shut_down();
        }
        ControlFlow::Continue(())
    }

    /// 把进服的 10 步串起来，全部写进这名玩家的同一条出站缓冲。`coverage.md` 4.1。
    fn on_join(&mut self, mut request: JoinRequest) {
        let id = request.id;
        if self.region.players.contains_key(&id) {
            warn!(player = id.0, "ignoring a second join with the same id");
            return;
        }
        // 连接层检查过一次，但封禁命令可能在两次检查之间改了名单。
        // 世界线程按自己的权威副本复查，理由和登录阶段完全一样。
        let refusal = if self.authoritative_admin {
            self.global.admin.login_refusal(
                request.uuid,
                &request.peer_ip,
                self.region.now_millis(),
                request.online_at_login,
                self.region.max_players() as u32,
            )
        } else {
            None
        };
        if let Some(refusal) = refusal {
            let reason = crate::command::admin::refusal_message(&refusal);
            self.region.links.add(id, request.sink);
            self.region.links.kick(id, &reason);
            return;
        }
        let (permission_level, is_op) = if self.authoritative_admin {
            let op = self.global.admin.op(request.uuid);
            (op.map_or(0, |entry| entry.level), op.is_some())
        } else {
            (request.permission_level, request.is_op)
        };
        // 官方 `CachedUserNameToIdResolver.add` 在进服时记下档案。
        self.global.admin.remember_player(&crate::admin::NameAndId {
            uuid: request.uuid,
            name: request.name.clone(),
        });

        // 恢复玩家存档：位置取整到方块、朝向、游戏模式、物品栏。
        // 位置的小数部分丢掉（不额外补发位置包），见 README 已知限制。
        // 离开时的写交给 IO 线程异步做，重进可能早于落盘：优先用世界线程
        // 记下的那份"已交给 IO、还没写完"的内存副本，而不是连接层读到的盘上旧字节。
        // Restore from the player save: block-rounded position, rotation, game
        // mode and inventory. The sub-block fraction is dropped (no extra
        // position packet); see README known limitations. A leave is written
        // asynchronously, so a rejoin can beat the disk: prefer the world
        // thread's in-flight in-memory copy over the connection-layer disk bytes.
        let uuid_string = player_uuid_string(request.uuid.as_bytes());
        let player_bytes = self
            .host
            .pending_player(&uuid_string)
            .map(<[u8]>::to_vec)
            .or_else(|| request.player_data.take());
        let restored = player_bytes.and_then(|bytes| match PlayerData::from_gzip(&bytes) {
            Ok(data) => Some(data),
            Err(error) => {
                warn!(%error, "could not decode player data; treating as a new player");
                None
            }
        });
        let game_mode = restored
            .as_ref()
            .map(|data| game_mode_from_id(data.game_type))
            .unwrap_or(self.global.state().default_game_mode);
        let join_spawn = match &restored {
            Some(data) => RespawnData::of(
                BlockPos::new(
                    data.position[0] as i32,
                    data.position[1] as i32,
                    data.position[2] as i32,
                ),
                data.rotation[0],
                data.rotation[1],
            ),
            None => self.global.state().spawn,
        };
        let uuid = request.uuid;
        let entry = RosterEntry {
            uuid,
            name: request.name.clone(),
            // 皮肤只有名单要用，直接搬过来。
            properties: std::mem::take(&mut request.properties),
            game_mode,
            latency: 0,
            show_hat: shows_hat(&request.client),
            chat_session: None,
        };

        let difficulty_frames = self.global.difficulty_frames();
        let spawn = join_spawn.pos;
        let spawn_chunk = ChunkPos::new(spawn.x >> 4, spawn.z >> 4);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: Some(spawn_chunk),
        }]);
        self.region.add_player(
            request,
            JoinSettings {
                game_mode,
                difficulty_frames: &difficulty_frames,
                spawn: join_spawn,
                hashed_seed: self.global.state().hashed_seed(),
                permission_level,
                is_op,
            },
        );
        self.region
            .last_action_millis
            .insert(id, self.region.now_millis());

        if let Some(frames) = self.global.tab_list() {
            self.region.links.send_frames(id, &frames);
        }
        let frames = self.global.add_player(id, entry);
        self.region.links.broadcast(&frames);

        self.region.send_own_entity_data(id);

        if let Some(frames) = self.global.joined_message(id) {
            // 官方 `PlayerList.broadcastSystemMessage` 逐玩家看可见性；进服消息跳过本人。
            //
            // Official `PlayerList.broadcastSystemMessage` checks visibility per player;
            // the join message skips the joining player.
            crate::chat::broadcast_system_message(&mut self.region, &frames, Some(id));
        }

        // 官方 `ServerScoreboard.getStartTrackingPackets` 和 `CustomBossEvents.onPlayerConnect`
        // 都在进服时只发给这名玩家。
        //
        // Official `ServerScoreboard.getStartTrackingPackets` and
        // `CustomBossEvents.onPlayerConnect` both go only to the joining player.
        let frames = self.global.scoreboard().join_frames();
        self.region.links.send_frames(id, &frames);
        self.global
            .boss_bars_mut()
            .on_join(&mut self.region, id, uuid);

        // `set_default_spawn_position` 发的是世界出生点（`LevelData`），不是玩家存档里的位置。
        // 玩家位置走上面的 `JoinSettings.spawn`（存档），区块中心走 `player.chunk_pos()`。
        // `set_default_spawn_position` carries the world spawn from `LevelData`, not
        // the saved player position. The player's position comes from
        // `JoinSettings.spawn` above (the save), and the chunk center from `player.chunk_pos()`.
        self.region.start_loading_chunks(
            id,
            self.global.time(),
            self.global.clock(),
            self.global.state().spawn,
            self.global.state().weather,
            self.global.world_border(),
        );

        // 物品栏 / 末影箱 / 朝向等运行时字段在玩家建好之后写回去。
        if let Some(data) = &restored
            && let Some(player) = self.region.players.get_mut(&id)
        {
            restore_inventory(player, data);
        }
    }

    /// 消费 `save-all` / `save-on` / `save-off` 压进来的请求（命令动作不碰 IO）。
    fn consume_save_commands(&mut self) {
        let requests = std::mem::take(&mut self.global.save_requests);
        for request in requests {
            match request {
                SaveCommand::SaveAll { flush, source } => {
                    // 照官方 saveEverything：先给所有脏的已加载区块拍快照交给 IO 线程，
                    // 再存玩家、level.dat、data/。到这一步所有快照都已入队。
                    // Like official saveEverything: snapshot every loaded dirty
                    // chunk to the IO thread, then save players, level.dat, data/.
                    self.host
                        .snapshot_all_dirty(&mut self.region, self.director_tick);
                    self.save_all_players();
                    let saved = self
                        .save
                        .as_mut()
                        .map(|save| save.save_all(&self.global, crate::admin::now_millis()));
                    if let Some(Err(error)) = saved {
                        warn!(%error, "save-all could not write level data");
                    }
                    if flush {
                        // 有 flush：排一道不阻塞的 IO 栅栏，成功消息等它回执再发。
                        // With flush: enqueue a non-blocking barrier; send success
                        // only after its ack, so the world thread never waits.
                        if let Some(seq) = self.host.submit_flush_barrier() {
                            self.pending_save_success.push((seq, source));
                        } else {
                            self.send_save_success(source);
                        }
                    } else {
                        // 没 flush：快照都交出去了，此刻发成功消息。
                        // Without flush: every snapshot is handed off, send now.
                        self.send_save_success(source);
                    }
                }
                SaveCommand::SaveOn => {
                    if let Some(save) = &mut self.save {
                        save.set_auto_save(true);
                    }
                }
                SaveCommand::SaveOff => {
                    if let Some(save) = &mut self.save {
                        save.set_auto_save(false);
                    }
                }
            }
        }
    }

    /// 发 `commands.save.success`（广播给 OP），和命令侧原来的一致。
    /// Send `commands.save.success` (broadcast to ops), as the command did.
    fn send_save_success(&mut self, source: PlayerId) {
        crate::command::feedback::success(
            &mut self.region,
            source,
            Component::translate("commands.save.success", []),
            true,
        );
    }

    /// 收回已到达的 IO 栅栏回执，发等在上面的 `save-all flush` 成功消息。
    /// Release `save-all flush` success messages whose IO barrier has acked.
    fn release_flushed_save_success(&mut self, flushed: Vec<u64>) {
        if flushed.is_empty() {
            return;
        }
        let mut remaining = Vec::new();
        for (seq, source) in std::mem::take(&mut self.pending_save_success) {
            if flushed.contains(&seq) {
                self.send_save_success(source);
            } else {
                remaining.push((seq, source));
            }
        }
        self.pending_save_success = remaining;
    }

    /// 存档一名在线玩家。文件写交给 IO 线程，世界线程不等待。
    fn save_one_player(&mut self, id: PlayerId) {
        let encoded = self.region.players.get(&id).and_then(encode_player_save);
        if let Some((uuid, bytes)) = encoded {
            self.host.submit_player_write(&uuid, bytes);
        }
    }

    /// 存档所有在线玩家（自动保存、关服、`save-all`）。
    fn save_all_players(&mut self) {
        // 先把所有存档编出来，避免遍历 `region.players` 时又可变借 `host`。
        // Encode every save first so we don't hold a borrow of `region.players`
        // while mutably borrowing `host`.
        let encoded: Vec<(String, Vec<u8>)> = self
            .region
            .players
            .values()
            .filter_map(encode_player_save)
            .collect();
        for (uuid, bytes) in encoded {
            self.host.submit_player_write(&uuid, bytes);
        }
    }

    /// 处理离开，顺序见 `coverage.md` 4.2。可以重复调用：世界里没有这名玩家时直接忽略，
    /// 连接任务还没发 `Join` 就结束时走的就是这条路。
    fn on_leave(&mut self, id: PlayerId) {
        if !self.region.players.contains_key(&id) {
            return;
        }
        // 连接已经结束了，后面的包都不用写给它。
        self.region.links.remove(id);
        self.region.last_action_millis.remove(&id);
        self.region.peer_ips.remove(&id);

        if let Some(frames) = self.global.left_message(id) {
            // 官方 `PlayerList.broadcastSystemMessage` 逐玩家看可见性。
            //
            // Official `PlayerList.broadcastSystemMessage` checks visibility per player.
            crate::chat::broadcast_system_message(&mut self.region, &frames, None);
        }
        entity_sync::on_leave(&mut self.region, id);
        entity_tracking::on_leave(&mut self.region, id);
        // 玩家断开时官方不回 `container_close`，光标上的东西直接丢弃。
        menu::on_leave(&mut self.region, id);
        lectern::on_leave(&mut self.region, id);
        // 官方玩家断开时 `entityRemoved` 直接 return，不广播 reset_score；
        // 分数和 Boss 栏的 UUID 集合都留着，下次进服还会补发 add 包。
        //
        // Official `entityRemoved` returns for players without broadcasting reset_score;
        // scores and boss-bar UUID sets stay so rejoining gets the add packet again.
        if let Some(name) = self
            .region
            .players
            .get(&id)
            .map(|player| player.name.clone())
        {
            self.global.scoreboard_mut().on_leave(&name);
            self.global.boss_bars_mut().on_leave(id);
        }
        // 官方 `PlayerList.remove` 在移除玩家前先存盘。
        // Official `PlayerList.remove` saves the player before removing them.
        self.save_one_player(id);
        self.region.players.remove(&id);
        self.region.player_grid.remove(id);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: None,
        }]);
        if let Some(frames) = self.global.remove_player(id) {
            self.region.links.broadcast(&frames);
        }
    }

    /// 把一条请求交给负责它的模块。被踢的玩家要等 `Leave` 才离开世界，这段时间它的请求都忽略。
    fn on_request(&mut self, player: PlayerId, request: PlayRequest) {
        if !self.region.links.is_open(player) {
            return;
        }
        crate::command::admin::note_action(&mut self.region, player, &request);
        match &request {
            PlayRequest::ChatCommand { command, signed } => {
                let command = command.clone();
                let signed = signed.clone();
                command::on_command(
                    &mut self.global,
                    &mut self.region,
                    player,
                    &command,
                    signed.as_ref(),
                );
                return;
            }
            PlayRequest::CommandSuggestion { id, command } => {
                let (transaction, command) = (*id, command.clone());
                command::on_suggestion(&mut self.region, player, transaction, &command);
                return;
            }
            PlayRequest::ChatSession(session) => {
                let session = session.clone();
                if let Some(update) = secure_chat::on_session(&mut self.region, player, session)
                    && let Some(frames) = self.global.initialize_chat(player, &update)
                {
                    self.region.links.broadcast(&frames);
                }
                return;
            }
            _ => {}
        }
        let region = &mut self.region;
        match request {
            PlayRequest::AcceptTeleportation {
                id,
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                let position = Vec3::new(x, y, z);
                movement::on_accept_teleportation(region, player, id, position, (yaw, pitch));
            }
            PlayRequest::MovePlayer {
                position,
                rotation,
                on_ground,
                horizontal_collision,
            } => movement::on_move_player(
                region,
                player,
                position,
                rotation,
                on_ground,
                horizontal_collision,
            ),
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick,
            } => chunk_tracking::on_chunk_batch_received(region, player, desired_chunks_per_tick),
            PlayRequest::ClientInformation(information) => {
                self.on_client_information(player, information);
            }
            PlayRequest::Chat {
                message,
                signature,
                time_millis,
                salt,
                last_seen,
            } => {
                if chat::on_chat(
                    region,
                    player,
                    chat::ChatMessage {
                        global: &self.global,
                        text: message,
                        signature: signature.map(|bytes| *bytes),
                        time_millis,
                        salt,
                        last_seen: &last_seen,
                    },
                ) {
                    chat::detect_chat_rate_spam(region, player);
                }
            }
            PlayRequest::ChatAck { offset } => secure_chat::on_ack(region, player, offset),
            // 聊天命令、建议和会话更新在上面处理过了，这里只是让匹配穷尽。
            //
            // Chat commands, suggestions, and session updates are handled above. This arm only makes the match exhaustive.
            PlayRequest::ChatSession(_)
            | PlayRequest::ChatCommand { .. }
            | PlayRequest::CommandSuggestion { .. } => {}
            PlayRequest::ClientCommand { action } => {
                if action
                    == mc_bamboo_protocol::packet::play::ClientCommandKind::RequestGameRuleValues
                {
                    crate::gamerules::on_request_values(&self.global, &mut self.region, player);
                } else {
                    chat::on_client_command(&mut self.region, player, action);
                }
            }
            PlayRequest::ChangeGameMode { mode } => self.on_change_game_mode(player, mode),
            PlayRequest::SetGameRule { entries } => {
                crate::gamerules::on_set_packet(
                    &mut self.global,
                    &mut self.region,
                    player,
                    entries,
                );
            }
            PlayRequest::SetCommandBlock {
                pos,
                command,
                mode,
                track_output,
                conditional,
                automatic,
            } => crate::command_block::on_set(
                region,
                player,
                crate::command_block::CommandBlockEdit {
                    pos,
                    command,
                    mode,
                    track_output,
                    conditional,
                    automatic,
                },
            ),
            PlayRequest::PlayerInput { flags } => {
                entity_sync::on_player_input(region, player, flags);
            }
            PlayRequest::PlayerCommand { action } => {
                entity_sync::on_player_command(region, player, action);
            }
            PlayRequest::PlayerAbilities { flags } => {
                entity_sync::on_player_abilities(region, player, flags);
            }
            PlayRequest::Punch => entity_sync::on_punch(region, player),
            PlayRequest::SetCarriedItem { slot } => {
                inventory::on_set_carried_item(region, player, slot);
            }
            PlayRequest::SetCreativeModeSlot { slot, stack } => {
                inventory::on_set_creative_mode_slot(region, player, slot, stack);
            }
            PlayRequest::PickItemFromBlock { pos, include_data } => {
                inventory::on_pick_item_from_block(region, player, pos, include_data);
            }
            PlayRequest::PlayerAction {
                action,
                pos,
                face,
                sequence,
            } => interaction::on_player_action(region, player, action, pos, face, sequence),
            PlayRequest::UseItemOn {
                hand,
                pos,
                face,
                cursor,
                inside,
                sequence,
                ..
            } => {
                let packet = UseItemOn {
                    hand,
                    pos,
                    face,
                    cursor,
                    inside,
                    sequence,
                };
                interaction::on_use_item_on(region, player, packet);
            }
            PlayRequest::UseItem {
                hand,
                sequence,
                yaw,
                pitch,
            } => interaction::on_use_item(region, player, hand, sequence, (yaw, pitch)),
            PlayRequest::ContainerClick {
                container_id,
                state_id,
                slot,
                button,
                input,
                changed_slots,
                carried,
            } => menu::on_click(
                region,
                player,
                menu::ClickPacket {
                    container_id,
                    state_id,
                    slot,
                    button,
                    input,
                    changed_slots,
                    carried,
                },
            ),
            PlayRequest::ContainerClose { .. } => {
                menu::on_close_request(region, player);
                lectern::on_player_close(region, player);
            }
            PlayRequest::RenameItem { name } => menu::on_rename(region, player, &name),
            PlayRequest::ContainerButtonClick {
                container_id,
                button_id,
            } => {
                menu::on_button(region, player, container_id, button_id);
                lectern::on_button(region, player, container_id, button_id);
            }
            PlayRequest::ContainerSlotStateChanged {
                container_id,
                slot_id,
                new_state,
            } => menu::on_slot_state_changed(region, player, container_id, slot_id, new_state),
            PlayRequest::SignUpdate { pos, lines, slot } => {
                sign::on_sign_update(region, player, pos, lines, slot);
            }
            PlayRequest::EditBook { slot, pages, title } => {
                book::on_edit_book(region, player, slot, pages, title);
            }
            // 连接任务把这两种转过来。其余 `Other` 它自己丢掉。
            //
            // The connection forwards these two. It drops every other `Other` packet.
            PlayRequest::Other(id) if id == serverbound::CLIENT_TICK_END => {
                movement::on_client_tick_end(region, player);
            }
            PlayRequest::Other(id) if id == serverbound::PLAYER_LOADED => {
                movement::on_player_loaded(region, player);
            }
            PlayRequest::KeepAlive(_) | PlayRequest::PingRequest(_) | PlayRequest::Other(_) => {}
        }
    }

    /// `handleChangeGameMode`。权限不够时只记日志。
    ///
    /// `handleChangeGameMode`. Without permission this only logs.
    fn on_change_game_mode(&mut self, id: PlayerId, mode: GameMode) {
        let Some((allowed, name)) = self.region.players.get(&id).map(|player| {
            (
                player.permission_level >= mc_bamboo_command::permission::GAMEMASTERS,
                player.name.clone(),
            )
        }) else {
            return;
        };
        if !allowed {
            warn!(
                player = %name,
                ?mode,
                "tried to change game mode without required permissions"
            );
            return;
        }
        command::change_own_mode(&mut self.global, &mut self.region, id, mode);
    }

    /// 客户端的设置变了。视距由下一次跟踪阶段生效，皮肤外层和主手由实体同步发出去。
    fn on_client_information(&mut self, id: PlayerId, information: ClientInformation) {
        let Some(player) = self.region.players.get_mut(&id) else {
            return;
        };
        player.client = information;

        let show_hat = player.shows_hat();
        if let Some(frames) = self.global.set_hat(id, show_hat) {
            self.region.links.broadcast(&frames);
        }
    }

    /// 按官方 `stopServer` 顺序关服：先存玩家、再存区块（后续步骤接入 IO 线程后补），
    /// 然后写 `level.dat` / `data/` / `chunk_tickets.dat`，最后释放锁、踢人。
    /// 任何一步出错都只记日志，不让关服挂死。
    ///
    /// Shut down in the official `stopServer` order: save players, then chunks
    /// (added with the IO thread in a later step), then write `level.dat`, `data/`
    /// and `chunk_tickets.dat`, release the lock, and finally kick everyone.
    /// Every step only logs on error so shutdown can never hang.
    fn shut_down(&mut self) -> ControlFlow<()> {
        // 官方顺序：先存所有在线玩家，再存区块。
        // Official order: save all online players first, then chunks.
        self.save_all_players();
        // 再存所有区块：Director 收回全部已加载区块、写盘、排空 IO 线程并 sync_all。
        // Save all chunks first: reclaim every loaded chunk, write, drain and sync_all.
        self.host.shutdown(&mut self.region);
        if let Some(save) = &mut self.save {
            if let Err(error) = save.save_all(&self.global, crate::admin::now_millis()) {
                warn!(%error, "could not save level data on shutdown");
            }
            info!(
                codec_nanos = crate::director_host::codec_nanos(),
                "chunk codec time on this run"
            );
            save.close();
        }
        let reason = Component::translate(SERVER_SHUTDOWN, []);
        self.region.links.kick_all(&reason);
        ControlFlow::Break(())
    }
}

/// 编出一名在线玩家的存档字节（纯函数，不碰 `self`，避免和 `host` 的可变借用冲突）。
/// 返回带连字符的 uuid 和 gzip 字节，交给 [`World::save_all_players`] 提交给 IO 线程。
/// Encode one live player's save (pure, so it does not conflict with the mutable
/// borrow of `host`). Returns the hyphenated uuid and gzip bytes for submission.
fn encode_player_save(player: &crate::player::Player) -> Option<(String, Vec<u8>)> {
    let data = runtime_player_data(player);
    match encode_player_data(&data) {
        Ok(bytes) => Some((player_uuid_string(player.uuid.as_bytes()), bytes)),
        Err(error) => {
            warn!(%error, "could not encode player data");
            None
        }
    }
}

/// 按绝对时间排下一 tick，sleep 的误差和 tick 本身的耗时都不会累积。
/// 第一刻立刻跑，`next_tick` 是第二刻的起点，之后每刻按 50ms 网格推进。
struct Schedule {
    next_tick: Instant,
    /// 本刻按计划应该几点开始；追赶时它落后于实际开始时刻。
    expected_start: Instant,
    /// 上一次 `after_tick` 有没有因为落后太多而放弃追赶。
    skipped_catch_up: bool,
    /// 上一次 `after_tick` 要不要连续跑（没睡）。
    catching_up: bool,
}

impl Schedule {
    /// 第一 tick 在 `now` 立刻开始，第二刻不早于 `now + TICK`。
    fn starting_at(now: Instant) -> Self {
        Self {
            next_tick: now + TICK,
            expected_start: now,
            skipped_catch_up: false,
            catching_up: false,
        }
    }

    /// 这一刻应该几点开始。
    fn expected_start(&self) -> Instant {
        self.expected_start
    }

    /// `after_tick` 算出的下一刻截止时刻。
    fn deadline(&self) -> Instant {
        self.expected_start
    }

    fn skipped_catch_up(&self) -> bool {
        self.skipped_catch_up
    }

    fn catching_up(&self) -> bool {
        self.catching_up
    }

    /// 一 tick 结束时调用，返回到下一 tick 开始还要等多久。已经落后时返回 0，连续跑 tick 追上。
    fn after_tick(&mut self, now: Instant) -> Duration {
        let behind = now.saturating_duration_since(self.next_tick);
        self.skipped_catch_up = behind > MAX_CATCH_UP;
        if self.skipped_catch_up {
            warn!(
                "Can't keep up! Running {}ms behind, skipping {} ticks",
                behind.as_millis(),
                behind.as_millis() / TICK.as_millis()
            );
            self.next_tick = now;
        }
        let wait = self.next_tick.saturating_duration_since(now);
        self.catching_up = wait.is_zero();
        self.expected_start = self.next_tick;
        self.next_tick += TICK;
        wait
    }
}

#[cfg(test)]
mod loaded_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod ticket_tests;
