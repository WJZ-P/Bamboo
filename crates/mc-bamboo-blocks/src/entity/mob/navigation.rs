//! 导航：官方 `PathNavigation` 加 `GroundPathNavigation`。
//!
//! 跟着路径走、卡住检测、超时、重算都在这里；真正的搜索在 [`super::path`]。
//! 精确模式下 `create_path` 当场同步算出路径，算的入口只有 [`find_path`] 一处，
//! 以后挪到独立的寻路阶段时从这里拆开。
//!
//! Navigation: vanilla `PathNavigation` plus `GroundPathNavigation`. The search itself
//! lives in [`super::path`]; [`find_path`] is the single seam for a later pathfinding phase.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{PathType, tags::block_tags, tags::fluid_tags};

use super::path::{EvaluatorFlags, Path, PathMob, PathRequest, find_path, max_y};
use crate::{entity::ctx::BlockView, shape::CollisionContext};

/// 导航要看的生物状态，每次调用前从生物身上拍一份。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavMob {
    pub path: PathMob,
    /// `LivingEntity.getSpeed()`。
    pub speed: f32,
    /// `isInLiquid()`。
    pub in_liquid: bool,
    /// `getAttributeValue(FOLLOW_RANGE)`。
    pub follow_range: f64,
    pub game_time: i64,
}

/// `PathNavigation` 的状态。
#[derive(Debug, Clone, PartialEq)]
pub struct Navigation {
    pub path: Option<Path>,
    pub avoid_sun: bool,
    pub speed_modifier: f64,
    pub tick: i32,
    pub last_stuck_check: i32,
    pub last_stuck_check_pos: Vec3,
    pub timeout_cached_node: BlockPos,
    pub timeout_timer: i64,
    /// 绝对时刻（岛的时钟），合并时要平移。
    pub last_timeout_check: i64,
    pub timeout_limit: f64,
    pub max_distance_to_waypoint: f32,
    pub has_delayed_recomputation: bool,
    /// 绝对时刻（岛的时钟），合并时要平移。
    pub time_last_recompute: i64,
    pub flags: EvaluatorFlags,
    pub target_pos: Option<BlockPos>,
    pub reach_range: i32,
    pub max_visited_nodes_multiplier: f32,
    /// `PathFinder.maxVisitedNodes`。
    pub max_visited_nodes: i32,
    pub is_stuck: bool,
    pub required_path_length: f32,
}

impl Navigation {
    /// 构造函数：节点上限是 `floor(FOLLOW_RANGE 基础值 * 16)`。
    pub fn new(follow_range_base: f64) -> Self {
        Self {
            path: None,
            avoid_sun: false,
            speed_modifier: 0.0,
            tick: 0,
            last_stuck_check: 0,
            last_stuck_check_pos: Vec3::default(),
            timeout_cached_node: BlockPos::new(0, 0, 0),
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            max_distance_to_waypoint: 0.5,
            has_delayed_recomputation: false,
            time_last_recompute: 0,
            flags: EvaluatorFlags::default(),
            target_pos: None,
            reach_range: 0,
            max_visited_nodes_multiplier: 1.0,
            max_visited_nodes: mth::floor(follow_range_base * 16.0),
            is_stuck: false,
            required_path_length: 16.0,
        }
    }

    /// 岛合并时把绝对时刻平移 `delta`。
    pub fn rebase(&mut self, delta: i64) {
        self.last_timeout_check = self.last_timeout_check.saturating_add(delta);
        self.time_last_recompute = self.time_last_recompute.saturating_add(delta);
    }

    /// `updatePathfinderMaxVisitedNodes`：跟随距离变了以后调。
    pub fn update_max_visited_nodes(&mut self, follow_range: f64) {
        self.max_visited_nodes = mth::floor_f32(self.max_path_length(follow_range) * 16.0);
    }

    fn max_path_length(&self, follow_range: f64) -> f32 {
        mth::java_max_f32(follow_range as f32, self.required_path_length)
    }

    pub fn is_done(&self) -> bool {
        self.path.as_ref().is_none_or(Path::is_done)
    }

    pub fn stop(&mut self) {
        self.path = None;
    }

    pub fn can_float(&self) -> bool {
        self.flags.can_float
    }

    /// `GroundPathNavigation.canUpdatePath`。
    fn can_update_path(mob: &NavMob) -> bool {
        mob.path.on_ground || mob.in_liquid
    }

    /// `moveTo(x, y, z, speed)`。
    pub fn move_to(&mut self, view: &dyn BlockView, mob: &NavMob, at: Vec3, speed: f64) -> bool {
        let path = self.create_path(view, mob, BlockPos::containing(at), 1);
        self.move_to_path(view, mob, path, speed)
    }

    /// `moveTo(entity, speed)`：`GroundPathNavigation` 也走地面搜索，终点是实体的方块坐标。
    /// `reach` 由调用方按官方 `createPath(entity, reach)` 给；近战目标是 0。
    pub fn move_to_entity(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        entity_pos: Vec3,
        speed: f64,
        reach: i32,
    ) -> bool {
        let path = self.create_path(view, mob, BlockPos::containing(entity_pos), reach);
        path.is_some() && self.move_to_path(view, mob, path, speed)
    }

    /// `moveTo(Path, speed)`。
    pub fn move_to_path(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        path: Option<Path>,
        speed: f64,
    ) -> bool {
        let Some(path) = path else {
            self.path = None;
            return false;
        };
        if !path.same_as(self.path.as_ref()) {
            self.path = Some(path);
        }
        if self.is_done() {
            return false;
        }
        self.trim_path(view, mob.path.position);
        if self.path.as_ref().is_none_or(|path| path.node_count() == 0) {
            return false;
        }
        self.speed_modifier = speed;
        let temp = self.temp_mob_pos(view, mob);
        self.last_stuck_check = self.tick;
        self.last_stuck_check_pos = temp;
        true
    }

    /// `GroundPathNavigation.createPath(BlockPos, int)`。
    pub fn create_path(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        pos: BlockPos,
        reach: i32,
    ) -> Option<Path> {
        if !view.is_loaded(pos) {
            return None;
        }
        let pos = find_surface_position(view, pos);
        self.create_path_to(view, mob, pos, reach)
    }

    /// `PathNavigation.createPath(Set, regionOffset, offsetUpward, reach, maxPathLength)`。
    fn create_path_to(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        pos: BlockPos,
        reach: i32,
    ) -> Option<Path> {
        if mob.path.position.y < f64::from(view.min_y()) {
            return None;
        }
        if !Self::can_update_path(mob) {
            return None;
        }
        if let Some(path) = &self.path
            && !path.is_done()
            && self.target_pos == Some(pos)
        {
            return Some(path.clone());
        }
        let request = PathRequest {
            mob: mob.path,
            flags: self.flags,
            target: pos,
            max_distance_from_start: self.max_path_length(mob.follow_range),
            reach_range: reach,
            max_visited_nodes: self.max_visited_nodes,
            max_visited_nodes_multiplier: self.max_visited_nodes_multiplier,
        };
        let path = find_path(view, &request);
        if let Some(path) = &path {
            self.target_pos = Some(path.target);
            self.reach_range = reach;
            self.reset_stuck_timeout();
        }
        path
    }

    fn reset_stuck_timeout(&mut self) {
        self.timeout_cached_node = BlockPos::new(0, 0, 0);
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
        self.is_stuck = false;
    }

    /// `PathNavigation.tick`。返回要交给 `MoveControl.setWantedPosition` 的位置和速度。
    pub fn tick(&mut self, view: &dyn BlockView, mob: &NavMob) -> Option<(Vec3, f64)> {
        self.tick += 1;
        if self.has_delayed_recomputation {
            self.recompute_path(view, mob);
        }
        if self.is_done() {
            return None;
        }
        let width = mob.path.width;
        if Self::can_update_path(mob) {
            self.follow_the_path(view, mob);
        } else if let Some(path) = &mut self.path
            && !path.is_done()
        {
            let temp = temp_mob_pos(view, mob, self.flags.can_float);
            let next = path.next_entity_pos(width);
            if temp.y > next.y
                && !mob.path.on_ground
                && mth::floor(temp.x) == mth::floor(next.x)
                && mth::floor(temp.z) == mth::floor(next.z)
            {
                path.advance();
            }
        }
        if self.is_done() {
            return None;
        }
        let next = self.path.as_ref()?.next_entity_pos(width);
        Some((
            Vec3::new(next.x, ground_y(view, next), next.z),
            self.speed_modifier,
        ))
    }

    fn temp_mob_pos(&self, view: &dyn BlockView, mob: &NavMob) -> Vec3 {
        temp_mob_pos(view, mob, self.flags.can_float)
    }

    fn follow_the_path(&mut self, view: &dyn BlockView, mob: &NavMob) {
        let temp = self.temp_mob_pos(view, mob);
        let width = mob.path.width;
        self.max_distance_to_waypoint = if width > 0.75 {
            width / 2.0
        } else {
            0.75 - width / 2.0
        };
        let Some(path) = &mut self.path else {
            return;
        };
        let next = path.next_node_pos();
        let position = mob.path.position;
        let dx = (position.x - (f64::from(next.x) + 0.5)).abs();
        let dy = (position.y - f64::from(next.y)).abs();
        let dz = (position.z - (f64::from(next.z) + 0.5)).abs();
        let limit = f64::from(self.max_distance_to_waypoint);
        let close = dx < limit && dz < limit && dy < 1.0;
        if close
            || (can_cut_corner(path.next_node().path_type)
                && should_target_next_node_in_direction(path, temp))
        {
            path.advance();
        }
        self.do_stuck_detection(mob, temp);
    }

    fn do_stuck_detection(&mut self, mob: &NavMob, temp: Vec3) {
        if self.tick - self.last_stuck_check > 100 {
            let speed = if mob.speed >= 1.0 {
                mob.speed
            } else {
                mob.speed * mob.speed
            };
            let threshold = speed * 100.0 * 0.25;
            if temp.distance_to_sqr(self.last_stuck_check_pos) < f64::from(threshold * threshold) {
                self.is_stuck = true;
                self.stop();
            } else {
                self.is_stuck = false;
            }
            self.last_stuck_check = self.tick;
            self.last_stuck_check_pos = temp;
        }
        if let Some(path) = &self.path
            && !path.is_done()
        {
            let next = path.next_node_pos();
            let now = mob.game_time;
            if next == self.timeout_cached_node {
                self.timeout_timer += now - self.last_timeout_check;
            } else {
                self.timeout_cached_node = next;
                let centre = Vec3::new(
                    f64::from(next.x) + 0.5,
                    f64::from(next.y),
                    f64::from(next.z) + 0.5,
                );
                let distance = temp.distance_to_sqr(centre).sqrt();
                self.timeout_limit = if mob.speed > 0.0 {
                    distance / f64::from(mob.speed) * 20.0
                } else {
                    0.0
                };
            }
            if self.timeout_limit > 0.0 && self.timeout_timer as f64 > self.timeout_limit * 3.0 {
                self.reset_stuck_timeout();
                self.stop();
            }
            self.last_timeout_check = now;
        }
    }

    /// `recomputePath`：方块变了以后的延迟重算。
    pub fn recompute_path(&mut self, view: &dyn BlockView, mob: &NavMob) {
        if mob.game_time - self.time_last_recompute > 20 && Self::can_update_path(mob) {
            if let Some(target) = self.target_pos {
                self.path = None;
                self.path = self.create_path(view, mob, target, self.reach_range);
                self.time_last_recompute = mob.game_time;
                self.has_delayed_recomputation = false;
            }
        } else {
            self.has_delayed_recomputation = true;
        }
    }

    /// `PathNavigation.trimPath`：炼药锅格子上抬一格。
    fn trim_path(&mut self, view: &dyn BlockView, feet: Vec3) {
        let Some(path) = &mut self.path else {
            return;
        };
        for index in 0..path.node_count() {
            let node = path.nodes[index];
            let next = path.nodes.get(index + 1).copied();
            if view
                .block_state(node.pos())
                .is_tagged(block_tags::CAULDRONS)
            {
                let mut moved = node;
                moved.y += 1;
                path.nodes[index] = moved;
                if let Some(next) = next
                    && node.y >= next.y
                {
                    let mut lifted = node;
                    lifted.x = next.x;
                    lifted.y = node.y + 1;
                    lifted.z = next.z;
                    path.nodes[index + 1] = lifted;
                }
            }
        }
        if self.avoid_sun
            && !view.can_see_sky(BlockPos::containing(feet.add(Vec3::new(0.0, 0.5, 0.0))))
            && let Some(first_sunny) = path
                .nodes
                .iter()
                .position(|node| view.can_see_sky(node.pos()))
        {
            path.nodes.truncate(first_sunny);
        }
    }

    /// `isStableDestination`：下面一格是实心渲染的。
    pub fn is_stable_destination(view: &dyn BlockView, pos: BlockPos) -> bool {
        view.block_state(pos.below()).is_solid_render()
    }
}

/// `GroundPathNavigation.getTempMobPos`。
fn temp_mob_pos(view: &dyn BlockView, mob: &NavMob, can_float: bool) -> Vec3 {
    let position = mob.path.position;
    Vec3::new(
        position.x,
        f64::from(surface_y(view, mob, can_float)),
        position.z,
    )
}

fn surface_y(view: &dyn BlockView, mob: &NavMob, can_float: bool) -> i32 {
    let position = mob.path.position;
    if mob.path.in_floatable_fluid && can_float {
        let block_y = mth::floor(position.y);
        let mut y = block_y;
        let at = |y: i32| BlockPos::new(mth::floor(position.x), y, mth::floor(position.z));
        let mut state = view.block_state(at(y));
        let mut count = 0;
        while state.fluid().is_tagged(fluid_tags::ENTITY_FLOATABLE) {
            y += 1;
            state = view.block_state(at(y));
            count += 1;
            if count > 16 {
                return block_y;
            }
        }
        return y;
    }
    mth::floor(position.y + 0.5)
}

/// `getGroundY`：下面是空气时原样，否则落到下面那格碰撞形状的顶上。
fn ground_y(view: &dyn BlockView, at: Vec3) -> f64 {
    let pos = BlockPos::containing(at);
    if view.block_state(pos.below()).is_air() {
        at.y
    } else {
        static_floor_level(view, pos)
    }
}

/// `WalkNodeEvaluator.getFloorLevel(BlockGetter, BlockPos)`。
pub(crate) fn static_floor_level(view: &dyn BlockView, pos: BlockPos) -> f64 {
    let below = pos.below();
    let state = view.block_state(below);
    let piston = state
        .is(mc_bamboo_data::blocks::MOVING_PISTON)
        .then(|| view.moving_piston_shape(below))
        .flatten();
    let shape = crate::shape::block_collision_boxes(
        state,
        below,
        CollisionContext::Empty,
        piston.as_deref(),
        view.shulker_box_shape(below),
    );
    f64::from(below.y) + max_y(&shape)
}

fn can_cut_corner(path_type: PathType) -> bool {
    path_type != PathType::FireInNeighbor
        && path_type != PathType::DamagingInNeighbor
        && path_type != PathType::WalkableDoor
}

fn should_target_next_node_in_direction(path: &Path, temp: Vec3) -> bool {
    if path.next_node_index + 1 >= path.node_count() {
        return false;
    }
    let bottom_centre = |pos: BlockPos| {
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        )
    };
    let next = bottom_centre(path.next_node_pos());
    if temp.distance_to_sqr(next).partial_cmp(&(2.0 * 2.0)) != Some(std::cmp::Ordering::Less) {
        return false;
    }
    let after = bottom_centre(path.nodes[path.next_node_index + 1].pos());
    let to_next = next.subtract(temp);
    let to_after = after.subtract(temp);
    let next_length = to_next.length_sqr();
    let after_length = to_after.length_sqr();
    let after_closer = after_length < next_length;
    let next_very_close = next_length < 0.5;
    if !after_closer && !next_very_close {
        return false;
    }
    let a = to_after.normalize();
    let b = to_next.normalize();
    a.x * b.x + a.y * b.y + a.z * b.z < 0.0
}

/// `GroundPathNavigation.findSurfacePosition`。
fn find_surface_position(view: &dyn BlockView, pos: BlockPos) -> BlockPos {
    let min_y = view.min_y();
    let max_y = view.max_y();
    let mut pos = pos;
    if view.block_state(pos).is_air() {
        let mut cursor = pos.below();
        while cursor.y >= min_y && view.block_state(cursor).is_air() {
            cursor = cursor.below();
        }
        if cursor.y >= min_y {
            return cursor.above();
        }
        cursor = BlockPos::new(pos.x, pos.y + 1, pos.z);
        while cursor.y <= max_y && view.block_state(cursor).is_air() {
            cursor = cursor.above();
        }
        pos = cursor;
    }
    if view.block_state(pos).is_solid() {
        let mut cursor = pos.above();
        while cursor.y <= max_y && view.block_state(cursor).is_solid() {
            cursor = cursor.above();
        }
        return cursor;
    }
    pos
}
