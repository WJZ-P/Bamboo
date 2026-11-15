//! `PoolElementStructurePiece.addAdditionalSaveData` 的完整编码：`pool_element` /
//! `rotation` / `junctions` / `liquid_settings`，外加基类四字段。
//!
//! 官方 `PoolElementStructurePiece` 的存档字段（基类 `StructurePiece.createTag` 的
//! `id`/`BB`/`O`/`GD` 之外）：
//!
//! * `PosX` / `PosY` / `PosZ`：`BlockPos` 三点；
//! * `ground_level_delta`：int；
//! * `pool_element`：`StructurePoolElement.CODEC` 编出的对象。`single` / `legacy` 是
//!   `{element_type, location, processors, projection}`（`override_liquid_settings`
//!   只在有值时写），`list` 是 `{element_type, elements, projection}`，
//!   `feature` 是 `{element_type, feature, projection}`，`empty` 是 `{element_type}`；
//! * `rotation`：`Rotation.LEGACY_CODEC`（`ExtraCodecs.legacyEnum`，写枚举常量名，
//!   即 `NONE` / `CLOCKWISE_90` / `CLOCKWISE_180` / `COUNTERCLOCKWISE_90`）；
//! * `junctions`：列表，每项 `{source_x, source_ground_y, source_z, delta_y, dest_proj}`；
//! * `liquid_settings`：只在不是默认值 `apply_waterlogging` 时才写。
//!
//! Full `PoolElementStructurePiece.addAdditionalSaveData` encoding: `pool_element` /
//! `rotation` / `junctions` / `liquid_settings` on top of the four base fields.

use mc_bamboo_nbt::Tag;

use super::{
    element::PoolElement,
    placer::{Junction, LiquidSettings, Piece},
    rotation::Rotation,
};
use crate::structure::{
    processor::{
        BlockSetRef, BlockStateSpec, PosRuleTest, Processor, ProcessorListRef, ProcessorRule,
        RuleBlockEntityModifier, RuleTest,
    },
    save::SavePiece,
};

impl Piece {
    /// `PoolElementStructurePiece.addAdditionalSaveData` 的全部键。
    ///
    /// Every key of `PoolElementStructurePiece.addAdditionalSaveData`.
    pub fn save_extras(&self) -> Result<Vec<(String, Tag)>, String> {
        let mut extras = vec![
            ("PosX".to_owned(), Tag::Int(self.position.0)),
            ("PosY".to_owned(), Tag::Int(self.position.1)),
            ("PosZ".to_owned(), Tag::Int(self.position.2)),
            (
                "ground_level_delta".to_owned(),
                Tag::Int(self.ground_level_delta),
            ),
            ("pool_element".to_owned(), self.element.to_tag()?),
            (
                "rotation".to_owned(),
                Tag::String(rotation_legacy_name(self.rotation).to_owned()),
            ),
            ("junctions".to_owned(), junctions_tag(&self.junctions)),
        ];
        // 默认值不写（`if (this.liquidSettings != DEFAULT_LIQUID_SETTINGS)`）。
        // The default is omitted (`if (this.liquidSettings != DEFAULT_LIQUID_SETTINGS)`).
        if self.liquid_settings != LiquidSettings::DEFAULT {
            extras.push((
                "liquid_settings".to_owned(),
                Tag::String(self.liquid_settings.name().to_owned()),
            ));
        }
        Ok(extras)
    }

    /// 带全部子类字段的存档结构件，可直接交给 `structure::save`。
    ///
    /// The save-side piece with every subclass field, ready for `structure::save`.
    pub fn to_full_save_piece(&self) -> Result<SavePiece, String> {
        Ok(SavePiece {
            id: "minecraft:jigsaw".to_owned(),
            bb: self.bounding_box,
            // `setOrientation` 从未被调用，`O` 写 -1。
            // `setOrientation` is never called, so `O` is written as -1.
            orientation: -1,
            // `super(StructurePieceType.JIGSAW, 0, ...)`：`GD` 恒为 0。
            // `super(StructurePieceType.JIGSAW, 0, ...)`: `GD` is always 0.
            gen_depth: 0,
            extras: self.save_extras()?,
        })
    }
}

impl PoolElement {
    /// `StructurePoolElement.CODEC` 在 NBT 里的对象形状。
    ///
    /// The object shape of `StructurePoolElement.CODEC` in NBT.
    pub fn to_tag(&self) -> Result<Tag, String> {
        let mut fields = Vec::new();
        match self {
            Self::Empty => {
                fields.push((
                    "element_type".to_owned(),
                    Tag::String("minecraft:empty_pool_element".to_owned()),
                ));
            }
            Self::Single {
                location,
                projection,
                legacy,
                processors,
                override_liquid_settings,
            } => {
                let element_type = if *legacy {
                    "minecraft:legacy_single_pool_element"
                } else {
                    "minecraft:single_pool_element"
                };
                fields.push((
                    "element_type".to_owned(),
                    Tag::String(element_type.to_owned()),
                ));
                fields.push(("location".to_owned(), Tag::String(location.clone())));
                fields.push((
                    "projection".to_owned(),
                    Tag::String(projection.name().to_owned()),
                ));
                fields.push(("processors".to_owned(), processor_ref_tag(processors)?));
                if let Some(name) = override_liquid_settings {
                    fields.push((
                        "override_liquid_settings".to_owned(),
                        Tag::String(name.clone()),
                    ));
                }
            }
            Self::List {
                elements,
                projection,
            } => {
                fields.push((
                    "element_type".to_owned(),
                    Tag::String("minecraft:list_pool_element".to_owned()),
                ));
                fields.push((
                    "elements".to_owned(),
                    Tag::List(
                        elements
                            .iter()
                            .map(Self::to_tag)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                ));
                fields.push((
                    "projection".to_owned(),
                    Tag::String(projection.name().to_owned()),
                ));
            }
            Self::Feature {
                feature,
                projection,
            } => {
                fields.push((
                    "element_type".to_owned(),
                    Tag::String("minecraft:feature_pool_element".to_owned()),
                ));
                fields.push(("feature".to_owned(), Tag::String(feature.clone())));
                fields.push((
                    "projection".to_owned(),
                    Tag::String(projection.name().to_owned()),
                ));
            }
        }
        Ok(Tag::Compound(fields))
    }
}

/// `Rotation.LEGACY_CODEC` 写的是枚举常量名（`Enum::toString`），不是 `CODEC` 的
/// `none`/`clockwise_90` 那套。
///
/// `Rotation.LEGACY_CODEC` writes the enum constant name (`Enum::toString`), not the
/// `none`/`clockwise_90` names used by `CODEC`.
pub fn rotation_legacy_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "NONE",
        Rotation::Clockwise90 => "CLOCKWISE_90",
        Rotation::Clockwise180 => "CLOCKWISE_180",
        Rotation::Counterclockwise90 => "COUNTERCLOCKWISE_90",
    }
}

/// `JigsawJunction.serialize`。
///
/// `JigsawJunction.serialize`.
pub fn junction_tag(junction: &Junction) -> Tag {
    Tag::Compound(vec![
        ("source_x".to_owned(), Tag::Int(junction.source_x)),
        (
            "source_ground_y".to_owned(),
            Tag::Int(junction.source_ground_y),
        ),
        ("source_z".to_owned(), Tag::Int(junction.source_z)),
        ("delta_y".to_owned(), Tag::Int(junction.delta_y)),
        (
            "dest_proj".to_owned(),
            Tag::String(junction.dest_projection.name().to_owned()),
        ),
    ])
}

fn junctions_tag(junctions: &[Junction]) -> Tag {
    Tag::List(junctions.iter().map(junction_tag).collect())
}

/// `StructureProcessorType.LIST_CODEC` 在 NBT 里的形状：注册名写字符串，内联列表写成
/// `{processors: [...]}` 复合（`DIRECT_CODEC` 的 `fieldOf("processors")` 形式）。
///
/// The NBT shape of `StructureProcessorType.LIST_CODEC`: a registry name as a string, an
/// inline list as a `{processors: [...]}` compound (the `DIRECT_CODEC` form with
/// `fieldOf("processors")`).
fn processor_ref_tag(reference: &ProcessorListRef) -> Result<Tag, String> {
    match reference {
        ProcessorListRef::Named(name) => Ok(Tag::String(name.clone())),
        ProcessorListRef::Inline(list) => Ok(Tag::Compound(vec![(
            "processors".to_owned(),
            Tag::List(
                list.iter()
                    .map(|processor| processor.to_tag())
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )])),
    }
}

impl Processor {
    /// `StructureProcessorType.SINGLE_CODEC` 在 NBT 里的对象形状。
    ///
    /// The object shape of `StructureProcessorType.SINGLE_CODEC` in NBT.
    pub fn to_tag(&self) -> Result<Tag, String> {
        let mut fields = Vec::new();
        match self {
            Self::BlockRot {
                integrity,
                rottable_blocks,
            } => {
                fields.push((
                    "processor_type".to_owned(),
                    Tag::String("minecraft:block_rot".to_owned()),
                ));
                // `RegistryCodecs.holderSet(Registries.BLOCK).optionalFieldOf(...)`：有值才写，
                // 标签写成 `#minecraft:x`，方块列表写成 id 数组。
                // Written only when present; a tag becomes `#minecraft:x`, a list becomes ids.
                if let Some(set) = rottable_blocks {
                    fields.push(("rottable_blocks".to_owned(), block_set_tag(set)));
                }
                fields.push(("integrity".to_owned(), Tag::Float(*integrity)));
            }
            Self::Rule { rules } => {
                fields.push((
                    "processor_type".to_owned(),
                    Tag::String("minecraft:rule".to_owned()),
                ));
                fields.push((
                    "rules".to_owned(),
                    Tag::List(rules.iter().map(ProcessorRule::to_tag).collect()),
                ));
            }
            Self::ProtectedBlocks { value } => {
                fields.push((
                    "processor_type".to_owned(),
                    Tag::String("minecraft:protected_blocks".to_owned()),
                ));
                fields.push(("value".to_owned(), block_set_tag(value)));
            }
            Self::Capped { delegate, limit } => {
                fields.push((
                    "processor_type".to_owned(),
                    Tag::String("minecraft:capped".to_owned()),
                ));
                fields.push(("delegate".to_owned(), delegate.to_tag()?));
                // `IntProviders.POSITIVE_CODEC` 的 `constant` 形式是裸 int。
                // The `constant` form of `IntProviders.POSITIVE_CODEC` is a bare int.
                fields.push(("limit".to_owned(), Tag::Int(*limit)));
            }
            Self::BlockIgnore { to_ignore } => {
                fields.push((
                    "processor_type".to_owned(),
                    Tag::String("minecraft:block_ignore".to_owned()),
                ));
                // `WEIRD_BLOCK_STATE_CODEC` 编码时只写方块默认状态（`Block::defaultBlockState`）。
                // `WEIRD_BLOCK_STATE_CODEC` writes the block's default state.
                fields.push((
                    "blocks".to_owned(),
                    Tag::List(
                        to_ignore
                            .iter()
                            .map(|name| Tag::String(name.clone()))
                            .collect(),
                    ),
                ));
            }
            Self::JigsawReplacement => fields.push((
                "processor_type".to_owned(),
                Tag::String("minecraft:jigsaw_replacement".to_owned()),
            )),
        }
        Ok(Tag::Compound(fields))
    }
}

impl ProcessorRule {
    fn to_tag(&self) -> Tag {
        let mut fields = vec![
            ("input_predicate".to_owned(), self.input_predicate.to_tag()),
            (
                "location_predicate".to_owned(),
                self.location_predicate.to_tag(),
            ),
            ("output_state".to_owned(), self.output_state.to_tag()),
        ];
        // `position_predicate` / `block_entity_modifier` 用 `lenientOptionalFieldOf`，
        // 默认值（`always_true` / `passthrough`）不写。
        // Both use `lenientOptionalFieldOf`; defaults (`always_true` / `passthrough`) are
        // omitted.
        if self.position_predicate != PosRuleTest::AlwaysTrue {
            fields.push((
                "position_predicate".to_owned(),
                self.position_predicate.to_tag(),
            ));
        }
        if self.block_entity_modifier != RuleBlockEntityModifier::Passthrough {
            fields.push((
                "block_entity_modifier".to_owned(),
                self.block_entity_modifier.to_tag(),
            ));
        }
        Tag::Compound(fields)
    }
}

impl RuleTest {
    /// `RuleTest.CODEC` 的对象形状。`always_true` 只有 `predicate_type`。
    ///
    /// The object shape of `RuleTest.CODEC`; `always_true` has only `predicate_type`.
    pub fn to_tag(&self) -> Tag {
        let mut fields = Vec::new();
        match self {
            Self::AlwaysTrue => fields.push((
                "predicate_type".to_owned(),
                Tag::String("minecraft:always_true".to_owned()),
            )),
            Self::BlockMatch { block } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:block_match".to_owned()),
                ));
                fields.push(("block".to_owned(), Tag::String(block.clone())));
            }
            Self::BlockStateMatch { block_state } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:blockstate_match".to_owned()),
                ));
                fields.push(("block_state".to_owned(), block_state.to_tag()));
            }
            Self::TagMatch { tag } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:tag_match".to_owned()),
                ));
                fields.push(("tag".to_owned(), Tag::String(tag.clone())));
            }
            Self::RandomBlockMatch { block, probability } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:random_block_match".to_owned()),
                ));
                fields.push(("block".to_owned(), Tag::String(block.clone())));
                fields.push(("probability".to_owned(), Tag::Float(*probability)));
            }
            Self::RandomBlockStateMatch {
                block_state,
                probability,
            } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:random_blockstate_match".to_owned()),
                ));
                fields.push(("block_state".to_owned(), block_state.to_tag()));
                fields.push(("probability".to_owned(), Tag::Float(*probability)));
            }
        }
        Tag::Compound(fields)
    }
}

impl PosRuleTest {
    fn to_tag(&self) -> Tag {
        let mut fields = Vec::new();
        match self {
            Self::AlwaysTrue => fields.push((
                "predicate_type".to_owned(),
                Tag::String("minecraft:always_true".to_owned()),
            )),
            Self::AxisAlignedLinear {
                min_chance,
                max_chance,
                min_dist,
                max_dist,
                axis,
            } => {
                fields.push((
                    "predicate_type".to_owned(),
                    Tag::String("minecraft:axis_aligned_linear_pos".to_owned()),
                ));
                // 全部字段都是 `optionalFieldOf`：等于默认值就不写。
                // Every field is `optionalFieldOf`: values equal to the default are omitted.
                if *min_chance != 0.0 {
                    fields.push(("min_chance".to_owned(), Tag::Float(*min_chance)));
                }
                if *max_chance != 0.0 {
                    fields.push(("max_chance".to_owned(), Tag::Float(*max_chance)));
                }
                if *min_dist != 0 {
                    fields.push(("min_dist".to_owned(), Tag::Int(*min_dist)));
                }
                if *max_dist != 0 {
                    fields.push(("max_dist".to_owned(), Tag::Int(*max_dist)));
                }
                if axis != "y" {
                    fields.push(("axis".to_owned(), Tag::String(axis.clone())));
                }
            }
        }
        Tag::Compound(fields)
    }
}

impl RuleBlockEntityModifier {
    fn to_tag(&self) -> Tag {
        let mut fields = Vec::new();
        match self {
            Self::Passthrough => fields.push((
                "type".to_owned(),
                Tag::String("minecraft:passthrough".to_owned()),
            )),
            Self::AppendLoot { loot_table } => {
                fields.push((
                    "type".to_owned(),
                    Tag::String("minecraft:append_loot".to_owned()),
                ));
                fields.push(("loot_table".to_owned(), Tag::String(loot_table.clone())));
            }
            Self::Clear => {
                fields.push(("type".to_owned(), Tag::String("minecraft:clear".to_owned())))
            }
        }
        Tag::Compound(fields)
    }
}

impl BlockStateSpec {
    /// `BlockState.CODEC`。26.3 的 NBT 形状是 `{id, properties}`（和 `.nbt` 模板里的
    /// palette 一致）；无属性时写裸 id 字符串。
    ///
    /// `BlockState.CODEC`. The 26.3 NBT shape is `{id, properties}` (the same as in a
    /// `.nbt` template palette); a bare id string when there are no properties.
    fn to_tag(&self) -> Tag {
        if self.properties.is_empty() {
            return Tag::String(self.name.clone());
        }
        let properties = Tag::Compound(
            self.properties
                .iter()
                .map(|(key, value)| (key.clone(), Tag::String(value.clone())))
                .collect(),
        );
        Tag::Compound(vec![
            ("id".to_owned(), Tag::String(self.name.clone())),
            ("properties".to_owned(), properties),
        ])
    }
}

/// `HolderSet<Block>`：标签写 `#minecraft:x`，显式列表写 id 数组。
///
/// `HolderSet<Block>`: a tag as `#minecraft:x`, an explicit list as an array of ids.
fn block_set_tag(set: &BlockSetRef) -> Tag {
    match set {
        BlockSetRef::Tag(text) => Tag::String(text.clone()),
        BlockSetRef::Blocks(ids) => {
            Tag::List(ids.iter().map(|id| Tag::String(id.clone())).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::jigsaw::element::Projection;

    fn single() -> PoolElement {
        PoolElement::Single {
            location: "minecraft:village/plains/houses/plains_small_house_1".to_owned(),
            projection: Projection::Rigid,
            legacy: false,
            processors: ProcessorListRef::Inline(Vec::new()),
            override_liquid_settings: None,
        }
    }

    /// `single` 元素的 `pool_element` 键形状：`element_type` / `location` /
    /// `projection` / `processors`，没有 `override_liquid_settings` 就不写。
    ///
    /// The `pool_element` shape of a `single` element; `override_liquid_settings` is
    /// omitted when absent.
    #[test]
    fn single_element_tag_has_codec_fields() {
        let tag = single().to_tag().unwrap();
        let Tag::Compound(fields) = tag else {
            panic!("pool element must be a compound");
        };
        let names: Vec<&str> = fields.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            names,
            ["element_type", "location", "projection", "processors"]
        );
        assert_eq!(
            fields[0].1,
            Tag::String("minecraft:single_pool_element".to_owned())
        );
        // 内联 processor 列表是 `{processors: [...]}` 复合（`DIRECT_CODEC` 的形式）。
        // An inline processor list is a `{processors: [...]}` compound (the `DIRECT_CODEC` form).
        assert_eq!(
            fields[3].1,
            Tag::Compound(vec![("processors".to_owned(), Tag::List(Vec::new()))])
        );
    }

    #[test]
    fn legacy_element_uses_the_legacy_element_type() {
        let mut element = single();
        if let PoolElement::Single { legacy, .. } = &mut element {
            *legacy = true;
        }
        let Tag::Compound(fields) = element.to_tag().unwrap() else {
            panic!("pool element must be a compound");
        };
        assert_eq!(
            fields[0].1,
            Tag::String("minecraft:legacy_single_pool_element".to_owned())
        );
    }

    /// `Rotation.LEGACY_CODEC` 写枚举常量名；`CODEC` 的 `none`/`clockwise_90` 不是这个字段。
    ///
    /// `Rotation.LEGACY_CODEC` writes enum constant names, not the `CODEC` names.
    #[test]
    fn rotation_legacy_names_match_the_enum_constants() {
        assert_eq!(rotation_legacy_name(Rotation::None), "NONE");
        assert_eq!(rotation_legacy_name(Rotation::Clockwise90), "CLOCKWISE_90");
        assert_eq!(
            rotation_legacy_name(Rotation::Clockwise180),
            "CLOCKWISE_180"
        );
        assert_eq!(
            rotation_legacy_name(Rotation::Counterclockwise90),
            "COUNTERCLOCKWISE_90"
        );
    }

    /// 默认液体设置不写 `liquid_settings`；非默认才写。
    ///
    /// The default liquid settings are omitted; non-default is written.
    #[test]
    fn liquid_settings_only_written_when_not_default() {
        let junction = Junction {
            source_x: 1,
            source_ground_y: 2,
            source_z: 3,
            delta_y: 4,
            dest_projection: Projection::TerrainMatching,
        };
        let mut piece = Piece {
            element: single(),
            position: (10, 64, -10),
            rotation: Rotation::Clockwise90,
            bounding_box: crate::structure::BoundingBox {
                min_x: 10,
                min_y: 64,
                min_z: -10,
                max_x: 20,
                max_y: 70,
                max_z: 0,
            },
            ground_level_delta: 1,
            junctions: vec![junction],
            liquid_settings: LiquidSettings::DEFAULT,
        };
        let extras = piece.save_extras().unwrap();
        assert!(!extras.iter().any(|(key, _)| key == "liquid_settings"));
        assert!(extras.iter().any(|(key, _)| key == "pool_element"));
        assert!(extras.iter().any(|(key, _)| key == "rotation"));
        assert!(extras.iter().any(|(key, _)| key == "junctions"));

        piece.liquid_settings = LiquidSettings::IgnoreWaterlogging;
        let extras = piece.save_extras().unwrap();
        let liquid = extras
            .iter()
            .find(|(key, _)| key == "liquid_settings")
            .map(|(_, value)| value.clone());
        assert_eq!(liquid, Some(Tag::String("ignore_waterlogging".to_owned())));
    }
}
