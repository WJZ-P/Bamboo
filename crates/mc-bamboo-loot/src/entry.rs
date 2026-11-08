//! 战利品池的条目，对应官方的 `LootPoolEntryContainer` 体系。
//!
//! 官方用「展开」收集可选条目：条件成立时把叶子条目交给调用方，组合条目按
//! `alternatives` / `sequence` / `group` 的短路规则继续展开。条目的 `modifier`
//! 在产出时包在输出消费者外面，所以这里把沿路的 modifier 记在候选上，等选中
//! 真正产出物品时按从内到外的顺序应用，随机数的取数时机和官方一致。

use serde_json::{Map, Value};

use crate::condition::LootCondition;
use crate::context::LootContext;
use crate::function::{parse_modifier, LootFunction};
use crate::json::{object, opt_bool, opt_i32, required, string, type_name};
use crate::names::{self, Registry};
use crate::stack::LootStack;
use crate::table::LootTable;
use crate::{LootError, Result};

/// 嵌套表的递归上限。官方用访问过的表集合防环，这里没有可比较的表身份，
/// 改用深度上限；数据包里没有环，正常求值不会碰到。
pub(crate) const MAX_DEPTH: u32 = 32;

/// 一个战利品条目，对应官方的一个 `LootPoolEntryContainer` 子类。
#[derive(Debug)]
pub struct LootEntry {
    /// 只对叶子条目有意义，组合条目的权重由子条目决定。
    weight: i32,
    quality: i32,
    condition: Option<LootCondition>,
    modifier: Option<LootFunction>,
    body: EntryBody,
}

#[derive(Debug)]
enum EntryBody {
    Empty,
    Item(i32),
    Dynamic(String),
    Tag {
        items: Vec<i32>,
        expand: bool,
    },
    Nested {
        tables: Vec<TableValue>,
        expand: bool,
    },
    Alternatives(Vec<LootEntry>),
    Sequence(Vec<LootEntry>),
    Group(Vec<LootEntry>),
    /// `slots` 条目要从实体或容器里取现成的物品，本实现没有这种数据来源。
    Slots,
}

/// `loot_table` 条目引用的表：注册表里的名字，或者内联在 JSON 里的表。
#[derive(Debug)]
pub(crate) enum TableValue {
    Named(String),
    Inline(Box<LootTable>),
}

/// 展开出来的一个候选条目。`modifiers` 是从内到外的应用顺序。
pub(crate) struct Candidate<'a> {
    pub(crate) weight: i32,
    pub(crate) body: CandidateBody<'a>,
    pub(crate) modifiers: Vec<&'a LootFunction>,
}

pub(crate) enum CandidateBody<'a> {
    Empty,
    Item(i32),
    Dynamic(&'a str),
    /// 没有展开的标签：选中后一次产出标签里的所有物品。
    Items(&'a [i32]),
    /// 没有展开的嵌套表：选中后依次求值这些表。
    Tables(&'a [TableValue]),
    Slots,
}

impl LootEntry {
    /// 解析一个条目。条目必须是带 `type` 的对象。
    pub fn parse(value: &Value) -> Result<Self> {
        let map = object(value, "loot entry")?;
        let kind = type_name(map, "loot entry")?;
        let condition = match crate::json::field(map, "condition") {
            Some(value) => Some(LootCondition::parse(value)?),
            None => None,
        };
        let modifier = parse_modifier(crate::json::field(map, "modifier"))?;
        // 组合条目没有 weight / quality 字段，官方 codec 也不读。
        let (weight, quality) = if is_composite(kind) {
            (1, 0)
        } else {
            (
                opt_i32(map, "weight", kind, 1)?,
                opt_i32(map, "quality", kind, 0)?,
            )
        };
        let body = parse_body(map, kind)?;
        Ok(Self {
            weight,
            quality,
            condition,
            modifier,
            body,
        })
    }

    /// `LootPoolEntryContainer.expand`。
    pub(crate) fn expand<'a>(
        &'a self,
        context: &LootContext<'_, '_, '_>,
        depth: u32,
        modifiers: &mut Vec<&'a LootFunction>,
        output: &mut Vec<Candidate<'a>>,
    ) -> Result<bool> {
        if let Some(condition) = &self.condition {
            if !condition.test(context)? {
                return Ok(false);
            }
        }
        if let Some(modifier) = &self.modifier {
            modifiers.push(modifier);
        }
        let expanded = self.expand_raw(context, depth, modifiers, output)?;
        if self.modifier.is_some() {
            modifiers.pop();
        }
        Ok(expanded)
    }

    fn expand_raw<'a>(
        &'a self,
        context: &LootContext<'_, '_, '_>,
        depth: u32,
        modifiers: &mut Vec<&'a LootFunction>,
        output: &mut Vec<Candidate<'a>>,
    ) -> Result<bool> {
        match &self.body {
            EntryBody::Alternatives(children) => {
                expand_alternatives(children, context, depth, modifiers, output)
            }
            EntryBody::Sequence(children) => {
                expand_sequence(children, context, depth, modifiers, output)
            }
            EntryBody::Group(children) => {
                expand_group(children, context, depth, modifiers, output)
            }
            _ => {
                self.expand_leaf(context, modifiers, output);
                Ok(true)
            }
        }
    }

    fn expand_leaf<'a>(
        &'a self,
        context: &LootContext<'_, '_, '_>,
        modifiers: &mut Vec<&'a LootFunction>,
        output: &mut Vec<Candidate<'a>>,
    ) {
        let weight = self.get_weight(context.luck());
        let outer: Vec<&'a LootFunction> = modifiers.iter().rev().copied().collect();
        match &self.body {
            EntryBody::Empty => output.push(Candidate {
                weight,
                body: CandidateBody::Empty,
                modifiers: outer,
            }),
            EntryBody::Item(item) => output.push(Candidate {
                weight,
                body: CandidateBody::Item(*item),
                modifiers: outer,
            }),
            EntryBody::Dynamic(name) => output.push(Candidate {
                weight,
                body: CandidateBody::Dynamic(name),
                modifiers: outer,
            }),
            EntryBody::Tag { items, expand } => {
                if *expand {
                    for item in items {
                        output.push(Candidate {
                            weight,
                            body: CandidateBody::Item(*item),
                            modifiers: outer.clone(),
                        });
                    }
                } else {
                    output.push(Candidate {
                        weight,
                        body: CandidateBody::Items(items),
                        modifiers: outer,
                    });
                }
            }
            EntryBody::Nested { tables, expand } => {
                if *expand {
                    for table in tables {
                        output.push(Candidate {
                            weight,
                            body: CandidateBody::Tables(std::slice::from_ref(table)),
                            modifiers: outer.clone(),
                        });
                    }
                } else {
                    output.push(Candidate {
                        weight,
                        body: CandidateBody::Tables(tables),
                        modifiers: outer,
                    });
                }
            }
            EntryBody::Slots => output.push(Candidate {
                weight,
                body: CandidateBody::Slots,
                modifiers: outer,
            }),
            // 组合条目在上面处理。
            EntryBody::Alternatives(_) | EntryBody::Sequence(_) | EntryBody::Group(_) => {}
        }
    }

    /// `UniformContainerBase.EntryBase.getWeight`。
    fn get_weight(&self, luck: f32) -> i32 {
        let weight = self.weight as f32 + self.quality as f32 * luck;
        (weight.floor() as i32).max(0)
    }
}

impl Candidate<'_> {
    /// 产出这个候选条目的物品。返回值里的每个栈都已经过条目、池、表的 modifier。
    pub(crate) fn create(
        &self,
        context: &LootContext<'_, '_, '_>,
        depth: u32,
    ) -> Result<Vec<LootStack>> {
        let mut stacks = match &self.body {
            CandidateBody::Empty => Vec::new(),
            CandidateBody::Item(item) => vec![LootStack::new(*item, 1)],
            CandidateBody::Dynamic(name) => {
                let mut stacks = Vec::new();
                context.add_dynamic_drops(name, &mut stacks)?;
                stacks
            }
            CandidateBody::Items(items) => {
                (*items).iter().map(|item| LootStack::new(*item, 1)).collect()
            }
            CandidateBody::Tables(tables) => {
                let mut stacks = Vec::new();
                for table in *tables {
                    match table {
                        TableValue::Named(name) => {
                            let table = LootTable::from_name(name)?;
                            table.get_random_items_raw(context, depth + 1, &mut stacks)?;
                        }
                        TableValue::Inline(table) => {
                            table.get_random_items_raw(context, depth + 1, &mut stacks)?;
                        }
                    }
                }
                stacks
            }
            CandidateBody::Slots => {
                return Err(LootError::Unsupported(
                    "slot loot needs an entity or container to read items from".to_owned(),
                ))
            }
        };
        if self.modifiers.is_empty() {
            return Ok(stacks);
        }
        let mut decorated = Vec::with_capacity(stacks.len());
        for stack in stacks.drain(..) {
            let mut stack = stack;
            for modifier in &self.modifiers {
                stack = modifier.apply(stack, context)?;
            }
            decorated.push(stack);
        }
        Ok(decorated)
    }
}

fn is_composite(kind: &str) -> bool {
    matches!(
        kind,
        "minecraft:alternatives" | "minecraft:sequence" | "minecraft:group"
    )
}

fn parse_body(map: &Map<String, Value>, kind: &str) -> Result<EntryBody> {
    match kind {
        "minecraft:empty" => Ok(EntryBody::Empty),
        "minecraft:item" => Ok(EntryBody::Item(names::single_registry_id(
            required(map, "name", kind)?,
            Registry::Item,
            "name",
        )?)),
        "minecraft:dynamic" => Ok(EntryBody::Dynamic(
            string(required(map, "name", kind)?, "name")?.to_owned(),
        )),
        "minecraft:tag" => Ok(EntryBody::Tag {
            items: names::holder_set(required(map, "items", kind)?, Registry::Item, "items")?,
            expand: opt_bool(map, "expand", kind, false)?,
        }),
        "minecraft:loot_table" => Ok(EntryBody::Nested {
            tables: parse_tables(required(map, "value", kind)?)?,
            expand: opt_bool(map, "expand", kind, false)?,
        }),
        "minecraft:alternatives" => Ok(EntryBody::Alternatives(parse_children(map, kind)?)),
        "minecraft:sequence" => Ok(EntryBody::Sequence(parse_children(map, kind)?)),
        "minecraft:group" => Ok(EntryBody::Group(parse_children(map, kind)?)),
        "minecraft:slots" => Ok(EntryBody::Slots),
        other => Err(LootError::Invalid(format!(
            "unknown loot entry type `{other}`"
        ))),
    }
}

fn parse_children(map: &Map<String, Value>, kind: &str) -> Result<Vec<LootEntry>> {
    let children = crate::json::opt_array(map, "children", "children")?.unwrap_or_default();
    children
        .iter()
        .map(LootEntry::parse)
        .collect::<Result<Vec<_>>>()
        .map_err(|error| match error {
            LootError::Invalid(message) => {
                LootError::Invalid(format!("{kind} children: {message}"))
            }
            error => error,
        })
}

/// `LootTable.LIST_CODEC`：单个表、`#标签`、或者表数组。本数据包里没有表标签。
fn parse_tables(value: &Value) -> Result<Vec<TableValue>> {
    match value {
        Value::Array(tables) => tables.iter().map(parse_table).collect(),
        other => Ok(vec![parse_table(other)?]),
    }
}

fn parse_table(value: &Value) -> Result<TableValue> {
    match value {
        Value::String(name) => {
            if name.starts_with('#') {
                return Err(LootError::Unsupported(
                    "loot table tags are not exported by the data generator".to_owned(),
                ));
            }
            if mc_bamboo_data::loot::loot_table(name).is_none() {
                return Err(LootError::UnknownTable(name.clone()));
            }
            Ok(TableValue::Named(name.clone()))
        }
        Value::Object(_) => Ok(TableValue::Inline(Box::new(LootTable::parse(value)?))),
        _ => Err(LootError::Invalid(
            "loot_table value must be a name, a table or a list".to_owned(),
        )),
    }
}

fn expand_alternatives<'a>(
    children: &'a [LootEntry],
    context: &LootContext<'_, '_, '_>,
    depth: u32,
    modifiers: &mut Vec<&'a LootFunction>,
    output: &mut Vec<Candidate<'a>>,
) -> Result<bool> {
    // 官方按子条目数量特化：0 个永远 false，2 个是短路 or，其余取第一个 true。
    match children {
        [] => Ok(false),
        [child] => child.expand(context, depth, modifiers, output),
        [first, second] => {
            if first.expand(context, depth, modifiers, output)? {
                Ok(true)
            } else {
                second.expand(context, depth, modifiers, output)
            }
        }
        _ => {
            for child in children {
                if child.expand(context, depth, modifiers, output)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

fn expand_sequence<'a>(
    children: &'a [LootEntry],
    context: &LootContext<'_, '_, '_>,
    depth: u32,
    modifiers: &mut Vec<&'a LootFunction>,
    output: &mut Vec<Candidate<'a>>,
) -> Result<bool> {
    match children {
        [] => Ok(true),
        [child] => child.expand(context, depth, modifiers, output),
        [first, second] => {
            if first.expand(context, depth, modifiers, output)? {
                second.expand(context, depth, modifiers, output)
            } else {
                Ok(false)
            }
        }
        _ => {
            for child in children {
                if !child.expand(context, depth, modifiers, output)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

fn expand_group<'a>(
    children: &'a [LootEntry],
    context: &LootContext<'_, '_, '_>,
    depth: u32,
    modifiers: &mut Vec<&'a LootFunction>,
    output: &mut Vec<Candidate<'a>>,
) -> Result<bool> {
    match children {
        [] => Ok(true),
        [child] => child.expand(context, depth, modifiers, output),
        _ => {
            for child in children {
                child.expand(context, depth, modifiers, output)?;
            }
            Ok(true)
        }
    }
}
