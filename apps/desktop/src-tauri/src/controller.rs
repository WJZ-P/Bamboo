//! 在桌面应用进程里启动和停止服务端，并把状态变化推送给界面。

use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_server::{
    GAME_VERSION, PROTOCOL_VERSION, PlayerInfo, Server, ServerEvent, ServerHandle,
};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter,
    async_runtime::{self, JoinHandle},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, broadcast::error::RecvError, oneshot},
};
use tracing::{info, warn};

use crate::settings::ServerSettings;

/// 服务端状态变化时发给界面的事件名。
const STATE_EVENT: &str = "bamboo://state";
/// 在线玩家变化时发给界面的事件名。
const PLAYERS_EVENT: &str = "bamboo://players";

struct RunningServer {
    address: SocketAddr,
    started_at_ms: u64,
    max_players: u32,
    handle: ServerHandle,
    stop: oneshot::Sender<()>,
    server_task: JoinHandle<()>,
    watcher_task: JoinHandle<()>,
}

/// 真正还在运行的服务端。世界线程崩溃时服务端会自己结束，这时还没有人调用 `stop`，
/// 那一项还留在 `running` 里，但已经不算运行了。
fn alive(running: &Option<RunningServer>) -> Option<&RunningServer> {
    running
        .as_ref()
        .filter(|server| !server.server_task.inner().is_finished())
}

/// 界面显示用的玩家信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    name: String,
    uuid: String,
}

impl From<PlayerInfo> for PlayerView {
    fn from(player: PlayerInfo) -> Self {
        Self {
            name: player.name,
            uuid: player.uuid,
        }
    }
}

/// 某一时刻的服务端状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSnapshot {
    running: bool,
    address: Option<String>,
    started_at_ms: Option<u64>,
    max_players: Option<u32>,
    players: Vec<PlayerView>,
    game_version: &'static str,
    protocol_version: i32,
}

/// 持有运行中的服务端。同一时间最多只有一个。
/// `force_handle` 在第一次优雅停止期间留着句柄，让第二次按停止能强制退出。
#[derive(Default)]
pub struct ServerController {
    running: Mutex<Option<RunningServer>>,
    force_handle: Mutex<Option<ServerHandle>>,
}

impl ServerController {
    pub async fn snapshot(&self) -> ServerSnapshot {
        snapshot_of(alive(&*self.running.lock().await))
    }

    pub async fn start(
        &self,
        app: &AppHandle,
        settings: &ServerSettings,
    ) -> Result<ServerSnapshot, String> {
        let mut running = self.running.lock().await;
        if alive(&running).is_some() {
            return Err("服务器已经在运行了".to_owned());
        }
        if let Some(ended) = running.take() {
            ended.watcher_task.abort();
        }
        // 界面也会校验，但设置文件是用户可以手改的，这里再拦一次。
        settings.validate()?;

        let target = format!("{}:{}", settings.host.trim(), settings.port);
        let listener = TcpListener::bind(&target)
            .await
            .map_err(|error| format!("无法监听 {target}：{error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("读取监听地址失败：{error}"))?;
        // 名单目录用 APP 自己的数据目录，不用前端传来的展示字符串。
        let directory = crate::settings::server_directory(app)?;
        let mut config = settings.to_server_config();
        config.list_directory = directory;
        let server =
            Server::new(listener, config).map_err(|error| format!("服务器配置无效：{error}"))?;
        let handle = server.handle();

        let (stop, stopped) = oneshot::channel::<()>();
        let exit_app = app.clone();
        let server_task = async_runtime::spawn(async move {
            server
                .run(async {
                    let _ = stopped.await;
                })
                .await;
            // 服务端自己结束时没有人调用 `stop`，界面只能从这里知道。
            emit_state(&exit_app, &snapshot_of(None));
        });
        let watcher_task = async_runtime::spawn(watch_events(app.clone(), handle.clone()));

        info!("Bamboo is listening on {address} for Minecraft {GAME_VERSION}");
        *running = Some(RunningServer {
            address,
            started_at_ms: now_ms(),
            max_players: settings.max_players,
            handle,
            stop,
            server_task,
            watcher_task,
        });

        let snapshot = snapshot_of(alive(&running));
        emit_state(app, &snapshot);
        Ok(snapshot)
    }

    pub async fn stop(&self, app: &AppHandle) -> ServerSnapshot {
        // 先把运行项摘出来就放掉锁，别在 `.await` 等服务端时一直占着：
        // 第二次按停止要能进来触发强制退出。
        // Take the running entry and drop the lock before awaiting, so a second
        // stop press can get in and force quit while the first one waits.
        let running = self.running.lock().await.take();
        if let Some(server) = running {
            // 优雅停止期间留着句柄，供第二次按停止强制退出。
            *self.force_handle.lock().await = Some(server.handle.clone());
            server.watcher_task.abort();
            // 接收端已经结束时发送会失败，那也正是我们要的结果。
            let _ = server.stop.send(());
            if let Err(error) = server.server_task.await {
                warn!(%error, "server task ended abnormally");
            }
            *self.force_handle.lock().await = None;
            info!("Bamboo stopped");
        } else if let Some(handle) = self.force_handle.lock().await.take() {
            // 第二次按停止：不再等存盘，强制退出。
            handle.force_stop();
            warn!("forcing the server to quit before the world finished saving");
        }

        let snapshot = snapshot_of(None);
        emit_state(app, &snapshot);
        snapshot
    }
}

fn snapshot_of(running: Option<&RunningServer>) -> ServerSnapshot {
    ServerSnapshot {
        running: running.is_some(),
        address: running.map(|server| server.address.to_string()),
        started_at_ms: running.map(|server| server.started_at_ms),
        max_players: running.map(|server| server.max_players),
        players: running
            .map(|server| {
                let mut players = server.handle.online_players();
                players.sort_by(|left, right| left.name.cmp(&right.name));
                players.into_iter().map(PlayerView::from).collect()
            })
            .unwrap_or_default(),
        game_version: GAME_VERSION,
        protocol_version: PROTOCOL_VERSION,
    }
}

/// 玩家进出时通知界面。界面自己持有快照，这里只推送变化后的玩家列表。
async fn watch_events(app: AppHandle, handle: ServerHandle) {
    let mut events = handle.subscribe();
    loop {
        match events.recv().await {
            Ok(ServerEvent::PlayerJoined(_) | ServerEvent::PlayerLeft(_)) => {}
            // 落后时直接重新读取完整列表，不需要补齐漏掉的事件。
            Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }

        let mut players = handle.online_players();
        players.sort_by(|left, right| left.name.cmp(&right.name));
        let players: Vec<PlayerView> = players.into_iter().map(PlayerView::from).collect();
        if let Err(error) = app.emit(PLAYERS_EVENT, players) {
            warn!(%error, "failed to send the player list to the window");
        }
    }
}

fn emit_state(app: &AppHandle, snapshot: &ServerSnapshot) {
    if let Err(error) = app.emit(STATE_EVENT, snapshot) {
        warn!(%error, "failed to send the server state to the window");
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_server::ServerConfig;
    use tokio::task::yield_now;

    use super::*;

    /// 一个已经停了的服务端留下的那一项，或者一个还在运行的。
    async fn entry(server_task: JoinHandle<()>) -> RunningServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = Server::new(listener, ServerConfig::default()).unwrap();
        RunningServer {
            address,
            started_at_ms: 0,
            max_players: 20,
            handle: server.handle(),
            stop: oneshot::channel().0,
            server_task,
            watcher_task: async_runtime::spawn(async {}),
        }
    }

    #[test]
    fn server_that_ended_on_its_own_does_not_count_as_running() {
        async_runtime::block_on(async {
            let (_keep_running, wait) = oneshot::channel::<()>();
            let running = Some(
                entry(async_runtime::spawn(async {
                    let _ = wait.await;
                }))
                .await,
            );
            assert!(snapshot_of(alive(&running)).running);

            let ended = async_runtime::spawn(async {});
            while !ended.inner().is_finished() {
                yield_now().await;
            }
            let running = Some(entry(ended).await);
            assert!(!snapshot_of(alive(&running)).running);
        });
    }
}
