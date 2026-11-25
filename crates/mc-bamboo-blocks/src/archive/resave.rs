//! 只保留存档态的方块实体：`saveAdditional` / `loadAdditional` 的纯函数往返。
//! Pure `saveAdditional` / `loadAdditional` round trips for state-only block entities.
//!
//! 官方读一条 `block_entities` 条目时按 `id` 造具体方块实体，写盘时无条件写出自己的
//! 键（有些字段按 codec 的省略规则可省）。这里照同一份规则做「读入 → 写出」，不改变
//! 方块实体在游戏里的行为：调用方拿返回的键覆盖建模条目，未知键仍按 D9 原样保留。
//! Vanilla builds a block entity per `id` and writes its own keys (some optional by
//! codec rules). This module replays the same load/save pair; unmodeled unknown keys
//! stay preserved under D9 and no gameplay behaviour lives here.
//!
//! 一个键只有在游戏层运行时表确实拥有它、且存盘时会写出当前值，才能放进 `omit`
//! 交给游戏层；否则本模块必须自己按官方 codec 把读盘的值带回去（有值就写，空 /
//! 默认按官方规则省略），不能把「不写」推给一个实际不写的上层。
//! A key may be omitted only when a runtime table in the game layer owns it and
//! writes its current value on save. Otherwise this module must carry the disk value
//! through the official codec itself, never delegating omission to a layer that does
//! not write the key.
//!
//! 世界生成只放下方块、没有读盘条目时用空复合标签跑同一份 `resave_additional`；
//! 宝库 / 幽匿那四种没有本模块的分支，由 [`default_additional`] 补构造器默认值。
//! Generated blocks without a disk entry run the same function on an empty compound;
//! the four passive vault/sculk types use `default_additional`.
//!
//! 字段名、默认值和 codec 的省略规则都用 `javap -c -p` 在
//! `BaseSpawner` / `SignBlockEntity` / `SignText` / `BeehiveBlockEntity` /
//! `JigsawBlockEntity` / `TheEndGatewayBlockEntity` / `SculkCatalystBlockEntity` /
//! `BeaconBlockEntity` / `PotentSulfurBlockEntity` / `StructureBlockEntity` /
//! `ShelfBlockEntity` / `CommandBlockEntity` / `JukeboxBlockEntity` /
//! `DecoratedPotBlockEntity` / `ShulkerBoxBlockEntity` / `RandomizableContainer`
//! 上核对过；坏数据一律按官方宽松解码回默认值，不 panic。
//! Field names, defaults and codec omission rules were checked with `javap -c -p`
//! against those official classes; malformed data falls back leniently, never panics.

use mc_bamboo_data::registries::data_component_type as component_kinds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::cursor::parse_identifier;
use mc_bamboo_protocol::components::{
    ComponentValue,
    archive::{components, slots, text},
};

use super::tag::{self, put};
use crate::{
    BlockState,
    behaviour::trial_spawner::TrialSpawner,
    state::{OMINOUS, ORIENTATION, TRIAL_SPAWNER_STATE},
};

/// 一条读盘条目往返后的结果。
/// The result of round-tripping one disk entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Resaved {
    /// 官方 `saveAdditional` 会写出的键和值。
    /// The keys and values written by vanilla's `saveAdditional`.
    pub fields: Vec<(String, Tag)>,
    /// 官方认识、但这一状态下不写的键，要从原样保留的条目里删掉。
    /// 不在这张表里的未知键仍按 D9 原样保留。
    /// Official optional keys absent in this state; unknown keys stay under D9.
    pub omit: Vec<&'static str>,
}

impl Resaved {
    fn new(fields: Vec<(String, Tag)>, omit: &'static [&'static str]) -> Self {
        Self {
            fields,
            omit: omit.to_vec(),
        }
    }
}

/// 按 `id` 还原官方 `loadStatic → saveWithFullMetadata` 写出的自有键。
/// Rebuild the keys written by vanilla's `loadStatic → saveWithFullMetadata`.
///
/// `state` 只用于推导 jigsaw 缺省 `joint` 的方向；`raw` 是读盘时的整条复合标签，
/// 没有读盘条目时传空复合标签。返回 `None` 表示这个类型不需要本模块补键。
/// 正式编码路径、`block_entity_resave` 测试和 `chunk-content` 对拍共用这一个函数。
/// `state` only derives the default jigsaw joint; `raw` is the disk entry (empty
/// when generated). Both the encoder and the oracle use this one function.
pub fn resave_additional(id: &str, state: BlockState, raw: &Tag) -> Option<Resaved> {
    let (fields, omit) = match id {
        // 刷怪笼由活方块实体读写；重存走同一条 `read` → `saveAdditional` 路径，
        // 存档格式只有一份实现。
        // Spawners are read and written by the live block entity; resaving goes
        // through the same `read` then `saveAdditional` path, so there is one format.
        "minecraft:mob_spawner" => (
            crate::behaviour::machines::spawner::SpawnerBlockEntity::read(
                mc_bamboo_core::BlockPos::new(0, 0, 0),
                raw,
            )
            .additional(),
            SPAWNER_OMIT,
        ),
        // 试炼刷怪笼的 `state` / `ominous` 来自方块状态，其余两组 codec 读同一条目；
        // 重存和活实例走同一份 `TrialSpawner::additional`，存档格式只有一份实现。
        // The trial spawner's state and ominous come from the block state; the
        // other two codecs share one entry. Resaving and the live entity use the
        // same `TrialSpawner::additional`, so there is one format.
        "minecraft:trial_spawner" => (
            TrialSpawner::read(raw, state.get(TRIAL_SPAWNER_STATE), state.get(OMINOUS))
                .additional(),
            &[][..],
        ),
        "minecraft:sign" | "minecraft:hanging_sign" => (sign(raw), SIGN_OMIT),
        // 蜂巢由活方块实体读写；重存走同一条 `read` → `saveAdditional` 路径，
        // 存档格式只有一份实现。
        // Beehives are read and written by the live block entity; resaving goes
        // through the same `read` then `saveAdditional` path, so there is one format.
        "minecraft:beehive" => (
            crate::behaviour::machines::beehive_entity::BeehiveEntity::read(
                mc_bamboo_core::BlockPos::new(0, 0, 0),
                raw,
            )
            .additional(),
            BEEHIVE_OMIT,
        ),
        "minecraft:jigsaw" => (jigsaw(raw, state), &[][..]),
        "minecraft:end_gateway" => (end_gateway(raw), &[][..]),
        "minecraft:sculk_catalyst" => (sculk_catalyst(raw), &[][..]),
        "minecraft:beacon" => (beacon(), &[][..]),
        "minecraft:potent_sulfur" => (potent_sulfur(raw), &[][..]),
        "minecraft:structure_block" => (structure_block(raw), &[][..]),
        "minecraft:ender_chest" => (Vec::new(), ENDER_CHEST_OMIT),
        "minecraft:jukebox" => (jukebox(raw), JUKEBOX_OMIT),
        "minecraft:shulker_box" => (shulker_box(raw), SHULKER_BOX_OMIT),
        "minecraft:decorated_pot" => (decorated_pot(raw), DECORATED_POT_OMIT),
        "minecraft:shelf" => (shelf(raw), &[][..]),
        "minecraft:command_block" => (command_block(raw), COMMAND_BLOCK_OMIT),
        "minecraft:test_block" => (test_block(raw), TEST_BLOCK_OMIT),
        "minecraft:test_instance_block" => (test_instance_block(raw), TEST_INSTANCE_OMIT),
        // `CopperGolemStatueBlockEntity` 不覆写 `saveAdditional`：自定义名在 `components`
        // 组件表里，是元数据键，由游戏层的组件补丁写；这里没有任何自有键。
        // A copper golem statue overrides no `saveAdditional`: its custom name lives in
        // the `components` metadata key, written by the game layer's component patch.
        "minecraft:copper_golem_statue" => (Vec::new(), &[][..]),
        _ => return None,
    };
    Some(Resaved::new(fields, omit))
}

/// 蜂巢的 `flower_pos` 只在有值时写。
/// A beehive writes `flower_pos` only when it has one.
const BEEHIVE_OMIT: &[&str] = &["flower_pos"];

/// 官方认识、可选且当前状态不写的键：编码时从读盘条目里删掉，避免带上旧值。
/// 不在这张表里的未知键按 D9 原样保留。
/// Official optional keys that are absent in this state; unknown keys stay under D9.
///
/// 这些键都由本模块自己写回（见各自 codec 函数），所以只有「运行时表确实拥有」的
/// 少数键才放进 omit。潜影盒 / 唱片机 / 陶罐的内容键没有运行时写入者：容器表读盘时
/// 不收这四类（`read_game_fields`），组件补丁只拥有 `sherds`，红石表只拥有
/// `ticks_since_song_started`；其余都由 `jukebox` / `shulker_box` /
/// `decorated_pot` 各自写回。
/// Most of these keys are written by this module. The container table does not load
/// these four types, the component patch owns only `sherds`, and the redstone table
/// owns only `ticks_since_song_started`; the rest come from each codec function.
const SPAWNER_OMIT: &[&str] = &["SpawnData"];
const SIGN_OMIT: &[&str] = &["allow_op_features"];
const JUKEBOX_OMIT: &[&str] = &[];
const DECORATED_POT_OMIT: &[&str] = &[];
const SHULKER_BOX_OMIT: &[&str] = &[];
/// 末影箱的物品在玩家身上，官方 `EnderChestBlockEntity` 不覆写 `saveAdditional`；
/// 读盘条目里的物品键按官方丢掉。
/// An ender chest stores its items on the player, so vanilla drops these disk keys.
const ENDER_CHEST_OMIT: &[&str] = &["Items", "LootTable", "LootTableSeed"];
/// `CustomName` 是 D9 原样保留的未知样式字符串，游戏层组件补丁只在能解码时
/// 才写当前值；这里不删它，`LastOutput` / `LastExecution` 由 `command_block`
/// 按官方条件写回。
/// The component patch only writes `CustomName` when it decodes; leave the raw
/// value under D9 and write the other two conditional keys here.
const COMMAND_BLOCK_OMIT: &[&str] = &[];
/// 测试方块没有可省键；测试实例方块的 `errors` 只在非空时写。
/// Test blocks have no optional keys; test instances write `errors` only when non-empty.
const TEST_BLOCK_OMIT: &[&str] = &[];
const TEST_INSTANCE_OMIT: &[&str] = &[];

/// 世界生成放置、没有读盘条目时的构造器默认值。
/// Constructor defaults for generated blocks without a disk entry.
pub fn default_additional(id: &str) -> Option<Vec<(String, Tag)>> {
    // 蜂巢的 `bees` 由活方块实体写；生成期放下的蜂巢没有读盘条目，官方仍无条件
    // 写空列表。这里补构造器缺省值，避免没有实例的生成期条目漏掉这个键。
    // A generated beehive has no disk entry; vanilla still writes the empty `bees`
    // list, so supply the constructor default here.
    if id == "minecraft:beehive" {
        return Some(vec![("bees".to_owned(), Tag::List(Vec::new()))]);
    }
    // 本模块负责的类型（含生成期无条目）都走 `resave_additional(空复合标签)`；
    // 这里只留上一轮那四种只保留存档态实体的构造器默认值。
    // Types here use `resave_additional(empty)`; only the four passive types need
    // their own constructor defaults.
    super::passive::default_additional(id)
}

/// 复合标签里的条目按名字查，同名键取最后一个。
/// Look up a compound field; a duplicate key uses the last value.
fn entry<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    tag::field(tag, key)
}

fn bool_tag(tag: &Tag, key: &str, default: bool) -> Tag {
    Tag::Byte(i8::from(tag::bool_or(tag, key, default)))
}

fn string_tag(tag: &Tag, key: &str) -> Tag {
    Tag::String(tag::string_or(tag, key).unwrap_or("").to_owned())
}

/// `Identifier.CODEC`：解不开的名字回到调用方给的缺省值。
/// An unparsable identifier falls back to the caller's default.
fn identifier_or(tag: &Tag, key: &str, default: &str) -> Tag {
    Tag::String(
        entry(tag, key)
            .and_then(|value| match value {
                Tag::String(text) => parse_identifier(text).ok(),
                _ => None,
            })
            .unwrap_or_else(|| default.to_owned()),
    )
}

/// `ContainerHelper.saveAllItems` 的 NBT 形状：去掉空堆、越界槽位和坏条目，
/// 空列表按 `keepEmpty` 决定是否保留（调用方决定要不要写这个键）。
/// The NBT shape of `ContainerHelper.saveAllItems`: drop empty stacks, out-of-range
/// slots and bad entries; the caller decides whether an empty list is written.
fn items(value: Option<&Tag>, size: usize) -> Tag {
    let Some(Tag::List(_)) = value else {
        return Tag::List(Vec::new());
    };
    let decoded = slots::decode_list(value.unwrap(), size);
    let slotted: Vec<_> = decoded
        .items
        .into_iter()
        .filter(|item| !item.stack.is_empty())
        .collect();
    slots::encode_list(&slotted).unwrap_or(Tag::List(Vec::new()))
}

// ---------------------------------------------------------------------------
// SignBlockEntity / HangingSignBlockEntity
// ---------------------------------------------------------------------------

fn blank_sign_text() -> Tag {
    Tag::Compound(vec![
        (
            "messages".to_owned(),
            Tag::List(vec![Tag::String(String::new()); 4]),
        ),
        ("color".to_owned(), Tag::String("black".to_owned())),
        ("has_glowing_text".to_owned(), Tag::Byte(0)),
    ])
}

fn sign_text(input: Option<&Tag>) -> Tag {
    let Some(Tag::Compound(_)) = input else {
        return blank_sign_text();
    };
    let tag = input.unwrap();
    let mut messages = Vec::new();
    if let Some(Tag::List(lines)) = entry(tag, "messages") {
        for line in lines.iter().take(4) {
            messages.push(match line {
                Tag::String(text) => text.clone(),
                _ => String::new(),
            });
        }
    }
    while messages.len() < 4 {
        messages.push(String::new());
    }
    Tag::Compound(vec![
        (
            "messages".to_owned(),
            Tag::List(messages.into_iter().map(Tag::String).collect()),
        ),
        (
            "color".to_owned(),
            Tag::String(
                tag::string_or(tag, "color")
                    .filter(|color| {
                        matches!(
                            *color,
                            "white"
                                | "orange"
                                | "magenta"
                                | "light_blue"
                                | "yellow"
                                | "lime"
                                | "pink"
                                | "gray"
                                | "light_gray"
                                | "cyan"
                                | "purple"
                                | "blue"
                                | "brown"
                                | "green"
                                | "red"
                                | "black"
                        )
                    })
                    .unwrap_or("black")
                    .to_owned(),
            ),
        ),
        (
            "has_glowing_text".to_owned(),
            bool_tag(tag, "has_glowing_text", false),
        ),
    ])
}

fn sign(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    // `allow_op_features` 只在真时写。
    // `allow_op_features` is written only when true.
    if tag::bool_or(raw, "allow_op_features", false) {
        put(&mut out, "allow_op_features", Tag::Byte(1));
    }
    put(&mut out, "front_text", sign_text(entry(raw, "front_text")));
    put(&mut out, "back_text", sign_text(entry(raw, "back_text")));
    put(&mut out, "is_waxed", bool_tag(raw, "is_waxed", false));
    out
}

// ---------------------------------------------------------------------------
// JigsawBlockEntity
// ---------------------------------------------------------------------------

fn jigsaw(raw: &Tag, state: BlockState) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    put(
        &mut out,
        "name",
        identifier_or(raw, "name", "minecraft:empty"),
    );
    put(
        &mut out,
        "target",
        identifier_or(raw, "target", "minecraft:empty"),
    );
    put(
        &mut out,
        "pool",
        identifier_or(raw, "pool", "minecraft:empty"),
    );
    put(
        &mut out,
        "final_state",
        Tag::String(
            tag::string_or(raw, "final_state")
                .unwrap_or("minecraft:air")
                .to_owned(),
        ),
    );
    // `JointType.CODEC` 解不开时按方块状态 `front` 的轴向推导：
    // 水平是 aligned，竖直是 rollable（`StructureTemplate.getDefaultJointType`）。
    // An invalid joint falls back to the front-facing axis: horizontal is aligned,
    // vertical is rollable.
    let default_joint = match state.try_get(ORIENTATION).map(|value| value.name()) {
        Some(
            "down_east" | "down_north" | "down_south" | "down_west" | "up_east" | "up_north"
            | "up_south" | "up_west",
        ) => "rollable",
        _ => "aligned",
    };
    let joint = tag::string_or(raw, "joint")
        .filter(|value| matches!(*value, "aligned" | "rollable"))
        .unwrap_or(default_joint);
    put(&mut out, "joint", Tag::String(joint.to_owned()));
    put(
        &mut out,
        "placement_priority",
        Tag::Int(tag::int_or(raw, "placement_priority", 0)),
    );
    put(
        &mut out,
        "selection_priority",
        Tag::Int(tag::int_or(raw, "selection_priority", 0)),
    );
    out
}

// ---------------------------------------------------------------------------
// TheEndGatewayBlockEntity
// ---------------------------------------------------------------------------

fn end_gateway(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = vec![("Age".to_owned(), Tag::Long(tag::long_or(raw, "Age", 0)))];
    // `exit_portal` 是 storeNullable 的 BlockPos.CODEC（三个 int 的 IntArray）。
    // `exit_portal` is storeNullable with a three-int array BlockPos codec.
    if let Some(Tag::IntArray(pos)) = entry(raw, "exit_portal")
        && pos.len() == 3
    {
        put(&mut out, "exit_portal", Tag::IntArray(pos.clone()));
    }
    if tag::bool_or(raw, "ExactTeleport", false) {
        put(&mut out, "ExactTeleport", Tag::Byte(1));
    }
    out
}

// ---------------------------------------------------------------------------
// SculkCatalystBlockEntity
// ---------------------------------------------------------------------------

fn sculk_catalyst(raw: &Tag) -> Vec<(String, Tag)> {
    vec![("cursors".to_owned(), charge_cursors(entry(raw, "cursors")))]
}

/// `ChargeCursor.CODEC`：`pos` 必填，其余四个字段有默认值/范围。
/// `Codec.listOf` 里一条坏数据让整个列表解码失败，`load` 回到空列表。
/// `pos` is required; one bad entry resets the whole list to empty.
fn charge_cursors(input: Option<&Tag>) -> Tag {
    let Some(Tag::List(values)) = input else {
        return Tag::List(Vec::new());
    };
    if values.len() > 32 {
        return Tag::List(Vec::new());
    }
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Tag::Compound(_) = value else {
            return Tag::List(Vec::new());
        };
        let Some(Tag::IntArray(pos)) = entry(value, "pos") else {
            return Tag::List(Vec::new());
        };
        if pos.len() != 3 {
            return Tag::List(Vec::new());
        }
        let charge = tag::int_or(value, "charge", 0);
        let decay = tag::int_or(value, "decay_delay", 1);
        let update = tag::int_or(value, "update_delay", 0);
        if !(0..=1000).contains(&charge) || !(0..=1).contains(&decay) || update < 0 {
            return Tag::List(Vec::new());
        }
        let mut one = Vec::new();
        put(&mut one, "pos", Tag::IntArray(pos.clone()));
        if charge != 0 {
            put(&mut one, "charge", Tag::Int(charge));
        }
        if decay != 1 {
            put(&mut one, "decay_delay", Tag::Int(decay));
        }
        if update != 0 {
            put(&mut one, "update_delay", Tag::Int(update));
        }
        if let Some(Tag::List(facings)) = entry(value, "facings") {
            let mut keep = Vec::new();
            for facing in facings {
                let Tag::String(text) = facing else {
                    return Tag::List(Vec::new());
                };
                if !matches!(
                    text.as_str(),
                    "down" | "up" | "north" | "south" | "west" | "east"
                ) {
                    return Tag::List(Vec::new());
                }
                keep.push(Tag::String(text.clone()));
            }
            if !keep.is_empty() {
                put(&mut one, "facings", Tag::List(keep));
            }
        }
        out.push(Tag::Compound(one));
    }
    Tag::List(out)
}

// ---------------------------------------------------------------------------
// BeaconBlockEntity / PotentSulfurBlockEntity
// ---------------------------------------------------------------------------

fn beacon() -> Vec<(String, Tag)> {
    // `Levels` 不读盘：官方 `loadAdditional` 不碰它，值恒为构造器的 0。
    // `Levels` is never loaded; it stays at the constructor's 0.
    vec![("Levels".to_owned(), Tag::Int(0))]
}

fn potent_sulfur(raw: &Tag) -> Vec<(String, Tag)> {
    // `loadAdditional` 用 `getInt("countdown").ifPresent`：缺键时保持构造器 -1。
    // A missing key keeps the constructor's -1.
    let value = match entry(raw, "countdown") {
        Some(tag) => tag::java_i32(tag).unwrap_or(-1),
        None => -1,
    };
    vec![("countdown".to_owned(), Tag::Int(value))]
}

// ---------------------------------------------------------------------------
// StructureBlockEntity
// ---------------------------------------------------------------------------

fn structure_block(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    for key in ["name", "author", "metadata"] {
        put(&mut out, key, string_tag(raw, key));
    }
    for (key, default) in [("posX", 0), ("posY", 1), ("posZ", 0)] {
        put(
            &mut out,
            key,
            Tag::Int(tag::int_or(raw, key, default).clamp(-48, 48)),
        );
    }
    for key in ["sizeX", "sizeY", "sizeZ"] {
        put(
            &mut out,
            key,
            Tag::Int(tag::int_or(raw, key, 0).clamp(0, 48)),
        );
    }
    put(
        &mut out,
        "rotation",
        Tag::String(
            tag::string_or(raw, "rotation")
                .filter(|value| {
                    matches!(
                        *value,
                        "NONE" | "CLOCKWISE_90" | "CLOCKWISE_180" | "COUNTERCLOCKWISE_90"
                    )
                })
                .unwrap_or("NONE")
                .to_owned(),
        ),
    );
    put(
        &mut out,
        "mirror",
        Tag::String(
            tag::string_or(raw, "mirror")
                .filter(|value| matches!(*value, "NONE" | "LEFT_RIGHT" | "FRONT_BACK"))
                .unwrap_or("NONE")
                .to_owned(),
        ),
    );
    put(
        &mut out,
        "mode",
        Tag::String(
            tag::string_or(raw, "mode")
                .filter(|value| matches!(*value, "SAVE" | "LOAD" | "CORNER" | "DATA"))
                .unwrap_or("DATA")
                .to_owned(),
        ),
    );
    for (key, default) in [
        ("ignoreEntities", true),
        ("strict", false),
        ("powered", false),
        ("showair", false),
        ("showboundingbox", true),
    ] {
        put(&mut out, key, bool_tag(raw, key, default));
    }
    put(
        &mut out,
        "integrity",
        Tag::Float(tag::float_or(raw, "integrity", 1.0)),
    );
    put(&mut out, "seed", Tag::Long(tag::long_or(raw, "seed", 0)));
    out
}

// ---------------------------------------------------------------------------
// 读盘多写键的四类容器、shelf、命令方块
// Container-like types, shelf and command block
// ---------------------------------------------------------------------------

/// 唱片机：`RecordItem`（`ItemStack.CODEC`）和 `ticks_since_song_started`
/// 只在有非空唱片 / 有歌曲时写。游戏层红石表会在有唱片时覆盖播放进度；
/// 读盘但运行时表没建立时，本函数按读盘值带回，不丢内容。
/// A jukebox writes `RecordItem` and `ticks_since_song_started` only with a disc.
/// The redstone table overrides the ticks when it owns a song; otherwise the disk
/// values survive so a load/save without gameplay never loses the record.
/// 注意：官方 `loadStatic` 只在唱片能被 `JukeboxSongPlayer` 识别时写播放进度；
/// 对普通物品 Bamboo 目前按「有唱片」保留读盘进度，见清单的已知限制。
/// Note: vanilla only writes ticks for playable discs; Bamboo keeps the disk value
/// for any non-empty record (a known limitation listed in the checklist).
fn jukebox(raw: &Tag) -> Vec<(String, Tag)> {
    let Some(stack) = entry(raw, "RecordItem")
        .filter(|tag| matches!(tag, Tag::Compound(_)))
        .and_then(|tag| slots::decode_item(tag).ok())
        .filter(|stack| !stack.is_empty())
    else {
        return Vec::new();
    };
    let Ok(item) = slots::encode_item(&stack) else {
        return Vec::new();
    };
    let mut out = vec![("RecordItem".to_owned(), item)];
    if let Some(ticks) = entry(raw, "ticks_since_song_started").and_then(|t| tag::java_i64(t).ok())
    {
        put(&mut out, "ticks_since_song_started", Tag::Long(ticks));
    }
    out
}

/// 潜影盒：`RandomizableContainer.trySaveLootTable` 为真时只写战利品表，
/// 否则 `ContainerHelper.saveAllItems(..., keepEmpty=false)`（空列表不写）。
/// 读盘时不建容器表条目，所以这几个键都由本函数写回。
/// A shulker box writes either its loot table or a non-empty `Items` list with
/// `keepEmpty=false`; no game-layer table owns these keys for this type.
fn shulker_box(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    if let Some(Tag::String(table)) = entry(raw, "LootTable") {
        put(&mut out, "LootTable", Tag::String(table.clone()));
        let seed = tag::long_or(raw, "LootTableSeed", 0);
        if seed != 0 {
            put(&mut out, "LootTableSeed", Tag::Long(seed));
        }
        return out;
    }
    let list = items(entry(raw, "Items"), 27);
    if let Tag::List(values) = &list
        && !values.is_empty()
    {
        put(&mut out, "Items", list);
    }
    out
}

/// 陶罐：非默认 `sherds`（`PotDecorations.CODEC`）、非空 `item`，或战利品表。
/// `sherds` 由组件补丁读写（`read_game_fields` 里从 `sherds` 键解出、
/// `write_component_fields` 再写回），本函数按同一 codec 写读盘值；
/// `item` 没有运行时写入者，也在这里带回。
/// A decorated pot writes non-default `sherds`, a non-empty `item`, or a loot table.
/// The component patch owns `sherds`; `item` has no runtime writer.
fn decorated_pot(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    if let Some(sherds) = entry(raw, "sherds")
        && let Ok(value) = components::decode_value(component_kinds::POT_DECORATIONS, sherds, 0)
        && !matches!(&value, ComponentValue::PotDecorations { back, left, right, front }
            if back.is_none() && left.is_none() && right.is_none() && front.is_none())
        && let Some(tag) = components::encode_value(&value, 0).ok().map(|v| v.to_tag())
    {
        put(&mut out, "sherds", tag);
    }
    if let Some(Tag::String(table)) = entry(raw, "LootTable") {
        put(&mut out, "LootTable", Tag::String(table.clone()));
        let seed = tag::long_or(raw, "LootTableSeed", 0);
        if seed != 0 {
            put(&mut out, "LootTableSeed", Tag::Long(seed));
        }
        return out;
    }
    if let Some(item) = entry(raw, "item")
        .filter(|tag| matches!(tag, Tag::Compound(_)))
        .and_then(|tag| slots::decode_item(tag).ok())
        .filter(|stack| !stack.is_empty())
        .and_then(|stack| slots::encode_item(&stack).ok())
    {
        put(&mut out, "item", item);
    }
    out
}

fn shelf(raw: &Tag) -> Vec<(String, Tag)> {
    vec![
        // `ContainerHelper.saveAllItems(..., true)`：空列表也写 `Items:[]`。
        // `keepEmpty=true` writes `Items:[]` even for an empty shelf.
        ("Items".to_owned(), items(entry(raw, "Items"), 3)),
        (
            "align_items_to_bottom".to_owned(),
            bool_tag(raw, "align_items_to_bottom", false),
        ),
    ]
}

/// 命令方块：`BaseCommandBlock.save` 的 7 个键 + 条件写出的 `LastOutput` /
/// `LastExecution`。`CustomName` 由组件补丁写，游戏层不拥有后两个键，
/// 所以读盘值必须在这里按官方条件（`TrackOutput` / `UpdateLastExecution`）带回。
/// `BaseCommandBlock.save` plus the two conditional keys; the component patch owns
/// `CustomName`, but no runtime table owns the other two.
fn command_block(raw: &Tag) -> Vec<(String, Tag)> {
    let track = tag::bool_or(raw, "TrackOutput", true);
    let update = tag::bool_or(raw, "UpdateLastExecution", true);
    let mut out = vec![
        ("Command".to_owned(), string_tag(raw, "Command")),
        (
            "SuccessCount".to_owned(),
            Tag::Int(tag::int_or(raw, "SuccessCount", 0)),
        ),
        ("TrackOutput".to_owned(), Tag::Byte(i8::from(track))),
        (
            "UpdateLastExecution".to_owned(),
            Tag::Byte(i8::from(update)),
        ),
        ("powered".to_owned(), bool_tag(raw, "powered", false)),
        (
            "conditionMet".to_owned(),
            bool_tag(raw, "conditionMet", false),
        ),
        ("auto".to_owned(), bool_tag(raw, "auto", false)),
    ];
    if track {
        // `parseCustomNameSafe` 解不开的名字回到空组件；`storeNullable` 只在非空时写。
        // An unparsable custom name is dropped, matching `storeNullable`.
        if let Some(name) = entry(raw, "LastOutput").and_then(chat_component) {
            put(&mut out, "LastOutput", name);
        }
    }
    if update
        && let Some(last) = entry(raw, "LastExecution").and_then(|t| tag::java_i64(t).ok())
        && last != -1
    {
        put(&mut out, "LastExecution", Tag::Long(last));
    }
    out
}

/// `ComponentSerialization.CODEC` 的文本组件：能解码就按 codec 重新编码。
/// A chat component is re-encoded through the codec when it decodes successfully.
fn chat_component(tag: &Tag) -> Option<Tag> {
    text::decode(tag).ok().and_then(|component| {
        let encoded = text::encode(&component, 0).ok()?;
        Some(encoded.to_tag())
    })
}

// ---------------------------------------------------------------------------
// TestBlockEntity / TestInstanceBlockEntity（只在测试模式出现）
// Test-mode block entities
// ---------------------------------------------------------------------------

fn test_block(raw: &Tag) -> Vec<(String, Tag)> {
    let mode = tag::string_or(raw, "mode")
        .filter(|value| matches!(*value, "start" | "log" | "fail" | "accept"))
        .unwrap_or("fail");
    vec![
        ("mode".to_owned(), Tag::String(mode.to_owned())),
        ("message".to_owned(), string_tag(raw, "message")),
        ("powered".to_owned(), bool_tag(raw, "powered", false)),
    ]
}

fn test_instance_block(raw: &Tag) -> Vec<(String, Tag)> {
    // `Data.CODEC` 的四个必填字段和两个可省字段；`errors` 只在非空时写。
    // Four required and two optional `Data` fields; `errors` is conditional.
    let data_tag = entry(raw, "data");
    let mut data = Vec::new();
    put(
        &mut data,
        "size",
        match data_tag.and_then(|d| entry(d, "size")) {
            Some(Tag::IntArray(size)) if size.len() == 3 => Tag::IntArray(size.clone()),
            _ => Tag::IntArray(vec![0, 0, 0]),
        },
    );
    let rotation = data_tag
        .and_then(|d| tag::string_or(d, "rotation"))
        .filter(|value| {
            matches!(
                *value,
                "none" | "clockwise_90" | "clockwise_180" | "counterclockwise_90"
            )
        })
        .unwrap_or("none");
    put(&mut data, "rotation", Tag::String(rotation.to_owned()));
    put(
        &mut data,
        "ignore_entities",
        match data_tag {
            Some(tag) => bool_tag(tag, "ignore_entities", false),
            None => Tag::Byte(0),
        },
    );
    let status = data_tag
        .and_then(|d| tag::string_or(d, "status"))
        .filter(|value| matches!(*value, "cleared" | "running" | "finished"))
        .unwrap_or("cleared");
    put(&mut data, "status", Tag::String(status.to_owned()));
    // `test` 是 optionalFieldOf 的 ResourceKey，`error_message` 是聊天组件；
    // 形状合法时原样带回，其余按 codec 的宽松解码省略。
    // Optional ResourceKey and chat component; valid shapes are carried through.
    for key in ["test", "error_message"] {
        if let Some(Tag::String(_)) = data_tag.and_then(|d| entry(d, key)) {
            put(
                &mut data,
                key,
                data_tag.and_then(|d| entry(d, key)).cloned().unwrap(),
            );
        }
    }
    let mut out = vec![("data".to_owned(), Tag::Compound(data))];
    if let Some(Tag::List(errors)) = entry(raw, "errors")
        && !errors.is_empty()
    {
        // 官方 `ErrorMarker.LIST_CODEC` 解不开时列表为空、不写这个键；
        // 这里按形状带回非空列表，不做逐条子 codec 规范化。
        // A non-empty `errors` list is written; bad entries are not re-encoded.
        put(&mut out, "errors", Tag::List(errors.clone()));
    }
    out
}
