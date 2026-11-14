//! 复现 Java `HashSet<BlockPos>` 的迭代顺序。
//!
//! 官方 `TreeFeature.place` 把树根、树干、树叶、装饰的位置放进 `HashSet<BlockPos>`，
//! 之后 `updateLeaves` 用 `iterator().next()` 取元素、`TreeDecorator.Context` 把集合
//! 拷成列表再按 Y 稳定排序。这两处的结果都取决于 `HashSet` 的迭代顺序，所以这里照
//! `java.util.HashMap` 的规则重做一份：容量 16 起步、负载因子 0.75、扩容时翻倍并
//! 保持桶内相对顺序、迭代按桶下标升序、桶内按插入顺序。
//!
//! 唯一没复现的是桶的树化（一个桶 9 个以上元素且表长至少 64 时变成红黑树，迭代
//! 顺序会变）。`BlockPos` 的哈希对一棵树里聚在一起的坐标几乎不冲突，真遇到时把
//! [`JavaHashSet::treeified`] 置位，调用方据此把这次放置标成不支持。
//!
//! Reproduces the iteration order of Java's `HashSet<BlockPos>`.
//!
//! Official `TreeFeature.place` collects root, trunk, foliage and decoration positions
//! in `HashSet<BlockPos>`; `updateLeaves` then takes elements with `iterator().next()`
//! and `TreeDecorator.Context` copies the sets into lists and stable-sorts them by Y.
//! Both results depend on the set's iteration order, so this follows
//! `java.util.HashMap`: capacity starts at 16, load factor 0.75, a resize doubles the
//! table and keeps the relative order inside a bin, iteration walks bins in index
//! order and each bin in insertion order.
//!
//! The one thing not reproduced is bin treeification (a bin with nine or more entries
//! in a table of at least 64 becomes a red-black tree and iterates differently).
//! `BlockPos` hashes of the clustered positions of one tree almost never collide; if
//! it does happen [`JavaHashSet::treeified`] is set so the caller can mark the
//! placement unsupported.

use super::Pos;

const INITIAL_CAPACITY: usize = 16;
/// `HashMap.TREEIFY_THRESHOLD`。
const TREEIFY_THRESHOLD: usize = 8;
/// `HashMap.MIN_TREEIFY_CAPACITY`。
const MIN_TREEIFY_CAPACITY: usize = 64;

/// 迭代顺序和 Java `HashSet<BlockPos>` 一致的集合。
///
/// A set that iterates in the same order as Java's `HashSet<BlockPos>`.
#[derive(Default)]
pub(crate) struct JavaHashSet {
    table: Vec<Vec<Pos>>,
    size: usize,
    /// 出现过本该树化的桶，迭代顺序不再可信。
    ///
    /// A bin that Java would have treeified was seen; the order is no longer trusted.
    pub treeified: bool,
}

impl JavaHashSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn contains(&self, pos: Pos) -> bool {
        if self.table.is_empty() {
            return false;
        }
        self.table[self.index_of(pos)].contains(&pos)
    }

    /// `HashSet.add`；已存在时返回 `false`，不改变顺序。
    ///
    /// `HashSet.add`; returns `false` and keeps the order when already present.
    pub fn insert(&mut self, pos: Pos) -> bool {
        if self.table.is_empty() {
            self.table = vec![Vec::new(); INITIAL_CAPACITY];
        }
        let index = self.index_of(pos);
        if self.table[index].contains(&pos) {
            return false;
        }
        // `putVal`：遍历到链尾时 binCount >= 7（链上已有 8 个）就调 `treeifyBin`；
        // 表长不到 64 时 `treeifyBin` 只是扩容。
        // `putVal`: appending to a bin that already holds eight entries calls
        // `treeifyBin`, which merely resizes while the table is shorter than 64.
        let crowded = self.table[index].len() >= TREEIFY_THRESHOLD;
        self.table[index].push(pos);
        if crowded {
            if self.table.len() < MIN_TREEIFY_CAPACITY {
                self.resize();
            } else {
                self.treeified = true;
            }
        }
        self.size += 1;
        if self.size > self.table.len() * 3 / 4 {
            self.resize();
        }
        true
    }

    pub fn remove(&mut self, pos: Pos) -> bool {
        if self.table.is_empty() {
            return false;
        }
        let index = self.index_of(pos);
        let Some(at) = self.table[index].iter().position(|&p| p == pos) else {
            return false;
        };
        self.table[index].remove(at);
        self.size -= 1;
        true
    }

    /// `iterator().next()`：迭代顺序里的第一个元素。
    ///
    /// `iterator().next()`: the first element in iteration order.
    pub fn first(&self) -> Option<Pos> {
        self.iter().next()
    }

    pub fn iter(&self) -> impl Iterator<Item = Pos> + '_ {
        self.table.iter().flatten().copied()
    }

    fn index_of(&self, pos: Pos) -> usize {
        (spread(hash_code(pos)) as u32 as usize) & (self.table.len() - 1)
    }

    /// `HashMap.resize`：表长翻倍，每个桶按 `hash & oldCap` 拆成低、高两条链，
    /// 两条链各自保持原来的相对顺序。重新按新表长分桶并保持遍历顺序，结果相同。
    ///
    /// `HashMap.resize`: the table doubles and each bin splits by `hash & oldCap`
    /// into a low and a high list that both keep their relative order. Re-binning in
    /// traversal order gives the same result.
    fn resize(&mut self) {
        let old = std::mem::take(&mut self.table);
        self.table = vec![Vec::new(); old.len() * 2];
        for pos in old.into_iter().flatten() {
            let index = self.index_of(pos);
            self.table[index].push(pos);
        }
    }
}

/// `Vec3i.hashCode`：`(y + z * 31) * 31 + x`，按 Java `int` 回绕。
///
/// `Vec3i.hashCode`: `(y + z * 31) * 31 + x` with Java `int` wrap-around.
fn hash_code(pos: Pos) -> i32 {
    pos.1
        .wrapping_add(pos.2.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.0)
}

/// `HashMap.hash`：`h ^ (h >>> 16)`。
fn spread(hash: i32) -> i32 {
    hash ^ ((hash as u32) >> 16) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_in_bin_order_not_insertion_order() {
        let mut set = JavaHashSet::new();
        // 哈希分别是 3、1、2，表长 16 时桶下标就是哈希。
        // Hashes are 3, 1 and 2; with a table of 16 the bin index equals the hash.
        for pos in [(3, 0, 0), (1, 0, 0), (2, 0, 0)] {
            assert!(set.insert(pos));
        }
        assert!(!set.insert((1, 0, 0)));
        let order: Vec<Pos> = set.iter().collect();
        assert_eq!(order, vec![(1, 0, 0), (2, 0, 0), (3, 0, 0)]);
        assert_eq!(set.first(), Some((1, 0, 0)));
    }

    #[test]
    fn colliding_entries_keep_insertion_order_and_split_on_resize() {
        let mut set = JavaHashSet::new();
        // x = 0 和 x = 16 在表长 16 时同桶，扩到 32 后分开。
        // x = 0 and x = 16 share a bin at 16 and separate once the table is 32.
        set.insert((16, 0, 0));
        set.insert((0, 0, 0));
        let order: Vec<Pos> = set.iter().collect();
        assert_eq!(order, vec![(16, 0, 0), (0, 0, 0)]);
        for x in 1..=11 {
            set.insert((x, 0, 0));
        }
        // 13 个元素超过 16 * 0.75，表长变 32。
        // Thirteen entries exceed 16 * 0.75, so the table is now 32.
        let order: Vec<Pos> = set.iter().collect();
        assert_eq!(order[0], (0, 0, 0));
        assert_eq!(order.last(), Some(&(16, 0, 0)));
        assert!(set.remove((16, 0, 0)));
        assert!(!set.contains((16, 0, 0)));
    }
}
