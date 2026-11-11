//! `level.dat` 的 gzip NBT 编解码，根是 `{ Data: { ... } }`。
//!
//! 字段照 26.3 `PrimaryLevelData.setTagData`（`createTag` 的私有实现）实际写出的
//! 内容：`LevelVersion`、`LevelName`、`GameType`、`spawn`、`Time`、`LastPlayed`、
//! `version_history`、`difficulty_settings`、`ServerBrands`、`WasModded`、
//! `WorldDataConfiguration.MAP_CODEC` 等等。读一侧对应静态的
//! `PrimaryLevelData.parse`。这一层不读写世界目录，也不接进游戏。
//!
//! Bamboo 不认识的键按 D9 原样保留：根、`Data` 和建模过的子复合标签
//! （`Version`、`difficulty_settings`、`spawn`）里的多余键都写回去。
//!
//! Persistent `level.dat` as gzip NBT with a `{ Data: { ... } }` root.
//!
//! The fields follow what `PrimaryLevelData.setTagData` actually writes in 26.3; the
//! read side mirrors the static `PrimaryLevelData.parse`. This module is pure data and
//! codec only. Unknown keys are preserved verbatim (D9).

use mc_bamboo_data::version;
use mc_bamboo_nbt::{Accounter, Compression, FileError, Tag, read_compressed, write_compressed};

/// NBT 存档版本，官方 `PrimaryLevelData` 的 `19133`。
pub const LEVEL_FORMAT_VERSION: i32 = 19133;
/// 官方 `LevelData.RespawnData.DEFAULT` 的维度。
pub const OVERWORLD: &str = "minecraft:overworld";

/// 编解码失败的原因。输入当成不可信数据，所有分支返回 `Result`，不 panic。
#[derive(Debug, thiserror::Error)]
pub enum LevelDatError {
    #[error("level.dat NBT file error: {0}")]
    File(#[from] FileError),
    #[error("level.dat root name is {0:?}, expected an empty string")]
    RootName(String),
    #[error("level.dat is missing the Data compound")]
    MissingData,
    #[error("level.dat field {name}: {message}")]
    Malformed { name: &'static str, message: String },
}

/// 官方 `LevelVersion` 的 `Version` 复合标签。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelVersion {
    /// `SharedConstants.getCurrentVersion().name()`。
    pub name: String,
    /// `WorldVersion.dataVersion().version()`，等于 [`version::DATA_VERSION`]。
    pub id: i32,
    /// `!WorldVersion.stable()`。
    pub snapshot: bool,
    /// `WorldVersion.dataVersion().series()`。
    pub series: String,
}

impl Default for LevelVersion {
    fn default() -> Self {
        Self {
            name: version::GAME_VERSION.to_owned(),
            id: version::DATA_VERSION,
            snapshot: false,
            series: "main".to_owned(),
        }
    }
}

/// 官方 `LevelData.RespawnData`：`GlobalPos` 加 `yaw` / `pitch`。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnData {
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for SpawnData {
    fn default() -> Self {
        Self {
            dimension: OVERWORLD.to_owned(),
            x: 0,
            y: 0,
            z: 0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// 官方 `LevelSettings.DifficultySettings.CODEC`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifficultySettings {
    pub difficulty: String,
    pub hardcore: bool,
    pub locked: bool,
}

impl Default for DifficultySettings {
    /// Bamboo 的 `WorldState` 默认难度是 EASY；官方
    /// `LevelSettings.DifficultySettings.DEFAULT` 是 NORMAL（存档里缺字段时用）。
    fn default() -> Self {
        Self {
            difficulty: "easy".to_owned(),
            hardcore: false,
            locked: false,
        }
    }
}

/// 官方 `LevelSettings.parse` 缺 `difficulty_settings` 时的默认值（NORMAL）。
fn official_difficulty_default() -> DifficultySettings {
    DifficultySettings {
        difficulty: "normal".to_owned(),
        hardcore: false,
        locked: false,
    }
}

/// 官方 `DataPackConfig.CODEC` 的 `Enabled` / `Disabled` 两个字符串列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPacks {
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

impl Default for DataPacks {
    /// `DataPackConfig.DEFAULT`：`Enabled = ["vanilla"]`、`Disabled = []`。
    fn default() -> Self {
        Self {
            enabled: vec!["vanilla".to_owned()],
            disabled: Vec::new(),
        }
    }
}

/// `level.dat` 的 `Data` 复合标签。`Default` 是官方新建世界时的默认值。
///
/// `LastPlayed` 是毫秒时间戳，官方 `createTag` 每次写都刷新成现在；这里只是数据，
/// 由调用方决定值。`server_brands` 对应 `ServerBrands`，
/// `removed_features` 为空时官方不写这个键。
///
/// The `Data` compound of `level.dat`. `LastPlayed` is a plain field here; the
/// caller decides its value. Unknown keys are preserved verbatim (D9).
#[derive(Debug, Clone, PartialEq)]
pub struct LevelData {
    pub server_brands: Vec<String>,
    pub was_modded: bool,
    pub removed_features: Vec<String>,
    pub version: LevelVersion,
    pub version_history: Vec<i32>,
    pub data_version: i32,
    pub game_type: i32,
    pub spawn: SpawnData,
    pub time: i64,
    pub last_played: i64,
    pub level_name: String,
    pub format_version: i32,
    pub allow_commands: bool,
    pub initialized: bool,
    pub difficulty: DifficultySettings,
    pub singleplayer_uuid: Option<[i32; 4]>,
    /// `None` 表示官方的默认配置，`lenientOptionalFieldOf` 编码时不写 `DataPacks`。
    pub data_packs: Option<DataPacks>,
    pub enabled_features: Vec<String>,
    /// `Data` 里本层不认识的键，重新编码时原样写回。
    pub preserved: Vec<(String, Tag)>,
    /// 建模过的子复合标签里不认识的键，键名带完整路径（例如 `Version/bamboo:extra`）。
    pub nested_preserved: Vec<(String, Tag)>,
    /// 根复合标签里除 `Data` 之外不认识的键（`{ Data: ..., bamboo:root: ... }`）。
    pub root_preserved: Vec<(String, Tag)>,
}

impl Default for LevelData {
    fn default() -> Self {
        Self {
            server_brands: vec!["vanilla".to_owned()],
            was_modded: false,
            removed_features: Vec::new(),
            version: LevelVersion::default(),
            version_history: vec![version::DATA_VERSION],
            data_version: version::DATA_VERSION,
            game_type: 1, // Bamboo 现有行为是创造模式
            spawn: SpawnData::default(),
            time: 0,
            last_played: 0,
            level_name: "Bamboo".to_owned(),
            format_version: LEVEL_FORMAT_VERSION,
            allow_commands: false,
            initialized: true,
            difficulty: DifficultySettings::default(),
            singleplayer_uuid: None,
            data_packs: Some(DataPacks::default()),
            enabled_features: vec!["minecraft:vanilla".to_owned()],
            preserved: Vec::new(),
            nested_preserved: Vec::new(),
            root_preserved: Vec::new(),
        }
    }
}

/// 官方 `PrimaryLevelData.setTagData` 会写的 `Data` 顶层键。
const DATA_KEYS: &[&str] = &[
    "ServerBrands",
    "WasModded",
    "removed_features",
    "Version",
    "version_history",
    "DataVersion",
    "GameType",
    "spawn",
    "Time",
    "LastPlayed",
    "LevelName",
    "version",
    "allowCommands",
    "initialized",
    "difficulty_settings",
    "singleplayer_uuid",
    "DataPacks",
    "enabled_features",
];

impl LevelData {
    /// 编成官方 `NbtIo.writeCompressed` 的 gzip 文件字节，根名字是空字符串。
    pub fn to_gzip(&self) -> Result<Vec<u8>, LevelDatError> {
        write_compressed("", &self.encode_root(), Compression::Gzip).map_err(LevelDatError::File)
    }

    /// 编成 `{ Data: { ... } }` 根复合标签，根里不认识的键原样写回。
    pub fn encode_root(&self) -> Tag {
        let mut root = vec![("Data".to_owned(), self.encode())];
        for (key, value) in &self.root_preserved {
            if key != "Data" && !root.iter().any(|(own, _)| own == key) {
                root.push((key.clone(), value.clone()));
            }
        }
        Tag::Compound(root)
    }

    /// 从官方 gzip 文件字节解码。不认识的键原样保留，坏字段返回 `Err`。
    pub fn from_gzip(bytes: &[u8]) -> Result<Self, LevelDatError> {
        let (name, entries) =
            read_compressed(bytes, Compression::Gzip, Accounter::uncompressed_quota())?;
        if !name.is_empty() {
            return Err(LevelDatError::RootName(name));
        }
        Self::decode_root(&Tag::Compound(entries))
    }

    /// 从未压缩的带名字 NBT 字节解码（对拍和单元测试用）。
    pub fn from_named(bytes: &[u8]) -> Result<Self, LevelDatError> {
        let (name, entries) = mc_bamboo_nbt::read_named(bytes, Accounter::uncompressed_quota())?;
        if !name.is_empty() {
            return Err(LevelDatError::RootName(name));
        }
        Self::decode_root(&Tag::Compound(entries))
    }

    /// 从根复合标签（`{ Data: ... }`）解码。
    pub fn decode_root(root: &Tag) -> Result<Self, LevelDatError> {
        let entries = compound(root, "root")?;
        let Some(data) = field_of(entries, "Data") else {
            return Err(LevelDatError::MissingData);
        };
        let mut data = Self::decode(data)?;
        data.root_preserved = entries
            .iter()
            .filter(|(key, _)| key != "Data")
            .cloned()
            .collect();
        Ok(data)
    }

    /// 编成 `Data` 复合标签。
    pub fn encode(&self) -> Tag {
        let mut entries = Vec::new();
        push(
            &mut entries,
            "ServerBrands",
            Tag::List(
                self.server_brands
                    .iter()
                    .map(|name| Tag::String(name.clone()))
                    .collect(),
            ),
        );
        push(
            &mut entries,
            "WasModded",
            Tag::Byte(i8::from(self.was_modded)),
        );
        if !self.removed_features.is_empty() {
            push(
                &mut entries,
                "removed_features",
                Tag::List(
                    self.removed_features
                        .iter()
                        .map(|name| Tag::String(name.clone()))
                        .collect(),
                ),
            );
        }
        push(&mut entries, "Version", self.version_tag());
        push(
            &mut entries,
            "version_history",
            Tag::List(
                self.version_history
                    .iter()
                    .map(|id| Tag::Int(*id))
                    .collect(),
            ),
        );
        push(&mut entries, "DataVersion", Tag::Int(self.data_version));
        push(&mut entries, "GameType", Tag::Int(self.game_type));
        push(&mut entries, "spawn", self.spawn_tag());
        push(&mut entries, "Time", Tag::Long(self.time));
        push(&mut entries, "LastPlayed", Tag::Long(self.last_played));
        push(
            &mut entries,
            "LevelName",
            Tag::String(self.level_name.clone()),
        );
        push(&mut entries, "version", Tag::Int(self.format_version));
        push(
            &mut entries,
            "allowCommands",
            Tag::Byte(i8::from(self.allow_commands)),
        );
        push(
            &mut entries,
            "initialized",
            Tag::Byte(i8::from(self.initialized)),
        );
        push(&mut entries, "difficulty_settings", self.difficulty_tag());
        if let Some(uuid) = self.singleplayer_uuid {
            push(
                &mut entries,
                "singleplayer_uuid",
                Tag::IntArray(uuid.to_vec()),
            );
        }
        // `WorldDataConfiguration.MAP_CODEC`：`lenientOptionalFieldOf(name, default)`，
        // 值等于默认对象时整个键不写，非默认时写 codec 编出的复合标签 / 列表。
        if let Some(packs) = &self.data_packs {
            push(&mut entries, "DataPacks", Self::data_packs_tag(packs));
        }
        for (path, value) in &self.nested_preserved {
            let Some((parent, key)) = path.split_once('/') else {
                continue;
            };
            let Some((_, Tag::Compound(nested))) =
                entries.iter_mut().find(|(name, _)| name == parent)
            else {
                continue;
            };
            if nested.iter().any(|(name, _)| name == key) {
                continue;
            }
            nested.push((key.to_owned(), value.clone()));
        }
        for (key, value) in &self.preserved {
            if !DATA_KEYS.contains(&key.as_str()) && !entries.iter().any(|(own, _)| own == key) {
                push(&mut entries, key, value.clone());
            }
        }
        Tag::Compound(entries)
    }

    /// 从 `Data` 复合标签解码，缺字段按 `PrimaryLevelData.parse` 的默认值。
    pub fn decode(data: &Tag) -> Result<Self, LevelDatError> {
        let entries = compound(data, "Data")?;
        let mut level = Self {
            server_brands: string_list(entries, "ServerBrands", Vec::new())?,
            ..Self::default()
        };
        level.was_modded = bool_or(entries, "WasModded", false)?;
        level.removed_features = string_list(entries, "removed_features", Vec::new())?;
        if let Some(tag) = field_of(entries, "Version") {
            level.version = decode_version(tag)?;
        }
        level.version_history = int_list(entries, "version_history", Vec::new())?;
        level.data_version = int_or(entries, "DataVersion", version::DATA_VERSION)?;
        level.game_type = int_or(entries, "GameType", 0)?;
        if let Some(tag) = field_of(entries, "spawn") {
            level.spawn = decode_spawn(tag)?;
        }
        level.time = long_or(entries, "Time", 0)?;
        level.last_played = long_or(entries, "LastPlayed", 0)?;
        level.level_name = string_or(entries, "LevelName", "")?;
        level.format_version = int_or(entries, "version", 0)?;
        level.allow_commands = bool_or(entries, "allowCommands", false)?;
        level.initialized = bool_or(entries, "initialized", true)?;
        level.difficulty = match field_of(entries, "difficulty_settings") {
            Some(tag) => decode_difficulty(tag)?,
            None => official_difficulty_default(),
        };
        level.singleplayer_uuid = match field_of(entries, "singleplayer_uuid") {
            Some(Tag::IntArray(values)) if values.len() == 4 => {
                Some([values[0], values[1], values[2], values[3]])
            }
            Some(_) => return Err(bad("singleplayer_uuid", "expected 4 ints")),
            None => None,
        };
        level.data_packs = match field_of(entries, "DataPacks") {
            Some(tag) => Some(decode_data_packs(tag)?),
            None => None,
        };
        level.enabled_features = string_list(
            entries,
            "enabled_features",
            vec!["minecraft:vanilla".to_owned()],
        )?;

        level.preserved = entries
            .iter()
            .filter(|(key, _)| !DATA_KEYS.contains(&key.as_str()))
            .cloned()
            .collect();
        for (name, known) in [
            ("Version", &["Name", "Id", "Snapshot", "Series"][..]),
            ("difficulty_settings", &["difficulty", "hardcore", "locked"]),
            ("spawn", &["dimension", "pos", "yaw", "pitch"]),
        ] {
            if let Some(Tag::Compound(nested)) = field_of(entries, name) {
                for (key, value) in nested {
                    if !known.contains(&key.as_str()) {
                        level
                            .nested_preserved
                            .push((format!("{name}/{key}"), value.clone()));
                    }
                }
            }
        }
        Ok(level)
    }

    fn version_tag(&self) -> Tag {
        let v = &self.version;
        Tag::Compound(vec![
            ("Name".to_owned(), Tag::String(v.name.clone())),
            ("Id".to_owned(), Tag::Int(v.id)),
            ("Snapshot".to_owned(), Tag::Byte(i8::from(v.snapshot))),
            ("Series".to_owned(), Tag::String(v.series.clone())),
        ])
    }

    fn spawn_tag(&self) -> Tag {
        let s = &self.spawn;
        // `GlobalPos.MAP_CODEC`：`dimension` 字符串、`pos` 是 int array(3)。
        Tag::Compound(vec![
            ("dimension".to_owned(), Tag::String(s.dimension.clone())),
            ("pos".to_owned(), Tag::IntArray(vec![s.x, s.y, s.z])),
            ("yaw".to_owned(), Tag::Float(s.yaw)),
            ("pitch".to_owned(), Tag::Float(s.pitch)),
        ])
    }

    fn difficulty_tag(&self) -> Tag {
        let d = &self.difficulty;
        Tag::Compound(vec![
            ("difficulty".to_owned(), Tag::String(d.difficulty.clone())),
            ("hardcore".to_owned(), Tag::Byte(i8::from(d.hardcore))),
            ("locked".to_owned(), Tag::Byte(i8::from(d.locked))),
        ])
    }

    fn data_packs_tag(packs: &DataPacks) -> Tag {
        Tag::Compound(vec![
            (
                "Enabled".to_owned(),
                Tag::List(
                    packs
                        .enabled
                        .iter()
                        .map(|name| Tag::String(name.clone()))
                        .collect(),
                ),
            ),
            (
                "Disabled".to_owned(),
                Tag::List(
                    packs
                        .disabled
                        .iter()
                        .map(|name| Tag::String(name.clone()))
                        .collect(),
                ),
            ),
        ])
    }
}

fn decode_version(tag: &Tag) -> Result<LevelVersion, LevelDatError> {
    let entries = compound(tag, "Version")?;
    let default = LevelVersion::default();
    Ok(LevelVersion {
        name: string_or(entries, "Name", version::GAME_VERSION)?,
        id: int_or(entries, "Id", version::DATA_VERSION)?,
        snapshot: bool_or(entries, "Snapshot", default.snapshot)?,
        series: string_or(entries, "Series", "main")?,
    })
}

fn decode_spawn(tag: &Tag) -> Result<SpawnData, LevelDatError> {
    let entries = compound(tag, "spawn")?;
    let mut spawn = SpawnData {
        dimension: string_or(entries, "dimension", OVERWORLD)?,
        ..SpawnData::default()
    };
    if let Some(pos) = field_of(entries, "pos") {
        let values = match pos {
            Tag::IntArray(values) if values.len() == 3 => values.clone(),
            Tag::List(values) => values
                .iter()
                .map(|tag| {
                    number(tag)
                        .map(|value| value as i32)
                        .ok_or_else(|| bad("spawn.pos", "expected a number"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(bad("spawn.pos", "expected 3 ints")),
        };
        if values.len() != 3 {
            return Err(bad("spawn.pos", "expected 3 ints"));
        }
        spawn.x = values[0];
        spawn.y = values[1];
        spawn.z = values[2];
    }
    spawn.yaw = float_or(entries, "yaw", 0.0)?;
    spawn.pitch = float_or(entries, "pitch", 0.0)?;
    Ok(spawn)
}

fn decode_difficulty(tag: &Tag) -> Result<DifficultySettings, LevelDatError> {
    let entries = compound(tag, "difficulty_settings")?;
    Ok(DifficultySettings {
        difficulty: string_or(entries, "difficulty", "normal")?,
        hardcore: bool_or(entries, "hardcore", false)?,
        locked: bool_or(entries, "locked", false)?,
    })
}

fn decode_data_packs(tag: &Tag) -> Result<DataPacks, LevelDatError> {
    let entries = compound(tag, "DataPacks")?;
    Ok(DataPacks {
        enabled: string_list(entries, "Enabled", vec!["vanilla".to_owned()])?,
        disabled: string_list(entries, "Disabled", Vec::new())?,
    })
}

fn field_of<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn push(entries: &mut Vec<(String, Tag)>, key: &str, value: Tag) {
    entries.push((key.to_owned(), value));
}

fn bad(name: &'static str, message: impl Into<String>) -> LevelDatError {
    LevelDatError::Malformed {
        name,
        message: message.into(),
    }
}

fn compound<'a>(tag: &'a Tag, name: &'static str) -> Result<&'a [(String, Tag)], LevelDatError> {
    match tag {
        Tag::Compound(entries) => Ok(entries),
        _ => Err(bad(name, "expected a compound tag")),
    }
}

/// `NbtOps` 的宽松数值规则：任意数值标签都能解成目标数值。
fn number(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Byte(v) => Some(f64::from(*v)),
        Tag::Short(v) => Some(f64::from(*v)),
        Tag::Int(v) => Some(f64::from(*v)),
        Tag::Long(v) => Some(*v as f64),
        Tag::Float(v) => Some(f64::from(*v)),
        Tag::Double(v) => Some(*v),
        _ => None,
    }
}

fn int_or(
    entries: &[(String, Tag)],
    key: &'static str,
    default: i32,
) -> Result<i32, LevelDatError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value as i32)
            .ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn long_or(
    entries: &[(String, Tag)],
    key: &'static str,
    default: i64,
) -> Result<i64, LevelDatError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value as i64)
            .ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn float_or(
    entries: &[(String, Tag)],
    key: &'static str,
    default: f32,
) -> Result<f32, LevelDatError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value as f32)
            .ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn bool_or(
    entries: &[(String, Tag)],
    key: &'static str,
    default: bool,
) -> Result<bool, LevelDatError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value != 0.0)
            .ok_or_else(|| bad(key, "expected a number for a boolean")),
        None => Ok(default),
    }
}

fn string_or(
    entries: &[(String, Tag)],
    key: &'static str,
    default: &str,
) -> Result<String, LevelDatError> {
    match field_of(entries, key) {
        Some(Tag::String(value)) => Ok(value.clone()),
        Some(_) => Err(bad(key, "expected a string")),
        None => Ok(default.to_owned()),
    }
}

fn string_list(
    entries: &[(String, Tag)],
    key: &'static str,
    default: Vec<String>,
) -> Result<Vec<String>, LevelDatError> {
    let Some(tag) = field_of(entries, key) else {
        return Ok(default);
    };
    let Tag::List(values) = tag else {
        // `OptionalDynamic.asStream` / `orElseEmptyList`：不是列表时按空列表处理。
        return Ok(Vec::new());
    };
    values
        .iter()
        .map(|value| match value {
            Tag::String(text) => Ok(text.clone()),
            _ => Err(bad(key, "expected a list of strings")),
        })
        .collect()
}

fn int_list(
    entries: &[(String, Tag)],
    key: &'static str,
    default: Vec<i32>,
) -> Result<Vec<i32>, LevelDatError> {
    let Some(tag) = field_of(entries, key) else {
        return Ok(default);
    };
    let Tag::List(values) = tag else {
        return Ok(Vec::new());
    };
    values
        .iter()
        .map(|value| {
            number(value)
                .map(|number| number as i32)
                .ok_or_else(|| bad(key, "expected a list of ints"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LevelData {
        let mut level = LevelData {
            level_name: "竹 Bamboo".to_owned(),
            ..LevelData::default()
        };
        level.time = 123_456;
        level.last_played = 1_700_000_000_000;
        level.game_type = 2;
        level.spawn = SpawnData {
            dimension: "minecraft:overworld".to_owned(),
            x: 8,
            y: -60,
            z: -30_000_000,
            yaw: 179.5,
            pitch: -90.0,
        };
        level.difficulty = DifficultySettings {
            difficulty: "hard".to_owned(),
            hardcore: true,
            locked: true,
        };
        level.singleplayer_uuid = Some([1, 2, 3, 4]);
        level.version_history = vec![4000, 5023];
        level
    }

    #[test]
    fn root_level_unknown_keys_are_preserved() {
        let mut bytes = Vec::new();
        mc_bamboo_nbt::write_named(
            "",
            &Tag::Compound(vec![
                ("Data".to_owned(), sample().encode()),
                ("bamboo:root".to_owned(), Tag::String("keep".to_owned())),
            ]),
            &mut bytes,
        );
        let decoded = LevelData::from_named(&bytes).unwrap();
        assert_eq!(decoded.root_preserved.len(), 1);
        let root = decoded.encode_root();
        assert!(matches!(
            super::field_of(match &root { Tag::Compound(entries) => entries, _ => unreachable!() }, "bamboo:root"),
            Some(Tag::String(text)) if text == "keep"
        ));
    }

    #[test]
    fn gzip_round_trip() {
        let level = sample();
        let bytes = level.to_gzip().unwrap();
        let decoded = LevelData::from_gzip(&bytes).unwrap();
        assert_eq!(decoded, level);
        assert_eq!(decoded.to_gzip().unwrap(), bytes);
    }

    #[test]
    fn unknown_keys_are_preserved_at_every_level() {
        let mut root = Tag::Compound(vec![
            ("bamboo:root".to_owned(), Tag::Int(1)),
            ("Data".to_owned(), sample().encode()),
        ]);
        if let Tag::Compound(entries) = &mut root
            && let Some((_, Tag::Compound(data))) =
                entries.iter_mut().find(|(key, _)| key == "Data")
        {
            data.push(("bamboo:data".to_owned(), Tag::Long(-7)));
            if let Some((_, Tag::Compound(version))) =
                data.iter_mut().find(|(key, _)| key == "Version")
            {
                version.push(("bamboo:version".to_owned(), Tag::Byte(3)));
            }
        }
        let mut plain = Vec::new();
        mc_bamboo_nbt::write_named("", &root, &mut plain);
        let decoded = LevelData::from_named(&plain).unwrap();
        assert_eq!(decoded.preserved.len(), 1);
        assert_eq!(decoded.nested_preserved.len(), 1);
        assert_eq!(decoded.root_preserved.len(), 1);
        let encoded = decoded.encode_root();
        let Tag::Compound(entries) = &encoded else {
            panic!("root is a compound");
        };
        assert_eq!(field_of(entries, "bamboo:root"), Some(&Tag::Int(1)));
        let Some(Tag::Compound(data)) = field_of(entries, "Data") else {
            panic!("Data is a compound");
        };
        assert_eq!(field_of(data, "bamboo:data"), Some(&Tag::Long(-7)));
        let Some(Tag::Compound(version)) = field_of(data, "Version") else {
            panic!("Version is a compound");
        };
        assert_eq!(field_of(version, "bamboo:version"), Some(&Tag::Byte(3)));
    }

    #[test]
    fn defaults_for_missing_fields() {
        let level = LevelData::decode(&Tag::Compound(Vec::new())).unwrap();
        assert_eq!(level.data_version, version::DATA_VERSION);
        assert_eq!(level.format_version, 0); // `LevelVersion.parse` 的缺字段默认
        assert!(level.initialized);
        assert_eq!(level.game_type, 0);
        assert_eq!(level.spawn.dimension, OVERWORLD);
    }

    #[test]
    fn bad_types_return_errors() {
        let level = Tag::Compound(vec![("Time".to_owned(), Tag::String("x".to_owned()))]);
        assert!(LevelData::decode(&level).is_err());
        let root = Tag::Compound(vec![("Data".to_owned(), Tag::Int(3))]);
        assert!(LevelData::decode_root(&root).is_err());
        assert!(matches!(
            LevelData::decode_root(&Tag::Compound(Vec::new())),
            Err(LevelDatError::MissingData)
        ));
    }

    #[test]
    fn fuzz_truncation_flips_and_huge_lists_do_not_panic() {
        let bytes = sample().to_gzip().unwrap();
        for cut in 0..bytes.len().min(64) {
            let _ = LevelData::from_gzip(&bytes[..bytes.len() - cut]);
        }
        for index in 0..bytes.len().min(64) {
            let mut flipped = bytes.clone();
            flipped[index] ^= 0xa5;
            let _ = LevelData::from_gzip(&flipped);
        }
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..256 {
            let mut random = Vec::new();
            for _ in 0..128 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                random.push((state >> 24) as u8);
            }
            let _ = LevelData::from_named(&random);
        }
        let huge = Tag::Compound(vec![(
            "Data".to_owned(),
            Tag::Compound(vec![(
                "version_history".to_owned(),
                Tag::List(vec![Tag::Int(0); 8192]),
            )]),
        )]);
        let mut plain = Vec::new();
        mc_bamboo_nbt::write_named("", &huge, &mut plain);
        assert_eq!(
            LevelData::from_named(&plain).unwrap().version_history.len(),
            8192
        );
    }
}
