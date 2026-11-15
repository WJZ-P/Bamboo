//! 海底遗迹的 `OceanRuinPieces.OceanRuinPiece.postProcess`：按 `OCEAN_FLOOR_WG` 与
//! `getHeight` 的整列搜索校正模板 Y，再走 `TemplateStructurePiece.postProcess`
//! （`BlockRotProcessor` 按 `integrity` 丢方块、`BlockIgnoreProcessor` 丢结构方块与空气、
//! 可疑方块 processor）。数据标记分箱子和溺尸两种。
//!
//! Vanilla `OceanRuinPieces.OceanRuinPiece.postProcess`: correct the template Y with
//! `OCEAN_FLOOR_WG` plus the `getHeight` column scan, then run
//! `TemplateStructurePiece.postProcess` (`BlockRotProcessor` drops blocks by `integrity`,
//! `BlockIgnoreProcessor` drops structure blocks and air, plus the suspicious-block
//! processor). Data markers are chests and drowned.

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            IGNORE_STRUCTURE_AND_AIR, InputTest, LiquidSettings, PieceOutcome, PlaceSettings,
            Processor, Rule, template_piece_post_process,
        },
        templated::{MIRROR_NONE, rotation_ordinal, transform},
    },
};

/// `OceanRuinPieces.OceanRuinPiece.postProcess` 的方块部分。
///
/// The block part of `OceanRuinPieces.OceanRuinPiece.postProcess`.
pub fn ocean_ruin(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    chunk_bb: Box,
    world_seed: i64,
    record: bool,
) -> Result<PieceOutcome, String> {
    let template_name = piece
        .get_str("Template")
        .ok_or("ocean ruin piece has no Template")?
        .to_owned();
    let rotation = rotation_ordinal(piece.get_str("Rot").ok_or("ocean ruin piece has no Rot")?)?;
    let integrity = extra_float(piece, "Integrity").ok_or("ocean ruin piece has no Integrity")?;
    let is_cold = piece
        .get_str("BiomeType")
        .ok_or("ocean ruin piece has no BiomeType")?
        == "COLD";
    let template_position = (
        extra_int(piece, "TPX").ok_or("ocean ruin piece has no TPX")?,
        extra_int(piece, "TPY").ok_or("ocean ruin piece has no TPY")?,
        extra_int(piece, "TPZ").ok_or("ocean ruin piece has no TPZ")?,
    );
    let template = StructureTemplate::load(&template_name)?;
    let settings = settings(rotation, integrity, is_cold, world_seed);

    // `int height = level.getHeight(OCEAN_FLOOR_WG, templatePosition.getX(), getZ())`。
    // `int height = level.getHeight(OCEAN_FLOOR_WG, templatePosition.getX(), getZ())`.
    let height = world.get_height(
        HeightmapKind::OceanFloorWg,
        template_position.0,
        template_position.2,
    );
    let first = (template_position.0, height, template_position.2);
    let corner_local = transform(
        (template.size.0 - 1, 0, template.size.2 - 1),
        MIRROR_NONE,
        rotation,
        (0, 0),
    );
    let corner = (
        first.0 + corner_local.0,
        first.1 + corner_local.1,
        first.2 + corner_local.2,
    );
    let corrected = (first.0, column_height(world, first, corner), first.2);

    Ok(template_piece_post_process(
        world, &template, corrected, &settings, chunk_bb, record,
    ))
}

/// `OceanRuinPiece.makeSettings(rotation, integrity, biomeType)`。
///
/// `OceanRuinPiece.makeSettings(rotation, integrity, biomeType)`.
fn settings(rotation: i32, integrity: f32, is_cold: bool, world_seed: i64) -> PlaceSettings {
    // `archyRuleProcessor(Blocks.SAND, Blocks.SUSPICIOUS_SAND, ...)`（暖水）/
    // `(Blocks.GRAVEL, Blocks.SUSPICIOUS_GRAVEL, ...)`（冷水），外面包一层
    // `CappedProcessor(..., ConstantInt.of(5))`：最多 5 个方块被替换成可疑方块。
    // `archyRuleProcessor(...)` wrapped in `CappedProcessor(..., ConstantInt.of(5))`: at
    // most five blocks become suspicious.
    let (from, to) = if is_cold {
        ("minecraft:gravel", "minecraft:suspicious_gravel")
    } else {
        ("minecraft:sand", "minecraft:suspicious_sand")
    };
    let output = mc_bamboo_data::block_by_name(to)
        .map(|block| block.default_state)
        .unwrap_or(mc_bamboo_data::blocks::AIR);
    PlaceSettings::new(MIRROR_NONE, rotation, (0, 0))
        .with_liquid(LiquidSettings::ApplyWaterlogging)
        .with_world_seed(world_seed)
        .with_processors(vec![
            Processor::Rot(integrity),
            Processor::Ignore(IGNORE_STRUCTURE_AND_AIR),
            Processor::Capped {
                limit: 5,
                delegate: std::boxed::Box::new(Processor::Rule(vec![Rule {
                    input: InputTest::Block(from),
                    output,
                }])),
            },
        ])
}

/// `OceanRuinPiece.getHeight(pos, level, corner)`：从每个角点向下跳过空气 / 水 / 冰，
/// 取最低的 floorY；当「最低点比顶点低两格以上」且「够多的列满足这个条件」时把 Y 降到
/// `minY + 1`。
///
/// `OceanRuinPiece.getHeight(pos, level, corner)`: from every column inside the corner
/// box, skip down over air / water / ice and take the lowest floorY; when it sits more
/// than two blocks below the top and enough columns reach it, lower the Y to `minY + 1`.
fn column_height(world: &FeatureWorld<'_>, pos: (i32, i32, i32), corner: (i32, i32, i32)) -> i32 {
    let mut new_y = pos.1;
    let mut min_y = 512;
    let top_y = new_y - 1;
    let mut area = 0;
    let min_x = pos.0.min(corner.0);
    let max_x = pos.0.max(corner.0);
    let min_z = pos.2.min(corner.2);
    let max_z = pos.2.max(corner.2);
    // `BlockPos.betweenClosed(pos, corner)`：x 内层、z 外层。
    // `BlockPos.betweenClosed(pos, corner)`: x innermost, z outermost.
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let mut floor_y = pos.1 - 1;
            loop {
                let state = world.get_block(x, floor_y, z);
                let fluid = mc_bamboo_data::state_fluid(state);
                let is_air = mc_bamboo_data::state_facts(state) & mc_bamboo_data::facts::AIR != 0;
                let is_water = fluid.fluid == mc_bamboo_data::registries::fluid::WATER
                    || fluid.fluid == mc_bamboo_data::registries::fluid::FLOWING_WATER;
                let is_ice = mc_bamboo_data::block_of_state(state)
                    .is_some_and(|block| block.name == "minecraft:ice");
                if !(is_air || is_water || is_ice) || floor_y <= world.min_y + 1 {
                    break;
                }
                floor_y -= 1;
            }
            min_y = min_y.min(floor_y);
            if floor_y < top_y - 2 {
                area += 1;
            }
        }
    }
    let width = (pos.0 - corner.0).abs();
    if top_y - min_y > 2 && area > width - 2 {
        new_y = min_y + 1;
    }
    new_y
}

/// 数据标记的类型。
///
/// The data-marker kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Chest,
    Drowned,
}

/// `OceanRuinPiece.handleDataMarker` 的分支判定。
///
/// The branch `OceanRuinPiece.handleDataMarker` takes.
pub fn marker(metadata: &str) -> Option<Marker> {
    match metadata {
        "chest" => Some(Marker::Chest),
        "drowned" => Some(Marker::Drowned),
        _ => None,
    }
}

fn extra_int(piece: &Piece, key: &str) -> Option<i32> {
    match piece.extras.get(key) {
        Some(crate::structure::pieces::Extra::Int(value)) => Some(*value),
        _ => None,
    }
}

fn extra_float(piece: &Piece, key: &str) -> Option<f32> {
    piece
        .extras
        .get(key)
        .and_then(|value| value.token().parse::<f32>().ok())
}
