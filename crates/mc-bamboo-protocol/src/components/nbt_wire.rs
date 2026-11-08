//! 没有 `networkSynchronized` 的五种组件：按官方 `Codec` 解码再编码。
//!
//! 官方 `DataComponentType.Builder.build` 对这几种用
//! `ByteBufCodecs.fromCodecWithRegistries`：先读 NBT（默认配额），再过 `Codec`，
//! 写回去的是 `Codec` 编出来的新标签，未知字段丢掉，默认值省略，复合标签的键序是
//! `HashMap` 的桶序。这里做同一件事。
//!
//! The five components without `networkSynchronized`.
//!
//! Official `DataComponentType.Builder.build` sends them through
//! `ByteBufCodecs.fromCodecWithRegistries`: read NBT with the default quota, run
//! the `Codec`, and write the tag the `Codec` produces. Unknown fields are
//! dropped, defaults are omitted, and compound key order is `HashMap` bucket
//! order. This module does the same thing.

use mc_bamboo_data::{blocks::BLOCKS, registries::data_component_type as kinds};
use mc_bamboo_nbt::Tag;

use super::{
    archive::{
        components::transcode_persistent,
        read::{self, Error},
        write,
    },
    cursor::Registry,
    java_hash::{hash_order, iteration_order},
};

const MAX_PREDICATE_DEPTH: u32 = 32;

/// 读到的标签换成官方 `Codec.encode` 会写出的那一份。
///
/// Replace a decoded tag with the one the official `Codec.encode` would write.
pub(crate) fn canonicalize(kind: i32, tag: &Tag) -> Result<Tag, Error> {
    let rewritten = match kind {
        kinds::LOCK => item_predicate(tag, 0)?,
        kinds::CONTAINER_LOOT => container_loot(tag)?,
        kinds::MAP_DECORATIONS => map_decorations(tag)?,
        kinds::DEBUG_STICK_STATE => debug_stick(tag)?,
        kinds::RECIPES => recipes(tag)?,
        _ => return Err(Error::new("component is not an NBT codec")),
    };
    Ok(rewritten)
}

/// 这一层的键按声明顺序放进新的 `HashMap`，再按桶序排好。嵌套标签已经排过，不再动。
///
/// Put this layer's keys into a fresh `HashMap` in declaration order, then emit
/// bucket order. Nested tags are already ordered and are left alone.
fn finish(fields: Vec<(String, Tag)>) -> Tag {
    let keys: Vec<String> = fields.iter().map(|(key, _)| key.clone()).collect();
    let order = iteration_order(&keys);
    Tag::Compound(
        order
            .into_iter()
            .map(|index| fields[index].clone())
            .collect(),
    )
}

/// 官方 `ItemPredicate.CODEC`，也是 `LockCode.CODEC`。
///
/// `ItemPredicate.CODEC`, which is also `LockCode.CODEC`.
fn item_predicate(tag: &Tag, depth: u32) -> Result<Tag, Error> {
    if depth > MAX_PREDICATE_DEPTH {
        return Err(Error::new("item predicate nested too deeply"));
    }
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(items) = read::field(entries, "items") {
        fields.push((
            "items".to_owned(),
            write::holder_set(Registry::ITEM, &read::holder_set(Registry::ITEM, items)?)?.to_tag(),
        ));
    }
    if let Some(count) = optional_int_bounds(read::field(entries, "count"))? {
        fields.push(("count".to_owned(), count));
    }
    if let Some(components) = read::field(entries, "components") {
        let encoded = exact_components(components)?;
        if !is_empty_compound(&encoded) {
            fields.push(("components".to_owned(), encoded));
        }
    }
    if let Some(predicates) = read::field(entries, "predicates") {
        let encoded = partial_predicates(predicates, depth)?;
        if !is_empty_compound(&encoded) {
            fields.push(("predicates".to_owned(), encoded));
        }
    }
    Ok(finish(fields))
}

fn container_loot(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let table =
        read::field(entries, "loot_table").ok_or_else(|| Error::new("loot table is missing"))?;
    let name = read::identifier(table)?;
    let mut fields = vec![("loot_table".to_owned(), Tag::String(name))];
    if let Some(seed) = read::field(entries, "seed") {
        let seed = read::long(seed)?;
        if seed != 0 {
            fields.push(("seed".to_owned(), Tag::Long(seed)));
        }
    }
    Ok(finish(fields))
}

fn map_decorations(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let entry = read::compound(value)?;
        let decoration = read::field(entry, "type")
            .ok_or_else(|| Error::new("map decoration type is missing"))?;
        let type_name = read::identifier(decoration)?;
        if read::name_id(map_decoration_registry(), &type_name).is_none() {
            return Err(Error::new(format!("unknown map decoration {type_name}")));
        }
        let x = read::field(entry, "x").ok_or_else(|| Error::new("map decoration x is missing"))?;
        let z = read::field(entry, "z").ok_or_else(|| Error::new("map decoration z is missing"))?;
        let rotation = read::field(entry, "rotation")
            .ok_or_else(|| Error::new("map decoration rotation is missing"))?;
        fields.push((
            key.clone(),
            finish(vec![
                ("type".to_owned(), Tag::String(type_name)),
                ("x".to_owned(), Tag::Double(read::double(x)?)),
                ("z".to_owned(), Tag::Double(read::double(z)?)),
                ("rotation".to_owned(), Tag::Float(read::float(rotation)?)),
            ]),
        ));
    }
    Ok(finish(fields))
}

fn debug_stick(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let block_name = read::identifier(&Tag::String(key.clone()))?;
        let block = read::name_id(Registry::BLOCK, &block_name)
            .ok_or_else(|| Error::new(format!("unknown block {block_name}")))?;
        let property = read::string(value)?;
        if !block_has_property(block, property) {
            return Err(Error::new(format!(
                "block {block_name} has no property {property}"
            )));
        }
        fields.push((block_name, Tag::String(property.to_owned())));
    }
    Ok(finish(fields))
}

fn recipes(tag: &Tag) -> Result<Tag, Error> {
    let values = read::list(tag)?;
    let mut ids = Vec::with_capacity(values.len());
    for value in values {
        ids.push(Tag::String(read::identifier(&value)?));
    }
    Ok(Tag::List(ids))
}

fn exact_components(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let name = read::identifier(&Tag::String(key.clone()))?;
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, &name)
            .ok_or_else(|| Error::new(format!("unknown component {name}")))?;
        let encoded = hash_order(transcode_persistent(kind, value)?);
        fields.push((name, encoded));
    }
    Ok(finish(fields))
}

fn partial_predicates(tag: &Tag, depth: u32) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let name = read::identifier(&Tag::String(key.clone()))?;
        let encoded = if read::name_id(Registry::DATA_COMPONENT_PREDICATE_TYPE, &name).is_some() {
            predicate_body(&name, value, depth)?
        } else if read::name_id(Registry::DATA_COMPONENT_TYPE, &name).is_some() {
            read::compound(value)?;
            Tag::Compound(Vec::new())
        } else {
            return Err(Error::new(format!("unknown predicate {name}")));
        };
        fields.push((name, encoded));
    }
    Ok(finish(fields))
}

fn predicate_body(name: &str, tag: &Tag, depth: u32) -> Result<Tag, Error> {
    match name {
        "minecraft:damage" => damage_predicate(tag),
        "minecraft:enchantments" | "minecraft:stored_enchantments" => enchantment_list(tag),
        "minecraft:potion_contents" => potion_predicate(tag),
        "minecraft:custom_data" => custom_data_predicate(tag),
        "minecraft:container" | "minecraft:bundle_contents" => {
            optional_items_predicate(tag, "items", depth)
        }
        "minecraft:firework_explosion" => firework_explosion_predicate(tag),
        "minecraft:fireworks" => fireworks_predicate(tag),
        "minecraft:writable_book_content" => writable_book_predicate(tag),
        "minecraft:written_book_content" => written_book_predicate(tag, depth),
        "minecraft:attribute_modifiers" => attribute_predicate(tag),
        "minecraft:trim" => trim_predicate(tag),
        "minecraft:jukebox_playable" => jukebox_predicate(tag),
        "minecraft:villager/variant" => holder_set_tag(Registry::VILLAGER_TYPE, tag),
        _ => Err(Error::new(format!("unknown predicate {name}"))),
    }
}

fn damage_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(value) = optional_int_bounds(read::field(entries, "durability"))? {
        fields.push(("durability".to_owned(), value));
    }
    if let Some(value) = optional_int_bounds(read::field(entries, "damage"))? {
        fields.push(("damage".to_owned(), value));
    }
    Ok(finish(fields))
}

fn enchantment_list(tag: &Tag) -> Result<Tag, Error> {
    let values = read::list(tag)?;
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        items.push(enchantment_predicate(&value)?);
    }
    Ok(Tag::List(items))
}

fn enchantment_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(enchantments) = read::field(entries, "enchantments") {
        fields.push((
            "enchantments".to_owned(),
            holder_set_tag(Registry::ENCHANTMENT, enchantments)?,
        ));
    }
    if let Some(levels) = optional_int_bounds(read::field(entries, "levels"))? {
        fields.push(("levels".to_owned(), levels));
    }
    Ok(finish(fields))
}

fn potion_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(potions) = read::field(entries, "potions") {
        fields.push((
            "potions".to_owned(),
            holder_set_tag(Registry::POTION, potions)?,
        ));
    }
    if let Some(effects) = read::field(entries, "effects") {
        fields.push(("effects".to_owned(), effect_collection(effects)?));
    }
    Ok(finish(fields))
}

fn effect_collection(tag: &Tag) -> Result<Tag, Error> {
    collection(tag, effect_predicate)
}

fn effect_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    for (key, value) in entries {
        let effect = read::identifier(&Tag::String(key.clone()))?;
        if read::name_id(Registry::MOB_EFFECT, &effect).is_none() {
            return Err(Error::new(format!("unknown effect {effect}")));
        }
        fields.push((effect, effect_instance(value)?));
    }
    Ok(finish(fields))
}

fn effect_instance(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(value) = optional_int_bounds(read::field(entries, "amplifier"))? {
        fields.push(("amplifier".to_owned(), value));
    }
    if let Some(value) = optional_int_bounds(read::field(entries, "duration"))? {
        fields.push(("duration".to_owned(), value));
    }
    if let Some(value) = optional_bool(read::field(entries, "ambient"))? {
        fields.push(("ambient".to_owned(), value));
    }
    if let Some(value) = optional_bool(read::field(entries, "visible"))? {
        fields.push(("visible".to_owned(), value));
    }
    Ok(finish(fields))
}

fn custom_data_predicate(tag: &Tag) -> Result<Tag, Error> {
    let compound = match tag {
        Tag::String(text) => parse_snbt_compound(text)?,
        other => read::compound(other)?.to_vec(),
    };
    // 官方编码走 `CompoundTag.toString()`，键序是这张 `HashMap` 的遍历顺序。
    // Official encoding uses `CompoundTag.toString()`, in this `HashMap`'s order.
    Ok(Tag::String(hash_order(Tag::Compound(compound)).to_snbt()))
}

fn optional_items_predicate(tag: &Tag, name: &str, depth: u32) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(items) = read::field(entries, name) {
        fields.push((
            name.to_owned(),
            collection_with(items, |item| item_predicate(item, depth + 1))?,
        ));
    }
    Ok(finish(fields))
}

fn firework_explosion_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(shape) = read::field(entries, "shape") {
        fields.push(("shape".to_owned(), enum_name(shape, FIREWORK_SHAPES)?));
    }
    if let Some(value) = optional_bool(read::field(entries, "has_twinkle"))? {
        fields.push(("has_twinkle".to_owned(), value));
    }
    if let Some(value) = optional_bool(read::field(entries, "has_trail"))? {
        fields.push(("has_trail".to_owned(), value));
    }
    Ok(finish(fields))
}

fn fireworks_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(explosions) = read::field(entries, "explosions") {
        fields.push((
            "explosions".to_owned(),
            collection(explosions, firework_explosion_predicate)?,
        ));
    }
    if let Some(duration) = optional_int_bounds(read::field(entries, "flight_duration"))? {
        fields.push(("flight_duration".to_owned(), duration));
    }
    Ok(finish(fields))
}

fn writable_book_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(pages) = read::field(entries, "pages") {
        fields.push(("pages".to_owned(), collection(pages, page_string)?));
    }
    Ok(finish(fields))
}

fn page_string(tag: &Tag) -> Result<Tag, Error> {
    Ok(Tag::String(read::string(tag)?.to_owned()))
}

fn written_book_predicate(tag: &Tag, depth: u32) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(pages) = read::field(entries, "pages") {
        fields.push((
            "pages".to_owned(),
            collection_with(pages, |page| written_page(page, depth))?,
        ));
    }
    if let Some(author) = read::field(entries, "author") {
        fields.push((
            "author".to_owned(),
            Tag::String(read::string(author)?.to_owned()),
        ));
    }
    if let Some(title) = read::field(entries, "title") {
        fields.push((
            "title".to_owned(),
            Tag::String(read::string(title)?.to_owned()),
        ));
    }
    if let Some(generation) = optional_int_bounds(read::field(entries, "generation"))? {
        fields.push(("generation".to_owned(), generation));
    }
    if let Some(resolved) = optional_bool(read::field(entries, "resolved"))? {
        fields.push(("resolved".to_owned(), resolved));
    }
    Ok(finish(fields))
}

fn written_page(tag: &Tag, depth: u32) -> Result<Tag, Error> {
    // 页的内容是文本组件。这里只确认它是组件 `Codec` 肯收的标签，原样保留给外层重排。
    // A page is a text component. Confirm the text codec accepts it; the outer
    // pass reorders the tag.
    if depth > MAX_PREDICATE_DEPTH {
        return Err(Error::new("written page nested too deeply"));
    }
    super::text::accept_tag(tag).map_err(|_| Error::new("written book page is not text"))?;
    Ok(tag.clone())
}

fn attribute_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(modifiers) = read::field(entries, "modifiers") {
        fields.push((
            "modifiers".to_owned(),
            collection(modifiers, attribute_entry)?,
        ));
    }
    Ok(finish(fields))
}

fn attribute_entry(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(attribute) = read::field(entries, "attribute") {
        fields.push((
            "attribute".to_owned(),
            holder_set_tag(Registry::ATTRIBUTE, attribute)?,
        ));
    }
    if let Some(id) = read::field(entries, "id") {
        fields.push(("id".to_owned(), Tag::String(read::identifier(id)?)));
    }
    if let Some(amount) = optional_double_bounds(read::field(entries, "amount"))? {
        fields.push(("amount".to_owned(), amount));
    }
    if let Some(operation) = read::field(entries, "operation") {
        fields.push((
            "operation".to_owned(),
            enum_name(operation, ATTRIBUTE_OPERATIONS)?,
        ));
    }
    if let Some(slot) = read::field(entries, "slot") {
        fields.push(("slot".to_owned(), enum_name(slot, EQUIPMENT_SLOTS)?));
    }
    Ok(finish(fields))
}

fn trim_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(material) = read::field(entries, "material") {
        fields.push((
            "material".to_owned(),
            holder_set_tag(Registry::TRIM_MATERIAL, material)?,
        ));
    }
    if let Some(pattern) = read::field(entries, "pattern") {
        fields.push((
            "pattern".to_owned(),
            holder_set_tag(Registry::TRIM_PATTERN, pattern)?,
        ));
    }
    Ok(finish(fields))
}

fn jukebox_predicate(tag: &Tag) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(song) = read::field(entries, "song") {
        fields.push((
            "song".to_owned(),
            holder_set_tag(Registry::JUKEBOX_SONG, song)?,
        ));
    }
    Ok(finish(fields))
}

fn collection(tag: &Tag, element: fn(&Tag) -> Result<Tag, Error>) -> Result<Tag, Error> {
    collection_with(tag, element)
}

fn collection_with(tag: &Tag, element: impl Fn(&Tag) -> Result<Tag, Error>) -> Result<Tag, Error> {
    let entries = read::compound(tag)?;
    let mut fields = Vec::new();
    if let Some(contains) = read::field(entries, "contains") {
        fields.push(("contains".to_owned(), map_list(contains, &element)?));
    }
    if let Some(count) = read::field(entries, "count") {
        let values = read::list(count)?;
        let mut items = Vec::with_capacity(values.len());
        for value in values {
            let test_entries = read::compound(&value)?;
            let test = read::field(test_entries, "test")
                .ok_or_else(|| Error::new("count predicate is missing test"))?;
            let count = read::field(test_entries, "count")
                .ok_or_else(|| Error::new("count predicate is missing count"))?;
            items.push(finish(vec![
                ("test".to_owned(), element(test)?),
                ("count".to_owned(), required_int_bounds(count)?),
            ]));
        }
        fields.push(("count".to_owned(), Tag::List(items)));
    }
    if let Some(size) = optional_int_bounds(read::field(entries, "size"))? {
        fields.push(("size".to_owned(), size));
    }
    Ok(finish(fields))
}

fn map_list(tag: &Tag, element: &impl Fn(&Tag) -> Result<Tag, Error>) -> Result<Tag, Error> {
    let values = read::list(tag)?;
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        items.push(element(&value)?);
    }
    Ok(Tag::List(items))
}

fn holder_set_tag(registry: Registry, tag: &Tag) -> Result<Tag, Error> {
    Ok(write::holder_set(registry, &read::holder_set(registry, tag)?)?.to_tag())
}

fn optional_bool(tag: Option<&Tag>) -> Result<Option<Tag>, Error> {
    match tag {
        None => Ok(None),
        Some(tag) => Ok(Some(Tag::Byte(i8::from(read::bool_value(tag)?)))),
    }
}

fn optional_int_bounds(tag: Option<&Tag>) -> Result<Option<Tag>, Error> {
    match tag {
        None => Ok(None),
        Some(tag) => {
            let bounds = required_int_bounds(tag)?;
            if is_empty_compound(&bounds) {
                Ok(None)
            } else {
                Ok(Some(bounds))
            }
        }
    }
}

fn optional_double_bounds(tag: Option<&Tag>) -> Result<Option<Tag>, Error> {
    match tag {
        None => Ok(None),
        Some(tag) => {
            let bounds = required_double_bounds(tag)?;
            if is_empty_compound(&bounds) {
                Ok(None)
            } else {
                Ok(Some(bounds))
            }
        }
    }
}

fn required_int_bounds(tag: &Tag) -> Result<Tag, Error> {
    bounds_tag(tag, true)
}

fn required_double_bounds(tag: &Tag) -> Result<Tag, Error> {
    bounds_tag(tag, false)
}

/// 官方 `MinMaxBounds.Bounds.createCodec`。两端相同写成裸数字，都空写成 `{}`。
///
/// `MinMaxBounds.Bounds.createCodec`. A point is a bare number. Both ends empty is `{}`.
fn bounds_tag(tag: &Tag, integer: bool) -> Result<Tag, Error> {
    if let Tag::Compound(entries) = tag {
        let min = read::field(entries, "min");
        let max = read::field(entries, "max");
        let min_number = min.map(read::double).transpose()?;
        let max_number = max.map(read::double).transpose()?;
        if let (Some(min), Some(max)) = (min_number, max_number)
            && min > max
        {
            return Err(Error::new("range bounds are swapped"));
        }
        if min_number.is_none() && max_number.is_none() {
            return Ok(Tag::Compound(Vec::new()));
        }
        if min_number == max_number {
            return Ok(number_tag(min_number.unwrap_or(0.0), integer));
        }
        let mut fields = Vec::new();
        if let Some(min) = min_number {
            fields.push(("min".to_owned(), number_tag(min, integer)));
        }
        if let Some(max) = max_number {
            fields.push(("max".to_owned(), number_tag(max, integer)));
        }
        return Ok(finish(fields));
    }
    let point = read::double(tag)?;
    Ok(number_tag(point, integer))
}

fn number_tag(value: f64, integer: bool) -> Tag {
    if integer {
        Tag::Int(java_i32_from_f64(value))
    } else {
        Tag::Double(value)
    }
}

fn java_i32_from_f64(value: f64) -> i32 {
    // 官方 `Number.intValue()`：浮点先收成 `int` 能表示的范围，再截断。
    // `Number.intValue()`: clamp a float into the `int` range, then truncate.
    if value.is_nan() {
        return 0;
    }
    if value >= f64::from(i32::MAX) {
        return i32::MAX;
    }
    if value <= f64::from(i32::MIN) {
        return i32::MIN;
    }
    value as i32
}

fn enum_name(tag: &Tag, names: &[&str]) -> Result<Tag, Error> {
    let name = read::string(tag)?;
    if names.contains(&name) {
        Ok(Tag::String(name.to_owned()))
    } else {
        Err(Error::new(format!("unknown name {name}")))
    }
}

fn is_empty_compound(tag: &Tag) -> bool {
    matches!(tag, Tag::Compound(entries) if entries.is_empty())
}

fn block_has_property(block: i32, property: &str) -> bool {
    usize::try_from(block)
        .ok()
        .and_then(|index| BLOCKS.get(index))
        .is_some_and(|block| {
            block
                .properties
                .iter()
                .any(|defined| defined.name == property)
        })
}

fn map_decoration_registry() -> Registry {
    Registry {
        name: "minecraft:map_decoration_type",
        size: mc_bamboo_data::component_registries::sizes::MAP_DECORATION_TYPE,
    }
}

fn parse_snbt_compound(text: &str) -> Result<Vec<(String, Tag)>, Error> {
    match mc_bamboo_nbt::parse_snbt(text) {
        Ok(Tag::Compound(entries)) => Ok(entries),
        Ok(_) => Err(Error::new("nbt predicate must be a compound")),
        Err(_) => Err(Error::new("nbt predicate is not valid snbt")),
    }
}

const FIREWORK_SHAPES: &[&str] = &["small_ball", "large_ball", "star", "creeper", "burst"];
const ATTRIBUTE_OPERATIONS: &[&str] = &["add_value", "add_multiplied_base", "add_multiplied_total"];
const EQUIPMENT_SLOTS: &[&str] = &[
    "any", "mainhand", "offhand", "hand", "feet", "legs", "chest", "head", "armor", "body",
    "saddle",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: &[(&str, Tag)]) -> Tag {
        Tag::Compound(
            entries
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn an_empty_lock_stays_empty_and_unknown_fields_are_dropped() {
        let tag = canonicalize(kinds::LOCK, &compound(&[("note", Tag::String("x".into()))]))
            .expect("empty predicate");
        assert_eq!(tag, Tag::Compound(Vec::new()));
    }

    #[test]
    fn a_point_count_is_an_int_and_keys_follow_hash_map_order() {
        let tag = canonicalize(
            kinds::LOCK,
            &compound(&[
                ("items", Tag::String("minecraft:tripwire_hook".into())),
                ("count", Tag::Int(1)),
            ]),
        )
        .expect("lock");
        assert_eq!(
            tag,
            compound(&[
                ("count", Tag::Int(1)),
                ("items", Tag::String("minecraft:tripwire_hook".into())),
            ])
        );
    }
}
