//! 区块：16×384×16 的方块柱，由 24 个区块段组成。

use mc_bamboo_data::blocks;

use crate::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    palette::{PalettedContainer, pack_values},
};

/// 一个方块柱在高度图里占用的位数，要能表示 0 到 384。
const HEIGHTMAP_BITS: u8 = 9;

/// 16×16×16 的区块段。
#[derive(Debug, Clone)]
pub struct Section {
    blocks: PalettedContainer,
    biomes: PalettedContainer,
    non_air_blocks: u16,
}

impl Section {
    fn empty(biome: u32) -> Self {
        Self {
            blocks: PalettedContainer::blocks(blocks::AIR),
            biomes: PalettedContainer::biomes(biome),
            non_air_blocks: 0,
        }
    }

    /// 方块在容器里的下标，顺序是 y、z、x。
    const fn block_index(x: usize, y: usize, z: usize) -> usize {
        (y << 8) | (z << 4) | x
    }

    fn block(&self, x: usize, y: usize, z: usize) -> u32 {
        self.blocks.get(Self::block_index(x, y, z))
    }

    fn set_block(&mut self, x: usize, y: usize, z: usize, state: u32) {
        let index = Self::block_index(x, y, z);
        let previous = self.blocks.get(index);
        match (previous == blocks::AIR, state == blocks::AIR) {
            (true, false) => self.non_air_blocks += 1,
            (false, true) => self.non_air_blocks -= 1,
            _ => {}
        }
        self.blocks.set(index, state);
    }

    pub fn blocks(&self) -> &PalettedContainer {
        &self.blocks
    }

    pub fn biomes(&self) -> &PalettedContainer {
        &self.biomes
    }

    /// 非空气方块的数量，客户端用它判断区块段是否为空。
    pub fn block_count(&self) -> u16 {
        self.non_air_blocks
    }
}

/// 一个区块。
#[derive(Debug, Clone)]
pub struct Chunk {
    sections: Vec<Section>,
}

impl Chunk {
    /// 创建一个全是空气的区块。
    pub fn empty(biome: u32) -> Self {
        Self {
            sections: vec![Section::empty(biome); SECTION_COUNT],
        }
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// 把世界高度拆成区块段下标和段内高度。
    fn locate(y: i32) -> (usize, usize) {
        let offset = y - MIN_Y;
        assert!(
            (0..WORLD_HEIGHT).contains(&offset),
            "高度 {y} 超出了世界范围"
        );
        (
            (offset / SECTION_SIZE) as usize,
            (offset % SECTION_SIZE) as usize,
        )
    }

    /// 读取方块状态。`x` 和 `z` 是区块内坐标，`y` 是世界高度。
    pub fn block(&self, x: usize, y: i32, z: usize) -> u32 {
        let (section, local_y) = Self::locate(y);
        self.sections[section].block(x, local_y, z)
    }

    /// 设置方块状态。`x` 和 `z` 是区块内坐标，`y` 是世界高度。
    pub fn set_block(&mut self, x: usize, y: i32, z: usize, state: u32) {
        let (section, local_y) = Self::locate(y);
        self.sections[section].set_block(x, local_y, z, state);
    }

    /// 这一列最高方块上方第一格的高度。整列都是空气时返回 [`MIN_Y`]。
    pub fn surface_y(&self, x: usize, z: usize) -> i32 {
        (MIN_Y..MIN_Y + WORLD_HEIGHT)
            .rev()
            .find(|&y| self.block(x, y, z) != blocks::AIR)
            .map_or(MIN_Y, |y| y + 1)
    }

    /// 编码高度图：每列存表面相对世界底部的高度，下标顺序是 z、x。
    pub fn heightmap(&self) -> Vec<u64> {
        let columns = (0..256).map(|index| {
            let (x, z) = (index % 16, index / 16);
            (self.surface_y(x, z) - MIN_Y) as u32
        });
        pack_values(columns.collect::<Vec<_>>().into_iter(), HEIGHTMAP_BITS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_blocks_in_the_right_section() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(1, MIN_Y, 2, blocks::STONE);
        chunk.set_block(3, 0, 4, blocks::DIRT);

        assert_eq!(chunk.block(1, MIN_Y, 2), blocks::STONE);
        assert_eq!(chunk.block(3, 0, 4), blocks::DIRT);
        assert_eq!(chunk.sections()[0].block_count(), 1);
        assert_eq!(chunk.sections()[4].block_count(), 1);
    }

    #[test]
    fn replacing_a_block_keeps_the_count() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        chunk.set_block(0, 0, 0, blocks::DIRT);
        assert_eq!(chunk.sections()[4].block_count(), 1);

        chunk.set_block(0, 0, 0, blocks::AIR);
        assert_eq!(chunk.sections()[4].block_count(), 0);
    }

    #[test]
    fn surface_is_the_first_air_above_the_top_block() {
        let mut chunk = Chunk::empty(0);
        assert_eq!(chunk.surface_y(0, 0), MIN_Y);

        chunk.set_block(0, 10, 0, blocks::STONE);
        assert_eq!(chunk.surface_y(0, 0), 11);
    }

    #[test]
    fn heightmap_is_relative_to_the_world_bottom() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(1, MIN_Y, 0, blocks::STONE);

        let heightmap = chunk.heightmap();
        assert_eq!(heightmap.len(), 37);
        // 第二列的高度是 1，紧跟在第一列的 9 位之后。
        assert_eq!(heightmap[0], 1 << 9);
    }

    #[test]
    #[should_panic(expected = "超出了世界范围")]
    fn rejects_heights_outside_the_world() {
        Chunk::empty(0).block(0, MIN_Y - 1, 0);
    }
}
