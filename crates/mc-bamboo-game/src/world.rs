//! 世界线程的主循环：20 TPS，每 tick 先处理队列里的消息，再按固定顺序推进各个系统。

use std::{
    ops::ControlFlow,
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{
    packet::play::{ClientInformation, GameMode, PlayRequest},
    text::Component,
};
use tracing::warn;

use crate::{
    book, chat, chunk_broadcast,
    chunk_tracking::{self, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE},
    container, entity_sync,
    global::{Global, RosterEntry},
    inbound::{Inbound, JoinRequest, PlayerId},
    interaction::{self, UseItemOn},
    inventory, light_updates, menu, movement,
    player::shows_hat,
    region::Region,
    sign,
};

const TICK: Duration = Duration::from_millis(50);
/// 落后超过这个时长就不追了，直接跳过去。短暂的卡顿靠连续跑 tick 追上。
const MAX_CATCH_UP: Duration = Duration::from_secs(2);

const GAME_MODE: GameMode = GameMode::Creative;
const SERVER_SHUTDOWN: &str = "multiplayer.disconnect.server_shutdown";

/// 世界的启动参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldConfig {
    /// 服务端视距，合法范围是 2 到 32。
    pub view_distance: i32,
    /// 写进 `login` 包的玩家上限。满员由连接层判断，世界线程不看它。
    pub max_players: i32,
    /// 服务端有没有开正版验证。只写进 `login` 包，验证本身在连接层做。
    pub online_mode: bool,
}

/// 所有游戏状态的唯一所有者。
pub struct World {
    global: Global,
    region: Region,
    inbox: Receiver<Inbound>,
}

impl World {
    /// 创建一个空世界。`config` 的视距超出合法范围时取最接近的值。
    pub fn new(config: WorldConfig, inbox: Receiver<Inbound>) -> Self {
        let view_distance = config
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
        Self {
            global: Global::default(),
            region: Region::new(view_distance, config.max_players, config.online_mode),
            inbox,
        }
    }

    /// 阻塞运行，直到收到 [`Inbound::Shutdown`] 或者所有发送端都没了。
    pub fn run(mut self) {
        let mut schedule = Schedule::starting_at(Instant::now());
        while self.tick().is_continue() {
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
        self.process_inbox()?;
        light_updates::run_light_updates(&mut self.region);

        self.global.advance_time();
        self.region.advance_tick();
        interaction::run_block_ticks(&mut self.region, self.global.time());
        // 官方 `ServerLevel.tick` 里随机刻在 `chunkSource.tick`，方块实体的 tick 在它之后。
        interaction::run_random_ticks(&mut self.region);
        // 玩家自己每 tick 的挖掘进度。官方在实体 tick 里跑 `ServerPlayerGameMode.tick`，
        // 这里跟着玩家 tick 放在随机刻之后。
        interaction::tick_mining(&mut self.region);
        // 方块实体的 tick：告示牌在这里清掉走远的编辑者。
        sign::tick_editors(&mut self.region);
        // 容器开合人数的重查和菜单同步。
        container::tick(&mut self.region);
        menu::tick(&mut self.region);
        chat::tick_spam_throttlers(&mut self.region);

        entity_sync::update_derived_state(&mut self.region);
        chunk_tracking::update_views(&mut self.region);
        chunk_broadcast::broadcast_changes(&mut self.region);
        entity_sync::sync_entities(&mut self.region);
        entity_sync::send_equipment_changes(&mut self.region);
        interaction::send_block_changed_acks(&mut self.region);
        if let Some(frames) = self.global.tick_latency_broadcast() {
            self.region.links.broadcast(&frames);
        }
        chunk_tracking::send_chunks(&mut self.region);
        self.region.links.flush();
        ControlFlow::Continue(())
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
        let entry = RosterEntry {
            uuid: request.uuid,
            name: request.name.clone(),
            // 皮肤只有名单要用，直接搬过来。
            properties: std::mem::take(&mut request.properties),
            game_mode: GAME_MODE,
            latency: 0,
            show_hat: shows_hat(&request.client),
        };

        self.region.add_player(request, GAME_MODE);

        if let Some(frames) = self.global.tab_list() {
            self.region.links.send_frames(id, &frames);
        }
        let frames = self.global.add_player(id, entry);
        self.region.links.broadcast(&frames);

        self.region.send_own_entity_data(id);

        if let Some(frames) = self.global.joined_message(id) {
            self.region.links.broadcast_except(id, &frames);
        }

        self.region.start_loading_chunks(id, self.global.time());
    }

    /// 处理离开，顺序见 `coverage.md` 4.2。可以重复调用：世界里没有这名玩家时直接忽略，
    /// 连接任务还没发 `Join` 就结束时走的就是这条路。
    fn on_leave(&mut self, id: PlayerId) {
        if !self.region.players.contains_key(&id) {
            return;
        }
        // 连接已经结束了，后面的包都不用写给它。
        self.region.links.remove(id);

        if let Some(frames) = self.global.left_message(id) {
            self.region.links.broadcast(&frames);
        }
        entity_sync::on_leave(&mut self.region, id);
        // 玩家断开时官方不回 `container_close`，光标上的东西直接丢弃。
        menu::on_leave(&mut self.region, id);
        self.region.players.remove(&id);
        if let Some(frames) = self.global.remove_player(id) {
            self.region.links.broadcast(&frames);
        }
    }

    /// 把一条请求交给负责它的模块。被踢的玩家要等 `Leave` 才离开世界，这段时间它的请求都忽略。
    fn on_request(&mut self, player: PlayerId, request: PlayRequest) {
        if !self.region.links.is_open(player) {
            return;
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
            } => movement::on_move_player(region, player, position, rotation, on_ground),
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick,
            } => chunk_tracking::on_chunk_batch_received(region, player, desired_chunks_per_tick),
            PlayRequest::ClientInformation(information) => {
                self.on_client_information(player, information);
            }
            PlayRequest::Chat { message } => {
                if let Some(frames) = chat::on_chat(region, player, message) {
                    region.links.broadcast(&frames);
                    chat::detect_chat_rate_spam(region, player);
                }
            }
            PlayRequest::ChatCommand { command } => {
                chat::on_chat_command(region, player, command);
            }
            PlayRequest::ClientCommand { action } => {
                chat::on_client_command(region, player, action);
            }
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
            PlayRequest::PickItemFromBlock { pos, .. } => {
                inventory::on_pick_item_from_block(region, player, pos);
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
            PlayRequest::ContainerClose { .. } => menu::on_close_request(region, player),
            PlayRequest::SignUpdate { pos, lines, slot } => {
                sign::on_sign_update(region, player, pos, lines, slot);
            }
            PlayRequest::EditBook { slot, pages, title } => {
                book::on_edit_book(region, player, slot, pages, title);
            }
            // 这三种连接任务就地处理了，不会发到这里。
            PlayRequest::KeepAlive(_) | PlayRequest::PingRequest(_) | PlayRequest::Other(_) => {}
        }
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
struct Schedule {
    next_tick: Instant,
}

impl Schedule {
    /// 第一 tick 在 `now` 开始。
    fn starting_at(now: Instant) -> Self {
        Self {
            next_tick: now + TICK,
        }
    }

    /// 一 tick 结束时调用，返回到下一 tick 开始还要等多久。已经落后时返回 0，连续跑 tick 追上。
    fn after_tick(&mut self, now: Instant) -> Duration {
        let behind = now.saturating_duration_since(self.next_tick);
        if behind > MAX_CATCH_UP {
            warn!(
                "Can't keep up! Running {}ms behind, skipping {} ticks",
                behind.as_millis(),
                behind.as_millis() / TICK.as_millis()
            );
            self.next_tick = now;
        }
        let wait = self.next_tick.saturating_duration_since(now);
        self.next_tick += TICK;
        wait
    }
}

#[cfg(test)]
pub(crate) mod tests;
