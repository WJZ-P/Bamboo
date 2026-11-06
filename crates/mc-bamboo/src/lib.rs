//! The Bamboo Minecraft server.

mod connection;

use std::{future::Future, sync::Arc, time::Duration};

use mc_bamboo_protocol::{
    GAME_VERSION, PROTOCOL_VERSION,
    frame::{FrameError, append_frame},
    packet::{LoginDisconnect, StatusResponse},
};
use serde_json::json;
use tokio::{net::TcpListener, sync::Semaphore};
use tracing::{debug, warn};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const LOGIN_REJECTED_MESSAGE: &str = "Bamboo is under construction: joining is not available yet.";

/// Server settings.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The message shown in the server list.
    pub motd: String,
    pub max_players: u32,
    /// How long a connection may stay in the states before Play, in total.
    pub pre_play_timeout: Duration,
    /// How many connections may be in those states at once. Any extra ones are dropped.
    pub max_pre_play_connections: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: "A Bamboo Server".to_owned(),
            max_players: 20,
            pre_play_timeout: Duration::from_secs(30),
            max_pre_play_connections: 1024,
        }
    }
}

/// Responses that are the same for every client, encoded once at startup so that answering
/// a server list ping costs no serialization.
struct PreEncodedResponses {
    status: Vec<u8>,
    login_rejected: Vec<u8>,
}

impl PreEncodedResponses {
    fn new(config: &ServerConfig) -> Result<Self, FrameError> {
        let status_json = json!({
            "version": { "name": format!("Bamboo {GAME_VERSION}"), "protocol": PROTOCOL_VERSION },
            "players": { "max": config.max_players, "online": 0 },
            "description": { "text": config.motd },
            "enforcesSecureChat": false,
        })
        .to_string();
        let reason_json = json!({ "text": LOGIN_REJECTED_MESSAGE }).to_string();

        let mut status = Vec::new();
        append_frame(&mut status, &StatusResponse { json: &status_json })?;
        let mut login_rejected = Vec::new();
        append_frame(
            &mut login_rejected,
            &LoginDisconnect {
                reason_json: &reason_json,
            },
        )?;
        Ok(Self {
            status,
            login_rejected,
        })
    }
}

/// Accepts connections on `listener` until `shutdown` completes.
pub async fn serve(
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()>,
) -> Result<(), FrameError> {
    let responses = Arc::new(PreEncodedResponses::new(&config)?);
    let slots = Arc::new(Semaphore::new(config.max_pre_play_connections));
    tokio::pin!(shutdown);

    loop {
        let (stream, peer) = tokio::select! {
            () = &mut shutdown => return Ok(()),
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(error) => {
                    // Usually out of sockets, so retrying at once would only spin.
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
        let responses = Arc::clone(&responses);
        let time_limit = config.pre_play_timeout;
        tokio::spawn(async move {
            connection::handle(stream, peer, &responses, time_limit).await;
            drop(slot);
        });
    }
}
