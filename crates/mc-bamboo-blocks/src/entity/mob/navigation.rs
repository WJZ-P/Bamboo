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

use super::{
    path::{EvaluatorFlags, Path, PathMob, PathRequest, find_path, max_y},
    pathfinding::{self, Action, NavRequest, Replay},
};
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
    /// 两栖求值器已经跑过一次：生物的 `PathType.WATER` 代价从此是 0。
    ///
    /// The amphibious evaluator has run once: the mob's `PathType.WATER` malus is 0
    /// from then on.
    pub cleared_water_malus: bool,
    pub target_pos: Option<BlockPos>,
    pub reach_range: i32,
    pub max_visited_nodes_multiplier: f32,
    /// `PathFinder.maxVisitedNodes`。
    pub max_visited_nodes: i32,
    pub is_stuck: bool,
    pub required_path_length: f32,
    /// `WallClimberNavigation`：路径走完后直接朝 `path_to_position` 推三维坐标。
    ///
    /// `WallClimberNavigation`: after the path ends, push straight at `path_to_position`.
    pub wall_climber: bool,
    /// `WallClimberNavigation.pathToPosition`。
    pub path_to_position: Option<BlockPos>,
    /// 独立寻路阶段（开关打开时）的挂起请求 / 下一刻结果 / 待办动作。
    /// 三样都只在开关打开时读写，缺省精确模式下恒为空，不存档、不同步。
    pub pending_request: Option<NavRequest>,
    pub replay: Option<Replay>,
    pub next_action: Option<Action>,
    /// 每只生物自己的请求序号；和 uid 一起组成全岛唯一序号。
    pub request_serial: u32,
    /// `stop()` 作废到哪个序号为止：小于等于它的结果到达时直接丢掉。
    ///
    /// Results at or below this serial are dropped: the request was cancelled by
    /// [`Navigation::stop`].
    pub stop_serial: u32,
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
            cleared_water_malus: false,
            target_pos: None,
            reach_range: 0,
            max_visited_nodes_multiplier: 1.0,
            max_visited_nodes: mth::floor(follow_range_base * 16.0),
            is_stuck: false,
            required_path_length: 16.0,
            wall_climber: false,
            path_to_position: None,
            pending_request: None,
            replay: None,
            next_action: None,
            request_serial: 0,
            stop_serial: 0,
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
        // 挂起的请求和还没取走的结果一起作废；结果到达时按序号丢掉。
        self.pending_request = None;
        self.replay = None;
        self.next_action = None;
        self.stop_serial = self.request_serial;
    }

    pub fn can_float(&self) -> bool {
        self.flags.can_float
    }

    /// `WaterBoundPathNavigation` 的 `allowBreaching`；地面导航是 `None`。
    ///
    /// `WaterBoundPathNavigation.allowBreaching`; `None` for ground navigation.
    const fn water(&self) -> Option<bool> {
        self.flags.swim
    }

    /// `GroundPathNavigation.canUpdatePath` / `WaterBoundPathNavigation.canUpdatePath`。
    fn can_update_path(&self, mob: &NavMob) -> bool {
        if self.flags.amphibious {
            // `AmphibiousPathNavigation.canUpdatePath` 恒真。
            return true;
        }
        if self.flags.fly {
            // `FlyingPathNavigation.canUpdatePath`：会浮水且在水里，或者不是乘客。
            // Bamboo 的生物不会骑生物，所以后一半恒真。
            // `FlyingPathNavigation.canUpdatePath`: floating in liquid, or not a passenger.
            // Bamboo mobs never ride mobs, so the second half is always true.
            return (self.can_float() && mob.in_liquid) || true;
        }
        match self.water() {
            Some(allow_breaching) => allow_breaching || mob.in_liquid,
            None => mob.path.on_ground || mob.in_liquid,
        }
    }

    /// `moveTo(x, y, z, speed)`。
    pub fn move_to(&mut self, view: &dyn BlockView, mob: &NavMob, at: Vec3, speed: f64) -> bool {
        // 开关打开时，这条动作随请求一起挂起；下一刻结果到了才真正开始走。
        if pathfinding::Config::of(&view.block_game_rules()).enabled {
            self.next_action = Some(Action::MoveTo { speed });
        }
        let path = self.create_path(view, mob, BlockPos::containing(at), 1);
        if path.is_some() {
            self.next_action = None;
        }
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
        // 这是三参数的 `moveTo(entity, reach, speed)`：`WallClimberNavigation` 只改写了
        // 两参数版本（找不到路径也返回真），三参数版本走基类，空路径就返回假、不改速度。
        // 蜘蛛的目标（`MeleeAttackGoal.tick`）只调三参数版本；`createPath` 照样记下目标方块。
        // This is the three-argument `moveTo(entity, reach, speed)`. `WallClimberNavigation`
        // only overrides the two-argument form (which succeeds without a path); the
        // three-argument form is the base one: a null path returns false and keeps the
        // speed. Spider goals (`MeleeAttackGoal.tick`) only call this form, and
        // `createPath` still records the destination block.
        if pathfinding::Config::of(&view.block_game_rules()).enabled {
            self.next_action = Some(Action::MoveToEntity { speed });
        }
        let path = self.create_path(view, mob, BlockPos::containing(entity_pos), reach);
        if path.is_some() {
            self.next_action = None;
        }
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
            // 独立寻路阶段：请求还挂着，沿旧路走或原地等，别把旧路径清掉。
            if self.pending_request.is_some() {
                return false;
            }
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
        if self.wall_climber {
            // `WallClimberNavigation.createPath`：先记下目标方块。
            // `WallClimberNavigation.createPath` records the destination first.
            self.path_to_position = Some(pos);
        }
        // `WaterBoundPathNavigation` 不覆盖 `createPath`：不查加载，也不找地面。
        // `AmphibiousPathNavigation` 同样用基类实现。飞行导航也一样。
        // `WaterBoundPathNavigation` keeps the base `createPath`: no loaded check and no
        // surface search. `AmphibiousPathNavigation` and `FlyingPathNavigation` do the same.
        if self.water().is_some() || self.flags.amphibious || self.flags.fly {
            let max_path_length = self.max_path_length(mob.follow_range);
            return self.create_path_to(view, mob, pos, reach, max_path_length);
        }
        if !view.is_loaded(pos) {
            return None;
        }
        let pos = find_surface_position(view, pos);
        let max_path_length = self.max_path_length(mob.follow_range);
        self.create_path_to(view, mob, pos, reach, max_path_length)
    }

    /// `PathNavigation.createPath(BlockPos, reachRange, maxPathLength)`：`GroundPathNavigation`
    /// 没有覆写这一个，所以不找地表，路径长度上限由调用方给（长跳用 8）。
    ///
    /// `PathNavigation.createPath(BlockPos, reachRange, maxPathLength)`, which
    /// `GroundPathNavigation` does not override: no surface search, and the caller gives
    /// the path length cap (8 for long jumps).
    pub fn create_path_within(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        pos: BlockPos,
        reach: i32,
        max_path_length: f32,
    ) -> Option<Path> {
        self.create_path_to(view, mob, pos, reach, max_path_length)
    }

    /// `PathNavigation.createPath(Set, regionOffset, offsetUpward, reach, maxPathLength)`。
    fn create_path_to(
        &mut self,
        view: &dyn BlockView,
        mob: &NavMob,
        pos: BlockPos,
        reach: i32,
        max_path_length: f32,
    ) -> Option<Path> {
        if mob.path.position.y < f64::from(view.min_y()) {
            return None;
        }
        if !self.can_update_path(mob) {
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
            max_distance_from_start: max_path_length,
            reach_range: reach,
            max_visited_nodes: self.max_visited_nodes,
            max_visited_nodes_multiplier: self.max_visited_nodes_multiplier,
        };
        // `AmphibiousNodeEvaluator.prepare` 把生物的水代价设成 0，`done` 只还原另外两种，
        // 所以第一次寻路之后水代价就一直是 0（见 [`Mob::pathfinding_malus`]）。
        // `AmphibiousNodeEvaluator.prepare` zeroes the mob's water malus and `done` only
        // restores the other two, so it stays 0 after the first search (see
        // [`Mob::pathfinding_malus`]).
        if self.flags.amphibious {
            self.cleared_water_malus = true;
        }
        // 独立寻路阶段打开时，把搜索换成一条挂起请求；下一刻结果到了再由
        // [`pathfinding::install`] 或下一次 `create_path` 取走。
        if pathfinding::Config::of(&view.block_game_rules()).enabled {
            return self.deferred_path(request, mob);
        }
        let path = find_path(view, &request);
        if let Some(path) = &path {
            self.target_pos = Some(path.target);
            self.reach_range = reach;
            self.reset_stuck_timeout();
        }
        path
    }

    /// 独立寻路阶段：把请求挂起，或者取走上一刻已经算好的结果。
    ///
    /// 结果的目标点和这次的请求一致才交付；目标换了就丢掉旧结果、重新挂请求。
    /// 返回 `None` 表示「这一刻还没有路径」，调用点按官方「没有路径」处理。
    fn deferred_path(&mut self, request: PathRequest, mob: &NavMob) -> Option<Path> {
        let at = request.target;
        if let Some(replay) = self.replay.take() {
            if !replay.consumed && replay.at == at {
                if replay.path.is_some() {
                    self.target_pos = Some(replay.at);
                    self.reach_range = request.reach_range;
                    self.reset_stuck_timeout();
                }
                return replay.path;
            }
            // 目标换了，旧结果作废，落到下面重新挂请求。
        }
        if self
            .pending_request
            .as_ref()
            .is_some_and(|pending| pending.at == at)
        {
            return None;
        }
        self.request_serial = self.request_serial.wrapping_add(1);
        let action = self
            .next_action
            .take()
            .unwrap_or(Action::Inspect(pathfinding::Use::Ignore));
        self.pending_request = Some(NavRequest {
            request,
            nav: *mob,
            action,
            at,
            local_serial: self.request_serial,
        });
        None
    }

    /// 这一刻是不是有一条请求还挂在阶段里（结果还没到）。
    /// 调用点用它在「等待」时跳过依赖结果的后续语句。
    pub fn awaiting(&self) -> bool {
        self.pending_request.is_some()
    }

    /// 已经交付、还没被取走的结果的目标点。
    /// 自己挑目标点的调用点用它在下一刻复用原来的目标，不重抽随机数。
    pub fn replay_target(&self) -> Option<BlockPos> {
        self.replay
            .as_ref()
            .filter(|replay| !replay.consumed)
            .map(|replay| replay.at)
    }

    /// 有没有已经交付、还没消费的结果。
    pub fn has_replay(&self) -> bool {
        self.replay.as_ref().is_some_and(|replay| !replay.consumed)
    }

    /// 已经交付、还没消费的结果对应的动作。
    pub fn replay_action(&self) -> Option<Action> {
        self.replay
            .as_ref()
            .filter(|replay| !replay.consumed)
            .map(|replay| replay.action)
    }

    /// 显式登记「结果到了以后做什么」。`move_to*` 自己登记，其余检查结果的调用点用这个。
    pub fn set_next_action(&mut self, action: Action) {
        self.next_action = Some(action);
    }

    pub(crate) fn reset_stuck_timeout(&mut self) {
        self.timeout_cached_node = BlockPos::new(0, 0, 0);
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
        self.is_stuck = false;
    }

    /// `PathNavigation.tick`。返回要交给 `MoveControl.setWantedPosition` 的位置和速度。
    pub fn tick(&mut self, view: &dyn BlockView, mob: &NavMob) -> Option<(Vec3, f64)> {
        if self.wall_climber && self.is_done() {
            // `WallClimberNavigation.tick` 走完路径的那一支。
            // The finished-path branch of `WallClimberNavigation.tick`.
            return self.wall_climb_target(mob);
        }
        self.tick += 1;
        if self.has_delayed_recomputation {
            self.recompute_path(view, mob);
        }
        if self.is_done() {
            return None;
        }
        let width = mob.path.width;
        if self.can_update_path(mob) {
            self.follow_the_path(view, mob);
        } else if let Some(path) = &mut self.path
            && !path.is_done()
        {
            let temp = temp_mob_pos(view, mob, self.flags);
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
        // `WaterBoundPathNavigation.getGroundY` 原样返回 `target.y`。
        // `WaterBoundPathNavigation.getGroundY` returns `target.y` unchanged; so does
        // `FlyingPathNavigation`, which never calls `getGroundY` at all.
        let y = if self.water().is_some() || self.flags.amphibious || self.flags.fly {
            next.y
        } else {
            ground_y(view, next)
        };
        Some((Vec3::new(next.x, y, next.z), self.speed_modifier))
    }

    fn temp_mob_pos(&self, view: &dyn BlockView, mob: &NavMob) -> Vec3 {
        temp_mob_pos(view, mob, self.flags)
    }

    /// `WallClimberNavigation.tick` 的 `else` 分支：路径走完后朝目标方块推三维坐标。
    /// 目标已经在 `width` 以内（或目标在上方且同一列已经够近）时清掉目标。
    ///
    /// The `else` branch of `WallClimberNavigation.tick`: push at the destination in
    /// 3D once the path is done, and clear it when the mob is close enough.
    fn wall_climb_target(&mut self, mob: &NavMob) -> Option<(Vec3, f64)> {
        let destination = self.path_to_position?;
        let width = f64::from(mob.path.width);
        let position = mob.path.position;
        let inside = |x: i32, y: i32, z: i32| {
            let dx = f64::from(x) + 0.5 - position.x;
            let dy = f64::from(y) + 0.5 - position.y;
            let dz = f64::from(z) + 0.5 - position.z;
            dx * dx + dy * dy + dz * dz < width * width
        };
        let at_destination = inside(destination.x, destination.y, destination.z);
        let same_column = inside(destination.x, mth::floor(position.y), destination.z);
        if at_destination || (position.y > f64::from(destination.y) && same_column) {
            self.path_to_position = None;
            return None;
        }
        Some((
            Vec3::new(
                f64::from(destination.x),
                f64::from(destination.y),
                f64::from(destination.z),
            ),
            self.speed_modifier,
        ))
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
        // `getMaxVerticalDistanceToWaypoint`：地面 1，水里 0.5。
        // `getMaxVerticalDistanceToWaypoint`: 1 on the ground, 0.5 in water.
        let vertical = if self.flags.swim.is_some() { 0.5 } else { 1.0 };
        let close = dx < limit && dz < limit && dy < vertical;
        // `FlyingPathNavigation.canMoveDirectly` 也走这条，但把流体也算作阻挡；
        // [`clear_for_movement`] 只挡源方块水，见已知差异。
        // `FlyingPathNavigation.canMoveDirectly` also takes this path but counts fluids
        // as blockers; [`clear_for_movement`] only sees source water (a known difference).
        let direct =
            self.flags.swim.is_some() || self.flags.fly || (self.flags.amphibious && mob.in_liquid);
        if close
            || (can_cut_corner(path.next_node().path_type)
                // `Frog.FrogPathNavigation.canCutCorner`：水边不抄近路。
                // `Frog.FrogPathNavigation.canCutCorner`: no shortcuts at the water border.
                && !(self.flags.frog && path.next_node().path_type == PathType::WaterBorder)
                && should_target_next_node_in_direction(view, mob, path, temp, direct))
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
        if mob.game_time - self.time_last_recompute > 20 && self.can_update_path(mob) {
            if let Some(target) = self.target_pos {
                self.path = None;
                if pathfinding::Config::of(&view.block_game_rules()).enabled {
                    self.next_action = Some(Action::Recompute);
                }
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

    /// `isStableDestination` 的虚分派：`WaterBoundPathNavigation` 只要求这一格不是实心渲染。
    ///
    /// The virtual `isStableDestination`: `WaterBoundPathNavigation` only wants the cell
    /// itself not to be solid-render.
    pub fn stable_destination(&self, view: &dyn BlockView, pos: BlockPos) -> bool {
        if self.flags.fly {
            // `FlyingPathNavigation.isStableDestination`：`entityCanStandOn`，这里用实心
            // 碰撞形状近似。飞行的闲逛目标不走它。
            // `FlyingPathNavigation.isStableDestination`: `entityCanStandOn`, approximated
            // here with a solid collision shape. The flying wander goal does not use it.
            return view.block_state(pos).is_solid();
        }
        if self.flags.amphibious {
            // `AmphibiousPathNavigation.isStableDestination`：下面不是空气。
            return !view.block_state(pos.below()).is_air();
        }
        if self.water().is_some() {
            !view.block_state(pos).is_solid_render()
        } else {
            Self::is_stable_destination(view, pos)
        }
    }
}

/// `GroundPathNavigation.getTempMobPos`；`WaterBoundPathNavigation` 取 `getY(0.5)`。
fn temp_mob_pos(view: &dyn BlockView, mob: &NavMob, flags: EvaluatorFlags) -> Vec3 {
    let position = mob.path.position;
    if flags.fly {
        // `FlyingPathNavigation.getTempMobPos`：直接用实体坐标。
        // `FlyingPathNavigation.getTempMobPos`: the entity position as-is.
        return position;
    }
    if flags.swim.is_some() || flags.amphibious {
        // `AmphibiousPathNavigation.getTempMobPos` 和 `WaterBoundPathNavigation` 一样取
        // `getY(0.5)`。
        let y = position.y + f64::from(mob.path.height) * 0.5;
        return Vec3::new(position.x, y, position.z);
    }
    let can_float = flags.can_float;
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

/// `PathNavigation.isClearForMovementBetween(mob, start, stop, false)`：碰撞体射线不被挡。
///
/// `PathNavigation.isClearForMovementBetween(mob, start, stop, false)`: a collider ray
/// that hits nothing.
fn clear_for_movement(view: &dyn BlockView, mob: &NavMob, start: Vec3, stop: Vec3) -> bool {
    let to = Vec3::new(stop.x, stop.y + f64::from(mob.path.height) * 0.5, stop.z);
    view.clip_collider(start, to, mob.path.context).is_none()
}

/// `shouldTargetNextNodeInDirection`。`direct` 是这种导航有没有 `canMoveDirectly`
/// （地面导航恒假，水中导航用 [`clear_for_movement`]）。
///
/// `shouldTargetNextNodeInDirection`. `direct` says whether this navigation has a
/// `canMoveDirectly` (always false on the ground, [`clear_for_movement`] in water).
fn should_target_next_node_in_direction(
    view: &dyn BlockView,
    mob: &NavMob,
    path: &Path,
    temp: Vec3,
    direct: bool,
) -> bool {
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
    if direct && clear_for_movement(view, mob, temp, path.next_entity_pos(mob.path.width)) {
        return true;
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
