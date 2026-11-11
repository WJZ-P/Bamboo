//! 玩家数据 `players/data/<uuid>.dat` 的 gzip NBT 编解码。
//!
//! 字段名和类型照 26.3 的 `Entity.saveWithoutId`、`LivingEntity.addAdditionalSaveData`、
//! `Player.addAdditionalSaveData`、`ServerPlayer.addAdditionalSaveData` 以及
//! `NbtUtils.addCurrentDataVersion`。这一层是纯数据结构和编解码，不读写世界目录，
//! 也不接进进服 / 离服流程。
//!
//! Bamboo 不认识的键（官方新加的字段、其它版本或模组写的数据）按 D9 原样保留：
//! 顶层键和已知的子复合标签（本层目前是 `abilities`）里的多余键在
//! [`PlayerData::encode`] 时都写回去，编解码器自己写的键优先。物品条目不保留多余键，
//! 见 `components::archive::slots`。
//!
//! Persistent player data (`players/data/<uuid>.dat`) as gzip NBT.
//!
//! The field names and types follow `Entity.saveWithoutId` and the `addAdditionalSaveData`
//! chain in 26.3. This module is pure data and codec only: it neither touches the world
//! directory nor participates in the join/leave flow. Unknown keys are preserved verbatim.

use mc_bamboo_data::version;
use mc_bamboo_nbt::{Accounter, Compression, FileError, Tag, read_compressed, write_compressed};
use mc_bamboo_protocol::{
    components::archive::slots::{
        self, DecodedItems, ENDER_CHEST_SLOTS, INVENTORY_SLOTS, SlottedItem,
    },
    item::ItemStack,
};

/// 主世界的维度名，官方 `ServerPlayer.addAdditionalSaveData` 的 `Dimension`。
pub const OVERWORLD: &str = "minecraft:overworld";
/// `version` 里游戏模式合法值之外的兜底：官方 `ByIdMap` 的 ZERO 策略折回生存模式。
const DEFAULT_GAME_TYPE: i32 = 0;
/// 官方 `Player.getScore` 的默认值。
const DEFAULT_SCORE: i32 = 0;
/// 官方 `FoodData` 的默认饥饿值。
const DEFAULT_FOOD_LEVEL: i32 = 20;
/// 官方 `Abilities` 的默认速度。
const DEFAULT_FLY_SPEED: f32 = 0.05;
const DEFAULT_WALK_SPEED: f32 = 0.1;
/// 官方玩家血量（`LivingEntity` 的 `Health`）。
const DEFAULT_HEALTH: f32 = 20.0;
/// 官方 `Player.airSupply` 的默认值。
const DEFAULT_AIR: i16 = 300;

/// 编解码失败的原因。输入当成不可信数据，所有分支返回 `Result`，不 panic。
#[derive(Debug, thiserror::Error)]
pub enum PlayerDataError {
    #[error("player NBT file error: {0}")]
    File(#[from] FileError),
    #[error("player NBT root name is {0:?}, expected an empty string")]
    RootName(String),
    #[error("player data field {name}: {message}")]
    Malformed { name: &'static str, message: String },
    #[error("cannot encode an item: {0}")]
    Item(String),
}

/// 一名玩家跨重启保留的状态。`Default` 是官方新建玩家时的默认值。
///
/// * `uuid` 是 `UUIDUtil.CODEC` 写成的四项 int array；
/// * `dimension` 是完整注册名字符串；
/// * `game_type` 是 `GameType.LEGACY_ID_CODEC` 的 legacy id；
/// * `inventory` / `ender_items` 是带槽位的物品，槽位分别是
///   [`INVENTORY_SLOTS`] / [`ENDER_CHEST_SLOTS`] 容器里的下标；
/// * `inventory_problems` / `ender_problems` 是解码时跳过的条目，交给调用方记日志；
/// * `preserved` 是本层不认识的顶层键，重新编码时原样写回。
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerData {
    pub data_version: i32,
    pub uuid: [i32; 4],
    pub position: [f64; 3],
    pub motion: [f64; 3],
    pub rotation: [f32; 2],
    pub fall_distance: f64,
    pub fire: i16,
    pub air: i16,
    pub on_ground: bool,
    pub invulnerable: bool,
    pub portal_cooldown: i32,
    pub health: f32,
    pub hurt_time: i16,
    pub death_time: i16,
    pub absorption: f32,
    pub fall_flying: bool,
    pub dimension: String,
    pub game_type: i32,
    pub previous_game_type: Option<i32>,
    pub inventory: Vec<SlottedItem>,
    pub ender_items: Vec<SlottedItem>,
    pub selected_item_slot: i32,
    pub sleep_timer: i16,
    pub xp_p: f32,
    pub xp_level: i32,
    pub xp_total: i32,
    pub xp_seed: i32,
    pub score: i32,
    pub food_level: i32,
    pub food_tick_timer: i32,
    pub food_saturation: f32,
    pub food_exhaustion: f32,
    pub abilities: Abilities,
    pub seen_credits: bool,
    pub spawn_extra_particles_on_fall: bool,
    /// 解码时跳过的物品条目，按 `Inventory` 然后 `EnderItems` 的顺序。
    pub problems: Vec<slots::ItemProblem>,
    /// 本层不认识的顶层键（`CompoundTag` 的键序不算，按键名排序稳定输出）。
    pub preserved: Vec<(String, Tag)>,
    /// 官方有而 Bamboo 没有建模的子复合标签里多余的键，键名带完整路径。
    ///
    /// 例如 `abilities/bamboo:extra`。重新编码时按键名合并回去。
    pub nested_preserved: Vec<(String, Tag)>,
}

/// 官方 `Abilities$Packed.CODEC` 的七个字段。
#[derive(Debug, Clone, PartialEq)]
pub struct Abilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub may_fly: bool,
    pub instabuild: bool,
    pub may_build: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            may_build: true,
            fly_speed: DEFAULT_FLY_SPEED,
            walk_speed: DEFAULT_WALK_SPEED,
        }
    }
}

impl Abilities {
    /// 官方 `ServerPlayerGameMode` 按游戏模式设置的能力。
    pub fn for_game_type(game_type: i32) -> Self {
        let creative = game_type == 1;
        let spectator = game_type == 3;
        Self {
            invulnerable: creative || spectator,
            may_fly: creative || spectator,
            instabuild: creative,
            may_build: !spectator,
            flying: spectator,
            ..Self::default()
        }
    }
}

impl Default for PlayerData {
    fn default() -> Self {
        Self {
            data_version: version::DATA_VERSION,
            uuid: [0; 4],
            position: [0.0, 0.0, 0.0],
            motion: [0.0; 3],
            rotation: [0.0; 2],
            fall_distance: 0.0,
            fire: 0,
            air: DEFAULT_AIR,
            on_ground: false,
            invulnerable: false,
            portal_cooldown: 0,
            health: DEFAULT_HEALTH,
            hurt_time: 0,
            death_time: 0,
            absorption: 0.0,
            fall_flying: false,
            dimension: OVERWORLD.to_owned(),
            game_type: DEFAULT_GAME_TYPE,
            previous_game_type: None,
            inventory: Vec::new(),
            ender_items: Vec::new(),
            selected_item_slot: 0,
            sleep_timer: 0,
            xp_p: 0.0,
            xp_level: 0,
            xp_total: 0,
            xp_seed: 0,
            score: DEFAULT_SCORE,
            food_level: DEFAULT_FOOD_LEVEL,
            food_tick_timer: 0,
            food_saturation: 5.0,
            food_exhaustion: 0.0,
            abilities: Abilities::default(),
            seen_credits: false,
            spawn_extra_particles_on_fall: false,
            problems: Vec::new(),
            preserved: Vec::new(),
            nested_preserved: Vec::new(),
        }
    }
}

/// 本层会自己写的顶层键。其余顶层键一律进 `preserved`。
///
/// `CompoundTag` 里同名键后者覆盖前者；这里按最后一次出现的键取值。
const KNOWN_KEYS: &[&str] = &[
    "DataVersion",
    "Pos",
    "Motion",
    "Rotation",
    "fall_distance",
    "Fire",
    "Air",
    "OnGround",
    "Invulnerable",
    "PortalCooldown",
    "UUID",
    "Health",
    "HurtTime",
    "DeathTime",
    "AbsorptionAmount",
    "FallFlying",
    "Inventory",
    "SelectedItemSlot",
    "SleepTimer",
    "XpP",
    "XpLevel",
    "XpTotal",
    "XpSeed",
    "Score",
    "foodLevel",
    "foodTickTimer",
    "foodSaturationLevel",
    "foodExhaustionLevel",
    "abilities",
    "EnderItems",
    "Dimension",
    "playerGameType",
    "previousPlayerGameType",
    "seenCredits",
    "spawn_extra_particles_on_fall",
];

/// 本层建模的复合标签。这些标签里的多余键进 `nested_preserved`。
const NESTED_COMPOUNDS: &[&str] = &["abilities"];

impl PlayerData {
    /// 编成官方 `NbtIo.writeCompressed` 的 gzip 文件字节，根名字是空字符串。
    pub fn to_gzip(&self) -> Result<Vec<u8>, PlayerDataError> {
        write_compressed("", &self.encode(), Compression::Gzip).map_err(PlayerDataError::File)
    }

    /// 从官方 gzip 文件字节解码。不认识的键原样保留，坏字段返回 `Err`。
    pub fn from_gzip(bytes: &[u8]) -> Result<Self, PlayerDataError> {
        let (name, entries) =
            read_compressed(bytes, Compression::Gzip, Accounter::uncompressed_quota())?;
        if !name.is_empty() {
            return Err(PlayerDataError::RootName(name));
        }
        Self::decode(&Tag::Compound(entries))
    }

    /// 从未压缩的带名字 NBT 字节解码（对拍和单元测试用）。
    pub fn from_named(bytes: &[u8]) -> Result<Self, PlayerDataError> {
        let (name, entries) = mc_bamboo_nbt::read_named(bytes, Accounter::uncompressed_quota())?;
        if !name.is_empty() {
            return Err(PlayerDataError::RootName(name));
        }
        Self::decode(&Tag::Compound(entries))
    }

    /// 编成未压缩的根复合标签。
    pub fn encode(&self) -> Tag {
        let mut root = Vec::new();
        push(&mut root, "DataVersion", Tag::Int(self.data_version));
        push(
            &mut root,
            "Pos",
            Tag::List(
                self.position
                    .iter()
                    .map(|value| Tag::Double(*value))
                    .collect(),
            ),
        );
        push(
            &mut root,
            "Motion",
            Tag::List(
                self.motion
                    .iter()
                    .map(|value| Tag::Double(*value))
                    .collect(),
            ),
        );
        push(
            &mut root,
            "Rotation",
            Tag::List(
                self.rotation
                    .iter()
                    .map(|value| Tag::Float(*value))
                    .collect(),
            ),
        );
        push(&mut root, "fall_distance", Tag::Double(self.fall_distance));
        push(&mut root, "Fire", Tag::Short(self.fire));
        push(&mut root, "Air", Tag::Short(self.air));
        push(&mut root, "OnGround", Tag::Byte(i8::from(self.on_ground)));
        push(
            &mut root,
            "Invulnerable",
            Tag::Byte(i8::from(self.invulnerable)),
        );
        push(&mut root, "PortalCooldown", Tag::Int(self.portal_cooldown));
        push(&mut root, "UUID", Tag::IntArray(self.uuid.to_vec()));
        push(&mut root, "Health", Tag::Float(self.health));
        push(&mut root, "HurtTime", Tag::Short(self.hurt_time));
        push(&mut root, "DeathTime", Tag::Short(self.death_time));
        push(&mut root, "AbsorptionAmount", Tag::Float(self.absorption));
        push(
            &mut root,
            "FallFlying",
            Tag::Byte(i8::from(self.fall_flying)),
        );
        if let Ok(items) = slots::encode_list(&self.inventory) {
            push(&mut root, "Inventory", items);
        }
        push(
            &mut root,
            "SelectedItemSlot",
            Tag::Int(self.selected_item_slot),
        );
        push(&mut root, "SleepTimer", Tag::Short(self.sleep_timer));
        push(&mut root, "XpP", Tag::Float(self.xp_p));
        push(&mut root, "XpLevel", Tag::Int(self.xp_level));
        push(&mut root, "XpTotal", Tag::Int(self.xp_total));
        push(&mut root, "XpSeed", Tag::Int(self.xp_seed));
        push(&mut root, "Score", Tag::Int(self.score));
        push(&mut root, "foodLevel", Tag::Int(self.food_level));
        push(&mut root, "foodTickTimer", Tag::Int(self.food_tick_timer));
        push(
            &mut root,
            "foodSaturationLevel",
            Tag::Float(self.food_saturation),
        );
        push(
            &mut root,
            "foodExhaustionLevel",
            Tag::Float(self.food_exhaustion),
        );
        push(&mut root, "abilities", self.abilities_tag());
        if let Ok(items) = slots::encode_list(&self.ender_items) {
            push(&mut root, "EnderItems", items);
        }
        push(&mut root, "Dimension", Tag::String(self.dimension.clone()));
        push(&mut root, "playerGameType", Tag::Int(self.game_type));
        if let Some(previous) = self.previous_game_type {
            push(&mut root, "previousPlayerGameType", Tag::Int(previous));
        }
        push(
            &mut root,
            "seenCredits",
            Tag::Byte(i8::from(self.seen_credits)),
        );
        push(
            &mut root,
            "spawn_extra_particles_on_fall",
            Tag::Byte(i8::from(self.spawn_extra_particles_on_fall)),
        );
        // D9：不认识的顶层键原样写回；重名时以本层写出的键为准。
        for (key, value) in &self.preserved {
            if !KNOWN_KEYS.contains(&key.as_str()) && !root.iter().any(|(own, _)| own == key) {
                push(&mut root, key, value.clone());
            }
        }
        Tag::Compound(root)
    }

    /// 从根复合标签解码。缺字段用官方 `ValueInput.get*Or` 的默认值。
    pub fn decode(root: &Tag) -> Result<Self, PlayerDataError> {
        let entries = compound(root, "root")?;
        let mut data = Self::default();
        data.data_version = opt_int(entries, "DataVersion", version::DATA_VERSION)?;
        data.uuid = uuid(entries)?;
        data.position = doubles3(entries, "Pos")?;
        data.motion = doubles3(entries, "Motion")?;
        data.rotation = floats2(entries, "Rotation")?;
        data.fall_distance = opt_f64(entries, "fall_distance", 0.0)?;
        data.fire = opt_short(entries, "Fire", 0)?;
        data.air = opt_short(entries, "Air", DEFAULT_AIR)?;
        data.on_ground = opt_bool(entries, "OnGround", false)?;
        data.invulnerable = opt_bool(entries, "Invulnerable", false)?;
        data.portal_cooldown = opt_int(entries, "PortalCooldown", 0)?;
        data.health = opt_f32(entries, "Health", DEFAULT_HEALTH)?;
        data.hurt_time = opt_short(entries, "HurtTime", 0)?;
        data.death_time = opt_short(entries, "DeathTime", 0)?;
        data.absorption = opt_f32(entries, "AbsorptionAmount", 0.0)?;
        data.fall_flying = opt_bool(entries, "FallFlying", false)?;
        data.selected_item_slot = opt_int(entries, "SelectedItemSlot", 0)?;
        data.sleep_timer = opt_short(entries, "SleepTimer", 0)?;
        data.xp_p = opt_f32(entries, "XpP", 0.0)?;
        data.xp_level = opt_int(entries, "XpLevel", 0)?;
        data.xp_total = opt_int(entries, "XpTotal", 0)?;
        data.xp_seed = opt_int(entries, "XpSeed", data.xp_seed)?;
        data.score = opt_int(entries, "Score", DEFAULT_SCORE)?;
        data.food_level = opt_int(entries, "foodLevel", DEFAULT_FOOD_LEVEL)?;
        data.food_tick_timer = opt_int(entries, "foodTickTimer", 0)?;
        data.food_saturation = opt_f32(entries, "foodSaturationLevel", 5.0)?;
        data.food_exhaustion = opt_f32(entries, "foodExhaustionLevel", 0.0)?;
        data.dimension = opt_string(entries, "Dimension", OVERWORLD)?;
        data.game_type = opt_int(entries, "playerGameType", DEFAULT_GAME_TYPE)?;
        data.previous_game_type = match field_of(entries, "previousPlayerGameType") {
            Some(tag) => Some(as_int(tag, "previousPlayerGameType")?),
            None => None,
        };
        data.seen_credits = opt_bool(entries, "seenCredits", false)?;
        data.spawn_extra_particles_on_fall =
            opt_bool(entries, "spawn_extra_particles_on_fall", false)?;
        if let Some(tag) = field_of(entries, "abilities") {
            data.abilities = decode_abilities(tag)?;
        } else {
            data.abilities = Abilities::for_game_type(data.game_type);
        }

        let inventory = field_of(entries, "Inventory").cloned();
        let ender = field_of(entries, "EnderItems").cloned();
        data.inventory = match inventory {
            Some(tag) => decode_items(&tag, INVENTORY_SLOTS, &mut data.problems).items,
            None => Vec::new(),
        };
        data.ender_items = match ender {
            Some(tag) => decode_items(&tag, ENDER_CHEST_SLOTS, &mut data.problems).items,
            None => Vec::new(),
        };

        // D9：不认识的顶层键和建模不全的复合标签里的键都原样保留。
        data.preserved = entries
            .iter()
            .filter(|(key, _)| !KNOWN_KEYS.contains(&key.as_str()))
            .cloned()
            .collect();
        for name in NESTED_COMPOUNDS {
            if let Some(tag) = field_of(entries, name)
                && let Tag::Compound(nested) = tag
            {
                for (key, value) in nested {
                    if !abilities_known(key) {
                        data.nested_preserved
                            .push((format!("{name}/{key}"), value.clone()));
                    }
                }
            }
        }
        Ok(data)
    }

    fn abilities_tag(&self) -> Tag {
        let a = &self.abilities;
        let mut entries = vec![
            (
                "invulnerable".to_owned(),
                Tag::Byte(i8::from(a.invulnerable)),
            ),
            ("flying".to_owned(), Tag::Byte(i8::from(a.flying))),
            ("mayfly".to_owned(), Tag::Byte(i8::from(a.may_fly))),
            ("instabuild".to_owned(), Tag::Byte(i8::from(a.instabuild))),
            ("mayBuild".to_owned(), Tag::Byte(i8::from(a.may_build))),
            ("flySpeed".to_owned(), Tag::Float(a.fly_speed)),
            ("walkSpeed".to_owned(), Tag::Float(a.walk_speed)),
        ];
        for (path, value) in &self.nested_preserved {
            let Some((parent, key)) = path.split_once('/') else {
                continue;
            };
            if parent == "abilities"
                && !abilities_known(key)
                && !entries.iter().any(|(own, _)| own == key)
            {
                entries.push((key.to_owned(), value.clone()));
            }
        }
        Tag::Compound(entries)
    }
}

/// 官方 `Abilities$Packed.CODEC` 的七个键都是 `optionalAlwaysPresentFieldOf`，
/// 读可省、写必写；读的时候缺的项用 `Abilities` 的字段默认值。
fn abilities_known(key: &str) -> bool {
    matches!(
        key,
        "invulnerable" | "flying" | "mayfly" | "instabuild" | "mayBuild" | "flySpeed" | "walkSpeed"
    )
}

fn decode_abilities(tag: &Tag) -> Result<Abilities, PlayerDataError> {
    let entries = compound(tag, "abilities")?;
    let default = Abilities::default();
    Ok(Abilities {
        invulnerable: opt_bool(entries, "invulnerable", default.invulnerable)?,
        flying: opt_bool(entries, "flying", default.flying)?,
        may_fly: opt_bool(entries, "mayfly", default.may_fly)?,
        instabuild: opt_bool(entries, "instabuild", default.instabuild)?,
        may_build: opt_bool(entries, "mayBuild", default.may_build)?,
        fly_speed: opt_f32(entries, "flySpeed", default.fly_speed)?,
        walk_speed: opt_f32(entries, "walkSpeed", default.walk_speed)?,
    })
}

/// 解一个物品列表，把跳过的条目记进 `problems`。
fn decode_items(tag: &Tag, size: usize, problems: &mut Vec<slots::ItemProblem>) -> DecodedItems {
    let mut decoded = slots::decode_list(tag, size);
    problems.append(&mut decoded.problems);
    decoded
}

/// 一个复合标签里的字段（`CompoundTag` 键序无关，重名按最后一次出现）。
///
/// Looks up a field in a compound tag. Duplicate keys use the last one.
pub fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
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

fn bad(name: &'static str, message: impl Into<String>) -> PlayerDataError {
    PlayerDataError::Malformed {
        name,
        message: message.into(),
    }
}

fn compound<'a>(tag: &'a Tag, name: &'static str) -> Result<&'a [(String, Tag)], PlayerDataError> {
    match tag {
        Tag::Compound(entries) => Ok(entries),
        _ => Err(bad(name, "expected a compound tag")),
    }
}

/// 官方 `DynamicOps.getNumberValue`：任意数值标签都能解出目标数值。
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

fn as_int(tag: &Tag, name: &'static str) -> Result<i32, PlayerDataError> {
    number(tag)
        .map(|value| value as i32)
        .ok_or_else(|| bad(name, "expected a number"))
}

fn opt_int(
    entries: &[(String, Tag)],
    key: &'static str,
    default: i32,
) -> Result<i32, PlayerDataError> {
    match field_of(entries, key) {
        Some(tag) => as_int(tag, key),
        None => Ok(default),
    }
}

fn opt_short(
    entries: &[(String, Tag)],
    key: &'static str,
    default: i16,
) -> Result<i16, PlayerDataError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value as i16)
            .ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn opt_f32(
    entries: &[(String, Tag)],
    key: &'static str,
    default: f32,
) -> Result<f32, PlayerDataError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value as f32)
            .ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn opt_f64(
    entries: &[(String, Tag)],
    key: &'static str,
    default: f64,
) -> Result<f64, PlayerDataError> {
    match field_of(entries, key) {
        Some(tag) => number(tag).ok_or_else(|| bad(key, "expected a number")),
        None => Ok(default),
    }
}

fn opt_bool(
    entries: &[(String, Tag)],
    key: &'static str,
    default: bool,
) -> Result<bool, PlayerDataError> {
    match field_of(entries, key) {
        Some(tag) => number(tag)
            .map(|value| value != 0.0)
            .ok_or_else(|| bad(key, "expected a number for a boolean")),
        None => Ok(default),
    }
}

fn opt_string(
    entries: &[(String, Tag)],
    key: &'static str,
    default: &str,
) -> Result<String, PlayerDataError> {
    match field_of(entries, key) {
        Some(Tag::String(value)) => Ok(value.clone()),
        Some(_) => Err(bad(key, "expected a string")),
        None => Ok(default.to_owned()),
    }
}

fn uuid(entries: &[(String, Tag)]) -> Result<[i32; 4], PlayerDataError> {
    let Some(tag) = field_of(entries, "UUID") else {
        return Ok([0; 4]);
    };
    let Tag::IntArray(values) = tag else {
        return Err(bad("UUID", "expected an int array"));
    };
    if values.len() != 4 {
        return Err(bad("UUID", "expected 4 ints"));
    }
    Ok([values[0], values[1], values[2], values[3]])
}

/// `Vec3.CODEC` / `Vec2.CODEC`：`Codec.DOUBLE.listOf()` 加 `Util.fixedSize(3)`，
/// 但 `NbtOps` 也接受三种数字数组。长度不对返回 `Err`。
fn doubles3(entries: &[(String, Tag)], key: &'static str) -> Result<[f64; 3], PlayerDataError> {
    let values = numeric_list(entries, key)?;
    if values.len() != 3 {
        return Err(bad(key, "expected 3 doubles"));
    }
    Ok([values[0], values[1], values[2]])
}

fn floats2(entries: &[(String, Tag)], key: &'static str) -> Result<[f32; 2], PlayerDataError> {
    let values = numeric_list(entries, key)?;
    if values.len() != 2 {
        return Err(bad(key, "expected 2 floats"));
    }
    Ok([values[0] as f32, values[1] as f32])
}

fn numeric_list(entries: &[(String, Tag)], key: &'static str) -> Result<Vec<f64>, PlayerDataError> {
    let default_len = if key == "Rotation" { 2 } else { 3 };
    let Some(tag) = field_of(entries, key) else {
        return Ok(vec![0.0; default_len]);
    };
    let values: Vec<Tag> = match tag {
        Tag::List(values) => values.clone(),
        Tag::ByteArray(values) => values.iter().map(|v| Tag::Byte(*v)).collect(),
        Tag::IntArray(values) => values.iter().map(|v| Tag::Int(*v)).collect(),
        Tag::LongArray(values) => values.iter().map(|v| Tag::Long(*v)).collect(),
        _ => return Err(bad(key, "expected a list of numbers")),
    };
    values
        .iter()
        .map(|tag| number(tag).ok_or_else(|| bad(key, "expected a number")))
        .collect()
}

/// 从游戏层的公开值造一份玩家存档（只是转换函数，不接进进服 / 离服流程）。
///
/// 这里只接受必要的字段，避免存档层依赖游戏层的私有类型。没有对应运行时状态的字段
/// 用官方默认值：血量恒 20、饥饿 / 经验 / 能力从游戏模式推。速度（`Motion`）和摔落
/// 距离（`fall_distance`）游戏层也有，但要保存它们的调用方在拿到结果后自己填。
impl PlayerData {
    pub fn from_runtime_parts(parts: &RuntimeParts<'_>) -> Self {
        let mut data = Self {
            uuid: uuid_to_ints(parts.uuid),
            ..Self::default()
        };
        data.position = parts.position;
        data.rotation = parts.rotation;
        data.on_ground = parts.on_ground;
        data.dimension = OVERWORLD.to_owned();
        data.game_type = parts.game_type;
        data.abilities = Abilities::for_game_type(parts.game_type);
        data.selected_item_slot = parts.selected_slot;
        // 菜单槽 5..8 是装备，9..35 是背包，36..44 是快捷栏，45 是副手。
        // 官方 `Inventory.items` 的顺序是快捷栏 0..8、背包 9..35、装备 36..39、副手 40。
        const MENU_SLOTS: usize = 46;
        let mut inventory = Vec::new();
        for (menu, stack) in parts.menu_slots.iter().enumerate().take(MENU_SLOTS) {
            if stack.is_empty() {
                continue;
            }
            let Some(slot) = menu_to_disk(menu) else {
                continue;
            };
            inventory.push(SlottedItem {
                slot,
                stack: stack.clone(),
            });
        }
        data.inventory = inventory;
        data.ender_items = parts
            .ender_slots
            .iter()
            .enumerate()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| SlottedItem {
                slot: slot as i32,
                stack: stack.clone(),
            })
            .collect();
        data
    }
}

/// [`PlayerData::from_runtime_parts`] 的输入，只含游戏层已经公开的值。
pub struct RuntimeParts<'a> {
    pub uuid: [u8; 16],
    pub position: [f64; 3],
    pub rotation: [f32; 2],
    pub on_ground: bool,
    pub game_type: i32,
    pub selected_slot: i32,
    /// `Inventory::slots` 的 46 个菜单槽。
    pub menu_slots: &'a [ItemStack],
    /// `EnderChest::slots` 的 27 格。
    pub ender_slots: &'a [ItemStack],
}

fn menu_to_disk(slot: usize) -> Option<i32> {
    // 官方 `Inventory.items` 的下标：0..8 快捷栏、9..35 背包、36..39 盔甲、40 副手。
    // Bamboo 的 `InventoryMenu` 槽位：`menu_slot(i)` 把 0..8 挪到 36..44，
    // 9..35 原样，5..8 是盔甲（逆序），45 是副手，0..4 是合成格不存。
    let disk = match slot {
        1..=4 => return None, // 合成输入
        5..=8 => 36 + (8 - slot),
        9..=35 => slot,
        36..=44 => slot - 36,
        45 => 40,
        _ => return None,
    };
    Some(disk as i32)
}

fn uuid_to_ints(bytes: [u8; 16]) -> [i32; 4] {
    let mut out = [0_i32; 4];
    for (index, chunk) in bytes.chunks(4).enumerate() {
        out[index] = i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items::STONE;

    fn sample() -> PlayerData {
        let mut data = PlayerData {
            uuid: [1, 2, 3, 4],
            ..PlayerData::default()
        };
        data.position = [1.5, -62.0, 1e300];
        data.rotation = [-179.5, 89.0];
        data.on_ground = true;
        data.game_type = 1;
        data.abilities = Abilities::for_game_type(1);
        data.inventory = vec![
            SlottedItem {
                slot: 0,
                stack: ItemStack::new(STONE, 1),
            },
            SlottedItem {
                slot: 35,
                stack: ItemStack::new(STONE, 99),
            },
        ];
        data.ender_items = vec![SlottedItem {
            slot: 26,
            stack: ItemStack::new(STONE, 7),
        }];
        data.selected_item_slot = 8;
        data
    }

    #[test]
    fn gzip_round_trip() {
        let data = sample();
        let bytes = data.to_gzip().unwrap();
        let decoded = PlayerData::from_gzip(&bytes).unwrap();
        // 浮点按位相等：这里的坐标都是能精确表示的数。
        assert_eq!(decoded.position, data.position);
        assert_eq!(decoded.inventory, data.inventory);
        assert_eq!(decoded.ender_items, data.ender_items);
        assert_eq!(decoded.abilities, data.abilities);
        assert_eq!(decoded.to_gzip().unwrap(), bytes);
    }

    #[test]
    fn unknown_top_level_and_nested_keys_are_preserved() {
        let mut tag = sample().encode();
        if let Tag::Compound(entries) = &mut tag {
            entries.push(("bamboo:custom".to_owned(), Tag::Long(-9)));
            if let Some((_, Tag::Compound(abilities))) =
                entries.iter_mut().find(|(key, _)| key == "abilities")
            {
                abilities.push(("bamboo:nested".to_owned(), Tag::String("keep".to_owned())));
            }
        }
        let mut plain = Vec::new();
        mc_bamboo_nbt::write_named("", &tag, &mut plain);
        let decoded = PlayerData::from_named(&plain).unwrap();
        assert_eq!(decoded.preserved.len(), 1);
        assert_eq!(decoded.preserved[0].0, "bamboo:custom");
        assert_eq!(decoded.nested_preserved.len(), 1);
        let encoded = decoded.encode();
        assert_eq!(field(&encoded, "bamboo:custom"), Some(&Tag::Long(-9)));
        let abilities = field(&encoded, "abilities").unwrap();
        assert_eq!(
            field(abilities, "bamboo:nested"),
            Some(&Tag::String("keep".to_owned()))
        );
    }

    #[test]
    fn bad_field_types_return_errors_without_panicking() {
        let tag = Tag::Compound(vec![
            ("UUID".to_owned(), Tag::Int(5)),
            ("Pos".to_owned(), Tag::String("oops".to_owned())),
            ("Health".to_owned(), Tag::String("oops".to_owned())),
        ]);
        assert!(PlayerData::decode(&tag).is_err());
        let tag = Tag::Compound(vec![("UUID".to_owned(), Tag::IntArray(vec![1, 2]))]);
        assert!(PlayerData::decode(&tag).is_err());
    }

    #[test]
    fn missing_fields_have_official_defaults() {
        let tag = Tag::Compound(vec![(
            "Dimension".to_owned(),
            Tag::String("minecraft:the_end".to_owned()),
        )]);
        let data = PlayerData::decode(&tag).unwrap();
        assert_eq!(data.dimension, "minecraft:the_end");
        assert_eq!(data.air, DEFAULT_AIR);
        assert_eq!(data.food_level, DEFAULT_FOOD_LEVEL);
        assert_eq!(data.data_version, version::DATA_VERSION);
        assert_eq!(data.abilities.fly_speed, DEFAULT_FLY_SPEED);
    }

    #[test]
    fn fuzz_truncation_flips_and_bad_tags_do_not_panic() {
        let bytes = sample().to_gzip().unwrap();
        for cut in 0..bytes.len().min(64) {
            let _ = PlayerData::from_gzip(&bytes[..bytes.len() - cut]);
        }
        for index in 0..bytes.len().min(64) {
            let mut flipped = bytes.clone();
            flipped[index] ^= 0x5a;
            let _ = PlayerData::from_gzip(&flipped);
        }
        let mut plain = Vec::new();
        mc_bamboo_nbt::write_named("", &Tag::Compound(Vec::new()), &mut plain);
        for _ in 0..256 {
            let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
            let mut random = Vec::new();
            for _ in 0..128 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                random.push((state >> 24) as u8);
            }
            let _ = PlayerData::from_named(&random);
        }
        // 类型不对、超大列表也不能让解码 panic。
        let huge = Tag::Compound(vec![(
            "Inventory".to_owned(),
            Tag::List(vec![Tag::Int(0); 8192]),
        )]);
        let mut plain = Vec::new();
        mc_bamboo_nbt::write_named("", &huge, &mut plain);
        let decoded = PlayerData::from_named(&plain).unwrap();
        assert_eq!(decoded.problems.len(), 8192);
        assert!(decoded.inventory.is_empty());
    }
}
