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

use std::{cell::RefCell, collections::HashMap};

use mc_bamboo_data::{blocks, registries};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    column::{ChunkColumn, ChunkColumnParts, SectionParts},
    palette::PalettedContainer,
};
use mc_bamboo_worldgen::{Worldgen, chunk};

use crate::{
    region::{RegionColumn, RegionColumnParts},
    world_options::WorldGenConfig,
};

/// 一个区块 16×16 个柱子，和 `chunk::CHUNK_WIDTH` 一致。
const WIDTH: usize = 16;
/// 一段的 quart 群系格数。
const SECTION_BIOMES: usize = 64;

/// 一个区块到 `terrain` 阶段的样子。
///
/// `blocks` 是 `WORLD_HEIGHT` 层 × 256（`x*16+z`）个默认状态 ID，y 从 `MIN_Y` 往上；
/// `biomes` 是 `SECTION_COUNT` 段 × 64 个 quart 格，顺序 `x → y → z`（和
/// `Generator::biomes` 的输出一致）。
///
/// A chunk at `terrain`: `blocks` is `WORLD_HEIGHT` layers × 256 (`x*16+z`)
/// default state ids, y from `MIN_Y` up; `biomes` is `SECTION_COUNT` sections ×
/// 64 quart cells in `x → y → z` order (same as `Generator::biomes` output).
#[derive(Debug, Clone)]
pub struct GeneratedChunk {
    pub blocks: Vec<Vec<u32>>,
    pub biomes: Vec<Vec<u32>>,
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
fn with_generator<T>(
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
        f(generator)
    })
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
    GeneratedChunk {
        blocks,
        biomes: vec![cells; SECTION_COUNT],
    }
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
    if config.level_type.is_flat() {
        return Ok(flat_chunk(config));
    }
    with_generator(config, |generator| {
        let biomes = generator.biomes(x, z)?;
        let blocks = generator.terrain(x, z, chunk::Stage::Carvers)?;
        Ok(GeneratedChunk { blocks, biomes })
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
        Ok(GeneratedChunk {
            blocks: vec![vec![blocks::AIR; WIDTH * WIDTH]; WORLD_HEIGHT as usize],
            biomes,
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
