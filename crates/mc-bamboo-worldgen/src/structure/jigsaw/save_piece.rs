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
use crate::structure::save::SavePiece;

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
                fields.push(("processors".to_owned(), json_to_tag(processors)?));
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

/// 把 `processors` 的原始 JSON 转成 NBT。`processors` 在 26.3 只有两种形状：
/// 注册名（字符串）或内联 `{processors: [...]}` 对象；内联对象里的 processor 列表
/// 目前只有空列表，非空对象留给放置里程碑补。
///
/// Convert the raw `processors` JSON to NBT. In 26.3 it only has two shapes: a registry
/// name (string) or an inline `{processors: [...]}` object whose list is currently always
/// empty; non-empty processor objects are left to the placement milestone.
fn json_to_tag(value: &serde_json::Value) -> Result<Tag, String> {
    match value {
        serde_json::Value::String(text) => Ok(Tag::String(text.clone())),
        serde_json::Value::Object(fields) => {
            let mut out = Vec::new();
            for (key, value) in fields {
                out.push((key.clone(), json_to_tag(value)?));
            }
            Ok(Tag::Compound(out))
        }
        serde_json::Value::Array(values) => Ok(Tag::List(
            values
                .iter()
                .map(json_to_tag)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        serde_json::Value::Bool(flag) => Ok(Tag::Byte(i8::from(*flag))),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(Tag::Int(value as i32))
            } else if let Some(value) = number.as_f64() {
                Ok(Tag::Double(value))
            } else {
                Err(format!("unsupported processor number {number}"))
            }
        }
        serde_json::Value::Null => Err("null is not representable in NBT".to_owned()),
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
            processors: serde_json::json!({ "processors": [] }),
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
        // 内联空 processor 列表是空列表，不是字符串。
        // The inline empty processor list is an empty list, not a string.
        assert_eq!(
            fields[3].1,
            Tag::Compound(vec![("processors".into(), Tag::List(Vec::new()))])
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
