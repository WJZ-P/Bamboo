//! 区块：16×384×16 的方块柱，由 24 个区块段组成。

use std::collections::BTreeMap;

use mc_bamboo_core::sign::SignContent;
use mc_bamboo_data::{block_of_state, blocks, facts, state_facts};

use crate::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    light::{SkyLightSources, sources::updated_lowest_source},
    palette::{PaletteView, PalettedContainer, pack_values},
};

/// `isAir`：空气、洞穴空气和虚空空气。
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// 官方 `BlockState.is(Block)`：两个状态属于同一个方块，不看朝向之类的属性。
fn is_same_block(first: u32, second: u32) -> bool {
    block_of_state(first).map(|block| block.name) == block_of_state(second).map(|block| block.name)
}

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
        match (is_air(previous), is_air(state)) {
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

    /// `hasOnlyAir`。
    pub fn is_empty(&self) -> bool {
        self.non_air_blocks == 0
    }

    /// `maybeHas`：调色板里有满足条件的状态。没有调色板时不知道，按有算。
    pub fn maybe_has(&self, predicate: impl Fn(u32) -> bool) -> bool {
        match self.blocks.palette() {
            PaletteView::Single(state) => predicate(state),
            PaletteView::Indirect(states) => states.iter().any(|&state| predicate(state)),
            PaletteView::Global => true,
        }
    }
}

/// 区块里的一个方块实体。现在只有位置和类型，内容都是默认值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEntity {
    /// 区块内坐标。
    pub x: usize,
    /// 世界高度。
    pub y: i32,
    /// 区块内坐标。
    pub z: usize,
    /// `minecraft:block_entity_type` 注册表 ID。
    pub kind: i32,
}

/// 一个区块。
#[derive(Debug, Clone)]
pub struct Chunk {
    sections: Vec<Section>,
    /// 有方块实体的位置和类型。键是 `(y, z, x)`，所以遍历顺序和放置的先后无关。
    block_entities: BTreeMap<(i32, usize, usize), i32>,
    /// 有内容的告示牌。没有条目时内容是空白的，和官方按状态临时建出来的方块实体一样。
    signs: BTreeMap<(i32, usize, usize), SignContent>,
    sky_light_sources: SkyLightSources,
}

impl Chunk {
    /// 创建一个全是空气的区块。天空光源表要等 [`initialize_light_sources`](Self::initialize_light_sources) 才算。
    pub fn empty(biome: u32) -> Self {
        Self {
            sections: vec![Section::empty(biome); SECTION_COUNT],
            block_entities: BTreeMap::new(),
            signs: BTreeMap::new(),
            sky_light_sources: SkyLightSources::default(),
        }
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// 每一列最低的天空光源。
    pub fn sky_light_sources(&self) -> &SkyLightSources {
        &self.sky_light_sources
    }

    /// `initializeLightSources`：按现在的方块重算天空光源，生成完区块、开始算光照之前调用一次。
    pub fn initialize_light_sources(&mut self) {
        self.sky_light_sources = SkyLightSources::fill_from(self);
    }

    /// `ChunkSkyLightSources.update`：`(x, y, z)` 换了方块，光照性质也变了。
    pub fn update_sky_light_sources(&mut self, x: usize, y: i32, z: usize) {
        if let Some(lowest) = updated_lowest_source(self, x, y, z) {
            self.sky_light_sources.set(x, z, lowest);
        }
    }

    /// 和 [`block`](Self::block) 相同，但世界外是空气，官方 `LevelChunk.getBlockState` 的做法。
    pub fn block_or_air(&self, x: usize, y: i32, z: usize) -> u32 {
        if (MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            self.block(x, y, z)
        } else {
            blocks::AIR
        }
    }

    /// 这一高度所在的区块段只有空气。
    pub fn is_section_empty(&self, y: i32) -> bool {
        self.sections[Self::locate(y).0].is_empty()
    }

    /// 区块里的方块实体，按高度、z、x 升序。
    pub fn block_entities(&self) -> impl Iterator<Item = BlockEntity> + '_ {
        self.block_entities
            .iter()
            .map(|(&(y, z, x), &kind)| BlockEntity { x, y, z, kind })
    }

    /// 这一格的告示牌内容。没写过时返回 `None`。
    pub fn sign(&self, x: usize, y: i32, z: usize) -> Option<&SignContent> {
        self.signs.get(&(y, z, x))
    }

    /// 写入告示牌内容。
    pub fn set_sign(&mut self, x: usize, y: i32, z: usize, content: SignContent) {
        self.signs.insert((y, z, x), content);
    }

    /// 有内容的告示牌，键和 [`block_entities`](Self::block_entities) 的顺序相同。
    pub fn signs(&self) -> impl Iterator<Item = (usize, i32, usize, &SignContent)> + '_ {
        self.signs
            .iter()
            .map(|(&(y, z, x), content)| (x, y, z, content))
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
    ///
    /// 和官方 `LevelChunk.setBlockState` 一样，只有换成别的方块时才丢掉方块实体：
    /// 旋转、含水这些状态变化不影响告示牌上的字。
    pub fn set_block(&mut self, x: usize, y: i32, z: usize, state: u32) {
        let previous = self.block(x, y, z);
        let (section, local_y) = Self::locate(y);
        self.sections[section].set_block(x, local_y, z, state);

        let key = (y, z, x);
        if !is_same_block(previous, state) {
            self.signs.remove(&key);
        }
        match block_of_state(state).and_then(|block| block.block_entity_type) {
            Some(kind) => self.block_entities.insert(key, kind),
            None => self.block_entities.remove(&key),
        };
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
    use mc_bamboo_data::registries::block_entity_type;

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

    #[test]
    fn records_block_entities_of_every_state_of_a_block() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(3, -60, 5, blocks::CHEST);
        // 同一个方块的其他状态也是方块实体，比如朝向不同的箱子。
        chunk.set_block(4, -60, 5, blocks::CHEST + 1);
        chunk.set_block(5, -60, 5, blocks::STONE);

        let entity = |x| BlockEntity {
            x,
            y: -60,
            z: 5,
            kind: block_entity_type::CHEST,
        };
        assert_eq!(
            chunk.block_entities().collect::<Vec<_>>(),
            [entity(3), entity(4)]
        );
    }

    #[test]
    fn replacing_a_block_entity_updates_the_list() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::CHEST);

        chunk.set_block(0, 0, 0, blocks::OAK_WALL_SIGN);
        let kinds: Vec<_> = chunk.block_entities().map(|entity| entity.kind).collect();
        assert_eq!(kinds, [block_entity_type::SIGN]);

        chunk.set_block(0, 0, 0, blocks::AIR);
        assert_eq!(chunk.block_entities().count(), 0);
    }

    #[test]
    fn block_entity_order_does_not_depend_on_placement_order() {
        let positions = [(1, 7, 2), (2, -60, 9), (0, 7, 2), (0, 7, 1)];
        let mut forward = Chunk::empty(0);
        let mut backward = Chunk::empty(0);
        for (x, y, z) in positions {
            forward.set_block(x, y, z, blocks::CHEST);
        }
        for (x, y, z) in positions.into_iter().rev() {
            backward.set_block(x, y, z, blocks::CHEST);
        }

        let order = |chunk: &Chunk| {
            chunk
                .block_entities()
                .map(|entity| (entity.y, entity.z, entity.x))
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&forward), order(&backward));
        assert_eq!(
            order(&forward),
            [(-60, 9, 2), (7, 1, 0), (7, 2, 0), (7, 2, 1)]
        );
    }
}
