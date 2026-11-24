//! 独立寻路阶段（开关，缺省关）。
//!
//! 见 `docs/mob-entity-design.md` 4.1。开关关着时 [`Navigation`](super::navigation) 当场
//! 同步算出路径，语句顺序和官方一致；打开后算阶段只把请求挂在生物自己的导航上，
//! 阶段末尾统一「去重 → 排优先级 → 按节点预算算」，下一刻开头把结果挂回去，
//! 由统一的结果处理函数开始走 / 交给原调用点继续。
//!
//! 结果只由刻号、uid、请求内容和固定的排序键决定，和线程数、机器快慢无关。
//! 配置不进存档、不同步客户端。

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::{BlockPos, Vec3, mth};

use crate::{
    BlockGameRules,
    entity::{
        EntityUid,
        ctx::{BlockView, PlayerView},
        mob::navigation::NavMob,
    },
};

use super::path::{Path, PathRequest};

/// 缺省的节点预算：chase 场景每刻约 730 个节点，留几倍余量仍能容下突发。
pub const DEFAULT_BUDGET: u32 = 4096;
/// 连续失败超过这么多次就进入冷却（Paper `SpigotWorldConfig` 的 `10`）。
pub const FAILURE_THRESHOLD: u32 = 10;
/// 冷却时长（Paper 的 40 刻）。
pub const COOLDOWN_TICKS: i64 = 40;

/// Bamboo 自己的寻路阶段配置，不是官方游戏规则，不进存档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// 开关。关着时走官方当场同步的路径。
    pub enabled: bool,
    /// 每个岛每刻最多展开的 A* 节点数；0 表示用 [`DEFAULT_BUDGET`]。
    pub budget: u32,
    /// 失败冷却开关。
    pub fail_cooldown: bool,
}

impl Config {
    /// 缺省全关：精确模式。
    pub const OFF: Self = Self {
        enabled: false,
        budget: 0,
        fail_cooldown: false,
    };

    pub const fn of(rules: &BlockGameRules) -> Self {
        Self {
            enabled: rules.mob_pathfinding_phase,
            budget: rules.mob_pathfinding_budget,
            fail_cooldown: rules.mob_pathfinding_fail_cooldown,
        }
    }

    pub const fn nodes_per_tick(self) -> u64 {
        if self.budget == 0 {
            DEFAULT_BUDGET as u64
        } else {
            self.budget as u64
        }
    }
}

/// 一次寻路请求的去重键：起点格、终点、体型、导航类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DedupKey {
    pub start: (i32, i32, i32),
    pub target: BlockPos,
    pub width: u32,
    pub height: u32,
    pub nav: u16,
}

impl DedupKey {
    pub fn of(request: &PathRequest) -> Self {
        let position = request.mob.position;
        // 起点格按 A* 里 `getStart` / `getFloorLevel` 的取整规则近似；同一生物同一刻的
        // 连续请求不会落到两个不同的起点格。
        let start = (
            mth::floor(position.x),
            mth::floor(position.y + 0.5),
            mth::floor(position.z),
        );
        Self {
            start,
            target: request.target,
            width: request.mob.width.to_bits(),
            height: request.mob.height.to_bits(),
            nav: request.flags.nav_id(),
        }
    }
}

/// 结果到了以后要做的后续动作，集中登记所有调用点的续延（4.1 的表和它一一对应）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// 把路径装进导航并按给定速度开始走（`moveTo(Path, speed)` 语义）。
    MoveTo { speed: f64 },
    /// 同 `MoveTo`，但三参数 `moveToEntity` 在空路径时不改旧路径。
    MoveToEntity { speed: f64 },
    /// 只检查结果：结果留在导航里，由原调用点下一刻的 `create_path` 取走。
    Inspect(Use),
    /// `PathNavigation.recomputePath`：结果直接覆盖导航里的路径。
    Recompute,
}

impl Action {
    /// 这条动作的结果要不要在下一刻开头就装进导航、开始走。
    pub const fn installs_path(self) -> bool {
        matches!(self, Self::MoveTo { .. } | Self::MoveToEntity { .. })
    }
}

/// 检查结果要送到哪个目标字段 / 后续动作（4.1 的表逐处登记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// `MeleeAttackGoal.canUse` 的 `start_path`。
    MeleeStart,
    /// `Goal::AvoidEntity` / `PandaAvoidGoal` 的 `path` 字段。
    GoalPath,
    /// 山羊长跳 / 冲撞：只看 `path.reached`。
    BrainReached,
    /// 大脑 `MoveToTargetSink`：写路径记忆并决定是否继续。
    WalkTarget,
    /// 青蛙伸舌头：`dist_to_target < 1.75`。
    FrogTongue,
    /// 蜜蜂找花：`is_some()` 决定选花 / 写不可达名单。
    BeeFlower,
    /// 蜜蜂回巢：路径为空时加黑名单、丢巢。
    BeeHive,
    /// 狐狸扑跳、马驹跟母亲一类：只触发导航记录。
    Ignore,
}

/// 挂在 `Navigation` 上的一条请求；驱动在实体 tick 之后收走它。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavRequest {
    pub request: PathRequest,
    pub nav: NavMob,
    pub action: Action,
    /// 请求里选定的目标点，结果处理时按它继续，不再抽随机数。
    pub at: BlockPos,
    /// 本生物自己的请求序号，和 uid 一起组成全岛唯一的 `serial`。
    pub local_serial: u32,
}

/// 下一刻交给生物的结果，挂在 `Navigation` 上由统一的结果处理函数消费。
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub serial: u64,
    pub local_serial: u32,
    /// 发请求时生物的 `target`（`-1` 表示没有）。
    pub target: EntityUid,
    pub at: BlockPos,
    pub path: Option<Path>,
    pub action: Action,
    pub nav: NavMob,
    /// 结果已经消费过（开始走 / 被 `create_path` 取走）。
    pub consumed: bool,
}

/// 一条进入阶段的请求。
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub uid: EntityUid,
    pub local_serial: u32,
    /// 请求发出的刻号（岛时钟），用于「等得越久优先级越高」。
    pub issued_at: i64,
    /// 有没有攻击目标：优先级高于没有的。
    pub has_attack_target: bool,
    /// 目标换了 / 停了就作废（-1 表示没有攻击目标）。
    pub target: EntityUid,
    pub request: PathRequest,
    pub action: Action,
    pub at: BlockPos,
    pub nav: NavMob,
}

impl Pending {
    /// 从生物身上收来的请求补齐驱动才知道的字段。
    pub fn new(
        uid: EntityUid,
        request: NavRequest,
        issued_at: i64,
        has_attack_target: bool,
        target: EntityUid,
    ) -> Self {
        Self {
            uid,
            local_serial: request.local_serial,
            issued_at,
            has_attack_target,
            target,
            request: request.request,
            action: request.action,
            at: request.at,
            nav: request.nav,
        }
    }
}

/// `(uid, 生物内序号)` 拼成全岛唯一的序号；两种驱动、任何线程数都给出同一个值。
pub const fn serial(uid: EntityUid, local_serial: u32) -> u64 {
    ((uid as u64) << 32) | local_serial as u64
}

/// 一条请求的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub uid: EntityUid,
    pub serial: u64,
    pub local_serial: u32,
    pub target: EntityUid,
    pub path: Option<Path>,
    pub action: Action,
    pub at: BlockPos,
    pub nav: NavMob,
}

impl Outcome {
    pub fn replay(&self) -> Replay {
        Replay {
            serial: self.serial,
            local_serial: self.local_serial,
            target: self.target,
            at: self.at,
            path: self.path.clone(),
            action: self.action,
            nav: self.nav,
            consumed: false,
        }
    }
}

/// 每个岛一份的阶段状态。放在 `EntityStore` 上，拆岛 / 并岛跟着实体表走。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhaseState {
    /// 下一刻开头要交付的结果，按（uid, 序号）升序。
    deliveries: BTreeMap<(EntityUid, u64), Outcome>,
    /// 本刻发出、预算用完顺延的请求；`issued_at` 保留原发出刻号。
    deferred: Vec<Pending>,
    /// 每只生物连续失败次数和冷却截止时刻（绝对刻号，岛时钟）。
    failures: BTreeMap<EntityUid, (u32, i64)>,
    /// 这一阶段最后一次运行的刻号。
    last_tick: i64,
    /// 最近一阶段展开的节点数（压测探针 / 测试断言用）。
    pub last_nodes: u64,
    /// 最近一阶段实际跑 A* 的请求数（去重后）。
    pub last_searches: u64,
}

impl PhaseState {
    /// 下一刻开头交付；调用方按（uid, 序号）顺序处理每条结果。
    pub fn take_deliveries(&mut self) -> Vec<Outcome> {
        std::mem::take(&mut self.deliveries).into_values().collect()
    }

    /// 作废一只生物还挂着的请求（目标停了 / 换了 / 导航 stop）。
    /// 已经算好但还没交付的结果也一起丢掉。
    pub fn cancel(&mut self, uid: EntityUid) {
        self.deferred.retain(|pending| pending.uid != uid);
        self.deliveries.retain(|&(owner, _), _| owner != uid);
        self.failures.remove(&uid);
    }

    /// 岛拆分：把 uid 在 `moved` 里的请求、结果和失败计数搬到新的一份。
    /// 没搬走的留在原状态里。
    pub fn split_by(&mut self, moved: &BTreeSet<EntityUid>) -> Self {
        let mut out = Self {
            last_tick: self.last_tick,
            ..Self::default()
        };
        self.deferred.retain(|pending| {
            if moved.contains(&pending.uid) {
                out.deferred.push(pending.clone());
                false
            } else {
                true
            }
        });
        let keys: Vec<_> = self
            .deliveries
            .keys()
            .filter(|(uid, _)| moved.contains(uid))
            .copied()
            .collect();
        for key in keys {
            if let Some(outcome) = self.deliveries.remove(&key) {
                out.deliveries.insert(key, outcome);
            }
        }
        let uids: Vec<_> = self
            .failures
            .keys()
            .filter(|uid| moved.contains(uid))
            .copied()
            .collect();
        for uid in uids {
            if let Some(entry) = self.failures.remove(&uid) {
                out.failures.insert(uid, entry);
            }
        }
        out
    }

    /// 岛合并时把绝对时刻平移 `delta`。
    pub fn rebase(&mut self, delta: i64) {
        self.last_tick = self.last_tick.saturating_add(delta);
        for pending in &mut self.deferred {
            pending.issued_at = pending.issued_at.saturating_add(delta);
        }
        for (_, until) in self.failures.values_mut() {
            *until = until.saturating_add(delta);
        }
    }

    /// 拆岛后新的一边从空状态开始，只保留刻号；挂起的请求随实体行走，
    /// 由新的岛下一刻重新发。
    pub fn split_full(&self) -> Self {
        Self {
            last_tick: self.last_tick,
            ..Self::default()
        }
    }

    /// 并岛：两个岛的刻号已对齐，保留请求和失败计数里较大的一份。
    pub fn absorb(&mut self, other: Self) {
        self.last_tick = self.last_tick.max(other.last_tick);
        self.deferred.extend(other.deferred);
        for (uid, (count, until)) in other.failures {
            let entry = self.failures.entry(uid).or_insert((0, i64::MIN));
            entry.0 = entry.0.max(count);
            entry.1 = entry.1.max(until);
        }
        for (key, outcome) in other.deliveries {
            self.deliveries.entry(key).or_insert(outcome);
        }
    }

    /// 冷却生效时这一刻不再发请求（调用点按「没有路径」处理）。
    pub fn cooling(&self, uid: EntityUid, now: i64, config: Config) -> bool {
        config.fail_cooldown
            && self
                .failures
                .get(&uid)
                .is_some_and(|&(_, until)| now < until)
    }

    /// 一只生物这一刻能不能发请求：冷却中就不发。
    pub fn may_request(&self, uid: EntityUid, now: i64, config: Config) -> bool {
        !self.cooling(uid, now, config)
    }

    /// 只给单元测试读。
    #[cfg(test)]
    pub(crate) fn deferred_len(&self) -> usize {
        self.deferred.len()
    }

    #[cfg(test)]
    pub(crate) fn failure_count(&self, uid: EntityUid) -> u32 {
        self.failures.get(&uid).map_or(0, |&(count, _)| count)
    }

    /// 收集请求后、跑搜索前的选择：去重、排优先级、按节点预算挑出这一刻要算的组。
    ///
    /// 组间顺序固定：组内最高优先级（等得久、有攻击目标、近玩家、uid 小）先算。
    /// 超预算的整组顺延；等待刻数增加，下一刻优先级升高。
    pub fn begin(
        &mut self,
        now: i64,
        config: Config,
        players: &[PlayerView],
        requests: Vec<Pending>,
    ) -> Vec<Vec<Pending>> {
        self.last_tick = now;
        self.last_nodes = 0;
        self.last_searches = 0;
        if !config.enabled {
            // 关着时这一段根本不会被调用；留这条保险，避免把请求攒到下一刻。
            self.deferred.clear();
            self.deliveries.clear();
            return Vec::new();
        }
        self.deferred.extend(requests);
        // 冷却中的请求直接丢掉，不计算；调用点下一刻自然再发。
        if config.fail_cooldown {
            let cooling: Vec<EntityUid> = self
                .failures
                .iter()
                .filter(|&(_, &(_, until))| now < until)
                .map(|(&uid, _)| uid)
                .collect();
            self.deferred
                .retain(|pending| !cooling.contains(&pending.uid));
        }
        let mut ordered = std::mem::take(&mut self.deferred);
        ordered.sort_by(|a, b| priority(a, now, players).cmp(&priority(b, now, players)));
        // 同一刻、同一只生物只留最后一条：先前的请求已被新目标取代。
        let mut latest: BTreeMap<EntityUid, u64> = BTreeMap::new();
        for pending in &ordered {
            latest.insert(pending.uid, serial(pending.uid, pending.local_serial));
        }
        let mut groups: BTreeMap<DedupKey, Vec<Pending>> = BTreeMap::new();
        for pending in ordered {
            if latest.get(&pending.uid) == Some(&serial(pending.uid, pending.local_serial)) {
                groups
                    .entry(DedupKey::of(&pending.request))
                    .or_default()
                    .push(pending);
            }
        }
        let mut groups: Vec<_> = groups.into_values().collect();
        groups.sort_by(|a, b| {
            let key = |group: &Vec<Pending>| {
                group
                    .iter()
                    .map(|pending| priority(pending, now, players))
                    .min()
                    .expect("组不为空")
            };
            key(a).cmp(&key(b))
        });
        let budget = config.nodes_per_tick();
        let mut used = 0_u64;
        let mut selected = Vec::new();
        for group in groups {
            let cost = group_cost(&group[0].request);
            if used.saturating_add(cost) > budget && used > 0 {
                // 超预算的整组顺延；等得越久下一刻优先级越高。
                self.deferred.extend(group);
                continue;
            }
            // 单条就超过预算时这一刻至少算一条，避免预算配得太小导致活锁。
            used = used.saturating_add(cost).min(budget.max(cost));
            selected.push(group);
        }
        selected
    }

    /// 把搜索的结果装回阶段状态：失败计数、下一刻的交付。
    ///
    /// `groups` 是 [`Self::begin`] 的返回值，`results` 和它一一对应，顺序固定。
    pub fn finish(
        &mut self,
        now: i64,
        config: Config,
        groups: Vec<Vec<Pending>>,
        results: Vec<(Option<Path>, u64)>,
    ) {
        debug_assert_eq!(groups.len(), results.len());
        for (group, (path, nodes)) in groups.into_iter().zip(results) {
            self.last_nodes = self.last_nodes.saturating_add(nodes);
            self.last_searches = self.last_searches.saturating_add(1);
            for pending in &group {
                if config.fail_cooldown {
                    let entry = self.failures.entry(pending.uid).or_insert((0, i64::MIN));
                    if path.is_some() {
                        *entry = (0, i64::MIN);
                    } else {
                        entry.0 = entry.0.saturating_add(1);
                        if entry.0 > FAILURE_THRESHOLD {
                            entry.1 = now.saturating_add(COOLDOWN_TICKS);
                        }
                    }
                }
            }
            for pending in group {
                let key = (pending.uid, serial(pending.uid, pending.local_serial));
                self.deliveries.insert(
                    key,
                    Outcome {
                        uid: pending.uid,
                        serial: serial(pending.uid, pending.local_serial),
                        local_serial: pending.local_serial,
                        target: pending.target,
                        path: path.clone(),
                        at: pending.at,
                        nav: pending.nav,
                        action: pending.action,
                    },
                );
            }
        }
    }

    /// 跑一刻的阶段（串行搜索版）：去重 → 排优先级 → 按节点预算选请求 →
    /// 「去重」后的每个组只调一次 `search` → 排下一刻的交付。
    ///
    /// `search` 是纯函数：同一个只读视图、同一条 `PathRequest` 给同样的
    /// `Option<Path>` 和展开节点数。预算按**节点数**不按时间。
    pub fn run(
        &mut self,
        now: i64,
        config: Config,
        players: &[PlayerView],
        requests: Vec<Pending>,
        search: &dyn Fn(&PathRequest) -> (Option<Path>, u64),
    ) {
        let groups = self.begin(now, config, players, requests);
        let results = groups
            .iter()
            .map(|group| search(&group[0].request))
            .collect();
        self.finish(now, config, groups, results);
    }
}

/// 一条请求进预算时要占的节点数（上界）。
fn group_cost(request: &PathRequest) -> u64 {
    let cost = request.max_visited_nodes as f32 * request.max_visited_nodes_multiplier;
    if cost.is_finite() && cost > 0.0 {
        (cost as u64).max(1)
    } else {
        1
    }
}

/// 固定顺序的优先级键：等待越久、有攻击目标、离玩家越近、uid 越小越先算。
/// 全部用整数，浮点距离按位比较，和线程数 / 墙上时间无关。
fn priority(pending: &Pending, now: i64, players: &[PlayerView]) -> (u64, u8, u64, EntityUid, u64) {
    // `Reverse(等待刻数)` 用位取反表达：等得越久键越小、越先算。
    let waited = !(now.saturating_sub(pending.issued_at).max(0) as u64);
    (
        waited,
        u8::from(!pending.has_attack_target),
        nearest_player_bits(pending.request.mob.position, players),
        pending.uid,
        serial(pending.uid, pending.local_serial),
    )
}

/// 到最近非旁观玩家的水平距离平方，按位比较（非负且有限，位序和数值序一致）。
pub fn nearest_player_bits(position: Vec3, players: &[PlayerView]) -> u64 {
    let mut best = f64::INFINITY;
    for player in players {
        if !player.alive || player.spectator {
            continue;
        }
        let dx = position.x - player.position.x;
        let dz = position.z - player.position.z;
        best = best.min(dx * dx + dz * dz);
    }
    if best.is_finite() {
        best.to_bits()
    } else {
        u64::MAX
    }
}

/// 给压测 / 单测读的一刻统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub deferred: usize,
    pub deliveries: usize,
    pub nodes: u64,
    pub searches: u64,
}

impl PhaseState {
    pub fn stats(&self) -> Stats {
        Stats {
            deferred: self.deferred.len(),
            deliveries: self.deliveries.len(),
            nodes: self.last_nodes,
            searches: self.last_searches,
        }
    }

    pub fn delivery_count(&self) -> usize {
        self.deliveries.len()
    }
}

/// 下一刻开头把结果装进生物的导航：开始走 / 覆盖重算路径 / 留给原调用点取走。
/// 目标换了、请求已作废、结果过期就丢掉，不把过期路径塞给它。
pub fn install(mob: &mut super::Mob, view: &dyn BlockView) {
    let Some(replay) = mob.navigation.replay.clone() else {
        return;
    };
    if replay.consumed {
        return;
    }
    if replay.local_serial <= mob.navigation.stop_serial {
        mob.navigation.replay = None;
        return;
    }
    let expected = (replay.target >= 0).then_some(replay.target);
    if mob.target != expected {
        mob.navigation.replay = None;
        return;
    }
    match replay.action {
        Action::MoveTo { speed } | Action::MoveToEntity { speed } => {
            // 结果到了才「开始走」；请求发出的那一刻沿旧路走或原地等。
            mob.navigation
                .move_to_path(view, &replay.nav, replay.path.clone(), speed);
            if let Some(slot) = mob.navigation.replay.as_mut() {
                slot.consumed = true;
            }
        }
        Action::Recompute => {
            mob.navigation.path = replay.path.clone();
            if replay.path.is_some() {
                mob.navigation.target_pos = Some(replay.at);
                mob.navigation.reset_stuck_timeout();
            }
            if let Some(slot) = mob.navigation.replay.as_mut() {
                slot.consumed = true;
            }
        }
        Action::Inspect(_) => {
            // 检查结果留在导航里，由原调用点这一刻的 `create_path` 取走，
            // 后续语句（写记忆、写黑名单、换目标、放弃）照官方顺序跑。
        }
    }
}

#[cfg(test)]
mod tests;
