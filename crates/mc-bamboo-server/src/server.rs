//! 监听端口、接受连接，并管理服务端的生命周期。

use std::{
    any::Any,
    collections::HashMap,
    future::Future,
    io,
    net::SocketAddr,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use mc_bamboo_auth::{ServerKeyPair, Timeouts, fetch_profile_keys, public_keys_url};
use mc_bamboo_data::{registries, tags};
use mc_bamboo_game::{Inbound, ServiceProfileKeys, World, WorldConfig};
use mc_bamboo_protocol::{
    GAME_VERSION, PROTOCOL_VERSION,
    frame::{FrameError, append_frame},
    packet::{
        configuration::{
            Brand, FinishConfiguration, RegistryData, SelectKnownPacks, UpdateEnabledFeatures,
            UpdateTags,
        },
        status::StatusResponse,
    },
};
use serde_json::json;
use tokio::{
    net::TcpListener,
    sync::{OnceCell, Semaphore, broadcast},
    time::timeout,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::{
    config::ServerConfig,
    connection,
    event::{PlayerInfo, ServerEvent},
    ops::{OPS_FILE, Ops},
    players::{AddPlayerError, OnlinePlayer},
};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const EVENT_BUFFER_LEN: usize = 256;
const SERVER_BRAND: &str = "Bamboo";
const WORLD_THREAD_NAME: &str = "bamboo-world";
/// 关闭时等世界线程结束的时限。它正常时一个 tick 就结束了，到时限说明卡死了，
/// 这时不能让 Ctrl+C 和桌面端的停止按钮跟着卡住，连接任务也还要给玩家发断开原因。
#[cfg(not(test))]
const WORLD_STOP_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const WORLD_STOP_TIMEOUT: Duration = Duration::from_millis(200);

/// 对所有客户端都一样的响应，启动时编码一次，之后直接发送。
pub(crate) struct PreEncodedResponses {
    /// 配置阶段开头的几个包。
    pub(crate) configuration_start: Vec<u8>,
    /// 注册表、标签，以及结束配置的通知。
    pub(crate) configuration_data: Vec<u8>,
}

impl PreEncodedResponses {
    fn new() -> Result<Self, FrameError> {
        let mut configuration_start = Vec::new();
        append_frame(&mut configuration_start, &Brand { name: SERVER_BRAND })?;
        append_frame(
            &mut configuration_start,
            &UpdateEnabledFeatures {
                features: registries::ENABLED_FEATURES,
            },
        )?;
        append_frame(
            &mut configuration_start,
            &SelectKnownPacks {
                packs: registries::KNOWN_PACKS,
            },
        )?;

        let mut configuration_data = Vec::new();
        for registry in registries::SYNCED_REGISTRIES {
            append_frame(&mut configuration_data, &RegistryData { registry })?;
        }
        append_frame(
            &mut configuration_data,
            &UpdateTags {
                groups: tags::TAG_GROUPS,
            },
        )?;
        append_frame(&mut configuration_data, &FinishConfiguration)?;

        Ok(Self {
            configuration_start,
            configuration_data,
        })
    }
}

/// 编码服务器列表的响应。只有在线人数变化时才需要重新编码。
fn encode_status(
    config: &ServerConfig,
    online: usize,
    enforces_secure_chat: bool,
) -> Result<Arc<[u8]>, FrameError> {
    let status_json = json!({
        "version": { "name": format!("Bamboo {GAME_VERSION}"), "protocol": PROTOCOL_VERSION },
        "players": { "max": config.max_players, "online": online },
        "description": { "text": config.motd },
        "enforcesSecureChat": enforces_secure_chat,
    })
    .to_string();

    let mut frame = Vec::new();
    append_frame(&mut frame, &StatusResponse { json: &status_json })?;
    Ok(frame.into())
}

/// 所有连接共享的状态。
pub(crate) struct Shared {
    pub(crate) config: ServerConfig,
    pub(crate) responses: PreEncodedResponses,
    /// 启动时从工作目录读到的 OP 表。之后不再读盘。
    ///
    /// The op list read from the working directory at startup. It is not read again.
    pub(crate) ops: Ops,
    /// 每次服务端启动时生成的会话 ID。
    pub(crate) session_id: Uuid,
    /// 这次启动的 RSA 密钥对，只有开了正版验证才有。官方不管开不开都生成，
    /// 但是关掉的时候它一次也用不上，生成 1024 位的钥匙又要花掉启动时间。
    pub(crate) keypair: Option<ServerKeyPair>,
    /// Mojang 用来签玩家公钥的钥匙。连接任务写，世界线程只读。
    ///
    /// The keys Mojang uses to sign player public keys. Connection tasks write them.
    /// The world thread only reads.
    pub(crate) profile_keys: Arc<ServiceProfileKeys>,
    /// 公钥只取一次。失败也记在这里，避免每个玩家都再问一遍。
    ///
    /// The profile keys are fetched once. A failure is recorded too, so later players do not ask again.
    keys_loaded: OnceCell<()>,
    /// 发给世界线程的消息。世界线程结束后发送会失败。
    pub(crate) inbox: Sender<Inbound>,
    status: RwLock<Arc<[u8]>>,
    players: Mutex<HashMap<Uuid, OnlinePlayer>>,
    events: broadcast::Sender<ServerEvent>,
}

impl Shared {
    /// 当前的服务器列表响应。
    pub(crate) fn status_frame(&self) -> Arc<[u8]> {
        let status = self.status.read().expect("状态缓存的锁不应该中毒");
        Arc::clone(&status)
    }

    /// 调用方要拿着玩家表的锁。否则两条连接同时进出时，先算出的人数可能最后才写进缓存，
    /// 服务器列表就会一直显示错的人数，直到下一次有人进出。
    fn refresh_status(&self, online: usize) {
        let enforced = enforces_secure_chat(&self.config, &self.profile_keys);
        match encode_status(&self.config, online, enforced) {
            Ok(frame) => *self.status.write().expect("状态缓存的锁不应该中毒") = frame,
            Err(error) => warn!(%error, "failed to refresh the server status"),
        }
    }

    pub(crate) fn add_player(
        &self,
        uuid: Uuid,
        player: &OnlinePlayer,
    ) -> Result<(), AddPlayerError> {
        let mut players = self.players.lock().expect("玩家表的锁不应该中毒");
        if let Some(existing) = players.get(&uuid) {
            return Err(AddPlayerError::Duplicate(existing.signals()));
        }
        if players.len() >= self.config.max_players as usize {
            return Err(AddPlayerError::Full);
        }
        players.insert(uuid, player.clone());
        self.refresh_status(players.len());
        Ok(())
    }

    pub(crate) fn remove_player(&self, uuid: Uuid) -> Option<OnlinePlayer> {
        let mut players = self.players.lock().expect("玩家表的锁不应该中毒");
        let removed = players.remove(&uuid);
        self.refresh_status(players.len());
        removed
    }

    pub(crate) fn publish(&self, event: ServerEvent) {
        // 没有订阅者时发送会失败，这是正常情况。
        let _ = self.events.send(event);
    }
}

/// 从外部观察运行中服务端的句柄，可以随意克隆。
#[derive(Clone)]
pub struct ServerHandle {
    shared: Arc<Shared>,
}

impl ServerHandle {
    /// 返回当前在线的玩家。
    pub fn online_players(&self) -> Vec<PlayerInfo> {
        let players = self.shared.players.lock().expect("玩家表的锁不应该中毒");
        players.values().map(|player| player.info.clone()).collect()
    }

    /// 订阅之后发生的服务端事件。
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.shared.events.subscribe()
    }
}

/// 绑定好端口、等待运行的服务端。
pub struct Server {
    listener: TcpListener,
    shared: Arc<Shared>,
    inbox: Receiver<Inbound>,
}

impl Server {
    /// 超出合法范围的配置项在这里修正，见 [`ServerConfig`] 各字段的说明。
    pub fn new(listener: TcpListener, config: ServerConfig) -> Result<Self, FrameError> {
        let config = config.normalized();
        let responses = PreEncodedResponses::new()?;
        let profile_keys = Arc::new(ServiceProfileKeys::empty());
        let status = encode_status(&config, 0, false)?;
        let (events, _) = broadcast::channel(EVENT_BUFFER_LEN);
        let (inbox_sender, inbox) = mpsc::channel();
        let keypair = config.online_mode.then(ServerKeyPair::generate);
        Ok(Self {
            listener,
            shared: Arc::new(Shared {
                config,
                responses,
                // 官方也是在世界线程启动之前读一次 `ops.json`。
                //
                // Official also reads `ops.json` once before the world thread starts.
                ops: Ops::load(Path::new(OPS_FILE)),
                session_id: Uuid::new_v4(),
                keypair,
                profile_keys,
                keys_loaded: OnceCell::new(),
                inbox: inbox_sender,
                status: RwLock::new(status),
                players: Mutex::new(HashMap::new()),
                events,
            }),
            inbox,
        })
    }

    /// 实际监听的地址。绑定 0 号端口时可以用它拿到系统分配的端口。
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn handle(&self) -> ServerHandle {
        ServerHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// 持续接受连接，直到 `shutdown` 完成，然后关闭所有连接再返回。
    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let config = world_config(&self.shared.config);
        let keys = Arc::clone(&self.shared.profile_keys);
        if self.shared.config.online_mode {
            let shared = Arc::clone(&self.shared);
            tokio::spawn(async move {
                ensure_profile_keys(&shared).await;
            });
        }
        self.run_with_world(
            move |inbox| World::with_service_keys(config, inbox, keys).run(),
            shutdown,
        )
        .await;
    }

    /// [`run`](Self::run) 的本体。测试用它换掉世界线程的内容。
    async fn run_with_world(
        self,
        world: impl FnOnce(Receiver<Inbound>) + Send + 'static,
        shutdown: impl Future<Output = ()>,
    ) {
        let Self {
            listener,
            shared,
            inbox,
        } = self;
        let slots = Arc::new(Semaphore::new(shared.config.max_pre_play_connections));
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let world_stopped = CancellationToken::new();
        tokio::pin!(shutdown);

        if let Err(error) = spawn_world(world, inbox, world_stopped.clone()) {
            error!(%error, "failed to start the world thread");
            world_stopped.cancel();
        }

        loop {
            let (stream, peer) = tokio::select! {
                () = &mut shutdown => break,
                () = world_stopped.cancelled() => break,
                accepted = listener.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        // 通常是 socket 耗尽了，马上重试只会空转。
                        warn!(%error, "failed to accept a connection");
                        tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                        continue;
                    }
                },
            };

            let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                debug!(%peer, "too many connections, dropping this one");
                continue;
            };
            let shared = Arc::clone(&shared);
            let cancel = cancel.clone();
            tasks.spawn(async move {
                connection::handle(stream, peer, shared, slot, cancel).await;
            });
        }

        // 先放开端口，桌面端可以马上重新启动。
        drop(listener);
        // 世界线程已经结束时发送会失败，下面的等待也会马上结束。
        let _ = shared.inbox.send(Inbound::Shutdown);
        if timeout(WORLD_STOP_TIMEOUT, world_stopped.cancelled())
            .await
            .is_err()
        {
            error!("the world thread did not stop in time, leaving it behind");
        }

        // 世界线程已经把关闭的原因写进了每个连接的出站队列，这里只是让连接任务不再等客户端。
        // 它写不了的时候，连接任务发现取消之后自己补发。
        cancel.cancel();
        tasks.close();
        tasks.wait().await;
    }
}

fn enforces_secure_chat(config: &ServerConfig, keys: &ServiceProfileKeys) -> bool {
    config.enforce_secure_profile && config.online_mode && keys.can_validate()
}

/// 正版验证打开时，在连接任务里把 Mojang 的公钥取回来。世界线程不碰网络。
///
/// When online mode is on, a connection task fetches Mojang's public keys.
/// The world thread never touches the network.
pub(crate) async fn ensure_profile_keys(shared: &Arc<Shared>) {
    shared
        .keys_loaded
        .get_or_init(|| load_profile_keys(Arc::clone(shared)))
        .await;
}

async fn load_profile_keys(shared: Arc<Shared>) {
    if !shared.config.online_mode {
        return;
    }
    let url = public_keys_url(&shared.config.session_server_url);
    let timeouts = Timeouts {
        connect: shared.config.auth_connect_timeout,
        read: shared.config.auth_read_timeout,
    };
    match fetch_profile_keys(&url, timeouts).await {
        Ok(keys) => {
            if !shared.profile_keys.replace(&keys) {
                warn!(
                    "discarding Mojang profile keys because one of them is not an RSA public key"
                );
            }
            let online = shared.players.lock().expect("玩家表的锁不应该中毒").len();
            shared.refresh_status(online);
        }
        Err(error) => {
            warn!(%error, "could not fetch Mojang profile keys; secure chat stays off");
        }
    }
}

fn world_config(config: &ServerConfig) -> WorldConfig {
    WorldConfig {
        view_distance: i32::from(config.view_distance),
        max_players: i32::try_from(config.max_players).unwrap_or(i32::MAX),
        online_mode: config.online_mode,
        enforce_secure_profile: config.enforce_secure_profile,
    }
}

/// 在专门的线程上运行世界。线程因为任何原因结束时 `stopped` 都会被取消。
///
/// 世界线程 panic 之后状态可能已经不一致，所以不重启，走关闭流程。
fn spawn_world(
    world: impl FnOnce(Receiver<Inbound>) + Send + 'static,
    inbox: Receiver<Inbound>,
    stopped: CancellationToken,
) -> io::Result<()> {
    thread::Builder::new()
        .name(WORLD_THREAD_NAME.to_owned())
        .spawn(move || {
            let _stopped = stopped.drop_guard();
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| world(inbox))) {
                error!(
                    reason = panic_message(panic.as_ref()),
                    "the world thread panicked, shutting down"
                );
            }
        })?;
    Ok(())
}

fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

#[cfg(test)]
mod tests {
    use std::{future::pending, sync::Barrier, time::Duration};

    use mc_bamboo_auth::Profile;
    use mc_bamboo_data::packets::{configuration, login, play};
    use mc_bamboo_protocol::{
        PROTOCOL_VERSION,
        codec::{Reader, write_string, write_uuid, write_varint},
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{ClientboundPacket, play::Disconnect},
        text::Component,
    };
    use serde_json::Value;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        sync::oneshot,
    };

    use super::*;
    use crate::{connection::MAX_QUEUED_BYTES, players::PlayerSession};

    const DEADLINE: Duration = Duration::from_secs(5);

    /// 这里的连接只检查进出游戏之外的部分，所以关掉压缩；
    /// 压缩由 `tests/` 里的客户端覆盖。
    async fn test_server() -> (Server, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = ServerConfig {
            compression_threshold: -1,
            ..ServerConfig::default()
        };
        let server = Server::new(listener, config).unwrap();
        (server, address)
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, payload.len() as i32);
        out.extend_from_slice(payload);
        out
    }

    /// 读到指定 ID 的帧，返回 ID 之后的内容。
    async fn read_until(stream: &mut TcpStream, buffer: &mut Vec<u8>, id: i32) -> Vec<u8> {
        loop {
            while let Some(bounds) = find_frame(buffer, MAX_FRAME_LEN).unwrap() {
                let end = bounds.header_len + bounds.payload_len;
                let payload = buffer[bounds.header_len..end].to_vec();
                buffer.drain(..end);
                let mut reader = Reader::new(&payload);
                if reader.varint().unwrap() == id {
                    return reader.rest().to_vec();
                }
            }
            let read = stream.read_buf(buffer).await.unwrap();
            assert_ne!(read, 0, "server closed the connection before the frame");
        }
    }

    /// 走完登录和配置，这时服务端会发 `Join` 给世界线程。
    async fn join(stream: &mut TcpStream, buffer: &mut Vec<u8>) {
        let mut handshake = vec![0x00];
        write_varint(&mut handshake, PROTOCOL_VERSION);
        write_string(&mut handshake, "localhost");
        handshake.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut handshake, 2);
        stream.write_all(&frame(&handshake)).await.unwrap();

        let mut hello = vec![login::serverbound::HELLO as u8];
        write_string(&mut hello, "Kitten");
        write_uuid(&mut hello, Uuid::nil());
        stream.write_all(&frame(&hello)).await.unwrap();

        read_until(stream, buffer, login::clientbound::LOGIN_FINISHED).await;
        let acknowledged = [login::serverbound::LOGIN_ACKNOWLEDGED as u8];
        stream.write_all(&frame(&acknowledged)).await.unwrap();

        let known_packs = configuration::clientbound::SELECT_KNOWN_PACKS;
        read_until(stream, buffer, known_packs).await;
        let mut packs = vec![configuration::serverbound::SELECT_KNOWN_PACKS as u8];
        write_varint(&mut packs, 1);
        write_string(&mut packs, "minecraft");
        write_string(&mut packs, "core");
        write_string(&mut packs, GAME_VERSION);
        stream.write_all(&frame(&packs)).await.unwrap();

        let finish = configuration::clientbound::FINISH_CONFIGURATION;
        read_until(stream, buffer, finish).await;
        let finished = [configuration::serverbound::FINISH_CONFIGURATION as u8];
        stream.write_all(&frame(&finished)).await.unwrap();
    }

    #[tokio::test]
    async fn stopped_token_fires_after_a_panic_and_after_a_normal_end() {
        let (_sender, inbox) = mpsc::channel();
        let stopped = CancellationToken::new();
        spawn_world(
            |_inbox| panic!("世界线程的测试 panic"),
            inbox,
            stopped.clone(),
        )
        .unwrap();
        timeout(DEADLINE, stopped.cancelled())
            .await
            .expect("线程 panic 后要通知服务端");

        let (_sender, inbox) = mpsc::channel();
        let stopped = CancellationToken::new();
        spawn_world(|_inbox| {}, inbox, stopped.clone()).unwrap();
        timeout(DEADLINE, stopped.cancelled())
            .await
            .expect("线程结束后要通知服务端");
    }

    #[test]
    fn panic_message_reads_both_payload_types() {
        let text: Box<dyn Any + Send> = Box::new("boom");
        assert_eq!(panic_message(text.as_ref()), "boom");

        let formatted: Box<dyn Any + Send> = Box::new(format!("boom {}", 1));
        assert_eq!(panic_message(formatted.as_ref()), "boom 1");

        let other: Box<dyn Any + Send> = Box::new(1);
        assert_eq!(panic_message(other.as_ref()), "unknown panic");
    }

    #[tokio::test]
    async fn accept_loop_stops_when_the_world_thread_ends() {
        let (server, address) = test_server().await;

        timeout(
            DEADLINE,
            server.run_with_world(|_inbox| panic!("世界线程的测试 panic"), pending()),
        )
        .await
        .expect("世界线程结束后服务端要停下来");

        assert!(TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn players_are_told_why_when_the_world_thread_dies() {
        let (server, address) = test_server().await;
        let world = |inbox: Receiver<Inbound>| {
            for message in inbox {
                if matches!(message, Inbound::Join(_)) {
                    panic!("世界线程的测试 panic");
                }
            }
        };
        let running = tokio::spawn(server.run_with_world(world, pending()));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        timeout(DEADLINE, async {
            join(&mut stream, &mut buffer).await;
            let body = read_until(&mut stream, &mut buffer, play::clientbound::DISCONNECT).await;
            let reason = Component::translate("multiplayer.disconnect.server_shutdown", []);
            let mut expected = Vec::new();
            Disconnect { reason: &reason }.write_body(&mut expected);
            assert_eq!(body, expected);
        })
        .await
        .expect("连接任务要补发断开原因");

        drop(stream);
        timeout(DEADLINE, running)
            .await
            .expect("世界线程结束后服务端要停下来")
            .unwrap();
    }

    /// 世界线程卡住了：不处理任何消息，直到 `release` 被丢掉。
    fn stalled_world() -> (
        mpsc::Sender<()>,
        impl FnOnce(Receiver<Inbound>) + Send + 'static,
    ) {
        let (release, released) = mpsc::channel::<()>();
        let world = move |inbox: Receiver<Inbound>| {
            let _ = released.recv();
            drop(inbox);
        };
        (release, world)
    }

    #[tokio::test]
    async fn requests_over_the_quota_disconnect_the_player() {
        let (server, address) = test_server().await;
        let (release, world) = stalled_world();
        let running = tokio::spawn(server.run_with_world(world, pending()));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        join(&mut stream, &mut buffer).await;

        // 世界线程不处理消息，所以配额只出不进，第 4097 条超过上限。
        let status_only = frame(&[play::serverbound::MOVE_PLAYER_STATUS_ONLY as u8, 0x01]);
        stream.write_all(&status_only.repeat(4097)).await.unwrap();

        let body = timeout(
            DEADLINE,
            read_until(&mut stream, &mut buffer, play::clientbound::DISCONNECT),
        )
        .await
        .expect("超过配额的玩家要被断开");
        let reason = Component::translate("disconnect.exceeded_packet_rate", []);
        let mut expected = Vec::new();
        Disconnect { reason: &reason }.write_body(&mut expected);
        assert_eq!(body, expected);

        drop(release);
        drop(stream);
        timeout(DEADLINE, running).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn quota_does_not_count_packets_the_world_never_sees() {
        let (server, address) = test_server().await;
        let (release, world) = stalled_world();
        let running = tokio::spawn(server.run_with_world(world, pending()));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        join(&mut stream, &mut buffer).await;

        let tick_end = frame(&[play::serverbound::CLIENT_TICK_END as u8]);
        stream.write_all(&tick_end.repeat(5000)).await.unwrap();

        // 连接任务还在正常工作：不进配额的包不影响 ping 的回应。
        let mut ping = vec![play::serverbound::PING_REQUEST as u8];
        ping.extend_from_slice(&7i64.to_be_bytes());
        stream.write_all(&frame(&ping)).await.unwrap();
        let body = timeout(
            DEADLINE,
            read_until(&mut stream, &mut buffer, play::clientbound::PONG_RESPONSE),
        )
        .await
        .expect("连接任务自己回 ping，不用等世界线程");
        assert_eq!(body, 7i64.to_be_bytes());

        drop(release);
        drop(stream);
        timeout(DEADLINE, running).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn keep_alive_does_not_depend_on_the_world_thread() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = ServerConfig {
            keep_alive_interval: Duration::from_millis(50),
            compression_threshold: -1,
            ..ServerConfig::default()
        };
        let server = Server::new(listener, config).unwrap();
        let (release, world) = stalled_world();
        let running = tokio::spawn(server.run_with_world(world, pending()));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        join(&mut stream, &mut buffer).await;

        for _ in 0..3 {
            let id = timeout(
                DEADLINE,
                read_until(&mut stream, &mut buffer, play::clientbound::KEEP_ALIVE),
            )
            .await
            .expect("世界线程卡住时心跳照常");
            let mut answer = vec![play::serverbound::KEEP_ALIVE as u8];
            answer.extend_from_slice(&id);
            stream.write_all(&frame(&answer)).await.unwrap();
        }

        drop(release);
        drop(stream);
        timeout(DEADLINE, running).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn outbound_overflow_aborts_the_connection_without_a_farewell() {
        let (server, address) = test_server().await;
        let world = |inbox: Receiver<Inbound>| {
            let mut sinks = Vec::new();
            for message in inbox {
                match message {
                    Inbound::Join(mut request) => {
                        assert!(!request.sink.send(vec![0; MAX_QUEUED_BYTES + 1]));
                        sinks.push(request.sink);
                    }
                    Inbound::Shutdown => return,
                    _ => {}
                }
            }
        };
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let running = tokio::spawn(server.run_with_world(world, async {
            let _ = stopped.await;
        }));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        join(&mut stream, &mut buffer).await;

        // 连接马上结束，队列里排着的东西都不写，也没有断开包。
        let mut rest = Vec::new();
        timeout(DEADLINE, stream.read_to_end(&mut rest))
            .await
            .expect("积压超过上限的连接要立刻中止")
            .ok();
        assert!(rest.is_empty());

        drop(stop);
        timeout(DEADLINE, running).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stopping_does_not_wait_forever_for_a_stuck_world_thread() {
        let (server, address) = test_server().await;
        let (release, world) = stalled_world();
        let (stop, stopped) = oneshot::channel::<()>();
        let running = tokio::spawn(server.run_with_world(world, async {
            let _ = stopped.await;
        }));

        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut buffer = Vec::new();
        join(&mut stream, &mut buffer).await;
        drop(stop);

        // 世界线程发不了断开包，这时要靠连接任务补发。
        let body = timeout(
            DEADLINE,
            read_until(&mut stream, &mut buffer, play::clientbound::DISCONNECT),
        )
        .await
        .expect("世界线程卡死时玩家也要知道服务端关了");
        let reason = Component::translate("multiplayer.disconnect.server_shutdown", []);
        let mut expected = Vec::new();
        Disconnect { reason: &reason }.write_body(&mut expected);
        assert_eq!(body, expected);

        drop(stream);
        timeout(DEADLINE, running)
            .await
            .expect("世界线程卡死时关闭流程也要结束")
            .unwrap();
        drop(release);
    }

    /// 服务器列表响应里的在线人数。
    fn online_in_status(shared: &Shared) -> i64 {
        let frame = shared.status_frame();
        let bounds = find_frame(&frame, MAX_FRAME_LEN).unwrap().unwrap();
        let mut reader = Reader::new(&frame[bounds.header_len..]);
        reader.varint().unwrap();
        let status: Value = serde_json::from_str(reader.string(32767).unwrap()).unwrap();
        status["players"]["online"].as_i64().unwrap()
    }

    /// 修好之前这里大约每 20 轮就有一轮留下错的人数，所以 5000 轮足够抓到它。
    #[tokio::test]
    async fn status_is_not_left_stale_when_players_leave_together() {
        const ROUNDS: usize = 5000;
        let (server, _) = test_server().await;
        let barrier = Arc::new(Barrier::new(2));

        let players = ["Alice", "Bob"].map(|name| {
            let shared = Arc::clone(&server.shared);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap();
                let uuid = Uuid::new_v4();
                let mut stale_rounds = 0;
                for _ in 0..ROUNDS {
                    let session = runtime
                        .block_on(PlayerSession::join(
                            &shared,
                            Profile {
                                uuid,
                                name: name.to_owned(),
                                properties: Vec::new(),
                            },
                        ))
                        .unwrap();
                    barrier.wait();
                    drop(session);
                    barrier.wait();
                    stale_rounds += usize::from(online_in_status(&shared) != 0);
                    barrier.wait();
                }
                stale_rounds
            })
        });

        for player in players {
            assert_eq!(player.join().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn config_is_normalized_before_anyone_uses_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = ServerConfig {
            view_distance: 0,
            max_players: u32::MAX,
            ..ServerConfig::default()
        };

        let server = Server::new(listener, config).unwrap();

        let world = world_config(&server.shared.config);
        assert_eq!(world.view_distance, 2);
        assert_eq!(world.max_players, i32::MAX);
    }
}
