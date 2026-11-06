//! 无界面的命令行入口：`bamboo [监听地址]`。

use std::{error::Error, future};

use mc_bamboo_server::{GAME_VERSION, Server, ServerConfig};
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:25565";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.to_owned());
    let listener = TcpListener::bind(&address).await?;
    info!(
        "Bamboo is listening on {} for Minecraft {GAME_VERSION}",
        listener.local_addr()?
    );

    Server::new(listener, ServerConfig::default())?
        .run(shutdown_signal())
        .await;
    info!("Bamboo stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "cannot listen for Ctrl+C");
        future::pending::<()>().await;
    }
}
