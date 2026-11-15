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
            IGNORE_STRUCTURE_AND_AIR, LiquidSettings, MarkerContext, PieceOutcome, PlaceSettings,
            Processor, heightmap_type, template_piece_post_process,
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
/// 取一次 `nextInt(3)`，箱子的战利品种子也从它取。高度只校正一次：官方校正后把
/// `heightAdjusted` 置 true 并改写 `templatePosition`，这里对应写回 `height_adjusted` 和
/// `TPY`，后面的区块直接用校正后的位置。
///
/// The block part of `ShipwreckPiece.postProcess`. `random` is the `postProcess` world
/// random: a beached shipwreck draws `nextInt(3)` from it inside
/// `calculateBeachedPosition`, and chest loot seeds come from it too. The height is
/// adjusted once: vanilla sets `heightAdjusted` and rewrites `templatePosition`, mirrored
/// here by writing `height_adjusted` and `TPY` back so later chunks reuse the position.
pub fn shipwreck(
    world: &mut FeatureWorld<'_>,
    piece: &mut Piece,
    chunk_bb: Box,
    random: &WorldgenRandom,
) -> Result<PieceOutcome, String> {
    let template_name = piece
        .get_str("Template")
        .ok_or("shipwreck piece has no Template")?
        .to_owned();
    let rotation = rotation_ordinal(piece.get_str("Rot").ok_or("shipwreck piece has no Rot")?)?;
    let is_beached = extra_bool(piece, "isBeached");
    let height_adjusted = extra_bool(piece, "height_adjusted");
    let mut template_position = (
        extra_int(piece, "TPX").ok_or("shipwreck piece has no TPX")?,
        extra_int(piece, "TPY").ok_or("shipwreck piece has no TPY")?,
        extra_int(piece, "TPZ").ok_or("shipwreck piece has no TPZ")?,
    );
    let template = StructureTemplate::load(&template_name)?;
    let settings = settings(rotation);

    if !height_adjusted && !is_too_big(template.size) {
        let kind = heightmap_type(!is_beached);
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
        // `adjustPositionHeight(newHeight)`。
        // `adjustPositionHeight(newHeight)`.
        template_position.1 = new_y;
        piece.put_int("TPY", new_y);
        piece.put_bool("height_adjusted", true);
    }

    let outcome = template_piece_post_process(
        world,
        &template,
        template_position,
        &settings,
        chunk_bb,
        random,
        &mut handle_data_marker,
    );
    piece.bb = outcome.bounding_box;
    Ok(outcome)
}

/// `ShipwreckPiece.handleDataMarker`：三个标记（`map_chest` / `treasure_chest` /
/// `supply_chest`）都调 `RandomizableContainer.setBlockEntityLootTable(level, random,
/// pos.below(), table)`，不写方块，只在下面那格是容器时取一次 `nextLong`。
///
/// `ShipwreckPiece.handleDataMarker`: the three markers call
/// `RandomizableContainer.setBlockEntityLootTable(level, random, pos.below(), table)`,
/// which writes no block and draws one `nextLong` when the block below is a container.
fn handle_data_marker(
    context: &mut MarkerContext<'_, '_, '_>,
    metadata: &str,
    pos: (i32, i32, i32),
) {
    if MARKERS.contains(&metadata) {
        context.seed_loot_table((pos.0, pos.1 - 1, pos.2));
    }
}

fn extra_int(piece: &Piece, key: &str) -> Option<i32> {
    match piece.extras.get(key) {
        Some(crate::structure::pieces::Extra::Int(value)) => Some(*value),
        _ => None,
    }
}

fn extra_bool(piece: &Piece, key: &str) -> bool {
    matches!(
        piece.extras.get(key),
        Some(crate::structure::pieces::Extra::Byte(value)) if *value != 0
    )
}
