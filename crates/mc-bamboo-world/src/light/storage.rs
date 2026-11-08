//! 一层光照按区块段存放，官方的 `LayerLightSectionStorage`、`SkyLightSectionStorage` 和 `BlockLightSectionStorage`。
//!
//! 一个区块段自己有方块，或者 26 个邻居里有一个有方块，才存光照；两种都不是的段读出来是默认值。
//! 官方分“正在算”和“给别人看”两份数据，算完一轮才把变过的段换过去。这里也一样：引擎改的是自己那一份，
//! 记下变过的段，一轮算完由 [`publish`](LightStorage::publish) 抄进给别人读的那一份。
//! 一轮可以分几个 tick 算完，算到一半时别人读到的还是上一轮的结果。
//!
//! 超平坦世界无穷大，所以每个区块柱默认是模板柱的样子，第一次写的时候才复制一份自己的。

use std::{
    collections::{BTreeSet, HashMap},
    mem,
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};

use super::{DataLayer, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos};

/// `SectionState` 里“这一段自己有方块”的位。
const HAS_DATA: u8 = 32;
/// `SectionState` 的低 5 位：有方块的邻居段个数。
const NEIGHBOR_COUNT: u8 = 31;
/// 一个段最多有这么多个邻居。
const MAX_NEIGHBORS: i32 = 26;
const FULL_LIGHT: u8 = 15;

/// 一个区块柱 26 个光照段的存储。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LightColumn {
    layers: [Option<DataLayer>; LIGHT_SECTION_COUNT],
    /// `sectionStates`：0 表示这一段不存光照。
    states: [u8; LIGHT_SECTION_COUNT],
    /// `columnsWithSources`：这一列打开了光照。
    light_on: bool,
    /// 最后一次写这一柱时拿到的版本号，见 [`LightStorage::revision`]。
    revision: u64,
    /// 哪几段已经记进 [`LightStorage::changed`]，第 n 位是第 n 段。
    changed: u32,
}

impl LightColumn {
    /// 什么都没存，光照也没打开。
    pub(crate) const fn empty() -> Self {
        Self {
            layers: [const { None }; LIGHT_SECTION_COUNT],
            states: [0; LIGHT_SECTION_COUNT],
            light_on: false,
            revision: 0,
            changed: 0,
        }
    }

    /// 同一份内容，光照打开。模板柱用它：世界里每个区块都已经算过光照了。
    pub(crate) fn lit(mut self) -> Self {
        self.light_on = true;
        self
    }
}

/// 一层光照的全部区块段。
#[derive(Debug, Clone)]
pub(crate) struct LightStorage {
    layer: LightLayer,
    /// 没有自己一份的区块柱都是这个样子。
    template: LightColumn,
    columns: HashMap<ChunkPos, LightColumn>,
    /// `currentLowestY`：存过光照的最低区块段，只降不升。
    lowest_section: i32,
    /// `toRemove`：这一轮结束时要丢掉的段。
    to_remove: BTreeSet<SectionPos>,
    /// `sectionsAffectedByLightUpdates`：这一轮写过的段和它们的邻居。
    affected: BTreeSet<SectionPos>,
    /// `changedSections`：上次 [`publish`](Self::publish) 以后数据变过的段，写过、新建、丢掉的都算。
    changed: Vec<SectionPos>,
    has_inconsistencies: bool,
    /// 发出去的最后一个版本号。
    last_revision: u64,
}

/// 光照段在区块柱里的下标。超出 26 段时没有。
fn slot(y: i32) -> Option<usize> {
    usize::try_from(y - MIN_LIGHT_SECTION)
        .ok()
        .filter(|&slot| slot < LIGHT_SECTION_COUNT)
}

/// 方块在区块段里的坐标。
fn local(pos: BlockPos) -> (usize, usize, usize) {
    (
        (pos.x & 15) as usize,
        (pos.y & 15) as usize,
        (pos.z & 15) as usize,
    )
}

impl LightStorage {
    /// `template` 是所有没写过的区块柱的样子，`lowest_section` 是它们存过的最低段。
    pub(crate) fn new(layer: LightLayer, template: LightColumn, lowest_section: i32) -> Self {
        Self {
            layer,
            // 模板柱的版本号是 0，写过的柱从 1 往上数，两者不会撞上。
            template: LightColumn {
                revision: 0,
                ..template
            },
            columns: HashMap::new(),
            lowest_section,
            to_remove: BTreeSet::new(),
            affected: BTreeSet::new(),
            changed: Vec::new(),
            has_inconsistencies: false,
            last_revision: 0,
        }
    }

    pub(crate) fn layer_kind(&self) -> LightLayer {
        self.layer
    }

    fn column(&self, pos: ChunkPos) -> &LightColumn {
        self.columns.get(&pos).unwrap_or(&self.template)
    }

    /// 要写这个区块柱了，没有自己一份的话先从模板复制。所有写都经过这里，每次换一个新的版本号。
    fn column_mut(&mut self, pos: ChunkPos) -> &mut LightColumn {
        self.last_revision += 1;
        let revision = self.last_revision;
        let template = &self.template;
        let column = self.columns.entry(pos).or_insert_with(|| template.clone());
        column.revision = revision;
        column
    }

    /// 这个区块柱写过，内容可能和模板不同。
    pub(crate) fn is_materialized(&self, pos: ChunkPos) -> bool {
        self.columns.contains_key(&pos)
    }

    /// 这个区块柱的版本号。每写一次都换成没用过的新值，没写过的柱是 0，
    /// 所以两次读到的值相同就说明中间没写过。
    ///
    /// 不看受影响的段：一轮结束时丢掉的段不算受影响，区块包里却少了它们。
    /// 别人读的那一份只在 [`publish`](Self::publish) 时被写，所以它的版本号跟着发布走。
    pub(crate) fn revision(&self, pos: ChunkPos) -> u64 {
        self.column(pos).revision
    }

    /// 复制一个区块柱的光照，算模板的时候用。复制出来的不带“变过”的记号。
    pub(crate) fn column_at(&self, pos: ChunkPos) -> LightColumn {
        LightColumn {
            changed: 0,
            ..self.column(pos).clone()
        }
    }

    /// `getBottomSectionY`。
    pub(crate) fn lowest_section(&self) -> i32 {
        self.lowest_section
    }

    /// `getDataLayer`：这一段存的光照，没存时是 `None`。
    pub(crate) fn layer(&self, section: SectionPos) -> Option<&DataLayer> {
        self.column(section.chunk()).layers[slot(section.y)?].as_ref()
    }

    /// `storingLightForSection`。
    pub(crate) fn storing_light_for_section(&self, section: SectionPos) -> bool {
        self.layer(section).is_some()
    }

    /// `getStoredLevel`。调用方先确认这一段存着光照，官方在这里不检查。
    pub(crate) fn stored_level(&self, pos: BlockPos) -> u8 {
        let (x, y, z) = local(pos);
        self.layer(SectionPos::of(pos))
            .expect("只读存着光照的区块段")
            .get(x, y, z)
    }

    /// `setStoredLevel`：写一格，记下受影响的段。
    pub(crate) fn set_stored_level(&mut self, pos: BlockPos, level: u8) {
        let (x, y, z) = local(pos);
        self.layer_to_write(SectionPos::of(pos))
            .expect("只写存着光照的区块段")
            .set(x, y, z, level);
        self.mark_affected_around(pos);
    }

    /// `getDataLayerToWrite`：直接改整段，不记受影响的段。
    pub(crate) fn layer_to_write(&mut self, section: SectionPos) -> Option<&mut DataLayer> {
        if !self.storing_light_for_section(section) {
            return None;
        }
        self.slot_to_write(section)?.as_mut()
    }

    /// 要改这一段的数据了：没有自己一份的区块柱先从模板复制，这一段记进 [`changed`](Self::changed)。
    /// 和 [`column_mut`](Self::column_mut) 一样换一个新的版本号；这里要同时改 `changed`，所以没法直接调它。
    fn slot_to_write(&mut self, section: SectionPos) -> Option<&mut Option<DataLayer>> {
        let slot = slot(section.y)?;
        self.last_revision += 1;
        let template = &self.template;
        let column = self
            .columns
            .entry(section.chunk())
            .or_insert_with(|| template.clone());
        column.revision = self.last_revision;
        if column.changed & (1 << slot) == 0 {
            column.changed |= 1 << slot;
            self.changed.push(section);
        }
        Some(&mut column.layers[slot])
    }

    /// `swapSectionMap` 的前半：把上次以后变过的段抄进 `visible`，别人从那里读光照。
    pub(crate) fn publish(&mut self, visible: &mut LightStorage) {
        for section in mem::take(&mut self.changed) {
            let slot = slot(section.y).expect("记下的段都在光照的高度范围里");
            let column = self
                .columns
                .get_mut(&section.chunk())
                .expect("记下的段所在的区块柱已经有了自己的一份");
            column.changed &= !(1 << slot);
            visible.column_mut(section.chunk()).layers[slot] = column.layers[slot].clone();
        }
        visible.lowest_section = self.lowest_section;
    }

    /// `lightOnInSection`。
    pub(crate) fn light_on(&self, pos: ChunkPos) -> bool {
        self.column(pos).light_on
    }

    /// `setLightEnabled`。
    pub(crate) fn set_light_enabled(&mut self, pos: ChunkPos, enable: bool) {
        if self.light_on(pos) != enable {
            self.column_mut(pos).light_on = enable;
        }
    }

    fn state(&self, section: SectionPos) -> u8 {
        slot(section.y).map_or(0, |slot| self.column(section.chunk()).states[slot])
    }

    /// `updateSectionStatus`：这一段变空了或者有了方块，它和 26 个邻居存不存光照可能跟着变。
    pub(crate) fn update_section_status(&mut self, section: SectionPos, empty: bool) {
        let state = self.state(section);
        let updated = if empty {
            state & !HAS_DATA
        } else {
            state | HAS_DATA
        };
        if state == updated {
            return;
        }
        self.put_section_state(section, updated);
        let increment = if empty { -1 } else { 1 };
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if (dx, dy, dz) != (0, 0, 0) {
                        self.add_neighbor_count(section.offset(dx, dy, dz), increment);
                    }
                }
            }
        }
    }

    fn add_neighbor_count(&mut self, section: SectionPos, increment: i32) {
        let state = self.state(section);
        let count = i32::from(state & NEIGHBOR_COUNT) + increment;
        assert!(
            (0..=MAX_NEIGHBORS).contains(&count),
            "邻居数 {count} 超出了 0 到 26"
        );
        self.put_section_state(section, state & !NEIGHBOR_COUNT | count as u8);
    }

    /// `putSectionState`：从 0 变成非 0 时开始存光照，变回 0 时这一轮结束后丢掉。
    fn put_section_state(&mut self, section: SectionPos, state: u8) {
        let slot = slot(section.y).expect("只有世界上下各多一段的范围里才有光照");
        let previous = self.state(section);
        if previous == state {
            return;
        }
        self.column_mut(section.chunk()).states[slot] = state;
        if previous == 0 {
            self.initialize_section(section);
        } else if state == 0 {
            self.to_remove.insert(section);
            self.has_inconsistencies = true;
        }
    }

    /// `initializeSection`。这一轮刚要丢掉的段不用重新建，原来的数据还在。
    fn initialize_section(&mut self, section: SectionPos) {
        if self.to_remove.remove(&section) {
            return;
        }
        let layer = self.create_data_layer(section);
        *self
            .slot_to_write(section)
            .expect("只有世界上下各多一段的范围里才有光照") = Some(layer);
        self.lowest_section = self.lowest_section.min(section.y);
        self.mark_section_and_neighbors_as_affected(section);
        self.has_inconsistencies = true;
    }

    /// `createDataLayer`。方块光从 0 开始。天空光下面有段时照抄上面那段的最底一层，
    /// 不然看这一列开没开光照：开了是 15，没开是 0。
    fn create_data_layer(&self, section: SectionPos) -> DataLayer {
        if self.layer == LightLayer::Block {
            return DataLayer::EMPTY;
        }
        let top = self.top_section_y(section.chunk());
        if top != self.lowest_section && section.y < top {
            let mut above = section.above();
            loop {
                if let Some(layer) = self.layer(above) {
                    return layer.repeat_first_layer();
                }
                above = above.above();
            }
        }
        if self.light_on(section.chunk()) {
            DataLayer::Filled(FULL_LIGHT)
        } else {
            DataLayer::EMPTY
        }
    }

    pub(crate) fn has_inconsistencies(&self) -> bool {
        self.has_inconsistencies
    }

    /// `markNewInconsistencies`：一轮结束时丢掉不再存光照的段。
    pub(crate) fn mark_new_inconsistencies(&mut self) {
        if !mem::take(&mut self.has_inconsistencies) {
            return;
        }
        for section in mem::take(&mut self.to_remove) {
            if let Some(slot) = self.slot_to_write(section) {
                *slot = None;
            }
        }
    }

    /// 这一轮写过的段，取走以后清空。
    pub(crate) fn take_affected(&mut self) -> BTreeSet<SectionPos> {
        mem::take(&mut self.affected)
    }

    /// `SectionPos.aroundAndAtBlockPos`：这一格和它 6 个邻居所在的段。
    fn mark_affected_around(&mut self, pos: BlockPos) {
        let range = |value: i32| ((value - 1) >> 4)..=((value + 1) >> 4);
        for x in range(pos.x) {
            for y in range(pos.y) {
                for z in range(pos.z) {
                    self.affected.insert(SectionPos::new(x, y, z));
                }
            }
        }
    }

    /// `markSectionAndNeighborsAsAffected`。
    fn mark_section_and_neighbors_as_affected(&mut self, section: SectionPos) {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    self.affected.insert(section.offset(dx, dy, dz));
                }
            }
        }
    }

    /// `getTopSectionY`：这一列最高的存光照的段再往上一段。一段都没有时是 [`lowest_section`](Self::lowest_section)。
    pub(crate) fn top_section_y(&self, pos: ChunkPos) -> i32 {
        self.column(pos)
            .layers
            .iter()
            .rposition(Option::is_some)
            .map_or(self.lowest_section, |slot| {
                slot as i32 + MIN_LIGHT_SECTION + 1
            })
    }

    /// `hasLightDataAtOrBelow`。
    pub(crate) fn has_light_data_at_or_below(&self, y: i32) -> bool {
        y >= self.lowest_section
    }

    /// `isAboveData`：这一段在这一列所有存光照的段上面。
    pub(crate) fn is_above_data(&self, section: SectionPos) -> bool {
        let top = self.top_section_y(section.chunk());
        top == self.lowest_section || section.y >= top
    }

    /// `getLightValue`：别人读到的光照。
    pub(crate) fn light_value(&self, pos: BlockPos) -> u8 {
        let section = SectionPos::of(pos);
        let (x, y, z) = local(pos);
        match self.layer {
            LightLayer::Block => self.layer(section).map_or(0, |layer| layer.get(x, y, z)),
            LightLayer::Sky => self.sky_light_value(section, x, y, z),
        }
    }

    /// 天空光：所有存光照的段上面是 15。没存光照的段读上面最近一段的最底一层。
    fn sky_light_value(&self, section: SectionPos, x: usize, y: usize, z: usize) -> u8 {
        let top = self.top_section_y(section.chunk());
        if top == self.lowest_section || section.y >= top {
            return FULL_LIGHT;
        }
        if let Some(layer) = self.layer(section) {
            return layer.get(x, y, z);
        }
        let mut above = section.above();
        while above.y < top {
            if let Some(layer) = self.layer(above) {
                return layer.get(x, 0, z);
            }
            above = above.above();
        }
        FULL_LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: SectionPos = SectionPos::new(0, -4, 0);

    fn sky() -> LightStorage {
        LightStorage::new(LightLayer::Sky, LightColumn::empty(), i32::MAX)
    }

    #[test]
    fn a_section_with_blocks_stores_light_for_itself_and_its_neighbors() {
        let mut storage = sky();
        storage.update_section_status(ORIGIN, false);

        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    assert!(storage.storing_light_for_section(ORIGIN.offset(dx, dy, dz)));
                }
            }
        }
        assert!(!storage.storing_light_for_section(ORIGIN.offset(0, 2, 0)));
        assert_eq!(storage.lowest_section(), -5);
        assert_eq!(storage.top_section_y(ChunkPos::new(0, 0)), -2);
    }

    #[test]
    fn emptied_sections_go_away_when_the_round_ends() {
        let mut storage = sky();
        storage.update_section_status(ORIGIN, false);
        storage.update_section_status(ORIGIN, true);
        assert!(storage.storing_light_for_section(ORIGIN));

        storage.mark_new_inconsistencies();
        assert!(!storage.storing_light_for_section(ORIGIN));
        // 最低段只降不升。
        assert_eq!(storage.lowest_section(), -5);
    }

    #[test]
    fn a_section_refilled_in_the_same_round_keeps_its_data() {
        let mut storage = sky();
        storage.update_section_status(ORIGIN, false);
        storage.set_stored_level(BlockPos::new(1, -60, 1), 7);
        storage.update_section_status(ORIGIN, true);
        storage.update_section_status(ORIGIN, false);
        storage.mark_new_inconsistencies();

        assert_eq!(storage.stored_level(BlockPos::new(1, -60, 1)), 7);
    }

    #[test]
    fn sky_reads_the_section_above_when_nothing_is_stored() {
        let mut storage = sky();
        storage.update_section_status(SectionPos::new(0, 0, 0), false);
        storage.set_stored_level(BlockPos::new(2, -1, 3), 9);
        storage.set_stored_level(BlockPos::new(2, -16, 3), 4);

        assert_eq!(storage.light_value(BlockPos::new(2, -16, 3)), 4);
        // -2 段没存光照，读 -1 段最底下那层。
        assert_eq!(storage.light_value(BlockPos::new(2, -40, 3)), 4);
        assert_eq!(storage.light_value(BlockPos::new(2, 100, 3)), 15);
        assert_eq!(storage.light_value(BlockPos::new(40, 0, 3)), 15);
    }

    #[test]
    fn new_sky_sections_below_copy_the_bottom_of_the_section_above() {
        let mut storage = sky();
        storage.set_light_enabled(ChunkPos::new(0, 0), true);
        storage.update_section_status(SectionPos::new(0, 3, 0), false);
        storage.set_stored_level(BlockPos::new(5, 32, 5), 6);

        // 新的有方块的段在下面，它的上面那段还在存光照。
        storage.update_section_status(SectionPos::new(0, 0, 0), false);
        let layer = storage.layer(SectionPos::new(0, 0, 0)).expect("刚存了光照");
        assert_eq!(layer.get(5, 9, 5), 6);
        assert_eq!(layer.get(4, 9, 5), 15);
    }

    #[test]
    fn every_write_gives_the_column_a_new_revision() {
        let mut storage = sky();
        let column = ChunkPos::new(0, 0);
        assert_eq!(storage.revision(column), 0);

        storage.update_section_status(ORIGIN, false);
        let created = storage.revision(column);
        assert_ne!(created, 0);

        storage.set_stored_level(BlockPos::new(1, -60, 1), 7);
        let written = storage.revision(column);
        assert_ne!(written, created);

        // 丢掉段也是写，哪怕没有一格光照变了。
        storage.update_section_status(ORIGIN, true);
        let emptied = storage.revision(column);
        storage.mark_new_inconsistencies();
        assert_ne!(storage.revision(column), emptied);
        assert!(!storage.storing_light_for_section(ORIGIN));
    }

    #[test]
    fn writes_mark_the_sections_next_to_the_block() {
        let mut storage = sky();
        storage.update_section_status(ORIGIN, false);
        storage.take_affected();

        storage.set_stored_level(BlockPos::new(15, -60, 7), 3);
        let affected: Vec<_> = storage.take_affected().into_iter().collect();
        assert_eq!(affected, [ORIGIN, SectionPos::new(1, -4, 0)]);
    }
}
