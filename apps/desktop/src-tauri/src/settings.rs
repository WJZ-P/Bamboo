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

use mc_bamboo_game::world_options::{FlatSettings, LevelType, WorldGenConfig};
use mc_bamboo_game::worldgen_cache::{
    MAX_FEATURE_CACHE_MB, MAX_IDLE_SECONDS, MAX_TERRAIN_CACHE_MB, WorldgenCacheSettings,
};
use mc_bamboo_game::{
    MAX_ENTITY_COLLISIONS, MAX_ENTITY_THREADS, MAX_MOB_DISTANCE, MAX_MOB_WAKEUPS_SCALE,
    MAX_PATHFINDING_BUDGET, MAX_VILLAGER_BRAIN_INTERVAL,
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

/// 新世界用的世界预设，和 [`LevelType`] 一一对应。
///
/// 另写一份是为了固定 JSON 里的拼写，不把世界生成的枚举直接暴露给前端。
///
/// The world preset for a new world, one-to-one with [`LevelType`]. A separate
/// copy fixes the JSON spelling instead of exposing the worldgen enum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorldType {
    #[default]
    Normal,
    Flat,
    LargeBiomes,
    Amplified,
}

impl From<WorldType> for LevelType {
    fn from(world_type: WorldType) -> Self {
        match world_type {
            WorldType::Normal => Self::Normal,
            WorldType::Flat => Self::Flat,
            WorldType::LargeBiomes => Self::LargeBiomes,
            WorldType::Amplified => Self::Amplified,
        }
    }
}

impl From<LevelType> for WorldType {
    fn from(level_type: LevelType) -> Self {
        match level_type {
            LevelType::Normal => Self::Normal,
            LevelType::Flat => Self::Flat,
            LevelType::LargeBiomes => Self::LargeBiomes,
            LevelType::Amplified => Self::Amplified,
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

/// 老版本界面的 JSON 里没有 `islands` 时用服务端的缺省（开）。
///
/// The server's default (on) when JSON from an older UI lacks `islands`.
fn default_islands() -> bool {
    mc_bamboo_game::DEFAULT_ISLANDS
}

fn default_villager_lobotomize_interval() -> i32 {
    100
}

fn default_villager_lobotomize_wait_trade_lock() -> bool {
    true
}

/// 旧草稿里没有 `logLevel` 时用服务端的缺省 info。
///
/// The server default info when an older draft has no `logLevel`.
fn default_log_level() -> String {
    "info".to_owned()
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
    /// 每只生物每刻最多处理几次推挤（0 = 不限，精确模式）。
    ///
    /// Per-mob per-tick push cap (0 = unlimited, exact mode).
    pub max_entity_collisions: u32,
    /// 三档降频的降频档起点（格）：0 = 不启用。
    ///
    /// The throttle-tier distance in blocks; 0 disables the tier.
    pub mob_throttle_distance: i32,
    /// 三档降频的休眠档起点（格）：0 = 不启用。
    ///
    /// The sleep-tier distance in blocks; 0 disables the tier.
    pub mob_sleep_distance: i32,
    /// 休眠生物每刻唤醒名额的倍率（1 = Paper 缺省）。
    ///
    /// Wake-up allowance multiplier for sleeping mobs (1 = Paper default).
    pub mob_wakeups_scale: u32,
    /// 独立寻路阶段（实验）：请求挂起一刻后统一计算，结果下一刻交给生物。
    /// 打开后生物转向最多晚 1 刻（50 毫秒）。缺省关，和官方逐位一致。
    ///
    /// The separate pathfinding phase (experimental). Requests are held for a
    /// tick, solved together and handed back at the start of the next one, so
    /// turns lag by up to 50 ms. Off by default for exact vanilla behaviour.
    pub mob_pathfinding_phase: bool,
    /// 每个岛每刻最多展开的 A* 节点数；0 = 65536。只在寻路阶段打开时有意义。
    /// 合法范围 0 到 [`MAX_PATHFINDING_BUDGET`]。
    ///
    /// Per-island A* node budget per tick, 0 = 65536. Only used with the
    /// pathfinding phase; the server accepts 0 to [`MAX_PATHFINDING_BUDGET`].
    pub mob_pathfinding_budget: u32,
    /// 寻路失败冷却（实验，Paper 的数值）：同一只生物连续失败超过 10 次后
    /// 40 刻（2 秒）内不再计算它的请求。只在寻路阶段打开时有意义，缺省关。
    ///
    /// Pathfinding failure cooldown (Paper's numbers): after more than 10
    /// consecutive failures a mob skips path requests for 40 ticks (2 s).
    /// Only meaningful with the pathfinding phase; off by default.
    pub mob_pathfinding_fail_cooldown: bool,
    /// 被困村民降频（照 Purpur lobotomize，缺省关）：四水平邻格均不可通行时按间隔运行大脑。
    ///
    /// Stuck villager brain throttling (Purpur lobotomize, off by default).
    pub villager_lobotomize: bool,
    /// 被困村民大脑运行间隔（刻），推荐 100，合法范围 1..=[`MAX_VILLAGER_BRAIN_INTERVAL`]。
    ///
    /// Brain tick interval for stuck villagers (ticks); recommended 100.
    #[serde(default = "default_villager_lobotomize_interval")]
    pub villager_lobotomize_interval: i32,
    /// 为真时只对已锁定交易（`xp > 0`）的被困村民降频，缺省 true。
    ///
    /// Only throttle stuck villagers whose trades are locked (`xp > 0`); default true.
    #[serde(default = "default_villager_lobotomize_wait_trade_lock")]
    pub villager_lobotomize_wait_trade_lock: bool,
    /// 睡觉村民大脑运行间隔（刻）：0 = 关（精确模式），推荐 5。
    ///
    /// Brain tick interval for sleeping villagers (ticks); 0 = off, recommended 5.
    pub villager_sleep_brain_interval: i32,
    /// 红石缓存（实验）：结果和官方逐位一致，打开后对大型红石电路有效。
    pub redstone_cache: bool,
    /// 区块计算池的线程数：0 = 内联（默认），≥ 1 = 常驻后台线程。
    pub chunk_worker_threads: u32,
    /// 世界生成地形缓存的上限（MiB），0 = 不缓存。只影响内存和速度。
    /// Worldgen terrain cache limit in MiB, 0 = no cache. Memory and speed only.
    pub worldgen_terrain_cache_mb: u32,
    /// 世界生成地物结果缓存的上限（MiB），0 = 不缓存。
    /// Worldgen feature-result cache limit in MiB, 0 = no cache.
    pub worldgen_feature_cache_mb: u32,
    /// 多少秒没有区块生成就清空这两份缓存，0 = 不清。
    /// Seconds without chunk generation before both caches are emptied, 0 = never.
    pub worldgen_cache_idle_seconds: u32,
    /// Director 信箱每刻最多交付多少个区块，0 表示不限。
    pub deliveries_per_tick: u32,
    /// Director 信箱每刻最多收回多少个区块，0 表示不限。
    pub reclaims_per_tick: u32,
    /// 岛的 tick 线程数：0 = 精确模式（岛在世界线程上跑），≥ 1 = 和主控并行（默认逻辑核数的
    /// 一半，夹在 2 到 8 之间）。
    pub island_threads: u32,
    /// 多岛：true = 相距很远的玩家群各自一个岛（默认）；false = 整个世界一个岛（精确模式）。
    /// 缺省是 `true`，理由同 `enforce_secure_profile`：`bool` 的 `Default` 是 `false`。
    ///
    /// Multi-island mode: `true` (the default) splits distant groups onto
    /// their own islands; `false` keeps the whole world one island (exact
    /// mode). Defaults to `true`, so like `enforce_secure_profile` it names
    /// its own default.
    #[serde(default = "default_islands")]
    pub islands: bool,
    /// 世界生成种子，对应 `level-seed` 的原文：空串表示新建世界时随机取一个，
    /// 数字按 `Long.parseLong`，其它字符串按 `String.hashCode` 算成种子（和官方相同）。
    /// 只在新建世界时生效，已有存档的种子以存档里的为准。
    ///
    /// The world seed, verbatim from `level-seed`: empty picks a random one when
    /// a world is created, digits use `Long.parseLong`, any other string uses
    /// `String.hashCode`. Only applies to new worlds; an existing save keeps its
    /// seed.
    pub level_seed: String,
    /// 世界类型，对应 `level-type`。只在新建世界时生效。
    ///
    /// The world preset (`level-type`). Only applies to new worlds.
    pub level_type: WorldType,
    /// 新世界要不要生成结构，对应 `generate-structures`。
    ///
    /// Whether a new world generates structures (`generate-structures`).
    pub generate_structures: bool,
    /// 超平坦预设的 JSON 原文，对应 `generator-settings`，只在 [`WorldType::Flat`] 下生效。
    ///
    /// The flat preset JSON, verbatim from `generator-settings`; only used by
    /// [`WorldType::Flat`].
    pub generator_settings: String,
    /// 全局日志级别（error/warn/info/debug/trace），写进 `bamboo.toml` 的 `log_level`。
    ///
    /// The global log level (error/warn/info/debug/trace), written to
    /// `log_level` in `bamboo.toml`.
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 按模块细调的 `EnvFilter` 指令，写进 `bamboo.toml` 的 `log_filter`，缺省空。
    ///
    /// Per-module `EnvFilter` directives, written to `log_filter` in
    /// `bamboo.toml`; default empty.
    pub log_filter: String,
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
            max_entity_collisions: u32::try_from(bamboo.max_entity_collisions).unwrap_or(u32::MAX),
            mob_throttle_distance: bamboo.mob_throttle_distance,
            mob_sleep_distance: bamboo.mob_sleep_distance,
            mob_wakeups_scale: u32::try_from(bamboo.mob_wakeups_scale).unwrap_or(u32::MAX),
            mob_pathfinding_phase: bamboo.mob_pathfinding_phase,
            mob_pathfinding_budget: bamboo.mob_pathfinding_budget,
            mob_pathfinding_fail_cooldown: bamboo.mob_pathfinding_fail_cooldown,
            villager_lobotomize: bamboo.villager_lobotomize,
            villager_lobotomize_interval: bamboo.villager_lobotomize_interval,
            villager_lobotomize_wait_trade_lock: bamboo.villager_lobotomize_wait_trade_lock,
            villager_sleep_brain_interval: bamboo.villager_sleep_brain_interval,
            redstone_cache: bamboo.redstone_cache,
            chunk_worker_threads: u32::try_from(bamboo.chunk_worker_threads).unwrap_or(u32::MAX),
            worldgen_terrain_cache_mb: u32::try_from(bamboo.worldgen_cache.terrain_mb)
                .unwrap_or(u32::MAX),
            worldgen_feature_cache_mb: u32::try_from(bamboo.worldgen_cache.feature_mb)
                .unwrap_or(u32::MAX),
            worldgen_cache_idle_seconds: u32::try_from(bamboo.worldgen_cache.idle_seconds)
                .unwrap_or(u32::MAX),
            deliveries_per_tick: u32::try_from(bamboo.deliveries_per_tick).unwrap_or(u32::MAX),
            reclaims_per_tick: u32::try_from(bamboo.reclaims_per_tick).unwrap_or(u32::MAX),
            island_threads: u32::try_from(bamboo.island_threads).unwrap_or(u32::MAX),
            islands: bamboo.islands,
            level_seed: official.level_seed.clone(),
            // 旧名 `default` / `largebiomes` 和大小写都按 `LevelType` 归一；
            // 认不出的值服务端本来也会回退 `normal`，界面直接显示成回退后的值。
            // Legacy names and casing normalize through `LevelType`; an unknown
            // value already falls back to `normal` on the server, so show that.
            level_type: LevelType::from_level_id(&official.level_type)
                .unwrap_or_default()
                .into(),
            generate_structures: official.generate_structures,
            generator_settings: official.generator_settings.clone(),
            log_level: bamboo.log_level.clone(),
            log_filter: bamboo.log_filter.clone(),
        }
    }

    /// 按官方 `DedicatedServerProperties` 把四个界面字段解析成生成参数。
    ///
    /// Parse the four UI fields into generation parameters the way vanilla's
    /// `DedicatedServerProperties` does.
    fn world_gen(&self) -> WorldGenConfig {
        WorldGenConfig::from_properties(
            &self.level_seed,
            LevelType::from(self.level_type).id(),
            &self.generator_settings,
            self.generate_structures,
        )
        .0
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
            // 世界目录 = 服务器目录 + level-name，和命令行版一致。
            // World dir = server dir + level-name, matching the CLI.
            world_dir: Some(PathBuf::from(&self.server_directory).join(self.level_name.trim())),
            entity_threads: self.entity_threads as usize,
            max_entity_collisions: self.max_entity_collisions as usize,
            mob_throttle_distance: self.mob_throttle_distance,
            mob_sleep_distance: self.mob_sleep_distance,
            mob_wakeups_scale: self.mob_wakeups_scale as usize,
            mob_pathfinding_phase: self.mob_pathfinding_phase,
            mob_pathfinding_budget: self.mob_pathfinding_budget,
            mob_pathfinding_fail_cooldown: self.mob_pathfinding_fail_cooldown,
            villager_lobotomize: self.villager_lobotomize,
            villager_lobotomize_interval: self.villager_lobotomize_interval,
            villager_lobotomize_wait_trade_lock: self.villager_lobotomize_wait_trade_lock,
            villager_sleep_brain_interval: self.villager_sleep_brain_interval,
            redstone_cache: self.redstone_cache,
            chunk_worker_threads: self.chunk_worker_threads as usize,
            worldgen_cache: WorldgenCacheSettings {
                terrain_mb: self.worldgen_terrain_cache_mb as usize,
                feature_mb: self.worldgen_feature_cache_mb as usize,
                idle_seconds: u64::from(self.worldgen_cache_idle_seconds),
            },
            // 桌面界面不暴露这一项，按官方/缺省开。
            // The desktop UI does not expose this knob; keep the default (on).
            spawn_per_player_cap: defaults.spawn_per_player_cap,
            mail_budget: mc_bamboo_game::MailBudget {
                deliveries: self.deliveries_per_tick as usize,
                reclaims: self.reclaims_per_tick as usize,
            },
            // 和命令行版一样，环境变量 `BAMBOO_ISLAND_THREADS` 设了就盖过设置（不写回文件）。
            // Like the CLI, `BAMBOO_ISLAND_THREADS` overrides the setting (never saved).
            island_threads: mc_bamboo_game::island_threads_from_env()
                .unwrap_or((self.island_threads as usize).min(mc_bamboo_game::MAX_ISLAND_THREADS)),
            // 和命令行版一样，环境变量 `BAMBOO_ISLANDS` 设了就盖过设置（不写回文件）。
            // Like the CLI, `BAMBOO_ISLANDS` overrides the setting (never saved).
            islands: mc_bamboo_game::islands_from_env().unwrap_or(self.islands),
            // 判死时限不在界面上：用缺省（环境变量 `BAMBOO_ISLAND_DEAD_SECONDS` 优先）。
            // The dead-island timeout is not in the UI: the default (the
            // `BAMBOO_ISLAND_DEAD_SECONDS` environment variable wins).
            island_dead_timeout: defaults.island_dead_timeout,
            world_gen: self.world_gen(),
            spawn_radius: None,
            // 桌面端有 GUI 控制台，不读进程 stdin；广播默认按官方 true。
            // The desktop has a GUI console, so it does not read process stdin;
            // the broadcast flag keeps the official default (true).
            broadcast_console_to_ops: true,
            console_enabled: false,
        }
    }

    /// 把界面上的值写回两份文件内容，保留已有的未实现官方键。
    fn as_files(&self, existing: &ConfigFiles) -> ConfigFiles {
        let mut files = ConfigFiles::from_server_config(&self.to_server_config(), existing);
        // 界面上没有的判死时限照文件里原来的写回，不被缺省值盖掉。
        // The dead-island timeout is not in the UI; keep the file's value.
        files.bamboo.island_dead_timeout = existing.bamboo.island_dead_timeout;
        files.official.server_ip = if self.host.trim() == "0.0.0.0" {
            String::new()
        } else {
            self.host.trim().to_owned()
        };
        files.official.server_port = self.port;
        files.official.level_name = self.level_name.trim().to_owned();
        // 四个世界生成键按官方 `Properties.store` 的写法写回原文（值的转义由那边负责）。
        // `level-type` 归一成注册名；种子和 JSON 前后空白在官方解析时本来就会忽略。
        //
        // Write the four world-generation keys back verbatim, escaped by the
        // official `Properties.store` rules. `level-type` normalizes to its
        // registry id; leading/trailing space is ignored by vanilla's parsers.
        files.official.level_seed = self.level_seed.trim().to_owned();
        files.official.level_type = LevelType::from(self.level_type).id().to_owned();
        files.official.generator_settings = self.generator_settings.trim().to_owned();
        files.official.generate_structures = self.generate_structures;
        // 日志两项不在 ServerConfig 里，from_server_config 会保留文件里的旧值，
        // 所以这里用界面草稿的值显式盖一遍。
        // The two log keys are not in ServerConfig; from_server_config keeps the
        // file's values, so overwrite them with the UI draft explicitly.
        files.bamboo.log_level = self.log_level.clone();
        files.bamboo.log_filter = self.log_filter.clone();
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
        // 官方只对 flat 预设解析 `generator-settings`，坏 JSON 会安静地退回默认预设；
        // 界面这边说清楚，免得用户以为自己填的 preset 生效了。
        // Vanilla parses `generator-settings` only for the flat preset and falls
        // back silently on bad JSON; the UI says so instead of dropping the input.
        if self.level_type == WorldType::Flat && !self.generator_settings.trim().is_empty() {
            FlatSettings::parse(&self.generator_settings)
                .map_err(|error| format!("超平坦设置不是合法的 JSON：{error}"))?;
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
        if self.max_entity_collisions as usize > MAX_ENTITY_COLLISIONS {
            return Err(format!("挤压上限不能超过 {MAX_ENTITY_COLLISIONS}"));
        }
        if !(0..=MAX_MOB_DISTANCE as i32).contains(&self.mob_throttle_distance) {
            return Err(format!("降频距离必须在 0 到 {MAX_MOB_DISTANCE} 格之间"));
        }
        if !(0..=MAX_MOB_DISTANCE as i32).contains(&self.mob_sleep_distance) {
            return Err(format!("休眠距离必须在 0 到 {MAX_MOB_DISTANCE} 格之间"));
        }
        if self.mob_wakeups_scale as usize > MAX_MOB_WAKEUPS_SCALE {
            return Err(format!("唤醒倍率不能超过 {MAX_MOB_WAKEUPS_SCALE}"));
        }
        if self.mob_pathfinding_budget > MAX_PATHFINDING_BUDGET {
            return Err(format!("寻路节点预算不能超过 {MAX_PATHFINDING_BUDGET}"));
        }
        if !(1..=MAX_VILLAGER_BRAIN_INTERVAL).contains(&self.villager_lobotomize_interval) {
            return Err(format!(
                "被困村民大脑间隔必须在 1 到 {MAX_VILLAGER_BRAIN_INTERVAL} 刻之间"
            ));
        }
        if !(0..=MAX_VILLAGER_BRAIN_INTERVAL).contains(&self.villager_sleep_brain_interval) {
            return Err(format!(
                "睡觉村民大脑间隔必须在 0 到 {MAX_VILLAGER_BRAIN_INTERVAL} 刻之间"
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
        if self.worldgen_terrain_cache_mb as usize > MAX_TERRAIN_CACHE_MB {
            return Err(format!("地形缓存上限不能超过 {MAX_TERRAIN_CACHE_MB} MiB"));
        }
        if self.worldgen_feature_cache_mb as usize > MAX_FEATURE_CACHE_MB {
            return Err(format!("地物缓存上限不能超过 {MAX_FEATURE_CACHE_MB} MiB"));
        }
        if u64::from(self.worldgen_cache_idle_seconds) > MAX_IDLE_SECONDS {
            return Err(format!("缓存空闲释放秒数不能超过 {MAX_IDLE_SECONDS}"));
        }
        // 服务端那边会把正版验证悄悄关掉，与其让用户以为两个都开着，不如在这里说清楚。
        if self.online_mode && self.forwarding != Forwarding::Off {
            return Err("正版验证和代理转发不能同时打开：代理已经替服务端验过身份了".to_owned());
        }
        if !matches!(
            self.log_level.as_str(),
            "error" | "warn" | "info" | "debug" | "trace"
        ) {
            return Err("日志级别只能是 error / warn / info / debug / trace".to_owned());
        }
        Ok(())
    }
}

/// 一键性能模式除实体线程数外的固定推荐值，集中成一张表。
///
/// 实体线程数按这台机器算，不在这张表里。寻路阶段的三项推荐打开阶段和失败
/// 冷却，节点预算用 0（服务端按 65536 算）。
///
/// The fixed one-click performance values apart from the machine-dependent
/// entity thread count. The pathfinding recommendations turn on the phase and
/// the failure cooldown and leave the node budget at 0 (65536 on the server).
const PRESET_MAX_ENTITY_COLLISIONS: u32 = 8;
const PRESET_MOB_THROTTLE_DISTANCE: i32 = 32;
const PRESET_MOB_SLEEP_DISTANCE: i32 = 64;
const PRESET_MOB_WAKEUPS_SCALE: u32 = 1;
const PRESET_MOB_PATHFINDING_PHASE: bool = true;
const PRESET_MOB_PATHFINDING_BUDGET: u32 = 0;
const PRESET_MOB_PATHFINDING_FAIL_COOLDOWN: bool = true;
const PRESET_VILLAGER_LOBOTOMIZE: bool = true;
const PRESET_VILLAGER_LOBOTOMIZE_INTERVAL: i32 = 100;
const PRESET_VILLAGER_LOBOTOMIZE_WAIT_TRADE_LOCK: bool = true;
const PRESET_VILLAGER_SLEEP_BRAIN_INTERVAL: i32 = 5;

/// 一键性能模式写进 `bamboo.toml` 的一组值。
///
/// 集中定义在这里：前端通过 `performance_presets` 命令取，不自己写死数字。
/// 推荐值和恢复原版精确模式的两套都在 [`performance_presets`] 里给出。
///
/// One set of values the one-click performance mode writes into `bamboo.toml`.
/// This is the single definition the UI reads through the
/// `performance_presets` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformancePreset {
    pub entity_threads: u32,
    pub max_entity_collisions: u32,
    pub mob_throttle_distance: i32,
    pub mob_sleep_distance: i32,
    pub mob_wakeups_scale: u32,
    pub mob_pathfinding_phase: bool,
    pub mob_pathfinding_budget: u32,
    pub mob_pathfinding_fail_cooldown: bool,
    pub villager_lobotomize: bool,
    pub villager_lobotomize_interval: i32,
    pub villager_lobotomize_wait_trade_lock: bool,
    pub villager_sleep_brain_interval: i32,
}

/// 一键预设写进设置的一项动作，只碰这十二个字段。
///
/// The single write action of a one-click preset; it touches only the twelve
/// knobs.
impl PerformancePreset {
    /// 只改这十二项，其它字段原样保留。
    ///
    /// Change only the twelve knobs and leave every other field untouched.
    fn apply_to(self, settings: &mut ServerSettings) {
        settings.entity_threads = self.entity_threads;
        settings.max_entity_collisions = self.max_entity_collisions;
        settings.mob_throttle_distance = self.mob_throttle_distance;
        settings.mob_sleep_distance = self.mob_sleep_distance;
        settings.mob_wakeups_scale = self.mob_wakeups_scale;
        settings.mob_pathfinding_phase = self.mob_pathfinding_phase;
        settings.mob_pathfinding_budget = self.mob_pathfinding_budget;
        settings.mob_pathfinding_fail_cooldown = self.mob_pathfinding_fail_cooldown;
        settings.villager_lobotomize = self.villager_lobotomize;
        settings.villager_lobotomize_interval = self.villager_lobotomize_interval;
        settings.villager_lobotomize_wait_trade_lock = self.villager_lobotomize_wait_trade_lock;
        settings.villager_sleep_brain_interval = self.villager_sleep_brain_interval;
    }
}

/// 以上次保存的设置为基准套用一键预设。
///
/// 界面上还没保存的修改不参与合并：结果里除了这八项，其它字段全部来自 `saved`。
/// 前端拿到结果后照常走 `save` 写文件，再把草稿里的这八项同步成新值。
///
/// Apply a one-click preset on top of the last saved settings. Unsaved edits
/// in the UI never take part: apart from the eight knobs every field comes from
/// `saved`. The frontend then writes the result through the normal `save` path
/// and syncs those eight values into its draft.
pub fn merge_performance_preset(
    saved: &ServerSettings,
    preset: PerformancePreset,
) -> ServerSettings {
    let mut merged = saved.clone();
    preset.apply_to(&mut merged);
    merged
}

/// 两张预设：推荐开启的值，和恢复原版精确模式的值。
///
/// 两套一起发给前端，避免前端各自写一份 0 / 1。
///
/// Both presets: the recommended performance values and the vanilla exact
/// mode values. Sending them together keeps the frontend from hard-coding
/// the zeros and ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformancePresets {
    pub recommended: PerformancePreset,
    pub exact: PerformancePreset,
}

/// 当前机器上的一键性能模式推荐值。
///
/// `entity_threads` 取本机逻辑核数，并夹在服务端的 `MAX_ENTITY_THREADS` 以内；
/// 取不到核数时按 1 条算（0 是精确模式，不能当推荐值）。其余几项取第 4 步压测
/// 里收益最大的档位：挤压上限 8、降频 32 格、休眠 64 格、唤醒倍率 1，并打开
/// 独立寻路阶段和失败冷却（节点预算 0 = 65536）。
///
/// The recommended one-click performance values for this machine. The entity
/// thread count comes from the logical core count, clamped to the server's
/// `MAX_ENTITY_THREADS` and never 0 (0 is exact mode). The mob knobs use the
/// best-measured step-4 tiers: 8 collisions, throttle at 32, sleep at 64 and
/// wake-up scale 1, plus the pathfinding phase and failure cooldown on (budget
/// 0 = 65536).
pub fn performance_presets() -> PerformancePresets {
    let entity_threads = std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
        .clamp(1, MAX_ENTITY_THREADS as u32);
    PerformancePresets {
        recommended: PerformancePreset {
            entity_threads,
            max_entity_collisions: PRESET_MAX_ENTITY_COLLISIONS,
            mob_throttle_distance: PRESET_MOB_THROTTLE_DISTANCE,
            mob_sleep_distance: PRESET_MOB_SLEEP_DISTANCE,
            mob_wakeups_scale: PRESET_MOB_WAKEUPS_SCALE,
            mob_pathfinding_phase: PRESET_MOB_PATHFINDING_PHASE,
            mob_pathfinding_budget: PRESET_MOB_PATHFINDING_BUDGET,
            mob_pathfinding_fail_cooldown: PRESET_MOB_PATHFINDING_FAIL_COOLDOWN,
            villager_lobotomize: PRESET_VILLAGER_LOBOTOMIZE,
            villager_lobotomize_interval: PRESET_VILLAGER_LOBOTOMIZE_INTERVAL,
            villager_lobotomize_wait_trade_lock: PRESET_VILLAGER_LOBOTOMIZE_WAIT_TRADE_LOCK,
            villager_sleep_brain_interval: PRESET_VILLAGER_SLEEP_BRAIN_INTERVAL,
        },
        // 恢复原版：各项全部关闭，唤醒倍率回到 Paper 的缺省 1。
        // Vanilla: every feature off, wake-up scale back to Paper's 1.
        exact: PerformancePreset {
            entity_threads: 0,
            max_entity_collisions: 0,
            mob_throttle_distance: 0,
            mob_sleep_distance: 0,
            mob_wakeups_scale: 1,
            mob_pathfinding_phase: false,
            mob_pathfinding_budget: 0,
            mob_pathfinding_fail_cooldown: false,
            villager_lobotomize: false,
            villager_lobotomize_interval: 100,
            villager_lobotomize_wait_trade_lock: true,
            villager_sleep_brain_interval: 0,
        },
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
        assert_eq!(defaults.max_entity_collisions, 0);
        assert_eq!(defaults.mob_throttle_distance, 0);
        assert_eq!(defaults.mob_sleep_distance, 0);
        assert_eq!(defaults.mob_wakeups_scale, 1);
        assert!(!defaults.mob_pathfinding_phase);
        assert_eq!(defaults.mob_pathfinding_budget, 0);
        assert!(!defaults.mob_pathfinding_fail_cooldown);
        assert!(!defaults.villager_lobotomize);
        assert_eq!(defaults.villager_lobotomize_interval, 100);
        assert!(defaults.villager_lobotomize_wait_trade_lock);
        assert_eq!(defaults.villager_sleep_brain_interval, 0);
        assert_eq!(defaults.deliveries_per_tick, 64);
        assert_eq!(defaults.reclaims_per_tick, 64);
        assert_eq!(defaults.port, 25565);
        assert_eq!(defaults.level_name, "world");
        assert_eq!(defaults.level_seed, "");
        assert_eq!(defaults.level_type, WorldType::Normal);
        assert!(defaults.generate_structures);
        assert_eq!(defaults.generator_settings, "{}");
        assert_eq!(defaults.log_level, "info");
        assert!(defaults.log_filter.is_empty());
        assert_eq!(ServerSettings::default().validate(), Ok(()));
    }

    /// 日志两项写进 bamboo.toml、读回来；缺字段的旧草稿取 info，非法级别被挡住。
    ///
    /// The log keys round-trip through bamboo.toml; a draft missing them gets
    /// info and an invalid level is rejected.
    #[test]
    fn log_settings_round_trip_and_old_drafts_default_to_info() {
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        let mut settings =
            ServerSettings::from_files(&ConfigFiles::default(), &directory.display().to_string());
        settings.log_level = "trace".to_owned();
        settings.log_filter = "mc_bamboo_game::world=debug".to_owned();
        assert_eq!(settings.validate(), Ok(()));
        write(&directory, &settings.as_files(&ConfigFiles::default())).unwrap();
        let loaded = ServerSettings::from_files(
            &read(&directory).unwrap(),
            &directory.display().to_string(),
        );
        assert_eq!(loaded.log_level, "trace");
        assert_eq!(loaded.log_filter, "mc_bamboo_game::world=debug");
        let bamboo = fs::read_to_string(directory.join(BAMBOO_FILE)).unwrap();
        assert!(bamboo.contains("log_level = \"trace\"\n"), "{bamboo}");
        // 没有 logLevel 字段的旧 JSON 取 info。
        // An old JSON without logLevel defaults to info.
        let parsed: ServerSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.log_level, "info");
        assert!(parsed.log_filter.is_empty());
        // 非法级别在界面就被挡住。
        // An invalid level is rejected by the UI.
        let mut bad = ServerSettings::default();
        bad.log_level = "verbose".to_owned();
        assert!(bad.validate().is_err());
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
            max_entity_collisions: 8,
            mob_throttle_distance: 32,
            mob_sleep_distance: 64,
            mob_wakeups_scale: 2,
            mob_pathfinding_phase: true,
            mob_pathfinding_budget: 8192,
            mob_pathfinding_fail_cooldown: true,
            villager_lobotomize: true,
            villager_lobotomize_interval: 120,
            villager_lobotomize_wait_trade_lock: false,
            villager_sleep_brain_interval: 5,
            redstone_cache: true,
            chunk_worker_threads: 3,
            worldgen_terrain_cache_mb: 48,
            worldgen_feature_cache_mb: 12,
            worldgen_cache_idle_seconds: 45,
            deliveries_per_tick: 16,
            reclaims_per_tick: 8,
            island_threads: 2,
            islands: true,
            level_seed: "123456789".to_owned(),
            level_type: WorldType::Amplified,
            generate_structures: false,
            generator_settings: r#"{"layers":[{"block":"minecraft:stone","height":3}]}"#.to_owned(),
            log_level: "debug".to_owned(),
            log_filter: "mc_bamboo_game=trace".to_owned(),
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
        assert_eq!(config.max_entity_collisions, 8);
        assert_eq!(config.mob_throttle_distance, 32);
        assert_eq!(config.mob_sleep_distance, 64);
        assert_eq!(config.mob_wakeups_scale, 2);
        assert!(config.mob_pathfinding_phase);
        assert_eq!(config.mob_pathfinding_budget, 8192);
        assert!(config.mob_pathfinding_fail_cooldown);
        assert!(config.villager_lobotomize);
        assert_eq!(config.villager_lobotomize_interval, 120);
        assert!(!config.villager_lobotomize_wait_trade_lock);
        assert_eq!(config.villager_sleep_brain_interval, 5);
        assert!(config.redstone_cache);
        assert_eq!(config.chunk_worker_threads, 3);
        assert_eq!(config.worldgen_cache.terrain_mb, 48);
        assert_eq!(config.worldgen_cache.feature_mb, 12);
        assert_eq!(config.worldgen_cache.idle_seconds, 45);
        assert_eq!(config.mail_budget.deliveries, 16);
        assert_eq!(config.mail_budget.reclaims, 8);
        // 环境变量 `BAMBOO_ISLAND_THREADS` 设了就盖过设置。
        // `BAMBOO_ISLAND_THREADS` overrides the setting when set.
        assert_eq!(
            config.island_threads,
            mc_bamboo_game::island_threads_from_env().unwrap_or(2)
        );
        assert_eq!(
            config.islands,
            mc_bamboo_game::islands_from_env().unwrap_or(true)
        );
        assert_eq!(config.list_directory, PathBuf::from("/tmp/server"));
        assert_eq!(config.world_gen.seed, 123_456_789);
        assert_eq!(config.world_gen.level_type, LevelType::Amplified);
        assert!(!config.world_gen.generate_structures);
        assert_eq!(config.world_gen.flat.layers.len(), 1);
        assert_eq!(config.world_gen.flat.layers[0].1, 3);
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
        settings.max_entity_collisions = 8;
        settings.mob_throttle_distance = 32;
        settings.mob_sleep_distance = 64;
        settings.mob_wakeups_scale = 3;
        settings.mob_pathfinding_phase = true;
        settings.mob_pathfinding_budget = 8192;
        settings.mob_pathfinding_fail_cooldown = true;
        settings.deliveries_per_tick = 32;
        settings.keep_alive_seconds = 20;
        settings.level_seed = "my seed".to_owned();
        settings.level_type = WorldType::Flat;
        settings.generate_structures = false;
        settings.generator_settings =
            r#"{"layers":[{"block":"minecraft:stone","height":3}],"biome":"minecraft:desert"}"#
                .to_owned();
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
        assert_eq!(loaded.max_entity_collisions, 8);
        assert_eq!(loaded.mob_throttle_distance, 32);
        assert_eq!(loaded.mob_sleep_distance, 64);
        assert_eq!(loaded.mob_wakeups_scale, 3);
        assert!(loaded.mob_pathfinding_phase);
        assert_eq!(loaded.mob_pathfinding_budget, 8192);
        assert!(loaded.mob_pathfinding_fail_cooldown);
        assert_eq!(loaded.deliveries_per_tick, 32);
        assert_eq!(loaded.keep_alive_seconds, 20);
        assert_eq!(loaded.level_seed, "my seed");
        assert_eq!(loaded.level_type, WorldType::Flat);
        assert!(!loaded.generate_structures);
        assert_eq!(loaded.generator_settings, settings.generator_settings);
        let properties = fs::read_to_string(directory.join(PROPERTIES_FILE)).unwrap();
        assert!(properties.contains("server-port=25600\n"));
        assert!(properties.contains("motd=my server\n"));
        assert!(properties.contains("white-list=true\n"));
        assert!(properties.contains("level-seed=my seed\n"));
        assert!(properties.contains("level-type=minecraft\\:flat\n"));
        assert!(properties.contains("generate-structures=false\n"));
        // `:` 按官方 `Properties.store` 转义成 `\:`，读回来再还原。
        // `:` is escaped to `\:` like `Properties.store`; reading restores it.
        assert!(
            properties.contains(
                r#"generator-settings={"layers"\:[{"block"\:"minecraft\:stone","height"\:3}],"biome"\:"minecraft\:desert"}"#
            ),
            "{properties}"
        );
        let bamboo = fs::read_to_string(directory.join(BAMBOO_FILE)).unwrap();
        assert!(bamboo.contains("entity_threads = 2\n"));
        assert!(bamboo.contains("max_entity_collisions = 8\n"));
        assert!(bamboo.contains("mob_throttle_distance = 32\n"));
        assert!(bamboo.contains("mob_sleep_distance = 64\n"));
        assert!(bamboo.contains("mob_wakeups_scale = 3\n"));
        assert!(bamboo.contains("mob_pathfinding_phase = true\n"));
        assert!(bamboo.contains("mob_pathfinding_budget = 8192\n"));
        assert!(bamboo.contains("mob_pathfinding_fail_cooldown = true\n"));
    }

    /// 世界生成四项在界面 JSON 里的拼写是固定的 camelCase，读写要能原样往返。
    /// The four world-gen fields keep fixed camelCase spellings in the UI JSON and
    /// must survive a round trip unchanged.
    #[test]
    fn world_gen_fields_round_trip_through_the_ui_json() {
        let settings = ServerSettings {
            level_seed: "hello".to_owned(),
            level_type: WorldType::LargeBiomes,
            generate_structures: false,
            generator_settings: r#"{"biome":"minecraft:desert"}"#.to_owned(),
            ..ServerSettings::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        assert!(text.contains(r#""levelSeed":"hello""#), "{text}");
        assert!(text.contains(r#""levelType":"largeBiomes""#), "{text}");
        assert!(text.contains(r#""generateStructures":false"#), "{text}");
        assert!(
            text.contains(r#""generatorSettings":"{\"biome\":\"minecraft:desert\"}""#),
            "{text}"
        );
        let parsed: ServerSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.level_seed, "hello");
        assert_eq!(parsed.level_type, WorldType::LargeBiomes);
        assert!(!parsed.generate_structures);
        assert_eq!(parsed.generator_settings, r#"{"biome":"minecraft:desert"}"#);
    }

    #[test]
    fn world_type_maps_one_to_one_to_the_level_type() {
        assert_eq!(LevelType::from(WorldType::Normal), LevelType::Normal);
        assert_eq!(LevelType::from(WorldType::Flat), LevelType::Flat);
        assert_eq!(
            LevelType::from(WorldType::LargeBiomes),
            LevelType::LargeBiomes
        );
        assert_eq!(LevelType::from(WorldType::Amplified), LevelType::Amplified);
        assert_eq!(WorldType::from(LevelType::Flat), WorldType::Flat);
    }

    /// 旧名 `default` 读进界面时归一成 `normal`，保存写回注册名。
    /// The legacy `default` alias shows up as `normal` and is saved as the id.
    #[test]
    fn legacy_level_type_aliases_load_as_the_canonical_preset() {
        let mut files = ConfigFiles::default();
        files.official.level_type = "default".to_owned();
        let settings = ServerSettings::from_files(&files, "");
        assert_eq!(settings.level_type, WorldType::Normal);
        assert_eq!(
            settings.as_files(&files).official.level_type,
            "minecraft:normal"
        );
    }

    #[test]
    fn a_bad_flat_preset_is_rejected_before_saving() {
        assert!(
            validate(|settings| {
                settings.level_type = WorldType::Flat;
                settings.generator_settings = "[".to_owned();
            })
            .is_err()
        );
        assert!(
            validate(|settings| {
                settings.level_type = WorldType::Flat;
                settings.generator_settings =
                    r#"{"layers":[{"block":"minecraft:nope","height":1}]}"#.to_owned();
            })
            .is_err()
        );
        assert_eq!(
            validate(|settings| {
                settings.level_type = WorldType::Flat;
                settings.generator_settings = String::new();
            }),
            Ok(())
        );
        // 非 flat 预设下官方根本不看这个键，坏 JSON 也不该拦住保存。
        // Outside the flat preset vanilla never reads the key, so bad JSON must
        // not block saving either.
        assert_eq!(
            validate(|settings| {
                settings.level_type = WorldType::Normal;
                settings.generator_settings = "[".to_owned();
            }),
            Ok(())
        );
    }

    /// 空种子保存成空的 `level-seed`，交给服务端在建世界时取随机值。
    /// An empty seed is saved as an empty `level-seed`; the server rolls one when
    /// it creates the world.
    #[test]
    fn an_empty_seed_is_written_as_an_empty_value() {
        let settings = ServerSettings {
            level_seed: "   ".to_owned(),
            ..ServerSettings::default()
        };
        let files = settings.as_files(&ConfigFiles::default());
        assert_eq!(files.official.level_seed, "");
        assert_ne!(settings.to_server_config().world_gen.seed, 0);
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
        // 四个世界生成键和 broadcast-console-to-ops 现在是已实现项，剩下的未实现键是 54 个。
        // The four world-gen keys and broadcast-console-to-ops are now implemented;
        // 54 official keys remain unimplemented.
        assert_eq!(files.unsupported_keys().len(), 54);
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
        assert!(validate(|settings| settings.max_entity_collisions = 0).is_ok());
        assert!(
            validate(|settings| settings.max_entity_collisions = MAX_ENTITY_COLLISIONS as u32)
                .is_ok()
        );
        assert!(
            validate(|settings| settings.max_entity_collisions = MAX_ENTITY_COLLISIONS as u32 + 1)
                .is_err()
        );
        assert!(validate(|settings| settings.mob_throttle_distance = -1).is_err());
        assert!(validate(|settings| settings.mob_throttle_distance = 0).is_ok());
        assert!(
            validate(|settings| settings.mob_throttle_distance = MAX_MOB_DISTANCE as i32).is_ok()
        );
        assert!(
            validate(|settings| settings.mob_throttle_distance = MAX_MOB_DISTANCE as i32 + 1)
                .is_err()
        );
        // 休眠档和降频档用同一组边界。
        assert!(validate(|settings| settings.mob_sleep_distance = -1).is_err());
        assert!(validate(|settings| settings.mob_sleep_distance = MAX_MOB_DISTANCE as i32).is_ok());
        assert!(
            validate(|settings| settings.mob_sleep_distance = MAX_MOB_DISTANCE as i32 + 1).is_err()
        );
        assert!(validate(|settings| settings.mob_wakeups_scale = 0).is_ok());
        assert!(
            validate(|settings| settings.mob_wakeups_scale = MAX_MOB_WAKEUPS_SCALE as u32).is_ok()
        );
        assert!(
            validate(|settings| settings.mob_wakeups_scale = MAX_MOB_WAKEUPS_SCALE as u32 + 1)
                .is_err()
        );
        assert!(validate(|settings| settings.mob_pathfinding_budget = 0).is_ok());
        assert!(
            validate(|settings| settings.mob_pathfinding_budget = MAX_PATHFINDING_BUDGET).is_ok()
        );
        assert!(
            validate(|settings| settings.mob_pathfinding_budget = MAX_PATHFINDING_BUDGET + 1)
                .is_err()
        );
        assert!(validate(|settings| settings.deliveries_per_tick = 0).is_ok());
        assert!(validate(|settings| settings.deliveries_per_tick = u32::MAX).is_err());
        assert!(validate(|settings| settings.reclaims_per_tick = u32::MAX).is_err());
        assert!(validate(|settings| settings.worldgen_terrain_cache_mb = 0).is_ok());
        assert!(validate(|settings| settings.worldgen_terrain_cache_mb = u32::MAX).is_err());
        assert!(validate(|settings| settings.worldgen_feature_cache_mb = u32::MAX).is_err());
        assert!(validate(|settings| settings.worldgen_cache_idle_seconds = 0).is_ok());
        assert!(validate(|settings| settings.worldgen_cache_idle_seconds = u32::MAX).is_err());
    }

    /// 把一份设置写进临时服务器目录，返回两份文件的原文。
    ///
    /// Write a settings draft into a temporary server directory and return both
    /// files' text.
    fn write_settings(settings: &ServerSettings) -> (String, String) {
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        write(&directory, &settings.as_files(&ConfigFiles::default())).unwrap();
        (
            fs::read_to_string(directory.join(BAMBOO_FILE)).unwrap(),
            fs::read_to_string(directory.join(PROPERTIES_FILE)).unwrap(),
        )
    }

    #[test]
    fn the_one_click_preset_writes_the_recommended_values() {
        let presets = performance_presets();
        let recommended = presets.recommended;
        // 实体线程数取本机逻辑核数，夹在服务端上限以内，且不能是 0（0 是精确模式）。
        let cores = std::thread::available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or(1);
        assert_eq!(
            recommended.entity_threads,
            cores.min(MAX_ENTITY_THREADS as u32)
        );
        assert!(recommended.entity_threads >= 1);
        assert_eq!(recommended.max_entity_collisions, 8);
        assert_eq!(recommended.mob_throttle_distance, 32);
        assert_eq!(recommended.mob_sleep_distance, 64);
        assert_eq!(recommended.mob_wakeups_scale, 1);
        assert!(recommended.mob_pathfinding_phase);
        assert_eq!(recommended.mob_pathfinding_budget, 0);
        assert!(recommended.mob_pathfinding_fail_cooldown);
        assert!(recommended.villager_lobotomize);
        assert_eq!(recommended.villager_lobotomize_interval, 100);
        assert!(recommended.villager_lobotomize_wait_trade_lock);
        assert_eq!(recommended.villager_sleep_brain_interval, 5);

        let settings = merge_performance_preset(&ServerSettings::default(), recommended);
        assert_eq!(settings.validate(), Ok(()));
        let (bamboo, _) = write_settings(&settings);
        assert!(
            bamboo.contains(&format!(
                "entity_threads = {}\n",
                recommended.entity_threads
            )),
            "{bamboo}"
        );
        assert!(bamboo.contains("max_entity_collisions = 8\n"), "{bamboo}");
        assert!(bamboo.contains("mob_throttle_distance = 32\n"), "{bamboo}");
        assert!(bamboo.contains("mob_sleep_distance = 64\n"), "{bamboo}");
        assert!(bamboo.contains("mob_wakeups_scale = 1\n"), "{bamboo}");
        assert!(
            bamboo.contains("mob_pathfinding_phase = true\n"),
            "{bamboo}"
        );
        assert!(bamboo.contains("mob_pathfinding_budget = 0\n"), "{bamboo}");
        assert!(
            bamboo.contains("mob_pathfinding_fail_cooldown = true\n"),
            "{bamboo}"
        );
        assert!(bamboo.contains("villager_lobotomize = true\n"), "{bamboo}");
        assert!(
            bamboo.contains("villager_lobotomize_interval = 100\n"),
            "{bamboo}"
        );
        assert!(
            bamboo.contains("villager_lobotomize_wait_trade_lock = true\n"),
            "{bamboo}"
        );
        assert!(
            bamboo.contains("villager_sleep_brain_interval = 5\n"),
            "{bamboo}"
        );

        // 读回来要和写进去的一致，界面刷新时看到的就是这些值。
        let temp = Temp::new();
        let directory = temp.0.join("server");
        fs::create_dir_all(&directory).unwrap();
        write(&directory, &settings.as_files(&ConfigFiles::default())).unwrap();
        let loaded = ServerSettings::from_files(
            &read(&directory).unwrap(),
            &directory.display().to_string(),
        );
        assert_eq!(loaded.entity_threads, recommended.entity_threads);
        assert_eq!(loaded.max_entity_collisions, 8);
        assert_eq!(loaded.mob_throttle_distance, 32);
        assert_eq!(loaded.mob_sleep_distance, 64);
        assert_eq!(loaded.mob_wakeups_scale, 1);
        assert!(loaded.mob_pathfinding_phase);
        assert_eq!(loaded.mob_pathfinding_budget, 0);
        assert!(loaded.mob_pathfinding_fail_cooldown);
        assert!(loaded.villager_lobotomize);
        assert_eq!(loaded.villager_lobotomize_interval, 100);
        assert!(loaded.villager_lobotomize_wait_trade_lock);
        assert_eq!(loaded.villager_sleep_brain_interval, 5);
    }

    #[test]
    fn the_exact_preset_restores_the_vanilla_values() {
        let presets = performance_presets();
        let recommended = merge_performance_preset(&ServerSettings::default(), presets.recommended);
        let settings = merge_performance_preset(&recommended, presets.exact);
        assert_eq!(settings.entity_threads, 0);
        assert_eq!(settings.max_entity_collisions, 0);
        assert_eq!(settings.mob_throttle_distance, 0);
        assert_eq!(settings.mob_sleep_distance, 0);
        assert_eq!(settings.mob_wakeups_scale, 1);
        assert!(!settings.mob_pathfinding_phase);
        assert_eq!(settings.mob_pathfinding_budget, 0);
        assert!(!settings.mob_pathfinding_fail_cooldown);
        assert!(!settings.villager_lobotomize);
        assert_eq!(settings.villager_lobotomize_interval, 100);
        assert!(settings.villager_lobotomize_wait_trade_lock);
        assert_eq!(settings.villager_sleep_brain_interval, 0);
        assert_eq!(settings.validate(), Ok(()));

        let (bamboo, _) = write_settings(&settings);
        assert!(bamboo.contains("entity_threads = 0\n"), "{bamboo}");
        assert!(bamboo.contains("max_entity_collisions = 0\n"), "{bamboo}");
        assert!(bamboo.contains("mob_throttle_distance = 0\n"), "{bamboo}");
        assert!(bamboo.contains("mob_sleep_distance = 0\n"), "{bamboo}");
        assert!(bamboo.contains("mob_wakeups_scale = 1\n"), "{bamboo}");
        assert!(
            bamboo.contains("mob_pathfinding_phase = false\n"),
            "{bamboo}"
        );
        assert!(bamboo.contains("mob_pathfinding_budget = 0\n"), "{bamboo}");
        assert!(
            bamboo.contains("mob_pathfinding_fail_cooldown = false\n"),
            "{bamboo}"
        );
        assert!(bamboo.contains("villager_lobotomize = false\n"), "{bamboo}");
        assert!(
            bamboo.contains("villager_lobotomize_interval = 100\n"),
            "{bamboo}"
        );
        assert!(
            bamboo.contains("villager_lobotomize_wait_trade_lock = true\n"),
            "{bamboo}"
        );
        assert!(
            bamboo.contains("villager_sleep_brain_interval = 0\n"),
            "{bamboo}"
        );
    }

    /// 一键预设以磁盘上已保存的设置为基准：界面上其它字段的未保存修改不能跟着写进文件。
    /// The preset merges on top of the last saved settings, so unsaved edits to
    /// any other field must never reach the files.
    #[test]
    fn the_one_click_preset_does_not_write_unsaved_edits() {
        let saved = ServerSettings {
            host: "saved.example".to_owned(),
            motd: "savedmotd".to_owned(),
            max_players: 20,
            view_distance: 12,
            ..Default::default()
        };
        let preset = performance_presets().recommended;
        // 界面上还没保存的修改：只有这八项以外的字段不同；预设里的寻路三项
        // 也先按相反的值改过，确认结果以预设为准。
        let draft = ServerSettings {
            host: "draft.example".to_owned(),
            motd: "unsavedmotd".to_owned(),
            max_players: 99,
            view_distance: 18,
            mob_pathfinding_phase: !preset.mob_pathfinding_phase,
            mob_pathfinding_budget: 4096,
            mob_pathfinding_fail_cooldown: !preset.mob_pathfinding_fail_cooldown,
            ..saved.clone()
        };
        let merged = merge_performance_preset(&saved, preset);

        assert_eq!(merged.entity_threads, preset.entity_threads);
        assert_eq!(merged.max_entity_collisions, preset.max_entity_collisions);
        assert_eq!(merged.mob_throttle_distance, preset.mob_throttle_distance);
        assert_eq!(merged.mob_sleep_distance, preset.mob_sleep_distance);
        assert_eq!(merged.mob_wakeups_scale, preset.mob_wakeups_scale);
        assert_eq!(merged.mob_pathfinding_phase, preset.mob_pathfinding_phase);
        assert_eq!(merged.mob_pathfinding_budget, preset.mob_pathfinding_budget);
        assert_eq!(
            merged.mob_pathfinding_fail_cooldown,
            preset.mob_pathfinding_fail_cooldown
        );
        assert_ne!(merged.mob_pathfinding_phase, draft.mob_pathfinding_phase);
        assert_ne!(merged.mob_pathfinding_budget, draft.mob_pathfinding_budget);
        assert_ne!(
            merged.mob_pathfinding_fail_cooldown,
            draft.mob_pathfinding_fail_cooldown
        );
        // 其它字段全部来自已保存的设置，草稿里的值一个都不在结果里。
        assert_eq!(merged.host, saved.host);
        assert_eq!(merged.motd, saved.motd);
        assert_eq!(merged.max_players, saved.max_players);
        assert_eq!(merged.view_distance, saved.view_distance);
        assert_ne!(merged.host, draft.host);
        assert_ne!(merged.motd, draft.motd);
        assert_ne!(merged.max_players, draft.max_players);
        assert_ne!(merged.view_distance, draft.view_distance);

        let (bamboo, properties) = write_settings(&merged);
        assert!(
            properties.contains("server-ip=saved.example\n"),
            "{properties}"
        );
        assert!(properties.contains("motd=savedmotd\n"), "{properties}");
        assert!(properties.contains("max-players=20\n"), "{properties}");
        assert!(properties.contains("view-distance=12\n"), "{properties}");
        assert!(!properties.contains("draft.example"), "{properties}");
        assert!(!properties.contains("unsavedmotd"), "{properties}");
        assert!(!properties.contains("max-players=99"), "{properties}");
        assert!(!properties.contains("view-distance=18"), "{properties}");
        assert!(
            bamboo.contains(&format!("entity_threads = {}\n", preset.entity_threads)),
            "{bamboo}"
        );
        assert!(
            bamboo.contains(&format!(
                "max_entity_collisions = {}\n",
                preset.max_entity_collisions
            )),
            "{bamboo}"
        );
        assert!(
            bamboo.contains(&format!(
                "mob_pathfinding_phase = {}\n",
                preset.mob_pathfinding_phase
            )),
            "{bamboo}"
        );
        assert!(
            bamboo.contains(&format!(
                "mob_pathfinding_budget = {}\n",
                preset.mob_pathfinding_budget
            )),
            "{bamboo}"
        );
        assert!(
            bamboo.contains(&format!(
                "mob_pathfinding_fail_cooldown = {}\n",
                preset.mob_pathfinding_fail_cooldown
            )),
            "{bamboo}"
        );
        // 草稿里把节点预算改成 4096 也不能写进文件。
        assert!(
            !bamboo.contains("mob_pathfinding_budget = 4096"),
            "{bamboo}"
        );
    }

    /// 两张预设的 JSON 用固定的 camelCase 拼写，前端才能直接套到字段上。
    /// The two presets keep fixed camelCase JSON names so the frontend can
    /// spread them straight onto its settings object.
    #[test]
    fn the_presets_round_trip_through_the_ui_json() {
        let presets = performance_presets();
        let text = serde_json::to_string(&presets).unwrap();
        assert!(text.contains(r#""maxEntityCollisions":8"#), "{text}");
        assert!(text.contains(r#""mobThrottleDistance":32"#), "{text}");
        assert!(text.contains(r#""mobSleepDistance":64"#), "{text}");
        assert!(text.contains(r#""mobWakeupsScale":1"#), "{text}");
        assert!(text.contains(r#""mobPathfindingPhase":true"#), "{text}");
        assert!(text.contains(r#""mobPathfindingBudget":0"#), "{text}");
        assert!(
            text.contains(r#""mobPathfindingFailCooldown":true"#),
            "{text}"
        );
        let parsed: PerformancePresets = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, presets);
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
        assert!(!parsed.mob_pathfinding_phase);
        assert_eq!(parsed.mob_pathfinding_budget, 0);
        assert!(!parsed.mob_pathfinding_fail_cooldown);
        assert_eq!(parsed.deliveries_per_tick, 64);
        // 世界生成四项也要保持官方默认，不能让旧草稿把新世界变成超平坦。
        // The four world-gen fields keep their official defaults too, so an old
        // draft cannot silently turn a new world into superflat.
        assert_eq!(parsed.level_seed, "");
        assert_eq!(parsed.level_type, WorldType::Normal);
        assert!(parsed.generate_structures);
        assert_eq!(parsed.generator_settings, "{}");
    }
}
