//! 寻路：官方 `PathFinder`、`BinaryHeap`、`Node`、`Path`、`WalkNodeEvaluator` 的逐位移植。
//!
//! 开放表是和官方同一套上滤 / 下滤规则的二叉堆，等 `f` 的节点按同样的顺序出堆；
//! 邻居按 北、东、南、西、东北、东南、西南、西北 的顺序产生。节点缓存按官方的
//! 32 位 `Node.createHash` 做键，牛的搜索半径内不会撞键。
//!
//! 寻路只读一个只读视图（[`BlockView`]），没有副作用，算一次是一个纯函数：给定起点、
//! 终点和世界，结果唯一。精确模式下当场同步调用；以后挪到独立的寻路阶段时，
//! 请求就是 [`PathRequest`]、结果就是 [`Path`]。
//!
//! Pathfinding: a bit-exact port of vanilla `PathFinder`, `BinaryHeap`, `Node`, `Path`
//! and `WalkNodeEvaluator`. The search reads a read-only [`BlockView`] and has no side
//! effects; [`PathRequest`] and [`Path`] are the seam for moving it to its own phase later.

use std::collections::HashMap;

use mc_bamboo_core::{Aabb, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{PathType, blocks, tags::fluid_tags};

use crate::{
    entity::ctx::BlockView,
    shape::{self, CollisionContext},
    state::BlockState,
};

/// 默认的路径类型代价，`PathType.getMalus()`，按枚举顺序。
const DEFAULT_MALUS: [f32; 27] = [
    -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, -1.0, -1.0, 8.0, 8.0, 0.0, -1.0, 8.0, 16.0, 8.0, -1.0,
    0.0, -1.0, -1.0, 4.0, -1.0, 8.0, 0.0, 0.0, 0.0, 4.0,
];

/// `PathType.getMalus()`。
pub fn default_malus(path_type: PathType) -> f32 {
    DEFAULT_MALUS[path_type as usize]
}

/// 生物自己设的路径代价，`Mob.pathfindingMalus`；没设的用默认值。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Malus {
    overrides: [Option<f32>; 27],
}

impl Malus {
    /// `Animal` 构造函数：邻格着火 16，着火 -1。
    pub fn animal() -> Self {
        let mut malus = Self::default();
        malus.set(PathType::FireInNeighbor, 16.0);
        malus.set(PathType::Fire, -1.0);
        malus
    }

    pub fn set(&mut self, path_type: PathType, value: f32) {
        self.overrides[path_type as usize] = Some(value);
    }

    /// `Mob.getPathfindingMalus`。
    pub fn get(&self, path_type: PathType) -> f32 {
        self.overrides[path_type as usize].unwrap_or_else(|| default_malus(path_type))
    }
}

/// 寻路时要知道的生物事实，由调用方从生物当时的状态拍下来。
///
/// The mob facts the search needs, snapshotted by the caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathMob {
    pub position: Vec3,
    pub on_ground: bool,
    /// `isInFloatableFluid()`。
    pub in_floatable_fluid: bool,
    pub width: f32,
    pub height: f32,
    /// `maxUpStep()`。
    pub max_up_step: f32,
    /// `getMaxFallDistance()`。
    pub max_fall_distance: i32,
    pub malus: Malus,
    /// `CollisionContext.of(mob)`。
    pub context: CollisionContext,
}

impl PathMob {
    fn block_position(&self) -> BlockPos {
        BlockPos::containing(self.position)
    }

    /// `getBoundingBox()`。
    pub fn bounding_box(&self) -> Aabb {
        let half = f64::from(self.width / 2.0);
        Aabb::new(
            self.position.x - half,
            self.position.y,
            self.position.z - half,
            self.position.x + half,
            self.position.y + f64::from(self.height),
            self.position.z + half,
        )
    }
}

/// 节点求值器的开关，`NodeEvaluator` 的四个布尔字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluatorFlags {
    pub can_pass_doors: bool,
    pub can_open_doors: bool,
    pub can_float: bool,
    pub can_walk_over_fences: bool,
}

impl Default for EvaluatorFlags {
    fn default() -> Self {
        Self {
            can_pass_doors: true,
            can_open_doors: false,
            can_float: false,
            can_walk_over_fences: false,
        }
    }
}

/// 路径上的一个节点，官方 `Node` 的字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub heap_idx: i32,
    pub g: f32,
    pub h: f32,
    pub f: f32,
    pub came_from: Option<usize>,
    pub closed: bool,
    pub walked_distance: f32,
    pub cost_malus: f32,
    pub path_type: PathType,
}

impl Node {
    fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            x,
            y,
            z,
            heap_idx: -1,
            g: 0.0,
            h: 0.0,
            f: 0.0,
            came_from: None,
            closed: false,
            walked_distance: 0.0,
            cost_malus: 0.0,
            path_type: PathType::Blocked,
        }
    }

    /// `Node.createHash`。
    pub fn create_hash(x: i32, y: i32, z: i32) -> i32 {
        (y & 255)
            | ((x & 32767) << 8)
            | ((z & 32767) << 24)
            | if x < 0 { i32::MIN } else { 0 }
            | if z < 0 { 32768 } else { 0 }
    }

    pub const fn pos(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z)
    }

    /// `Node.distanceTo(Node)`。
    fn distance_to(&self, x: i32, y: i32, z: i32) -> f32 {
        let dx = (x - self.x) as f32;
        let dy = (y - self.y) as f32;
        let dz = (z - self.z) as f32;
        mth::sqrt_f32(dx * dx + dy * dy + dz * dz)
    }

    /// `Node.distanceManhattan`。
    fn distance_manhattan(&self, x: i32, y: i32, z: i32) -> f32 {
        (x - self.x).abs() as f32 + (y - self.y).abs() as f32 + (z - self.z).abs() as f32
    }

    const fn in_open_set(&self) -> bool {
        self.heap_idx >= 0
    }
}

/// 一条路径，官方 `Path`。
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub nodes: Vec<Node>,
    pub next_node_index: usize,
    pub target: BlockPos,
    pub dist_to_target: f32,
    pub reached: bool,
}

impl Path {
    fn new(nodes: Vec<Node>, target: BlockPos, reached: bool) -> Self {
        let dist_to_target = nodes.last().map_or(f32::MAX, |node| {
            node.distance_manhattan(target.x, target.y, target.z)
        });
        Self {
            nodes,
            next_node_index: 0,
            target,
            dist_to_target,
            reached,
        }
    }

    pub fn advance(&mut self) {
        self.next_node_index += 1;
    }

    pub fn is_done(&self) -> bool {
        self.next_node_index >= self.nodes.len()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn next_node(&self) -> &Node {
        &self.nodes[self.next_node_index]
    }

    pub fn next_node_pos(&self) -> BlockPos {
        self.nodes[self.next_node_index].pos()
    }

    pub fn end_node(&self) -> Option<&Node> {
        self.nodes.last()
    }

    /// `getEntityPosAtNode`：`(int)(bbWidth + 1.0F)` 是截断，不是向下取整。
    pub fn entity_pos_at_node(&self, width: f32, index: usize) -> Vec3 {
        let node = &self.nodes[index];
        let offset = f64::from((width + 1.0) as i32) * 0.5;
        Vec3::new(
            f64::from(node.x) + offset,
            f64::from(node.y),
            f64::from(node.z) + offset,
        )
    }

    pub fn next_entity_pos(&self, width: f32) -> Vec3 {
        self.entity_pos_at_node(width, self.next_node_index)
    }

    /// `Path.sameAs`：节点表按 `Node.equals`（坐标）相等。
    pub fn same_as(&self, other: Option<&Path>) -> bool {
        other.is_some_and(|other| {
            self.nodes.len() == other.nodes.len()
                && self
                    .nodes
                    .iter()
                    .zip(&other.nodes)
                    .all(|(a, b)| a.x == b.x && a.y == b.y && a.z == b.z)
        })
    }
}

/// 寻路请求：起点由生物快照决定，终点一个格子。以后独立的寻路阶段按它去重。
///
/// A path request; a later dedicated pathfinding phase can deduplicate on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathRequest {
    pub mob: PathMob,
    pub flags: EvaluatorFlags,
    pub target: BlockPos,
    pub max_distance_from_start: f32,
    pub reach_range: i32,
    pub max_visited_nodes: i32,
    pub max_visited_nodes_multiplier: f32,
}

/// 一次搜索的终点，`Target`。
struct Target {
    x: i32,
    y: i32,
    z: i32,
    best_heuristic: f32,
    best_node: Option<usize>,
    reached: bool,
}

/// 一次搜索的工作区：节点表、开放表、路径类型缓存。
struct Search<'a> {
    view: &'a dyn BlockView,
    mob: PathMob,
    flags: EvaluatorFlags,
    nodes: Vec<Node>,
    by_hash: HashMap<i32, usize>,
    heap: Vec<usize>,
    type_cache: HashMap<(i32, i32, i32), PathType>,
    entity_width: i32,
    entity_height: i32,
    entity_depth: i32,
    min_y: i32,
}

/// `PathFinder.findPath` 加上 `PathNavigation.createPath` 里那段 `findPath` 调用。
pub fn find_path(view: &dyn BlockView, request: &PathRequest) -> Option<Path> {
    let mut search = Search::new(view, request.mob, request.flags);
    let start = search.get_start();
    let mut target = {
        let node = search.get_node(request.target.x, request.target.y, request.target.z);
        let node = search.nodes[node];
        Target {
            x: node.x,
            y: node.y,
            z: node.z,
            best_heuristic: f32::MAX,
            best_node: None,
            reached: false,
        }
    };
    search.run(start, &mut target, request);
    let best = target.best_node?;
    let mut chain = vec![search.nodes[best]];
    let mut current = search.nodes[best].came_from;
    while let Some(index) = current {
        chain.push(search.nodes[index]);
        current = search.nodes[index].came_from;
    }
    chain.reverse();
    Some(Path::new(chain, request.target, target.reached))
}

impl<'a> Search<'a> {
    fn new(view: &'a dyn BlockView, mob: PathMob, flags: EvaluatorFlags) -> Self {
        Self {
            view,
            mob,
            flags,
            nodes: Vec::new(),
            by_hash: HashMap::new(),
            heap: Vec::new(),
            type_cache: HashMap::new(),
            entity_width: mth::floor_f32(mob.width + 1.0),
            entity_height: mth::floor_f32(mob.height + 1.0),
            entity_depth: mth::floor_f32(mob.width + 1.0),
            min_y: view.min_y(),
        }
    }

    // ---- 区域读取：`PathNavigationRegion` 把没加载的区块当成虚空 ----

    fn state(&self, pos: BlockPos) -> BlockState {
        if pos.y < self.min_y || pos.y > self.view.max_y() {
            return BlockState::AIR;
        }
        if !self.view.is_loaded(pos) {
            return BlockState(blocks::VOID_AIR);
        }
        self.view.block_state(pos)
    }

    fn collision_shape_empty(&self, pos: BlockPos) -> Vec<Aabb> {
        let state = self.state(pos);
        let piston = state
            .is(blocks::MOVING_PISTON)
            .then(|| self.view.moving_piston_shape(pos))
            .flatten();
        shape::block_collision_boxes(
            state,
            pos,
            CollisionContext::Empty,
            piston.as_deref(),
            self.view.shulker_box_shape(pos),
        )
        .into_owned()
    }

    // ---- 节点表 ----

    fn get_node(&mut self, x: i32, y: i32, z: i32) -> usize {
        let hash = Node::create_hash(x, y, z);
        if let Some(&index) = self.by_hash.get(&hash) {
            return index;
        }
        self.nodes.push(Node::new(x, y, z));
        let index = self.nodes.len() - 1;
        self.by_hash.insert(hash, index);
        index
    }

    // ---- 二叉堆，`BinaryHeap` ----

    fn heap_insert(&mut self, node: usize) {
        self.heap.push(node);
        let index = self.heap.len() - 1;
        self.nodes[node].heap_idx = index as i32;
        self.up_heap(index);
    }

    fn heap_pop(&mut self) -> usize {
        let top = self.heap[0];
        let last = self.heap.pop().expect("非空");
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.nodes[last].heap_idx = 0;
            self.down_heap(0);
        }
        self.nodes[top].heap_idx = -1;
        top
    }

    fn change_cost(&mut self, node: usize, cost: f32) {
        let old = self.nodes[node].f;
        self.nodes[node].f = cost;
        let index = self.nodes[node].heap_idx as usize;
        if cost < old {
            self.up_heap(index);
        } else {
            self.down_heap(index);
        }
    }

    fn up_heap(&mut self, mut index: usize) {
        let node = self.heap[index];
        let cost = self.nodes[node].f;
        while index > 0 {
            let parent_index = (index - 1) >> 1;
            let parent = self.heap[parent_index];
            if !less(cost, self.nodes[parent].f) {
                break;
            }
            self.heap[index] = parent;
            self.nodes[parent].heap_idx = index as i32;
            index = parent_index;
        }
        self.heap[index] = node;
        self.nodes[node].heap_idx = index as i32;
    }

    fn down_heap(&mut self, mut index: usize) {
        let node = self.heap[index];
        let cost = self.nodes[node].f;
        let size = self.heap.len();
        loop {
            let left_index = 1 + (index << 1);
            let right_index = left_index + 1;
            if left_index >= size {
                break;
            }
            let left = self.heap[left_index];
            let left_cost = self.nodes[left].f;
            let (right, right_cost) = if right_index >= size {
                (None, f32::INFINITY)
            } else {
                let right = self.heap[right_index];
                (Some(right), self.nodes[right].f)
            };
            if left_cost < right_cost {
                if !less(left_cost, cost) {
                    break;
                }
                self.heap[index] = left;
                self.nodes[left].heap_idx = index as i32;
                index = left_index;
            } else {
                if !less(right_cost, cost) {
                    break;
                }
                let right = right.expect("右孩子的代价不是无穷大");
                self.heap[index] = right;
                self.nodes[right].heap_idx = index as i32;
                index = right_index;
            }
        }
        self.heap[index] = node;
        self.nodes[node].heap_idx = index as i32;
    }

    // ---- `PathFinder.findPath` 的主循环 ----

    fn best_h(&self, node: usize, target: &mut Target) -> f32 {
        let distance = self.nodes[node].distance_to(target.x, target.y, target.z);
        if distance < target.best_heuristic {
            target.best_heuristic = distance;
            target.best_node = Some(node);
        }
        mth::java_min_f32(distance, f32::MAX)
    }

    fn run(&mut self, start: usize, target: &mut Target, request: &PathRequest) {
        self.nodes[start].g = 0.0;
        let h = self.best_h(start, target);
        self.nodes[start].h = h;
        self.nodes[start].f = h;
        self.heap.clear();
        self.heap_insert(start);
        let max_visited =
            (request.max_visited_nodes as f32 * request.max_visited_nodes_multiplier) as i32;
        let max_distance = request.max_distance_from_start;
        let mut visited = 0;
        let mut neighbours = Vec::with_capacity(8);
        while !self.heap.is_empty() {
            visited += 1;
            if visited >= max_visited {
                break;
            }
            let current = self.heap_pop();
            self.nodes[current].closed = true;
            let node = self.nodes[current];
            if node.distance_manhattan(target.x, target.y, target.z) <= request.reach_range as f32 {
                target.reached = true;
                break;
            }
            let start_node = self.nodes[start];
            if node.distance_to(start_node.x, start_node.y, start_node.z) >= max_distance {
                continue;
            }
            neighbours.clear();
            self.get_neighbors(current, &mut neighbours);
            for &neighbour in &neighbours {
                let other = self.nodes[neighbour];
                let current_node = self.nodes[current];
                let distance = current_node.distance_to(other.x, other.y, other.z);
                self.nodes[neighbour].walked_distance = current_node.walked_distance + distance;
                let g = current_node.g + distance + other.cost_malus;
                let in_open = self.nodes[neighbour].in_open_set();
                if self.nodes[neighbour].walked_distance < max_distance
                    && (!in_open || g < self.nodes[neighbour].g)
                {
                    self.nodes[neighbour].came_from = Some(current);
                    self.nodes[neighbour].g = g;
                    let h = self.best_h(neighbour, target) * 1.5;
                    self.nodes[neighbour].h = h;
                    let f = g + h;
                    if in_open {
                        self.change_cost(neighbour, f);
                    } else {
                        self.nodes[neighbour].f = f;
                        self.heap_insert(neighbour);
                    }
                }
            }
        }
    }

    // ---- `WalkNodeEvaluator` ----

    fn malus(&self, path_type: PathType) -> f32 {
        self.mob.malus.get(path_type)
    }

    fn mob_jump_height(&self) -> f64 {
        mth::java_max(1.125, f64::from(self.mob.max_up_step))
    }

    /// `getStart`。
    fn get_start(&mut self) -> usize {
        let mob = self.mob;
        let mut y = mth::floor(mob.position.y);
        let containing =
            |y: i32| BlockPos::new(mth::floor(mob.position.x), y, mth::floor(mob.position.z));
        let mut state = self.state(containing(y));
        if self.flags.can_float && mob.in_floatable_fluid {
            while state.fluid().is_tagged(fluid_tags::ENTITY_FLOATABLE) {
                y += 1;
                state = self.state(containing(y));
            }
            y -= 1;
        } else if mob.on_ground {
            y = mth::floor(mob.position.y + 0.5);
        } else {
            let mut cursor = mth::floor(mob.position.y + 1.0);
            while cursor > self.min_y {
                y = cursor;
                cursor -= 1;
                let below = self.state(containing(cursor));
                if !below.is_air() && !is_pathfindable_land(below) {
                    break;
                }
            }
        }
        let block = mob.block_position();
        if !self.can_start_at(BlockPos::new(block.x, y, block.z)) {
            let bounds = mob.bounding_box();
            let corners = [
                (bounds.min_x, bounds.min_z),
                (bounds.min_x, bounds.max_z),
                (bounds.max_x, bounds.min_z),
                (bounds.max_x, bounds.max_z),
            ];
            for (x, z) in corners {
                let pos = BlockPos::new(mth::floor(x), y, mth::floor(z));
                if self.can_start_at(pos) {
                    return self.get_start_node(pos);
                }
            }
        }
        self.get_start_node(BlockPos::new(block.x, y, block.z))
    }

    fn get_start_node(&mut self, pos: BlockPos) -> usize {
        let node = self.get_node(pos.x, pos.y, pos.z);
        let path_type = self.cached_path_type(pos.x, pos.y, pos.z);
        self.nodes[node].path_type = path_type;
        self.nodes[node].cost_malus = self.malus(path_type);
        node
    }

    fn can_start_at(&mut self, pos: BlockPos) -> bool {
        let path_type = self.cached_path_type(pos.x, pos.y, pos.z);
        path_type != PathType::Open && self.malus(path_type) >= 0.0
    }

    /// `getNeighbors`：北、东、南、西，然后四个对角。
    fn get_neighbors(&mut self, parent: usize, out: &mut Vec<usize>) {
        let node = self.nodes[parent];
        let mut jump_size = 0;
        let above = self.cached_path_type(node.x, node.y + 1, node.z);
        let here = self.cached_path_type(node.x, node.y, node.z);
        if self.malus(above) >= 0.0 && here != PathType::StickyHoney {
            jump_size = mth::floor_f32(mth::java_max_f32(1.0, self.mob.max_up_step));
        }
        let floor = self.floor_level(node.pos());
        let mut reusable: [Option<usize>; 4] = [None; 4];
        for direction in HORIZONTAL {
            let (dx, _, dz) = direction.normal();
            let found = self.find_accepted_node(
                node.x + dx,
                node.y,
                node.z + dz,
                jump_size,
                floor,
                direction,
                here,
            );
            reusable[data_2d(direction)] = found;
            if self.is_neighbor_valid(found, parent) {
                out.push(found.expect("刚检查过"));
            }
        }
        for direction in HORIZONTAL {
            let clockwise = direction.clockwise();
            if self.is_diagonal_valid(
                parent,
                reusable[data_2d(direction)],
                reusable[data_2d(clockwise)],
            ) {
                let (dx, _, dz) = direction.normal();
                let (cx, _, cz) = clockwise.normal();
                let found = self.find_accepted_node(
                    node.x + dx + cx,
                    node.y,
                    node.z + dz + cz,
                    jump_size,
                    floor,
                    direction,
                    here,
                );
                if self.is_diagonal_node_valid(found) {
                    out.push(found.expect("刚检查过"));
                }
            }
        }
    }

    fn is_neighbor_valid(&self, node: Option<usize>, parent: usize) -> bool {
        node.is_some_and(|index| {
            let node = &self.nodes[index];
            !node.closed && (node.cost_malus >= 0.0 || self.nodes[parent].cost_malus < 0.0)
        })
    }

    fn is_diagonal_valid(&self, root: usize, first: Option<usize>, second: Option<usize>) -> bool {
        let (Some(first), Some(second)) = (first, second) else {
            return false;
        };
        let root = &self.nodes[root];
        let (a, b) = (&self.nodes[first], &self.nodes[second]);
        if b.y > root.y || a.y > root.y {
            return false;
        }
        if a.path_type == PathType::WalkableDoor || b.path_type == PathType::WalkableDoor {
            return false;
        }
        if self.mob.width > 1.0 && (a.cost_malus > 0.0 || b.cost_malus > 0.0) {
            return false;
        }
        let fence_like = b.path_type == PathType::Fence
            && a.path_type == PathType::Fence
            && f64::from(self.mob.width) < 0.5;
        (b.y < root.y || b.cost_malus >= 0.0 || fence_like)
            && (a.y < root.y || a.cost_malus >= 0.0 || fence_like)
    }

    fn is_diagonal_node_valid(&self, node: Option<usize>) -> bool {
        node.is_some_and(|index| {
            let node = &self.nodes[index];
            !node.closed && node.path_type != PathType::WalkableDoor && node.cost_malus >= 0.0
        })
    }

    #[expect(clippy::too_many_arguments, reason = "官方方法就是这几个参数")]
    fn find_accepted_node(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        jump_size: i32,
        parent_floor: f64,
        direction: Direction,
        parent_type: PathType,
    ) -> Option<usize> {
        let mut node = None;
        let floor = self.floor_level(BlockPos::new(x, y, z));
        if floor - parent_floor > self.mob_jump_height() {
            return None;
        }
        let path_type = self.cached_path_type(x, y, z);
        let malus = self.malus(path_type);
        if malus >= 0.0 {
            node = Some(self.node_with_cost(x, y, z, path_type, malus));
        }
        if partial_collision(parent_type)
            && let Some(index) = node
            && self.nodes[index].cost_malus >= 0.0
            && !self.can_reach_without_collision(index)
        {
            node = None;
        }
        if path_type == PathType::Walkable {
            return node;
        }
        let blocked = node.is_none_or(|index| self.nodes[index].cost_malus < 0.0);
        if blocked
            && jump_size > 0
            && (path_type != PathType::Fence || self.flags.can_walk_over_fences)
            && path_type != PathType::UnpassableRail
            && path_type != PathType::Trapdoor
            && path_type != PathType::PowderSnow
        {
            node = self.try_jump_on(x, y, z, jump_size, parent_floor, direction, parent_type);
        } else if path_type == PathType::Water && !self.flags.can_float {
            node = self.first_non_water_below(x, y, z, node);
        } else if path_type == PathType::Open {
            node = self.first_ground_below(x, y, z);
        } else if partial_collision(path_type) && node.is_none() {
            node = Some(self.closed_node(x, y, z, path_type));
        }
        node
    }

    fn node_with_cost(&mut self, x: i32, y: i32, z: i32, path_type: PathType, malus: f32) -> usize {
        let node = self.get_node(x, y, z);
        self.nodes[node].path_type = path_type;
        self.nodes[node].cost_malus = mth::java_max_f32(self.nodes[node].cost_malus, malus);
        node
    }

    fn blocked_node(&mut self, x: i32, y: i32, z: i32) -> usize {
        let node = self.get_node(x, y, z);
        self.nodes[node].path_type = PathType::Blocked;
        self.nodes[node].cost_malus = -1.0;
        node
    }

    fn closed_node(&mut self, x: i32, y: i32, z: i32, path_type: PathType) -> usize {
        let node = self.get_node(x, y, z);
        self.nodes[node].closed = true;
        self.nodes[node].path_type = path_type;
        self.nodes[node].cost_malus = default_malus(path_type);
        node
    }

    #[expect(clippy::too_many_arguments, reason = "官方方法就是这几个参数")]
    fn try_jump_on(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        jump_size: i32,
        parent_floor: f64,
        direction: Direction,
        parent_type: PathType,
    ) -> Option<usize> {
        let index = self.find_accepted_node(
            x,
            y + 1,
            z,
            jump_size - 1,
            parent_floor,
            direction,
            parent_type,
        )?;
        if self.mob.width >= 1.0 {
            return Some(index);
        }
        let node = self.nodes[index];
        if node.path_type != PathType::Open && node.path_type != PathType::Walkable {
            return Some(index);
        }
        let (sx, _, sz) = direction.normal();
        let centre_x = f64::from(x - sx) + 0.5;
        let centre_z = f64::from(z - sz) + 0.5;
        let half = f64::from(self.mob.width) / 2.0;
        let bottom = self.floor_level(BlockPos::new(
            mth::floor(centre_x),
            y + 1,
            mth::floor(centre_z),
        )) + 0.001;
        let top = f64::from(self.mob.height) + self.floor_level(node.pos()) - 0.002;
        // 官方 `AABB` 的构造函数逐轴取小、取大。
        // The vanilla `AABB` constructor takes the min and max per axis.
        let bounds = Aabb::new(
            centre_x - half,
            mth::java_min(bottom, top),
            centre_z - half,
            centre_x + half,
            mth::java_max(bottom, top),
            centre_z + half,
        );
        if self.has_collisions(bounds) {
            None
        } else {
            Some(index)
        }
    }

    fn first_non_water_below(
        &mut self,
        x: i32,
        mut y: i32,
        z: i32,
        mut node: Option<usize>,
    ) -> Option<usize> {
        y -= 1;
        while y > self.min_y {
            let path_type = self.cached_path_type(x, y, z);
            if path_type != PathType::Water {
                return node;
            }
            let malus = self.malus(path_type);
            node = Some(self.node_with_cost(x, y, z, path_type, malus));
            y -= 1;
        }
        node
    }

    fn first_ground_below(&mut self, x: i32, y: i32, z: i32) -> Option<usize> {
        let mut current = y - 1;
        while current >= self.min_y {
            if y - current > self.mob.max_fall_distance {
                return Some(self.blocked_node(x, current, z));
            }
            let path_type = self.cached_path_type(x, current, z);
            let malus = self.malus(path_type);
            if path_type != PathType::Open {
                if malus >= 0.0 {
                    return Some(self.node_with_cost(x, current, z, path_type, malus));
                }
                return Some(self.blocked_node(x, current, z));
            }
            current -= 1;
        }
        Some(self.blocked_node(x, y, z))
    }

    /// `canReachWithoutCollision`：从生物现在的碰撞盒扫过去，不是从父节点。
    fn can_reach_without_collision(&self, node: usize) -> bool {
        let node = self.nodes[node];
        let mut bounds = self.mob.bounding_box();
        let size_x = bounds.max_x - bounds.min_x;
        let size_y = bounds.max_y - bounds.min_y;
        let size_z = bounds.max_z - bounds.min_z;
        let mut delta = Vec3::new(
            f64::from(node.x) - self.mob.position.x + size_x / 2.0,
            f64::from(node.y) - self.mob.position.y + size_y / 2.0,
            f64::from(node.z) - self.mob.position.z + size_z / 2.0,
        );
        let size = (size_x + size_y + size_z) / 3.0;
        let steps = mth::ceil(delta.length() / size);
        delta = delta.scale(f64::from(1.0_f32 / steps as f32));
        for _ in 1..=steps {
            bounds = bounds.moved(delta.x, delta.y, delta.z);
            if self.has_collisions(bounds) {
                return false;
            }
        }
        true
    }

    /// `hasCollisions`：`!noCollision(mob, box)`，方块按生物的碰撞上下文，加上世界边界。
    fn has_collisions(&self, bounds: Aabb) -> bool {
        has_block_collision(self.view, bounds, self.mob.context, |pos| self.state(pos))
    }

    fn floor_level(&self, pos: BlockPos) -> f64 {
        if self.flags.can_float
            && self
                .state(pos)
                .fluid()
                .is_tagged(fluid_tags::ENTITY_FLOATABLE)
        {
            return f64::from(pos.y) + 0.5;
        }
        let below = pos.below();
        let shape = self.collision_shape_empty(below);
        f64::from(below.y) + max_y(&shape)
    }

    fn cached_path_type(&mut self, x: i32, y: i32, z: i32) -> PathType {
        if let Some(&found) = self.type_cache.get(&(x, y, z)) {
            return found;
        }
        let found = self.path_type_of_mob(x, y, z);
        self.type_cache.insert((x, y, z), found);
        found
    }

    /// `getPathTypeOfMob`。
    fn path_type_of_mob(&self, x: i32, y: i32, z: i32) -> PathType {
        let mut types = [false; 27];
        let mut count = 0;
        for i in 0..self.entity_width {
            for j in 0..self.entity_height {
                for k in 0..self.entity_depth {
                    let mut path_type = self.path_type_static(BlockPos::new(i + x, j + y, k + z));
                    let mob_pos = self.mob.block_position();
                    if path_type == PathType::DoorWoodClosed
                        && self.flags.can_open_doors
                        && self.flags.can_pass_doors
                    {
                        path_type = PathType::WalkableDoor;
                    }
                    if path_type == PathType::DoorOpen && !self.flags.can_pass_doors {
                        path_type = PathType::Blocked;
                    }
                    if path_type == PathType::Rail
                        && self.path_type_static(mob_pos) != PathType::Rail
                        && self.path_type_static(mob_pos.below()) != PathType::Rail
                    {
                        path_type = PathType::UnpassableRail;
                    }
                    if !types[path_type as usize] {
                        types[path_type as usize] = true;
                        count += 1;
                    }
                }
            }
        }
        let ordered = ALL_PATH_TYPES
            .iter()
            .copied()
            .filter(|&t| types[t as usize]);
        if count == 1 {
            return ordered.clone().next().expect("一种");
        }
        if types[PathType::Fence as usize] {
            return PathType::Fence;
        }
        if types[PathType::UnpassableRail as usize] {
            return PathType::UnpassableRail;
        }
        let mut best = PathType::Blocked;
        let mut best_malus = self.malus(PathType::Blocked);
        for path_type in ordered {
            let malus = self.malus(path_type);
            if malus < 0.0 {
                return path_type;
            }
            if malus >= best_malus {
                best_malus = malus;
                best = path_type;
            }
        }
        let centre = self.path_type_static(BlockPos::new(x, y, z));
        if self.entity_width > 1 {
            let lower = self.malus(centre) < best_malus;
            let danger = lower && self.malus(PathType::BigMobsCloseToDanger) < best_malus;
            return if danger {
                PathType::BigMobsCloseToDanger
            } else {
                best
            };
        }
        if centre == PathType::Open && best != PathType::Open && best_malus == 0.0 {
            return PathType::Open;
        }
        best
    }

    fn path_type_static(&self, pos: BlockPos) -> PathType {
        path_type_static_with(&|pos| self.state(pos), self.min_y, pos)
    }
}

/// `WalkNodeEvaluator.getPathTypeStatic(mob, pos)`：直接读世界，不经寻路区域。
pub fn static_path_type(view: &dyn BlockView, pos: BlockPos) -> PathType {
    path_type_static_with(&|pos| view.block_state(pos), view.min_y(), pos)
}

/// `getPathTypeStatic`。
fn path_type_static_with(
    state_at: &dyn Fn(BlockPos) -> BlockState,
    min_y: i32,
    pos: BlockPos,
) -> PathType {
    let own = state_path_type(state_at(pos));
    if own != PathType::Open || pos.y < min_y + 1 {
        return own;
    }
    match state_path_type(state_at(pos.below())) {
        PathType::Open | PathType::Water | PathType::Lava | PathType::Walkable => PathType::Open,
        PathType::Fire => PathType::Fire,
        PathType::Damaging => PathType::Damaging,
        PathType::StickyHoney => PathType::StickyHoney,
        PathType::PowderSnow => PathType::OnTopOfPowderSnow,
        PathType::DamageCautious => PathType::DamageCautious,
        PathType::Trapdoor => PathType::OnTopOfTrapdoor,
        _ => check_neighbour_blocks(state_at, pos),
    }
}

/// `checkNeighbourBlocks(ctx, x, y, z, WALKABLE)`：按 x、y、z 的顺序，第一个危险的邻居说了算。
fn check_neighbour_blocks(state_at: &dyn Fn(BlockPos) -> BlockState, pos: BlockPos) -> PathType {
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                match state_path_type(state_at(pos.offset(dx, dy, dz))) {
                    PathType::Damaging => return PathType::DamagingInNeighbor,
                    PathType::Fire | PathType::Lava => return PathType::FireInNeighbor,
                    PathType::Water => return PathType::WaterBorder,
                    PathType::DamageCautious => return PathType::DamageCautious,
                    _ => {}
                }
            }
        }
    }
    PathType::Walkable
}

/// Java 的 `a < b`：有 NaN 时为假。堆的上滤、下滤写成「不小于就停」。
fn less(first: f32, second: f32) -> bool {
    first.partial_cmp(&second) == Some(std::cmp::Ordering::Less)
}

/// `Direction.Plane.HORIZONTAL` 的顺序。
const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `Direction.get2DDataValue`。
const fn data_2d(direction: Direction) -> usize {
    match direction {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        _ => 3,
    }
}

const ALL_PATH_TYPES: [PathType; 27] = [
    PathType::Blocked,
    PathType::Open,
    PathType::Walkable,
    PathType::WalkableDoor,
    PathType::Trapdoor,
    PathType::PowderSnow,
    PathType::OnTopOfPowderSnow,
    PathType::Fence,
    PathType::Lava,
    PathType::Water,
    PathType::WaterBorder,
    PathType::Rail,
    PathType::UnpassableRail,
    PathType::FireInNeighbor,
    PathType::Fire,
    PathType::DamagingInNeighbor,
    PathType::Damaging,
    PathType::DoorOpen,
    PathType::DoorWoodClosed,
    PathType::DoorIronClosed,
    PathType::Breach,
    PathType::Leaves,
    PathType::StickyHoney,
    PathType::Cocoa,
    PathType::DamageCautious,
    PathType::OnTopOfTrapdoor,
    PathType::BigMobsCloseToDanger,
];

fn partial_collision(path_type: PathType) -> bool {
    matches!(
        path_type,
        PathType::Fence | PathType::DoorWoodClosed | PathType::DoorIronClosed
    )
}

/// `VoxelShape.max(Y)`，空形状不调用。
pub(crate) fn max_y(shape: &[Aabb]) -> f64 {
    if shape.is_empty() {
        return 0.0;
    }
    shape
        .iter()
        .map(|bounds| bounds.max_y)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// `WalkNodeEvaluator.getPathTypeFromState`。表里没有的是形状依赖方块实体或实体的
/// 那几种方块（竹子、潜影盒、移动活塞、滴水石锥、细雪、脚手架、硫磺刺）；它们走同一条
/// 判断链，链上用到的只有标签、流体和生成期导出的 `isPathfindable(LAND)`，都和形状无关。
///
/// `WalkNodeEvaluator.getPathTypeFromState`. States missing from the table (dynamic
/// shapes) take the same chain, which only needs tags, the fluid and the exported
/// `isPathfindable(LAND)`.
pub fn state_path_type(state: BlockState) -> PathType {
    if let Some(found) = mc_bamboo_data::path_type_of_state(state.0) {
        return found;
    }
    if state.is_air() {
        return PathType::Open;
    }
    if state.is(blocks::POWDER_SNOW) {
        return PathType::PowderSnow;
    }
    if state.is_tagged(mc_bamboo_data::tags::block_tags::SPELEOTHEMS) {
        return PathType::DamageCautious;
    }
    let fluid = state.fluid();
    if fluid.is_tagged(fluid_tags::LAVA) {
        return PathType::Lava;
    }
    if !state.is_pathfindable_land() {
        return PathType::Blocked;
    }
    if fluid.is_tagged(fluid_tags::WATER) {
        return PathType::Water;
    }
    PathType::Open
}

/// `BlockBehaviour.isPathfindable(state, LAND)`，生成期从官方逐状态导出。
pub fn is_pathfindable_land(state: BlockState) -> bool {
    state.is_pathfindable_land()
}

/// `CollisionGetter.noCollision(entity, box)` 的反面，没有实体碰撞体。
pub(crate) fn has_block_collision(
    view: &dyn BlockView,
    bounds: Aabb,
    context: CollisionContext,
    state_at: impl Fn(BlockPos) -> BlockState,
) -> bool {
    let collisions = crate::world::collect_block_collisions(
        bounds,
        context,
        crate::world::CollisionBlocks {
            block_at: |pos| state_at(pos),
            piston_at: |pos| view.moving_piston_shape(pos),
            shulker_at: |pos| view.shulker_box_shape(pos),
        },
    );
    if !collisions.is_empty() {
        return true;
    }
    let centre_x = (bounds.min_x + bounds.max_x) / 2.0;
    let centre_z = (bounds.min_z + bounds.max_z) / 2.0;
    !view
        .world_border()
        .no_border_collision(centre_x, centre_z, bounds)
}
