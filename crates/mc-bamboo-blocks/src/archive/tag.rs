//! 存档 NBT 的小工具：和官方 `ValueOutput` / `ValueInput` 的宽松读法对齐。
//!
//! 复合标签用 `Vec<(String, Tag)>` 保序，同一键名取最后一个（官方 `HashMap` 的行为）。
//! 官方 `ValueInput.getIntOr` 等读法接受任意数值标签，按 Java 的数值转换取值；这里照做。

use mc_bamboo_data::{block_by_name, block_id_of_state, blocks, state_count};
use mc_bamboo_nbt::Tag;

use crate::BlockState;

/// 解码一段存档 NBT 失败。客户端或磁盘上的坏数据只会得到这个错误，不会 panic。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// 写好一个复合标签条目。同名键由 [`with_defaults`] 或合并阶段去重。
pub fn put(entries: &mut Vec<(String, Tag)>, key: &str, value: Tag) {
    entries.push((key.to_owned(), value));
}

pub fn compound(entries: Vec<(String, Tag)>) -> Tag {
    Tag::Compound(entries)
}

/// 按名字查字段。同名键取最后一个。
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

pub fn entries(tag: &Tag) -> Result<&[(String, Tag)]> {
    match tag {
        Tag::Compound(entries) => Ok(entries),
        _ => Err(Error::new("expected a compound tag")),
    }
}

/// `Tag` 的 NBT 类型名，出错信息里用。
pub fn tag_name(tag: &Tag) -> &'static str {
    match tag {
        Tag::Byte(_) => "byte",
        Tag::Short(_) => "short",
        Tag::Int(_) => "int",
        Tag::Long(_) => "long",
        Tag::Float(_) => "float",
        Tag::Double(_) => "double",
        Tag::ByteArray(_) => "byte array",
        Tag::String(_) => "string",
        Tag::List(_) => "list",
        Tag::Compound(_) => "compound",
        Tag::IntArray(_) => "int array",
        Tag::LongArray(_) => "long array",
    }
}

/// 官方 `NbtOps` 从任意数值标签取 Java 的 `intValue()`。
pub fn java_i32(tag: &Tag) -> Result<i32> {
    Ok(match tag {
        Tag::Byte(v) => i32::from(*v),
        Tag::Short(v) => i32::from(*v),
        Tag::Int(v) => *v,
        Tag::Long(v) => *v as i32,
        Tag::Float(v) => *v as i32,
        Tag::Double(v) => *v as i32,
        _ => return Err(Error::new(format!("{} is not a number", tag_name(tag)))),
    })
}

pub fn java_i64(tag: &Tag) -> Result<i64> {
    Ok(match tag {
        Tag::Byte(v) => i64::from(*v),
        Tag::Short(v) => i64::from(*v),
        Tag::Int(v) => i64::from(*v),
        Tag::Long(v) => *v,
        Tag::Float(v) => *v as i64,
        Tag::Double(v) => *v as i64,
        _ => return Err(Error::new(format!("{} is not a number", tag_name(tag)))),
    })
}

pub fn java_f32(tag: &Tag) -> Result<f32> {
    Ok(match tag {
        Tag::Byte(v) => f32::from(*v),
        Tag::Short(v) => f32::from(*v),
        Tag::Int(v) => *v as f32,
        Tag::Long(v) => *v as f32,
        Tag::Float(v) => *v,
        Tag::Double(v) => *v as f32,
        _ => return Err(Error::new(format!("{} is not a number", tag_name(tag)))),
    })
}

pub fn java_f64(tag: &Tag) -> Result<f64> {
    Ok(match tag {
        Tag::Byte(v) => f64::from(*v),
        Tag::Short(v) => f64::from(*v),
        Tag::Int(v) => f64::from(*v),
        Tag::Long(v) => *v as f64,
        Tag::Float(v) => f64::from(*v),
        Tag::Double(v) => *v,
        _ => return Err(Error::new(format!("{} is not a number", tag_name(tag)))),
    })
}

pub fn bool_or(tag: &Tag, key: &str, default: bool) -> bool {
    match field(tag, key) {
        Some(value) => java_i64(value).unwrap_or(i64::from(default)) != 0,
        None => default,
    }
}

pub fn byte_or(tag: &Tag, key: &str, default: i8) -> i8 {
    match field(tag, key) {
        Some(value) => java_i32(value).unwrap_or(i32::from(default)) as i8,
        None => default,
    }
}

pub fn short_or(tag: &Tag, key: &str, default: i16) -> i16 {
    match field(tag, key) {
        Some(value) => java_i32(value).unwrap_or(i32::from(default)) as i16,
        None => default,
    }
}

pub fn int_or(tag: &Tag, key: &str, default: i32) -> i32 {
    match field(tag, key) {
        Some(value) => java_i32(value).unwrap_or(default),
        None => default,
    }
}

pub fn long_or(tag: &Tag, key: &str, default: i64) -> i64 {
    match field(tag, key) {
        Some(value) => java_i64(value).unwrap_or(default),
        None => default,
    }
}

pub fn float_or(tag: &Tag, key: &str, default: f32) -> f32 {
    match field(tag, key) {
        Some(value) => java_f32(value).unwrap_or(default),
        None => default,
    }
}

pub fn double_or(tag: &Tag, key: &str, default: f64) -> f64 {
    match field(tag, key) {
        Some(value) => java_f64(value).unwrap_or(default),
        None => default,
    }
}

pub fn string_or<'a>(tag: &'a Tag, key: &str) -> Option<&'a str> {
    match field(tag, key) {
        Some(Tag::String(value)) => Some(value),
        _ => None,
    }
}

/// `Vec3.CODEC` 是三个 double 的列表。缺了或者坏了一律用缺省向量。
pub fn vec3(tag: &Tag, key: &str, default: [f64; 3]) -> [f64; 3] {
    let Some(Tag::List(values)) = field(tag, key) else {
        return default;
    };
    if values.len() < 3 {
        return default;
    }
    let mut out = default;
    for (index, value) in values.iter().take(3).enumerate() {
        out[index] = java_f64(value).unwrap_or(default[index]);
    }
    out
}

/// `Vec2.CODEC` 是两个 float 的列表。
pub fn vec2(tag: &Tag, key: &str, default: [f32; 2]) -> [f32; 2] {
    let Some(Tag::List(values)) = field(tag, key) else {
        return default;
    };
    if values.len() < 2 {
        return default;
    }
    let mut out = default;
    for (index, value) in values.iter().take(2).enumerate() {
        out[index] = java_f32(value).unwrap_or(default[index]);
    }
    out
}

pub fn int_array(tag: &Tag, key: &str) -> Vec<i32> {
    match field(tag, key) {
        Some(Tag::IntArray(values)) => values.clone(),
        Some(Tag::List(values)) => values
            .iter()
            .map(java_i32)
            .collect::<Result<_>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 把建模状态写出的键覆盖到原样保留的条目上，其余键留着；
/// `omit` 里的键等于官方"可省"且当前值就是默认值，要删掉，避免写回旧值。
pub fn with_defaults(raw: &Tag, modeled: Vec<(String, Tag)>, omit: &[&str]) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    if let Tag::Compound(values) = raw {
        for (key, value) in values {
            if modeled.iter().any(|(modeled, _)| modeled == key) || omit.contains(&key.as_str()) {
                continue;
            }
            // 同名键只留最后一个，和官方 HashMap 一致。
            out.retain(|(existing, _)| existing != key);
            out.push((key.clone(), value.clone()));
        }
    }
    for (key, value) in modeled {
        out.retain(|(existing, _)| existing != &key);
        out.push((key, value));
    }
    out
}

/// 官方 `BlockState.CODEC`：默认状态写成裸注册名字符串，其它状态写成
/// `{ Name, Properties: { ... } }`，属性值都是字符串。
pub fn encode_block_state(state: BlockState) -> Tag {
    let block = &blocks::BLOCKS[state.block_id()];
    if state.0 == block.default_state {
        return Tag::String(block.name.to_owned());
    }
    let mut properties = Vec::new();
    let mut offset = state.0 - block.first_state;
    let mut remaining = state_count(state.block_id());
    for property in block.properties {
        let choices = property.values.len() as u32;
        remaining /= choices;
        let index = (offset / remaining) as usize;
        offset %= remaining;
        if let Some(value) = property.values.get(index) {
            put(
                &mut properties,
                property.name,
                Tag::String((*value).to_owned()),
            );
        }
    }
    let mut entries = Vec::with_capacity(2);
    put(&mut entries, "Name", Tag::String(block.name.to_owned()));
    if !properties.is_empty() {
        put(&mut entries, "Properties", compound(properties));
    }
    compound(entries)
}

/// 官方 `BlockState.CODEC` 的读法。坏数据返回 [`Error`]。
pub fn decode_block_state(tag: &Tag) -> Result<BlockState> {
    let (name, properties_tag) = match tag {
        Tag::String(value) => (value.as_str(), None),
        Tag::Compound(_) => (
            match field(tag, "Name") {
                Some(Tag::String(value)) => value.as_str(),
                _ => return Err(Error::new("block state has no Name")),
            },
            field(tag, "Properties"),
        ),
        _ => return Err(Error::new("block state must be a string or a compound")),
    };
    let block = block_by_name(name).ok_or_else(|| Error::new(format!("unknown block {name}")))?;
    let id = block_id_of_state(block.first_state)
        .ok_or_else(|| Error::new(format!("block {name} has no states")))?;
    let Some(properties_tag) = properties_tag else {
        return Ok(BlockState(block.default_state));
    };
    let Tag::Compound(properties) = properties_tag else {
        return Err(Error::new("block state Properties must be a compound"));
    };
    let count = state_count(id);
    let mut offset = 0u32;
    let mut remaining = count;
    for property in block.properties {
        let choices = property.values.len() as u32;
        remaining /= choices;
        let value = properties
            .iter()
            .rev()
            .find(|(key, _)| key == property.name)
            .map(|(_, value)| value);
        let index = match value {
            None => 0u32,
            Some(Tag::String(value)) => property
                .values
                .iter()
                .position(|candidate| *candidate == value)
                .ok_or_else(|| Error::new(format!("unknown value {value:?} for {name}")))?
                as u32,
            _ => return Err(Error::new("block state property must be a string")),
        };
        offset += index * remaining;
    }
    let state = block.first_state + offset;
    (state < blocks::TOTAL_STATES)
        .then_some(BlockState(state))
        .ok_or_else(|| Error::new(format!("block {name} state {state} is out of range")))
}

/// 从原样保留的条目里去掉一组键，交给调用方重建。
pub fn remove_known(raw: &Tag, keys: &[&str]) -> Vec<(String, Tag)> {
    match raw {
        Tag::Compound(values) => values
            .iter()
            .filter(|(key, _)| !keys.contains(&key.as_str()))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}
