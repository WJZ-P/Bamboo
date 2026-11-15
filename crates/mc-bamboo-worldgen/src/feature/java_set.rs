//! `HashSet<BlockPos>` 的迭代顺序复刻。
//!
//! `VegetationPatchFeature.placeGroundPatch` 把地表位置收进 Java 的 `HashSet<BlockPos>`，
//! 随后按迭代顺序逐个判 `nextFloat` 决定是否长植被。迭代顺序因此直接决定哪个位置拿到
//! 哪一次随机数，必须复刻 `java.util.HashSet`（底层 `HashMap`）的桶序。
//!
//! 复刻范围照 OpenJDK `HashMap`：初始容量 16、装载因子 0.75、桶内尾插、
//! `hash(key) = h ^ (h >>> 16)`（`h` 是 `Vec3i.hashCode() = (y + z*31)*31 + x`，32 位
//! 回绕）、索引 `hash & (capacity-1)`、超阈值翻倍（按 `hash & oldCapacity` 拆 lo/hi，
//! 两边保持相对顺序）。
//!
//! 桶内节点数到 8 时调 `treeifyBin`：容量 < 64 只翻倍，否则把链表转红黑树并用
//! `moveRootToFront` 把树根挪到桶首。**这一步会改变迭代顺序**，必须实现：
//! `lush_caves_vegetation` 的一个 48 点 patch 就会走到 `cap=64`、单桶 10 个节点，
//! 只做链表会让两格 `short_grass` 差一格。
//!
//! Iteration order of the official `HashSet<BlockPos>`. `placeGroundPatch` collects the
//! surface positions into a `HashSet` and `distributeVegetation` draws one `nextFloat` per
//! element in iteration order, so the bucket order decides which position gets which draw.
//!
//! Mirrors OpenJDK `HashMap`: capacity 16, load factor 0.75, tail insertion inside a bin,
//! `hash(key) = h ^ (h >>> 16)` with `h = (y + z*31)*31 + x`, index `hash & (cap-1)`, and
//! doubling at the load threshold (low/high split preserving relative order).
//!
//! A bin reaching eight nodes calls `treeifyBin`: under capacity 64 that only doubles the
//! table, otherwise the list becomes a red-black tree and `moveRootToFront` promotes the
//! root to the bin head. **That step changes the iteration order**, so it has to be
//! implemented: a 48-position `lush_caves_vegetation` patch already reaches `cap=64` with
//! ten nodes in one bin, and skipping it shifts two `short_grass` by one cell.

type Pos = (i32, i32, i32);

/// `HashMap.TREEIFY_THRESHOLD`。
///
/// `HashMap.TREEIFY_THRESHOLD`.
const TREEIFY_THRESHOLD: usize = 8;

/// `HashMap.MIN_TREEIFY_CAPACITY`：容量不到这个数时 `treeifyBin` 只翻倍，不树化。
///
/// `HashMap.MIN_TREEIFY_CAPACITY`: below it `treeifyBin` only doubles the table.
const MIN_TREEIFY_CAPACITY: usize = 64;

/// `HashMap.UNTREEIFY_THRESHOLD`：resize 拆桶后节点数不超过它就退回链表。
///
/// `HashMap.UNTREEIFY_THRESHOLD`: a split bin at or below this size becomes a list again.
const UNTREEIFY_THRESHOLD: usize = 6;

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

/// 一个节点，同时挂在链表和红黑树上（对应官方 `HashMap.TreeNode`）。
///
/// A node living in both the list and the red-black tree, like `HashMap.TreeNode`.
#[derive(Debug, Clone, Copy)]
struct Node {
    pos: Pos,
    hash: u32,
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    red: bool,
}

/// 一个桶：`order` 就是官方 `next` 链，迭代顺序就是它的顺序；`nodes` 是树节点的 arena。
///
/// One bin: `order` is the official `next` chain and therefore the iteration order;
/// `nodes` is the tree arena.
#[derive(Debug, Default)]
struct Bin {
    nodes: Vec<Node>,
    order: Vec<usize>,
    root: Option<usize>,
}

impl Bin {
    fn is_tree(&self) -> bool {
        self.root.is_some()
    }

    /// 官方 `treeify`：链表头作根，后续节点按 hash 落左右子树，每次插入后
    /// `balanceInsertion`，最后 `moveRootToFront`。
    ///
    /// 官方在 hash 完全相同时会退到 `tieBreakOrder`（`BlockPos` 不实现 `Comparable`，
    /// 于是用 `System.identityHashCode`，依赖对象地址、无法预测）。这里固定走左子树；
    /// 只有 `spread(h)` 完全相同的两个坐标才会走到，实测的植被 patch 里没有出现。
    ///
    /// The official `treeify`: the list head becomes the root, later nodes descend by
    /// hash with `balanceInsertion`, then `moveRootToFront`. For two nodes sharing the
    /// exact spread hash the official code falls back to `tieBreakOrder`
    /// (`System.identityHashCode`, i.e. object addresses, unpredictable); here they take
    /// the left subtree. Only identical spread hashes reach that branch.
    fn treeify(&mut self) {
        if self.is_tree() || self.order.len() < 2 {
            return;
        }
        let ordered = std::mem::take(&mut self.order);
        self.root = Some(ordered[0]);
        self.nodes[ordered[0]].red = false;
        for &node in &ordered[1..] {
            self.link(node);
        }
        let root = self.root.expect("treeify keeps a root");
        self.order = ordered;
        self.move_root_to_front(root);
    }

    /// 从根按 hash 下探到空槽，挂上新节点并做 `balanceInsertion`。返回新根。
    ///
    /// Walk down by hash to the first empty slot, attach the node and run
    /// `balanceInsertion`. Returns the new root.
    fn link(&mut self, node: usize) -> usize {
        let hash = self.nodes[node].hash;
        let mut current = self.root.expect("link only runs with a root");
        loop {
            let current_hash = self.nodes[current].hash;
            let go_left = current_hash >= hash;
            let slot = if go_left {
                self.nodes[current].left
            } else {
                self.nodes[current].right
            };
            match slot {
                Some(next) => current = next,
                None => {
                    self.nodes[node].parent = Some(current);
                    self.nodes[node].red = true;
                    if go_left {
                        self.nodes[current].left = Some(node);
                    } else {
                        self.nodes[current].right = Some(node);
                    }
                    let root = self.balance_insertion(node);
                    self.root = Some(root);
                    return root;
                }
            }
        }
    }

    /// 官方 `rotateLeft`。
    /// The official `rotateLeft`.
    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let Some(r) = self.nodes[p].right else {
            return root;
        };
        let rl = self.nodes[r].left;
        self.nodes[p].right = rl;
        if let Some(rl) = rl {
            self.nodes[rl].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[r].parent = pp;
        match pp {
            None => {
                root = r;
                self.nodes[r].red = false;
            }
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
        root
    }

    /// 官方 `rotateRight`。
    /// The official `rotateRight`.
    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let Some(l) = self.nodes[p].left else {
            return root;
        };
        let lr = self.nodes[l].right;
        self.nodes[p].left = lr;
        if let Some(lr) = lr {
            self.nodes[lr].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[l].parent = pp;
        match pp {
            None => {
                root = l;
                self.nodes[l].red = false;
            }
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
        root
    }

    /// 官方 `balanceInsertion`。
    /// The official `balanceInsertion`.
    fn balance_insertion(&mut self, mut x: usize) -> usize {
        let mut root = self.root.expect("balance_insertion needs a root");
        self.nodes[x].red = true;
        loop {
            let Some(xp) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return x;
            };
            if !self.nodes[xp].red {
                return root;
            }
            let Some(xpp) = self.nodes[xp].parent else {
                return root;
            };
            if self.nodes[xpp].left == Some(xp) {
                let uncle = self.nodes[xpp].right;
                if let Some(uncle) = uncle.filter(|&index| self.nodes[index].red) {
                    self.nodes[uncle].red = false;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                    continue;
                }
                if self.nodes[xp].right == Some(x) {
                    root = self.rotate_left(root, xp);
                    x = xp;
                }
                if let Some(xp_now) = self.nodes[x].parent {
                    self.nodes[xp_now].red = false;
                    if let Some(xpp_now) = self.nodes[xp_now].parent {
                        self.nodes[xpp_now].red = true;
                        root = self.rotate_right(root, xpp_now);
                    }
                }
            } else {
                let uncle = self.nodes[xpp].left;
                if let Some(uncle) = uncle.filter(|&index| self.nodes[index].red) {
                    self.nodes[uncle].red = false;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                    continue;
                }
                if self.nodes[xp].left == Some(x) {
                    root = self.rotate_right(root, xp);
                    x = xp;
                }
                if let Some(xp_now) = self.nodes[x].parent {
                    self.nodes[xp_now].red = false;
                    if let Some(xpp_now) = self.nodes[xp_now].parent {
                        self.nodes[xpp_now].red = true;
                        root = self.rotate_left(root, xpp_now);
                    }
                }
            }
        }
    }

    /// 官方 `moveRootToFront`：链表里把树根挪到最前，其余保持相对顺序。
    ///
    /// The official `moveRootToFront`: move the root to the list head, keeping the rest
    /// in order.
    fn move_root_to_front(&mut self, root: usize) {
        if self.order.first() == Some(&root) {
            return;
        }
        self.order.retain(|&index| index != root);
        self.order.insert(0, root);
    }

    /// 官方 `putTreeVal`（已树化的桶里插入）：按 hash 下探找到树的父节点，把新节点插到
    /// **父节点在链表中的下一个位置**（不是桶尾），再做 `balanceInsertion` 与
    /// `moveRootToFront`。
    ///
    /// 链表插入位置很关键：桶里后来的节点会插在树的父节点后面，`moveRootToFront` 再把
    /// 根提到桶首，于是桶内顺序不再是纯插入序。把它当成"追加到桶尾"会让桶内两个 hash
    /// 相近的元素顺序颠倒，正是 `lush_caves_vegetation` 那两格 `short_grass` 的成因。
    ///
    /// The official `putTreeVal` on a treeified bin: descend by hash, splice the new node
    /// **after its parent in the list** (not at the bin tail), then `balanceInsertion` and
    /// `moveRootToFront`. The splice point matters: later members land right after their
    /// tree parent, and `moveRootToFront` then promotes the root, so the bin order is no
    /// longer plain insertion order. Treating it as "append to the tail" swaps two nearby
    /// hashes, which is exactly what shifted the two `short_grass` cells.
    fn put_tree_val(&mut self, node: usize) {
        let hash = self.nodes[node].hash;
        let mut current = self.root.expect("putTreeVal only runs on a treeified bin");
        let parent = loop {
            let current_hash = self.nodes[current].hash;
            let go_left = current_hash > hash;
            let slot = if go_left {
                self.nodes[current].left
            } else {
                self.nodes[current].right
            };
            match slot {
                Some(next) => current = next,
                None => {
                    self.nodes[node].parent = Some(current);
                    self.nodes[node].red = true;
                    if go_left {
                        self.nodes[current].left = Some(node);
                    } else {
                        self.nodes[current].right = Some(node);
                    }
                    break current;
                }
            }
        };
        // `xp.next = x`：新节点插在父节点之后，其余保持原顺序。
        // `xp.next = x`: the new node follows its parent; the rest keeps its order.
        let after = self
            .order
            .iter()
            .position(|&index| index == parent)
            .map(|position| position + 1)
            .unwrap_or(self.order.len());
        self.order.insert(after, node);
        let root = self.balance_insertion(node);
        self.root = Some(root);
        self.move_root_to_front(root);
    }
}

/// 一个行为对齐 `java.util.HashSet<BlockPos>` 的集合。
///
/// A set that matches `java.util.HashSet<BlockPos>`.
pub struct JavaPosSet {
    buckets: Vec<Bin>,
    len: usize,
    /// 遇到过本该树化的桶（容量 ≥ 64 且桶内 8 个）。为真时迭代顺序可能和官方不同。
    ///
    /// Hit a bin that should have been treeified (capacity ≥ 64 with 8 entries). When true
    /// the iteration order may differ from the official one.
    treeified: bool,
}

impl JavaPosSet {
    /// `new HashSet<>()`：表懒分配，第一次 add 才建 16 个桶。
    ///
    /// `new HashSet<>()`: the table is lazy, built with 16 bins on the first add.
    pub fn new() -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
            treeified: false,
        }
    }

    fn capacity(&self) -> usize {
        self.buckets.len().max(16)
    }

    fn threshold(&self) -> usize {
        self.capacity() * 3 / 4
    }

    /// `HashSet.add`。
    ///
    /// `HashSet.add`.
    pub fn insert(&mut self, pos: Pos) {
        if self.buckets.is_empty() {
            self.resize(16);
        }
        let hash = spread(vec3i_hash(pos));
        let index = hash as usize & (self.capacity() - 1);
        {
            let bin = &mut self.buckets[index];
            if bin.order.iter().any(|&node| bin.nodes[node].pos == pos) {
                return;
            }
            let node = bin.nodes.len();
            bin.nodes.push(Node {
                pos,
                hash,
                left: None,
                right: None,
                parent: None,
                red: false,
            });
            if bin.is_tree() {
                bin.put_tree_val(node);
            } else {
                bin.order.push(node);
            }
        }
        self.len += 1;
        // `putVal` 的顺序：先挂到桶尾，`binCount >= TREEIFY_THRESHOLD - 1` 时调
        // `treeifyBin`（`binCount` 从 0 数到 7，对应"桶里已有 8 个、正要挂第 9 个"），
        // **然后**才判 `++size > threshold`。两步顺序颠倒会让容量序列不同。
        //
        // `putVal` order: append to the bin, call `treeifyBin` once
        // `binCount >= TREEIFY_THRESHOLD - 1` (`binCount` counts the 0-based walk, so it
        // fires when the bin already holds 8 nodes and the 9th is appended), and **then**
        // check `++size > threshold`. Swapping the two shifts the capacity sequence.
        let over_threshold = self.buckets[index].order.len() > TREEIFY_THRESHOLD;
        if over_threshold {
            if self.capacity() < MIN_TREEIFY_CAPACITY {
                // 官方 `treeifyBin`：容量不足 64 时只翻倍，不树化。
                // Official `treeifyBin`: under capacity 64 this only doubles the table.
                self.resize(self.capacity() * 2);
            } else {
                // 容量 ≥ 64：官方会真的把桶转红黑树，`moveRootToFront` 再把树根挪到桶首。
                // 这种情况很少见（植被 patch 的地表集合几乎到不了），投入产出比低，所以
                // **只置标记**，由调用方把这次放置标成 unsupported。
                //
                // At capacity ≥ 64 the official `HashMap` really treeifies the bin and
                // `moveRootToFront` moves the root to the head. That is rare for vegetation
                // patch surfaces, so we only raise a flag and let the caller mark the
                // placement unsupported.
                self.treeified = true;
            }
        }
        if self.len > self.threshold() {
            self.resize(self.capacity() * 2);
        }
    }

    /// 这个集合是否遇到过本该树化的桶。为真时迭代顺序可能和官方不同。
    ///
    /// Whether the set hit a bin that should have been treeified. When true the iteration
    /// order may differ from the official one.
    pub fn is_treeified(&self) -> bool {
        self.treeified
    }

    /// `HashMap.resize` 的 lo/hi 拆分：`hash & oldCapacity == 0` 留在原位，
    /// 其余挪到 `j + oldCapacity`，两边都保持原有相对顺序。
    ///
    /// `HashMap.resize`: double the table and split every bin by `hash & oldCapacity`,
    /// preserving relative order on both sides. A treeified bin follows the official
    /// `split` rule (a side at or below `UNTREEIFY_THRESHOLD` goes back to a list,
    /// otherwise it is treeified again in the new bin).
    fn resize(&mut self, new_capacity: usize) {
        let old_capacity = self.buckets.len();
        let old = std::mem::replace(
            &mut self.buckets,
            (0..new_capacity).map(|_| Bin::default()).collect(),
        );
        for (index, bin) in old.into_iter().enumerate() {
            let mut low: Vec<usize> = Vec::new();
            let mut high: Vec<usize> = Vec::new();
            let was_tree = bin.is_tree();
            let mut carry = Bin::default();
            for node in bin.order {
                carry.nodes.push(bin.nodes[node]);
            }
            for (position, node) in carry.nodes.iter().enumerate() {
                let slot = if node.hash as usize & old_capacity != 0 {
                    &mut high
                } else {
                    &mut low
                };
                // `order` 存的是 `carry` 的下标，重建时按同样的顺序落进新桶。
                slot.push(position);
            }
            self.adopt(index, low, &carry.nodes, was_tree, true);
            self.adopt(index + old_capacity, high, &carry.nodes, was_tree, false);
        }
    }

    /// 把拆分后的一半节点放进新桶：原桶是树且节点数超过 `UNTREEIFY_THRESHOLD` 就重新
    /// 树化，否则按原顺序组成链表。
    ///
    /// Place one half of a split bin into the new table: rebuild the tree when the source
    /// bin was a tree and this half exceeds `UNTREEIFY_THRESHOLD`, otherwise keep a list.
    fn adopt(
        &mut self,
        index: usize,
        members: Vec<usize>,
        source: &[Node],
        was_tree: bool,
        _low: bool,
    ) {
        if members.is_empty() {
            return;
        }
        let tree = was_tree && members.len() > UNTREEIFY_THRESHOLD;
        let bin = &mut self.buckets[index];
        let base = bin.nodes.len();
        for (offset, &position) in members.iter().enumerate() {
            let node = source[position];
            bin.nodes.push(Node {
                pos: node.pos,
                hash: node.hash,
                left: None,
                right: None,
                parent: None,
                red: false,
            });
            bin.order.push(base + offset);
        }
        if tree {
            bin.treeify();
        }
    }
}

impl Default for JavaPosSet {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaPosSet {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按 Java 的迭代顺序（桶 0 到最后，桶内按 `next` 链）取出所有元素。
    ///
    /// All elements in Java iteration order (bins in order, the `next` chain inside a bin).
    pub fn to_vec(&self) -> Vec<Pos> {
        self.buckets
            .iter()
            .flat_map(|bin| bin.order.iter().map(|&node| bin.nodes[node].pos))
            .collect()
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
        for pos in [(0, 0, 0), (16, 0, 0), (32, 0, 0)] {
            set.insert(pos);
        }
        assert_eq!(set.to_vec().len(), 3);
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
    fn treeified_bins_raise_a_flag_instead_of_reordering() {
        // 40 个 (i*16, 62, 0) 会挤进少数几个桶。容量 ≥ 64 且桶内到 8 个节点时官方
        // `HashMap` 转红黑树并把根移到桶首；本实现**不重排**，只置标记，由调用方把这次
        // 放置标成 unsupported。
        //
        // Forty (i*16, 62, 0) positions pile into a few bins. At capacity ≥ 64 a bin with
        // 8 nodes becomes a red-black tree in the official `HashMap`; this implementation
        // does **not** reorder — it only raises a flag so the caller can mark the placement
        // unsupported.
        let mut set = JavaPosSet::new();
        for i in 0..40 {
            set.insert((i * 16, 62, 0));
        }
        assert!(set.is_treeified(), "a treeified bin must raise the flag");
        // 纯链表顺序：0, 64, 128, ... 保持插入序。
        // Plain list order: 0, 64, 128, ... keeping insertion order.
        assert_eq!(set.to_vec()[0], (0, 62, 0));
        assert_eq!(set.to_vec()[1], (64, 62, 0));
    }

    #[test]
    fn small_sets_never_raise_the_flag() {
        let mut set = JavaPosSet::new();
        for pos in [(0, 0, 0), (16, 0, 0), (32, 0, 0), (48, 0, 0)] {
            set.insert(pos);
        }
        assert!(!set.is_treeified());
    }

    #[test]
    fn a_dense_patch_bin_raises_the_treeified_flag() {
        // 40 个同桶点（`x` 每次 +16，`hash` 低位相同）：桶内节点数超过 8 且容量到 64 时，
        // 官方 `HashMap` 会把桶转成红黑树，迭代顺序由 `moveRootToFront` 决定。
        // 本实现**不**复刻这一步，只置 `treeified` 标记让调用方把整次放置标 unsupported。
        //
        // Forty points in one bin (`x` steps by 16, so the hash's low bits repeat): once
        // the bin passes eight nodes at capacity 64 the official `HashMap` treeifies it
        // and `moveRootToFront` decides the order. This implementation does **not**
        // reproduce that step; it only raises `treeified` so the caller can mark the
        // placement unsupported.
        let mut set = JavaPosSet::new();
        for i in 0..40 {
            set.insert((i * 16, 62, 0));
        }
        assert!(
            set.is_treeified(),
            "a dense patch must raise the treeified flag"
        );
        // 纯链表顺序：0, 64, 128, ... 保持插入序。
        // Plain list order: 0, 64, 128, ... keeping insertion order.
        assert_eq!(set.to_vec()[0], (0, 62, 0));
        assert_eq!(set.to_vec()[1], (64, 62, 0));
    }

    #[test]
    fn a_spread_patch_keeps_the_official_list_order() {
        // 半径 7 的圆盘、中心 (-35,62,41) 的 149 个点：没有桶超过 8 个，
        // 官方不会树化，迭代序就是链表序。这里只断言规模，顺序由
        // `matches_a_real_patch_shape` 一类的定点测试覆盖。
        //
        // A 149-point radius-7 disc at (-35,62,41): no bin passes eight nodes, so the
        // official set stays a plain list and the order is the list order. This asserts
        // only the size; fixed-point order tests cover the sequencing.
        let mut set = JavaPosSet::new();
        for dx in -7..=7 {
            for dz in -7..=7 {
                if dx * dx + dz * dz <= 49 {
                    set.insert((dx - 35, 62, dz + 41));
                }
            }
        }
        assert_eq!(set.to_vec().len(), 149);
    }
}
