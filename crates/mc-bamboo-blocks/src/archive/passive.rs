//! 只保留存档态、还没有游戏行为的方块实体：宝库和三种幽匿方块实体。
//!
//! 官方这四种类型的 `saveAdditional` 无条件写自己的键，哪怕值是构造器默认值。
//! 三个宝库 codec 的字段用 `lenientOptionalFieldOf`（解码后等于默认值的字段在
//! 编码时省略）；`VibrationSystem.Data` 的 `event_delay` 用
//! `optionalAlwaysPresentFieldOf` 永远写，`selector.tick` 是必填的 `Codec.LONG`。
//! 这里照 codec 的规则做「读入 → 写出」往返，不实现开箱、振动监听等行为，
//! 数据结构留着以后接行为。
//!
//! 字段名、`DEFAULT` 和 codec 的省略规则都用 `javap -c -p` 在
//! `VaultBlockEntity` / `VaultConfig` / `VaultSharedData` / `VaultServerData` /
//! `SculkSensorBlockEntity` / `CalibratedSculkSensorBlockEntity` /
//! `SculkShriekerBlockEntity` / `VibrationSystem.Data` 上核对过；坏数据一律按官方
//! 宽松解码回默认值，不 panic。
//!
//! Block entities that keep only their persisted state: the vault and the three
//! sculk blocks. Vanilla always writes their keys; this module round-trips them
//! through the codecs' omission rules without implementing gameplay behaviour.

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, items::ITEMS};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::{archive::slots, cursor::parse_identifier};

use super::tag::{self, Error, Result, put};
use crate::{BlockEntity, BlockState, World};

/// 官方 `VaultConfig.DEFAULT` 的 `keyItem` 是一把试炼钥匙；`ItemStack.CODEC`
/// 的 `count` 用 `optionalAlwaysPresentFieldOf`，所以 1 也要写出来。
fn default_key_item() -> Tag {
    Tag::Compound(vec![
        (
            "id".to_owned(),
            Tag::String(
                ITEMS
                    .get(items::TRIAL_KEY as usize)
                    .map_or("minecraft:trial_key", |item| item.name)
                    .to_owned(),
            ),
        ),
        ("count".to_owned(), Tag::Int(1)),
    ])
}

/// 官方 `VaultConfig.DEFAULT` 的三个可省字段默认值。
const DEFAULT_LOOT_TABLE: &str = "minecraft:chests/trial_chambers/reward";
const DEFAULT_ACTIVATION_RANGE: f64 = 4.0;
const DEFAULT_DEACTIVATION_RANGE: f64 = 4.5;
/// `VaultSharedData` 的 `connectedParticlesRange` 初值取 `VaultConfig.DEFAULT`。
const DEFAULT_PARTICLES_RANGE: f64 = 4.5;

/// 只保留存档态的四种类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassiveKind {
    Vault,
    SculkSensor,
    CalibratedSculkSensor,
    SculkShrieker,
}

impl PassiveKind {
    /// 按官方 `minecraft:block_entity_type` 的注册名找类型。
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "minecraft:vault" => Self::Vault,
            "minecraft:sculk_sensor" => Self::SculkSensor,
            "minecraft:calibrated_sculk_sensor" => Self::CalibratedSculkSensor,
            "minecraft:sculk_shrieker" => Self::SculkShrieker,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Vault => "minecraft:vault",
            Self::SculkSensor => "minecraft:sculk_sensor",
            Self::CalibratedSculkSensor => "minecraft:calibrated_sculk_sensor",
            Self::SculkShrieker => "minecraft:sculk_shrieker",
        }
    }

    /// 测试用它盯住工厂分支不要漏。
    pub const ALL: [Self; 4] = [
        Self::Vault,
        Self::SculkSensor,
        Self::CalibratedSculkSensor,
        Self::SculkShrieker,
    ];
}

/// 官方 codec 往返后的存档字段。
#[derive(Debug, Clone, PartialEq)]
struct Data {
    config: Tag,
    shared_data: Tag,
    server_data: Tag,
    listener: Tag,
    frequency: i32,
    warning_level: i32,
}

impl Data {
    fn new() -> Self {
        Self {
            // 新建宝库的 `keyItem` 不是空物品堆，codec 要写出钥匙。
            config: Tag::Compound(vec![("key_item".to_owned(), default_key_item())]),
            shared_data: Tag::Compound(Vec::new()),
            server_data: Tag::Compound(Vec::new()),
            listener: default_listener(),
            frequency: 0,
            warning_level: 0,
        }
    }
}

/// 一个只保存存档态的方块实体。没有 tick 行为，外部改动才叫醒它。
#[derive(Debug)]
pub struct PassiveBlockEntity {
    pos: BlockPos,
    kind: PassiveKind,
    data: Data,
}

impl PassiveBlockEntity {
    /// 按 `id` 从官方条目的 NBT 还原。`state` 暂时用不上，留着给以后接行为。
    pub fn read(kind: PassiveKind, pos: BlockPos, _state: BlockState, tag: &Tag) -> Result<Self> {
        let mut data = Data::new();
        match kind {
            PassiveKind::Vault => {
                data.config = read_vault_config(field(tag, "config"))?;
                data.shared_data = read_vault_shared(field(tag, "shared_data"))?;
                data.server_data = read_vault_server(field(tag, "server_data"))?;
            }
            PassiveKind::SculkSensor | PassiveKind::CalibratedSculkSensor => {
                data.frequency = tag::int_or(tag, "last_vibration_frequency", 0);
                data.listener = read_listener(field(tag, "listener"))?;
            }
            PassiveKind::SculkShrieker => {
                data.warning_level = tag::int_or(tag, "warning_level", 0);
                data.listener = read_listener(field(tag, "listener"))?;
            }
        }
        Ok(Self { pos, kind, data })
    }

    /// 官方 `saveAdditional` 写出的键，不含 `id` / 坐标 / `components`。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        match self.kind {
            PassiveKind::Vault => {
                put(&mut out, "config", self.data.config.clone());
                put(&mut out, "shared_data", self.data.shared_data.clone());
                put(&mut out, "server_data", self.data.server_data.clone());
            }
            PassiveKind::SculkSensor | PassiveKind::CalibratedSculkSensor => {
                put(
                    &mut out,
                    "last_vibration_frequency",
                    Tag::Int(self.data.frequency),
                );
                put(&mut out, "listener", self.data.listener.clone());
            }
            PassiveKind::SculkShrieker => {
                put(&mut out, "warning_level", Tag::Int(self.data.warning_level));
                put(&mut out, "listener", self.data.listener.clone());
            }
        }
        out
    }
}

impl BlockEntity for PassiveBlockEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, _world: &mut World) {}

    /// 没有可见行为，睡到外部改动叫醒；避免每刻白跑一个空 ticker。
    fn sleep_from(&mut self, _now: i64) -> Option<i64> {
        Some(i64::MAX)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>> {
        Ok(self.additional())
    }
}

/// 官方构造器默认值：`saveAdditional` 无条件写出这一组键。
///
/// The keys vanilla's constructors write unconditionally.
pub fn default_additional(id: &str) -> Option<Vec<(String, Tag)>> {
    let kind = PassiveKind::from_name(id)?;
    let entity = PassiveBlockEntity {
        pos: BlockPos::new(0, 0, 0),
        kind,
        data: Data::new(),
    };
    Some(entity.additional())
}

/// 复合标签里按名字查字段，同名键取最后一个。
fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    tag::field(tag, key)
}

/// 复合标签的条目数组里按名字查字段，同名键取最后一个。
fn entry<'a>(values: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    values
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// `ResourceKey` 的编解码器：补全命名空间后仍等于默认值时省略。
fn identifier(tag: &Tag) -> Option<String> {
    let Tag::String(text) = tag else {
        return None;
    };
    parse_identifier(text).ok()
}

// ---------------------------------------------------------------------------
// VaultConfig.CODEC / VaultSharedData.CODEC / VaultServerData.CODEC
// ---------------------------------------------------------------------------

/// `VaultConfig.CODEC`：五个 `lenientOptionalFieldOf` 字段，等于默认值时省略。
/// `validate` 要求激活范围不大于失效范围，违反时整个配置回到 `DEFAULT`。
fn read_vault_config(input: Option<&Tag>) -> Result<Tag> {
    let Some(Tag::Compound(values)) = input else {
        return Ok(Tag::Compound(vec![(
            "key_item".to_owned(),
            default_key_item(),
        )]));
    };
    let mut out = Vec::new();
    optional_string(values, &mut out, "loot_table", DEFAULT_LOOT_TABLE);
    let activation = optional_double(
        values,
        &mut out,
        "activation_range",
        DEFAULT_ACTIVATION_RANGE,
    );
    let deactivation = optional_double(
        values,
        &mut out,
        "deactivation_range",
        DEFAULT_DEACTIVATION_RANGE,
    );
    // `VaultConfig.CODEC` 解码时走的是五参数构造器：缺 `key_item` 得到
    // `ItemStack.EMPTY`，编码时省掉这个键。只有整份 `config` 不存在（回到
    // `VaultConfig.DEFAULT`）才写出默认的那把试炼钥匙。
    // A present `config` compound without `key_item` decodes to EMPTY and omits
    // the key; only a missing `config` falls back to `VaultConfig.DEFAULT`.
    if let Some(stack) = optional_item(values, "key_item")? {
        put(&mut out, "key_item", stack);
    }
    // `Optional<ResourceKey>`：没有值（或坏值）时省略。
    if let Some(text) = entry(values, "override_loot_table_to_display").and_then(identifier) {
        put(
            &mut out,
            "override_loot_table_to_display",
            Tag::String(text),
        );
    }
    if activation > deactivation {
        return Ok(Tag::Compound(vec![(
            "key_item".to_owned(),
            default_key_item(),
        )]));
    }
    Ok(Tag::Compound(out))
}

/// `VaultSharedData.CODEC`：展示物品、已连接玩家、粒子范围。
fn read_vault_shared(input: Option<&Tag>) -> Result<Tag> {
    let Some(Tag::Compound(values)) = input else {
        return Ok(Tag::Compound(Vec::new()));
    };
    let mut out = Vec::new();
    if let Some(stack) = optional_item(values, "display_item")? {
        put(&mut out, "display_item", stack);
    }
    optional_uuid_set(values, &mut out, "connected_players");
    optional_double(
        values,
        &mut out,
        "connected_particles_range",
        DEFAULT_PARTICLES_RANGE,
    );
    Ok(Tag::Compound(out))
}

/// `VaultServerData.CODEC`：奖励玩家、状态恢复时刻、待弹出物品、总弹出次数。
fn read_vault_server(input: Option<&Tag>) -> Result<Tag> {
    let Some(Tag::Compound(values)) = input else {
        return Ok(Tag::Compound(Vec::new()));
    };
    let mut out = Vec::new();
    optional_uuid_set(values, &mut out, "rewarded_players");
    if let Some(tag) = entry(values, "state_updating_resumes_at") {
        let value = tag::java_i64(tag).unwrap_or(0);
        if value != 0 {
            put(&mut out, "state_updating_resumes_at", Tag::Long(value));
        }
    }
    if let Some(items) = entry(values, "items_to_eject").map(item_list)
        && !items.is_empty()
    {
        put(&mut out, "items_to_eject", Tag::List(items));
    }
    // `loadAdditional` 用 `serverData.set(decoded)`：只搬 `stateUpdatingResumesAt`、
    // `itemsToEject` 和 `rewardedPlayers`，`totalEjectionsNeeded` 仍是构造器的 0。
    // `VaultServerData.set` does not copy `totalEjectionsNeeded`; it stays at 0
    // after a load, so the codec omits the key.
    Ok(Tag::Compound(out))
}

/// `LootTable.KEY_CODEC` / `ResourceKey`：编码成字符串，等于默认值时省略。
fn optional_string(
    values: &[(String, Tag)],
    out: &mut Vec<(String, Tag)>,
    key: &str,
    default: &str,
) {
    if let Some(text) = entry(values, key).and_then(identifier)
        && text != default
    {
        put(out, key, Tag::String(text));
    }
}

/// `Codec.DOUBLE.lenientOptionalFieldOf`：任意数值标签，按 Java 的 `doubleValue()` 取值。
/// 返回解出来的值（没写或坏数据就是默认值），给 `VaultConfig.validate` 用。
fn optional_double(
    values: &[(String, Tag)],
    out: &mut Vec<(String, Tag)>,
    key: &str,
    default: f64,
) -> f64 {
    let value = entry(values, key)
        .and_then(|tag| tag::java_f64(tag).ok())
        .unwrap_or(default);
    if value.to_bits() != default.to_bits() {
        put(out, key, Tag::Double(value));
    }
    value
}

/// `ItemStack.lenientOptionalFieldOf`：空物品堆省略，非空的按 `ItemStack.CODEC` 重新编码。
fn optional_item(values: &[(String, Tag)], key: &str) -> Result<Option<Tag>> {
    let Some(tag) = entry(values, key) else {
        return Ok(None);
    };
    let Ok(stack) = slots::decode_item(tag) else {
        // lenient：解不开的条目按空处理，不 panic。
        return Ok(None);
    };
    if stack.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        slots::encode_item(&stack).map_err(|e| Error::new(e.to_string()))?,
    ))
}

/// `ItemStack.CODEC.listOf()`：一条解不开就让整个列表回到空列表，和
/// `lenientOptionalFieldOf(空列表)` 的解码结果一致。
fn item_list(tag: &Tag) -> Vec<Tag> {
    let Tag::List(values) = tag else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for value in values {
        let Ok(stack) = slots::decode_item(value) else {
            return Vec::new();
        };
        let Ok(encoded) = slots::encode_item(&stack) else {
            return Vec::new();
        };
        out.push(encoded);
    }
    out
}

/// `UUIDUtil.CODEC_LINKED_SET.lenientOptionalFieldOf(空集)`：`List<IntArray>`，
/// 空集省略；一条坏数据让整个集合回到空集。
fn optional_uuid_set(values: &[(String, Tag)], out: &mut Vec<(String, Tag)>, key: &str) {
    let Some(Tag::List(items)) = entry(values, key) else {
        return;
    };
    let mut written = Vec::new();
    for item in items {
        let Tag::IntArray(parts) = item else {
            return;
        };
        if parts.len() != 4 {
            return;
        }
        written.push(Tag::IntArray(parts.clone()));
    }
    if !written.is_empty() {
        put(out, key, Tag::List(written));
    }
}

// ---------------------------------------------------------------------------
// VibrationSystem.Data.CODEC / VibrationSelector.CODEC / VibrationInfo.CODEC
// ---------------------------------------------------------------------------

/// 新建的 `Data`：`{event_delay:0,selector:{tick:-1L}}`，键序照
/// `VibrationSystem.Data.CODEC` 的 `RecordCodecBuilder.group(event, selector, event_delay)`。
fn default_listener() -> Tag {
    Tag::Compound(vec![
        (
            "selector".to_owned(),
            Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
        ),
        ("event_delay".to_owned(), Tag::Int(0)),
    ])
}

/// `VibrationSystem.Data.CODEC`：`event` 可省、`selector` 必写、
/// `event_delay` 用 `optionalAlwaysPresentFieldOf` 永远写。任何必填字段解码
/// 失败时 `loadAdditional` 的 `orElseGet(Data::new)` 让整个监听器回到默认值。
fn read_listener(input: Option<&Tag>) -> Result<Tag> {
    let Some(Tag::Compound(values)) = input else {
        return Ok(default_listener());
    };
    let Some(selector) = read_selector(entry(values, "selector")) else {
        return Ok(default_listener());
    };
    let delay = match entry(values, "event_delay") {
        Some(tag) => match tag::java_i32(tag) {
            // `ExtraCodecs.NON_NEGATIVE_INT`。
            Ok(value) if value >= 0 => value,
            _ => return Ok(default_listener()),
        },
        None => 0,
    };
    let mut out = Vec::new();
    // `event` 是 lenientOptionalFieldOf，坏数据只丢这一个字段。
    if let Some(event) = vibration_event(entry(values, "event")) {
        put(&mut out, "event", event);
    }
    put(&mut out, "selector", selector);
    put(&mut out, "event_delay", Tag::Int(delay));
    Ok(Tag::Compound(out))
}

/// `VibrationSystem.Data` 的运行时状态：codec 读完就交给方块实体的 ticker。
/// 按 codec 读 `listener`，坏数据一律回到默认值。行为层从这里还原飞行中的振动。
pub fn read_listener_for_vibration(tag: &Tag) -> crate::game_event::vibration::VibrationData {
    use crate::game_event::vibration::{VibrationData, VibrationInfo, VibrationSelector};
    let Tag::Compound(values) = tag else {
        return VibrationData::new();
    };
    let current = entry(values, "event").and_then(VibrationInfo::read_codec);
    let Some(Tag::Compound(selector_values)) = entry(values, "selector") else {
        return VibrationData::new();
    };
    let candidate = entry(selector_values, "event").and_then(VibrationInfo::read_codec);
    let Some(tick) = entry(selector_values, "tick").and_then(|tag| tag::java_i64(tag).ok()) else {
        return VibrationData::new();
    };
    let mut selector = VibrationSelector::default();
    if let Some(info) = candidate {
        selector.add_candidate(info, tick);
    }
    let Some(travel) = entry(values, "event_delay").and_then(|tag| tag::java_i32(tag).ok()) else {
        return VibrationData::new();
    };
    if travel < 0 {
        return VibrationData::new();
    }
    VibrationData::from_codec(current, selector, travel)
}

/// `VibrationSelector.CODEC`：`event` 可省，`tick` 是必写的 `Codec.LONG`。
fn read_selector(input: Option<&Tag>) -> Option<Tag> {
    let Tag::Compound(values) = input? else {
        return None;
    };
    let event = vibration_event(entry(values, "event"));
    // `VibrationSelector.CODEC` 解码时从 `Optional(event)` 和 `tick` 造
    // `VibrationEvent`；没有 `event` 时 `currentVibrationData` 为空，编码的
    // `tick` 恒为 -1L（`lambda$static$2` 的 `orElse`）。
    // Without an event the selector's `tick` getter returns -1L, no matter
    // what was on disk.
    let tick = if event.is_some() {
        let tag = entry(values, "tick")?;
        tag::java_i64(tag).ok()?
    } else {
        -1
    };
    let mut out = Vec::new();
    if let Some(event) = event {
        put(&mut out, "event", event);
    }
    put(&mut out, "tick", Tag::Long(tick));
    Some(Tag::Compound(out))
}

/// `VibrationInfo.CODEC`：`game_event` 是 `GameEvent.CODEC`（注册表 holder），
/// 名字必须是 `minecraft:game_event` 里注册过的条目；`distance` / `pos` 必写，
/// `source` / `projectile_owner` 用 `lenientOptionalFieldOf` 可省。
fn vibration_event(input: Option<&Tag>) -> Option<Tag> {
    let Tag::Compound(values) = input? else {
        return None;
    };
    let game_event = identifier(entry(values, "game_event")?)?;
    // `RegistryCodecs.holder` 解码失败时整个 `VibrationInfo` 解码失败，`lenient`
    // 字段把事件丢掉；这里照官方做法只按注册表接受名字，不抄注册表内容进存档。
    mc_bamboo_data::registries::game_event::by_name(&game_event)?;
    let distance = tag::java_f32(entry(values, "distance")?).ok()?;
    if !(0.0..=f32::MAX).contains(&distance) {
        return None;
    }
    let Tag::List(pos) = entry(values, "pos")? else {
        return None;
    };
    if pos.len() != 3 {
        return None;
    }
    let mut coords = Vec::with_capacity(3);
    for value in pos {
        coords.push(Tag::Double(tag::java_f64(value).ok()?));
    }
    let mut out = Vec::new();
    put(&mut out, "game_event", Tag::String(game_event));
    put(&mut out, "distance", Tag::Float(distance));
    put(&mut out, "pos", Tag::List(coords));
    for key in ["source", "projectile_owner"] {
        if let Some(Tag::IntArray(parts)) = entry(values, key)
            && parts.len() == 4
        {
            put(&mut out, key, Tag::IntArray(parts.clone()));
        }
    }
    Some(Tag::Compound(out))
}
