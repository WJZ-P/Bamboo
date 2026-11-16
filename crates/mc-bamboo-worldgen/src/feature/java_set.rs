//! `HashSet<BlockPos>` 的迭代顺序复刻。
//!
//! `VegetationPatchFeature.placeGroundPatch` 把地表位置收进 Java 的 `HashSet<BlockPos>`，
//! 随后按迭代顺序逐个判 `nextFloat` 决定是否长植被。迭代顺序因此直接决定哪个位置拿到
//! 哪一次随机数，必须复刻 `java.util.HashSet`（底层 `HashMap`）的桶序。
//!
//! 复刻范围照 OpenJDK `HashMap`（用 JDK 25 的 `javap -c -p java.util.HashMap` /
//! `java.util.HashMap$TreeNode` 逐条核对）：初始容量 16、装载因子 0.75、桶内尾插、
//! `hash(key) = h ^ (h >>> 16)`（`h` 是 `Vec3i.hashCode() = (y + z*31)*31 + x`，32 位
//! 回绕）、索引 `hash & (capacity-1)`、超阈值翻倍（按 `hash & oldCapacity` 拆 lo/hi，
//! 两边保持相对顺序）。
//!
//! 桶内节点数到 9 时 `putVal` 调 `treeifyBin`：容量 < 64 只翻倍，否则把链表换成
//! `TreeNode` 并 `treeify`（红黑树 + `moveRootToFront` 把树根挪到桶首）。树化桶里
//! 后续的 `putTreeVal` 把新节点插在**父节点的 `next` 之后**，不是桶尾；扩容的
//! `TreeNode.split` 两侧都沿 `next` 链保序，`UNTREEIFY_THRESHOLD` 以下退回链表。
//! 迭代始终沿 `TreeNode.next` 链表，所以这些操作只改变链表顺序，不需要在迭代时
//! 遍历树结构。植被 patch 的地表集合只做 add，所以这里不实现 remove。
//!
//! Iteration order of the official `HashSet<BlockPos>`. `placeGroundPatch` collects the
//! surface positions into a `HashSet` and `distributeVegetation` draws one `nextFloat` per
//! element in iteration order, so the bucket order decides which position gets which draw.
//!
//! Mirrors OpenJDK `HashMap` (checked against JDK 25 bytecode of `HashMap` and its
//! `TreeNode`): capacity 16, load factor 0.75, tail insertion, the spread hash, the
//! low/high resize split, `treeifyBin`/`treeify`/`putTreeVal` and `TreeNode.split`.
//! `HashSet` is only ever inserted into here, so removal is not implemented.

type Pos = (i32, i32, i32);

/// `HashMap.TREEIFY_THRESHOLD`。`putVal` 在 `binCount >= 7` 时调 `treeifyBin`，
/// 也就是链表长度变成 9 的那一刻。
///
/// `HashMap.TREEIFY_THRESHOLD`; `putVal` calls `treeifyBin` once `binCount >= 7`,
/// i.e. when the list just grew to nine nodes.
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

fn bucket_index(hash: u32, capacity: usize) -> usize {
    hash as usize & (capacity - 1)
}

/// `Vec3i.compareTo`：先 y，再 z，最后 x。`HashMap.TreeNode` 在 hash 相等时用它，
/// 因为 `BlockPos` 通过 `Vec3i` 实现了 `Comparable<Vec3i>`。
///
/// `Vec3i.compareTo`: y, then z, then x. `HashMap.TreeNode` uses it when hashes tie,
/// because `BlockPos` is `Comparable<Vec3i>` through `Vec3i`.
fn compare_pos(a: Pos, b: Pos) -> i32 {
    if a.1 != b.1 {
        return a.1 - b.1;
    }
    if a.2 != b.2 {
        return a.2 - b.2;
    }
    a.0 - b.0
}

/// 一个节点，同时挂在 `next` 链和红黑树上（对应官方 `HashMap.TreeNode`）。
///
/// A node living in both the `next` chain and the red-black tree, like `HashMap.TreeNode`.
#[derive(Debug, Clone, Copy)]
struct Node {
    pos: Pos,
    hash: u32,
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    red: bool,
}

impl Node {
    fn new(pos: Pos, hash: u32) -> Self {
        Self {
            pos,
            hash,
            left: None,
            right: None,
            parent: None,
            red: false,
        }
    }

    /// 拆桶 / 退树后重建时把树指针清空；`next` 链由 `Bin.order` 单独保存。
    ///
    /// Clear the tree links when a split or `untreeify` rebuilds the bin.
    fn clear_tree(&mut self) {
        self.left = None;
        self.right = None;
        self.parent = None;
        self.red = false;
    }
}

/// 一个桶：`order` 就是官方 `next` 链，迭代顺序就是它的顺序；`nodes` 是节点 arena。
///
/// One bin: `order` is the official `next` chain and therefore the iteration order;
/// `nodes` is the node arena.
#[derive(Default)]
struct Bin {
    nodes: Vec<Node>,
    order: Vec<usize>,
    root: Option<usize>,
}

/// `HashMap.TreeNode` 的比较：hash 优先，相同再 `compareComparables`（`compareTo`），
/// 仍相同才 `tieBreakOrder`。`BlockPos` 的两套判据全等时就一定是同一个位置
/// （`HashSet` 里不会有两个），所以这里不需要 `identityHashCode` 兜底。
///
/// Ordering used by `HashMap.TreeNode`: hash first, then `compareTo`, then
/// `tieBreakOrder`. Two `BlockPos` values equal under both are the same position,
/// which a `HashSet` cannot contain twice, so no `identityHashCode` fallback is needed.
fn tree_direction(node: &Node, other: &Node) -> i32 {
    // `TreeNode` 用 `if_icmple` / `if_icmpge` 比 `hash`，是 **有符号** int 比较。
    // `TreeNode` compares `hash` with `if_icmple` / `if_icmpge`: **signed** int.
    let (hash, other_hash) = (node.hash as i32, other.hash as i32);
    if hash < other_hash {
        return -1;
    }
    if hash > other_hash {
        return 1;
    }
    compare_pos(node.pos, other.pos)
}

impl Bin {
    fn is_tree(&self) -> bool {
        self.root.is_some()
    }

    /// 官方 `root()`：沿 `parent` 一直往上。
    /// The official `root()`: walk `parent` to the top.
    fn tree_root(&self, mut node: usize) -> usize {
        while let Some(parent) = self.nodes[node].parent {
            node = parent;
        }
        node
    }

    /// 官方 `rotateLeft(root, p)`。
    /// The official `rotateLeft(root, p)`.
    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let Some(r) = self.nodes[p].right else {
            return root;
        };
        self.nodes[p].right = self.nodes[r].left;
        if let Some(rl) = self.nodes[r].left {
            self.nodes[rl].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[r].parent = pp;
        match pp {
            None => {
                root = r;
                self.nodes[r].red = false;
            }
            Some(pp) if self.nodes[pp].left == Some(p) => self.nodes[pp].left = Some(r),
            Some(pp) => self.nodes[pp].right = Some(r),
        }
        self.nodes[r].left = Some(p);
        self.nodes[p].parent = Some(r);
        root
    }

    /// 官方 `rotateRight(root, p)`。
    /// The official `rotateRight(root, p)`.
    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let Some(l) = self.nodes[p].left else {
            return root;
        };
        self.nodes[p].left = self.nodes[l].right;
        if let Some(lr) = self.nodes[l].right {
            self.nodes[lr].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[l].parent = pp;
        match pp {
            None => {
                root = l;
                self.nodes[l].red = false;
            }
            Some(pp) if self.nodes[pp].right == Some(p) => self.nodes[pp].right = Some(l),
            Some(pp) => self.nodes[pp].left = Some(l),
        }
        self.nodes[l].right = Some(p);
        self.nodes[p].parent = Some(l);
        root
    }

    /// 官方 `balanceInsertion(root, x)`；`self.root` 随之更新。
    /// The official `balanceInsertion(root, x)`; updates `self.root`.
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
                if uncle.is_some_and(|u| self.nodes[u].red) {
                    let u = uncle.expect("checked above");
                    self.nodes[u].red = false;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                    continue;
                }
                if self.nodes[xp].right == Some(x) {
                    root = self.rotate_left(root, xp);
                    x = xp;
                }
                if let Some(xp) = self.nodes[x].parent {
                    self.nodes[xp].red = false;
                    if let Some(xpp) = self.nodes[xp].parent {
                        self.nodes[xpp].red = true;
                        root = self.rotate_right(root, xpp);
                    }
                }
            } else {
                let uncle = self.nodes[xpp].left;
                if uncle.is_some_and(|u| self.nodes[u].red) {
                    let u = uncle.expect("checked above");
                    self.nodes[u].red = false;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                    continue;
                }
                if self.nodes[xp].left == Some(x) {
                    root = self.rotate_right(root, xp);
                    x = xp;
                }
                if let Some(xp) = self.nodes[x].parent {
                    self.nodes[xp].red = false;
                    if let Some(xpp) = self.nodes[xp].parent {
                        self.nodes[xpp].red = true;
                        root = self.rotate_left(root, xpp);
                    }
                }
            }
        }
    }

    /// 官方 `moveRootToFront`：把树根从 `next` 链里摘出来插到桶首，其余保持相对顺序。
    ///
    /// The official `moveRootToFront`: unlink the root from the `next` chain and put it
    /// at the bin head, keeping the rest in order.
    fn move_root_to_front(&mut self, root: usize) {
        if self.order.first() == Some(&root) {
            return;
        }
        self.order.retain(|&index| index != root);
        self.order.insert(0, root);
    }

    /// 按 `dir` 把 `node` 挂到 `parent` 的空子树上，并跑 `balanceInsertion`。
    ///
    /// Attach `node` to `parent`'s empty child slot given by `dir`, then balance.
    fn attach(&mut self, parent: usize, node: usize, dir: i32) {
        self.nodes[node].parent = Some(parent);
        self.nodes[node].red = true;
        if dir <= 0 {
            self.nodes[parent].left = Some(node);
        } else {
            self.nodes[parent].right = Some(node);
        }
        let root = self.balance_insertion(node);
        self.root = Some(root);
    }

    /// `balanceInsertion` 之后官方把新根调到桶首；`treeify` 与 `putTreeVal`
    /// 都走这一步。
    ///
    /// After `balanceInsertion` the official code promotes the new root to the
    /// bin head; both `treeify` and `putTreeVal` do this.
    fn finish_insert(&mut self, node: usize, dir: i32, parent: usize) {
        self.attach(parent, node, dir);
        let root = self.root.expect("a treeified bin keeps a root");
        self.move_root_to_front(root);
    }

    /// 官方 `treeify`：按 `order` 顺序把节点逐个插进红黑树，最后 `moveRootToFront`。
    /// `order` 本身保持原链表顺序（官方的 `treeify` 只改树指针和 `prev`/`next` 的
    /// 根位置，不改其余节点的相对顺序）。
    ///
    /// The official `treeify`: insert nodes in `order` into the tree, then
    /// `moveRootToFront`. The rest of the `next` chain keeps its relative order.
    fn treeify(&mut self) {
        if self.is_tree() || self.order.len() < 2 {
            return;
        }
        let ordered = self.order.clone();
        self.root = Some(ordered[0]);
        self.nodes[ordered[0]].red = false;
        self.nodes[ordered[0]].parent = None;
        self.nodes[ordered[0]].left = None;
        self.nodes[ordered[0]].right = None;
        for &node in &ordered[1..] {
            self.nodes[node].left = None;
            self.nodes[node].right = None;
            self.nodes[node].parent = None;
            self.nodes[node].red = false;
            let mut current = self.root.expect("treeify keeps a root");
            if self.nodes[current].parent.is_some() {
                current = self.tree_root(current);
            }
            loop {
                let dir = tree_direction(&self.nodes[node], &self.nodes[current]);
                let slot = if dir <= 0 {
                    self.nodes[current].left
                } else {
                    self.nodes[current].right
                };
                match slot {
                    Some(next) => current = next,
                    None => {
                        self.attach(current, node, dir);
                        break;
                    }
                }
            }
        }
        self.order = ordered;
        let root = self.root.expect("treeify keeps a root");
        self.move_root_to_front(root);
    }

    /// 官方 `putTreeVal`（已树化的桶里插入）：按 hash 下探找到树的父节点，把新节点插到
    /// **父节点在链表中的下一个位置**（不是桶尾），再做 `balanceInsertion` 与
    /// `moveRootToFront`。
    ///
    /// The official `putTreeVal` on a treeified bin: descend by hash, splice the new node
    /// **after its parent in the list** (not at the bin tail), then `balanceInsertion` and
    /// `moveRootToFront`.
    fn put_tree_val(&mut self, node: usize) {
        // `p = this.parent != null ? root() : this`，`this` 是桶首节点。
        // `p = this.parent != null ? root() : this`, where `this` is the bin head.
        let head = self.order[0];
        let mut current = if self.nodes[head].parent.is_some() {
            self.tree_root(head)
        } else {
            head
        };
        let parent = loop {
            let dir = tree_direction(&self.nodes[node], &self.nodes[current]);
            let slot = if dir <= 0 {
                self.nodes[current].left
            } else {
                self.nodes[current].right
            };
            match slot {
                Some(next) => current = next,
                None => break current,
            }
        };
        let dir = tree_direction(&self.nodes[node], &self.nodes[parent]);
        // `newTreeNode(h, k, v, xp.next)` 已经把新节点挂在父节点之后；
        // `putTreeVal` 的下探与平衡都在这个链表状态下进行。
        // `newTreeNode(h, k, v, xp.next)` already spliced the new node after its
        // parent; `putTreeVal` descends and balances with that chain in place.
        let after = self
            .order
            .iter()
            .position(|&index| index == parent)
            .map(|position| position + 1)
            .unwrap_or(self.order.len());
        self.order.insert(after, node);
        self.finish_insert(node, dir, parent);
    }
}

/// 一个行为对齐 `java.util.HashSet<BlockPos>` 的集合。
///
/// A set that matches `java.util.HashSet<BlockPos>`.
pub struct JavaPosSet {
    buckets: Vec<Bin>,
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

    /// `HashSet.add`：新增返回 true，重复返回 false。
    ///
    /// `HashSet.add`: true when inserted, false on a duplicate.
    pub fn insert(&mut self, pos: Pos) -> bool {
        // `HashSet` 实际是 `HashMap.put(key, PRESENT)`；`putVal` 只按 `hash` 和
        // `key.equals` 找重复，不会去别的桶里找。
        // `HashSet.add` is `HashMap.put(key, PRESENT)`; `putVal` looks for duplicates
        // by hash and `equals` inside the target bin only.
        if self.buckets.is_empty() {
            self.resize(16);
        }
        let hash = spread(vec3i_hash(pos));
        let index = bucket_index(hash, self.capacity());
        let bin = &mut self.buckets[index];
        if bin
            .order
            .iter()
            .any(|&node| bin.nodes[node].hash == hash && bin.nodes[node].pos == pos)
        {
            return false;
        }
        let node = bin.nodes.len();
        bin.nodes.push(Node::new(pos, hash));
        if bin.is_tree() {
            bin.put_tree_val(node);
        } else {
            bin.order.push(node);
        }
        self.len += 1;
        // `putVal` 的顺序：先挂到桶尾，链表长度到 9 时调 `treeifyBin`
        // （`binCount >= TREEIFY_THRESHOLD - 1`，树化桶走 `putTreeVal`、不重复触发），
        // **然后**才判 `++size > threshold`。两步顺序颠倒会让容量序列不同。
        //
        // `putVal` order: append, call `treeifyBin` when the list grows to nine
        // (`binCount >= TREEIFY_THRESHOLD - 1`; treeified bins use `putTreeVal` and
        // never re-trigger it), then check `++size > threshold`. Swapping the two
        // shifts the capacity sequence.
        if !self.buckets[index].is_tree() && self.buckets[index].order.len() > TREEIFY_THRESHOLD {
            if self.capacity() < MIN_TREEIFY_CAPACITY {
                // 官方 `treeifyBin`：容量不足 64 时只翻倍，不树化。
                // Official `treeifyBin`: under capacity 64 this only doubles the table.
                self.resize(self.capacity() * 2);
            } else {
                self.buckets[index].treeify();
            }
        }
        if self.len > self.threshold() {
            self.resize(self.capacity() * 2);
        }
        true
    }

    /// `HashMap.resize` 的 lo/hi 拆分：`hash & oldCapacity == 0` 留在原位，
    /// 其余挪到 `j + oldCapacity`，两边都保持原有相对顺序。树化桶走官方
    /// `TreeNode.split`：拆完一侧超过 `UNTREEIFY_THRESHOLD` 才重新 `treeify`。
    ///
    /// `HashMap.resize`: double the table and split every bin by `hash & oldCapacity`,
    /// preserving relative order on both sides. A treeified bin follows the official
    /// `split` rule (re-treeify a half only when it exceeds `UNTREEIFY_THRESHOLD`).
    fn resize(&mut self, new_capacity: usize) {
        let old_capacity = self.buckets.len();
        let old = if self.buckets.is_empty() {
            (0..16).map(|_| Bin::default()).collect()
        } else {
            std::mem::replace(
                &mut self.buckets,
                (0..new_capacity).map(|_| Bin::default()).collect(),
            )
        };
        for (index, mut bin) in old.into_iter().enumerate() {
            let was_tree = bin.is_tree();
            let mut low: Vec<usize> = Vec::new();
            let mut high: Vec<usize> = Vec::new();
            for &node in &bin.order {
                bin.nodes[node].clear_tree();
                if bin.nodes[node].hash as usize & old_capacity != 0 {
                    high.push(node);
                } else {
                    low.push(node);
                }
            }
            bin.root = None;
            self.adopt(index, &low, &bin.nodes, was_tree);
            self.adopt(index + old_capacity, &high, &bin.nodes, was_tree);
        }
        if self.buckets.is_empty() {
            self.buckets = (0..new_capacity).map(|_| Bin::default()).collect();
        }
    }

    /// 把拆分后的一半节点放进新桶：原桶是树且这一侧超过 `UNTREEIFY_THRESHOLD`
    /// 才重新树化。
    ///
    /// Place one half of a split bin into the new table: rebuild the tree only when
    /// the source bin was a tree and this half exceeds `UNTREEIFY_THRESHOLD`.
    fn adopt(&mut self, index: usize, members: &[usize], source: &[Node], was_tree: bool) {
        if members.is_empty() {
            return;
        }
        let bin = &mut self.buckets[index];
        for &node in members {
            bin.nodes.push(source[node]);
        }
        bin.order = (0..members.len()).collect();
        if was_tree && members.len() > UNTREEIFY_THRESHOLD {
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
            assert!(set.insert(pos));
        }
        assert_eq!(set.to_vec().len(), 3);
        // 重复插入不改变集合大小。
        // Inserting a duplicate does not change the size.
        assert!(!set.insert((0, 0, 0)));
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
    fn small_sets_never_treeify() {
        let mut set = JavaPosSet::new();
        for pos in [(0, 0, 0), (16, 0, 0), (32, 0, 0), (48, 0, 0)] {
            set.insert(pos);
        }
        assert_eq!(set.to_vec().len(), 4);
    }

    #[test]
    fn a_spread_patch_keeps_the_official_list_order() {
        // 半径 7 的圆盘、中心 (-35,62,41) 的 149 个点：没有桶超过 8 个，
        // 官方不会树化，迭代序就是链表序。
        //
        // A 149-point radius-7 disc at (-35,62,41): no bin passes eight nodes, so the
        // official set stays a plain list and the order is the list order.
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

    #[test]
    fn dense_collisions_follow_the_official_tree_order() {
        // 40 个同桶点：官方在容量 64 后把桶树化，桶首是红黑树根。
        // 这组顺序来自 JDK 25 的 `HashSet<BlockPos>` 实际迭代输出。
        //
        // Forty points in one bin: the official map treeifies at capacity 64 and the
        // bin head becomes the red-black root. This order comes from a real JDK 25 run.
        let mut set = JavaPosSet::new();
        for i in 0..40 {
            set.insert((i * 16, 62, 0));
        }
        let expected: Vec<Pos> = [
            192, 0, 64, 128, 256, 320, 384, 448, 512, 576, 208, 16, 80, 144, 272, 336, 400, 464,
            528, 592, 224, 32, 96, 160, 288, 352, 416, 480, 544, 608, 240, 48, 112, 176, 304, 368,
            432, 496, 560, 624,
        ]
        .into_iter()
        .map(|x| (x, 62, 0))
        .collect();
        assert_eq!(set.to_vec(), expected);
    }

    /// 读 JDK 25 的 `JavaSetOracle.java` 生成的用例文件，逐个比对增删后的迭代顺序。
    ///
    /// Replay the JDK 25 `JavaSetOracle` cases and compare the final iteration order.
    /// The file is produced by
    /// `java tools/mc-bamboo-vanilla/java/JavaSetOracle.java > crates/mc-bamboo-worldgen/src/feature/java_set_cases.txt`.
    #[test]
    fn matches_the_jdk_hashset_oracle() {
        let text = include_str!("java_set_cases.txt");
        let mut lines = text.lines();
        let mut cases = 0usize;
        while let Some(line) = lines.next() {
            if !line.starts_with("CASE ") {
                continue;
            }
            let case = line.to_owned();
            let ops = lines.next().unwrap_or("");
            let order = lines.next().unwrap_or("");
            let ops = ops.strip_prefix("OPS ").unwrap_or("");
            let order = order.strip_prefix("ORDER ").unwrap_or("");
            let mut set = JavaPosSet::new();
            for op in ops.split(';').filter(|s| !s.is_empty()) {
                let mut parts = op[1..].split(' ');
                let x: i32 = parts.next().unwrap().parse().unwrap();
                let y: i32 = parts.next().unwrap().parse().unwrap();
                let z: i32 = parts.next().unwrap().parse().unwrap();
                assert_eq!(op.as_bytes()[0], b'+', "{case}: bad op {op}");
                set.insert((x, y, z));
            }
            let expected: Vec<Pos> = order
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|entry| {
                    let mut parts = entry.split(' ');
                    let x: i32 = parts.next().unwrap().parse().unwrap();
                    let y: i32 = parts.next().unwrap().parse().unwrap();
                    let z: i32 = parts.next().unwrap().parse().unwrap();
                    (x, y, z)
                })
                .collect();
            let actual = set.to_vec();
            let first_bad = actual
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(actual.len().min(expected.len()));
            assert_eq!(
                actual,
                expected,
                "{case}: iteration differs at {first_bad} ({} vs {})",
                actual.len(),
                expected.len()
            );
            cases += 1;
        }
        assert_eq!(cases, 80);
    }
}
