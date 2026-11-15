//! 雪屋的 `IglooPieces.IglooPiece.postProcess`：按 `WORLD_SURFACE_WG` 把模板 Y 校正到
//! 地表，再走 `TemplateStructurePiece.postProcess`（放模板、跑数据标记、拼图替换），
//! 最后给入口的活板门补雪块。
//!
//! Vanilla `IglooPieces.IglooPiece.postProcess`: correct the template Y from
//! `WORLD_SURFACE_WG`, run `TemplateStructurePiece.postProcess` (place the template, data
//! markers, jigsaw replacement), then cap the entrance trapdoor with snow.

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            IGNORE_STRUCTURE_BLOCK, LiquidSettings, PieceOutcome, PlaceSettings, Processor,
            template_piece_post_process,
        },
        templated::{MIRROR_NONE, rotation_ordinal},
    },
};

/// `IglooPieces.GENERATION_HEIGHT`：`postProcess` 用它算 Y 偏移。
///
/// `IglooPieces.GENERATION_HEIGHT`: `postProcess` subtracts it from the surface height.
pub const GENERATION_HEIGHT: i32 = 90;

/// `IglooPieces.PIVOTS` 与 `OFFSETS` 的合并表项。
///
/// One entry of the merged `IglooPieces.PIVOTS` / `OFFSETS` tables.
struct Part {
    template: &'static str,
    /// `PIVOTS` 的 X/Z（`StructureTemplate.transform` 不用 Y）。
    ///
    /// The X/Z of `PIVOTS` (`StructureTemplate.transform` ignores Y).
    pivot: (i32, i32),
    offset: (i32, i32, i32),
}

const IGLOO: Part = Part {
    template: "minecraft:igloo/top",
    pivot: (3, 5),
    offset: (0, 0, 0),
};
const LADDER: Part = Part {
    template: "minecraft:igloo/middle",
    pivot: (1, 1),
    offset: (2, -3, 4),
};
const LABORATORY: Part = Part {
    template: "minecraft:igloo/bottom",
    pivot: (3, 7),
    offset: (0, -3, -2),
};

/// 按模板名查 pivot / offset。
///
/// Look the pivot / offset up by template name.
fn part(template: &str) -> Option<&'static Part> {
    [&IGLOO, &LADDER, &LABORATORY]
        .into_iter()
        .find(|part| part.template == template)
}

/// 雪屋 `postProcess` 的产物。
///
/// The outcome of one igloo `postProcess`.
pub struct IglooOutcome {
    /// 模板放置的结果。
    ///
    /// The template placement outcome.
    pub placement: PieceOutcome,
    /// `postProcess` 之后的 `templatePosition`（官方最后会还原成旧值，这里给出校正后的）。
    ///
    /// The corrected `templatePosition` (vanilla restores the old value afterwards).
    pub corrected_position: (i32, i32, i32),
    /// 入口活板门补的雪块位置（没补就是 `None`）。
    ///
    /// The trapdoor position capped with snow, if any.
    pub snow: Option<(i32, i32, i32)>,
}

/// `IglooPieces.IglooPiece.makeSettings(rotation, templateLocation)`。
///
/// `IglooPieces.IglooPiece.makeSettings(rotation, templateLocation)`.
fn settings(rotation: i32, part: &Part) -> PlaceSettings {
    PlaceSettings::new(MIRROR_NONE, rotation, part.pivot)
        .with_liquid(LiquidSettings::IgnoreWaterlogging)
        .with_processors(vec![Processor::Ignore(IGNORE_STRUCTURE_BLOCK)])
}

/// `IglooPieces.IglooPiece.postProcess` 的方块部分。
///
/// 只处理与本区块盒相交的结构件；调用方按 `StructureStart.placeInChunk` 的编排决定调哪些。
///
/// The block part of `IglooPieces.IglooPiece.postProcess`. The caller decides which
/// pieces to run, following `StructureStart.placeInChunk`.
pub fn igloo(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    chunk_bb: Box,
    record: bool,
) -> Result<IglooOutcome, String> {
    let template_name = piece
        .get_str("Template")
        .ok_or("igloo piece has no Template")?
        .to_owned();
    let rotation = rotation_ordinal(piece.get_str("Rot").ok_or("igloo piece has no Rot")?)?;
    let part =
        part(&template_name).ok_or_else(|| format!("unknown igloo template {template_name}"))?;
    let settings = settings(rotation, part);
    let template = StructureTemplate::load(&template_name)?;
    let template_position = (piece.bb[0], piece.bb[1], piece.bb[2]);
    // `TPX` / `TPY` / `TPZ` 才是模板位置，`BB` 是它算出来的盒子。
    let template_position = (
        piece
            .extras
            .get("TPX")
            .and_then(extra_int)
            .unwrap_or(template_position.0),
        piece
            .extras
            .get("TPY")
            .and_then(extra_int)
            .unwrap_or(template_position.1),
        piece
            .extras
            .get("TPZ")
            .and_then(extra_int)
            .unwrap_or(template_position.2),
    );

    let relative = settings.relative((3 - part.offset.0, 0, -part.offset.2));
    let entrance = (
        template_position.0 + relative.0,
        template_position.1 + relative.1,
        template_position.2 + relative.2,
    );
    let height = world.get_height(HeightmapKind::WorldSurfaceWg, entrance.0, entrance.2);
    let corrected = (
        template_position.0,
        template_position.1 + height - GENERATION_HEIGHT - 1,
        template_position.2,
    );

    let placement =
        template_piece_post_process(world, &template, corrected, &settings, chunk_bb, record);

    let mut snow = None;
    if template_name == IGLOO.template {
        let offset = settings.relative((3, 0, 5));
        let trapdoor = (
            corrected.0 + offset.0,
            corrected.1 + offset.1,
            corrected.2 + offset.2,
        );
        let below = world.get_block(trapdoor.0, trapdoor.1 - 1, trapdoor.2);
        let is_air = mc_bamboo_data::state_facts(below) & mc_bamboo_data::facts::AIR != 0;
        let is_ladder = mc_bamboo_data::block_of_state(below)
            .is_some_and(|block| block.name == "minecraft:ladder");
        if !is_air && !is_ladder {
            let snow_block = mc_bamboo_data::block_by_name("minecraft:snow_block")
                .map(|block| block.default_state)
                .ok_or("missing snow_block")?;
            world.set_block_with_heightmaps(trapdoor.0, trapdoor.1, trapdoor.2, snow_block);
            snow = Some(trapdoor);
        }
    }

    Ok(IglooOutcome {
        placement,
        corrected_position: corrected,
        snow,
    })
}

/// 读结构件 extras 里的整数字段。
///
/// Read an integer field from the piece extras.
fn extra_int(value: &crate::structure::pieces::Extra) -> Option<i32> {
    match value {
        crate::structure::pieces::Extra::Int(value) => Some(*value),
        _ => None,
    }
}
