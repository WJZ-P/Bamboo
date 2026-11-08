//! 工作目录里的 `ops.json`。
//!
//! 官方的 `PlayerList.OPLIST_FILE` 就是工作目录下的这个文件，启动时由
//! `StoredUserList.load` 读一次。这里在 [`Server::new`](crate::Server::new) 里同步读完，
//! 世界线程只拿到查表结果，不做 IO。
//!
//! 文件不存在时当作没有 OP 的服务器，和官方一样不报警。整个文件读不出来或者不是
//! JSON 数组时警告一次，然后当作空表：权限判断宁可按最低等级走，也不能让服务端起不来。

use std::{collections::HashMap, fs, io, path::Path};

use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

/// 官方 `PlayerList.OPLIST_FILE`。
pub(crate) const OPS_FILE: &str = "ops.json";

/// `ops.json` 里的一项。被列出来的玩家都是 OP，`level` 只决定权限等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpEntry {
    /// 官方 `PermissionLevel` 的 ID，0 到 4。
    pub(crate) level: u8,
    pub(crate) bypasses_player_limit: bool,
}

/// 按 UUID 查的 OP 表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ops(HashMap<Uuid, OpEntry>);

impl Ops {
    /// 从 `path` 读。文件不存在时不报警，其他读取错误警告一次。
    pub(crate) fn load(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                warn!(%error, path = %path.display(), "could not read the op list");
                return Self::default();
            }
        };
        match Self::parse(&text) {
            Some(ops) => ops,
            None => {
                warn!(path = %path.display(), "the op list is not valid ops.json");
                Self::default()
            }
        }
    }

    /// 解析 `ops.json` 的内容。整个文件不合法时返回 `None`。
    ///
    /// 单条记录缺 `uuid`、`name` 或者 UUID 写法不对时跳过，官方
    /// `NameAndId.fromJson` 返回 `null` 后 `StoredUserList.load` 也是直接忽略。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let entries: Vec<Value> = serde_json::from_str(text).ok()?;
        let mut ops = HashMap::with_capacity(entries.len());
        for entry in &entries {
            let Some((uuid, op)) = parse_entry(entry)? else {
                continue;
            };
            // 官方按顺序 `put`，同一个 UUID 出现多次时最后一项生效。
            ops.insert(uuid, op);
        }
        Some(Self(ops))
    }

    pub(crate) fn get(&self, uuid: Uuid) -> Option<OpEntry> {
        self.0.get(&uuid).copied()
    }
}

/// 官方 `ServerOpListEntry(JsonObject)`。
///
/// 返回 `None` 表示整个文件不能要：条目不是对象、或者 `level` 不是数字时，
/// 官方会抛异常并放弃整个列表。返回 `Some(None)` 表示这一项被跳过，官方在
/// `NameAndId.fromJson` 返回 `null` 后也是直接忽略。
fn parse_entry(entry: &Value) -> Option<Option<(Uuid, OpEntry)>> {
    let object = entry.as_object()?;
    // 官方先读 `level` 再判断玩家能不能识别，所以坏等级会拖垮整份列表。
    let level = match object.get("level") {
        Some(level) => parse_level(level)?,
        None => 0,
    };
    let bypasses_player_limit = object
        .get("bypassesPlayerLimit")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let Some(uuid) = object.get("uuid").and_then(Value::as_str) else {
        return Some(None);
    };
    let Ok(uuid) = Uuid::parse_str(uuid) else {
        return Some(None);
    };
    // 官方 `NameAndId.fromJson` 要求 `name` 也在，否则这一项被丢掉。
    if !object.get("name").is_some_and(is_primitive) {
        return Some(None);
    }

    // 官方 `PermissionLevel.byId` 用 `ByIdMap.continuous(..., CLAMP)`，越界会夹到 0 和 4。
    Some(Some((
        uuid,
        OpEntry {
            level: level.clamp(0, 4) as u8,
            bypasses_player_limit,
        },
    )))
}

/// 官方 `JsonPrimitive.getAsInt`：数字直接取整，字符串交给 `Integer.parseInt`。
///
/// 小数截断、字符串必须是整数、布尔和 null 直接报错，都是 Gson 的行为。
fn parse_level(level: &Value) -> Option<i64> {
    match level {
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Some(integer)
            } else {
                number.as_f64().map(|float| float.trunc() as i64)
            }
        }
        Value::String(text) => text.parse::<i32>().ok().map(i64::from),
        _ => None,
    }
}

/// Gson 的 `getAsString` 能接受任何原始值，对象、数组和 null 不行。
fn is_primitive(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Array(_) | Value::Object(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEVE: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
    const ALEX: &str = "853c80ef-3c37-49fd-aa49-938b674adae6";

    fn parse(text: &str) -> Ops {
        Ops::parse(text).expect("这是合法的 ops.json")
    }

    #[test]
    fn an_empty_list_has_no_ops() {
        assert!(parse("[]").0.is_empty());
    }

    #[test]
    fn listed_players_carry_their_level() {
        let text = format!(
            r#"[{{"uuid":"{STEVE}","name":"Steve","level":2,"bypassesPlayerLimit":true}}]"#
        );
        let ops = parse(&text);
        assert_eq!(
            ops.get(Uuid::parse_str(STEVE).unwrap()),
            Some(OpEntry {
                level: 2,
                bypasses_player_limit: true,
            })
        );
    }

    #[test]
    fn missing_level_is_the_lowest_permission_but_still_an_op() {
        let text = format!(r#"[{{"uuid":"{STEVE}","name":"Steve"}}]"#);
        let ops = parse(&text);
        assert_eq!(
            ops.get(Uuid::parse_str(STEVE).unwrap()),
            Some(OpEntry {
                level: 0,
                bypasses_player_limit: false,
            })
        );
    }

    #[test]
    fn levels_are_clamped_like_the_official_enum() {
        for (level, expected) in [(-3, 0), (0, 0), (4, 4), (9, 4)] {
            let text = format!(r#"[{{"uuid":"{STEVE}","name":"Steve","level":{level}}}]"#);
            assert_eq!(
                parse(&text).get(Uuid::parse_str(STEVE).unwrap()),
                Some(OpEntry {
                    level: expected,
                    bypasses_player_limit: false,
                }),
                "level {level}"
            );
        }
    }

    #[test]
    fn gson_style_levels_are_accepted() {
        // 小数截断、数字字符串按 `Integer.parseInt` 解析。
        for (level, expected) in [("2.9", 2), ("\"3\"", 3), ("-0.5", 0)] {
            let text = format!(r#"[{{"uuid":"{STEVE}","name":"Steve","level":{level}}}]"#);
            assert_eq!(
                parse(&text)
                    .get(Uuid::parse_str(STEVE).unwrap())
                    .map(|op| op.level),
                Some(expected),
                "level {level}"
            );
        }
    }

    #[test]
    fn entries_without_a_usable_name_and_id_are_skipped() {
        let text = format!(
            r#"[
                {{"uuid":"{STEVE}","level":4}},
                {{"name":"Alex","level":4}},
                {{"uuid":"not-a-uuid","name":"Alex","level":4}},
                {{"uuid":"{ALEX}","name":"Alex","level":1}}
            ]"#
        );
        let ops = parse(&text);
        assert_eq!(ops.get(Uuid::parse_str(STEVE).unwrap()), None);
        assert_eq!(
            ops.get(Uuid::parse_str(ALEX).unwrap()).map(|op| op.level),
            Some(1)
        );
    }

    #[test]
    fn the_last_entry_for_a_uuid_wins() {
        let text = format!(
            r#"[
                {{"uuid":"{STEVE}","name":"Steve","level":1}},
                {{"uuid":"{STEVE}","name":"Steve","level":3}}
            ]"#
        );
        assert_eq!(
            parse(&text)
                .get(Uuid::parse_str(STEVE).unwrap())
                .map(|op| op.level),
            Some(3)
        );
    }

    #[test]
    fn the_level_is_read_before_the_player_id() {
        // 官方先把 `level` 转成整数，再判断 `uuid`/`name`，所以这条记录连
        // “跳过”都轮不到，整份文件直接作废。
        assert_eq!(Ops::parse(r#"[{"level":"high"}]"#), None);
    }

    #[test]
    fn a_broken_file_yields_nothing_at_all() {
        assert_eq!(Ops::parse("not json"), None);
        assert_eq!(Ops::parse(r#"{"uuid":"x"}"#), None);
        assert_eq!(Ops::parse(r#"[42]"#), None);
        assert_eq!(
            Ops::parse(&format!(
                r#"[{{"uuid":"{STEVE}","name":"Steve","level":"high"}}]"#
            )),
            None
        );
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let path = std::env::temp_dir().join("bamboo-no-such-ops.json");
        let _ = fs::remove_file(&path);
        assert!(Ops::load(&path).0.is_empty());
    }

    #[test]
    fn loading_reads_the_currency_file() {
        let path = std::env::temp_dir().join("bamboo-ops-test.json");
        let text = format!(r#"[{{"uuid":"{STEVE}","name":"Steve","level":3}}]"#);
        fs::write(&path, text).unwrap();
        let ops = Ops::load(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(
            ops.get(Uuid::parse_str(STEVE).unwrap()).map(|op| op.level),
            Some(3)
        );
    }
}
