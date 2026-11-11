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

/// 一个位置和它 8 个邻居柱各自的 26 个段状态，只由方块决定。
///
/// 搬一个区块柱时四份光照存储（天空光、方块光，各有「正在算」和「给别人读」两份）
/// 要写进同样的段状态，所以只算一次，再分别交给 [`LightStorage::rebind_area`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectionStatesAround {
    columns: [(ChunkPos, [u8; LIGHT_SECTION_COUNT]); 9],
}

impl SectionStatesAround {
    /// 按 `chunks` 此刻的方块算。HAS_DATA 位是这一段自己有没有方块，低 5 位是
    /// 26 个邻居段里有方块的个数（官方 `updateSectionStatus` 累加出来的结果）。
    ///
    /// 每个区块柱先压成一个掩码（第 n 位 = 第 n 个光照段有方块），邻居计数就是
    /// 在 3×3 个掩码上数上下各一段的窗口里有几位。区块只查 25 次。
    pub(crate) fn compute(pos: ChunkPos, chunks: super::Chunks<'_>) -> Self {
        // 坐标用饱和加法，贴着坐标上限时几个偏移会落到同一个位置；掩码按实际坐标缓存。
        let mut masks: Vec<(ChunkPos, u32)> = Vec::with_capacity(25);
        let mut mask_of = |column: ChunkPos| -> u32 {
            if let Some(&(_, mask)) = masks.iter().find(|(known, _)| *known == column) {
                return mask;
            }
            let mut mask = 0u32;
            // 光照段比方块段多上下各一段，那两段没有方块段。
            for (index, section) in chunks.chunk(column).sections().iter().enumerate() {
                if !section.is_empty() {
                    mask |= 1 << (index + 1);
                }
            }
            masks.push((column, mask));
            mask
        };
        let offset = |column: ChunkPos, dx: i32, dz: i32| {
            ChunkPos::new(column.x.saturating_add(dx), column.z.saturating_add(dz))
        };
        let mut columns = [(pos, [0u8; LIGHT_SECTION_COUNT]); 9];
        let mut next = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let column = offset(pos, dx, dz);
                let own = mask_of(column);
                let mut states = [0u8; LIGHT_SECTION_COUNT];
                for ndx in -1..=1 {
                    for ndz in -1..=1 {
                        let mask = mask_of(offset(column, ndx, ndz));
                        for (slot, state) in states.iter_mut().enumerate() {
                            let low = slot.saturating_sub(1);
                            let high = (slot + 1).min(LIGHT_SECTION_COUNT - 1);
                            let window = (u32::MAX >> (31 - high)) & (u32::MAX << low);
                            let mut count = (mask & window).count_ones() as u8;
                            if (ndx, ndz) == (0, 0) {
                                // 自己不算自己的邻居。
                                count -= ((mask >> slot) & 1) as u8;
                            }
                            *state += count;
                        }
                    }
                }
                for (slot, state) in states.iter_mut().enumerate() {
                    if (own >> slot) & 1 == 1 {
                        *state |= HAS_DATA;
                    }
                }
                columns[next] = (column, states);
                next += 1;
            }
        }
        Self { columns }
    }
}

/// 一个区块柱 26 个光照段的存储。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LightColumn {
    layers: [Option<DataLayer>; LIGHT_SECTION_COUNT],
    /// `sectionStates`：0 表示这一段不存光照。
    states: [u8; LIGHT_SECTION_COUNT],
    /// `columnsWithSources`：这一列打开了光照。
    light_on: bool,
    /// 哪几段已经记进 [`LightStorage::changed`]，第 n 位是第 n 段。搬走时要清掉。
    changed: u32,
}

impl LightColumn {
    /// 什么都没存，光照也没打开。
    pub(crate) const fn empty() -> Self {
        Self {
            layers: [const { None }; LIGHT_SECTION_COUNT],
            states: [0; LIGHT_SECTION_COUNT],
            light_on: false,
            changed: 0,
        }
    }

    /// 同一份内容，光照打开。模板柱用它：世界里每个区块都已经算过光照了。
    pub(crate) fn lit(mut self) -> Self {
        self.light_on = true;
        self
    }

    /// 按磁盘上 26 段的光照数据和方块段重建一柱。
    ///
    /// 跨到相邻区块柱的邻居计数这里看不到：`sectionStates` 只按「哪些段存了
    /// 光照」记，存了光照的段标 `HAS_DATA`。放进 `ChunkStore` 时由
    /// `insert_column` 调用 [`rebind_area`](Self::rebind_area) 按目标世界重新登记。
    pub(crate) fn from_layers(
        layers: [Option<DataLayer>; LIGHT_SECTION_COUNT],
        has_blocks: &[bool; LIGHT_SECTION_COUNT],
        light_on: bool,
    ) -> Self {
        let states = std::array::from_fn(|slot| {
            let mut state = 0;
            if has_blocks[slot] {
                state |= HAS_DATA;
            }
            if layers[slot].is_some() && !has_blocks[slot] {
                // 这一段只因为邻居而存着光照：记一个邻居，保证状态非 0，
                // 否则光照引擎会以为这一段不该存在。
                state |= 1;
            }
            state
        });
        Self {
            layers,
            states,
            light_on,
            changed: 0,
        }
    }

    /// 这一柱的光照是不是已经打开。
    pub(crate) const fn light_on(&self) -> bool {
        self.light_on
    }

    /// 发布出去的光照数据一样：`states` 只在引擎里有，别人读的那一份不记。
    pub(crate) fn has_same_data(&self, other: &Self) -> bool {
        self.layers == other.layers
    }

    /// 这一柱最低的存着光照的段；一段都没有时是 `i32::MAX`。
    pub(crate) fn lowest_stored_section(&self) -> i32 {
        self.layers
            .iter()
            .position(Option::is_some)
            .map_or(i32::MAX, |slot| slot as i32 + MIN_LIGHT_SECTION)
    }

    /// `getDataLayer`：这一段存的光照，没存时是 `None`。
    pub(crate) fn layer(&self, section_y: i32) -> Option<&DataLayer> {
        self.layers[slot(section_y)?].as_ref()
    }

    /// `getTopSectionY`：这一列最高的存光照的段再往上一段。一段都没有时是 `lowest_section`。
    pub(crate) fn top_section_y(&self, lowest_section: i32) -> i32 {
        self.layers
            .iter()
            .rposition(Option::is_some)
            .map_or(lowest_section, |slot| slot as i32 + MIN_LIGHT_SECTION + 1)
    }

    /// `getLightValue`：读这一柱里的一格。
    /// 天空光在没存光照的段里读上面最近一段的最底一层。
    pub(crate) fn light_value(
        &self,
        layer: LightLayer,
        section_y: i32,
        x: usize,
        y: usize,
        z: usize,
    ) -> u8 {
        match layer {
            LightLayer::Block => self.layer(section_y).map_or(0, |data| data.get(x, y, z)),
            LightLayer::Sky => self.sky_light_value(section_y, x, y, z),
        }
    }

    fn sky_light_value(&self, section_y: i32, x: usize, y: usize, z: usize) -> u8 {
        let lowest = self.lowest_stored_section();
        if lowest == i32::MAX {
            return FULL_LIGHT;
        }
        let top = self.top_section_y(lowest);
        if section_y >= top {
            return FULL_LIGHT;
        }
        if let Some(data) = self.layer(section_y) {
            return data.get(x, y, z);
        }
        // 从世界最低的光照段起往上找，段以下的坐标直接跳过，免得走到很远的负数高度。
        for above in (section_y + 1).max(MIN_LIGHT_SECTION)..top {
            if let Some(data) = self.layer(above) {
                return data.get(x, 0, z);
            }
        }
        FULL_LIGHT
    }
}

/// 一层光照的全部区块段。
#[derive(Debug, Clone)]
pub(crate) struct LightStorage {
    layer: LightLayer,
    /// 没有自己一份的区块柱都是这个样子。
    template: LightColumn,
    columns: HashMap<ChunkPos, LightColumn>,
    /// 每个区块柱最后一次被写时拿到的版本号，见 [`revision`](Self::revision)。
    revisions: HashMap<ChunkPos, u64>,
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
    /// `None` = 无限超平坦（每个位置都存在）。`Some` = 已加载模式，
    /// 集合外的位置没有柱：26 个段状态全 0、没有数据层、也不接受任何写入。
    loaded: Option<BTreeSet<ChunkPos>>,
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
            template,
            columns: HashMap::new(),
            revisions: HashMap::new(),
            lowest_section,
            to_remove: BTreeSet::new(),
            affected: BTreeSet::new(),
            changed: Vec::new(),
            has_inconsistencies: false,
            last_revision: 0,
            loaded: None,
        }
    }

    /// 切到已加载模式：只有 `loaded` 里的坐标有柱。
    /// 无限模式传 `None`。已加载模式之外的位置读成 `EMPTY_COLUMN`。
    pub(crate) fn set_loaded(&mut self, loaded: Option<BTreeSet<ChunkPos>>) {
        self.loaded = loaded;
    }

    /// 增量登记一个已加载位置，避免每次加载都克隆整张集合。
    pub(crate) fn add_loaded(&mut self, pos: ChunkPos) {
        if let Some(set) = self.loaded.as_mut() {
            set.insert(pos);
        }
    }

    /// 增量注销一个已加载位置，并把它的光照柱真的删掉。
    pub(crate) fn remove_loaded(&mut self, pos: ChunkPos) {
        if let Some(set) = self.loaded.as_mut() {
            set.remove(&pos);
        }
        self.columns.remove(&pos);
        self.revisions.remove(&pos);
    }

    /// 这个位置在这层光照里有没有柱。无限模式恒为真（没写过的柱读模板）。
    /// 已加载模式下集合外恒为假，集合内按有没有存过自己的一份算。
    pub(crate) fn has_column(&self, pos: ChunkPos) -> bool {
        match &self.loaded {
            None => true,
            Some(set) => set.contains(&pos),
        }
    }

    pub(crate) fn layer_kind(&self) -> LightLayer {
        self.layer
    }

    fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.loaded.as_ref().is_none_or(|set| set.contains(&pos))
    }

    fn column(&self, pos: ChunkPos) -> &LightColumn {
        if !self.is_loaded(pos) {
            static EMPTY: std::sync::OnceLock<LightColumn> = std::sync::OnceLock::new();
            return EMPTY.get_or_init(LightColumn::empty);
        }
        self.columns.get(&pos).unwrap_or(&self.template)
    }

    /// 要写这个区块柱了，没有自己一份的话先从模板复制。所有写都经过这里，每次换一个新的版本号。
    /// 已加载模式下没加载的位置不能通过这里建柱（调用方先问 `is_loaded`）。
    fn column_mut(&mut self, pos: ChunkPos) -> &mut LightColumn {
        debug_assert!(self.is_loaded(pos), "没加载的位置不该写入光照柱");
        self.assign_revision(pos);
        let template = &self.template;
        self.columns.entry(pos).or_insert_with(|| template.clone())
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
        self.revisions.get(&pos).copied().unwrap_or_default()
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

    /// 这一柱的 26 个段状态，给测试核对登记/注销用。
    #[cfg(test)]
    pub(crate) fn states_at(&self, pos: ChunkPos) -> [u8; LIGHT_SECTION_COUNT] {
        self.column(pos).states
    }

    /// 每个有柱的位置，状态非 0 的段都存着数据层（测试用）。
    #[cfg(test)]
    pub(crate) fn debug_states_have_layers(&self) -> bool {
        self.columns.iter().all(|(_, column)| {
            (0..LIGHT_SECTION_COUNT)
                .all(|slot| column.states[slot] == 0 || column.layers[slot].is_some())
        })
    }

    /// `getDataLayer`：这一段存的光照，没存时是 `None`。
    pub(crate) fn layer(&self, section: SectionPos) -> Option<&DataLayer> {
        self.column(section.chunk()).layer(section.y)
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
        self.assign_revision(section.chunk());
        let template = &self.template;
        let column = self
            .columns
            .entry(section.chunk())
            .or_insert_with(|| template.clone());
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

    /// 区块束搬进或搬出一个世界之后，按**目标世界此刻的方块**重算 `pos` 和 8 个邻居柱
    /// 的 `sectionStates`，照官方 `ThreadedLevelLightEngine.initializeLight`（登记）和
    /// `updateChunkStatus`（注销）的定义：HAS_DATA 位是这一段自己有没有方块，
    /// 低 5 位是 26 个邻居段里有方块的个数。
    ///
    /// 盒子自带的 `sectionStates` 是来源世界的，单独插进一圈不同的邻居中间就不对了，
    /// 所以这里不用它，而是从 `chunks` 重新算整片 3×3 邻域。平坦世界里每个位置都有
    /// 同样的方块，算出来的值和模板相同，一整片相邻区块一起搬时一次都不写。
    ///
    /// 不经过 `putSectionState`：盒子里装的是已经算好的光照数据，它的
    /// `initializeSection` 会用默认值覆盖掉。这里对光照数据只做一件事——
    /// **状态非 0 而还没有数据层的段，补建一层**（内容照 `createDataLayer`）。
    /// 引擎只往存着数据层的段里传光（[`storing_light_for_section`](Self::storing_light_for_section)），
    /// 不补的话，因为新邻居才需要存光的段永远是黑的。
    ///
    /// 反过来，状态变成 0 的段**不丢数据层**。官方是在很远的地方才真正卸载区块
    /// （加载等级 45 以外），那里的邻居早就不发给客户端了；这里在紧挨着可见范围的
    /// 地方就把区块收走，邻居段里从它那边照过来的光要原样留着，等它回来时两边才对得上。
    /// 多留的数据层不影响读数；以后那一段的状态再从 0 变成非 0 时，
    /// `putSectionState` 照常重新初始化它。
    ///
    /// 返回这次补建了数据层的段。补建会换掉那一柱的版本号（区块包里多了一段）。
    pub(crate) fn rebind_area(&mut self, area: &SectionStatesAround) -> Vec<SectionPos> {
        let mut created = Vec::new();
        for &(column, states) in &area.columns {
            // 已加载模式只重算有柱的邻居；没加载的柱在算邻居计数时按
            // 「没有方块」计入，但不能给它建柱。
            if self.has_column(column) {
                self.rebind_column(column, states, &mut created);
            }
        }
        created
    }

    /// 把一柱的段状态换成 `states`，并给状态非 0 却没有数据层的段补建一层。
    fn rebind_column(
        &mut self,
        pos: ChunkPos,
        states: [u8; LIGHT_SECTION_COUNT],
        created: &mut Vec<SectionPos>,
    ) {
        let current = self.column(pos);
        let missing =
            |column: &LightColumn, slot: usize| states[slot] != 0 && column.layers[slot].is_none();
        if !(0..LIGHT_SECTION_COUNT).any(|slot| missing(current, slot)) {
            if current.states != states {
                // 只有计数变了：存着的光照数据一段没动，区块包不变，不换版本号。
                let template = &self.template;
                self.columns
                    .entry(pos)
                    .or_insert_with(|| template.clone())
                    .states = states;
            }
            return;
        }
        // 自下而上，和官方 `initializeLight` 逐段登记的顺序一致：天空光新建的一层
        // 照抄它上面最近一层的最底一片，上面没有就是全亮（或者这一列还没开光照时全暗）。
        self.column_mut(pos).states = states;
        for slot in 0..LIGHT_SECTION_COUNT {
            if !missing(self.column(pos), slot) {
                continue;
            }
            let section = SectionPos::new(pos.x, slot as i32 + MIN_LIGHT_SECTION, pos.z);
            let layer = self.create_data_layer(section);
            self.column_mut(pos).layers[slot] = Some(layer);
            self.lowest_section = self.lowest_section.min(section.y);
            created.push(section);
        }
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
        // 方块变化引起的邻居计数落到没加载的位置时直接丢掉：不能因此给
        // 没加载的位置造出一柱。它加载进来时 `rebind_area` 会按当时的方块重算。
        if !self.is_loaded(section.chunk()) {
            return;
        }
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
        self.column(pos).top_section_y(self.lowest_section)
    }

    /// 把整个区块柱搬走，这个位置回到模板柱，并换上这个存储里从没用过的版本号。
    /// 只在光照全部算完时调用：这时一轮一清的登记簿都是空的，柱里也没有没发布的段。
    pub(crate) fn take_column(&mut self, pos: ChunkPos) -> LightColumn {
        debug_assert!(self.is_settled(pos), "只在光照全部算完时搬走区块柱");
        let mut column = self
            .columns
            .remove(&pos)
            .unwrap_or_else(|| self.template.clone());
        column.changed = 0;
        self.assign_revision(pos);
        column
    }

    /// 放进一个区块柱，换上这个存储里从没用过的版本号。引擎和给别人读的那份各放一份。
    pub(crate) fn insert_column(&mut self, pos: ChunkPos, column: &LightColumn) {
        debug_assert!(column.changed == 0, "放进来的柱没有没发布的段");
        self.columns.insert(pos, column.clone());
        self.lowest_section = self.lowest_section.min(column.lowest_stored_section());
        self.assign_revision(pos);
    }

    /// 这个位置和它那一轮有关的登记簿都清干净了。
    fn is_settled(&self, pos: ChunkPos) -> bool {
        let none_here = |sections: &BTreeSet<SectionPos>| {
            !sections.iter().any(|section| section.chunk() == pos)
        };
        none_here(&self.to_remove)
            && none_here(&self.affected)
            && !self.changed.iter().any(|section| section.chunk() == pos)
            && self.column(pos).changed == 0
            && !self.has_inconsistencies
    }

    /// 给这个区块柱记下一个从没用过的版本号。
    fn assign_revision(&mut self, pos: ChunkPos) {
        self.last_revision += 1;
        self.revisions.insert(pos, self.last_revision);
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
        self.column(section.chunk())
            .light_value(self.layer, section.y, x, y, z)
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
