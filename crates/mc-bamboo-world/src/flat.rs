//! 超平坦世界：和官方默认的超平坦预设相同的地层。

use mc_bamboo_data::{blocks, registries::biome};

use crate::{MIN_Y, chunk::Chunk};

/// 从世界底部往上的地层：方块和厚度。
const LAYERS: [(u32, i32); 3] = [
    (blocks::BEDROCK, 1),
    (blocks::DIRT, 2),
    (blocks::GRASS_BLOCK, 1),
];

/// 世界的海平面高度，和官方的超平坦世界一致。
pub const SEA_LEVEL: i32 = -63;

/// 地表高度：玩家站立的第一格空气。
pub const fn surface_y() -> i32 {
    let mut y = MIN_Y;
    let mut index = 0;
    while index < LAYERS.len() {
        y += LAYERS[index].1;
        index += 1;
    }
    y
}

/// 超平坦模板的地层，按从下往上的顺序，给生成器导出 `world_gen_settings.dat` 用。
/// 名字带命名空间；和官方 `FlatLevelGeneratorSettings` 的 `layers` 一致。
pub fn archive_layers() -> Vec<(&'static str, i32)> {
    LAYERS
        .iter()
        .map(|&(state, thickness)| {
            let name = mc_bamboo_data::block_of_state(state)
                .expect("模板层必须是已知方块")
                .name;
            (name, thickness)
        })
        .collect()
}

/// 生成一个超平坦区块。所有区块的内容都相同，所以不需要坐标。
pub fn generate_chunk() -> Chunk {
    let mut chunk = Chunk::empty(biome::PLAINS as u32);
    let mut y = MIN_Y;
    for (block, thickness) in LAYERS {
        for _ in 0..thickness {
            for z in 0..16 {
                for x in 0..16 {
                    chunk.set_block(x, y, z, block);
                }
            }
            y += 1;
        }
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_official_flat_preset() {
        let chunk = generate_chunk();
        assert_eq!(chunk.block(0, -64, 0), blocks::BEDROCK);
        assert_eq!(chunk.block(5, -63, 5), blocks::DIRT);
        assert_eq!(chunk.block(5, -62, 5), blocks::DIRT);
        assert_eq!(chunk.block(15, -61, 15), blocks::GRASS_BLOCK);
        assert_eq!(chunk.block(0, -60, 0), blocks::AIR);
    }

    #[test]
    fn only_the_lowest_section_has_blocks() {
        let chunk = generate_chunk();
        assert_eq!(chunk.sections()[0].block_count(), 4 * 256);
        assert!(
            chunk.sections()[1..]
                .iter()
                .all(|section| section.block_count() == 0)
        );
    }

    #[test]
    fn surface_is_above_the_grass() {
        assert_eq!(surface_y(), -60);
        assert_eq!(generate_chunk().surface_y(7, 7), surface_y());
    }
}
