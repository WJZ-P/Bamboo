//! 监听端口、接受连接，并管理服务端的生命周期。

use std::{
    collections::HashMap,
    future::Future,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use mc_bamboo_data::{registries, tags};
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
use mc_bamboo_world::flat;
use serde_json::json;
use tokio::{
    net::TcpListener,
    sync::{Semaphore, broadcast},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    chunk_encoding::encode_chunk,
    config::ServerConfig,
    connection,
    event::{PlayerInfo, ServerEvent},
    players::JoinRefusal,
};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const EVENT_BUFFER_LEN: usize = 256;
const SERVER_BRAND: &str = "Bamboo";

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
fn encode_status(config: &ServerConfig, online: usize) -> Result<Arc<[u8]>, FrameError> {
    let status_json = json!({
        "version": { "name": format!("Bamboo {GAME_VERSION}"), "protocol": PROTOCOL_VERSION },
        "players": { "max": config.max_players, "online": online },
        "description": { "text": config.motd },
        "enforcesSecureChat": false,
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
    /// 每次服务端启动时生成的会话 ID。
    pub(crate) session_id: Uuid,
    /// 超平坦区块内容都相同，编码一次，所有区块、所有玩家共用。
    pub(crate) flat_chunk_body: Vec<u8>,
    status: RwLock<Arc<[u8]>>,
    players: Mutex<HashMap<Uuid, PlayerInfo>>,
    events: broadcast::Sender<ServerEvent>,
}

impl Shared {
    /// 当前的服务器列表响应。
    pub(crate) fn status_frame(&self) -> Arc<[u8]> {
        let status = self.status.read().expect("状态缓存的锁不应该中毒");
        Arc::clone(&status)
    }

    fn refresh_status(&self, online: usize) {
        match encode_status(&self.config, online) {
            Ok(frame) => *self.status.write().expect("状态缓存的锁不应该中毒") = frame,
            Err(error) => warn!(%error, "failed to refresh the server status"),
        }
    }

    pub(crate) fn add_player(&self, uuid: Uuid, info: PlayerInfo) -> Result<(), JoinRefusal> {
        let online = {
            let mut players = self.players.lock().expect("玩家表的锁不应该中毒");
            if players.contains_key(&uuid) {
                return Err(JoinRefusal::AlreadyOnline);
            }
            if players.len() >= self.config.max_players as usize {
                return Err(JoinRefusal::ServerFull);
            }
            players.insert(uuid, info);
            players.len()
        };
        self.refresh_status(online);
        Ok(())
    }

    pub(crate) fn remove_player(&self, uuid: Uuid) -> Option<PlayerInfo> {
        let (removed, online) = {
            let mut players = self.players.lock().expect("玩家表的锁不应该中毒");
            (players.remove(&uuid), players.len())
        };
        self.refresh_status(online);
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
        players.values().cloned().collect()
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
}

impl Server {
    pub fn new(listener: TcpListener, config: ServerConfig) -> Result<Self, FrameError> {
        let responses = PreEncodedResponses::new()?;
        let status = encode_status(&config, 0)?;
        let (events, _) = broadcast::channel(EVENT_BUFFER_LEN);
        Ok(Self {
            listener,
            shared: Arc::new(Shared {
                config,
                responses,
                session_id: Uuid::new_v4(),
                flat_chunk_body: encode_chunk(&flat::generate_chunk()),
                status: RwLock::new(status),
                players: Mutex::new(HashMap::new()),
                events,
            }),
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
        let slots = Arc::new(Semaphore::new(self.shared.config.max_pre_play_connections));
        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        tokio::pin!(shutdown);

        loop {
            let (stream, peer) = tokio::select! {
                () = &mut shutdown => break,
                accepted = self.listener.accept() => match accepted {
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
            let shared = Arc::clone(&self.shared);
            let cancel = cancel.clone();
            tasks.spawn(async move {
                connection::handle(stream, peer, shared, slot, cancel).await;
            });
        }

        // 先放开端口，桌面端可以马上重新启动。
        drop(self.listener);
        cancel.cancel();
        tasks.close();
        tasks.wait().await;
    }
}
