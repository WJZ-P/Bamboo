//! 区块：16×384×16 的方块柱，由 24 个区块段组成。

use std::collections::BTreeMap;

use mc_bamboo_core::sign::SignContent;
use mc_bamboo_data::{
    block_of_state, blocks, facts, heightmaps, registries, state_facts, state_fluid,
};
use mc_bamboo_nbt::Tag;

use crate::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    light::{SkyLightSources, sources::updated_lowest_source},
    palette::{PaletteView, PalettedContainer, pack_values},
};

use super::column::ColumnPartsError;

/// `isAir`：空气、洞穴空气和虚空空气。
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// 官方 `BlockState.is(Block)`：两个状态属于同一个方块，不看朝向之类的属性。
fn is_same_block(first: u32, second: u32) -> bool {
    block_of_state(first).map(|block| block.name) == block_of_state(second).map(|block| block.name)
}

/// `FluidState.isEmpty()`：只有空流体才算没有流体。含水方块的流体不是空的。
fn fluid_is_empty(state: u32) -> bool {
    state_fluid(state).fluid == registries::fluid::EMPTY
}

/// 区块里一直留着的高度图，官方 `ChunkStatus.FINAL_HEIGHTMAPS`。每种用不同的判定找地面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightmapKind {
    /// 最高的非空气方块。
    WorldSurface,
    /// 最高的 `BLOCKS_MOTION_IN_HEIGHTMAP` 方块，水不算。
    OceanFloor,
    /// 最高的挡运动方块，水也算。
    MotionBlocking,
    /// 和 [`MotionBlocking`](Self::MotionBlocking) 一样，但树叶不算。
    MotionBlockingNoLeaves,
}

impl HeightmapKind {
    /// 全部种类。
    pub const ALL: [Self; 4] = [
        Self::WorldSurface,
        Self::OceanFloor,
        Self::MotionBlocking,
        Self::MotionBlockingNoLeaves,
    ];

    /// 生成数据里的类型对应的种类。只在世界生成时用、区块里不留的类型返回 `None`。
    pub fn of_type(heightmap: &heightmaps::HeightmapType) -> Option<Self> {
        Some(match heightmap.name {
            "WORLD_SURFACE" => Self::WorldSurface,
            "OCEAN_FLOOR" => Self::OceanFloor,
            "MOTION_BLOCKING" => Self::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Self::MotionBlockingNoLeaves,
            _ => return None,
        })
    }

    /// 官方的 `Types.isOpaque()`：这一格算不算这一列的地面。
    fn is_opaque(self, state: u32) -> bool {
        let found = state_facts(state);
        match self {
            Self::WorldSurface => found & facts::AIR == 0,
            Self::OceanFloor => found & facts::MOTION_BLOCKING != 0,
            Self::MotionBlocking => found & facts::MOTION_BLOCKING != 0 || !fluid_is_empty(state),
            Self::MotionBlockingNoLeaves => {
                found & facts::MOTION_BLOCKING_NO_LEAVES != 0 || !fluid_is_empty(state)
            }
        }
    }
}

/// 一张高度图：256 列，每列是这一列最高地面的上面一格相对世界底部的高度。
#[derive(Debug, Clone, Copy)]
struct Heightmap {
    columns: [u32; 256],
}

impl Heightmap {
    /// 还没有方块的高度图，每列都在世界底部。
    const EMPTY: Self = Self { columns: [0; 256] };

    /// 从磁盘上的 9 位一项、按字对齐的 long 数组还原。坏数据一律报错。
    fn from_words(words: &[u64]) -> Result<Self, ColumnPartsError> {
        let mut columns = [0u32; 256];
        for (index, column) in columns.iter_mut().enumerate() {
            let per_word = 64 / HEIGHTMAP_BITS as usize;
            let shift = (index % per_word) * HEIGHTMAP_BITS as usize;
            let Some(&word) = words.get(index / per_word) else {
                return Err(ColumnPartsError::BadHeightmap);
            };
            let value = (word >> shift) & ((1 << HEIGHTMAP_BITS) - 1);
            if value > WORLD_HEIGHT as u64 {
                return Err(ColumnPartsError::BadHeightmap);
            }
            *column = value as u32;
        }
        Ok(Self { columns })
    }

    const fn index(x: usize, z: usize) -> usize {
        x + z * 16
    }

    /// 官方的 `Heightmap.update`：`(x, y, z)` 换成了 `state`。
    fn update(
        &mut self,
        kind: HeightmapKind,
        sections: &[Section],
        x: usize,
        y: i32,
        z: usize,
        state: u32,
    ) {
        let index = Self::index(x, z);
        let first_available = self.columns[index] as i32 + MIN_Y;
        if y <= first_available - 2 {
            return;
        }

        if kind.is_opaque(state) {
            if y >= first_available {
                self.set_height(index, y + 1);
            }
        } else if first_available - 1 == y {
            // 移走了这一列最高的地面，往下找下一格。方块已经写进区块段了。
            for found in (MIN_Y..y).rev() {
                if kind.is_opaque(section_block(sections, x, found, z)) {
                    self.set_height(index, found + 1);
                    return;
                }
            }
            self.set_height(index, MIN_Y);
        }
    }

    fn set_height(&mut self, index: usize, height: i32) {
        self.columns[index] = (height - MIN_Y) as u32;
    }
}

/// 区块段里 `(x, y, z)` 的方块。`y` 是世界高度，调用前保证在世界里。
fn section_block(sections: &[Section], x: usize, y: i32, z: usize) -> u32 {
    let offset = (y - MIN_Y) as usize;
    sections[offset / SECTION_SIZE as usize].block(x, offset % SECTION_SIZE as usize, z)
}

/// 高度图每列占的位数，官方 `Mth.ceillog2(getHeight() + 1)`。
const fn heightmap_bits() -> u8 {
    let mut bits = 0;
    while (1 << bits) < WORLD_HEIGHT as u32 + 1 {
        bits += 1;
    }
    bits
}

const HEIGHTMAP_BITS: u8 = heightmap_bits();

/// 区块段里哪些位置有方块实体。只有调色板里可能有带方块实体的状态时才扫那一段。
fn find_block_entities(sections: &[Section]) -> BTreeMap<(i32, usize, usize), i32> {
    let mut found = BTreeMap::new();
    for (index, section) in sections.iter().enumerate() {
        let candidates: Vec<u32> = match section.blocks().palette() {
            PaletteView::Single(state) => vec![state],
            PaletteView::Indirect(states) => states.to_vec(),
            PaletteView::Global => Vec::new(),
        };
        let indexed: Vec<(u32, i32)> = candidates
            .iter()
            .copied()
            .filter_map(|state| {
                block_of_state(state)
                    .and_then(|block| block.block_entity_type)
                    .map(|kind| (state, kind))
            })
            .collect();
        let needs_scan =
            matches!(section.blocks().palette(), PaletteView::Global) || !indexed.is_empty();
        if !needs_scan {
            continue;
        }
        let section_y = MIN_Y + index as i32 * SECTION_SIZE;
        for y in 0..SECTION_SIZE as usize {
            for z in 0..16 {
                for x in 0..16 {
                    let state = section.block(x, y, z);
                    let kind = if indexed.is_empty() {
                        block_of_state(state).and_then(|block| block.block_entity_type)
                    } else {
                        indexed
                            .iter()
                            .find(|(candidate, _)| *candidate == state)
                            .map(|(_, kind)| *kind)
                    };
                    if let Some(kind) = kind {
                        found.insert((section_y + y as i32, z, x), kind);
                    }
                }
            }
        }
    }
    found
}

/// 16×16×16 的区块段。
#[derive(Debug, Clone)]
pub struct Section {
    blocks: PalettedContainer,
    biomes: PalettedContainer,
    non_air_blocks: u16,
    /// 会随机刻的方块数，官方 `tickingBlockCount` 和 `tickingFluidCount` 合在一起。
    random_tickers: u16,
    fluids: u16,
}

/// 这个状态的方块或者它的流体有随机刻。
fn ticks_randomly(state: u32) -> bool {
    state_facts(state) & (facts::RANDOMLY_TICKING | facts::FLUID_RANDOMLY_TICKING) != 0
}

impl Section {
    fn empty(biome: u32) -> Self {
        Self {
            blocks: PalettedContainer::blocks(blocks::AIR),
            biomes: PalettedContainer::biomes(biome),
            non_air_blocks: 0,
            random_tickers: 0,
            fluids: 0,
        }
    }

    /// 从磁盘调色板组装区块段，按官方 `LevelChunkSection.recalcBlockCounts` 统计计数。
    pub(crate) fn from_disk(blocks: PalettedContainer, biomes: PalettedContainer) -> Self {
        let mut section = Self {
            blocks,
            biomes,
            non_air_blocks: 0,
            random_tickers: 0,
            fluids: 0,
        };
        section.recount();
        section
    }

    /// 统计每个调色板下标出现的次数，再按调色板汇总三种计数。
    fn recount(&mut self) {
        let counts = self.blocks.index_counts();
        match self.blocks.palette() {
            PaletteView::Single(state) => {
                self.non_air_blocks = if is_air(state) { 0 } else { 4096 };
                self.random_tickers = if ticks_randomly(state) { 4096 } else { 0 };
                self.fluids = if fluid_is_empty(state) { 0 } else { 4096 };
            }
            PaletteView::Indirect(states) => {
                for (index, &count) in counts.iter().enumerate() {
                    if let Some(&state) = states.get(index) {
                        self.non_air_blocks += u16::from(!is_air(state)) * count;
                        self.random_tickers += u16::from(ticks_randomly(state)) * count;
                        self.fluids += u16::from(!fluid_is_empty(state)) * count;
                    }
                }
            }
            PaletteView::Global => {
                // 全局形式没有调色板，退回按格查状态属性；磁盘上的段很少走到这里。
                for index in 0..4096 {
                    let state = self.blocks.get(index);
                    self.non_air_blocks += u16::from(!is_air(state));
                    self.random_tickers += u16::from(ticks_randomly(state));
                    self.fluids += u16::from(!fluid_is_empty(state));
                }
            }
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
        self.random_tickers -= u16::from(ticks_randomly(previous));
        self.random_tickers += u16::from(ticks_randomly(state));
        match (fluid_is_empty(previous), fluid_is_empty(state)) {
            (true, false) => self.fluids += 1,
            (false, true) => self.fluids -= 1,
            _ => {}
        }
        self.blocks.set(index, state);
    }

    /// `LevelChunkSection.isRandomlyTicking`：里面有会随机刻的方块或者流体。
    pub fn is_randomly_ticking(&self) -> bool {
        self.random_tickers > 0
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

    /// 有流体的方块数量，区块段头里的 `fluidCount`。
    pub fn fluid_count(&self) -> u16 {
        self.fluids
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
    /// 和 [`HeightmapKind::ALL`] 一一对应。
    heightmaps: [Heightmap; HeightmapKind::ALL.len()],
    /// 有方块实体的位置和类型。键是 `(y, z, x)`，所以遍历顺序和放置的先后无关。
    block_entities: BTreeMap<(i32, usize, usize), i32>,
    /// 有内容的告示牌。没有条目时内容是空白的，和官方按状态临时建出来的方块实体一样。
    signs: BTreeMap<(i32, usize, usize), SignContent>,
    /// 方块实体要发给客户端的那段 NBT，官方 `BlockEntity.getUpdateTag` 编好的网络字节。
    ///
    /// 内容由上层算（旗帜的图案、头颅的档案这些来自物品的组件），这里只跟着方块一起存和丢。
    update_tags: BTreeMap<(i32, usize, usize), Vec<u8>>,
    /// 读盘时原样保留的方块实体条目（D9），按位置挂在类型表旁边。
    /// 值是当时的类型 ID 和整条复合标签；换类型或删掉方块时一起丢。
    preserved_block_entities: BTreeMap<(i32, usize, usize), (i32, Tag)>,
    sky_light_sources: SkyLightSources,
}

impl Chunk {
    /// 创建一个全是空气的区块。天空光源表要等 [`initialize_light_sources`](Self::initialize_light_sources) 才算。
    pub fn empty(biome: u32) -> Self {
        Self {
            sections: vec![Section::empty(biome); SECTION_COUNT],
            heightmaps: [Heightmap::EMPTY; HeightmapKind::ALL.len()],
            block_entities: BTreeMap::new(),
            signs: BTreeMap::new(),
            update_tags: BTreeMap::new(),
            preserved_block_entities: BTreeMap::new(),
            sky_light_sources: SkyLightSources::default(),
        }
    }

    /// 从磁盘上重新建好的区块段组装区块。
    ///
    /// 计数和方块实体表在段的构造里按官方 `recalcBlockCounts` 的方式算好了；
    /// 这里只在有高度图缺失时按方块重算（官方读盘对缺失的高度图也是重算）。
    /// 方块实体表由段里的调色板决定，只有可能有方块实体时才扫那一段。
    pub(crate) fn from_sections(
        sections: Vec<Section>,
        heightmaps: &[Option<Vec<u64>>; HeightmapKind::ALL.len()],
    ) -> Result<Self, ColumnPartsError> {
        if sections.len() != SECTION_COUNT {
            return Err(ColumnPartsError::WrongSectionCount {
                expected: SECTION_COUNT,
                actual: sections.len(),
            });
        }
        let all_given = heightmaps.iter().all(Option::is_some);
        let mut maps = [Heightmap::EMPTY; HeightmapKind::ALL.len()];
        if !all_given {
            for (index, section) in sections.iter().enumerate() {
                let section_y = MIN_Y + index as i32 * SECTION_SIZE;
                for y in 0..SECTION_SIZE as usize {
                    for z in 0..16 {
                        for x in 0..16 {
                            let state = section.block(x, y, z);
                            for (kind, heightmap) in HeightmapKind::ALL.into_iter().zip(&mut maps) {
                                heightmap.update(
                                    kind,
                                    &sections,
                                    x,
                                    section_y + y as i32,
                                    z,
                                    state,
                                );
                            }
                        }
                    }
                }
            }
        }
        for (kind, given) in HeightmapKind::ALL.into_iter().zip(heightmaps) {
            if let Some(words) = given {
                let position = HeightmapKind::ALL
                    .iter()
                    .position(|&candidate| candidate == kind)
                    .expect("HeightmapKind::ALL 里有每一种高度图");
                maps[position] = Heightmap::from_words(words)?;
            }
        }
        let block_entities = find_block_entities(&sections);
        let mut chunk = Self {
            sections,
            heightmaps: maps,
            block_entities,
            signs: BTreeMap::new(),
            update_tags: BTreeMap::new(),
            preserved_block_entities: BTreeMap::new(),
            sky_light_sources: SkyLightSources::default(),
        };
        chunk.initialize_light_sources();
        Ok(chunk)
    }

    /// 写入一个告示牌，坐标由调用方校验过。
    pub(crate) fn put_sign(
        &mut self,
        x: usize,
        y: i32,
        z: usize,
        content: SignContent,
    ) -> Result<(), ColumnPartsError> {
        if x >= 16 || z >= 16 || !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return Err(ColumnPartsError::BadPosition { x, y, z });
        }
        self.signs.insert((y, z, x), content);
        Ok(())
    }

    /// 写入方块实体要发给客户端的 NBT，坐标由调用方校验过。
    pub(crate) fn put_update_tag(
        &mut self,
        x: usize,
        y: i32,
        z: usize,
        tag: Vec<u8>,
    ) -> Result<(), ColumnPartsError> {
        if x >= 16 || z >= 16 || !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return Err(ColumnPartsError::BadPosition { x, y, z });
        }
        self.update_tags.insert((y, z, x), tag);
        Ok(())
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

    /// 方块实体发给客户端的 NBT，键和 [`block_entities`](Self::block_entities) 的顺序相同。
    pub(crate) fn update_tags(&self) -> impl Iterator<Item = (usize, i32, usize, &[u8])> + '_ {
        self.update_tags
            .iter()
            .map(|(&(y, z, x), tag)| (x, y, z, tag.as_slice()))
    }

    /// 有内容的告示牌，键和 [`block_entities`](Self::block_entities) 的顺序相同。
    pub fn signs(&self) -> impl Iterator<Item = (usize, i32, usize, &SignContent)> + '_ {
        self.signs
            .iter()
            .map(|(&(y, z, x), content)| (x, y, z, content))
    }

    /// 这一格的方块实体要发给客户端的 NBT。没有时返回 `None`，和官方的空标签一样不发。
    pub fn update_tag(&self, x: usize, y: i32, z: usize) -> Option<&[u8]> {
        self.update_tags.get(&(y, z, x)).map(Vec::as_slice)
    }

    /// 这一格读盘时原样保留的方块实体条目，按位置挂在类型表旁边。
    pub fn preserved_block_entity(&self, x: usize, y: i32, z: usize) -> Option<&Tag> {
        self.preserved_block_entities
            .get(&(y, z, x))
            .map(|(_, tag)| tag)
    }

    /// 读盘时挂上一条原样保留的方块实体条目。坐标和类型都校验过才收。
    pub(crate) fn put_preserved_block_entity(
        &mut self,
        x: usize,
        y: i32,
        z: usize,
        kind: i32,
        tag: Tag,
    ) -> Result<(), ColumnPartsError> {
        if x >= 16 || z >= 16 || !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return Err(ColumnPartsError::BadPosition { x, y, z });
        }
        let key = (y, z, x);
        if self.block_entities.get(&key) == Some(&kind) {
            self.preserved_block_entities.insert(key, (kind, tag));
        }
        Ok(())
    }

    /// 所有原样保留的方块实体条目，键和 [`block_entities`](Self::block_entities) 的顺序相同。
    pub fn preserved_block_entities(
        &self,
    ) -> impl Iterator<Item = (usize, i32, usize, i32, &Tag)> + '_ {
        self.preserved_block_entities
            .iter()
            .map(|(&(y, z, x), &(kind, ref tag))| (x, y, z, kind, tag))
    }

    /// 写入方块实体要发给客户端的 NBT。`None` 表示这一格没有要发的内容。
    pub fn set_update_tag(&mut self, x: usize, y: i32, z: usize, tag: Option<Vec<u8>>) {
        match tag {
            Some(tag) => self.update_tags.insert((y, z, x), tag),
            None => self.update_tags.remove(&(y, z, x)),
        };
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
        // 官方 `LevelChunk.setBlockState`：区块段先写，再用新状态更新每一张高度图。
        for (kind, heightmap) in HeightmapKind::ALL.into_iter().zip(&mut self.heightmaps) {
            heightmap.update(kind, &self.sections, x, y, z, state);
        }

        let key = (y, z, x);
        let old_kind = self.block_entities.get(&key).copied();
        if !is_same_block(previous, state) {
            self.signs.remove(&key);
            self.update_tags.remove(&key);
        }
        let kind = block_of_state(state).and_then(|block| block.block_entity_type);
        match kind {
            Some(kind) => {
                self.block_entities.insert(key, kind);
            }
            None => {
                self.block_entities.remove(&key);
            }
        }
        // 保留的条目属于「这个位置的这一个方块实体」：类型换了或方块没了就丢掉。
        if old_kind.is_none_or(|old| Some(old) != kind) {
            self.preserved_block_entities.remove(&key);
        }
    }

    /// 这一列最高方块上方第一格的高度。整列都是空气时返回 [`MIN_Y`]。
    pub fn surface_y(&self, x: usize, z: usize) -> i32 {
        (MIN_Y..MIN_Y + WORLD_HEIGHT)
            .rev()
            .find(|&y| self.block(x, y, z) != blocks::AIR)
            .map_or(MIN_Y, |y| y + 1)
    }

    /// 编码一种高度图：每列存地面上面一格相对世界底部的高度，下标顺序是 z、x。
    pub fn heightmap(&self, kind: HeightmapKind) -> Vec<u64> {
        let position = HeightmapKind::ALL
            .iter()
            .position(|&candidate| candidate == kind)
            .expect("HeightmapKind::ALL 里有每一种高度图");
        pack_values(
            self.heightmaps[position].columns.into_iter(),
            HEIGHTMAP_BITS,
        )
    }

    /// `getHeightmapPos(kind, pos).getY()`：这一列在高度图里的世界高度。
    pub fn heightmap_y(&self, kind: HeightmapKind, x: usize, z: usize) -> i32 {
        let position = HeightmapKind::ALL
            .iter()
            .position(|&candidate| candidate == kind)
            .expect("HeightmapKind::ALL 里有每一种高度图");
        MIN_Y + self.heightmaps[position].columns[Heightmap::index(x, z)] as i32
    }

    /// 这一格所在 quart 位置的群系注册表 ID，读区块段的群系调色板。
    pub fn biome_at(&self, x: usize, y: i32, z: usize) -> u32 {
        let (section, _) = Self::locate(y);
        let quart_x = (x / 4) & 3;
        let quart_y = ((y - MIN_Y) / 4) as usize & 3;
        let quart_z = (z / 4) & 3;
        let index = quart_y * 16 + quart_z * 4 + quart_x;
        self.sections[section].biomes().get(index)
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
    fn sections_know_when_something_ticks_randomly() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        assert!(!chunk.sections()[4].is_randomly_ticking());

        chunk.set_block(0, 0, 0, blocks::GRASS_BLOCK);
        chunk.set_block(1, 0, 0, blocks::LAVA);
        assert!(chunk.sections()[4].is_randomly_ticking());

        chunk.set_block(0, 0, 0, blocks::DIRT);
        assert!(chunk.sections()[4].is_randomly_ticking());
        chunk.set_block(1, 0, 0, blocks::AIR);
        assert!(!chunk.sections()[4].is_randomly_ticking());
    }

    #[test]
    fn surface_is_the_first_air_above_the_top_block() {
        let mut chunk = Chunk::empty(0);
        assert_eq!(chunk.surface_y(0, 0), MIN_Y);

        chunk.set_block(0, 10, 0, blocks::STONE);
        assert_eq!(chunk.surface_y(0, 0), 11);
    }

    /// 从编码好的高度图里读出 `(x, z)` 这一列。
    fn column_height(chunk: &Chunk, kind: HeightmapKind, x: usize, z: usize) -> u32 {
        let heightmap = chunk.heightmap(kind);
        let index = x + z * 16;
        let per_word = 64 / HEIGHTMAP_BITS as usize;
        let shift = (index % per_word) * HEIGHTMAP_BITS as usize;
        ((heightmap[index / per_word] >> shift) & ((1 << HEIGHTMAP_BITS) - 1)) as u32
    }

    #[test]
    fn heightmap_is_relative_to_the_world_bottom() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(1, MIN_Y, 0, blocks::STONE);

        let heightmap = chunk.heightmap(HeightmapKind::WorldSurface);
        assert_eq!(heightmap.len(), 37);
        // 第二列的高度是 1，紧跟在第一列的 9 位之后。
        assert_eq!(heightmap[0], 1 << 9);
        assert_eq!(column_height(&chunk, HeightmapKind::WorldSurface, 1, 0), 1);
    }

    #[test]
    fn breaking_the_top_block_finds_the_next_ground() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, MIN_Y, 0, blocks::STONE);
        chunk.set_block(0, MIN_Y + 1, 0, blocks::DIRT);
        assert_eq!(column_height(&chunk, HeightmapKind::WorldSurface, 0, 0), 2);

        chunk.set_block(0, MIN_Y + 1, 0, blocks::AIR);
        assert_eq!(column_height(&chunk, HeightmapKind::WorldSurface, 0, 0), 1);

        chunk.set_block(0, MIN_Y, 0, blocks::AIR);
        assert_eq!(column_height(&chunk, HeightmapKind::WorldSurface, 0, 0), 0);
    }

    #[test]
    fn leaves_count_for_every_heightmap_except_the_one_without_leaves() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        chunk.set_block(0, 5, 0, blocks::OAK_LEAVES);

        let height = |kind| column_height(&chunk, kind, 0, 0);
        // 世界底部是 -64，地面在 (0, 0, 0)，树叶在 (0, 5, 0)，高度从世界底部算。
        assert_eq!(height(HeightmapKind::WorldSurface), 70);
        assert_eq!(height(HeightmapKind::OceanFloor), 70);
        assert_eq!(height(HeightmapKind::MotionBlocking), 70);
        assert_eq!(height(HeightmapKind::MotionBlockingNoLeaves), 65);

        chunk.set_block(0, 5, 0, blocks::AIR);
        assert_eq!(
            column_height(&chunk, HeightmapKind::MotionBlocking, 0, 0),
            65
        );
    }

    #[test]
    fn water_counts_only_where_the_predicate_accepts_fluid() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        chunk.set_block(0, 5, 0, blocks::WATER);

        let height = |kind| column_height(&chunk, kind, 0, 0);
        // 水不是空气，所以是最高的非空气方块；它不在挡运动标签里，海洋地面不看它。
        assert_eq!(height(HeightmapKind::WorldSurface), 70);
        assert_eq!(height(HeightmapKind::OceanFloor), 65);
        assert_eq!(height(HeightmapKind::MotionBlocking), 70);
        assert_eq!(height(HeightmapKind::MotionBlockingNoLeaves), 70);
    }

    #[test]
    fn glass_counts_because_it_is_in_the_motion_tags() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        chunk.set_block(0, 5, 0, blocks::GLASS);

        let height = |kind| column_height(&chunk, kind, 0, 0);
        assert_eq!(height(HeightmapKind::WorldSurface), 70);
        assert_eq!(height(HeightmapKind::OceanFloor), 70);
        assert_eq!(height(HeightmapKind::MotionBlocking), 70);
        assert_eq!(height(HeightmapKind::MotionBlockingNoLeaves), 70);
    }

    #[test]
    fn short_grass_is_surface_but_not_motion_blocking() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::STONE);
        chunk.set_block(0, 5, 0, blocks::SHORT_GRASS);

        let height = |kind| column_height(&chunk, kind, 0, 0);
        assert_eq!(height(HeightmapKind::WorldSurface), 70);
        assert_eq!(height(HeightmapKind::OceanFloor), 65);
        assert_eq!(height(HeightmapKind::MotionBlocking), 65);
        assert_eq!(height(HeightmapKind::MotionBlockingNoLeaves), 65);
    }

    #[test]
    fn waterlogged_blocks_count_as_fluid() {
        let mut chunk = Chunk::empty(0);
        // 海草默认含水，但不在挡运动的标签里。
        chunk.set_block(0, MIN_Y, 0, blocks::SEAGRASS);
        assert_ne!(chunk.sections()[0].fluid_count(), 0);
        assert_eq!(
            column_height(&chunk, HeightmapKind::MotionBlocking, 0, 0),
            1
        );
        assert_eq!(column_height(&chunk, HeightmapKind::OceanFloor, 0, 0), 0);
    }

    #[test]
    fn section_header_counts_blocks_and_fluids() {
        let mut chunk = Chunk::empty(0);
        chunk.set_block(0, 0, 0, blocks::WATER);
        chunk.set_block(1, 0, 0, blocks::STONE);

        let section = &chunk.sections()[4];
        assert_eq!(section.block_count(), 2);
        assert_eq!(section.fluid_count(), 1);

        chunk.set_block(0, 0, 0, blocks::STONE);
        assert_eq!(chunk.sections()[4].block_count(), 2);
        assert_eq!(chunk.sections()[4].fluid_count(), 0);
    }

    #[test]
    fn generated_heightmap_types_have_a_kind() {
        let live: Vec<HeightmapKind> = heightmaps::TYPES
            .iter()
            .filter(|heightmap| heightmap.keep_after_worldgen)
            .filter_map(HeightmapKind::of_type)
            .collect();
        assert_eq!(live.len(), HeightmapKind::ALL.len());
        for kind in HeightmapKind::ALL {
            assert!(live.contains(&kind), "{kind:?}");
        }
        for heightmap in heightmaps::TYPES
            .iter()
            .filter(|heightmap| heightmap.send_to_client)
        {
            assert!(
                HeightmapKind::of_type(heightmap).is_some(),
                "{}",
                heightmap.name
            );
        }
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

    /// 保留条目跟着类型表：换个状态（不改方块）还在，换类型或拆掉方块就丢。
    #[test]
    fn preserved_block_entities_follow_the_type_table() {
        use mc_bamboo_data::registries::block_entity_type as kinds;
        use mc_bamboo_nbt::Tag;

        let tag = Tag::Compound(vec![("Unknown".to_owned(), Tag::Byte(3))]);
        let mut chunk = Chunk::empty(0);
        chunk.set_block(3, -60, 5, blocks::CHEST);
        chunk
            .put_preserved_block_entity(3, -60, 5, kinds::CHEST, tag.clone())
            .expect("位置合法");
        assert_eq!(chunk.preserved_block_entity(3, -60, 5), Some(&tag));
        // 同一方块的另一种状态（旋转等）不丢。
        chunk.set_block(3, -60, 5, blocks::CHEST + 1);
        assert_eq!(chunk.preserved_block_entity(3, -60, 5), Some(&tag));
        // 换成另一种方块实体：旧内容不能挂到熔炉上。
        chunk.set_block(3, -60, 5, blocks::FURNACE);
        assert!(chunk.preserved_block_entity(3, -60, 5).is_none());
        chunk
            .put_preserved_block_entity(3, -60, 5, kinds::FURNACE, tag.clone())
            .expect("位置合法");
        assert_eq!(chunk.preserved_block_entity(3, -60, 5), Some(&tag));
        chunk.set_block(3, -60, 5, blocks::AIR);
        assert!(chunk.preserved_block_entity(3, -60, 5).is_none());
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
