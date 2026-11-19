//! 按方块位置存的表，带按区块的索引（岛的实施计划 3.3、`docs/profiling.md` 第 18 节）。
//!
//! 容器、方块组件、命令方块、讲台上的书这几张表原来是 `BTreeMap<BlockPos, _>`。岛拆分要
//! 拿出「位置落在这些区块里」的条目；`BlockPos` 按 (x, y, z) 排序，同一个区块的条目
//! 不连续，只能扫整张表，耗时和岛的大小成正比。这里在表旁边维护「区块 → 这一区块里的
//! 位置」，拆分、合并和按区块装盒只碰要搬的那些位置。
//!
//! 读的时候就是一张普通的 `BTreeMap`（[`Deref`]），遍历顺序不变。写只能经过这里的方法：
//! 故意不实现 `DerefMut`，以后加新的写法时绕不过索引。索引和表逐项一致，调试构建在
//! 拆合前断言。
//!
//! Position-keyed tables with a per-chunk index. Containers, block
//! components, command blocks and lectern books used to be plain
//! `BTreeMap<BlockPos, _>`; since `BlockPos` orders by (x, y, z), one chunk's
//! entries are not contiguous and an island split had to scan the whole
//! table. A chunk → positions index lets splits, merges and chunk boxing
//! touch only the moved positions. Reads go through [`Deref`] to the map
//! (same iteration order); writes only through the methods here (no
//! `DerefMut`), so no write can bypass the index. Debug builds assert the
//! index matches before splits and merges.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Deref,
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_world::ticket::ChunkHasherBuilder;

/// 按方块位置存的表，带按区块的索引（见模块说明）。
///
/// A position-keyed table with a per-chunk index (see the module docs).
#[derive(Clone)]
pub(crate) struct PosMap<V> {
    map: BTreeMap<BlockPos, V>,
    /// 每个区块里有条目的位置，和 `map` 的键逐项一致。
    by_chunk: HashMap<ChunkPos, BTreeSet<BlockPos>, ChunkHasherBuilder>,
}

impl<V> Default for PosMap<V> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
            by_chunk: HashMap::default(),
        }
    }
}

impl<V: std::fmt::Debug> std::fmt::Debug for PosMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.map.fmt(f)
    }
}

impl<V> Deref for PosMap<V> {
    type Target = BTreeMap<BlockPos, V>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a, V> IntoIterator for &'a PosMap<V> {
    type Item = (&'a BlockPos, &'a V);
    type IntoIter = std::collections::btree_map::Iter<'a, BlockPos, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

impl<V> IntoIterator for PosMap<V> {
    type Item = (BlockPos, V);
    type IntoIter = std::collections::btree_map::IntoIter<BlockPos, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<V> FromIterator<(BlockPos, V)> for PosMap<V> {
    fn from_iter<I: IntoIterator<Item = (BlockPos, V)>>(iter: I) -> Self {
        let mut out = Self::default();
        out.extend(iter);
        out
    }
}

impl<V> Extend<(BlockPos, V)> for PosMap<V> {
    fn extend<I: IntoIterator<Item = (BlockPos, V)>>(&mut self, iter: I) {
        for (pos, value) in iter {
            self.insert(pos, value);
        }
    }
}

impl<V: PartialEq> PartialEq for PosMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<V> PosMap<V> {
    pub(crate) fn insert(&mut self, pos: BlockPos, value: V) -> Option<V> {
        let previous = self.map.insert(pos, value);
        if previous.is_none() {
            self.by_chunk.entry(pos.chunk()).or_default().insert(pos);
        }
        previous
    }

    pub(crate) fn remove(&mut self, pos: &BlockPos) -> Option<V> {
        let value = self.map.remove(pos)?;
        self.unindex(*pos);
        Some(value)
    }

    pub(crate) fn get_mut(&mut self, pos: &BlockPos) -> Option<&mut V> {
        self.map.get_mut(pos)
    }

    /// 这一格的条目，没有时用 `make` 新建一个放进去（`entry(pos).or_insert_with(make)`）。
    pub(crate) fn get_or_insert_with(&mut self, pos: BlockPos, make: impl FnOnce() -> V) -> &mut V {
        if !self.map.contains_key(&pos) {
            self.by_chunk.entry(pos.chunk()).or_default().insert(pos);
        }
        self.map.entry(pos).or_insert_with(make)
    }

    pub(crate) fn values_mut(&mut self) -> std::collections::btree_map::ValuesMut<'_, BlockPos, V> {
        self.map.values_mut()
    }

    /// 只留 `keep` 为真的条目，按位置升序问（同 `BTreeMap::retain`）。
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&BlockPos, &mut V) -> bool) {
        let mut dropped = Vec::new();
        self.map.retain(|pos, value| {
            let kept = keep(pos, value);
            if !kept {
                dropped.push(*pos);
            }
            kept
        });
        for pos in dropped {
            self.unindex(pos);
        }
    }

    fn unindex(&mut self, pos: BlockPos) {
        let chunk = pos.chunk();
        if let Some(positions) = self.by_chunk.get_mut(&chunk) {
            positions.remove(&pos);
            if positions.is_empty() {
                self.by_chunk.remove(&chunk);
            }
        }
    }

    /// 把位置落在 `chunks` 里的条目取出来，按区块升序、区块里按位置升序。只碰这些区块。
    ///
    /// Take every entry positioned in `chunks`, by ascending chunk and then
    /// position, touching only those chunks.
    pub(crate) fn take_chunks(&mut self, chunks: &BTreeSet<ChunkPos>) -> Vec<(BlockPos, V)> {
        debug_assert!(self.index_matches(), "按区块的索引和表对不上");
        let mut out = Vec::new();
        for chunk in chunks {
            let Some(positions) = self.by_chunk.remove(chunk) else {
                continue;
            };
            for pos in positions {
                if let Some(value) = self.map.remove(&pos) {
                    out.push((pos, value));
                }
            }
        }
        out
    }

    /// 岛拆分：位置落在 `chunks` 里的条目整批搬进一张新表，只碰这些区块。
    ///
    /// Island split: move every entry positioned in `chunks` into a new
    /// table, touching only those chunks.
    pub(crate) fn split_off_chunks(&mut self, chunks: &BTreeSet<ChunkPos>) -> Self {
        debug_assert!(self.index_matches(), "按区块的索引和表对不上");
        let mut out = Self::default();
        for chunk in chunks {
            let Some(positions) = self.by_chunk.remove(chunk) else {
                continue;
            };
            for &pos in &positions {
                if let Some(value) = self.map.remove(&pos) {
                    out.map.insert(pos, value);
                }
            }
            out.by_chunk.insert(*chunk, positions);
        }
        out
    }

    /// 岛合并：另一张表的条目并进来（两边的位置不重叠，断言）。小的那张并进大的那张：
    /// 两张都是按位置排好的表，谁并进谁结果一样。
    ///
    /// Island merge: the other table's entries join this one (positions never
    /// overlap, asserted). The smaller one goes into the larger; both are
    /// position-ordered, so the result is the same either way.
    pub(crate) fn absorb(&mut self, mut other: Self) {
        debug_assert!(self.index_matches() && other.index_matches());
        if other.map.len() > self.map.len() {
            std::mem::swap(self, &mut other);
        }
        let expected = self.map.len() + other.map.len();
        for (pos, value) in other.map {
            self.map.insert(pos, value);
        }
        debug_assert_eq!(self.map.len(), expected, "两张表都有同一个位置");
        for (chunk, positions) in other.by_chunk {
            self.by_chunk.entry(chunk).or_default().extend(positions);
        }
    }

    /// 索引和表逐项一致（调试构建的断言、测试用）。
    pub(crate) fn index_matches(&self) -> bool {
        let indexed: usize = self.by_chunk.values().map(BTreeSet::len).sum();
        indexed == self.map.len()
            && self.by_chunk.iter().all(|(chunk, positions)| {
                !positions.is_empty()
                    && positions
                        .iter()
                        .all(|pos| pos.chunk() == *chunk && self.map.contains_key(pos))
            })
    }

    /// 测试专用：从索引里漏掉一个位置（改坏确认：漏一个索引项，拆分要被核对抓到）。
    #[cfg(test)]
    pub(crate) fn forget_index_for_test(&mut self, pos: BlockPos) {
        self.unindex(pos);
    }
}
