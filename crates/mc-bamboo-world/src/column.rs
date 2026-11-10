//! 区块束：一个区块位置上这一层知道的全部内容。
//!
//! 区块束是唯一的搬运单位。方块和两层光照装在一个盒子里，加载、卸载、存盘、
//! region 合并拆分都是搬盒子，见 `docs/world-model-design.md` 第 4.2 节。
//! 方块实体的内容、计划刻这些在上层，以后由上层装进它自己的盒子。
//!
//! 搬区块束之前调用方要先用 [`ChunkStore::finish_light_updates`](super::store::ChunkStore::finish_light_updates)
//! 把光照跑完，也可以用 [`ChunkStore::ready_to_move_columns`](super::store::ChunkStore::ready_to_move_columns)
//! 先问一声；盒子里只装一份已经算稳的光照。

use mc_bamboo_core::{BlockPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{blocks, registries::biome};

use crate::{
    SECTION_COUNT,
    chunk::{Chunk, HeightmapKind, Section},
    light::{DataLayer, LIGHT_SECTION_COUNT, LightColumn, MIN_LIGHT_SECTION, SectionPos},
    palette::{PackedError, PalettedContainer},
};

/// 还有没算完的光照，不能搬区块束。
///
/// 调用方先把光照跑完（[`ChunkStore::finish_light_updates`](super::store::ChunkStore::finish_light_updates)）再取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightPending;

impl std::fmt::Display for LightPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("light updates are still pending")
    }
}

impl std::error::Error for LightPending {}

/// 磁盘上一段的方块和生物群系调色板形式。
///
/// `palette` 的下标就是 `data` 里存的下标；`data` 的位数由调色板长度按官方
/// `Strategy.getConfigurationForPaletteSize` 算出，单值段为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionParts {
    pub block_palette: Vec<u32>,
    pub block_data: Vec<u64>,
    pub biome_palette: Vec<u32>,
    pub biome_data: Vec<u64>,
}

/// 从磁盘数据构造一个区块束所需的全部内容。
///
/// 方块和群系都是调色板形式，见 [`SectionParts`]。所有值都当成不可信输入：
/// 段数、调色板长度、打包数组长度、状态 ID、坐标越界都会返回 [`ColumnPartsError`]。
#[derive(Debug, Clone)]
pub struct ChunkColumnParts {
    /// 24 个方块段，从世界最低段往上。
    pub sections: Vec<SectionParts>,
    /// 四种高度图，和 [`HeightmapKind::ALL`] 一一对应；`None` 时从方块重算。
    pub heightmaps: [Option<Vec<u64>>; HeightmapKind::ALL.len()],
    /// 26 个光照段，从 [`MIN_LIGHT_SECTION`] 往上，天空光在前、方块光在后。
    /// `None` 表示官方没存这一段的光照，和 `DataLayer::Filled(0)` 不同。
    pub light: [Vec<Option<DataLayer>>; 2],
    /// `isLightOn`。
    pub light_on: bool,
    /// 有内容的告示牌：区块内 x、世界 y、区块内 z 和内容。
    pub signs: Vec<(usize, i32, usize, SignContent)>,
    /// 方块实体要发给客户端的 NBT：区块内 x、世界 y、区块内 z 和网络字节。
    pub update_tags: Vec<(usize, i32, usize, Vec<u8>)>,
}

/// 磁盘数据不能组装成一个区块束。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ColumnPartsError {
    #[error("packed section data is invalid: {0}")]
    Packed(#[from] PackedError),
    #[error("expected {expected} sections, got {actual}")]
    WrongSectionCount { expected: usize, actual: usize },
    #[error("expected {expected} light sections, got {actual}")]
    WrongLightSectionCount { expected: usize, actual: usize },
    #[error("unknown block state id {0}")]
    UnknownState(u32),
    #[error("unknown biome id {0}")]
    UnknownBiome(u32),
    #[error("heightmap words cannot describe 256 columns")]
    BadHeightmap,
    #[error("position ({x}, {y}, {z}) is outside the chunk")]
    BadPosition { x: usize, y: i32, z: usize },
}

impl Default for ChunkColumnParts {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            heightmaps: std::array::from_fn(|_| None),
            light: std::array::from_fn(|_| vec![None; LIGHT_SECTION_COUNT]),
            light_on: false,
            signs: Vec::new(),
            update_tags: Vec::new(),
        }
    }
}

impl ChunkColumnParts {
    /// 从现有的区块束读出全部内容，字段和 [`ChunkColumn::from_parts`] 的输入一一对应。
    pub fn read_from(column: &ChunkColumn) -> Self {
        let sections = column
            .chunk
            .sections()
            .iter()
            .map(|section| {
                let (block_palette, block_data) = section.blocks().packed_for_disk();
                let (biome_palette, biome_data) = section.biomes().packed_for_disk();
                SectionParts {
                    block_palette,
                    block_data,
                    biome_palette,
                    biome_data,
                }
            })
            .collect();
        let heightmaps =
            std::array::from_fn(|index| Some(column.chunk.heightmap(HeightmapKind::ALL[index])));
        let light = std::array::from_fn(|layer_index| {
            let layer = if layer_index == 0 {
                LightLayer::Sky
            } else {
                LightLayer::Block
            };
            (0..LIGHT_SECTION_COUNT)
                .map(|slot| {
                    column
                        .section(
                            layer,
                            SectionPos::new(0, MIN_LIGHT_SECTION + slot as i32, 0),
                        )
                        .cloned()
                })
                .collect()
        });
        let signs = column
            .chunk
            .signs()
            .map(|(x, y, z, content)| (x, y, z, content.clone()))
            .collect();
        let update_tags = column
            .chunk
            .update_tags()
            .map(|(x, y, z, tag)| (x, y, z, tag.to_vec()))
            .collect();
        Self {
            sections,
            heightmaps,
            light,
            light_on: column.sky.light_on(),
            signs,
            update_tags,
        }
    }
}

/// 一个区块位置上这一层知道的全部内容。
///
/// 盒子里只有一份光照：取出只允许在光照没有算到一半的时候做，那时引擎正在算的那份和
/// 给别人读的那份内容相同。放进别的 [`ChunkStore`](super::store::ChunkStore) 时照原样放进它的两份。
#[derive(Debug, Clone)]
pub struct ChunkColumn {
    chunk: Chunk,
    /// 这一柱的天空光，含每段的状态和“光照已打开”。
    sky: LightColumn,
    /// 这一柱的方块光。
    block: LightColumn,
}

impl ChunkColumn {
    /// 从磁盘数据构造一个能被 [`ChunkStore::insert_column`](super::store::ChunkStore::insert_column)
    /// 接受的区块束。
    ///
    /// 段和光照按调色板 / 26 段数组直接建好，不逐格重放 `set_block`；段计数按官方
    /// `recalcBlockCounts` 的办法统计。高度图给了就校验后用，缺了就从方块重算。
    /// `initialize_light_sources` 在这里跑完，光照引擎之后能接着算。
    ///
    /// 光照的 `sectionStates` 只按「这一柱」重建：跨到相邻区块柱的邻居计数这里
    /// 看不到，放进 `ChunkStore` 时由 `insert_column` 按目标世界的方块重新登记。
    pub fn from_parts(parts: ChunkColumnParts) -> Result<Self, ColumnPartsError> {
        if parts.sections.len() != SECTION_COUNT {
            return Err(ColumnPartsError::WrongSectionCount {
                expected: SECTION_COUNT,
                actual: parts.sections.len(),
            });
        }
        for layer in &parts.light {
            if layer.len() != LIGHT_SECTION_COUNT {
                return Err(ColumnPartsError::WrongLightSectionCount {
                    expected: LIGHT_SECTION_COUNT,
                    actual: layer.len(),
                });
            }
        }
        let mut sections = Vec::with_capacity(SECTION_COUNT);
        for section in parts.sections {
            for &state in &section.block_palette {
                if state >= blocks::TOTAL_STATES {
                    return Err(ColumnPartsError::UnknownState(state));
                }
            }
            for &id in &section.biome_palette {
                if id >= biome::TOTAL {
                    return Err(ColumnPartsError::UnknownBiome(id));
                }
            }
            let blocks =
                PalettedContainer::from_blocks_disk(section.block_palette, section.block_data)?;
            let biomes =
                PalettedContainer::from_biomes_disk(section.biome_palette, section.biome_data)?;
            sections.push(Section::from_disk(blocks, biomes));
        }
        let mut chunk = Chunk::from_sections(sections, &parts.heightmaps)?;
        for (x, y, z, content) in parts.signs {
            chunk.put_sign(x, y, z, content)?;
        }
        for (x, y, z, tag) in parts.update_tags {
            chunk.put_update_tag(x, y, z, tag)?;
        }
        let mut has_blocks = [false; LIGHT_SECTION_COUNT];
        for (index, section) in chunk.sections().iter().enumerate() {
            has_blocks[index + 1] = !section.is_empty();
        }
        let columns = parts.light.map(|layers| {
            let array: [Option<DataLayer>; LIGHT_SECTION_COUNT] = layers
                .try_into()
                .unwrap_or_else(|_| unreachable!("上面已经校验过光照段数"));
            LightColumn::from_layers(array, &has_blocks, parts.light_on)
        });
        let [sky, block] = columns;
        Ok(Self { chunk, sky, block })
    }

    pub(super) fn new(chunk: Chunk, [sky, block]: [LightColumn; 2]) -> Self {
        Self { chunk, sky, block }
    }

    pub(super) fn into_parts(self) -> (Chunk, [LightColumn; 2]) {
        (self.chunk, [self.sky, self.block])
    }

    /// 这个区块的方块、高度图、方块实体类型、告示牌、update tag 和天空光源。
    pub fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// `getDataLayerData`：一个光照段存的数据，没存时是 `None`。
    /// `section` 是段坐标，其中 x、z 是这个区块的位置，y 以段为单位，范围见
    /// [`MIN_LIGHT_SECTION`](crate::light::MIN_LIGHT_SECTION)。
    pub fn section(&self, layer: LightLayer, section: SectionPos) -> Option<&DataLayer> {
        self.column(layer).layer(section.y)
    }

    /// `getLightValue`：这一柱里一格的天空光或者方块光。
    /// `pos` 是区块内坐标：x、z 在 0 到 15，y 是世界高度。
    /// 天空光在没存光照的段里读上面最近一段的最底一层。
    pub fn light_value(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        self.column(layer).light_value(
            layer,
            pos.y >> 4,
            (pos.x & 15) as usize,
            (pos.y & 15) as usize,
            (pos.z & 15) as usize,
        )
    }

    /// 一种高度图的 256 列，和 [`Chunk::heightmap`] 一样。
    pub fn heightmap(&self, kind: HeightmapKind) -> Vec<u64> {
        self.chunk.heightmap(kind)
    }

    /// `isLightOn`：这一柱的光照算完了没有，对应区块 NBT 里的 `isLightOn`。
    pub fn light_on(&self) -> bool {
        self.sky.light_on()
    }

    fn column(&self, layer: LightLayer) -> &LightColumn {
        match layer {
            LightLayer::Sky => &self.sky,
            LightLayer::Block => &self.block,
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
    use mc_bamboo_data::blocks;

    use crate::{
        SECTION_COUNT,
        chunk::HeightmapKind,
        light::{DataLayer, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos},
        store::ChunkStore,
    };

    use super::{ChunkColumn, ChunkColumnParts, ColumnPartsError, LightPending, SectionParts};

    /// 萤石放在 (0, 0) 的东边界，光溢到 (1, 0)。
    const LAMP: BlockPos = BlockPos::new(15, -60, 3);
    const CENTER: ChunkPos = ChunkPos::new(0, 0);
    const NEIGHBOR: ChunkPos = ChunkPos::new(1, 0);
    /// 一起搬的区块：发光方块所在的和所有被光溢到的。
    const AREA: [ChunkPos; 2] = [CENTER, NEIGHBOR];
    /// 逐格比较的范围，只覆盖搬走的这两个区块。
    const FROM: (i32, i32, i32) = (0, -62, 1);
    const TO: (i32, i32, i32) = (31, -56, 5);

    fn assert_is_send<T: Send>() {}

    /// 相邻区块里改方块，边界旁放发光方块，把光照跑完。
    fn edited(with_lamp: bool) -> ChunkStore {
        let mut store = ChunkStore::flat();
        store.set_block(BlockPos::new(3, -60, 3), blocks::STONE);
        store.set_block(BlockPos::new(20, -60, 4), blocks::OAK_SIGN);
        if with_lamp {
            store.set_block(LAMP, blocks::GLOWSTONE);
        }
        store.finish_light_updates();
        store
    }

    /// 这一片所有格子的方块状态、两层光照值和高度图，外加四种高度图。
    /// 长度固定，不受 `DataLayer` 有没有分配数组影响。
    fn readings(store: &ChunkStore) -> Vec<u8> {
        let mut values = Vec::new();
        for x in FROM.0..=TO.0 {
            for y in FROM.1..=TO.1 {
                for z in FROM.2..=TO.2 {
                    let pos = BlockPos::new(x, y, z);
                    values.extend(store.block(pos).unwrap().to_be_bytes());
                    values.push(store.light_value(LightLayer::Sky, pos));
                    values.push(store.light_value(LightLayer::Block, pos));
                    values.extend(store.heightmap_y(pos).unwrap().to_be_bytes());
                }
            }
        }
        for &pos in &AREA {
            for kind in HeightmapKind::ALL {
                values.extend(
                    store
                        .chunk(pos)
                        .heightmap(kind)
                        .into_iter()
                        .flat_map(u64::to_be_bytes),
                );
            }
        }
        values
    }

    /// 每一段在存储里的形状：没存光照、存了但没分配数组、分配了数组。
    /// 读出来一样，编出去的区块包却不同，所以往返时也要逐个相同。
    fn shapes(store: &ChunkStore) -> Vec<u8> {
        let mut values = Vec::new();
        for &pos in &AREA {
            for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
                let section = SectionPos::new(pos.x, y, pos.z);
                for layer in [LightLayer::Sky, LightLayer::Block] {
                    match store.light().section(layer, section) {
                        Some(DataLayer::Filled(value)) => {
                            values.push(1);
                            values.push(*value);
                        }
                        Some(DataLayer::Data(_)) => values.push(2),
                        None => values.push(0),
                    }
                }
            }
        }
        values
    }

    /// 这一片所有光照段的逐格数据，不管有没有分配数组。
    fn data_layers(store: &ChunkStore) -> Vec<u8> {
        let mut values = Vec::new();
        for &pos in &AREA {
            for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
                let section = SectionPos::new(pos.x, y, pos.z);
                for layer in [LightLayer::Sky, LightLayer::Block] {
                    match store.light().section(layer, section) {
                        Some(data) => values.extend(data.bytes().as_ref()),
                        None => values.extend([0; 2048]),
                    }
                }
            }
        }
        values
    }

    fn take(store: &mut ChunkStore, pos: ChunkPos) -> Box<ChunkColumn> {
        store.take_column(pos).expect("光照已经算完了")
    }

    fn insert(store: &mut ChunkStore, pos: ChunkPos, column: Box<ChunkColumn>) {
        store
            .insert_column(pos, column)
            .unwrap_or_else(|_| panic!("光照已经算完了"));
    }

    /// 逐字节比较，失败时报出第一处不同的下标，不打印整个大数组。
    fn assert_same(left: &[u8], right: &[u8]) {
        assert_eq!(left.len(), right.len(), "快照长度不同");
        if let Some(index) = left.iter().zip(right).position(|(a, b)| a != b) {
            panic!(
                "快照第 {index} 个字节不同：{} 和 {}",
                left[index], right[index]
            );
        }
    }

    fn assert_stores_match(left: &ChunkStore, right: &ChunkStore) {
        assert_same(&readings(left), &readings(right));
    }

    #[test]
    fn chunk_column_is_send() {
        assert_is_send::<ChunkColumn>();
        assert_is_send::<Box<ChunkColumn>>();
        assert_is_send::<LightPending>();
    }

    /// 公开的查询让调用方可以先问再决定要不要跑光照。
    #[test]
    fn the_public_query_tracks_when_columns_can_move() {
        let mut store = ChunkStore::flat();
        assert!(store.ready_to_move_columns());
        store.set_block(LAMP, blocks::GLOWSTONE);
        assert!(!store.ready_to_move_columns());
        store.finish_light_updates();
        assert!(store.ready_to_move_columns());
    }

    #[test]
    fn a_column_reads_what_the_store_read() {
        let mut source = edited(true);
        let column = take(&mut source, CENTER);
        let store = edited(true);

        assert_eq!(column.chunk().block(3, -60, 3), blocks::STONE);
        assert_eq!(column.chunk().block(15, -60, 3), blocks::GLOWSTONE);
        for kind in HeightmapKind::ALL {
            assert_eq!(column.heightmap(kind), store.chunk(CENTER).heightmap(kind));
        }
        for x in 0..16 {
            for y in [-64, -61, -60, -59, 0, 319] {
                for z in 0..16 {
                    let pos = BlockPos::new(x, y, z);
                    for layer in [LightLayer::Sky, LightLayer::Block] {
                        assert_eq!(
                            column.light_value(layer, pos),
                            store.light_value(layer, pos)
                        );
                    }
                }
            }
        }
        for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
            let section = SectionPos::new(0, y, 0);
            for layer in [LightLayer::Sky, LightLayer::Block] {
                assert_eq!(
                    column.section(layer, section),
                    store.light().section(layer, section)
                );
            }
        }
    }

    #[test]
    fn taking_and_inserting_preserves_every_reading() {
        let mut store = edited(true);
        let before = readings(&store);
        let shape = shapes(&store);
        let data = data_layers(&store);

        let column = take(&mut store, CENTER);
        insert(&mut store, CENTER, column);

        assert_same(&readings(&store), &before);
        assert_same(&shapes(&store), &shape);
        assert_same(&data_layers(&store), &data);
    }

    /// 单独把一柱插进一圈**不同的邻居**中间，之后跨区块边界的光照传播要和
    /// 「从没搬过」的世界逐格一致。这是区块平面（卸载/重新加载）的前提：
    /// 盒子自带的邻居计数是来源世界的，`insert_column` 必须按目标世界重新登记。
    #[test]
    fn one_column_among_different_neighbors_keeps_border_light() {
        // 来源：中心柱里放一个跨边界的发光方块，光会溢到邻居柱。
        let mut source = ChunkStore::flat();
        source.set_block(LAMP, blocks::GLOWSTONE);
        source.set_block(BlockPos::new(3, -60, 3), blocks::STONE);
        source.finish_light_updates();
        let column = take(&mut source, CENTER);

        // 目标：邻居柱里事先有别的内容（高处的屋顶会挡天空光、方块会挡光）。
        // 这些内容和来源世界完全不同，来源的段计数不能照搬。
        let mut target = ChunkStore::flat();
        for x in 16..32 {
            target.set_block(BlockPos::new(x, 100, 3), blocks::STONE);
        }
        target.set_block(BlockPos::new(20, -60, 4), blocks::OAK_SIGN);
        target.finish_light_updates();
        insert(&mut target, CENTER, column);

        // 对照：同样的世界，从头直接把中心柱的内容放进去。
        let mut direct = ChunkStore::flat();
        for x in 16..32 {
            direct.set_block(BlockPos::new(x, 100, 3), blocks::STONE);
        }
        direct.set_block(BlockPos::new(20, -60, 4), blocks::OAK_SIGN);
        direct.set_block(LAMP, blocks::GLOWSTONE);
        direct.set_block(BlockPos::new(3, -60, 3), blocks::STONE);
        direct.finish_light_updates();

        // 段状态按目标世界重算过：中心与 8 邻居逐项和「从没搬过」一致。
        // 盒子自带的来源世界计数不能整柱照搬，否则边界段的存废会错。
        for dx in -1..=1 {
            for dz in -1..=1 {
                let pos = ChunkPos::new(CENTER.x + dx, CENTER.z + dz);
                assert_eq!(
                    target.light().section_states(pos),
                    direct.light().section_states(pos),
                    "段状态不同：{pos:?}"
                );
            }
        }

        // 之后在边界上做一轮「拆掉再放回」，让光照重新跨区块边界传播。
        // 段状态登记对了，两边就收敛到同一个结果，且之后继续加遮挡也一样。
        for (lamp, roof) in [
            (blocks::AIR, false),
            (blocks::GLOWSTONE, false),
            (blocks::GLOWSTONE, true),
            (blocks::AIR, true),
        ] {
            target.set_block(LAMP, lamp);
            direct.set_block(LAMP, lamp);
            if roof {
                target.set_block(BlockPos::new(3, -59, 3), blocks::STONE);
                direct.set_block(BlockPos::new(3, -59, 3), blocks::STONE);
            }
            target.finish_light_updates();
            direct.finish_light_updates();
            if lamp == blocks::GLOWSTONE && !roof {
                // 从这一轮起，跨边界的光已经重新传播过。
                assert_stores_match(&target, &direct);
            }
        }
        assert_stores_match(&target, &direct);
    }

    #[test]
    fn the_light_engine_still_works_after_a_round_trip() {
        let mut store = edited(true);
        let before = readings(&store);
        let without_lamp = readings(&edited(false));

        let column = take(&mut store, CENTER);
        insert(&mut store, CENTER, column);

        // 拆掉发光方块，光照回到没有它时的值；再放回去，回到第一步的值。
        store.set_block(LAMP, blocks::AIR);
        store.finish_light_updates();
        assert_same(&readings(&store), &without_lamp);

        store.set_block(LAMP, blocks::GLOWSTONE);
        store.finish_light_updates();
        assert_same(&readings(&store), &before);
        assert_stores_match(&store, &edited(true));
    }

    #[test]
    fn columns_can_move_between_stores() {
        let mut a = edited(true);
        let before = readings(&a);
        let shape = shapes(&a);
        let data = data_layers(&a);
        let mut b = ChunkStore::flat();

        // 发光方块所在的区块和所有被光溢到的邻居一起搬。
        for &pos in &AREA {
            insert(&mut b, pos, take(&mut a, pos));
        }
        assert_same(&readings(&b), &before);
        assert_same(&shapes(&b), &shape);
        assert_same(&data_layers(&b), &data);

        // 在 B 里拆掉发光方块，结果和在 A 里直接拆一样。
        let mut direct = edited(true);
        direct.set_block(LAMP, blocks::AIR);
        direct.finish_light_updates();
        b.set_block(LAMP, blocks::AIR);
        b.finish_light_updates();
        assert_stores_match(&b, &direct);

        // 放回去也和 A 里一样。
        direct.set_block(LAMP, blocks::GLOWSTONE);
        direct.finish_light_updates();
        b.set_block(LAMP, blocks::GLOWSTONE);
        b.finish_light_updates();
        assert_stores_match(&b, &direct);
    }

    /// 天上新建区块段时，`lowest_section` 和邻居段的登记簿也要跟着搬。
    #[test]
    fn tall_structures_move_between_stores() {
        let mut a = ChunkStore::flat();
        for x in 14..18 {
            for z in 2..6 {
                a.set_block(BlockPos::new(x, 200, z), blocks::STONE);
            }
        }
        a.set_block(BlockPos::new(15, 199, 3), blocks::GLOWSTONE);
        a.finish_light_updates();
        let before = readings(&a);
        let mut b = ChunkStore::flat();

        for &pos in &AREA {
            insert(&mut b, pos, take(&mut a, pos));
        }
        // 和 A 里取出前的读数逐个相同；B 里接着拆、接着放也一样。
        let mut direct = ChunkStore::flat();
        for x in 14..18 {
            for z in 2..6 {
                direct.set_block(BlockPos::new(x, 200, z), blocks::STONE);
            }
        }
        direct.set_block(BlockPos::new(15, 199, 3), blocks::GLOWSTONE);
        direct.finish_light_updates();
        assert_same(&readings(&b), &before);
        assert_stores_match(&b, &direct);

        b.set_block(BlockPos::new(15, 199, 3), blocks::AIR);
        b.finish_light_updates();
        direct.set_block(BlockPos::new(15, 199, 3), blocks::AIR);
        direct.finish_light_updates();
        assert_stores_match(&b, &direct);
    }

    /// 一整片互相相邻的区块（3×3）一起搬：钉住 `sectionStates` 里邻居段计数的适用范围。
    /// 四个角都放光源，光会溢到八个邻居，所有互相记了计数的柱都在这一片里。
    #[test]
    fn a_three_by_three_area_moves_between_stores() {
        let mut a = ChunkStore::flat();
        for &(x, z) in &[(0, 0), (0, 15), (15, 0), (15, 15)] {
            a.set_block(BlockPos::new(x, -60, z), blocks::GLOWSTONE);
        }
        a.finish_light_updates();
        let area: Vec<ChunkPos> = (-1..=1)
            .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
            .collect();
        let readings_before = readings(&a);
        let mut b = ChunkStore::flat();
        for &pos in &area {
            insert(&mut b, pos, take(&mut a, pos));
        }
        // 中间两列的读数和搬之前一样。
        assert_same(&readings(&b), &readings_before);

        // 之后继续改，和没搬过的世界一样。
        let mut direct = ChunkStore::flat();
        for &(x, z) in &[(0, 0), (0, 15), (15, 0), (15, 15)] {
            direct.set_block(BlockPos::new(x, -60, z), blocks::GLOWSTONE);
        }
        direct.finish_light_updates();
        assert_stores_match(&b, &direct);

        for &(x, z) in &[(0, 0), (0, 15), (15, 0), (15, 15)] {
            b.set_block(BlockPos::new(x, -60, z), blocks::AIR);
            direct.set_block(BlockPos::new(x, -60, z), blocks::AIR);
        }
        b.finish_light_updates();
        direct.finish_light_updates();
        assert_stores_match(&b, &direct);
    }

    #[test]
    fn pending_light_blocks_taking_a_column() {
        let mut store = ChunkStore::flat();
        store.set_block(LAMP, blocks::GLOWSTONE);
        assert!(store.has_pending_light_updates());
        let before = store.revision(CENTER);
        let initial = readings(&store);

        assert!(matches!(store.take_column(CENTER), Err(LightPending)));
        assert_eq!(store.revision(CENTER), before);
        assert_same(&readings(&store), &initial);

        store.finish_light_updates();
        assert!(store.take_column(CENTER).is_ok());
    }

    /// 光照没算完时放入返回 `Err`，盒子原样还回来，store 一点没变；跑完再放入成功。
    #[test]
    fn pending_light_blocks_inserting_and_returns_the_box() {
        let mut source = edited(true);
        let expected = readings(&source);
        let column = take(&mut source, CENTER);

        let mut target = ChunkStore::flat();
        target.set_block(BlockPos::new(3, -60, 3), blocks::STONE);
        assert!(target.has_pending_light_updates());
        assert!(!target.ready_to_move_columns());
        let before = target.revision(CENTER);
        let initial = readings(&target);

        let Err((LightPending, returned)) = target.insert_column(CENTER, column) else {
            panic!("光照没算完时应该拒绝放入");
        };
        assert_eq!(target.revision(CENTER), before);
        assert_same(&readings(&target), &initial);

        // 拿回来的盒子和传进去的是同一份内容：放回已经算完的 source 里逐格比。
        insert(&mut source, CENTER, returned);
        assert_same(&readings(&source), &expected);
        assert_stores_match(&source, &edited(true));

        // 目标自己跑完光照以后，同一个盒子就能放进去了。
        target.finish_light_updates();
        assert!(target.ready_to_move_columns());
        assert!(
            target
                .insert_column(CENTER, take(&mut source, CENTER))
                .is_ok()
        );
    }

    #[test]
    fn a_taken_chunk_reads_the_template_and_revisions_are_fresh() {
        let mut store = edited(true);
        let mut seen = vec![store.revision(CENTER)];
        let column = take(&mut store, CENTER);
        seen.push(store.revision(CENTER));

        assert!(store.is_template(CENTER));
        let plain = ChunkStore::flat();
        for x in 0..16usize {
            for z in 0..16usize {
                for y in [-64, -61, -60, -59, 0, 319] {
                    let pos = BlockPos::new(x as i32, y, z as i32);
                    assert_eq!(store.block(pos), Some(plain.template().block(x, y, z)));
                    assert_eq!(store.heightmap_y(pos), plain.heightmap_y(pos));
                    for layer in [LightLayer::Sky, LightLayer::Block] {
                        assert_eq!(store.light_value(layer, pos), plain.light_value(layer, pos));
                    }
                }
            }
        }
        for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
            let section = SectionPos::new(0, y, 0);
            for layer in [LightLayer::Sky, LightLayer::Block] {
                assert_eq!(
                    store.light().section(layer, section),
                    plain.light().section(layer, section)
                );
            }
        }

        insert(&mut store, CENTER, column);
        seen.push(store.revision(CENTER));
        // 取出和放入各换一次版本号，三个值互不相同；方块和两层光照都换了。
        for (index, revision) in seen.iter().enumerate() {
            for other in &seen[..index] {
                assert_ne!(revision, other);
            }
        }
    }

    #[test]
    fn inserting_uses_the_target_stores_own_revision_counters() {
        let mut a = edited(true);
        // A 里已经发出去过不少版本号。
        let before = a.revision(CENTER);
        let mut b = ChunkStore::flat();
        let fresh = b.revision(CENTER);
        insert(&mut b, CENTER, take(&mut a, CENTER));

        let inserted = b.revision(CENTER);
        assert_ne!(inserted, fresh);
        // B 从没写过这个位置，所以它的计数器从 1 开始，而不是沿用 A 里的值。
        assert_ne!(inserted, before);
        assert_eq!(inserted, b.revision(CENTER));
    }

    #[test]
    fn untouched_chunks_can_be_taken_and_put_back() {
        let mut store = ChunkStore::flat();
        let before = readings(&store);
        let column = take(&mut store, NEIGHBOR);
        assert_eq!(column.chunk().block(0, -61, 0), blocks::GRASS_BLOCK);
        assert!(store.is_template(NEIGHBOR));

        insert(&mut store, NEIGHBOR, column);
        assert_same(&readings(&store), &before);
    }

    /// 一轮算完但还有其他轮次排着时改方块，`forming.blocks` 和 `past` 会先留着改动。
    /// 全部算完以后这些登记簿也要清空，`take_column` 才能安全地搬。
    /// 一轮停在半路时（引擎队列里还有活）也必须拒绝，哪怕 `has_pending_light_updates` 为假。
    #[test]
    fn a_half_finished_round_blocks_taking_a_column() {
        let mut store = ChunkStore::flat();
        store.set_block(LAMP, blocks::GLOWSTONE);
        store.run_light_updates(1);
        assert!(store.has_pending_light_updates());
        let initial = readings(&store);
        let revision = store.revision(CENTER);

        assert!(matches!(store.take_column(CENTER), Err(LightPending)));
        assert_eq!(store.revision(CENTER), revision);
        assert_same(&readings(&store), &initial);

        store.finish_light_updates();
        assert!(store.take_column(CENTER).is_ok());
    }

    #[test]
    fn a_column_can_be_taken_after_several_overlapping_rounds() {
        let mut store = ChunkStore::flat();
        // 1000 个任务排成第一轮，剩下第二轮。
        for x in 0..1000 {
            store.set_block(BlockPos::new(x + 100, -60, 0), blocks::STONE);
        }
        store.set_block(LAMP, blocks::GLOWSTONE);
        store.run_light_updates(1);
        assert!(store.has_pending_light_updates());
        // 第一轮还没算完时改方块，会留 past 和 forming.blocks。
        store.set_block(LAMP, blocks::AIR);
        store.finish_light_updates();

        assert!(store.take_column(CENTER).is_ok());
        // 搬走以后引擎照旧能用。
        store.set_block(LAMP, blocks::GLOWSTONE);
        store.finish_light_updates();
        assert_eq!(store.light_value(LightLayer::Block, LAMP), 15);
    }

    #[test]
    fn taking_discards_untaken_light_changes_but_inserting_makes_none() {
        let mut store = edited(true);
        assert!(store.take_light_changes().contains_key(&CENTER));

        let column = take(&mut store, CENTER);
        assert!(!store.take_light_changes().contains_key(&CENTER));
        insert(&mut store, CENTER, column);
        assert!(store.take_light_changes().is_empty());

        // 插入以后再改方块，照旧能报出变化。
        store.set_block(LAMP, blocks::AIR);
        store.finish_light_updates();
        assert!(store.take_light_changes().contains_key(&CENTER));
    }

    /// 读盘路径：调色板形式的内容组装成区块束，方块、高度图、方块实体、告示牌、
    /// 两层光照和光照开关逐项相同；放进新的 `ChunkStore` 后读数也相同。
    #[test]
    fn parts_round_trip_into_a_fresh_store() {
        let mut source = edited(true);
        let column = take(&mut source, CENTER);
        let parts = ChunkColumnParts::read_from(&column);
        assert_eq!(parts.sections.len(), SECTION_COUNT);
        assert_eq!(parts.light[0].len(), LIGHT_SECTION_COUNT);
        assert_eq!(parts.light_on, column.light_on());

        let rebuilt = ChunkColumn::from_parts(parts).expect("组装成功");
        assert_eq!(rebuilt.light_on(), column.light_on());
        for kind in HeightmapKind::ALL {
            assert_eq!(rebuilt.heightmap(kind), column.heightmap(kind));
        }
        for &(x, y, z) in &[(3, -60, 3), (15, -60, 3), (20, -60, 4)] {
            assert_eq!(
                rebuilt.chunk().block(x, y, z),
                column.chunk().block(x, y, z)
            );
        }
        assert_eq!(
            rebuilt.chunk().block_entities().collect::<Vec<_>>(),
            column.chunk().block_entities().collect::<Vec<_>>()
        );
        for x in 0..16 {
            for y in [-64, -61, -60, -59, 0, 319] {
                for z in 0..16 {
                    let pos = BlockPos::new(x, y, z);
                    for layer in [LightLayer::Sky, LightLayer::Block] {
                        assert_eq!(
                            rebuilt.light_value(layer, pos),
                            column.light_value(layer, pos),
                            "{layer:?} at {pos:?}"
                        );
                    }
                }
            }
        }
        for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
            let section = SectionPos::new(0, y, 0);
            for layer in [LightLayer::Sky, LightLayer::Block] {
                assert_eq!(
                    rebuilt.section(layer, section),
                    column.section(layer, section)
                );
            }
        }

        // 放进新建的 ChunkStore 后，方块和高度图能按世界坐标读到。
        let mut target = ChunkStore::flat();
        insert(&mut target, CENTER, Box::new(rebuilt));
        let source2 = edited(true);
        for x in 0..16 {
            for y in -64..=-59 {
                for z in 0..16 {
                    let pos = BlockPos::new(x, y, z);
                    assert_eq!(target.block(pos), source2.block(pos));
                    assert_eq!(target.heightmap_y(pos), source2.heightmap_y(pos));
                }
            }
        }
        assert_eq!(target.block(LAMP), Some(blocks::GLOWSTONE));
    }

    /// 磁盘数据一律不可信：段数、调色板、long 数组、状态 ID、坐标错了都要报错。
    #[test]
    fn from_parts_rejects_bad_disk_data() {
        let mut store = ChunkStore::flat();
        let source = take(&mut store, CENTER);
        let good = ChunkColumnParts::read_from(&source);
        assert!(ChunkColumn::from_parts(good.clone()).is_ok());

        let mut wrong = good.clone();
        wrong.sections.pop();
        assert!(matches!(
            ChunkColumn::from_parts(wrong),
            Err(ColumnPartsError::WrongSectionCount { .. })
        ));

        let mut wrong = good.clone();
        wrong.light[0].pop();
        assert!(matches!(
            ChunkColumn::from_parts(wrong),
            Err(ColumnPartsError::WrongLightSectionCount { .. })
        ));

        let mut wrong = good.clone();
        wrong.sections[0].block_palette.clear();
        assert!(ChunkColumn::from_parts(wrong).is_err());

        let mut wrong = good.clone();
        wrong.sections[0].block_data.clear();
        assert!(ChunkColumn::from_parts(wrong).is_err());

        let mut wrong = good.clone();
        wrong.sections[0].block_palette[0] = u32::MAX;
        assert!(matches!(
            ChunkColumn::from_parts(wrong),
            Err(ColumnPartsError::UnknownState(u32::MAX))
        ));

        let mut wrong = good.clone();
        wrong.sections[0].biome_palette[0] = u32::MAX;
        assert!(matches!(
            ChunkColumn::from_parts(wrong),
            Err(ColumnPartsError::UnknownBiome(u32::MAX))
        ));

        let mut wrong = good.clone();
        wrong.heightmaps[0] = Some(Vec::new());
        assert!(matches!(
            ChunkColumn::from_parts(wrong),
            Err(ColumnPartsError::BadHeightmap)
        ));

        let mut wrong = good.clone();
        wrong.signs.push((99, -60, 0, SignContent::BLANK));
        assert!(ChunkColumn::from_parts(wrong).is_err());
    }

    /// 模糊：随机的调色板和 long 数组不能让构造过程 panic。
    #[test]
    fn fuzzed_parts_never_panic() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..200 {
            let sections = (0..SECTION_COUNT)
                .map(|_| {
                    let palette_len = (next() % 6) as usize + 1;
                    let palette: Vec<u32> =
                        (0..palette_len).map(|_| (next() % 200) as u32).collect();
                    let words: Vec<u64> = (0..(next() % 40) as usize).map(|_| next()).collect();
                    SectionParts {
                        block_palette: palette.clone(),
                        block_data: words.clone(),
                        biome_palette: palette,
                        biome_data: words,
                    }
                })
                .collect();
            let _ = ChunkColumn::from_parts(ChunkColumnParts {
                sections,
                ..Default::default()
            });
        }
    }
}
