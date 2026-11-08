//! 官方 `net.minecraft.world.level.biome.Climate`：六个气候参数和找最近群系的 R 树。
//!
//! 参数全是定点数（原值乘一万再截断成 long），所以查找里没有浮点。
//! 树的形状会影响平局时挑哪一个，官方的 `build` 怎么排序、怎么分桶，这里照抄。
//! 查找还带着「上一次的结果」当初始候选，平局时上一次赢，所以这个状态也要留着。

use std::cell::Cell;

use crate::math::quart_to_block;

/// `Climate.PARAMETER_COUNT`。
pub const PARAMETER_COUNT: usize = 7;
/// `Climate.QUANTIZATION_FACTOR`。
const QUANTIZATION_FACTOR: f32 = 10_000.0;
/// `Climate.RTree.CHILDREN_PER_NODE`。
const CHILDREN_PER_NODE: usize = 19;

/// `Climate.quantizeCoord(coord)`。
pub fn quantize(coord: f32) -> i64 {
    (coord * QUANTIZATION_FACTOR) as i64
}

/// `Climate.unquantizeCoord(coord)`。
pub fn unquantize(coord: i64) -> f32 {
    coord as f32 / QUANTIZATION_FACTOR
}

/// `Climate.Parameter`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    pub min: i64,
    pub max: i64,
}

impl Parameter {
    pub fn point(value: i64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    /// `Climate.Parameter.distance(target)`。
    fn distance(self, target: i64) -> i64 {
        let above = target.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target);
        if above > 0 { above } else { below.max(0) }
    }

    /// `Climate.Parameter.span(other)`，`other` 是空的时候返回自己。
    fn span(self, other: Option<Self>) -> Self {
        match other {
            None => self,
            Some(other) => Self {
                min: self.min.min(other.min),
                max: self.max.max(other.max),
            },
        }
    }

    /// 排序用的中心，官方是 `(min + max) / 2L`，long 除法往零取整。
    fn center(self) -> i64 {
        self.min.wrapping_add(self.max) / 2
    }
}

/// `Climate.TargetPoint`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPoint {
    pub temperature: i64,
    pub humidity: i64,
    pub continentalness: i64,
    pub erosion: i64,
    pub depth: i64,
    pub weirdness: i64,
}

impl TargetPoint {
    /// `Climate.target(...)`：六个浮点量化成定点。
    pub fn of(values: [f32; 6]) -> Self {
        Self {
            temperature: quantize(values[0]),
            humidity: quantize(values[1]),
            continentalness: quantize(values[2]),
            erosion: quantize(values[3]),
            depth: quantize(values[4]),
            weirdness: quantize(values[5]),
        }
    }

    /// `Climate.TargetPoint.toParameterArray()`：第七项是 0，对应 `offset`。
    fn to_array(self) -> [i64; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0,
        ]
    }
}

/// `Climate.ParameterPoint`：六个区间加一个 `offset`，一共七维。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterPoint {
    pub space: [Parameter; PARAMETER_COUNT],
}

impl ParameterPoint {
    /// 从生成器导出的 13 个定点数造出来：六个区间加一个 `offset`。
    pub fn from_quantized(values: [i64; 13]) -> Self {
        let mut space = [Parameter::point(0); PARAMETER_COUNT];
        for (index, slot) in space.iter_mut().take(6).enumerate() {
            *slot = Parameter {
                min: values[index * 2],
                max: values[index * 2 + 1],
            };
        }
        space[6] = Parameter::point(values[12]);
        Self { space }
    }

    /// `Climate.ParameterPoint.fitness(target)`，暴力查找用。
    pub fn fitness(&self, target: TargetPoint) -> i64 {
        let array = target.to_array();
        let mut sum: i64 = 0;
        for (parameter, value) in self.space.iter().zip(array) {
            let distance = parameter.distance(value);
            sum = sum.wrapping_add(distance.wrapping_mul(distance));
        }
        sum
    }
}

/// R 树的一个节点。叶子存的是条目下标。
#[derive(Debug)]
enum Node {
    Leaf(usize),
    SubTree {
        space: [Parameter; PARAMETER_COUNT],
        children: Vec<Node>,
    },
}

impl Node {
    fn space<'a>(&'a self, leaves: &'a [ParameterPoint]) -> &'a [Parameter; PARAMETER_COUNT] {
        match self {
            Self::Leaf(index) => &leaves[*index].space,
            Self::SubTree { space, .. } => space,
        }
    }
}

/// `Climate.RTree.Node.distance(target)`。
fn node_distance(space: &[Parameter; PARAMETER_COUNT], target: &[i64; PARAMETER_COUNT]) -> i64 {
    let mut distance: i64 = 0;
    for (parameter, value) in space.iter().zip(target) {
        let delta = parameter.distance(*value);
        distance = distance.wrapping_add(delta.wrapping_mul(delta));
    }
    distance
}

/// `Climate.ParameterList`：一串（参数点，值），带一棵 R 树。
#[derive(Debug)]
pub struct ParameterList<T> {
    points: Vec<ParameterPoint>,
    values: Vec<T>,
    root: Node,
    /// `Climate.RTree.lastResult`：上一次找到的叶子，当下一次查找的初始候选。
    last_result: Cell<Option<usize>>,
}

impl<T> ParameterList<T> {
    /// `new Climate.ParameterList(values)`。
    pub fn new(entries: Vec<(ParameterPoint, T)>) -> Self {
        Self::with_children_per_node(entries, CHILDREN_PER_NODE)
    }

    /// `rebuildWithChildrenPerNode`，测试用。
    pub fn with_children_per_node(
        entries: Vec<(ParameterPoint, T)>,
        children_per_node: usize,
    ) -> Self {
        assert!(
            !entries.is_empty(),
            "need at least one value to build the search tree"
        );
        let mut points = Vec::with_capacity(entries.len());
        let mut values = Vec::with_capacity(entries.len());
        for (point, value) in entries {
            points.push(point);
            values.push(value);
        }
        let leaves: Vec<Node> = (0..points.len()).map(Node::Leaf).collect();
        let root = build(leaves, &points, children_per_node);
        Self {
            points,
            values,
            root,
            last_result: Cell::new(None),
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&ParameterPoint, &T)> {
        self.points.iter().zip(&self.values)
    }

    /// `Climate.ParameterList.findValue(target)`，走 R 树。
    pub fn find_value(&self, target: TargetPoint) -> &T {
        let array = target.to_array();
        let leaf = search(&self.root, &array, self.last_result.get(), &self.points);
        self.last_result.set(Some(leaf));
        &self.values[leaf]
    }

    /// `Climate.ParameterList.findValueBruteForce(target)`，用来校验 R 树。
    pub fn find_value_brute_force(&self, target: TargetPoint) -> &T {
        let mut best = 0;
        let mut best_fitness = self.points[0].fitness(target);
        for (index, point) in self.points.iter().enumerate().skip(1) {
            let fitness = point.fitness(target);
            if fitness < best_fitness {
                best_fitness = fitness;
                best = index;
            }
        }
        &self.values[best]
    }

    /// 清掉「上一次的结果」，换种子重新开始时用。
    pub fn reset(&self) {
        self.last_result.set(None);
    }
}

/// `Climate.RTree.Node.search`。
fn search(
    node: &Node,
    target: &[i64; PARAMETER_COUNT],
    candidate: Option<usize>,
    leaves: &[ParameterPoint],
) -> usize {
    match node {
        Node::Leaf(index) => *index,
        Node::SubTree { children, .. } => {
            let mut min_distance = candidate.map_or(i64::MAX, |index| {
                node_distance(&leaves[index].space, target)
            });
            let mut closest = candidate;
            for child in children {
                let child_distance = node_distance(child.space(leaves), target);
                if min_distance > child_distance {
                    let leaf = search(child, target, closest, leaves);
                    // 官方比较的是引用：子节点本身就是叶子时距离不用重算。
                    let leaf_distance = if matches!(child, Node::Leaf(_)) {
                        child_distance
                    } else {
                        node_distance(&leaves[leaf].space, target)
                    };
                    if min_distance > leaf_distance {
                        min_distance = leaf_distance;
                        closest = Some(leaf);
                    }
                }
            }
            closest.expect("搜索至少会进一个子节点")
        }
    }
}

/// `Climate.RTree.build`。
fn build(mut children: Vec<Node>, leaves: &[ParameterPoint], children_per_node: usize) -> Node {
    assert!(
        !children.is_empty(),
        "need at least one child to build a node"
    );
    if children.len() == 1 {
        return children.pop().expect("只有一个子节点");
    }
    if children.len() <= children_per_node {
        // 官方按七个维度的中心的绝对值之和排序。
        children.sort_by_key(|child| {
            child.space(leaves).iter().fold(0_i64, |sum, parameter| {
                sum.wrapping_add(parameter.center().abs())
            })
        });
        return sub_tree(children, leaves);
    }

    let mut min_cost = i64::MAX;
    let mut min_dimension = 0;
    let mut min_buckets: Vec<Vec<Node>> = Vec::new();
    for dimension in 0..PARAMETER_COUNT {
        sort_nodes(&mut children, leaves, dimension, false);
        let buckets = bucketize(&children, leaves, children_per_node);
        let total_cost = buckets.iter().fold(0_i64, |sum, bucket| {
            sum.wrapping_add(cost(&build_parameter_space(bucket, leaves)))
        });
        if min_cost > total_cost {
            min_cost = total_cost;
            min_dimension = dimension;
            min_buckets = buckets;
        }
    }

    // 官方在这里排的是桶（按桶的范围的中心的绝对值），再对每个桶递归。
    let mut bucket_nodes: Vec<Node> = min_buckets
        .into_iter()
        .map(|bucket| sub_tree(bucket, leaves))
        .collect();
    sort_nodes(&mut bucket_nodes, leaves, min_dimension, true);
    let children = bucket_nodes
        .into_iter()
        .map(|bucket| match bucket {
            Node::SubTree { children, .. } => build(children, leaves, children_per_node),
            leaf @ Node::Leaf(_) => leaf,
        })
        .collect();
    sub_tree(children, leaves)
}

/// `Climate.RTree.sort`：先按指定维度，再按后面的维度依次打破平局。
fn sort_nodes(nodes: &mut [Node], leaves: &[ParameterPoint], dimension: usize, absolute: bool) {
    let key = |node: &Node| {
        let space = node.space(leaves);
        let mut keys = [0_i64; PARAMETER_COUNT];
        for offset in 0..PARAMETER_COUNT {
            let center = space[(dimension + offset) % PARAMETER_COUNT].center();
            keys[offset] = if absolute { center.abs() } else { center };
        }
        keys
    };
    nodes.sort_by_key(key);
}

/// `Climate.RTree.bucketize`。
fn bucketize(
    nodes: &[Node],
    leaves: &[ParameterPoint],
    children_per_node: usize,
) -> Vec<Vec<Node>> {
    let count = nodes.len() as f64;
    let per_node = children_per_node as f64;
    let expected = per_node.powf(((count - 0.01).ln() / per_node.ln()).floor()) as usize;
    let mut buckets = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    for node in nodes {
        current.push(clone_node(node, leaves));
        if current.len() >= expected {
            buckets.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        buckets.push(current);
    }
    buckets
}

/// `Climate.RTree.cost(parameterSpace)`。
fn cost(space: &[Parameter; PARAMETER_COUNT]) -> i64 {
    space.iter().fold(0_i64, |sum, parameter| {
        sum.wrapping_add(parameter.max.wrapping_sub(parameter.min).abs())
    })
}

/// `Climate.RTree.buildParameterSpace(children)`。
fn build_parameter_space(
    children: &[Node],
    leaves: &[ParameterPoint],
) -> [Parameter; PARAMETER_COUNT] {
    assert!(!children.is_empty(), "sub tree needs at least one child");
    let mut bounds: [Option<Parameter>; PARAMETER_COUNT] = [None; PARAMETER_COUNT];
    for child in children {
        let space = child.space(leaves);
        for (index, slot) in bounds.iter_mut().enumerate() {
            *slot = Some(space[index].span(*slot));
        }
    }
    bounds.map(|bound| bound.expect("每个维度都有范围"))
}

fn sub_tree(children: Vec<Node>, leaves: &[ParameterPoint]) -> Node {
    let space = build_parameter_space(&children, leaves);
    Node::SubTree { space, children }
}

/// 分桶时官方把节点放进新的列表，这里只搬动叶子下标和子树，所以照原样重建。
fn clone_node(node: &Node, leaves: &[ParameterPoint]) -> Node {
    match node {
        Node::Leaf(index) => Node::Leaf(*index),
        Node::SubTree { children, .. } => {
            let children = children
                .iter()
                .map(|child| clone_node(child, leaves))
                .collect();
            sub_tree(children, leaves)
        }
    }
}

/// `Climate.Sampler`：六个密度函数采样器加上 quart 坐标到方块坐标的换算。
#[derive(Debug)]
pub struct Sampler<'a> {
    pub temperature: &'a crate::sampler::Sampler,
    pub humidity: &'a crate::sampler::Sampler,
    pub continentalness: &'a crate::sampler::Sampler,
    pub erosion: &'a crate::sampler::Sampler,
    pub depth: &'a crate::sampler::Sampler,
    pub weirdness: &'a crate::sampler::Sampler,
}

impl Sampler<'_> {
    /// `Climate.Sampler.sample(quartX, quartY, quartZ)`。
    pub fn sample(
        &self,
        context: &mut crate::sampler::Context,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> TargetPoint {
        let x = quart_to_block(quart_x);
        let y = quart_to_block(quart_y);
        let z = quart_to_block(quart_z);
        let path = crate::sampler::Path::Point;
        TargetPoint::of([
            self.temperature.sample(context, path, x, y, z),
            self.humidity.sample(context, path, x, y, z),
            self.continentalness.sample(context, path, x, y, z),
            self.erosion.sample(context, path, x, y, z),
            self.depth.sample(context, path, x, y, z),
            self.weirdness.sample(context, path, x, y, z),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(temperature: i64, humidity: i64) -> ParameterPoint {
        ParameterPoint::from_quantized([
            temperature,
            temperature,
            humidity,
            humidity,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ])
    }

    fn target(temperature: i64, humidity: i64) -> TargetPoint {
        TargetPoint {
            temperature,
            humidity,
            continentalness: 0,
            erosion: 0,
            depth: 0,
            weirdness: 0,
        }
    }

    #[test]
    fn quantization_truncates_towards_zero() {
        assert_eq!(quantize(0.4), 4000);
        assert_eq!(quantize(-0.4), -4000);
        assert_eq!(quantize(1.0), 10_000);
        assert_eq!(unquantize(3750), 0.375);
    }

    #[test]
    fn parameter_distance_is_zero_inside_the_span() {
        let parameter = Parameter {
            min: -100,
            max: 100,
        };
        assert_eq!(parameter.distance(0), 0);
        assert_eq!(parameter.distance(100), 0);
        assert_eq!(parameter.distance(150), 50);
        assert_eq!(parameter.distance(-150), 50);
    }

    /// R 树找到的那一条和最小的 fitness 一样。
    ///
    /// 不能直接和暴力查找比值：平局时两边挑的不是同一条（暴力取列表里最靠前的，
    /// R 树取遍历先碰到的），官方也是这样。
    fn check_nearest<T: std::fmt::Debug + PartialEq>(
        list: &ParameterList<T>,
        target: TargetPoint,
        place: &str,
    ) {
        let best = list
            .entries()
            .map(|(point, _)| point.fitness(target))
            .min()
            .expect("至少一条");
        let found = list.find_value(target);
        let found_fitness = list
            .entries()
            .find(|(_, value)| *value == found)
            .map(|(point, _)| point.fitness(target))
            .expect("找到的那一条在表里");
        assert_eq!(found_fitness, best, "{place}");
    }

    /// 条目少到不用分桶时，R 树找的是最近的一条。
    #[test]
    fn shallow_trees_find_a_nearest_entry() {
        let mut entries = Vec::new();
        for temperature in -2..=2_i64 {
            for humidity in -2..=2_i64 {
                entries.push((
                    point(temperature * 5000, humidity * 5000),
                    format!("{temperature}:{humidity}"),
                ));
            }
        }
        let list = ParameterList::new(entries);
        for temperature in -20..=20_i64 {
            for humidity in -20..=20_i64 {
                let target = target(temperature * 500, humidity * 500);
                check_nearest(&list, target, &format!("{temperature} {humidity}"));
            }
        }
    }

    /// 条目多到要分桶、要分层时也一样。
    #[test]
    fn deep_trees_find_a_nearest_entry() {
        let mut entries = Vec::new();
        for index in 0..200_i64 {
            entries.push((
                point((index % 17 - 8) * 1200, (index / 17 - 5) * 1500),
                index,
            ));
        }
        let list = ParameterList::new(entries);
        for index in -40..40_i64 {
            let target = target(index * 250, index * -300);
            check_nearest(&list, target, &format!("{index}"));
        }
    }

    /// 「上一次的结果」会被当成初始候选，平局时它赢。
    #[test]
    fn the_previous_result_wins_ties() {
        let entries = vec![(point(-1000, 0), "a"), (point(1000, 0), "b")];
        let list = ParameterList::new(entries);
        list.reset();
        // 正中间的目标到两条的距离相同。
        let first = *list.find_value(target(0, 0));
        let again = *list.find_value(target(0, 0));
        assert_eq!(first, again);
    }
}
