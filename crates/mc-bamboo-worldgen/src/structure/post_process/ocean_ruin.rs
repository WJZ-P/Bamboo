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
    random::WorldgenRandom,
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            IGNORE_STRUCTURE_AND_AIR, InputTest, LiquidSettings, MarkerContext, PieceOutcome,
            PlaceSettings, Processor, Rule, in_tag, template_piece_post_process,
        },
        templated::{MIRROR_NONE, rotation_ordinal, transform},
    },
};

/// `OceanRuinPieces.OceanRuinPiece.postProcess` 的方块部分。
///
/// 官方每次都从 `OCEAN_FLOOR_WG` 重算 `templatePosition` 的 Y 并留在结构件上，这里对应
/// 写回 `TPY`。`random` 是 `postProcess` 的随机源，箱子的战利品种子从它取。
///
/// The block part of `OceanRuinPieces.OceanRuinPiece.postProcess`. Vanilla recomputes the
/// `templatePosition` Y from `OCEAN_FLOOR_WG` on every call and leaves it on the piece,
/// mirrored here by writing `TPY` back. `random` is the `postProcess` random that chest
/// loot seeds are drawn from.
pub fn ocean_ruin(
    world: &mut FeatureWorld<'_>,
    piece: &mut Piece,
    chunk_bb: Box,
    random: &WorldgenRandom,
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
    let is_large = piece
        .extras
        .get("IsLarge")
        .is_some_and(|value| matches!(value, crate::structure::pieces::Extra::Byte(1)));
    let template_position = (
        extra_int(piece, "TPX").ok_or("ocean ruin piece has no TPX")?,
        extra_int(piece, "TPY").ok_or("ocean ruin piece has no TPY")?,
        extra_int(piece, "TPZ").ok_or("ocean ruin piece has no TPZ")?,
    );
    let template = StructureTemplate::load(&template_name)?;
    let settings = settings(rotation, integrity, is_cold, world.seed);

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
    piece.put_int("TPY", corrected.1);

    let sea_level = world.sea_level;
    let outcome = template_piece_post_process(
        world,
        &template,
        corrected,
        &settings,
        chunk_bb,
        random,
        &mut |context, metadata, pos| {
            handle_data_marker(context, metadata, pos, sea_level, is_large)
        },
    );
    piece.bb = outcome.bounding_box;
    Ok(outcome)
}

/// `OceanRuinPiece.handleDataMarker`。
///
/// `chest`：写一个箱子，`WATERLOGGED` 取该位置当时的流体是不是水，再给它设战利品表
/// （`random.nextLong()`）。`drowned`：生成一只溺尸（实体不在这里处理，它的
/// `finalizeSpawn` 用的是世界随机源而不是 `postProcess` 的随机源），然后把标记格换成
/// 空气（高于海平面）或水。
///
/// `OceanRuinPiece.handleDataMarker`. `chest`: write a chest whose `WATERLOGGED` is
/// whether the fluid there is water at that moment, then set its loot table
/// (`random.nextLong()`). `drowned`: spawn a drowned (the entity is not handled here; its
/// `finalizeSpawn` uses the level random, not the `postProcess` one), then turn the
/// marker cell into air (above sea level) or water.
fn handle_data_marker(
    context: &mut MarkerContext<'_, '_, '_>,
    metadata: &str,
    pos: (i32, i32, i32),
    sea_level: i32,
    is_large: bool,
) {
    match metadata {
        "chest" => {
            let fluid = mc_bamboo_data::state_fluid(context.world.get_block(pos.0, pos.1, pos.2));
            let water = fluid.fluid == mc_bamboo_data::registries::fluid::WATER
                || fluid.fluid == mc_bamboo_data::registries::fluid::FLOWING_WATER;
            let mut state = mc_bamboo_data::blocks::CHEST;
            if let Some(next) = crate::feature::state::with_property(
                state,
                "waterlogged",
                if water { "true" } else { "false" },
            ) {
                state = next;
            }
            context.set_block(pos, state);
            // `OceanRuinPiece.handleDataMarker` 给箱子设
            // `UNDERWATER_RUIN_BIG` / `UNDERWATER_RUIN_SMALL` 表与一次
            // `random.nextLong()` 种子，和 `IglooPiece` 是同一条路径。
            // `OceanRuinPiece.handleDataMarker` sets the `UNDERWATER_RUIN_*`
            // loot table and one `random.nextLong()` seed on the chest.
            context.set_chest_loot_table(
                pos,
                if is_large {
                    "minecraft:chests/underwater_ruin_big"
                } else {
                    "minecraft:chests/underwater_ruin_small"
                },
            );
        }
        "drowned" => {
            let state = if pos.1 > sea_level {
                mc_bamboo_data::blocks::AIR
            } else {
                mc_bamboo_data::blocks::WATER
            };
            context.set_block(pos, state);
        }
        _ => {}
    }
}

/// `OceanRuinPiece.makeSettings(rotation, integrity, biomeType)`。
///
/// `OceanRuinPiece.makeSettings(rotation, integrity, biomeType)`.
fn settings(rotation: i32, integrity: f32, is_cold: bool, world_seed: i64) -> PlaceSettings {
    // `archyRuleProcessor(Blocks.SAND, Blocks.SUSPICIOUS_SAND, ...)`（暖水）/
    // `(Blocks.GRAVEL, Blocks.SUSPICIOUS_GRAVEL, ...)`（冷水），外面包一层
    // `CappedProcessor(..., ConstantInt.of(5))`。
    //
    // `archyRuleProcessor(...)` wrapped in `CappedProcessor(..., ConstantInt.of(5))`.
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
                    loot_table: Some(if is_cold {
                        "minecraft:archaeology/ocean_ruin_cold"
                    } else {
                        "minecraft:archaeology/ocean_ruin_warm"
                    }),
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
                let is_ice = in_tag(state, "minecraft:ice");
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

fn extra_int(piece: &Piece, key: &str) -> Option<i32> {
    match piece.extras.get(key) {
        Some(crate::structure::pieces::Extra::Int(value)) => Some(*value),
        _ => None,
    }
}

fn extra_float(piece: &Piece, key: &str) -> Option<f32> {
    piece.extras.get(key).and_then(|value| match value {
        crate::structure::pieces::Extra::Float(value) => Some(*value),
        _ => None,
    })
}
