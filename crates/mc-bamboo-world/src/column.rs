//! 区块束：一个区块位置上这一层知道的全部内容。
//!
//! 区块束是唯一的搬运单位。方块和两层光照装在一个盒子里，加载、卸载、存盘、
//! region 合并拆分都是搬盒子，见 `docs/world-model-design.md` 第 4.2 节。
//! 方块实体的内容、计划刻这些在上层，以后由上层装进它自己的盒子。

use mc_bamboo_core::{BlockPos, LightLayer};

use crate::{
    chunk::{Chunk, HeightmapKind},
    light::{DataLayer, LightColumn, SectionPos},
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

    fn column(&self, layer: LightLayer) -> &LightColumn {
        match layer {
            LightLayer::Sky => &self.sky,
            LightLayer::Block => &self.block,
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
    use mc_bamboo_data::blocks;

    use crate::{
        chunk::HeightmapKind,
        light::{DataLayer, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos},
        store::ChunkStore,
    };

    use super::{ChunkColumn, LightPending};

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
        store.insert_column(CENTER, column);

        assert_same(&readings(&store), &before);
        assert_same(&shapes(&store), &shape);
        assert_same(&data_layers(&store), &data);
    }

    #[test]
    fn the_light_engine_still_works_after_a_round_trip() {
        let mut store = edited(true);
        let before = readings(&store);
        let without_lamp = readings(&edited(false));

        let column = take(&mut store, CENTER);
        store.insert_column(CENTER, column);

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
            b.insert_column(pos, take(&mut a, pos));
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
            b.insert_column(pos, take(&mut a, pos));
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

    /// 区块四个角都放光源，光会溢到八个邻居；整片（3×3）一起搬。
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
            b.insert_column(pos, take(&mut a, pos));
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

        store.insert_column(CENTER, column);
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
        b.insert_column(CENTER, take(&mut a, CENTER));

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

        store.insert_column(NEIGHBOR, column);
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
        store.insert_column(CENTER, column);
        assert!(store.take_light_changes().is_empty());

        // 插入以后再改方块，照旧能报出变化。
        store.set_block(LAMP, blocks::AIR);
        store.finish_light_updates();
        assert!(store.take_light_changes().contains_key(&CENTER));
    }
}
