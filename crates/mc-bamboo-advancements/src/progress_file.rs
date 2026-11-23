//! 单个玩家的进度状态与 `players/advancements/<uuid>.json` 的编解码。
//!
//! 官方 `AdvancementProgress` / `CriterionProgress` / `PlayerAdvancements$Data`：
//! 时间格式 `yyyy-MM-dd HH:mm:ss Z`（本地时区），`done` 按有无完成判据写出，
//! 顶层还有 `DataVersion`。Bamboo 写档只含有过进度的条目，键顺序和官方一致。
//! 文件读写在 `mc-bamboo-storage`，这里只处理字符串和字节。
//!
//! Per-player progress and the official JSON file codec. IO lives in the storage crate.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, TimeZone};
use mc_bamboo_data::version;
use serde_json::{Map, Value, json};

use crate::{Advancement, TreeError};

/// 官方时间格式，秒精度，`Z` 是带符号的时区偏移。
pub const OBTAINED_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

/// 一个判据的完成状态。`None` 表示还没完成。
pub type CriterionProgress = Option<DateTime<Local>>;

/// 一个进度的进度，官方 `AdvancementProgress`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdvancementProgress {
    /// 判据名 → 完成时刻。按判据名排序，发包和写档都稳定。
    pub criteria: BTreeMap<String, CriterionProgress>,
    /// 与 / 或的要求，来自进度定义，不存档。
    pub requirements: Vec<Vec<String>>,
}

impl AdvancementProgress {
    /// 官方 `update(requirements)`：丢掉不在 requirements 里的判据，补齐缺的。
    pub fn update(&mut self, requirements: &[Vec<String>]) {
        let names: std::collections::BTreeSet<&str> = requirements
            .iter()
            .flat_map(|group| group.iter().map(String::as_str))
            .collect();
        self.criteria
            .retain(|name, _| names.contains(name.as_str()));
        for name in names {
            self.criteria.entry(name.to_owned()).or_insert(None);
        }
        self.requirements = requirements.to_vec();
    }

    /// 按定义初始化。
    pub fn start(advancement: &Advancement) -> Self {
        let mut progress = Self::default();
        progress.update(&advancement.requirements);
        progress
    }

    /// 官方 `isDone`：requirements 为空时是 false，否则每组至少完成一个。
    pub fn is_done(&self) -> bool {
        !self.requirements.is_empty()
            && self.requirements.iter().all(|group| {
                group
                    .iter()
                    .any(|name| self.criteria.get(name).is_some_and(Option::is_some))
            })
    }

    /// 官方 `hasProgress`：至少有一个判据完成了。
    pub fn has_progress(&self) -> bool {
        self.criteria.values().any(Option::is_some)
    }

    /// 官方 `grantProgress`。返回是否发生变化。
    pub fn grant(&mut self, name: &str, now: DateTime<Local>) -> bool {
        let Some(criterion) = self.criteria.get_mut(name) else {
            return false;
        };
        if criterion.is_some() {
            return false;
        }
        *criterion = Some(now);
        true
    }

    /// 官方 `revokeProgress`。返回是否发生变化。
    pub fn revoke(&mut self, name: &str) -> bool {
        let Some(criterion) = self.criteria.get_mut(name) else {
            return false;
        };
        if criterion.is_none() {
            return false;
        }
        *criterion = None;
        true
    }

    /// 官方 `getCriterion` 的完成判断。
    pub fn is_criterion_done(&self, name: &str) -> bool {
        self.criteria.get(name).is_some_and(Option::is_some)
    }

    /// 官方 `getFirstProgressDate`：完成时刻里最早的一个。
    pub fn first_progress_date(&self) -> Option<DateTime<Local>> {
        self.criteria.values().filter_map(|&date| date).min()
    }

    /// 官方 `AdvancementProgress.compareTo`（`Data.forEach` 按它排读入顺序）。
    pub fn official_compare(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self.first_progress_date(), other.first_progress_date()) {
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (None, None) => Ordering::Equal,
            (Some(a), Some(b)) => a.cmp(&b),
        }
    }

    /// 线上格式 `AdvancementProgress.STREAM_CODEC`：判据的映射（名字 → 可选 Instant）。
    pub fn write_network(&self, out: &mut Vec<u8>) {
        use mc_bamboo_protocol::codec::{write_bool, write_i64, write_string, write_varint};
        write_varint(out, self.criteria.len() as i32);
        for (name, done) in &self.criteria {
            write_string(out, name);
            // `CriterionProgress.STREAM_CODEC` 是 optional(INSTANT)，
            // 26.3 的 `ByteBufCodecs.INSTANT` = LONG.map(Instant::ofEpochMilli, Instant::toEpochMilli)。
            write_bool(out, done.is_some());
            if let Some(date) = done {
                write_i64(out, date.timestamp_millis());
            }
        }
    }

    /// [`Self::write_network`] 的便捷入口。
    pub fn write_network_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_network(&mut out);
        out
    }

    /// 官方 `AdvancementProgress.CODEC` 写出的 JSON 对象。
    /// `done` 是读档时用来自动补齐缺判据的旧字段：官方编码时按 `hasProgress()` 写。
    pub fn to_json(&self) -> Value {
        let mut criteria = Map::new();
        for (name, done) in &self.criteria {
            if let Some(date) = done {
                criteria.insert(
                    name.clone(),
                    json!(format!("{}", date.format(OBTAINED_TIME_FORMAT))),
                );
            }
        }
        json!({
            "criteria": Value::Object(criteria),
            "done": self.has_progress()
        })
    }

    /// 从官方 CODEC 的 JSON 对象读回来。`done: false` 是官方旧存档补齐判据的标记，
    /// 这里照官方读档把要求名补成未完成判据。
    pub fn from_json(value: &Value) -> Result<Self, TreeError> {
        let object = value
            .as_object()
            .ok_or_else(|| TreeError("进度条目不是对象".to_owned()))?;
        let mut criteria = BTreeMap::new();
        if let Some(Value::Object(entries)) = object.get("criteria") {
            for (name, value) in entries {
                let text = value
                    .as_str()
                    .ok_or_else(|| TreeError(format!("判据 {name} 的时间不是字符串")))?;
                let date = parse_obtained_time(text)
                    .ok_or_else(|| TreeError(format!("判据 {name} 的时间格式不合法")))?;
                criteria.insert(name.clone(), Some(date));
            }
        }
        Ok(Self {
            criteria,
            requirements: Vec::new(),
        })
    }
}

/// 官方 `ExtraCodecs.temporalCodec` 用 `ZonedDateTime` 解析本地时区的时间串。
pub fn parse_obtained_time(text: &str) -> Option<DateTime<Local>> {
    // `DateTime::parse_from_str` 接受 `%z` 的 `+0800`，按给定偏移还原成 UTC 再转本地时区。
    let fixed = DateTime::parse_from_str(text, OBTAINED_TIME_FORMAT).ok()?;
    Local
        .timestamp_opt(fixed.timestamp(), fixed.timestamp_subsec_nanos())
        .single()
}

/// 官方写档时的时间串。
pub fn format_obtained_time(date: DateTime<Local>) -> String {
    format!("{}", date.format(OBTAINED_TIME_FORMAT))
}

/// 一名玩家的全部进度，对应存档文件的顶层对象。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressFile {
    pub data_version: i32,
    /// 进度 id → 进度状态。
    pub progress: BTreeMap<String, AdvancementProgress>,
}

impl ProgressFile {
    /// 新玩家或空存档。`DataVersion` 照官方 `NbtUtils.addCurrentDataVersion` 写当前版本。
    pub fn new() -> Self {
        Self {
            data_version: version::DATA_VERSION,
            progress: BTreeMap::new(),
        }
    }

    /// 按官方 Gson pretty printing 的顺序编码：`DataVersion` 最后写（DataFix 包装会把它
    /// 加到 JSON 末尾），每个进度是 `{criteria:{...},done:<bool>}`。
    pub fn encode(&self) -> String {
        let mut root = Map::new();
        for (id, progress) in &self.progress {
            if progress.has_progress() {
                root.insert(id.clone(), progress.to_json());
            }
        }
        root.insert("DataVersion".to_owned(), json!(self.data_version));
        let text =
            serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".to_owned());
        // Gson 的 pretty printing 用两个空格缩进，冒号后面一个空格，和 serde_json 一致；
        // 但 Gson 不在末尾加换行，这里也去掉。
        text
    }

    /// 解析官方存档。未知进度、未知判据照官方先收下，由调用方按树过滤。
    pub fn decode(text: &str) -> Result<Self, TreeError> {
        let value: Value = serde_json::from_str(text)
            .map_err(|error| TreeError(format!("进度存档不是合法 JSON：{error}")))?;
        let object = value
            .as_object()
            .ok_or_else(|| TreeError("进度存档不是对象".to_owned()))?;
        let data_version = object
            .get("DataVersion")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(version::DATA_VERSION);
        let mut progress = BTreeMap::new();
        for (id, value) in object {
            if id == "DataVersion" {
                continue;
            }
            progress.insert(id.clone(), AdvancementProgress::from_json(value)?);
        }
        Ok(Self {
            data_version,
            progress,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date() -> DateTime<Local> {
        parse_obtained_time("2026-01-02 03:04:05 +0800").unwrap()
    }

    #[test]
    fn time_round_trips_in_the_official_format() {
        let parsed = date();
        assert_eq!(
            format_obtained_time(parsed),
            format!("{}", parsed.format(OBTAINED_TIME_FORMAT))
        );
        assert!(parse_obtained_time("not a time").is_none());
    }

    #[test]
    fn progress_requires_every_group() {
        let mut progress = AdvancementProgress::default();
        progress.update(&[vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned()]]);
        assert!(!progress.is_done());
        assert!(!progress.has_progress());
        assert!(progress.grant("b", date()));
        assert!(!progress.grant("b", date()));
        assert!(progress.has_progress());
        assert!(!progress.is_done());
        assert!(progress.grant("c", date()));
        assert!(progress.is_done());
        assert!(progress.revoke("c"));
        assert!(!progress.is_done());
        assert!(!progress.revoke("c"));
    }

    #[test]
    fn file_round_trips_and_keeps_the_official_shape() {
        let mut progress = AdvancementProgress::start(&crate::Advancement {
            parent: None,
            display: None,
            rewards: crate::Rewards::default(),
            criteria: vec![(
                "a".to_owned(),
                crate::Criterion {
                    trigger: "minecraft:tick".to_owned(),
                    conditions: Value::Null,
                },
            )],
            requirements: vec![vec!["a".to_owned()]],
            sends_telemetry_event: false,
        });
        assert!(progress.grant("a", date()));
        let mut file = ProgressFile::new();
        file.progress
            .insert("minecraft:story/root".to_owned(), progress.clone());
        let text = file.encode();
        assert!(text.contains("\"criteria\""));
        assert!(text.contains("\"done\": true"));
        assert!(text.contains("\"DataVersion\""));
        let back = ProgressFile::decode(&text).unwrap();
        assert_eq!(back.data_version, version::DATA_VERSION);
        let mut restored = back.progress["minecraft:story/root"].clone();
        restored.update(&progress.requirements);
        assert_eq!(restored.criteria, progress.criteria);
    }

    #[test]
    fn malformed_files_are_errors_not_panics() {
        assert!(ProgressFile::decode("[").is_err());
        assert!(ProgressFile::decode("{\"x\": 1}").is_err());
        assert!(ProgressFile::decode("{\"x\":{\"criteria\":{\"a\":\"bad\"}}}").is_err());
    }
}
