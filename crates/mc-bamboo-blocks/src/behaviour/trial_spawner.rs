//! `TrialSpawner` / `TrialSpawnerState` / `TrialSpawnerStateData` 的服务端状态机。
//!
//! 官方这一个类同时管一场试炼的六个状态、玩家探测、刷怪、奖励弹出、不祥试炼切换和
//! 存档。这里把世界副作用收在 [`TrialSpawnerContext`] 里，状态机本身不碰实体表和网络包，
//! 既能挂到方块实体的 ticker 上，也能逐刻对拍。字段、codec 的省略规则、状态转移和事件
//! 编号都在 26.3 的字节码与数据包上核对过。

use std::collections::BTreeSet;

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::Tag;

use crate::{
    LegacyRandom,
    archive::tag::{self, field, put},
    entity::{
        ctx::BlockView,
        mob::{MobType, spawn::SpawnReason},
    },
    level::Effect,
    shape::{Aabb, CollisionContext},
    state::{OMINOUS, TRIAL_SPAWNER_STATE, TrialSpawnerState},
};

/// 官方 `TrialSpawnerConfig.DEFAULT.ticksBetweenItemSpawners()`。
pub const TICKS_BETWEEN_ITEM_SPAWNERS: i64 = 160;
/// `TrialSpawner$FullConfig` 的默认冷却和玩家范围。
pub const DEFAULT_TARGET_COOLDOWN_LENGTH: i32 = 36_000;
pub const DEFAULT_REQUIRED_PLAYER_RANGE: i32 = 14;
/// `TrialSpawnerState.DELAY_BEFORE_EJECT_AFTER_KILLING_LAST_MOB`。
const DELAY_BEFORE_EJECT: f32 = 40.0;
/// `TrialSpawnerState.TIME_BETWEEN_EACH_EJECTION`。
const TIME_BETWEEN_EACH_EJECTION: f32 = 30.0;
/// 每 20 刻扫一次附近玩家。
const DELAY_BETWEEN_PLAYER_SCANS: i64 = 20;
/// `TRIAL_OMEN_PER_BAD_OMEN_LEVEL`。
const TRIAL_OMEN_PER_BAD_OMEN_LEVEL: i32 = 18_000;

/// `LevelEvent` 里试炼刷怪笼用的编号。
pub mod level_event {
    pub const SPAWN_MOB_AT_SPAWNER: i32 = 3011;
    pub const SPAWN_MOB_AT_MOB: i32 = 3012;
    pub const DETECT_PLAYER: i32 = 3013;
    pub const EJECT_ITEM: i32 = 3014;
    pub const DETECT_PLAYER_OMINOUS: i32 = 3019;
    pub const OMINOUS_ACTIVATE: i32 = 3020;
}

/// `TrialSpawner$FlameParticle.encode()`。
pub mod flame_particle {
    pub const NORMAL: i32 = 0;
    pub const OMINOUS: i32 = 1;
}

/// 一组带权重的条目，`WeightedList` 的顺序和取数照官方。
#[derive(Debug, Clone, PartialEq)]
pub struct Weighted<T> {
    pub data: T,
    pub weight: i32,
}

impl<T> Weighted<T> {
    pub const fn new(data: T, weight: i32) -> Self {
        Self { data, weight }
    }
}

/// `TrialSpawnerConfig` 的服务端可用形式；`spawn_data` 保留官方的复合标签原样。
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub spawn_range: i32,
    pub total_mobs: f32,
    pub simultaneous_mobs: f32,
    pub total_mobs_added_per_player: f32,
    pub simultaneous_mobs_added_per_player: f32,
    pub ticks_between_spawn: i32,
    pub spawn_potentials: Vec<Weighted<Tag>>,
    pub loot_tables_to_eject: Vec<Weighted<String>>,
    pub items_to_drop_when_ominous: String,
    /// 存档里的原始配置；注册表引用是字符串，内联配置是复合标签。
    pub raw: Tag,
}

pub const DEFAULT_LOOT_TABLE: &str = "minecraft:spawners/trial_chamber/consumables";
pub const DEFAULT_KEY_LOOT_TABLE: &str = "minecraft:spawners/trial_chamber/key";
pub const DEFAULT_ITEMS_TO_DROP_WHEN_OMINOUS: &str =
    "minecraft:spawners/trial_chamber/items_to_drop_when_ominous";

impl Default for Config {
    fn default() -> Self {
        Self {
            spawn_range: 4,
            total_mobs: 6.0,
            simultaneous_mobs: 2.0,
            total_mobs_added_per_player: 2.0,
            simultaneous_mobs_added_per_player: 1.0,
            ticks_between_spawn: 40,
            spawn_potentials: Vec::new(),
            loot_tables_to_eject: vec![
                Weighted::new(DEFAULT_LOOT_TABLE.to_owned(), 1),
                Weighted::new(DEFAULT_KEY_LOOT_TABLE.to_owned(), 1),
            ],
            items_to_drop_when_ominous: DEFAULT_ITEMS_TO_DROP_WHEN_OMINOUS.to_owned(),
            raw: Tag::Compound(Vec::new()),
        }
    }
}

impl Config {
    /// 官方 `calculateTargetTotalMobs` / `calculateTargetSimultaneousMobs`：`floor(base + per * add)`。
    /// 坏配置里的 NaN / 无穷按官方的 Java 强转成 int 的饱和结果处理，不 panic。
    fn target(base: f32, per: f32, additional: i32) -> i32 {
        let value = f64::from(base) + f64::from(per) * f64::from(additional);
        if !value.is_finite() {
            return if value.is_sign_negative() {
                i32::MIN
            } else {
                i32::MAX
            };
        }
        value
            .floor()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    }

    pub fn target_total_mobs(&self, additional_players: i32) -> i32 {
        Self::target(
            self.total_mobs,
            self.total_mobs_added_per_player,
            additional_players,
        )
    }

    pub fn target_simultaneous_mobs(&self, additional_players: i32) -> i32 {
        Self::target(
            self.simultaneous_mobs,
            self.simultaneous_mobs_added_per_player,
            additional_players,
        )
    }

    /// 按官方 `DIRECT_CODEC` 的九个 `optionalFieldOf` 宽松解码；等于默认值时编码省略。
    /// 注册表引用（字符串）没有内联数值，用默认值参与计算，但存档原样保留字符串。
    pub fn read(raw: &Tag) -> Self {
        let mut out = Self {
            raw: raw.clone(),
            ..Self::default()
        };
        let Tag::Compound(values) = raw else {
            return out;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, v)| v)
        };
        let numeric = |key: &str, default: f32, min: f32| -> f32 {
            let Some(tag) = get(key) else {
                return default;
            };
            let Ok(value) = tag::java_f32(tag) else {
                return default;
            };
            if value.is_nan() || value < min {
                default
            } else {
                value
            }
        };
        out.spawn_range = numeric("spawn_range", 4.0, 1.0).clamp(1.0, 128.0) as i32;
        out.total_mobs = numeric("total_mobs", 6.0, 0.0);
        out.simultaneous_mobs = numeric("simultaneous_mobs", 2.0, 0.0);
        out.total_mobs_added_per_player = numeric("total_mobs_added_per_player", 2.0, 0.0);
        out.simultaneous_mobs_added_per_player =
            numeric("simultaneous_mobs_added_per_player", 1.0, 0.0);
        out.ticks_between_spawn =
            numeric("ticks_between_spawn", 40.0, 0.0).clamp(0.0, i32::MAX as f32) as i32;
        if let Some(Tag::List(items)) = get("spawn_potentials") {
            out.spawn_potentials = weighted(items, |data| Some(data.clone()));
        }
        if let Some(Tag::List(items)) = get("loot_tables_to_eject") {
            out.loot_tables_to_eject = weighted(items, |data| match data {
                Tag::String(text) => Some(text.clone()),
                _ => None,
            });
        }
        if let Some(Tag::String(text)) = get("items_to_drop_when_ominous") {
            out.items_to_drop_when_ominous = text.clone();
        }
        out
    }

    /// 官方 `DIRECT_CODEC` 的写出规则：九个字段只有不等于默认值时写。
    pub fn write(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        if self.raw != Tag::Compound(Vec::new()) && !matches!(self.raw, Tag::Compound(_)) {
            // 注册表引用没有可解析的数值字段，整条原样保留。
            return Vec::new();
        }
        let Tag::Compound(values) = &self.raw else {
            return out;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, v)| v)
        };
        // 只重写数值和列表字段；其余由 raw 的调用方原样保留。
        for (key, value, default) in [
            ("spawn_range", self.spawn_range as f32, 4.0),
            ("total_mobs", self.total_mobs, 6.0),
            ("simultaneous_mobs", self.simultaneous_mobs, 2.0),
            (
                "total_mobs_added_per_player",
                self.total_mobs_added_per_player,
                2.0,
            ),
            (
                "simultaneous_mobs_added_per_player",
                self.simultaneous_mobs_added_per_player,
                1.0,
            ),
            ("ticks_between_spawn", self.ticks_between_spawn as f32, 40.0),
        ] {
            if value != default {
                put(&mut out, key, numeric_tag(get(key), value));
            }
        }
        if let Some(Tag::List(items)) = get("spawn_potentials") {
            put(&mut out, "spawn_potentials", Tag::List(items.clone()));
        }
        if let Some(Tag::List(items)) = get("loot_tables_to_eject") {
            put(&mut out, "loot_tables_to_eject", Tag::List(items.clone()));
        }
        if let Some(Tag::String(text)) = get("items_to_drop_when_ominous")
            && text != &self.items_to_drop_when_ominous
        {
            put(
                &mut out,
                "items_to_drop_when_ominous",
                Tag::String(self.items_to_drop_when_ominous.clone()),
            );
        }
        out
    }
}

fn numeric_tag(original: Option<&Tag>, value: f32) -> Tag {
    match original {
        Some(Tag::Int(_)) => Tag::Int(value as i32),
        Some(Tag::Float(_)) => Tag::Float(value),
        Some(Tag::Double(_)) => Tag::Double(f64::from(value)),
        Some(Tag::Long(_)) => Tag::Long(value as i64),
        Some(Tag::Byte(_)) => Tag::Byte(value as i8),
        Some(Tag::Short(_)) => Tag::Short(value as i16),
        _ if value.fract() == 0.0 && value.abs() <= i32::MAX as f32 => Tag::Int(value as i32),
        _ => Tag::Float(value),
    }
}

fn weighted<T>(items: &[Tag], data: impl Fn(&Tag) -> Option<T>) -> Vec<Weighted<T>> {
    items
        .iter()
        .filter_map(|entry| {
            let Tag::Compound(values) = entry else {
                return None;
            };
            let inner = values
                .iter()
                .rev()
                .find(|(name, _)| name == "data")
                .map(|(_, value)| value)?;
            let weight = values
                .iter()
                .rev()
                .find(|(name, _)| name == "weight")
                .and_then(|(_, value)| tag::java_i32(value).ok())
                .unwrap_or(1)
                .max(0);
            Some(Weighted::new(data(inner)?, weight))
        })
        .collect()
}

/// `WeightedList.getRandom(random)`：总权重为 0 时不取随机数。
fn pick<T: Clone>(items: &[Weighted<T>], random: &LegacyRandom) -> Option<T> {
    let total: i32 = items
        .iter()
        .try_fold(0_i32, |sum, item| sum.checked_add(item.weight))?;
    if total <= 0 {
        return None;
    }
    let mut ticket = random.next_int(total);
    for item in items {
        if ticket < item.weight {
            return Some(item.data.clone());
        }
        ticket -= item.weight;
    }
    None
}

/// `FullConfig`：普通 / 不祥两份配置加冷却长度、玩家范围。
#[derive(Debug, Clone, PartialEq)]
pub struct FullConfig {
    pub normal: Config,
    pub ominous: Config,
    pub target_cooldown_length: i32,
    pub required_player_range: i32,
    /// `normal_config` / `ominous_config` 的原始标签（可能是注册表引用）。
    pub normal_raw: Option<Tag>,
    pub ominous_raw: Option<Tag>,
}

impl Default for FullConfig {
    fn default() -> Self {
        Self {
            normal: Config::default(),
            ominous: Config::default(),
            target_cooldown_length: DEFAULT_TARGET_COOLDOWN_LENGTH,
            required_player_range: DEFAULT_REQUIRED_PLAYER_RANGE,
            normal_raw: None,
            ominous_raw: None,
        }
    }
}

impl FullConfig {
    pub fn read(tag: &Tag) -> Self {
        let mut out = Self {
            normal_raw: field(tag, "normal_config").cloned(),
            ominous_raw: field(tag, "ominous_config").cloned(),
            ..Self::default()
        };
        if let Some(raw) = &out.normal_raw {
            out.normal = Config::read(raw);
        }
        if let Some(raw) = &out.ominous_raw {
            out.ominous = Config::read(raw);
        }
        out.target_cooldown_length = tag::int_or(tag, "target_cooldown_length", 36_000).max(0);
        out.required_player_range = tag::int_or(tag, "required_player_range", 14).clamp(1, 128);
        out
    }

    /// 按官方 `FullConfig.MAP_CODEC` 的顺序写四个字段。
    pub fn write(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        if let Some(raw) = &self.normal_raw {
            put(
                &mut out,
                "normal_config",
                write_config_ref(raw, &self.normal),
            );
        }
        if let Some(raw) = &self.ominous_raw {
            put(
                &mut out,
                "ominous_config",
                write_config_ref(raw, &self.ominous),
            );
        }
        if self.target_cooldown_length != DEFAULT_TARGET_COOLDOWN_LENGTH {
            put(
                &mut out,
                "target_cooldown_length",
                Tag::Int(self.target_cooldown_length),
            );
        }
        if self.required_player_range != DEFAULT_REQUIRED_PLAYER_RANGE {
            put(
                &mut out,
                "required_player_range",
                Tag::Int(self.required_player_range),
            );
        }
        out
    }

    pub fn active(&self, ominous: bool) -> &Config {
        if ominous { &self.ominous } else { &self.normal }
    }
}

fn write_config_ref(raw: &Tag, parsed: &Config) -> Tag {
    if !matches!(raw, Tag::Compound(_)) {
        return raw.clone();
    }
    Tag::Compound(parsed.write())
}

/// 一次扫描看到的玩家。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedPlayer {
    pub uuid: [u8; 16],
    pub trial_omen: bool,
    pub bad_omen_amplifier: Option<i32>,
    /// 眼睛所在方块；`levelEvent(3020, ...)` 用它。
    pub eye_block: BlockPos,
}

impl Default for DetectedPlayer {
    fn default() -> Self {
        Self {
            uuid: [0; 16],
            trial_omen: false,
            bad_omen_amplifier: None,
            eye_block: BlockPos::new(0, 0, 0),
        }
    }
}

/// 世界副作用：纯逻辑状态机只通过这些回调操作世界。
pub trait TrialSpawnerContext {
    fn game_time(&self) -> i64;
    fn random(&self) -> &LegacyRandom;
    fn can_spawn_in_level(&self) -> bool;
    /// 附近符合探测器条件的玩家；`line_of_sight` 为真时还要看得见方块中心。
    fn detect_players(&self, pos: BlockPos, range: f64, line_of_sight: bool)
    -> Vec<DetectedPlayer>;
    /// 状态里的 `displayEntity` 能不能造出来；`hasSpinningMob` 为真才问。
    fn has_display_entity(&self, state: TrialSpawnerState, spawn_data: Option<&Tag>) -> bool;
    /// 这只已记录的生物是否已经不活 / 太远 / 换维度。
    fn mob_untracked(&self, uuid: [u8; 16], pos: BlockPos) -> bool;
    /// 生成一只带 `SpawnData` 的生物，返回它的 UUID；同时发出刷怪粒子和声音。
    fn spawn_mob(
        &mut self,
        pos: BlockPos,
        spawn_data: &Tag,
        ominous: bool,
        spawn_range: i32,
    ) -> Option<[u8; 16]>;
    /// `ejectReward`：按战利品表弹出一堆物品，返回有没有弹过。
    fn eject_reward(&mut self, pos: BlockPos, loot_table: &str) -> bool;
    /// `transformBadOmenIntoTrialOmen` 和激活不祥试炼的客户端事件。
    fn transform_bad_omen(&mut self, player: [u8; 16], duration: i32);
    /// 生成 `OminousItemSpawner`，成功时返回真。
    fn spawn_ominous_item(&mut self, pos: BlockPos, loot_table: &str) -> bool;
    /// `levelEvent(type, pos, data)`。
    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32);
    /// 开关闸门的音效。
    fn shutter_sound(&mut self, pos: BlockPos, open: bool);
    /// 改变方块状态里的 `trial_spawner_state`。
    fn set_state(&mut self, state: TrialSpawnerState);
    /// 改变方块状态里的 `ominous`。
    fn set_ominous(&mut self, ominous: bool);
    /// 变成不祥试炼时让已记录的生物消失。
    fn discard_tracked_mobs(&mut self, mobs: &[[u8; 16]]);
}

/// `TrialSpawnerStateData.Packed`：官方存档里持久化的七个字段。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateData {
    /// 存档键 `registered_players`。
    pub detected_players: BTreeSet<[u8; 16]>,
    pub current_mobs: BTreeSet<[u8; 16]>,
    pub cooldown_ends_at: i64,
    pub next_mob_spawns_at: i64,
    pub total_mobs_spawned: i32,
    pub next_spawn_data: Option<Tag>,
    pub ejecting_loot_table: Option<String>,
}

impl StateData {
    pub fn reset_statistics(&mut self) {
        self.detected_players.clear();
        self.total_mobs_spawned = 0;
        self.next_mob_spawns_at = 0;
        self.cooldown_ends_at = 0;
    }

    pub fn reset(&mut self) {
        self.current_mobs.clear();
        self.next_spawn_data = None;
        self.reset_statistics();
    }

    pub fn additional_players(&self) -> i32 {
        (self.detected_players.len() as i32)
            .saturating_sub(1)
            .max(0)
    }

    pub fn has_finished_spawning_all_mobs(&self, config: &Config, additional: i32) -> bool {
        self.total_mobs_spawned >= config.target_total_mobs(additional)
    }

    pub fn ready_to_spawn_next_mob(&self, now: i64, config: &Config, additional: i32) -> bool {
        now >= self.next_mob_spawns_at
            && (self.current_mobs.len() as i32) < config.target_simultaneous_mobs(additional)
    }

    /// `isReadyToOpenShutter(delay, cooldownLength)`。
    pub fn ready_to_open_shutter(&self, now: i64, cooldown_length: i32) -> bool {
        let start = self
            .cooldown_ends_at
            .wrapping_sub(i64::from(cooldown_length));
        (now as f32) >= (start as f32) + DELAY_BEFORE_EJECT
    }

    /// `isReadyToEjectItems(period, cooldownLength)`：官方把 gameTime 转 float 后取模。
    pub fn ready_to_eject_items(&self, now: i64, cooldown_length: i32) -> bool {
        let start = self
            .cooldown_ends_at
            .wrapping_sub(i64::from(cooldown_length));
        let elapsed = now.wrapping_sub(start) as f32;
        elapsed % TIME_BETWEEN_EACH_EJECTION == 0.0
    }

    /// `Packed.MAP_CODEC`：等于默认值的字段全部省略。
    pub fn read(tag: &Tag) -> Self {
        let mut out = Self {
            detected_players: uuid_set(tag, "registered_players"),
            current_mobs: uuid_set(tag, "current_mobs"),
            cooldown_ends_at: tag::long_or(tag, "cooldown_ends_at", 0),
            next_mob_spawns_at: tag::long_or(tag, "next_mob_spawns_at", 0),
            total_mobs_spawned: tag::int_or(tag, "total_mobs_spawned", 0).clamp(0, i32::MAX),
            ..Self::default()
        };
        if let Some(Tag::Compound(_)) = field(tag, "spawn_data") {
            out.next_spawn_data = field(tag, "spawn_data").cloned();
        }
        if let Some(Tag::String(text)) = field(tag, "ejecting_loot_table") {
            out.ejecting_loot_table = Some(text.clone());
        }
        out
    }

    pub fn write(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        if !self.detected_players.is_empty() {
            put(
                &mut out,
                "registered_players",
                uuid_list(&self.detected_players),
            );
        }
        if !self.current_mobs.is_empty() {
            put(&mut out, "current_mobs", uuid_list(&self.current_mobs));
        }
        if self.cooldown_ends_at != 0 {
            put(
                &mut out,
                "cooldown_ends_at",
                Tag::Long(self.cooldown_ends_at),
            );
        }
        if self.next_mob_spawns_at != 0 {
            put(
                &mut out,
                "next_mob_spawns_at",
                Tag::Long(self.next_mob_spawns_at),
            );
        }
        if self.total_mobs_spawned != 0 {
            put(
                &mut out,
                "total_mobs_spawned",
                Tag::Int(self.total_mobs_spawned),
            );
        }
        if let Some(data) = &self.next_spawn_data {
            put(&mut out, "spawn_data", data.clone());
        }
        if let Some(table) = &self.ejecting_loot_table {
            put(&mut out, "ejecting_loot_table", Tag::String(table.clone()));
        }
        out
    }

    /// `getUpdateTag`：只有 `next_mob_spawns_at`（ACTIVE 时）和 `spawn_data`。
    pub fn update_tag(&self, state: TrialSpawnerState) -> Tag {
        let mut out = Vec::new();
        if state == TrialSpawnerState::Active {
            put(
                &mut out,
                "next_mob_spawns_at",
                Tag::Long(self.next_mob_spawns_at),
            );
        }
        if let Some(data) = &self.next_spawn_data {
            put(&mut out, "spawn_data", data.clone());
        }
        Tag::Compound(out)
    }
}

fn uuid_set(tag: &Tag, key: &str) -> BTreeSet<[u8; 16]> {
    let Some(Tag::List(items)) = field(tag, key) else {
        return BTreeSet::new();
    };
    items.iter().filter_map(uuid_of).collect()
}

fn uuid_of(tag: &Tag) -> Option<[u8; 16]> {
    let Tag::IntArray(parts) = tag else {
        return None;
    };
    let array: [i32; 4] = parts.as_slice().try_into().ok()?;
    let mut out = [0_u8; 16];
    for (index, part) in array.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&part.to_be_bytes());
    }
    Some(out)
}

fn uuid_list(set: &BTreeSet<[u8; 16]>) -> Tag {
    Tag::List(
        set.iter()
            .map(|uuid| {
                let mut parts = Vec::with_capacity(4);
                for index in 0..4 {
                    let start = index * 4;
                    parts.push(i32::from_be_bytes([
                        uuid[start],
                        uuid[start + 1],
                        uuid[start + 2],
                        uuid[start + 3],
                    ]));
                }
                Tag::IntArray(parts)
            })
            .collect(),
    )
}

/// 一场试炼的全部运行时状态。
#[derive(Debug, Clone, PartialEq)]
pub struct TrialSpawner {
    pub state: TrialSpawnerState,
    pub ominous: bool,
    pub config: FullConfig,
    pub data: StateData,
}

impl Default for TrialSpawner {
    fn default() -> Self {
        Self {
            state: TrialSpawnerState::Inactive,
            ominous: false,
            config: FullConfig::default(),
            data: StateData::default(),
        }
    }
}

impl TrialSpawner {
    /// 官方 `loadAdditional`：`state` 和 `ominous` 来自方块状态，其余两组 codec 读同一个条目。
    pub fn read(tag: &Tag, state: TrialSpawnerState, ominous: bool) -> Self {
        Self {
            state,
            ominous,
            config: FullConfig::read(tag),
            data: StateData::read(tag),
        }
    }

    /// 官方 `saveAdditional` 写出的自有键（不含 `id` / 坐标 / `components`）。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = self.data.write();
        out.extend(self.config.write());
        out
    }

    pub fn update_tag(&self) -> Tag {
        self.data.update_tag(self.state)
    }

    /// `tickServer`：`isOminous` 从方块状态来，清掉已经不存在的怪，再跑一次状态机。
    pub fn tick<C: TrialSpawnerContext>(&mut self, pos: BlockPos, ominous: bool, ctx: &mut C) {
        self.ominous = ominous;
        let stale: Vec<[u8; 16]> = self
            .data
            .current_mobs
            .iter()
            .copied()
            .filter(|uuid| ctx.mob_untracked(*uuid, pos))
            .collect();
        if !stale.is_empty() {
            let ticks = i64::from(self.config.active(self.ominous).ticks_between_spawn);
            for uuid in stale {
                self.data.current_mobs.remove(&uuid);
            }
            self.data.next_mob_spawns_at = ctx.game_time().saturating_add(ticks);
        }
        let next = self.tick_and_get_next(pos, ctx);
        if next != self.state {
            self.state = next;
            ctx.set_state(next);
        }
    }

    /// `TrialSpawnerState.tickAndGetNext`，逐分支照官方。
    fn tick_and_get_next<C: TrialSpawnerContext>(
        &mut self,
        pos: BlockPos,
        ctx: &mut C,
    ) -> TrialSpawnerState {
        let now = ctx.game_time();
        let config = self.config.active(self.ominous).clone();
        match self.state {
            TrialSpawnerState::Inactive => {
                if ctx.has_display_entity(
                    TrialSpawnerState::WaitingForPlayers,
                    self.data.next_spawn_data.as_ref(),
                ) {
                    TrialSpawnerState::WaitingForPlayers
                } else {
                    TrialSpawnerState::Inactive
                }
            }
            TrialSpawnerState::WaitingForPlayers => {
                if !ctx.can_spawn_in_level() {
                    self.data.reset_statistics();
                    return TrialSpawnerState::WaitingForPlayers;
                }
                if !self.has_mob_to_spawn(ctx) {
                    return TrialSpawnerState::Inactive;
                }
                self.try_detect_players(pos, ctx);
                if self.data.detected_players.is_empty() {
                    TrialSpawnerState::WaitingForPlayers
                } else {
                    TrialSpawnerState::Active
                }
            }
            TrialSpawnerState::Active => {
                if !ctx.can_spawn_in_level() {
                    self.data.reset_statistics();
                    return TrialSpawnerState::WaitingForPlayers;
                }
                if !self.has_mob_to_spawn(ctx) {
                    return TrialSpawnerState::Inactive;
                }
                // 官方在探测玩家之前就算 additional，之后加进来的人不影响这一刻。
                let additional = self.data.additional_players();
                self.try_detect_players(pos, ctx);
                if self.ominous {
                    self.spawn_ominous_item_spawner(pos, ctx, &config);
                }
                if self
                    .data
                    .has_finished_spawning_all_mobs(&config, additional)
                {
                    if self.data.current_mobs.is_empty() {
                        self.data.cooldown_ends_at =
                            now.saturating_add(i64::from(self.config.target_cooldown_length));
                        self.data.total_mobs_spawned = 0;
                        self.data.next_mob_spawns_at = 0;
                        return TrialSpawnerState::WaitingForRewardEjection;
                    }
                } else if self.data.ready_to_spawn_next_mob(now, &config, additional)
                    && let Some(data) = self.data.next_spawn_data.clone()
                    && let Some(uuid) = ctx.spawn_mob(pos, &data, self.ominous, config.spawn_range)
                {
                    self.data.current_mobs.insert(uuid);
                    self.data.total_mobs_spawned = self.data.total_mobs_spawned.saturating_add(1);
                    self.data.next_mob_spawns_at =
                        now.saturating_add(i64::from(config.ticks_between_spawn));
                    if let Some(next) = pick(&config.spawn_potentials, ctx.random()) {
                        self.data.next_spawn_data = Some(next);
                    }
                }
                TrialSpawnerState::Active
            }
            TrialSpawnerState::WaitingForRewardEjection => {
                if self
                    .data
                    .ready_to_open_shutter(now, self.config.target_cooldown_length)
                {
                    ctx.shutter_sound(pos, true);
                    TrialSpawnerState::EjectingReward
                } else {
                    TrialSpawnerState::WaitingForRewardEjection
                }
            }
            TrialSpawnerState::EjectingReward => {
                if !self
                    .data
                    .ready_to_eject_items(now, self.config.target_cooldown_length)
                {
                    return TrialSpawnerState::EjectingReward;
                }
                if let Some(first) = self.data.detected_players.iter().next().copied() {
                    if self.data.ejecting_loot_table.is_none() {
                        self.data.ejecting_loot_table =
                            pick(&config.loot_tables_to_eject, ctx.random());
                    }
                    if let Some(table) = self.data.ejecting_loot_table.clone()
                        && ctx.eject_reward(pos, &table)
                    {
                        ctx.level_event(level_event::EJECT_ITEM, pos, 0);
                    }
                    let _ = first;
                    self.data.detected_players.pop_first();
                    TrialSpawnerState::EjectingReward
                } else {
                    ctx.shutter_sound(pos, false);
                    self.data.ejecting_loot_table = None;
                    TrialSpawnerState::Cooldown
                }
            }
            TrialSpawnerState::Cooldown => {
                self.try_detect_players(pos, ctx);
                if !self.data.detected_players.is_empty() {
                    self.data.total_mobs_spawned = 0;
                    self.data.next_mob_spawns_at = 0;
                    TrialSpawnerState::Active
                } else if now >= self.data.cooldown_ends_at {
                    self.remove_ominous(pos, ctx);
                    self.data.reset();
                    TrialSpawnerState::WaitingForPlayers
                } else {
                    TrialSpawnerState::Cooldown
                }
            }
        }
    }

    /// `hasMobToSpawn` 会顺手把 `nextSpawnData` 从 `spawnPotentials` 里抽出来。
    fn has_mob_to_spawn<C: TrialSpawnerContext>(&mut self, ctx: &C) -> bool {
        let config = self.config.active(self.ominous);
        if self.data.next_spawn_data.is_none()
            && !config.spawn_potentials.is_empty()
            && let Some(data) = pick(&config.spawn_potentials, ctx.random())
        {
            self.data.next_spawn_data = Some(data);
        }
        self.data
            .next_spawn_data
            .as_ref()
            .is_some_and(spawn_data_has_id)
            || !config.spawn_potentials.is_empty()
    }

    /// `tryDetectPlayers`：`(pos.asLong + gameTime) % 20 == 0` 才扫。
    fn try_detect_players<C: TrialSpawnerContext>(&mut self, pos: BlockPos, ctx: &mut C) {
        let now = ctx.game_time();
        if pos
            .pack()
            .wrapping_add(now)
            .rem_euclid(DELAY_BETWEEN_PLAYER_SCANS)
            != 0
        {
            return;
        }
        if self.state == TrialSpawnerState::Cooldown && self.ominous {
            return;
        }
        let range = f64::from(self.config.required_player_range);
        let list = ctx.detect_players(pos, range, true);
        let mut became_ominous = false;
        if !self.ominous && !list.is_empty() {
            // TRIAL_OMEN 优先；否则记最后一个 BAD_OMEN 玩家。
            let found = list.iter().find(|player| player.trial_omen).or_else(|| {
                list.iter()
                    .rev()
                    .find(|player| player.bad_omen_amplifier.is_some())
            });
            if let Some(player) = found {
                if !player.trial_omen {
                    let amplifier = player.bad_omen_amplifier.unwrap_or(0).max(0);
                    let duration = amplifier
                        .saturating_add(1)
                        .saturating_mul(TRIAL_OMEN_PER_BAD_OMEN_LEVEL);
                    ctx.transform_bad_omen(player.uuid, duration);
                }
                ctx.level_event(level_event::OMINOUS_ACTIVATE, player.eye_block, 0);
                self.apply_ominous(pos, ctx);
                became_ominous = true;
            }
        }
        if self.state == TrialSpawnerState::Cooldown && !became_ominous {
            return;
        }
        let detected: Vec<[u8; 16]> = if self.data.detected_players.is_empty() {
            list.iter().map(|player| player.uuid).collect()
        } else {
            ctx.detect_players(pos, range, false)
                .iter()
                .map(|player| player.uuid)
                .collect()
        };
        let before = self.data.detected_players.len();
        self.data.detected_players.extend(detected);
        if self.data.detected_players.len() != before {
            self.data.next_mob_spawns_at = self.data.next_mob_spawns_at.max(now.saturating_add(40));
            if !became_ominous {
                let event = if self.ominous {
                    level_event::DETECT_PLAYER_OMINOUS
                } else {
                    level_event::DETECT_PLAYER
                };
                ctx.level_event(event, pos, self.data.detected_players.len() as i32);
            }
        }
    }

    fn apply_ominous<C: TrialSpawnerContext>(&mut self, pos: BlockPos, ctx: &mut C) {
        ctx.set_ominous(true);
        ctx.level_event(level_event::OMINOUS_ACTIVATE, pos, 1);
        self.ominous = true;
        let mobs: Vec<[u8; 16]> = self.data.current_mobs.iter().copied().collect();
        ctx.discard_tracked_mobs(&mobs);
        let ominous = &self.config.ominous;
        if !ominous.spawn_potentials.is_empty() {
            self.data.next_spawn_data = None;
        }
        self.data.total_mobs_spawned = 0;
        self.data.current_mobs.clear();
        self.data.next_mob_spawns_at = ctx
            .game_time()
            .saturating_add(i64::from(ominous.ticks_between_spawn));
        self.data.cooldown_ends_at = ctx.game_time().saturating_add(TICKS_BETWEEN_ITEM_SPAWNERS);
    }

    fn remove_ominous<C: TrialSpawnerContext>(&mut self, _pos: BlockPos, ctx: &mut C) {
        ctx.set_ominous(false);
        self.ominous = false;
    }

    /// `spawnOminousOminousItemSpawner`：到点时在探测到的玩家上方生成一个不祥物品生成器。
    fn spawn_ominous_item_spawner<C: TrialSpawnerContext>(
        &mut self,
        pos: BlockPos,
        ctx: &mut C,
        config: &Config,
    ) {
        if ctx.game_time() < self.data.cooldown_ends_at {
            return;
        }
        if ctx.spawn_ominous_item(pos, &config.items_to_drop_when_ominous) {
            self.data.cooldown_ends_at =
                ctx.game_time().saturating_add(TICKS_BETWEEN_ITEM_SPAWNERS);
        }
    }
}

fn spawn_data_has_id(tag: &Tag) -> bool {
    field(tag, "entity")
        .and_then(|entity| field(entity, "id"))
        .is_some_and(|id| matches!(id, Tag::String(text) if !text.is_empty()))
}

/// 试炼刷怪笼方块实体：持有 [`TrialSpawner`]，逐刻跑一次状态机，存档走
/// [`TrialSpawner::additional`]。方块状态里的 `trial_spawner_state` 和 `ominous`
/// 由 `set_state` / `set_ominous` 写回。
pub struct TrialSpawnerBlockEntity {
    pos: BlockPos,
    spawner: TrialSpawner,
}

impl TrialSpawnerBlockEntity {
    pub fn new(pos: BlockPos, spawner: TrialSpawner) -> Self {
        Self { pos, spawner }
    }
}

impl crate::BlockEntity for TrialSpawnerBlockEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut crate::World) {
        // 官方 `tickServer` 每刻从方块状态读回 `state`，`ominous` 由调用方传入。
        let block = world.block_state(self.pos);
        self.spawner.state = block.get(TRIAL_SPAWNER_STATE);
        let ominous = block.get(OMINOUS);
        let mut ctx = TrialSpawnerWorld {
            world,
            pos: self.pos,
        };
        self.spawner.tick(self.pos, ominous, &mut ctx);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        Ok(self.spawner.additional())
    }
}

/// [`TrialSpawnerContext`] 在 [`crate::World`] 上的实现。能直接做的读写直接做；
/// 需要游戏层数据的（不祥效果、装备表、战利品表、实体位置）转发给 [`crate::Level`]
/// 的钩子，缺省是不刷怪、不掉落、没有玩家。
struct TrialSpawnerWorld<'a, 'w> {
    world: &'a mut crate::World<'w>,
    pos: BlockPos,
}

impl TrialSpawnerWorld<'_, '_> {
    fn shutter(&mut self, open: bool) {
        let name = if open {
            "minecraft:block.trial_spawner.open_shutter"
        } else {
            "minecraft:block.trial_spawner.close_shutter"
        };
        self.world.play_block_sound(
            self.pos,
            mc_bamboo_data::sounds::sound_event(name),
            1.0,
            1.0,
        );
    }
}

impl TrialSpawnerContext for TrialSpawnerWorld<'_, '_> {
    fn game_time(&self) -> i64 {
        self.world.game_time()
    }

    fn random(&self) -> &LegacyRandom {
        self.world.random()
    }

    fn can_spawn_in_level(&self) -> bool {
        let rules = self.world.block_game_rules();
        rules.spawner_blocks_work && self.world.difficulty_id() != 0 && rules.spawn_mobs
    }

    fn detect_players(
        &self,
        pos: BlockPos,
        range: f64,
        line_of_sight: bool,
    ) -> Vec<DetectedPlayer> {
        self.world
            .trial_spawner_players(pos, range, line_of_sight)
            .into_iter()
            .map(|player| DetectedPlayer {
                uuid: player.uuid,
                trial_omen: player.trial_omen,
                bad_omen_amplifier: player.bad_omen_amplifier,
                eye_block: player.eye_block,
            })
            .collect()
    }

    fn has_display_entity(&self, _state: TrialSpawnerState, _spawn_data: Option<&Tag>) -> bool {
        self.world.trial_spawner_has_display_entity()
    }

    fn mob_untracked(&self, uuid: [u8; 16], pos: BlockPos) -> bool {
        self.world.trial_spawner_mob_untracked(uuid, pos)
    }

    fn spawn_mob(
        &mut self,
        pos: BlockPos,
        spawn_data: &Tag,
        ominous: bool,
        spawn_range: i32,
    ) -> Option<[u8; 16]> {
        let entity_tag = field(spawn_data, "entity")?;
        let equipment = field(spawn_data, "equipment");
        // `lambda$spawnMob$1`：生点 (x + (d-d)*range + 0.5, y + nextInt(3) - 1, z + ...)。
        let random = self.world.random();
        let range = f64::from(spawn_range);
        let position = Vec3::new(
            f64::from(pos.x) + (random.next_double() - random.next_double()) * range + 0.5,
            f64::from(pos.y + random.next_int(3) - 1),
            f64::from(pos.z) + (random.next_double() - random.next_double()) * range + 0.5,
        );
        // `noCollision(entityType.getSpawnAABB(x,y,z))`：生点周围不能撞方块。
        let id = field(entity_tag, "id").and_then(|value| match value {
            Tag::String(text) if !text.is_empty() => Some(text.as_str()),
            _ => None,
        })?;
        let kind = MobType::by_name(id)?;
        let (width, height, _) = kind.dimensions(false);
        let half = f64::from(width / 2.0);
        let spawn_box = Aabb::new(
            position.x - half,
            position.y,
            position.z - half,
            position.x + half,
            position.y + f64::from(height),
            position.z + half,
        );
        if !BlockView::block_collisions(&*self.world, spawn_box, CollisionContext::Empty).is_empty()
        {
            return None;
        }
        // `inLineOfSight(atCenterOf(pos), position)`：从生点到刷怪笼中心没有视觉遮挡。
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        if let Some(hit) =
            BlockView::clip_collider(&*self.world, position, center, CollisionContext::Empty)
            && hit.pos != BlockPos::containing(center)
        {
            return None;
        }
        // `lambda$spawnMob$2`：朝向 (nextFloat*360, 0)。
        let yaw = random.next_float() * 360.0;
        let uuid = self.world.spawn_from_spawn_data(
            entity_tag,
            position,
            yaw,
            0.0,
            SpawnReason::TrialSpawner,
            equipment,
        )?;
        let flame = if ominous {
            flame_particle::OMINOUS
        } else {
            flame_particle::NORMAL
        };
        self.world
            .level_event(level_event::SPAWN_MOB_AT_SPAWNER, pos, flame);
        self.world.level_event(
            level_event::SPAWN_MOB_AT_MOB,
            BlockPos::containing(position),
            flame,
        );
        Some(uuid)
    }

    fn eject_reward(&mut self, pos: BlockPos, loot_table: &str) -> bool {
        self.world.trial_spawner_eject_reward(pos, loot_table)
    }

    fn transform_bad_omen(&mut self, player: [u8; 16], duration: i32) {
        self.world
            .push_effect(Effect::TrialSpawnerTransformBadOmen { player, duration });
    }

    fn spawn_ominous_item(&mut self, pos: BlockPos, loot_table: &str) -> bool {
        self.world.trial_spawner_spawn_ominous_item(pos, loot_table)
    }

    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32) {
        self.world.level_event(event, pos, data);
    }

    fn shutter_sound(&mut self, _pos: BlockPos, open: bool) {
        self.shutter(open);
    }

    fn set_state(&mut self, state: TrialSpawnerState) {
        let block = self.world.block_state(self.pos);
        self.world
            .set_block_and_update(self.pos, block.with(TRIAL_SPAWNER_STATE, state));
    }

    fn set_ominous(&mut self, ominous: bool) {
        let block = self.world.block_state(self.pos);
        self.world
            .set_block_and_update(self.pos, block.with(OMINOUS, ominous));
    }

    fn discard_tracked_mobs(&mut self, mobs: &[[u8; 16]]) {
        self.world.trial_spawner_discard_mobs(mobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::Vec3;
    use std::cell::Cell;

    fn uuid(n: u8) -> [u8; 16] {
        let mut out = [0; 16];
        out[15] = n;
        out
    }

    fn spawn_data(id: &str) -> Tag {
        Tag::Compound(vec![(
            "entity".to_owned(),
            Tag::Compound(vec![("id".to_owned(), Tag::String(id.to_owned()))]),
        )])
    }

    fn config_with_potentials(total: f32) -> Tag {
        Tag::Compound(vec![
            ("total_mobs".to_owned(), Tag::Float(total)),
            (
                "spawn_potentials".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    ("data".to_owned(), spawn_data("minecraft:zombie")),
                    ("weight".to_owned(), Tag::Int(1)),
                ])]),
            ),
        ])
    }

    struct Fake {
        time: i64,
        random: LegacyRandom,
        players: Vec<DetectedPlayer>,
        detected_calls: Cell<usize>,
        spawned: Vec<[u8; 16]>,
        next_uuid: u8,
        ejected: Vec<String>,
        states: Vec<TrialSpawnerState>,
        ominous: Vec<bool>,
        events: Vec<(i32, BlockPos, i32)>,
        shutters: Vec<bool>,
        ominous_items: usize,
        live: BTreeSet<[u8; 16]>,
        display: bool,
    }

    impl Fake {
        fn new(time: i64) -> Self {
            Self {
                time,
                random: LegacyRandom::new(7),
                players: Vec::new(),
                detected_calls: Cell::new(0),
                spawned: Vec::new(),
                next_uuid: 1,
                ejected: Vec::new(),
                states: Vec::new(),
                ominous: Vec::new(),
                events: Vec::new(),
                shutters: Vec::new(),
                ominous_items: 0,
                live: BTreeSet::new(),
                display: true,
            }
        }

        fn player(mut self, id: u8) -> Self {
            self.players.push(DetectedPlayer {
                uuid: uuid(id),
                ..DetectedPlayer::default()
            });
            self
        }
    }

    impl TrialSpawnerContext for Fake {
        fn game_time(&self) -> i64 {
            self.time
        }
        fn random(&self) -> &LegacyRandom {
            &self.random
        }
        fn can_spawn_in_level(&self) -> bool {
            true
        }
        fn detect_players(
            &self,
            _pos: BlockPos,
            range: f64,
            _line_of_sight: bool,
        ) -> Vec<DetectedPlayer> {
            self.detected_calls.set(self.detected_calls.get() + 1);
            let _ = range;
            self.players.clone()
        }
        fn has_display_entity(&self, _state: TrialSpawnerState, _spawn_data: Option<&Tag>) -> bool {
            self.display
        }
        fn mob_untracked(&self, id: [u8; 16], _pos: BlockPos) -> bool {
            !self.live.contains(&id)
        }
        fn spawn_mob(
            &mut self,
            _pos: BlockPos,
            _spawn_data: &Tag,
            _ominous: bool,
            _spawn_range: i32,
        ) -> Option<[u8; 16]> {
            let id = uuid(self.next_uuid);
            self.next_uuid += 1;
            self.spawned.push(id);
            self.live.insert(id);
            Some(id)
        }
        fn eject_reward(&mut self, _pos: BlockPos, loot_table: &str) -> bool {
            self.ejected.push(loot_table.to_owned());
            true
        }
        fn transform_bad_omen(&mut self, _player: [u8; 16], _duration: i32) {}
        fn spawn_ominous_item(&mut self, _pos: BlockPos, _loot_table: &str) -> bool {
            self.ominous_items += 1;
            true
        }
        fn level_event(&mut self, event: i32, pos: BlockPos, data: i32) {
            self.events.push((event, pos, data));
        }
        fn shutter_sound(&mut self, _pos: BlockPos, open: bool) {
            self.shutters.push(open);
        }
        fn set_state(&mut self, state: TrialSpawnerState) {
            self.states.push(state);
        }
        fn set_ominous(&mut self, ominous: bool) {
            self.ominous.push(ominous);
        }
        fn discard_tracked_mobs(&mut self, mobs: &[[u8; 16]]) {
            for mob in mobs {
                self.live.remove(mob);
            }
        }
    }

    fn spawner_with(total: f32, state: TrialSpawnerState) -> TrialSpawner {
        let raw = config_with_potentials(total);
        let mut spawner = TrialSpawner::read(
            &Tag::Compound(vec![("normal_config".to_owned(), raw.clone())]),
            state,
            false,
        );
        spawner.config.normal = Config::read(&raw);
        spawner
    }

    #[test]
    fn defaults_match_the_datapack_builder() {
        let config = Config::default();
        assert_eq!(config.spawn_range, 4);
        assert_eq!(config.target_total_mobs(0), 6);
        assert_eq!(config.target_total_mobs(2), 10);
        assert_eq!(config.target_simultaneous_mobs(0), 2);
        assert_eq!(config.target_simultaneous_mobs(3), 5);
        assert_eq!(config.ticks_between_spawn, 40);
        assert_eq!(TICKS_BETWEEN_ITEM_SPAWNERS, 160);
        assert_eq!(FullConfig::default().target_cooldown_length, 36_000);
        assert_eq!(FullConfig::default().required_player_range, 14);
    }

    #[test]
    fn one_full_round_goes_through_all_six_states() {
        let pos = BlockPos::new(0, 0, 0);
        let mut spawner = spawner_with(1.0, TrialSpawnerState::WaitingForPlayers);
        let mut fake = Fake::new(0).player(1);
        // 第一次扫描发现玩家；下一次 ACTIVE 时刷出唯一的一只怪。
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::Active);
        assert_eq!(fake.events[0].0, level_event::DETECT_PLAYER);
        assert_eq!(spawner.data.next_mob_spawns_at, 40);
        fake.time = 40;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(fake.spawned.len(), 1);
        assert_eq!(spawner.data.total_mobs_spawned, 1);
        // 怪死了，且到了 20 的整数刻：进入等待奖励。玩家之后已经离开。
        fake.live.clear();
        fake.players.clear();
        fake.time = 80;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::WaitingForRewardEjection);
        assert_eq!(spawner.data.cooldown_ends_at, 80 + 36_000);
        // 40 刻后开闸；弹出节奏是冷却起点之后每 30 刻，所以 140 给唯一的那位玩家弹一次。
        fake.time = 120;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::EjectingReward);
        assert_eq!(fake.shutters, vec![true]);
        fake.time = 140;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(fake.ejected.len(), 1);
        fake.time = 170;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::Cooldown);
        assert_eq!(fake.shutters, vec![true, false]);
        // 冷却结束：清掉不祥标记、重置数据，回到等待玩家。
        fake.time = 80 + 36_000;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::WaitingForPlayers);
        assert!(spawner.data.detected_players.is_empty());
        assert!(spawner.data.current_mobs.is_empty());
    }

    #[test]
    fn additional_players_raise_both_targets() {
        let mut spawner = spawner_with(20.0, TrialSpawnerState::Active);
        let mut fake = Fake::new(0).player(1).player(2).player(3);
        let pos = BlockPos::new(0, 0, 0);
        spawner.tick(pos, false, &mut fake);
        assert_eq!(spawner.data.detected_players.len(), 3);
        assert_eq!(spawner.data.additional_players(), 2);
        assert_eq!(spawner.config.normal.target_total_mobs(2), 24);
        assert_eq!(spawner.config.normal.target_simultaneous_mobs(2), 4);
        // 探测时把下一次刷怪推到 40 刻；到点以后同时最多 4 只，这一刻先刷 1 只。
        fake.time = 40;
        spawner.tick(pos, false, &mut fake);
        assert_eq!(fake.spawned.len(), 1);
    }

    #[test]
    fn scan_only_runs_every_twenty_ticks() {
        let mut spawner = spawner_with(1.0, TrialSpawnerState::WaitingForPlayers);
        let mut fake = Fake::new(1).player(1);
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert_eq!(fake.detected_calls.get(), 0);
        assert_eq!(spawner.state, TrialSpawnerState::WaitingForPlayers);
        fake.time = 20;
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert_eq!(fake.detected_calls.get(), 1);
        assert_eq!(spawner.state, TrialSpawnerState::Active);
    }

    #[test]
    fn bad_omen_switches_to_ominous_and_resets_the_round() {
        let mut spawner = spawner_with(4.0, TrialSpawnerState::Active);
        spawner.data.current_mobs.insert(uuid(9));
        spawner.data.total_mobs_spawned = 3;
        let mut fake = Fake::new(20).player(1);
        fake.players[0].bad_omen_amplifier = Some(1);
        fake.players[0].eye_block = BlockPos::new(1, 65, 2);
        fake.live.insert(uuid(9));
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert!(spawner.ominous);
        assert_eq!(fake.ominous, vec![true]);
        assert_eq!(
            fake.events[0],
            (level_event::OMINOUS_ACTIVATE, BlockPos::new(1, 65, 2), 0)
        );
        assert_eq!(
            fake.events[1],
            (level_event::OMINOUS_ACTIVATE, BlockPos::new(0, 0, 0), 1)
        );
        assert!(spawner.data.current_mobs.is_empty());
        assert_eq!(spawner.data.total_mobs_spawned, 0);
        assert_eq!(
            spawner.data.cooldown_ends_at,
            20 + TICKS_BETWEEN_ITEM_SPAWNERS
        );
        // 不祥激活后每 160 刻生成一个不祥物品。
        fake.time = 180;
        spawner.tick(BlockPos::new(0, 0, 0), true, &mut fake);
        assert_eq!(fake.ominous_items, 1);
        assert_eq!(
            spawner.data.cooldown_ends_at,
            180 + TICKS_BETWEEN_ITEM_SPAWNERS
        );
    }

    #[test]
    fn disk_round_trip_and_update_tag_follow_the_codecs() {
        let mut spawner = TrialSpawner::read(
            &Tag::Compound(vec![
                ("normal_config".to_owned(), config_with_potentials(2.0)),
                ("target_cooldown_length".to_owned(), Tag::Int(100)),
                ("required_player_range".to_owned(), Tag::Int(5)),
                (
                    "registered_players".to_owned(),
                    Tag::List(vec![Tag::IntArray(vec![1, 2, 3, 4])]),
                ),
                (
                    "current_mobs".to_owned(),
                    Tag::List(vec![Tag::IntArray(vec![5, 6, 7, 8])]),
                ),
                ("cooldown_ends_at".to_owned(), Tag::Long(700)),
                ("next_mob_spawns_at".to_owned(), Tag::Long(80)),
                ("total_mobs_spawned".to_owned(), Tag::Int(3)),
                ("spawn_data".to_owned(), spawn_data("minecraft:zombie")),
                (
                    "ejecting_loot_table".to_owned(),
                    Tag::String("minecraft:spawners/trial_chamber/key".to_owned()),
                ),
            ]),
            TrialSpawnerState::Active,
            false,
        );
        let tag = Tag::Compound(spawner.additional());
        let back = TrialSpawner::read(&tag, TrialSpawnerState::Active, false);
        assert_eq!(back.data, spawner.data);
        assert_eq!(back.config.target_cooldown_length, 100);
        assert_eq!(back.config.required_player_range, 5);
        assert_eq!(back.config.normal.target_total_mobs(0), 2);
        assert_eq!(
            field(&tag, "spawn_data"),
            Some(&spawn_data("minecraft:zombie"))
        );
        let update = spawner.update_tag();
        assert_eq!(field(&update, "next_mob_spawns_at"), Some(&Tag::Long(80)));
        assert_eq!(
            field(&update, "spawn_data"),
            Some(&spawn_data("minecraft:zombie"))
        );
        spawner.state = TrialSpawnerState::Cooldown;
        assert!(field(&spawner.update_tag(), "next_mob_spawns_at").is_none());
        // 默认值不写进存档。
        assert!(TrialSpawner::default().additional().is_empty());
    }

    #[test]
    fn malformed_client_data_never_panics() {
        let bad = Tag::Compound(vec![
            ("normal_config".to_owned(), Tag::List(vec![Tag::Byte(1)])),
            (
                "required_player_range".to_owned(),
                Tag::String("x".to_owned()),
            ),
            ("total_mobs_spawned".to_owned(), Tag::String("x".to_owned())),
            ("registered_players".to_owned(), Tag::Byte(3)),
            ("spawn_data".to_owned(), Tag::Int(9)),
        ]);
        let mut spawner = TrialSpawner::read(&bad, TrialSpawnerState::WaitingForPlayers, false);
        let mut fake = Fake::new(0).player(1);
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::Inactive);
        let _ = Vec3::new(0.0, 0.0, 0.0);
    }

    #[test]
    fn inactive_waits_for_a_display_entity() {
        let mut spawner = spawner_with(1.0, TrialSpawnerState::Inactive);
        let mut fake = Fake::new(0);
        fake.display = false;
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::Inactive);
        fake.display = true;
        spawner.tick(BlockPos::new(0, 0, 0), false, &mut fake);
        assert_eq!(spawner.state, TrialSpawnerState::WaitingForPlayers);
    }
}
