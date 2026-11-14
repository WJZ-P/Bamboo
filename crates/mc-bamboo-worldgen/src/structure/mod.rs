//! 世界生成的结构部分：放置判定和注册表解析骨架。
//!
//! 对照官方 `ChunkStatus` 的 `structure_starts` / `structure_references` 两个阶段，本模块
//! 提供纯函数 `StructureState`：给定种子和区块坐标，算出「哪些结构集把这里当起点」。
//! 结构件的生成（`Structure.generate`）、加权选具体结构、群系有效性检查、
//! `features` 阶段的 `placeInChunk`、terrain 里的 `Beardifier` 和存档 `structures` 键
//! 是后续里程碑，见 `docs/worldgen-structures-plan.md`。
//!
//! 对接流水线的接口按该文档第 8 节：`structure_starts` 阶段拿本模块的
//! `start_candidates`，再对命中结构跑 `Structure.generate` 得到 `StructureStart`；
//! `structure_references` 阶段用 `createReferences` 的 17×17 邻域和包围盒相交写引用。
//! 两个阶段的完整签名等结构件移植时补齐。
//!
//! The structure part of worldgen: placement checks and registry parsing skeleton.
//! Mirroring the `structure_starts` / `structure_references` stages, this module provides
//! the pure `StructureState`: given a seed and chunk coordinates, which structure sets
//! treat the chunk as a start. Structure-piece generation, weighted structure selection,
//! biome validity checks, `features`-stage `placeInChunk`, the terrain `Beardifier` and the
//! save `structures` tag are later milestones; see `docs/worldgen-structures-plan.md`.

pub mod placement;
pub mod registry;
pub mod rings;
pub mod start;

use std::collections::{HashMap, HashSet};

pub use placement::{
    Common, ConcentricRings, ExclusionZone, FrequencyReductionMethod, Placement, RandomSpread,
    SpreadType,
};
pub use registry::{
    SelectionEntry, StructureDef, StructureSetEntry, StructureSets, Structures,
    possible_structure_sets, resolve_biome_holders,
};
pub use start::{
    BoundingBox, References, StartPlan, StructurePiece, StructureStart, StructureStarts,
    chunk_pos_pack, selection_sequence, structure_references,
};

/// `hasStructureChunkInRange` 递归查别的集合时允许的最大深度。
///
/// 官方 `ChunkGeneratorStructureState.hasStructureChunkInRange` 调的是对方集合的完整
/// `isStructureChunk`；纯函数实现里用一个上限防止自定义数据包里 A 排除 B、B 又排除 A
/// 造成无限递归。原版 26.3 只有 `pillager_outposts` 排除 `villages`，深度为 1。
///
/// Max recursion depth when `hasStructureChunkInRange` checks another set. Vanilla calls the
/// other set's full `isStructureChunk`; the cap guards against custom datapacks where two
/// sets exclude each other. Vanilla 26.3 only has `pillager_outposts` excluding `villages`
/// (depth 1).
const MAX_EXCLUSION_DEPTH: u32 = 8;

/// `createStructures` 的放置门命中结果：结构集名字和它的加权结构表。
///
/// 具体选哪个结构还要 `setLargeFeatureSeed` 加权 + `Structure.generate`（结构件里程碑），
/// 本轮只到「结构集」这一层。
///
/// A hit from the `createStructures` placement gate: the set name and its weighted structure
/// list. Picking the concrete structure still needs weighted selection + `Structure.generate`
/// (structure-piece milestone); this round stops at the set level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCandidate {
    pub set: String,
    pub structures: Vec<SelectionEntry>,
}

/// `ChunkGeneratorStructureState` 的纯数据等价物：结构集放置规则的集合。
///
/// 只依赖解析好的注册表，不碰世界状态、不取系统随机数，可进计算池。
///
/// Pure-data counterpart of `ChunkGeneratorStructureState`: the collection of structure-set
/// placements. It depends only on the parsed registry, touches no world state and draws no
/// system randomness, so it can run on the compute pool.
#[derive(Debug, Clone)]
pub struct StructureState {
    sets: StructureSets,
    /// `concentric_rings` 结构集算好的环位置（`ChunkGeneratorStructureState.ringPositions`）。
    /// `ConcentricRingsStructurePlacement.isPlacementChunk` 用它做 `contains`。
    ///
    /// Precomputed ring positions for `concentric_rings` sets
    /// (`ChunkGeneratorStructureState.ringPositions`), used by `isPlacementChunk`.
    rings: HashMap<String, HashSet<(i32, i32)>>,
}

impl StructureState {
    pub fn new(sets: StructureSets) -> Self {
        Self {
            sets,
            rings: HashMap::new(),
        }
    }

    /// 把 `rings::ring_positions` 的结果注入某个 `concentric_rings` 结构集。
    ///
    /// Feed `rings::ring_positions` output into a `concentric_rings` set.
    pub fn set_ring_positions(
        &mut self,
        set_name: &str,
        positions: impl IntoIterator<Item = (i32, i32)>,
    ) {
        self.rings
            .insert(set_name.to_owned(), positions.into_iter().collect());
    }

    pub fn sets(&self) -> &StructureSets {
        &self.sets
    }

    /// 官方 `StructurePlacement.isStructureChunk(state, sourceX, sourceZ)`：
    /// `isPlacementChunk && applyAdditionalChunkRestrictions && applyInteractionsWithOtherStructures`。
    ///
    /// Vanilla `StructurePlacement.isStructureChunk(state, sourceX, sourceZ)`:
    /// `isPlacementChunk && applyAdditionalChunkRestrictions && applyInteractionsWithOtherStructures`.
    pub fn is_structure_chunk(&self, set_name: &str, seed: i64, x: i32, z: i32) -> bool {
        self.is_structure_chunk_inner(set_name, seed, x, z, 0)
    }

    fn is_structure_chunk_inner(
        &self,
        set_name: &str,
        seed: i64,
        x: i32,
        z: i32,
        depth: u32,
    ) -> bool {
        let Some(set) = self.sets.get(set_name) else {
            return false;
        };
        // `concentric_rings` 的 isPlacementChunk 是 `ringPositions.contains(chunkPos)`；
        // 没注入环位置时和之前一样返回 false（还不支持）。
        // `concentric_rings` isPlacementChunk is `ringPositions.contains(chunkPos)`;
        // without injected positions it stays false (unsupported), as before.
        let placement_hit = match &set.placement {
            Placement::RandomSpread(p) => p.is_placement_chunk(seed, x, z),
            Placement::ConcentricRings(_) => self
                .rings
                .get(set_name)
                .is_some_and(|positions| positions.contains(&(x, z))),
        };
        // 三项都是与运算，顺序照搬官方（虽然都是纯函数，但决定了语义）。
        // All three terms are ANDed in vanilla order (pure, but the order defines semantics).
        let base = placement_hit
            && set
                .placement
                .apply_additional_chunk_restrictions(seed, x, z);
        if !base {
            return false;
        }
        let Some(exclusion) = &set.placement.common().exclusion_zone else {
            return true;
        };
        if depth >= MAX_EXCLUSION_DEPTH {
            return true;
        }
        !self.has_structure_chunk_in_range(
            &exclusion.other_set,
            seed,
            x,
            z,
            exclusion.chunk_count,
            depth + 1,
        )
    }

    /// `ChunkGeneratorStructureState.hasStructureChunkInRange`：`range` 格半径的方形邻域内
    /// 是否有该集合的结构起点。
    ///
    /// `ChunkGeneratorStructureState.hasStructureChunkInRange`: whether the set has a
    /// structure start within a `range`-chunk square around `(x, z)`.
    fn has_structure_chunk_in_range(
        &self,
        set_name: &str,
        seed: i64,
        x: i32,
        z: i32,
        range: i32,
        depth: u32,
    ) -> bool {
        for test_x in (x - range)..=(x + range) {
            for test_z in (z - range)..=(z + range) {
                if self.is_structure_chunk_inner(set_name, seed, test_x, test_z, depth) {
                    return true;
                }
            }
        }
        false
    }

    /// `createStructures` 的放置门：对每个结构集判 `isStructureChunk`，返回命中的结构集。
    /// 结果按结构集名字排序（注册表顺序），与官方 `possibleStructureSets()` 的遍历序一致。
    ///
    /// The `createStructures` placement gate: returns the sets whose `isStructureChunk` is
    /// true, sorted by name (registry order, matching `possibleStructureSets()` iteration).
    pub fn start_candidates(&self, seed: i64, x: i32, z: i32) -> Vec<StartCandidate> {
        self.sets
            .iter()
            .filter(|set| self.is_structure_chunk(&set.name, seed, x, z))
            .map(|set| StartCandidate {
                set: set.name.clone(),
                structures: set.structures.clone(),
            })
            .collect()
    }

    /// `createStructures` 的放置门 + `existingStart` 短路 + 加权选择序列。
    ///
    /// 官方对每个结构集先检查 `getStartForStructure(...)` 有没有已经有效的起点，
    /// 有就跳过整个结构集；否则 `isStructureChunk` 通过才做加权选择。这里产出
    /// 每个命中结构集的 `StartPlan`（尝试序列），不含 `Structure.generate` 的有效性
    /// 判定，所以没有「最终选中哪个结构」。
    ///
    /// The `createStructures` gate + `existingStart` short-circuit + weighted selection.
    /// Vanilla first checks `getStartForStructure(...)` for an already-valid start and
    /// skips the whole set; otherwise it only selects when `isStructureChunk` passes.
    /// This yields a `StartPlan` (attempt sequence) per hit; without `Structure.generate`
    /// validity there is no "finally selected structure".
    pub fn start_plans(
        &self,
        seed: i64,
        x: i32,
        z: i32,
        existing: &StructureStarts,
    ) -> Vec<StartPlan> {
        self.sets
            .iter()
            .filter(|set| !existing.has_valid_for_set(&set.structures))
            .filter(|set| self.is_structure_chunk(&set.name, seed, x, z))
            .map(|set| StartPlan {
                set: set.name.clone(),
                attempts: start::selection_sequence(&set.structures, seed, x, z),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> StructureState {
        let sets = StructureSets::parse(&json!({
            "minecraft:villages": {
                "placement": {"type":"minecraft:random_spread","salt":10387312,"separation":8,"spacing":34},
                "structures": [{"structure":"minecraft:village_plains","weight":1}]
            },
            "minecraft:pillager_outposts": {
                "placement": {"type":"minecraft:random_spread","salt":165745296,"separation":8,"spacing":32,
                    "frequency":0.2,"frequency_reduction_method":"legacy_type_1",
                    "exclusion_zone":{"other_set":"minecraft:villages","chunk_count":10}},
                "structures": [{"structure":"minecraft:pillager_outpost","weight":1}]
            }
        }))
        .unwrap();
        StructureState::new(sets)
    }

    #[test]
    fn start_candidates_is_deterministic() {
        let state = state();
        assert_eq!(
            state.start_candidates(42, 0, 0),
            state.start_candidates(42, 0, 0)
        );
    }

    #[test]
    fn villages_and_pillager_outposts_never_coincide() {
        // 排除区：`pillager_outposts` 起点 10 格内不能有 `villages` 起点。
        // The exclusion zone: a pillager outpost start can't be within 10 chunks of a village.
        let state = state();
        for x in -40..=40 {
            for z in -40..=40 {
                let village = state.is_structure_chunk("minecraft:villages", 42, x, z);
                let outpost = state.is_structure_chunk("minecraft:pillager_outposts", 42, x, z);
                assert!(!(village && outpost), "both at ({x},{z})");
            }
        }
    }
}
