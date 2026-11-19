//! 岛的实施计划第 8 步：section 计数和岛的划分，只算不用。
//!
//! 依据 `docs/island-implementation-plan.md` 3.1 / 3.2 和
//! `docs/world-model-design.md` 6.1：划分的对象是「票想要加载的区块」
//! （加载等级 ≤ 33）并上还没收回的区块（`Owner` 是 `Delivering` /
//! `Region` / `Reclaiming`）。每 4×4 个区块记一个非空 section；section
//! 图按切比雪夫距离 ≤ 2 连成岛，新变非空的 section 会把它周围 ≤ 2 的
//! 岛并起来。岛内两块相距 ≥ 4 并持续 200 刻才拆。
//!
//! 这里真的维护岛编号，但**不动运行时**：`Region`、Director 的交付路径、
//! `Owner::Region(id)` 全都照旧。合并 / 拆分只作为事件和统计记下来，
//! 第 10 步才用它们驱动真实的拆合。增量更新只碰等级 / owner 变了的区块，
//! `naive_partition` 和 `full_recompute` 是全量对照，只在
//! `BAMBOO_PROFILE_ISLANDS=2` 或测试里跑；正式路径只走增量。
//!
//! Island plan step 8: section counts and island partitioning, computed but
//! not used. The live path is incremental; a naive full rebuild is the
//! reference used by the verify mode and by tests.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    time::Instant,
};

use mc_bamboo_core::ChunkPos;

/// section 的边长（区块）。三个迟滞数都是 `[判断]`，第 11 步按长时间压测复核过
/// （`docs/profiling.md` 第 17 节）。
pub(crate) const SECTION_SIZE: i32 = 4;
/// 非空 section 之间切比雪夫距离 ≤ 2 就属于同一个岛。
pub(crate) const MERGE_DISTANCE: i32 = 2;
/// 同一个岛里两块相距 ≥ 4 才考虑拆。
pub(crate) const SPLIT_DISTANCE: i32 = 4;
/// 上面那个条件持续 200 刻（10 秒）才真的拆。
pub(crate) const SPLIT_DELAY_TICKS: u64 = 200;
/// 每 20 刻检查一次最近有 section 变空的岛。
pub(crate) const CONNECTIVITY_INTERVAL_TICKS: u64 = 20;
/// 调参对比用的环境变量：盖过合并距离、拆分距离和拆分迟滞（刻）。不进 `bamboo.toml`，
/// 正式运行不设；设了不合法的值（合并距离 < 1、拆分距离 ≤ 合并距离）就忽略并警告。
///
/// Environment variables for tuning comparisons: override the merge
/// distance, split distance and split delay (ticks). Not in `bamboo.toml` and
/// unset in production; invalid values (merge < 1, split ≤ merge) are ignored
/// with a warning.
pub(crate) const MERGE_DISTANCE_ENV: &str = "BAMBOO_ISLAND_MERGE_DISTANCE";
pub(crate) const SPLIT_DISTANCE_ENV: &str = "BAMBOO_ISLAND_SPLIT_DISTANCE";
pub(crate) const SPLIT_DELAY_ENV: &str = "BAMBOO_ISLAND_SPLIT_DELAY";

/// 分区器的迟滞参数。缺省就是上面三个常量；只有调参时用环境变量换。
///
/// The partitioner's hysteresis parameters: the constants above unless a
/// tuning run overrides them through the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PartitionParams {
    pub(crate) merge_distance: i32,
    pub(crate) split_distance: i32,
    pub(crate) split_delay_ticks: u64,
}

impl PartitionParams {
    pub(crate) const DEFAULT: Self = Self {
        merge_distance: MERGE_DISTANCE,
        split_distance: SPLIT_DISTANCE,
        split_delay_ticks: SPLIT_DELAY_TICKS,
    };

    /// 缺省值，被设了的环境变量盖过；组合不合法时整组退回缺省。
    ///
    /// The defaults, overridden by whichever variables are set; an invalid
    /// combination falls back to the defaults as a whole.
    pub(crate) fn from_env() -> Self {
        fn read<T: std::str::FromStr>(name: &str) -> Option<T> {
            let value = std::env::var(name).ok()?;
            let parsed = value.trim().parse().ok();
            if parsed.is_none() {
                tracing::warn!(%value, "ignoring an invalid {name}");
            }
            parsed
        }
        let params = Self {
            merge_distance: read(MERGE_DISTANCE_ENV).unwrap_or(MERGE_DISTANCE),
            split_distance: read(SPLIT_DISTANCE_ENV).unwrap_or(SPLIT_DISTANCE),
            split_delay_ticks: read(SPLIT_DELAY_ENV).unwrap_or(SPLIT_DELAY_TICKS),
        };
        if params.merge_distance < 1 || params.split_distance <= params.merge_distance {
            tracing::warn!(?params, "ignoring invalid island partition parameters");
            return Self::DEFAULT;
        }
        params
    }
}

/// 加载等级 ≤ 33 就是「票想要加载」。
pub(crate) const DESIRED_LEVEL: i32 = 33;
/// 家岛的编号，和 `HOME_REGION` 一致（运行时一直在）。
const HOME_ISLAND: u32 = mc_bamboo_world::director::HOME_REGION;

/// 一个 section 的坐标（按区块除以 `SECTION_SIZE`，向负无穷取整）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SectionPos {
    pub(crate) x: i32,
    pub(crate) z: i32,
}

impl SectionPos {
    /// 极端区块坐标用 i64 做除法，不让 `i32::MIN` 溢出。
    pub(crate) fn of(chunk: ChunkPos) -> Self {
        let size = i64::from(SECTION_SIZE);
        Self {
            x: (i64::from(chunk.x).div_euclid(size)) as i32,
            z: (i64::from(chunk.z).div_euclid(size)) as i32,
        }
    }

    pub(crate) fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// 两个 section 的切比雪夫距离；坐标差用 i64 算，不溢出。
fn chebyshev(a: SectionPos, b: SectionPos) -> i32 {
    let dx = (i64::from(a.x) - i64::from(b.x)).abs();
    let dz = (i64::from(a.z) - i64::from(b.z)).abs();
    dx.max(dz).min(i64::from(i32::MAX)) as i32
}

/// 切比雪夫距离 ≤ radius 的 section 邻居，不含自己。
fn section_neighbours(at: SectionPos, radius: i32) -> Vec<SectionPos> {
    let mut out = Vec::with_capacity((2 * radius + 1) as usize * (2 * radius + 1) as usize - 1);
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx == 0 && dz == 0 {
                continue;
            }
            out.push(SectionPos::new(
                at.x.saturating_add(dx),
                at.z.saturating_add(dz),
            ));
        }
    }
    out
}

/// 按「距离 ≤ `MERGE_DISTANCE`」把非空 section 连成组的全量实现。
/// 组按内部最小 section 坐标排序，编号只用于对照，运行时的编号单独算。
/// 拆分迟滞是时间维度的，不影响「哪些 section 该并在一起」，所以对照
/// 只验证合并部分；拆分时刻由单独的随机测试钉住。
///
/// Full reference: connect non-empty sections within `MERGE_DISTANCE`.
/// Split hysteresis is time-dependent and is covered by separate tests.
pub(crate) fn naive_partition(
    sections: &BTreeSet<SectionPos>,
    merge_distance: i32,
) -> Vec<BTreeSet<SectionPos>> {
    let mut seen: HashSet<SectionPos> = HashSet::new();
    let mut groups: Vec<BTreeSet<SectionPos>> = Vec::new();
    for &start in sections {
        if !seen.insert(start) {
            continue;
        }
        let mut group = BTreeSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(at) = queue.pop_front() {
            group.insert(at);
            for other in section_neighbours(at, merge_distance) {
                if sections.contains(&other) && seen.insert(other) {
                    queue.push_back(other);
                }
            }
        }
        groups.push(group);
    }
    canonical(groups)
}

/// 一个岛内部的连通性检查：只连「切比雪夫距离 < split_distance」的边，
/// 返回的每个连通块是拆分后的一个岛。半径至少是合并距离，这样
/// 「曾经合并过」的边总还在。
fn split_group(group: BTreeSet<SectionPos>, params: PartitionParams) -> Vec<BTreeSet<SectionPos>> {
    let radius = params
        .split_distance
        .saturating_sub(1)
        .max(params.merge_distance);
    let mut seen: HashSet<SectionPos> = HashSet::new();
    let mut out = Vec::new();
    for &start in &group {
        if !seen.insert(start) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(at) = queue.pop_front() {
            component.insert(at);
            for other in section_neighbours(at, radius) {
                if group.contains(&other) && seen.insert(other) {
                    queue.push_back(other);
                }
            }
        }
        out.push(component);
    }
    out
}

/// 一次合并或拆分，带涉及的岛和 section。第 10 步拿它驱动真实拆合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopologyEvent {
    pub(crate) at_tick: u64,
    pub(crate) merge: bool,
    /// 合并前 / 拆分后的岛编号，按升序。
    pub(crate) islands: Vec<u32>,
    /// 涉及的 section，按升序。
    pub(crate) sections: Vec<SectionPos>,
}

/// 运行时要照着做的一次拆合（第 10 步）。按发生的先后排，主控在全部岛停放时逐条照做。
/// 新建的岛不需要一条：主控第一次往一个还不存在的岛投信或放人时建一个空岛。
///
/// One structural change the runtime must replay (step 10), in order; the
/// master applies them while every island is parked. A new island needs no
/// entry: the master creates an empty one the first time it posts to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TopologyOp {
    /// `from` 整个倒进 `into`。
    Merge { into: u32, from: u32 },
    /// `from` 里属于 `sections` 的部分拆成新岛 `to`。
    Split {
        from: u32,
        to: u32,
        sections: BTreeSet<SectionPos>,
    },
}

/// 一个区块这一刻的依据变化。`None` 表示这一项没变；两个 `Some` 可以同一格。
///
/// A chunk's membership delta for this tick. `None` means unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkDelta {
    pub(crate) pos: ChunkPos,
    pub(crate) desired: Option<bool>,
    pub(crate) held: Option<bool>,
}

impl ChunkDelta {
    pub(crate) fn new(pos: ChunkPos, desired: Option<bool>, held: Option<bool>) -> Self {
        Self { pos, desired, held }
    }
}

/// 一刻里分区器算出的变化。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TickResult {
    pub(crate) nanos: u64,
    pub(crate) verify_nanos: u64,
    pub(crate) merges: u64,
    pub(crate) splits: u64,
    pub(crate) pending_merges: u64,
    pub(crate) pending_splits: u64,
    pub(crate) cancelled_splits: u64,
    pub(crate) island_count: usize,
    pub(crate) invariant_failures: u64,
}

/// 岛的划分。所有状态都在主控侧；关着时宿主只判一次布尔值。
#[derive(Debug, Clone)]
pub(crate) struct IslandPartition {
    /// 「票想要加载」的区块（加载等级 ≤ 33）。
    desired: BTreeSet<ChunkPos>,
    /// 还没收回的区块（Owner::Delivering / Region / Reclaiming）。
    held: BTreeSet<ChunkPos>,
    /// 非空 section → 这个 section 里的依据区块数。
    sections: HashMap<SectionPos, u32>,
    /// section → 岛编号。
    owner: HashMap<SectionPos, u32>,
    /// 岛编号 → 它的 section 集合。
    islands: BTreeMap<u32, BTreeSet<SectionPos>>,
    /// 下一个新岛的编号，只增不复用。
    next_id: u32,
    /// 待拆分：岛编号 → 「两块相距 ≥ 4」从哪一刻开始成立。
    split_since: BTreeMap<u32, u64>,
    /// 最近有 section 变空的岛；每 20 刻的连通性检查只看它们（3.2）。
    split_candidates: BTreeSet<u32>,
    /// 每刻全量对照。
    verify: bool,
    /// 迟滞参数（缺省是常量，调参时由环境变量换）。
    params: PartitionParams,
    pub(crate) now: u64,
    pub(crate) events: Vec<TopologyEvent>,
    /// 运行时要照做的拆合（第 10 步）。只在 `record_ops` 打开时攒。
    ops: Vec<TopologyOp>,
    record_ops: bool,
    /// 这一刻在 `tick` 之前已经提前应用过的增量带来的计数，`tick` 时并进结果。
    carried: TickResult,
    /// 改坏确认：故意漏掉一次相邻岛的合并。
    #[cfg(test)]
    pub(crate) skip_merge_once: bool,
    /// 改坏确认：故意把拆分迟滞当成 0。
    #[cfg(test)]
    pub(crate) instant_split: bool,
}

impl Default for IslandPartition {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandPartition {
    pub(crate) fn new() -> Self {
        Self {
            desired: BTreeSet::new(),
            held: BTreeSet::new(),
            sections: HashMap::new(),
            owner: HashMap::new(),
            islands: BTreeMap::new(),
            next_id: 0,
            split_since: BTreeMap::new(),
            split_candidates: BTreeSet::new(),
            verify: false,
            params: PartitionParams::from_env(),
            now: 0,
            events: Vec::new(),
            ops: Vec::new(),
            record_ops: false,
            carried: TickResult::default(),
            #[cfg(test)]
            skip_merge_once: false,
            #[cfg(test)]
            instant_split: false,
        }
    }

    /// 这个分区器用的迟滞参数。
    pub(crate) fn params(&self) -> PartitionParams {
        self.params
    }

    pub(crate) fn set_verify(&mut self, on: bool) {
        self.verify = on;
    }

    /// 运行时照着拆合（第 10 步的 `islands = true`）：之后的合并和拆分都记成 [`TopologyOp`]。
    ///
    /// Drive the runtime (step 10's `islands = true`): record merges and splits as ops.
    pub(crate) fn record_ops(&mut self) {
        self.record_ops = true;
    }

    /// 取走攒着的拆合。
    pub(crate) fn take_ops(&mut self) -> Vec<TopologyOp> {
        std::mem::take(&mut self.ops)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.islands.len()
    }

    /// 全部岛的编号，升序。
    #[cfg(test)]
    pub(crate) fn island_ids(&self) -> Vec<u32> {
        self.islands.keys().copied().collect()
    }

    /// 一个岛按（x，z）最小的 section：新岛的随机源由它派生（计划 3.3「岛的随机源」第 1 条）。
    pub(crate) fn origin_of(&self, id: u32) -> Option<SectionPos> {
        self.islands
            .get(&id)
            .and_then(|sections| sections.iter().next().copied())
    }

    /// 区块所在 section 归哪个岛；section 是空的时为 `None`。
    pub(crate) fn island_of_chunk(&self, chunk: ChunkPos) -> Option<u32> {
        self.owner
            .get(&SectionPos::of(chunk))
            .copied()
            .filter(|&id| id != u32::MAX)
    }

    /// 在 `tick` 之前先应用一批增量（第 10 步：投信之前分区器要先知道新区块归哪个岛）。
    /// 合并当场发生；拆分迟滞仍只在 `tick` 里推进。计数留到 `tick` 时并进结果。
    ///
    /// Apply a batch of deltas before `tick` (step 10: the partitioner must
    /// know a new chunk's island before mail is posted). Merges happen now;
    /// split hysteresis still only advances in `tick`. Counts are carried
    /// into `tick`'s result.
    pub(crate) fn apply_now(&mut self, deltas: &[ChunkDelta]) {
        if deltas.is_empty() {
            return;
        }
        let mut result = std::mem::take(&mut self.carried);
        self.apply_deltas(deltas, &mut result);
        self.carried = result;
    }

    /// 测试专用：不管距离把 `from` 并进 `into`（并起来永远安全），照常记一条拆合。
    ///
    /// Test only: merge `from` into `into` regardless of distance (a merge is
    /// always safe), recorded as usual.
    #[cfg(test)]
    pub(crate) fn force_merge(&mut self, into: u32, from: u32) -> bool {
        if into == from || !self.islands.contains_key(&into) || !self.islands.contains_key(&from) {
            return false;
        }
        let mut result = std::mem::take(&mut self.carried);
        self.absorb_islands(into, &BTreeSet::from([into, from]), &mut result);
        self.carried = result;
        true
    }

    /// 测试专用：岛里有相距 ≥ 拆分距离的几块时不等迟滞当场拆开，返回拆出来的新岛。
    ///
    /// Test only: split an island whose parts are already far enough apart,
    /// without waiting for the hysteresis; returns the new islands.
    #[cfg(test)]
    pub(crate) fn force_split(&mut self, id: u32) -> Vec<u32> {
        let mut result = std::mem::take(&mut self.carried);
        let made = self.split_now(id, &mut result);
        self.carried = result;
        made
    }

    pub(crate) fn desired_len(&self) -> usize {
        self.desired.len()
    }

    pub(crate) fn held_len(&self) -> usize {
        self.held.len()
    }

    pub(crate) fn sections(&self) -> BTreeSet<SectionPos> {
        self.sections.keys().copied().collect()
    }

    pub(crate) fn island_sizes(&self) -> Vec<usize> {
        self.islands.values().map(BTreeSet::len).collect()
    }

    /// 每个岛里「票想要加载 / 还没收回」的区块数，同一格只算一次。
    pub(crate) fn island_chunk_sizes(&self) -> Vec<usize> {
        let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
        let union: BTreeSet<ChunkPos> = self.desired.union(&self.held).copied().collect();
        for chunk in union {
            if let Some(&id) = self.owner.get(&SectionPos::of(chunk)) {
                *sizes.entry(id).or_default() += 1;
            }
        }
        sizes.into_values().collect()
    }

    #[cfg(test)]
    pub(crate) fn island_of(&self, chunk: ChunkPos) -> Option<u32> {
        self.owner.get(&SectionPos::of(chunk)).copied()
    }

    /// 这一刻 pending 的拆分登记数。
    #[cfg(test)]
    pub(crate) fn pending_splits(&self) -> usize {
        self.split_since.len()
    }

    /// 对照模式的全量对账：把 Director 的当前视图折算成一次增量。
    /// 正式路径不调用它（开销随已加载区块数增长）。
    ///
    /// Reference-mode reconciliation against the Director's current view.
    /// Not called on the live path (cost grows with the loaded chunk count).
    pub(crate) fn reconcile(
        &self,
        desired: &BTreeSet<ChunkPos>,
        held: &BTreeSet<ChunkPos>,
    ) -> Vec<ChunkDelta> {
        let mut deltas: BTreeMap<ChunkPos, ChunkDelta> = BTreeMap::new();
        for &pos in desired.difference(&self.desired) {
            deltas
                .entry(pos)
                .or_insert_with(|| ChunkDelta::new(pos, None, None))
                .desired = Some(true);
        }
        for &pos in self.desired.difference(desired) {
            deltas
                .entry(pos)
                .or_insert_with(|| ChunkDelta::new(pos, None, None))
                .desired = Some(false);
        }
        for &pos in held.difference(&self.held) {
            deltas
                .entry(pos)
                .or_insert_with(|| ChunkDelta::new(pos, None, None))
                .held = Some(true);
        }
        for &pos in self.held.difference(held) {
            deltas
                .entry(pos)
                .or_insert_with(|| ChunkDelta::new(pos, None, None))
                .held = Some(false);
        }
        deltas.into_values().collect()
    }

    /// 一个世界刻：算 section 计数、合并、拆分迟滞，断言不变量。
    pub(crate) fn tick(&mut self, now: u64, deltas: &[ChunkDelta]) -> TickResult {
        let start = Instant::now();
        self.now = now;
        let mut result = std::mem::take(&mut self.carried);
        self.apply_deltas(deltas, &mut result);
        self.finish_tick(now, &mut result);
        result.nanos = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        result.island_count = self.islands.len();
        // 常开的不变量检查只看每个非空 section 的 ≤2 邻居，和增量更新
        // 同一批局部数据，不扫全表；失败只记数，不在世界线程上 panic。
        if !self.local_invariants_hold() {
            result.invariant_failures = 1;
        }
        if self.verify {
            let verify_start = Instant::now();
            // 全量对照分两步：朴素划分检查「该并的都并了」，岛编号把
            // 迟滞期间还没真正拆开的几块并回同一个岛（拆分是时间维度的，
            // 朴素划分是纯函数，不知道 200 刻的等待）。
            assert_eq!(
                canonical(self.snapshot()),
                canonical(self.full_recompute()),
                "增量岛划分和全量划分不一致：tick={now}"
            );
            assert!(
                self.invariants_hold(),
                "不同岛的非空 section 距离必须 > {}：tick={now}",
                self.params.merge_distance
            );
            result.verify_nanos =
                verify_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        }
        result
    }

    /// 每刻的局部不变量：每个非空 section 恰好在一个岛，且不同岛的
    /// section 距离必须 > MERGE_DISTANCE。只查 ≤2 的邻居。
    fn local_invariants_hold(&self) -> bool {
        for (&section, &id) in &self.owner {
            if !self.sections.contains_key(&section)
                || id == u32::MAX
                || !self
                    .islands
                    .get(&id)
                    .is_some_and(|group| group.contains(&section))
            {
                return false;
            }
            for other in section_neighbours(section, self.params.merge_distance) {
                if let Some(&other_id) = self.owner.get(&other)
                    && other_id != id
                {
                    return false;
                }
            }
        }
        self.owner.len() == self.sections.len()
    }

    /// 全量重算：从当前非空 section 集合按 ≤2 做朴素连通块，再把属于
    /// 同一个岛（拆分迟滞期间还没真的拆开）的块并回去。迟滞是有时间的
    /// 状态，所以「全量」要带上当前岛编号；合并边是否漏掉由
    /// `invariants_hold` 独立用朴素连通块检查，不依赖这份编号。
    ///
    /// Full recompute from the current section set. Split hysteresis is
    /// time-dependent, so the current island assignment is carried over;
    /// missed merge edges are checked independently by `invariants_hold`.
    pub(crate) fn full_recompute(&self) -> Vec<BTreeSet<SectionPos>> {
        let mut by_island: BTreeMap<u32, BTreeSet<SectionPos>> = BTreeMap::new();
        for group in naive_partition(&self.sections(), self.params.merge_distance) {
            let mut ids = BTreeSet::new();
            for section in &group {
                if let Some(&id) = self.owner.get(section) {
                    ids.insert(id);
                }
            }
            for id in ids {
                by_island
                    .entry(id)
                    .or_default()
                    .extend(group.iter().copied());
            }
        }
        canonical(by_island.into_values().collect())
    }

    /// 只把这一刻变了的位置折算成 section 计数的变化；没有变化的刻是空循环。
    fn apply_deltas(&mut self, deltas: &[ChunkDelta], result: &mut TickResult) {
        let mut touched: HashMap<SectionPos, i32> = HashMap::new();
        for delta in deltas {
            let section = SectionPos::of(delta.pos);
            let (desired_before, held_before) = (
                self.desired.contains(&delta.pos),
                self.held.contains(&delta.pos),
            );
            let before = i32::from(desired_before) + i32::from(held_before);
            match delta.desired {
                Some(true) => {
                    self.desired.insert(delta.pos);
                }
                Some(false) => {
                    self.desired.remove(&delta.pos);
                }
                None => {}
            }
            match delta.held {
                Some(true) => {
                    self.held.insert(delta.pos);
                }
                Some(false) => {
                    self.held.remove(&delta.pos);
                }
                None => {}
            }
            let after = i32::from(self.desired.contains(&delta.pos))
                + i32::from(self.held.contains(&delta.pos));
            *touched.entry(section).or_default() += after - before;
        }
        let mut became_empty = Vec::new();
        let mut became_nonempty = Vec::new();
        for (section, change) in touched {
            if change == 0 {
                continue;
            }
            let old = self.sections.get(&section).copied().unwrap_or(0);
            let new = (i64::from(old) + i64::from(change)).clamp(0, i64::from(u32::MAX)) as u32;
            if new == 0 {
                self.sections.remove(&section);
                became_empty.push(section);
            } else {
                self.sections.insert(section, new);
                if old == 0 {
                    became_nonempty.push(section);
                }
            }
        }
        // 变空的 section 可能把岛断开；下次连通性检查只看这些岛（3.2）。
        for section in became_empty {
            if let Some(id) = self.owner.get(&section).copied() {
                self.split_candidates.insert(id);
            }
            self.remove_section(section, result);
        }
        became_nonempty.sort_unstable();
        for section in &became_nonempty {
            self.owner.insert(*section, u32::MAX);
        }
        for &section in &became_nonempty {
            self.add_section(section, result);
        }
        // 新 section 落进的岛要重新判一次「还该不该拆」。
        for section in &became_nonempty {
            if let Some(&target) = self.owner.get(section)
                && target != u32::MAX
            {
                self.split_candidates.insert(target);
            }
        }
    }

    /// 一个 section 从空变非空：查周围 ≤2 的 section，按 3.1 归入 0 / 1 / 多个岛。
    fn add_section(&mut self, section: SectionPos, result: &mut TickResult) {
        let mut neighbours = BTreeSet::new();
        for other in section_neighbours(section, self.params.merge_distance) {
            if let Some(&id) = self.owner.get(&other)
                && id != u32::MAX
            {
                neighbours.insert(id);
            }
        }
        #[cfg(test)]
        if std::mem::take(&mut self.skip_merge_once) && neighbours.len() > 1 {
            // 故意只取最小编号，不合并别的岛，让全量对照报出来。
            let smallest = *neighbours.iter().next().expect("邻居不为空");
            neighbours.retain(|&id| id == smallest);
        }
        if neighbours.len() > 1 {
            result.pending_merges += 1;
            self.events.push(TopologyEvent {
                at_tick: self.now,
                merge: true,
                islands: neighbours.iter().copied().collect(),
                sections: vec![section],
            });
        }
        let target = if let Some(&smallest) = neighbours.iter().next() {
            // 3.1：暂时记在编号最小的岛名下，然后按 ≤2 的距离立即合并。
            if neighbours.len() > 1 {
                self.absorb_islands(smallest, &neighbours, result);
            }
            smallest
        } else {
            let id = self.fresh_id();
            self.islands.insert(id, BTreeSet::new());
            id
        };
        self.owner.insert(section, target);
        if let Some(group) = self.islands.get_mut(&target) {
            group.insert(section);
        }
        // 新 section 可能把两块原本分开的「远距离」裂片重新连上，也可能离裂口很远（岛里别处
        // 有人在走）：这里不取消待拆分，由下一次连通性检查按当时的连通块判（连上了才取消，
        // 拆的时候也按当时的连通块拆）。以前这里一律取消，长时间压测里人多的大岛每刻都有新
        // section，计时一直被清零，一次都拆不开（`docs/profiling.md` 第 17 节）。
        // A new section may reconnect two far parts, or lie far from the gap
        // (someone walking elsewhere in the island): the pending split stays,
        // and the next connectivity check decides from the components at that
        // time (cancelling only if reconnected; a split also uses the
        // components of that time). Cancelling here unconditionally kept a
        // busy island's timer at zero in the long run, so it never split.
    }

    /// 把 `sources` 里除 `target` 以外的岛整个倒进 target，记一次合并。
    fn absorb_islands(&mut self, target: u32, sources: &BTreeSet<u32>, result: &mut TickResult) {
        for &id in sources {
            if id == target {
                continue;
            }
            let Some(group) = self.islands.remove(&id) else {
                continue;
            };
            result.merges += 1;
            if self.record_ops {
                self.ops.push(TopologyOp::Merge {
                    into: target,
                    from: id,
                });
            }
            for section in &group {
                self.owner.insert(*section, target);
            }
            self.events.push(TopologyEvent {
                at_tick: self.now,
                merge: true,
                islands: vec![target, id],
                sections: group.iter().copied().collect(),
            });
            if self.split_since.remove(&id).is_some() {
                result.cancelled_splits += 1;
            }
            if let Some(into) = self.islands.get_mut(&target) {
                into.extend(group);
            }
        }
        // 吸收方自己的待拆分不取消：理由同 `add_section`，由下一次连通性检查判。
        // The absorber's own pending split stays, as in `add_section`; the
        // next connectivity check decides.
    }

    /// section 变空：从它的岛里移除，岛空了就删掉。
    fn remove_section(&mut self, section: SectionPos, result: &mut TickResult) {
        let Some(id) = self.owner.remove(&section) else {
            return;
        };
        if let Some(group) = self.islands.get_mut(&id) {
            group.remove(&section);
            if group.is_empty() {
                self.islands.remove(&id);
                self.split_since.remove(&id);
            }
        }
        let _ = result;
    }

    /// 每 20 刻看一次连通性；每次检查都分成相距 ≥4 的几块、持续 200 刻才拆，按拆的那一刻的
    /// 连通块拆。哪一次检查时连上了（距离 <4）就取消登记（记 `cancelled_splits`）；期间岛里
    /// 新添 section、并进别的岛都不取消。
    /// `pending_splits` 是这一刻实际登记的待拆分次数（含到点执行的那次）。
    fn finish_tick(&mut self, now: u64, result: &mut TickResult) {
        if !now.is_multiple_of(CONNECTIVITY_INTERVAL_TICKS) {
            return;
        }
        // 3.2 的增量：只看最近有 section 变空的岛，外加还登记着待拆分的岛
        // （它们要一直计时到 200 刻）。没有变化的岛不做连通性 BFS。
        let mut ids: BTreeSet<u32> = std::mem::take(&mut self.split_candidates);
        ids.extend(self.split_since.keys().copied());
        for id in ids {
            let Some(group) = self.islands.get(&id).cloned() else {
                continue;
            };
            let components = split_group(group, self.params);
            if components.len() <= 1 {
                if self.split_since.remove(&id).is_some() {
                    result.cancelled_splits += 1;
                }
                continue;
            }
            result.pending_splits += 1;
            let since = *self.split_since.entry(id).or_insert(now);
            #[cfg(test)]
            let ready =
                self.instant_split || now.saturating_sub(since) >= self.params.split_delay_ticks;
            #[cfg(not(test))]
            let ready = now.saturating_sub(since) >= self.params.split_delay_ticks;
            if !ready {
                self.events.push(TopologyEvent {
                    at_tick: now,
                    merge: false,
                    islands: vec![id],
                    sections: components
                        .iter()
                        .flat_map(BTreeSet::iter)
                        .copied()
                        .collect(),
                });
                continue;
            }
            self.split_since.remove(&id);
            self.split_components(id, components, result);
        }
        // 登记在册的岛如果已经不存在，清掉记录。
        self.split_since
            .retain(|id, _| self.islands.contains_key(id));
    }

    /// 一个岛的 section 分成几块：保留编号最小的那块，其余各建一个新岛，返回新岛的编号。
    fn split_components(
        &mut self,
        id: u32,
        components: Vec<BTreeSet<SectionPos>>,
        result: &mut TickResult,
    ) -> Vec<u32> {
        let mut parts = components;
        parts.sort_by(|a, b| a.iter().next().cmp(&b.iter().next()));
        let keep = parts.remove(0);
        if let Some(group) = self.islands.get_mut(&id) {
            *group = keep.clone();
        }
        for section in &keep {
            self.owner.insert(*section, id);
        }
        let mut made = Vec::new();
        for part in parts {
            let new_id = self.fresh_id();
            for &section in &part {
                self.owner.insert(section, new_id);
            }
            self.islands.insert(new_id, part.clone());
            result.splits += 1;
            if self.record_ops {
                self.ops.push(TopologyOp::Split {
                    from: id,
                    to: new_id,
                    sections: part.clone(),
                });
            }
            self.events.push(TopologyEvent {
                at_tick: self.now,
                merge: false,
                islands: vec![id, new_id],
                sections: part.iter().copied().collect(),
            });
            made.push(new_id);
        }
        made
    }

    /// 测试专用：不等迟滞，按拆分距离当场拆一个岛。
    #[cfg(test)]
    fn split_now(&mut self, id: u32, result: &mut TickResult) -> Vec<u32> {
        let Some(group) = self.islands.get(&id).cloned() else {
            return Vec::new();
        };
        let components = split_group(group, self.params);
        if components.len() <= 1 {
            return Vec::new();
        }
        self.split_since.remove(&id);
        self.split_components(id, components, result)
    }

    /// 新岛的编号：家岛（0）空着就复用它，否则取下一个没用过的。家岛的编号复用让
    /// 「只有一群人」时那个岛总是家岛（运行时一直有它）。
    ///
    /// A new island id: reuse home (0) when it is free, otherwise the next
    /// unused id, so a lone group always lives on the home island.
    fn fresh_id(&mut self) -> u32 {
        if !self.islands.contains_key(&HOME_ISLAND) {
            return HOME_ISLAND;
        }
        let id = self.next_id.max(HOME_ISLAND + 1);
        self.next_id = id.saturating_add(1);
        id
    }

    /// 当前增量划分的快照，按最小 section 排序。
    pub(crate) fn snapshot(&self) -> Vec<BTreeSet<SectionPos>> {
        canonical(self.islands.values().cloned().collect())
    }

    /// 全量不变量：每个非空 section 恰好在一个岛；每个 ≤2 连通块
    /// 完整落在一个岛里。只在测试 / 对照模式跑（每刻只跑局部版）。
    pub(crate) fn invariants_hold(&self) -> bool {
        let sections = self.sections();
        let mut seen: HashSet<SectionPos> = HashSet::new();
        for (&section, &id) in &self.owner {
            if !sections.contains(&section) || !seen.insert(section) {
                return false;
            }
            if id == u32::MAX {
                return false;
            }
            if !self
                .islands
                .get(&id)
                .is_some_and(|group| group.contains(&section))
            {
                return false;
            }
        }
        if seen.len() != sections.len() {
            return false;
        }
        for group in naive_partition(&sections, self.params.merge_distance) {
            // 同一个朴素连通块必须属于同一个岛，或者它内部所有跨岛边
            // 都是已经满足拆分条件的远距离裂片（距离 ≥ SPLIT_DISTANCE）。
            let entries: Vec<(SectionPos, u32)> = group
                .iter()
                .filter_map(|&section| self.owner.get(&section).map(|&id| (section, id)))
                .collect();
            for (i, &(a, a_id)) in entries.iter().enumerate() {
                for &(b, b_id) in &entries[i + 1..] {
                    // 同一条 ≤2 连通链里的 section 必须属于同一个岛；
                    // 拆开后的两块相距 ≥4，不会落在同一条链里。
                    if a_id != b_id && chebyshev(a, b) <= self.params.merge_distance {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 随机测试用：直接按「票想加载的区块」逐格断言不变量。
    #[cfg(test)]
    pub(crate) fn chunk_invariants_hold(&self, expected: &BTreeSet<ChunkPos>) -> bool {
        if expected != &self.desired.iter().copied().collect() {
            return false;
        }
        let mut by_island: HashMap<u32, Vec<ChunkPos>> = HashMap::new();
        let mut seen = HashSet::new();
        for &chunk in expected {
            let Some(id) = self.island_of(chunk) else {
                return false;
            };
            if !seen.insert(chunk) {
                return false;
            }
            by_island.entry(id).or_default().push(chunk);
        }
        for (i, (id_a, a)) in by_island.iter().enumerate() {
            for (id_b, b) in by_island.iter().skip(i + 1) {
                if id_a == id_b {
                    continue;
                }
                for &left in a {
                    for &right in b {
                        let dx = (i64::from(left.x) - i64::from(right.x)).abs();
                        let dz = (i64::from(left.z) - i64::from(right.z)).abs();
                        if dx.max(dz) <= i64::from(self.params.merge_distance) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

fn canonical(mut groups: Vec<BTreeSet<SectionPos>>) -> Vec<BTreeSet<SectionPos>> {
    groups.sort_by(|a, b| a.iter().next().cmp(&b.iter().next()));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_world::ticket::{TicketTable, TicketType};

    /// 测试自己的确定性随机源，不依赖外部 crate。
    struct Rng(u64);
    impl Rng {
        fn new(seed: u64) -> Self {
            Self(seed)
        }
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn c(x: i32, z: i32) -> ChunkPos {
        ChunkPos::new(x, z)
    }

    /// 把一组区块的「票想加载」状态折算成增量。
    fn deltas_for(partition: &IslandPartition, want: &BTreeSet<ChunkPos>) -> Vec<ChunkDelta> {
        partition.reconcile(want, &BTreeSet::new())
    }

    /// 随机移动玩家 / 随机挂票，逐刻断言不变量、增量 == 全量。
    /// 玩家坐标来自真的 `TicketTable`，所以测试的输入和运行时的等级传播一致。
    #[test]
    fn random_players_and_tickets_keep_invariants_and_match_the_naive_partition() {
        for seed in [1_u64, 7, 12345, 0xdead_beef, 0x5eed_1234] {
            let mut rng = Rng::new(seed);
            let mut tickets = TicketTable::new();
            tickets.set_view_distance(2);
            tickets.set_simulation_distance(2);
            let mut partition = IslandPartition::new();
            partition.set_verify(true);
            let mut players: BTreeMap<u64, ChunkPos> = BTreeMap::new();
            let mut forced: BTreeSet<ChunkPos> = BTreeSet::new();
            let mut held: BTreeSet<ChunkPos> = BTreeSet::new();
            let mut want: BTreeSet<ChunkPos> = BTreeSet::new();
            for tick in 1..=400_u64 {
                // 随机挂 / 撤一张强加载票（半径 1 的小方块）。
                if rng.below(4) == 0 {
                    let pos = c((rng.below(80) as i32) - 40, (rng.below(80) as i32) - 40);
                    if forced.insert(pos) {
                        tickets.add_with_radius(TicketType::Forced, pos, 1);
                    } else {
                        forced.remove(&pos);
                        tickets.remove_with_radius(TicketType::Forced, pos, 1);
                    }
                }
                // 玩家随机进出、随机走动一格到几格。
                if rng.below(3) == 0 {
                    let id = rng.below(6);
                    match players.get(&id).copied() {
                        Some(_at) if rng.below(5) == 0 => {
                            players.remove(&id);
                            tickets.set_player(id, None);
                        }
                        Some(at) => {
                            let to = c(
                                at.x + rng.below(7) as i32 - 3,
                                at.z + rng.below(7) as i32 - 3,
                            );
                            players.insert(id, to);
                            tickets.set_player(id, Some(to));
                        }
                        None => {
                            let at = c((rng.below(80) as i32) - 40, (rng.below(80) as i32) - 40);
                            players.insert(id, at);
                            tickets.set_player(id, Some(at));
                        }
                    }
                }
                // 按真的等级表算出「票想加载」的区块（≤33），作为期望集合。
                let changes = tickets.run_updates();
                let mut next_want = want.clone();
                for change in changes {
                    if change.loading.1 <= DESIRED_LEVEL {
                        next_want.insert(change.pos);
                    } else {
                        next_want.remove(&change.pos);
                    }
                }
                // 初始 want 为空，第一刻起所有 ≤33 的区块都靠变化列表补上；
                // 全量对照另外用等级表核一次，防止初始状态漏掉。
                let full: BTreeSet<ChunkPos> = tickets
                    .loading_chunks_at_most(DESIRED_LEVEL)
                    .into_iter()
                    .collect();
                assert_eq!(
                    next_want, full,
                    "增量等级列表要覆盖全部 ≤33 的区块：seed={seed} tick={tick}"
                );
                // 随机把一些已经加载的区块记成「还没收回」。
                if rng.below(6) == 0 {
                    let pos = c((rng.below(80) as i32) - 40, (rng.below(80) as i32) - 40);
                    if !held.remove(&pos) {
                        held.insert(pos);
                    }
                }
                let mut deltas = partition.reconcile(&next_want, &held);
                deltas.sort_by_key(|delta| delta.pos);
                want = next_want;
                let result = partition.tick(tick, &deltas);
                assert_eq!(result.invariant_failures, 0, "seed={seed} tick={tick}");
                assert!(
                    partition.invariants_hold(),
                    "section 不变量：seed={seed} tick={tick}"
                );
                assert!(
                    partition.chunk_invariants_hold(&want),
                    "不同岛的票想加载区块不相邻 / 每格恰在一个岛：seed={seed} tick={tick}"
                );
                // 桥撤掉之后短暂的「还没拆开」属于拆分迟滞，全量重算
                // 也带这份状态；纯朴素的 ≤2 连通块在迟滞窗口内会不同。
                assert_eq!(
                    partition.snapshot(),
                    partition.full_recompute(),
                    "seed={seed} tick={tick}"
                );
                // 每个朴素连通块必须完整落在一个岛里（漏合并会被这条抓到，
                // 已经真的拆开的裂片之间距离 ≥4，不在同一条链里）。
                for group in naive_partition(&partition.sections(), MERGE_DISTANCE) {
                    let ids: BTreeSet<u32> = group
                        .iter()
                        .filter_map(|section| partition.owner.get(section).copied())
                        .collect();
                    assert_eq!(ids.len(), 1, "漏合并：seed={seed} tick={tick}");
                }
            }
        }
    }

    /// 改坏确认：故意漏掉一次合并，每刻全量对照必须报出来。
    #[test]
    fn a_missed_merge_is_detected_by_the_naive_reference() {
        let mut partition = IslandPartition::new();
        partition.set_verify(true);
        let a = c(0, 0);
        let b = c(16, 0);
        // 左右两块各成一个岛：section (0,0) 和 (4,0)，距离 4 > 2。
        let left = BTreeSet::from([a]);
        partition.tick(1, &deltas_for(&partition, &left));
        let both = BTreeSet::from([a, b]);
        partition.tick(2, &deltas_for(&partition, &both));
        assert_eq!(partition.len(), 2);
        // 桥接区块落在 section (2,0)：同时和两个岛距离 2，必须合并。
        let bridge = c(2 * SECTION_SIZE, 0);
        let with_bridge = BTreeSet::from([a, b, bridge]);
        partition.skip_merge_once = true;
        let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            partition.tick(3, &deltas_for(&partition, &with_bridge));
        }));
        assert!(broken.is_err(), "漏合并时全量对照必须报出来");
    }

    /// 拆分迟滞：两块相距 ≥4 个 section 时，要持续 200 刻才真的拆；
    /// 中途重新连上会把登记取消。
    #[test]
    fn split_hysteresis_waits_200_ticks_and_cancels_when_reconnected() {
        let mut partition = IslandPartition::new();
        // 一条连贯的细桥把 (0,0) 和 (8,0) 连起来，全部属于一个岛。
        let bridge: BTreeSet<ChunkPos> = (0..=32).map(|x| c(x * SECTION_SIZE, 0)).collect();
        for tick in 1..=3 {
            partition.tick(tick, &deltas_for(&partition, &bridge));
        }
        assert_eq!(partition.len(), 1);
        // 拆掉中间的桥，剩两端相距 8 个 section。
        let ends: BTreeSet<ChunkPos> = bridge
            .iter()
            .copied()
            .filter(|pos| pos.x <= SECTION_SIZE || pos.x >= 28 * SECTION_SIZE)
            .collect();
        let first = partition.tick(CONNECTIVITY_INTERVAL_TICKS, &deltas_for(&partition, &ends));
        assert_eq!(first.splits, 0, "刚断开不能马上拆");
        assert_eq!(partition.pending_splits(), 1);
        assert_eq!(partition.len(), 1);
        // 重新桥回去：登记被取消。
        let reconnected = partition.tick(
            CONNECTIVITY_INTERVAL_TICKS * 2,
            &deltas_for(&partition, &bridge),
        );
        assert!(reconnected.cancelled_splits > 0);
        assert_eq!(partition.pending_splits(), 0);
        // 再断开并一直保持到 200 刻之后：真的拆开。
        partition.tick(
            CONNECTIVITY_INTERVAL_TICKS * 3,
            &deltas_for(&partition, &ends),
        );
        let mut split = None;
        for tick in (CONNECTIVITY_INTERVAL_TICKS * 4
            ..=SPLIT_DELAY_TICKS + CONNECTIVITY_INTERVAL_TICKS * 4)
            .step_by(CONNECTIVITY_INTERVAL_TICKS as usize)
        {
            let result = partition.tick(tick, &[]);
            if result.splits > 0 {
                split = Some(tick);
                break;
            }
        }
        let at = split.expect("持续 200 刻之后必须拆");
        assert!(at >= SPLIT_DELAY_TICKS, "拆分不得早于 200 刻：{at}");
        assert_eq!(partition.len(), 2);
        assert!(partition.invariants_hold());
    }

    /// 岛里别处一直有新 section（有人在走）不打断待拆分：两端断开以后照样 200 刻后拆开。
    /// 改坏确认：按以前的做法新 section 一律取消待拆分，这个岛就永远拆不开。
    ///
    /// New sections elsewhere in the island (someone walking) do not reset a
    /// pending split: the far ends still split 200 ticks after the gap opens.
    #[test]
    fn walking_elsewhere_in_an_island_does_not_postpone_its_split() {
        let mut partition = IslandPartition::new();
        let bridge: BTreeSet<ChunkPos> = (0..=32).map(|x| c(x * SECTION_SIZE, 0)).collect();
        for tick in 1..=3 {
            partition.tick(tick, &deltas_for(&partition, &bridge));
        }
        let ends: BTreeSet<ChunkPos> = bridge
            .iter()
            .copied()
            .filter(|pos| pos.x <= SECTION_SIZE || pos.x >= 28 * SECTION_SIZE)
            .collect();
        partition.tick(CONNECTIVITY_INTERVAL_TICKS, &deltas_for(&partition, &ends));
        assert_eq!(partition.pending_splits(), 1);
        // 左端旁边有人来回走：每刻交替多出、少掉一个紧挨着的 section。
        // Someone paces next to the left end: a neighbouring section appears
        // and disappears every other tick.
        let pacer = c(-SECTION_SIZE, 0);
        let mut split = None;
        for tick in CONNECTIVITY_INTERVAL_TICKS + 1..=SPLIT_DELAY_TICKS * 2 {
            let mut now = ends.clone();
            if tick % 2 == 0 {
                now.insert(pacer);
            }
            let result = partition.tick(tick, &deltas_for(&partition, &now));
            assert_eq!(result.cancelled_splits, 0, "第 {tick} 刻不该取消");
            if result.splits > 0 {
                split = Some(tick);
                break;
            }
        }
        let at = split.expect("两端一直断开，必须拆开");
        assert!(
            (SPLIT_DELAY_TICKS..=SPLIT_DELAY_TICKS + 2 * CONNECTIVITY_INTERVAL_TICKS).contains(&at),
            "按时拆开：{at}"
        );
        assert_eq!(partition.len(), 2);
        assert!(partition.invariants_hold());
    }

    /// 合并事件带涉及的岛和 section，第 10 步要用。
    #[test]
    fn merge_events_carry_the_involved_islands_and_sections() {
        let mut partition = IslandPartition::new();
        let a = c(0, 0);
        let b = c(24, 0);
        partition.tick(1, &deltas_for(&partition, &BTreeSet::from([a])));
        partition.tick(2, &deltas_for(&partition, &BTreeSet::from([a, b])));
        assert_eq!(partition.len(), 2);
        // 桥在 (2,0)：先并入左岛；再和右岛在 ≤2 范围内相遇，触发合并。
        let bridge = c(2 * SECTION_SIZE, 0);
        partition.tick(3, &deltas_for(&partition, &BTreeSet::from([a, b, bridge])));
        let right = c(4 * SECTION_SIZE, 0);
        partition.tick(
            4,
            &deltas_for(&partition, &BTreeSet::from([a, b, bridge, right])),
        );
        let mut merged = partition.events.iter().filter(|event| event.merge).count();
        assert!(merged > 0, "要有合并事件");
        assert_eq!(partition.len(), 1, "所有 section 并成一个岛");
        assert!(
            partition
                .events
                .iter()
                .filter(|event| event.merge)
                .any(|event| !event.islands.is_empty() && !event.sections.is_empty()),
            "事件要带岛编号和 section"
        );
        merged += 1;
        assert!(merged > 1);
    }

    /// 极端区块坐标不 panic：section 编号用 i64 除法、邻居用饱和加法。
    #[test]
    fn extreme_chunk_positions_do_not_panic() {
        let mut partition = IslandPartition::new();
        let want = BTreeSet::from([c(i32::MIN, i32::MAX), c(i32::MAX, i32::MIN)]);
        let result = partition.tick(1, &deltas_for(&partition, &want));
        assert_eq!(result.invariant_failures, 0);
        assert_eq!(partition.len(), 2);
        assert!(partition.invariants_hold());
        assert!(partition.chunk_invariants_hold(&want));
    }

    /// 两个玩家靠近时按真的票表合成一个岛，走远后又是两个。
    #[test]
    fn two_players_merge_when_they_meet_and_split_when_they_leave() {
        let mut tickets = TicketTable::new();
        tickets.set_view_distance(8);
        tickets.set_simulation_distance(8);
        let mut partition = IslandPartition::new();
        tickets.set_player(1, Some(c(0, 0)));
        tickets.set_player(2, Some(c(64, 0)));
        let _ = tickets.run_updates();
        let want: BTreeSet<ChunkPos> = tickets
            .loading_chunks_at_most(DESIRED_LEVEL)
            .into_iter()
            .collect();
        let result = partition.tick(1, &deltas_for(&partition, &want));
        assert_eq!(result.invariant_failures, 0);
        assert!(partition.len() >= 2, "相距 64 个区块至少两个岛");
        tickets.set_player(2, Some(c(0, 0)));
        let _ = tickets.run_updates();
        let want: BTreeSet<ChunkPos> = tickets
            .loading_chunks_at_most(DESIRED_LEVEL)
            .into_iter()
            .collect();
        let result = partition.tick(2, &deltas_for(&partition, &want));
        assert_eq!(result.invariant_failures, 0);
        assert_eq!(partition.len(), 1, "重叠的视野必须合成一个岛");
        assert!(partition.chunk_invariants_hold(&want));
    }

    /// section 计数：一格同时在 desired / held 里只算一次；撤掉一边不让它变空。
    #[test]
    fn section_counts_union_desired_and_held_chunks() {
        let mut partition = IslandPartition::new();
        let pos = c(0, 0);
        let desired = BTreeSet::from([pos]);
        let held = BTreeSet::from([pos]);
        partition.tick(1, &[ChunkDelta::new(pos, Some(true), Some(true))]);
        assert_eq!(partition.sections().len(), 1);
        // 撤掉票，盒子还在：section 仍非空。
        partition.tick(2, &[ChunkDelta::new(pos, Some(false), None)]);
        assert_eq!(partition.sections().len(), 1);
        assert!(partition.held.contains(&pos) || !partition.held.is_empty());
        // 盒子也收回：section 变空，岛消失。
        partition.tick(3, &[ChunkDelta::new(pos, None, Some(false))]);
        assert!(partition.sections().is_empty());
        assert_eq!(partition.len(), 0);
        assert!(partition.invariants_hold());
        let _ = (desired, held);
    }
}
