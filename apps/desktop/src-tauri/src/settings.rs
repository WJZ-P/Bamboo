//! 用户设置，保存在应用配置目录的 JSON 文件里。
//!
//! 这里是界面和 [`ServerConfig`] 之间唯一的转换点：加一个服务端选项时，
//! 在 [`ServerSettings`] 上加一个字段，再在 [`ServerSettings::to_server_config`] 里接上去。
//! 那个函数写全了 `ServerConfig` 的每个字段，服务端新增选项时这里会编译不过，不会被忘掉。

use std::{fs, io::ErrorKind, path::PathBuf, time::Duration};

use mc_bamboo_server::{
    CLIENT_READ_TIMEOUT, ForwardingMode, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE, ServerConfig,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "server-settings.json";

/// 进入游戏之前的停留时限上限，纯粹是界面上的护栏，服务端本身不限制。
const MAX_PRE_PLAY_TIMEOUT_SECONDS: u64 = 3600;

/// 从代理接收玩家信息的方式，和 [`ForwardingMode`] 一一对应。
///
/// 另写一份是为了不让服务端 crate 依赖 serde，顺便固定下 JSON 里的拼写。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Forwarding {
    #[default]
    Off,
    Legacy,
    Modern,
}

impl From<Forwarding> for ForwardingMode {
    fn from(forwarding: Forwarding) -> Self {
        match forwarding {
            Forwarding::Off => Self::Off,
            Forwarding::Legacy => Self::Legacy,
            Forwarding::Modern => Self::Modern,
        }
    }
}

/// 界面里可以修改的服务端设置。
///
/// 缺失的字段用默认值补齐，这样旧版本的设置文件升级后还能继续用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettings {
    /// 监听地址。只有桌面端用得上，服务端拿到的是已经绑好的监听器。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    pub motd: String,
    pub max_players: u32,
    pub view_distance: u8,
    /// 连接进入游戏之前可以停留的秒数。
    pub pre_play_timeout_seconds: u64,
    /// 同时处于进入游戏之前阶段的连接上限。
    pub max_pre_play_connections: u32,
    /// 发送心跳的间隔秒数。
    pub keep_alive_seconds: u64,
    /// 启用压缩的包负载字节数，-1 表示关闭压缩。
    pub compression_threshold: i32,
    pub forwarding: Forwarding,
    /// Velocity 现代转发用的签名密钥。
    pub forwarding_secret: String,
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
            pre_play_timeout_seconds: defaults.pre_play_timeout.as_secs(),
            max_pre_play_connections: u32::try_from(defaults.max_pre_play_connections)
                .unwrap_or(u32::MAX),
            keep_alive_seconds: defaults.keep_alive_interval.as_secs(),
            compression_threshold: defaults.compression_threshold,
            forwarding: Forwarding::Off,
            forwarding_secret: defaults.forwarding_secret,
        }
    }
}

impl ServerSettings {
    pub fn to_server_config(&self) -> ServerConfig {
        ServerConfig {
            motd: self.motd.clone(),
            max_players: self.max_players,
            view_distance: self.view_distance,
            pre_play_timeout: Duration::from_secs(self.pre_play_timeout_seconds),
            max_pre_play_connections: self.max_pre_play_connections as usize,
            keep_alive_interval: Duration::from_secs(self.keep_alive_seconds),
            compression_threshold: self.compression_threshold,
            forwarding: self.forwarding.into(),
            forwarding_secret: self.forwarding_secret.clone(),
        }
    }

    /// 检查设置是不是能直接交给服务端。
    ///
    /// 边界和 `ServerConfig::normalized` 一致：那边会把越界的值悄悄改掉，
    /// 与其让用户以为自己填的值生效了，不如在保存和启动时就说清楚。
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("监听地址不能为空".to_owned());
        }
        if self.port == 0 {
            return Err("端口必须在 1 到 65535 之间".to_owned());
        }
        if self.max_players == 0 {
            return Err("最大玩家数至少是 1".to_owned());
        }
        if !(MIN_VIEW_DISTANCE..=MAX_VIEW_DISTANCE).contains(&self.view_distance) {
            return Err(format!(
                "视距必须在 {MIN_VIEW_DISTANCE} 到 {MAX_VIEW_DISTANCE} 之间"
            ));
        }
        if !(1..=MAX_PRE_PLAY_TIMEOUT_SECONDS).contains(&self.pre_play_timeout_seconds) {
            return Err(format!(
                "进服时限必须在 1 到 {MAX_PRE_PLAY_TIMEOUT_SECONDS} 秒之间"
            ));
        }
        if self.max_pre_play_connections == 0 {
            return Err("进服连接数上限至少是 1，否则没有人能连进来".to_owned());
        }
        // 心跳必须留出余量：到了客户端的读取时限它就自己断开了。
        let max_keep_alive = CLIENT_READ_TIMEOUT.as_secs() - 1;
        if !(1..=max_keep_alive).contains(&self.keep_alive_seconds) {
            return Err(format!("心跳间隔必须在 1 到 {max_keep_alive} 秒之间"));
        }
        if self.compression_threshold < -1 {
            return Err("压缩阈值最小是 -1，-1 表示关闭压缩".to_owned());
        }
        if self.forwarding == Forwarding::Modern && self.forwarding_secret.is_empty() {
            return Err("现代转发必须填写转发密钥".to_owned());
        }
        Ok(())
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
    settings.validate()?;
    let path = settings_path(app)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| format!("创建配置目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| format!("保存设置失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_come_from_the_server_config() {
        let defaults = ServerConfig::default();
        let config = ServerSettings::default().to_server_config();

        assert_eq!(config.motd, defaults.motd);
        assert_eq!(config.max_players, defaults.max_players);
        assert_eq!(config.view_distance, defaults.view_distance);
        assert_eq!(config.pre_play_timeout, defaults.pre_play_timeout);
        assert_eq!(
            config.max_pre_play_connections,
            defaults.max_pre_play_connections
        );
        assert_eq!(config.keep_alive_interval, defaults.keep_alive_interval);
        assert_eq!(config.compression_threshold, defaults.compression_threshold);
        assert_eq!(config.forwarding, defaults.forwarding);
        assert_eq!(config.forwarding_secret, defaults.forwarding_secret);
    }

    #[test]
    fn every_option_reaches_the_server_config() {
        let settings = ServerSettings {
            host: "0.0.0.0".to_owned(),
            port: 25566,
            motd: "hello".to_owned(),
            max_players: 7,
            view_distance: 12,
            pre_play_timeout_seconds: 45,
            max_pre_play_connections: 64,
            keep_alive_seconds: 20,
            compression_threshold: -1,
            forwarding: Forwarding::Modern,
            forwarding_secret: "secret".to_owned(),
        };
        let config = settings.to_server_config();

        assert_eq!(config.motd, "hello");
        assert_eq!(config.max_players, 7);
        assert_eq!(config.view_distance, 12);
        assert_eq!(config.pre_play_timeout, Duration::from_secs(45));
        assert_eq!(config.max_pre_play_connections, 64);
        assert_eq!(config.keep_alive_interval, Duration::from_secs(20));
        assert_eq!(config.compression_threshold, -1);
        assert_eq!(config.forwarding, ForwardingMode::Modern);
        assert_eq!(config.forwarding_secret, "secret");
    }

    #[test]
    fn forwarding_modes_map_one_to_one() {
        assert_eq!(ForwardingMode::from(Forwarding::Off), ForwardingMode::Off);
        assert_eq!(
            ForwardingMode::from(Forwarding::Legacy),
            ForwardingMode::Legacy
        );
        assert_eq!(
            ForwardingMode::from(Forwarding::Modern),
            ForwardingMode::Modern
        );
    }

    #[test]
    fn forwarding_is_stored_as_a_lower_case_word() {
        let settings = ServerSettings {
            forwarding: Forwarding::Modern,
            ..ServerSettings::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        assert!(text.contains(r#""forwarding":"modern""#), "{text}");

        let parsed: ServerSettings = serde_json::from_str(r#"{"forwarding":"legacy"}"#).unwrap();
        assert_eq!(parsed.forwarding, Forwarding::Legacy);
    }

    #[test]
    fn an_empty_settings_file_reads_as_the_defaults() {
        let parsed: ServerSettings = serde_json::from_str("{}").unwrap();
        let defaults = ServerSettings::default();

        assert_eq!(parsed.host, defaults.host);
        assert_eq!(parsed.port, defaults.port);
        assert_eq!(parsed.compression_threshold, defaults.compression_threshold);
        assert_eq!(parsed.forwarding, defaults.forwarding);
    }

    #[test]
    fn an_old_settings_file_keeps_what_it_has() {
        // 加入新选项之前保存的文件：已有的字段要留住，新字段取默认值。
        let text = r#"{
            "host": "0.0.0.0",
            "port": 25600,
            "motd": "old",
            "maxPlayers": 50,
            "viewDistance": 16
        }"#;
        let parsed: ServerSettings = serde_json::from_str(text).unwrap();
        let defaults = ServerSettings::default();

        assert_eq!(parsed.host, "0.0.0.0");
        assert_eq!(parsed.port, 25600);
        assert_eq!(parsed.motd, "old");
        assert_eq!(parsed.max_players, 50);
        assert_eq!(parsed.view_distance, 16);
        assert_eq!(
            parsed.keep_alive_seconds, defaults.keep_alive_seconds,
            "新字段要取默认值"
        );
        assert_eq!(parsed.forwarding, Forwarding::Off);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn the_defaults_are_valid() {
        assert_eq!(ServerSettings::default().validate(), Ok(()));
    }

    /// 改一项之后的校验结果，方便一项一项地试边界。
    fn validate(patch: impl FnOnce(&mut ServerSettings)) -> Result<(), String> {
        let mut settings = ServerSettings::default();
        patch(&mut settings);
        settings.validate()
    }

    #[test]
    fn the_listening_address_cannot_be_blank() {
        assert!(validate(|settings| settings.host = String::new()).is_err());
        assert!(validate(|settings| settings.host = "   ".to_owned()).is_err());
        assert!(validate(|settings| settings.host = "0.0.0.0".to_owned()).is_ok());
    }

    #[test]
    fn port_zero_is_rejected() {
        assert!(validate(|settings| settings.port = 0).is_err());
        assert!(validate(|settings| settings.port = 1).is_ok());
        assert!(validate(|settings| settings.port = u16::MAX).is_ok());
    }

    #[test]
    fn the_view_distance_bounds_match_the_server() {
        assert!(validate(|settings| settings.view_distance = MIN_VIEW_DISTANCE - 1).is_err());
        assert!(validate(|settings| settings.view_distance = MIN_VIEW_DISTANCE).is_ok());
        assert!(validate(|settings| settings.view_distance = MAX_VIEW_DISTANCE).is_ok());
        assert!(validate(|settings| settings.view_distance = MAX_VIEW_DISTANCE + 1).is_err());
    }

    #[test]
    fn the_keep_alive_interval_must_stay_under_the_client_timeout() {
        let timeout = CLIENT_READ_TIMEOUT.as_secs();
        assert!(validate(|settings| settings.keep_alive_seconds = 0).is_err());
        assert!(validate(|settings| settings.keep_alive_seconds = 1).is_ok());
        assert!(validate(|settings| settings.keep_alive_seconds = timeout - 1).is_ok());
        assert!(validate(|settings| settings.keep_alive_seconds = timeout).is_err());
    }

    #[test]
    fn the_pre_play_limits_leave_room_for_one_connection() {
        assert!(validate(|settings| settings.pre_play_timeout_seconds = 0).is_err());
        assert!(validate(|settings| settings.pre_play_timeout_seconds = 1).is_ok());
        assert!(
            validate(
                |settings| settings.pre_play_timeout_seconds = MAX_PRE_PLAY_TIMEOUT_SECONDS + 1
            )
            .is_err()
        );
        assert!(validate(|settings| settings.max_pre_play_connections = 0).is_err());
        assert!(validate(|settings| settings.max_pre_play_connections = 1).is_ok());
    }

    #[test]
    fn compression_below_minus_one_is_rejected() {
        assert!(validate(|settings| settings.compression_threshold = i32::MIN).is_err());
        assert!(validate(|settings| settings.compression_threshold = -2).is_err());
        assert!(validate(|settings| settings.compression_threshold = -1).is_ok());
        assert!(validate(|settings| settings.compression_threshold = 0).is_ok());
        assert!(validate(|settings| settings.compression_threshold = i32::MAX).is_ok());
    }

    #[test]
    fn max_players_cannot_be_zero() {
        assert!(validate(|settings| settings.max_players = 0).is_err());
        assert!(validate(|settings| settings.max_players = 1).is_ok());
    }

    #[test]
    fn modern_forwarding_needs_a_secret() {
        assert!(validate(|settings| settings.forwarding = Forwarding::Modern).is_err());
        assert!(
            validate(|settings| {
                settings.forwarding = Forwarding::Modern;
                settings.forwarding_secret = "secret".to_owned();
            })
            .is_ok()
        );
        // 旧式转发由代理自己保证可信，不需要密钥。
        assert!(validate(|settings| settings.forwarding = Forwarding::Legacy).is_ok());
    }
}
