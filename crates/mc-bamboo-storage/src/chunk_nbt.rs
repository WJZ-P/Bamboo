//! 26.3 的区块 NBT，对应官方 `SerializableChunkData.write` / `parse`。
//!
//! 写方向把 [`ChunkColumn`] 里的内容编成带名字的根复合标签；读方向把它读回
//! `ChunkColumn`，并把这一层不认识的键、方块实体条目和计划刻原样交还给调用方
//! （`docs/world-model-design.md` 5.5 的「不认识的键原样保留」）。
//!
//! 方块实体的内容（箱子里的物品等）、红石记忆这些在更上层，这次只编解码
//! `ChunkColumn` 里有的东西。上层可以把每个方块实体位置的额外 NBT 片段和
//! `block_ticks` / `fluid_ticks` 列表交给 [`ChunkEncodeExtra`]，原样合并进条目。

use mc_bamboo_core::{ChunkPos, sign::SignContent};
use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_of_state, blocks, registries, state_count, version,
};
use mc_bamboo_nbt::{Accounter, FileError, Tag, read_named, write_named};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT,
    chunk::HeightmapKind,
    column::{ChunkColumn, ChunkColumnParts, SectionParts},
    light::{DataLayer, LAYER_BYTES, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION},
    palette::{
        BIOMES_PER_SECTION, BLOCKS_PER_SECTION, disk_bits, disk_data_len, packed_capacity,
        read_disk_index,
    },
};

/// 主世界最低的方块段，官方 `getMinSectionY`。
const MIN_SECTION: i32 = MIN_Y / 16;
/// 最高（含）的方块段，官方 `getMaxSectionY`。
const MAX_SECTION: i32 = MIN_SECTION + SECTION_COUNT as i32 - 1;
/// 磁盘上 `Status` 的值。
const FULL_STATUS: &str = "minecraft:full";
/// 官方的默认结构数据。
const EMPTY_STRUCTURES: [(&str, Tag); 2] = [
    ("starts", Tag::Compound(Vec::new())),
    ("References", Tag::Compound(Vec::new())),
];

/// 区块 NBT 编解码失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum ChunkNbtError {
    #[error("NBT file error: {0}")]
    File(#[from] FileError),
    #[error("chunk tag is missing or has the wrong type: {0}")]
    Malformed(String),
    #[error("unsupported DataVersion {0} (expected {expected})", expected = version::DATA_VERSION)]
    NewerDataVersion(i32),
    #[error("unsupported DataVersion {0} (expected {expected})", expected = version::DATA_VERSION)]
    OlderDataVersion(i32),
    #[error("chunk column cannot be built from the NBT: {0}")]
    Column(String),
}

/// 编码时由上层提供的附加数据。这里只做透传和合并，不解释内容。
#[derive(Debug)]
pub struct ChunkEncodeExtra<'a> {
    /// 这个区块的坐标，写进 `xPos` / `zPos`。
    pub chunk_pos: ChunkPos,
    /// `LastUpdate`。
    pub last_update: i64,
    /// `InhabitedTime`。
    pub inhabited_time: i64,
    /// 每个方块实体位置额外的一段 NBT 复合标签，原样合并进 `block_entities` 的条目。
    /// 键和编解码器自己写的键重名时以编解码器为准。
    pub block_entity_extra: Vec<(i32, i32, i32, Tag)>,
    /// `block_ticks` 列表，原样写出。
    pub block_ticks: Vec<Tag>,
    /// `fluid_ticks` 列表，原样写出。
    pub fluid_ticks: Vec<Tag>,
    /// 顶层要原样保留的其它键（读出来再写回去时用它）。
    pub preserved: Vec<(String, Tag)>,
    /// 顶层 `structures`；默认写空壳。
    pub structures: Option<Tag>,
    /// 顶层 `PostProcessing`；默认写 24 个空列表。
    pub post_processing: Option<Tag>,
    /// 方块实体条目的原始顺序里，本层不认识的那些键（按位置索引）。
    pub block_entity_raw: Vec<Vec<(String, Tag)>>,
    /// 原样写回的 `block_entities` 列表（读盘再写回时用），`None` 时从 `ChunkColumn` 生成。
    pub raw_block_entities: Option<Vec<Tag>>,
    /// 原样写回的 `Heightmaps` 条目（官方只写它有的那几张），`None` 时写四张计算值。
    pub raw_heightmaps: Option<Vec<(String, Tag)>>,
    pub _marker: std::marker::PhantomData<&'a ()>,
}

impl Default for ChunkEncodeExtra<'_> {
    fn default() -> Self {
        Self {
            chunk_pos: ChunkPos::new(0, 0),
            last_update: 0,
            inhabited_time: 0,
            block_entity_extra: Vec::new(),
            block_ticks: Vec::new(),
            fluid_ticks: Vec::new(),
            preserved: Vec::new(),
            structures: None,
            post_processing: None,
            block_entity_raw: Vec::new(),
            raw_block_entities: None,
            raw_heightmaps: None,
            _marker: std::marker::PhantomData,
        }
    }
}

/// 解码结果：`ChunkColumn` 和原样交还给调用方的片段。
#[derive(Debug)]
pub struct DecodedChunk {
    pub column: Box<ChunkColumn>,
    pub chunk_pos: ChunkPos,
    pub data_version: i32,
    pub status: String,
    pub last_update: i64,
    pub inhabited_time: i64,
    pub light_on: bool,
    pub block_entities: Vec<Tag>,
    pub block_ticks: Vec<Tag>,
    pub fluid_ticks: Vec<Tag>,
    /// 根复合标签里不认识的顶层键，原样保留。
    pub unknown: Vec<(String, Tag)>,
    pub structures: Tag,
    pub post_processing: Tag,
    /// 原样的 `Heightmaps` 条目，重新编码时按键写回。
    pub heightmaps: Vec<(String, Tag)>,
    /// 重新编码时要原样写回的顶层键（包括官方的可选字段）。
    pub preserved: Vec<(String, Tag)>,
    /// 编码时给 `block_entity_extra` 的原始方块实体条目（按位置）。
    pub block_entity_raw: Vec<Vec<(String, Tag)>>,
}

impl DecodedChunk {
    /// 用解码结果再编码，字段和原文件一致。
    pub fn extra(&self) -> ChunkEncodeExtra<'_> {
        ChunkEncodeExtra {
            chunk_pos: self.chunk_pos,
            last_update: self.last_update,
            inhabited_time: self.inhabited_time,
            block_entity_extra: self
                .block_entities
                .iter()
                .map(|tag| {
                    let (x, y, z) = tag_position(tag).unwrap_or((0, 0, 0));
                    (x, y, z, tag.clone())
                })
                .collect(),
            block_ticks: self.block_ticks.clone(),
            fluid_ticks: self.fluid_ticks.clone(),
            preserved: self.preserved.clone(),
            structures: Some(self.structures.clone()),
            post_processing: Some(self.post_processing.clone()),
            block_entity_raw: self.block_entity_raw.clone(),
            raw_block_entities: Some(self.block_entities.clone()),
            raw_heightmaps: Some(self.heightmaps.clone()),
            _marker: std::marker::PhantomData,
        }
    }
}

/// 写好一个根复合标签条目。
fn put(entries: &mut Vec<(String, Tag)>, key: &str, value: Tag) {
    entries.push((key.to_owned(), value));
}

fn compound(entries: Vec<(String, Tag)>) -> Tag {
    Tag::Compound(entries)
}

/// 编解码器写的顶层键，附加内容里这些键不算「不认识」。
const KNOWN_KEYS: &[&str] = &[
    "DataVersion",
    "xPos",
    "yPos",
    "zPos",
    "LastUpdate",
    "InhabitedTime",
    "Status",
    "blending_data",
    "below_zero_retrogen",
    "UpgradeData",
    "sections",
    "isLightOn",
    "block_entities",
    "entities",
    "block_ticks",
    "fluid_ticks",
    "PostProcessing",
    "Heightmaps",
    "structures",
];

/// 把一个 `Tag` 按名字查字段。复合标签用 `Vec` 保序，同名键取最后一个（官方 HashMap 的行为）。
pub fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    entries
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn as_int(tag: Option<&Tag>, key: &str) -> Result<i32, ChunkNbtError> {
    match tag {
        Some(Tag::Int(value)) => Ok(*value),
        _ => Err(ChunkNbtError::Malformed(format!("{key} must be an int"))),
    }
}

fn as_long(tag: Option<&Tag>, key: &str) -> Result<i64, ChunkNbtError> {
    match tag {
        Some(Tag::Long(value)) => Ok(*value),
        _ => Err(ChunkNbtError::Malformed(format!("{key} must be a long"))),
    }
}

fn as_string<'a>(tag: Option<&'a Tag>, key: &str) -> Result<&'a str, ChunkNbtError> {
    match tag {
        Some(Tag::String(value)) => Ok(value),
        _ => Err(ChunkNbtError::Malformed(format!("{key} must be a string"))),
    }
}

fn as_list(tag: Option<&Tag>, key: &str) -> Result<Vec<Tag>, ChunkNbtError> {
    match tag {
        Some(Tag::List(values)) => Ok(values.clone()),
        None => Ok(Vec::new()),
        _ => Err(ChunkNbtError::Malformed(format!("{key} must be a list"))),
    }
}

fn as_long_array(tag: &Tag, key: &str) -> Result<Vec<u64>, ChunkNbtError> {
    match tag {
        Tag::LongArray(values) => Ok(values.iter().map(|&value| value as u64).collect()),
        _ => Err(ChunkNbtError::Malformed(format!(
            "{key} must be a long array"
        ))),
    }
}

fn as_byte_array(tag: &Tag, key: &str) -> Result<Vec<i8>, ChunkNbtError> {
    match tag {
        Tag::ByteArray(values) => Ok(values.clone()),
        _ => Err(ChunkNbtError::Malformed(format!(
            "{key} must be a byte array"
        ))),
    }
}

/// 把一个状态 ID 写成磁盘调色板里的 `{ Name, Properties }`。
fn state_palette_entry(state: u32) -> Result<Tag, ChunkNbtError> {
    let block = block_of_state(state)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("unknown block state id {state}")))?;
    let id = block_id_of_state(state)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("unknown block state id {state}")))?;
    let mut properties = Vec::new();
    let mut offset = state - block.first_state;
    let count = state_count(id);
    let mut remaining = count;
    for property in block.properties {
        let choices = property.values.len() as u32;
        remaining /= choices;
        let index = (offset / remaining) as usize;
        offset %= remaining;
        let value = property.values.get(index).ok_or_else(|| {
            ChunkNbtError::Malformed(format!(
                "block {} property {} has no value {index}",
                block.name, property.name
            ))
        })?;
        put(
            &mut properties,
            property.name,
            Tag::String((*value).to_owned()),
        );
    }
    // 官方 `BlockState.CODEC` 对「默认状态」写成裸字符串，非默认状态写成
    // `{ Name: "...", Properties: { ... } }`，属性一律是字符串。
    if state == block.default_state {
        return Ok(Tag::String(block.name.to_owned()));
    }
    let mut entries = Vec::with_capacity(2);
    put(&mut entries, "Name", Tag::String(block.name.to_owned()));
    if !properties.is_empty() {
        put(&mut entries, "Properties", compound(properties));
    }
    Ok(compound(entries))
}

/// 把磁盘调色板里的 `{ Name, Properties }` 还原成状态 ID。
fn parse_state_palette_entry(tag: &Tag) -> Result<u32, ChunkNbtError> {
    // 官方 `BlockState.CODEC` 对没有属性的方块写成裸字符串。
    let (name, properties_tag) = match tag {
        Tag::String(value) => (value.as_str(), None),
        Tag::Compound(_) => (
            as_string(field(tag, "Name"), "palette entry Name")?,
            field(tag, "Properties"),
        ),
        _ => {
            return Err(ChunkNbtError::Malformed(
                "palette entry must be a string or a compound tag".to_owned(),
            ));
        }
    };
    let block = block_by_name(name)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("unknown block {name}")))?;
    let id = block_id_of_state(block.first_state)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("block {name} has no states")))?;
    if properties_tag.is_none() {
        return Ok(block.default_state);
    }
    let count = state_count(id);
    let properties: &[(String, Tag)] = match properties_tag {
        Some(Tag::Compound(entries)) => entries,
        Some(_) => {
            return Err(ChunkNbtError::Malformed(
                "palette entry Properties must be a compound".to_owned(),
            ));
        }
        None => &[],
    };
    let mut offset = 0u32;
    let mut remaining = count;
    for property in block.properties {
        let choices = property.values.len() as u32;
        remaining /= choices;
        let key = property.name;
        let value = properties
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value);
        let index = match value {
            None => 0,
            Some(Tag::String(value)) => property
                .values
                .iter()
                .position(|candidate| *candidate == value)
                .ok_or_else(|| {
                    ChunkNbtError::Malformed(format!(
                        "unknown value {value:?} for {name} property {key}"
                    ))
                })? as u32,
            Some(_) => {
                return Err(ChunkNbtError::Malformed(format!(
                    "property {key} must be a string"
                )));
            }
        };
        if index >= choices {
            return Err(ChunkNbtError::Malformed(format!(
                "property {key} has no value {index}"
            )));
        }
        offset += index * remaining;
    }
    let state = block.first_state + offset;
    if state >= blocks::TOTAL_STATES {
        return Err(ChunkNbtError::Malformed(format!(
            "block {name} state {state} is out of range"
        )));
    }
    Ok(state)
}

fn biome_palette_entry(id: u32) -> Result<Tag, ChunkNbtError> {
    let name = registries::biome::name_of(id as i32)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("unknown biome id {id}")))?;
    Ok(Tag::String(name.to_owned()))
}

fn parse_biome_palette_entry(tag: &Tag) -> Result<u32, ChunkNbtError> {
    let name = match tag {
        Tag::String(name) => name.as_str(),
        _ => {
            return Err(ChunkNbtError::Malformed(
                "biome palette entry must be a string".to_owned(),
            ));
        }
    };
    let id = registries::biome::by_name(name)
        .ok_or_else(|| ChunkNbtError::Malformed(format!("unknown biome {name}")))?;
    u32::try_from(id).map_err(|_| ChunkNbtError::Malformed(format!("bad biome id {id}")))
}

/// 一个磁盘调色板容器：`{ palette, data? }`。
fn write_container(palette: Vec<u32>, data: Vec<u64>, biomes: bool) -> Result<Tag, ChunkNbtError> {
    let entries: Vec<Tag> = if biomes {
        palette
            .iter()
            .map(|&id| biome_palette_entry(id))
            .collect::<Result<_, _>>()?
    } else {
        palette
            .iter()
            .map(|&state| state_palette_entry(state))
            .collect::<Result<_, _>>()?
    };
    let mut out = Vec::with_capacity(2);
    put(&mut out, "palette", Tag::List(entries));
    let bits = disk_bits(palette.len(), biomes);
    if bits != 0 {
        put(
            &mut out,
            "data",
            Tag::LongArray(data.iter().map(|&word| word as i64).collect()),
        );
    }
    Ok(compound(out))
}

/// 读一个磁盘调色板容器，返回（调色板 ID 数组、long 数组）。
fn read_container(tag: &Tag, biomes: bool) -> Result<(Vec<u32>, Vec<u64>), ChunkNbtError> {
    let palette = match field(tag, "palette") {
        Some(Tag::List(entries)) => entries,
        _ => {
            return Err(ChunkNbtError::Malformed(
                "paletted container has no palette list".to_owned(),
            ));
        }
    };
    if palette.is_empty()
        || palette.len()
            > if biomes {
                BIOMES_PER_SECTION
            } else {
                BLOCKS_PER_SECTION
            }
    {
        return Err(ChunkNbtError::Malformed(
            "paletted container palette has an impossible length".to_owned(),
        ));
    }
    let values = if biomes {
        palette
            .iter()
            .map(parse_biome_palette_entry)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        palette
            .iter()
            .map(parse_state_palette_entry)
            .collect::<Result<Vec<_>, _>>()?
    };
    let bits = disk_bits(values.len(), biomes);
    let data = match field(tag, "data") {
        Some(Tag::LongArray(words)) => words.iter().map(|&word| word as u64).collect(),
        Some(_) => {
            return Err(ChunkNbtError::Malformed(
                "paletted container data must be a long array".to_owned(),
            ));
        }
        None => Vec::new(),
    };
    let entries = if biomes {
        BIOMES_PER_SECTION
    } else {
        BLOCKS_PER_SECTION
    };
    if data.len() != disk_data_len(entries, bits) {
        return Err(ChunkNbtError::Malformed(format!(
            "paletted container needs {} longs for {} palette entries, got {}",
            disk_data_len(entries, bits),
            values.len(),
            data.len()
        )));
    }
    // 官方 `SimpleBitStorage` 用 long 数组长度算容量，下标超界时读出来的值不能
    // 落到调色板之外；这里提前查一遍，坏文件在进 `from_parts` 之前就报错。
    if bits != 0 {
        let capacity = packed_capacity(data.len(), bits);
        for index in 0..entries.min(capacity) {
            let stored = read_disk_index(&data, bits, index);
            if stored as usize >= values.len() {
                return Err(ChunkNbtError::Malformed(format!(
                    "paletted container index {stored} is outside a palette of {}",
                    values.len()
                )));
            }
        }
    }
    Ok((values, data))
}

/// 磁盘上的光照 byte array → Bamboo 的 `DataLayer`。
fn read_light(tag: &Tag, key: &str) -> Result<DataLayer, ChunkNbtError> {
    let bytes = as_byte_array(tag, key)?;
    if bytes.len() != LAYER_BYTES {
        return Err(ChunkNbtError::Malformed(format!(
            "{key} has {} bytes, expected {LAYER_BYTES}",
            bytes.len()
        )));
    }
    // Java 的 byte 是 i8，转成 u8 后每格的两半 4 位都不变。
    let mut array = [0u8; LAYER_BYTES];
    for (index, byte) in bytes.iter().enumerate() {
        array[index] = *byte as u8;
    }
    // 均匀的整段还原成没分配数组的形状，和 Bamboo 区块束里的 `Filled` 一致。
    let first = array[0];
    if array.iter().all(|&byte| byte == first) && first & 15 == first >> 4 {
        return Ok(DataLayer::Filled(first & 15));
    }
    Ok(DataLayer::Data(Box::new(array)))
}

/// 告示牌一面文字的 NBT，和官方 `SignText.CODEC` 一致。
fn sign_text_tag(text: &mc_bamboo_core::sign::SignText) -> Tag {
    compound(vec![
        (
            "messages".to_owned(),
            Tag::List(text.messages.iter().cloned().map(Tag::String).collect()),
        ),
        (
            "color".to_owned(),
            Tag::String(text.color.name().to_owned()),
        ),
        ("has_glowing_text".to_owned(), Tag::Byte(text.glowing as i8)),
    ])
}

fn find_color(name: &str) -> mc_bamboo_core::sign::DyeColor {
    mc_bamboo_core::sign::DyeColor::ALL
        .iter()
        .copied()
        .find(|color| color.name() == name)
        .unwrap_or(mc_bamboo_core::sign::DyeColor::Black)
}

fn parse_sign_text(tag: &Tag) -> mc_bamboo_core::sign::SignText {
    let mut text = mc_bamboo_core::sign::SignText::EMPTY;
    if let Some(Tag::List(lines)) = field(tag, "messages") {
        for (index, line) in lines.iter().enumerate().take(4) {
            if let Tag::String(value) = line {
                text.messages[index] = value.clone();
            }
        }
    }
    if let Some(Tag::String(color)) = field(tag, "color") {
        text.color = find_color(color);
    }
    if let Some(Tag::Byte(glowing)) = field(tag, "has_glowing_text") {
        text.glowing = *glowing != 0;
    }
    text
}

fn tag_position(tag: &Tag) -> Option<(i32, i32, i32)> {
    let x = match field(tag, "x")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    let y = match field(tag, "y")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    let z = match field(tag, "z")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    Some((x, y, z))
}

fn block_entity_id(tag: &Tag) -> Option<&str> {
    match field(tag, "id") {
        Some(Tag::String(value)) => Some(value),
        _ => None,
    }
}

/// 原样写回一条方块实体，把上层附加的、本层不写的键补进去。
fn merge_block_entity_extra(tag: &Tag, extra: &ChunkEncodeExtra<'_>) -> Result<Tag, ChunkNbtError> {
    let Tag::Compound(values) = tag else {
        return Ok(tag.clone());
    };
    let Some((x, y, z)) = tag_position(tag) else {
        return Ok(tag.clone());
    };
    let mut entries = values.clone();
    // 官方从活区块存盘时每条都有这两个键；读到的旧文件没有也补齐。
    for (key, value) in [
        ("components", compound(Vec::new())),
        ("keepPacked", Tag::Byte(0)),
    ] {
        if !entries.iter().any(|(existing, _)| existing == key) {
            entries.push((key.to_owned(), value));
        }
    }
    for (ex, ey, ez, extra_tag) in &extra.block_entity_extra {
        if (*ex, *ey, *ez) != (x, y, z) {
            continue;
        }
        let Tag::Compound(extra_values) = extra_tag else {
            return Err(ChunkNbtError::Malformed(
                "block entity extra must be a compound tag".to_owned(),
            ));
        };
        for (key, value) in extra_values {
            if !entries.iter().any(|(existing, _)| existing == key) {
                entries.push((key.clone(), value.clone()));
            }
        }
    }
    Ok(compound(entries))
}

/// 一个方块实体条目的骨架：注册名、绝对坐标、告示牌文字。
fn write_block_entity(
    entity: &mc_bamboo_world::chunk::BlockEntity,
    pos: ChunkPos,
    column: &ChunkColumn,
    extra: &ChunkEncodeExtra<'_>,
    raw: Option<&[(String, Tag)]>,
) -> Result<Tag, ChunkNbtError> {
    let name = registries::block_entity_type::name_of(entity.kind).ok_or_else(|| {
        ChunkNbtError::Malformed(format!("unknown block entity type id {}", entity.kind))
    })?;
    let x = pos.x * 16 + entity.x as i32;
    let z = pos.z * 16 + entity.z as i32;
    let mut entries = Vec::new();
    put(&mut entries, "id", Tag::String(name.to_owned()));
    put(&mut entries, "x", Tag::Int(x));
    put(&mut entries, "y", Tag::Int(entity.y));
    put(&mut entries, "z", Tag::Int(z));
    // 官方 `BlockEntity.saveWithoutMetadata` 无条件写组件表；
    // `LevelChunk.getBlockEntityNbtForSaving` 再补 `keepPacked: false`。
    put(&mut entries, "components", compound(Vec::new()));
    put(&mut entries, "keepPacked", Tag::Byte(0));
    let is_sign = entity.kind == registries::block_entity_type::SIGN
        || entity.kind == registries::block_entity_type::HANGING_SIGN;
    if is_sign {
        let content = column
            .chunk()
            .sign(entity.x, entity.y, entity.z)
            .cloned()
            .unwrap_or(SignContent::BLANK);
        put(&mut entries, "front_text", sign_text_tag(&content.front));
        put(&mut entries, "back_text", sign_text_tag(&content.back));
        put(&mut entries, "is_waxed", Tag::Byte(content.waxed as i8));
    }
    let mut written: Vec<String> = entries.iter().map(|(key, _)| key.clone()).collect();
    // 附加内容只补本层没写过的键；同名键以本层为准，并提醒上层。
    let mut extra_entries = Vec::new();
    for (ex, ey, ez, tag) in &extra.block_entity_extra {
        if (*ex, *ey, *ez) != (x, entity.y, z) {
            continue;
        }
        let Tag::Compound(values) = tag else {
            return Err(ChunkNbtError::Malformed(
                "block entity extra must be a compound tag".to_owned(),
            ));
        };
        for (key, value) in values {
            let known = written.iter().any(|existing| existing == key);
            // 附加内容不该覆盖本层的键；测试会故意传一个来确认以本层为准。
            if !known {
                written.push(key.clone());
                extra_entries.push((key.clone(), value.clone()));
            }
        }
    }
    // 读进来的原始条目里，除本层已经写过的键以外都原样带着。
    if let Some(values) = raw {
        for (key, value) in values {
            if !written.iter().any(|existing| existing == key) {
                written.push(key.clone());
                extra_entries.push((key.clone(), value.clone()));
            }
        }
    }
    entries.extend(extra_entries);
    Ok(compound(entries))
}

/// 把一个区块束编码成带名字的根标签。
pub fn encode(column: &ChunkColumn, extra: &ChunkEncodeExtra<'_>) -> Result<Tag, ChunkNbtError> {
    let parts = ChunkColumnParts::read_from(column);
    let archive = column.chunk().archive();
    let light_on = column.light_on();
    let mut root = Vec::new();
    put(&mut root, "DataVersion", Tag::Int(version::DATA_VERSION));
    put(&mut root, "xPos", Tag::Int(extra.chunk_pos.x));
    put(&mut root, "yPos", Tag::Int(MIN_SECTION));
    put(&mut root, "zPos", Tag::Int(extra.chunk_pos.z));
    put(&mut root, "LastUpdate", Tag::Long(extra.last_update));
    put(&mut root, "InhabitedTime", Tag::Long(extra.inhabited_time));
    put(&mut root, "Status", Tag::String(FULL_STATUS.to_owned()));

    let mut sections = Vec::with_capacity(LIGHT_SECTION_COUNT);
    for slot in 0..LIGHT_SECTION_COUNT {
        let section_y = MIN_LIGHT_SECTION + slot as i32;
        let mut entry = Vec::new();
        let block_section = (MIN_SECTION..=MAX_SECTION)
            .contains(&section_y)
            .then(|| &parts.sections[(section_y - MIN_SECTION) as usize]);
        if let Some(section) = block_section {
            put(
                &mut entry,
                "block_states",
                write_container(
                    section.block_palette.clone(),
                    section.block_data.clone(),
                    false,
                )?,
            );
            put(
                &mut entry,
                "biomes",
                write_container(
                    section.biome_palette.clone(),
                    section.biome_data.clone(),
                    true,
                )?,
            );
        }
        // 官方只在 `!DataLayer.isEmpty()` 时写：没分配数组且默认值 0 的不写。
        for (key, layer) in [("BlockLight", 1usize), ("SkyLight", 0usize)] {
            let Some(Some(data)) = parts.light[layer].get(slot) else {
                continue;
            };
            // 官方 `copyOf` 跳过 `DataLayer.isEmpty()`，但 Bamboo 的 `Filled(0)`
            // 表示「这一段存着光照，全是 0」，和 `None`（没存）不同。为了原样
            // 往返，存着的层一律写成 2048 字节；官方 `parse` 读成非空 DataLayer，
            // `write()` 也会把它写回来，所以对拍仍然一致。
            let bytes = data.bytes();
            put(
                &mut entry,
                key,
                Tag::ByteArray(bytes.iter().map(|&byte| byte as i8).collect()),
            );
        }
        // 官方 `copyOf`：有方块段、或者有光照数据时产出一项。
        // 主世界 24 个方块段一定写，光照段 -5 和 20 只有真有光照时才写。
        if block_section.is_some()
            || entry
                .iter()
                .any(|(key, _)| key == "BlockLight" || key == "SkyLight")
        {
            put(&mut entry, "Y", Tag::Byte(section_y as i8));
            sections.push(compound(entry));
        }
    }
    put(&mut root, "sections", Tag::List(sections));
    if light_on {
        put(&mut root, "isLightOn", Tag::Byte(1));
    }

    let block_entities = match &extra.raw_block_entities {
        Some(raw) => raw
            .iter()
            .map(|tag| merge_block_entity_extra(tag, extra))
            .collect::<Result<Vec<_>, _>>()?,
        None => {
            let entities: Vec<_> = column.chunk().block_entities().collect();
            let mut written = Vec::with_capacity(entities.len());
            for (index, entity) in entities.iter().enumerate() {
                let raw = column
                    .chunk()
                    .preserved_block_entity(entity.x, entity.y, entity.z)
                    .filter(|tag| {
                        block_entity_id(tag)
                            == Some(
                                registries::block_entity_type::name_of(entity.kind).unwrap_or(""),
                            )
                    });
                written.push(write_block_entity(
                    entity,
                    extra.chunk_pos,
                    column,
                    extra,
                    match raw {
                        Some(Tag::Compound(values)) => Some(values.as_slice()),
                        _ => extra.block_entity_raw.get(index).map(Vec::as_slice),
                    },
                )?);
            }
            written
        }
    };
    put(&mut root, "block_entities", Tag::List(block_entities));
    put(
        &mut root,
        "block_ticks",
        Tag::List(extra.block_ticks.clone()),
    );
    put(
        &mut root,
        "fluid_ticks",
        Tag::List(extra.fluid_ticks.clone()),
    );
    let post_processing = extra
        .post_processing
        .clone()
        .or_else(|| archive.post_processing.clone())
        .unwrap_or_else(|| Tag::List((0..SECTION_COUNT).map(|_| Tag::List(Vec::new())).collect()));
    put(&mut root, "PostProcessing", post_processing);

    let heightmaps = match extra
        .raw_heightmaps
        .as_ref()
        .or(archive.raw_heightmaps.as_ref())
    {
        Some(raw) => raw.clone(),
        None => {
            let mut maps = Vec::new();
            for (kind, name) in HeightmapKind::ALL.into_iter().zip(HEIGHTMAP_KEYS) {
                put(
                    &mut maps,
                    name,
                    Tag::LongArray(
                        column
                            .heightmap(kind)
                            .iter()
                            .map(|&word| word as i64)
                            .collect(),
                    ),
                );
            }
            maps
        }
    };
    put(&mut root, "Heightmaps", compound(heightmaps));
    let structures = extra
        .structures
        .clone()
        .or_else(|| archive.structures.clone())
        .unwrap_or_else(|| {
            compound(
                EMPTY_STRUCTURES
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), value.clone()))
                    .collect(),
            )
        });
    put(&mut root, "structures", structures);

    // 顶层要保留的键：同名时不覆盖本层写出来的字段。
    let written: Vec<String> = root.iter().map(|(key, _)| key.clone()).collect();
    for (key, value) in &extra.preserved {
        if !written.iter().any(|existing| existing == key) {
            put(&mut root, key, value.clone());
        }
    }
    Ok(compound(root))
}

const HEIGHTMAP_KEYS: [&str; 4] = [
    "WORLD_SURFACE",
    "OCEAN_FLOOR",
    "MOTION_BLOCKING",
    "MOTION_BLOCKING_NO_LEAVES",
];

/// 带名字的根标签写成未压缩的 NBT 字节。
pub fn encode_bytes(
    column: &ChunkColumn,
    extra: &ChunkEncodeExtra<'_>,
) -> Result<Vec<u8>, ChunkNbtError> {
    let tag = encode(column, extra)?;
    let mut out = Vec::new();
    write_named("", &tag, &mut out);
    Ok(out)
}

/// 从未压缩的 NBT 字节读成一个区块束和透传片段。
pub fn decode_bytes(bytes: &[u8]) -> Result<DecodedChunk, ChunkNbtError> {
    let (name, entries) = read_named(bytes, Accounter::uncompressed_quota())?;
    if !name.is_empty() {
        return Err(ChunkNbtError::Malformed(format!(
            "chunk NBT root name is {name:?}, expected an empty string"
        )));
    }
    decode(&compound(entries))
}

/// 从根复合标签读成一个区块束和透传片段。
pub fn decode(root: &Tag) -> Result<DecodedChunk, ChunkNbtError> {
    let Tag::Compound(entries) = root else {
        return Err(ChunkNbtError::Malformed(
            "chunk NBT root must be a compound tag".to_owned(),
        ));
    };
    let version_id = as_int(field(root, "DataVersion"), "DataVersion")?;
    if version_id > version::DATA_VERSION {
        return Err(ChunkNbtError::NewerDataVersion(version_id));
    }
    if version_id < version::DATA_VERSION {
        return Err(ChunkNbtError::OlderDataVersion(version_id));
    }
    let status = as_string(field(root, "Status"), "Status")?.to_owned();
    // 官方 `SerializableChunkData.parse` 读任意 `Status`（`ChunkStatus.CODEC`，
    // 读不出来当 EMPTY），`read` 造 ProtoChunk 后由生成流水线从该状态继续跑到
    // Full。这里也接受非 full，`status` 原样交给上层决定怎么继续生成。
    // Vanilla parses any status into a ProtoChunk and resumes generation from
    // it; keep the raw status for the caller instead of rejecting the chunk.
    let full = status == FULL_STATUS;
    let chunk_pos = ChunkPos::new(
        as_int(field(root, "xPos"), "xPos")?,
        as_int(field(root, "zPos"), "zPos")?,
    );
    let last_update = as_long(field(root, "LastUpdate"), "LastUpdate")?;
    let inhabited_time = as_long(field(root, "InhabitedTime"), "InhabitedTime")?;
    let light_on = matches!(field(root, "isLightOn"), Some(Tag::Byte(value)) if *value != 0);

    let mut parts = ChunkColumnParts {
        sections: vec![
            SectionParts {
                block_palette: vec![blocks::AIR],
                block_data: Vec::new(),
                biome_palette: vec![registries::biome::PLAINS as u32],
                biome_data: Vec::new(),
            };
            SECTION_COUNT
        ],
        light_on,
        ..Default::default()
    };
    let section_list = as_list(field(root, "sections"), "sections")?;
    for tag in &section_list {
        let y = match field(tag, "Y") {
            Some(Tag::Byte(value)) => i32::from(*value),
            _ => {
                return Err(ChunkNbtError::Malformed("section has no Y byte".to_owned()));
            }
        };
        let slot = usize::try_from(y - MIN_LIGHT_SECTION)
            .ok()
            .filter(|&slot| slot < LIGHT_SECTION_COUNT)
            .ok_or_else(|| {
                ChunkNbtError::Malformed(format!("section Y {y} is outside the light range"))
            })?;
        if (MIN_SECTION..=MAX_SECTION).contains(&y) {
            let index = (y - MIN_SECTION) as usize;
            let (palette, data) = match field(tag, "block_states") {
                Some(tag) => read_container(tag, false)?,
                None => (vec![blocks::AIR], Vec::new()),
            };
            let (biomes, biome_data) = match field(tag, "biomes") {
                Some(tag) => read_container(tag, true)?,
                None => (vec![registries::biome::PLAINS as u32], Vec::new()),
            };
            parts.sections[index] = SectionParts {
                block_palette: palette,
                block_data: data,
                biome_palette: biomes,
                biome_data,
            };
        }
        for (key, layer) in [("BlockLight", 1usize), ("SkyLight", 0usize)] {
            if let Some(tag) = field(tag, key) {
                parts.light[layer][slot] = Some(read_light(tag, key)?);
            }
        }
    }

    let heightmaps_raw = match field(root, "Heightmaps") {
        Some(Tag::Compound(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    let heightmaps = field(root, "Heightmaps");
    for (index, key) in HEIGHTMAP_KEYS.iter().enumerate() {
        if let Some(tag) = heightmaps.and_then(|maps| field(maps, key)) {
            parts.heightmaps[index] = Some(as_long_array(tag, key)?);
        }
    }

    let block_entities = as_list(field(root, "block_entities"), "block_entities")?;
    let mut block_entity_raw = Vec::with_capacity(block_entities.len());
    let mut seen_signs = Vec::new();
    let mut preserved_entities = Vec::new();
    for tag in &block_entities {
        let Tag::Compound(values) = tag else {
            return Err(ChunkNbtError::Malformed(
                "block entity entry must be a compound tag".to_owned(),
            ));
        };
        block_entity_raw.push(values.clone());
        let Some((x, y, z)) = tag_position(tag) else {
            continue;
        };
        let Some(id) = block_entity_id(tag) else {
            continue;
        };
        // D9：整条挂进 ChunkColumn，按位置跟方块实体类型表一起走。
        // 类型或坐标对不上时 from_parts 会丢掉，不会挂到新方块上。
        if let Some(kind) = registries::block_entity_type::by_name(id) {
            preserved_entities.push((
                x.rem_euclid(16) as usize,
                y,
                z.rem_euclid(16) as usize,
                kind,
                tag.clone(),
            ));
        }
        let is_sign = id == "minecraft:sign" || id == "minecraft:hanging_sign";
        if !is_sign {
            continue;
        }
        let mut content = SignContent::BLANK;
        if let Some(front) = field(tag, "front_text") {
            content.front = parse_sign_text(front);
        }
        if let Some(back) = field(tag, "back_text") {
            content.back = parse_sign_text(back);
        }
        if let Some(Tag::Byte(waxed)) = field(tag, "is_waxed") {
            content.waxed = *waxed != 0;
        }
        // 空白告示牌在 Chunk 里没有条目：官方也是按状态临时建一个空的方块实体。
        if content == SignContent::BLANK {
            continue;
        }
        let local_x = x.rem_euclid(16) as usize;
        let local_z = z.rem_euclid(16) as usize;
        seen_signs.push((local_x, y, local_z, content));
    }
    parts.signs = seen_signs;
    parts.preserved_block_entities = preserved_entities;
    let block_ticks = as_list(field(root, "block_ticks"), "block_ticks")?;
    let fluid_ticks = as_list(field(root, "fluid_ticks"), "fluid_ticks")?;
    let structures = field(root, "structures").cloned().unwrap_or_else(|| {
        compound(
            EMPTY_STRUCTURES
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    });
    let post_processing = field(root, "PostProcessing")
        .cloned()
        .unwrap_or_else(|| Tag::List((0..SECTION_COUNT).map(|_| Tag::List(Vec::new())).collect()));

    let preserved: Vec<(String, Tag)> = entries
        .iter()
        .filter(|(key, _)| KNOWN_KEYS.contains(&key.as_str()))
        .filter(|(key, _)| {
            ![
                "DataVersion",
                "xPos",
                "yPos",
                "zPos",
                "LastUpdate",
                "InhabitedTime",
                "Status",
                "sections",
                "isLightOn",
                "block_entities",
                "block_ticks",
                "fluid_ticks",
                "PostProcessing",
                "Heightmaps",
                "structures",
            ]
            .contains(&key.as_str())
        })
        .cloned()
        .collect();
    let mut unknown: Vec<(String, Tag)> = entries
        .iter()
        .filter(|(key, _)| !KNOWN_KEYS.contains(&key.as_str()))
        .cloned()
        .collect();

    unknown.extend(preserved.iter().cloned());
    parts.archive.preserved = unknown.clone();
    parts.archive.structures = Some(structures.clone());
    parts.archive.post_processing = Some(post_processing.clone());
    // 非 full 的 ProtoChunk：官方把已写出的阶段内容装进 ProtoChunk，从该状态
    // 继续跑生成流水线。Bamboo 只有一步超平坦生成，所以用模板补齐方块层和
    // 光照（这些阶段在盘上可能还没写出），但保留盘上带来的顶层附加信息，
    // 让上层能把它当成「已经生成完的 ProtoChunk」继续交付；上层还会保留
    // 上层内容（方块实体、计划刻、实体），不再整块换成模板。
    // A non-full ProtoChunk keeps its written upper-layer data and has its
    // missing block/light stages filled from the flat template.
    if !full {
        // 高度图按补齐后的方块重算写四张，不把半成品的原始高度图带回去；
        // 其它顶层附加信息（LastUpdate / InhabitedTime / 不认识的键）保留。
        // Recompute the heightmaps from the filled blocks; keep the other
        // top-level metadata that came from disk.
        parts = ChunkColumnParts::read_from(&ChunkColumn::from_template());
        parts.archive = parts_archive_meta(
            last_update,
            inhabited_time,
            &unknown,
            &structures,
            &post_processing,
            &[],
        );
        parts.archive.raw_heightmaps = None;
    } else {
        parts.archive.raw_heightmaps = Some(heightmaps_raw.clone());
    }
    // `from_parts` 把 parts.archive 装进 Chunk；非 full 时高度图按补齐后的
    // 方块重算，存档元数据里不带原始 Heightmaps。
    // from_parts installs parts.archive; for a non-full chunk there are no raw
    // heightmaps, so the next save writes the four recomputed ones.
    let mut column =
        ChunkColumn::from_parts(parts).map_err(|error| ChunkNbtError::Column(error.to_string()))?;
    // 显式再装一次：`from_parts` 之后上层组装会替换方块层，把读盘带来的
    // LastUpdate / InhabitedTime / 原样保留键固定在这一柱上。
    // Re-install explicitly: the caller may rebuild the block layer afterwards,
    // so pin the on-disk metadata to this column now.
    let archive = if full {
        parts_archive_meta(
            last_update,
            inhabited_time,
            &unknown,
            &structures,
            &post_processing,
            &heightmaps_raw,
        )
    } else {
        let mut meta = parts_archive_meta(
            last_update,
            inhabited_time,
            &unknown,
            &structures,
            &post_processing,
            &[],
        );
        meta.raw_heightmaps = None;
        meta
    };
    column.chunk_mut().set_archive(archive);

    Ok(DecodedChunk {
        column: Box::new(column),
        chunk_pos,
        data_version: version_id,
        status,
        last_update,
        inhabited_time,
        light_on,
        block_entities,
        block_ticks,
        fluid_ticks,
        unknown,
        structures,
        post_processing,
        heightmaps: heightmaps_raw,
        preserved,
        block_entity_raw,
    })
}

fn parts_archive_meta(
    last_update: i64,
    inhabited_time: i64,
    preserved: &[(String, Tag)],
    structures: &Tag,
    post_processing: &Tag,
    heightmaps: &[(String, Tag)],
) -> mc_bamboo_world::chunk::ChunkArchiveMeta {
    mc_bamboo_world::chunk::ChunkArchiveMeta {
        last_update,
        inhabited_time,
        preserved: preserved.to_vec(),
        structures: Some(structures.clone()),
        post_processing: Some(post_processing.clone()),
        raw_heightmaps: Some(heightmaps.to_vec()),
    }
}

/// 把方块实体原始条目按位置整理成编码接口的附加片段，供上层使用。
pub fn block_entity_extras(decoded: &DecodedChunk) -> Vec<(i32, i32, i32, Tag)> {
    let mut out = Vec::new();
    for (tag, raw) in decoded.block_entities.iter().zip(&decoded.block_entity_raw) {
        if let Some((x, y, z)) = tag_position(tag) {
            out.push((x, y, z, compound(raw.clone())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{
        BlockPos, ChunkPos, LightLayer,
        sign::{DyeColor, SignContent},
    };
    use mc_bamboo_data::{blocks, version};
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_world::{
        MIN_Y,
        chunk::HeightmapKind,
        column::{ChunkColumn, ChunkColumnParts, SectionParts},
        light::{DataLayer, LAYER_BYTES, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION},
        palette::{disk_bits, disk_data_len, write_disk_index},
        store::ChunkStore,
    };

    use super::{
        ChunkEncodeExtra, ChunkNbtError, DecodedChunk, compound, decode, decode_bytes, encode,
        encode_bytes, field,
    };

    /// 用超平坦模板造一个经过光照引擎算稳的区块束。
    fn flat_column() -> Box<ChunkColumn> {
        let mut store = ChunkStore::flat();
        store.finish_light_updates();
        store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
    }

    /// 一个内容丰富、调色板大小各异的区块束，固定种子生成。
    fn rich_column(seed: u64) -> Box<ChunkColumn> {
        let mut store = ChunkStore::flat();
        let mut state = seed | 1;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for index in 0..1200 {
            let x = (next() % 16) as i32;
            let z = (next() % 16) as i32;
            let y = -64 + (next() % 64) as i32;
            let state = [
                blocks::STONE,
                blocks::DIRT,
                blocks::GLOWSTONE,
                blocks::OAK_SIGN,
            ][index % 4];
            store.set_block(BlockPos::new(x, y, z), state);
        }
        store.set_block(BlockPos::new(3, -60, 5), blocks::CHEST);
        store.set_block(BlockPos::new(7, -60, 9), blocks::OAK_SIGN);
        let mut content = SignContent::BLANK;
        content.front.messages[0] = "竹 hello".to_owned();
        content.front.color = DyeColor::Red;
        content.front.glowing = true;
        content.back.messages[1] = "back".to_owned();
        content.waxed = true;
        store.set_sign(BlockPos::new(7, -60, 9), content);
        store.finish_light_updates();
        store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
    }

    fn extra<'a>(pos: ChunkPos) -> ChunkEncodeExtra<'a> {
        ChunkEncodeExtra {
            chunk_pos: pos,
            last_update: 1234,
            inhabited_time: 99,
            ..Default::default()
        }
    }

    /// 按键比较两个复合标签，忽略官方 `HashMap` 的键顺序。
    fn same_tag(left: &Tag, right: &Tag) -> Result<(), String> {
        match (left, right) {
            (Tag::Compound(a), Tag::Compound(b)) => {
                if a.len() != b.len() {
                    return Err(format!("compound sizes {} vs {}", a.len(), b.len()));
                }
                for (key, value) in a {
                    let other = b
                        .iter()
                        .rev()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value)
                        .ok_or_else(|| format!("missing key {key}"))?;
                    same_tag(value, other).map_err(|error| format!("{key}: {error}"))?;
                }
                Ok(())
            }
            (Tag::List(a), Tag::List(b)) => {
                if a.len() != b.len() {
                    return Err(format!("list sizes {} vs {}", a.len(), b.len()));
                }
                for (index, (x, y)) in a.iter().zip(b).enumerate() {
                    same_tag(x, y).map_err(|error| format!("[{index}]: {error}"))?;
                }
                Ok(())
            }
            (a, b) if a == b => Ok(()),
            (a, b) => Err(format!("{a:?} vs {b:?}")),
        }
    }

    fn assert_items_equal(decoded: &DecodedChunk, column: &ChunkColumn) {
        assert_eq!(decoded.column.light_on(), column.light_on());
        assert_eq!(decoded.chunk_pos, ChunkPos::new(3, -9));
        assert_eq!(decoded.data_version, version::DATA_VERSION);
        assert_eq!(decoded.last_update, 1234);
        assert_eq!(decoded.inhabited_time, 99);
        for kind in HeightmapKind::ALL {
            assert_eq!(decoded.column.heightmap(kind), column.heightmap(kind));
        }
        assert_eq!(
            decoded.column.chunk().block_entities().collect::<Vec<_>>(),
            column.chunk().block_entities().collect::<Vec<_>>()
        );
        for x in 0..16 {
            for y in [-64, -61, -60, -59, 0, 100, 319] {
                for z in 0..16 {
                    let pos = BlockPos::new(x, y, z);
                    assert_eq!(
                        decoded.column.chunk().block(x as usize, y, z as usize),
                        column.chunk().block(x as usize, y, z as usize),
                        "block at {pos:?}"
                    );
                    for layer in [LightLayer::Sky, LightLayer::Block] {
                        assert_eq!(
                            decoded.column.light_value(layer, pos),
                            column.light_value(layer, pos),
                            "{layer:?} at {pos:?}"
                        );
                    }
                }
            }
        }
        for slot in 0..LIGHT_SECTION_COUNT {
            let y = MIN_LIGHT_SECTION + slot as i32;
            let pos = mc_bamboo_world::light::SectionPos::new(0, y, 0);
            for layer in [LightLayer::Sky, LightLayer::Block] {
                let before = column.section(layer, pos).map(|data| data.bytes());
                let after = decoded.column.section(layer, pos).map(|data| data.bytes());
                assert_eq!(after, before, "{layer:?} section {y}");
            }
        }
        assert_eq!(
            decoded.column.chunk().signs().count(),
            column.chunk().signs().count()
        );
        for (x, y, z, content) in column.chunk().signs() {
            assert_eq!(decoded.column.chunk().sign(x, y, z), Some(content));
        }
    }

    /// 官方 `SerializableChunkData.write()` 生成的 NBT 能被读回来。
    /// 这个文件由 `tools/mc-bamboo-vanilla` 的 storage oracle 生成，不启动服务端。
    #[test]
    fn official_chunk_is_readable_when_present() {
        let Ok(bytes) = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/mc-bamboo-vanilla/tests/official-chunk.nbt"
        )) else {
            return;
        };
        let decoded = decode_bytes(&bytes).expect("官方 NBT 能读");
        assert_eq!(decoded.status, "minecraft:full");
        assert_eq!(decoded.data_version, version::DATA_VERSION);
        assert_eq!(decoded.chunk_pos, ChunkPos::new(3, -9));
        assert!(decoded.light_on);
        assert_eq!(decoded.block_entities.len(), 1);
    }

    #[test]
    fn flat_chunk_round_trips_with_identical_bytes() {
        let column = flat_column();
        let options = extra(ChunkPos::new(3, -9));
        let bytes = encode_bytes(&column, &options).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        assert_items_equal(&decoded, &column);
        let again = encode_bytes(&decoded.column, &decoded.extra()).expect("再编码成功");
        assert_eq!(bytes, again, "Bamboo 写 → 读 → 写，字节完全相同");
    }

    #[test]
    fn rich_chunks_round_trip_with_fixed_seeds() {
        for seed in [1, 0x9e37_79b9, u64::MAX] {
            let column = rich_column(seed);
            let options = extra(ChunkPos::new(3, -9));
            let bytes = encode_bytes(&column, &options).expect("编码成功");
            let decoded = decode_bytes(&bytes).expect("解码成功");
            assert_items_equal(&decoded, &column);
            let again = encode_bytes(&decoded.column, &decoded.extra()).expect("再编码成功");
            assert_eq!(bytes, again, "种子 {seed} 的往返字节不相同");
        }
    }

    #[test]
    fn palette_bit_boundaries_match_the_official_rules() {
        // 官方 `Strategy.getConfigurationForPaletteSize(n)`：先算 ceillog2(n)，
        // 方块 1..4 位都落到 FOUR_BITS_LINEAR，5..8 位各用各的，>8 走 Global
        // （bitsInStorage 仍是请求的 ceillog2(n)）；生物群系直接用 1..3 位，
        // 更多也是 Global(ceillog2(n))。
        assert_eq!(disk_bits(1, false), 0);
        assert_eq!(disk_bits(2, false), 4);
        assert_eq!(disk_bits(4, false), 4);
        assert_eq!(disk_bits(5, false), 4);
        assert_eq!(disk_bits(16, false), 4);
        assert_eq!(disk_bits(17, false), 5);
        assert_eq!(disk_bits(256, false), 8);
        assert_eq!(disk_bits(257, false), 9);
        assert_eq!(disk_bits(1, true), 0);
        assert_eq!(disk_bits(2, true), 1);
        assert_eq!(disk_bits(3, true), 2);
        assert_eq!(disk_bits(4, true), 2);
        assert_eq!(disk_bits(5, true), 3);
        assert_eq!(disk_bits(8, true), 3);
        assert_eq!(disk_bits(9, true), 4);
        assert_eq!(disk_data_len(4096, 4), 256);
        assert_eq!(disk_data_len(4096, 9), 586);
        assert_eq!(disk_data_len(64, 1), 1);
        assert_eq!(disk_data_len(64, 0), 0);
    }

    /// 一段里只用少数几种方块，检查调试色板的位数和往返。
    #[test]
    fn palettes_of_every_size_round_trip() {
        for &size in &[1usize, 4, 5, 16, 17, 256, 257] {
            let mut parts = ChunkColumnParts::default();
            let palette: Vec<u32> = (0..size)
                .map(|index| blocks::AIR + (index as u32 * 97) % 3000)
                .collect();
            let bits = disk_bits(size, false);
            let mut data = vec![0u64; disk_data_len(4096, bits)];
            if bits != 0 {
                for index in 0..4096 {
                    write_disk_index(&mut data, bits, index, (index % size) as u32);
                }
            }
            parts.sections = vec![
                SectionParts {
                    block_palette: palette.clone(),
                    block_data: data.clone(),
                    biome_palette: vec![registries_biome_plains()],
                    biome_data: Vec::new(),
                };
                24
            ];
            let column = ChunkColumn::from_parts(parts).expect("组装成功");
            let bytes = encode_bytes(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
            let decoded = decode_bytes(&bytes).expect("解码成功");
            for index in 0..4096 {
                let expected = palette[index % size];
                assert_eq!(
                    decoded.column.chunk().sections()[0].blocks().get(index),
                    expected,
                    "size {size} index {index}"
                );
            }
        }

        for &size in &[1usize, 2, 3, 4, 5] {
            let mut parts = ChunkColumnParts::default();
            let palette: Vec<u32> = (0..size).map(|index| index as u32).collect();
            let bits = disk_bits(size, true);
            let mut data = vec![0u64; disk_data_len(64, bits)];
            if bits != 0 {
                for index in 0..64 {
                    write_disk_index(&mut data, bits, index, (index % size) as u32);
                }
            }
            parts.sections = vec![
                SectionParts {
                    block_palette: vec![blocks::AIR],
                    block_data: Vec::new(),
                    biome_palette: palette.clone(),
                    biome_data: data,
                };
                24
            ];
            let column = ChunkColumn::from_parts(parts).expect("组装成功");
            let bytes = encode_bytes(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
            let decoded = decode_bytes(&bytes).expect("解码成功");
            for index in 0..64 {
                assert_eq!(
                    decoded.column.chunk().sections()[0].biomes().get(index),
                    palette[index % size]
                );
            }
        }
    }

    fn registries_biome_plains() -> u32 {
        mc_bamboo_data::registries::biome::PLAINS as u32
    }

    #[test]
    fn light_sections_cover_the_two_extra_sections() {
        let column = flat_column();
        let options = extra(ChunkPos::new(3, -9));
        let tag = encode(&column, &options).expect("编码成功");
        let sections = match field(&tag, "sections") {
            Some(Tag::List(sections)) => sections,
            _ => panic!("sections 是列表"),
        };
        let ys: Vec<i8> = sections
            .iter()
            .filter_map(|section| match field(section, "Y") {
                Some(Tag::Byte(y)) => Some(*y),
                _ => None,
            })
            .collect();
        // 官方 `copyOf` 遍历 26 个光照段；存了光照的 -5 / 20 也会产出条目。
        assert_eq!(ys.first(), Some(&-5), "最低光照段是 -5");
        assert_eq!(ys.last(), Some(&19), "最高方块段是 19");
        // 超平坦模板的顶部光照段没存光照，官方 `copyOf` 会跳过它。
        assert_eq!(ys.len(), LIGHT_SECTION_COUNT - 1);
        for (slot, y) in ys.iter().enumerate() {
            assert_eq!(*y as i32, MIN_LIGHT_SECTION + slot as i32);
        }
        // 光照段 -5 和 20 的专属用例：手工在柱里放光照，两端必须出现且只有光照。
        let mut parts = ChunkColumnParts::read_from(&column);
        for slot in [0usize, LIGHT_SECTION_COUNT - 1] {
            parts.light[0][slot] = Some(DataLayer::Filled(15));
        }
        let edged = ChunkColumn::from_parts(parts).expect("组装成功");
        let tag = encode(&edged, &options).expect("编码成功");
        let Some(Tag::List(edgelist)) = field(&tag, "sections") else {
            panic!("sections 是列表");
        };
        let ys: Vec<i8> = edgelist
            .iter()
            .filter_map(|section| match field(section, "Y") {
                Some(Tag::Byte(y)) => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(ys.first(), Some(&-5));
        assert_eq!(ys.last(), Some(&20));
        assert_eq!(ys.len(), LIGHT_SECTION_COUNT);
        for section in edgelist {
            let y = match field(section, "Y") {
                Some(Tag::Byte(y)) => *y,
                _ => continue,
            };
            if y == -5 || y == 20 {
                assert!(field(section, "block_states").is_none());
                assert!(field(section, "biomes").is_none());
                assert!(field(section, "SkyLight").is_some());
            } else {
                assert!(field(section, "block_states").is_some());
                assert!(field(section, "biomes").is_some());
            }
        }
    }

    #[test]
    fn light_data_is_only_written_when_the_layer_has_data() {
        let column = flat_column();
        let options = extra(ChunkPos::new(3, -9));
        let bytes = encode_bytes(&column, &options).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        for slot in 0..LIGHT_SECTION_COUNT {
            let y = MIN_LIGHT_SECTION + slot as i32;
            let pos = mc_bamboo_world::light::SectionPos::new(0, y, 0);
            for layer in [LightLayer::Sky, LightLayer::Block] {
                let before = column.section(layer, pos).map(|data| data.bytes());
                let after = decoded.column.section(layer, pos).map(|data| data.bytes());
                assert_eq!(after, before, "{layer:?} section {y}");
            }
        }
        // 写出来的光照 byte array 一定是 2048 字节。
        let tag = encode(&column, &options).expect("编码成功");
        if let Some(Tag::List(sections)) = field(&tag, "sections") {
            for section in sections {
                for key in ["BlockLight", "SkyLight"] {
                    if let Some(tag) = field(section, key) {
                        let Tag::ByteArray(bytes) = tag else {
                            panic!("{key} 是 byte array");
                        };
                        assert_eq!(bytes.len(), LAYER_BYTES);
                    }
                }
            }
        }
    }

    #[test]
    fn unknown_top_level_keys_are_preserved_verbatim() {
        let column = flat_column();
        let mut options = extra(ChunkPos::new(3, -9));
        options.preserved.push((
            "bamboo:custom".to_owned(),
            Tag::Compound(vec![("answer".to_owned(), Tag::Int(42))]),
        ));
        let bytes = encode_bytes(&column, &options).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        assert_eq!(decoded.unknown.len(), 1);
        assert_eq!(decoded.unknown[0].0, "bamboo:custom");
        let mut again = decoded.extra();
        again.preserved.extend(decoded.unknown.clone());
        let again_bytes = encode_bytes(&decoded.column, &again).expect("再编码成功");
        assert_eq!(bytes, again_bytes);
    }

    #[test]
    fn extra_block_entity_fields_are_merged_without_overwriting() {
        let column = rich_column(7);
        let mut options = extra(ChunkPos::new(3, -9));
        let pos = (3 * 16 + 3, -60, -9 * 16 + 5);
        options.block_entity_extra.push((
            pos.0,
            pos.1,
            pos.2,
            Tag::Compound(vec![
                ("Items".to_owned(), Tag::List(vec![Tag::Byte(9)])),
                // 编解码器写 id，附加的同名键必须被丢掉。
                (
                    "id".to_owned(),
                    Tag::String("minecraft:not_a_chest".to_owned()),
                ),
            ]),
        ));
        let bytes = encode_bytes(&column, &options).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        let chest = decoded
            .block_entities
            .iter()
            .find(|tag| super::tag_position(tag) == Some(pos))
            .expect("箱子条目存在");
        assert_eq!(
            super::block_entity_id(chest),
            Some("minecraft:chest"),
            "id 以编解码器为准"
        );
        assert!(field(chest, "Items").is_some(), "附加的 Items 原样合并");
        assert_eq!(decoded.block_entity_raw.len(), decoded.block_entities.len());
        let again = encode_bytes(&decoded.column, &decoded.extra()).expect("再编码成功");
        assert_eq!(bytes, again);
    }

    /// 官方 `BlockEntity.saveWithoutMetadata` 与 `getBlockEntityNbtForSaving` 必写的两个键。
    #[test]
    fn block_entities_always_write_components_and_keep_packed() {
        let column = rich_column(1);
        let tag = encode(&column, &extra(ChunkPos::new(3, -9))).expect("编码成功");
        let Tag::List(entities) = field(&tag, "block_entities").expect("block_entities") else {
            panic!("block_entities 是列表");
        };
        assert!(!entities.is_empty());
        for entity in entities {
            assert_eq!(field(entity, "components"), Some(&compound(Vec::new())));
            assert_eq!(field(entity, "keepPacked"), Some(&Tag::Byte(0)));
        }
        // 原样写回一条旧条目时也补齐，不改它已有的值。
        let mut options = extra(ChunkPos::new(3, -9));
        options.raw_block_entities = Some(vec![Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:chest".to_owned())),
            ("x".to_owned(), Tag::Int(0)),
            ("y".to_owned(), Tag::Int(0)),
            ("z".to_owned(), Tag::Int(0)),
            ("keepPacked".to_owned(), Tag::Byte(1)),
        ])]);
        let tag = encode(&column, &options).expect("编码成功");
        let Tag::List(entities) = field(&tag, "block_entities").expect("block_entities") else {
            panic!("block_entities 是列表");
        };
        assert_eq!(
            field(&entities[0], "components"),
            Some(&compound(Vec::new()))
        );
        assert_eq!(field(&entities[0], "keepPacked"), Some(&Tag::Byte(1)));
    }

    /// 读进带不认识键的箱子 → 不动 → 键还在；拆掉箱子原地放熔炉 → 旧键不挂过去。
    #[test]
    fn preserved_block_entity_keys_follow_the_block_type() {
        let pos = ChunkPos::new(3, -9);
        let world_pos = (3 * 16 + 3, -60, -9 * 16 + 5);
        let mut options = extra(pos);
        options.raw_block_entities = Some(vec![Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:chest".to_owned())),
            ("x".to_owned(), Tag::Int(world_pos.0)),
            ("y".to_owned(), Tag::Int(world_pos.1)),
            ("z".to_owned(), Tag::Int(world_pos.2)),
            ("Items".to_owned(), Tag::List(Vec::new())),
            ("UnknownKey".to_owned(), Tag::String("keep".to_owned())),
        ])]);
        let column = rich_column(1);
        // rich_column 的箱子就在 (3, -60, 5)。
        let bytes = encode_bytes(&column, &options).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        let preserved = decoded
            .column
            .chunk()
            .preserved_block_entity(3, -60, 5)
            .expect("保留条目挂在方块实体类型表旁边");
        assert_eq!(
            field(preserved, "UnknownKey"),
            Some(&Tag::String("keep".to_owned()))
        );
        let again = encode_bytes(&decoded.column, &decoded.extra()).expect("再编码成功");
        let reread = decode_bytes(&again).expect("再解码成功");
        let chest = reread
            .block_entities
            .iter()
            .find(|tag| super::tag_position(tag) == Some(world_pos))
            .expect("箱子条目还在");
        assert_eq!(
            field(chest, "UnknownKey"),
            Some(&Tag::String("keep".to_owned()))
        );

        // 换成另一种方块实体：旧的整条复合标签不能挂到熔炉上。
        let mut parts = ChunkColumnParts::read_from(&decoded.column);
        let section = (world_pos.1 - MIN_Y) as usize / 16;
        parts.sections[section].block_palette = vec![blocks::FURNACE];
        parts.sections[section].block_data.clear();
        // 手动换掉类型时把保留条目一起丢掉，模拟运行时 set_block 的效果。
        parts.preserved_block_entities.clear();
        let mut replaced = ChunkColumn::from_parts(parts).expect("组装成功");
        let chunk = replaced.chunk_mut();
        chunk.set_block(3, -60, 5, blocks::FURNACE);
        assert!(chunk.preserved_block_entity(3, -60, 5).is_none());
        let tag = encode(&replaced, &extra(pos)).expect("编码成功");
        let Tag::List(entities) = field(&tag, "block_entities").expect("block_entities") else {
            panic!("block_entities 是列表");
        };
        let furnace = entities
            .iter()
            .find(|tag| super::tag_position(tag) == Some(world_pos))
            .expect("熔炉条目存在");
        assert_eq!(super::block_entity_id(furnace), Some("minecraft:furnace"));
        assert!(field(furnace, "UnknownKey").is_none());
    }

    #[test]
    fn ticks_and_structures_are_written_even_when_empty() {
        let column = flat_column();
        let tag = encode(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        assert!(matches!(field(&tag, "block_ticks"), Some(Tag::List(v)) if v.is_empty()));
        assert!(matches!(field(&tag, "fluid_ticks"), Some(Tag::List(v)) if v.is_empty()));
        let structures = field(&tag, "structures").expect("structures 必写");
        assert!(field(structures, "starts").is_some());
        assert!(field(structures, "References").is_some());
        let post = field(&tag, "PostProcessing").expect("PostProcessing 必写");
        let Tag::List(lists) = post else {
            panic!("PostProcessing 是列表");
        };
        assert_eq!(lists.len(), 24);
    }

    #[test]
    fn sign_content_survives_the_round_trip() {
        let column = rich_column(1);
        let bytes = encode_bytes(&column, &extra(ChunkPos::new(3, -9))).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        let sign = decoded
            .block_entities
            .iter()
            .filter(|tag| super::block_entity_id(tag) == Some("minecraft:sign"))
            .find(|tag| {
                let front = super::field(tag, "front_text");
                let messages = front.and_then(|t| super::field(t, "messages"));
                matches!(messages, Some(Tag::List(values)) if !values.is_empty() && values[0] != Tag::String(String::new()))
            })
            .expect("告示牌条目存在");
        let front = field(sign, "front_text").expect("front_text");
        let Tag::List(messages) = field(front, "messages").expect("messages") else {
            panic!("messages 是列表");
        };
        assert_eq!(messages[0], Tag::String("竹 hello".to_owned()));
        assert_eq!(field(front, "color"), Some(&Tag::String("red".to_owned())));
        assert_eq!(field(front, "has_glowing_text"), Some(&Tag::Byte(1)));
        assert_eq!(field(sign, "is_waxed"), Some(&Tag::Byte(1)));
        assert!(field(sign, "playerWhoMayEdit").is_none());
    }

    #[test]
    fn a_column_from_disk_can_be_inserted_into_a_store() {
        let column = rich_column(3);
        let bytes = encode_bytes(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        let decoded = decode_bytes(&bytes).expect("解码成功");
        let mut store = ChunkStore::flat();
        store
            .insert_column(ChunkPos::new(0, 0), decoded.column)
            .unwrap_or_else(|_| panic!("光照算完了，可以放入"));
        assert_eq!(
            store.block(BlockPos::new(7, -60, 9)),
            Some(blocks::OAK_SIGN)
        );
        assert_eq!(
            store.chunk(ChunkPos::new(0, 0)).block(7, -60, 9),
            blocks::OAK_SIGN
        );
        for kind in HeightmapKind::ALL {
            assert_eq!(
                store.chunk(ChunkPos::new(0, 0)).heightmap(kind),
                column.heightmap(kind)
            );
        }
    }

    #[test]
    fn status_and_data_version_are_checked() {
        let column = flat_column();
        let mut tag = encode(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        let Tag::Compound(entries) = &mut tag else {
            unreachable!()
        };
        for (key, value) in entries.iter_mut() {
            if key == "Status" {
                *value = Tag::String("minecraft:structure_starts".to_owned());
            }
        }
        // 非 full 的 ProtoChunk 不再被拒绝：原样返回 status，方块/光照用模板补齐。
        let proto = decode(&tag).expect("非 full 的区块也要能读回");
        assert_eq!(proto.status, "minecraft:structure_starts");
        assert_eq!(
            proto.column.chunk().block(3, -61, 5),
            mc_bamboo_data::blocks::GRASS_BLOCK,
            "没写出的方块阶段用超平坦模板补齐"
        );

        let mut newer = encode(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        let Tag::Compound(entries) = &mut newer else {
            unreachable!()
        };
        for (key, value) in entries.iter_mut() {
            if key == "DataVersion" {
                *value = Tag::Int(version::DATA_VERSION + 1);
            }
        }
        assert!(matches!(
            decode(&newer),
            Err(ChunkNbtError::NewerDataVersion(_))
        ));
        let mut older = newer.clone();
        let Tag::Compound(entries) = &mut older else {
            unreachable!()
        };
        for (key, value) in entries.iter_mut() {
            if key == "DataVersion" {
                *value = Tag::Int(version::DATA_VERSION - 1);
            }
        }
        assert!(matches!(
            decode(&older),
            Err(ChunkNbtError::OlderDataVersion(_))
        ));
    }

    #[test]
    fn fuzzed_nbt_never_panics() {
        let column = rich_column(11);
        let good = encode_bytes(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..400 {
            let mut bytes = good.clone();
            match next() % 3 {
                0 => {
                    let cut = (next() as usize) % (bytes.len() + 1);
                    bytes.truncate(cut);
                }
                1 => {
                    let index = (next() as usize) % bytes.len();
                    bytes[index] ^= 1 << (next() % 8);
                }
                _ => {
                    bytes.extend_from_slice(&good[..(next() as usize % 32).min(good.len())]);
                }
            }
            let _ = decode_bytes(&bytes);
        }
    }

    #[test]
    fn malformed_palettes_are_rejected_without_panicking() {
        let column = flat_column();
        let tag = encode(&column, &extra(ChunkPos::new(0, 0))).expect("编码成功");
        let cases: Vec<Tag> = Vec::new();
        let _ = cases;
        let mut broken = tag.clone();
        if let Tag::Compound(entries) = &mut broken {
            for (key, value) in entries.iter_mut() {
                if key != "sections" {
                    continue;
                }
                let Tag::List(sections) = value else {
                    unreachable!()
                };
                for section in sections.iter_mut() {
                    let Some(Tag::Byte(y)) = field(section, "Y") else {
                        continue;
                    };
                    if *y != 0 {
                        continue;
                    }
                    let Tag::Compound(values) = section else {
                        unreachable!()
                    };
                    for (name, content) in values.iter_mut() {
                        if name != "block_states" {
                            continue;
                        }
                        let Tag::Compound(fields) = content else {
                            unreachable!()
                        };
                        for (field_name, field_value) in fields.iter_mut() {
                            if field_name == "palette" {
                                let Tag::List(items) = field_value else {
                                    unreachable!()
                                };
                                items.push(items[0].clone());
                            }
                        }
                    }
                }
            }
        }
        assert!(decode(&broken).is_err(), "long 数组缺了要报错");
        let _ = same_tag(&tag, &tag);
        let _ = DataLayer::EMPTY;
    }
}
