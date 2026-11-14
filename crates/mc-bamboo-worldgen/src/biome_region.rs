//! 真实服务端路径的群系来源：读区块里已经填好的 quart 群系格。
//!
//! 官方 `ChunkStatusTasks.buildTerrain` 建的 `WorldGenRegion` 既提供
//! `possibleBiomes`（周围 3×3 区块的调色板并集），又提供 `buildSurface` 用的
//! `getBiomeManager()`。后者的 `BiomeResolver` 就是 region 自己：
//! `LevelReader.getNoiseBiome` 先按 quart 坐标找到区块、读 `ChunkStatus.BIOMES` 成批
//! 填好的格子，读不到才回退 `getUncachedNoiseBiome()`。
//!
//! 成批 `sampleVolume` 和单点 `sampleValue` 的浮点顺序不同：气候参数落在 `ParameterList`
//! R 树最近邻的临界点上时，两条路径会选出不同群系。地表规则的材质（恶地的陶瓦条带、
//! 沙漠的砂岩）直接依赖群系，所以真实路径必须读格子，不能重算气候。
//!
//! 这里的格子和 [`crate::chunk::Generator::biomes`] 的输出同一种布局：每个 section
//! 64 格，下标 `x*16 + y*4 + z`。
//!
//! The real-server biome source: the quart biome cells stored in the chunks.
//!
//! Vanilla's `ChunkStatusTasks.buildTerrain` builds a `WorldGenRegion` that supplies
//! both `possibleBiomes` (the 3×3 palette union) and the `getBiomeManager()` used by
//! `buildSurface`. That manager's `BiomeResolver` is the region itself:
//! `LevelReader.getNoiseBiome` finds the chunk by quart coordinate, reads the cells
//! the `ChunkStatus.BIOMES` batch fill wrote, and only falls back to
//! `getUncachedNoiseBiome()` when no chunk is there.
//!
//! Batch `sampleVolume` and single-point `sampleValue` differ in floating-point order:
//! where the climate parameters sit on a `ParameterList` R-tree nearest-neighbour tie,
//! the two paths pick different biomes. Surface material (badlands terracotta bands,
//! desert sandstone) depends on the biome, so the real path must read the cells
//! instead of recomputing the climate.
//!
//! The cells use the same layout as [`crate::chunk::Generator::biomes`]: 64 per
//! section, indexed `x*16 + y*4 + z`.

use crate::{
    biome_manager::{BiomeManager, ClimateCache, fiddled_corner},
    math::java_floor_div,
    settings::Samplers,
};

/// 3×3 视图里的半径（单位：区块）。
const RADIUS: i32 = 1;
/// 3×3 视图的边长。
const SIDE: i32 = RADIUS * 2 + 1;
/// 中心区块在 3×3 视图里的下标。
pub const CENTER: usize = (SIDE * SIDE / 2) as usize;

/// 一个区块在 `ChunkStatus.BIOMES` 之后填好的 quart 群系格。
///
/// The quart biome cells one chunk has after `ChunkStatus.BIOMES`.
#[derive(Debug, Clone)]
pub struct ChunkBiomes {
    chunk_x: i32,
    chunk_z: i32,
    min_quart_y: i32,
    sections: Vec<Vec<u32>>,
}

impl ChunkBiomes {
    /// 用 `Generator::biomes` 的输出造一个；`min_y` 是维度的最低方块高度。
    ///
    /// Build one from `Generator::biomes`' output; `min_y` is the dimension's
    /// lowest block height.
    pub fn new(chunk_x: i32, chunk_z: i32, min_y: i32, sections: Vec<Vec<u32>>) -> Self {
        Self {
            chunk_x,
            chunk_z,
            min_quart_y: java_floor_div(min_y, 4),
            sections,
        }
    }

    pub fn pos(&self) -> (i32, i32) {
        (self.chunk_x, self.chunk_z)
    }

    /// 每个 section 的 64 个 quart 格，下标 `x*16 + y*4 + z`。
    pub fn sections(&self) -> &[Vec<u32>] {
        &self.sections
    }

    /// `ChunkAccess.getNoiseBiome`：quartY 夹到区块范围内再读格子。
    ///
    /// `ChunkAccess.getNoiseBiome`: clamp quartY into the chunk and read the cell.
    fn get(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u32 {
        let last_quart_y = self.min_quart_y + self.sections.len() as i32 * 4 - 1;
        let clamped = quart_y.clamp(self.min_quart_y, last_quart_y);
        let section = java_floor_div(clamped - self.min_quart_y, 4) as usize;
        let local_x = (quart_x & 3) as usize;
        let local_y = (clamped & 3) as usize;
        let local_z = (quart_z & 3) as usize;
        self.sections[section][local_x * 16 + local_y * 4 + local_z]
    }
}

/// 中心区块和它周围 3×3 的群系格，等价于官方 `WorldGenRegion` 的群系视图。
///
/// The centre chunk plus its 3×3 biome cells, matching vanilla's `WorldGenRegion`
/// biome view.
#[derive(Debug, Clone)]
pub struct RegionBiomes {
    zoom_seed: i64,
    /// 按下标 `(dz+1)*3 + (dx+1)` 存 3×3，中心在 [`CENTER`]。
    chunks: Vec<ChunkBiomes>,
}

impl RegionBiomes {
    /// 按「中心在前、邻居按 `dx` 外层 `dz` 内层」的顺序接收 9 个区块的格子。
    ///
    /// 顺序和 `ChunkgenOracle` 造 region 的顺序一致：中心先跑 `createBiomes`，
    /// 邻居按 `generateCarvers` 的邻域循环补齐。
    ///
    /// Take the nine chunks' cells in "centre first, then neighbours with `dx` outer
    /// and `dz` inner" order — the same order as the oracle builds its region.
    pub fn new(zoom_seed: i64, center: ChunkBiomes, neighbors: Vec<ChunkBiomes>) -> Self {
        let (center_x, center_z) = center.pos();
        let mut ordered: [Option<ChunkBiomes>; (SIDE * SIDE) as usize] =
            std::array::from_fn(|_| None);
        ordered[CENTER] = Some(center);
        for neighbor in neighbors {
            let (x, z) = neighbor.pos();
            let index = (z - center_z + RADIUS) * SIDE + (x - center_x + RADIUS);
            ordered[index as usize] = Some(neighbor);
        }
        // 九个格子必须齐全；缺一个是调用方的错，早报比读出别处的群系好。
        // All nine slots must be filled; a missing one is a caller bug, and failing
        // here beats silently reading another chunk's biome.
        let chunks = ordered
            .into_iter()
            .map(|slot| slot.expect("biome region needs all 3x3 chunks"))
            .collect();
        Self { zoom_seed, chunks }
    }

    /// 中心区块的格子。
    /// The centre chunk's cells.
    pub fn center(&self) -> &ChunkBiomes {
        &self.chunks[CENTER]
    }

    /// `LevelReader.getNoiseBiome(quartX, quartY, quartZ)` 里「找区块、读格子」那一段。
    ///
    /// 3×3 之外返回 `None`：官方此时回退 `getUncachedNoiseBiome()`，调用方要接着
    /// 走单点重算。
    ///
    /// The "find the chunk and read the cell" half of `LevelReader.getNoiseBiome`.
    /// Outside the 3×3 this returns `None`; vanilla then falls back to
    /// `getUncachedNoiseBiome()`, so the caller must recompute the climate.
    pub fn get(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<u32> {
        let chunk_x = java_floor_div(quart_x, 4);
        let chunk_z = java_floor_div(quart_z, 4);
        self.chunks
            .iter()
            .find(|chunk| chunk.chunk_x == chunk_x && chunk.chunk_z == chunk_z)
            .map(|chunk| chunk.get(quart_x, quart_y, quart_z))
    }

    /// `BiomeManager.getBiome` 在 region 上的结果：先做八角插值定位 quart 角，再读格子。
    ///
    /// `BiomeManager.getBiome` against the region: locate the quart corner with the
    /// octant interpolation, then read that cell.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        let corner = fiddled_corner(self.zoom_seed, x, y, z);
        self.get(corner.0, corner.1, corner.2)
    }

    /// `ChunkStatusTasks.collectPossibleBiomes(region, 1)`：3×3 区块调色板的并集。
    ///
    /// 官方 `ChunkAccess.collectBiomesInPalette` 收的是每个 section 调色板里的群系，
    /// 也就是这个区块里出现过的全部群系；这里对 64 格取并集等价。
    ///
    /// `ChunkStatusTasks.collectPossibleBiomes(region, 1)`: the union of the 3×3
    /// chunks' palettes. Vanilla's `ChunkAccess.collectBiomesInPalette` collects the
    /// biomes present in each section's palette, i.e. every biome that appears in the
    /// chunk; a union over the 64 cells is equivalent.
    pub fn possible_biomes(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for chunk in &self.chunks {
            for section in &chunk.sections {
                for &id in section {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
        }
        out
    }
}

/// 地表规则问群系的两种来源。
///
/// 简化路径（`chunkgen`、`beard` 对拍）用 [`Self::Uncached`]，在空的上下文上单点重算；
/// 真实服务端路径用 [`Self::Region`]，先读区块里存的 quart 格。
///
/// The two biome sources surface rules can ask. The simplified path (`chunkgen`,
/// `beard`) uses [`Self::Uncached`] and recomputes per point on an empty context; the
/// real-server path uses [`Self::Region`] and reads the stored quart cells first.
pub enum BiomeGetter<'a> {
    /// 单点气候采样，和官方 `createUncachedResolver` 一样。
    /// Single-point climate sampling, like vanilla's `createUncachedResolver`.
    Uncached(&'a BiomeManager<'a>),
    /// 先读区块里的 quart 格，读不到再单点重算。
    /// Read the stored quart cells, falling back to single-point sampling.
    Region {
        manager: &'a BiomeManager<'a>,
        region: &'a RegionBiomes,
    },
}

impl BiomeGetter<'_> {
    /// `BiomeManager.getBiome(x, y, z)`：八角插值后取群系注册表 ID。
    ///
    /// `BiomeManager.getBiome(x, y, z)`: octant interpolation, then the biome id.
    pub fn biome_cached(
        &self,
        samplers: &Samplers,
        context: &mut crate::sampler::Context,
        x: i32,
        y: i32,
        z: i32,
        cache: Option<&mut ClimateCache>,
    ) -> u32 {
        match self {
            Self::Uncached(manager) => manager.biome_cached(samplers, context, x, y, z, cache),
            Self::Region { manager, region } => match region.biome_at(x, y, z) {
                Some(id) => id,
                // 3×3 之外：`WorldGenRegion.getUncachedNoiseBiome` 回到单点重算。
                // Outside the 3×3: `WorldGenRegion.getUncachedNoiseBiome` recomputes.
                None => {
                    let corner = fiddled_corner(region.zoom_seed, x, y, z);
                    manager.noise_biome(samplers, context, corner.0, corner.1, corner.2)
                }
            },
        }
    }
}
