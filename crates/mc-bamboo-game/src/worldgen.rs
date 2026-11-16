//! 把一个主世界区块生成到 `terrain`（CARVERS 跑完）的纯计算入口。
//!
//! 游戏里的 [`crate::chunk_generator::OverworldGenerator`] 和 `mc-bamboo-vanilla` 的
//! `gen-world` 对拍都调用这里的 [`generate_chunk`]，保证两边比的是同一份代码。
//! 官方 26.3 的 `terrain` 状态一步跑 `doFill → buildSurface → generateCarvers`；
//! 这里照调 `mc_bamboo_worldgen::chunk::Generator`，到 CARVERS 为止（地物、结构、
//! 光照是后面的阶段，本轮先不接）。
//!
//! Pure entry point that generates one overworld chunk up to `terrain` (after
//! CARVERS). Both the in-game [`crate::chunk_generator::OverworldGenerator`] and
//! the `gen-world` oracle call [`generate_chunk`] here, so they exercise the
//! same code. Vanilla 26.3's `terrain` status runs `doFill → buildSurface →
//! generateCarvers` in one step; we call the matching
//! `mc_bamboo_worldgen::chunk::Generator`, stopping at CARVERS (features,
//! structures and light are later stages, not wired this round).

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
};

use mc_bamboo_data::{blocks, registries};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    chunk::HeightmapKind,
    column::{ChunkColumn, ChunkColumnParts, SectionParts},
    palette::PalettedContainer,
};
use mc_bamboo_worldgen::{Worldgen, beardifier::Beardifier, chunk};

use crate::{
    region::{RegionColumn, RegionColumnParts},
    world_options::WorldGenConfig,
};

/// 一个区块 16×16 个柱子，和 `chunk::CHUNK_WIDTH` 一致。
const WIDTH: usize = 16;
/// 一段的 quart 群系格数。
const SECTION_BIOMES: usize = 64;

/// 官方 `ChunkStatus` 各阶段 prime/更新的六张高度图的序列化名。
///
/// `WORLDGEN_HEIGHTMAPS`（`OCEAN_FLOOR_WG`、`WORLD_SURFACE_WG`）是 EMPTY..BIOMES
/// 之后的两张，`FINAL_HEIGHTMAPS`（其余四张）是 TERRAIN 及之后 prime 的四张。
///
/// The six heightmap serialization names the `ChunkStatus` stages prime/update.
/// `WORLDGEN_HEIGHTMAPS` are the two after EMPTY..BIOMES; `FINAL_HEIGHTMAPS` are
/// the four primed at TERRAIN and later.
pub const HEIGHTMAP_KEYS: [&str; 6] = [
    "WORLD_SURFACE_WG",
    "WORLD_SURFACE",
    "OCEAN_FLOOR_WG",
    "OCEAN_FLOOR",
    "MOTION_BLOCKING",
    "MOTION_BLOCKING_NO_LEAVES",
];

/// 一个区块到 `terrain` 阶段的样子。
///
/// `blocks` 是 `WORLD_HEIGHT` 层 × 256（`x*16+z`）个默认状态 ID，y 从 `MIN_Y` 往上；
/// `biomes` 是 `SECTION_COUNT` 段 × 64 个 quart 格，顺序 `x → y → z`（和
/// `Generator::biomes` 的输出一致）；`heightmaps` 是 [`HEIGHTMAP_KEYS`] 里每张图的
/// 256 列世界高度（`Heightmap.getFirstAvailable`，即最高地面的上面一格），下标 `x+z*16`。
///
/// A chunk at `terrain`: `blocks` is `WORLD_HEIGHT` layers × 256 (`x*16+z`)
/// default state ids, y from `MIN_Y` up; `biomes` is `SECTION_COUNT` sections ×
/// 64 quart cells in `x → y → z` order; `heightmaps` holds, for each
/// [`HEIGHTMAP_KEYS`] type, the 256 per-column world heights
/// (`Heightmap.getFirstAvailable`), indexed `x+z*16`.
#[derive(Debug, Clone)]
pub struct GeneratedChunk {
    pub blocks: Vec<Vec<u32>>,
    pub biomes: Vec<Vec<u32>>,
    pub heightmaps: BTreeMap<&'static str, Vec<i32>>,
}

// 每条计算线程各缓存一份解析好的 `Worldgen` 和按种子/设置建好的 `Generator`。
// `chunk::Generator` 里的 `ParameterList` 用 `Cell` 记 R 树的 `lastResult`
// （官方是 `ThreadLocal`），一个实例不能跨线程共享；按线程各存一份，和官方每线程
// 一个 `RandomState` 的模型一致。`Worldgen` 用 `Box::leak` 变成 `'static` 引用。
// Each compute thread caches one parsed `Worldgen` and per-seed `Generator`s.
// `chunk::Generator`'s `ParameterList` keeps the R-tree `lastResult` in a `Cell`
// (vanilla: `ThreadLocal`), so one instance cannot be shared across threads;
// per-thread caches match vanilla's per-thread `RandomState`. `Worldgen` is
// leaked to a `'static` reference.
thread_local! {
    static GENERATORS: RefCell<HashMap<(i64, &'static str), chunk::Generator<'static>>> =
        RefCell::new(HashMap::new());
}

/// 在按 `config` 建好的 `Generator` 上跑一段计算。
///
/// `settings_id` 由世界类型决定；三种噪声世界共用 `minecraft:overworld` 群系预设
/// （`WorldPresets.bootstrap`），只有 `NoiseGeneratorSettings` 不同。
///
/// Run a computation on the `Generator` built for `config`. `settings_id` comes
/// from the level type; all three noise worlds share the `minecraft:overworld`
/// biome preset (`WorldPresets.bootstrap`) and differ only in
/// `NoiseGeneratorSettings`.
pub(crate) fn with_generator<T>(
    config: &WorldGenConfig,
    f: impl FnOnce(&chunk::Generator<'static>) -> Result<T, String>,
) -> Result<T, String> {
    let settings_id = config
        .level_type
        .settings_id()
        .ok_or_else(|| "flat worlds have no noise generator".to_owned())?;
    let key = (config.seed, settings_id);
    GENERATORS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(key) {
            // 解析官方数据一次就泄漏成 'static：进程里只有一份，之后一直复用。
            // Parse the official data once and leak it to 'static; reused thereafter.
            let worldgen: &'static Worldgen =
                Box::leak(Box::new(Worldgen::load().map_err(|e| e.to_string())?));
            let generator = chunk::Generator::with_settings(
                worldgen,
                settings_id,
                "minecraft:overworld",
                config.seed,
            )?;
            entry.insert(generator);
        }
        let generator = cache.get(&key).expect("generator was just inserted");
        // 每次进入生成纯函数都清掉群系参数表的 R 树 `lastResult`：结构阶段的
        // 单点群系查询会改写它，不清的话 1/4 线程的区块到达顺序会让 quart
        // 群系平局落到不同条目，最终方块不同。官方一条线程一个 `RandomState`，
        // 这里按线程泄漏的 `Worldgen` 需要显式隔离。
        // Clear the biome parameter list's R-tree `lastResult` on every entry:
        // structure-stage single-point queries mutate it, and without this reset
        // chunk arrival order changes batch quart-biome tie-breaks.
        generator.reset_biome_state();
        f(generator)
    })
}

/// 官方 `Heightmap.Types.isOpaque()` 对某个方块状态的判定。
///
/// Vanilla `Heightmap.Types.isOpaque()` for one block state.
fn heightmap_predicate(key: &str, state: u32) -> bool {
    let facts = mc_bamboo_data::state_facts(state);
    let motion = facts & mc_bamboo_data::facts::MOTION_BLOCKING != 0;
    let motion_no_leaves = facts & mc_bamboo_data::facts::MOTION_BLOCKING_NO_LEAVES != 0;
    let fluid_nonempty =
        mc_bamboo_data::state_fluid(state).fluid != mc_bamboo_data::registries::fluid::EMPTY;
    match key {
        // `NOT_AIR`：air / cave_air / void_air 都不算地面。
        // `NOT_AIR`: air, cave_air and void_air are all transparent to this map.
        "WORLD_SURFACE_WG" | "WORLD_SURFACE" => facts & mc_bamboo_data::facts::AIR == 0,
        // `MATERIAL_MOTION_BLOCKING`：不含流体。 `MATERIAL_MOTION_BLOCKING`: no fluids.
        "OCEAN_FLOOR_WG" | "OCEAN_FLOOR" => motion,
        "MOTION_BLOCKING" => motion || fluid_nonempty,
        "MOTION_BLOCKING_NO_LEAVES" => motion_no_leaves || fluid_nonempty,
        _ => false,
    }
}

/// 照 `Heightmap.primeHeightmaps`：每列从顶往下找第一个满足各自谓词的方块，记 `y+1`；
/// 整列没有就留在 `MIN_Y`。六张图都从最终 CARVERS 方块 prime（官方 doFill 的两张 WG
/// 经 setBlockState 增量维护到 CARVERS，最终值与从最终方块 prime 相同）。
///
/// Mirrors `Heightmap.primeHeightmaps`: per column, top-down, find the first block
/// satisfying each type's predicate and record `y+1`; an empty column stays at
/// `MIN_Y`. All six maps are primed from the final CARVERS blocks (vanilla's two
/// doFill WG maps are maintained by setBlockState up to CARVERS, so their final
/// values equal a prime over the final blocks).
fn compute_heightmaps(blocks: &[Vec<u32>]) -> BTreeMap<&'static str, Vec<i32>> {
    let mut maps: BTreeMap<&'static str, Vec<i32>> = BTreeMap::new();
    for &key in &HEIGHTMAP_KEYS {
        maps.insert(key, vec![MIN_Y; WIDTH * WIDTH]);
    }
    let height = blocks.len() as i32;
    for x in 0..WIDTH {
        for z in 0..WIDTH {
            // 方块层是 `x*16+z`（GeneratedChunk.blocks 的布局），高度图是官方的
            // `Heightmap.getIndex(x,z) = x + z*16`，两者下标顺序不同。
            // The block layers use `x*16+z` (GeneratedChunk.blocks layout) while
            // heightmaps use vanilla's `Heightmap.getIndex(x,z) = x + z*16`.
            let block_index = x * WIDTH + z;
            let map_index = x + z * WIDTH;
            let mut pending: Vec<&'static str> = HEIGHTMAP_KEYS.to_vec();
            for local_y in (0..height).rev() {
                if pending.is_empty() {
                    break;
                }
                let state = blocks[local_y as usize][block_index];
                let mut kept = Vec::with_capacity(pending.len());
                for key in pending {
                    if heightmap_predicate(key, state) {
                        if let Some(map) = maps.get_mut(key) {
                            map[map_index] = MIN_Y + local_y + 1;
                        }
                    } else {
                        kept.push(key);
                    }
                }
                pending = kept;
            }
        }
    }
    maps
}

/// 超平坦：按预设的层从 `MIN_Y` 往上填方块，整个区块一个群系。
///
/// Flat: fill blocks from `MIN_Y` using the preset layers; one biome everywhere.
fn flat_chunk(config: &WorldGenConfig) -> GeneratedChunk {
    let mut blocks = vec![vec![blocks::AIR; WIDTH * WIDTH]; WORLD_HEIGHT as usize];
    let mut y = MIN_Y;
    for &(state, thickness) in &config.flat.layers {
        for _ in 0..thickness {
            if (0..WORLD_HEIGHT).contains(&(y - MIN_Y)) {
                blocks[(y - MIN_Y) as usize].fill(state);
            }
            y += 1;
        }
    }
    let cells = vec![config.flat.biome; SECTION_BIOMES];
    let heightmaps = compute_heightmaps(&blocks);
    GeneratedChunk {
        blocks,
        biomes: vec![cells; SECTION_COUNT],
        heightmaps,
    }
}

// 每条计算线程各缓存一份到 CARVERS 的地形，供 `features` 阶段拼 3×3 视图时免于
// 反复重算。键含种子和世界类型；地形是纯函数，命中与否不改变结果，只省时间。
// 只在 `features` 阶段用，条目数封顶，超过就整表清掉（不逐条淘汰、不依赖顺序）。
// Each compute thread caches CARVERS terrain so the `features` 3×3 view does not
// recompute it. The key includes seed and world type; terrain is pure, so a hit
// never changes the result. Bounded: clear the whole table past the cap.
type TerrainCache = HashMap<(i64, Option<&'static str>, i32, i32), GeneratedChunk>;
thread_local! {
    static TERRAIN_CACHE: RefCell<TerrainCache> = RefCell::new(HashMap::new());
}
const TERRAIN_CACHE_CAP: usize = 1024;

/// 带线程本地缓存的 `generate_chunk`，`features` 阶段拼 3×3 视图用。
///
/// Cached `generate_chunk` for building the `features` 3×3 view.
pub(crate) fn cached_generate_chunk(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
) -> Result<GeneratedChunk, String> {
    cached_generate_chunk_with_beardifier(config, x, z, None)
}

/// 带 Beardifier 的地形：非空密度只在有结构的区块上出现，不进 EMPTY 的缓存。
///
/// Terrain with a Beardifier: the non-EMPTY result only occurs on chunks with
/// structures and is never cached under the EMPTY key.
pub(crate) fn cached_generate_chunk_with_beardifier(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
    beardifier: Option<&Beardifier>,
) -> Result<GeneratedChunk, String> {
    if beardifier.is_some() {
        return generate_chunk_with_beardifier(config, x, z, beardifier);
    }
    let key = (config.seed, config.level_type.settings_id(), x, z);
    TERRAIN_CACHE.with(|cache| {
        if let Some(chunk) = cache.borrow().get(&key) {
            return Ok(chunk.clone());
        }
        let chunk = generate_chunk(config, x, z)?;
        let mut cache = cache.borrow_mut();
        if cache.len() >= TERRAIN_CACHE_CAP {
            cache.clear();
        }
        cache.insert(key, chunk.clone());
        Ok(chunk)
    })
}

/// 生成中心区块到 `terrain` 阶段。
///
/// 纯函数：只依赖 `config`（种子、世界类型、flat 预设）和坐标。噪声世界用
/// `Generator::biomes` + `Generator::terrain(Carvers)`；flat 用官方
/// `FlatLevelSource` 的层和群系。
///
/// Generate the center chunk up to `terrain`. Pure: depends only on `config`
/// (seed, level type, flat preset) and coordinates. Noise worlds use
/// `Generator::biomes` + `Generator::terrain(Carvers)`; flat uses vanilla
/// `FlatLevelSource` layers and biome.
pub fn generate_chunk(config: &WorldGenConfig, x: i32, z: i32) -> Result<GeneratedChunk, String> {
    generate_chunk_with_beardifier(config, x, z, None)
}

/// [`generate_chunk`] 加一个本区块的 `Beardifier`。官方 `createNoiseChunk` 用
/// `Beardifier.forStructuresInChunk(structureManager, chunkPos)`；没有结构时传 `None`
/// 等于官方 `Beardifier.EMPTY`。`first_occupied_height` 等基线高度查询固定 EMPTY。
///
/// `generate_chunk` with the chunk's Beardifier. Vanilla's `createNoiseChunk` uses
/// `Beardifier.forStructuresInChunk`; `None` equals `Beardifier.EMPTY`. Baseline
/// height queries stay on EMPTY.
pub fn generate_chunk_with_beardifier(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
    beardifier: Option<&Beardifier>,
) -> Result<GeneratedChunk, String> {
    if config.level_type.is_flat() {
        return Ok(flat_chunk(config));
    }
    with_generator(config, |generator| {
        let region = generator.biome_region(x, z)?;
        let blocks =
            generator.terrain_with_region(x, z, chunk::Stage::Carvers, beardifier, &region)?;
        let biomes = region.center().sections().to_vec();
        let heightmaps = compute_heightmaps(&blocks);
        Ok(GeneratedChunk {
            blocks,
            biomes,
            heightmaps,
        })
    })
}

/// 只算 `BIOMES` 阶段（供流水线里 `biomes` 这一步用），方块全是空气。
///
/// Compute only the `BIOMES` stage (for the `biomes` pipeline step); blocks are air.
pub fn generate_biomes(config: &WorldGenConfig, x: i32, z: i32) -> Result<GeneratedChunk, String> {
    if config.level_type.is_flat() {
        return Ok(flat_chunk(config));
    }
    with_generator(config, |generator| {
        let biomes = generator.biomes(x, z)?;
        let blocks = vec![vec![blocks::AIR; WIDTH * WIDTH]; WORLD_HEIGHT as usize];
        let heightmaps = compute_heightmaps(&blocks);
        Ok(GeneratedChunk {
            blocks,
            biomes,
            heightmaps,
        })
    })
}

/// 把生成的方块和群系组装成一个可交付的 [`RegionColumn`]。
///
/// 方块和群系按官方 `PalettedContainer` 的下标顺序写进 24 个 `SectionParts`，
/// 高度图给 `None` 让 `Chunk::from_sections` 从方块重算（等价于
/// `Heightmap.primeHeightmaps`）；光照层留空、`isLightOn=false`，由运行时光照
/// 引擎在交付后算（设计稿 5.6，见 README 已知限制）。
///
/// Assemble generated blocks/biomes into a deliverable [`RegionColumn`]. Blocks
/// and biomes are written into 24 `SectionParts` using vanilla's
/// `PalettedContainer` index order; heightmaps are `None` so
/// `Chunk::from_sections` recomputes them from the blocks (equivalent to
/// `Heightmap.primeHeightmaps`); light layers are empty with `isLightOn=false`,
/// settled by the runtime light engine after delivery (design 5.6, see README
/// known limitations).
pub fn build_region_column(chunk: &GeneratedChunk) -> Result<Box<RegionColumn>, String> {
    if chunk.blocks.len() != WORLD_HEIGHT as usize || chunk.biomes.len() != SECTION_COUNT {
        return Err("generated chunk has the wrong number of layers or sections".to_owned());
    }
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for section in 0..SECTION_COUNT {
        let mut block_container = PalettedContainer::blocks(blocks::AIR);
        let mut biome_container = PalettedContainer::biomes(config_default_biome());
        for local_y in 0..SECTION_SIZE as usize {
            let layer = &chunk.blocks[section * SECTION_SIZE as usize + local_y];
            for x in 0..WIDTH {
                for z in 0..WIDTH {
                    let index = (local_y << 8) | (z << 4) | x;
                    block_container.set(index, layer[x * WIDTH + z]);
                }
            }
        }
        // `Strategy.getIndex(x,y,z) = y*16 + z*4 + x`（biomes 的 bitsPerAxis=2）。
        // `Generator::biomes` 的 cells 是 `x → y → z` 压入，下标 `qx*16 + qy*4 + qz`。
        // `Strategy.getIndex(x,y,z) = y*16 + z*4 + x` for biomes;
        // `Generator::biomes` pushes cells in `x → y → z` order.
        let cells = &chunk.biomes[section];
        for qx in 0..4 {
            for qy in 0..4 {
                for qz in 0..4 {
                    let value = cells[qx * 16 + qy * 4 + qz];
                    biome_container.set(qy * 16 + qz * 4 + qx, value);
                }
            }
        }
        let (block_palette, block_data) = block_container.packed_for_disk();
        let (biome_palette, biome_data) = biome_container.packed_for_disk();
        sections.push(SectionParts {
            block_palette,
            block_data,
            biome_palette,
            biome_data,
        });
    }
    // 高度图 None -> 从方块重算；光照层 None、isLightOn=false -> 运行时补算。
    // Heightmaps None -> recomputed from blocks; light None/off -> settled at runtime.
    let parts = ChunkColumnParts {
        sections,
        ..ChunkColumnParts::default()
    };
    let column = ChunkColumn::from_parts(parts)
        .map_err(|error| format!("cannot assemble generated column: {error}"))?;
    Ok(RegionColumn::from_parts(RegionColumnParts {
        column: Box::new(column),
        ticks: None,
        block_entities: Default::default(),
        redstone: Default::default(),
        containers: Default::default(),
        block_items: Default::default(),
        command_blocks: Default::default(),
        books: Default::default(),
        entities: Default::default(),
    }))
}

/// 交付/存档会写出的四张 `FINAL_HEIGHTMAPS` 的打包位流（`LongArrayTag` 的内容）。
///
/// 用 [`build_region_column`] 组装后再读，和游戏写盘走的是同一条
/// `Chunk::from_sections` 重算路径，所以对拍比较的正是存档里真正写出的值。
///
/// The packed `LongArrayTag` words of the four `FINAL_HEIGHTMAPS` that delivery/save
/// writes. Assembles via [`build_region_column`] first, so the comparison covers the
/// very values the game persists through `Chunk::from_sections`.
pub fn final_heightmap_words(
    chunk: &GeneratedChunk,
) -> Result<BTreeMap<&'static str, Vec<u64>>, String> {
    let column = build_region_column(chunk)?;
    let world = column.parts().column.chunk();
    let mut out = BTreeMap::new();
    for (key, kind) in [
        ("WORLD_SURFACE", HeightmapKind::WorldSurface),
        ("OCEAN_FLOOR", HeightmapKind::OceanFloor),
        ("MOTION_BLOCKING", HeightmapKind::MotionBlocking),
        (
            "MOTION_BLOCKING_NO_LEAVES",
            HeightmapKind::MotionBlockingNoLeaves,
        ),
    ] {
        out.insert(key, world.heightmap(kind));
    }
    Ok(out)
}

/// 建 biome 容器时用的初始群系；`set` 会立刻覆盖全部 64 格，这个值不进结果。
/// Initial biome for the container; `set` overwrites all 64 cells so it never
/// appears in the result. Kept as a concrete valid id.
fn config_default_biome() -> u32 {
    registries::biome::PLAINS as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::LevelType;

    fn normal(seed: i64) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        }
    }

    #[test]
    fn flat_chunk_matches_the_official_default_preset() {
        let cfg = WorldGenConfig::default();
        let chunk = flat_chunk(&cfg);
        assert_eq!(chunk.blocks.len(), WORLD_HEIGHT as usize);
        assert_eq!(
            chunk.blocks[0]
                .iter()
                .filter(|&&s| s == blocks::BEDROCK)
                .count(),
            256
        );
        assert_eq!(
            chunk.blocks[1]
                .iter()
                .filter(|&&s| s == blocks::DIRT)
                .count(),
            256
        );
        assert_eq!(
            chunk.blocks[2]
                .iter()
                .filter(|&&s| s == blocks::DIRT)
                .count(),
            256
        );
        assert_eq!(
            chunk.blocks[3]
                .iter()
                .filter(|&&s| s == blocks::GRASS_BLOCK)
                .count(),
            256
        );
        assert_eq!(chunk.blocks[4][0], blocks::AIR);
        for section in &chunk.biomes {
            assert_eq!(section.len(), 64);
            assert!(
                section
                    .iter()
                    .all(|&b| b == registries::biome::PLAINS as u32)
            );
        }
    }

    #[test]
    fn flat_assembles_into_a_readable_column() {
        let cfg = WorldGenConfig::default();
        let chunk = generate_chunk(&cfg, 3, -2).unwrap();
        let column = build_region_column(&chunk).unwrap();
        let world = column.parts().column.chunk();
        assert_eq!(world.block(0, MIN_Y, 0), blocks::BEDROCK);
        assert_eq!(world.block(5, -62, 5), blocks::DIRT);
        assert_eq!(world.block(15, -61, 15), blocks::GRASS_BLOCK);
        assert_eq!(world.block(0, -60, 0), blocks::AIR);
        // 群系经 `biome_at` 读回还是 plains（下标映射正确）。
        // Biomes read back through `biome_at` are still plains (index mapping correct).
        let quart = world.biome_at(0, MIN_Y, 0);
        assert_eq!(quart, registries::biome::PLAINS as u32);
    }

    /// 回归：种子 0、区块 (644792,109695) 是恶地和沙漠的交界。官方 `buildTerrain`
    /// 的地表阶段读相邻区块里 `BIOMES` 阶段成批填好的 quart 群系格；如果改回单点
    /// 气候采样，这一列的气候参数落在 R 树最近邻的临界点上，会选到砂岩而不是陶瓦，
    /// 出现 33 处不一致（`gen-world --seeds 1 --chunk 644792 109695`）。
    ///
    /// 断言用官方给出的几个已知格子：y 66–74 的 x 12–15、z 8–12 是砂岩或沙子，
    /// 不是各色陶瓦。这样即使生成器整体退化也能定位到「真实路径没生效」。
    ///
    /// Regression: seed 0 chunk (644792,109695) sits on a badlands/desert boundary.
    /// Vanilla's `buildTerrain` surface pass reads the quart biome cells the `BIOMES`
    /// stage wrote into the neighbouring chunks. Sampling the climate per point
    /// instead lands on an R-tree nearest-neighbour tie in this column and picks
    /// sandstone instead of terracotta, giving 33 mismatches in
    /// `gen-world --seeds 1 --chunk 644792 109695`.
    #[test]
    fn badlands_desert_boundary_uses_stored_quart_biomes() {
        let cfg = normal(0);
        let chunk = generate_chunk(&cfg, 644_792, 109_695).unwrap();
        let sandstone = mc_bamboo_data::block_by_name("minecraft:sandstone")
            .expect("有砂岩")
            .default_state;
        let sand = mc_bamboo_data::block_by_name("minecraft:sand")
            .expect("有沙子")
            .default_state;
        // 这几格在修复前是各色陶瓦（13211/13212/13215/13219/13223/14759），
        // 官方是砂岩或沙子。只断言「不是陶瓦、是砂岩或沙子」。
        let terracotta: Vec<u32> = [
            "minecraft:terracotta",
            "minecraft:orange_terracotta",
            "minecraft:yellow_terracotta",
            "minecraft:brown_terracotta",
            "minecraft:white_terracotta",
            "minecraft:light_gray_terracotta",
            "minecraft:red_terracotta",
        ]
        .iter()
        .filter_map(|name| mc_bamboo_data::block_by_name(name).map(|b| b.default_state))
        .collect();
        for (y, x, z) in [
            (66, 12, 12),
            (66, 13, 12),
            (67, 13, 12),
            (68, 13, 10),
            (69, 14, 11),
            (72, 13, 8),
            (73, 13, 9),
            (74, 13, 10),
            (75, 13, 8),
            (77, 15, 11),
            (79, 15, 11),
        ] {
            let state = chunk.blocks[(y - MIN_Y) as usize][x * WIDTH + z];
            assert!(
                state == sandstone || state == sand,
                "({x},{y},{z}) should be sandstone or sand, got state {state}"
            );
            assert!(
                !terracotta.contains(&state),
                "({x},{y},{z}) must not be terracotta"
            );
        }
    }

    #[test]
    fn normal_generation_is_deterministic_and_nonempty() {
        let cfg = normal(12345);
        let a = generate_chunk(&cfg, 5, 7).unwrap();
        let b = generate_chunk(&cfg, 5, 7).unwrap();
        assert_eq!(a.blocks, b.blocks);
        assert_eq!(a.biomes, b.biomes);
        // 地表附近有非空气方块，且群系不止一个值（有噪声地形）。
        // Non-air blocks near the surface and more than one biome (real terrain).
        assert!(a.blocks.iter().flatten().any(|&s| s != blocks::AIR));
        let distinct: std::collections::HashSet<u32> = a.biomes.iter().flatten().copied().collect();
        assert!(
            distinct.len() > 1,
            "normal world should have several biomes"
        );
        let column = build_region_column(&a).unwrap();
        assert!(!column.parts().column.chunk().sections().is_empty());
    }

    #[test]
    fn large_biomes_and_amplified_are_supported() {
        for level in [LevelType::LargeBiomes, LevelType::Amplified] {
            let cfg = WorldGenConfig {
                seed: 42,
                level_type: level,
                ..WorldGenConfig::default()
            };
            let chunk = generate_chunk(&cfg, 0, 0).unwrap();
            assert!(chunk.blocks.iter().flatten().any(|&s| s != blocks::AIR));
        }
    }

    #[test]
    fn section_palettes_round_trip_the_block_states() {
        // 造一个三种方块交错的层，确认下标映射 `y*256 + z*16 + x` 没写反。
        // Interleave three block states to confirm the `y*256 + z*16 + x` mapping.
        let cfg = WorldGenConfig::default();
        let mut chunk = flat_chunk(&cfg);
        for x in 0..WIDTH {
            for z in 0..WIDTH {
                chunk.blocks[10][x * WIDTH + z] = if (x + z) % 3 == 0 {
                    blocks::STONE
                } else if (x + z) % 3 == 1 {
                    blocks::DIRT
                } else {
                    blocks::AIR
                };
            }
        }
        let column = build_region_column(&chunk).unwrap();
        let world = column.parts().column.chunk();
        for x in 0..WIDTH {
            for z in 0..WIDTH {
                let expected = if (x + z) % 3 == 0 {
                    blocks::STONE
                } else if (x + z) % 3 == 1 {
                    blocks::DIRT
                } else {
                    blocks::AIR
                };
                assert_eq!(world.block(x, MIN_Y + 10, z), expected, "at ({x}, 10, {z})");
            }
        }
    }
}
