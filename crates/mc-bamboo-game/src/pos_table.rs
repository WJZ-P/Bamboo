//! 按区块分存的方块位置表，见 `docs/world-model-design.md` 第 4.2 节。
//!
//! 对外看起来是一张 `BlockPos → V` 的表，内部是「区块坐标 → 这个区块那份小表」。
//! 每个区块可以整盒拿走、整盒放回（`take_chunk` / `insert_chunk`），供 region 拆合
//! 和以后的单元并行用；region 级不另留一份全局有序的位置集合。
//!
//! `BlockPos` 的 `Ord` 是 (x, y, z)，所以不同区块 x 坐标（cx）的条目不会交错，
//! 只有 cx 相同、cz 不同的区块之间才会交错。全表遍历按 cx 升序，同一个 cx 里对
//! 各 cz 的小表做 k 路归并，得到的顺序和原来整张 `BTreeMap` 逐位相同，不排序全表。
//! Split a positional table into one small map per chunk. Per-chunk tables can be
//! taken and inserted as a whole. A full traversal still yields the exact old
//! global `BTreeMap<BlockPos, V>` order: sort the cx groups, then k-way merge the
//! cz maps inside one cx group (the (x, y, z) order only interleaves those).

use std::{
    collections::{BTreeMap, HashMap, btree_map},
    hash::{BuildHasherDefault, Hasher},
    ops::Index,
};

use mc_bamboo_core::{BlockPos, ChunkPos};

/// `ChunkPos` 的两个小整数混成一个 64 位值；标准库的 SipHash 对按位置查太贵。
/// 和 `mc-bamboo-world::ticket::ChunkHasherBuilder` 是同一套做法，这里不依赖世界层。
#[derive(Default)]
struct PosHasher(u64);

impl Hasher for PosHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_i32(&mut self, value: i32) {
        let mixed = (value as u32 as u64)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .rotate_left(29);
        self.0 = (self.0 ^ mixed).wrapping_mul(0xff51_afd7_ed55_8ccd);
        self.0 ^= self.0 >> 33;
    }
}

type Chunks<V> = HashMap<ChunkPos, BTreeMap<BlockPos, V>, BuildHasherDefault<PosHasher>>;

/// 一张按区块分存的 `BlockPos → V` 表。
///
/// 只在世界线程上用；`Clone` 给存档指纹和单元测试用。遍历顺序保持全局位置升序。
#[derive(Clone)]
pub(crate) struct PosTable<V> {
    chunks: Chunks<V>,
    len: usize,
}

impl<V> Default for PosTable<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> PosTable<V> {
    pub(crate) const fn new() -> Self {
        Self {
            chunks: HashMap::with_hasher(BuildHasherDefault::new()),
            len: 0,
        }
    }

    #[allow(dead_code, reason = "按位置查和全表遍历之外，测试和指纹会用")]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code, reason = "空盒判定在测试和原样检查里用")]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn contains_key(&self, pos: &BlockPos) -> bool {
        self.chunks
            .get(&pos.chunk())
            .is_some_and(|cells| cells.contains_key(pos))
    }

    pub(crate) fn get(&self, pos: &BlockPos) -> Option<&V> {
        self.chunks.get(&pos.chunk())?.get(pos)
    }

    pub(crate) fn get_mut(&mut self, pos: &BlockPos) -> Option<&mut V> {
        self.chunks.get_mut(&pos.chunk())?.get_mut(pos)
    }

    /// 拿到这一格的可变入口。调用方总是接 `or_*`，不会留下空区块。
    pub(crate) fn entry(&mut self, pos: BlockPos) -> PosEntry<'_, V> {
        let cells = self.chunks.entry(pos.chunk()).or_default();
        let occupies = matches!(cells.entry(pos), btree_map::Entry::Occupied(_));
        PosEntry {
            table: self,
            pos,
            occupies,
        }
    }

    /// 有旧值就还回来，没有就记一条新的。
    pub(crate) fn insert(&mut self, pos: BlockPos, value: V) -> Option<V> {
        let cells = self.chunks.entry(pos.chunk()).or_default();
        let old = cells.insert(pos, value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    pub(crate) fn remove(&mut self, pos: &BlockPos) -> Option<V> {
        let chunk = pos.chunk();
        let cells = self.chunks.get_mut(&chunk)?;
        let old = cells.remove(pos);
        if old.is_some() {
            self.len -= 1;
            if cells.is_empty() {
                self.chunks.remove(&chunk);
            }
        }
        old
    }

    /// 所有位置，按全局位置升序。
    pub(crate) fn keys(&self) -> impl Iterator<Item = &BlockPos> + '_ {
        self.iter().map(|(pos, _)| pos)
    }

    /// 按全局位置升序遍历，和原来的 `BTreeMap<BlockPos, V>` 逐位相同。
    pub(crate) fn iter(&self) -> Iter<'_, V> {
        let mut by_cx: BTreeMap<i32, Vec<btree_map::Iter<'_, BlockPos, V>>> = BTreeMap::new();
        for (chunk, cells) in &self.chunks {
            if !cells.is_empty() {
                by_cx.entry(chunk.x).or_default().push(cells.iter());
            }
        }
        let groups: Vec<_> = by_cx.into_values().collect();
        Iter::new(groups)
    }

    /// 按全局位置升序遍历可变值，语义和 `BTreeMap::values_mut` 一样。
    pub(crate) fn values_mut(&mut self) -> ValuesMut<'_, V> {
        // `chunks.iter_mut()` 给的是每个区块小表各自的可变借用，互不重叠；
        // 收进按 cx 分组的 `IterMut` 之后，归并的每一步只把一个游标的 &mut V 借出去。
        let mut by_cx: BTreeMap<i32, Vec<btree_map::IterMut<'_, BlockPos, V>>> = BTreeMap::new();
        for (chunk, cells) in self.chunks.iter_mut() {
            if !cells.is_empty() {
                by_cx.entry(chunk.x).or_default().push(cells.iter_mut());
            }
        }
        let groups: Vec<_> = by_cx.into_values().collect();
        ValuesMut::new(groups)
    }

    /// `BTreeMap::retain`，按全局位置升序调用 `keep`。
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&BlockPos, &mut V) -> bool) {
        // 先收集要删的键，避免在归并游标里改表。开销和条目数成正比，不排序。
        let mut removed: Vec<BlockPos> = Vec::new();
        for (pos, value) in self.values_mut() {
            if !keep(pos, value) {
                removed.push(*pos);
            }
        }
        for pos in removed {
            self.remove(&pos);
        }
    }

    /// 整盒拿走一个区块的内容。没有内容的区块返回空表。
    pub(crate) fn take_chunk(&mut self, chunk: ChunkPos) -> BTreeMap<BlockPos, V> {
        let Some(cells) = self.chunks.remove(&chunk) else {
            return BTreeMap::new();
        };
        self.len -= cells.len();
        cells
    }

    /// 这个区块的那份小表，只读。给存档快照用。
    pub(crate) fn chunk(&self, chunk: ChunkPos) -> Option<&BTreeMap<BlockPos, V>> {
        self.chunks.get(&chunk)
    }

    /// 整盒放回一个区块的内容，调用方保证这个位置原来没有条目。
    /// 万一已有条目，把旧的那盒还回来，不丢数据。
    pub(crate) fn insert_chunk(
        &mut self,
        chunk: ChunkPos,
        cells: BTreeMap<BlockPos, V>,
    ) -> BTreeMap<BlockPos, V> {
        if cells.is_empty() {
            return cells;
        }
        debug_assert!(cells.keys().all(|pos| pos.chunk() == chunk));
        self.len += cells.len();
        match self.chunks.insert(chunk, cells) {
            Some(old) => {
                self.len -= old.len();
                old
            }
            None => BTreeMap::new(),
        }
    }

    /// 只给整批读写用的克隆视图：按全局位置升序收集新的 `BTreeMap`。
    #[cfg(test)]
    pub(crate) fn to_btree(&self) -> BTreeMap<BlockPos, V>
    where
        V: Clone,
    {
        self.iter()
            .map(|(pos, value)| (*pos, value.clone()))
            .collect()
    }
}

/// `entry` 的返回值：只在缺条目时建一份新值，同时把总数加上去。
pub(crate) struct PosEntry<'a, V> {
    table: &'a mut PosTable<V>,
    pos: BlockPos,
    occupies: bool,
}

impl<'a, V> PosEntry<'a, V> {
    #[allow(dead_code, reason = "测试和未来的写入口会用")]
    pub(crate) fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        self.or_insert_with(V::default)
    }

    /// 消费入口，把整表的借用交出去，返回的引用活得和 `entry` 借的表一样长。
    pub(crate) fn or_insert_with(mut self, make: impl FnOnce() -> V) -> &'a mut V {
        if !self.occupies {
            self.table.insert(self.pos, make());
            self.occupies = true;
        }
        self.table.get_mut(&self.pos).expect("刚刚插入的条目一定在")
    }

    /// 已有条目时改它，缺条目时什么也不做；链式接 `or_*`。
    pub(crate) fn and_modify(self, f: impl FnOnce(&mut V)) -> Self {
        if let Some(value) = self.table.get_mut(&self.pos) {
            f(value);
        }
        self
    }
}

/// `iter()` 的游标：每个 cz 小表当前最小的那一项。
struct Cursor<'a, V> {
    pos: &'a BlockPos,
    value: &'a V,
}

type ReadGroup<'a, V> = Vec<btree_map::Iter<'a, BlockPos, V>>;
type WriteGroup<'a, V> = Vec<btree_map::IterMut<'a, BlockPos, V>>;

/// 全局位置升序的只读遍历。
///
/// `cursors[i]` 始终对应 `groups[group][i]`：游标用完的小表留一个 `None` 槽位，
/// 不能用 `swap_remove` 把下标错开。
pub(crate) struct Iter<'a, V> {
    groups: Vec<ReadGroup<'a, V>>,
    group: usize,
    cursors: Vec<Option<Cursor<'a, V>>>,
}

impl<'a, V> Iter<'a, V> {
    fn new(mut groups: Vec<ReadGroup<'a, V>>) -> Self {
        let cursors = groups
            .first_mut()
            .map(|group| {
                group
                    .iter_mut()
                    .map(|iter| iter.next().map(|(pos, value)| Cursor { pos, value }))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            groups,
            group: 0,
            cursors,
        }
    }

    fn open_group(&mut self) {
        self.cursors = self.groups[self.group]
            .iter_mut()
            .map(|iter| iter.next().map(|(pos, value)| Cursor { pos, value }))
            .collect();
    }
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a BlockPos, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let best = self
                .cursors
                .iter()
                .enumerate()
                .filter_map(|(index, cursor)| cursor.as_ref().map(|c| (index, *c.pos)))
                .min_by_key(|(_, pos)| *pos)
                .map(|(index, _)| index);
            if let Some(index) = best {
                let cursor = self.cursors[index].take().expect("过滤时只挑有值的游标");
                self.cursors[index] = self.groups[self.group][index]
                    .next()
                    .map(|(pos, value)| Cursor { pos, value });
                return Some((cursor.pos, cursor.value));
            }
            self.group += 1;
            if self.group >= self.groups.len() {
                return None;
            }
            self.open_group();
        }
    }
}

/// `values_mut()` 的游标：`value` 用 `Option`，借出去以后补上该小表的下一项。
struct MutCursor<'a, V> {
    pos: &'a BlockPos,
    value: Option<&'a mut V>,
}

/// 全局位置升序的可变遍历。
///
/// 各 cz 小表来自 `HashMap` 的不同条目，键互不相等；每次只把一个游标的 `&mut V`
/// 借出去，同时从对应的 `IterMut` 取下一项，所以不会出现两个指向同一格的可变引用。
/// 组与组之间严格串行：上一组游标全空了才切到下一组。
pub(crate) struct ValuesMut<'a, V> {
    groups: Vec<WriteGroup<'a, V>>,
    group: usize,
    cursors: Vec<Option<MutCursor<'a, V>>>,
}

impl<'a, V> ValuesMut<'a, V> {
    fn new(mut groups: Vec<WriteGroup<'a, V>>) -> Self {
        let cursors = groups
            .first_mut()
            .map(|group| {
                group
                    .iter_mut()
                    .map(|iter| {
                        iter.next().map(|(pos, value)| MutCursor {
                            pos,
                            value: Some(value),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            groups,
            group: 0,
            cursors,
        }
    }

    fn open_group(&mut self) {
        self.cursors = self.groups[self.group]
            .iter_mut()
            .map(|iter| {
                iter.next().map(|(pos, value)| MutCursor {
                    pos,
                    value: Some(value),
                })
            })
            .collect();
    }
}

impl<'a, V> Iterator for ValuesMut<'a, V> {
    type Item = (&'a BlockPos, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let best = self
                .cursors
                .iter()
                .enumerate()
                .filter_map(|(index, cursor)| cursor.as_ref().map(|c| (index, *c.pos)))
                .min_by_key(|(_, pos)| *pos)
                .map(|(index, _)| index);
            if let Some(index) = best {
                let mut cursor = self.cursors[index].take().expect("过滤时只挑有值的游标");
                let pos = cursor.pos;
                let value = cursor.value.take().expect("有值的游标一定有 value");
                self.cursors[index] =
                    self.groups[self.group][index]
                        .next()
                        .map(|(next_pos, next_value)| MutCursor {
                            pos: next_pos,
                            value: Some(next_value),
                        });
                return Some((pos, value));
            }
            self.group += 1;
            if self.group >= self.groups.len() {
                return None;
            }
            self.open_group();
        }
    }
}

impl<V> Index<&BlockPos> for PosTable<V> {
    type Output = V;

    fn index(&self, pos: &BlockPos) -> &Self::Output {
        self.get(pos).expect("这个位置有条目")
    }
}

impl<V: std::fmt::Debug> std::fmt::Debug for PosTable<V> {
    /// 按全局位置升序打印，和原来 `BTreeMap` 的 `{:?}` 逐字相同，测试快照不用改。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V: PartialEq> PartialEq for PosTable<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

impl<V: Eq> Eq for PosTable<V> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(x, y, z)
    }

    /// 跨多个 cx、多个 cz 的位置，含负坐标。
    fn positions() -> Vec<BlockPos> {
        let mut out = Vec::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                for &(dx, y, dz) in &[(1, 3, 2), (15, 3, 2), (1, 70, 15), (0, -5, 0)] {
                    out.push(key(cx * 16 + dx, y, cz * 16 + dz));
                }
            }
        }
        out
    }

    #[test]
    fn iteration_is_global_block_pos_order_not_chunk_then_pos() {
        let mut table = PosTable::new();
        let mut want = BTreeMap::new();
        // 故意按区块顺序插，和全局位置顺序不同。
        for pos in positions() {
            table.insert(pos, pos.x * 1000 + pos.z);
            want.insert(pos, pos.x * 1000 + pos.z);
        }
        let got: Vec<_> = table.iter().map(|(pos, value)| (*pos, *value)).collect();
        assert_eq!(got, want.into_iter().collect::<Vec<_>>());
        assert_eq!(table.len(), positions().len());
        assert_eq!(table.to_btree().len(), positions().len());
    }

    /// 改坏确认的对照组：故意按「区块再按位置」排一遍，它和全局顺序必须不同，
    /// 上面的 `iteration_is_global_block_pos_order_not_chunk_then_pos` 才能真的报出改坏。
    #[test]
    fn chunk_then_pos_order_is_detectably_different() {
        let mut table = PosTable::new();
        for pos in positions() {
            table.insert(pos, pos.x);
        }
        let global: Vec<_> = table.keys().copied().collect();
        let mut chunked: Vec<_> = positions();
        chunked.sort_unstable_by_key(|pos| (pos.chunk(), *pos));
        assert_ne!(global, chunked, "这份数据要能区分两种顺序");

        // 同 cx 不同 cz 的格子确实交错，不能只按 cz 分组顺序拼起来。
        let cx0: Vec<_> = global
            .iter()
            .copied()
            .filter(|pos| pos.chunk().x == 0)
            .collect();
        let mut by_cz: Vec<_> = cx0.clone();
        by_cz.sort_unstable_by_key(|pos| (pos.chunk().z, *pos));
        assert_ne!(cx0, by_cz);
    }

    #[test]
    fn values_mut_keeps_global_order_and_applies_writes() {
        let mut table = PosTable::new();
        for pos in positions() {
            table.insert(pos, 0);
        }
        for (pos, value) in table.values_mut() {
            *value = pos.x * 1000 + pos.z;
        }
        let mut want = BTreeMap::new();
        for pos in positions() {
            want.insert(pos, pos.x * 1000 + pos.z);
        }
        assert_eq!(table.to_btree(), want);
    }

    #[test]
    fn retain_visits_in_global_order_and_removes() {
        let mut table = PosTable::new();
        for pos in positions() {
            table.insert(pos, pos.x);
        }
        let mut seen = Vec::new();
        table.retain(|pos, _| {
            seen.push(*pos);
            *pos > BlockPos::new(0, 0, 0)
        });
        let mut want: Vec<_> = positions()
            .into_iter()
            .filter(|pos| *pos > BlockPos::new(0, 0, 0))
            .collect();
        want.sort_unstable();
        let mut sorted = positions();
        sorted.sort_unstable();
        assert_eq!(seen, sorted);
        assert_eq!(table.iter().map(|(pos, _)| *pos).collect::<Vec<_>>(), want);
        assert_eq!(table.len(), want.len());
    }

    #[test]
    fn taking_and_inserting_a_chunk_moves_the_whole_box() {
        let mut table = PosTable::new();
        for pos in positions() {
            table.insert(pos, pos.x);
        }
        let chunk = ChunkPos::new(1, -1);
        let mut want: Vec<_> = positions()
            .into_iter()
            .filter(|pos| pos.chunk() == chunk)
            .collect();
        want.sort_unstable();
        let boxed = table.take_chunk(chunk);
        assert_eq!(boxed.keys().copied().collect::<Vec<_>>(), want);
        assert_eq!(
            table.len(),
            positions().len() - want.len(),
            "取走的条目从总数里扣掉"
        );
        assert!(table.chunk(chunk).is_none());
        assert!(table.insert_chunk(chunk, boxed).is_empty());
        assert_eq!(table.len(), positions().len());
        assert_eq!(table.chunk(chunk).unwrap().len(), want.len());
    }

    #[test]
    fn empty_chunk_boxes_are_not_kept() {
        let mut table = PosTable::new();
        assert!(table.take_chunk(ChunkPos::new(0, 0)).is_empty());
        table.insert(key(1, 2, 3), 7);
        table.remove(&key(1, 2, 3));
        assert!(table.is_empty());
        assert!(table.chunks.is_empty(), "空区块不留键");
        assert_eq!(
            table
                .insert_chunk(ChunkPos::new(0, 0), BTreeMap::new())
                .len(),
            0
        );
        assert!(table.chunks.is_empty());
    }

    #[test]
    fn entry_and_index_behave_like_a_map() {
        let mut table = PosTable::new();
        table.entry(key(33, 1, 2)).or_insert_with(|| 5);
        *table.entry(key(33, 1, 2)).or_default() += 1;
        assert_eq!(table[&key(33, 1, 2)], 6);
        assert_eq!(table.len(), 1);
        assert_eq!(table.remove(&key(33, 1, 2)), Some(6));
        assert!(table.is_empty());
    }

    #[test]
    fn debug_output_is_a_globally_ordered_map() {
        let mut table = PosTable::new();
        let mut want = BTreeMap::new();
        for pos in positions().into_iter().take(6) {
            table.insert(pos, pos.x);
            want.insert(pos, pos.x);
        }
        assert_eq!(format!("{table:?}"), format!("{want:?}"));
        assert_eq!(table, table.clone());
    }
}
