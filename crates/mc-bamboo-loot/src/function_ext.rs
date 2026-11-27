//! 会改物品组件的战利品函数，对应官方 `loot/functions/` 里的那些类。
//!
//! 和 crate 里其它函数一样，这里只记下「要往哪个组件写什么」的 JSON：组件值原样
//! 保存成 JSON，真正的组件字节由协议层负责。`update` 类的函数先读已经写在补丁里
//! 的组件（没有就用官方 codec 的缺省值），再按官方逻辑算新值写回去，这样多个函数
//! 串起来时行为也一致。需要世界或实体数据的来源在取不到数据时按官方的 no-op 处理，
//! 实在要不到的数据报 [`LootError::Unsupported`]，见 README 的已知限制。
//!
//! Loot functions that write item components, mirroring the classes under
//! `loot/functions/`. Like the rest of this crate, each function only records the
//! component patch as JSON. `update`-style functions read the component already on the
//! patch (or the vanilla codec default) before applying the official transformation,
//! so chaining functions behaves the same way. Sources that need world or entity data
//! follow vanilla's no-op when the data is absent; data we cannot obtain at all raises
//! [`LootError::Unsupported`], see the README known limitations.

use mc_bamboo_data::items;
use mc_bamboo_nbt::{NbtPath, Tag};
use serde_json::{Map, Value, json};

use crate::context::{EntityTarget, LootContext};
use crate::entry::LootEntry;
use crate::function::{LootFunction, parse_modifier};
use crate::json::{
    array, field, i32_value, invalid, object, opt_bool, opt_i32, opt_string, required, string,
};
use crate::number::{parse_float_provider, parse_int_provider};
use crate::stack::LootStack;
use crate::{LootError, Result};

// 组件名字集中放在这里，避免拼错。
const CUSTOM_DATA: &str = "minecraft:custom_data";
const ATTRIBUTE_MODIFIERS: &str = "minecraft:attribute_modifiers";
const CUSTOM_NAME: &str = "minecraft:custom_name";
const LORE: &str = "minecraft:lore";
const BANNER_PATTERNS: &str = "minecraft:banner_patterns";
const FIREWORKS: &str = "minecraft:fireworks";
const FIREWORK_EXPLOSION: &str = "minecraft:firework_explosion";
const WRITTEN_BOOK_CONTENT: &str = "minecraft:written_book_content";
const WRITABLE_BOOK_CONTENT: &str = "minecraft:writable_book_content";
const TOOLTIP_DISPLAY: &str = "minecraft:tooltip_display";
const CUSTOM_MODEL_DATA: &str = "minecraft:custom_model_data";
const CONTAINER_LOOT: &str = "minecraft:container_loot";
const PROFILE: &str = "minecraft:profile";
const CONTAINER: &str = "minecraft:container";

/// 一个新做的函数体。`function.rs` 遇到这些 `type` 就交给这里解析和求值。
///
/// A newly implemented function body. `function.rs` delegates these `type`s here.
#[derive(Debug)]
pub enum Extended {
    SetCustomData(Tag),
    SetAttributes {
        modifiers: Vec<AttributeModifier>,
        replace: bool,
    },
    CopyName(String),
    SetContents {
        component: String,
        entries: Vec<LootEntry>,
    },
    ModifyContents {
        component: String,
        modifier: Box<LootFunction>,
    },
    SetLootTable {
        table: String,
        seed: i64,
    },
    SetLore {
        lore: Vec<Value>,
        mode: ListOp,
        entity: Option<String>,
    },
    FillPlayerHead(String),
    CopyCustomData(CopyCustomDataSpec),
    SetBannerPattern {
        patterns: Value,
        append: bool,
    },
    SetFireworks {
        explosions: Option<StandAlone>,
        flight_duration: Option<i32>,
    },
    SetFireworkExplosion(Value),
    SetBookCover(Value),
    SetWrittenBookPages {
        pages: Vec<Value>,
        mode: ListOp,
    },
    SetWritableBookPages {
        pages: Vec<Value>,
        mode: ListOp,
    },
    ToggleTooltips(Vec<(String, bool)>),
    SetCustomModelData(Value),
}

/// `SetAttributesFunction.Modifier` 的字段。
///
/// The fields of `SetAttributesFunction.Modifier`.
#[derive(Debug)]
pub struct AttributeModifier {
    id: String,
    attribute: String,
    operation: String,
    amount: Value,
    slots: Vec<String>,
}

/// `CopyCustomDataFunction` 的来源和操作列表。
///
/// The source and operations of `CopyCustomDataFunction`.
#[derive(Debug)]
pub struct CopyCustomDataSpec {
    source: Value,
    ops: Vec<CopyOp>,
}

#[derive(Debug)]
pub struct CopyOp {
    source_path: String,
    target_path: String,
    op: MergeStrategy,
}

#[derive(Debug, Clone, Copy)]
pub enum MergeStrategy {
    Replace,
    Append,
    Merge,
}

/// 官方 `ListOperation` 的四种模式。`apply` 的顺序和边界检查照官方。
///
/// The four `ListOperation` modes; `apply` follows vanilla's bounds checks.
#[derive(Debug, Clone)]
pub enum ListOp {
    Append,
    Insert(usize),
    ReplaceAll,
    ReplaceSection { offset: usize, size: Option<usize> },
}

impl ListOp {
    /// `ListOperation.codec`：`mode` 分派，平铺在所在对象里。
    ///
    /// `ListOperation.codec`: dispatched by `mode`, flattened into the object.
    fn parse(map: &Map<String, Value>) -> Result<Self> {
        let mode = string(required(map, "mode", "list operation")?, "mode")?;
        match mode {
            "append" => Ok(Self::Append),
            "replace_all" => Ok(Self::ReplaceAll),
            "insert" => Ok(Self::Insert(
                opt_i32(map, "offset", "offset", 0)?.max(0) as usize
            )),
            "replace_section" => Ok(Self::ReplaceSection {
                offset: opt_i32(map, "offset", "offset", 0)?.max(0) as usize,
                size: match field(map, "size") {
                    Some(value) => Some(i32_value(value, "size")?.max(0) as usize),
                    None => None,
                },
            }),
            other => Err(invalid(format!("unknown list operation mode `{other}`"))),
        }
    }

    /// `ListOperation.apply(original, replacement, maxSize)`。
    fn apply(&self, original: &[Value], replacement: &[Value], max: usize) -> Vec<Value> {
        match self {
            Self::Append => {
                if original.len() + replacement.len() > max {
                    original.to_vec()
                } else {
                    let mut result = original.to_vec();
                    result.extend(replacement.iter().cloned());
                    result
                }
            }
            Self::Insert(offset) => {
                if *offset > original.len() || original.len() + replacement.len() > max {
                    return original.to_vec();
                }
                let mut result = Vec::with_capacity(original.len() + replacement.len());
                result.extend_from_slice(&original[..*offset]);
                result.extend(replacement.iter().cloned());
                result.extend_from_slice(&original[*offset..]);
                result
            }
            Self::ReplaceAll => replacement.to_vec(),
            Self::ReplaceSection { offset, size } => {
                if *offset > original.len() {
                    return original.to_vec();
                }
                let resume = (*offset + size.unwrap_or(replacement.len())).min(original.len());
                let mut result = Vec::new();
                result.extend_from_slice(&original[..*offset]);
                result.extend(replacement.iter().cloned());
                result.extend_from_slice(&original[resume..]);
                if result.len() > max {
                    original.to_vec()
                } else {
                    result
                }
            }
        }
    }
}

/// `ListOperation.StandAlone`：`values` 加平铺的列表操作。
///
/// `ListOperation.StandAlone`: `values` plus a flattened list operation.
#[derive(Debug, Clone)]
pub struct StandAlone {
    values: Vec<Value>,
    operation: ListOp,
}

impl StandAlone {
    fn parse(value: &Value) -> Result<Self> {
        let map = object(value, "stand-alone operation")?;
        Ok(Self {
            values: array(required(map, "values", "stand-alone")?, "values")?.to_vec(),
            operation: ListOp::parse(map)?,
        })
    }
}

/// 解析一个「新做」的函数。返回 `None` 表示这个 `type` 由 `function.rs` 自己处理。
///
/// Parses a newly implemented function. `None` means `function.rs` handles this `type`.
pub fn parse(kind: &str, map: &Map<String, Value>) -> Result<Option<Extended>> {
    let extended = match kind {
        "minecraft:set_custom_data" => {
            let tag = required(map, "tag", kind)?;
            let entries = if let Some(text) = tag.as_str() {
                mc_bamboo_nbt::parse_snbt_compound(text)
                    .map_err(|error| invalid(format!("set_custom_data tag: {error}")))?
            } else {
                object(tag, "set_custom_data tag")?;
                json_to_compound(Some(tag))
            };
            Extended::SetCustomData(Tag::Compound(entries))
        }
        "minecraft:set_attributes" => Extended::SetAttributes {
            modifiers: parse_attribute_modifiers(map)?,
            replace: opt_bool(map, "replace", kind, true)?,
        },
        "minecraft:copy_name" => {
            Extended::CopyName(string(required(map, "source", kind)?, "source")?.to_owned())
        }
        "minecraft:set_contents" => Extended::SetContents {
            component: parse_container_component(required(map, "component", kind)?)?,
            entries: parse_entries(required(map, "entries", kind)?)?,
        },
        "minecraft:modify_contents" => Extended::ModifyContents {
            component: string(required(map, "component", kind)?, "component")?.to_owned(),
            modifier: Box::new(
                parse_modifier(Some(required(map, "modifier", kind)?))?
                    .ok_or_else(|| invalid("modify_contents has no modifier"))?,
            ),
        },
        "minecraft:set_loot_table" => Extended::SetLootTable {
            table: string(required(map, "loot_table_id", kind)?, "loot_table_id")?.to_owned(),
            seed: opt_seed(map)?,
        },
        "minecraft:set_lore" => Extended::SetLore {
            lore: array(required(map, "lore", kind)?, "lore")?.to_vec(),
            mode: ListOp::parse(map)?,
            entity: opt_string(map, "entity", "entity")?.map(str::to_owned),
        },
        "minecraft:fill_player_head" => {
            Extended::FillPlayerHead(string(required(map, "entity", kind)?, "entity")?.to_owned())
        }
        "minecraft:copy_custom_data" => Extended::CopyCustomData(parse_copy_custom_data(map)?),
        "minecraft:set_banner_pattern" => Extended::SetBannerPattern {
            patterns: required(map, "patterns", kind)?.clone(),
            append: opt_bool(map, "append", kind, false)?,
        },
        "minecraft:set_fireworks" => Extended::SetFireworks {
            explosions: field(map, "explosions")
                .map(StandAlone::parse)
                .transpose()?,
            flight_duration: match field(map, "flight_duration") {
                Some(value) => Some(i32_value(value, "flight_duration")?),
                None => None,
            },
        },
        "minecraft:set_firework_explosion" => {
            Extended::SetFireworkExplosion(required_map_fields(map)?)
        }
        "minecraft:set_book_cover" => Extended::SetBookCover(required_map_fields(map)?),
        "minecraft:set_written_book_pages" => Extended::SetWrittenBookPages {
            pages: array(required(map, "pages", kind)?, "pages")?.to_vec(),
            mode: ListOp::parse(map)?,
        },
        "minecraft:set_writable_book_pages" => Extended::SetWritableBookPages {
            pages: array(required(map, "pages", kind)?, "pages")?.to_vec(),
            mode: ListOp::parse(map)?,
        },
        "minecraft:toggle_tooltips" => {
            Extended::ToggleTooltips(parse_toggles(required(map, "toggles", kind)?)?)
        }
        "minecraft:set_custom_model_data" => {
            Extended::SetCustomModelData(required_map_fields(map)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(extended))
}

/// 求值。返回改过的 `LootStack`。
///
/// Evaluates the function, returning the modified `LootStack`.
pub fn run(
    extended: &Extended,
    mut stack: LootStack,
    context: &LootContext<'_, '_, '_>,
) -> Result<LootStack> {
    if stack.is_empty()
        && matches!(
            extended,
            Extended::SetContents { .. } | Extended::ModifyContents { .. }
        )
    {
        return Ok(stack);
    }
    match extended {
        Extended::SetCustomData(tag) => {
            let mut merged = json_to_compound(stack.component(CUSTOM_DATA));
            if let Tag::Compound(entries) = tag {
                merge_compound(&mut merged, entries);
            }
            stack.set_component(CUSTOM_DATA, tag_to_json(&Tag::Compound(merged)));
        }
        Extended::SetAttributes { modifiers, replace } => {
            let mut list = if *replace {
                Vec::new()
            } else {
                array_field(stack.component(ATTRIBUTE_MODIFIERS))
            };
            for spec in modifiers {
                // `Util.getRandom(modifier.slots, random)` 先取一次，再算 amount。
                // `Util.getRandom` draws once, then the amount provider is evaluated.
                let index = context.random().next_int(spec.slots.len() as i32) as usize;
                let slot = &spec.slots[index];
                let amount =
                    parse_float_provider(&spec.amount, "attribute amount")?.get(context)?;
                let mut entry = json!({
                    "id": spec.id,
                    "type": spec.attribute,
                    "amount": f64::from(amount),
                    "operation": spec.operation,
                    "slot": slot,
                });
                // `withModifierAdded` 移除属性和 ID 都相同的条目，再把新条目加到末尾。
                // `withModifierAdded` removes matching attribute/ID pairs, then appends.
                if slot == "any" {
                    entry
                        .as_object_mut()
                        .expect("modifier is an object")
                        .remove("slot");
                }
                let id = Value::from(spec.id.clone());
                list.retain(|value| {
                    value.get("id") != Some(&id) || value.get("type") != entry.get("type")
                });
                list.push(entry);
            }
            stack.set_component(ATTRIBUTE_MODIFIERS, Value::Array(list));
        }
        Extended::CopyName(source) => {
            if let Some(name) = resolve_custom_name(source, context) {
                stack.set_component(CUSTOM_NAME, name);
            }
            // 官方 `instanceof Nameable` 不成立时原样退回，不报错。
            // Vanilla returns unchanged when the source is not a Nameable.
        }
        Extended::SetContents { component, entries } => {
            let mut contents = Vec::new();
            for entry in entries {
                let mut candidates = Vec::new();
                let mut modifiers = Vec::new();
                entry.expand(context, 0, &mut modifiers, &mut candidates)?;
                for candidate in &candidates {
                    candidate.create(context, 0, &mut |item| {
                        contents.push(item);
                        Ok(())
                    })?;
                }
            }
            let split = crate::table::create_stack_splitter(contents);
            stack.set_component(
                component,
                encode_contents(component, split.into_iter().enumerate()),
            );
        }
        Extended::ModifyContents {
            component,
            modifier,
        } => {
            let Some(contents) = stack.component(component) else {
                return Ok(stack);
            };
            let mut changed = Vec::new();
            for (index, value) in array(contents, "container contents")?.iter().enumerate() {
                let (slot, value) = if component == CONTAINER {
                    let entry = object(value, "container slot")?;
                    let slot = required(entry, "slot", "container slot")?
                        .as_u64()
                        .and_then(|slot| u8::try_from(slot).ok())
                        .ok_or_else(|| invalid("container slot is out of range"))?;
                    (
                        usize::from(slot),
                        required(entry, "item", "container slot")?,
                    )
                } else {
                    (index, value)
                };
                changed.push((slot, stack_from_json(value)?));
            }
            changed.sort_by_key(|(slot, _)| *slot);
            for (_, item) in &mut changed {
                *item = modifier.apply(std::mem::replace(item, LootStack::empty()), context)?;
                item.set_count(item.count.min(item.max_stack_size()));
            }
            stack.set_component(component, encode_contents(component, changed));
        }
        Extended::SetLootTable { table, seed } => {
            let mut value = json!({"loot_table": table});
            if *seed != 0 {
                value["seed"] = Value::from(*seed);
            }
            stack.set_component(CONTAINER_LOOT, value);
        }
        Extended::SetLore { lore, mode, entity } => {
            if entity.is_some() {
                return Err(LootError::Unsupported(
                    "set_lore with an entity resolver needs command text resolution".to_owned(),
                ));
            }
            let existing = array_field(stack.component(LORE));
            let lines = mode.apply(&existing, lore, 256);
            stack.set_component(LORE, Value::Array(lines));
        }
        Extended::FillPlayerHead(target) => {
            // 官方只在物品是玩家头、目标也是玩家时写 profile。
            // Vanilla only writes a profile for a player head with a player source.
            if stack.item == items::PLAYER_HEAD
                && let Some(profile) = resolve_profile(target, context)
            {
                stack.set_component(PROFILE, profile);
            }
        }
        Extended::CopyCustomData(spec) => {
            let source = resolve_nbt_source(&spec.source, context)?;
            let Some(source) = source else {
                return Ok(stack);
            };
            let mut target = json_to_compound(stack.component(CUSTOM_DATA));
            for op in &spec.ops {
                apply_copy_op(&mut target, &source, op)?;
            }
            stack.set_component(CUSTOM_DATA, tag_to_json(&Tag::Compound(target)));
        }
        Extended::SetBannerPattern { patterns, append } => {
            let base = array_field(stack.component(BANNER_PATTERNS));
            let added = array_field(Some(patterns));
            let layers = if *append {
                let mut combined = base;
                combined.extend(added);
                combined
            } else {
                added
            };
            stack.set_component(BANNER_PATTERNS, Value::Array(layers));
        }
        Extended::SetFireworks {
            explosions,
            flight_duration,
        } => {
            let existing = fireworks(stack.component(FIREWORKS));
            let duration = flight_duration.unwrap_or(existing.0);
            let list = match explosions {
                Some(stand) => {
                    // `FireworkExplosion.CODEC` 会给每个值补上缺省字段，编码时再省略缺省值。
                    // `FireworkExplosion.CODEC` fills every default and omits default fields on encode.
                    let values: Vec<Value> = stand
                        .values
                        .iter()
                        .map(normalize_firework_explosion)
                        .collect();
                    stand.operation.apply(&existing.1, &values, 256)
                }
                None => existing.1,
            };
            let value = json!({"flight_duration": duration, "explosions": list});
            stack.set_component(
                FIREWORKS,
                omit_defaults(
                    value,
                    &[("flight_duration", json!(0)), ("explosions", json!([]))],
                ),
            );
        }
        Extended::SetFireworkExplosion(fields) => {
            // `FireworkExplosion` 的缺省值打底，函数里给的可选字段覆盖上去。
            // Start from the `FireworkExplosion` default and overlay the optional fields.
            let mut result = firework_explosion_default();
            result.extend(object_field(stack.component(FIREWORK_EXPLOSION)));
            result = merge_optional(result, fields);
            for (input, output) in [("trail", "has_trail"), ("twinkle", "has_twinkle")] {
                if let Some(value) = result.remove(input) {
                    result.insert(output.to_owned(), value);
                }
            }
            stack.set_component(
                FIREWORK_EXPLOSION,
                omit_defaults(
                    Value::Object(result),
                    &[
                        ("colors", json!([])),
                        ("fade_colors", json!([])),
                        ("has_trail", json!(false)),
                        ("has_twinkle", json!(false)),
                    ],
                ),
            );
        }
        Extended::SetBookCover(fields) => {
            let mut book = book_content(stack.component(WRITTEN_BOOK_CONTENT));
            if let Some(title) = fields.get("title") {
                book.insert("title".to_owned(), filterable(title));
            }
            if let Some(author) = fields.get("author") {
                book.insert("author".to_owned(), author.clone());
            }
            if let Some(generation) = fields.get("generation") {
                book.insert("generation".to_owned(), generation.clone());
            }
            book.entry("resolved").or_insert(Value::Bool(true));
            stack.set_component(
                WRITTEN_BOOK_CONTENT,
                omit_defaults(
                    Value::Object(book),
                    &[
                        ("generation", json!(0)),
                        ("pages", json!([])),
                        ("resolved", json!(false)),
                    ],
                ),
            );
        }
        Extended::SetWrittenBookPages { pages, mode } => {
            let mut book = book_content(stack.component(WRITTEN_BOOK_CONTENT));
            let existing = array_field(book.get("pages"));
            let list = mode.apply(
                &existing,
                &pages.iter().map(filterable).collect::<Vec<_>>(),
                usize::MAX,
            );
            book.insert("pages".to_owned(), Value::Array(list));
            book.remove("resolved");
            stack.set_component(
                WRITTEN_BOOK_CONTENT,
                omit_defaults(
                    Value::Object(book),
                    &[
                        ("generation", json!(0)),
                        ("pages", json!([])),
                        ("resolved", json!(false)),
                    ],
                ),
            );
        }
        Extended::SetWritableBookPages { pages, mode } => {
            let existing = array_field(
                stack
                    .component(WRITABLE_BOOK_CONTENT)
                    .and_then(|v| v.get("pages")),
            );
            let list = mode.apply(
                &existing,
                &pages.iter().map(filterable).collect::<Vec<_>>(),
                100,
            );
            stack.set_component(WRITABLE_BOOK_CONTENT, json!({"pages": list}));
        }
        Extended::ToggleTooltips(toggles) => {
            let mut hidden: Vec<String> = string_array(
                stack
                    .component(TOOLTIP_DISPLAY)
                    .and_then(|v| v.get("hidden_components")),
            );
            for (name, shown) in toggles {
                if *shown {
                    hidden.retain(|entry| entry != name);
                } else if !hidden.contains(name) {
                    hidden.push(name.clone());
                }
            }
            hidden.sort();
            stack.set_component(TOOLTIP_DISPLAY, json!({"hidden_components": hidden}));
        }
        Extended::SetCustomModelData(fields) => {
            let mut data = custom_model_data(stack.component(CUSTOM_MODEL_DATA));
            for key in ["floats", "flags", "strings", "colors"] {
                if let Some(value) = fields.get(key) {
                    let current = data
                        .get(key)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let stand = StandAlone::parse(value)?;
                    let mapped = stand
                        .values
                        .iter()
                        .map(|item| evaluate_custom_model_value(key, item, context))
                        .collect::<Result<Vec<_>>>()?;
                    let list = stand.operation.apply(&current, &mapped, usize::MAX);
                    data.insert(key.to_owned(), Value::Array(list));
                }
            }
            data.retain(|_, value| !value.as_array().is_some_and(Vec::is_empty));
            stack.set_component(CUSTOM_MODEL_DATA, Value::Object(data));
        }
    }
    Ok(stack)
}

// ---- 解析辅助 ----

fn parse_attribute_modifiers(map: &Map<String, Value>) -> Result<Vec<AttributeModifier>> {
    let mut result = Vec::new();
    for value in array(required(map, "modifiers", "set_attributes")?, "modifiers")? {
        let entry = object(value, "attribute modifier")?;
        let slots = match required(entry, "slot", "attribute modifier")? {
            Value::String(name) => vec![name.clone()],
            Value::Array(values) => values
                .iter()
                .map(|value| string(value, "slot").map(str::to_owned))
                .collect::<Result<Vec<_>>>()?,
            _ => {
                return Err(invalid(
                    "attribute modifier slot must be a string or a list",
                ));
            }
        };
        if slots.is_empty() {
            return Err(invalid("attribute modifier needs at least one slot"));
        }
        result.push(AttributeModifier {
            id: string(required(entry, "id", "attribute modifier")?, "id")?.to_owned(),
            attribute: string(
                required(entry, "attribute", "attribute modifier")?,
                "attribute",
            )?
            .to_owned(),
            operation: string(
                required(entry, "operation", "attribute modifier")?,
                "operation",
            )?
            .to_owned(),
            amount: required(entry, "amount", "attribute modifier")?.clone(),
            slots,
        });
    }
    Ok(result)
}

fn parse_entries(value: &Value) -> Result<Vec<LootEntry>> {
    array(value, "set_contents entries")?
        .iter()
        .map(LootEntry::parse)
        .collect()
}

fn parse_toggles(value: &Value) -> Result<Vec<(String, bool)>> {
    let map = object(value, "toggles")?;
    map.iter()
        .map(|(name, value)| {
            value
                .as_bool()
                .map(|shown| (name.clone(), shown))
                .ok_or_else(|| invalid("toggle_tooltips values must be bools"))
        })
        .collect()
}

/// `Codec.LONG.optionalFieldOf("seed", 0L)`：seed 可以是数字或十进制字符串。
///
/// `Codec.LONG.optionalFieldOf("seed", 0L)`: a number or a decimal string.
fn opt_seed(map: &Map<String, Value>) -> Result<i64> {
    match field(map, "seed") {
        Some(Value::Number(number)) => number
            .as_i64()
            .ok_or_else(|| invalid("set_loot_table seed is out of long range")),
        Some(Value::String(text)) => text
            .parse()
            .map_err(|_| invalid("set_loot_table seed is not a long")),
        Some(_) => Err(invalid("set_loot_table seed must be a long")),
        None => Ok(0),
    }
}

fn parse_copy_custom_data(map: &Map<String, Value>) -> Result<CopyCustomDataSpec> {
    let mut ops = Vec::new();
    for value in array(required(map, "ops", "copy_custom_data")?, "ops")? {
        let entry = object(value, "copy operation")?;
        let strategy = match string(required(entry, "op", "copy operation")?, "op")? {
            "replace" => MergeStrategy::Replace,
            "append" => MergeStrategy::Append,
            "merge" => MergeStrategy::Merge,
            other => return Err(invalid(format!("unknown copy_custom_data op `{other}`"))),
        };
        ops.push(CopyOp {
            source_path: string(required(entry, "source", "copy operation")?, "source")?.to_owned(),
            target_path: string(required(entry, "target", "copy operation")?, "target")?.to_owned(),
            op: strategy,
        });
    }
    Ok(CopyCustomDataSpec {
        source: required(map, "source", "copy_custom_data")?.clone(),
        ops,
    })
}

/// 把「可选字段」型函数的顶层字段收成一个对象（去掉 `type` / `condition`）。
///
/// Collects the top-level fields of an optional-field function (minus `type`/`condition`).
fn required_map_fields(map: &Map<String, Value>) -> Result<Value> {
    let mut fields = Map::new();
    for (name, value) in map {
        if name == "type" || name == "condition" {
            continue;
        }
        fields.insert(name.clone(), value.clone());
    }
    Ok(Value::Object(fields))
}

// ---- 组件缺省值和读取 ----

fn object_field(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn array_field(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn fireworks(value: Option<&Value>) -> (i32, Vec<Value>) {
    let object = object_field(value);
    let duration = object
        .get("flight_duration")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0);
    (duration, array_field(object.get("explosions")))
}

fn omit_defaults(mut value: Value, defaults: &[(&str, Value)]) -> Value {
    let fields = value
        .as_object_mut()
        .expect("component fields are an object");
    for (key, default) in defaults {
        if fields.get(*key) == Some(default) {
            fields.remove(*key);
        }
    }
    value
}

fn firework_explosion_default() -> Map<String, Value> {
    let mut map = Map::new();
    for key in ["shape", "colors", "fade_colors", "has_trail", "has_twinkle"] {
        map.insert(key.to_owned(), default_value(key));
    }
    map
}

/// `SetFireworksFunction` 的 `explosions` 值按 `FireworkExplosion.CODEC` 解析：
/// `shape` 总在场，其余字段是缺省值时编码时省略。
///
/// `SetFireworksFunction` parses each `explosions` value through
/// `FireworkExplosion.CODEC`: `shape` is always present, while the other fields are
/// omitted on encode when they equal their defaults.
fn normalize_firework_explosion(value: &Value) -> Value {
    let mut result = firework_explosion_default();
    result.extend(object_field(Some(value)));
    omit_defaults(
        Value::Object(result),
        &[
            ("colors", json!([])),
            ("fade_colors", json!([])),
            ("has_trail", json!(false)),
            ("has_twinkle", json!(false)),
        ],
    )
}

fn default_value(key: &str) -> Value {
    match key {
        "shape" => Value::String("small_ball".to_owned()),
        "colors" | "fade_colors" => Value::Array(Vec::new()),
        "has_trail" | "has_twinkle" => Value::Bool(false),
        _ => Value::Null,
    }
}

fn merge_optional(mut base: Map<String, Value>, fields: &Value) -> Map<String, Value> {
    if let Some(object) = fields.as_object() {
        for (name, value) in object {
            base.insert(name.clone(), value.clone());
        }
    }
    base
}

fn book_content(value: Option<&Value>) -> Map<String, Value> {
    let mut map = object_field(value);
    for (key, default) in [
        ("title", json!({"raw": ""})),
        ("author", Value::String(String::new())),
        ("generation", Value::from(0)),
        ("pages", Value::Array(Vec::new())),
    ] {
        map.entry(key.to_owned()).or_insert(default);
    }
    map
}

fn custom_model_data(value: Option<&Value>) -> Map<String, Value> {
    let mut map = object_field(value);
    for key in ["floats", "flags", "strings", "colors"] {
        map.entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
    }
    map
}

fn filterable(value: &Value) -> Value {
    if value.get("raw").is_some() {
        value.clone()
    } else {
        json!({"raw": value})
    }
}

fn parse_container_component(value: &Value) -> Result<String> {
    let name = string(value, "component")?;
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    match name.as_str() {
        CONTAINER | "minecraft:charged_projectiles" => Ok(name),
        "minecraft:bundle_contents" => Err(LootError::Unsupported(
            "bundle loot contents need bundle insertion rules".to_owned(),
        )),
        _ => Err(invalid(format!("unknown loot container component {name}"))),
    }
}

fn encode_contents(component: &str, items: impl IntoIterator<Item = (usize, LootStack)>) -> Value {
    Value::Array(
        items
            .into_iter()
            .filter_map(|(slot, stack)| {
                if stack.is_empty() || (component == CONTAINER && slot > usize::from(u8::MAX)) {
                    return None;
                }
                let item = stack_to_json(&stack);
                Some(if component == CONTAINER {
                    json!({"slot": slot, "item": item})
                } else {
                    item
                })
            })
            .collect(),
    )
}

fn stack_to_json(stack: &LootStack) -> Value {
    let mut map = Map::new();
    map.insert(
        "id".to_owned(),
        Value::from(crate::names::item_name(stack.item).unwrap_or("minecraft:air")),
    );
    if stack.count != 1 {
        map.insert("count".to_owned(), Value::from(stack.count));
    }
    if !stack.components.is_empty() {
        let components: Map<String, Value> = stack
            .components
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        map.insert("components".to_owned(), Value::Object(components));
    }
    Value::Object(map)
}

fn stack_from_json(value: &Value) -> Result<LootStack> {
    let (name, count) = if let Some(name) = value.as_str() {
        (name, 1)
    } else {
        let map = object(value, "container item")?;
        let name = string(required(map, "id", "container item")?, "container item id")?;
        let count = map
            .get("count")
            .map(|v| i32_value(v, "container count"))
            .transpose()?
            .unwrap_or(1);
        (name, count)
    };
    let item = crate::names::item_id(name)
        .ok_or_else(|| invalid(format!("unknown container item {name}")))?;
    let mut stack = LootStack::new(item, count);
    if let Some(components) = value.get("components").and_then(Value::as_object) {
        for (name, value) in components {
            stack.set_component(name, value.clone());
        }
    }
    Ok(stack)
}

fn evaluate_custom_model_value(
    key: &str,
    value: &Value,
    context: &LootContext<'_, '_, '_>,
) -> Result<Value> {
    match key {
        "floats" => Ok(Value::from(
            parse_float_provider(value, "custom_model_data float")?.get(context)?,
        )),
        "colors" => Ok(Value::from(
            parse_int_provider(value, "custom_model_data color")?.get(context)?,
        )),
        _ => Ok(value.clone()),
    }
}

// ---- 世界 / 实体数据来源 ----

/// `CopyNameFunction` / `FillPlayerHead` / `CopyCustomDataFunction` 的来源解析。
/// 只认本实现拿得到的数据：方块实体的 `custom_name`，以及实体视图里存的组件。
/// 拿不到时返回 `None`，官方对应的 `instanceof` / `getOptional` 也是这样 no-op。
///
/// Resolves sources the crate can actually see: a block entity's `custom_name` and
/// components stored on entity views. `None` means vanilla would no-op too.
fn resolve_custom_name(source: &str, context: &LootContext<'_, '_, '_>) -> Option<Value> {
    let target = EntityTarget::from_name(source);
    if source == "block_entity" {
        return context
            .params
            .block_entity
            .as_ref()
            .and_then(|block| block.components.get(CUSTOM_NAME))
            .cloned();
    }
    let entity = target?.get(context.params)?;
    entity.components.get(CUSTOM_NAME).cloned()
}

fn resolve_profile(target: &str, context: &LootContext<'_, '_, '_>) -> Option<Value> {
    let entity = EntityTarget::from_name(target)?.get(context.params)?;
    if entity.entity_type != mc_bamboo_data::registries::entity_type::PLAYER {
        return None;
    }
    entity.components.get(PROFILE).cloned()
}

/// `NbtProviders` 的来源：从世界视图取命令存储，或从实体视图读 NBT。
/// 实体来源缺失时和官方一样不修改物品。
///
/// `NbtProviders` sources: command storage from the level view, or NBT from an entity view.
/// An absent entity source leaves the stack unchanged, as in vanilla.
fn resolve_nbt_source(source: &Value, context: &LootContext<'_, '_, '_>) -> Result<Option<Tag>> {
    if let Some(target) = source.as_str() {
        return Ok(EntityTarget::from_name(target)
            .and_then(|target| target.get(context.params))
            .and_then(|entity| entity.nbt.clone()));
    }
    let map = object(source, "nbt provider source")?;
    let kind = string(required(map, "type", "nbt provider source")?, "type")?;
    match kind {
        "minecraft:context" => {
            let target = string(required(map, "target", "nbt provider source")?, "target")?;
            Ok(EntityTarget::from_name(target)
                .and_then(|target| target.get(context.params))
                .and_then(|entity| entity.nbt.clone()))
        }
        "minecraft:storage" => {
            let storage = string(required(map, "source", "nbt provider source")?, "source")?;
            context
                .params
                .level
                .and_then(|level| level.command_storage(storage))
                .map(Some)
                .ok_or_else(|| {
                    LootError::Unsupported("copy_custom_data needs command storage".to_owned())
                })
        }
        other => Err(invalid(format!("unknown nbt provider source `{other}`"))),
    }
}

/// `CopyCustomDataFunction` 的一条 op。目标路径按复合标签键分层，来源用 `NbtPath.get`
/// 取最后一个匹配值；`Replace` 覆盖、`Append` 往列表追加、`Merge` 并进复合标签。
///
/// One `CopyCustomDataFunction` op. The target is a compound-key path; the source is
/// the last `NbtPath.get` match. `Replace` overwrites, `Append` extends a list, `Merge`
/// merges a compound.
fn apply_copy_op(target: &mut Vec<(String, Tag)>, source: &Tag, op: &CopyOp) -> Result<()> {
    let path = NbtPath::parse(&op.source_path)
        .map_err(|_| invalid("copy_custom_data source path is invalid"))?;
    let found = path.get(source);
    let Some(last) = found.last() else {
        return Ok(());
    };
    let value = (**last).clone();
    let keys = compound_keys(&op.target_path);
    match op.op {
        MergeStrategy::Replace => set_in_compound(target, &keys, value),
        MergeStrategy::Append => append_in_compound(target, &keys, &value),
        MergeStrategy::Merge => merge_in_compound(target, &keys, &value),
    }
    Ok(())
}

fn set_in_compound(entries: &mut Vec<(String, Tag)>, keys: &[String], value: Tag) {
    let Some((first, rest)) = keys.split_first() else {
        return;
    };
    let index = match entries.iter().position(|(name, _)| name == first) {
        Some(index) => index,
        None => {
            entries.push((first.clone(), Tag::Compound(Vec::new())));
            entries.len() - 1
        }
    };
    if rest.is_empty() {
        entries[index].1 = value;
        return;
    }
    if !matches!(entries[index].1, Tag::Compound(_)) {
        entries[index].1 = Tag::Compound(Vec::new());
    }
    if let Tag::Compound(child) = &mut entries[index].1 {
        set_in_compound(child, rest, value);
    }
}

fn append_in_compound(entries: &mut [(String, Tag)], keys: &[String], source: &Tag) {
    let Some((first, rest)) = keys.split_first() else {
        return;
    };
    let index = match entries.iter().position(|(name, _)| name == first) {
        Some(index) => index,
        None => return,
    };
    if rest.is_empty() {
        if let Tag::List(list) = &mut entries[index].1
            && let Tag::List(items) = source
        {
            list.extend(items.iter().cloned());
        }
        return;
    }
    if let Tag::Compound(child) = &mut entries[index].1 {
        append_in_compound(child, rest, source);
    }
}

fn merge_in_compound(entries: &mut Vec<(String, Tag)>, keys: &[String], source: &Tag) {
    let Some((first, rest)) = keys.split_first() else {
        return;
    };
    let index = match entries.iter().position(|(name, _)| name == first) {
        Some(index) => index,
        None => {
            entries.push((first.clone(), Tag::Compound(Vec::new())));
            entries.len() - 1
        }
    };
    if !matches!(entries[index].1, Tag::Compound(_)) {
        entries[index].1 = Tag::Compound(Vec::new());
    }
    if let Tag::Compound(child) = &mut entries[index].1 {
        if rest.is_empty() {
            if let Tag::Compound(source_entries) = source {
                merge_compound(child, source_entries);
            }
        } else {
            merge_in_compound(child, rest, source);
        }
    }
}

fn compound_keys(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .collect()
}

// ---- NBT <-> JSON ----

fn tag_to_json(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!(*value),
        Tag::Short(value) => json!(*value),
        Tag::Int(value) => json!(*value),
        Tag::Long(value) => json!(*value),
        Tag::Float(value) => json!(*value),
        Tag::Double(value) => json!(*value),
        Tag::String(value) => Value::String(value.clone()),
        Tag::ByteArray(values) => Value::Array(values.iter().map(|value| json!(*value)).collect()),
        Tag::IntArray(values) => Value::Array(values.iter().map(|value| json!(*value)).collect()),
        Tag::LongArray(values) => Value::Array(values.iter().map(|value| json!(*value)).collect()),
        Tag::List(values) => Value::Array(values.iter().map(tag_to_json).collect()),
        Tag::Compound(entries) => Value::Object(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), tag_to_json(value)))
                .collect(),
        ),
    }
}

fn json_to_compound(value: Option<&Value>) -> Vec<(String, Tag)> {
    match value.and_then(Value::as_object) {
        Some(map) => map
            .iter()
            .map(|(name, value)| (name.clone(), json_to_tag(value)))
            .collect(),
        None => Vec::new(),
    }
}

fn json_to_tag(value: &Value) -> Tag {
    match value {
        Value::Bool(flag) => Tag::Byte(i8::from(*flag)),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                if let Ok(value) = i32::try_from(value) {
                    return Tag::Int(value);
                }
                return Tag::Long(value);
            }
            Tag::Double(number.as_f64().unwrap_or(0.0))
        }
        Value::String(text) => Tag::String(text.clone()),
        Value::Array(values) => Tag::List(values.iter().map(json_to_tag).collect()),
        Value::Object(map) => Tag::Compound(
            map.iter()
                .map(|(name, value)| (name.clone(), json_to_tag(value)))
                .collect(),
        ),
        Value::Null => Tag::Byte(0),
    }
}

/// `CompoundTag.merge`：同名且两边都是复合标签时递归合并，否则覆盖。
///
/// `CompoundTag.merge`: recurse when both sides are compounds, otherwise overwrite.
fn merge_compound(target: &mut Vec<(String, Tag)>, source: &[(String, Tag)]) {
    for (name, value) in source {
        if let Some(index) = target.iter().position(|(existing, _)| existing == name) {
            match (&mut target[index].1, value) {
                (Tag::Compound(dst), Tag::Compound(src)) => merge_compound(dst, src),
                (dst, _) => *dst = value.clone(),
            }
        } else {
            target.push((name.clone(), value.clone()));
        }
    }
}
