//! 把提取到的数据写成 Rust 源文件。

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Write},
    fs,
    path::Path,
};

use crate::extract::{
    Block, BlockField, BlockPredicate, CompostLayers, ExtractedIngredient,
    ExtractedPotionIngredient, ExtractedRecipe, ExtractedRecipeKind, ExtractedShape,
    ExtractedStack, FieldValue, HeightmapType, Item, Property, StateProvider, TagGroup,
    VersionData,
};

const HEADER: &str = "// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。\n\n";
const ATTRIBUTE_REGISTRY: &str = "minecraft:attribute";
const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
const BLOCK_ENTITY_TYPE_REGISTRY: &str = "minecraft:block_entity_type";
const CHAT_TYPE_REGISTRY: &str = "minecraft:chat_type";
const CONSUME_EFFECT_TYPE_REGISTRY: &str = "minecraft:consume_effect_type";
const DATA_COMPONENT_TYPE_REGISTRY: &str = "minecraft:data_component_type";
const DIMENSION_TYPE_REGISTRY: &str = "minecraft:dimension_type";
const ENTITY_TYPE_REGISTRY: &str = "minecraft:entity_type";
const BLOCK_REGISTRY: &str = "minecraft:block";
const FLUID_REGISTRY: &str = "minecraft:fluid";
const ITEM_REGISTRY: &str = "minecraft:item";
const MENU_REGISTRY: &str = "minecraft:menu";
const WORLD_CLOCK_REGISTRY: &str = "minecraft:world_clock";

pub fn write_all(data: &VersionData, output_dir: &Path) -> Result<(), String> {
    // 这几个文件带校验。先生成好，校验失败时不留下写了一半的目录。
    let blocks = blocks_file(data)?;
    let items = items_file(data)?;
    let states = states_file(data)?;
    let shapes = shapes_file(data)?;
    let light = light_file(data)?;
    let tags = tags_file(data)?;
    let transformers = block_transformers_file(data)?;
    let recipes = recipes_file(data)?;
    let heightmaps = heightmaps_file(data)?;

    write_file(output_dir, "mod.rs", &module_file())?;
    write_file(output_dir, "version.rs", &version_file(data))?;
    write_file(output_dir, "packets.rs", &packets_file(data))?;
    write_file(output_dir, "blocks.rs", &blocks)?;
    write_file(output_dir, "block_classes.rs", &block_classes_file(data))?;
    write_file(output_dir, "block_maps.rs", &block_maps_file(data))?;
    write_file(output_dir, "block_transformers.rs", &transformers)?;
    write_file(output_dir, "items.rs", &items)?;
    write_file(output_dir, "recipes.rs", &recipes)?;
    write_file(output_dir, "light.rs", &light)?;
    write_file(output_dir, "states.rs", &states)?;
    write_file(output_dir, "shapes.rs", &shapes)?;
    write_file(output_dir, "registries.rs", &registries_file(data))?;
    write_file(
        output_dir,
        "component_registries.rs",
        &component_registries_file(data)?,
    )?;
    write_file(
        output_dir,
        "component_enums.rs",
        &component_enums_file(data)?,
    )?;
    write_file(
        output_dir,
        "item_components.rs",
        &item_components_file(data)?,
    )?;
    write_file(output_dir, "heightmaps.rs", &heightmaps)?;
    write_file(output_dir, "tags.rs", &tags)?;
    write_worldgen(data, output_dir)
}

/// 世界生成的数据写成一个 JSON 文件，再加一个只有 `include_str!` 的 Rust 文件。
///
/// 这几个注册表是递归的树（密度函数里套密度函数、样条里套样条），生成等价的 Rust 结构
/// 会是一大堆嵌套字面量，升级版本时也看不出差异；JSON 是官方 codec 的原始形态，
/// `mc-bamboo-worldgen` 用同一套 codec 的语义解析，所以这里原样存。
fn write_worldgen(data: &VersionData, output_dir: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&data.worldgen)
        .map_err(|error| format!("无法写出世界生成数据：{error}"))?;
    let path = output_dir.join("worldgen.json");
    fs::write(&path, format!("{json}\n"))
        .map_err(|error| format!("无法写入 {}：{error}", path.display()))?;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// 官方数据包里世界生成的几个注册表：`worldgen/noise`、`worldgen/density_function`、"
    );
    let _ = writeln!(
        out,
        "/// `worldgen/noise_settings`、两个多噪声群系预设和末地的五个群系。"
    );
    let _ = writeln!(out, "///");
    let _ = writeln!(
        out,
        "/// 前三个是官方 codec 编出来的 JSON，预设是量化好的定点数。解析在 `mc-bamboo-worldgen`。"
    );
    let _ = writeln!(
        out,
        "pub const WORLDGEN: &str = include_str!(\"worldgen.json\");"
    );
    write_file(output_dir, "worldgen.rs", &out)
}

/// 组件里按 ID 查表的枚举：连续的常量个数，稀疏的整张 ID 表。
///
/// 官方 `ByIdMap.continuous` 要求 ID 刚好是 0 到个数减一的排列，所以连续的枚举只要个数。
/// 越界时怎么折回（`ZERO`、`CLAMP`、`WRAP`）是代码，留在 `mc-bamboo-protocol` 里。
fn component_enums_file(data: &VersionData) -> Result<String, String> {
    /// 官方加了新枚举或者改了名字时要在这里跟着改，顺序就是生成文件里的顺序。
    const COUNTS: &[&str] = &[
        "attribute_display_type",
        "attribute_operation",
        "axolotl_variant",
        "dye_color",
        "equipment_slot",
        "equipment_slot_group",
        "firework_shape",
        "fox_variant",
        "horse_variant",
        "item_use_animation",
        "llama_variant",
        "map_post_processing",
        "mooshroom_variant",
        "parrot_variant",
        "rarity",
        "salmon_size",
        "swing_animation_type",
    ];
    const SPARSE: &[&str] = &["rabbit_variant", "tropical_fish_pattern"];

    let enums = &data.component_enums;
    check_enum_names("counts", COUNTS, enums.counts.keys())?;
    check_enum_names("sparse", SPARSE, enums.sparse.keys())?;

    let mut out = String::new();
    let _ = writeln!(out, "/// 连续枚举的常量个数，ID 就是 0 到这个数减一。");
    let _ = writeln!(out, "pub mod counts {{");
    for name in COUNTS {
        let count = enums.counts[*name];
        if count == 0 {
            return Err(format!("枚举 {name} 一个常量都没有"));
        }
        let _ = writeln!(out, "    pub const {}: i32 = {count};", upper(name));
    }
    let _ = writeln!(out, "}}\n");

    let _ = writeln!(
        out,
        "/// 稀疏枚举的 ID，按官方的声明顺序。官方的默认值都是第一个声明的常量。"
    );
    let _ = writeln!(out, "pub mod sparse {{");
    for name in SPARSE {
        let ids = &enums.sparse[*name];
        if ids.is_empty() {
            return Err(format!("枚举 {name} 一个常量都没有"));
        }
        let _ = writeln!(out, "    pub const {}: &[i32] = &{ids:?};", upper(name));
    }
    let _ = writeln!(out, "}}");
    Ok(out)
}

/// 生成文件里的名字和官方导出的名字必须一一对应，官方加了新枚举时要报错。
fn check_enum_names<'a>(
    kind: &str,
    expected: &[&str],
    found: impl Iterator<Item = &'a String>,
) -> Result<(), String> {
    let found: BTreeSet<&str> = found.map(String::as_str).collect();
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    if found != expected {
        return Err(format!(
            "{kind} 里的枚举和生成器对不上：官方 {found:?}，生成器 {expected:?}"
        ));
    }
    Ok(())
}

fn upper(name: &str) -> String {
    name.to_uppercase()
}

/// 每个物品的默认组件，按官方 `DataComponentPatch.STREAM_CODEC` 编出来的字节。
///
/// 全部字节拼成一个 `&[u8]`，`RANGES` 给出每个物品在里面的位置，下标是物品的注册表 ID。
/// 分成一千多个字节串字面量会让生成文件大一倍，编译也慢。
fn item_components_file(data: &VersionData) -> Result<String, String> {
    if data.item_components.len() != data.items.len() {
        return Err(format!(
            "导出了 {} 个物品的默认组件，物品有 {} 个",
            data.item_components.len(),
            data.items.len()
        ));
    }

    let mut blob: Vec<u8> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(data.item_components.len());
    for (index, hex) in data.item_components.iter().enumerate() {
        let bytes = decode_hex(hex)
            .ok_or_else(|| format!("{} 的默认组件不是十六进制", data.items[index].name))?;
        let start = blob.len();
        blob.extend_from_slice(&bytes);
        ranges.push((start, blob.len()));
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// 全部物品的默认组件拼在一起，每个物品的位置见 [`RANGES`]。"
    );
    let _ = writeln!(out, "pub const DEFAULTS: &[u8] = b\"{}\";\n", escape(&blob));
    let _ = writeln!(
        out,
        "/// 每个物品的默认组件在 [`DEFAULTS`] 里的起止位置，下标是物品的注册表 ID。"
    );
    let _ = writeln!(out, "pub const RANGES: &[(u32, u32)] = &[");
    for (start, end) in ranges {
        let _ = writeln!(out, "    ({start}, {end}),");
    }
    let _ = writeln!(out, "];");
    Ok(out)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok())
        .collect()
}

/// 把字节写成 Rust 的字节串字面量，能直接认出来的 ASCII 保持原样。
fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(byte as char),
            _ => {
                let _ = write!(out, "\\x{byte:02x}");
            }
        }
    }
    out
}

/// 物品组件解码要用到的注册表大小，外加几个要按 ID 分派的小注册表的常量。
///
/// 组件的 `StreamCodec` 把 `Holder` 和注册表条目写成数字 ID，解码时只需要知道条目个数
/// 就能判断 ID 合不合法，所以这里只导出大小，不导出上千个用不到的名字。
fn component_registries_file(data: &VersionData) -> Result<String, String> {
    /// 要导出大小的注册表，`(模块里的常量名, 注册表名)`。
    const SIZES: &[(&str, &str)] = &[
        ("BANNER_PATTERN", "minecraft:banner_pattern"),
        ("BLOCK", BLOCK_REGISTRY),
        ("BLOCK_ENTITY_TYPE", BLOCK_ENTITY_TYPE_REGISTRY),
        ("BLOCK_TRANSFORMER", "minecraft:block_transformer"),
        ("CAT_SOUND_VARIANT", "minecraft:cat_sound_variant"),
        ("CAT_VARIANT", "minecraft:cat_variant"),
        ("CHICKEN_SOUND_VARIANT", "minecraft:chicken_sound_variant"),
        ("CHICKEN_VARIANT", "minecraft:chicken_variant"),
        ("CONSUME_EFFECT_TYPE", CONSUME_EFFECT_TYPE_REGISTRY),
        ("COW_SOUND_VARIANT", "minecraft:cow_sound_variant"),
        ("COW_VARIANT", "minecraft:cow_variant"),
        ("DAMAGE_TYPE", "minecraft:damage_type"),
        (
            "DATA_COMPONENT_PREDICATE_TYPE",
            "minecraft:data_component_predicate_type",
        ),
        ("DATA_COMPONENT_TYPE", DATA_COMPONENT_TYPE_REGISTRY),
        ("DECORATED_POT_PATTERN", "minecraft:decorated_pot_pattern"),
        ("ENCHANTMENT", "minecraft:enchantment"),
        ("ENTITY_TYPE", ENTITY_TYPE_REGISTRY),
        ("FROG_VARIANT", "minecraft:frog_variant"),
        ("INSTRUMENT", "minecraft:instrument"),
        ("ITEM", ITEM_REGISTRY),
        ("JUKEBOX_SONG", "minecraft:jukebox_song"),
        ("MOB_EFFECT", "minecraft:mob_effect"),
        ("PAINTING_VARIANT", "minecraft:painting_variant"),
        ("PIG_SOUND_VARIANT", "minecraft:pig_sound_variant"),
        ("PIG_VARIANT", "minecraft:pig_variant"),
        ("POTION", "minecraft:potion"),
        ("SOUND_EVENT", "minecraft:sound_event"),
        ("TRIM_MATERIAL", "minecraft:trim_material"),
        ("TRIM_PATTERN", "minecraft:trim_pattern"),
        ("VILLAGER_TYPE", "minecraft:villager_type"),
        ("WOLF_SOUND_VARIANT", "minecraft:wolf_sound_variant"),
        ("WOLF_VARIANT", "minecraft:wolf_variant"),
        (
            "ZOMBIE_NAUTILUS_VARIANT",
            "minecraft:zombie_nautilus_variant",
        ),
    ];

    let mut out = String::new();
    let _ = writeln!(out, "/// 组件解码要校验的注册表条目个数。");
    let _ = writeln!(out, "pub mod sizes {{");
    for &(name, registry) in SIZES {
        let entries = registry_entries(data, registry)
            .ok_or_else(|| format!("注册表报告里没有 {registry}"))?;
        if entries.is_empty() {
            return Err(format!("{registry} 是空的"));
        }
        let _ = writeln!(out, "    /// `{registry}`。");
        let _ = writeln!(out, "    pub const {name}: u32 = {};", entries.len());
    }
    let _ = writeln!(out, "}}\n");

    write_registry_ids(
        &mut out,
        data,
        "consume_effect_type",
        CONSUME_EFFECT_TYPE_REGISTRY,
    );
    Ok(out)
}

fn write_file(output_dir: &Path, name: &str, content: &str) -> Result<(), String> {
    let path = output_dir.join(name);
    fs::write(&path, format!("{HEADER}{content}"))
        .map_err(|error| format!("无法写入 {}：{error}", path.display()))
}

fn module_file() -> String {
    "pub mod block_classes;
pub mod block_maps;
pub mod block_transformers;
pub mod blocks;
pub mod component_enums;
pub mod component_registries;
pub mod heightmaps;
pub mod item_components;
pub mod items;
pub mod light;
pub mod packets;
pub mod recipes;
pub mod registries;
pub mod shapes;
pub mod states;
pub mod tags;
pub mod version;
pub mod worldgen;
"
    .to_owned()
}

fn version_file(data: &VersionData) -> String {
    let version = &data.version;
    let mut out = String::new();
    let _ = writeln!(out, "/// 这份数据对应的游戏版本。");
    let _ = writeln!(out, "pub const GAME_VERSION: &str = {:?};", version.name);
    let _ = writeln!(out, "/// 这个版本的网络协议号。");
    let _ = writeln!(
        out,
        "pub const PROTOCOL_VERSION: i32 = {};",
        version.protocol
    );
    let _ = writeln!(out, "/// 这个版本的世界数据版本号。");
    let _ = writeln!(
        out,
        "pub const DATA_VERSION: i32 = {};",
        version.data_version
    );
    out
}

/// 官方的 `Heightmap.Types`。区块包按 `id` 排列，`id` 也是网络上的键。
fn heightmaps_file(data: &VersionData) -> Result<String, String> {
    check_heightmap_types(&data.heightmaps)?;

    let mut out = String::new();
    let _ = writeln!(out, "/// 一个高度图类型。");
    let _ = writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
    let _ = writeln!(out, "pub struct HeightmapType {{");
    let _ = writeln!(
        out,
        "    /// 官方的序列化名，比如 `MOTION_BLOCKING_NO_LEAVES`。"
    );
    let _ = writeln!(out, "    pub name: &'static str,");
    let _ = writeln!(out, "    /// 网络上的键，也是官方 `Types` 的声明顺序。");
    let _ = writeln!(out, "    pub id: i32,");
    let _ = writeln!(out, "    /// `sendToClient`：区块包带上这一种。");
    let _ = writeln!(out, "    pub send_to_client: bool,");
    let _ = writeln!(out, "    /// `keepAfterWorldgen`：区块里一直留着这一种。");
    let _ = writeln!(out, "    pub keep_after_worldgen: bool,");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    for type_ in &data.heightmaps {
        let _ = writeln!(out, "/// `{}`。", type_.name);
        let _ = writeln!(
            out,
            "pub const {}: HeightmapType = HeightmapType {{",
            constant_name(&type_.name)
        );
        let _ = writeln!(out, "    name: {:?},", type_.name);
        let _ = writeln!(out, "    id: {},", type_.id);
        let _ = writeln!(out, "    send_to_client: {},", type_.send_to_client);
        let _ = writeln!(
            out,
            "    keep_after_worldgen: {},",
            type_.keep_after_worldgen
        );
        let _ = writeln!(out, "}};");
        let _ = writeln!(out);
    }

    let constants: Vec<String> = data
        .heightmaps
        .iter()
        .map(|type_| constant_name(&type_.name))
        .collect();
    let _ = writeln!(out, "/// 全部类型，顺序和官方 `Types.values()` 相同。");
    let _ = writeln!(
        out,
        "pub static TYPES: &[HeightmapType] = &[{}];",
        constants.join(", ")
    );
    Ok(out)
}

/// 生成的文件按下标取类型，所以 `id` 必须是从 0 开始连续的，名字也要唯一。
fn check_heightmap_types(types: &[HeightmapType]) -> Result<(), String> {
    if types.is_empty() {
        return Err("没有导出高度图类型".to_owned());
    }
    for (index, type_) in types.iter().enumerate() {
        if type_.id != index as i32 {
            return Err(format!(
                "高度图 {} 的 id 是 {}，应该是 {index}",
                type_.name, type_.id
            ));
        }
    }
    let mut names: Vec<&str> = types.iter().map(|type_| type_.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.len() != types.len() {
        return Err("高度图类型的名字不唯一".to_owned());
    }
    Ok(())
}

/// `minecraft:level_chunk_with_light` → `LEVEL_CHUNK_WITH_LIGHT`。
fn constant_name(identifier: &str) -> String {
    let path = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    path.replace(['/', '.', '-'], "_").to_uppercase()
}

fn packets_file(data: &VersionData) -> String {
    let mut out = String::new();
    for (state, directions) in &data.packets {
        let _ = writeln!(out, "pub mod {state} {{");
        for (direction, packets) in directions {
            let _ = writeln!(out, "    pub mod {direction} {{");
            for (name, packet) in packets {
                let _ = writeln!(
                    out,
                    "        pub const {}: i32 = {};",
                    constant_name(name),
                    packet.protocol_id
                );
            }
            let _ = writeln!(out, "    }}");
        }
        let _ = writeln!(out, "}}\n");
    }
    out
}

fn blocks_file(data: &VersionData) -> Result<String, String> {
    let states = &data.block_states;
    check_state_ids(&data.blocks, states.total)?;
    check_properties(&data.blocks)?;
    let block_tags = tag_indexes(&data.tags, BLOCK_REGISTRY)?;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "use crate::{{Block, BlockClass, BlockField, BlockOffset, FieldValue, PropertyDef, PushReaction}};"
    );
    let _ = writeln!(out, "use crate::tags::block_tags;\n");
    let _ = writeln!(out, "/// 方块状态的总数，决定了全局调色板的位数。");
    let _ = writeln!(out, "pub const TOTAL_STATES: u32 = {};\n", states.total);
    let _ = writeln!(out, "/// 各方块默认状态的 ID。");
    for (name, id) in &states.defaults {
        let _ = writeln!(out, "pub const {}: u32 = {id};", constant_name(name));
    }

    let mut properties = SharedLists::new("PROPERTIES", "&[PropertyDef]");
    let mut fields = SharedLists::new("FIELDS", "&[BlockField]");
    let mut table = String::new();
    for block in &data.blocks {
        let _ = writeln!(
            table,
            "    Block {{ name: {:?}, first_state: {}, default_state: {}, item: {}, has_collision: {}, has_fluid: {}, two_blocks: {}, push_reaction: PushReaction::{}, unbreakable: {}, block_entity_type: {:?}, class: BlockClass::{}, properties: {}, fields: {}, instrument: {:?}, offset: {}, dynamic_shape: {}, destroy_time: {:?}, requires_correct_tool_for_drops: {} }},",
            block.name,
            block.states[0],
            block.default_state,
            block.item,
            block.has_collision,
            block.has_fluid,
            block.two_blocks,
            push_reaction(&block.push_reaction)?,
            block.unbreakable,
            block.block_entity_type,
            block.class,
            properties.name_of(property_list(&block.properties)),
            fields.name_of(field_list(&block.fields, &block_tags)?),
            block.instrument,
            offset_literal(block),
            block.dynamic_shape,
            block.destroy_time,
            block.requires_correct_tool_for_drops,
        );
    }

    out.push('\n');
    out.push_str(&properties.definitions());
    out.push_str(&fields.definitions());
    let _ = writeln!(out, "\n/// 全部方块，下标就是注册表 ID。");
    let _ = writeln!(out, "pub const BLOCKS: &[Block] = &[");
    out.push_str(&table);
    let _ = writeln!(out, "];");
    Ok(out)
}

/// 很多方块的属性和字段完全相同，相同的列表只写一次。
struct SharedLists {
    prefix: &'static str,
    type_name: &'static str,
    /// 列表的源码 → 它的编号。
    known: BTreeMap<String, usize>,
    ordered: Vec<String>,
}

impl SharedLists {
    fn new(prefix: &'static str, type_name: &'static str) -> Self {
        Self {
            prefix,
            type_name,
            known: BTreeMap::new(),
            ordered: Vec::new(),
        }
    }

    fn name_of(&mut self, list: String) -> String {
        let next = self.ordered.len();
        let index = *self.known.entry(list.clone()).or_insert(next);
        if index == next {
            self.ordered.push(list);
        }
        format!("{}_{index}", self.prefix)
    }

    fn definitions(&self) -> String {
        let mut out = String::new();
        for (index, list) in self.ordered.iter().enumerate() {
            let _ = writeln!(
                out,
                "const {}_{index}: {} = {list};",
                self.prefix, self.type_name
            );
        }
        out
    }
}

fn property_list(properties: &[Property]) -> String {
    let items: Vec<String> = properties
        .iter()
        .map(|property| {
            format!(
                "PropertyDef {{ name: {:?}, values: &{:?} }}",
                property.name, property.values
            )
        })
        .collect();
    format!("&[{}]", items.join(", "))
}

fn field_list(fields: &[BlockField], block_tags: &TagIndexes) -> Result<String, String> {
    let items = fields
        .iter()
        .map(|field| {
            Ok(format!(
                "BlockField {{ name: {:?}, value: {} }}",
                field.name,
                field_value(&field.value, block_tags)?
            ))
        })
        .collect::<Result<Vec<String>, String>>()?;
    Ok(format!("&[{}]", items.join(", ")))
}

fn field_value(value: &FieldValue, block_tags: &TagIndexes) -> Result<String, String> {
    Ok(match value {
        FieldValue::Bool(value) => format!("FieldValue::Bool({value})"),
        FieldValue::Int(value) => format!("FieldValue::Int({value})"),
        FieldValue::Float(value) => format!("FieldValue::Float({value:?})"),
        FieldValue::Double(value) => format!("FieldValue::Double({value:?})"),
        FieldValue::Block(value) => format!("FieldValue::Block({value})"),
        FieldValue::State(value) => format!("FieldValue::State({value})"),
        FieldValue::Item(value) => format!("FieldValue::Item({value})"),
        FieldValue::Direction(value) => format!("FieldValue::Direction({value})"),
        FieldValue::Name(value) => format!("FieldValue::Name({value:?})"),
        FieldValue::Tag(name) => {
            if !block_tags.contains_key(name.as_str()) {
                return Err(format!("字段引用了不存在的方块标签 {name}"));
            }
            format!("FieldValue::Tag(block_tags::{})", constant_name(name))
        }
    })
}

/// 官方 `PushReaction` 的名字，小写换成 Rust 的常量名。官方加了新的值时这里要跟着加。
fn push_reaction(name: &str) -> Result<&'static str, String> {
    match name {
        "push_pull" => Ok("PushPull"),
        "push" => Ok("Push"),
        "popped" => Ok("Popped"),
        "immoveable" => Ok("Immoveable"),
        "ignore_entity" => Ok("IgnoreEntity"),
        other => Err(format!("未知的 PushReaction {other}")),
    }
}

fn offset_literal(block: &Block) -> String {
    block.offset.map_or_else(
        || "None".to_owned(),
        |offset| {
            format!(
                "Some(BlockOffset {{ vertical: {}, max_horizontal: {:?}, max_vertical: {:?}, moves_shape: {} }})",
                offset.vertical, offset.max_horizontal, offset.max_vertical, offset.moves_shape
            )
        },
    )
}

/// `BlockState` 的取值和改值靠这条规律：状态数就是属性取值数的乘积。
/// `Extractor.java` 校验了逐个状态的取值，这里再确认数量，防止两边的表对不上。
fn check_properties(blocks: &[Block]) -> Result<(), String> {
    for block in blocks {
        let product: usize = block
            .properties
            .iter()
            .map(|property| property.values.len())
            .product();
        if product != block.states.len() {
            return Err(format!(
                "{} 有 {} 个状态，属性的笛卡尔积是 {product}",
                block.name,
                block.states.len()
            ));
        }
    }
    Ok(())
}

fn block_classes_file(data: &VersionData) -> String {
    let classes: BTreeMap<&str, &[String]> = data
        .blocks
        .iter()
        .map(|block| (block.class.as_str(), block.ancestors.as_slice()))
        .collect();
    let ancestors: BTreeSet<&str> = classes
        .values()
        .flat_map(|names| names.iter().map(String::as_str))
        .collect();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// 方块的官方类名。行为按它分派，新版本多一个类时分派表编译失败。"
    );
    let _ = writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]");
    let _ = writeln!(out, "pub enum BlockClass {{");
    for class in classes.keys() {
        let _ = writeln!(out, "    {class},");
    }
    let _ = writeln!(out, "}}\n");

    let _ = writeln!(
        out,
        "/// 官方的类和方块包里的接口，也就是 `instanceof` 能写的名字，不含 `Block` 自己。"
    );
    let _ = writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]");
    let _ = writeln!(out, "pub enum BlockAncestor {{");
    for ancestor in &ancestors {
        let _ = writeln!(out, "    {ancestor},");
    }
    let _ = writeln!(out, "}}\n");

    let _ = writeln!(out, "impl BlockClass {{");
    let _ = writeln!(out, "    /// 这个类、它的父类和实现的接口。");
    let _ = writeln!(
        out,
        "    pub const fn ancestors(self) -> &'static [BlockAncestor] {{"
    );
    let _ = writeln!(out, "        match self {{");
    for (class, names) in &classes {
        let list: Vec<String> = names
            .iter()
            .map(|name| format!("BlockAncestor::{name}"))
            .collect();
        let _ = writeln!(out, "            Self::{class} => &[{}],", list.join(", "));
    }
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    out
}

fn block_maps_file(data: &VersionData) -> String {
    let maps = &data.block_maps;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// `FlowerPotBlock.POTTED_BY_CONTENT`：内容 → 花盆，都是方块 ID。"
    );
    let _ = writeln!(
        out,
        "pub const FLOWER_POTS: &[(i32, i32)] = &{:?};\n",
        maps.flower_pots
    );
    let _ = writeln!(
        out,
        "/// `CandleCakeBlock.BY_CANDLE`：蜡烛 → 蜡烛蛋糕，都是方块 ID。"
    );
    let _ = writeln!(
        out,
        "pub const CANDLE_CAKES: &[(i32, i32)] = &{:?};\n",
        maps.candle_cakes
    );
    let _ = writeln!(
        out,
        "/// `WeatheringCopper.NEXT_BY_BLOCK`：氧化前 → 氧化后，都是方块 ID。"
    );
    let _ = writeln!(
        out,
        "pub const WEATHERING: &[(i32, i32)] = &{:?};\n",
        maps.weathering
    );
    let _ = writeln!(
        out,
        "/// `HoneycombItem.WAXABLES`：涂蜡前 → 涂蜡后，都是方块 ID。"
    );
    let _ = writeln!(
        out,
        "pub const WAXABLES: &[(i32, i32)] = &{:?};\n",
        maps.waxables
    );
    let _ = writeln!(
        out,
        "/// `FireBlock` 的两张表：方块 ID、`igniteOdds`、`burnOdds`。"
    );
    let _ = writeln!(
        out,
        "pub const FIRE_ODDS: &[(i32, i32, i32)] = &{:?};\n",
        maps.fire_odds
    );
    let _ = writeln!(
        out,
        "/// `ignitedByLava` 的方块 ID，升序。岩浆的随机刻只点燃旁边的这些方块。"
    );
    let _ = writeln!(
        out,
        "pub const IGNITED_BY_LAVA: &[i32] = &{:?};\n",
        maps.ignited_by_lava
    );
    let _ = writeln!(
        out,
        "/// `RecipePropertySet.CAMPFIRE_INPUT`：能放上营火的物品 ID。"
    );
    let _ = writeln!(
        out,
        "pub const CAMPFIRE_INPUTS: &[i32] = &{:?};",
        maps.campfire_inputs
    );
    out
}

fn block_transformers_file(data: &VersionData) -> Result<String, String> {
    let block_tags = tag_indexes(&data.tags, BLOCK_REGISTRY)?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "use crate::{{BlockPredicate, BlockTransform, BlockTransformer, StateProvider, StateRule}};"
    );
    let _ = writeln!(out, "use crate::tags::block_tags;");
    for transformer in &data.block_transformers {
        let _ = writeln!(out, "\n/// `{}`。", transformer.name);
        let _ = writeln!(
            out,
            "pub static {}: BlockTransformer = BlockTransformer {{",
            constant_name(&transformer.name)
        );
        let _ = writeln!(out, "    name: {:?},", transformer.name);
        let _ = writeln!(out, "    transforms: &[");
        for transform in &transformer.transforms {
            let _ = writeln!(
                out,
                "        BlockTransform {{ provider: {}, disallowed_faces: &{:?}, update_from_neighbors: {} }},",
                state_provider(&transform.provider, data, &block_tags)?,
                transform.disallowed_faces,
                transform.update_from_neighbors
            );
        }
        let _ = writeln!(out, "    ],");
        let _ = writeln!(out, "}};");
    }
    Ok(out)
}

/// 规则多的提供器一条规则一行，斧子有一百多条。
fn state_provider(
    provider: &StateProvider,
    data: &VersionData,
    block_tags: &TagIndexes,
) -> Result<String, String> {
    Ok(match provider {
        StateProvider::Simple(state) => {
            if *state >= data.block_states.total {
                return Err(format!("状态提供器引用了不存在的状态 {state}"));
            }
            format!("StateProvider::Simple({state})")
        }
        StateProvider::CopyProperties(source) => format!(
            "StateProvider::CopyProperties(&{})",
            state_provider(source, data, block_tags)?
        ),
        StateProvider::RuleBased { rules, fallback } => {
            let rules = rules
                .iter()
                .map(|rule| {
                    Ok(format!(
                        "StateRule {{ if_true: {}, then: {} }}",
                        block_predicate(&rule.if_true, data, block_tags)?,
                        state_provider(&rule.then, data, block_tags)?
                    ))
                })
                .collect::<Result<Vec<String>, String>>()?;
            let fallback = match fallback {
                Some(fallback) => format!("Some(&{})", state_provider(fallback, data, block_tags)?),
                None => "None".to_owned(),
            };
            format!(
                "StateProvider::RuleBased {{ rules: &[\n            {},\n        ], fallback: {fallback} }}",
                rules.join(",\n            ")
            )
        }
    })
}

fn block_predicate(
    predicate: &BlockPredicate,
    data: &VersionData,
    block_tags: &TagIndexes,
) -> Result<String, String> {
    Ok(match predicate {
        BlockPredicate::MatchingBlocks { offset, blocks } => {
            let known = |id: &i32| usize::try_from(*id).is_ok_and(|id| id < data.blocks.len());
            if let Some(unknown) = blocks.iter().find(|id| !known(id)) {
                return Err(format!("方块判定引用了不存在的方块 {unknown}"));
            }
            format!("BlockPredicate::MatchingBlocks {{ offset: {offset:?}, blocks: &{blocks:?} }}")
        }
        BlockPredicate::MatchingBlockTag { offset, tag } => {
            if !block_tags.contains_key(tag.as_str()) {
                return Err(format!("方块判定引用了不存在的方块标签 {tag}"));
            }
            format!(
                "BlockPredicate::MatchingBlockTag {{ offset: {offset:?}, tag: block_tags::{} }}",
                constant_name(tag)
            )
        }
        BlockPredicate::AllOf(parts) => {
            let parts = parts
                .iter()
                .map(|part| block_predicate(part, data, block_tags))
                .collect::<Result<Vec<String>, String>>()?;
            format!("BlockPredicate::AllOf(&[{}])", parts.join(", "))
        }
    })
}

/// 写一张每个状态一项的表，一行 32 项。
fn write_state_table<T: Debug>(out: &mut String, doc: &str, name: &str, kind: &str, values: &[T]) {
    let _ = writeln!(out, "/// {doc}");
    let _ = writeln!(out, "pub static {name}: &[{kind}] = &[");
    for row in values.chunks(32) {
        let row: Vec<String> = row.iter().map(|value| format!("{value:?}")).collect();
        let _ = writeln!(out, "    {},", row.join(", "));
    }
    let _ = writeln!(out, "];\n");
}

fn check_table_length<T>(name: &str, values: &[T], total: u32) -> Result<(), String> {
    if values.len() == total as usize {
        Ok(())
    } else {
        Err(format!("{name} 有 {} 项，状态总数是 {total}", values.len()))
    }
}

fn states_file(data: &VersionData) -> Result<String, String> {
    let tables = &data.state_tables;
    let total = data.block_states.total;
    check_table_length("STATE_FACTS", &tables.facts, total)?;
    check_table_length("STATE_FLUID", &tables.fluid, total)?;
    check_table_length("LIGHT_DAMPENING", &tables.light_dampening, total)?;

    let mut out = String::new();
    let _ = writeln!(out, "use crate::FluidFacts;\n");
    write_state_table(
        &mut out,
        "每个状态的事实，位的含义见 [`crate::facts`]。",
        "STATE_FACTS",
        "u64",
        &tables.facts,
    );
    write_state_table(
        &mut out,
        "每个状态的流体在 [`FLUID_STATES`] 里的下标。",
        "STATE_FLUID",
        "u8",
        &tables.fluid,
    );
    write_state_table(
        &mut out,
        "每个状态的 `getLightDampening`。",
        "LIGHT_DAMPENING",
        "u8",
        &tables.light_dampening,
    );

    let _ = writeln!(out, "/// 去重后的流体状态，第一项是空流体。");
    let _ = writeln!(out, "pub static FLUID_STATES: &[FluidFacts] = &[");
    for state in &tables.fluid_states {
        let _ = writeln!(
            out,
            "    FluidFacts {{ fluid: {}, source: {}, amount: {}, falling: {}, legacy_block: {} }},",
            state.fluid, state.source, state.amount, state.falling, state.legacy_block
        );
    }
    let _ = writeln!(out, "];");
    Ok(out)
}

fn shapes_file(data: &VersionData) -> Result<String, String> {
    let tables = &data.state_tables;
    let total = data.block_states.total;
    check_table_length("COLLISION_SHAPE", &tables.collision, total)?;
    check_table_length("OUTLINE_SHAPE", &tables.outline, total)?;
    check_table_length("SUPPORT_SHAPE", &tables.support, total)?;

    let mut out = String::new();
    let _ = writeln!(out, "use mc_bamboo_core::Aabb;\n");
    let _ = writeln!(
        out,
        "/// 去重后的形状，下标 0 是空形状，下标 1 是完整方块。一个形状是若干盒子的并集。"
    );
    let _ = writeln!(out, "pub static SHAPES: &[&[Aabb]] = &[");
    for shape in &tables.shapes {
        let boxes: Vec<String> = shape
            .iter()
            .map(|[x1, y1, z1, x2, y2, z2]| {
                format!("Aabb::new({x1:?}, {y1:?}, {z1:?}, {x2:?}, {y2:?}, {z2:?})")
            })
            .collect();
        let _ = writeln!(out, "    &[{}],", boxes.join(", "));
    }
    let _ = writeln!(out, "];\n");

    write_state_table(
        &mut out,
        "每个形状 6 个方向的官方结果：低 6 位 `Block.isFaceFull`，高 6 位这个面的切片不为空。",
        "SHAPE_FACES",
        "u16",
        &tables.shape_faces,
    );
    write_state_table(
        &mut out,
        "`getCollisionShape`（`CollisionContext.empty()`）在 [`SHAPES`] 里的下标。",
        "COLLISION_SHAPE",
        "u16",
        &tables.collision,
    );
    write_state_table(
        &mut out,
        "`getShape` 在 [`SHAPES`] 里的下标。",
        "OUTLINE_SHAPE",
        "u16",
        &tables.outline,
    );
    write_state_table(
        &mut out,
        "`getBlockSupportShape` 在 [`SHAPES`] 里的下标。",
        "SUPPORT_SHAPE",
        "u16",
        &tables.support,
    );

    let _ = writeln!(
        out,
        "/// `placementContext` 下碰撞和普通上下文不同的状态：状态 ID 和形状下标，按 ID 排序。"
    );
    let _ = writeln!(
        out,
        "pub static PLACEMENT_COLLISION: &[(u32, u16)] = &{:?};\n",
        tables.placement_collision
    );
    let _ = writeln!(
        out,
        "/// 形状跟着渲染偏移走的状态在几个位置上的 `isFaceSturdy`：状态 ID、x、y、z、18 位。"
    );
    let _ = writeln!(
        out,
        "pub static OFFSET_STURDY_SAMPLES: &[(u32, i32, i32, i32, u32)] = &{:?};",
        tables.offset_sturdy_samples
    );
    Ok(out)
}

fn light_file(data: &VersionData) -> Result<String, String> {
    let tables = &data.light_tables;
    let total = data.block_states.total;
    check_table_length("LIGHT_EMISSION", &tables.emission, total)?;
    check_table_length("LIGHT_OCCLUSION", &tables.occlusion, total)?;
    let words = (tables.face_count * tables.face_count).div_ceil(64);
    if tables.face_occludes.len() != words {
        return Err(format!(
            "FACE_OCCLUDES 有 {} 项，{} 个面形状要 {words} 项",
            tables.face_occludes.len(),
            tables.face_count
        ));
    }

    let mut out = String::new();
    write_state_table(
        &mut out,
        "每个状态的 `getLightEmission`。",
        "LIGHT_EMISSION",
        "u8",
        &tables.emission,
    );
    write_state_table(
        &mut out,
        "每个状态在 [`OCCLUSION_FACES`] 里的下标。",
        "LIGHT_OCCLUSION",
        "u16",
        &tables.occlusion,
    );
    let _ = writeln!(
        out,
        "/// 去重后的 `LightEngine.getOcclusionShape`，按 `Direction` 的 ID 排列 6 个面形状的编号。\n/// 第一项全是空形状。编号 0 是空形状，1 是完整方块。"
    );
    let _ = writeln!(out, "pub static OCCLUSION_FACES: &[[u16; 6]] = &[");
    for faces in &tables.occlusion_faces {
        let _ = writeln!(out, "    {faces:?},");
    }
    let _ = writeln!(out, "];\n");
    let _ = writeln!(out, "/// 去重后的面形状个数。");
    let _ = writeln!(
        out,
        "pub const FACE_SHAPE_COUNT: usize = {};\n",
        tables.face_count
    );
    let occludes: Vec<u64> = tables
        .face_occludes
        .iter()
        .map(|&word| word as u64)
        .collect();
    write_state_table(
        &mut out,
        "第 `a * FACE_SHAPE_COUNT + b` 位是官方的 `Shapes.faceShapeOccludes(a, b)`，低位在前。",
        "FACE_OCCLUDES",
        "u64",
        &occludes,
    );
    Ok(out)
}

/// `block_of_state` 的二分查找依赖这条规律：方块按注册表 ID 排列时，状态 ID 从 0 开始连续递增。
fn check_state_ids(blocks: &[Block], total: u32) -> Result<(), String> {
    let mut next = 0;
    for block in blocks {
        if block.states.is_empty() {
            return Err(format!("{} 没有方块状态", block.name));
        }
        for &state in &block.states {
            if state != next {
                return Err(format!(
                    "{} 的状态 ID 不连续：应该是 {next}，实际是 {state}",
                    block.name
                ));
            }
            next += 1;
        }
    }
    if next == total {
        Ok(())
    } else {
        Err(format!("方块一共有 {next} 个状态，官方的总数是 {total}"))
    }
}

fn items_file(data: &VersionData) -> Result<String, String> {
    let mut out = String::new();
    let _ = writeln!(out, "use crate::{{CompostLayers, Item, ItemKind}};\n");
    let _ = writeln!(out, "/// 各物品的注册表 ID。");
    for (id, item) in data.items.iter().enumerate() {
        let _ = writeln!(out, "pub const {}: i32 = {id};", constant_name(&item.name));
    }

    let _ = writeln!(out, "\n/// 全部物品，下标就是注册表 ID。");
    let _ = writeln!(out, "pub const ITEMS: &[Item] = &[");
    for item in &data.items {
        let _ = writeln!(
            out,
            "    Item {{ name: {:?}, kind: ItemKind::{}, block: {:?}, wall_block: {:?}, attachment: {:?}, max_stack_size: {}, can_destroy_blocks_in_creative: {}, consumable: {}, compostable: {}, jukebox_playable: {}, block_transformer: {}, blocks_attacks: {} }},",
            item.name,
            item_kind(item)?,
            item.block,
            item.wall_block,
            item.attachment,
            item.max_stack_size,
            item.can_destroy_blocks_in_creative,
            item.consumable,
            compost_layers_of(item),
            item.jukebox_playable,
            block_transformer_of(item, data)?,
            item.blocks_attacks
        );
    }
    let _ = writeln!(out, "];");
    Ok(out)
}

/// 物品的 `compostable` 组件。
fn compost_layers_of(item: &Item) -> String {
    match &item.compostable {
        None => "None".to_owned(),
        Some(CompostLayers::Constant(layers)) => format!("Some(CompostLayers::Constant({layers}))"),
        Some(CompostLayers::Weighted {
            when_empty,
            distribution,
        }) => format!(
            "Some(CompostLayers::Weighted {{ when_empty: {when_empty}, distribution: &{distribution:?} }})"
        ),
    }
}

/// 物品的 `block_transformer` 组件，指向 `block_transformers.rs` 里的条目。
fn block_transformer_of(item: &Item, data: &VersionData) -> Result<String, String> {
    let Some(name) = &item.block_transformer else {
        return Ok("None".to_owned());
    };
    if !data
        .block_transformers
        .iter()
        .any(|transformer| &transformer.name == name)
    {
        return Err(format!(
            "{} 的 block_transformer 是不存在的 {name}",
            item.name
        ));
    }
    Ok(format!(
        "Some(&crate::block_transformers::{})",
        constant_name(name)
    ))
}

/// 官方的物品类决定放置走哪条路，或者非方块物品的 `useOn` 走哪条路。
/// 新出现的方块物品类要先看过它怎么放，再加进这张表。
fn item_kind(item: &Item) -> Result<&'static str, String> {
    match item.class.as_str() {
        "FlintAndSteelItem" => Ok("FlintAndSteel"),
        "FireChargeItem" => Ok("FireCharge"),
        "HoneycombItem" => Ok("Honeycomb"),
        "ShearsItem" => Ok("Shears"),
        "EnderEyeItem" => Ok("EnderEye"),
        "DebugStickItem" => Ok("DebugStick"),
        "BrushItem" => Ok("Brush"),
        "BoneMealItem" => Ok("BoneMeal"),
        "BlockItem" => Ok("Block"),
        "StandingAndWallBlockItem" => Ok("StandingAndWall"),
        "BannerItem" => Ok("Banner"),
        "HangingSignItem" => Ok("HangingSign"),
        "PlayerHeadItem" => Ok("PlayerHead"),
        "DoubleHighBlockItem" => Ok("DoubleHigh"),
        "PlaceOnWaterBlockItem" => Ok("PlaceOnWater"),
        "GameMasterBlockItem" => Ok("GameMaster"),
        "ScaffoldingBlockItem" => Ok("Scaffolding"),
        "SolidBucketItem" => Ok("SolidBucket"),
        _ if item.block.is_none() => Ok("Other"),
        class => Err(format!("{} 用了未知的方块物品类 {class}", item.name)),
    }
}

/// 原料本身不是列表，去重池单独写一份。
struct SharedIngredients {
    /// 源码 → 编号。
    known: BTreeMap<String, usize>,
    ordered: Vec<String>,
}

impl SharedIngredients {
    fn new() -> Self {
        Self {
            known: BTreeMap::new(),
            ordered: Vec::new(),
        }
    }

    fn name_of(&mut self, ingredient: String) -> String {
        let next = self.ordered.len();
        let index = *self.known.entry(ingredient.clone()).or_insert(next);
        if index == next {
            self.ordered.push(ingredient);
        }
        format!("INGREDIENT_{index}")
    }

    fn definitions(&self) -> String {
        let mut out = String::new();
        for (index, ingredient) in self.ordered.iter().enumerate() {
            let _ = writeln!(out, "const INGREDIENT_{index}: Ingredient = {ingredient};");
        }
        out
    }
}

/// 物品标签名 → 成员物品 ID。配方里的标签原料靠它核对展开结果。
fn item_tag_members(data: &VersionData) -> Result<BTreeMap<&str, &[i32]>, String> {
    let group_index = tag_group_index(&data.tags, ITEM_REGISTRY)?;
    Ok(tag_indexes(&data.tags, ITEM_REGISTRY)?
        .into_iter()
        .map(|(name, index)| (name, data.tags[group_index].tags[index].ids.as_slice()))
        .collect())
}

fn check_item(item: i32, data: &VersionData) -> Result<(), String> {
    if item < 0 || item as usize >= data.items.len() {
        return Err(format!("引用了不存在的物品 ID {item}"));
    }
    Ok(())
}

/// 核对一份物品 ID 列表，保证可以直接用二分查找。
fn checked_item_list<'a>(
    items: &'a [i32],
    data: &VersionData,
    what: &str,
) -> Result<&'a [i32], String> {
    for &item in items {
        check_item(item, data)?;
    }
    if items.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("{what}列表不是升序且不重复"));
    }
    Ok(items)
}

/// 核对一个原料：物品 ID 都在注册表里，标签确实存在且展开结果和官方标签一致。
fn check_ingredient(
    ingredient: &ExtractedIngredient,
    data: &VersionData,
    item_tags: &BTreeMap<&str, &[i32]>,
) -> Result<(), String> {
    for &item in &ingredient.items {
        check_item(item, data)?;
    }
    if ingredient.items.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("原料的物品列表不是升序且不重复".to_owned());
    }
    if let Some(tag) = &ingredient.tag {
        let Some(members) = item_tags.get(tag.as_str()) else {
            return Err(format!("原料引用了不存在的物品标签 {tag}"));
        };
        let mut expected = members.to_vec();
        expected.sort_unstable();
        expected.dedup();
        if expected != ingredient.items {
            return Err(format!("标签 {tag} 展开的物品和官方标签不一致"));
        }
    }
    Ok(())
}

/// 导出端给的序列化名必须和种类对得上，防止两边对同一个配方有两种说法。
fn check_serializer(recipe: &ExtractedRecipe) -> Result<(), String> {
    let expected = match &recipe.kind {
        ExtractedRecipeKind::Shaped { .. } => "minecraft:crafting_shaped",
        ExtractedRecipeKind::Shapeless { .. } => "minecraft:crafting_shapeless",
        ExtractedRecipeKind::Transmute { .. } => "minecraft:crafting_transmute",
        ExtractedRecipeKind::Dye { .. } => "minecraft:crafting_dye",
        ExtractedRecipeKind::Imbue { .. } => "minecraft:crafting_imbue",
        ExtractedRecipeKind::DecoratedPot { .. } => "minecraft:crafting_decorated_pot",
        ExtractedRecipeKind::Smelting { .. } => "minecraft:smelting",
        ExtractedRecipeKind::Blasting { .. } => "minecraft:blasting",
        ExtractedRecipeKind::Smoking { .. } => "minecraft:smoking",
        ExtractedRecipeKind::CampfireCooking { .. } => "minecraft:campfire_cooking",
        ExtractedRecipeKind::Stonecutting { .. } => "minecraft:stonecutting",
        ExtractedRecipeKind::SmithingTransform { .. } => "minecraft:smithing_transform",
        ExtractedRecipeKind::SmithingTrim { .. } => "minecraft:smithing_trim",
        ExtractedRecipeKind::Brewing { .. } => "minecraft:brewing",
        ExtractedRecipeKind::FireworkRocket { .. } => "minecraft:crafting_special_firework_rocket",
        ExtractedRecipeKind::FireworkStar { .. } => "minecraft:crafting_special_firework_star",
        ExtractedRecipeKind::FireworkStarFade { .. } => {
            "minecraft:crafting_special_firework_star_fade"
        }
        ExtractedRecipeKind::Mapextending { .. } => "minecraft:crafting_special_mapextending",
        ExtractedRecipeKind::BannerDuplicate { .. } => "minecraft:crafting_special_bannerduplicate",
        ExtractedRecipeKind::BookCloning { .. } => "minecraft:crafting_special_bookcloning",
        ExtractedRecipeKind::ShieldDecoration { .. } => {
            "minecraft:crafting_special_shielddecoration"
        }
        ExtractedRecipeKind::RepairItem => "minecraft:crafting_special_repairitem",
    };
    if recipe.serializer != expected {
        return Err(format!(
            "序列化名是 {}，按种类应该是 {expected}",
            recipe.serializer
        ));
    }
    Ok(())
}

/// 生成配方表时共用的去重池和标签表。
struct RecipeWriter<'a> {
    data: &'a VersionData,
    item_tags: BTreeMap<&'a str, &'a [i32]>,
    item_lists: SharedLists,
    ingredients: SharedIngredients,
    patterns: SharedLists,
    ingredient_slices: SharedLists,
    shapes: SharedLists,
}

impl<'a> RecipeWriter<'a> {
    fn new(data: &'a VersionData) -> Result<Self, String> {
        Ok(Self {
            data,
            item_tags: item_tag_members(data)?,
            item_lists: SharedLists::new("ITEM_LIST", "&[i32]"),
            ingredients: SharedIngredients::new(),
            patterns: SharedLists::new("PATTERN", "&[Option<Ingredient>]"),
            ingredient_slices: SharedLists::new("INGREDIENTS", "&[Ingredient]"),
            shapes: SharedLists::new("SHAPES", "&[(FireworkShape, Ingredient)]"),
        })
    }

    fn kind(&mut self, recipe: &ExtractedRecipe) -> Result<String, String> {
        check_serializer(recipe)?;
        Ok(match &recipe.kind {
            ExtractedRecipeKind::Shaped {
                width,
                height,
                pattern,
                result,
            } => self.shaped(*width, *height, pattern, result)?,
            ExtractedRecipeKind::Shapeless {
                ingredients,
                result,
            } => self.shapeless(ingredients, result)?,
            ExtractedRecipeKind::Transmute {
                input,
                material,
                min_material,
                max_material,
                result_item,
                result_count,
                result_has_components,
                add_material_count,
            } => {
                let input = self.ingredient(input)?;
                let material = self.ingredient(material)?;
                let result_item = match result_item {
                    None => "None".to_owned(),
                    Some(item) => {
                        check_item(*item, self.data)?;
                        format!("Some({item})")
                    }
                };
                if *result_count <= 0 {
                    return Err(format!("产物数量是 {result_count}"));
                }
                format!(
                    "RecipeKind::Transmute {{ input: {input}, material: {material}, \
                     min_material: {min_material}, max_material: {max_material}, \
                     result_item: {result_item}, result_count: {result_count}, \
                     result_has_components: {result_has_components}, \
                     add_material_count: {add_material_count} }}"
                )
            }
            ExtractedRecipeKind::Dye {
                target,
                dye,
                result,
            } => {
                let target = self.ingredient(target)?;
                let dye = self.ingredient(dye)?;
                let result = self.stack(result)?;
                format!("RecipeKind::Dye {{ target: {target}, dye: {dye}, result: {result} }}")
            }
            ExtractedRecipeKind::Imbue {
                source,
                material,
                result,
            } => {
                let source = self.ingredient(source)?;
                let material = self.ingredient(material)?;
                let result = self.stack(result)?;
                format!(
                    "RecipeKind::Imbue {{ source: {source}, material: {material}, \
                     result: {result} }}"
                )
            }
            ExtractedRecipeKind::Smelting {
                ingredient,
                result,
                experience,
                time,
            } => self.cooking("Smelting", ingredient, result, *experience, *time)?,
            ExtractedRecipeKind::Blasting {
                ingredient,
                result,
                experience,
                time,
            } => self.cooking("Blasting", ingredient, result, *experience, *time)?,
            ExtractedRecipeKind::Smoking {
                ingredient,
                result,
                experience,
                time,
            } => self.cooking("Smoking", ingredient, result, *experience, *time)?,
            ExtractedRecipeKind::CampfireCooking {
                ingredient,
                result,
                experience,
                time,
            } => self.cooking("Campfire", ingredient, result, *experience, *time)?,
            ExtractedRecipeKind::Stonecutting { ingredient, result } => {
                self.stonecutting(ingredient, result)?
            }
            ExtractedRecipeKind::SmithingTransform {
                template,
                base,
                addition,
                result,
            } => self.smithing_transform(template, base, addition, result)?,
            ExtractedRecipeKind::SmithingTrim {
                template,
                base,
                addition,
                pattern,
            } => self.smithing_trim(template, base, addition, pattern)?,
            ExtractedRecipeKind::Brewing {
                input,
                reagent,
                result,
            } => self.brewing(input, reagent, result)?,
            ExtractedRecipeKind::FireworkRocket {
                shell,
                fuel,
                star,
                result,
            } => self.firework_rocket(shell, fuel, star, result)?,
            ExtractedRecipeKind::FireworkStar {
                shapes,
                trail,
                twinkle,
                fuel,
                dye,
                result,
            } => self.firework_star(shapes, trail, twinkle, fuel, dye, result)?,
            ExtractedRecipeKind::FireworkStarFade {
                target,
                dye,
                result,
            } => self.firework_star_fade(target, dye, result)?,
            ExtractedRecipeKind::Mapextending {
                map,
                material,
                result_item,
                result_count,
                result_has_components,
            } => self.map_extending(
                map,
                material,
                *result_item,
                *result_count,
                *result_has_components,
            )?,
            ExtractedRecipeKind::BannerDuplicate { banner, result } => {
                self.banner_duplicate(banner, result)?
            }
            ExtractedRecipeKind::BookCloning {
                source,
                material,
                min_generation,
                max_generation,
                result,
            } => self.book_cloning(source, material, *min_generation, *max_generation, result)?,
            ExtractedRecipeKind::ShieldDecoration {
                banner,
                target,
                result,
            } => self.shield_decoration(banner, target, result)?,
            ExtractedRecipeKind::RepairItem => "RecipeKind::RepairItem".to_owned(),
            ExtractedRecipeKind::DecoratedPot {
                back,
                left,
                right,
                front,
                result,
            } => self.decorated_pot(back, left, right, front, result)?,
        })
    }

    fn ingredient(&mut self, ingredient: &ExtractedIngredient) -> Result<String, String> {
        check_ingredient(ingredient, self.data, &self.item_tags)?;
        let items = self.item_lists.name_of(format!("&{:?}", ingredient.items));
        let tag = match &ingredient.tag {
            None => "None".to_owned(),
            Some(tag) => format!("Some({tag:?})"),
        };
        Ok(self
            .ingredients
            .name_of(format!("Ingredient {{ tag: {tag}, items: {items} }}")))
    }

    fn optional_ingredient(
        &mut self,
        ingredient: &Option<ExtractedIngredient>,
    ) -> Result<String, String> {
        Ok(match ingredient {
            None => "None".to_owned(),
            Some(ingredient) => format!("Some({})", self.ingredient(ingredient)?),
        })
    }

    fn ingredient_list(&mut self, list: &[ExtractedIngredient]) -> Result<String, String> {
        let names = list
            .iter()
            .map(|ingredient| self.ingredient(ingredient))
            .collect::<Result<Vec<_>, String>>()?;
        Ok(self
            .ingredient_slices
            .name_of(format!("&[{}]", names.join(", "))))
    }

    fn stack(&self, stack: &ExtractedStack) -> Result<String, String> {
        check_item(stack.item, self.data)?;
        if stack.count <= 0 {
            return Err(format!("产物数量是 {}", stack.count));
        }
        Ok(format!(
            "Stack {{ item: {}, count: {}, has_components: {} }}",
            stack.item, stack.count, stack.has_components
        ))
    }

    fn potion_ingredient(
        &mut self,
        ingredient: &ExtractedPotionIngredient,
    ) -> Result<String, String> {
        let item = self.ingredient(&ingredient.item)?;
        let potions = match &ingredient.potions {
            None => "None".to_owned(),
            Some(potions) => {
                // 官方只当集合用，排序去重后可以直接二分。
                let mut sorted = potions.clone();
                sorted.sort_unstable();
                sorted.dedup();
                for &potion in &sorted {
                    if potion < 0 || potion as usize >= self.data.recipes.potions.len() {
                        return Err(format!("酿造原料引用了不存在的药水 ID {potion}"));
                    }
                }
                format!("Some(&{sorted:?})")
            }
        };
        Ok(format!(
            "PotionIngredient {{ item: {item}, potions: {potions} }}"
        ))
    }

    fn pattern(&mut self, pattern: &[Option<ExtractedIngredient>]) -> Result<String, String> {
        let cells = pattern
            .iter()
            .map(|cell| match cell {
                None => Ok("None".to_owned()),
                Some(ingredient) => Ok(format!("Some({})", self.ingredient(ingredient)?)),
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(self.patterns.name_of(format!("&[{}]", cells.join(", "))))
    }

    fn shaped(
        &mut self,
        width: u8,
        height: u8,
        pattern: &[Option<ExtractedIngredient>],
        result: &ExtractedStack,
    ) -> Result<String, String> {
        if width == 0 || height == 0 || width > 3 || height > 3 {
            return Err(format!("有序配方是 {width}x{height} 格"));
        }
        if pattern.len() != usize::from(width) * usize::from(height) {
            return Err(format!(
                "有序配方有 {} 格，但尺寸是 {width}x{height}",
                pattern.len()
            ));
        }
        let pattern = self.pattern(pattern)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::Shaped {{ width: {width}, height: {height}, \
             pattern: {pattern}, result: {result} }}"
        ))
    }

    fn shapeless(
        &mut self,
        ingredients: &[ExtractedIngredient],
        result: &ExtractedStack,
    ) -> Result<String, String> {
        if ingredients.is_empty() {
            return Err("无序配方没有原料".to_owned());
        }
        let ingredients = self.ingredient_list(ingredients)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::Shapeless {{ ingredients: {ingredients}, result: {result} }}"
        ))
    }

    fn cooking(
        &mut self,
        kind: &str,
        ingredient: &ExtractedIngredient,
        result: &ExtractedStack,
        experience: f32,
        time: i32,
    ) -> Result<String, String> {
        if time <= 0 {
            return Err(format!("烹饪时间是 {time}"));
        }
        let ingredient = self.ingredient(ingredient)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::Cooking {{ kind: CookingKind::{kind}, ingredient: {ingredient}, \
             result: {result}, experience: {experience:?}, time: {time} }}"
        ))
    }

    fn stonecutting(
        &mut self,
        ingredient: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let ingredient = self.ingredient(ingredient)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::Stonecutting {{ ingredient: {ingredient}, result: {result} }}"
        ))
    }

    fn smithing_transform(
        &mut self,
        template: &Option<ExtractedIngredient>,
        base: &ExtractedIngredient,
        addition: &Option<ExtractedIngredient>,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let template = self.optional_ingredient(template)?;
        let base = self.ingredient(base)?;
        let addition = self.optional_ingredient(addition)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::SmithingTransform {{ template: {template}, base: {base}, \
             addition: {addition}, result: {result} }}"
        ))
    }

    fn smithing_trim(
        &mut self,
        template: &ExtractedIngredient,
        base: &ExtractedIngredient,
        addition: &ExtractedIngredient,
        pattern: &str,
    ) -> Result<String, String> {
        let template = self.ingredient(template)?;
        let base = self.ingredient(base)?;
        let addition = self.ingredient(addition)?;
        Ok(format!(
            "RecipeKind::SmithingTrim {{ template: {template}, base: {base}, \
             addition: {addition}, pattern: {pattern:?} }}"
        ))
    }

    fn brewing(
        &mut self,
        input: &ExtractedPotionIngredient,
        reagent: &ExtractedPotionIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let input = self.potion_ingredient(input)?;
        let reagent = self.potion_ingredient(reagent)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::Brewing {{ input: {input}, reagent: {reagent}, result: {result} }}"
        ))
    }

    fn firework_rocket(
        &mut self,
        shell: &ExtractedIngredient,
        fuel: &ExtractedIngredient,
        star: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let shell = self.ingredient(shell)?;
        let fuel = self.ingredient(fuel)?;
        let star = self.ingredient(star)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::FireworkRocket {{ shell: {shell}, fuel: {fuel}, star: {star}, \
             result: {result} }}"
        ))
    }

    fn firework_star(
        &mut self,
        shapes: &[ExtractedShape],
        trail: &ExtractedIngredient,
        twinkle: &ExtractedIngredient,
        fuel: &ExtractedIngredient,
        dye: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let mut sorted: Vec<&ExtractedShape> = shapes.iter().collect();
        sorted.sort_by_key(|shape| shape.shape.as_str());
        let mut entries = Vec::with_capacity(sorted.len());
        for shape in sorted {
            let kind = match shape.shape.as_str() {
                "small_ball" => "SmallBall",
                "large_ball" => "LargeBall",
                "star" => "Star",
                "creeper" => "Creeper",
                "burst" => "Burst",
                other => return Err(format!("未知的烟花形状 {other}")),
            };
            let ingredient = self.ingredient(&shape.ingredient)?;
            entries.push(format!("(FireworkShape::{kind}, {ingredient})"));
        }
        let shapes = self.shapes.name_of(format!("&[{}]", entries.join(", ")));
        let trail = self.ingredient(trail)?;
        let twinkle = self.ingredient(twinkle)?;
        let fuel = self.ingredient(fuel)?;
        let dye = self.ingredient(dye)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::FireworkStar {{ shapes: {shapes}, trail: {trail}, \
             twinkle: {twinkle}, fuel: {fuel}, dye: {dye}, result: {result} }}"
        ))
    }

    fn firework_star_fade(
        &mut self,
        target: &ExtractedIngredient,
        dye: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let target = self.ingredient(target)?;
        let dye = self.ingredient(dye)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::FireworkStarFade {{ target: {target}, dye: {dye}, result: {result} }}"
        ))
    }

    fn map_extending(
        &mut self,
        map: &ExtractedIngredient,
        material: &ExtractedIngredient,
        result_item: Option<i32>,
        result_count: i32,
        result_has_components: bool,
    ) -> Result<String, String> {
        let map = self.ingredient(map)?;
        let material = self.ingredient(material)?;
        let result_item = match result_item {
            None => "None".to_owned(),
            Some(item) => {
                check_item(item, self.data)?;
                format!("Some({item})")
            }
        };
        if result_count <= 0 {
            return Err(format!("产物数量是 {result_count}"));
        }
        Ok(format!(
            "RecipeKind::MapExtending {{ map: {map}, material: {material}, \
             result_item: {result_item}, result_count: {result_count}, \
             result_has_components: {result_has_components} }}"
        ))
    }

    fn banner_duplicate(
        &mut self,
        banner: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let banner = self.ingredient(banner)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::BannerDuplicate {{ banner: {banner}, result: {result} }}"
        ))
    }

    fn book_cloning(
        &mut self,
        source: &ExtractedIngredient,
        material: &ExtractedIngredient,
        min_generation: i32,
        max_generation: i32,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let source = self.ingredient(source)?;
        let material = self.ingredient(material)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::BookCloning {{ source: {source}, material: {material}, \
             min_generation: {min_generation}, max_generation: {max_generation}, \
             result: {result} }}"
        ))
    }

    fn shield_decoration(
        &mut self,
        banner: &ExtractedIngredient,
        target: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let banner = self.ingredient(banner)?;
        let target = self.ingredient(target)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::ShieldDecoration {{ banner: {banner}, target: {target}, \
             result: {result} }}"
        ))
    }

    fn decorated_pot(
        &mut self,
        back: &ExtractedIngredient,
        left: &ExtractedIngredient,
        right: &ExtractedIngredient,
        front: &ExtractedIngredient,
        result: &ExtractedStack,
    ) -> Result<String, String> {
        let back = self.ingredient(back)?;
        let left = self.ingredient(left)?;
        let right = self.ingredient(right)?;
        let front = self.ingredient(front)?;
        let result = self.stack(result)?;
        Ok(format!(
            "RecipeKind::DecoratedPot {{ back: {back}, left: {left}, right: {right}, \
             front: {front}, result: {result} }}"
        ))
    }
}

fn recipes_file(data: &VersionData) -> Result<String, String> {
    let mut writer = RecipeWriter::new(data)?;
    let mut entries = Vec::with_capacity(data.recipes.entries.len());
    let mut seen = BTreeSet::new();
    for recipe in &data.recipes.entries {
        if !seen.insert(recipe.id.as_str()) {
            return Err(format!("配方 {} 出现了两次", recipe.id));
        }
        let kind = writer
            .kind(recipe)
            .map_err(|error| format!("{}：{error}", recipe.id))?;
        entries.push(format!(
            "    Recipe {{ id: {:?}, kind: {kind} }},",
            recipe.id
        ));
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "use crate::{{CookingKind, FireworkShape, Ingredient, PotionIngredient, Recipe, \
         RecipeKind, Stack}};\n"
    );
    let _ = writeln!(out, "/// 药水注册表的条目名，下标就是 ID。");
    let _ = writeln!(
        out,
        "pub static POTIONS: &[&str] = &{:?};\n",
        data.recipes.potions
    );
    let damageable = checked_item_list(&data.recipes.damageable, data, "可损坏物品")?;
    let _ = writeln!(
        out,
        "/// 默认物品栈带 `max_damage` 和 `damage` 的物品，升序。"
    );
    let _ = writeln!(out, "pub static DAMAGEABLE: &[i32] = &{:?};\n", damageable);
    let dyed = checked_item_list(&data.recipes.dyed, data, "带染料组件的物品")?;
    let _ = writeln!(out, "/// 默认物品栈带 `dye` 组件的物品，升序。");
    let _ = writeln!(out, "pub static DYED: &[i32] = &{:?};\n", dyed);
    out.push_str(&writer.item_lists.definitions());
    out.push_str(&writer.ingredients.definitions());
    out.push_str(&writer.patterns.definitions());
    out.push_str(&writer.ingredient_slices.definitions());
    out.push_str(&writer.shapes.definitions());
    let _ = writeln!(out, "/// 官方数据包的全部配方，顺序就是注册表顺序。");
    let _ = writeln!(out, "pub static RECIPES: &[Recipe] = &[");
    for entry in &entries {
        let _ = writeln!(out, "{entry}");
    }
    let _ = writeln!(out, "];");
    Ok(out)
}

fn registries_file(data: &VersionData) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "use crate::{{KnownPack, SyncedRegistry}};\n");

    let _ = writeln!(out, "/// 客户端需要认识的官方内容包。");
    let _ = writeln!(out, "pub const KNOWN_PACKS: &[KnownPack] = &[");
    for pack in &data.known_packs {
        let _ = writeln!(
            out,
            "    KnownPack {{ namespace: {:?}, id: {:?}, version: {:?} }},",
            pack.namespace, pack.id, pack.version
        );
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(out, "/// 启用的功能开关。");
    let _ = writeln!(
        out,
        "pub const ENABLED_FEATURES: &[&str] = &{:?};\n",
        data.enabled_features
    );

    let _ = writeln!(out, "/// 配置阶段要同步给客户端的注册表，顺序和官方一致。");
    let _ = writeln!(out, "pub const SYNCED_REGISTRIES: &[SyncedRegistry] = &[");
    for registry in &data.synced_registries {
        let _ = writeln!(
            out,
            "    SyncedRegistry {{ name: {:?}, entries: &{:?} }},",
            registry.name, registry.entries
        );
    }
    let _ = writeln!(out, "];\n");

    write_registry_ids(&mut out, data, "biome", BIOME_REGISTRY);
    write_registry_ids(&mut out, data, "chat_type", CHAT_TYPE_REGISTRY);
    write_registry_ids(&mut out, data, "dimension_type", DIMENSION_TYPE_REGISTRY);
    write_registry_ids(&mut out, data, "world_clock", WORLD_CLOCK_REGISTRY);
    write_registry_ids(&mut out, data, "attribute", ATTRIBUTE_REGISTRY);
    write_registry_ids(
        &mut out,
        data,
        "block_entity_type",
        BLOCK_ENTITY_TYPE_REGISTRY,
    );
    write_registry_ids(&mut out, data, "entity_type", ENTITY_TYPE_REGISTRY);
    write_registry_ids(
        &mut out,
        data,
        "data_component_type",
        DATA_COMPONENT_TYPE_REGISTRY,
    );
    write_registry_ids(&mut out, data, "fluid", FLUID_REGISTRY);
    write_registry_ids(&mut out, data, "menu", MENU_REGISTRY);

    let _ = writeln!(out, "/// 属性的默认值，没有任何状态效果和附魔时的起点。");
    let _ = writeln!(out, "pub mod attribute_defaults {{");
    for (name, value) in &data.attribute_defaults {
        let _ = writeln!(
            out,
            "    pub const {}: f64 = {value:?};",
            constant_name(name)
        );
    }
    let _ = writeln!(out, "}}\n");
    out
}

/// 一个注册表按 ID 排列的条目，先找同步注册表，再找内置注册表。
fn registry_entries<'a>(data: &'a VersionData, registry: &str) -> Option<&'a [String]> {
    data.synced_registries
        .iter()
        .find(|candidate| candidate.name == registry)
        .map(|found| found.entries.as_slice())
        .or_else(|| data.builtin_registries.get(registry).map(Vec::as_slice))
}

/// 把一个注册表的条目写成常量，数字 ID 就是条目在列表里的位置。
fn write_registry_ids(out: &mut String, data: &VersionData, module: &str, registry: &str) {
    let entries = registry_entries(data, registry).unwrap_or(&[]);

    let _ = writeln!(out, "pub mod {module} {{");
    let _ = writeln!(out, "    pub const TOTAL: u32 = {};", entries.len());
    for (id, name) in entries.iter().enumerate() {
        let _ = writeln!(out, "    pub const {}: i32 = {id};", constant_name(name));
    }
    let _ = writeln!(out, "}}\n");
}

/// 一个注册表下的标签：名字 → 它在 `TagGroup` 里的下标。
type TagIndexes<'a> = BTreeMap<&'a str, usize>;

fn tag_group_index(groups: &[TagGroup], registry: &str) -> Result<usize, String> {
    groups
        .iter()
        .position(|group| group.registry == registry)
        .ok_or_else(|| format!("标签里没有 {registry}"))
}

fn tag_indexes<'a>(groups: &'a [TagGroup], registry: &str) -> Result<TagIndexes<'a>, String> {
    let group = &groups[tag_group_index(groups, registry)?];
    Ok(group
        .tags
        .iter()
        .enumerate()
        .map(|(index, tag)| (tag.name.as_str(), index))
        .collect())
}

/// 给一个注册表的标签写常量，查询时靠下标，不用比较名字。
fn write_tag_constants(
    out: &mut String,
    groups: &[TagGroup],
    module: &str,
    type_name: &str,
    registry: &str,
) -> Result<(), String> {
    let group_index = tag_group_index(groups, registry)?;
    let _ = writeln!(out, "pub mod {module} {{");
    let _ = writeln!(out, "    use crate::{type_name};\n");
    let _ = writeln!(out, "    /// 这些标签在 [`super::TAG_GROUPS`] 里的下标。");
    let _ = writeln!(out, "    pub const GROUP: usize = {group_index};\n");
    for (index, tag) in groups[group_index].tags.iter().enumerate() {
        let _ = writeln!(
            out,
            "    pub const {}: {type_name} = {type_name} {{ name: {:?}, index: {index} }};",
            constant_name(&tag.name),
            tag.name
        );
    }
    let _ = writeln!(out, "}}\n");
    Ok(())
}

fn tags_file(data: &VersionData) -> Result<String, String> {
    let mut out = String::new();
    let _ = writeln!(out, "use crate::{{Tag, TagGroup}};\n");
    write_tag_constants(
        &mut out,
        &data.tags,
        "block_tags",
        "BlockTag",
        BLOCK_REGISTRY,
    )?;
    write_tag_constants(&mut out, &data.tags, "item_tags", "ItemTag", ITEM_REGISTRY)?;
    write_tag_constants(
        &mut out,
        &data.tags,
        "fluid_tags",
        "FluidTag",
        FLUID_REGISTRY,
    )?;

    let _ = writeln!(out, "/// 配置阶段要发给客户端的标签。");
    let _ = writeln!(out, "pub const TAG_GROUPS: &[TagGroup] = &[");
    for group in &data.tags {
        let _ = writeln!(
            out,
            "    TagGroup {{ registry: {:?}, tags: &[",
            group.registry
        );
        for tag in &group.tags {
            let _ = writeln!(
                out,
                "        Tag {{ name: {:?}, ids: &{:?} }},",
                tag.name, tag.ids
            );
        }
        let _ = writeln!(out, "    ] }},");
    }
    let _ = writeln!(out, "];");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, states: &[u32]) -> Block {
        Block {
            name: name.to_owned(),
            states: states.to_vec(),
            default_state: states.first().copied().unwrap_or_default(),
            item: 0,
            has_collision: false,
            has_fluid: false,
            two_blocks: false,
            push_reaction: "push_pull".to_owned(),
            unbreakable: false,
            block_entity_type: None,
            class: "Block".to_owned(),
            ancestors: Vec::new(),
            properties: Vec::new(),
            fields: Vec::new(),
            instrument: "harp".to_owned(),
            dynamic_shape: false,
            offset: None,
            destroy_time: 0.0,
            requires_correct_tool_for_drops: false,
        }
    }

    fn property(values: &[&str]) -> Property {
        Property {
            name: "test".to_owned(),
            values: values.iter().map(|&value| value.to_owned()).collect(),
        }
    }

    fn item(class: &str, block: Option<i32>) -> Item {
        Item {
            name: "minecraft:test".to_owned(),
            class: class.to_owned(),
            block,
            wall_block: None,
            attachment: None,
            max_stack_size: 64,
            can_destroy_blocks_in_creative: true,
            consumable: false,
            compostable: None,
            jukebox_playable: false,
            block_transformer: None,
            blocks_attacks: false,
        }
    }

    #[test]
    fn accepts_contiguous_state_ids() {
        let blocks = [block("a", &[0]), block("b", &[1, 2, 3]), block("c", &[4])];
        assert_eq!(check_state_ids(&blocks, 5), Ok(()));
    }

    #[test]
    fn rejects_state_ids_that_break_the_binary_search() {
        let gap = [block("a", &[0]), block("b", &[2])];
        assert!(check_state_ids(&gap, 2).unwrap_err().contains('b'));

        let out_of_order = [block("a", &[0, 2, 1])];
        assert!(check_state_ids(&out_of_order, 3).is_err());

        let not_from_zero = [block("a", &[1])];
        assert!(check_state_ids(&not_from_zero, 1).is_err());

        let no_states = [block("a", &[0]), block("b", &[]), block("c", &[1])];
        assert!(check_state_ids(&no_states, 2).is_err());
    }

    #[test]
    fn rejects_a_wrong_state_total() {
        assert!(check_state_ids(&[block("a", &[0, 1])], 3).is_err());
    }

    #[test]
    fn state_count_has_to_be_the_product_of_the_properties() {
        let mut door = block("door", &[0, 1, 2, 3]);
        door.properties = vec![property(&["a", "b"]), property(&["c", "d"])];
        assert_eq!(check_properties(&[door]), Ok(()));

        let mut broken = block("broken", &[0, 1, 2]);
        broken.properties = vec![property(&["a", "b"])];
        assert!(check_properties(&[broken]).unwrap_err().contains("broken"));
    }

    #[test]
    fn shared_lists_are_written_once() {
        let mut lists = SharedLists::new("LIST", "&[u8]");
        assert_eq!(lists.name_of("&[1]".to_owned()), "LIST_0");
        assert_eq!(lists.name_of("&[2]".to_owned()), "LIST_1");
        assert_eq!(lists.name_of("&[1]".to_owned()), "LIST_0");
        assert_eq!(
            lists.definitions(),
            "const LIST_0: &[u8] = &[1];\nconst LIST_1: &[u8] = &[2];\n"
        );
    }

    #[test]
    fn field_values_refuse_unknown_tags() {
        let known = TagIndexes::from([("minecraft:logs", 3)]);
        let tag = FieldValue::Tag("minecraft:logs".to_owned());
        assert_eq!(
            field_value(&tag, &known),
            Ok("FieldValue::Tag(block_tags::LOGS)".to_owned())
        );
        let missing = FieldValue::Tag("minecraft:no_such_tag".to_owned());
        assert!(field_value(&missing, &known).is_err());
    }

    #[test]
    fn maps_item_classes_to_kinds() {
        assert_eq!(item_kind(&item("BlockItem", Some(1))), Ok("Block"));
        assert_eq!(item_kind(&item("BannerItem", Some(1))), Ok("Banner"));
        assert_eq!(item_kind(&item("SwordItem", None)), Ok("Other"));
    }

    #[test]
    fn refuses_a_block_item_class_it_has_not_seen() {
        let error = item_kind(&item("NewBlockItem", Some(1))).unwrap_err();
        assert!(error.contains("NewBlockItem"));
    }

    #[test]
    fn non_block_items_with_their_own_use_on_get_a_kind() {
        assert_eq!(
            item_kind(&item("FlintAndSteelItem", None)),
            Ok("FlintAndSteel")
        );
        assert_eq!(item_kind(&item("Item", None)), Ok("Other"));
    }

    fn heightmap(name: &str, id: i32) -> HeightmapType {
        HeightmapType {
            name: name.to_owned(),
            id,
            send_to_client: false,
            keep_after_worldgen: true,
        }
    }

    #[test]
    fn heightmap_ids_are_the_declaration_order() {
        assert_eq!(
            check_heightmap_types(&[heightmap("A", 0), heightmap("B", 1)]),
            Ok(())
        );
        assert!(check_heightmap_types(&[]).is_err());
        assert!(check_heightmap_types(&[heightmap("A", 1)]).is_err());
        assert!(check_heightmap_types(&[heightmap("A", 0), heightmap("A", 1)]).is_err());
    }
}
