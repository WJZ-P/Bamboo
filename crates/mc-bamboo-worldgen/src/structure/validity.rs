//! `Structure.generate` 里 `findValidGenerationPoint` 的通用判定：群系检查和高度检查。
//!
//! 官方 `Structure.GenerationContext` 提供三件与具体结构件无关的事：
//! `couldValidBiomeExistOnTopOfChunkCenter`（整列 quart 群系成批扫描）、
//! `getFirstOccupiedHeight` / `getLowestY` / `getMeanFirstOccupiedHeight`（四角高度）、
//! 和 `isValidBiome`（起点 quart 单点群系）。`onTopOfChunkCenter` 把「整列有合法群系」
//! 作为短路，取中心列首个占用高度当起点，再由 `findValidGenerationPoint` 用
//! `isValidBiome` 过滤。这里把这几件原语和它们的组合移植成纯函数。
//!
//! 注意：每个结构真正的 `findGenerationPoint`（结构件生成、模板、jigsaw）属于下一步，
//! 本模块只复刻通用判定；因此 `find_valid_generation_point` 的「有效」是 onTopOfChunkCenter
//! 这条基线路径的有效，不等于 `Structure.generate` 的最终有效。
//!
//! The generic gates of `Structure.generate` / `findValidGenerationPoint`: biome and height
//! checks. Vanilla `Structure.GenerationContext` provides three piece-independent operations:
//! `couldValidBiomeExistOnTopOfChunkCenter` (batched quart-column biome scan),
//! `getFirstOccupiedHeight` / `getLowestY` / `getMeanFirstOccupiedHeight` (corner heights),
//! and `isValidBiome` (single-point start biome). `onTopOfChunkCenter` short-circuits on the
//! column check, uses the center column's first occupied height as the stub, and
//! `findValidGenerationPoint` filters with `isValidBiome`. These primitives and their
//! composition are ported here as pure functions.
//!
//! Each structure's real `findGenerationPoint` (piece generation, templates, jigsaw) is a later
//! milestone; this module only reproduces the generic gates, so "valid" here is the
//! onTopOfChunkCenter baseline, not `Structure.generate`'s final validity.

use std::collections::HashSet;

use mc_bamboo_data::BIOME_CLIMATES;

use crate::{
    chunk::Generator,
    math::java_floor_div,
    structure::{
        StructureStarts, StructureState,
        registry::{StructureDef, Structures},
        rings::BiomeResolver,
    },
};

/// 一个 quart 坐标：把方块坐标右移 2 位（floor）。
///
/// A quart coordinate: floor-divide a block coordinate by 4.
fn quart(block: i32) -> i32 {
    java_floor_div(block, 4)
}

/// 把 `StructureDef.biomes`（`BIOME_CLIMATES` 下标）展开成群系名集合。
///
/// Expand `StructureDef.biomes` (`BIOME_CLIMATES` indices) into a set of biome names.
pub fn allowed_biome_names(def: &StructureDef) -> Result<HashSet<String>, String> {
    def.biomes
        .iter()
        .map(|&id| {
            BIOME_CLIMATES
                .get(id as usize)
                .map(|biome| biome.name.to_owned())
                .ok_or_else(|| format!("biome id {id} has no name"))
        })
        .collect()
}

/// `ChunkPos.getMiddleBlockX/Z`：区块中心方块坐标。
///
/// `ChunkPos.getMiddleBlockX/Z`: the center block of a chunk.
fn chunk_center(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x * 16 + 8, chunk_z * 16 + 8)
}

/// `couldValidBiomeExistOnTopOfChunkCenter`：中心列整段 quart Y 里是否有一个合法群系。
///
/// 官方整列范围是 `[heightAccessor.getMinY()-1, heightAccessor.getMaxY()]`，
/// 换成 quart 后走 `MultiNoiseBiomeSource.createResolverForChunk` 的成批路径。
///
/// `couldValidBiomeExistOnTopOfChunkCenter`: whether any quart Y of the center column has an
/// allowed biome. The vanilla column spans `[getMinY()-1, getMaxY()]`; converted to quart
/// coords it goes through the batched `createResolverForChunk` path.
pub fn column_has_valid_biome(
    generator: &Generator,
    chunk_x: i32,
    chunk_z: i32,
    allowed: &HashSet<String>,
    min_y: i32,
    height: i32,
) -> Result<bool, String> {
    let (center_x, center_z) = chunk_center(chunk_x, chunk_z);
    let min_quart_y = quart(min_y - 1);
    let max_quart_y = quart(min_y + height - 1);
    let names = generator.column_biome_names(quart(center_x), quart(center_z), min_quart_y, max_quart_y)?;
    Ok(names.iter().any(|name| allowed.contains(name)))
}

/// `getCornerHeights`：四角的 `getFirstOccupiedHeight(WORLD_SURFACE_WG)`，顺序照官方。
///
/// `getCornerHeights`: the four corners' `getFirstOccupiedHeight(WORLD_SURFACE_WG)`,
/// in vanilla order.
pub fn corner_heights(
    generator: &Generator,
    min_x: i32,
    min_z: i32,
    size_x: i32,
    size_z: i32,
) -> Result<[i32; 4], String> {
    Ok([
        generator.first_occupied_height(min_x, min_z)?,
        generator.first_occupied_height(min_x, min_z + size_z)?,
        generator.first_occupied_height(min_x + size_x, min_z)?,
        generator.first_occupied_height(min_x + size_x, min_z + size_z)?,
    ])
}

/// `getLowestY`：四角高度的最小值。
///
/// `getLowestY`: the minimum of the four corner heights.
pub fn lowest_y(heights: [i32; 4]) -> i32 {
    heights.into_iter().min().unwrap_or(i32::MAX)
}

/// `getMeanFirstOccupiedHeight`：四角高度之和整除 4（向零取整，和 Java int 除法一致）。
///
/// `getMeanFirstOccupiedHeight`: sum of the four corner heights / 4 (truncating toward zero,
/// matching Java int division).
pub fn mean_first_occupied_height(heights: [i32; 4]) -> i32 {
    heights.iter().sum::<i32>() / 4
}

/// `GenerationContext.isValidBiome(stub)`：起点 quart 坐标的单点群系是否在允许集合里。
///
/// 这条走 `createResolver(climateSampler)` 的单点路径，不是整列的成批路径。
///
/// `GenerationContext.isValidBiome(stub)`: whether the single-point biome at the stub's quart
/// coords is allowed. This uses the single-point `createResolver(climateSampler)` path, not the
/// batched column path.
pub fn is_valid_biome_at(
    resolver: &mut BiomeResolver,
    stub_x: i32,
    stub_y: i32,
    stub_z: i32,
    allowed: &HashSet<String>,
) -> bool {
    let biome = resolver.biome_at(quart(stub_x), quart(stub_y), quart(stub_z));
    allowed.contains(&biome)
}

/// `findValidGenerationPoint` 在 `onTopOfChunkCenter` 这条基线路径上的产出。
///
/// Output of `findValidGenerationPoint` along the `onTopOfChunkCenter` baseline path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationPoint {
    /// 起点方块坐标 `(centerX, firstOccupiedHeight, centerZ)`。
    ///
    /// Stub block position `(centerX, firstOccupiedHeight, centerZ)`.
    pub stub: (i32, i32, i32),
    /// 整列是否有合法群系（`onTopOfChunkCenter` 的短路条件）。
    ///
    /// Whether the column has an allowed biome (`onTopOfChunkCenter` short-circuit).
    pub column_has_valid_biome: bool,
    /// 起点单点群系是否合法（`isValidBiome` 过滤）。
    ///
    /// Whether the start's single-point biome is allowed (`isValidBiome` filter).
    pub start_biome_valid: bool,
    /// 基线有效性 = 整列 && 起点群系。
    ///
    /// Baseline validity = column && start biome.
    pub valid: bool,
}

/// `onTopOfChunkCenter` + `isValidBiome` 的组合：算出起点和基线有效性。
///
/// 官方顺序是先整列短路、再取中心列高度、最后在起点 quart 上做 `isValidBiome`。
///
/// Composition of `onTopOfChunkCenter` + `isValidBiome`: compute the stub and baseline validity.
/// Vanilla order: column short-circuit, then center height, then `isValidBiome` at the stub quart.
pub fn find_valid_generation_point(
    generator: &Generator,
    resolver: &mut BiomeResolver,
    chunk_x: i32,
    chunk_z: i32,
    allowed: &HashSet<String>,
    min_y: i32,
    height: i32,
) -> Result<GenerationPoint, String> {
    let (center_x, center_z) = chunk_center(chunk_x, chunk_z);
    let column = column_has_valid_biome(generator, chunk_x, chunk_z, allowed, min_y, height)?;
    let stub_y = generator.first_occupied_height(center_x, center_z)?;
    let start_biome_valid =
        is_valid_biome_at(resolver, center_x, stub_y, center_z, allowed);
    Ok(GenerationPoint {
        stub: (center_x, stub_y, center_z),
        column_has_valid_biome: column,
        start_biome_valid,
        valid: column && start_biome_valid,
    })
}

/// `createStructures` 在「基线判定」这一层的产出：选中的结构和起点是否有效。
///
/// Output of `createStructures` at the baseline-gate level: the selected structure and whether
/// its start is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedStart {
    pub set: String,
    pub structure: String,
    pub valid: bool,
    pub stub: (i32, i32, i32),
}

/// 用 `start_plans`（放置门 + existingStart 短路 + 加权尝试序列）加上基线判定，
/// 复刻 `createStructures` 的「按结构集顺序、每个结构按加权顺序试，第一个有效就返回」。
///
/// 这里只有基线的 `findValidGenerationPoint`，真正的 `findGenerationPoint`（结构件生成）
/// 属于下一步，所以「有效」是通用判定，不是 `Structure.generate` 的最终有效。
///
/// Combines `start_plans` (placement gate + existingStart short-circuit + weighted attempt
/// sequence) with the baseline gate to reproduce `createStructures`: iterate sets in order,
/// try each structure in weighted order, return the first valid one. Only the baseline
/// `findValidGenerationPoint` is used here; real piece generation is a later milestone, so
/// "valid" is the generic gate, not `Structure.generate`'s final validity.
pub fn select_start(
    state: &StructureState,
    structures: &Structures,
    generator: &Generator,
    resolver: &mut BiomeResolver,
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height: i32,
) -> Result<Option<SelectedStart>, String> {
    let plans = state.start_plans(seed, chunk_x, chunk_z, &StructureStarts::default());
    for plan in &plans {
        for structure_id in &plan.attempts {
            let Some(def) = structures.get(structure_id) else {
                continue;
            };
            let allowed = allowed_biome_names(def)?;
            let point = find_valid_generation_point(
                generator, resolver, chunk_x, chunk_z, &allowed, min_y, height,
            )?;
            if point.valid {
                return Ok(Some(SelectedStart {
                    set: plan.set.clone(),
                    structure: structure_id.clone(),
                    valid: true,
                    stub: point.stub,
                }));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quart_matches_java_floor_division() {
        assert_eq!(quart(8), 2);
        assert_eq!(quart(-65), -17);
        assert_eq!(quart(319), 79);
    }

    #[test]
    fn lowest_and_mean_corner_heights() {
        let heights = [60, 70, -3, 65];
        assert_eq!(lowest_y(heights), -3);
        assert_eq!(mean_first_occupied_height(heights), 48);
        // Java int 除法向零取整，负和时和 floor 不同。
        // Java int division truncates toward zero, unlike floor, for a negative sum.
        assert_eq!(mean_first_occupied_height([-1, -1, -1, -1]), -1);
    }
}
