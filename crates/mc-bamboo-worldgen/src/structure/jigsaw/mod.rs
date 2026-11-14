//! Jigsaw 结构件生成。
//!
//! 本轮先移植 `JigsawStructure.findGenerationPoint` + `JigsawPlacement.addPieces`
//! 生成中心结构件的部分：`HeightProvider.sample`、`Rotation.getRandom`、
//! `getRandomTemplate`、`getRandomNamedJigsaw`、模板包围盒的旋转变换、
//! `project_start_to_heightmap` / `getFirstFreeHeight`、`centerPiece.move`。
//!
//! Jigsaw piece generation. This round ports the center-piece part of
//! `JigsawStructure.findGenerationPoint` + `JigsawPlacement.addPieces`:
//! `HeightProvider.sample`, `Rotation.getRandom`, `getRandomTemplate`,
//! `getRandomNamedJigsaw`, the rotated template bounding box,
//! `project_start_to_heightmap` / `getFirstFreeHeight`, and `centerPiece.move`.

pub mod config;
pub mod element;
pub mod pool;
pub mod rotation;

pub use config::{HeightProvider, JigsawConfig, JigsawStructures, WorldGenerationContext};
pub use element::{JigsawBlock, PoolElement, Projection};
pub use pool::{TemplatePool, TemplatePools};
pub use rotation::{Rotation, shuffle, shuffled_copy};

use std::collections::HashSet;

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    chunk::Generator,
    random::{Random, WorldgenRandom},
    structure::BoundingBox,
};

/// 中心结构件（`PoolElementStructurePiece`），放置阶段可直接用这些字段。
///
/// The center piece (`PoolElementStructurePiece`); the placement stage can use
/// these fields directly.
#[derive(Debug, Clone, PartialEq)]
pub struct CenterPiece {
    pub element: PoolElement,
    pub position: (i32, i32, i32),
    pub rotation: Rotation,
    pub bounding_box: BoundingBox,
    pub ground_level_delta: i32,
}

/// `Structure.GenerationStub` 的中心部分：stub 位置 + 中心结构件。
///
/// The center part of `Structure.GenerationStub`: stub position + center piece.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationPoint {
    pub position: (i32, i32, i32),
    pub center: CenterPiece,
}

/// 复刻 `findGenerationPoint` + `addPieces` 到中心结构件为止。
///
/// `None` 对应官方 `Optional.empty()`（空 element、找不到命名拼图、整列群系不合法、
/// 或中心件越过 dimension padding）。
///
/// Mirror `findGenerationPoint` + `addPieces` up to the center piece. `None` is
/// vanilla `Optional.empty()` (empty element, no named jigsaw, no legal biome in
/// the column, or the center piece crossing the dimension padding).
#[allow(clippy::too_many_arguments)]
pub fn center_piece(
    config: &JigsawConfig,
    pools: &TemplatePools,
    generator: &Generator<'_>,
    allowed: &HashSet<String>,
    dims: (i32, i32, i32),
    seed: i64,
    chunk: (i32, i32),
) -> Result<Option<GenerationPoint>, String> {
    let (cx, cz) = chunk;
    let (min_y, height, sea_level) = dims;
    let context = WorldGenerationContext {
        min_y,
        height,
        sea_level,
    };

    // `GenerationContext.makeRandom` 用 setLargeFeatureSeed，startHeight.sample 与
    // addPieces 共用同一个 WorldgenRandom。
    let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    random.set_large_feature_seed(seed, cx, cz);

    let height_value = config.start_height.sample(&random, &context);
    let position = (cx * 16, height_value, cz * 16);
    let center_rotation = Rotation::get_random(&random);

    // `poolAliasLookup.lookup(startPool)` 尚未移植：这里直接用配置的 start_pool。
    let center_pool = pools
        .get(&config.start_pool)
        .ok_or_else(|| format!("missing start pool {}", config.start_pool))?;
    let center_element = center_pool.get_random_template(&random)?;
    if center_element == PoolElement::Empty {
        return Ok(None);
    }

    // `getRandomNamedJigsaw`：洗牌后找第一个 name 匹配的拼图方块。
    let anchored_position = if let Some(target) = &config.start_jigsaw_name {
        let jigsaws = center_element.shuffled_jigsaws(position, center_rotation, &random)?;
        match jigsaws
            .iter()
            .find(|jigsaw| jigsaw.name.as_deref() == Some(target.as_str()))
        {
            Some(jigsaw) => jigsaw.pos,
            None => return Ok(None),
        }
    } else {
        position
    };
    let local_anchor = (
        anchored_position.0 - position.0,
        anchored_position.1 - position.1,
        anchored_position.2 - position.2,
    );
    let adjusted_position = (
        position.0 - local_anchor.0,
        position.1 - local_anchor.1,
        position.2 - local_anchor.2,
    );

    let ground_level_delta = center_element.ground_level_delta();
    let mut bounding_box = center_element.bounding_box(adjusted_position, center_rotation)?;
    let center_x = (bounding_box.max_x + bounding_box.min_x) / 2;
    let center_z = (bounding_box.max_z + bounding_box.min_z) / 2;

    let bottom_y = if config.project_start_to_heightmap.is_none() {
        adjusted_position.1
    } else {
        let max_y = min_y + height - 1;
        if !could_structure_exist_in_column(generator, allowed, center_x, center_z, min_y, max_y)? {
            return Ok(None);
        }
        // `getFirstFreeHeight` = `getBaseHeight` = `first_occupied_height + 1`。
        position.1 + generator.first_occupied_height(center_x, center_z)? + 1
    };

    let old_absolute_ground_y = bounding_box.min_y + ground_level_delta;
    let dy = bottom_y - old_absolute_ground_y;
    let piece_position = (
        adjusted_position.0,
        adjusted_position.1 + dy,
        adjusted_position.2,
    );
    bounding_box.min_y += dy;
    bounding_box.max_y += dy;

    // `isStartTooCloseToWorldHeightLimits`。
    let (pad_bottom, pad_top) = config.dimension_padding;
    if pad_bottom != 0 || pad_top != 0 {
        let max_y = min_y + height - 1;
        if bounding_box.min_y < min_y + pad_bottom || bounding_box.max_y > max_y - pad_top {
            return Ok(None);
        }
    }

    let center_y = bottom_y + local_anchor.1;
    Ok(Some(GenerationPoint {
        position: (center_x, center_y, center_z),
        center: CenterPiece {
            element: center_element,
            position: piece_position,
            rotation: center_rotation,
            bounding_box,
            ground_level_delta,
        },
    }))
}

/// `GenerationContext.couldStructureExistInColumn`：整列 quart 群系里有没有合法群系。
///
/// `GenerationContext.couldStructureExistInColumn`: whether any quart-Y biome in
/// the column is allowed for the structure.
fn could_structure_exist_in_column(
    generator: &Generator<'_>,
    allowed: &HashSet<String>,
    center_x: i32,
    center_z: i32,
    min_y: i32,
    max_y: i32,
) -> Result<bool, String> {
    let names = generator.column_biome_names(
        quart(center_x),
        quart(center_z),
        quart(min_y),
        quart(max_y),
    )?;
    Ok(names.iter().any(|name| allowed.contains(name)))
}

/// `QuartPos.fromBlock`：floorDiv(x, 4)。
fn quart(block: i32) -> i32 {
    block.div_euclid(4)
}
