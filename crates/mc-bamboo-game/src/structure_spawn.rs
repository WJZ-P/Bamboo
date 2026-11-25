//! `ChunkGenerator.getMobsAt` 的结构刷怪覆盖：位置落在某结构的覆盖盒里时，用该结构
//! 的加权表替换群系表（掠夺者前哨站、女巫小屋等）。
//!
//! 官方先取该区块的 `STRUCTURE_REFERENCES`（`getAllStructuresAt`），再逐个引用区块读
//! `STRUCTURE_STARTS`，按 `StructureSpawnOverride.boundingBox` 判断位置：`STRUCTURE`
//! 用整个起点的包围盒（含 `terrainAdaptation` 外扩），`PIECE` 要求落在某个结构件里。
//! 命中就返回覆盖表，没命中回退群系表。
//!
//! 结构是种子的纯函数，所以按区块把「覆盖行 + 包围盒 + 结构件盒」预先算好存进
//! [`StructureSpawnIndex`]，之后每次查询只查表，不再克隆引用表、也不再逐次重进
//! 结构阶段（那条路径的缓存命中也会 reset 群系 R 树 / 采样器上下文）。
//!
//! The structure spawn override of `ChunkGenerator.getMobsAt`: inside a structure's
//! override box the structure's weighted list replaces the biome table (pillager
//! outposts, swamp huts, ...). Vanilla reads the chunk's `STRUCTURE_REFERENCES`
//! (`getAllStructuresAt`), then each referenced chunk's `STRUCTURE_STARTS`, and checks the
//! position against `StructureSpawnOverride.boundingBox`: `STRUCTURE` uses the start's
//! whole bounding box (with the `terrainAdaptation` inflation), `PIECE` requires a piece
//! box. A hit returns the override list, otherwise the biome table is used.
//!
//! Structure data is a pure function of the seed, so the per-chunk
//! (override row + boxes + piece boxes) is precomputed into [`StructureSpawnIndex`];
//! later queries only look up that table.

use std::{cell::RefCell, collections::HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::spawning::{self, MobCategory, MobSpawnerDef, SpawnOverrideBox};
use mc_bamboo_worldgen::structure::{
    pipeline::WorldStructureStart,
    start::{BoundingBox, StructurePiece},
};

use crate::world_options::WorldGenConfig;

/// 一个区块里所有可能命中的结构覆盖行，按官方引用遍历序排好。
///
/// All structure override rows that can hit a chunk, in vanilla reference order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChunkSpawnRows {
    rows: Vec<SpawnRow>,
}

impl PartialEq for ChunkSpawnRows {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl Eq for ChunkSpawnRows {}

/// 一条已拍平的覆盖行：类别、覆盖表和两种盒。
///
/// One flattened override row: category, override list and both kinds of box.
#[derive(Debug, Clone)]
struct SpawnRow {
    category: MobCategory,
    spawns: &'static [MobSpawnerDef],
    bounding: SpawnOverrideBox,
    whole: [i32; 6],
    pieces: Vec<[i32; 6]>,
}

impl SpawnRow {
    fn contains(&self, pos: BlockPos) -> bool {
        match self.bounding {
            SpawnOverrideBox::Structure => box_contains(&self.whole, pos),
            SpawnOverrideBox::Piece => self.pieces.iter().any(|piece| box_contains(piece, pos)),
        }
    }
}

impl PartialEq for SpawnRow {
    fn eq(&self, other: &Self) -> bool {
        self.category == other.category
            && std::ptr::eq(self.spawns, other.spawns)
            && self.bounding == other.bounding
            && self.whole == other.whole
            && self.pieces == other.pieces
    }
}

impl Eq for SpawnRow {}

fn box_contains(box_: &[i32; 6], pos: BlockPos) -> bool {
    box_[0] <= pos.x
        && pos.x <= box_[3]
        && box_[1] <= pos.y
        && pos.y <= box_[4]
        && box_[2] <= pos.z
        && pos.z <= box_[5]
}

fn box_of(bb: BoundingBox) -> [i32; 6] {
    [bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z]
}

fn piece_box(piece: &StructurePiece) -> [i32; 6] {
    box_of(piece.bounding_box)
}

/// 按岛维护的结构刷怪索引：区块加载或者第一次被问到时算一次，之后直接查表。
///
/// Per-island structure spawn index: computed once when the chunk is loaded or first
/// queried, then reused.
#[derive(Debug, Clone, Default)]
pub struct StructureSpawnIndex {
    chunks: HashMap<ChunkPos, ChunkSpawnRows>,
    /// 已经按这份配置算过索引的区块；`None` 表示还没有配置（索引空）。
    config: Option<WorldGenConfig>,
}

impl StructureSpawnIndex {
    /// 世界生成配置换了（读档改种子）时清掉按旧配置算出的行。
    ///
    /// Drop rows computed under a different world-generation config.
    fn ensure_config(&mut self, config: &WorldGenConfig) {
        if self.config.as_ref() != Some(config) {
            self.chunks.clear();
            self.config = Some(config.clone());
        }
    }

    /// 登记一个区块的拍平行；`None` 表示这个区块没有任何结构覆盖（空行永真）。
    ///
    /// Store a chunk's flattened rows; `None` means no structure override at all.
    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        config: &WorldGenConfig,
        chunk: ChunkPos,
        rows: ChunkSpawnRows,
    ) {
        self.ensure_config(config);
        self.chunks.insert(chunk, rows);
    }

    /// 岛拆分：把 `chunks` 里的条目搬到新索引里，两边都不保留多余的行。结果是纯函数，
    /// 留下来的任何一边都能按需重算，所以这里不搬也不会改变答案。
    ///
    /// Island split: move the entries of `chunks` into the new index. The result is a
    /// pure function, so leaving entries in the old index cannot change the answer.
    pub fn split_off(&mut self, chunks: &std::collections::BTreeSet<ChunkPos>) -> Self {
        let mut moved = Self {
            chunks: HashMap::new(),
            config: self.config.clone(),
        };
        for &chunk in chunks {
            if let Some(rows) = self.chunks.remove(&chunk) {
                moved.chunks.insert(chunk, rows);
            }
        }
        moved
    }

    /// 岛合并：把另一份索引的行并进来；配置不同时保留自己的行（纯函数重算）。
    ///
    /// Island merge: absorb another index's rows; rows computed under a different
    /// config are dropped and recomputed on demand.
    pub fn absorb(&mut self, mut other: Self) {
        if self.config.is_none() {
            self.config = other.config.clone();
        }
        if self.config == other.config {
            self.chunks.extend(std::mem::take(&mut other.chunks));
        }
    }

    /// 这个索引里已经算过多少个区块（压测 / 单测用）。
    ///
    /// How many chunks have been computed (used by benchmarks and tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// 索引是否为空。
    ///
    /// Whether the index is empty.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// 拍平一个区块的全部结构起点：按 `STRUCTURE_SPAWN_OVERRIDES` 的注册顺序、引用里
/// packed 键的顺序，把命中类别的行的两种盒都存下来。
///
/// Flatten all structure starts for one chunk: in registry and reference order, store
/// both kinds of boxes for every row whose category has an override.
pub(crate) fn build_rows(
    references: &mc_bamboo_worldgen::structure::References,
    starts: impl Fn(
        i32,
        i32,
    ) -> Option<
        std::sync::Arc<mc_bamboo_worldgen::structure::pipeline::WorldStructureStarts>,
    >,
) -> ChunkSpawnRows {
    let mut rows = Vec::new();
    for (structure, keys) in references {
        for row in spawning::STRUCTURE_SPAWN_OVERRIDES
            .iter()
            .filter(|row| row.structure == structure)
        {
            for &packed in keys {
                let (x, z) = crate::worldgen_structures::unpack_chunk_pos(packed);
                let Some(all) = starts(x, z) else {
                    continue;
                };
                let Some(start) = all.get(structure) else {
                    continue;
                };
                if !start.is_valid() {
                    continue;
                }
                rows.push(flat_row(row, start));
            }
        }
    }
    ChunkSpawnRows { rows }
}

fn flat_row(
    row: &'static spawning::StructureSpawnOverride,
    start: &WorldStructureStart,
) -> SpawnRow {
    let whole = box_of(start.bounding_box());
    let pieces = match &start.pieces {
        mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(pieces) => pieces
            .iter()
            .map(|piece| {
                [
                    piece.bb[0],
                    piece.bb[1],
                    piece.bb[2],
                    piece.bb[3],
                    piece.bb[4],
                    piece.bb[5],
                ]
            })
            .collect(),
        mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(pieces) => pieces
            .iter()
            .map(|piece| box_of(piece.bounding_box))
            .collect(),
        mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(pieces) => {
            pieces.iter().map(piece_box).collect()
        }
    };
    SpawnRow {
        category: row.category,
        spawns: row.spawns,
        bounding: row.bounding,
        whole,
        pieces,
    }
}

/// 纯查表：按注册顺序返回第一条覆盖这个位置的行的刷怪表。
///
/// Pure table lookup: the first row covering the position, in registry order.
pub(crate) fn resolve_rows(
    rows: &ChunkSpawnRows,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    rows.rows
        .iter()
        .find(|row| row.category == category && row.contains(pos))
        .map(|row| row.spawns)
}

/// 压测 / 差分用的同一条正式路径，从别的 crate 也能量到真实的结构查询。
///
/// The same production path, exposed for benchmarks and differential tests in
/// other crates.
#[doc(hidden)]
pub fn override_at_indexed_for_bench(
    config: &WorldGenConfig,
    index: &RefCell<StructureSpawnIndex>,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    override_at_indexed(config, index, pos, category)
}

/// 按位置和类别查结构刷怪覆盖（正式路径：先取 / 后建按区块的索引，再查表）。
/// 结构数据由世界生成按种子重算（和交付区块时是同一份纯函数），所以不需要读
/// 区块里存的 `structures` 标签。
///
/// The structure override at a position and category (production path: look up or
/// build the per-chunk index, then read the table).
pub(crate) fn override_at_indexed(
    config: &WorldGenConfig,
    index: &RefCell<StructureSpawnIndex>,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    // 查询只写「还没算过的区块」，所有 `&Level` 的调用都是同一世界线程上的单线程借用；
    // 借用冲突时按没有结构覆盖处理（回退群系表），不会 panic。
    //
    // Queries only fill in not-yet-computed chunks; all `&Level` callers are on the
    // world thread. A borrow conflict falls back to the biome table instead of panicking.
    let Ok(mut index) = index.try_borrow_mut() else {
        return None;
    };
    override_at_indexed_mut(config, &mut index, pos, category)
}

fn override_at_indexed_mut(
    config: &WorldGenConfig,
    index: &mut StructureSpawnIndex,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    if !config.generate_structures || !spawning::has_structure_spawn_override(category) {
        return None;
    }
    let chunk = pos.chunk();
    // 热路径只做一次哈希查表。配置换了的时候 `Global::publish` 会清空整张索引，
    // 所以这里不必每次查询都比较整份 `WorldGenConfig`（含超平坦地层数组）。
    //
    // The hot path is a single hash lookup. `Global::publish` clears the whole
    // index when the config changes, so no per-query config comparison here.
    if !index.chunks.contains_key(&chunk) {
        index.ensure_config(config);
        let rows = build_rows_from_worldgen(config, chunk);
        index.chunks.insert(chunk, rows);
    }
    resolve_rows(&index.chunks[&chunk], pos, category)
}

fn build_rows_from_worldgen(config: &WorldGenConfig, chunk: ChunkPos) -> ChunkSpawnRows {
    let Ok(references) =
        crate::worldgen_structures::structure_references_stage(config, chunk.x, chunk.z)
    else {
        return ChunkSpawnRows::default();
    };
    build_rows(&references, |x, z| {
        crate::worldgen_structures::structure_starts(config, x, z).ok()
    })
}

/// 朴素实现，供单测差分和压测 A/B 用：每次查询都重新走引用 → 起点 → 盒判定。
///
/// Naive implementation for differential tests and benchmark A/B runs.
#[doc(hidden)]
pub fn override_at_naive_for_bench(
    config: &WorldGenConfig,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    if !config.generate_structures || !spawning::has_structure_spawn_override(category) {
        return None;
    }
    let chunk = pos.chunk();
    let references =
        crate::worldgen_structures::structure_references_stage(config, chunk.x, chunk.z).ok()?;
    for (structure, keys) in &references {
        let Some(row) = spawning::structure_spawn_override_row(structure, category) else {
            continue;
        };
        for &packed in keys {
            let (x, z) = crate::worldgen_structures::unpack_chunk_pos(packed);
            let Ok(starts) = crate::worldgen_structures::structure_starts(config, x, z) else {
                continue;
            };
            let Some(start) = starts.get(structure) else {
                continue;
            };
            if start.is_valid() && contains(row.bounding, start, pos) {
                return Some(row.spawns);
            }
        }
    }
    None
}

/// 纯判定，供单测和对拍用：给定 `(结构名, 起点)` 对，返回命中的覆盖表。
///
/// Pure check for unit tests and the oracle: given `(structure, start)` pairs, return
/// the override list that covers the position.
pub fn resolve_override<'a>(
    starts: impl IntoIterator<Item = (&'a str, &'a WorldStructureStart)>,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    for (structure, start) in starts {
        let Some(row) = spawning::structure_spawn_override_row(structure, category) else {
            continue;
        };
        if start.is_valid() && contains(row.bounding, start, pos) {
            return Some(row.spawns);
        }
    }
    None
}

/// `StructureSpawnOverride` 的盒判定。
///
/// The box check of `StructureSpawnOverride`.
fn contains(bounding: SpawnOverrideBox, start: &WorldStructureStart, pos: BlockPos) -> bool {
    match bounding {
        SpawnOverrideBox::Structure => start.bounding_box().contains(pos.x, pos.y, pos.z),
        SpawnOverrideBox::Piece => start.pieces.has_piece_at(pos.x, pos.y, pos.z),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mc_bamboo_data::registries::entity_type;
    use mc_bamboo_worldgen::structure::{
        pipeline::{StartPieces, WorldStructureStart, WorldStructureStarts},
        start::{BoundingBox, StructurePiece, chunk_pos_pack},
    };

    use super::*;
    use crate::world_options::{LevelType, WorldGenConfig};
    use crate::worldgen_structures::{
        clear_disk_structures, encode_structures, install_disk_structures,
    };

    /// 一个只有一件、包围盒固定的起点。
    ///
    /// A one-piece start with a fixed box.
    fn start(structure: &str, bb: [i32; 6], adaptation: bool) -> WorldStructureStart {
        WorldStructureStart {
            structure: structure.to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            references: 1,
            pieces: StartPieces::Sparse(vec![StructurePiece {
                // 读盘重建只认注册表里的结构件 id（`load_pieces` 会跳过未知 id），
                // `minecraft:shstart` 是女巫小屋的起始件。
                // Save reload only keeps known piece ids (`load_pieces` drops unknown
                // ones); `minecraft:shstart` is the swamp-hut start piece.
                piece_type: "minecraft:shstart".to_owned(),
                bounding_box: BoundingBox {
                    min_x: bb[0],
                    min_y: bb[1],
                    min_z: bb[2],
                    max_x: bb[3],
                    max_y: bb[4],
                    max_z: bb[5],
                },
                orientation: -1,
                ground_level_delta: 0,
            }]),
            terrain_adaptation: adaptation,
        }
    }

    /// 前哨站用整个起点的盒，并且 `terrainAdaptation` 外扩 12；女巫小屋只认结构件。
    ///
    /// Outposts use the whole start box inflated by 12; swamp huts only piece boxes.
    #[test]
    fn structure_and_piece_boxes_differ() {
        let outpost = start("minecraft:pillager_outpost", [0, 60, 0, 10, 80, 10], true);
        let inside = BlockPos::new(5, 70, 5);
        let inflated = BlockPos::new(-11, 70, 5);
        assert_eq!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                inside,
                MobCategory::Monster
            )
            .map(|table| table[0].entity_type),
            Some(entity_type::PILLAGER)
        );
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                inflated,
                MobCategory::Monster
            )
            .is_some()
        );
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                BlockPos::new(-13, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
        // `PIECE` 只看结构件，不吃外扩。
        let hut = start("minecraft:swamp_hut", [0, 60, 0, 10, 80, 10], false);
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &hut)],
                BlockPos::new(5, 70, 5),
                MobCategory::Monster
            )
            .is_some()
        );
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &hut)],
                BlockPos::new(-11, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
    }

    /// 类别没有注册覆盖时回退群系表（这里返回 `None`）。
    ///
    /// A category without a registered override yields `None` (the biome table).
    #[test]
    fn category_without_override_falls_back() {
        let outpost = start("minecraft:pillager_outpost", [0, 60, 0, 10, 80, 10], true);
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                BlockPos::new(5, 70, 5),
                MobCategory::Creature
            )
            .is_none()
        );
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &outpost)],
                BlockPos::new(20, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
    }

    /// 活路径：读盘来的 `structures` 标签 → 引用 → 起点 → 覆盖表。用一个人造的女巫
    /// 小屋起点，避免为测试扫几万个区块找真结构。
    ///
    /// The live path: an on-disk `structures` tag → references → starts → override.
    #[test]
    fn override_at_follows_stored_structure_references() {
        let config = WorldGenConfig {
            seed: 20261122,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        install_test_hut(&config);
        let index = RefCell::new(StructureSpawnIndex::default());
        assert_eq!(
            override_at_indexed(
                &config,
                &index,
                BlockPos::new(5, 65, 5),
                MobCategory::Monster
            )
            .map(|table| table[0].entity_type),
            Some(entity_type::WITCH)
        );
        assert_eq!(
            override_at_indexed(
                &config,
                &index,
                BlockPos::new(5, 65, 5),
                MobCategory::Creature
            )
            .map(|table| table[0].entity_type),
            Some(entity_type::CAT)
        );
        clear_disk_structures();
    }

    /// 覆盖表是静态数据：两条路径要么都是 `None`，要么是同一张静态表的同一个指针。
    ///
    /// Override lists are static: both paths must return `None` or the same pointer.
    fn same_table(
        a: Option<&'static [MobSpawnerDef]>,
        b: Option<&'static [MobSpawnerDef]>,
    ) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }

    fn install_test_hut(config: &WorldGenConfig) {
        let hut = start("minecraft:swamp_hut", [2, 60, 2, 8, 70, 8], false);
        let mut starts = WorldStructureStarts::default();
        starts.insert(hut);
        let references = std::collections::BTreeMap::from([(
            "minecraft:swamp_hut".to_owned(),
            vec![chunk_pos_pack(0, 0)],
        )]);
        let tag = encode_structures(config, &starts, &references).unwrap();
        install_disk_structures(config, 0, 0, Some(&tag));
    }

    /// 索引路径和朴素路径逐位相同：同一区块里扫一批位置 × 全部类别，且索引只在该
    /// 区块构建一次。
    ///
    /// Indexed and naive paths are bit-identical over a grid of positions and all
    /// categories; the index builds the chunk exactly once.
    #[test]
    fn index_matches_naive_bit_for_bit() {
        let config = WorldGenConfig {
            seed: 20261122,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        install_test_hut(&config);
        let index = RefCell::new(StructureSpawnIndex::default());
        for x in -14..=24 {
            for z in -14..=24 {
                for y in [58, 60, 65, 70, 72] {
                    let pos = BlockPos::new(x, y, z);
                    for category in MobCategory::ALL {
                        assert!(
                            same_table(
                                override_at_indexed(&config, &index, pos, category),
                                override_at_naive_for_bench(&config, pos, category),
                            ),
                            "位置 {pos:?} 类别 {category:?}"
                        );
                    }
                }
            }
        }
        assert_eq!(index.borrow().len(), 9, "x/z 都横跨 -1、0、1 三个区块");
        clear_disk_structures();
    }

    /// 拍平行对 `Layout` / `Jigsaw` / `Sparse` 三种结构件都保留逐位相同的盒判定。
    ///
    /// Flattened rows keep the same box checks for all three piece representations.
    #[test]
    fn flattened_rows_match_start_box_checks() {
        let outpost = start("minecraft:pillager_outpost", [0, 60, 0, 10, 80, 10], true);
        let mut starts = WorldStructureStarts::default();
        starts.insert(outpost.clone());
        let references = std::collections::BTreeMap::from([(
            "minecraft:pillager_outpost".to_owned(),
            vec![chunk_pos_pack(0, 0)],
        )]);
        let rows = build_rows(&references, |x, z| {
            (x == 0 && z == 0).then(|| Arc::new(starts.clone()))
        });
        for x in -14..=24 {
            for z in -14..=24 {
                let pos = BlockPos::new(x, 70, z);
                let naive = resolve_override(
                    [("minecraft:pillager_outpost", &outpost)],
                    pos,
                    MobCategory::Monster,
                );
                assert!(same_table(
                    resolve_rows(&rows, pos, MobCategory::Monster),
                    naive,
                ));
            }
        }
    }

    /// 拆岛 / 并岛只搬行，不改答案。
    ///
    /// Splitting and merging an island moves rows without changing answers.
    #[test]
    fn split_and_absorb_keep_rows() {
        let config = WorldGenConfig::default();
        let mut index = StructureSpawnIndex::default();
        index.insert(&config, ChunkPos::new(1, 2), ChunkSpawnRows::default());
        let moved = index.split_off(&[ChunkPos::new(1, 2)].into_iter().collect());
        assert!(index.is_empty());
        assert_eq!(moved.len(), 1);
        index.absorb(moved);
        assert_eq!(index.len(), 1);
    }

    /// 结构查询单价的压测（`#[ignore]`）：同一份盘上女巫小屋，索引路径查表 vs 朴素路径
    /// 每次克隆引用 + reset 线程状态。数字写进 `docs/performance-optimizations.md` 第 30 节。
    ///
    /// Ignored micro-benchmark of structure query cost: indexed lookup vs the naive
    /// per-query reference clone and thread-state reset.
    #[cfg(test)]
    #[test]
    #[ignore = "timing run, not a check"]
    fn structure_query_cost() {
        use std::time::Instant;

        let config = WorldGenConfig {
            seed: 20261122,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        install_test_hut(&config);
        let index = RefCell::new(StructureSpawnIndex::default());
        let pos = BlockPos::new(5, 65, 5);
        // 先把两条路径的线程本地缓存都热起来。
        for _ in 0..128 {
            let _ = override_at_indexed_for_bench(&config, &index, pos, MobCategory::Monster);
            let _ = override_at_naive_for_bench(&config, pos, MobCategory::Monster);
        }
        const ROUNDS: usize = 3;
        const QUERIES: usize = 200_000;
        let mut indexed = Vec::new();
        let mut naive = Vec::new();
        for round in 0..ROUNDS {
            // 每轮交替顺序，抵掉机器负载漂移。
            let order = if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for is_naive in order {
                let start = Instant::now();
                let mut hits = 0_usize;
                for _ in 0..QUERIES {
                    let table = if is_naive {
                        override_at_naive_for_bench(&config, pos, MobCategory::Monster)
                    } else {
                        override_at_indexed_for_bench(&config, &index, pos, MobCategory::Monster)
                    };
                    hits += usize::from(table.is_some());
                }
                let ns = start.elapsed().as_nanos() as f64 / QUERIES as f64;
                assert_eq!(hits, QUERIES, "全部查询都要命中女巫小屋");
                if is_naive {
                    naive.push(ns);
                } else {
                    indexed.push(ns);
                }
            }
        }
        let median = |mut values: Vec<f64>| {
            values.sort_by(f64::total_cmp);
            values[1]
        };
        let a = median(indexed);
        let b = median(naive);
        println!(
            "structure-query (disk hut): indexed {a:.1} ns/query naive {b:.1} ns/query ratio {:.2}x",
            b / a
        );
        clear_disk_structures();

        // 没有盘上标签的纯生成区块：朴素路径不再逐次解析 NBT，只剩引用表克隆和
        // 线程状态 reset，量这一项的单价。
        let index = RefCell::new(StructureSpawnIndex::default());
        for _ in 0..128 {
            let _ = override_at_indexed_for_bench(&config, &index, pos, MobCategory::Monster);
            let _ = override_at_naive_for_bench(&config, pos, MobCategory::Monster);
        }
        let mut indexed = Vec::new();
        let mut naive = Vec::new();
        for round in 0..ROUNDS {
            let order = if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for is_naive in order {
                let start = Instant::now();
                for _ in 0..QUERIES {
                    let _ = if is_naive {
                        override_at_naive_for_bench(&config, pos, MobCategory::Monster)
                    } else {
                        override_at_indexed_for_bench(&config, &index, pos, MobCategory::Monster)
                    };
                }
                let ns = start.elapsed().as_nanos() as f64 / QUERIES as f64;
                if is_naive {
                    naive.push(ns);
                } else {
                    indexed.push(ns);
                }
            }
        }
        let a = median(indexed);
        let b = median(naive);
        println!(
            "structure-query (generated): indexed {a:.1} ns/query naive {b:.1} ns/query ratio {:.2}x",
            b / a
        );
    }
}
