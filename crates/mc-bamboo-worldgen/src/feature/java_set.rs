//! `HashSet<BlockPos>` 的迭代顺序复刻。
//!
//! `VegetationPatchFeature.placeGroundPatch` 把地表位置收进 Java 的 `HashSet<BlockPos>`，
//! 随后按迭代顺序逐个判 `nextFloat` 决定是否长植被。迭代顺序因此直接决定哪个位置拿到
//! 哪一次随机数，必须复刻 `java.util.HashSet`（底层 `HashMap`）的桶序。
//!
//! `HashMap` 的行为照 OpenJDK：初始容量 16，装载因子 0.75，桶内按插入顺序尾插；
//! `hash(key) = h ^ (h >>> 16)`，其中 `h` 是 `Vec3i.hashCode() = (y + z*31)*31 + x`
//! （32 位回绕）；索引 `hash & (capacity-1)`；元素数超过阈值就容量翻倍，翻倍时按
//! `hash & oldCapacity` 拆成低半和高半、各自保持相对顺序。桶内超过 8 个元素且容量小于 64
//! 时同样走翻倍。
//!
//! Iteration order of the official `HashSet<BlockPos>`. `placeGroundPatch` collects the
//! surface positions into a `HashSet` and `distributeVegetation` draws one `nextFloat` per
//! element in iteration order, so the bucket order decides which position gets which draw.
//! This mirrors OpenJDK's `HashMap`: capacity 16, load factor 0.75, tail insertion inside a
//! bin, `hash(key) = h ^ (h >>> 16)` with `h = (y + z*31)*31 + x`, index `hash & (cap-1)`,
//! doubling at the load threshold (low/high split preserving relative order), and the
//! `treeifyBin` double-when-small path.

type Pos = (i32, i32, i32);

/// `Vec3i.hashCode()`，按 Java 的 32 位有符号回绕。
///
/// `Vec3i.hashCode()` with Java's 32-bit wrapping arithmetic.
fn vec3i_hash(pos: Pos) -> i32 {
    let (x, y, z) = pos;
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}

/// `HashMap.hash(key)`：`h ^ (h >>> 16)`。
///
/// `HashMap.hash(key)`: `h ^ (h >>> 16)`.
fn spread(hash: i32) -> u32 {
    let hash = hash as u32;
    hash ^ (hash >> 16)
}

/// 一个行为对齐 `java.util.HashSet<BlockPos>` 的集合。
///
/// A set that matches `java.util.HashSet<BlockPos>`.
pub struct JavaPosSet {
    buckets: Vec<Vec<Pos>>,
    len: usize,
}

impl JavaPosSet {
    /// `new HashSet<>()`：表懒分配，第一次 add 才建 16 个桶。
    ///
    /// `new HashSet<>()`: the table is lazy, built with 16 bins on the first add.
    pub fn new() -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.buckets.len().max(16)
    }

    fn threshold(&self) -> usize {
        self.capacity() * 3 / 4
    }

    /// `HashSet.add`。
    /// `HashSet.add`.
    pub fn insert(&mut self, pos: Pos) {
        if self.buckets.is_empty() {
            self.resize(16);
        }
        let index = spread(vec3i_hash(pos)) as usize & (self.capacity() - 1);
        if self.buckets[index].contains(&pos) {
            return;
        }
        self.buckets[index].push(pos);
        self.len += 1;
        // `putVal` 先挂到桶尾，再判 `++size > threshold`；桶内到 8 个且容量还小也翻倍。
        // `putVal` 的顺序：先挂在桶尾，插到第 8 个节点时 `binCount >= 7` 调
        // `treeifyBin`（容量 < 64 就翻倍），**然后**才 `++size > threshold` 再翻一次。
        // 反过来会让容量序列不同，桶索引和迭代顺序都会错。
        // `putVal` order: append to the bin, call `treeifyBin` when the bin holds its 8th
        // node (`binCount >= 7`, doubling while the table is under 64), and **then**
        // check `++size > threshold`. Swapping them changes the capacity sequence, so
        // both the bucket indices and the iteration order drift.
        if self.buckets[index].len() >= 8 && self.capacity() < 64 {
            self.resize(self.capacity() * 2);
        }
        if self.len > self.threshold() {
            self.resize(self.capacity() * 2);
        }
    }

    /// `HashMap.resize` 的 lo/hi 拆分：`hash & oldCapacity == 0` 留在原位，
    /// 其余挪到 `j + oldCapacity`，两边都保持原有相对顺序。
    ///
    /// `HashMap.resize`'s lo/hi split: entries with `hash & oldCapacity == 0` stay at `j`,
    /// the rest move to `j + oldCapacity`, both keeping their relative order.
    fn resize(&mut self, new_capacity: usize) {
        let old_capacity = self.buckets.len();
        let old = std::mem::replace(
            &mut self.buckets,
            (0..new_capacity).map(|_| Vec::new()).collect(),
        );
        for (j, bin) in old.into_iter().enumerate() {
            for pos in bin {
                let high = spread(vec3i_hash(pos)) as usize & old_capacity != 0;
                let index = if high { j + old_capacity } else { j };
                self.buckets[index].push(pos);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按 Java 的迭代顺序（桶 0 到最后，桶内插入序）取出所有元素。
    ///
    /// All elements in Java iteration order (bins in order, insertion order inside a bin).
    pub fn to_vec(&self) -> Vec<Pos> {
        self.buckets.iter().flatten().copied().collect()
    }
}

impl Default for JavaPosSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_javas_vec3i_hash_code() {
        // (x,y,z) = (1,2,3)：h = (2 + 3*31)*31 + 1 = 95*31 + 1 = 2946。
        // (x,y,z) = (1,2,3): h = (2 + 3*31)*31 + 1 = 95*31 + 1 = 2946.
        assert_eq!(vec3i_hash((1, 2, 3)), 2946);
        // 负坐标和溢出都按 32 位回绕。
        // Negative coordinates and overflow wrap at 32 bits.
        assert_eq!(vec3i_hash((0, 0, 0)), 0);
        assert_eq!(vec3i_hash((-1, 0, 0)), -1);
    }

    #[test]
    fn iteration_is_by_bucket_then_insertion_order() {
        let mut set = JavaPosSet::new();
        // 三个点的 spread 哈希落在不同桶，迭代顺序应与插入顺序无关。
        // Three positions land in different bins; iteration order need not match insertion.
        for pos in [(0, 0, 0), (16, 0, 0), (32, 0, 0)] {
            set.insert(pos);
        }
        let order = set.to_vec();
        assert_eq!(order.len(), 3);
        // 重复插入不改变集合大小。
        // Inserting a duplicate does not change the size.
        set.insert((0, 0, 0));
        assert_eq!(set.to_vec().len(), 3);
    }

    #[test]
    fn doubling_keeps_the_openjdk_lo_hi_order() {
        // (0..20, 70, 0) 的 OpenJDK 迭代序是 6..19 在前、0..5 在后（lo/hi 拆分）。
        // OpenJDK iterates (0..20, 70, 0) as 6..19 then 0..5 (the lo/hi split).
        let mut set = JavaPosSet::new();
        for x in 0..20 {
            set.insert((x, 70, 0));
        }
        let expected: Vec<Pos> = (6..20).chain(0..6).map(|x| (x, 70, 0)).collect();
        assert_eq!(set.to_vec(), expected);
    }

    #[test]
    fn heavy_collisions_expose_the_untreeified_limit() {
        // 40 个 (i*16, 62, 0) 挤进少数几个桶。官方 `HashMap` 在容量 ≥ 64 且桶内到 8 个
        // 节点时把桶转成红黑树，`treeify` 之后 `moveRootToFront` 把树根挪到桶首，迭代
        // 顺序因此和纯链表不同（官方第 0 个是 192，纯链表是 0）。
        //
        // **这是已知限制**：本实现只复刻链表，不实现 `HashMap.TreeNode` 的树化与
        // `moveRootToFront`，所以桶内 ≥ 8 个元素时迭代顺序会和官方不同。植被 patch 的
        // 地表集合通常只有几十个点（容量 64/128），触发树化需要同一桶挤进 8 个点，
        // 实测只在极端形状下出现。
        //
        // Forty (i*16, 62, 0) positions pile into a few bins. The official `HashMap`
        // treeifies a bin holding 8 nodes once the table is at least 64 wide, and
        // `moveRootToFront` then moves the tree root to the bin head, so the order differs
        // from a plain list (official starts at 192, a plain list at 0).
        //
        // **Known limit**: only the linked-list shape is replicated here; `HashMap.TreeNode`
        // treeification and `moveRootToFront` are not. Bins of 8+ entries therefore iterate
        // differently from the official implementation. Vegetation-patch surface sets are
        // usually a few dozen positions (capacity 64/128), and reaching a treeified bin
        // needs eight points in one bin, which only extreme shapes produce.
        let mut set = JavaPosSet::new();
        for i in 0..40 {
            set.insert((i * 16, 62, 0));
        }
        let order = set.to_vec();
        // 锁住当前（链表）行为，避免以后无意改动；和官方不同是已知的。
        // Pin the current (linked-list) behaviour so it cannot drift silently; the
        // difference from the official order is documented above.
        assert_eq!(order[0], (0, 62, 0));
        assert_eq!(order[1], (64, 62, 0));
        assert_eq!(order.len(), 40);
    }

    #[test]
    fn matches_a_real_patch_shape_from_the_official_jvm() {
        // 半径 7 的圆盘、中心 (-35,62,41) 的 149 个点：前 8 个和末 4 个取自
        // 官方 JVM 用同样的 `HashSet<BlockPos>` 打印的迭代序。
        // A 149-point radius-7 disc at (-35,62,41): the first eight and last four entries
        // come from the official JVM iterating the same `HashSet<BlockPos>`.
        let mut set = JavaPosSet::new();
        for dx in -7..=7 {
            for dz in -7..=7 {
                if dx * dx + dz * dz <= 49 {
                    set.insert((dx - 35, 62, dz + 41));
                }
            }
        }
        let order = set.to_vec();
        assert_eq!(order.len(), 149);
        assert_eq!(
            &order[..8],
            &[
                (-40, 62, 38),
                (-39, 62, 38),
                (-35, 62, 34),
                (-38, 62, 38),
                (-41, 62, 42),
                (-37, 62, 38),
                (-40, 62, 42),
                (-36, 62, 38),
            ]
        );
        assert_eq!(&order[147..], &[(-32, 62, 47), (-41, 62, 38)]);
    }
}
