//! 只保留存档态的方块实体：`saveAdditional` / `loadAdditional` 的纯函数往返。
//!
//! 官方读一条 `block_entities` 条目时按 `id` 造具体方块实体，写盘时无条件写出自己的
//! 键（有些字段按 codec 的省略规则可省）。这里照同一份规则做「读入 → 写出」，不改变
//! 方块实体在游戏里的行为：调用方拿返回的键覆盖建模条目，未知键仍按 D9 原样保留。
//! 世界生成只放下方块、没有读盘条目时用空复合标签跑同一份 `resave_additional`；
//! 宝库 / 幽匿那四种没有本模块的分支，由 [`default_additional`] 补构造器默认值。
//!
//! 字段名、默认值和 codec 的省略规则都用 `javap -c -p` 在
//! `BaseSpawner` / `SignBlockEntity` / `SignText` / `BeehiveBlockEntity` /
//! `JigsawBlockEntity` / `TheEndGatewayBlockEntity` / `SculkCatalystBlockEntity` /
//! `BeaconBlockEntity` / `PotentSulfurBlockEntity` / `StructureBlockEntity` /
//! `ShelfBlockEntity` / `CommandBlockEntity` 上核对过；坏数据一律按官方宽松解码回
//! 默认值，不 panic。
//!
//! Pure `saveAdditional` / `loadAdditional` round trips for state-only block entities.
//! Vanilla writes these keys unconditionally; the returned keys override the modeled
//! entry while unmodeled keys stay preserved. No gameplay behaviour lives here.

use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::archive::slots;
use mc_bamboo_protocol::components::cursor::parse_identifier;

use super::tag::{self, put};
use crate::{BlockState, state::ORIENTATION};

/// 一条读盘条目往返后的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Resaved {
    /// 官方 `saveAdditional` 会写出的键和值。
    pub fields: Vec<(String, Tag)>,
    /// 官方认识、但这一状态下不写的键，要从原样保留的条目里删掉。
    /// 不在这张表里的未知键仍按 D9 原样保留。
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
///
/// `state` 只用于推导 jigsaw 缺省 `joint` 的方向；`raw` 是读盘时的整条复合标签，
/// 没有读盘条目时传空复合标签。返回 `None` 表示这个类型不需要本模块补键。
/// 正式编码路径、`block_entity_resave` 测试和 `chunk-content` 对拍共用这一个函数。
pub fn resave_additional(id: &str, state: BlockState, raw: &Tag) -> Option<Resaved> {
    let (fields, omit) = match id {
        "minecraft:mob_spawner" => (spawner(raw), SPAWNER_OMIT),
        "minecraft:sign" | "minecraft:hanging_sign" => (sign(raw), SIGN_OMIT),
        "minecraft:beehive" => (beehive(raw), BEEHIVE_OMIT),
        "minecraft:jigsaw" => (jigsaw(raw, state), &[][..]),
        "minecraft:end_gateway" => (end_gateway(raw), &[][..]),
        "minecraft:sculk_catalyst" => (sculk_catalyst(raw), &[][..]),
        "minecraft:beacon" => (beacon(), &[][..]),
        "minecraft:potent_sulfur" => (potent_sulfur(raw), &[][..]),
        "minecraft:structure_block" => (structure_block(raw), &[][..]),
        "minecraft:ender_chest"
        | "minecraft:jukebox"
        | "minecraft:shulker_box"
        | "minecraft:decorated_pot" => (non_item_containers(id, raw), NON_ITEM_CONTAINER_OMIT),
        "minecraft:shelf" => (shelf(raw), &[][..]),
        "minecraft:command_block" => (command_block(raw), COMMAND_BLOCK_OMIT),
        "minecraft:test_block" => (test_block(raw), TEST_BLOCK_OMIT),
        "minecraft:test_instance_block" => (test_instance_block(raw), TEST_INSTANCE_OMIT),
        _ => return None,
    };
    Some(Resaved::new(fields, omit))
}

/// 官方认识、可选且当前状态不写的键：编码时从读盘条目里删掉，避免带上旧值。
/// 不在这张表里的未知键按 D9 原样保留。
const SPAWNER_OMIT: &[&str] = &["SpawnData"];
const SIGN_OMIT: &[&str] = &["allow_op_features"];
const BEEHIVE_OMIT: &[&str] = &["flower_pos"];
/// 末影箱 / 唱片机 / 潜影盒 / 陶罐的容器表：官方根本不写 `Items` 等键。
const NON_ITEM_CONTAINER_OMIT: &[&str] = &[
    "Items",
    "RecordItem",
    "ticks_since_song_started",
    "sherds",
    "item",
    "LootTable",
    "LootTableSeed",
];
/// `BaseCommandBlock.save` 的可省键；`LastOutput` / `LastExecution` 也在游戏层 `omit` 里。
const COMMAND_BLOCK_OMIT: &[&str] = &["CustomName", "LastOutput", "LastExecution"];
/// 测试方块没有可省键；测试实例方块的 `errors` 只在非空时写。
const TEST_BLOCK_OMIT: &[&str] = &[];
const TEST_INSTANCE_OMIT: &[&str] = &["errors"];

/// 世界生成放置、没有读盘条目时的构造器默认值。
pub fn default_additional(id: &str) -> Option<Vec<(String, Tag)>> {
    // 本模块负责的类型（含生成期无条目）都走 `resave_additional(空复合标签)`；
    // 这里只留上一轮那四种只保留存档态实体的构造器默认值。
    super::passive::default_additional(id)
}

/// 复合标签里的条目按名字查，同名键取最后一个。
fn entry<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    tag::field(tag, key)
}

fn short_tag(tag: &Tag, key: &str, default: i16) -> Tag {
    Tag::Short(tag::short_or(tag, key, default))
}

fn bool_tag(tag: &Tag, key: &str, default: bool) -> Tag {
    Tag::Byte(i8::from(tag::bool_or(tag, key, default)))
}

fn string_tag(tag: &Tag, key: &str) -> Tag {
    Tag::String(tag::string_or(tag, key).unwrap_or("").to_owned())
}

/// `Identifier.CODEC`：解不开的名字回到调用方给的缺省值。
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
// BaseSpawner.save / load
// ---------------------------------------------------------------------------

/// `BaseSpawner.load` 里 `SpawnPotentials` 缺失时按 `nextSpawnData` 造一份带一项的
/// 列表：空的 `SpawnData`（`{entity:{}}`）权重 1。
fn default_spawn_potentials() -> Tag {
    Tag::List(vec![Tag::Compound(vec![
        (
            "data".to_owned(),
            Tag::Compound(vec![("entity".to_owned(), Tag::Compound(Vec::new()))]),
        ),
        ("weight".to_owned(), Tag::Int(1)),
    ])])
}

/// `SpawnData.CODEC`：`entity` 是必填的 `CompoundTag.CODEC`，另外两个可选字段
/// 存在时原样带回；`entity.id` 由构造器用 `Identifier.CODEC` 重新规范化。
fn spawn_data(tag: &Tag) -> Tag {
    let mut entity = match entry(tag, "entity") {
        Some(Tag::Compound(values)) => values.clone(),
        _ => Vec::new(),
    };
    if let Some(Tag::String(id)) = entry(tag, "entity").and_then(|e| entry(e, "id")) {
        // `SpawnData` 的构造器用 `Identifier.CODEC` 把 `entity.id` 规范化；
        // 解不开时移除 `id`（`CompoundTag.remove`）。
        entity.retain(|(key, _)| key != "id");
        if let Ok(name) = parse_identifier(id) {
            entity.push(("id".to_owned(), Tag::String(name)));
        }
    }
    let mut out = vec![("entity".to_owned(), Tag::Compound(entity))];
    for key in ["custom_spawn_rules", "equipment"] {
        if let Some(Tag::Compound(_)) = entry(tag, key) {
            out.push((key.to_owned(), entry(tag, key).cloned().unwrap()));
        }
    }
    Tag::Compound(out)
}

fn spawn_potentials(raw: &Tag) -> Tag {
    let fallback = entry(raw, "SpawnData")
        .filter(|tag| matches!(tag, Tag::Compound(_)))
        .map(|data| {
            Tag::List(vec![Tag::Compound(vec![
                ("data".to_owned(), spawn_data(data)),
                ("weight".to_owned(), Tag::Int(1)),
            ])])
        })
        .unwrap_or_else(default_spawn_potentials);
    let Some(Tag::List(values)) = entry(raw, "SpawnPotentials") else {
        return fallback;
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Tag::Compound(_) = value else {
            return fallback;
        };
        let Some(data) = entry(value, "data").filter(|tag| matches!(tag, Tag::Compound(_))) else {
            return fallback;
        };
        let mut one = Vec::new();
        put(&mut one, "data", spawn_data(data));
        put(
            &mut one,
            "weight",
            Tag::Int(tag::int_or(value, "weight", 1)),
        );
        out.push(Tag::Compound(one));
    }
    Tag::List(out)
}

fn spawner(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    for (key, default) in [
        ("Delay", 20_i16),
        ("MinSpawnDelay", 200),
        ("MaxSpawnDelay", 800),
        ("SpawnCount", 4),
        ("MaxNearbyEntities", 6),
        ("RequiredPlayerRange", 16),
        ("SpawnRange", 4),
    ] {
        put(&mut out, key, short_tag(raw, key, default));
    }
    // `SpawnData` 是 storeNullable：只有读盘条目里解得出 `SpawnData` 时才写。
    if let Some(data) = entry(raw, "SpawnData").filter(|tag| matches!(tag, Tag::Compound(_))) {
        put(&mut out, "SpawnData", spawn_data(data));
    }
    put(&mut out, "SpawnPotentials", spawn_potentials(raw));
    out
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
    if tag::bool_or(raw, "allow_op_features", false) {
        put(&mut out, "allow_op_features", Tag::Byte(1));
    }
    put(&mut out, "front_text", sign_text(entry(raw, "front_text")));
    put(&mut out, "back_text", sign_text(entry(raw, "back_text")));
    put(&mut out, "is_waxed", bool_tag(raw, "is_waxed", false));
    out
}

// ---------------------------------------------------------------------------
// BeehiveBlockEntity
// ---------------------------------------------------------------------------

fn beehive(raw: &Tag) -> Vec<(String, Tag)> {
    let mut out = vec![("bees".to_owned(), bee_occupants(entry(raw, "bees")))];
    // `flower_pos` 是 storeNullable 的 BlockPos.CODEC；形状不对或没有值时不写。
    if let Some(Tag::IntArray(pos)) = entry(raw, "flower_pos")
        && pos.len() == 3
    {
        put(&mut out, "flower_pos", Tag::IntArray(pos.clone()));
    }
    out
}

/// `Occupant.CODEC`：三个必填字段；一条解不开让整个 `bees` 列表回到空列表。
fn bee_occupants(input: Option<&Tag>) -> Tag {
    let Some(Tag::List(values)) = input else {
        return Tag::List(Vec::new());
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Tag::Compound(_) = value else {
            return Tag::List(Vec::new());
        };
        let Some(entity_data) =
            entry(value, "entity_data").filter(|t| matches!(t, Tag::Compound(_)))
        else {
            return Tag::List(Vec::new());
        };
        let ticks = tag::java_i32(entry(value, "ticks_in_hive").unwrap_or(&Tag::Int(0)));
        let min = tag::java_i32(entry(value, "min_ticks_in_hive").unwrap_or(&Tag::Int(0)));
        let (Ok(ticks), Ok(min)) = (ticks, min) else {
            return Tag::List(Vec::new());
        };
        let mut one = Vec::new();
        put(&mut one, "entity_data", entity_data.clone());
        put(&mut one, "ticks_in_hive", Tag::Int(ticks));
        put(&mut one, "min_ticks_in_hive", Tag::Int(min));
        out.push(Tag::Compound(one));
    }
    Tag::List(out)
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
    vec![("Levels".to_owned(), Tag::Int(0))]
}

fn potent_sulfur(raw: &Tag) -> Vec<(String, Tag)> {
    // `loadAdditional` 用 `getInt("countdown").ifPresent`：缺键时保持构造器 -1。
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
// ---------------------------------------------------------------------------

/// 官方这四类不写 `Items`：末影箱没有自己的存档键；唱片机写 `RecordItem`，
/// 潜影盒只在有物品时写 `Items`，陶罐写 `sherds` / `item`。这些键由游戏层的
/// 容器表和组件补丁负责，本函数只处理那些「官方认识但该状态下不写」的可省键。
fn non_item_containers(_id: &str, _raw: &Tag) -> Vec<(String, Tag)> {
    // 官方的这些可省键都由游戏层运行时表（`Items` / `RecordItem` / `sherds` / `item`
    // / 红石记忆）负责；本模块只删掉读盘条目里「官方认识但当前不写」的键，
    // 不拿读盘旧值盖掉运行时改过的内容。
    Vec::new()
}

fn shelf(raw: &Tag) -> Vec<(String, Tag)> {
    vec![
        // `ContainerHelper.saveAllItems(..., true)`：空列表也写 `Items:[]`。
        ("Items".to_owned(), items(entry(raw, "Items"), 3)),
        (
            "align_items_to_bottom".to_owned(),
            bool_tag(raw, "align_items_to_bottom", false),
        ),
    ]
}

fn command_block(raw: &Tag) -> Vec<(String, Tag)> {
    vec![
        ("Command".to_owned(), string_tag(raw, "Command")),
        (
            "SuccessCount".to_owned(),
            Tag::Int(tag::int_or(raw, "SuccessCount", 0)),
        ),
        ("TrackOutput".to_owned(), bool_tag(raw, "TrackOutput", true)),
        (
            "UpdateLastExecution".to_owned(),
            bool_tag(raw, "UpdateLastExecution", true),
        ),
        ("powered".to_owned(), bool_tag(raw, "powered", false)),
        (
            "conditionMet".to_owned(),
            bool_tag(raw, "conditionMet", false),
        ),
        ("auto".to_owned(), bool_tag(raw, "auto", false)),
    ]
}

// ---------------------------------------------------------------------------
// TestBlockEntity / TestInstanceBlockEntity（只在测试模式出现）
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
    for key in ["test", "error_message"] {
        if let Some(Tag::String(_)) = data_tag.and_then(|d| entry(d, key)) {
            put(
                &mut data,
                key,
                data_tag.and_then(|d| entry(d, key)).cloned().unwrap(),
            );
        }
    }
    vec![("data".to_owned(), Tag::Compound(data))]
}
