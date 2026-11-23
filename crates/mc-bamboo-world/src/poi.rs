//! 兴趣点（POI）：官方 `PoiManager` / `PoiSection` / `PoiRecord` 的存储层。
//!
//! 按 16³ 段（`SectionPos`）索引，段内按类型再建一层索引。POI 是方块状态的派生
//! 索引，和区块列同批搬、单独写到 `poi/r.X.Z.mca`；改动不进区块包的版本号。
//!
//! 输入（位置、半径、票数）可能直接来自客户端：所有入口都做边界检查并返回
//! `Option` / `bool`，越界、未加载、坏存档都不会 panic。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::poi::{self, POI_TYPES, for_state};

use crate::{MIN_Y, WORLD_HEIGHT, light::SectionPos, ticket::ChunkHasherBuilder};

/// 官方 `PoiManager.MAX_VILLAGE_DISTANCE`：村庄距离超过 6 段就是「不在村庄里」。
pub const MAX_VILLAGE_DISTANCE: i32 = 6;
/// `DistanceTracker` 的缺省级别（7 = 不可达）。
pub const NO_VILLAGE_LEVEL: i32 = MAX_VILLAGE_DISTANCE + 1;
/// 最低 / 最高区块段（含端点，官方 `LevelHeightAccessor.getMinSectionY/getMaxSectionY`）。
pub const MIN_SECTION_Y: i32 = MIN_Y >> 4;
pub const MAX_SECTION_Y: i32 = (MIN_Y + WORLD_HEIGHT - 1) >> 4;

/// POI 类型的注册表 ID。存档用名字，运行时按 ID 索引。
pub type PoiType = u8;

/// `PoiManager.Occupancy`：过滤记录时的三种口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occupancy {
    /// `freeTickets > 0`。
    HasSpace,
    /// `freeTickets != maxTickets`（被占用过哪怕现在有空位）。
    IsOccupied,
    Any,
}

impl Occupancy {
    fn accepts(self, free: i32, max: i32) -> bool {
        match self {
            Self::HasSpace => free > 0,
            Self::IsOccupied => free != max,
            Self::Any => true,
        }
    }
}

/// 一条 POI 记录，对应官方 `PoiRecord`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoiRecord {
    pub pos: BlockPos,
    pub kind: PoiType,
    /// 剩余可用的票，初值 [`max_tickets`](Self::max_tickets)。
    pub free_tickets: i32,
}

impl PoiRecord {
    /// 新建一条记录（官方 `PoiRecord.<init>`）。
    pub fn new(pos: BlockPos, kind: PoiType) -> Self {
        Self {
            pos,
            kind,
            free_tickets: max_tickets(kind),
        }
    }

    pub fn max_tickets(&self) -> i32 {
        max_tickets(self.kind)
    }

    pub fn has_space(&self) -> bool {
        self.free_tickets > 0
    }

    pub fn is_occupied(&self) -> bool {
        self.free_tickets != self.max_tickets()
    }

    /// `acquireTicket`：有票才扣，返回是否扣成功。
    pub fn acquire_ticket(&mut self) -> bool {
        if self.free_tickets <= 0 {
            return false;
        }
        self.free_tickets -= 1;
        true
    }

    /// `releaseTicket`：没到上限才还，返回是否还成功。
    pub fn release_ticket(&mut self) -> bool {
        if self.free_tickets >= self.max_tickets() {
            return false;
        }
        self.free_tickets += 1;
        true
    }

    /// 官方 `BlockPos.hashCode`，用于 `HashSet<PoiRecord>` 的桶序。
    fn java_hash(&self) -> i32 {
        self.pos
            .y
            .wrapping_mul(31)
            .wrapping_add(self.pos.z)
            .wrapping_mul(31)
            .wrapping_add(self.pos.x)
    }
}

/// 一种 POI 类型的票数上限；未知 ID 按 0（存档坏数据不能 panic）。
pub fn max_tickets(kind: PoiType) -> i32 {
    POI_TYPES
        .get(usize::from(kind))
        .map_or(0, |poi| poi.max_tickets)
}

/// 一种 POI 类型的有效范围；未知 ID 按 1。
pub fn valid_range(kind: PoiType) -> i32 {
    POI_TYPES
        .get(usize::from(kind))
        .map_or(1, |poi| poi.valid_range)
}

/// POI 类型是不是属于官方 `#minecraft:village` 标签。
pub fn is_village_type(kind: PoiType) -> bool {
    POI_TYPES
        .get(usize::from(kind))
        .is_some_and(|poi| poi.village)
}

/// 类型的存档名（`minecraft:xxx`）。
pub fn type_name(kind: PoiType) -> Option<&'static str> {
    POI_TYPES.get(usize::from(kind)).map(|poi| poi.name)
}

/// 从存档名查类型 ID。
pub fn type_from_name(name: &str) -> Option<PoiType> {
    poi::poi_type_id(name)
}

/// `PoiTypes.forState`：方块状态 ID → POI 类型。
pub fn poi_for_state(state: u32) -> Option<PoiType> {
    for_state(state)
}

/// `PoiTypes.hasPoi`。
pub fn has_poi(state: u32) -> bool {
    for_state(state).is_some()
}

/// 段内下标：官方 `SectionPos.sectionRelativePos`，`x<<8 | z<<4 | y`。
pub const fn section_relative(pos: BlockPos) -> u16 {
    (((pos.x & 15) << 8) | ((pos.z & 15) << 4) | (pos.y & 15)) as u16
}

/// 一个 16³ 段，对应官方 `PoiSection`。
///
/// `records` 按段内下标索引；`by_type` 按类型索引同一批记录（用段内下标引用，
/// 顺序按 Java `HashSet<PoiRecord>` 的桶序排好，见 [`java_set_order`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoiSection {
    pub valid: bool,
    records: BTreeMap<u16, PoiRecord>,
    by_type: BTreeMap<PoiType, Vec<u16>>,
}

impl PoiSection {
    /// 官方 `PoiSection(Runnable)` 造的空段，`isValid = true`。
    pub fn empty() -> Self {
        Self {
            valid: true,
            records: BTreeMap::new(),
            by_type: BTreeMap::new(),
        }
    }

    /// 读盘造段：`Valid` 缺省 false，随后 `checkConsistencyWithBlocks` 会重建。
    pub fn from_packed(valid: bool, records: Vec<PoiRecord>) -> Self {
        let mut section = Self {
            valid,
            records: BTreeMap::new(),
            by_type: BTreeMap::new(),
        };
        for record in records {
            section.insert(record);
        }
        section
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 全部记录，按段内下标升序（存档编码用；官方 `pack()` 走 fastutil 的桶序，
    /// 但 NBT 比对按内容不按顺序，见 `poi-oracle`）。
    pub fn records(&self) -> impl Iterator<Item = &PoiRecord> + '_ {
        self.records.values()
    }

    pub fn get(&self, pos: BlockPos) -> Option<&PoiRecord> {
        self.records.get(&section_relative(pos))
    }

    pub fn get_mut(&mut self, pos: BlockPos) -> Option<&mut PoiRecord> {
        self.records.get_mut(&section_relative(pos))
    }

    /// `PoiSection.add`：同位置同类型已存在时返回 `false`（官方不打日志也不扣票）。
    pub fn add(&mut self, pos: BlockPos, kind: PoiType) -> bool {
        let key = section_relative(pos);
        if let Some(old) = self.records.get(&key)
            && old.kind == kind
        {
            return false;
        }
        self.insert(PoiRecord::new(pos, kind));
        true
    }

    fn insert(&mut self, record: PoiRecord) {
        let key = section_relative(record.pos);
        if let Some(old) = self.records.remove(&key) {
            self.by_type
                .entry(old.kind)
                .and_modify(|keys| keys.retain(|&k| k != key));
        }
        self.by_type.entry(record.kind).or_default().push(key);
        self.records.insert(key, record);
        self.sort_by_type(record_type(&self.records, key));
    }

    /// `PoiSection.remove`：返回是否真的删掉了一条。
    pub fn remove(&mut self, pos: BlockPos) -> bool {
        let key = section_relative(pos);
        let Some(old) = self.records.remove(&key) else {
            return false;
        };
        if let Some(keys) = self.by_type.get_mut(&old.kind) {
            keys.retain(|&k| k != key);
            if keys.is_empty() {
                self.by_type.remove(&old.kind);
            }
        }
        true
    }

    fn sort_by_type(&mut self, kind: Option<PoiType>) {
        let Some(kind) = kind else { return };
        let hashes: HashMap<u16, i32> = self
            .records
            .iter()
            .map(|(&key, record)| (key, record.java_hash()))
            .collect();
        let Some(keys) = self.by_type.get_mut(&kind) else {
            return;
        };
        java_set_order(keys, |key| hashes.get(key).copied().unwrap_or(0));
    }

    /// 官方 `getRecords(typePredicate, Occupancy)` 的遍历序：
    /// `byType` 的键序（见模块文档的偏差说明），键内 `HashSet` 的桶序。
    pub fn find(
        &self,
        type_predicate: impl Fn(PoiType) -> bool,
        occupancy: Occupancy,
    ) -> Vec<&PoiRecord> {
        let mut out = Vec::new();
        for (&kind, keys) in &self.by_type {
            if !type_predicate(kind) {
                continue;
            }
            for key in keys {
                if let Some(record) = self.records.get(key)
                    && occupancy.accepts(record.free_tickets, record.max_tickets())
                {
                    out.push(record);
                }
            }
        }
        out
    }

    /// 有没有一条 `#minecraft:village` 类型且被占用过的 POI。
    /// 官方 `isVillageCenter` 只看已加载的段（`get()`），这里同义。
    pub fn is_village_center(&self) -> bool {
        self.find(is_village_type, Occupancy::IsOccupied)
            .into_iter()
            .next()
            .is_some()
    }

    /// 官方 `PoiSection.refresh`：`Valid=false` 时按当前方块重建，保留同位置记录的
    /// `freeTickets`（`old.computeIfAbsent(short, ...)`），并置脏。
    pub fn refresh(&mut self, from_blocks: Vec<(BlockPos, PoiType)>) -> bool {
        if self.valid {
            return false;
        }
        let old: BTreeMap<u16, PoiRecord> = std::mem::take(&mut self.records);
        self.by_type.clear();
        for (pos, kind) in from_blocks {
            let key = section_relative(pos);
            let record = old
                .get(&key)
                .cloned()
                .unwrap_or_else(|| PoiRecord::new(pos, kind));
            self.insert(record);
        }
        self.valid = true;
        true
    }
}

fn record_type(records: &BTreeMap<u16, PoiRecord>, key: u16) -> Option<PoiType> {
    records.get(&key).map(|record| record.kind)
}

/// 复刻 Java `HashSet` / `HashMap` 的遍历序：按桶下标升序，同桶保持插入序。
/// `HashSet` 的初始容量是 16，装到 `0.75 * capacity` 时翻倍；`hash(h) = h ^ (h >>> 16)`。
/// 这里用稳定排序而不是真的开链，同桶里的插入序因此保住（官方 `HashMap` 尾插同义）。
fn java_set_order(items: &mut Vec<u16>, hash: impl Fn(&u16) -> i32) {
    let capacity = java_hash_capacity(items.len());
    let mask = capacity - 1;
    let mut decorated: Vec<(usize, usize, u16)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let h = hash(item) as u32;
            let spread = h ^ (h >> 16);
            ((spread as usize) & mask, index, *item)
        })
        .collect();
    decorated.sort_by_key(|&(bucket, index, _)| (bucket, index));
    items.clear();
    items.extend(decorated.into_iter().map(|(_, _, key)| key));
}

/// Java `HashMap` 的容量：首次插入 16，之后每超过 `0.75*n` 翻倍。
fn java_hash_capacity(len: usize) -> usize {
    let mut capacity = 16usize;
    while len > capacity * 3 / 4 {
        capacity *= 2;
    }
    capacity
}

/// 官方 `SectionPos.asLong` 的打包（和 Bamboo 的 `SectionPos` 一致的三元组）。
pub const fn section_key(pos: SectionPos) -> (i32, i32, i32) {
    (pos.x, pos.y, pos.z)
}

/// 一个区块列上按 `y` 分好组的 POI 段，跟方块一起装进 `RegionColumn`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoiColumn {
    pub sections: BTreeMap<i32, PoiSection>,
}

impl PoiColumn {
    pub fn is_empty(&self) -> bool {
        self.sections.values().all(PoiSection::is_empty) && self.sections.is_empty()
    }
}

/// 整个区域的 POI 表，和 [`ChunkStore`](crate::store::ChunkStore) 平级。
///
/// `sections` 里没有键和「键上挂着一个空段」不同：官方 `SectionStorage` 用
/// `Optional.empty()` 记录「这个段盘上没有数据」，用 `Some(PoiSection)` 记录
/// 已加载 / 新建的段。查询只遍历 `Some` 的段。
#[derive(Debug, Clone, Default)]
pub struct PoiStore {
    sections: HashMap<(i32, i32, i32), Option<PoiSection>, ChunkHasherBuilder>,
    /// `SectionStorage.dirtyChunks`：按 `ChunkPos` 去重，官方 `setDirty` 的时机。
    dirty: BTreeSet<ChunkPos>,
    /// 已经从盘上读过的区块；读坏 / 没文件时也记下，避免重复读。
    loaded_chunks: HashSet<ChunkPos, ChunkHasherBuilder>,
}

impl PoiStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 这个区块的 POI 数据有没有从盘上装载过（含「盘上没有」）。
    pub fn is_chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.loaded_chunks.contains(&pos)
    }

    /// 标记一个区块已经处理过盘上的数据；`None` 表示盘上没有 / 读坏了。
    /// 先把这个位置原有的段全部清掉，再按 `column` 装新的（岛合并 / 放入盒子用）。
    pub fn install_chunk(&mut self, pos: ChunkPos, column: Option<Vec<(i32, PoiSection)>>) {
        for y in MIN_SECTION_Y..=MAX_SECTION_Y {
            self.sections.remove(&(pos.x, y, pos.z));
        }
        self.loaded_chunks.insert(pos);
        match column {
            Some(sections) => {
                for (y, section) in sections {
                    self.sections.insert((pos.x, y, pos.z), Some(section));
                }
            }
            None => {
                for y in MIN_SECTION_Y..=MAX_SECTION_Y {
                    self.sections.insert((pos.x, y, pos.z), None);
                }
            }
        }
    }

    /// 把一个盒子里带回来的 POI 段整列装进去（不标脏：脏标记由写方块 / 票数变化管）。
    pub fn install_column(&mut self, pos: ChunkPos, column: PoiColumn) {
        self.install_chunk(pos, Some(column.sections.into_iter().collect()));
    }

    /// 卸载：把这一列上所有已加载段取出来（存盘用），并从表里丢掉。
    pub fn take_column(&mut self, pos: ChunkPos) -> PoiColumn {
        let mut out = PoiColumn::default();
        for y in MIN_SECTION_Y..=MAX_SECTION_Y {
            if let Some(Some(section)) = self.sections.remove(&(pos.x, y, pos.z)) {
                out.sections.insert(y, section);
            } else {
                self.sections.remove(&(pos.x, y, pos.z));
            }
        }
        self.loaded_chunks.remove(&pos);
        self.dirty.remove(&pos);
        out
    }

    /// 岛拆分 / 合岛：按区块整批搬段、脏标记和已装载登记。
    pub fn split_off(&mut self, chunks: &BTreeSet<ChunkPos>) -> Self {
        let mut out = Self::new();
        for &pos in chunks {
            for y in MIN_SECTION_Y..=MAX_SECTION_Y {
                if let Some(section) = self.sections.remove(&(pos.x, y, pos.z)) {
                    out.sections.insert((pos.x, y, pos.z), section);
                }
            }
            if self.dirty.remove(&pos) {
                out.dirty.insert(pos);
            }
            if self.loaded_chunks.remove(&pos) {
                out.loaded_chunks.insert(pos);
            }
        }
        out
    }

    /// [`Self::split_off`] 的逆操作。
    pub fn absorb(&mut self, other: Self) {
        self.sections.extend(other.sections);
        self.dirty.extend(other.dirty);
        self.loaded_chunks.extend(other.loaded_chunks);
    }

    /// 已加载段的只读快照，按 `(段坐标, 段内下标)` 排好，拆合前后逐项比。
    pub fn snapshot(&self) -> Vec<((i32, i32, i32), Vec<PoiRecord>)> {
        let mut keys: Vec<_> = self
            .sections
            .iter()
            .filter_map(|(&key, section)| section.as_ref().map(|_| key))
            .collect();
        keys.sort_unstable();
        keys.into_iter()
            .map(|key| {
                let section = self.sections[&key].as_ref().expect("刚筛过");
                (key, section.records().cloned().collect())
            })
            .collect()
    }

    /// 已加载段数（核对 / 测试用）。
    pub fn loaded_section_count(&self) -> usize {
        self.sections
            .values()
            .filter(|section| section.is_some())
            .count()
    }

    /// 脏区块集合，给存盘编排按 `(pos, seq)` 排队。
    pub fn dirty_chunks(&self) -> &BTreeSet<ChunkPos> {
        &self.dirty
    }

    /// 取走脏区块集合（存盘快照前调用）。
    pub fn take_dirty_chunks(&mut self) -> BTreeSet<ChunkPos> {
        std::mem::take(&mut self.dirty)
    }

    /// 手动标脏（读盘时 `DataVersion` 变化、刷新无效段等）。
    pub fn mark_dirty(&mut self, pos: ChunkPos) {
        self.dirty.insert(pos);
    }

    /// 官方 `SectionStorage.setDirty(long)`：这一段的区块标脏。
    fn set_dirty(&mut self, pos: SectionPos) {
        self.dirty.insert(pos.chunk());
    }

    /// 只读克隆这一列的全部已加载段，给存盘快照用（不移动、不改脏标记）。
    pub fn column_snapshot(&self, pos: ChunkPos) -> PoiColumn {
        PoiColumn {
            sections: self
                .column_sections(pos)
                .into_iter()
                .map(|(y, section)| (y, section.clone()))
                .collect(),
        }
    }

    /// 编出一个区块的 `poi/` 根标签所需的全部段（按 y 升序、只含 `Some` 段）。
    pub fn column_sections(&self, pos: ChunkPos) -> Vec<(i32, &PoiSection)> {
        (MIN_SECTION_Y..=MAX_SECTION_Y)
            .filter_map(|y| {
                self.sections
                    .get(&(pos.x, y, pos.z))
                    .and_then(|section| section.as_ref())
                    .map(|section| (y, section))
            })
            .collect()
    }

    /// `get(long)`：只查内存，不触发读盘。官方 `isVillageCenter` 走这条。
    pub fn get(&self, pos: SectionPos) -> Option<&PoiSection> {
        self.sections.get(&section_key(pos))?.as_ref()
    }

    /// `getOrCreate(long)`：没有段就造一个空段并标脏。越界返回 `None`，不 panic。
    pub fn get_or_create(&mut self, pos: SectionPos) -> Option<&mut PoiSection> {
        if !(MIN_SECTION_Y..=MAX_SECTION_Y).contains(&pos.y) {
            return None;
        }
        let key = section_key(pos);
        if self.sections.get(&key).is_none_or(Option::is_none) {
            self.sections.insert(key, Some(PoiSection::empty()));
            self.set_dirty(pos);
        }
        self.sections.get_mut(&key).map(Option::as_mut).flatten()
    }

    /// `PoiManager.add`：登记一条 POI，返回是否是新登记。
    pub fn add(&mut self, pos: BlockPos, kind: PoiType) -> bool {
        let Some(section) = self.get_or_create(SectionPos::of(pos)) else {
            return false;
        };
        let added = section.add(pos, kind);
        if added {
            self.set_dirty(SectionPos::of(pos));
        }
        added
    }

    /// `PoiManager.remove`：注销一条 POI。没登记过时不标脏（官方会记一条 error 日志）。
    pub fn remove(&mut self, pos: BlockPos) -> bool {
        let section_pos = SectionPos::of(pos);
        let Some(Some(section)) = self.sections.get_mut(&section_key(section_pos)) else {
            return false;
        };
        let removed = section.remove(pos);
        if removed {
            self.set_dirty(section_pos);
        }
        removed
    }

    /// `PoiTypes.forState(old) == forState(new)` 时什么都不做；否则先删再（可能）加。
    /// 官方 `ServerLevel.updatePOIOnBlockStateChange` 的同步版本（见文档里记的差异）。
    pub fn on_block_state_change(&mut self, pos: BlockPos, old: u32, new: u32) -> PoiDelta {
        let before = poi_for_state(old);
        let after = poi_for_state(new);
        if before == after {
            return PoiDelta::None;
        }
        self.remove(pos);
        if let Some(kind) = after {
            self.add(pos, kind);
            PoiDelta::Added { pos, kind }
        } else if before.is_some() {
            PoiDelta::Removed(pos)
        } else {
            PoiDelta::None
        }
    }

    /// `exists(pos, typePredicate)`。
    pub fn exists(&self, pos: BlockPos, predicate: impl Fn(PoiType) -> bool) -> bool {
        self.get(SectionPos::of(pos))
            .and_then(|section| section.get(pos))
            .is_some_and(|record| predicate(record.kind))
    }

    /// `existsAtPosition(key, pos)`。
    pub fn exists_at_position(&self, kind: PoiType, pos: BlockPos) -> bool {
        self.exists(pos, |found| found == kind)
    }

    /// `getType(pos)`。
    pub fn get_type(&self, pos: BlockPos) -> Option<PoiType> {
        self.get(SectionPos::of(pos))
            .and_then(|section| section.get(pos))
            .map(|record| record.kind)
    }

    /// 这一格的剩余票数；没有记录返回 0（官方 `getFreeTickets` 的 `orElse(0)`）。
    pub fn free_tickets(&self, pos: BlockPos) -> i32 {
        self.get(SectionPos::of(pos))
            .and_then(|section| section.get(pos))
            .map_or(0, |record| record.free_tickets)
    }

    /// `release(pos)`：官方找不到记录会抛异常，这里返回 `false`，不 panic。
    pub fn release(&mut self, pos: BlockPos) -> bool {
        let section_pos = SectionPos::of(pos);
        let Some(Some(section)) = self.sections.get_mut(&section_key(section_pos)) else {
            return false;
        };
        let Some(record) = section.get_mut(pos) else {
            return false;
        };
        let released = record.release_ticket();
        // 官方 `PoiSection.release` 无论还没还成都 `setDirty`。
        self.set_dirty(section_pos);
        released
    }

    /// `getInSquare(typePredicate, center, radius, occupancy)` 的全部记录，保持官方
    /// `ChunkPos.rangeClosed` 的区块顺序与 `getInChunk` 的段 y 升序。
    pub fn get_in_square(
        &self,
        predicate: impl Fn(PoiType) -> bool + Copy,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Vec<PoiRecord> {
        // 官方 radius/16 + 1；负数半径按 Java 的向零截断。
        let extent = radius / 16 + 1;
        let chunk = ChunkPos::new(center.x >> 4, center.z >> 4);
        let mut out = Vec::new();
        for &chunk_pos in &chunk_range_closed(chunk, extent) {
            for y in MIN_SECTION_Y..=MAX_SECTION_Y {
                let Some(Some(section)) = self.sections.get(&(chunk_pos.x, y, chunk_pos.z)) else {
                    continue;
                };
                for record in section.find(predicate, occupancy) {
                    let dx = (record.pos.x - center.x).abs();
                    let dz = (record.pos.z - center.z).abs();
                    if dx <= radius && dz <= radius {
                        out.push(record.clone());
                    }
                }
            }
        }
        out
    }

    /// `getInRange`：在方环结果上再按欧氏距离平方过滤（`<= radius²`）。
    pub fn get_in_range(
        &self,
        predicate: impl Fn(PoiType) -> bool + Copy,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Vec<PoiRecord> {
        let limit = i64::from(radius) * i64::from(radius);
        self.get_in_square(predicate, center, radius, occupancy)
            .into_iter()
            .filter(|record| distance_sqr(center, record.pos) <= limit)
            .collect()
    }

    /// `getCountInRange`。
    pub fn get_count_in_range(
        &self,
        predicate: impl Fn(PoiType) -> bool + Copy,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> usize {
        self.get_in_range(predicate, center, radius, occupancy)
            .len()
    }

    /// `findAll(predicate, posPredicate, center, radius, occupancy)`。
    pub fn find_all(
        &self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        pos_predicate: impl Fn(BlockPos) -> bool,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Vec<BlockPos> {
        self.get_in_range(type_predicate, center, radius, occupancy)
            .into_iter()
            .map(|record| record.pos)
            .filter(|pos| pos_predicate(*pos))
            .collect()
    }

    /// `findAllClosestFirstWithType`：稳定排序（Java `Stream.sorted` 对有序流稳定），
    /// 并列时保留遍历序。
    pub fn find_all_closest_first_with_type(
        &self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        pos_predicate: impl Fn(BlockPos) -> bool,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Vec<(PoiType, BlockPos)> {
        let mut found: Vec<(PoiType, BlockPos)> = self
            .get_in_range(type_predicate, center, radius, occupancy)
            .into_iter()
            .filter(|record| pos_predicate(record.pos))
            .map(|record| (record.kind, record.pos))
            .collect();
        found.sort_by_key(|&(_, pos)| distance_sqr(center, pos));
        found
    }

    /// `findClosest(typePredicate, posPredicate, center, radius, occupancy)`：
    /// `Stream.min` 并列取遍历序第一个。
    pub fn find_closest(
        &self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        pos_predicate: impl Fn(BlockPos) -> bool,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Option<BlockPos> {
        self.get_in_range(type_predicate, center, radius, occupancy)
            .into_iter()
            .filter(|record| pos_predicate(record.pos))
            .min_by_key(|record| distance_sqr(center, record.pos))
            .map(|record| record.pos)
    }

    /// `findClosestWithType`。
    pub fn find_closest_with_type(
        &self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        center: BlockPos,
        radius: i32,
        occupancy: Occupancy,
    ) -> Option<(PoiType, BlockPos)> {
        self.get_in_range(type_predicate, center, radius, occupancy)
            .into_iter()
            .min_by_key(|record| distance_sqr(center, record.pos))
            .map(|record| (record.kind, record.pos))
    }

    /// `take(typePredicate, biPredicate, pos, range)`：`HAS_SPACE` 里取第一个，
    /// 扣票并标脏，返回它的位置。
    pub fn take(
        &mut self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        bi_predicate: impl Fn(PoiType, BlockPos) -> bool,
        center: BlockPos,
        radius: i32,
    ) -> Option<BlockPos> {
        let candidates = self.get_in_range(type_predicate, center, radius, Occupancy::HasSpace);
        let chosen = candidates
            .into_iter()
            .find(|record| bi_predicate(record.kind, record.pos))?;
        let section_pos = SectionPos::of(chosen.pos);
        let section = self.sections.get_mut(&section_key(section_pos))?.as_mut()?;
        section.get_mut(chosen.pos)?.acquire_ticket();
        self.set_dirty(section_pos);
        Some(chosen.pos)
    }

    /// `getRandom`：`Util.toShuffledList` 用 Fisher–Yates（官方从后往前，`nextInt(i+1)`），
    /// 再按位置谓词取第一个。调用方给一个与官方同种子、同取数顺序的随机源。
    pub fn get_random(
        &self,
        type_predicate: impl Fn(PoiType) -> bool + Copy,
        pos_predicate: impl Fn(BlockPos) -> bool,
        occupancy: Occupancy,
        center: BlockPos,
        radius: i32,
        mut next_int: impl FnMut(i32) -> i32,
    ) -> Option<BlockPos> {
        let mut shuffled: Vec<BlockPos> = self
            .get_in_range(type_predicate, center, radius, occupancy)
            .into_iter()
            .map(|record| record.pos)
            .collect();
        for i in (1..shuffled.len()).rev() {
            let bound = i32::try_from(i + 1).unwrap_or(i32::MAX);
            let j = next_int(bound);
            let j = usize::try_from(j).unwrap_or(0).min(i);
            shuffled.swap(i, j);
        }
        shuffled.into_iter().find(|pos| pos_predicate(*pos))
    }

    /// 区块加载时按官方 `checkConsistencyWithBlocks` 逐段校验：`Valid=false` 的段
    /// 按当前方块重建，盘上没写过的段（`Optional.empty()`）在方块里确实有 POI 时
    /// 才建段并扫一遍。`may_have_poi(y)` 是 `LevelChunkSection.maybeHas` 的等价
    /// 快速判断（调色板里有没有 POI 状态），`block_at` 只读方块状态。
    pub fn check_loaded_column(
        &mut self,
        may_have_poi: impl Fn(i32) -> bool,
        block_at: impl Fn(BlockPos) -> Option<u32>,
        pos: ChunkPos,
    ) {
        for y in MIN_SECTION_Y..=MAX_SECTION_Y {
            let section_pos = SectionPos::new(pos.x, y, pos.z);
            let key = section_key(section_pos);
            let (needs_scan, invalid_loaded) = match self.sections.get(&key) {
                // 盘上 / 盒子里没写过的段：先看方块里有没有 POI，避免给每个空段建盒子。
                None | Some(None) => (true, false),
                Some(Some(section)) => (!section.is_valid(), !section.is_valid()),
            };
            if !needs_scan || (!may_have_poi(y) && !invalid_loaded) {
                continue;
            }
            let bx = pos.x * 16;
            let bz = pos.z * 16;
            let mut found = Vec::new();
            for dy in 0..16 {
                for dz in 0..16 {
                    for dx in 0..16 {
                        let at = BlockPos::new(bx + dx, y * 16 + dy, bz + dz);
                        if let Some(state) = block_at(at)
                            && let Some(kind) = poi_for_state(state)
                        {
                            found.push((at, kind));
                        }
                    }
                }
            }
            self.check_consistency_with_blocks(section_pos, found);
        }
    }

    /// 官方 `checkConsistencyWithBlocks(SectionPos, LevelChunkSection)`：
    /// 段无效时按当前方块重建；段不存在且方块里确实可能有 POI 时先建段再扫。
    /// `from_blocks` 由调用方按 `SectionPos.blocksInside()` 的顺序给出。
    pub fn check_consistency_with_blocks(
        &mut self,
        pos: SectionPos,
        from_blocks: Vec<(BlockPos, PoiType)>,
    ) -> bool {
        let key = section_key(pos);
        match self.sections.get_mut(&key) {
            Some(Some(section)) => {
                if section.refresh(from_blocks) {
                    self.set_dirty(pos);
                    true
                } else {
                    false
                }
            }
            Some(None) | None => {
                if from_blocks.is_empty() {
                    return false;
                }
                let Some(section) = self.get_or_create(pos) else {
                    return false;
                };
                section.valid = false;
                let rebuilt = section.refresh(from_blocks);
                if rebuilt {
                    self.set_dirty(pos);
                }
                rebuilt
            }
        }
    }
}

/// 官方 `PoiManager.DistanceTracker`（`SectionTracker(7,16,256)`）的简化版：
/// 源段 = 已加载段里有 `#minecraft:village` 类型且被占用过的 POI
/// （`isVillageCenter` 用 `get()` 只看内存里的段，不触发读盘）。
/// 图的边是 3×3×3 的 26 个邻段，每条边 +1，`levelCount = 7`，`setLevel` 把
/// >6 的段从表里删掉：所以 `level` 就是到最近源段的切比雪夫距离，缺省 7。
///
/// 官方用 `DynamicGraphMinFixedPoint` 增量维护并在 `sectionsToVillage` 前
/// `runAllUpdates()`；这里每次查询按当前段重算一遍不动点，结果逐段相同
/// （少了排队次序，但最小解唯一）。
#[derive(Debug, Clone, Default)]
pub struct DistanceTracker {
    levels: BTreeMap<(i32, i32, i32), u8>,
}

impl DistanceTracker {
    /// 按当前已加载的段重算不动点。
    pub fn update(&mut self, store: &PoiStore) {
        self.levels.clear();
        let mut queue = std::collections::VecDeque::new();
        for (&key, section) in store.sections_loaded() {
            if section.is_some_and(PoiSection::is_village_center) {
                self.levels.insert(key, 0);
                queue.push_back((key, 0u8));
            }
        }
        while let Some((key, level)) = queue.pop_front() {
            if level >= MAX_VILLAGE_DISTANCE as u8 {
                continue;
            }
            let next = level + 1;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let neighbour = (key.0 + dx, key.1 + dy, key.2 + dz);
                        if self.levels.get(&neighbour).is_none_or(|&old| old > next) {
                            self.levels.insert(neighbour, next);
                            queue.push_back((neighbour, next));
                        }
                    }
                }
            }
        }
    }

    /// `sectionsToVillage(SectionPos)`：不动点算完后查级别，没条目就是 7。
    pub fn level(&self, pos: SectionPos) -> i32 {
        self.levels
            .get(&section_key(pos))
            .map_or(NO_VILLAGE_LEVEL, |&level| i32::from(level))
    }

    /// 全部算出的级别（≤6），按段坐标排好，对拍逐段比。
    pub fn levels(&self) -> Vec<((i32, i32, i32), i32)> {
        self.levels
            .iter()
            .map(|(&key, &level)| (key, i32::from(level)))
            .collect()
    }
}

impl PoiStore {
    /// `sectionsToVillage`：重算不动点后取级别。
    pub fn sections_to_village(&self, pos: SectionPos) -> i32 {
        let mut tracker = DistanceTracker::default();
        tracker.update(self);
        tracker.level(pos)
    }

    /// `ServerLevel.isCloseToVillage(pos, sectionDistance)`，超过 6 直接假。
    pub fn is_close_to_village(&self, pos: BlockPos, section_distance: i32) -> bool {
        if section_distance > MAX_VILLAGE_DISTANCE {
            return false;
        }
        self.sections_to_village(SectionPos::of(pos)) <= section_distance
    }

    /// 只读地列出已加载的段（含 `None` 段），给 `DistanceTracker` 用。
    fn sections_loaded(
        &self,
    ) -> impl Iterator<Item = (&(i32, i32, i32), Option<&PoiSection>)> + '_ {
        self.sections
            .iter()
            .map(|(key, section)| (key, section.as_ref()))
    }
}

/// 一次方块变化引起的 POI 增量，供 `Region` 在写方块时同刻应用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoiDelta {
    None,
    Added { pos: BlockPos, kind: PoiType },
    Removed(BlockPos),
}

/// `BlockPos.distSqr(Vec3i)` 用 double 累加，这里用 i64 等价（坐标都在 i32 范围，
/// 平方和按 f64 精确可表示到 2⁵³；Bamboo 的坐标上限比这小）。
fn distance_sqr(from: BlockPos, to: BlockPos) -> i64 {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    let dz = i64::from(to.z) - i64::from(from.z);
    dx * dx + dy * dy + dz * dz
}

/// 官方 `ChunkPos.rangeClosed(center, radius)`：从 `(x-r, z-r)` 出发，x 逐个
/// 增到 `x+r` 后回到 `x-r` 并把 z 加一，直到 `(x+r, z+r)`。也就是 z 外层、
/// x 内层都升序（见 `ChunkPos$2.tryAdvance` 字节码）。
fn chunk_range_closed(center: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    if radius < 0 {
        return Vec::new();
    }
    let size = i64::from(radius) * 2 + 1;
    if size > 4096 {
        // 客户端给的半径不受信任：不能分配无界的表。
        return Vec::new();
    }
    let mut out = Vec::with_capacity((size * size) as usize);
    for z in center.z - radius..=center.z + radius {
        for x in center.x - radius..=center.x + radius {
            out.push(ChunkPos::new(x, z));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::{blocks, poi};

    fn bed_head() -> u32 {
        // 16 色床里挑一个 HEAD 状态；官方导出的 `home` 匹配表里任一个都行。
        *poi::POI_TYPES[poi::poi_type_id("home").unwrap() as usize]
            .states
            .first()
            .unwrap()
    }

    fn bell() -> u32 {
        *poi::POI_TYPES[poi::poi_type_id("meeting").unwrap() as usize]
            .states
            .first()
            .unwrap()
    }

    #[test]
    fn tickets_follow_the_official_limits() {
        let mut bed = PoiRecord::new(BlockPos::new(1, -60, 2), poi::poi_type_id("home").unwrap());
        assert_eq!(bed.free_tickets, 1);
        assert!(bed.acquire_ticket());
        assert!(!bed.acquire_ticket(), "只有一张票");
        assert!(bed.is_occupied());
        assert!(bed.release_ticket());
        assert!(!bed.release_ticket(), "已经满了");

        let mut bell = PoiRecord::new(
            BlockPos::new(3, -60, 4),
            poi::poi_type_id("meeting").unwrap(),
        );
        assert_eq!(bell.free_tickets, 32);
        for _ in 0..32 {
            assert!(bell.acquire_ticket());
        }
        assert!(!bell.acquire_ticket());

        let rod = PoiRecord::new(
            BlockPos::new(5, 80, 6),
            poi::poi_type_id("lightning_rod").unwrap(),
        );
        assert_eq!(rod.free_tickets, 0);
        assert!(!rod.has_space());
        assert!(!rod.is_occupied(), "0 票的 POI 永远不算被占用");
    }

    #[test]
    fn block_changes_register_and_unregister() {
        let mut store = PoiStore::new();
        let pos = BlockPos::new(3, -60, 5);
        let bed = bed_head();
        let bell = bell();
        assert_eq!(
            store.on_block_state_change(pos, blocks::AIR, bed),
            PoiDelta::Added {
                pos,
                kind: poi::poi_type_id("home").unwrap()
            }
        );
        assert_eq!(store.get_type(pos), poi::poi_type_id("home"));
        assert_eq!(
            store.on_block_state_change(pos, bed, bell),
            PoiDelta::Added {
                pos,
                kind: poi::poi_type_id("meeting").unwrap()
            }
        );
        assert_eq!(store.get_type(pos), poi::poi_type_id("meeting"));
        assert_eq!(
            store.on_block_state_change(pos, bell, blocks::AIR),
            PoiDelta::Removed(pos)
        );
        assert_eq!(store.get_type(pos), None);
        // 同类型之间换状态：官方直接返回，既不删也不重建、票数不动。
        let another_bed = *poi::POI_TYPES[poi::poi_type_id("home").unwrap() as usize]
            .states
            .last()
            .unwrap();
        assert_eq!(
            store.on_block_state_change(pos, blocks::AIR, bed),
            PoiDelta::Added {
                pos,
                kind: poi::poi_type_id("home").unwrap()
            }
        );
        store.take(|_| true, |_, _| true, pos, 4);
        assert_eq!(store.free_tickets(pos), 0);
        assert_eq!(
            store.on_block_state_change(pos, bed, another_bed),
            PoiDelta::None,
            "同一 POI 类型换状态不动记录"
        );
        assert_eq!(store.free_tickets(pos), 0, "票数也要保住");
    }

    #[test]
    fn queries_use_the_official_traversal_order() {
        let mut store = PoiStore::new();
        let bed = bed_head();
        // 三段：同段内 + 跨段，半径覆盖 2 个区块环。
        for (x, z) in [(1, 1), (3, 1), (17, 1), (1, 17)] {
            let pos = BlockPos::new(x, -60, z);
            store.on_block_state_change(pos, blocks::AIR, bed);
        }
        let center = BlockPos::new(0, -60, 0);
        let all = store.get_in_range(|_| true, center, 48, Occupancy::Any);
        assert_eq!(all.len(), 4);
        // 官方 ChunkPos.rangeClosed 的 z 外层、x 内层升序；同段内按 Java
        // HashSet<PoiRecord> 的桶序：hash(3,-60,1) 落在 hash(1,-60,1) 之前。
        let positions: Vec<_> = all
            .iter()
            .map(|record| (record.pos.x, record.pos.z))
            .collect();
        assert_eq!(positions, vec![(3, 1), (1, 1), (17, 1), (1, 17)]);
        // 并列距离时 `findClosest` 取遍历序第一个。
        assert_eq!(
            store.find_closest(
                |_| true,
                |_| true,
                BlockPos::new(2, -60, 1),
                48,
                Occupancy::Any
            ),
            Some(BlockPos::new(3, -60, 1))
        );
    }

    #[test]
    fn take_and_release_update_tickets_and_dirty_chunks() {
        let mut store = PoiStore::new();
        let pos = BlockPos::new(1, -60, 1);
        store.on_block_state_change(pos, blocks::AIR, bed_head());
        store.take_dirty_chunks();
        assert_eq!(store.take(|_| true, |_, _| true, pos, 8), Some(pos));
        assert_eq!(store.free_tickets(pos), 0);
        assert!(store.dirty_chunks().contains(&pos.chunk()));
        assert!(store.release(pos));
        assert_eq!(store.free_tickets(pos), 1);
        assert!(!store.release(pos), "已经满了也标脏，但返回 false");
        assert!(!store.release(BlockPos::new(99, 0, 99)), "没登记过不 panic");
    }

    /// 官方的源条件是「VILLAGE 标签 + IS_OCCUPIED」：新建的钟有 32 张票，
    /// 还没被村民认领时 `freeTickets == maxTickets`，不算村庄中心。
    #[test]
    fn village_distance_starts_only_from_an_occupied_center() {
        let mut store = PoiStore::new();
        let bell = bell();
        let pos = BlockPos::new(4, -60, 4);
        store.on_block_state_change(pos, blocks::AIR, bell);
        let center = SectionPos::of(pos);
        assert_eq!(
            store.sections_to_village(center),
            NO_VILLAGE_LEVEL,
            "没被占用过的钟不是源"
        );
        assert!(store.take(|_| true, |_, _| true, pos, 8).is_some());
        let mut tracker = DistanceTracker::default();
        tracker.update(&store);
        assert_eq!(tracker.level(center), 0);
        assert_eq!(tracker.level(center.offset(3, -2, 1)), 3, "切比雪夫距离");
        assert_eq!(tracker.level(center.offset(6, 0, 0)), 6);
        assert_eq!(tracker.level(center.offset(7, 0, 0)), NO_VILLAGE_LEVEL);
        assert!(store.is_close_to_village(pos.offset(0, 0, 0), 1));
        assert!(!store.is_close_to_village(pos.offset(7 * 16, 0, 0), 1));
        assert!(!store.is_close_to_village(pos, 7), "超过 6 直接假");
    }

    #[test]
    fn refresh_keeps_tickets_and_marks_the_section_valid() {
        let mut store = PoiStore::new();
        let pos = BlockPos::new(2, -60, 3);
        store.on_block_state_change(pos, blocks::AIR, bed_head());
        store.take(|_| true, |_, _| true, pos, 8);
        let section_pos = SectionPos::of(pos);
        let mut section = store
            .sections
            .remove(&section_key(section_pos))
            .expect("刚登记过")
            .expect("段已加载");
        section.valid = false;
        store
            .sections
            .insert(section_key(section_pos), Some(section));
        let from_blocks = vec![(pos, poi::poi_type_id("home").unwrap())];
        assert!(store.check_consistency_with_blocks(section_pos, from_blocks.clone()));
        assert_eq!(store.free_tickets(pos), 0, "refresh 保留扣掉的票");
        assert!(store.get(section_pos).unwrap().is_valid());
        assert!(
            !store.check_consistency_with_blocks(section_pos, from_blocks),
            "只做一次"
        );
    }
}
