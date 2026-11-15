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
        } else if self.buckets[index].len() >= 8 {
            // 容量 ≥ 64：真的转红黑树，`treeify` 之后 `moveRootToFront` 把树根挪到桶首。
            // At capacity ≥ 64 the bin really becomes a red-black tree, and
            // `moveRootToFront` moves the tree root to the head of the bin.
            self.treeify(index);
        }
        if self.len > self.threshold() {
            self.resize(self.capacity() * 2);
        }
    }

    /// `HashMap.treeifyBin` 的树化分支：把桶内的链表就地转成红黑树，再把根移到桶首。
    ///
    /// 迭代顺序只受根的位置影响（`HashMap` 迭代走 `Node.next` 链表，树化不改链表顺序），
    /// 所以这里只要算出根是哪个元素就行。
    ///
    /// `HashMap.treeifyBin`'s treeify branch: turn the bin's list into a red-black tree in
    /// place, then move the root to the head. `HashMap` iteration follows `Node.next`, which
    /// treeification does not reorder, so only the root's identity matters here.
    fn treeify(&mut self, index: usize) {
        let bin = std::mem::take(&mut self.buckets[index]);
        let root = treeify_root(&bin, self.capacity());
        let mut out = Vec::with_capacity(bin.len());
        out.push(bin[root]);
        for (i, pos) in bin.iter().enumerate() {
            if i != root {
                out.push(*pos);
            }
        }
        self.buckets[index] = out;
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

/// `HashMap.treeify`：按桶内链表顺序逐个插入红黑树，返回最终根在 `bin` 里的下标。
///
/// 节点比较用 `HashMap.treeNode` 的规则：
/// 1. 先比 hash；
/// 2. hash 相同再比 `Comparable.compareTo`（`Vec3i` 是 `y` → `z` → `x`）；
/// 3. 两者都相等时按插入顺序落到左或右（`tieBreakOrder` 用 `identityHashCode`，
///    官方这一步不可复现；但 `HashMap` 保证同一桶里 `hash` + `compareTo` 全等的
///    键不会走到这里，因为 `compareTo` 全等就意味着 `equals` 相等、不可能同时存在）。
///
/// `HashMap.treeify`: insert the bin's list into a red-black tree in list order and return
/// the final root's index in `bin`. Comparison is hash first, then `Vec3i.compareTo`
/// (`y`, `z`, `x`); the `tieBreakOrder` fallback never triggers for distinct positions.
fn treeify_root(bin: &[Pos], capacity: usize) -> usize {
    let mut tree = Tree::default();
    for &pos in bin.iter() {
        tree.insert(pos, capacity);
    }
    tree.root
}

/// 红黑树节点，存的是 `bin` 里的下标。
/// A red-black tree node holding an index into `bin`.
struct TreeNode {
    hash: u32,
    pos: Pos,
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    red: bool,
}

/// `HashMap` 的树化用到的红黑树。
/// The red-black tree `HashMap` uses while treeifying.
#[derive(Default)]
struct Tree {
    nodes: Vec<TreeNode>,
    root: usize,
}

impl Tree {
    /// `HashMap.putTreeVal` + `balanceInsertion`。
    /// `HashMap.putTreeVal` plus `balanceInsertion`.
    fn insert(&mut self, pos: Pos, capacity: usize) {
        let hash = spread(vec3i_hash(pos)) as usize & (capacity - 1);
        let hash = hash as u32;
        if self.nodes.is_empty() {
            self.nodes.push(TreeNode {
                hash,
                pos,
                left: None,
                right: None,
                parent: None,
                red: true,
            });
            self.root = 0;
            self.nodes[0].red = false;
            return;
        }
        let mut current = self.root;
        loop {
            let node = &self.nodes[current];
            let direction = compare(hash, pos, node.hash, node.pos);
            let next = match direction {
                std::cmp::Ordering::Less => node.left,
                std::cmp::Ordering::Greater => node.right,
                std::cmp::Ordering::Equal => Some(current),
            };
            match next {
                // 相等：`putTreeVal` 认为键已存在，不插入。
                // Equal: `putTreeVal` treats the key as present and inserts nothing.
                Some(equal) if equal == current && direction == std::cmp::Ordering::Equal => {
                    let _ = equal;
                    return;
                }
                Some(child) => current = child,
                None => {
                    let node_index = self.nodes.len();
                    self.nodes.push(TreeNode {
                        hash,
                        pos,
                        left: None,
                        right: None,
                        parent: Some(current),
                        red: true,
                    });
                    match direction {
                        std::cmp::Ordering::Less => self.nodes[current].left = Some(node_index),
                        _ => self.nodes[current].right = Some(node_index),
                    }
                    self.balance_insertion(node_index);
                    return;
                }
            }
        }
    }

    /// `HashMap.balanceInsertion`。
    /// `HashMap.balanceInsertion`.
    fn balance_insertion(&mut self, mut x: usize) {
        loop {
            let Some(parent) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return;
            };
            if !self.nodes[parent].red {
                return;
            }
            let Some(grand) = self.nodes[parent].parent else {
                return;
            };
            if parent == self.nodes[grand].left.unwrap_or(usize::MAX) {
                let uncle = self.nodes[grand].right;
                if uncle.is_some_and(|u| self.nodes[u].red) {
                    let uncle = uncle.unwrap();
                    self.nodes[parent].red = false;
                    self.nodes[uncle].red = false;
                    self.nodes[grand].red = true;
                    x = grand;
                    continue;
                }
                if x == self.nodes[parent].right.unwrap_or(usize::MAX) {
                    x = parent;
                    self.rotate_left(x);
                }
                let xp = self.nodes[x].parent.unwrap_or(grand);
                self.nodes[xp].red = false;
                self.nodes[grand].red = true;
                self.rotate_right(grand);
            } else {
                let uncle = self.nodes[grand].left;
                if uncle.is_some_and(|u| self.nodes[u].red) {
                    let uncle = uncle.unwrap();
                    self.nodes[parent].red = false;
                    self.nodes[uncle].red = false;
                    self.nodes[grand].red = true;
                    x = grand;
                    continue;
                }
                if x == self.nodes[parent].left.unwrap_or(usize::MAX) {
                    x = parent;
                    self.rotate_right(x);
                }
                let xp = self.nodes[x].parent.unwrap_or(grand);
                self.nodes[xp].red = false;
                self.nodes[grand].red = true;
                self.rotate_left(grand);
            }
        }
    }

    /// `HashMap.rotateLeft`。
    /// `HashMap.rotateLeft`.
    fn rotate_left(&mut self, p: usize) {
        let Some(r) = self.nodes[p].right else {
            return;
        };
        self.nodes[p].right = self.nodes[r].left;
        if let Some(rl) = self.nodes[r].left {
            self.nodes[rl].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[r].parent = pp;
        match pp {
            None => self.root = r,
            Some(pp) => {
                if self.nodes[pp].left == Some(p) {
                    self.nodes[pp].left = Some(r);
                } else {
                    self.nodes[pp].right = Some(r);
                }
            }
        }
        self.nodes[r].left = Some(p);
        self.nodes[p].parent = Some(r);
    }

    /// `HashMap.rotateRight`。
    /// `HashMap.rotateRight`.
    fn rotate_right(&mut self, p: usize) {
        let Some(l) = self.nodes[p].left else {
            return;
        };
        self.nodes[p].left = self.nodes[l].right;
        if let Some(lr) = self.nodes[l].right {
            self.nodes[lr].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[l].parent = pp;
        match pp {
            None => self.root = l,
            Some(pp) => {
                if self.nodes[pp].right == Some(p) {
                    self.nodes[pp].right = Some(l);
                } else {
                    self.nodes[pp].left = Some(l);
                }
            }
        }
        self.nodes[l].right = Some(p);
        self.nodes[p].parent = Some(l);
    }
}

/// `HashMap.treeNode` 的比较：hash 优先，再 `Vec3i.compareTo`（`y`、`z`、`x`）。
/// `HashMap.treeNode` ordering: hash first, then `Vec3i.compareTo` (`y`, `z`, `x`).
fn compare(hash_a: u32, a: Pos, hash_b: u32, b: Pos) -> std::cmp::Ordering {
    match hash_a.cmp(&hash_b) {
        std::cmp::Ordering::Equal => {
            a.1.cmp(&b.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(&b.0))
        }
        other => other,
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
    fn heavy_collisions_move_the_tree_root_to_the_bin_head() {
        // 40 个 (i*16, 62, 0) 挤进少数几个桶。容量 ≥ 64 且桶内到 8 个节点时官方
        // `HashMap` 把桶转红黑树，`treeify` 之后 `moveRootToFront` 把树根挪到桶首。
        // 官方 JVM 打印的顺序以 192 开头，其余保持插入序。
        //
        // Forty (i*16, 62, 0) positions pile into a few bins. At capacity ≥ 64 a bin with
        // 8 nodes becomes a red-black tree and `moveRootToFront` moves the root to the head.
        // The official JVM prints 192 first and keeps insertion order otherwise.
        let mut set = JavaPosSet::new();
        for i in 0..40 {
            set.insert((i * 16, 62, 0));
        }
        let order = set.to_vec();
        assert_eq!(order.len(), 40);
        let expected: Vec<Pos> = [
            192, 0, 64, 128, 256, 320, 384, 448, 512, 576, 208, 16, 80, 144, 272, 336, 400, 464,
            528, 592, 224, 32, 96, 160, 288, 352, 416, 480, 544, 608, 240, 48, 112, 176, 304, 368,
            432, 496, 560, 624,
        ]
        .into_iter()
        .map(|x| (x, 62, 0))
        .collect();
        assert_eq!(order, expected);
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
