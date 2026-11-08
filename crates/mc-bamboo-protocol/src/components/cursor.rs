//! 组件编解码共用的读写原语：注册表 ID、`Holder`、`HolderSet`、列表、NBT。
//!
//! 每个函数对应官方 `ByteBufCodecs` 里的一个工厂方法，上限也照官方。

use mc_bamboo_data::{component_registries::sizes, tags};
use mc_bamboo_nbt::{Accounter, Tag};
use uuid::Uuid;

use crate::codec::{DecodeError, Reader, write_bool, write_string, write_varint};

/// 嵌套物品堆的层数上限。
///
/// 官方不限制，靠数据包大小兜着：收纳袋里能放收纳袋，容器里能放收纳袋，每层只要几个字节。
/// 官方在 Java 里递归，几万层会抛 `StackOverflowError`；Rust 会直接崩，所以这里加了上限。
/// 原版玩法最深的组合（潜影盒里的收纳袋里的收纳袋……）远在这个数以内。
pub const MAX_STACK_DEPTH: u32 = 16;

/// 官方 `ByteBufCodecs.STRING_UTF8` 的长度上限，按 UTF-16 码元数。
pub const MAX_STRING: usize = 32767;

/// 一个注册表：`name` 用来查标签，`size` 用来校验数字 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registry {
    pub name: &'static str,
    pub size: u32,
}

impl Registry {
    const fn new(name: &'static str, size: u32) -> Self {
        Self { name, size }
    }

    pub const ATTRIBUTE: Self = Self::new(
        "minecraft:attribute",
        mc_bamboo_data::registries::attribute::TOTAL,
    );
    pub const BANNER_PATTERN: Self = Self::new("minecraft:banner_pattern", sizes::BANNER_PATTERN);
    pub const BLOCK: Self = Self::new("minecraft:block", sizes::BLOCK);
    pub const BLOCK_ENTITY_TYPE: Self =
        Self::new("minecraft:block_entity_type", sizes::BLOCK_ENTITY_TYPE);
    pub const BLOCK_TRANSFORMER: Self =
        Self::new("minecraft:block_transformer", sizes::BLOCK_TRANSFORMER);
    pub const CAT_SOUND_VARIANT: Self =
        Self::new("minecraft:cat_sound_variant", sizes::CAT_SOUND_VARIANT);
    pub const CAT_VARIANT: Self = Self::new("minecraft:cat_variant", sizes::CAT_VARIANT);
    pub const CHICKEN_SOUND_VARIANT: Self = Self::new(
        "minecraft:chicken_sound_variant",
        sizes::CHICKEN_SOUND_VARIANT,
    );
    pub const CHICKEN_VARIANT: Self =
        Self::new("minecraft:chicken_variant", sizes::CHICKEN_VARIANT);
    pub const CONSUME_EFFECT_TYPE: Self =
        Self::new("minecraft:consume_effect_type", sizes::CONSUME_EFFECT_TYPE);
    pub const COW_SOUND_VARIANT: Self =
        Self::new("minecraft:cow_sound_variant", sizes::COW_SOUND_VARIANT);
    pub const COW_VARIANT: Self = Self::new("minecraft:cow_variant", sizes::COW_VARIANT);
    pub const DAMAGE_TYPE: Self = Self::new("minecraft:damage_type", sizes::DAMAGE_TYPE);
    pub const DATA_COMPONENT_PREDICATE_TYPE: Self = Self::new(
        "minecraft:data_component_predicate_type",
        sizes::DATA_COMPONENT_PREDICATE_TYPE,
    );
    pub const DATA_COMPONENT_TYPE: Self =
        Self::new("minecraft:data_component_type", sizes::DATA_COMPONENT_TYPE);
    pub const DECORATED_POT_PATTERN: Self = Self::new(
        "minecraft:decorated_pot_pattern",
        sizes::DECORATED_POT_PATTERN,
    );
    pub const ENCHANTMENT: Self = Self::new("minecraft:enchantment", sizes::ENCHANTMENT);
    pub const ENTITY_TYPE: Self = Self::new("minecraft:entity_type", sizes::ENTITY_TYPE);
    pub const FROG_VARIANT: Self = Self::new("minecraft:frog_variant", sizes::FROG_VARIANT);
    pub const INSTRUMENT: Self = Self::new("minecraft:instrument", sizes::INSTRUMENT);
    pub const ITEM: Self = Self::new("minecraft:item", sizes::ITEM);
    pub const JUKEBOX_SONG: Self = Self::new("minecraft:jukebox_song", sizes::JUKEBOX_SONG);
    pub const MOB_EFFECT: Self = Self::new("minecraft:mob_effect", sizes::MOB_EFFECT);
    pub const PAINTING_VARIANT: Self =
        Self::new("minecraft:painting_variant", sizes::PAINTING_VARIANT);
    pub const PIG_SOUND_VARIANT: Self =
        Self::new("minecraft:pig_sound_variant", sizes::PIG_SOUND_VARIANT);
    pub const PIG_VARIANT: Self = Self::new("minecraft:pig_variant", sizes::PIG_VARIANT);
    pub const POTION: Self = Self::new("minecraft:potion", sizes::POTION);
    pub const SOUND_EVENT: Self = Self::new("minecraft:sound_event", sizes::SOUND_EVENT);
    pub const TRIM_MATERIAL: Self = Self::new("minecraft:trim_material", sizes::TRIM_MATERIAL);
    pub const TRIM_PATTERN: Self = Self::new("minecraft:trim_pattern", sizes::TRIM_PATTERN);
    pub const VILLAGER_TYPE: Self = Self::new("minecraft:villager_type", sizes::VILLAGER_TYPE);
    pub const WOLF_SOUND_VARIANT: Self =
        Self::new("minecraft:wolf_sound_variant", sizes::WOLF_SOUND_VARIANT);
    pub const WOLF_VARIANT: Self = Self::new("minecraft:wolf_variant", sizes::WOLF_VARIANT);
    pub const ZOMBIE_NAUTILUS_VARIANT: Self = Self::new(
        "minecraft:zombie_nautilus_variant",
        sizes::ZOMBIE_NAUTILUS_VARIANT,
    );

    /// 这个注册表里有没有叫 `name` 的标签。没有标签的注册表一律返回 `false`。
    fn has_tag(self, name: &str) -> bool {
        tags::TAG_GROUPS
            .iter()
            .find(|group| group.registry == self.name)
            .is_some_and(|group| group.tags.iter().any(|tag| tag.name == name))
    }
}

/// 官方的 `Holder`：注册表里的条目，或者一份内联的定义。
///
/// 只有能内联的注册表才用它（音效、乐器、唱片、纹饰材料和图案、旗帜图案、画）。
/// 其他注册表官方用 `holderRegistry`，线上只有一个数字 ID，这里直接存 `i32`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder<T> {
    /// 注册表 ID。线上写的是 ID 加一。
    Id(i32),
    /// 内联定义。线上前面是一个 0。
    Inline(T),
}

/// 官方的 `HolderSet`：一个标签名，或者一串注册表 ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderSet {
    /// 标签名，已经确认这个注册表里有它。
    Tag(String),
    /// 直接列出的注册表 ID。
    Ids(Vec<i32>),
}

/// 带嵌套深度的读取游标，包着普通的 [`Reader`]。
#[derive(Debug)]
pub struct Cursor<'a> {
    reader: Reader<'a>,
    depth: u32,
}

impl<'a> Cursor<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            reader: Reader::new(input),
            depth: MAX_STACK_DEPTH,
        }
    }

    /// 结束解码，还有剩余字节时报错。
    pub fn finish(self) -> Result<(), DecodeError> {
        self.reader.finish()
    }

    /// 还没读的字节，不移动游标。
    pub const fn remaining(&self) -> &'a [u8] {
        self.reader.remaining()
    }

    /// 往下一层嵌套物品堆。超过上限时报错。
    pub fn nested<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<T, DecodeError> {
        if self.depth == 0 {
            return Err(DecodeError::StacksTooDeep {
                max_depth: MAX_STACK_DEPTH,
            });
        }
        self.depth -= 1;
        let result = read(self);
        self.depth += 1;
        result
    }

    pub fn varint(&mut self) -> Result<i32, DecodeError> {
        self.reader.varint()
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        self.reader.bool()
    }

    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        self.reader.i32()
    }

    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        self.reader.i64()
    }

    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        self.reader.f32()
    }

    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        self.reader.f64()
    }

    pub fn uuid(&mut self) -> Result<Uuid, DecodeError> {
        self.reader.uuid()
    }

    /// 取走接下来的 `len` 个字节。
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        self.reader.bytes(len)
    }

    /// 读列表的元素个数，限制在 `max` 以内。
    pub fn count(&mut self, max: usize) -> Result<usize, DecodeError> {
        self.reader.count(max)
    }

    /// 官方 `ByteBufCodecs.stringUtf8(max)`。
    pub fn string(&mut self, max: usize) -> Result<String, DecodeError> {
        self.reader.string(max).map(str::to_owned)
    }

    /// 官方 `Identifier.STREAM_CODEC`：一个 32767 长的字符串，按 `Identifier.parse` 解析。
    ///
    /// 返回规范形式 `命名空间:路径`。官方也会把 `stone` 这样的输入补成 `minecraft:stone`，
    /// 再写回去时就变了，所以这里也规范化。
    pub fn identifier(&mut self) -> Result<String, DecodeError> {
        let raw = self.reader.string(MAX_STRING)?;
        parse_identifier(raw)
    }

    /// 官方 `ByteBufCodecs.registry` 和 `holderRegistry`：一个数字 ID。
    pub fn registry_id(&mut self, registry: Registry) -> Result<i32, DecodeError> {
        let id = self.varint()?;
        if !u32::try_from(id).is_ok_and(|id| id < registry.size) {
            return Err(DecodeError::UnknownRegistryId {
                registry: registry.name,
                id,
            });
        }
        Ok(id)
    }

    /// 官方 `ByteBufCodecs.holder`：0 后面跟内联定义，否则是 ID 加一。
    pub fn holder<T>(
        &mut self,
        registry: Registry,
        inline: impl FnOnce(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Holder<T>, DecodeError> {
        let tag = self.varint()?;
        if tag == 0 {
            return inline(self).map(Holder::Inline);
        }
        let id = tag - 1;
        if !u32::try_from(id).is_ok_and(|id| id < registry.size) {
            return Err(DecodeError::UnknownRegistryId {
                registry: registry.name,
                id,
            });
        }
        Ok(Holder::Id(id))
    }

    /// 官方 `ByteBufCodecs.holderSet`：0 后面是标签名，否则是个数加一再跟 ID。
    pub fn holder_set(&mut self, registry: Registry) -> Result<HolderSet, DecodeError> {
        let count = self.varint()?.wrapping_sub(1);
        if count == -1 {
            let name = self.identifier()?;
            if !registry.has_tag(&name) {
                return Err(DecodeError::UnknownTag {
                    registry: registry.name,
                });
            }
            return Ok(HolderSet::Tag(name));
        }
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidCount(count))?;
        let mut ids = Vec::new();
        for _ in 0..count {
            ids.push(self.registry_id(registry)?);
        }
        Ok(HolderSet::Ids(ids))
    }

    /// 官方 `ByteBufCodecs.optional`：一个布尔加上内容。
    pub fn optional<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Option<T>, DecodeError> {
        if !self.bool()? {
            return Ok(None);
        }
        read(self).map(Some)
    }

    /// 官方 `ByteBufCodecs.either`：`true` 是左边，`false` 是右边。
    pub fn either<L, R>(
        &mut self,
        left: impl FnOnce(&mut Self) -> Result<L, DecodeError>,
        right: impl FnOnce(&mut Self) -> Result<R, DecodeError>,
    ) -> Result<Result<L, R>, DecodeError> {
        if self.bool()? {
            left(self).map(Ok)
        } else {
            right(self).map(Err)
        }
    }

    /// 官方 `ByteBufCodecs.list(max)`：个数加元素。
    ///
    /// 官方不预先分配，超长的个数要靠后面的字节不够用才报错，这里也一样。
    pub fn list<T>(
        &mut self,
        max: usize,
        mut read: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let count = self.count(max)?;
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(read(self)?);
        }
        Ok(values)
    }

    /// 官方 `ByteBufCodecs.fixedSizeList(size)`：没有个数前缀，固定读这么多个。
    pub fn fixed_list<T>(
        &mut self,
        size: usize,
        mut read: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let mut values = Vec::with_capacity(size);
        for _ in 0..size {
            values.push(read(self)?);
        }
        Ok(values)
    }

    /// 官方 `ByteBufCodecs.map(max)`。按线上的顺序存，重复的键不合并。
    pub fn map<K, V>(
        &mut self,
        max: usize,
        mut key: impl FnMut(&mut Self) -> Result<K, DecodeError>,
        mut value: impl FnMut(&mut Self) -> Result<V, DecodeError>,
    ) -> Result<Vec<(K, V)>, DecodeError> {
        let count = self.count(max)?;
        let mut entries = Vec::new();
        for _ in 0..count {
            let key = key(self)?;
            entries.push((key, value(self)?));
        }
        Ok(entries)
    }

    /// 官方 `ByteBufCodecs.TAG`：读一个 NBT 标签，默认配额，`TAG_End` 算错误。
    pub fn nbt_tag(&mut self) -> Result<Tag, DecodeError> {
        let mut nbt =
            mc_bamboo_nbt::Reader::new(self.reader.remaining(), Accounter::default_quota());
        let tag = nbt.tag()?;
        self.reader.bytes(nbt.read())?;
        Ok(tag)
    }

    /// 官方 `ByteBufCodecs.COMPOUND_TAG`：读一个 `TAG_Compound`，默认配额。
    pub fn nbt_compound(&mut self) -> Result<Vec<(String, Tag)>, DecodeError> {
        let mut nbt =
            mc_bamboo_nbt::Reader::new(self.reader.remaining(), Accounter::default_quota());
        let entries = nbt.compound()?;
        self.reader.bytes(nbt.read())?;
        Ok(entries)
    }
}

/// 官方 `Identifier.parse`：第一个冒号前是命名空间，没有冒号时补 `minecraft`。
pub fn parse_identifier(raw: &str) -> Result<String, DecodeError> {
    let (namespace, path) = match raw.find(':') {
        Some(0) => ("minecraft", &raw[1..]),
        Some(index) => (&raw[..index], &raw[index + 1..]),
        None => ("minecraft", raw),
    };
    if !valid_path(path) || !valid_namespace(namespace) {
        return Err(DecodeError::InvalidIdentifier);
    }
    Ok(format!("{namespace}:{path}"))
}

/// 官方 `Identifier.validPathChar`。
fn valid_path(path: &str) -> bool {
    path.chars()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '/' | '.'))
}

/// 官方 `Identifier.isValidNamespace`，连 `..` 的特例一起。
fn valid_namespace(namespace: &str) -> bool {
    namespace != ".."
        && namespace
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.'))
}

/// 按 ID 查表的枚举超出范围时怎么折回，官方的 `ByIdMap.OutOfBoundsStrategy`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutOfBounds {
    Zero,
    Clamp,
    Wrap,
}

/// 官方 `ByteBufCodecs.idMapper(ByIdMap.continuous(..))`：把读到的 ID 折回 `0..count`。
pub fn continuous_id(id: i32, count: i32, strategy: OutOfBounds) -> i32 {
    if (0..count).contains(&id) {
        return id;
    }
    match strategy {
        OutOfBounds::Zero => 0,
        OutOfBounds::Clamp => id.clamp(0, count - 1),
        OutOfBounds::Wrap => id.rem_euclid(count),
    }
}

/// 官方 `ByteBufCodecs.idMapper(ByIdMap.sparse(..))`：ID 不在表里就用默认值。
pub fn sparse_id(id: i32, ids: &[i32], default: i32) -> i32 {
    if ids.contains(&id) { id } else { default }
}

/// 把 `Identifier` 写成带长度前缀的字符串。
pub fn write_identifier(out: &mut Vec<u8>, value: &str) {
    write_string(out, value);
}

/// 官方 `ByteBufCodecs.optional` 的写入。
pub fn write_optional<T>(
    out: &mut Vec<u8>,
    value: Option<&T>,
    write: impl FnOnce(&mut Vec<u8>, &T),
) {
    match value {
        Some(value) => {
            write_bool(out, true);
            write(out, value);
        }
        None => write_bool(out, false),
    }
}

/// 官方 `ByteBufCodecs.list` 的写入。
pub fn write_list<T>(out: &mut Vec<u8>, values: &[T], mut write: impl FnMut(&mut Vec<u8>, &T)) {
    write_varint(out, values.len() as i32);
    for value in values {
        write(out, value);
    }
}

/// `Holder` 的写入，内联定义前面是一个 0。
pub fn write_holder<T>(
    out: &mut Vec<u8>,
    holder: &Holder<T>,
    write: impl FnOnce(&mut Vec<u8>, &T),
) {
    match holder {
        Holder::Id(id) => write_varint(out, id + 1),
        Holder::Inline(value) => {
            write_varint(out, 0);
            write(out, value);
        }
    }
}

/// `HolderSet` 的写入。
pub fn write_holder_set(out: &mut Vec<u8>, set: &HolderSet) {
    match set {
        HolderSet::Tag(name) => {
            write_varint(out, 0);
            write_identifier(out, name);
        }
        HolderSet::Ids(ids) => {
            write_varint(out, ids.len() as i32 + 1);
            for &id in ids {
                write_varint(out, id);
            }
        }
    }
}

/// NBT 标签的写入，和官方 `FriendlyByteBuf.writeNbt` 一样带类型字节。
pub fn write_nbt_tag(out: &mut Vec<u8>, tag: &Tag) {
    tag.write_network(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_normalised_like_the_game() {
        assert_eq!(parse_identifier("stone").as_deref(), Ok("minecraft:stone"));
        assert_eq!(
            parse_identifier("minecraft:stone").as_deref(),
            Ok("minecraft:stone")
        );
        assert_eq!(parse_identifier(":stone").as_deref(), Ok("minecraft:stone"));
        assert_eq!(
            parse_identifier("a_b-c.d:e/f").as_deref(),
            Ok("a_b-c.d:e/f")
        );
        assert_eq!(parse_identifier("").as_deref(), Ok("minecraft:"));
    }

    #[test]
    fn identifiers_reject_what_the_game_rejects() {
        for raw in ["Stone", "a:B", "a b", "a:b:c", "a/b:c", "..:c"] {
            assert_eq!(
                parse_identifier(raw),
                Err(DecodeError::InvalidIdentifier),
                "{raw}"
            );
        }
    }

    #[test]
    fn out_of_range_ids_follow_the_official_strategy() {
        assert_eq!(continuous_id(2, 4, OutOfBounds::Zero), 2);
        assert_eq!(continuous_id(9, 4, OutOfBounds::Zero), 0);
        assert_eq!(continuous_id(-1, 4, OutOfBounds::Zero), 0);
        assert_eq!(continuous_id(9, 4, OutOfBounds::Clamp), 3);
        assert_eq!(continuous_id(-5, 4, OutOfBounds::Clamp), 0);
        assert_eq!(continuous_id(9, 4, OutOfBounds::Wrap), 1);
        assert_eq!(continuous_id(-1, 4, OutOfBounds::Wrap), 3);
        assert_eq!(sparse_id(99, &[0, 99], 0), 99);
        assert_eq!(sparse_id(5, &[0, 99], 0), 0);
    }

    #[test]
    fn registry_tags_come_from_the_generated_tables() {
        assert!(Registry::BLOCK.has_tag("minecraft:replaceable"));
        assert!(!Registry::BLOCK.has_tag("minecraft:no_such_tag"));
        // 这个版本的药水效果注册表没有标签，任何标签名都不该通过。
        assert!(!Registry::MOB_EFFECT.has_tag("minecraft:anything"));
    }

    #[test]
    fn holder_sets_round_trip() {
        let mut out = Vec::new();
        write_holder_set(&mut out, &HolderSet::Ids(vec![1, 2]));
        assert_eq!(
            Cursor::new(&out).holder_set(Registry::BLOCK),
            Ok(HolderSet::Ids(vec![1, 2]))
        );

        let mut out = Vec::new();
        write_holder_set(
            &mut out,
            &HolderSet::Tag("minecraft:replaceable".to_owned()),
        );
        assert_eq!(
            Cursor::new(&out).holder_set(Registry::BLOCK),
            Ok(HolderSet::Tag("minecraft:replaceable".to_owned()))
        );
    }

    #[test]
    fn holder_sets_reject_a_bad_count_and_an_unknown_tag() {
        let mut out = Vec::new();
        write_varint(&mut out, 0);
        write_string(&mut out, "minecraft:no_such_tag");
        assert_eq!(
            Cursor::new(&out).holder_set(Registry::BLOCK),
            Err(DecodeError::UnknownTag {
                registry: "minecraft:block"
            })
        );

        let mut out = Vec::new();
        write_varint(&mut out, -1);
        assert_eq!(
            Cursor::new(&out).holder_set(Registry::BLOCK),
            Err(DecodeError::InvalidCount(-2))
        );
    }

    #[test]
    fn registry_ids_are_checked_against_the_registry_size() {
        let mut out = Vec::new();
        write_varint(&mut out, sizes::BLOCK as i32);
        assert_eq!(
            Cursor::new(&out).registry_id(Registry::BLOCK),
            Err(DecodeError::UnknownRegistryId {
                registry: "minecraft:block",
                id: sizes::BLOCK as i32,
            })
        );
    }

    #[test]
    fn nbt_tags_advance_the_cursor_by_exactly_their_size() {
        let mut out = Vec::new();
        write_nbt_tag(&mut out, &Tag::Byte(7));
        out.push(0xaa);

        let mut cursor = Cursor::new(&out);
        assert_eq!(cursor.nbt_tag(), Ok(Tag::Byte(7)));
        assert_eq!(cursor.remaining().len(), 1);
    }

    #[test]
    fn holders_write_the_id_plus_one() {
        let mut out = Vec::new();
        write_holder(&mut out, &Holder::<()>::Id(0), |_, ()| {});
        assert_eq!(out, [1]);

        let mut out = Vec::new();
        write_holder(&mut out, &Holder::Inline(7u8), |out, value| {
            out.push(*value);
        });
        assert_eq!(out, [0, 7]);
    }
}
