//! `FeatureChunks` 的读写视图：一个纯内存的 3×3 `WorldGenLevel`。
//!
//! 读写范围照 `WorldGenRegion`（`writeRadius = 1`）：3×3 之外的写直接丢弃，
//! 读超出会抛错（对拍时那意味着地物访问了我们没准备的区块）。

use mc_bamboo_data::{facts, registries, state_facts, state_fluid};

use super::{CENTER, ChunkBlocks, FeatureChunks, FeatureResult, Heightmaps};

/// 3×3 世界生成空间的读写接口。
pub struct FeatureWorld<'a> {
    pub center_x: i32,
    pub center_z: i32,
    pub min_y: i32,
    pub height: i32,
    /// `WorldGenRegion.getSeaLevel()`，来自 `NoiseGeneratorSettings.seaLevel()`。
    /// `WorldGenRegion.getSeaLevel()`, from `NoiseGeneratorSettings.seaLevel()`.
    pub sea_level: i32,
    /// 9 个区块的可变方块；中心在 [`CENTER`]。
    pub blocks: [ChunkBlocks; 9],
    /// 只读的 quart 群系格。
    pub biomes: &'a [Vec<Vec<u32>>; 9],
    /// primed 高度图；`set_block_state` 按 `Heightmap.update` 增量更新。
    /// Primed heightmaps; `set_block_state` updates them via `Heightmap.update`.
    pub heights: [Heightmaps; 9],
    /// 8 个邻居的写增量，中心不记（中心直接交回 `blocks[4]`）。
    pub deltas: [Vec<(i32, i32, i32, u32)>; 9],
}

impl<'a> FeatureWorld<'a> {
    pub fn new(input: &'a FeatureChunks, sea_level: i32) -> Result<Self, String> {
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
            blocks: input.blocks.clone(),
            biomes: &input.biomes,
            heights: input.heights,
            deltas: empty(),
        })
    }

    pub fn finish(self, unsupported: bool) -> FeatureResult {
        FeatureResult {
            blocks: self.blocks,
            deltas: self.deltas,
            unsupported,
        }
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
    /// OreFeature writes through BulkSectionAccess (bypassing setBlockState), so no
    /// heightmap update here.
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

    /// `level.setBlock(...)` 走 `ChunkAccess.setBlockState`：写方块并按
    /// `Heightmap.update` 增量更新 `FINAL_HEIGHTMAPS` 四种高度图。
    ///
    /// `level.setBlock(...)` goes through `ChunkAccess.setBlockState`: writes the
    /// block and incrementally updates the four `FINAL_HEIGHTMAPS` via `Heightmap.update`.
    pub fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        let Some((chunk, index)) = self.block_index(x, y, z) else {
            return false;
        };
        self.blocks[chunk][index / 256][index % 256] = state;
        if chunk != CENTER {
            self.deltas[chunk].push((x, y, z, state));
        }
        let column = (x & 15) as usize * 16 + (z & 15) as usize;
        // 先只读算好四个新值，再写回，避开 &mut 与 &self 的借用冲突。
        // Compute the four new values read-only first, then write back.
        let mut updates = [(HeightmapKind::WorldSurface, 0_i32); 4];
        for (slot, kind) in FINAL_HEIGHTMAPS.iter().enumerate() {
            let current = heightmap_column(&self.heights[chunk], *kind)[column];
            updates[slot] = (
                *kind,
                heightmap_update(state, *kind, y, self.min_y, current, |probe_y| {
                    self.get_block(x, probe_y, z)
                }),
            );
        }
        for (kind, value) in updates {
            heightmap_column_mut(&mut self.heights[chunk], kind)[column] = value;
        }
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
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u32 {
        let Some(index) = self.index_of(quart_x >> 2, quart_z >> 2) else {
            return 0;
        };
        let sections = &self.biomes[index];
        let section = (quart_y - (self.min_y >> 2)) as usize;
        let local_x = ((quart_x & 3) * 16 + (quart_y & 3) * 4 + (quart_z & 3)) as usize;
        sections
            .get(section)
            .and_then(|cells| cells.get(local_x))
            .copied()
            .unwrap_or(0)
    }

    /// `BiomeManager.getBiome(x,y,z)` 的逐点平滑：由调用方通过回调给出群系 ID。
    ///
    /// `BiomeFilter` 需要这个，见 `feature::run`。
    pub fn is_air(&self, x: i32, y: i32, z: i32) -> bool {
        state_facts(self.get_block(x, y, z)) & facts::AIR != 0
    }
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

/// `ChunkStatus.FINAL_HEIGHTMAPS`：`setBlockState` 只更新这四种。
/// `heightmapsAfter` 由 `ProtoChunk.getPersistedStatus()` 给出，features 阶段是
/// `ChunkStatus.TERRAIN`，对应 `FINAL_HEIGHTMAPS`。
///
/// `ChunkStatus.FINAL_HEIGHTMAPS`: only these four are updated by `setBlockState`.
const FINAL_HEIGHTMAPS: [HeightmapKind; 4] = [
    HeightmapKind::WorldSurface,
    HeightmapKind::OceanFloor,
    HeightmapKind::MotionBlocking,
    HeightmapKind::MotionBlockingNoLeaves,
];

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

/// `Heightmap.Types` 的 `isOpaque` 谓词（26.3 字节码）。
///
/// - `NOT_AIR` = `!isAir()`（WORLD_SURFACE*）
/// - `MATERIAL_MOTION_BLOCKING` = `state.is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP)`
///   （OCEAN_FLOOR*）
/// - `MOTION_BLOCKING` = `state.is(BLOCKS_MOTION_IN_HEIGHTMAP) || !getFluidState().isEmpty()`
/// - `MOTION_BLOCKING_NO_LEAVES` = `state.is(BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES)
///   || !getFluidState().isEmpty()`
///
/// 导出的 `facts` 位分别对应这些判定；`state_fluid` 的 `fluid` 是流体注册表 ID，
/// `EMPTY`（0）表示没有流体。
///
/// `Heightmap.Types.isOpaque` predicates; the exported `facts` bits and
/// `state_fluid().fluid` (fluid registry ID, 0 = EMPTY) cover these tests.
pub fn is_heightmap_opaque(state: u32, kind: HeightmapKind) -> bool {
    let bits = state_facts(state);
    let has_fluid = state_fluid(state).fluid != registries::fluid::EMPTY;
    match kind {
        HeightmapKind::WorldSurfaceWg | HeightmapKind::WorldSurface => bits & facts::AIR == 0,
        HeightmapKind::OceanFloorWg | HeightmapKind::OceanFloor => {
            bits & facts::MOTION_BLOCKING != 0
        }
        HeightmapKind::MotionBlocking => bits & facts::MOTION_BLOCKING != 0 || has_fluid,
        HeightmapKind::MotionBlockingNoLeaves => {
            bits & facts::MOTION_BLOCKING_NO_LEAVES != 0 || has_fluid
        }
    }
}

/// 取某个高度图的一列（只读）。
/// Get one heightmap column (read-only).
fn heightmap_column(heights: &Heightmaps, kind: HeightmapKind) -> &[i32; 256] {
    match kind {
        HeightmapKind::WorldSurfaceWg => &heights.world_surface_wg,
        HeightmapKind::WorldSurface => &heights.world_surface,
        HeightmapKind::OceanFloorWg => &heights.ocean_floor_wg,
        HeightmapKind::OceanFloor => &heights.ocean_floor,
        HeightmapKind::MotionBlocking => &heights.motion_blocking,
        HeightmapKind::MotionBlockingNoLeaves => &heights.motion_blocking_no_leaves,
    }
}

/// 取某个高度图的一列（可变）。
/// Get one heightmap column (mutable).
fn heightmap_column_mut(heights: &mut Heightmaps, kind: HeightmapKind) -> &mut [i32; 256] {
    match kind {
        HeightmapKind::WorldSurfaceWg => &mut heights.world_surface_wg,
        HeightmapKind::WorldSurface => &mut heights.world_surface,
        HeightmapKind::OceanFloorWg => &mut heights.ocean_floor_wg,
        HeightmapKind::OceanFloor => &mut heights.ocean_floor,
        HeightmapKind::MotionBlocking => &mut heights.motion_blocking,
        HeightmapKind::MotionBlockingNoLeaves => &mut heights.motion_blocking_no_leaves,
    }
}

/// `Heightmap.update(localX, localY, localZ, state)`：返回该列新的 `getHeight` 值。
///
/// `getFirstAvailable = stored + 1`，`setHeight(h)` 把 data 设为 `h - minY`。
/// 26.3 字节码：先判 `localY <= firstAvailable - 2` 直接返回；新状态不透明且
/// `localY >= firstAvailable` 时抬到 `localY`；新状态透明且刚好削掉顶格时向下扫。
///
/// `Heightmap.update(...)`: returns the column's new `getHeight` value following
/// the 26.3 bytecode (raise above current first available, or scan down on removal).
fn heightmap_update(
    state: u32,
    kind: HeightmapKind,
    local_y: i32,
    min_y: i32,
    stored: i32,
    get_block: impl Fn(i32) -> u32,
) -> i32 {
    let first_available = stored + 1;
    if local_y <= first_available - 2 {
        return stored;
    }
    if is_heightmap_opaque(state, kind) {
        if local_y >= first_available {
            return local_y;
        }
    } else if first_available - 1 == local_y {
        for probe in (min_y..local_y).rev() {
            if is_heightmap_opaque(get_block(probe), kind) {
                return probe;
            }
        }
        return min_y - 1;
    }
    stored
}

/// 从 terrain 阶段的方块自顶向下扫描出 primed 高度图，逐类型用 `is_heightmap_opaque`。
/// Scan terrain blocks top-down to prime each heightmap with `is_heightmap_opaque`.
pub fn prime_heightmaps(blocks: &ChunkBlocks, min_y: i32, height: i32) -> Heightmaps {
    let _ = height;
    let mut result = Heightmaps::new(min_y);
    for column in 0..256 {
        for kind in [
            HeightmapKind::WorldSurfaceWg,
            HeightmapKind::WorldSurface,
            HeightmapKind::OceanFloorWg,
            HeightmapKind::OceanFloor,
            HeightmapKind::MotionBlocking,
            HeightmapKind::MotionBlockingNoLeaves,
        ] {
            for y in (min_y..min_y + blocks.len() as i32).rev() {
                let state = blocks[(y - min_y) as usize][column];
                if is_heightmap_opaque(state, kind) {
                    heightmap_column_mut(&mut result, kind)[column] = y;
                    break;
                }
            }
        }
    }
    result
}
