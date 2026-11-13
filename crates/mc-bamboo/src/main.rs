//! 无界面的命令行入口：`bamboo [--server-dir <目录>] [监听地址]`。
//!
//! 配置从服务器目录里的 `server.properties` 和 `bamboo.toml` 读；两个文件都缺时
//! 用默认值写出来。可选的位置参数 `host:port` 只覆盖这一次的监听地址，不写回文件。

use std::{error::Error, future, path::PathBuf};

use mc_bamboo_server::{
    GAME_VERSION, Server,
    config_files::{ConfigFiles, load_or_create, log_startup, world_dir},
};
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "0.0.0.0";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let (directory, address_override) = parse_args(std::env::args().skip(1))?;
    let files: ConfigFiles = load_or_create(&directory, true)?;
    log_startup(&directory, &files);

    let address = match address_override {
        Some(address) => {
            info!(
                %address,
                "listening address comes from the command line and overrides server-ip / server-port"
            );
            address
        }
        None => {
            let host = if files.official.server_ip.trim().is_empty() {
                DEFAULT_HOST
            } else {
                files.official.server_ip.trim()
            };
            format!("{host}:{}", files.official.server_port)
        }
    };
    let listener = TcpListener::bind(&address).await?;
    let config = files.to_server_config(&directory);
    info!(
        "Bamboo is listening on {} for Minecraft {GAME_VERSION}; world directory {}",
        listener.local_addr()?,
        world_dir(&directory, &files).display()
    );

    Server::new(listener, config)?.run(shutdown_signal()).await;
    info!("Bamboo stopped");
    Ok(())
}

/// 解析命令行。只有 `--server-dir=<目录>` / `--server-dir <目录>` 和一个可选的
/// `host:port` 位置参数；多余参数直接报错，不猜用户想干什么。
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<(PathBuf, Option<String>), String> {
    let mut directory = PathBuf::from(".");
    let mut address = None;
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--server-dir=") {
            if value.is_empty() {
                return Err("--server-dir 后面要有目录路径".to_owned());
            }
            directory = PathBuf::from(value);
        } else if arg == "--server-dir" {
            let value = args
                .next()
                .ok_or_else(|| "--server-dir 后面要有目录路径".to_owned())?;
            if value.is_empty() {
                return Err("--server-dir 后面要有目录路径".to_owned());
            }
            directory = PathBuf::from(value);
        } else if arg.starts_with('-') {
            return Err(format!("不认识的参数：{arg}"));
        } else if address.is_some() {
            return Err("最多只能给一个监听地址".to_owned());
        } else {
            address = Some(arg);
        }
    }
    Ok((directory, address))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "cannot listen for Ctrl+C");
        future::pending::<()>().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_default_directory_is_the_current_one() {
        let (directory, address) = parse_args([]).unwrap();
        assert_eq!(directory, PathBuf::from("."));
        assert_eq!(address, None);
    }

    #[test]
    fn the_server_dir_can_be_given_in_both_forms() {
        let (directory, _) = parse_args(strings(&["--server-dir", "/tmp/bamboo"])).unwrap();
        assert_eq!(directory, PathBuf::from("/tmp/bamboo"));
        let (directory, _) = parse_args(strings(&["--server-dir=/tmp/bamboo"])).unwrap();
        assert_eq!(directory, PathBuf::from("/tmp/bamboo"));
    }

    #[test]
    fn a_position_argument_overrides_the_listening_address() {
        let (directory, address) = parse_args(strings(&["127.0.0.1:25566"])).unwrap();
        assert_eq!(directory, PathBuf::from("."));
        assert_eq!(address.as_deref(), Some("127.0.0.1:25566"));
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(parse_args(strings(&["--server-dir"])).is_err());
        assert!(parse_args(strings(&["--server-dir="])).is_err());
        assert!(parse_args(strings(&["--other"])).is_err());
        assert!(parse_args(strings(&["a:1", "b:2"])).is_err());
    }
}
