//! 沉船的 `ShipwreckPieces.ShipwreckPiece.postProcess`：按海床 / 地表高度校正模板 Y，
//! 再走 `TemplateStructurePiece.postProcess`。
//!
//! Vanilla `ShipwreckPieces.ShipwreckPiece.postProcess`: adjust the template Y from the
//! ocean-floor (`OCEAN_FLOOR_WG`) or surface (`WORLD_SURFACE_WG`) heightmap, then run
//! `TemplateStructurePiece.postProcess`.

use crate::{
    feature::world::FeatureWorld,
    random::WorldgenRandom,
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            IGNORE_STRUCTURE_AND_AIR, LiquidSettings, PieceOutcome, PlaceSettings, Processor,
            heightmap_type, template_piece_post_process,
        },
        templated::{MIRROR_NONE, rotation_ordinal},
    },
};

/// `ShipwreckPieces.PIVOT` 的 X/Z。
///
/// The X/Z of `ShipwreckPieces.PIVOT`.
const PIVOT: (i32, i32) = (4, 15);
/// `ShipwreckPieces.NUMBER_OF_BLOCKS_ALLOWED_IN_WORLD_GEN_REGION`。
///
/// `ShipwreckPieces.NUMBER_OF_BLOCKS_ALLOWED_IN_WORLD_GEN_REGION`.
const MAX_TEMPLATE_SIZE: i32 = 32;

/// `ShipwreckPieces.MARKERS_TO_LOOT` 的三个 marker 名（战利品表本身只取随机数）。
///
/// The three marker names of `ShipwreckPieces.MARKERS_TO_LOOT` (the loot table itself only
/// draws a random number).
const MARKERS: [&str; 3] = ["map_chest", "treasure_chest", "supply_chest"];

/// `ShipwreckPiece.makeSettings(rotation)`。
///
/// `ShipwreckPiece.makeSettings(rotation)`.
fn settings(rotation: i32) -> PlaceSettings {
    PlaceSettings::new(MIRROR_NONE, rotation, PIVOT)
        .with_liquid(LiquidSettings::ApplyWaterlogging)
        .with_processors(vec![Processor::Ignore(IGNORE_STRUCTURE_AND_AIR)])
}

/// `ShipwreckPiece.isTooBigToFitInWorldGenRegion()`。
///
/// `ShipwreckPiece.isTooBigToFitInWorldGenRegion()`.
pub fn is_too_big(size: (i32, i32, i32)) -> bool {
    size.0 > MAX_TEMPLATE_SIZE || size.1 > MAX_TEMPLATE_SIZE
}

/// `ShipwreckPiece.postProcess` 的方块部分。
///
/// `random` 是 `postProcess` 的世界随机源：沙滩沉船的 `calculateBeachedPosition` 会从它
/// 取一次 `nextInt(3)`。调用方必须在与官方相同的位置调用本函数。
///
/// The block part of `ShipwreckPiece.postProcess`. `random` is the `postProcess` world
/// random: a beached shipwreck draws `nextInt(3)` from it inside
/// `calculateBeachedPosition`. The caller must invoke this at the same point vanilla does.
pub fn shipwreck(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    chunk_bb: Box,
    random: &WorldgenRandom,
    record: bool,
) -> Result<PieceOutcome, String> {
    let template_name = piece
        .get_str("Template")
        .ok_or("shipwreck piece has no Template")?
        .to_owned();
    let rotation = rotation_ordinal(piece.get_str("Rot").ok_or("shipwreck piece has no Rot")?)?;
    let is_beached = piece
        .extras
        .get("isBeached")
        .is_some_and(|value| value.token() == "B:1");
    let template_position = (
        extra_int(piece, "TPX").ok_or("shipwreck piece has no TPX")?,
        extra_int(piece, "TPY").ok_or("shipwreck piece has no TPY")?,
        extra_int(piece, "TPZ").ok_or("shipwreck piece has no TPZ")?,
    );
    let template = StructureTemplate::load(&template_name)?;
    let settings = settings(rotation);

    let corrected = if is_too_big(template.size) {
        template_position
    } else {
        let ocean_floor = !is_beached;
        let kind = heightmap_type(ocean_floor);
        // `int minY = level.getMaxY() + 1`。
        // `int minY = level.getMaxY() + 1`.
        let mut lowest = world.min_y + world.height;
        let mut mean = 0i32;
        let base_size = template.size.0 * template.size.2;
        if base_size == 0 {
            mean = world.get_height(kind, template_position.0, template_position.2);
        } else {
            let corner = (
                template_position.0 + template.size.0 - 1,
                template_position.2 + template.size.2 - 1,
            );
            // `BlockPos.betweenClosed` 的顺序是 x 内层、z 外层；Y 只有一个值。
            // `BlockPos.betweenClosed` iterates x innermost and z outermost; Y is a
            // single value.
            for z in template_position.2..=corner.1 {
                for x in template_position.0..=corner.0 {
                    let height = world.get_height(kind, x, z);
                    mean += height;
                    lowest = lowest.min(height);
                }
            }
            mean /= base_size;
        }
        let new_y = if is_beached {
            // `calculateBeachedPosition(minY, random)`。
            // `calculateBeachedPosition(minY, random)`.
            lowest - template.size.1 / 2 - random.next_int(3)
        } else {
            mean
        };
        (template_position.0, new_y, template_position.2)
    };

    Ok(template_piece_post_process(
        world, &template, corrected, &settings, chunk_bb, record,
    ))
}

/// 战利品标记名，供 `handleDataMarker` 的调用方判分支。
///
/// The loot marker names, for the `handleDataMarker` caller.
pub fn is_loot_marker(metadata: &str) -> bool {
    MARKERS.contains(&metadata)
}

fn extra_int(piece: &Piece, key: &str) -> Option<i32> {
    match piece.extras.get(key) {
        Some(crate::structure::pieces::Extra::Int(value)) => Some(*value),
        _ => None,
    }
}
