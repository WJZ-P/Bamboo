//! 服务器设置：界面和服务器目录里的两份配置文件之间唯一的转换点。
//!
//! 官方 `server.properties` 和 Bamboo 的 `bamboo.toml` 由
//! [`mc_bamboo_server::config_files`] 读写，命令行版用的是同一套代码。
//! 这里只把文件内容变成前端好用的 [`ServerSettings`]，并做界面上的校验。
//!
//! 老版本把设置存在 APP 配置目录的 `server-settings.json` 里：第一次启动时
//! 如果服务器目录里两个新文件都不存在，就导入一次并写成新文件，之后不再读它。

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use mc_bamboo_server::{
    CLIENT_READ_TIMEOUT, ForwardingMode, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE, ServerConfig,
    config_files::{
        BAMBOO_FILE, ConfigFiles, PROPERTIES_FILE, load_or_create, log_startup, read, write,
    },
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

const OLD_SETTINGS_FILE: &str = "server-settings.json";
/// 服务器目录固定放在 APP 自己的数据目录下。
const SERVER_SUBDIRECTORY: &str = "server";

/// 进入游戏之前的停留时限上限，纯粹是界面上的护栏，服务端本身不限制。
const MAX_PRE_PLAY_TIMEOUT_SECONDS: u64 = 3600;

/// 从代理接收玩家信息的方式，和 [`ForwardingMode`] 一一对应。
///
/// 另写一份是为了固定 JSON 里的拼写，不把服务端的枚举直接暴露给前端。
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

impl From<ForwardingMode> for Forwarding {
    fn from(forwarding: ForwardingMode) -> Self {
        match forwarding {
            ForwardingMode::Off => Self::Off,
            ForwardingMode::Legacy => Self::Legacy,
            ForwardingMode::Modern => Self::Modern,
        }
    }
}

/// 老版本 `server-settings.json` 的结构。字段缺省时按 `ServerSettings::default()` 补齐。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct OldSettings {
    host: String,
    port: u16,
    motd: String,
    max_players: u32,
    view_distance: u8,
    pre_play_timeout_seconds: u64,
    max_pre_play_connections: u32,
    keep_alive_seconds: u64,
    compression_threshold: i32,
    online_mode: bool,
    prevent_proxy_connections: bool,
    enforce_secure_profile: bool,
    forwarding: Forwarding,
    forwarding_secret: String,
}

impl Default for OldSettings {
    fn default() -> Self {
        let defaults = ServerSettings::default();
        Self {
            host: defaults.host,
            port: defaults.port,
            motd: defaults.motd,
            max_players: defaults.max_players,
            view_distance: defaults.view_distance,
            pre_play_timeout_seconds: defaults.pre_play_timeout_seconds,
            max_pre_play_connections: defaults.max_pre_play_connections,
            keep_alive_seconds: defaults.keep_alive_seconds,
            compression_threshold: defaults.compression_threshold,
            online_mode: defaults.online_mode,
            prevent_proxy_connections: defaults.prevent_proxy_connections,
            enforce_secure_profile: defaults.enforce_secure_profile,
            forwarding: defaults.forwarding,
            forwarding_secret: defaults.forwarding_secret,
        }
    }
}

fn default_enforce_secure_profile() -> bool {
    true
}

/// 界面里可以修改的服务端设置，分成「官方设置」和「Bamboo 优化设置」两组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettings {
    /// 服务器目录（只读展示），APP 和命令行版共用这里的两个文件。
    pub server_directory: String,
    /// 监听地址。`server-ip` 为空时表示监听所有网卡。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    /// 世界名字。`level-name`，服务器目录 + 它就是世界目录。
    pub level_name: String,
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
    /// 要不要做正版验证，和官方的 `online-mode` 相同。和代理转发互斥。
    pub online_mode: bool,
    /// 问 session server 时带上客户端的 IP，和官方的 `prevent-proxy-connections` 相同。
    pub prevent_proxy_connections: bool,
    /// 正版聊天必须签名，和官方的 `enforce-secure-profile` 相同。没开正版验证时不生效。
    ///
    /// 缺省必须是 `true`。`serde(default)` 会用字段类型的 `Default`，而 `bool` 的默认是
    /// `false`，所以这里单独指定。
    #[serde(default = "default_enforce_secure_profile")]
    pub enforce_secure_profile: bool,
    pub forwarding: Forwarding,
    /// Velocity 现代转发用的签名密钥。
    pub forwarding_secret: String,
    /// 实体阶段线程数（实验）：0 = 精确模式，≥ 1 = 性能模式。
    pub entity_threads: u32,
    /// Director 信箱每刻最多交付多少个区块，0 表示不限。
    pub deliveries_per_tick: u32,
    /// Director 信箱每刻最多收回多少个区块，0 表示不限。
    pub reclaims_per_tick: u32,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self::from_files(&ConfigFiles::default(), "")
    }
}

impl ServerSettings {
    /// 从两份配置文件组装界面用的设置。`server_directory` 只是展示，不参与保存。
    fn from_files(files: &ConfigFiles, directory: &str) -> Self {
        let official = &files.official;
        let bamboo = &files.bamboo;
        Self {
            server_directory: directory.to_owned(),
            // 空 server-ip 在界面上显示成「所有网卡」用的地址；保存时再换回空。
            host: if official.server_ip.is_empty() {
                "0.0.0.0".to_owned()
            } else {
                official.server_ip.clone()
            },
            port: official.server_port,
            level_name: official.level_name.clone(),
            motd: official.motd.clone(),
            max_players: official.max_players,
            view_distance: official.view_distance,
            pre_play_timeout_seconds: bamboo.pre_play_timeout.as_secs(),
            max_pre_play_connections: u32::try_from(bamboo.max_pre_play_connections)
                .unwrap_or(u32::MAX),
            keep_alive_seconds: bamboo.keep_alive_interval.as_secs(),
            compression_threshold: official.compression_threshold,
            online_mode: official.online_mode,
            prevent_proxy_connections: official.prevent_proxy_connections,
            enforce_secure_profile: official.enforce_secure_profile,
            forwarding: bamboo.forwarding.into(),
            forwarding_secret: bamboo.forwarding_secret.clone(),
            entity_threads: u32::try_from(bamboo.entity_threads).unwrap_or(u32::MAX),
            deliveries_per_tick: u32::try_from(bamboo.deliveries_per_tick).unwrap_or(u32::MAX),
            reclaims_per_tick: u32::try_from(bamboo.reclaims_per_tick).unwrap_or(u32::MAX),
        }
    }

    pub fn to_server_config(&self) -> ServerConfig {
        let defaults = ServerConfig::default();
        ServerConfig {
            motd: self.motd.clone(),
            max_players: self.max_players,
            view_distance: self.view_distance,
            pre_play_timeout: Duration::from_secs(self.pre_play_timeout_seconds),
            max_pre_play_connections: self.max_pre_play_connections as usize,
            keep_alive_interval: Duration::from_secs(self.keep_alive_seconds),
            compression_threshold: self.compression_threshold,
            online_mode: self.online_mode,
            prevent_proxy_connections: self.prevent_proxy_connections,
            enforce_secure_profile: self.enforce_secure_profile,
            // session server 的地址只有测试会改，不出现在任何配置文件里。
            session_server_url: defaults.session_server_url,
            auth_connect_timeout: defaults.auth_connect_timeout,
            auth_read_timeout: defaults.auth_read_timeout,
            forwarding: self.forwarding.into(),
            forwarding_secret: self.forwarding_secret.clone(),
            // 四份管理名单固定放在服务器目录里，由 load 时的路径决定。
            list_directory: PathBuf::from(&self.server_directory),
            entity_threads: self.entity_threads as usize,
            mail_budget: mc_bamboo_game::MailBudget {
                deliveries: self.deliveries_per_tick as usize,
                reclaims: self.reclaims_per_tick as usize,
            },
        }
    }

    /// 把界面上的值写回两份文件内容，保留已有的未实现官方键。
    fn as_files(&self, existing: &ConfigFiles) -> ConfigFiles {
        let mut files = ConfigFiles::from_server_config(&self.to_server_config(), existing);
        files.official.server_ip = if self.host.trim() == "0.0.0.0" {
            String::new()
        } else {
            self.host.trim().to_owned()
        };
        files.official.server_port = self.port;
        files.official.level_name = self.level_name.trim().to_owned();
        files
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
        if self.level_name.trim().is_empty() {
            return Err("世界名字不能为空".to_owned());
        }
        if self.level_name.contains('/') || self.level_name.contains('\\') {
            return Err("世界名字不能带路径分隔符".to_owned());
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
        if self.entity_threads as usize > mc_bamboo_game::MAX_ENTITY_THREADS {
            return Err(format!(
                "实体并行线程数不能超过 {}",
                mc_bamboo_game::MAX_ENTITY_THREADS
            ));
        }
        if self.deliveries_per_tick as usize > mc_bamboo_game::MAX_MAIL_BUDGET
            || self.reclaims_per_tick as usize > mc_bamboo_game::MAX_MAIL_BUDGET
        {
            return Err(format!(
                "每刻区块预算不能超过 {}",
                mc_bamboo_game::MAX_MAIL_BUDGET
            ));
        }
        // 服务端那边会把正版验证悄悄关掉，与其让用户以为两个都开着，不如在这里说清楚。
        if self.online_mode && self.forwarding != Forwarding::Off {
            return Err("正版验证和代理转发不能同时打开：代理已经替服务端验过身份了".to_owned());
        }
        Ok(())
    }
}

/// APP 自己的数据目录下面的 `server/`，命令行版用 `--server-dir` 指到这里即可共用。
pub fn server_directory(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("找不到应用数据目录：{error}"))?
        .join(SERVER_SUBDIRECTORY);
    fs::create_dir_all(&directory).map_err(|error| format!("创建服务器目录失败：{error}"))?;
    Ok(directory)
}

fn old_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("找不到配置目录：{error}"))?;
    Ok(directory.join(OLD_SETTINGS_FILE))
}

/// 读取设置。两个新文件都不存在且老 `server-settings.json` 存在时导入一次，
/// 之后只读新文件。导入后老文件保留，但不再读它。
pub fn load(app: &AppHandle) -> Result<ServerSettings, String> {
    let directory = server_directory(app)?;
    let new_properties = directory.join(PROPERTIES_FILE);
    let new_bamboo = directory.join(BAMBOO_FILE);
    let old_path = old_settings_path(app)?;
    let needs_import =
        !new_properties.exists() && !new_bamboo.exists() && fs::read_to_string(&old_path).is_ok();
    if needs_import {
        import_old_settings(&directory, &old_path)?;
    }
    let files =
        load_or_create(&directory, true).map_err(|error| format!("读取配置失败：{error}"))?;
    log_startup(&directory, &files);
    Ok(ServerSettings::from_files(
        &files,
        &directory.display().to_string(),
    ))
}

/// 把老 JSON 映射到两份新文件，缺的字段取老版本默认值。
fn import_old_settings(directory: &Path, old_path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(old_path).map_err(|error| format!("读取旧设置失败：{error}"))?;
    let old: OldSettings = match serde_json::from_str(&text) {
        Ok(settings) => settings,
        Err(error) => {
            warn!(%error, file = %old_path.display(), "cannot parse the old settings file, using defaults");
            OldSettings::default()
        }
    };
    let mut settings = ServerSettings {
        host: old.host,
        port: old.port,
        motd: old.motd,
        max_players: old.max_players,
        view_distance: old.view_distance,
        pre_play_timeout_seconds: old.pre_play_timeout_seconds,
        max_pre_play_connections: old.max_pre_play_connections,
        keep_alive_seconds: old.keep_alive_seconds,
        compression_threshold: old.compression_threshold,
        online_mode: old.online_mode,
        prevent_proxy_connections: old.prevent_proxy_connections,
        enforce_secure_profile: old.enforce_secure_profile,
        forwarding: old.forwarding,
        forwarding_secret: old.forwarding_secret,
        ..Default::default()
    };
    // 旧文件里非法的组合照服务端的行为修正，导入不能比启动更严格。
    if settings.online_mode && settings.forwarding != Forwarding::Off {
        settings.online_mode = false;
    }
    let existing = read(directory).unwrap_or_default();
    let files = settings.as_files(&existing);
    write(directory, &files).map_err(|error| format!("写入新配置失败：{error}"))?;
    info!(file = %old_path.display(), directory = %directory.display(), "imported the old server-settings.json once; it will not be read again");
    Ok(())
}

pub fn save(app: &AppHandle, settings: &ServerSettings) -> Result<(), String> {
    settings.validate()?;
    let directory = server_directory(app)?;
    let existing = read(&directory).unwrap_or_default();
    write(&directory, &settings.as_files(&existing))
        .map_err(|error| format!("保存设置失败：{error}"))
}

/// 出厂设置。界面用它显示每一项的默认值，也用来恢复默认。
pub fn defaults(app: &AppHandle) -> Result<ServerSettings, String> {
    let directory = server_directory(app)?;
    Ok(ServerSettings::from_files(
        &ConfigFiles::default(),
        &directory.display().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bamboo-desktop-settings-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            Self(temp_dir())
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 绕过 Tauri 的 AppHandle，直接测导入和文件往返。
    fn import_into(directory: &Path, old_text: &str) {
        let old = directory.parent().unwrap().join(OLD_SETTINGS_FILE);
        fs::write(&old, old_text).unwrap();
        import_old_settings(directory, &old).unwrap();
    }

    #[test]
    fn defaults_come_from_the_config_files() {
        let defaults = ServerSettings::default();
        assert_eq!(defaults.motd, "A Minecraft Server");
        assert_eq!(defaults.view_distance, 10);
        assert!(defaults.online_mode);
        assert_eq!(defaults.entity_threads, 0);
        assert_eq!(defaults.deliveries_per_tick, 64);
        assert_eq!(defaults.reclaims_per_tick, 64);
        assert_eq!(defaults.port, 25565);
        assert_eq!(defaults.level_name, "world");
        assert_eq!(ServerSettings::default().validate(), Ok(()));
    }

    #[test]
    fn every_option_reaches_the_server_config() {
        let settings = ServerSettings {
            server_directory: "/tmp/server".to_owned(),
            host: "0.0.0.0".to_owned(),
            port: 25566,
            level_name: "survival".to_owned(),
            motd: "hello".to_owned(),
            max_players: 7,
            view_distance: 12,
            pre_play_timeout_seconds: 45,
            max_pre_play_connections: 64,
            keep_alive_seconds: 20,
            compression_threshold: -1,
            // 转发开着时正版验证不能开，所以这一项在下一个测试里单独试。
            online_mode: false,
            prevent_proxy_connections: true,
            enforce_secure_profile: false,
            forwarding: Forwarding::Modern,
            forwarding_secret: "secret".to_owned(),
            entity_threads: 4,
            deliveries_per_tick: 16,
            reclaims_per_tick: 8,
        };
        let config = settings.to_server_config();
        assert_eq!(config.motd, "hello");
        assert_eq!(config.max_players, 7);
        assert_eq!(config.view_distance, 12);
        assert_eq!(config.pre_play_timeout, Duration::from_secs(45));
        assert_eq!(config.max_pre_play_connections, 64);
        assert_eq!(config.keep_alive_interval, Duration::from_secs(20));
        assert_eq!(config.compression_threshold, -1);
        assert!(!config.online_mode);
        assert!(config.prevent_proxy_connections);
        assert!(!config.enforce_secure_profile);
        assert_eq!(config.forwarding, ForwardingMode::Modern);
        assert_eq!(config.forwarding_secret, "secret");
        assert_eq!(config.entity_threads, 4);
        assert_eq!(config.mail_budget.deliveries, 16);
        assert_eq!(config.mail_budget.reclaims, 8);
        assert_eq!(config.list_directory, PathBuf::from("/tmp/server"));
    }

    #[test]
    fn online_mode_reaches_the_server_config() {
        let settings = ServerSettings {
            online_mode: true,
            ..ServerSettings::default()
        };
        assert!(settings.validate().is_ok());
        assert!(settings.to_server_config().online_mode);
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
        assert_eq!(Forwarding::from(ForwardingMode::Modern), Forwarding::Modern);
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
    fn an_empty_settings_draft_reads_as_the_defaults() {
        let parsed: ServerSettings = serde_json::from_str("{}").unwrap();
        let defaults = ServerSettings::default();
        assert_eq!(parsed.host, defaults.host);
        assert_eq!(parsed.port, defaults.port);
        assert_eq!(parsed.compression_threshold, defaults.compression_threshold);
        assert_eq!(parsed.forwarding, defaults.forwarding);
    }

    #[test]
    fn saving_writes_both_files_and_reading_back_is_identical() {
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        let mut settings =
            ServerSettings::from_files(&ConfigFiles::default(), &directory.display().to_string());
        settings.motd = "my server".to_owned();
        settings.port = 25600;
        settings.level_name = "creative".to_owned();
        settings.entity_threads = 2;
        settings.deliveries_per_tick = 32;
        settings.keep_alive_seconds = 20;
        let files = settings.as_files(&ConfigFiles::default());
        write(&directory, &files).unwrap();
        let loaded = ServerSettings::from_files(
            &read(&directory).unwrap(),
            &directory.display().to_string(),
        );
        assert_eq!(loaded.motd, "my server");
        assert_eq!(loaded.port, 25600);
        assert_eq!(loaded.level_name, "creative");
        assert_eq!(loaded.entity_threads, 2);
        assert_eq!(loaded.deliveries_per_tick, 32);
        assert_eq!(loaded.keep_alive_seconds, 20);
        let properties = fs::read_to_string(directory.join(PROPERTIES_FILE)).unwrap();
        assert!(properties.contains("server-port=25600\n"));
        assert!(properties.contains("motd=my server\n"));
        assert!(properties.contains("white-list=true\n"));
        let bamboo = fs::read_to_string(directory.join(BAMBOO_FILE)).unwrap();
        assert!(bamboo.contains("entity_threads = 2\n"));
    }

    #[test]
    fn an_old_settings_file_is_imported_once() {
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        import_into(
            &directory,
            r#"{
                "host": "0.0.0.0",
                "port": 25600,
                "motd": "old",
                "maxPlayers": 50,
                "viewDistance": 16,
                "onlineMode": true,
                "forwarding": "legacy",
                "keepAliveSeconds": 20
            }"#,
        );
        let files = read(&directory).unwrap();
        assert_eq!(files.official.motd, "old");
        assert_eq!(files.official.server_port, 25600);
        assert_eq!(files.official.max_players, 50);
        assert_eq!(files.official.view_distance, 16);
        assert_eq!(files.bamboo.keep_alive_interval, Duration::from_secs(20));
        // 旧文件里非法组合（正版验证 + 转发）按服务端的行为修正。
        assert!(!files.official.online_mode);
        assert_eq!(files.bamboo.forwarding, ForwardingMode::Legacy);
        // 导入后两个新文件都在；之后 load 就不再看旧文件了。
        assert!(directory.join(PROPERTIES_FILE).exists());
        assert!(directory.join(BAMBOO_FILE).exists());
        assert_eq!(files.unsupported_keys().len(), 59);
    }

    #[test]
    fn a_broken_old_settings_file_still_writes_usable_defaults() {
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        import_into(&directory, "{not json");
        let files = read(&directory).unwrap();
        assert_eq!(files.official, ConfigFiles::default().official);
        assert_eq!(files.bamboo, ConfigFiles::default().bamboo);
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
    fn port_and_level_name_cannot_be_blank() {
        assert!(validate(|settings| settings.port = 0).is_err());
        assert!(validate(|settings| settings.port = 1).is_ok());
        assert!(validate(|settings| settings.level_name = String::new()).is_err());
        assert!(validate(|settings| settings.level_name = "../x".to_owned()).is_err());
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
            validate(|settings| settings.pre_play_timeout_seconds = MAX_PRE_PLAY_TIMEOUT_SECONDS)
                .is_ok()
        );
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
    fn the_performance_knobs_have_upper_bounds() {
        assert!(validate(|settings| settings.entity_threads = 1).is_ok());
        assert!(validate(|settings| settings.entity_threads = u32::MAX).is_err());
        assert!(validate(|settings| settings.deliveries_per_tick = 0).is_ok());
        assert!(validate(|settings| settings.deliveries_per_tick = u32::MAX).is_err());
        assert!(validate(|settings| settings.reclaims_per_tick = u32::MAX).is_err());
    }

    #[test]
    fn max_players_cannot_be_zero() {
        assert!(validate(|settings| settings.max_players = 0).is_err());
        assert!(validate(|settings| settings.max_players = 1).is_ok());
    }

    #[test]
    fn modern_forwarding_needs_a_secret() {
        // 官方默认是开正版验证的，测试转发时要先把它关掉（两者互斥）。
        let without_online_mode = |patch: fn(&mut ServerSettings)| {
            let mut settings = ServerSettings {
                online_mode: false,
                ..Default::default()
            };
            patch(&mut settings);
            settings.validate()
        };
        assert!(without_online_mode(|settings| settings.forwarding = Forwarding::Modern).is_err());
        assert!(
            without_online_mode(|settings| {
                settings.forwarding = Forwarding::Modern;
                settings.forwarding_secret = "secret".to_owned();
            })
            .is_ok()
        );
        // 旧式转发由代理自己保证可信，不需要密钥。
        assert!(without_online_mode(|settings| settings.forwarding = Forwarding::Legacy).is_ok());
    }

    #[test]
    fn online_mode_and_forwarding_are_mutually_exclusive() {
        assert!(validate(|settings| settings.online_mode = true).is_ok());
        assert!(
            validate(|settings| {
                settings.online_mode = true;
                settings.forwarding = Forwarding::Legacy;
            })
            .is_err()
        );
        assert!(
            validate(|settings| {
                settings.online_mode = true;
                settings.forwarding = Forwarding::Modern;
                settings.forwarding_secret = "secret".to_owned();
            })
            .is_err()
        );
    }

    /// 新字段在旧结构的 JSON 里不存在时取默认值，不能让旧草稿偷偷改变服务端行为。
    #[test]
    fn old_drafts_keep_the_new_defaults() {
        let parsed: ServerSettings = serde_json::from_str("{}").unwrap();
        assert!(parsed.online_mode);
        assert!(!parsed.prevent_proxy_connections);
        assert_eq!(parsed.entity_threads, 0);
        assert_eq!(parsed.deliveries_per_tick, 64);
    }
}
