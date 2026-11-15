//! `PoolElementStructurePiece(context, tag)` 的读回：`pool_element` / `rotation` /
//! `junctions` / `liquid_settings`，并按官方重算包围盒。
//!
//! 官方读回顺序（`PoolElementStructurePiece.java` 54 行起）：
//!
//! 1. `super(StructurePieceType.JIGSAW, tag)`：`GD = tag.getIntOr("GD", 0)`、
//!    `BB = tag.read("BB", BoundingBox.CODEC).orElseThrow()`、`O` 解出朝向
//!    （`-1` 表示 null；jigsaw 结构件从不 `setOrientation`，所以是 `-1`）。
//! 2. `PosX` / `PosY` / `PosZ`（缺省 0）、`ground_level_delta`（缺省 0）。
//! 3. `pool_element`：`StructurePoolElement.CODEC`，读不出就抛异常（整个结构件读失败）。
//! 4. `rotation`：`Rotation.LEGACY_CODEC`，读不出就抛异常。
//! 5. **`boundingBox` 用 `element.getBoundingBox(manager, position, rotation)` 重算**，
//!    不用存档里的 `BB`。
//! 6. `junctions`：缺省空列表，每项 `JigsawJunction.deserialize`。
//! 7. `liquid_settings`：缺省 `JigsawStructure.DEFAULT_LIQUID_SETTINGS`。
//!
//! Read-back of `PoolElementStructurePiece(context, tag)`: `pool_element` / `rotation` /
//! `junctions` / `liquid_settings`, with the bounding box recomputed from the element the
//! way vanilla does (the saved `BB` is ignored).

use mc_bamboo_nbt::Tag;

use crate::structure::processor::{
    BlockSetRef, BlockStateSpec, PosRuleTest, Processor, ProcessorListRef, RuleTest,
};

use super::{
    element::{PoolElement, Projection},
    placer::{Junction, LiquidSettings, Piece},
    rotation::Rotation,
};

impl Piece {
    /// `PoolElementStructurePiece(context, tag)`：从存档标签读回一个 jigsaw 结构件。
    ///
    /// `None` 对应官方抛异常（`pool_element` 或 `rotation` 读不出）——调用方
    /// （`PiecesContainer.load`）会跳过这个结构件。
    ///
    /// `PoolElementStructurePiece(context, tag)`: read one jigsaw piece back from its save
    /// tag. `None` mirrors the vanilla throw (`pool_element` / `rotation` unreadable),
    /// which makes `PiecesContainer.load` skip the piece.
    pub fn from_save_tag(tag: &Tag) -> Result<Option<Self>, String> {
        let Tag::Compound(_) = tag else {
            return Ok(None);
        };
        // `pool_element` 与 `rotation` 是 `orElseThrow`，读不出就整体失败。
        // `pool_element` and `rotation` are `orElseThrow`: unreadable means the whole
        // piece fails.
        let Some(element_tag) = compound_field(tag, "pool_element") else {
            return Ok(None);
        };
        let Ok(element) = PoolElement::from_tag(element_tag) else {
            return Ok(None);
        };
        let Some(rotation_tag) = string_field(tag, "rotation") else {
            return Ok(None);
        };
        let Ok(rotation) = rotation_from_legacy_name(rotation_tag) else {
            return Ok(None);
        };

        let position = (
            int_field(tag, "PosX").unwrap_or(0),
            int_field(tag, "PosY").unwrap_or(0),
            int_field(tag, "PosZ").unwrap_or(0),
        );
        let ground_level_delta = int_field(tag, "ground_level_delta").unwrap_or(0);
        // 官方用 `element.getBoundingBox(manager, position, rotation)` 重算，忽略存档 `BB`。
        // Vanilla recomputes with `element.getBoundingBox(manager, position, rotation)` and
        // ignores the saved `BB`.
        let bounding_box = element.bounding_box(position, rotation)?;

        let mut junctions = Vec::new();
        if let Some(Tag::List(entries)) = field(tag, "junctions") {
            for entry in entries {
                junctions.push(junction_from_tag(entry)?);
            }
        }

        let liquid_settings = match string_field(tag, "liquid_settings") {
            Some(name) => LiquidSettings::parse(name)?,
            None => LiquidSettings::DEFAULT,
        };

        Ok(Some(Self {
            element,
            position,
            rotation,
            bounding_box,
            ground_level_delta,
            junctions,
            liquid_settings,
        }))
    }
}

impl PoolElement {
    /// `StructurePoolElement.CODEC` 从 NBT 读回。`processor` 列表按
    /// `StructureProcessorType.LIST_CODEC` 的两种形状解析。
    ///
    /// `StructurePoolElement.CODEC` read back from NBT. The processor list follows the
    /// two shapes of `StructureProcessorType.LIST_CODEC`.
    pub fn from_tag(tag: &Tag) -> Result<Self, String> {
        let element_type = string_field(tag, "element_type").ok_or("pool element has no type")?;
        match element_type {
            "minecraft:empty_pool_element" => Ok(Self::Empty),
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                let location = string_field(tag, "location")
                    .ok_or("single pool element has no location")?
                    .to_owned();
                let projection = projection_from_name(
                    string_field(tag, "projection")
                        .ok_or("single pool element has no projection")?,
                )?;
                let processors = match field(tag, "processors") {
                    Some(value) => processor_ref_from_tag(value)?,
                    None => return Err("single pool element has no processors".to_owned()),
                };
                Ok(Self::Single {
                    location,
                    projection,
                    legacy: element_type == "minecraft:legacy_single_pool_element",
                    processors,
                    override_liquid_settings: string_field(tag, "override_liquid_settings")
                        .map(str::to_owned),
                })
            }
            "minecraft:list_pool_element" => {
                let elements = match field(tag, "elements") {
                    Some(Tag::List(entries)) => entries
                        .iter()
                        .map(Self::from_tag)
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err("list pool element has no elements".to_owned()),
                };
                // `ListPoolElement`'s constructor throws for an empty list, so read-back
                // must reject it too.
                if elements.is_empty() {
                    return Err("list pool element has an empty elements list".to_owned());
                }
                Ok(Self::List {
                    elements,
                    projection: projection_from_name(
                        string_field(tag, "projection")
                            .ok_or("list pool element has no projection")?,
                    )?,
                })
            }
            "minecraft:feature_pool_element" => Ok(Self::Feature {
                feature: string_field(tag, "feature")
                    .ok_or("feature pool element has no feature")?
                    .to_owned(),
                projection: projection_from_name(
                    string_field(tag, "projection")
                        .ok_or("feature pool element has no projection")?,
                )?,
            }),
            other => Err(format!("unsupported pool element {other}")),
        }
    }
}

/// `Rotation.LEGACY_CODEC` 读的是枚举常量名。
///
/// `Rotation.LEGACY_CODEC` reads the enum constant name.
pub fn rotation_from_legacy_name(name: &str) -> Result<Rotation, String> {
    match name {
        "NONE" => Ok(Rotation::None),
        "CLOCKWISE_90" => Ok(Rotation::Clockwise90),
        "CLOCKWISE_180" => Ok(Rotation::Clockwise180),
        "COUNTERCLOCKWISE_90" => Ok(Rotation::Counterclockwise90),
        other => Err(format!("bad rotation {other}")),
    }
}

/// `JigsawJunction.deserialize`：五个键都有缺省值。
///
/// `JigsawJunction.deserialize`: every key has a default.
fn junction_from_tag(tag: &Tag) -> Result<Junction, String> {
    Ok(Junction {
        source_x: int_field(tag, "source_x").unwrap_or(0),
        source_ground_y: int_field(tag, "source_ground_y").unwrap_or(0),
        source_z: int_field(tag, "source_z").unwrap_or(0),
        delta_y: int_field(tag, "delta_y").unwrap_or(0),
        dest_projection: projection_from_name(string_field(tag, "dest_proj").unwrap_or_default())?,
    })
}

fn projection_from_name(name: &str) -> Result<Projection, String> {
    match name {
        "rigid" => Ok(Projection::Rigid),
        "terrain_matching" => Ok(Projection::TerrainMatching),
        other => Err(format!("bad projection {other}")),
    }
}

/// `StructureProcessorType.LIST_CODEC`：注册名或 `{processors: [...]}` 复合。
///
/// `StructureProcessorType.LIST_CODEC`: a registry name or a `{processors: [...]}` compound.
fn processor_ref_from_tag(tag: &Tag) -> Result<ProcessorListRef, String> {
    match tag {
        Tag::String(name) => Ok(ProcessorListRef::Named(name.clone())),
        Tag::Compound(_) => {
            let entries = match field(tag, "processors") {
                Some(Tag::List(entries)) => entries,
                _ => return Err("inline processors has no list".to_owned()),
            };
            Ok(ProcessorListRef::Inline(
                entries
                    .iter()
                    .map(processor_from_tag)
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        other => Err(format!("bad processors {other:?}")),
    }
}

/// `StructureProcessorType.SINGLE_CODEC`。
///
/// `StructureProcessorType.SINGLE_CODEC`.
pub fn processor_from_tag(tag: &Tag) -> Result<Processor, String> {
    let kind = string_field(tag, "processor_type").ok_or("processor has no processor_type")?;
    match kind {
        "minecraft:block_rot" => Ok(Processor::BlockRot {
            integrity: float_field(tag, "integrity").ok_or("block_rot has no integrity")?,
            rottable_blocks: field(tag, "rottable_blocks")
                .map(block_set_from_tag)
                .transpose()?,
        }),
        "minecraft:rule" => {
            let rules = match field(tag, "rules") {
                Some(Tag::List(entries)) => entries
                    .iter()
                    .map(processor_rule_from_tag)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err("rule processor has no rules".to_owned()),
            };
            Ok(Processor::Rule { rules })
        }
        "minecraft:protected_blocks" => Ok(Processor::ProtectedBlocks {
            value: block_set_from_tag(field(tag, "value").ok_or("protected_blocks has no value")?)?,
        }),
        "minecraft:capped" => Ok(Processor::Capped {
            delegate: Box::new(processor_from_tag(
                field(tag, "delegate").ok_or("capped has no delegate")?,
            )?),
            limit: int_field(tag, "limit").ok_or("capped has no limit")?,
        }),
        other => Err(format!("unsupported processor {other}")),
    }
}

fn processor_rule_from_tag(
    tag: &Tag,
) -> Result<crate::structure::processor::ProcessorRule, String> {
    use crate::structure::processor::{ProcessorRule, RuleBlockEntityModifier};
    Ok(ProcessorRule {
        input_predicate: rule_test_from_tag(
            field(tag, "input_predicate").ok_or("rule has no input_predicate")?,
        )?,
        location_predicate: rule_test_from_tag(
            field(tag, "location_predicate").ok_or("rule has no location_predicate")?,
        )?,
        position_predicate: match field(tag, "position_predicate") {
            Some(value) => pos_rule_test_from_tag(value)?,
            None => PosRuleTest::AlwaysTrue,
        },
        output_state: block_state_from_tag(
            field(tag, "output_state").ok_or("rule has no output_state")?,
        )?,
        block_entity_modifier: match field(tag, "block_entity_modifier") {
            Some(value) => block_entity_modifier_from_tag(value)?,
            None => RuleBlockEntityModifier::Passthrough,
        },
    })
}

fn rule_test_from_tag(tag: &Tag) -> Result<RuleTest, String> {
    let kind = string_field(tag, "predicate_type").ok_or("rule test has no predicate_type")?;
    match kind {
        "minecraft:always_true" => Ok(RuleTest::AlwaysTrue),
        "minecraft:block_match" => Ok(RuleTest::BlockMatch {
            block: string_field(tag, "block")
                .ok_or("block_match has no block")?
                .to_owned(),
        }),
        "minecraft:blockstate_match" => Ok(RuleTest::BlockStateMatch {
            block_state: block_state_from_tag(
                field(tag, "block_state").ok_or("blockstate_match has no block_state")?,
            )?,
        }),
        "minecraft:tag_match" => Ok(RuleTest::TagMatch {
            tag: string_field(tag, "tag")
                .ok_or("tag_match has no tag")?
                .to_owned(),
        }),
        "minecraft:random_block_match" => Ok(RuleTest::RandomBlockMatch {
            block: string_field(tag, "block")
                .ok_or("random_block_match has no block")?
                .to_owned(),
            probability: float_field(tag, "probability")
                .ok_or("random_block_match has no probability")?,
        }),
        "minecraft:random_blockstate_match" => Ok(RuleTest::RandomBlockStateMatch {
            block_state: block_state_from_tag(
                field(tag, "block_state").ok_or("random_blockstate_match has no block_state")?,
            )?,
            probability: float_field(tag, "probability")
                .ok_or("random_blockstate_match has no probability")?,
        }),
        other => Err(format!("unsupported rule test {other}")),
    }
}

fn pos_rule_test_from_tag(tag: &Tag) -> Result<PosRuleTest, String> {
    let kind = string_field(tag, "predicate_type").ok_or("pos rule test has no predicate_type")?;
    match kind {
        "minecraft:always_true" => Ok(PosRuleTest::AlwaysTrue),
        "minecraft:axis_aligned_linear_pos" => Ok(PosRuleTest::AxisAlignedLinear {
            min_chance: float_field(tag, "min_chance").unwrap_or(0.0),
            max_chance: float_field(tag, "max_chance").unwrap_or(0.0),
            min_dist: int_field(tag, "min_dist").unwrap_or(0),
            max_dist: int_field(tag, "max_dist").unwrap_or(0),
            axis: string_field(tag, "axis").unwrap_or("y").to_owned(),
        }),
        other => Err(format!("unsupported pos rule test {other}")),
    }
}

fn block_entity_modifier_from_tag(
    tag: &Tag,
) -> Result<crate::structure::processor::RuleBlockEntityModifier, String> {
    use crate::structure::processor::RuleBlockEntityModifier;
    match string_field(tag, "type").ok_or("block entity modifier has no type")? {
        "minecraft:passthrough" => Ok(RuleBlockEntityModifier::Passthrough),
        "minecraft:append_loot" => Ok(RuleBlockEntityModifier::AppendLoot {
            loot_table: string_field(tag, "loot_table")
                .ok_or("append_loot has no loot_table")?
                .to_owned(),
        }),
        "minecraft:clear" => Ok(RuleBlockEntityModifier::Clear),
        other => Err(format!("unsupported block entity modifier {other}")),
    }
}

fn block_state_from_tag(tag: &Tag) -> Result<BlockStateSpec, String> {
    match tag {
        Tag::String(name) => Ok(BlockStateSpec {
            name: name.clone(),
            properties: Vec::new(),
        }),
        Tag::Compound(_) => {
            let name = string_field(tag, "id")
                .or_else(|| string_field(tag, "Name"))
                .ok_or("block state has no id")?
                .to_owned();
            let properties = match field(tag, "properties").or_else(|| field(tag, "Properties")) {
                Some(Tag::Compound(entries)) => entries
                    .iter()
                    .filter_map(|(key, value)| match value {
                        Tag::String(text) => Some((key.clone(), text.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok(BlockStateSpec { name, properties })
        }
        other => Err(format!("bad block state {other:?}")),
    }
}

fn block_set_from_tag(tag: &Tag) -> Result<BlockSetRef, String> {
    match tag {
        Tag::String(text) => Ok(BlockSetRef::Tag(text.clone())),
        Tag::List(entries) => Ok(BlockSetRef::Blocks(
            entries
                .iter()
                .filter_map(|entry| match entry {
                    Tag::String(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
        )),
        other => Err(format!("bad block set {other:?}")),
    }
}

fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    compound_field(tag, key)
}

fn compound_field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn string_field<'a>(tag: &'a Tag, key: &str) -> Option<&'a str> {
    match field(tag, key) {
        Some(Tag::String(text)) => Some(text),
        _ => None,
    }
}

fn int_field(tag: &Tag, key: &str) -> Option<i32> {
    match field(tag, key) {
        Some(Tag::Byte(value)) => Some(i32::from(*value)),
        Some(Tag::Short(value)) => Some(i32::from(*value)),
        Some(Tag::Int(value)) => Some(*value),
        _ => None,
    }
}

fn float_field(tag: &Tag, key: &str) -> Option<f32> {
    match field(tag, key) {
        Some(Tag::Float(value)) => Some(*value),
        Some(Tag::Double(value)) => Some(*value as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{
        jigsaw::{element::Projection, placer::LiquidSettings, rotation::Rotation},
        start::BoundingBox,
    };

    fn piece_tag() -> Tag {
        Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:jigsaw".to_owned())),
            (
                "BB".to_owned(),
                Tag::IntArray(vec![-20, -29, -18, -1, -29, 0]),
            ),
            ("O".to_owned(), Tag::Int(-1)),
            ("GD".to_owned(), Tag::Int(0)),
            ("PosX".to_owned(), Tag::Int(-20)),
            ("PosY".to_owned(), Tag::Int(-29)),
            ("PosZ".to_owned(), Tag::Int(0)),
            ("ground_level_delta".to_owned(), Tag::Int(1)),
            (
                "pool_element".to_owned(),
                Tag::Compound(vec![
                    (
                        "element_type".to_owned(),
                        Tag::String("minecraft:single_pool_element".to_owned()),
                    ),
                    (
                        "location".to_owned(),
                        Tag::String("minecraft:empty".to_owned()),
                    ),
                    ("projection".to_owned(), Tag::String("rigid".to_owned())),
                    (
                        "processors".to_owned(),
                        Tag::Compound(vec![("processors".to_owned(), Tag::List(Vec::new()))]),
                    ),
                ]),
            ),
            (
                "rotation".to_owned(),
                Tag::String("COUNTERCLOCKWISE_90".to_owned()),
            ),
            (
                "junctions".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    ("source_x".to_owned(), Tag::Int(0)),
                    ("source_ground_y".to_owned(), Tag::Int(-28)),
                    ("source_z".to_owned(), Tag::Int(-9)),
                    ("delta_y".to_owned(), Tag::Int(0)),
                    ("dest_proj".to_owned(), Tag::String("rigid".to_owned())),
                ])]),
            ),
            (
                "liquid_settings".to_owned(),
                Tag::String("ignore_waterlogging".to_owned()),
            ),
        ])
    }

    /// 空的 `list_pool_element` 在读回时也必须被拒绝（官方构造器抛异常）。
    ///
    /// An empty `list_pool_element` must be rejected on read-back too (vanilla's
    /// constructor throws).
    #[test]
    fn empty_list_pool_element_is_rejected_on_read_back() {
        let empty_list = Tag::Compound(vec![
            (
                "element_type".to_owned(),
                Tag::String("minecraft:list_pool_element".to_owned()),
            ),
            ("elements".to_owned(), Tag::List(Vec::new())),
            ("projection".to_owned(), Tag::String("rigid".to_owned())),
        ]);
        assert!(PoolElement::from_tag(&empty_list).is_err());

        // 非空列表仍然可以读回。
        // A non-empty list still reads back.
        let one = Tag::Compound(vec![
            (
                "element_type".to_owned(),
                Tag::String("minecraft:single_pool_element".to_owned()),
            ),
            (
                "location".to_owned(),
                Tag::String("minecraft:empty".to_owned()),
            ),
            ("projection".to_owned(), Tag::String("rigid".to_owned())),
            (
                "processors".to_owned(),
                Tag::Compound(vec![("processors".to_owned(), Tag::List(Vec::new()))]),
            ),
        ]);
        let non_empty = Tag::Compound(vec![
            (
                "element_type".to_owned(),
                Tag::String("minecraft:list_pool_element".to_owned()),
            ),
            ("elements".to_owned(), Tag::List(vec![one])),
            ("projection".to_owned(), Tag::String("rigid".to_owned())),
        ]);
        assert!(PoolElement::from_tag(&non_empty).is_ok());
    }

    /// 全部子类字段都要读回，且包围盒按元素重算（不是存档里的 `BB`）。
    ///
    /// Every subclass field must come back, and the bounding box is recomputed from the
    /// element rather than taken from the saved `BB`.
    #[test]
    fn reads_every_subclass_field() {
        let piece = Piece::from_save_tag(&piece_tag()).unwrap().unwrap();
        assert_eq!(piece.position, (-20, -29, 0));
        assert_eq!(piece.ground_level_delta, 1);
        assert_eq!(piece.rotation, Rotation::Counterclockwise90);
        assert_eq!(piece.junctions.len(), 1);
        assert_eq!(piece.junctions[0].source_ground_y, -28);
        assert_eq!(piece.junctions[0].dest_projection, Projection::Rigid);
        assert_eq!(piece.liquid_settings, LiquidSettings::IgnoreWaterlogging);
        // `minecraft:empty` 的 size 是 0，所以重算的盒是单点，和存档 `BB` 不同。
        // `minecraft:empty` has size 0, so the recomputed box is a single point and
        // differs from the saved `BB`.
        assert_eq!(
            piece.bounding_box,
            BoundingBox {
                min_x: -20,
                min_y: -29,
                min_z: 0,
                max_x: -20,
                max_y: -29,
                max_z: 0,
            }
        );
    }

    /// `liquid_settings` 缺省时是 `apply_waterlogging`。
    ///
    /// A missing `liquid_settings` means `apply_waterlogging`.
    #[test]
    fn missing_liquid_settings_defaults_to_apply() {
        let Tag::Compound(mut entries) = piece_tag() else {
            panic!("fixture must be a compound");
        };
        entries.retain(|(key, _)| key != "liquid_settings");
        let piece = Piece::from_save_tag(&Tag::Compound(entries))
            .unwrap()
            .unwrap();
        assert_eq!(piece.liquid_settings, LiquidSettings::DEFAULT);
    }

    /// 读不出 `pool_element` / `rotation` 时整体失败（官方 `orElseThrow`）。
    ///
    /// An unreadable `pool_element` / `rotation` fails the whole piece (`orElseThrow`).
    #[test]
    fn missing_pool_element_or_rotation_fails() {
        let Tag::Compound(entries) = piece_tag() else {
            panic!("fixture must be a compound");
        };
        let without = |skip: &str| {
            Tag::Compound(
                entries
                    .iter()
                    .filter(|(key, _)| key != skip)
                    .cloned()
                    .collect(),
            )
        };
        assert_eq!(
            Piece::from_save_tag(&without("pool_element")).unwrap(),
            None
        );
        assert_eq!(Piece::from_save_tag(&without("rotation")).unwrap(), None);
    }

    /// `rotation` 的四个枚举常量名都要能读回。
    ///
    /// All four rotation constant names must round-trip.
    #[test]
    fn rotation_legacy_names_round_trip() {
        for rotation in Rotation::ALL {
            let name = crate::structure::jigsaw::save_piece::rotation_legacy_name(rotation);
            assert_eq!(rotation_from_legacy_name(name), Ok(rotation));
        }
    }

    /// 存档的 `BB` 被**忽略**：用一个明显错误的 `BB` 也必须得到元素重算的盒。
    /// 这条钉住官方的 `this.boundingBox = this.element.getBoundingBox(...)`。
    ///
    /// The saved `BB` is **ignored**: even a deliberately wrong `BB` must yield the box
    /// recomputed from the element, pinning vanilla's
    /// `this.boundingBox = this.element.getBoundingBox(...)`.
    #[test]
    fn saved_bounding_box_is_ignored_in_favour_of_the_element() {
        let Tag::Compound(mut entries) = piece_tag() else {
            panic!("fixture must be a compound");
        };
        for entry in &mut entries {
            if entry.0 == "BB" {
                entry.1 = Tag::IntArray(vec![999, 999, 999, 1000, 1000, 1000]);
            }
        }
        let piece = Piece::from_save_tag(&Tag::Compound(entries))
            .unwrap()
            .unwrap();
        assert_eq!(
            piece.bounding_box,
            BoundingBox {
                min_x: -20,
                min_y: -29,
                min_z: 0,
                max_x: -20,
                max_y: -29,
                max_z: 0,
            }
        );
    }
}
