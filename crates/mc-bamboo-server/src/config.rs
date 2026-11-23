//! 服务端配置。

use std::time::Duration;

use mc_bamboo_auth::{CONNECT_TIMEOUT, READ_TIMEOUT, SESSION_SERVER_URL};
use mc_bamboo_game::{MailBudget, WorldGenConfig};
use tracing::warn;

/// 视距的下限，再小客户端就没有可渲染的区块了。
pub const MIN_VIEW_DISTANCE: u8 = 2;
/// 视距的上限，和官方的 `view-distance` 相同。
pub const MAX_VIEW_DISTANCE: u8 = 32;
/// 客户端这么久收不到任何字节就断开。没有别的流量时，心跳是唯一能让它收到字节的东西。
pub const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// 从代理接收玩家信息的方式。默认关闭，和以前一样由服务端自己生成离线 UUID。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ForwardingMode {
    /// 不信任代理。握手地址里的 NUL 分隔内容只是普通文本。
    #[default]
    Off,
    /// BungeeCord 的旧式转发：握手包的服务器地址里带 IP、UUID 和属性。
    Legacy,
    /// Velocity 的现代转发：登录时在 `velocity:player_info` 频道询问，回应带 HMAC-SHA256 签名。
    Modern,
}

/// 服务端运行参数。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 服务器列表里显示的介绍文字。
    pub motd: String,
    /// 同时在线的玩家上限。
    pub max_players: u32,
    /// 服务端视距，也是玩家视距的上限。合法范围是 2 到 32，超出时启动时会被修正。
    ///
    /// 这是客户端的渲染半径。实际发送的区块沿坐标轴多一圈，四角是圆的，
    /// 这样渲染范围里每个区块的邻居都在客户端手里。视距 8 时是 329 个区块。
    pub view_distance: u8,
    /// 连接进入游戏之前可以停留的总时长。
    pub pre_play_timeout: Duration,
    /// 同时处于进入游戏之前阶段的连接上限，超出的连接会被直接丢弃。
    pub max_pre_play_connections: usize,
    /// 发送心跳的间隔。上一次心跳到下一次发送时还没有回应，玩家就会被断开。
    ///
    /// 必须大于 0 且小于 30 秒，否则客户端会先超时，启动时会换成默认值。
    pub keep_alive_interval: Duration,
    /// 登录之后启用压缩的阈值，单位是包负载的字节数，和官方的 `network-compression-threshold` 相同。
    ///
    /// 小于 0 表示关闭压缩，0 表示每个包都压缩。默认 256。
    pub compression_threshold: i32,
    /// 要不要做正版验证，和官方的 `online-mode` 相同。
    ///
    /// 打开之后登录过程会加密，并且每名玩家都要经过 Mojang 的 session server 确认；
    /// 关掉时既不加密也不验证，玩家名由客户端随便填，UUID 由玩家名算出来。
    ///
    /// 和代理转发互斥：代理已经替我们验过了，打开转发时这一项会被关掉。
    ///
    /// 默认关闭：Bamboo 还在开发阶段，本地连进来调试不该依赖外网。
    pub online_mode: bool,
    /// 问 session server 时带上客户端的 IP，和官方的 `prevent-proxy-connections` 相同。
    ///
    /// Mojang 会比对这个 IP 和客户端自己报备的那个，对不上就不认这次登录。
    /// 玩家经过代理进来时两边的 IP 不一样，所以默认关闭。
    pub prevent_proxy_connections: bool,
    /// session server 的 `hasJoined` 地址。默认是 Mojang 的官方地址，测试把它指到本地。
    pub session_server_url: String,
    /// 连接 session server 的时限，对应 authlib `MinecraftClient.CONNECT_TIMEOUT_MS`。
    pub auth_connect_timeout: Duration,
    /// 等 session server 吐下一段字节的时限，对应 authlib `MinecraftClient.READ_TIMEOUT_MS`。
    pub auth_read_timeout: Duration,
    /// 正版玩家的聊天必须带 Mojang 签过的公钥，对应官方的 `enforce-secure-profile`。
    ///
    /// 默认打开。没开正版验证、或者服务公钥还没取到时不生效，和官方
    /// `DedicatedServer.enforceSecureProfile` 一样。
    ///
    /// Signed chat from online-mode players, matching official `enforce-secure-profile`.
    ///
    /// On by default. It does nothing until online mode is on and the service keys have been
    /// fetched, matching `DedicatedServer.enforceSecureProfile`.
    pub enforce_secure_profile: bool,
    /// 玩家信息由谁提供。
    pub forwarding: ForwardingMode,
    /// Velocity 现代转发用的签名密钥，和代理的 `forwarding.secret` 文件内容相同。
    ///
    /// 只有 [`ForwardingMode::Modern`] 会用到。留空时启动会退回 [`ForwardingMode::Off`]，
    /// 因为没有密钥就无法验证代理的回应。
    pub forwarding_secret: String,
    /// 四份管理名单文件所在的目录。空路径表示工作目录，和官方一样。
    /// 测试把这个字段指到临时目录，不在仓库里留文件。
    pub list_directory: std::path::PathBuf,
    /// 世界目录。`None` 表示不存档（测试、oracle、压测用），启动时不会建目录或拿锁。
    /// 命令行版由 `ConfigFiles::to_server_config` 按 `level-name` 设好。
    ///
    /// The world directory. `None` disables saving (tests, oracle, load tests): no
    /// directory is created and no lock is taken at startup.
    pub world_dir: Option<std::path::PathBuf>,
    /// 实体阶段的线程数。0 = 精确驱动（和官方逐位一致）；≥ 1 = 并行驱动（实验）。
    pub entity_threads: usize,
    /// 红石缓存（实验）：结果和官方逐位一致，打开后对大型红石电路有效。默认关。
    pub redstone_cache: bool,
    /// 区块计算池的线程数。0 = 内联执行器（默认）；≥ 1 = 常驻后台线程。
    pub chunk_worker_threads: usize,
    /// 世界生成缓存的上限和空闲释放秒数（`bamboo.toml` 的 `worldgen_terrain_cache_mb` /
    /// `worldgen_feature_cache_mb` / `worldgen_cache_idle_seconds`）。
    ///
    /// Worldgen cache limits and idle-release delay (three `bamboo.toml` keys).
    pub worldgen_cache: mc_bamboo_game::worldgen_cache::WorldgenCacheSettings,
    /// 自然刷怪按玩家算上限（`bamboo.toml` 的 `spawn_per_player_cap`，默认开）；
    /// 关掉只剩官方的全局上限。
    ///
    /// Per-player natural spawn caps (`spawn_per_player_cap` in `bamboo.toml`, on by
    /// default); off keeps only the vanilla global cap.
    pub spawn_per_player_cap: bool,
    /// Director 信箱每刻交付/收回区块的个数上限，0 表示不限。
    pub mail_budget: MailBudget,
    /// 岛的 tick 线程数（`bamboo.toml` 的 `island_threads`）：0 = 齐步、内联（精确模式，
    /// 逐位一致）；≥ 1 = 分离，岛在这么多条 tick 线程上和主控并行跑，区块交付晚一刻。
    /// 缺省见 `mc_bamboo_game::default_island_threads`（环境变量 `BAMBOO_ISLAND_THREADS`
    /// 优先）；要求逐位一致的测试显式给 0。
    ///
    /// Island tick threads (`island_threads` in `bamboo.toml`): 0 is lockstep,
    /// inline (exact mode, bit for bit); 1 or more is detached, the island
    /// running on that many tick threads beside the master, with chunks one
    /// tick later. See `mc_bamboo_game::default_island_threads` for the default
    /// (`BAMBOO_ISLAND_THREADS` wins); bit-exact tests give 0 explicitly.
    pub island_threads: usize,
    /// 多岛（`bamboo.toml` 的 `islands`）：`false` = 整个世界一个岛（精确模式、对拍、
    /// 要求逐位一致的测试）；`true` = 分区器按相距很远的玩家群拆出各自的岛。缺省见
    /// `mc_bamboo_game::default_islands`（环境变量 `BAMBOO_ISLANDS` 优先）。
    ///
    /// Multi-island mode (`islands` in `bamboo.toml`): `false` keeps the whole
    /// world one island (exact mode, oracles, bit-exact tests); `true` lets the
    /// partitioner split distant groups onto their own islands. See
    /// `mc_bamboo_game::default_islands` for the default (`BAMBOO_ISLANDS` wins).
    pub islands: bool,
    /// 多岛的混合驱动下判死的时限（`bamboo.toml` 的 `island_dead_seconds`）：一个岛的同一刻
    /// 在 tick 线程上连续跑超过这么久就判为死岛，它的玩家被断开、区块从上次存盘重新
    /// 加载。缺省见 `mc_bamboo_game::default_island_dead_timeout`（30 秒，环境变量
    /// `BAMBOO_ISLAND_DEAD_SECONDS` 优先）。
    ///
    /// The dead-island timeout under the multi-island hybrid drive
    /// (`island_dead_seconds` in `bamboo.toml`): an island whose tick runs on
    /// its thread longer than this is declared dead, its players disconnected
    /// and its chunks reloaded from the last save. See
    /// `mc_bamboo_game::default_island_dead_timeout` for the default (30 s,
    /// `BAMBOO_ISLAND_DEAD_SECONDS` wins).
    pub island_dead_timeout: Duration,
    /// 世界生成参数（种子、世界类型、flat 预设、是否生成结构）。
    /// 嵌入用法（测试、oracle、压测）默认超平坦、种子 0；由 `server.properties`
    /// 新建的世界在 `ConfigFiles::to_server_config` 里改成官方默认 `normal`。
    ///
    /// World generation parameters (seed, level type, flat preset, structures).
    /// Embedded use defaults to superflat / seed 0; a world created from
    /// `server.properties` is switched to the official default `normal`.
    pub world_gen: WorldGenConfig,
    /// 启动时给官方 `respawn_radius` 定值；`None` 用官方缺省 10。嵌入式测试
    /// 需要固定出生点时可以显式给 0，等价于官方规则为 0 的结果。
    /// Optional initial value for the official `respawn_radius`; `None` keeps
    /// the vanilla default 10. Embedded tests can pin 0 for a fixed spawn.
    pub spawn_radius: Option<i32>,
    /// 官方 `server.properties` 的 `broadcast-console-to-ops`：控制台命令反馈要不要
    /// 广播给在线 OP。默认 true。
    ///
    /// Official `server.properties` `broadcast-console-to-ops`: whether console command
    /// feedback is broadcast to online ops. Defaults to true.
    pub broadcast_console_to_ops: bool,
    /// 要不要起读 stdin 的控制台输入线程。命令行版（从 `server.properties` 建的配置）
    /// 打开；嵌入式用法（测试、oracle、压测、桌面端 GUI）关掉，不去读进程 stdin。
    ///
    /// Whether to start the stdin-reading console thread. Enabled for the CLI (config
    /// built from `server.properties`); disabled for embedded use (tests, oracle, load
    /// tests, the desktop GUI) so they do not read the process stdin.
    pub console_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: "A Bamboo Server".to_owned(),
            max_players: 20,
            view_distance: 8,
            pre_play_timeout: Duration::from_secs(30),
            max_pre_play_connections: 1024,
            keep_alive_interval: Duration::from_secs(15),
            compression_threshold: 256,
            online_mode: false,
            prevent_proxy_connections: false,
            session_server_url: SESSION_SERVER_URL.to_owned(),
            auth_connect_timeout: CONNECT_TIMEOUT,
            auth_read_timeout: READ_TIMEOUT,
            enforce_secure_profile: true,
            forwarding: ForwardingMode::Off,
            forwarding_secret: String::new(),
            list_directory: std::path::PathBuf::new(),
            world_dir: None,
            entity_threads: 0,
            redstone_cache: false,
            chunk_worker_threads: 0,
            worldgen_cache: mc_bamboo_game::worldgen_cache::WorldgenCacheSettings::default(),
            spawn_per_player_cap: true,
            mail_budget: MailBudget::default(),
            island_threads: mc_bamboo_game::default_island_threads(),
            islands: mc_bamboo_game::default_islands(),
            island_dead_timeout: mc_bamboo_game::default_island_dead_timeout(),
            world_gen: WorldGenConfig::flat_template(),
            spawn_radius: None,
            broadcast_console_to_ops: true,
            console_enabled: false,
        }
    }
}

impl ServerConfig {
    /// 把配置修正到能运行的范围。世界线程和 `login` 包都用修正后的值。
    pub(crate) fn normalized(mut self) -> Self {
        self.view_distance = self
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);

        let interval = self.keep_alive_interval;
        if interval.is_zero() || interval >= CLIENT_READ_TIMEOUT {
            let default = Self::default().keep_alive_interval;
            warn!(
                configured = ?interval,
                used = ?default,
                "keep-alive interval must be above zero and below {CLIENT_READ_TIMEOUT:?}"
            );
            self.keep_alive_interval = default;
        }

        // 所有负数都表示关闭压缩，统一成 -1，后面只比较这一个值。
        self.compression_threshold = self.compression_threshold.max(-1);

        // 没有密钥就验不了签名，与其让每个代理都连不上，不如退回关闭模式。
        if self.forwarding == ForwardingMode::Modern && self.forwarding_secret.is_empty() {
            warn!("modern forwarding needs a secret, forwarding is disabled");
            self.forwarding = ForwardingMode::Off;
        }

        // 上面那一步可能把转发关掉，所以互斥判断放在它后面。
        if self.online_mode && self.uses_proxy_forwarding() {
            warn!("online mode cannot be used together with proxy forwarding, turning it off");
            self.online_mode = false;
        }
        self
    }

    /// 代理转发（Velocity / BungeeCord）开着没有。
    ///
    /// 代理会代替我们完成正版验证，再把玩家的身份放进握手包或者插件回应里转发过来，
    /// 这时服务端自己再去问一次 Mojang 一定会失败，所以两者互斥。
    fn uses_proxy_forwarding(&self) -> bool {
        self.forwarding != ForwardingMode::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_view_distance(view_distance: u8) -> u8 {
        let config = ServerConfig {
            view_distance,
            ..ServerConfig::default()
        };
        config.normalized().view_distance
    }

    fn with_keep_alive(keep_alive_interval: Duration) -> Duration {
        let config = ServerConfig {
            keep_alive_interval,
            ..ServerConfig::default()
        };
        config.normalized().keep_alive_interval
    }

    fn with_compression(compression_threshold: i32) -> i32 {
        let config = ServerConfig {
            compression_threshold,
            ..ServerConfig::default()
        };
        config.normalized().compression_threshold
    }

    fn with_forwarding(forwarding: ForwardingMode, secret: &str) -> ForwardingMode {
        let config = ServerConfig {
            forwarding,
            forwarding_secret: secret.to_owned(),
            ..ServerConfig::default()
        };
        config.normalized().forwarding
    }

    #[test]
    fn view_distance_is_clamped_like_the_official_server() {
        assert_eq!(with_view_distance(0), 2);
        assert_eq!(with_view_distance(1), 2);
        assert_eq!(with_view_distance(2), 2);
        assert_eq!(with_view_distance(32), 32);
        assert_eq!(with_view_distance(33), 32);
        assert_eq!(with_view_distance(u8::MAX), 32);
    }

    #[test]
    fn keep_alive_must_leave_room_before_the_client_times_out() {
        let default = ServerConfig::default().keep_alive_interval;

        assert_eq!(
            with_keep_alive(Duration::from_millis(100)),
            Duration::from_millis(100)
        );
        assert_eq!(
            with_keep_alive(Duration::from_millis(29_999)),
            Duration::from_millis(29_999)
        );
        assert_eq!(with_keep_alive(Duration::from_secs(30)), default);
        assert_eq!(with_keep_alive(Duration::from_secs(3600)), default);
    }

    #[test]
    fn keep_alive_cannot_be_zero() {
        // 周期为 0 时 tokio 的 interval 会 panic。
        assert_eq!(
            with_keep_alive(Duration::ZERO),
            ServerConfig::default().keep_alive_interval
        );
    }

    fn with_online_mode(forwarding: ForwardingMode, secret: &str) -> bool {
        let config = ServerConfig {
            online_mode: true,
            forwarding,
            forwarding_secret: secret.to_owned(),
            ..ServerConfig::default()
        };
        config.normalized().online_mode
    }

    #[test]
    fn online_mode_is_off_by_default() {
        let config = ServerConfig::default();
        assert!(!config.online_mode);
        assert!(!config.prevent_proxy_connections);
        assert_eq!(config.session_server_url, SESSION_SERVER_URL);
        assert_eq!(config.auth_connect_timeout, Duration::from_secs(5));
        assert_eq!(config.auth_read_timeout, Duration::from_secs(5));
    }

    #[test]
    fn online_mode_stays_on_while_nothing_forwards() {
        assert!(with_online_mode(ForwardingMode::Off, ""));
    }

    /// 代理已经替服务端验过身份了，再自己问一次 Mojang 必然失败，所以转发赢。
    #[test]
    fn forwarding_turns_online_mode_off() {
        assert!(!with_online_mode(ForwardingMode::Legacy, ""));
        assert!(!with_online_mode(ForwardingMode::Modern, "secret"));
    }

    /// 没有密钥的现代转发会先退回关闭，这时正版验证留着。
    #[test]
    fn modern_forwarding_without_a_secret_leaves_online_mode_alone() {
        assert!(with_online_mode(ForwardingMode::Modern, ""));
    }

    #[test]
    fn the_default_compression_threshold_matches_the_official_server() {
        assert_eq!(ServerConfig::default().compression_threshold, 256);
    }

    #[test]
    fn negative_compression_thresholds_all_mean_off() {
        // 官方只看阈值是不是非负，这里统一成 -1 方便比较。
        assert_eq!(with_compression(i32::MIN), -1);
        assert_eq!(with_compression(-256), -1);
        assert_eq!(with_compression(-1), -1);
        // 0 是合法的：每个包都压缩。
        assert_eq!(with_compression(0), 0);
        assert_eq!(with_compression(256), 256);
        assert_eq!(with_compression(i32::MAX), i32::MAX);
    }

    #[test]
    fn forwarding_is_off_by_default() {
        assert_eq!(ServerConfig::default().forwarding, ForwardingMode::Off);
        assert!(ServerConfig::default().forwarding_secret.is_empty());
    }

    #[test]
    fn modern_forwarding_without_a_secret_falls_back_to_off() {
        assert_eq!(
            with_forwarding(ForwardingMode::Modern, ""),
            ForwardingMode::Off
        );
        assert_eq!(
            with_forwarding(ForwardingMode::Modern, "secret"),
            ForwardingMode::Modern
        );
    }

    #[test]
    fn legacy_forwarding_needs_no_secret() {
        assert_eq!(
            with_forwarding(ForwardingMode::Legacy, ""),
            ForwardingMode::Legacy
        );
        assert_eq!(
            with_forwarding(ForwardingMode::Legacy, "unused"),
            ForwardingMode::Legacy
        );
    }
}
