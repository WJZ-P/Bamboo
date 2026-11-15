//! `processor_list` 的解析：结构模板放置阶段的 `StructureProcessor` 列表。
//!
//! 官方 `StructureProcessorType.DIRECT_CODEC` 是
//! `Codec.withAlternative(LIST_OBJECT_CODEC.fieldOf("processors").codec(), LIST_OBJECT_CODEC)`：
//! 注册表里的条目要么是裸数组，要么是 `{processors: [...]}`。`SINGLE_CODEC` 按
//! `processor_type` 分发。这里照官方 codec 把 26.3 数据里出现的四种 processor
//! （`block_rot` / `rule` / `protected_blocks` / `capped`）、五种 `RuleTest`、
//! 一种 `PosRuleTest` 和一种 `RuleBlockEntityModifier` 解析成 Rust 结构体；
//! **只解析，不执行**（执行属放置阶段）。
//!
//! Parse `processor_list`: the `StructureProcessor` lists used at template placement.
//! Vanilla `StructureProcessorType.DIRECT_CODEC` accepts either a bare array or
//! `{processors: [...]}`, and `SINGLE_CODEC` dispatches on `processor_type`. This mirrors
//! the codec for the four processors, five `RuleTest`s, one `PosRuleTest` and one
//! `RuleBlockEntityModifier` that appear in the 26.3 data. Parsing only; running them is
//! the placement stage's job.

use std::collections::BTreeMap;

use serde_json::Value;

/// `StructureProcessorType.LIST_CODEC` 的一个条目：注册名引用或内联列表。
///
/// One `StructureProcessorType.LIST_CODEC` entry: a registry reference or an inline list.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessorListRef {
    /// 注册名（`minecraft:ancient_city_generic_degradation`）。
    ///
    /// A registry name.
    Named(String),
    /// 内联的 processor 列表（`{processors: [...]}` 展开后的内容）。
    ///
    /// An inline processor list (the contents of `{processors: [...]}`).
    Inline(Vec<Processor>),
}

impl ProcessorListRef {
    /// 从 `pool_element.processors` 的原始 JSON 解析。
    ///
    /// Parse from a `pool_element.processors` raw JSON value.
    pub fn parse(raw: &Value) -> Result<Self, String> {
        match raw {
            Value::String(name) => Ok(Self::Named(name.clone())),
            Value::Object(fields) => {
                let list = fields
                    .get("processors")
                    .ok_or("inline processor list has no processors")?;
                Ok(Self::Inline(parse_processor_array(list)?))
            }
            Value::Array(_) => Ok(Self::Inline(parse_processor_array(raw)?)),
            other => Err(format!("bad processors {other:?}")),
        }
    }
}

/// 一个 `StructureProcessor`。
///
/// One `StructureProcessor`.
#[derive(Debug, Clone, PartialEq)]
pub enum Processor {
    /// `BlockRotProcessor`：按 `integrity` 随机把方块换成空气。
    ///
    /// `BlockRotProcessor`: randomly turns blocks into air by `integrity`.
    BlockRot {
        integrity: f32,
        /// `rottable_blocks`：方块标签（`#minecraft:x`）或方块 id 列表。
        ///
        /// `rottable_blocks`: a block tag (`#minecraft:x`) or a list of block ids.
        rottable_blocks: Option<BlockSetRef>,
    },
    /// `RuleProcessor`：按顺序试每条 `ProcessorRule`。
    ///
    /// `RuleProcessor`: tries each `ProcessorRule` in order.
    Rule { rules: Vec<ProcessorRule> },
    /// `ProtectedBlockProcessor`：`features_cannot_replace` 里的方块不替换。
    ///
    /// `ProtectedBlockProcessor`: blocks in `features_cannot_replace` are not replaced.
    ProtectedBlocks { value: BlockSetRef },
    /// `CappedProcessor`：最多应用 `limit` 次被包裹的 processor。
    ///
    /// `CappedProcessor`: applies the wrapped processor at most `limit` times.
    Capped {
        delegate: Box<Processor>,
        limit: i32,
    },
}

impl Processor {
    /// `StructureProcessor.evaluatesEntirePieceState()`：26.3 只有 `CappedProcessor`
    /// 返回 true，其余都是基类的 false。返回 true 时 `processBlockInfos` 会关掉
    /// 「只处理当前区块」的优化。
    ///
    /// `StructureProcessor.evaluatesEntirePieceState()`: in 26.3 only `CappedProcessor`
    /// returns true; the rest keep the default false. When true, `processBlockInfos`
    /// disables the "current chunk only" optimisation.
    pub const fn evaluates_entire_piece_state(&self) -> bool {
        matches!(self, Self::Capped { .. })
    }

    fn parse(raw: &Value) -> Result<Self, String> {
        let kind = raw
            .get("processor_type")
            .and_then(Value::as_str)
            .ok_or("processor has no processor_type")?;
        match kind {
            "minecraft:block_rot" => {
                let integrity = raw
                    .get("integrity")
                    .and_then(Value::as_f64)
                    .ok_or("block_rot has no integrity")? as f32;
                let rottable_blocks = raw
                    .get("rottable_blocks")
                    .map(BlockSetRef::parse)
                    .transpose()?;
                Ok(Self::BlockRot {
                    integrity,
                    rottable_blocks,
                })
            }
            "minecraft:rule" => {
                let rules = raw
                    .get("rules")
                    .and_then(Value::as_array)
                    .ok_or("rule processor has no rules")?
                    .iter()
                    .map(ProcessorRule::parse)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Rule { rules })
            }
            "minecraft:protected_blocks" => Ok(Self::ProtectedBlocks {
                value: BlockSetRef::parse(
                    raw.get("value").ok_or("protected_blocks has no value")?,
                )?,
            }),
            "minecraft:capped" => Ok(Self::Capped {
                delegate: Box::new(Self::parse(
                    raw.get("delegate").ok_or("capped has no delegate")?,
                )?),
                limit: raw
                    .get("limit")
                    .and_then(Value::as_i64)
                    .ok_or("capped has no limit")? as i32,
            }),
            other => Err(format!("unsupported processor {other}")),
        }
    }
}

/// 一条 `ProcessorRule`。
///
/// One `ProcessorRule`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessorRule {
    pub input_predicate: RuleTest,
    pub location_predicate: RuleTest,
    /// `position_predicate`，缺省是 `PosAlwaysTrueTest.INSTANCE`。
    ///
    /// `position_predicate`; defaults to `PosAlwaysTrueTest.INSTANCE`.
    pub position_predicate: PosRuleTest,
    /// `output_state`：方块 id 加可选属性。26.3 的 `BlockState.CODEC` 就是
    /// `{Name, Properties}` 或裸 id 字符串。
    ///
    /// `output_state`: a block id with optional properties. The 26.3 `BlockState.CODEC`
    /// is `{Name, Properties}` or a bare id string.
    pub output_state: BlockStateSpec,
    /// `block_entity_modifier`，缺省是 `Passthrough.INSTANCE`（不写时才用）。
    ///
    /// `block_entity_modifier`; defaults to `Passthrough.INSTANCE` (written only when set).
    pub block_entity_modifier: RuleBlockEntityModifier,
}

impl ProcessorRule {
    fn parse(raw: &Value) -> Result<Self, String> {
        Ok(Self {
            input_predicate: RuleTest::parse(
                raw.get("input_predicate")
                    .ok_or("rule has no input_predicate")?,
            )?,
            location_predicate: RuleTest::parse(
                raw.get("location_predicate")
                    .ok_or("rule has no location_predicate")?,
            )?,
            position_predicate: match raw.get("position_predicate") {
                Some(value) => PosRuleTest::parse(value)?,
                None => PosRuleTest::AlwaysTrue,
            },
            output_state: BlockStateSpec::parse(
                raw.get("output_state").ok_or("rule has no output_state")?,
            )?,
            block_entity_modifier: match raw.get("block_entity_modifier") {
                Some(value) => RuleBlockEntityModifier::parse(value)?,
                None => RuleBlockEntityModifier::Passthrough,
            },
        })
    }
}

/// `RuleTest.CODEC`：按 `predicate_type` 分发。
///
/// `RuleTest.CODEC`: dispatched on `predicate_type`.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleTest {
    AlwaysTrue,
    /// `BlockMatchTest`：`block` 必须是方块 id。
    ///
    /// `BlockMatchTest`: `block` must be a block id.
    BlockMatch {
        block: String,
    },
    /// `BlockStateMatchTest`：精确匹配整个方块状态。
    ///
    /// `BlockStateMatchTest`: matches the whole block state exactly.
    BlockStateMatch {
        block_state: BlockStateSpec,
    },
    /// `TagMatchTest`：`tag` 是不带 `#` 的标签名。
    ///
    /// `TagMatchTest`: `tag` is a tag name without the leading `#`.
    TagMatch {
        tag: String,
    },
    /// `RandomBlockMatchTest`：`block` 匹配时有 `probability` 的概率通过。
    ///
    /// `RandomBlockMatchTest`: passes with `probability` when `block` matches.
    RandomBlockMatch {
        block: String,
        probability: f32,
    },
    /// `RandomBlockStateMatchTest`：同上，但精确匹配状态。
    ///
    /// `RandomBlockStateMatchTest`: same, but matching the exact state.
    RandomBlockStateMatch {
        block_state: BlockStateSpec,
        probability: f32,
    },
}

impl RuleTest {
    fn parse(raw: &Value) -> Result<Self, String> {
        let kind = raw
            .get("predicate_type")
            .and_then(Value::as_str)
            .ok_or("rule test has no predicate_type")?;
        match kind {
            "minecraft:always_true" => Ok(Self::AlwaysTrue),
            "minecraft:block_match" => Ok(Self::BlockMatch {
                block: string_field(raw, "block")?,
            }),
            "minecraft:blockstate_match" => Ok(Self::BlockStateMatch {
                block_state: BlockStateSpec::parse(
                    raw.get("block_state")
                        .ok_or("blockstate_match has no block_state")?,
                )?,
            }),
            "minecraft:tag_match" => Ok(Self::TagMatch {
                tag: string_field(raw, "tag")?,
            }),
            "minecraft:random_block_match" => Ok(Self::RandomBlockMatch {
                block: string_field(raw, "block")?,
                probability: float_field(raw, "probability")?,
            }),
            "minecraft:random_blockstate_match" => Ok(Self::RandomBlockStateMatch {
                block_state: BlockStateSpec::parse(
                    raw.get("block_state")
                        .ok_or("random_blockstate_match has no block_state")?,
                )?,
                probability: float_field(raw, "probability")?,
            }),
            other => Err(format!("unsupported rule test {other}")),
        }
    }
}

/// `PosRuleTest.CODEC`。
///
/// `PosRuleTest.CODEC`.
#[derive(Debug, Clone, PartialEq)]
pub enum PosRuleTest {
    AlwaysTrue,
    /// `AxisAlignedLinearPosTest`：按 Y 轴线性概率。
    ///
    /// `AxisAlignedLinearPosTest`: linear probability along an axis.
    AxisAlignedLinear {
        min_chance: f32,
        max_chance: f32,
        min_dist: i32,
        max_dist: i32,
        axis: String,
    },
}

impl PosRuleTest {
    fn parse(raw: &Value) -> Result<Self, String> {
        let kind = raw
            .get("predicate_type")
            .and_then(Value::as_str)
            .ok_or("pos rule test has no predicate_type")?;
        match kind {
            "minecraft:always_true" => Ok(Self::AlwaysTrue),
            "minecraft:axis_aligned_linear_pos" => Ok(Self::AxisAlignedLinear {
                // `AxisAlignedLinearPosTest.CODEC` 五个字段全是 `optionalFieldOf`，
                // 默认 `0.0F` / `0.0F` / `0` / `0` / `Direction.Axis.Y`。
                // All five fields are `optionalFieldOf` with those defaults.
                min_chance: opt_float(raw, "min_chance", 0.0)?,
                max_chance: opt_float(raw, "max_chance", 0.0)?,
                min_dist: opt_int(raw, "min_dist", 0)?,
                max_dist: opt_int(raw, "max_dist", 0)?,
                axis: raw
                    .get("axis")
                    .and_then(Value::as_str)
                    .unwrap_or("y")
                    .to_owned(),
            }),
            other => Err(format!("unsupported pos rule test {other}")),
        }
    }
}

/// `RuleBlockEntityModifier.CODEC`。
///
/// `RuleBlockEntityModifier.CODEC`.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleBlockEntityModifier {
    /// `Passthrough`：保留原方块实体 NBT。
    ///
    /// `Passthrough`: keeps the original block-entity NBT.
    Passthrough,
    /// `AppendLoot`：往方块实体里追加战利品表。
    ///
    /// `AppendLoot`: appends a loot table to the block entity.
    AppendLoot { loot_table: String },
    /// `Clear`：清空方块实体。
    ///
    /// `Clear`: clears the block entity.
    Clear,
}

impl RuleBlockEntityModifier {
    fn parse(raw: &Value) -> Result<Self, String> {
        let kind = raw
            .get("type")
            .and_then(Value::as_str)
            .ok_or("block entity modifier has no type")?;
        match kind {
            "minecraft:passthrough" => Ok(Self::Passthrough),
            "minecraft:append_loot" => Ok(Self::AppendLoot {
                loot_table: string_field(raw, "loot_table")?,
            }),
            "minecraft:clear" => Ok(Self::Clear),
            other => Err(format!("unsupported block entity modifier {other}")),
        }
    }
}

/// `BlockState.CODEC`：方块 id 加属性。
///
/// `BlockState.CODEC`: a block id plus properties.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStateSpec {
    pub name: String,
    pub properties: Vec<(String, String)>,
}

impl BlockStateSpec {
    /// 从 `minecraft:oak_stairs[facing=north,half=bottom]` 这样的文本解析。
    /// 用于对拍与测试构造（JSON 走 `parse`）。
    ///
    /// Parse from text such as `minecraft:oak_stairs[facing=north,half=bottom]`, used by
    /// the oracle and tests (JSON goes through `parse`).
    pub fn parse_str(text: &str) -> Result<Self, String> {
        let (name, rest) = match text.find('[') {
            None => (text, ""),
            Some(index) => (
                &text[..index],
                text.get(index + 1..text.len().saturating_sub(1))
                    .ok_or("unterminated block state")?,
            ),
        };
        let mut properties = Vec::new();
        if !rest.is_empty() {
            for pair in rest.split(',') {
                let (key, value) = pair.split_once('=').ok_or("bad property pair")?;
                properties.push((key.to_owned(), value.to_owned()));
            }
        }
        Ok(Self {
            name: name.to_owned(),
            properties,
        })
    }

    /// 写回 `minecraft:oak_stairs[facing=north,half=bottom]` 形式的文本，属性顺序按
    /// 当前列表。官方 `BlockState.toString` 的属性顺序是属性定义顺序，对拍里两边都用
    /// 各自的状态表，所以这里只要求**双方一致**的规范化：按键名排序。
    ///
    /// The text form `minecraft:oak_stairs[facing=...]`, with properties sorted by name so
    /// both sides normalise identically.
    pub fn text(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let mut sorted = self.properties.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let body = sorted
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{}[{body}]", self.name)
    }

    fn parse(raw: &Value) -> Result<Self, String> {
        match raw {
            Value::String(name) => Ok(Self {
                name: name.clone(),
                properties: Vec::new(),
            }),
            Value::Object(fields) => {
                let name = fields
                    .get("Name")
                    .or_else(|| fields.get("id"))
                    .and_then(Value::as_str)
                    .ok_or("block state has no Name")?;
                let properties = match fields
                    .get("Properties")
                    .or_else(|| fields.get("properties"))
                {
                    Some(Value::Object(props)) => props
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|text| (key.clone(), text.to_owned()))
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Ok(Self {
                    name: name.to_owned(),
                    properties,
                })
            }
            other => Err(format!("bad block state {other:?}")),
        }
    }
}

/// 方块集合引用：`#minecraft:x` 标签或 id 列表。
///
/// A block set reference: a `#minecraft:x` tag or a list of ids.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockSetRef {
    Tag(String),
    Blocks(Vec<String>),
}

impl BlockSetRef {
    fn parse(raw: &Value) -> Result<Self, String> {
        match raw {
            Value::String(text) => Ok(Self::Tag(text.clone())),
            Value::Array(values) => Ok(Self::Blocks(
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("bad block id {value:?}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            other => Err(format!("bad block set {other:?}")),
        }
    }

    /// 去掉 `#` 前缀后的标签名；`Blocks` 时返回 `None`。
    ///
    /// The tag name with the leading `#` removed; `None` for `Blocks`.
    pub fn tag_name(&self) -> Option<&str> {
        match self {
            Self::Tag(text) => Some(text.strip_prefix('#').unwrap_or(text)),
            Self::Blocks(_) => None,
        }
    }
}

/// `worldgen.json` 的 `processor_list` 对象。
///
/// The `processor_list` object of `worldgen.json`.
#[derive(Debug, Clone)]
pub struct ProcessorLists {
    lists: BTreeMap<String, Vec<Processor>>,
}

impl ProcessorLists {
    /// 解析整张 `processor_list` 表。条目可以是裸数组或 `{processors: [...]}`。
    ///
    /// Parse the whole `processor_list` table. Entries are either a bare array or
    /// `{processors: [...]}`.
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("processor_list is not an object")?;
        let mut lists = BTreeMap::new();
        for (name, value) in object {
            lists.insert(name.clone(), parse_processor_array(value)?);
        }
        Ok(Self { lists })
    }

    pub fn get(&self, name: &str) -> Option<&[Processor]> {
        self.lists.get(name).map(Vec::as_slice)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.lists.keys()
    }

    pub fn len(&self) -> usize {
        self.lists.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }
}

/// `LIST_OBJECT_CODEC` 的两种形状：裸数组，或 `{processors: [...]}`。
///
/// The two shapes of `LIST_OBJECT_CODEC`: a bare array, or `{processors: [...]}`.
fn parse_processor_array(raw: &Value) -> Result<Vec<Processor>, String> {
    let array = match raw {
        Value::Array(values) => values.clone(),
        Value::Object(fields) => fields
            .get("processors")
            .and_then(Value::as_array)
            .cloned()
            .ok_or("processor list object has no processors")?,
        other => return Err(format!("bad processor list {other:?}")),
    };
    array.iter().map(Processor::parse).collect()
}

fn string_field(raw: &Value, key: &str) -> Result<String, String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing {key}"))
}

fn float_field(raw: &Value, key: &str) -> Result<f32, String> {
    raw.get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| format!("missing {key}"))
}

/// `optionalFieldOf(key, default)` 的 float 版本。
///
/// The float form of `optionalFieldOf(key, default)`.
fn opt_float(raw: &Value, key: &str, default: f32) -> Result<f32, String> {
    match raw.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .map(|number| number as f32)
            .ok_or_else(|| format!("bad {key}")),
    }
}

/// `optionalFieldOf(key, default)` 的 int 版本。
///
/// The int form of `optionalFieldOf(key, default)`.
fn opt_int(raw: &Value, key: &str, default: i32) -> Result<i32, String> {
    match raw.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_i64()
            .map(|number| number as i32)
            .ok_or_else(|| format!("bad {key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::worldgen::WORLDGEN;

    fn worldgen() -> Value {
        serde_json::from_str(WORLDGEN).unwrap()
    }

    /// 26.3 的全部 40 个 `processor_list` 都要能解析，且计数与 jar 里的条目一致。
    ///
    /// Every one of the 40 `processor_list` entries must parse with the expected counts.
    #[test]
    fn every_processor_list_parses() {
        let raw = worldgen();
        let lists = ProcessorLists::parse(raw.get("processor_list").unwrap()).unwrap();
        assert_eq!(lists.len(), 40);

        let mut counts = BTreeMap::new();
        for name in lists.names().cloned().collect::<Vec<_>>() {
            for processor in lists.get(&name).unwrap() {
                let key = match processor {
                    Processor::BlockRot { .. } => "block_rot",
                    Processor::Rule { .. } => "rule",
                    Processor::ProtectedBlocks { .. } => "protected_blocks",
                    Processor::Capped { .. } => "capped",
                };
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        // 顶层计数（`capped` 的 delegate 不重复计）；jar 里另有 4 个 `rule` 在
        // `capped.delegate` 里，由 `nested_rule_count` 单独验。
        // Top-level counts (`capped` delegates are not double counted); the jar has four
        // more `rule` processors nested in `capped.delegate`, checked separately.
        assert_eq!(counts.get("block_rot"), Some(&6));
        assert_eq!(counts.get("rule"), Some(&35));
        assert_eq!(counts.get("protected_blocks"), Some(&7));
        assert_eq!(counts.get("capped"), Some(&4));
        assert_eq!(nested_rule_count(&lists), 4);
    }

    /// 所有 `capped.delegate` 都是 `rule`（26.3 数据的实际情况）。
    ///
    /// Every `capped.delegate` is a `rule` (what the 26.3 data contains).
    fn nested_rule_count(lists: &ProcessorLists) -> usize {
        let mut nested = 0;
        for name in lists.names().cloned().collect::<Vec<_>>() {
            for processor in lists.get(&name).unwrap() {
                if let Processor::Capped { delegate, .. } = processor
                    && matches!(**delegate, Processor::Rule { .. })
                {
                    nested += 1;
                }
            }
        }
        nested
    }

    /// 所有 `RuleTest` 子类型都要被覆盖到（数据驱动，避免漏掉某个分支）。
    ///
    /// All `RuleTest` subtypes must be exercised (data driven, so no branch is missed).
    #[test]
    fn every_rule_test_kind_appears() {
        let raw = worldgen();
        let lists = ProcessorLists::parse(raw.get("processor_list").unwrap()).unwrap();
        let mut kinds = BTreeMap::new();
        for name in lists.names().cloned().collect::<Vec<_>>() {
            for processor in lists.get(&name).unwrap() {
                collect_rule_tests(processor, &mut kinds);
            }
        }
        for kind in [
            "always_true",
            "block_match",
            "blockstate_match",
            "tag_match",
            "random_block_match",
        ] {
            assert!(kinds.contains_key(kind), "missing rule test {kind}");
        }
    }

    fn collect_rule_tests(processor: &Processor, kinds: &mut BTreeMap<&'static str, usize>) {
        match processor {
            Processor::Rule { rules } => {
                for rule in rules {
                    for test in [&rule.input_predicate, &rule.location_predicate] {
                        let key = match test {
                            RuleTest::AlwaysTrue => "always_true",
                            RuleTest::BlockMatch { .. } => "block_match",
                            RuleTest::BlockStateMatch { .. } => "blockstate_match",
                            RuleTest::TagMatch { .. } => "tag_match",
                            RuleTest::RandomBlockMatch { .. } => "random_block_match",
                            RuleTest::RandomBlockStateMatch { .. } => "random_blockstate_match",
                        };
                        *kinds.entry(key).or_insert(0) += 1;
                    }
                }
            }
            Processor::Capped { delegate, .. } => collect_rule_tests(delegate, kinds),
            _ => {}
        }
    }

    /// `pool_element.processors` 的两种形状（注册名 / 内联对象）都要能解析。
    ///
    /// Both shapes of `pool_element.processors` (name / inline object) must parse.
    #[test]
    fn processor_ref_accepts_both_shapes() {
        let named = serde_json::json!("minecraft:ancient_city_generic_degradation");
        assert_eq!(
            ProcessorListRef::parse(&named).unwrap(),
            ProcessorListRef::Named("minecraft:ancient_city_generic_degradation".to_owned())
        );
        let inline = serde_json::json!({ "processors": [] });
        assert_eq!(
            ProcessorListRef::parse(&inline).unwrap(),
            ProcessorListRef::Inline(Vec::new())
        );
    }
}
