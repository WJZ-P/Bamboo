//! 用户设置，保存在应用配置目录的 JSON 文件里。

use std::{fs, io::ErrorKind, path::PathBuf};

use mc_bamboo_server::ServerConfig;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "server-settings.json";

/// 界面里可以修改的服务端设置。
///
/// 缺失的字段用默认值补齐，这样旧版本的设置文件升级后还能继续用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub motd: String,
    pub max_players: u32,
    pub view_distance: u8,
}

impl Default for ServerSettings {
    fn default() -> Self {
        let defaults = ServerConfig::default();
        Self {
            // 默认只监听本机：不会触发防火墙弹窗，要开放给局域网时再手动修改。
            host: "127.0.0.1".to_owned(),
            port: 25565,
            motd: defaults.motd,
            max_players: defaults.max_players,
            view_distance: defaults.view_distance,
        }
    }
}

impl ServerSettings {
    pub fn to_server_config(&self) -> ServerConfig {
        ServerConfig {
            motd: self.motd.clone(),
            max_players: self.max_players,
            view_distance: self.view_distance,
            ..ServerConfig::default()
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("找不到配置目录：{error}"))?;
    Ok(directory.join(SETTINGS_FILE))
}

/// 读取设置。还没有设置文件时返回默认值。
pub fn load(app: &AppHandle) -> Result<ServerSettings, String> {
    let path = settings_path(app)?;
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| format!("设置文件已损坏：{error}")),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ServerSettings::default()),
        Err(error) => Err(format!("读取设置失败：{error}")),
    }
}

pub fn save(app: &AppHandle, settings: &ServerSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| format!("创建配置目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| format!("保存设置失败：{error}"))
}
