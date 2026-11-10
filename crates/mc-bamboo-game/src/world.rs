//! 世界线程的主循环：20 TPS，每 tick 先处理队列里的消息，再按固定顺序推进各个系统。

use std::{
    ops::ControlFlow,
    sync::{
        Arc,
        mpsc::{Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use mc_bamboo_core::Vec3;
use mc_bamboo_data::packets::play::serverbound;
use mc_bamboo_protocol::{
    packet::play::{ClientInformation, GameMode, PlayRequest},
    text::Component,
};
use tracing::warn;

use crate::{
    book, chat, chunk_broadcast,
    chunk_tracking::{self, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE},
    command, container, entity_sync, entity_tracking,
    global::{Global, RosterEntry},
    inbound::{Inbound, JoinRequest, PlayerId},
    interaction::{self, UseItemOn},
    inventory, lectern, light_updates, menu, movement,
    player::shows_hat,
    profiler::{Metric, Phase},
    region::{JoinSettings, Region},
    secure_chat::{self, ServiceProfileKeys},
    sign, weather,
};

const TICK: Duration = Duration::from_millis(50);
/// 落后超过这个时长就不追了，直接跳过去。短暂的卡顿靠连续跑 tick 追上。
const MAX_CATCH_UP: Duration = Duration::from_secs(2);

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
}

/// 所有游戏状态的唯一所有者。
pub struct World {
    pub(crate) global: Global,
    pub(crate) region: Region,
    inbox: Receiver<Inbound>,
    /// 只有连接层起的世界才用权威 OP 名单重算进服权限。
    /// 直接嵌 `World::new` 的测试/工具按 `JoinRequest` 传入的等级走。
    authoritative_admin: bool,
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
        Self::build(config, inbox, keys, None)
    }

    /// 连接层把启动时读到的名单和发布变更的通道交给世界线程。
    /// 名单变更作为 owned 消息发给连接层，世界线程不会等它。
    pub fn with_admin(
        config: WorldConfig,
        inbox: Receiver<Inbound>,
        keys: Arc<ServiceProfileKeys>,
        admin: crate::admin::AdminInit,
    ) -> Self {
        Self::build(config, inbox, keys, Some(admin))
    }

    fn build(
        config: WorldConfig,
        inbox: Receiver<Inbound>,
        keys: Arc<ServiceProfileKeys>,
        admin: Option<crate::admin::AdminInit>,
    ) -> Self {
        let view_distance = config
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
        let mut global = Global::default();
        let authoritative_admin = admin.is_some();
        if let Some(init) = admin {
            global.admin = init.snapshot.into_state();
            global.admin_messages = init.messages;
        }
        let mut world = Self {
            authoritative_admin,
            global,
            region: Region::with_keys(
                view_distance,
                config.max_players,
                config.online_mode,
                config.enforce_secure_profile,
                keys,
            ),
            inbox,
        };
        world.region.profiler.on_world_start();
        world
    }

    /// 阻塞运行，直到收到 [`Inbound::Shutdown`] 或者所有发送端都没了。
    pub fn run(mut self) {
        let mut schedule = Schedule::starting_at(Instant::now());
        loop {
            let expected = schedule.expected_start();
            let actual = Instant::now();
            let skipped = schedule.skipped_catch_up();
            self.region
                .profiler
                .begin_scheduled_tick(expected, actual, skipped);
            if self.tick().is_break() {
                break;
            }
            let wait = schedule.after_tick(Instant::now());
            if !wait.is_zero() {
                thread::sleep(wait);
            }
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
    fn record_tick_counts(&mut self) {
        let next_time = self.global.time().0 + 1;
        let due = self
            .region
            .block_ticks
            .pending()
            .filter(move |tick| tick.trigger_tick <= next_time)
            .count()
            + self
                .region
                .block_ticks
                .pending_fluids()
                .filter(move |tick| tick.trigger_tick <= next_time)
                .count();
        let profiler = &mut self.region.profiler;
        profiler.record(Metric::OnlinePlayers, self.region.players.len() as u64);
        profiler.record(Metric::Entities, self.region.entities.len() as u64);
        profiler.record(Metric::DueScheduledTicks, due as u64);
    }

    /// 拆成 `tick` 和这一段只是为了保证提前返回时也能收尾；顺序仍然是原来的顺序。
    fn tick_phases(&mut self) -> ControlFlow<()> {
        self.process_inbox()?;
        self.region.profiler.lap(Phase::Inbox);
        light_updates::run_light_updates(&mut self.region);
        self.region.profiler.lap(Phase::LightUpdates);

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
        interaction::run_entity_ticks(&mut self.region);
        self.region.profiler.lap(Phase::Entities);
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
        self.region.links.flush();
        self.region.profiler.lap(Phase::Flush);
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
        let refusal = self
            .authoritative_admin
            .then(|| {
                self.global.admin.login_refusal(
                    request.uuid,
                    &request.peer_ip,
                    self.region.now_millis(),
                )
            })
            .flatten();
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

        let game_mode = self.global.state().default_game_mode;
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
        self.region.add_player(
            request,
            JoinSettings {
                game_mode,
                difficulty_frames: &difficulty_frames,
                spawn: self.global.state().spawn,
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
            self.region.links.broadcast_except(id, &frames);
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

        self.region.start_loading_chunks(
            id,
            self.global.time(),
            self.global.clock(),
            self.global.state().spawn,
            self.global.state().weather,
        );
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
            self.region.links.broadcast(&frames);
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
        self.region.players.remove(&id);
        self.region.player_grid.remove(id);
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

    /// 给所有人发关闭的原因，然后结束世界线程。
    fn shut_down(&mut self) -> ControlFlow<()> {
        let reason = Component::translate(SERVER_SHUTDOWN, []);
        self.region.links.kick_all(&reason);
        ControlFlow::Break(())
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
}

impl Schedule {
    /// 第一 tick 在 `now` 立刻开始，第二刻不早于 `now + TICK`。
    fn starting_at(now: Instant) -> Self {
        Self {
            next_tick: now + TICK,
            expected_start: now,
            skipped_catch_up: false,
        }
    }

    /// 这一刻应该几点开始。
    fn expected_start(&self) -> Instant {
        self.expected_start
    }

    fn skipped_catch_up(&self) -> bool {
        self.skipped_catch_up
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
        self.expected_start = self.next_tick;
        self.next_tick += TICK;
        wait
    }
}

#[cfg(test)]
pub(crate) mod tests;
