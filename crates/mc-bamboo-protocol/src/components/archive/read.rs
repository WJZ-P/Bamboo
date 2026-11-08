//! 从 NBT 读出一个值，语义照官方的 `DynamicOps` 和 `Codec`。
//!
//! 官方的数值编解码器接受任意数值标签再转成目标类型（`Codec.INT` 从 `ByteTag` 也能解），
//! 字符串和布尔的规则也各有各的宽松之处，这里一条一条照着 `NbtOps` 实现。

use mc_bamboo_data::{component_registries::registry_names, tags};
use mc_bamboo_nbt::Tag;

use crate::components::cursor::{Holder, HolderSet, Registry, parse_identifier};

/// 存档编解码器的错误。只用来报「哪个字段不对」，不参与网络协议。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// 一个复合标签的条目。
pub fn compound(tag: &Tag) -> Result<&[(String, Tag)]> {
    match tag {
        Tag::Compound(entries) => Ok(entries),
        _ => Err(Error::new("expected a compound")),
    }
}

/// 一个字段，没有时返回 `None`。
pub fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// 一个必填字段。
pub fn required<'a>(entries: &'a [(String, Tag)], name: &str) -> Result<&'a Tag> {
    field(entries, name).ok_or_else(|| Error::new(format!("missing field {name}")))
}

/// 一个必填的复合字段。
pub fn required_compound<'a>(
    entries: &'a [(String, Tag)],
    name: &str,
) -> Result<&'a [(String, Tag)]> {
    compound(required(entries, name)?)
}

/// 官方 `Codec.STRING`。
pub fn string(tag: &Tag) -> Result<&str> {
    match tag {
        Tag::String(value) => Ok(value),
        _ => Err(Error::new("expected a string")),
    }
}

/// 官方 `Identifier.CODEC`，返回值是补全命名空间之后的完整名字。
pub fn identifier(tag: &Tag) -> Result<String> {
    let raw = string(tag)?;
    parse_identifier(raw).map_err(|_| Error::new(format!("{raw} is not a resource location")))
}

/// 官方 `Codec.BOOL`：任意数值，非零就是真。
pub fn bool_value(tag: &Tag) -> Result<bool> {
    Ok(number(tag)? != 0.0)
}

/// 官方 `StringRepresentable.fromEnum` 的解码：NBT 里只接受枚举常量名。
pub fn enum_id(names: &[&str], tag: &Tag) -> Result<i32> {
    let name = string(tag)?;
    names
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
        .ok_or_else(|| Error::new(format!("{name} is not a known enum name")))
}

/// 官方 `Codec.BYTE`。
pub fn byte(tag: &Tag) -> Result<i8> {
    Ok(java_i32(tag)? as i8)
}

/// 官方 `Codec.SHORT`。
pub fn short(tag: &Tag) -> Result<i16> {
    Ok(java_i32(tag)? as i16)
}

/// 官方 `Codec.INT`。
pub fn int(tag: &Tag) -> Result<i32> {
    java_i32(tag)
}

/// 官方 `Codec.LONG`。
pub fn long(tag: &Tag) -> Result<i64> {
    match tag {
        Tag::Byte(value) => Ok(i64::from(*value)),
        Tag::Short(value) => Ok(i64::from(*value)),
        Tag::Int(value) => Ok(i64::from(*value)),
        Tag::Long(value) => Ok(*value),
        Tag::Float(value) => Ok(*value as i64),
        Tag::Double(value) => Ok(*value as i64),
        _ => Err(Error::new("expected a number")),
    }
}

/// 官方 `Codec.FLOAT`。
pub fn float(tag: &Tag) -> Result<f32> {
    Ok(number(tag)? as f32)
}

/// 官方 `Codec.DOUBLE`。
pub fn double(tag: &Tag) -> Result<f64> {
    number(tag)
}

/// 官方 `DynamicOps.getNumberValue`。
pub fn number(tag: &Tag) -> Result<f64> {
    match tag {
        Tag::Byte(value) => Ok(f64::from(*value)),
        Tag::Short(value) => Ok(f64::from(*value)),
        Tag::Int(value) => Ok(f64::from(*value)),
        Tag::Long(value) => Ok(*value as f64),
        Tag::Float(value) => Ok(f64::from(*value)),
        Tag::Double(value) => Ok(*value),
        _ => Err(Error::new("expected a number")),
    }
}

/// Java 的 `Number.intValue()`：长整数截低 32 位，浮点数按 Java 的转换规则夹住。
pub fn java_i32(tag: &Tag) -> Result<i32> {
    match tag {
        Tag::Byte(value) => Ok(i32::from(*value)),
        Tag::Short(value) => Ok(i32::from(*value)),
        Tag::Int(value) => Ok(*value),
        Tag::Long(value) => Ok(*value as i32),
        Tag::Float(value) => Ok(*value as i32),
        Tag::Double(value) => Ok(*value as i32),
        _ => Err(Error::new("expected a number")),
    }
}

/// 官方 `DynamicOps.getStream`：普通列表和三种数字数组都当成列表。
///
/// 数组里的数字要变成独立的标签，所以这里返回克隆出来的列表。
pub fn list(tag: &Tag) -> Result<Vec<Tag>> {
    match tag {
        Tag::List(values) => Ok(values.clone()),
        Tag::ByteArray(values) => Ok(values.iter().map(|value| Tag::Byte(*value)).collect()),
        Tag::IntArray(values) => Ok(values.iter().map(|value| Tag::Int(*value)).collect()),
        Tag::LongArray(values) => Ok(values.iter().map(|value| Tag::Long(*value)).collect()),
        _ => Err(Error::new("expected a list")),
    }
}

/// 官方 `DynamicOps.getMap`：复合标签。
pub fn map(tag: &Tag) -> Result<&[(String, Tag)]> {
    compound(tag)
}

/// 官方 `Codec.INT_STREAM` 这类数组编解码器。
pub fn int_array(tag: &Tag) -> Result<&[i32]> {
    match tag {
        Tag::IntArray(values) => Ok(values),
        _ => Err(Error::new("expected an int array")),
    }
}

/// 官方 `Codec.LONG_STREAM`。
pub fn long_array(tag: &Tag) -> Result<&[i64]> {
    match tag {
        Tag::LongArray(values) => Ok(values),
        _ => Err(Error::new("expected a long array")),
    }
}

/// 官方 `Codec.BYTE_BUFFER` / `NbtOps.getByteBuffer`。
pub fn byte_array(tag: &Tag) -> Result<&[i8]> {
    match tag {
        Tag::ByteArray(values) => Ok(values),
        _ => Err(Error::new("expected a byte array")),
    }
}

/// 一个注册表条目名查 ID。查不到时返回错误。
pub fn registry_id(registry: Registry, tag: &Tag) -> Result<i32> {
    let name = identifier(tag)?;
    name_id(registry, &name)
        .ok_or_else(|| Error::new(format!("{name} is not in {}", registry.name)))
}

/// 一个注册表条目名查 ID。
pub fn name_id(registry: Registry, name: &str) -> Option<i32> {
    names(registry)?
        .iter()
        .position(|entry| *entry == name)
        .map(|id| id as i32)
}

/// ID 反查注册表条目名。
pub fn id_name(registry: Registry, id: i32) -> Result<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| names(registry).and_then(|entries| entries.get(index).copied()))
        .ok_or_else(|| Error::new(format!("{id} is not in {}", registry.name)))
}

/// 一个注册表的全部条目名，下标就是网络 ID。
fn names(registry: Registry) -> Option<&'static [&'static str]> {
    Some(match registry.name {
        "minecraft:attribute" => registry_names::ATTRIBUTE,
        "minecraft:banner_pattern" => registry_names::BANNER_PATTERN,
        "minecraft:consume_effect_type" => registry_names::CONSUME_EFFECT_TYPE,
        "minecraft:block" => registry_names::BLOCK,
        "minecraft:block_entity_type" => registry_names::BLOCK_ENTITY_TYPE,
        "minecraft:block_transformer" => registry_names::BLOCK_TRANSFORMER,
        "minecraft:cat_sound_variant" => registry_names::CAT_SOUND_VARIANT,
        "minecraft:cat_variant" => registry_names::CAT_VARIANT,
        "minecraft:chicken_sound_variant" => registry_names::CHICKEN_SOUND_VARIANT,
        "minecraft:chicken_variant" => registry_names::CHICKEN_VARIANT,
        "minecraft:cow_sound_variant" => registry_names::COW_SOUND_VARIANT,
        "minecraft:cow_variant" => registry_names::COW_VARIANT,
        "minecraft:damage_type" => registry_names::DAMAGE_TYPE,
        "minecraft:data_component_predicate_type" => {
            registry_names::DATA_COMPONENT_PREDICATE_TYPE
        }
        "minecraft:data_component_type" => {
            mc_bamboo_data::component_registries::names::DATA_COMPONENT_TYPE
        }
        "minecraft:decorated_pot_pattern" => registry_names::DECORATED_POT_PATTERN,
        "minecraft:enchantment" => registry_names::ENCHANTMENT,
        "minecraft:entity_type" => registry_names::ENTITY_TYPE,
        "minecraft:frog_variant" => registry_names::FROG_VARIANT,
        "minecraft:instrument" => registry_names::INSTRUMENT,
        "minecraft:item" => registry_names::ITEM,
        "minecraft:jukebox_song" => registry_names::JUKEBOX_SONG,
        "minecraft:map_decoration_type" => registry_names::MAP_DECORATION_TYPE,
        "minecraft:mob_effect" => registry_names::MOB_EFFECT,
        "minecraft:painting_variant" => registry_names::PAINTING_VARIANT,
        "minecraft:pig_sound_variant" => registry_names::PIG_SOUND_VARIANT,
        "minecraft:pig_variant" => registry_names::PIG_VARIANT,
        "minecraft:potion" => registry_names::POTION,
        "minecraft:sound_event" => registry_names::SOUND_EVENT,
        "minecraft:trim_material" => registry_names::TRIM_MATERIAL,
        "minecraft:trim_pattern" => registry_names::TRIM_PATTERN,
        "minecraft:villager_type" => registry_names::VILLAGER_TYPE,
        "minecraft:wolf_sound_variant" => registry_names::WOLF_SOUND_VARIANT,
        "minecraft:wolf_variant" => registry_names::WOLF_VARIANT,
        "minecraft:zombie_nautilus_variant" => registry_names::ZOMBIE_NAUTILUS_VARIANT,
        _ => return None,
    })
}

/// 官方 `RegistryFixedCodec.decode`：只接受注册表条目名。
pub fn holder_fixed<T>(registry: Registry, tag: &Tag) -> Result<Holder<T>> {
    Ok(Holder::Id(registry_id(registry, tag)?))
}

/// 官方 `RegistryFileCodec.decode`：字符串按注册表查，其它标签走内联定义。
pub fn holder<T>(
    registry: Registry,
    tag: &Tag,
    inline: impl FnOnce(&Tag) -> Result<T>,
) -> Result<Holder<T>> {
    match tag {
        Tag::String(_) => holder_fixed(registry, tag),
        _ => inline(tag).map(Holder::Inline),
    }
}

/// 官方 `HolderSetCodec.decode`：`#` 开头的是标签，其余是直接列出的条目。
pub fn holder_set(registry: Registry, tag: &Tag) -> Result<HolderSet> {
    match tag {
        Tag::String(name) => match name.strip_prefix('#') {
            Some(name) => {
                let name = parse_identifier(name)
                    .map_err(|_| Error::new(format!("{name} is not a tag id")))?;
                if !has_tag(registry, &name) {
                    return Err(Error::new(format!("missing tag {name}")));
                }
                Ok(HolderSet::Tag(name))
            }
            None => Ok(HolderSet::Ids(vec![registry_id(registry, tag)?])),
        },
        _ => {
            let values = list(tag)?;
            let ids = values
                .iter()
                .map(|value| registry_id(registry, value))
                .collect::<Result<Vec<_>>>()?;
            Ok(HolderSet::Ids(ids))
        }
    }
}

/// 一个注册表里有没有这个标签。没有标签的注册表一律返回假。
pub fn has_tag(registry: Registry, name: &str) -> bool {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == registry.name)
        .is_some_and(|group| group.tags.iter().any(|tag| tag.name == name))
}
