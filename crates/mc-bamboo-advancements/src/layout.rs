//! 官方 `TreeNodePosition` 的显示节点排版，逐个步骤照 26.3 的字节码移植。
//!
//! 兄弟顺序由 [`super::AdvancementTree::apply_order`] 决定：官方用 `ReferenceOpenHashSet`
//! 的身份哈希，每次启动都可能不同；Bamboo 正式运行按 id 排序。对拍时把官方当次的
//! 顺序喂进来，本模块只用这个顺序算坐标，不读官方算好的坐标。
//!
//! Port of vanilla `TreeNodePosition`: the Buchheim/Walker layout. Node indices are
//! indices into the tree's node vector; invisible nodes are skipped by walking
//! down to their displayed descendants, exactly like `addChild` does.

use crate::{AdvancementTree, Node};

/// 排版里的一棵「有显示」的树。下标是 [`Tree`] 的 `positions` 下标。
struct Position {
    /// 对应的树节点下标。
    node: usize,
    parent: Option<usize>,
    previous_sibling: Option<usize>,
    child_index: i32,
    children: Vec<usize>,
    ancestor: usize,
    thread: Option<usize>,
    x: i32,
    y: f32,
    modifier: f32,
    change: f32,
    shift: f32,
}

struct Tree {
    positions: Vec<Position>,
    root: usize,
}

impl Tree {
    fn new(source: &AdvancementTree, root: usize) -> Self {
        let mut tree = Self {
            positions: Vec::new(),
            root: 0,
        };
        // 官方 `TreeNodePosition.run` 要求根自己带显示。
        if source.nodes[root].advancement.display.is_none() {
            return tree;
        }
        let index = tree.add_node(source, root, None, None, 1, 0);
        tree.root = index;
        tree
    }

    fn add_node(
        &mut self,
        source: &AdvancementTree,
        node: usize,
        parent: Option<usize>,
        previous_sibling: Option<usize>,
        child_index: i32,
        x: i32,
    ) -> usize {
        let index = self.positions.len();
        self.positions.push(Position {
            node,
            parent,
            previous_sibling,
            child_index,
            children: Vec::new(),
            ancestor: index,
            thread: None,
            x,
            y: -1.0,
            modifier: 0.0,
            change: 0.0,
            shift: 0.0,
        });
        // 官方构造器里的局部变量 `previousSibling` 初始化成 null，和本节点自己的
        // `previousSibling` 字段同名但不是同一个值：子节点链表从空开始。
        let mut previous = None;
        for &child in &source.nodes[node].children {
            previous = self.add_child(source, child, index, previous);
        }
        index
    }

    /// 官方 `addChild`：无显示节点不下沉，而是把它的显示后代挂到当前层。
    fn add_child(
        &mut self,
        source: &AdvancementTree,
        node: usize,
        parent: usize,
        previous_sibling: Option<usize>,
    ) -> Option<usize> {
        if source.nodes[node].advancement.display.is_some() {
            let child_index = self.positions[parent].children.len() as i32 + 1;
            let x = self.positions[parent].x + 1;
            let index = self.add_node(source, node, Some(parent), previous_sibling, child_index, x);
            self.positions[parent].children.push(index);
            return Some(index);
        }
        let mut previous = previous_sibling;
        for &grandchild in &source.nodes[node].children {
            previous = self.add_child(source, grandchild, parent, previous);
        }
        previous
    }

    fn previous_or_thread(&self, index: usize) -> Option<usize> {
        // 官方的 thread 优先；没有 thread 时 children 非空才返回第一个孩子。
        self.positions[index]
            .thread
            .or_else(|| self.positions[index].children.first().copied())
    }

    fn next_or_thread(&self, index: usize) -> Option<usize> {
        self.positions[index]
            .thread
            .or_else(|| self.positions[index].children.last().copied())
    }

    fn first_walk(&mut self, index: usize) {
        if self.positions[index].children.is_empty() {
            self.positions[index].y = match self.positions[index].previous_sibling {
                Some(previous) => self.positions[previous].y + 1.0,
                None => 0.0,
            };
            return;
        }
        let children = self.positions[index].children.clone();
        let mut default_ancestor = None;
        for &child in &children {
            self.first_walk(child);
            let candidate = default_ancestor.unwrap_or(child);
            default_ancestor = Some(self.apportion(child, candidate));
        }
        self.execute_shifts(index);
        let first = self.positions[children[0]].y;
        let last = self.positions[*children.last().expect("children 非空")].y;
        // 官方 `firstWalk` 在 children 为空时走 previousSibling.y+1；这里 children 非空。
        let midpoint = (first + last) / 2.0;
        if let Some(previous) = self.positions[index].previous_sibling {
            self.positions[index].y = self.positions[previous].y + 1.0;
            self.positions[index].modifier = self.positions[index].y - midpoint;
        } else {
            self.positions[index].y = midpoint;
        }
    }

    fn second_walk(&mut self, index: usize, modulus: f32, depth: i32, mut minimum: f32) -> f32 {
        self.positions[index].y += modulus;
        self.positions[index].x = depth;
        if self.positions[index].y < minimum {
            minimum = self.positions[index].y;
        }
        let child_modulus = modulus + self.positions[index].modifier;
        for child in self.positions[index].children.clone() {
            minimum = self.second_walk(child, child_modulus, depth + 1, minimum);
        }
        minimum
    }

    fn third_walk(&mut self, index: usize, amount: f32) {
        self.positions[index].y += amount;
        for child in self.positions[index].children.clone() {
            self.third_walk(child, amount);
        }
    }

    fn execute_shifts(&mut self, index: usize) {
        let children = self.positions[index].children.clone();
        let mut shift = 0.0;
        let mut change = 0.0;
        for &child in children.iter().rev() {
            self.positions[child].y += shift;
            self.positions[child].modifier += shift;
            change += self.positions[child].change;
            shift += self.positions[child].shift + change;
        }
    }

    /// 官方 `apportion(defaultAncestor)`，`this` 是当前子节点。局部变量名字照字节码：
    /// `vir/vor`(本子树的左/右轮廓)、`vil/vol`(前一个兄弟/父长子的轮廓)，
    /// `sir/sor/sil/sol` 是各自累计的 mod。
    fn apportion(&mut self, index: usize, default_ancestor: usize) -> usize {
        let Some(vil) = self.positions[index].previous_sibling else {
            return default_ancestor;
        };
        let parent = self.positions[index].parent.expect("有兄长就有父");
        let vol = self.positions[parent].children[0];
        let mut vir = index;
        let mut vor = index;
        let mut vil = vil;
        let mut vol = vol;
        let mut sir = self.positions[vir].modifier;
        let mut sor = self.positions[vor].modifier;
        let mut sil = self.positions[vil].modifier;
        let mut sol = self.positions[vol].modifier;
        while self.next_or_thread(vil).is_some() && self.previous_or_thread(vir).is_some() {
            vil = self.next_or_thread(vil).expect("刚检查过");
            vir = self.previous_or_thread(vir).expect("刚检查过");
            // 官方是直接赋值 `vol = vol.previousOrThread()`；若为 null 会在后面
            // `sol += vol.mod` 处抛 NPE，所以正常排版里轮廓线程保证它非空。Rust 没有
            // null，这里用等价的兜底值，正常输入下这个分支永不生效。
            // Vanilla assigns directly (a null here would NPE at `sol += vol.mod`);
            // the contour threads keep it non-null. These `unwrap_or` guards are
            // unreachable for valid trees.
            vol = self
                .previous_or_thread(vol)
                .unwrap_or(self.positions[parent].children[0]);
            vor = self.next_or_thread(vor).unwrap_or(index);
            self.positions[vor].ancestor = index;
            // 官方字节码：vil.y + sil 与 vir.y + sir 各先相加，再 fsub、再 +1。
            // 左结合减法会在 f32 下舍入出不同结果，不能改写成连续减法。
            let shift = (self.positions[vil].y + sil) - (self.positions[vir].y + sir) + 1.0;
            if shift > 0.0 {
                let ancestor = self.get_ancestor(vil, default_ancestor, index);
                self.move_subtree(index, ancestor, shift);
                sir += shift;
                sor += shift;
            }
            sil += self.positions[vil].modifier;
            sir += self.positions[vir].modifier;
            sol += self.positions[vol].modifier;
            sor += self.positions[vor].modifier;
        }
        if self.next_or_thread(vil).is_some() && self.next_or_thread(vor).is_none() {
            self.positions[vor].thread = self.next_or_thread(vil);
            self.positions[vor].modifier += sil - sor;
            // 官方字节码在设置右上轮廓 thread 后直接 `goto` 返回：这里保留原来的
            // defaultAncestor（apportion 的参数），不更新成 this。Buchheim 算法里
            // `defaultAncestor = v` 只在 else 分支执行。
            // Vanilla jumps straight to the return here, leaving defaultAncestor as
            // the incoming argument; `defaultAncestor = v` only runs in the else branch.
            default_ancestor
        } else {
            if self.previous_or_thread(vir).is_some() && self.previous_or_thread(vol).is_none() {
                self.positions[vol].thread = self.previous_or_thread(vir);
                self.positions[vol].modifier += sir - sol;
            }
            // 官方在 else 分支里把 defaultAncestor 更新成 this 再返回。
            // Vanilla sets defaultAncestor = this before returning from the else branch.
            index
        }
    }

    /// 官方 `vil.getAncestor(this, defaultAncestor)`。
    fn get_ancestor(&self, vil: usize, default: usize, index: usize) -> usize {
        let ancestor = self.positions[vil].ancestor;
        let parent = self.positions[index].parent.expect("apportion 的节点有父");
        if self.positions[parent].children.contains(&ancestor) {
            ancestor
        } else {
            default
        }
    }

    /// 官方 `ancestor.moveSubtree(this, shift)`：receiver 是 ancestor，参数是当前子节点。
    fn move_subtree(&mut self, index: usize, ancestor: usize, shift: f32) {
        // 官方 `ancestor.moveSubtree(this, shift)` 里 aload_0 = ancestor、aload_1 = this：
        // subtrees = arg1.childIndex - receiver.childIndex = index - ancestor。
        let subtrees = self.positions[index].child_index - self.positions[ancestor].child_index;
        if subtrees != 0 {
            let portion = shift / subtrees as f32;
            self.positions[index].change -= portion;
            self.positions[ancestor].change += portion;
        }
        self.positions[index].shift += shift;
        self.positions[index].y += shift;
        self.positions[index].modifier += shift;
    }
}

/// 算一棵有显示根的子树里每个显示节点的 `(x, y)`，返回 `(树节点下标, 坐标)`。
/// 和官方 `TreeNodePosition.finalizePosition` 一样只写有显示的节点。
pub fn position(source: &AdvancementTree, root: usize) -> Vec<(usize, (f32, f32))> {
    if source.nodes[root].advancement.display.is_none() {
        return Vec::new();
    }
    let mut tree = Tree::new(source, root);
    let root_index = tree.root;
    tree.first_walk(root_index);
    let minimum = tree.second_walk(root_index, 0.0, 0, tree.positions[root_index].y);
    if minimum < 0.0 {
        tree.third_walk(root_index, -minimum);
    }
    tree.positions
        .iter()
        .map(|position| (position.node, (position.x as f32, position.y)))
        .collect()
}

/// 给测试的公开入口：检查一个节点是不是显示节点。
pub fn is_displayed(node: &Node) -> bool {
    node.advancement.display.is_some()
}
