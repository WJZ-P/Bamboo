//! 服务器管理名单：OP、封禁、白名单和空闲超时。
//!
//! 世界线程是这些名单的唯一写者。连接层只拿发布出去的只读快照做登录准入，
//! 写盘由连接层那侧的任务完成，世界线程不碰 IO。
//!
//! Server admin lists: ops, bans, the whitelist and the idle timeout. The world thread is
//! their only writer. The connection layer reads published snapshots for login admission
//! and writes files on its own task, so the world thread never does IO.

use std::{collections::BTreeMap, sync::mpsc};

use uuid::Uuid;

/// 一名玩家的身份。官方 `NameAndId`：名单按 UUID 查，名字只用来显示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameAndId {
    pub uuid: Uuid,
    pub name: String,
}

/// `ops.json` 里的一项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpEntry {
    /// 官方 `PermissionLevel` 的 ID，0 到 4。
    pub level: u8,
    pub bypasses_player_limit: bool,
}

/// 封禁名单里的一项。官方 `BanListEntry`。
///
/// 日期保留官方文件里的文本，同时在读入时算出毫秒值供到期判断使用。
/// `expires` 为 `None` 对应官方的 `forever`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanEntry {
    pub created: String,
    pub source: String,
    pub expires: Option<String>,
    pub expires_millis: Option<i64>,
    pub reason: Option<String>,
}

impl BanEntry {
    pub(crate) fn expired(&self, now_millis: i64) -> bool {
        self.expires_millis
            .is_some_and(|expires| expires <= now_millis)
    }
}

/// 名单里的一项 OP，带显示用的名字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpListEntry {
    pub name: String,
    pub entry: OpEntry,
}

/// 名单里的一项封禁，带显示用的名字（玩家封禁）或 IP（IP 封禁）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanListEntry {
    pub name: String,
    pub entry: BanEntry,
}

/// 所有管理名单和开关。世界线程持有唯一一份权威副本。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminState {
    pub ops: BTreeMap<Uuid, OpListEntry>,
    pub player_bans: BTreeMap<Uuid, BanListEntry>,
    pub ip_bans: BTreeMap<String, BanListEntry>,
    pub whitelist: BTreeMap<Uuid, NameAndId>,
    /// 官方 `white-list`，由 `/whitelist on|off` 改，重启恢复 false。
    pub using_whitelist: bool,
    /// 官方 `enforce-whitelist`，官方默认关闭。Bamboo 没有这个配置项。
    pub enforce_whitelist: bool,
    /// 官方 `playerIdleTimeout`，单位是分钟，0 表示关闭。
    pub idle_timeout_minutes: i32,
    /// `/stop` 发的成功消息已经进了出站缓冲，这一 tick 末尾走现有关服流程。
    pub stop_requested: bool,
}

impl AdminState {
    /// 官方 `operatorUserPermissions` 默认 OWNER（4）。
    pub const OP_LEVEL: u8 = 4;

    pub fn op(&self, uuid: Uuid) -> Option<OpEntry> {
        self.ops.get(&uuid).map(|entry| entry.entry)
    }

    /// 官方 `PlayerList.canPlayerLogin` 的顺序：玩家封禁、白名单、IP 封禁。
    /// 满员由连接层判断，世界线程只检查这三项。
    ///
    /// Official `PlayerList.canPlayerLogin` order: player ban, whitelist, IP ban.
    /// The connection layer checks the player limit; the world thread checks these three.
    pub fn login_refusal(&self, uuid: Uuid, ip: &str, now_millis: i64) -> Option<LoginRefusal> {
        if let Some(ban) = self.player_bans.get(&uuid)
            && !ban.entry.expired(now_millis)
        {
            return Some(LoginRefusal::PlayerBanned(ban.entry.clone()));
        }
        if self.using_whitelist
            && !self.ops.contains_key(&uuid)
            && !self.whitelist.contains_key(&uuid)
        {
            return Some(LoginRefusal::NotWhitelisted);
        }
        if let Some(ban) = self.ip_bans.get(ip)
            && !ban.entry.expired(now_millis)
        {
            return Some(LoginRefusal::IpBanned(ban.entry.clone()));
        }
        None
    }

    /// 官方 `MinecraftServer.kickUnlistedPlayers` 的条件。
    pub fn kicks_unlisted(&self) -> bool {
        self.enforce_whitelist && self.using_whitelist
    }

    /// `/op` 新建的条目。命令只在玩家还不是 OP 时走到这里。
    pub fn op_player(&mut self, player: NameAndId) {
        self.ops.insert(
            player.uuid,
            OpListEntry {
                name: player.name,
                entry: OpEntry {
                    level: Self::OP_LEVEL,
                    bypasses_player_limit: false,
                },
            },
        );
    }

    pub fn deop_player(&mut self, uuid: Uuid) -> bool {
        self.ops.remove(&uuid).is_some()
    }
}

/// 登录被拒绝的原因。带封禁项时客户端看到的是带原因和到期时间的翻译文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginRefusal {
    PlayerBanned(BanEntry),
    IpBanned(BanEntry),
    NotWhitelisted,
    /// 满员（或绕过人数上限检查）。
    ServerFull,
}

/// 一条翻译文本和它的参数。Login 阶段的断开原因是 JSON 文本，
/// Play 阶段是 NBT 文本组件，两边都用这个中间形式拼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalText {
    pub key: &'static str,
    pub args: Vec<RefusalArg>,
    /// 到期时间的第二行，官方 `append` 到第一行后面。
    pub expiration: Option<(&'static str, String)>,
}

/// 翻译参数：字面文本或者另一条翻译文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalArg {
    Text(String),
    Translate(&'static str),
}

impl LoginRefusal {
    /// 官方 `PlayerList.canPlayerLogin` 产出的文本结构。
    pub fn text(&self) -> RefusalText {
        match self {
            Self::PlayerBanned(ban) => ban_text(
                "multiplayer.disconnect.banned.reason",
                "multiplayer.disconnect.banned.expiration",
                ban,
            ),
            Self::IpBanned(ban) => ban_text(
                "multiplayer.disconnect.banned_ip.reason",
                "multiplayer.disconnect.banned_ip.expiration",
                ban,
            ),
            Self::NotWhitelisted => RefusalText {
                key: "multiplayer.disconnect.not_whitelisted",
                args: Vec::new(),
                expiration: None,
            },
            Self::ServerFull => RefusalText {
                key: "multiplayer.disconnect.server_full",
                args: Vec::new(),
                expiration: None,
            },
        }
    }
}

fn ban_text(key: &'static str, expiration_key: &'static str, ban: &BanEntry) -> RefusalText {
    let reason = match &ban.reason {
        Some(reason) => RefusalArg::Text(reason.clone()),
        None => RefusalArg::Translate("multiplayer.disconnect.banned.reason.default"),
    };
    RefusalText {
        key,
        args: vec![reason],
        expiration: ban
            .expires
            .as_ref()
            .map(|expires| (expiration_key, expires.clone())),
    }
}

/// 启动时从磁盘读进来的全部名单，交给世界线程；之后每次变更也会发布一份。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminSnapshot {
    pub ops: BTreeMap<Uuid, OpListEntry>,
    pub player_bans: BTreeMap<Uuid, BanListEntry>,
    pub ip_bans: BTreeMap<String, BanListEntry>,
    pub whitelist: BTreeMap<Uuid, NameAndId>,
    /// `/whitelist on|off` 改的开关，登录检查也读它。
    pub using_whitelist: bool,
}

impl AdminSnapshot {
    /// 变成世界线程里的权威副本。`enforce-whitelist` 和空闲超时按官方默认值，
    /// 重启后恢复；`/whitelist on|off` 的开关在名单文件之外，重启恢复 false。
    pub fn into_state(self) -> AdminState {
        AdminState {
            ops: self.ops,
            player_bans: self.player_bans,
            ip_bans: self.ip_bans,
            whitelist: self.whitelist,
            using_whitelist: self.using_whitelist,
            ..AdminState::default()
        }
    }
}

impl AdminSnapshot {
    /// 官方 `PlayerList.canPlayerLogin` 的顺序：玩家封禁、白名单、IP 封禁。
    /// 连接层只用这一份只读快照，满员由连接层自己判断。
    pub fn login_refusal(&self, uuid: Uuid, ip: &str, now_millis: i64) -> Option<LoginRefusal> {
        if let Some(ban) = self.player_bans.get(&uuid)
            && !ban.entry.expired(now_millis)
        {
            return Some(LoginRefusal::PlayerBanned(ban.entry.clone()));
        }
        if self.using_whitelist
            && !self.ops.contains_key(&uuid)
            && !self.whitelist.contains_key(&uuid)
        {
            return Some(LoginRefusal::NotWhitelisted);
        }
        if let Some(ban) = self.ip_bans.get(ip)
            && !ban.entry.expired(now_millis)
        {
            return Some(LoginRefusal::IpBanned(ban.entry.clone()));
        }
        None
    }
}

impl AdminState {
    /// 发布给连接层的只读快照。
    pub fn snapshot(&self) -> AdminSnapshot {
        AdminSnapshot {
            ops: self.ops.clone(),
            player_bans: self.player_bans.clone(),
            ip_bans: self.ip_bans.clone(),
            whitelist: self.whitelist.clone(),
            using_whitelist: self.using_whitelist,
        }
    }
}

/// 登录准入判断到期时间用的当前时刻，单位毫秒。
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// 官方 `BanListEntry.DATE_FORMAT` 是 `yyyy-MM-dd HH:mm:ss Z`。
/// 服务端用本地时区写，读入时按同样的格式解析；这里的实现用 UTC 的 `+0000`。
pub const BAN_DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

/// 把毫秒时间戳写成官方的日期文本。天数换算用 civil_from_days，不需要额外依赖。
pub fn ban_date(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let within = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} +0000",
        within / 3600,
        within / 60 % 60,
        within % 60
    )
}

/// 官方日期文本到毫秒。解析不了时返回 `None`，和官方 `SimpleDateFormat.parse`
/// 失败后回到 `new Date()` / `null` 的宽松处理不同，见 README。
pub fn parse_ban_date(text: &str) -> Option<i64> {
    if !text.is_char_boundary(20) || text.len() < 20 {
        return None;
    }
    let number = |start: usize, end: usize| -> Option<i64> { text.get(start..end)?.parse().ok() };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let offset = parse_zone_offset(text.get(20..)?)?;
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3600 + minute * 60 + second - offset) * 1000)
}

fn parse_zone_offset(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("Z") {
        return Some(0);
    }
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = digits.chars().filter(|c| c.is_ascii_digit()).collect();
    let (hours, minutes) = match digits.len() {
        2 => (digits.parse::<i64>().ok()?, 0),
        4 => (
            digits.get(..2)?.parse().ok()?,
            digits.get(2..)?.parse().ok()?,
        ),
        _ => return None,
    };
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Howard Hinnant 的 civil calendar 换算，1970-01-01 是第 0 天。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month_prime = month + i64::from(month > 2) * (-3) + i64::from(month <= 2) * 9;
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

/// 世界线程交给连接层的名单变更。每个文件只在真的变了时写。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdminChanges {
    pub ops: bool,
    pub player_bans: bool,
    pub ip_bans: bool,
    pub whitelist: bool,
    /// `white-list` 开关变了：重启不保留，但登录用的快照要跟上。
    pub using_whitelist: bool,
}

impl AdminChanges {
    pub fn any(&self) -> bool {
        self.ops || self.player_bans || self.ip_bans || self.whitelist || self.using_whitelist
    }

    pub fn merge(&mut self, other: Self) {
        self.ops |= other.ops;
        self.player_bans |= other.player_bans;
        self.ip_bans |= other.ip_bans;
        self.whitelist |= other.whitelist;
        self.using_whitelist |= other.using_whitelist;
    }
}

/// 换掉登录用的快照，并写回变动过的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUpdate {
    pub snapshot: AdminSnapshot,
    pub changes: AdminChanges,
}

/// 世界线程发给连接层写盘任务的消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminMessage {
    Update(AdminUpdate),
    /// `/whitelist reload` 让连接层重新读盘，再把结果送回来。
    ReloadWhitelist,
}

/// `/whitelist reload` 读回来的白名单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhitelistReload {
    pub entries: BTreeMap<Uuid, NameAndId>,
}

/// `World::with_admin` 的启动参数。`/whitelist reload` 的读盘结果走
/// `Inbound::WhitelistReload`，和别的世界消息同一条队列。
#[derive(Debug)]
pub struct AdminInit {
    pub snapshot: AdminSnapshot,
    pub messages: Option<mpsc::Sender<AdminMessage>>,
}

impl AdminInit {
    /// 取走启动参数，留一份空的占位。`Server::run` 只调用一次。
    pub fn split(&mut self) -> Self {
        Self {
            snapshot: std::mem::take(&mut self.snapshot),
            messages: self.messages.take(),
        }
    }
}

/// 官方 26.3 `version.json` 里没有单独进 `mc-bamboo-data` 的固定字段。
pub const SERIES: &str = "main";
pub const BUILD_TIME: &str = "2026-09-15T11:20:48+00:00";
pub const PACK_RESOURCE: &str = "97.1";
pub const PACK_DATA: &str = "121.0";

/// 集成测试用的桥：把命令树序列化成 `commands.json` 的形状。
#[doc(hidden)]
pub mod tests {
    use mc_bamboo_command::{ArgumentType, NodeKind, StringKind};

    /// 返回 `{"op": {...}, "deop": {...}, ...}`，只包含这次加的根节点。
    pub fn command_tree_json() -> String {
        let graph = &crate::command::commands().graph;
        let roots = [
            "op",
            "deop",
            "ban",
            "ban-ip",
            "banlist",
            "pardon",
            "pardon-ip",
            "whitelist",
            "stop",
            "setidletimeout",
            "transfer",
            "version",
        ];
        let mut out = serde_json::Map::new();
        for name in roots {
            let node = graph.node(graph.root()).children.iter().copied().find(
                |&child| matches!(&graph.node(child).kind, NodeKind::Literal(text) if text == name),
            );
            if let Some(node) = node {
                out.insert(name.to_owned(), node_json(graph, node));
            }
        }
        serde_json::Value::Object(out).to_string()
    }

    fn node_json(graph: &mc_bamboo_command::CommandGraph, id: usize) -> serde_json::Value {
        let node = graph.node(id);
        let mut out = serde_json::Map::new();
        match &node.kind {
            NodeKind::Root => {
                out.insert("type".to_owned(), "root".into());
            }
            NodeKind::Literal(_) => {
                out.insert("type".to_owned(), "literal".into());
            }
            NodeKind::Argument { name: _, kind } => {
                out.insert("type".to_owned(), "argument".into());
                out.insert("parser".to_owned(), kind.parser_name().into());
                if let Some(properties) = argument_properties(kind) {
                    out.insert("properties".to_owned(), properties);
                }
            }
        }
        if node.executable {
            out.insert("executable".to_owned(), true.into());
        }
        if node.requires > 0 {
            let level = match node.requires {
                1 => "moderators",
                2 => "gamemasters",
                3 => "admins",
                _ => "owners",
            };
            out.insert(
                "permissions".to_owned(),
                serde_json::json!({
                    "type": "minecraft:require",
                    "permission": {
                        "type": "minecraft:command_level",
                        "level": level,
                    },
                }),
            );
        }
        if let Some(redirect) = node.redirect {
            out.insert("redirect".to_owned(), redirect.into());
        }
        if !node.children.is_empty() {
            let mut children = serde_json::Map::new();
            for &child in &node.children {
                let child_node = graph.node(child);
                let key = match &child_node.kind {
                    NodeKind::Literal(text) => text.clone(),
                    NodeKind::Argument { name, .. } => name.clone(),
                    NodeKind::Root => "root".to_owned(),
                };
                children.insert(key, node_json(graph, child));
            }
            out.insert("children".to_owned(), serde_json::Value::Object(children));
        }
        serde_json::Value::Object(out)
    }

    fn argument_properties(kind: &ArgumentType) -> Option<serde_json::Value> {
        Some(match *kind {
            ArgumentType::Integer { min, max } => {
                // 官方报告省略 IntegerArgumentType 默认的最大值。
                if max == i32::MAX {
                    serde_json::json!({ "min": min })
                } else {
                    serde_json::json!({ "min": min, "max": max })
                }
            }
            ArgumentType::String(StringKind::SingleWord) => {
                serde_json::json!({ "type": "word" })
            }
            ArgumentType::String(StringKind::Quotable) => {
                serde_json::json!({ "type": "phrase" })
            }
            ArgumentType::String(StringKind::Greedy) => {
                serde_json::json!({ "type": "greedy" })
            }
            ArgumentType::Entity {
                single,
                players_only,
            } => serde_json::json!({
                "type": if players_only { "players" } else { "entities" },
                "amount": if single { "single" } else { "multiple" },
            }),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    fn ban(expires: Option<&str>) -> BanEntry {
        BanEntry {
            created: "2026-09-15 11:20:48 +0000".to_owned(),
            source: "Server".to_owned(),
            expires: expires.map(str::to_owned),
            expires_millis: expires.and_then(parse_ban_date),
            reason: Some("testing".to_owned()),
        }
    }

    #[test]
    fn login_refusal_follows_the_official_order() {
        let uuid = Uuid::from_u128(1);
        let mut state = AdminState::default();
        state.player_bans.insert(
            uuid,
            BanListEntry {
                name: "Kitten".to_owned(),
                entry: ban(None),
            },
        );
        state.ip_bans.insert(
            "127.0.0.1".to_owned(),
            BanListEntry {
                name: "127.0.0.1".to_owned(),
                entry: ban(None),
            },
        );
        assert!(matches!(
            state.login_refusal(uuid, "127.0.0.1", 0),
            Some(LoginRefusal::PlayerBanned(_))
        ));
        state.player_bans.clear();
        assert!(matches!(
            state.login_refusal(uuid, "127.0.0.1", 0),
            Some(LoginRefusal::IpBanned(_))
        ));
    }

    #[test]
    fn whitelist_lets_ops_and_listed_players_through() {
        let listed = Uuid::from_u128(1);
        let op = Uuid::from_u128(2);
        let stranger = Uuid::from_u128(3);
        let mut state = AdminState {
            using_whitelist: true,
            ..Default::default()
        };
        state.whitelist.insert(
            listed,
            NameAndId {
                uuid: listed,
                name: "Listed".to_owned(),
            },
        );
        state.ops.insert(
            op,
            OpListEntry {
                name: "Op".to_owned(),
                entry: OpEntry {
                    level: 4,
                    bypasses_player_limit: false,
                },
            },
        );
        assert_eq!(state.login_refusal(listed, "127.0.0.1", 0), None);
        assert_eq!(state.login_refusal(op, "127.0.0.1", 0), None);
        assert_eq!(
            state.login_refusal(stranger, "127.0.0.1", 0),
            Some(LoginRefusal::NotWhitelisted)
        );
        assert!(!state.kicks_unlisted());
        state.enforce_whitelist = true;
        assert!(state.kicks_unlisted());
    }

    #[test]
    fn expired_bans_do_not_block_login() {
        let uuid = Uuid::from_u128(1);
        let mut state = AdminState::default();
        let past = "2020-01-01 00:00:00 +0000";
        state.player_bans.insert(
            uuid,
            BanListEntry {
                name: "Kitten".to_owned(),
                entry: ban(Some(past)),
            },
        );
        assert_eq!(
            state.login_refusal(uuid, "127.0.0.1", 1_800_000_000_000),
            None
        );
    }
}
