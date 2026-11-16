//! `FeatureChunks` 的读写视图：一个纯内存的 3×3 `WorldGenLevel`。
//!
//! 读写范围照 `WorldGenRegion`（`writeRadius = 1`）：3×3 之外的写直接丢弃，
//! 读超出会抛错（对拍时那意味着地物访问了我们没准备的区块）。

use mc_bamboo_data::{facts, state_facts};

use super::{CENTER, ChunkBlocks, FeatureChunks, FeatureResult, Heightmaps, heightmap::is_opaque};

/// 3×3 世界生成空间的读写接口。
pub struct FeatureWorld<'a> {
    pub center_x: i32,
    pub center_z: i32,
    pub min_y: i32,
    pub height: i32,
    /// `WorldGenRegion.getSeaLevel()`，来自 `NoiseGeneratorSettings.seaLevel()`。
    ///
    /// `WorldGenRegion.getSeaLevel()`, from `NoiseGeneratorSettings.seaLevel()`.
    pub sea_level: i32,
    /// `level.getSeed()`：世界种子，晶洞这类固定参数噪声要用。
    ///
    /// `level.getSeed()`: the world seed, needed by fixed-parameter noises such as the geode.
    pub seed: i64,
    /// 9 个区块的可变方块；中心在 [`CENTER`]。
    pub blocks: [ChunkBlocks; 9],
    /// 只读的 quart 群系格。
    pub biomes: &'a [Vec<Vec<u32>>; 9],
    /// primed 高度图（terrain 阶段算好的）；`set_block_with_heightmaps` 会按
    /// `Heightmap.update` 增量更新四种 FINAL 类型。拥有一份可变拷贝，不借用输入，
    /// 后续 feature 才能读到更新后的值。
    ///
    /// Primed heightmaps, owned so `set_block_with_heightmaps` can update the four
    /// FINAL types incrementally via `Heightmap.update`; later features read the
    /// updated values.
    pub heights: [Heightmaps; 9],
    /// 8 个邻居的写增量，中心不记（中心直接交回 `blocks[4]`）。
    pub deltas: [Vec<(i32, i32, i32, u32)>; 9],
    /// 写进去的方块实体 nbt，`(世界坐标, nbt)`，按写入顺序。`ChunkAccess` 在
    /// `setBlockState` 里会把旧方块实体丢掉，所以这里也是「最后写入者胜」。
    ///
    /// Block-entity nbt written into the world, `(world position, nbt)` in write order.
    /// `ChunkAccess.setBlockState` drops the old block entity, so the last writer wins
    /// here too.
    pub block_entities: Vec<((i32, i32, i32), mc_bamboo_nbt::Tag)>,
}

impl<'a> FeatureWorld<'a> {
    pub fn new(input: &'a FeatureChunks, sea_level: i32, seed: i64) -> Result<Self, String> {
        if input.blocks.len() != 9 || input.biomes.len() != 9 || input.heights.len() != 9 {
            return Err("FeatureChunks must contain 9 chunks".to_owned());
        }
        let empty = || -> [Vec<(i32, i32, i32, u32)>; 9] { std::array::from_fn(|_| Vec::new()) };
        Ok(Self {
            center_x: input.center_x,
            center_z: input.center_z,
            min_y: input.min_y,
            height: input.height,
            sea_level,
            seed,
            blocks: input.blocks.clone(),
            biomes: &input.biomes,
            heights: input.heights,
            deltas: empty(),
            block_entities: Vec::new(),
        })
    }

    /// 交出结果。写增量里每一格只留最后一次写：一次地物运行可能把同一格写好几次
    /// （树叶先放成 `distance=7`，`updateLeaves` 再改成真实距离；藤蔓放下后又被
    /// `updateShapeAtEdge` 移除），邻居区块要的是最终状态。调用方之后把多个中心的增量
    /// 合到一起、按规范序落盘，那一步分不出先后，所以必须在这里先收敛。
    ///
    /// Hand back the result. Each cell keeps only its last write: one feature run can
    /// write a cell several times (a leaf is placed with `distance=7` and `updateLeaves`
    /// then rewrites the real distance; a vine is placed and `updateShapeAtEdge` removes
    /// it), and the neighbour chunk needs the final state. The caller later merges
    /// several centres' deltas and applies them in canonical order, which cannot tell
    /// earlier from later, so the collapse has to happen here.
    pub fn finish(self, unsupported: bool) -> FeatureResult {
        FeatureResult {
            blocks: self.blocks,
            deltas: self.deltas.map(last_writes),
            block_entities: self.block_entities,
            unsupported,
        }
    }

    /// `LevelWriter.setBlockEntity` / `ChunkAccess.setBlockEntityNbt`：记下该位置的方块实体
    /// nbt。同一位置再次写入会覆盖前一次（`setBlockState` 先丢旧方块实体）。
    ///
    /// `LevelWriter.setBlockEntity` / `ChunkAccess.setBlockEntityNbt`: record the block
    /// entity nbt for that position. A later write to the same position replaces the
    /// earlier one (`setBlockState` drops the old block entity first).
    pub fn set_block_entity(&mut self, x: i32, y: i32, z: i32, nbt: &mc_bamboo_nbt::Tag) {
        self.block_entities.retain(|(pos, _)| *pos != (x, y, z));
        self.block_entities.push(((x, y, z), nbt.clone()));
    }

    /// `level.getFluidState(pos)`：流体类型与是否源，由状态表给出。
    ///
    /// `level.getFluidState(pos)`: the fluid type and whether it is a source, both from the
    /// state table.
    pub fn fluid_state(&self, x: i32, y: i32, z: i32) -> (bool, bool) {
        let fluid = mc_bamboo_data::state_fluid(self.get_block(x, y, z));
        // `FluidState.is(Fluids.WATER)`：`water` 与 `flowing_water` 都算。
        // `is(WATER)` accepts both `water` and `flowing_water`.
        let is_water = fluid.fluid == mc_bamboo_data::registries::fluid::WATER
            || fluid.fluid == mc_bamboo_data::registries::fluid::FLOWING_WATER;
        (is_water, fluid.source)
    }

    /// 区块坐标转 9 宫格下标；超出 3×3 返回 `None`。
    pub fn index_of(&self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        let dx = chunk_x - self.center_x;
        let dz = chunk_z - self.center_z;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            return None;
        }
        Some(((dz + 1) * 3 + dx + 1) as usize)
    }

    fn block_index(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize)> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        let index = self.index_of(x >> 4, z >> 4)?;
        let local_y = (y - self.min_y) as usize;
        let local = ((x & 15) as usize) * 16 + (z & 15) as usize;
        Some((index, local_y * 256 + local))
    }

    /// `WorldGenRegion.getBlockState`：范围外返回空气（不 panic）。
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        self.block_index(x, y, z)
            .map(|(chunk, index)| self.blocks[chunk][index / 256][index % 256])
            .unwrap_or(mc_bamboo_data::blocks::AIR)
    }

    /// `WorldGenRegion.ensureCanWrite`：写半径 1，即 3×3。
    pub fn can_write(&self, x: i32, y: i32, z: i32) -> bool {
        if y < self.min_y || y >= self.min_y + self.height {
            return false;
        }
        self.index_of(x >> 4, z >> 4).is_some()
    }

    /// `WorldGenRegion.setBlock`：`ensureCanWrite` + `ChunkAccess.setBlockState`。
    ///
    /// `OreFeature` 走 `BulkSectionAccess` 直接写 `LevelChunkSection`，
    /// 不触发 `ChunkAccess` 的高度图更新，所以这里也不更新 `heights`。
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        let Some((chunk, index)) = self.block_index(x, y, z) else {
            return false;
        };
        self.blocks[chunk][index / 256][index % 256] = state;
        if chunk != CENTER {
            self.deltas[chunk].push((x, y, z, state));
        }
        true
    }

    /// `Feature.safeSetBlock` / `level.setBlock`：先写 `ChunkAccess.setBlockState`，
    /// 再按 `Heightmap.update` 增量更新四种 FINAL 高度图。只有走 `setBlockState` 的
    /// 地物（simple_block / tree / 组合类里真正放方块的那些）用这条；`OreFeature` 走
    /// `BulkSectionAccess`，仍用 [`Self::set_block`]。
    ///
    /// `Feature.safeSetBlock` path: write via `ChunkAccess.setBlockState` then
    /// incrementally update the four FINAL heightmaps. Only features that go through
    /// `setBlockState` use this; `OreFeature` (BulkSectionAccess) uses `set_block`.
    pub fn set_block_with_heightmaps(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        let Some((chunk, index)) = self.block_index(x, y, z) else {
            return false;
        };
        self.blocks[chunk][index / 256][index % 256] = state;
        if chunk != CENTER {
            self.deltas[chunk].push((x, y, z, state));
        }
        // 两个不相交的字段借用：`blocks` 只读（向下重扫要用），`heights` 可变。
        // Disjoint field borrows: `blocks` shared (for the downward rescan), `heights` mutable.
        let blocks = &self.blocks[chunk];
        let heights = &mut self.heights[chunk];
        super::heightmap::update(heights, blocks, self.min_y, x, y, z, state);
        true
    }

    /// `WorldGenRegion.getHeight(type, x, z)` = `ChunkAccess.getHeight + 1`。
    pub fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        let Some(index) = self.index_of(x >> 4, z >> 4) else {
            return self.min_y;
        };
        let column = (x & 15) as usize * 16 + (z & 15) as usize;
        let heights = &self.heights[index];
        let value = match kind {
            HeightmapKind::WorldSurfaceWg => heights.world_surface_wg[column],
            HeightmapKind::WorldSurface => heights.world_surface[column],
            HeightmapKind::OceanFloorWg => heights.ocean_floor_wg[column],
            HeightmapKind::OceanFloor => heights.ocean_floor[column],
            HeightmapKind::MotionBlocking => heights.motion_blocking[column],
            HeightmapKind::MotionBlockingNoLeaves => heights.motion_blocking_no_leaves[column],
        };
        value + 1
    }

    /// `LevelReader.getNoiseBiome(quartX, quartY, quartZ)`：读已存的 quart 格。
    /// 3×3 之外返回 `None`，由调用方回退到 `getUncachedNoiseBiome()` 单点重算。
    /// quartY 越界按官方 `ChunkAccess.getNoiseBiome` 夹到区块范围内。
    ///
    /// `LevelReader.getNoiseBiome`: read the stored quart cell. Outside the 3x3
    /// this returns `None` so the caller falls back to single-point sampling;
    /// out-of-range quartY is clamped like vanilla `ChunkAccess.getNoiseBiome`.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<u32> {
        noise_biome_in(
            self.biomes,
            self.center_x,
            self.center_z,
            self.min_y,
            quart_x,
            quart_y,
            quart_z,
        )
    }

    /// `BiomeManager.getBiome(x,y,z)` 的逐点平滑：由调用方通过回调给出群系 ID。
    ///
    /// `BiomeFilter` 需要这个，见 `feature::run`。
    pub fn is_air(&self, x: i32, y: i32, z: i32) -> bool {
        state_facts(self.get_block(x, y, z)) & facts::AIR != 0
    }

    /// `LevelHeightAccessor.isInsideBuildHeight(y)`。
    ///
    /// `LevelHeightAccessor.isInsideBuildHeight(y)`.
    pub fn is_inside_build_height(&self, y: i32) -> bool {
        y >= self.min_y && y < self.min_y + self.height
    }

    /// `LevelHeightAccessor.isOutsideBuildHeight(y)`。
    ///
    /// `LevelHeightAccessor.isOutsideBuildHeight(y)`.
    pub fn is_outside_build_height(&self, y: i32) -> bool {
        !self.is_inside_build_height(y)
    }
}

/// `LevelReader.getNoiseBiome` 的纯函数版，只读 3×3 群系格。
/// `BiomeFilter` 的回调在放置阶段要同时可变借用 `FeatureWorld`，所以这里单独
/// 收一份只读借用，避免闭包把整个世界借住。
///
/// Pure read-only version of `LevelReader.getNoiseBiome`. The `BiomeFilter`
/// callback runs while `FeatureWorld` is mutably borrowed by placement, so it
/// takes only the biome array.
pub fn noise_biome_in(
    biomes: &[Vec<Vec<u32>>; 9],
    center_x: i32,
    center_z: i32,
    min_y: i32,
    quart_x: i32,
    quart_y: i32,
    quart_z: i32,
) -> Option<u32> {
    let dx = (quart_x >> 2) - center_x;
    let dz = (quart_z >> 2) - center_z;
    if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
        return None;
    }
    let index = ((dz + 1) * 3 + dx + 1) as usize;
    let cells = biomes.get(index)?;
    let min_quart_y = min_y >> 2;
    let last_quart_y = min_quart_y + cells.len() as i32 * 4 - 1;
    let clamped = quart_y.clamp(min_quart_y, last_quart_y.max(min_quart_y));
    let section = ((clamped - min_quart_y) >> 2) as usize;
    let local = ((quart_x & 3) * 16 + (clamped & 3) * 4 + (quart_z & 3)) as usize;
    cells.get(section)?.get(local).copied()
}

/// `Heightmap.Types` 的六种，顺序和 `heightmaps::TYPES` 一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightmapKind {
    WorldSurfaceWg,
    WorldSurface,
    OceanFloorWg,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

impl HeightmapKind {
    pub fn parse(name: &str) -> Result<Self, String> {
        Ok(match name {
            "WORLD_SURFACE_WG" => Self::WorldSurfaceWg,
            "WORLD_SURFACE" => Self::WorldSurface,
            "OCEAN_FLOOR_WG" => Self::OceanFloorWg,
            "OCEAN_FLOOR" => Self::OceanFloor,
            "MOTION_BLOCKING" => Self::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Self::MotionBlockingNoLeaves,
            _ => return Err(format!("unknown heightmap {name}")),
        })
    }
}

/// 从 terrain 阶段的方块自顶向下扫描出 primed 高度图，判定用官方 `Heightmap.Types.isOpaque()`
/// （`heightmap::is_opaque`）。`WORLD_SURFACE_WG`/`WORLD_SURFACE` 共用 `NOT_AIR`，
/// `OCEAN_FLOOR_WG`/`OCEAN_FLOOR` 共用 `MATERIAL_MOTION_BLOCKING`；
/// `MOTION_BLOCKING` 与 `MOTION_BLOCKING_NO_LEAVES` 还要把含流体（水）的方块算进来。
///
/// Prime heightmaps using the official `Heightmap.Types.isOpaque()` predicates.
/// WORLD_SURFACE(_WG) shares NOT_AIR, OCEAN_FLOOR(_WG) shares MATERIAL_MOTION_BLOCKING,
/// and the two MOTION_BLOCKING variants also count blocks with a non-empty fluid.
pub fn prime_heightmaps(blocks: &ChunkBlocks, min_y: i32, height: i32) -> Heightmaps {
    let _ = height;
    let mut result = Heightmaps::new(min_y);
    for column in 0..256 {
        // 四种谓词都由状态的静态事实决定；按状态 ID 缓存成 4 位掩码，逐列从顶往下
        // 扫时就不必每层重复查 `state_facts` / `state_fluid`。每种找到最高的一格后
        // 就不再需要看它，四种都找到即可停。
        // All four predicates are static facts of a state; cache them as a 4-bit
        // mask. Once each kind has found its topmost cell the column is done.
        const SURFACE: u8 = 1 << 0;
        const OCEAN: u8 = 1 << 1;
        const MOTION: u8 = 1 << 2;
        const MOTION_NO_LEAVES: u8 = 1 << 3;
        let mut pending = SURFACE | OCEAN | MOTION | MOTION_NO_LEAVES;
        let mut world_surface = min_y - 1;
        let mut ocean_floor = min_y - 1;
        let mut motion = min_y - 1;
        let mut motion_no_leaves = min_y - 1;
        for y in (min_y..min_y + blocks.len() as i32).rev() {
            if pending == 0 {
                break;
            }
            let state = blocks[(y - min_y) as usize][column];
            let found = opaque_mask(state) & pending;
            if found & SURFACE != 0 {
                world_surface = y;
                pending &= !SURFACE;
            }
            if found & OCEAN != 0 {
                ocean_floor = y;
                pending &= !OCEAN;
            }
            if found & MOTION != 0 {
                motion = y;
                pending &= !MOTION;
            }
            if found & MOTION_NO_LEAVES != 0 {
                motion_no_leaves = y;
                pending &= !MOTION_NO_LEAVES;
            }
        }
        result.world_surface_wg[column] = world_surface;
        result.world_surface[column] = world_surface;
        result.ocean_floor_wg[column] = ocean_floor;
        result.ocean_floor[column] = ocean_floor;
        result.motion_blocking[column] = motion;
        result.motion_blocking_no_leaves[column] = motion_no_leaves;
    }
    result
}

/// 一个方块状态对四种高度图各自算不算「地面」，按状态 ID 缓存。
///
/// 0 表示「还没算过」；`1<<4` 是哨兵，让「四位全 0」的空气也能命中缓存。
/// 掩码是状态的静态事实，并发写同一个槽只会写入完全相同的值。
///
/// Which of the four heightmaps count a block state as ground, cached by state id.
/// Zero means "not computed"; `1<<4` is a sentinel so an all-zero mask is cached too.
fn opaque_mask(state: u32) -> u8 {
    use mc_bamboo_data::{facts, state_facts, state_fluid};
    use std::sync::atomic::{AtomicU8, Ordering};
    const SIZE: usize = mc_bamboo_data::blocks::TOTAL_STATES as usize;
    static CACHE: [AtomicU8; SIZE] = [const { AtomicU8::new(0) }; SIZE];
    let slot = &CACHE[state as usize];
    let cached = slot.load(Ordering::Relaxed);
    if cached != 0 {
        return cached & 0xf;
    }
    let bits = state_facts(state);
    let has_fluid = state_fluid(state).fluid != mc_bamboo_data::registries::fluid::EMPTY;
    let mut mask = 0;
    if bits & facts::AIR == 0 {
        mask |= 1 << 0;
        // WORLD_SURFACE_WG 和 WORLD_SURFACE 是同一个谓词，合成一位。
    }
    if bits & facts::MOTION_BLOCKING != 0 {
        mask |= 1 << 1;
    }
    if bits & facts::MOTION_BLOCKING != 0 || has_fluid {
        mask |= 1 << 2;
    }
    if bits & facts::MOTION_BLOCKING_NO_LEAVES != 0 || has_fluid {
        mask |= 1 << 3;
    }
    slot.store(mask | 1 << 4, Ordering::Relaxed);
    mask
}

/// `Heightmap.Types.isOpaque()`，转发到 [`super::heightmap::is_opaque`]。
pub fn is_heightmap_opaque(state: u32, kind: HeightmapKind) -> bool {
    is_opaque(state, kind)
}

/// 每一格只留最后一次写，保持这些「最后一次写」原来的先后顺序。
///
/// Keep only the last write of each cell, in the original order of those last writes.
fn last_writes(deltas: Vec<(i32, i32, i32, u32)>) -> Vec<(i32, i32, i32, u32)> {
    let mut seen = std::collections::HashSet::with_capacity(deltas.len());
    let mut out: Vec<_> = deltas
        .into_iter()
        .rev()
        .filter(|&(x, y, z, _)| seen.insert((x, y, z)))
        .collect();
    out.reverse();
    out
}

#[cfg(test)]
mod prime_heightmap_tests {
    use super::*;
    use mc_bamboo_data::blocks;

    /// 掩码缓存对每个状态、每种高度图都与原谓词逐位相同。
    /// The cached mask matches the per-cell predicate for every state and kind.
    #[test]
    fn cached_masks_match_is_opaque() {
        for state in 0..blocks::TOTAL_STATES {
            let mask = opaque_mask(state);
            assert_eq!(
                mask & 1 << 0 != 0,
                is_opaque(state, HeightmapKind::WorldSurface)
            );
            assert_eq!(
                mask & 1 << 1 != 0,
                is_opaque(state, HeightmapKind::OceanFloor)
            );
            assert_eq!(
                mask & 1 << 2 != 0,
                is_opaque(state, HeightmapKind::MotionBlocking)
            );
            assert_eq!(
                mask & 1 << 3 != 0,
                is_opaque(state, HeightmapKind::MotionBlockingNoLeaves)
            );
            assert_eq!(mask & !0xf, 0, "state {state}");
        }
    }

    /// 提前退出后的 `prime_heightmaps` 与逐格调用 `is_opaque` 的实现等价。
    /// Early-exit `prime_heightmaps` equals the straightforward per-cell version.
    #[test]
    fn early_exit_matches_the_full_scan() {
        // 三层：全空气 / 混有石头、水、树叶 / 全石头；每列用不同状态交错。
        let mut blocks: ChunkBlocks = vec![vec![blocks::AIR; 256]; 40];
        for column in 0..256 {
            let state = [
                blocks::AIR,
                blocks::STONE,
                blocks::WATER,
                blocks::OAK_LEAVES,
            ][column % 4];
            blocks[7][column] = state;
            blocks[19][column] = blocks::STONE;
            blocks[31][column] = if column % 3 == 0 {
                blocks::WATER
            } else {
                blocks::AIR
            };
        }
        let expected = {
            let mut result = Heightmaps::new(-64);
            for column in 0..256 {
                for y in (-64..-64 + blocks.len() as i32).rev() {
                    let state = blocks[(y + 64) as usize][column];
                    if is_opaque(state, HeightmapKind::WorldSurfaceWg) {
                        result.world_surface[column] = result.world_surface[column].max(y);
                    }
                    if is_opaque(state, HeightmapKind::OceanFloorWg) {
                        result.ocean_floor[column] = result.ocean_floor[column].max(y);
                    }
                    if is_opaque(state, HeightmapKind::MotionBlocking) {
                        result.motion_blocking[column] = result.motion_blocking[column].max(y);
                    }
                    if is_opaque(state, HeightmapKind::MotionBlockingNoLeaves) {
                        result.motion_blocking_no_leaves[column] =
                            result.motion_blocking_no_leaves[column].max(y);
                    }
                }
            }
            result
        };
        let got = prime_heightmaps(&blocks, -64, 40);
        assert_eq!(got.world_surface, expected.world_surface);
        assert_eq!(got.ocean_floor, expected.ocean_floor);
        assert_eq!(got.motion_blocking, expected.motion_blocking);
        assert_eq!(
            got.motion_blocking_no_leaves,
            expected.motion_blocking_no_leaves
        );
        assert_eq!(got.world_surface_wg, got.world_surface);
        assert_eq!(got.ocean_floor_wg, got.ocean_floor);
    }
}

#[cfg(test)]
mod last_write_tests {
    use super::last_writes;

    #[test]
    fn a_cell_written_twice_keeps_the_later_state() {
        // 第一格先写 294 再写 270：留下 270，哪怕 294 的编号更大。
        // The first cell is written 294 then 270: 270 stays although 294 is larger.
        let deltas = vec![(1, 70, 1, 294), (2, 70, 1, 139), (1, 70, 1, 270)];
        assert_eq!(last_writes(deltas), vec![(2, 70, 1, 139), (1, 70, 1, 270)]);
    }

    /// 输出顺序是各「最后一次写」在输入里的先后，不是按坐标或状态排序。
    /// The output keeps the original order of the last writes, not a key/state sort.
    #[test]
    fn last_writes_keep_the_original_last_write_order() {
        let deltas = vec![
            (7, 64, 3, 10), // A 首次
            (1, 70, 1, 20), // B 首次（被后面覆写）
            (9, 80, 9, 30), // C 唯一
            (1, 70, 1, 21), // B 最后一次
            (7, 64, 3, 11), // A 最后一次
            (5, 65, 5, 40), // D 唯一
            (9, 80, 9, 31), // C 最后一次
        ];
        assert_eq!(
            last_writes(deltas),
            vec![
                (1, 70, 1, 21),
                (7, 64, 3, 11),
                (5, 65, 5, 40),
                (9, 80, 9, 31),
            ]
        );
        assert_eq!(last_writes(Vec::new()), Vec::<(i32, i32, i32, u32)>::new());
        assert_eq!(
            last_writes(vec![(0, 0, 0, 1), (0, 0, 0, 2), (0, 0, 0, 3)]),
            vec![(0, 0, 0, 3)]
        );
    }
}

#[cfg(test)]
mod block_writer_tests {
    use crate::feature::world::FeatureWorld;
    use crate::structure::jigsaw::place::BlockWriter;

    /// 一个 y=64 是水的 3×3 输入。`FeatureWorld` 借用它，所以由调用方持有。
    ///
    /// A 3x3 input whose y=64 is water. `FeatureWorld` borrows it, so the caller owns it.
    fn water_chunks() -> crate::feature::FeatureChunks {
        use crate::feature::{CENTER, ChunkBlocks, FeatureChunks, Heightmaps};
        let min_y = -64;
        let height = 384;
        let blocks: ChunkBlocks = vec![vec![mc_bamboo_data::blocks::AIR; 256]; height as usize];
        let biomes = std::array::from_fn(|_| vec![vec![0u32; 64]; 4]);
        let mut blocks_all: [ChunkBlocks; 9] = std::array::from_fn(|_| blocks.clone());
        blocks_all[CENTER] = blocks;
        // `minecraft:water[level=0]` 是水方块：流体是 `water`、`source = true`。
        // `minecraft:water[level=0]` is a water block: fluid `water`, `source = true`.
        let water = mc_bamboo_data::block_by_name("minecraft:water")
            .expect("water block")
            .default_state;
        blocks_all[CENTER][(64 - min_y) as usize][0] = water;
        FeatureChunks {
            center_x: 0,
            center_z: 0,
            min_y,
            height,
            blocks: blocks_all,
            biomes,
            heights: [Heightmaps::new(min_y); 9],
        }
    }

    /// `fluid_at` 从状态表读出「是水 / 是否源」，`block_state` 读真实状态。
    ///
    /// `fluid_at` reads the fluid kind and source flag from the state table; `block_state`
    /// reads the real state.
    #[test]
    fn fluid_at_and_block_state_read_the_world() {
        let chunks = water_chunks();
        let world = FeatureWorld::new(&chunks, 63, 0).unwrap();
        // y=64 有源水；y=65 是空气。
        // y=64 holds source water; y=65 is air.
        assert_eq!(world.fluid_state(0, 64, 0), (true, true));
        assert_eq!(world.fluid_state(0, 65, 0), (false, false));
        assert_eq!(
            world.block_state(0, 64, 0),
            mc_bamboo_data::block_by_name("minecraft:water")
                .unwrap()
                .default_state
        );
        // 3×3 之外读空气：与 `WorldGenRegion.getBlockState` 一致。
        // Outside the 3x3 it reads air, matching `WorldGenRegion.getBlockState`.
        assert_eq!(
            world.block_state(1000, 64, 1000),
            mc_bamboo_data::blocks::AIR
        );
    }

    /// 写进流体源的水可含方块会被灌上 `waterlogged`，这只有在 `fluid_at` 真的返回流体
    /// 模型时才发生——默认实现返回 `None` 时整段 watering 跳过。
    ///
    /// A waterloggable block written into a fluid source gets `waterlogged = true`, which
    /// only happens when `fluid_at` actually returns a fluid model; the default `None`
    /// skips the whole watering pass.
    #[test]
    fn watering_works_through_the_real_world() {
        use crate::structure::jigsaw::place::{PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;

        let chunks = water_chunks();
        let mut world = FeatureWorld::new(&chunks, 63, 0).unwrap();
        let bb = crate::structure::BoundingBox {
            min_x: -16,
            min_y: 0,
            min_z: -16,
            max_x: 16,
            max_y: 128,
            max_z: 16,
        };
        let blocks = vec![PlaceBlock {
            pos: (0, 64, 0),
            state: BlockStateSpec::parse_str("minecraft:oak_slab[type=bottom,waterlogged=false]")
                .unwrap(),
            nbt: None,
        }];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0));
        place_processed(&blocks, bb, 0, 0, &random, &mut world).unwrap();
        let state = world.get_block(0, 64, 0);
        assert_eq!(
            crate::structure::state_transform::property_of(state, "waterlogged"),
            Some("true"),
            "the slab in the water source is waterlogged"
        );
    }

    /// `set_block_entity` 记下 nbt，后写覆盖前写。
    ///
    /// `set_block_entity` records the nbt; a later write replaces an earlier one.
    #[test]
    fn set_block_entity_keeps_the_last_write() {
        use mc_bamboo_nbt::Tag;

        let chunks = water_chunks();
        let mut world = FeatureWorld::new(&chunks, 63, 0).unwrap();
        let first = Tag::Compound(vec![("a".to_owned(), Tag::Int(1))]);
        let second = Tag::Compound(vec![("a".to_owned(), Tag::Int(2))]);
        world.set_block_entity(1, 65, 1, &first);
        assert_eq!(world.block_entities, vec![((1, 65, 1), first.clone())]);
        world.set_block_entity(1, 65, 1, &second);
        assert_eq!(
            world.block_entities,
            vec![((1, 65, 1), second)],
            "the later write wins"
        );
        world.set_block_entity(2, 65, 1, &first);
        assert_eq!(
            world.block_entities.len(),
            2,
            "another position is separate"
        );
    }

    /// `finish` 把方块实体表带进 `FeatureResult`，上层才拿得到。
    ///
    /// `finish` carries the block-entity table into `FeatureResult` so callers see it.
    #[test]
    fn finish_carries_the_block_entities() {
        use mc_bamboo_nbt::Tag;

        let chunks = water_chunks();
        let mut world = FeatureWorld::new(&chunks, 63, 0).unwrap();
        let nbt = Tag::Compound(vec![("LootTable".to_owned(), Tag::String("x".to_owned()))]);
        world.set_block_entity(1, 65, 1, &nbt);
        let result = world.finish(false);
        assert_eq!(result.block_entities, vec![((1, 65, 1), nbt)]);
    }

    /// 端到端：真实模板（村庄房屋带箱子）走 `place_piece` 写进 `FeatureWorld`，
    /// 带 nbt 的位置要出现在 `block_entities` 里，并随 `finish` 交出去。
    ///
    /// End to end: a real template (a village house with chests) goes through
    /// `place_piece` into a `FeatureWorld`; positions carrying nbt must show up in
    /// `block_entities` and be handed out by `finish`.
    #[test]
    fn place_piece_fills_the_block_entity_table() {
        use crate::structure::jigsaw::place::{place_piece, state_id_of};
        use crate::structure::processor::BlockStateSpec;
        use crate::structure::processor_apply::PlaceSettings;
        use crate::structure::template::StructureTemplate;

        // 挑一个带方块实体 nbt 的模板。
        // Pick a template whose palette blocks carry block-entity nbt.
        let mut chosen: Option<(String, StructureTemplate)> = None;
        for &(name, _, _) in mc_bamboo_data::structures::ENTRIES {
            let Ok(template) = StructureTemplate::load(name) else {
                continue;
            };
            if template.palettes.len() != 1 || template.blocks.is_empty() {
                continue;
            }
            if template.blocks.iter().any(|block| block.nbt.is_some()) {
                chosen = Some((name.to_owned(), template));
                break;
            }
        }
        let Some((name, template)) = chosen else {
            return; // 数据里没有带 nbt 的模板：跳过。
        };

        let chunks = water_chunks();
        let mut world = FeatureWorld::new(&chunks, 63, 0).unwrap();
        struct Level;
        impl crate::structure::processor_apply::ProcessorLevel for Level {
            fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
                BlockStateSpec::parse_str("minecraft:air").unwrap()
            }
            fn in_block_set(
                &self,
                _state: &BlockStateSpec,
                _set: &crate::structure::processor::BlockSetRef,
            ) -> bool {
                false
            }
        }
        let level = Level;
        let bounds = crate::structure::BoundingBox {
            min_x: -64,
            min_y: -64,
            min_z: -64,
            max_x: 64,
            max_y: 128,
            max_z: 64,
        };
        let placed = place_piece(
            &template,
            (0, 64, 0),
            &[],
            &level,
            PlaceSettings {
                piece_position: (0, 64, 0),
                reference_pos: (0, 64, 0),
                chunk_bb: Some(bounds),
                level_seed: 0,
                explicit_random: None,
            },
            0,
            0,
            &crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)),
            &mut world,
        )
        .unwrap_or_else(|error| panic!("{name}: place_piece failed: {error}"));
        assert!(
            !placed.placed.is_empty(),
            "{name}: expected some block writes"
        );
        // 所有落在 `chunk_bb` 内的带 nbt 方块都应该登记到方块实体表。
        // Every nbt-carrying block inside `chunk_bb` must be registered.
        let expected = placed
            .placed
            .iter()
            .filter(|(_, nbt)| nbt.is_some())
            .count();
        assert!(
            expected > 0,
            "{name}: template has block-entity nbt but none got written"
        );
        assert_eq!(
            world.block_entities.len(),
            expected,
            "{name}: every nbt write must land in the block-entity table"
        );
        let result = world.finish(false);
        assert_eq!(result.block_entities.len(), expected);
        let _ = state_id_of(&BlockStateSpec::parse_str("minecraft:air").unwrap());
    }
}
