//! 工作目录里的四份管理名单：`ops.json`、`banned-players.json`、
//! `banned-ips.json`、`whitelist.json`。
//!
//! 启动时同步读一次（和官方 `StoredUserList.load` 一样），之后世界线程把变更
//! 发布回来，由连接层这侧的任务写盘，世界线程不做 IO。
//! 文件格式照官方 `StoredUserList.save`：pretty-printed JSON 数组，字段顺序和
//! 官方 `serialize` 一致。写盘先写临时文件再 rename，同一个文件按顺序写。
//!
//! The four admin lists in the working directory. They are read once at startup and
//! written back from the connection side whenever the world thread publishes a change.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use mc_bamboo_game::admin::{
    AdminChanges, AdminSnapshot, BanEntry, BanListEntry, NameAndId, OpListEntry, parse_ban_date,
};
use serde_json::{Map, Value};
use tracing::warn;
use uuid::Uuid;

pub(crate) use crate::ops::OPS_FILE;
pub(crate) const PLAYER_BANS_FILE: &str = "banned-players.json";
pub(crate) const IP_BANS_FILE: &str = "banned-ips.json";
pub(crate) const WHITELIST_FILE: &str = "whitelist.json";

/// 四份文件所在的目录。`Server` 启动时从这里读，写盘也写回这里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListDirectory(PathBuf);

impl ListDirectory {
    pub(crate) fn at(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// 启动时读完四份文件。缺文件当成空名单，坏文件警告后当成空名单。
    pub(crate) fn load(&self) -> AdminSnapshot {
        AdminSnapshot {
            ops: parse_ops(&read_text(&self.file(OPS_FILE))),
            player_bans: parse_player_bans(&read_array(&self.file(PLAYER_BANS_FILE))),
            ip_bans: parse_ip_bans(&read_array(&self.file(IP_BANS_FILE))),
            whitelist: parse_whitelist(&read_array(&self.file(WHITELIST_FILE))),
            using_whitelist: false,
        }
    }

    /// 只重读白名单，`/whitelist reload` 用。
    pub(crate) fn load_whitelist(&self) -> BTreeMap<Uuid, NameAndId> {
        parse_whitelist(&read_array(&self.file(WHITELIST_FILE)))
    }

    /// 把变了的文件写回。每个文件独立：一次改名失败不影响别的文件。
    pub(crate) fn save(&self, snapshot: &AdminSnapshot, changes: AdminChanges) {
        if changes.ops {
            write_pretty(&self.file(OPS_FILE), &ops_json(&snapshot.ops));
        }
        if changes.player_bans {
            write_pretty(
                &self.file(PLAYER_BANS_FILE),
                &player_bans_json(&snapshot.player_bans),
            );
        }
        if changes.ip_bans {
            write_pretty(&self.file(IP_BANS_FILE), &ip_bans_json(&snapshot.ip_bans));
        }
        if changes.whitelist {
            write_pretty(
                &self.file(WHITELIST_FILE),
                &whitelist_json(&snapshot.whitelist),
            );
        }
    }
}

fn read_text(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            warn!(%error, path = %path.display(), "could not read an admin list");
            String::new()
        }
    }
}

fn read_array(path: &Path) -> Vec<Value> {
    let text = read_text(path);
    if text.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<Value>>(&text) {
        Ok(entries) => entries,
        Err(error) => {
            warn!(%error, path = %path.display(), "an admin list is not a JSON array");
            Vec::new()
        }
    }
}

/// 官方 `ServerOpListEntry(JsonObject)` 的解析。`ops.rs` 里的 `Ops::parse` 已经
/// 实现了 Gson 兼容的 `level` 规则和「整个文件作废」的判断，这里复用它，再补上
/// 写回文件要用的玩家名。
fn parse_ops(text: &str) -> BTreeMap<Uuid, OpListEntry> {
    let Some(ops) = crate::ops::Ops::parse(text) else {
        return BTreeMap::new();
    };
    let mut entries = BTreeMap::new();
    let Ok(values) = serde_json::from_str::<Vec<Value>>(text) else {
        return entries;
    };
    for value in &values {
        let Some(object) = value.as_object() else {
            continue;
        };
        let Some(player) = name_and_id(object) else {
            continue;
        };
        if let Some(op) = ops.get(player.uuid) {
            entries.insert(
                player.uuid,
                OpListEntry {
                    name: player.name,
                    entry: op,
                },
            );
        }
    }
    entries
}

/// Gson 的 `getAsString` 接受任何原始值。
fn primitive_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// 官方 `NameAndId.fromJson`。没有 `uuid`/`name` 或 UUID 写错时跳过这一条。
fn name_and_id(object: &Map<String, Value>) -> Option<NameAndId> {
    let uuid = Uuid::parse_str(object.get("uuid")?.as_str()?).ok()?;
    let name = primitive_string(object.get("name")?)?;
    Some(NameAndId { uuid, name })
}

fn parse_player_bans(entries: &[Value]) -> BTreeMap<Uuid, BanListEntry> {
    let mut bans = BTreeMap::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(player) = name_and_id(object) else {
            continue;
        };
        bans.insert(
            player.uuid,
            BanListEntry {
                name: player.name,
                entry: ban_entry(object),
            },
        );
    }
    bans
}

fn parse_ip_bans(entries: &[Value]) -> BTreeMap<String, BanListEntry> {
    let mut bans = BTreeMap::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(ip) = object.get("ip").and_then(Value::as_str) else {
            continue;
        };
        bans.insert(
            ip.to_owned(),
            BanListEntry {
                name: ip.to_owned(),
                entry: ban_entry(object),
            },
        );
    }
    bans
}

fn parse_whitelist(entries: &[Value]) -> BTreeMap<Uuid, NameAndId> {
    let mut list = BTreeMap::new();
    for entry in entries {
        if let Some(object) = entry.as_object()
            && let Some(player) = name_and_id(object)
        {
            list.insert(player.uuid, player);
        }
    }
    list
}

/// 官方 `BanListEntry(JsonObject)`。日期格式 `yyyy-MM-dd HH:mm:ss Z`。
fn ban_entry(object: &Map<String, Value>) -> BanEntry {
    let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    let created = text("created").unwrap_or_default();
    let source = text("source").unwrap_or_else(|| "(Unknown)".to_owned());
    let expires = text("expires").filter(|value| value != "forever");
    let expires_millis = expires.as_deref().and_then(parse_ban_date);
    BanEntry {
        created,
        source,
        expires,
        expires_millis,
        reason: text("reason"),
    }
}

/// 官方 `ServerOpListEntry.serialize` 的字段顺序。
fn ops_json(ops: &BTreeMap<Uuid, OpListEntry>) -> Value {
    Value::Array(
        ops.iter()
            .map(|(uuid, op)| {
                let mut object = Map::new();
                object.insert("uuid".to_owned(), Value::String(uuid.to_string()));
                object.insert("name".to_owned(), Value::String(op.name.clone()));
                object.insert("level".to_owned(), Value::from(op.entry.level));
                object.insert(
                    "bypassesPlayerLimit".to_owned(),
                    Value::Bool(op.entry.bypasses_player_limit),
                );
                Value::Object(object)
            })
            .collect(),
    )
}

/// 官方 `BanListEntry.serialize` 的字段顺序，加上玩家名单的 `uuid`/`name`
/// 或 IP 名单的 `ip`。
fn ban_fields(ban: &BanListEntry, object: &mut Map<String, Value>) {
    object.insert(
        "created".to_owned(),
        Value::String(ban.entry.created.clone()),
    );
    object.insert("source".to_owned(), Value::String(ban.entry.source.clone()));
    object.insert(
        "expires".to_owned(),
        Value::String(
            ban.entry
                .expires
                .clone()
                .unwrap_or_else(|| "forever".to_owned()),
        ),
    );
    object.insert(
        "reason".to_owned(),
        ban.entry.reason.clone().map_or(Value::Null, Value::String),
    );
}

fn player_bans_json(bans: &BTreeMap<Uuid, BanListEntry>) -> Value {
    Value::Array(
        bans.iter()
            .map(|(uuid, ban)| {
                let mut object = Map::new();
                object.insert("uuid".to_owned(), Value::String(uuid.to_string()));
                object.insert("name".to_owned(), Value::String(ban.name.clone()));
                ban_fields(ban, &mut object);
                Value::Object(object)
            })
            .collect(),
    )
}

fn ip_bans_json(bans: &BTreeMap<String, BanListEntry>) -> Value {
    Value::Array(
        bans.iter()
            .map(|(ip, ban)| {
                let mut object = Map::new();
                object.insert("ip".to_owned(), Value::String(ip.clone()));
                ban_fields(ban, &mut object);
                Value::Object(object)
            })
            .collect(),
    )
}

fn whitelist_json(entries: &BTreeMap<Uuid, NameAndId>) -> Value {
    Value::Array(
        entries
            .values()
            .map(|entry| {
                let mut object = Map::new();
                object.insert("uuid".to_owned(), Value::String(entry.uuid.to_string()));
                object.insert("name".to_owned(), Value::String(entry.name.clone()));
                Value::Object(object)
            })
            .collect(),
    )
}

/// 官方 `StoredUserList.save` 用 Gson 的 pretty printing：两个空格缩进。
/// `serde_json::Map` 默认按键排序，这里自己按插入顺序写，字段顺序才和官方一致。
/// 先写临时文件再 rename，避免断电留下半个文件。
fn write_pretty(path: &Path, value: &Value) {
    let text = match pretty_gson(value, 0) {
        Ok(text) => text,
        Err(error) => {
            warn!(%error, path = %path.display(), "could not serialize an admin list");
            return;
        }
    };
    let temp = path.with_extension("json.tmp");
    if let Err(error) = fs::write(&temp, format!("{text}\n")) {
        warn!(%error, path = %path.display(), "could not write an admin list");
        return;
    }
    if let Err(error) = fs::rename(&temp, path) {
        warn!(%error, path = %path.display(), "could not replace an admin list");
        let _ = fs::remove_file(&temp);
    }
}

#[cfg(test)]
mod tests;

/// 按官方 Gson pretty printing 的形状写 JSON：两个空格缩进，对象字段按插入顺序。
fn pretty_gson(value: &Value, depth: usize) -> Result<String, serde_json::Error> {
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    Ok(match value {
        Value::Array(entries) => {
            if entries.is_empty() {
                "[]".to_owned()
            } else {
                let mut out = String::from("[\n");
                for (index, entry) in entries.iter().enumerate() {
                    out.push_str(&child_indent);
                    out.push_str(&pretty_gson(entry, depth + 1)?);
                    if index + 1 < entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push(']');
                out
            }
        }
        Value::Object(entries) => {
            if entries.is_empty() {
                "{}".to_owned()
            } else {
                let mut out = String::from("{\n");
                for (index, (key, entry)) in entries.iter().enumerate() {
                    out.push_str(&child_indent);
                    out.push_str(&serde_json::to_string(key)?);
                    out.push_str(": ");
                    out.push_str(&pretty_gson(entry, depth + 1)?);
                    if index + 1 < entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push('}');
                out
            }
        }
        other => serde_json::to_string(other)?,
    })
}
