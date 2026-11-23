//! `StructurePoolElement` 的五种类型及其 `getBoundingBox` / `getShuffledJigsawBlocks`。
//!
//! 照官方 `StructurePoolElement` / `SinglePoolElement` / `ListPoolElement` /
//! `FeaturePoolElement` / `EmptyPoolElement` 的逻辑，用 Rust 重写。`getJigsaws` 的初始
//! 顺序是 `Palette.jigsaws()` 的顺序（`buildInfoList` 按 `(y, x, z)` 排序，拼图方块
//! 都在 blockEntities 组），再 `Util.shuffle`、最后稳定按 selectionPriority 降序。
//!
//! Port of the five `StructurePoolElement` types and their `getBoundingBox` /
//! `getShuffledJigsawBlocks`. `getJigsaws` starts in `Palette.jigsaws()` order
//! (`buildInfoList` sorts by `(y, x, z)`; jigsaws all land in the block-entity group),
//! then `Util.shuffle`, then a stable sort by descending selectionPriority.

use std::cmp::Reverse;

use crate::{
    random::WorldgenRandom,
    structure::{
        BoundingBox,
        processor::ProcessorListRef,
        template::{Direction, JointType, TemplateShape},
    },
};
use serde_json::Value;

use super::rotation::{Rotation, shuffle};

/// `StructureTemplatePool.Projection`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    Rigid,
    TerrainMatching,
}

impl Projection {
    fn parse(raw: Option<&Value>) -> Result<Self, String> {
        match raw.and_then(Value::as_str) {
            Some("rigid") => Ok(Self::Rigid),
            Some("terrain_matching") => Ok(Self::TerrainMatching),
            other => Err(format!("bad projection {other:?}")),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Rigid => "rigid",
            Self::TerrainMatching => "terrain_matching",
        }
    }
}

/// 五种 pool element。`processors` / `override_liquid_settings` 只留给放置阶段，
/// 结构件生成不读；`processors` 已按 `StructureProcessorType.LIST_CODEC` 解析成
/// 类型化的 `ProcessorListRef`（注册名或内联列表），放置那一步不用再解析 JSON。
///
/// The five pool element types. `processors` / `override_liquid_settings` are only for
/// the placement stage; piece generation never reads them. `processors` is parsed into a
/// typed `ProcessorListRef` (a registry name or an inline list) per
/// `StructureProcessorType.LIST_CODEC`, so the placement stage does not re-parse JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolElement {
    Empty,
    Single {
        location: String,
        projection: Projection,
        legacy: bool,
        /// `processors`：注册名或内联列表。
        ///
        /// `processors`: a registry name or an inline list.
        processors: ProcessorListRef,
        /// `SinglePoolElement.overrideLiquidSettings`。
        override_liquid_settings: Option<String>,
    },
    List {
        elements: Vec<PoolElement>,
        projection: Projection,
    },
    Feature {
        feature: String,
        projection: Projection,
    },
}

/// 洗牌后的拼图方块，`getShuffledJigsawBlocks` 的产出。
///
/// A shuffled jigsaw block, the output of `getShuffledJigsawBlocks`.
#[derive(Debug, Clone, PartialEq)]
pub struct JigsawBlock {
    pub pos: (i32, i32, i32),
    /// `JigsawBlockInfo.name()`；`FeaturePoolElement` 的是 `null`。
    pub name: Option<String>,
    pub target: String,
    pub pool: String,
    pub placement_priority: i32,
    pub selection_priority: i32,
    /// 经 `Rotation` 旋转后的 front / top（`OctahedralGroup.rotate(FrontAndTop)`）。
    pub front: Direction,
    pub top: Direction,
    /// `JigsawBlockEntity.JointType`，不受旋转影响。
    pub joint: JointType,
}

/// `JigsawBlock.canAttach(source, target)`。
///
/// `sourceFront == targetFront.getOpposite()`，且（rollable 或 top 相同），
/// 且 target 的 name 为 null 或 `source.target == target.name`。
pub fn can_attach(source: &JigsawBlock, target: &JigsawBlock) -> bool {
    let rollable = source.joint == JointType::Rollable;
    source.front == target.front.opposite()
        && (rollable || source.top == target.top)
        && target
            .name
            .as_ref()
            .is_none_or(|name| source.target == *name)
}

/// 解析 `orientation` 名字 `<front>_<top>`（如 `north_up` / `down_south`）。
fn parse_orientation(value: &str) -> Result<(Direction, Direction), String> {
    let mut parts = value.splitn(2, '_');
    let front = parts
        .next()
        .and_then(Direction::from_name)
        .ok_or_else(|| format!("bad orientation {value}"))?;
    let top = parts
        .next()
        .and_then(Direction::from_name)
        .ok_or_else(|| format!("bad orientation {value}"))?;
    Ok((front, top))
}

impl PoolElement {
    /// 从 `template_pool` 的 `element` JSON 解析。
    ///
    /// Parse one `element` object of a `template_pool`.
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let element_type = raw
            .get("element_type")
            .and_then(Value::as_str)
            .ok_or("pool element has no element_type")?;
        match element_type {
            "minecraft:empty_pool_element" => Ok(Self::Empty),
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                Ok(Self::Single {
                    location: string_field(raw, "location")?,
                    projection: Projection::parse(raw.get("projection"))?,
                    legacy: element_type == "minecraft:legacy_single_pool_element",
                    processors: ProcessorListRef::parse(
                        raw.get("processors")
                            .ok_or("single_pool_element has no processors")?,
                    )?,
                    override_liquid_settings: raw
                        .get("override_liquid_settings")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
            }
            "minecraft:list_pool_element" => {
                let elements = raw
                    .get("elements")
                    .and_then(Value::as_array)
                    .ok_or("list_pool_element has no elements")?
                    .iter()
                    .map(Self::parse)
                    .collect::<Result<Vec<_>, _>>()?;
                // `ListPoolElement` 构造器：`if (elements.isEmpty()) throw new
                // IllegalArgumentException("Elements are empty")`，所以空列表是解码错误。
                // `ListPoolElement`'s constructor throws for an empty list, so it is a
                // decode error rather than an empty element.
                if elements.is_empty() {
                    return Err("list_pool_element has an empty elements list".to_owned());
                }
                Ok(Self::List {
                    elements,
                    projection: Projection::parse(raw.get("projection"))?,
                })
            }
            "minecraft:feature_pool_element" => Ok(Self::Feature {
                feature: string_field(raw, "feature")?,
                projection: Projection::parse(raw.get("projection"))?,
            }),
            other => Err(format!("unsupported pool element {other}")),
        }
    }

    /// `single` / `legacy` 的模板注册名。
    pub fn location(&self) -> Option<&str> {
        match self {
            Self::Single { location, .. } => Some(location),
            _ => None,
        }
    }

    pub const fn projection(&self) -> Projection {
        match self {
            // `EmptyPoolElement` 的构造器传的是 `TERRAIN_MATCHING`。
            // `EmptyPoolElement` passes `TERRAIN_MATCHING` to its constructor.
            Self::Empty => Projection::TerrainMatching,
            Self::Single { projection, .. }
            | Self::List { projection, .. }
            | Self::Feature { projection, .. } => *projection,
        }
    }

    /// `StructurePoolElement.getGroundLevelDelta()`：基类默认 1，无子类重写。
    pub const fn ground_level_delta(&self) -> i32 {
        1
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Single { legacy, .. } => {
                if *legacy {
                    "legacy"
                } else {
                    "single"
                }
            }
            Self::List { .. } => "list",
            Self::Feature { .. } => "feature",
        }
    }

    /// `getSize(manager, rotation)`。
    pub fn size(&self, rotation: Rotation) -> Result<(i32, i32, i32), String> {
        match self {
            Self::Empty | Self::Feature { .. } => Ok((0, 0, 0)),
            Self::Single { location, .. } => {
                let template = TemplateShape::load(location)?;
                Ok(rotation.size(template.size))
            }
            Self::List { elements, .. } => {
                let mut size = (0, 0, 0);
                for element in elements {
                    let (sx, sy, sz) = element.size(rotation)?;
                    size = (size.0.max(sx), size.1.max(sy), size.2.max(sz));
                }
                Ok(size)
            }
        }
    }

    /// `getBoundingBox(manager, position, rotation)`。
    pub fn bounding_box(
        &self,
        position: (i32, i32, i32),
        rotation: Rotation,
    ) -> Result<BoundingBox, String> {
        match self {
            Self::Empty => Err("EmptyPoolElement has no bounding box".to_owned()),
            Self::Single { location, .. } => {
                let template = TemplateShape::load(location)?;
                Ok(rotation.bounding_box(position, template.size))
            }
            Self::List { elements, .. } => {
                let mut result = BoundingBox::EMPTY;
                for element in elements {
                    if matches!(element, Self::Empty) {
                        continue;
                    }
                    result = BoundingBox::encapsulating(
                        result,
                        element.bounding_box(position, rotation)?,
                    );
                }
                Ok(result)
            }
            Self::Feature { .. } => Ok(BoundingBox {
                min_x: position.0,
                min_y: position.1,
                min_z: position.2,
                max_x: position.0,
                max_y: position.1,
                max_z: position.2,
            }),
        }
    }

    /// `getShuffledJigsawBlocks(manager, position, rotation, random)`。
    pub fn shuffled_jigsaws(
        &self,
        position: (i32, i32, i32),
        rotation: Rotation,
        random: &WorldgenRandom,
    ) -> Result<Vec<JigsawBlock>, String> {
        match self {
            Self::Empty => Ok(Vec::new()),
            Self::List { elements, .. } => elements
                .first()
                .ok_or("empty list pool element")?
                .shuffled_jigsaws(position, rotation, random),
            Self::Feature { .. } => Ok(vec![JigsawBlock {
                pos: position,
                name: None,
                target: "minecraft:empty".to_owned(),
                pool: "minecraft:empty".to_owned(),
                placement_priority: 0,
                selection_priority: 0,
                front: Direction::Down,
                top: Direction::South,
                joint: JointType::Rollable,
            }]),
            Self::Single { location, .. } => {
                let template = TemplateShape::load(location)?;
                // `getJigsaws(position, rotation)`：`getRandomPalette` 按**世界位置**选
                // palette，然后 `Palette.jigsaws()` 已是 `(y, x, z)` 顺序。
                // `getJigsaws(position, rotation)`: `getRandomPalette` picks the palette
                // from the **world position**; `Palette.jigsaws()` is already in
                // `(y, x, z)` order.
                let mut blocks: Vec<JigsawBlock> = template
                    .jigsaws_at(position)
                    .iter()
                    .map(|jigsaw| {
                        let (front0, top0) = parse_orientation(&jigsaw.orientation)?;
                        let local = rotation.transform(jigsaw.pos);
                        Ok(JigsawBlock {
                            pos: (
                                local.0 + position.0,
                                local.1 + position.1,
                                local.2 + position.2,
                            ),
                            name: Some(jigsaw.name.clone()),
                            target: jigsaw.target.clone(),
                            pool: jigsaw.pool.clone(),
                            placement_priority: jigsaw.placement_priority,
                            selection_priority: jigsaw.selection_priority,
                            front: rotation.rotate_direction(front0),
                            top: rotation.rotate_direction(top0),
                            joint: jigsaw.joint,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                shuffle(&mut blocks, random);
                // Java `List.sort` 是稳定的；同 selectionPriority 保持洗牌后的顺序。
                // Java `List.sort` is stable; equal priorities keep the shuffled order.
                blocks.sort_by_key(|block| Reverse(block.selection_priority));
                Ok(blocks)
            }
        }
    }
}

fn string_field(raw: &Value, key: &str) -> Result<String, String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("pool element has no {key}"))
}
