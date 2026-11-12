//! 服务器目录里的两份配置文件：`server.properties` 和 `bamboo.toml`。
//!
//! 官方已有的项只在 `server.properties` 里：键名、写法和默认值都照 26.3 的
//! `DedicatedServerProperties`。Bamboo 自己加的项在 `bamboo.toml`。命令行版和桌面版
//! 共用这里的读写，桌面版只要给出同一个服务器目录。
//!
//! 未实现的官方键读进来原样留着，写回时也原样写出去，这样从官方服务端迁过来的
//! `server.properties` 不会丢东西。`ServerConfig::default()` 是「不读配置文件」的
//! 嵌入用法（测试、oracle、压测）的默认值，和这里写出来的官方默认值不同。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use mc_bamboo_game::{MAX_ENTITY_THREADS, MAX_MAIL_BUDGET, MailBudget};
use toml::{Table, Value};
use tracing::{info, warn};

use crate::{ForwardingMode, ServerConfig};

/// 官方配置文件的文件名。
pub const PROPERTIES_FILE: &str = "server.properties";
/// Bamboo 自己的配置文件的文件名。
pub const BAMBOO_FILE: &str = "bamboo.toml";

/// 官方 26.3 `DedicatedServerProperties` 构造函数里的全部键和默认值，按官方第一次
/// 启动时 `Properties.store` 的顺序排列。管理服务器的密钥是启动时随机生成的，
/// 这里留空（Bamboo 不实现管理服务器，空值写回时等价于官方的空密钥）。
const OFFICIAL_DEFAULTS: &[(&str, &str)] = &[
    ("accepts-transfers", r#"false"#),
    ("allow-flight", r#"false"#),
    ("broadcast-console-to-ops", r#"true"#),
    ("broadcast-rcon-to-ops", r#"true"#),
    ("bug-report-link", r#""#),
    ("chat-spam-threshold-seconds", r#"10"#),
    ("command-spam-threshold-seconds", r#"10"#),
    ("difficulty", r#"easy"#),
    ("enable-code-of-conduct", r#"false"#),
    ("enable-jmx-monitoring", r#"false"#),
    ("enable-query", r#"false"#),
    ("enable-rcon", r#"false"#),
    ("enable-status", r#"true"#),
    ("enforce-secure-profile", r#"true"#),
    ("enforce-whitelist", r#"false"#),
    ("entity-broadcast-range-percentage", r#"100"#),
    ("force-gamemode", r#"false"#),
    ("function-permission-level", r#"2"#),
    ("gamemode", r#"survival"#),
    ("generate-structures", r#"true"#),
    ("generator-settings", r#"{}"#),
    ("hardcore", r#"false"#),
    ("hide-online-players", r#"false"#),
    ("initial-disabled-packs", r#""#),
    ("initial-enabled-packs", r#"vanilla"#),
    ("level-name", r#"world"#),
    ("level-seed", r#""#),
    ("level-type", "minecraft:normal"),
    ("log-ips", r#"true"#),
    ("management-server-allowed-origins", r#""#),
    ("management-server-enabled", r#"false"#),
    ("management-server-host", r#"localhost"#),
    ("management-server-port", r#"0"#),
    ("management-server-secret", ""),
    ("management-server-tls-enabled", r#"true"#),
    ("management-server-tls-keystore", r#""#),
    ("management-server-tls-keystore-password", r#""#),
    ("max-chained-neighbor-updates", r#"1000000"#),
    ("max-players", r#"20"#),
    ("max-tick-time", r#"60000"#),
    ("max-world-size", r#"29999984"#),
    ("motd", r#"A Minecraft Server"#),
    ("network-compression-threshold", r#"256"#),
    ("online-mode", r#"true"#),
    ("op-permission-level", r#"4"#),
    ("pause-when-empty-seconds", r#"60"#),
    ("player-idle-timeout", r#"0"#),
    ("prevent-proxy-connections", r#"false"#),
    ("query.port", r#"25565"#),
    ("rate-limit", r#"0"#),
    ("rcon.password", r#""#),
    ("rcon.port", r#"25575"#),
    ("region-file-compression", r#"deflate"#),
    ("require-resource-pack", r#"false"#),
    ("resource-pack", r#""#),
    ("resource-pack-id", r#""#),
    ("resource-pack-prompt", r#""#),
    ("resource-pack-sha1", r#""#),
    ("server-ip", r#""#),
    ("server-port", r#"25565"#),
    ("simulation-distance", r#"10"#),
    ("spawn-protection", r#"16"#),
    ("status-heartbeat-interval", r#"0"#),
    ("sync-chunk-writes", r#"true"#),
    ("text-filtering-config", r#""#),
    ("text-filtering-version", r#"0"#),
    ("use-native-transport", r#"true"#),
    ("view-distance", r#"10"#),
    ("white-list", r#"true"#),
];

/// `server.properties` 里 Bamboo 已经实现的键。
const IMPLEMENTED: &[&str] = &[
    "online-mode",
    "prevent-proxy-connections",
    "server-ip",
    "motd",
    "level-name",
    "server-port",
    "view-distance",
    "max-players",
    "network-compression-threshold",
    "enforce-secure-profile",
];

/// `bamboo.toml` 里的全部键。
const BAMBOO_KEYS: &[&str] = &[
    "pre_play_timeout_seconds",
    "max_pre_play_connections",
    "keep_alive_seconds",
    "forwarding",
    "forwarding_secret",
    "auth_connect_timeout_seconds",
    "auth_read_timeout_seconds",
    "entity_threads",
    "deliveries_per_tick",
    "reclaims_per_tick",
];

/// Bamboo 自己的配置项。每一项都对应 [`ServerConfig`] 里的一个字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BambooConfig {
    /// 连接进入游戏之前可以停留的总时长。
    pub pre_play_timeout: Duration,
    /// 同时处于进入游戏之前阶段的连接上限。
    pub max_pre_play_connections: usize,
    /// 发送心跳的间隔。
    pub keep_alive_interval: Duration,
    /// 从代理接收玩家信息的方式。
    pub forwarding: ForwardingMode,
    /// Velocity 现代转发用的签名密钥。
    pub forwarding_secret: String,
    /// 连接 session server 的时限。
    pub auth_connect_timeout: Duration,
    /// 等 session server 吐下一段字节的时限。
    pub auth_read_timeout: Duration,
    /// 实体阶段线程数。0 = 精确驱动（和官方逐位一致）；≥ 1 = 并行驱动（实验）。
    pub entity_threads: usize,
    /// Director 信箱每刻交付区块的个数上限，0 表示不限。
    pub deliveries_per_tick: usize,
    /// Director 信箱每刻收回区块的个数上限，0 表示不限。
    pub reclaims_per_tick: usize,
}

impl Default for BambooConfig {
    fn default() -> Self {
        let defaults = ServerConfig::default();
        Self {
            pre_play_timeout: defaults.pre_play_timeout,
            max_pre_play_connections: defaults.max_pre_play_connections,
            keep_alive_interval: defaults.keep_alive_interval,
            forwarding: defaults.forwarding,
            forwarding_secret: defaults.forwarding_secret,
            auth_connect_timeout: defaults.auth_connect_timeout,
            auth_read_timeout: defaults.auth_read_timeout,
            entity_threads: defaults.entity_threads,
            deliveries_per_tick: defaults.mail_budget.deliveries,
            reclaims_per_tick: defaults.mail_budget.reclaims,
        }
    }
}

impl BambooConfig {
    /// 每一项写一行中文注释，说明它是什么、默认值是多少。
    fn to_toml(&self) -> String {
        let forwarding = match self.forwarding {
            ForwardingMode::Off => "off",
            ForwardingMode::Legacy => "legacy",
            ForwardingMode::Modern => "modern",
        };
        let secret = toml_string(&self.forwarding_secret);
        format!(
            concat!(
                "# Bamboo 服务端自己的配置。官方 server.properties 里的项不在这里。\n",
                "# 改完要重启服务端才生效；值非法时只把那一项换成默认值并记一条警告。\n",
                "\n",
                "# 一个连接从握手到进入游戏的总时长，单位秒，默认 30。\n",
                "pre_play_timeout_seconds = {}\n",
                "\n",
                "# 同时处于握手、登录、配置阶段的连接上限，超出的连接直接丢弃，默认 1024。\n",
                "max_pre_play_connections = {}\n",
                "\n",
                "# 发送心跳的间隔，单位秒，默认 15；必须小于客户端的 30 秒读取时限。\n",
                "keep_alive_seconds = {}\n",
                "\n",
                "# 代理转发的模式：off（默认）/ legacy（BungeeCord）/ modern（Velocity）。\n",
                "forwarding = \"{}\"\n",
                "\n",
                "# Velocity 现代转发的签名密钥，和代理 forwarding.secret 的内容相同，默认留空。\n",
                "forwarding_secret = {}\n",
                "\n",
                "# 连接 Mojang session server 的时限，单位秒，默认 5。\n",
                "auth_connect_timeout_seconds = {}\n",
                "\n",
                "# 等 session server 回下一段字节的时限，单位秒，默认 5。\n",
                "auth_read_timeout_seconds = {}\n",
                "\n",
                "# 实体并行线程数（实验）：0 = 精确模式（默认，和官方逐位一致）；>= 1 = 性能模式。\n",
                "entity_threads = {}\n",
                "\n",
                "# Director 信箱每刻最多交付多少个区块，0 表示不限，默认 64。\n",
                "deliveries_per_tick = {}\n",
                "\n",
                "# Director 信箱每刻最多收回多少个区块，0 表示不限，默认 64。\n",
                "reclaims_per_tick = {}\n",
            ),
            self.pre_play_timeout.as_secs(),
            self.max_pre_play_connections,
            self.keep_alive_interval.as_secs(),
            forwarding,
            secret,
            self.auth_connect_timeout.as_secs(),
            self.auth_read_timeout.as_secs(),
            self.entity_threads,
            self.deliveries_per_tick,
            self.reclaims_per_tick,
        )
    }

    /// 从解析好的 TOML 表格逐键取值。语法已经检查过，坏的单个键只回落自己。
    fn from_table(table: &Table, file: &Path) -> Self {
        let defaults = Self::default();
        let seconds = |key: &str, default: Duration| -> Duration {
            let used = integer(table, key, file, default.as_secs() as usize, 1, 3600);
            Duration::from_secs(used as u64)
        };
        let forwarding = match string(table, "forwarding", file, "off").as_str() {
            "legacy" => ForwardingMode::Legacy,
            "modern" => ForwardingMode::Modern,
            "off" => ForwardingMode::Off,
            other => {
                warn!(%other, file = %file.display(), key = "forwarding", used = "off", "invalid Bamboo config value, using default");
                ForwardingMode::Off
            }
        };
        Self {
            pre_play_timeout: seconds("pre_play_timeout_seconds", defaults.pre_play_timeout),
            max_pre_play_connections: integer(
                table,
                "max_pre_play_connections",
                file,
                defaults.max_pre_play_connections,
                1,
                usize::MAX,
            ),
            keep_alive_interval: Duration::from_secs(integer(
                table,
                "keep_alive_seconds",
                file,
                defaults.keep_alive_interval.as_secs() as usize,
                1,
                29,
            ) as u64),
            forwarding,
            forwarding_secret: string(table, "forwarding_secret", file, ""),
            auth_connect_timeout: seconds(
                "auth_connect_timeout_seconds",
                defaults.auth_connect_timeout,
            ),
            auth_read_timeout: seconds("auth_read_timeout_seconds", defaults.auth_read_timeout),
            entity_threads: integer(table, "entity_threads", file, 0, 0, MAX_ENTITY_THREADS),
            deliveries_per_tick: integer(
                table,
                "deliveries_per_tick",
                file,
                defaults.deliveries_per_tick,
                0,
                MAX_MAIL_BUDGET,
            ),
            reclaims_per_tick: integer(
                table,
                "reclaims_per_tick",
                file,
                defaults.reclaims_per_tick,
                0,
                MAX_MAIL_BUDGET,
            ),
        }
    }
}

/// 用 TOML 的基本字符串写出，转义引号和反斜杠。
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn string(table: &Table, key: &str, file: &Path, default: &str) -> String {
    match table.get(key) {
        None => default.to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => {
            warn!(key, original = %value, file = %file.display(), used = default, "invalid Bamboo config value, using default");
            default.to_owned()
        }
    }
}

fn integer(table: &Table, key: &str, file: &Path, default: usize, min: usize, max: usize) -> usize {
    match table.get(key) {
        None => default,
        Some(value) => match value.as_integer().and_then(|v| usize::try_from(v).ok()) {
            Some(v) if (min..=max).contains(&v) => v,
            _ => {
                warn!(key, original = %value, file = %file.display(), used = default, "invalid Bamboo config value, using default");
                default
            }
        },
    }
}

/// 官方 `server.properties` 的键值对。已实现的键有强类型的值，其余原样保留。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialConfig {
    /// 服务器列表里的介绍文字。
    pub motd: String,
    /// 同时在线的玩家上限。
    pub max_players: u32,
    /// 服务端视距。
    pub view_distance: u8,
    /// 要不要做正版验证。
    pub online_mode: bool,
    /// 问 session server 时带上客户端的 IP。
    pub prevent_proxy_connections: bool,
    /// 正版聊天必须带 Mojang 签过的公钥。
    pub enforce_secure_profile: bool,
    /// 登录之后启用压缩的阈值，小于 0 表示关闭。
    pub compression_threshold: i32,
    /// 监听地址，空字符串表示监听所有网卡。
    pub server_ip: String,
    /// 监听端口。
    pub server_port: u16,
    /// 世界名字。服务器目录 + 这个名字就是世界目录。
    pub level_name: String,
    /// 未实现或未知的键，按读入顺序保存原文，写回时原样输出。
    passthrough: Vec<(String, String)>,
}

impl Default for OfficialConfig {
    fn default() -> Self {
        let mut defaults = BTreeMap::new();
        for &(key, value) in OFFICIAL_DEFAULTS {
            defaults.insert(key, value);
        }
        let get = |key: &str| defaults.get(key).copied().unwrap_or("");
        let passthrough = OFFICIAL_DEFAULTS
            .iter()
            .filter(|(key, _)| !IMPLEMENTED.contains(key))
            .map(|&(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        Self {
            motd: get("motd").to_owned(),
            max_players: get("max-players").parse().unwrap_or(20),
            view_distance: get("view-distance").parse().unwrap_or(10),
            online_mode: parse_bool(get("online-mode")),
            prevent_proxy_connections: parse_bool(get("prevent-proxy-connections")),
            enforce_secure_profile: parse_bool(get("enforce-secure-profile")),
            compression_threshold: get("network-compression-threshold").parse().unwrap_or(256),
            server_ip: String::new(),
            server_port: get("server-port").parse().unwrap_or(25565),
            level_name: get("level-name").to_owned(),
            passthrough,
        }
    }
}

impl OfficialConfig {
    /// 照 Java `Properties.store` 的写法序列化：`key=value`，只转义键里的分隔符。
    fn to_properties(&self) -> String {
        let mut out = String::from("#Minecraft server properties\n");
        for (key, value) in self.entries() {
            out.push_str(&escape_key(&key));
            out.push('=');
            out.push_str(&escape_value(&value));
            out.push('\n');
        }
        out
    }

    fn entries(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(OFFICIAL_DEFAULTS.len());
        for (key, _) in OFFICIAL_DEFAULTS {
            let value = match *key {
                "motd" => self.motd.clone(),
                "max-players" => self.max_players.to_string(),
                "view-distance" => self.view_distance.to_string(),
                "online-mode" => self.online_mode.to_string(),
                "prevent-proxy-connections" => self.prevent_proxy_connections.to_string(),
                "enforce-secure-profile" => self.enforce_secure_profile.to_string(),
                "network-compression-threshold" => self.compression_threshold.to_string(),
                "server-ip" => self.server_ip.clone(),
                "server-port" => self.server_port.to_string(),
                "level-name" => self.level_name.clone(),
                _ => continue,
            };
            out.push(((*key).to_owned(), value));
        }
        out.extend(self.passthrough.iter().cloned());
        out
    }

    /// 官方逐键读 `Properties`：坏的整型用默认值并写回默认值，坏的布尔按
    /// `Boolean.valueOf` 一律当 false，字符串原样用。
    fn from_properties(text: &str, file: &Path) -> Self {
        let mut config = Self::default();
        let mut passthrough = Vec::new();
        let mut seen = BTreeSet::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.starts_with('#') || line.starts_with('!') || line.trim().is_empty() {
                continue;
            }
            let (key, value) = split_property(line);
            if key.is_empty() || seen.contains(&key) {
                continue;
            }
            seen.insert(key.clone());
            if !IMPLEMENTED.contains(&key.as_str()) {
                // 存的是解码后的原值：写回时统一转义一次，避免反复写回把反斜杠越堆越多。
                passthrough.push((key, unescape(&value)));
                continue;
            }
            match key.as_str() {
                "motd" => config.motd = value,
                "max-players" => config.max_players = parse_u32(&value, "max-players", file, 20),
                "view-distance" => {
                    config.view_distance = parse_u8(&value, "view-distance", file, 10)
                }
                "online-mode" => config.online_mode = parse_bool(&value),
                "prevent-proxy-connections" => {
                    config.prevent_proxy_connections = parse_bool(&value)
                }
                "enforce-secure-profile" => config.enforce_secure_profile = parse_bool(&value),
                "network-compression-threshold" => {
                    config.compression_threshold =
                        parse_i32(&value, "network-compression-threshold", file, 256);
                }
                "server-ip" => config.server_ip = value,
                "server-port" => config.server_port = parse_u16(&value, "server-port", file, 25565),
                "level-name" => config.level_name = value,
                _ => unreachable!("IMPLEMENTED 和 match 必须同步"),
            }
        }
        // 官方第一次启动会把所有缺省键写回文件，缺的键也补上默认值。
        for (key, value) in OFFICIAL_DEFAULTS {
            if !IMPLEMENTED.contains(key) && !seen.contains(*key) {
                passthrough.push(((*key).to_owned(), (*value).to_owned()));
            }
        }
        config.passthrough = passthrough;
        config
    }
}

/// 官方 `Boolean.valueOf`：只有忽略大小写等于 "true" 才是真，其它值都是假。
fn parse_bool(value: &str) -> bool {
    value.eq_ignore_ascii_case("true")
}

fn parse_u32(value: &str, key: &str, file: &Path, default: u32) -> u32 {
    value.parse().unwrap_or_else(|_| {
        warn!(key, original = value, file = %file.display(), used = default, "invalid server.properties value, using default");
        default
    })
}

fn parse_u8(value: &str, key: &str, file: &Path, default: u8) -> u8 {
    value.parse().unwrap_or_else(|_| {
        warn!(key, original = value, file = %file.display(), used = default, "invalid server.properties value, using default");
        default
    })
}

fn parse_i32(value: &str, key: &str, file: &Path, default: i32) -> i32 {
    value.parse().unwrap_or_else(|_| {
        warn!(key, original = value, file = %file.display(), used = default, "invalid server.properties value, using default");
        default
    })
}

fn parse_u16(value: &str, key: &str, file: &Path, default: u16) -> u16 {
    value.parse().unwrap_or_else(|_| {
        warn!(key, original = value, file = %file.display(), used = default, "invalid server.properties value, using default");
        default
    })
}

/// Java `Properties.load` 的一行：键和值用第一个未转义的 `=`、`:` 或空白分开。
fn split_property(line: &str) -> (String, String) {
    let mut escaped = false;
    let mut separator = None;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '=' || ch == ':' || ch.is_whitespace() {
            separator = Some(index);
            break;
        }
    }
    let Some(at) = separator else {
        return (unescape(line), String::new());
    };
    let key = unescape(&line[..at]);
    let rest = &line[at + 1..];
    let rest = rest.trim_start_matches(|c: char| c == '=' || c == ':' || c.is_whitespace());
    (key, unescape(rest))
}

/// Java 属性文件里 `\=` `\:` `\ ` `\\` `\n` 这些转义还原成原字符。
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// 写回时值里的 `=`、`:`、`#`、`!` 和反斜杠要转义，和 `Properties.store` 一致；
/// 空白只有开头那一段要转义，中间的空格原样写（官方的 `motd` 默认值就是这样）。
fn escape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut leading = true;
    for ch in value.chars() {
        leading &= ch == ' ';
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{000c}' => out.push_str("\\f"),
            ' ' if leading => out.push_str("\\ "),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(ch);
            }
            other => out.push(other),
        }
    }
    out
}

/// 键里的 `=`、`:`、空格、反斜杠也要转义；官方键里只有 `.` 这类字符。
fn escape_key(key: &str) -> String {
    escape_value(key)
}

/// 两份配置文件合起来的可编辑内容。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFiles {
    /// 官方 `server.properties` 的内容。
    pub official: OfficialConfig,
    /// Bamboo 自己的项。
    pub bamboo: BambooConfig,
}

impl ConfigFiles {
    /// 变成运行时用的 [`ServerConfig`]。四份管理名单固定放在服务器目录里。
    pub fn to_server_config(&self, directory: &Path) -> ServerConfig {
        let defaults = ServerConfig::default();
        ServerConfig {
            motd: self.official.motd.clone(),
            max_players: self.official.max_players,
            view_distance: self.official.view_distance,
            pre_play_timeout: self.bamboo.pre_play_timeout,
            max_pre_play_connections: self.bamboo.max_pre_play_connections,
            keep_alive_interval: self.bamboo.keep_alive_interval,
            compression_threshold: self.official.compression_threshold,
            online_mode: self.official.online_mode,
            prevent_proxy_connections: self.official.prevent_proxy_connections,
            session_server_url: defaults.session_server_url,
            auth_connect_timeout: self.bamboo.auth_connect_timeout,
            auth_read_timeout: self.bamboo.auth_read_timeout,
            enforce_secure_profile: self.official.enforce_secure_profile,
            forwarding: self.bamboo.forwarding,
            forwarding_secret: self.bamboo.forwarding_secret.clone(),
            list_directory: directory.to_path_buf(),
            entity_threads: self.bamboo.entity_threads,
            mail_budget: MailBudget {
                deliveries: self.bamboo.deliveries_per_tick,
                reclaims: self.bamboo.reclaims_per_tick,
            },
        }
    }

    /// 从 [`ServerConfig`] 反向组装文件内容，保留已有的未实现键。
    pub fn from_server_config(config: &ServerConfig, existing: &Self) -> Self {
        let mut official = existing.official.clone();
        official.motd = config.motd.clone();
        official.max_players = config.max_players;
        official.view_distance = config.view_distance;
        official.online_mode = config.online_mode;
        official.prevent_proxy_connections = config.prevent_proxy_connections;
        official.enforce_secure_profile = config.enforce_secure_profile;
        official.compression_threshold = config.compression_threshold;
        let mut bamboo = existing.bamboo.clone();
        bamboo.pre_play_timeout = config.pre_play_timeout;
        bamboo.max_pre_play_connections = config.max_pre_play_connections;
        bamboo.keep_alive_interval = config.keep_alive_interval;
        bamboo.forwarding = config.forwarding;
        bamboo.forwarding_secret = config.forwarding_secret.clone();
        bamboo.auth_connect_timeout = config.auth_connect_timeout;
        bamboo.auth_read_timeout = config.auth_read_timeout;
        bamboo.entity_threads = config.entity_threads;
        bamboo.deliveries_per_tick = config.mail_budget.deliveries;
        bamboo.reclaims_per_tick = config.mail_budget.reclaims;
        Self { official, bamboo }
    }

    /// 和官方默认值不同的项，给启动日志用。密钥只报「已设置」，不打印值。
    pub fn non_defaults(&self) -> Vec<String> {
        let defaults = Self::default();
        let mut out = Vec::new();
        let official = [
            (
                "motd",
                self.official.motd.clone(),
                defaults.official.motd.clone(),
            ),
            (
                "max-players",
                self.official.max_players.to_string(),
                defaults.official.max_players.to_string(),
            ),
            (
                "view-distance",
                self.official.view_distance.to_string(),
                defaults.official.view_distance.to_string(),
            ),
            (
                "online-mode",
                self.official.online_mode.to_string(),
                defaults.official.online_mode.to_string(),
            ),
            (
                "prevent-proxy-connections",
                self.official.prevent_proxy_connections.to_string(),
                defaults.official.prevent_proxy_connections.to_string(),
            ),
            (
                "enforce-secure-profile",
                self.official.enforce_secure_profile.to_string(),
                defaults.official.enforce_secure_profile.to_string(),
            ),
            (
                "network-compression-threshold",
                self.official.compression_threshold.to_string(),
                defaults.official.compression_threshold.to_string(),
            ),
            (
                "server-ip",
                self.official.server_ip.clone(),
                defaults.official.server_ip.clone(),
            ),
            (
                "server-port",
                self.official.server_port.to_string(),
                defaults.official.server_port.to_string(),
            ),
            (
                "level-name",
                self.official.level_name.clone(),
                defaults.official.level_name.clone(),
            ),
        ];
        for (key, value, default) in official {
            if value != default {
                out.push(format!("{key}={value}"));
            }
        }
        let bamboo = [
            (
                "pre_play_timeout_seconds",
                self.bamboo.pre_play_timeout.as_secs(),
                defaults.bamboo.pre_play_timeout.as_secs(),
            ),
            (
                "max_pre_play_connections",
                self.bamboo.max_pre_play_connections as u64,
                defaults.bamboo.max_pre_play_connections as u64,
            ),
            (
                "keep_alive_seconds",
                self.bamboo.keep_alive_interval.as_secs(),
                defaults.bamboo.keep_alive_interval.as_secs(),
            ),
            (
                "auth_connect_timeout_seconds",
                self.bamboo.auth_connect_timeout.as_secs(),
                defaults.bamboo.auth_connect_timeout.as_secs(),
            ),
            (
                "auth_read_timeout_seconds",
                self.bamboo.auth_read_timeout.as_secs(),
                defaults.bamboo.auth_read_timeout.as_secs(),
            ),
            (
                "entity_threads",
                self.bamboo.entity_threads as u64,
                defaults.bamboo.entity_threads as u64,
            ),
            (
                "deliveries_per_tick",
                self.bamboo.deliveries_per_tick as u64,
                defaults.bamboo.deliveries_per_tick as u64,
            ),
            (
                "reclaims_per_tick",
                self.bamboo.reclaims_per_tick as u64,
                defaults.bamboo.reclaims_per_tick as u64,
            ),
        ];
        for (key, value, default) in bamboo {
            if value != default {
                out.push(format!("{key}={value}"));
            }
        }
        if self.bamboo.forwarding != defaults.bamboo.forwarding {
            let mode = match self.bamboo.forwarding {
                ForwardingMode::Off => "off",
                ForwardingMode::Legacy => "legacy",
                ForwardingMode::Modern => "modern",
            };
            out.push(format!("forwarding={mode}"));
        }
        if !self.bamboo.forwarding_secret.is_empty() {
            out.push("forwarding_secret=已设置".to_owned());
        }
        out
    }

    /// 官方有、Bamboo 还没实现的键。启动时列一次「这些键现在不起作用」。
    pub fn unsupported_keys(&self) -> Vec<String> {
        let official: BTreeSet<&str> = OFFICIAL_DEFAULTS.iter().map(|(key, _)| *key).collect();
        self.official
            .passthrough
            .iter()
            .filter(|(key, _)| official.contains(key.as_str()))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// 官方键之外用户自己加的键，原样保留但不列进「未实现」日志。
    pub fn unknown_keys(&self) -> Vec<String> {
        let official: BTreeSet<&str> = OFFICIAL_DEFAULTS.iter().map(|(key, _)| *key).collect();
        self.official
            .passthrough
            .iter()
            .filter(|(key, _)| !official.contains(key.as_str()))
            .map(|(key, _)| key.clone())
            .collect()
    }
}

/// 读两份文件，必要时用默认值创建它们，并把读入的值写回一遍（照官方第一次
/// 启动的 `forceSave()`：坏值换成默认值、缺的键补齐）。
///
/// `write_back` 为 false 时只读不写，给测试和 APP 的「恢复默认」用。
pub fn load_or_create(directory: &Path, write_back: bool) -> io::Result<ConfigFiles> {
    fs::create_dir_all(directory)?;
    let mut files = ConfigFiles::default();
    let official_path = directory.join(PROPERTIES_FILE);
    files.official = read_official(&official_path)?;
    let bamboo_path = directory.join(BAMBOO_FILE);
    files.bamboo = read_bamboo(&bamboo_path)?;
    if write_back {
        write(directory, &files)?;
    }
    Ok(files)
}

/// 只读两份文件，不创建也不写回；文件不存在时用默认值。
pub fn read(directory: &Path) -> io::Result<ConfigFiles> {
    Ok(ConfigFiles {
        official: read_official(&directory.join(PROPERTIES_FILE))?,
        bamboo: read_bamboo(&directory.join(BAMBOO_FILE))?,
    })
}

fn read_official(path: &Path) -> io::Result<OfficialConfig> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(OfficialConfig::from_properties(&text, path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OfficialConfig::default()),
        Err(error) => {
            warn!(file = %path.display(), %error, "cannot read server.properties, using official defaults");
            Ok(OfficialConfig::default())
        }
    }
}

fn read_bamboo(path: &Path) -> io::Result<BambooConfig> {
    match fs::read_to_string(path) {
        Ok(text) => match text.parse::<Table>() {
            Ok(table) => Ok(BambooConfig::from_table(&table, path)),
            Err(error) => {
                warn!(file = %path.display(), %error, "cannot parse bamboo.toml, using all defaults");
                Ok(BambooConfig::default())
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BambooConfig::default()),
        Err(error) => {
            warn!(file = %path.display(), %error, "cannot read bamboo.toml, using all defaults");
            Ok(BambooConfig::default())
        }
    }
}

/// 把两份文件写到服务器目录。写之前先建目录，写失败只返回错误、不崩。
pub fn write(directory: &Path, files: &ConfigFiles) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    fs::write(
        directory.join(PROPERTIES_FILE),
        files.official.to_properties(),
    )?;
    fs::write(directory.join(BAMBOO_FILE), files.bamboo.to_toml())?;
    Ok(())
}

/// 启动时把文件位置和「不是默认值的项」记一条日志，再报一次未实现的官方键。
pub fn log_startup(directory: &Path, files: &ConfigFiles) {
    let canonical = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    let official = canonical.join(PROPERTIES_FILE);
    let bamboo = canonical.join(BAMBOO_FILE);
    let non_defaults = files.non_defaults();
    if non_defaults.is_empty() {
        info!(properties = %official.display(), bamboo = %bamboo.display(), "configuration files (all defaults)");
    } else {
        info!(properties = %official.display(), bamboo = %bamboo.display(), changed = %non_defaults.join(", "), "configuration files");
    }
    let unsupported = files.unsupported_keys();
    if !unsupported.is_empty() {
        warn!(keys = %unsupported.join(", "), "these server.properties keys are not implemented by Bamboo yet and currently have no effect");
    }
    let unknown = files.unknown_keys();
    if !unknown.is_empty() {
        info!(keys = %unknown.join(", "), "unknown server.properties keys are preserved as-is");
    }
    let known: BTreeSet<&str> = BAMBOO_KEYS.iter().copied().collect();
    // TOML 里的额外表/键在解析阶段就被忽略，这里补一条提示，免得用户以为生效了。
    let extra: Vec<String> = fs::read_to_string(canonical.join(BAMBOO_FILE))
        .ok()
        .and_then(|text| text.parse::<Table>().ok())
        .map(|table| {
            table
                .keys()
                .filter(|key| !known.contains(key.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if !extra.is_empty() {
        info!(keys = %extra.join(", "), "unknown bamboo.toml keys are ignored");
    }
}

/// 服务器目录 + `level-name` = 世界目录。存档接进来之后由调用方把它放进
/// `ServerConfig.world_dir`；这里先只提供换算。
pub fn world_dir(directory: &Path, files: &ConfigFiles) -> PathBuf {
    directory.join(&files.official.level_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// 每个测试用独立的临时目录，不碰仓库里的文件。
    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("bamboo-config-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            Self(temp_dir())
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn defaults_match_the_official_26_3_file() {
        let defaults = ConfigFiles::default();
        assert_eq!(defaults.official.motd, "A Minecraft Server");
        assert_eq!(defaults.official.view_distance, 10);
        assert_eq!(defaults.official.max_players, 20);
        assert_eq!(defaults.official.server_port, 25565);
        assert!(defaults.official.online_mode);
        assert!(defaults.official.enforce_secure_profile);
        assert_eq!(defaults.official.compression_threshold, 256);
        assert_eq!(defaults.official.level_name, "world");
        assert_eq!(defaults.official.passthrough.len(), 59);
        assert_eq!(defaults.bamboo.entity_threads, 0);
        assert_eq!(defaults.bamboo.deliveries_per_tick, 64);
        assert_eq!(defaults.bamboo.reclaims_per_tick, 64);
        assert_eq!(defaults.bamboo.forwarding, ForwardingMode::Off);
    }

    #[test]
    fn first_start_writes_both_files_with_all_defaults() {
        let dir = Temp::new();
        let files = load_or_create(dir.path(), true).unwrap();
        assert_eq!(files, ConfigFiles::default());
        let properties = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        assert!(properties.starts_with("#Minecraft server properties\n"));
        for (key, value) in OFFICIAL_DEFAULTS {
            assert!(
                properties.contains(&format!("{}={}\n", escape_key(key), escape_value(value))),
                "missing {key} in {properties}"
            );
        }
        let bamboo = fs::read_to_string(dir.path().join(BAMBOO_FILE)).unwrap();
        for key in BAMBOO_KEYS {
            assert!(bamboo.contains(&format!("{key} = ")), "missing {key}");
        }
        // 每一项上方都有一行中文注释说明默认值。
        assert!(bamboo.contains("默认 30"));
        assert!(bamboo.contains("默认 1024"));
    }

    #[test]
    fn round_trip_is_byte_identical_and_keeps_unimplemented_keys() {
        let dir = Temp::new();
        let mut files = ConfigFiles::default();
        files.official.motd = "a=b:c 中文 \\z".to_owned();
        files.official.view_distance = 12;
        files.official.online_mode = false;
        files.official.server_ip = "127.0.0.1".to_owned();
        files.official.server_port = 25999;
        files.official.level_name = "survival".to_owned();
        files
            .official
            .passthrough
            .push(("my-custom-key".to_owned(), "keep=this".to_owned()));
        files.bamboo.entity_threads = 4;
        files.bamboo.forwarding = ForwardingMode::Modern;
        files.bamboo.forwarding_secret = "s\"ec\\ret".to_owned();
        files.bamboo.deliveries_per_tick = 0;
        write(dir.path(), &files).unwrap();
        let properties = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        let bamboo = fs::read_to_string(dir.path().join(BAMBOO_FILE)).unwrap();

        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded, files);
        write(dir.path(), &loaded).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap(),
            properties
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(BAMBOO_FILE)).unwrap(),
            bamboo
        );
        assert_eq!(loaded.unsupported_keys().len(), 59);
        assert_eq!(loaded.unknown_keys(), ["my-custom-key"]);
        assert_eq!(world_dir(dir.path(), &loaded), dir.path().join("survival"));
    }

    #[test]
    fn official_illegal_values_fall_back_per_key_like_vanilla() {
        let dir = Temp::new();
        fs::write(
            dir.path().join(PROPERTIES_FILE),
            "view-distance=abc\nmax-players=-1\nnetwork-compression-threshold=x\nserver-port=99999\nonline-mode=YES\nmotd=hello\n",
        )
        .unwrap();
        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded.official.view_distance, 10);
        assert_eq!(loaded.official.max_players, 20);
        assert_eq!(loaded.official.compression_threshold, 256);
        assert_eq!(loaded.official.server_port, 25565);
        // Boolean.valueOf 只有忽略大小写的 true 是真。
        assert!(!loaded.official.online_mode);
        assert_eq!(loaded.official.motd, "hello");
        // 写回时坏值换成默认值，缺失的键补齐。
        load_or_create(dir.path(), true).unwrap();
        let text = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        assert!(text.contains("view-distance=10\n"));
        assert!(text.contains("max-players=20\n"));
        assert!(text.contains("server-port=25565\n"));
        assert!(text.contains("white-list=true\n"));
    }

    #[test]
    fn bamboo_illegal_values_fall_back_per_key() {
        let dir = Temp::new();
        fs::write(
            dir.path().join(BAMBOO_FILE),
            "entity_threads = -1\ndeliveries_per_tick = \"many\"\nkeep_alive_seconds = 99\nforwarding = \"other\"\nreclaims_per_tick = 16\n",
        )
        .unwrap();
        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded.bamboo.entity_threads, 0);
        assert_eq!(loaded.bamboo.deliveries_per_tick, 64);
        assert_eq!(loaded.bamboo.keep_alive_interval, Duration::from_secs(15));
        assert_eq!(loaded.bamboo.forwarding, ForwardingMode::Off);
        assert_eq!(loaded.bamboo.reclaims_per_tick, 16);
    }

    #[test]
    fn a_broken_toml_uses_all_defaults() {
        let dir = Temp::new();
        fs::write(dir.path().join(BAMBOO_FILE), "entity_threads = [1,\n").unwrap();
        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded.bamboo, BambooConfig::default());
    }

    #[test]
    fn server_config_carries_every_value_and_the_list_directory() {
        let dir = Temp::new();
        let mut files = ConfigFiles::default();
        files.official.view_distance = 16;
        files.bamboo.entity_threads = 8;
        files.bamboo.deliveries_per_tick = 32;
        let config = files.to_server_config(dir.path());
        assert_eq!(config.view_distance, 16);
        assert_eq!(config.entity_threads, 8);
        assert_eq!(config.mail_budget.deliveries, 32);
        assert_eq!(config.list_directory, dir.path());
        // session server 地址只能由代码指定，不出现在任何配置文件里。
        assert_eq!(
            config.session_server_url,
            ServerConfig::default().session_server_url
        );
        let rebuilt = ConfigFiles::from_server_config(&config, &files);
        assert_eq!(rebuilt, files);
    }

    #[test]
    fn non_defaults_hide_secrets() {
        let mut files = ConfigFiles::default();
        files.bamboo.forwarding_secret = "supersecret".to_owned();
        let changed = files.non_defaults();
        assert!(
            changed
                .iter()
                .any(|line| line == "forwarding_secret=已设置")
        );
        assert!(!changed.iter().any(|line| line.contains("supersecret")));
    }

    /// 反复写回不能把未实现键里的转义一层层堆起来。
    #[test]
    fn repeated_start_does_not_pile_up_escapes() {
        let dir = Temp::new();
        load_or_create(dir.path(), true).unwrap();
        let first = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        for _ in 0..3 {
            load_or_create(dir.path(), true).unwrap();
        }
        let last = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        assert_eq!(last, first);
        assert!(last.contains("level-type=minecraft\\:normal\n"));
        assert!(last.contains("motd=A Minecraft Server\n"));
    }

    #[test]
    fn properties_escaping_round_trips_like_java() {
        let dir = Temp::new();
        let text = "motd=a\\=b\\:c\\ \\#d \\\\z\\nline\nlevel-type=minecraft\\:normal\n";
        fs::write(dir.path().join(PROPERTIES_FILE), text).unwrap();
        let loaded = read(dir.path()).unwrap();
        assert_eq!(loaded.official.motd, "a=b:c #d \\z\nline");
        write(dir.path(), &loaded).unwrap();
        let reread = fs::read_to_string(dir.path().join(PROPERTIES_FILE)).unwrap();
        assert_eq!(read(dir.path()).unwrap(), loaded);
        assert!(reread.contains("level-type=minecraft\\:normal\n"));
    }
}
