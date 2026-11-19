//! 大型滴水石的选点探针：找出确实生成了滴水石柱的区块。
//!
//! 这个地物只在滴水石洞穴群系里、且要撞上空腔高度 >= 4 的位置；该群系在多噪声分布里
//! 很稀疏，单个种子附近常常一个都不出现。这里扫一片网格，把中心区块写出了滴水石块的
//! 坐标打出来，供 `chunkgen --chunk X Z` 逐个复核。
//!
//! A large-dripstone spot probe: find chunks that really generate a dripstone column.
//! The feature only fires in dripstone caves and needs a cavity at least 4 high, and that
//! biome is sparse in the multi-noise distribution, so a single seed's neighborhood often
//! has none. This scans a grid and prints the chunks whose center wrote dripstone blocks,
//! so they can be rechecked with `chunkgen --chunk X Z`.

use crate::{
    Worldgen,
    chunk::{Generator, Stage},
    feature::{FeatureChunks, world::prime_heightmaps},
};

/// `GenerationStep.Decoration.LOCAL_MODIFICATIONS` 的位掩码。
///
/// The bit mask for `GenerationStep.Decoration.LOCAL_MODIFICATIONS`.
const LOCAL_MODIFICATIONS: i32 = 1 << 2;

#[test]
#[ignore]
fn list_large_dripstones() {
    let seed: i64 = std::env::var("DRIP_SEED")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let radius: i32 = std::env::var("DRIP_RADIUS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(8);
    let center_x: i32 = std::env::var("DRIP_X")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let center_z: i32 = std::env::var("DRIP_Z")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let worldgen = Worldgen::load().expect("worldgen");
    let generator = Generator::new(&worldgen, seed).expect("generator");
    let allowed = vec!["minecraft:large_dripstone".to_owned()];
    let dripstone = mc_bamboo_data::block_by_name("minecraft:dripstone_block")
        .and_then(|block| mc_bamboo_data::block_id_of_state(block.first_state));
    let registry = generator.feature_registry();
    let dripstone_biomes: Vec<u32> = (0..128u32)
        .filter(|&id| {
            registry
                .biome_features(id, 2)
                .iter()
                .any(|name| name == "minecraft:large_dripstone")
        })
        .collect();
    let mut found = 0;
    let mut biome_chunks = 0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let cx = center_x + dx;
            let cz = center_z + dz;
            let Some(input) = nine_chunk_input(&generator, cx, cz) else {
                continue;
            };
            if input
                .biomes
                .iter()
                .flat_map(|sections| sections.iter().flatten())
                .any(|cell| dripstone_biomes.contains(cell))
            {
                biome_chunks += 1;
            }
            let Ok(result) = generator.features(&input, LOCAL_MODIFICATIONS, Some(&allowed)) else {
                continue;
            };
            if result.unsupported {
                continue;
            }
            let hit = result.blocks[4].iter().flatten().any(|&state| {
                dripstone.is_some_and(|id| mc_bamboo_data::block_id_of_state(state) == Some(id))
            });
            if hit {
                println!("DRIP seed {seed} chunk ({cx},{cz})");
                found += 1;
            }
        }
    }
    println!(
        "DRIP seed {seed} around ({center_x},{center_z}) radius {radius}: {found} chunk(s), {biome_chunks} with a dripstone biome"
    );
}

/// 造中心加 8 个邻居到 CARVERS 阶段的输入；任一邻居失败时返回 `None`。
///
/// Build the center plus 8 neighbors at CARVERS; `None` when any neighbor fails.
fn nine_chunk_input(generator: &Generator<'_>, cx: i32, cz: i32) -> Option<FeatureChunks> {
    let mut blocks = Vec::with_capacity(9);
    let mut biomes = Vec::with_capacity(9);
    let mut heights = Vec::with_capacity(9);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let layers = generator.terrain(cx + dx, cz + dz, Stage::Carvers).ok()?;
            heights.push(prime_heightmaps(&layers, -64, 384));
            blocks.push(layers);
            biomes.push(generator.biomes(cx + dx, cz + dz).ok()?);
        }
    }
    Some(FeatureChunks {
        center_x: cx,
        center_z: cz,
        min_y: -64,
        height: 384,
        blocks: blocks.try_into().expect("9 chunks"),
        biomes: biomes.try_into().expect("9 chunks"),
        heights: heights.try_into().expect("9 chunks"),
        post_processing: std::array::from_fn(|_| Vec::new()),
    })
}

/// 只采群系、找滴石洞穴区块：地形和雕刻器太慢，先定位群系再对拍。
///
/// Sample biomes only to locate dripstone-caves chunks: terrain and carvers are far too
/// slow for a wide sweep, so find the biome first and run the oracle there.
#[test]
#[ignore]
fn find_dripstone_caves_chunks() {
    let seed: i64 = std::env::var("DRIP_SEED")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let radius: i32 = std::env::var("DRIP_RADIUS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(64);
    let worldgen = Worldgen::load().expect("worldgen");
    let generator = Generator::new(&worldgen, seed).expect("generator");
    let registry = generator.feature_registry();
    let dripstone_biomes: Vec<u32> = (0..128u32)
        .filter(|&id| {
            registry
                .biome_features(id, 2)
                .iter()
                .any(|name| name == "minecraft:large_dripstone")
        })
        .collect();
    let mut found = 0;
    for cz in -radius..=radius {
        for cx in -radius..=radius {
            let Ok(sections) = generator.biomes(cx, cz) else {
                continue;
            };
            if sections
                .iter()
                .flatten()
                .any(|cell| dripstone_biomes.contains(cell))
            {
                println!("DRIPBIOME seed {seed} chunk ({cx},{cz})");
                found += 1;
                if found >= 20 {
                    println!("DRIPBIOME seed {seed}: stopping after 20");
                    return;
                }
            }
        }
    }
    println!("DRIPBIOME seed {seed}: {found} chunk(s) in radius {radius}");
}

/// 找冻洋区块：`blue_ice` 只在这些群系出现，先定位再对拍。
///
/// Locate frozen-ocean chunks: `blue_ice` only appears there, so find them first.
#[test]
#[ignore]
fn find_frozen_ocean_chunks() {
    let seed: i64 = std::env::var("ICE_SEED")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let radius: i32 = std::env::var("ICE_RADIUS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(64);
    let worldgen = Worldgen::load().expect("worldgen");
    let generator = Generator::new(&worldgen, seed).expect("generator");
    let registry = generator.feature_registry();
    let ice_biomes: Vec<u32> = (0..128u32)
        .filter(|&id| {
            registry
                .biome_features(id, 4)
                .iter()
                .any(|name| name == "minecraft:blue_ice")
        })
        .collect();
    println!("ICE biome ids with blue_ice: {ice_biomes:?}");
    let mut found = 0;
    for cz in -radius..=radius {
        for cx in -radius..=radius {
            let Ok(sections) = generator.biomes(cx, cz) else {
                continue;
            };
            if sections
                .iter()
                .flatten()
                .any(|cell| ice_biomes.contains(cell))
            {
                println!("ICECHUNK seed {seed} chunk ({cx},{cz})");
                found += 1;
                if found >= 20 {
                    println!("ICECHUNK seed {seed}: stopping after 20");
                    return;
                }
            }
        }
    }
    println!("ICECHUNK seed {seed}: {found} chunk(s) in radius {radius}");
}

/// 找真正长出蓝冰的冻洋区块：蓝冰要求水面 + 相邻浮冰，条件很苛刻。
///
/// Find frozen-ocean chunks that really grow blue ice: it needs water plus an adjacent
/// packed-ice face, which is a strict condition.
#[test]
#[ignore]
fn list_blue_ice_chunks() {
    let seed: i64 = std::env::var("ICE_SEED")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let radius: i32 = std::env::var("ICE_RADIUS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(8);
    let center_x: i32 = std::env::var("ICE_X")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let center_z: i32 = std::env::var("ICE_Z")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let worldgen = Worldgen::load().expect("worldgen");
    let generator = Generator::new(&worldgen, seed).expect("generator");
    let allowed = vec!["minecraft:blue_ice".to_owned()];
    let blue_ice = mc_bamboo_data::block_by_name("minecraft:blue_ice")
        .and_then(|block| mc_bamboo_data::block_id_of_state(block.first_state));
    let mut found = 0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let cx = center_x + dx;
            let cz = center_z + dz;
            let Some(input) = nine_chunk_input(&generator, cx, cz) else {
                continue;
            };
            let Ok(result) = generator.features(&input, 1 << 4, Some(&allowed)) else {
                continue;
            };
            if result.unsupported {
                continue;
            }
            let hit = result.blocks[4].iter().flatten().any(|&state| {
                blue_ice.is_some_and(|id| mc_bamboo_data::block_id_of_state(state) == Some(id))
            });
            if hit {
                println!("BLUEICE seed {seed} chunk ({cx},{cz})");
                found += 1;
            }
        }
    }
    println!("BLUEICE seed {seed} around ({center_x},{center_z}) radius {radius}: {found}");
}
