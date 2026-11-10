//! 一名假玩家：握手 → 登录 → 配置 → Play，进服之后按 20 Hz 发移动包。
//!
//! 每名假玩家只有一条 tokio 任务，读、写、定时都在这一条任务里用 `select!` 串起来；
//! 200 名假玩家也只有一个 tokio 运行时，不会一名玩家一个线程。

use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use mc_bamboo_data::packets::configuration;
use mc_bamboo_protocol::{
    codec::{Reader, write_varint},
    compression,
    frame::{MAX_FRAME_LEN, find_frame},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    motion::{Layout, Motion, Pattern},
    stats::{Second, SharedStats},
    wire::{self, ABILITY_FLYING, CLIENT_TICKS_PER_SECOND, SPAWN, ServerPacket, State},
};

/// 进服和读包的时限。200 名玩家同时连上时服务端可能忙，留足余量。
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
/// 移动包的周期，20 Hz。
const CLIENT_TICK: Duration = Duration::from_millis(1000 / CLIENT_TICKS_PER_SECOND as u64);
/// 一秒的统计桶。
const SECOND: Duration = Duration::from_secs(1);

/// 一次假玩家运行需要的参数。
#[derive(Debug, Clone)]
pub struct BotConfig {
    pub addr: String,
    pub index: usize,
    pub bots: usize,
    pub pattern: Pattern,
    pub layout: Layout,
    pub speed: f64,
    pub start: Instant,
    pub stop: Instant,
    pub stats: SharedStats,
    pub name_prefix: Arc<str>,
}

/// 一名假玩家的结果。`Joined` 表示它走到过 Play 状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 跑满了时长，正常结束。
    Completed,
    /// 没走到 Play 就结束了。
    JoinFailed(String),
    /// 进服之后中途掉线。
    Dropped(String),
}

/// 一名假玩家跑完，返回它的结果。
pub async fn run(config: BotConfig) -> Outcome {
    run_inner(&config).await.unwrap_or_else(|outcome| outcome)
}

async fn run_inner(config: &BotConfig) -> Result<Outcome, Outcome> {
    let BotConfig {
        addr,
        index,
        bots,
        pattern,
        layout,
        speed,
        start,
        stop: _,
        stats,
        name_prefix,
    } = config;
    let name = format!("{name_prefix}{index:04}");
    let stream = tokio::time::timeout(JOIN_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| Outcome::JoinFailed("connect timed out".to_owned()))?
        .map_err(|error| Outcome::JoinFailed(format!("connect: {error}")))?;
    stream
        .set_nodelay(true)
        .map_err(|error| Outcome::JoinFailed(format!("set_nodelay: {error}")))?;
    let mut connection = Connection::new(stream);
    let joined = tokio::time::timeout(JOIN_TIMEOUT, connection.join(&name))
        .await
        .map_err(|_| Outcome::JoinFailed("login timed out".to_owned()))??;
    stats.connected(start.elapsed(), joined);

    let mut motion = Motion::new(*pattern, *layout, *index, *bots, *speed);
    play(config, &mut connection, &mut motion)
        .await
        .map(|()| Outcome::Completed)
        .map_err(Outcome::Dropped)
}

/// 一条连接的读写状态和压缩阈值。
struct Connection {
    stream: TcpStream,
    read_buffer: Vec<u8>,
    compression: Option<i32>,
    state: State,
    login_done: bool,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buffer: Vec::with_capacity(64 * 1024),
            compression: None,
            state: State::Login,
            login_done: false,
        }
    }

    /// 走完 Login 和 Configuration，返回连接用了多久。
    async fn join(&mut self, name: &str) -> Result<Duration, Outcome> {
        let began = Instant::now();
        // 握手和 hello 都在压缩启用之前，直接写未压缩帧。
        self.write(&wire::handshake_frame())
            .await
            .map_err(Outcome::JoinFailed)?;
        self.write(&wire::hello_frame(name))
            .await
            .map_err(Outcome::JoinFailed)?;

        loop {
            let payload = self.read_packet().await.map_err(Outcome::JoinFailed)?;
            match wire::classify(self.state, &payload) {
                ServerPacket::LoginCompression(threshold) => {
                    self.compression = Some(threshold);
                    // 压缩包本身是未压缩的，之后两个方向都要带压缩层。
                }
                ServerPacket::LoginFinished => {
                    self.login_done = true;
                    self.state = State::Configuration;
                    self.write(&wire::login_acknowledged_frame())
                        .await
                        .map_err(Outcome::JoinFailed)?;
                    self.write(&wire::client_information_frame())
                        .await
                        .map_err(Outcome::JoinFailed)?;
                }
                ServerPacket::LoginDisconnect(_) => {
                    return Err(Outcome::JoinFailed(format!(
                        "login refused: {}",
                        wire::disconnect_key(State::Login, &payload)
                            .unwrap_or_else(|| "unknown reason".to_owned())
                    )));
                }
                ServerPacket::SelectKnownPacks => {
                    self.write(&wire::select_known_packs_frame())
                        .await
                        .map_err(Outcome::JoinFailed)?;
                }
                ServerPacket::FinishConfiguration => {
                    self.write(&wire::finish_configuration_frame())
                        .await
                        .map_err(Outcome::JoinFailed)?;
                    self.state = State::Play;
                    return Ok(began.elapsed());
                }
                ServerPacket::ConfigurationDisconnect => {
                    return Err(Outcome::JoinFailed(format!(
                        "configuration refused: {}",
                        wire::disconnect_key(State::Configuration, &payload)
                            .unwrap_or_else(|| "unknown reason".to_owned())
                    )));
                }
                ServerPacket::ConfigurationKeepAlive(id) => {
                    let mut body = Vec::new();
                    write_varint(&mut body, configuration::serverbound::KEEP_ALIVE);
                    body.extend_from_slice(&id.to_be_bytes());
                    self.write(&wire::frame(&body))
                        .await
                        .map_err(Outcome::JoinFailed)?;
                }
                _ => {}
            }
        }
    }

    /// 读一个包。压缩包在客户端这边直接消化，返回的总是解压后的负载。
    async fn read_packet(&mut self) -> Result<Vec<u8>, String> {
        loop {
            let bounds = find_frame(&self.read_buffer, MAX_FRAME_LEN)
                .map_err(|error| format!("invalid frame: {error}"))?;
            let Some(bounds) = bounds else {
                let before = self.read_buffer.len();
                let read = self
                    .stream
                    .read_buf(&mut self.read_buffer)
                    .await
                    .map_err(|error| format!("read: {error}"))?;
                if read == 0 {
                    let reason = if self.read_buffer.is_empty() {
                        "server closed the connection".to_owned()
                    } else {
                        "truncated frame".to_owned()
                    };
                    return Err(reason);
                }
                self.on_bytes_read(before + read);
                continue;
            };
            let end = bounds.header_len + bounds.payload_len;
            let mut payload = self.read_buffer[bounds.header_len..end].to_vec();
            self.read_buffer.drain(..end);

            if self.compression.is_none()
                && !self.login_done
                && wire::packet_id(&payload)
                    == Some(mc_bamboo_data::packets::login::clientbound::LOGIN_COMPRESSION)
            {
                self.compression = wire::login_compression_threshold(&payload);
                continue;
            }
            if let Some(threshold) = self.compression {
                payload = compression::decode(&payload, threshold)
                    .map_err(|error| format!("invalid compressed packet: {error}"))?;
            }
            return Ok(payload);
        }
    }

    /// 供 [`Play`] 统计每秒收到的字节；进服阶段不统计。
    fn on_bytes_read(&mut self, _len: usize) {}
}

/// Play 状态的主循环：读包、自动回应、按 20 Hz 移动、按秒汇总计数。
async fn play(
    config: &BotConfig,
    connection: &mut Connection,
    motion: &mut Motion,
) -> Result<(), String> {
    let BotConfig {
        pattern,
        stop,
        stats,
        ..
    } = config;
    let mut turn = 0_u64;
    let mut second = Second::default();
    let mut second_index = 0_u64;
    // 客户端主动发的 ping 用负数 id，不会和服务端心跳的 id 撞上。
    let mut ping_id = -1_i64;
    let mut pending: HashMap<i64, Instant> = HashMap::new();
    let mut flying = false;
    let mut last_position = SPAWN;
    let close_reason = "duration elapsed".to_owned();

    // 真实客户端画出世界之后才发 player_loaded；这里进服就发，服务端才会接受移动。
    connection.write(&wire::player_loaded_frame()).await?;
    // stand 也要每 tick 发位置包，服务端才持续看到在线活动。
    let mut second_ends = Instant::now() + SECOND;
    let mut tick_at = Instant::now() + CLIENT_TICK;

    loop {
        let now = Instant::now();
        let remaining = stop.saturating_duration_since(now);
        if remaining.is_zero() {
            break;
        }
        let next_tick = tick_at.saturating_duration_since(now).min(remaining);
        let next_second = second_ends.saturating_duration_since(now).min(remaining);
        let sleep = next_tick.min(next_second);
        tokio::select! {
            // 读 socket 的同时等下一个 20 Hz 时刻，不让定时器另起任务。
            readable = connection.stream.readable(), if sleep > Duration::ZERO => {
                let _ = readable;
                // 读满一帧或者读到一个完整包后退出这次 select 分支。
                drain_available(connection, &mut second, stats, &mut pending, motion, &mut last_position).await?;
            }
            _ = tokio::time::sleep(sleep) => {}
        }

        let now = Instant::now();
        if now >= second_ends {
            stats.second(second_index, second);
            second = Second::default();
            second_index += 1;
            second_ends += SECOND;
            // 每秒一次往返测量。服务端的 pong 在连接任务里就地回，不经过世界线程，
            // 所以量的是网络加连接层的往返，和心跳回应走同一条路径。
            let now = Instant::now();
            pending.insert(ping_id, now);
            let bytes = connection.write(&wire::ping_request_frame(ping_id)).await?;
            second.bytes_out += bytes;
            second.packets_out += 1;
            ping_id -= 1;
        }
        if now >= tick_at {
            tick_at += CLIENT_TICK;
            // 被服务端拉回之后从服务端给的位置继续走。
            let position = motion.step(CLIENT_TICK.as_secs_f64());
            let on_ground = !pattern.needs_flying();
            let batch = [
                wire::move_player_pos_frame(position, on_ground),
                wire::client_tick_end_frame(),
            ]
            .concat();
            let bytes = connection.write(&batch).await?;
            second.bytes_out += bytes;
            second.packets_out += 2;
            turn += 1;
            if !flying && pattern.needs_flying() && turn >= 2 {
                // 服务端按创造模式允许飞行；先上报正在飞，避免飞行踢。
                let bytes = connection
                    .write(&wire::player_abilities_frame(ABILITY_FLYING))
                    .await?;
                second.bytes_out += bytes;
                second.packets_out += 1;
                flying = true;
            }
            last_position = position;
        }
    }

    // 结束前把最后一秒的零头记进去。
    stats.second(second_index, second);
    let _ = last_position;
    let _ = close_reason;
    Ok(())
}

/// 把 socket 里已经能读到的字节都消化掉，回应需要回应的包。
///
/// 返回 `Err` 表示连接结束（正常关闭、服务端断开、协议错误）。
async fn drain_available(
    connection: &mut Connection,
    second: &mut Second,
    stats: &SharedStats,
    pending: &mut HashMap<i64, Instant>,
    motion: &mut Motion,
    last_position: &mut (f64, f64, f64),
) -> Result<(), String> {
    loop {
        // 只在缓冲区里已有完整帧时解析；没有就读一次 socket。
        let has_frame = matches!(
            find_frame(&connection.read_buffer, MAX_FRAME_LEN),
            Ok(Some(_))
        );
        if !has_frame {
            match connection.stream.try_read_buf(&mut connection.read_buffer) {
                Ok(read) if read > 0 => {
                    second.bytes_in += read as u64;
                    continue;
                }
                Ok(_) => return Err("server closed the connection".to_owned()),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(format!("read: {error}")),
            }
        }
        let payload = match connection.read_packet().await {
            Ok(payload) => payload,
            Err(reason) => return Err(reason),
        };
        second.packets_in += 1;
        // 一帧的长度前缀不进统计里：read_buf 记的是 socket 上真实收到的字节。
        handle(
            connection,
            second,
            stats,
            pending,
            motion,
            last_position,
            &payload,
        )
        .await?;
    }
}

/// 回应一个服务端发来的包，其余只计数。
async fn handle(
    connection: &mut Connection,
    second: &mut Second,
    stats: &SharedStats,
    pending: &mut HashMap<i64, Instant>,
    motion: &mut Motion,
    last_position: &mut (f64, f64, f64),
    payload: &[u8],
) -> Result<(), String> {
    match wire::classify(State::Play, payload) {
        ServerPacket::KeepAlive(id) => {
            // 用客户端发起的 ping 量往返：服务端会立刻回一个 pong。
            // 心跳本身也原样回，服务端才不会按超时断开。
            let now = Instant::now();
            pending.insert(id, now);
            let batch = [wire::keep_alive_frame(id), wire::ping_request_frame(id)].concat();
            let bytes = connection.write(&batch).await?;
            second.bytes_out += bytes;
            second.packets_out += 2;
        }
        ServerPacket::ChunkBatchFinished => {
            let bytes = connection
                .write(&wire::chunk_batch_received_frame())
                .await?;
            second.bytes_out += bytes;
            second.packets_out += 1;
        }
        ServerPacket::PlayerPosition(position) => {
            motion.snap_to((position.x, position.y, position.z));
            *last_position = motion.position();
            let bytes = connection
                .write(&wire::accept_teleportation_frame(
                    position.id,
                    (position.x, position.y, position.z),
                ))
                .await?;
            second.bytes_out += bytes;
            second.packets_out += 1;
        }
        ServerPacket::LevelChunkWithLight => second.chunks_in += 1,
        ServerPacket::Disconnect(_) => {
            return Err(format!(
                "kicked: {}",
                wire::disconnect_key(State::Play, payload)
                    .unwrap_or_else(|| "unknown reason".to_owned())
            ));
        }
        _ => {
            // pong_response 带客户端发起 ping 时的 id，用它量一次往返。
            let mut reader = Reader::new(payload);
            if reader.varint() == Ok(mc_bamboo_data::packets::play::clientbound::PONG_RESPONSE)
                && let Ok(time) = reader.i64()
                && let Some(sent) = pending.remove(&time)
            {
                stats.keep_alive_rtt(sent.elapsed().as_millis() as u64);
            }
        }
    }
    Ok(())
}

/// 写一批未压缩帧，返回写出去的字节数。
impl Connection {
    async fn write(&mut self, frames: &[u8]) -> Result<u64, String> {
        let encoded = wire::encode_outgoing(frames, self.compression)?;
        let len = encoded.len() as u64;
        self.stream
            .write_all(&encoded)
            .await
            .map_err(|error| format!("write: {error}"))?;
        Ok(len)
    }
}
