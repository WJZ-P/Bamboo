//! `SnowAndFreezeFeature`（`freeze_top_layer`）：给每个 column 的表面结冰、盖雪。
//!
//! 官方 `Feature.place` 遍历 16×16 的局部坐标，读 `MOTION_BLOCKING` 高度图拿地表 y，
//! 再问这一格所属群系的 `shouldFreeze` / `shouldSnow`：冷到能下雪时，地表下一格的水
//! 换成冰、地表那格铺雪层，可选的 `SnowyBlock.SNOWY` 属性也跟着置位。
//!
//! `SnowAndFreezeFeature` (`freeze_top_layer`): freeze and snow the surface of every
//! column. `Feature.place` walks the 16×16 local grid, reads the `MOTION_BLOCKING`
//! height for the top cell, then asks the biome there via `shouldFreeze` / `shouldSnow`:
//! cold biomes turn the water below the surface into ice, place a snow layer on top,
//! and flip the optional `SnowyBlock.SNOWY` property on the block underneath.

use mc_bamboo_data::{block_id_of_state, block_tag, blocks, facts, state_facts, tags};

use crate::{
    biome_temperature::{BiomeClimate, Precipitation, precipitation_at},
    random::WorldgenRandom,
};

use super::{
    state::apply_property,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// `SnowAndFreezeFeature.place`：`origin` 是区块原点的 (x, minY, z)，返回恒为 true。
///
/// `SnowAndFreezeFeature.place`: `origin` is the chunk origin `(x, minY, z)`; always true.
pub fn place(
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: Pos,
    biome_at: &mut dyn FnMut(i32, i32, i32) -> u32,
) -> bool {
    let _ = random;
    for dx in 0..16 {
        for dz in 0..16 {
            let x = origin.0 + dx;
            let z = origin.2 + dz;
            let y = world.get_height(HeightmapKind::MotionBlocking, x, z);
            // 官方 `level.getBiome(topPos)` 走逐点 `BiomeManager.getBiome`。
            // The official `level.getBiome(topPos)` goes through `BiomeManager.getBiome`.
            let climate = BiomeClimate::of(biome_at(x, y, z) as i32);
            let below = (x, y - 1, z);
            if should_freeze(climate, world, below) {
                world.set_block_with_heightmaps(x, y - 1, z, blocks::ICE);
            }
            if should_snow(climate, world, (x, y, z)) {
                world.set_block_with_heightmaps(x, y, z, blocks::SNOW);
                let below_state = world.get_block(x, y - 1, z);
                if let Some(snowy) = apply_property(below_state, "snowy", "true") {
                    world.set_block_with_heightmaps(x, y - 1, z, snowy);
                }
            }
        }
    }
    true
}

/// 方块光。官方这里读 `level.getBrightness(LightLayer.BLOCK, pos)`，地物阶段还没跑
/// `light`，整片区域都是 0。
///
/// Block light: the official code reads `level.getBrightness(LightLayer.BLOCK, pos)`,
/// which is 0 everywhere during the feature stage (light runs later).
const BLOCK_LIGHT: i32 = 0;

/// `Biome.shouldFreeze(level, pos, checkNeighbors = false)`。
///
/// `Biome.shouldFreeze(level, pos, checkNeighbors = false)`.
fn should_freeze(climate: BiomeClimate, world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let (x, y, z) = pos;
    if climate.warm_enough_to_rain(x, y, z, world.sea_level) {
        return false;
    }
    if !inside_build_height(world, y) || BLOCK_LIGHT >= 10 {
        return false;
    }
    let state = world.get_block(x, y, z);
    // `fluidState.is(Fluids.WATER) && blockState.getBlock() instanceof LiquidBlock`：
    // 水面方块就是水的 `LiquidBlock`。
    // `fluidState.is(Fluids.WATER) && blockState.getBlock() instanceof LiquidBlock`:
    // the water block is the `LiquidBlock` for water.
    fluid_is_water(state) && block_id_of_state(state) == block_id_of_state(blocks::WATER)
}

/// `Biome.shouldSnow(level, pos)`。
///
/// `Biome.shouldSnow(level, pos)`.
fn should_snow(climate: BiomeClimate, world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let (x, y, z) = pos;
    if precipitation_at(climate, x, y, z, world.sea_level) != Precipitation::Snow {
        return false;
    }
    if !inside_build_height(world, y) || BLOCK_LIGHT >= 10 {
        return false;
    }
    let state = world.get_block(x, y, z);
    let air_or_snow = state_facts(state) & facts::AIR != 0
        || block_id_of_state(state) == block_id_of_state(blocks::SNOW);
    if !air_or_snow {
        return false;
    }
    snow_can_survive(world, pos)
}

/// `SnowLayerBlock.canSurvive`：下方不能是 `CANNOT_SUPPORT_SNOW_LAYER`；
/// `SUPPORT_OVERRIDE_SNOW_LAYER` 直接放行；否则下面要么上表面满、要么是 8 层雪。
///
/// `SnowLayerBlock.canSurvive`: reject `CANNOT_SUPPORT_SNOW_LAYER` below, accept
/// `SUPPORT_OVERRIDE_SNOW_LAYER`, else require a full top face or a full snow layer.
fn snow_can_survive(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    if in_block_tag(below, tags::block_tags::CANNOT_SUPPORT_SNOW_LAYER.name) {
        return false;
    }
    if in_block_tag(below, tags::block_tags::SUPPORT_OVERRIDE_SNOW_LAYER.name) {
        return true;
    }
    // `Block.isFaceFull(below.getCollisionShape(level, below), UP)`：碰撞形状盖满整格。
    // `Block.isFaceFull(below.getCollisionShape(level, below), UP)`: full-cell collision.
    let full_top = mc_bamboo_data::collision_shape(below).iter().any(|box_| {
        box_.min_x == 0.0
            && box_.min_y == 0.0
            && box_.min_z == 0.0
            && box_.max_x == 1.0
            && box_.max_y == 1.0
            && box_.max_z == 1.0
    });
    if full_top {
        return true;
    }
    // `belowState.is(this) && belowState.getValue(LAYERS) == 8`。
    // `belowState.is(this) && belowState.getValue(LAYERS) == 8`.
    block_id_of_state(below) == block_id_of_state(blocks::SNOW)
        && apply_property(below, "layers", "8").is_some()
}

/// `LevelReader.isInsideBuildHeight(y)`。
///
/// `LevelReader.isInsideBuildHeight(y)`.
fn inside_build_height(world: &FeatureWorld<'_>, y: i32) -> bool {
    y >= world.min_y && y < world.min_y + world.height
}

/// `FluidState.is(FluidTags.WATER)`。
///
/// `FluidState.is(FluidTags.WATER)`.
fn fluid_is_water(state: u32) -> bool {
    let fluid = mc_bamboo_data::state_fluid(state).fluid;
    fluid == mc_bamboo_data::registries::fluid::WATER
        || fluid == mc_bamboo_data::registries::fluid::FLOWING_WATER
}

fn in_block_tag(state: u32, tag: &str) -> bool {
    block_id_of_state(state)
        .is_some_and(|id| block_tag(tag).is_some_and(|ids| ids.contains(&(id as i32))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snow_survives_on_full_blocks_and_rejects_cannot_support() {
        // 石头是整格碰撞形状，可以托住雪层。
        // Stone has a full collision shape, so it can hold a snow layer.
        assert!(
            mc_bamboo_data::collision_shape(blocks::STONE)
                .iter()
                .any(|b| b.max_y == 1.0 && b.max_x == 1.0 && b.max_z == 1.0)
        );
        // 空气不是整格，托不住。
        // Air is not full, so it cannot hold one.
        assert!(mc_bamboo_data::collision_shape(blocks::AIR).is_empty());
        // `CANNOT_SUPPORT_SNOW_LAYER` 里应当有冰。
        // Ice is in `CANNOT_SUPPORT_SNOW_LAYER`.
        let ice_id = block_id_of_state(blocks::ICE).unwrap() as i32;
        assert!(
            block_tag(tags::block_tags::CANNOT_SUPPORT_SNOW_LAYER.name)
                .unwrap()
                .contains(&ice_id)
        );
    }

    #[test]
    fn block_tag_ids_lookup_matches_the_constant() {
        assert!(block_tag("minecraft:cannot_support_snow_layer").is_some());
        assert!(in_block_tag(
            blocks::ICE,
            "minecraft:cannot_support_snow_layer"
        ));
    }
}
