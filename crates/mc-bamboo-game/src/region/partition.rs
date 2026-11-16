//! 多区域 tick 第 2 步：忙区块集合与单元划分，只算不用。
//!
//! 见 `docs/multi-region-tick-design.md` 4.1 / 4.3 / 4.4：忙区块是最近
//! `quiet_ticks` 刻内排入过计划刻 / 方块事件、有排队事件、有醒着方块实体，
//! 或最近 `quiet_ticks` 刻内满足过「有醒着方块实体」的区块；按切比雪夫距离
//! `≤ 2 × halo` 连成连通块，每个连通块加缓冲圈是一个单元。跨区块的方块实体
//! 关注关系是一条额外的连接边（4.8）。
//!
//! 划分只用于统计，三个阶段仍照常串行执行。增量更新只碰状态变了的区块：
//! 加入时并联周围的单元，退出 / 关注关系变化时只把受影响的旧单元拆开重算。
//! 朴素的 `naive_partition` 是对照实现，只在测试和一个调试开关下运行。
//!
//! Step 2 of multi-region tick: busy chunks and unit partitioning, computed but
//! not used. Additions unite neighbouring units; removals locally rebuild the
//! affected component. A naive full rebuild is the reference implementation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use mc_bamboo_blocks::BlockEntities;
use mc_bamboo_core::ChunkPos;

/// 迟滞刻度：安静满这么多刻，区块才退出忙集合。设计稿 4.1 的默认值。
pub(crate) const QUIET_TICKS: i64 = 100;

/// 一个单元的划分结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unit {
    /// 忙区块，按坐标升序。
    pub(crate) busy: BTreeSet<ChunkPos>,
    /// 忙区块加 `halo` 圈缓冲，按坐标升序。
    pub(crate) area: BTreeSet<ChunkPos>,
    pub(crate) halo: i32,
}

/// 划分的一次变化，用来统计单元随时间的稳定性。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Topology {
    /// 这一刻并起来的次数（k 个旧单元合成 1 个算 k−1 次）。
    pub(crate) merges: u64,
    /// 这一刻拆开的次数（1 个旧单元裂成 k 个算 k−1 次）。
    pub(crate) splits: u64,
}

/// 忙区块集合：每个区块记最后一次「活动」的时刻，退出按 `quiet_ticks` 迟滞。
///
/// 活动来源用第 1 步的接口增量给出：计划刻 / 方块事件本刻排入过的区块
/// （`drain_touched_chunks`）、醒着 ticker 的区块（`awake_chunks`）、还排着
/// 事件的区块（`queued_chunks`）。没有来源变化的一刻只做一次过期检查。
#[derive(Debug, Clone)]
pub(crate) struct BusyChunks {
    /// 区块 → 最后一次活动的时刻。表里的区块全是忙的。
    last_active: HashMap<ChunkPos, i64>,
    quiet_ticks: i64,
}

impl Default for BusyChunks {
    fn default() -> Self {
        Self::new(QUIET_TICKS)
    }
}

impl BusyChunks {
    pub(crate) fn new(quiet_ticks: i64) -> Self {
        Self {
            last_active: HashMap::new(),
            quiet_ticks: quiet_ticks.max(1),
        }
    }

    /// 一个世界刻的活动增量。`touched_ticks` / `touched_events` 来自 drain 接口，
    /// `awake` 是本刻醒着 ticker 的区块全集，`queued` 是还排着事件的区块全集。
    /// 后两者通常每刻都一样：只比较集合，不变时不重复记活动。
    pub(crate) fn observe(
        &mut self,
        now: i64,
        touched_ticks: &[ChunkPos],
        touched_events: &[ChunkPos],
        awake: &BTreeSet<ChunkPos>,
        queued: &BTreeSet<ChunkPos>,
    ) -> Vec<ChunkPos> {
        let mut added = Vec::new();
        let mut mark = |chunk: ChunkPos, added: &mut Vec<ChunkPos>| {
            if self.last_active.insert(chunk, now).is_none() {
                added.push(chunk);
            }
        };
        for &chunk in touched_ticks {
            mark(chunk, &mut added);
        }
        for &chunk in touched_events {
            mark(chunk, &mut added);
        }
        // 醒着的 ticker 每刻都算活动，直到它睡着为止；睡着后还要安静
        // quiet_ticks 刻才退出（4.1 第 4 条）。
        for &chunk in awake {
            mark(chunk, &mut added);
        }
        // 排着队但本刻没新排入的事件一直算忙。集合每刻比较一次，绝大多数刻
        // 两边都是空的，只有一次集合相等判断。
        for &chunk in queued {
            mark(chunk, &mut added);
        }
        added
    }

    /// 推进到 `now`：安静满 `quiet_ticks` 刻的区块退出。没变化时是常数时间。
    pub(crate) fn advance(&mut self, now: i64) -> Vec<ChunkPos> {
        let mut gone = Vec::new();
        self.last_active.retain(|&chunk, &mut last| {
            if last.saturating_add(self.quiet_ticks) >= now {
                true
            } else {
                gone.push(chunk);
                false
            }
        });
        gone
    }

    /// 按坐标升序的忙区块。
    pub(crate) fn chunks(&self) -> BTreeSet<ChunkPos> {
        self.last_active.keys().copied().collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.last_active.len()
    }
}

/// 睡眠方块实体的跨区块关注关系：按区块去重后升序返回。
pub(crate) fn watcher_pairs(entities: &BlockEntities) -> Vec<(ChunkPos, ChunkPos)> {
    entities.watcher_pairs()
}

/// 切比雪夫距离 ≤ radius 的邻居，不含自己。半径是客户端给不了的小整数，
/// 坐标用饱和算术，不让极端坐标触发溢出。
fn neighbours(chunk: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    let mut out = Vec::with_capacity((2 * radius + 1) as usize * (2 * radius + 1) as usize - 1);
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx == 0 && dz == 0 {
                continue;
            }
            out.push(ChunkPos::new(
                chunk.x.saturating_add(dx),
                chunk.z.saturating_add(dz),
            ));
        }
    }
    out
}

fn area_of(busy: &BTreeSet<ChunkPos>, halo: i32) -> BTreeSet<ChunkPos> {
    let mut out = BTreeSet::new();
    for &chunk in busy {
        for dx in -halo..=halo {
            for dz in -halo..=halo {
                out.insert(ChunkPos::new(
                    chunk.x.saturating_add(dx),
                    chunk.z.saturating_add(dz),
                ));
            }
        }
    }
    out
}

fn extra_edges(
    watchers: &[(ChunkPos, ChunkPos)],
    busy: &BTreeSet<ChunkPos>,
) -> HashMap<ChunkPos, Vec<ChunkPos>> {
    let mut out: HashMap<ChunkPos, Vec<ChunkPos>> = HashMap::new();
    for &(a, b) in watchers {
        // 只有两端都忙时这条边才影响划分；安静的那端不需要强行连接。
        if a != b && busy.contains(&a) && busy.contains(&b) {
            out.entry(a).or_default().push(b);
            out.entry(b).or_default().push(a);
        }
    }
    for edges in out.values_mut() {
        edges.sort_unstable();
        edges.dedup();
    }
    out
}

/// 关注关系可能把相距很远的两个区块连起来，两个单元的缓冲圈因此可能相交。
/// 这在不变量里不允许，所以最后把范围相交的单元并起来，直到互不相交。
fn merge_overlaps(mut groups: Vec<BTreeSet<ChunkPos>>, halo: i32) -> Vec<BTreeSet<ChunkPos>> {
    loop {
        let mut merged_any = false;
        let mut parent: Vec<usize> = (0..groups.len()).collect();
        fn find(parent: &[usize], mut x: usize) -> usize {
            while parent[x] != x {
                x = parent[x];
            }
            x
        }
        let areas: Vec<BTreeSet<ChunkPos>> =
            groups.iter().map(|group| area_of(group, halo)).collect();
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                if areas[i].iter().any(|chunk| areas[j].contains(chunk)) {
                    let (a, b) = (find(&parent, i), find(&parent, j));
                    if a != b {
                        parent[b] = a;
                        merged_any = true;
                    }
                }
            }
        }
        if !merged_any {
            return groups;
        }
        let mut by_root: BTreeMap<usize, BTreeSet<ChunkPos>> = BTreeMap::new();
        for (index, group) in groups.iter_mut().enumerate() {
            let root = find(&parent, index);
            by_root
                .entry(root)
                .or_default()
                .extend(std::mem::take(group));
        }
        // `areas` 只在上面用，下一轮重新算。
        drop(areas);
        groups = by_root.into_values().collect();
    }
}

/// 朴素的对照实现：把忙区块按邻接 + 关注关系连成连通块，每个块加缓冲圈。
/// 只在测试和一个调试开关下调用，不在 tick 热路径上。
pub(crate) fn naive_partition(
    busy: &BTreeSet<ChunkPos>,
    watchers: &[(ChunkPos, ChunkPos)],
    halo: i32,
) -> Vec<Unit> {
    let halo = halo.max(1);
    let radius = halo.saturating_mul(2);
    let extra = extra_edges(watchers, busy);
    let mut seen: HashSet<ChunkPos> = HashSet::new();
    let mut groups: Vec<BTreeSet<ChunkPos>> = Vec::new();
    for &start in busy {
        if !seen.insert(start) {
            continue;
        }
        let mut group = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            group.insert(at);
            for other in neighbours(at, radius) {
                if busy.contains(&other) && seen.insert(other) {
                    stack.push(other);
                }
            }
            if let Some(edges) = extra.get(&at) {
                for &other in edges {
                    if seen.insert(other) {
                        stack.push(other);
                    }
                }
            }
        }
        groups.push(group);
    }
    let mut units: Vec<Unit> = merge_overlaps(groups, halo)
        .into_iter()
        .map(|busy| Unit {
            area: area_of(&busy, halo),
            busy,
            halo,
        })
        .collect();
    units.sort_by(|a, b| a.busy.cmp(&b.busy));
    units
}

/// 增量划分。加入时并联周围的单元，退出 / 关注关系变化时只把受影响的旧单元
/// 拆开重算。每一刻可和 [`naive_partition`] 对照（默认关）。
#[derive(Debug, Clone)]
pub(crate) struct Partition {
    halo: i32,
    radius: i32,
    /// 每个忙区块所属的单元下标。
    owner: HashMap<ChunkPos, usize>,
    /// 下标 → 单元；删除时留下空槽供复用。
    units: Vec<Option<Unit>>,
    watchers: Vec<(ChunkPos, ChunkPos)>,
    topology: Topology,
    /// 调试开关：每次 update 全量重算并断言和增量结果相同。
    verify: bool,
    pub(crate) verify_nanos: u64,
    /// 改坏确认用：故意漏掉一次合并，让全量对照把它报出来。
    #[cfg(test)]
    pub(crate) skip_merge_once: bool,
}

impl Partition {
    pub(crate) fn new(halo: i32) -> Self {
        let halo = halo.max(1);
        Self {
            halo,
            radius: halo.saturating_mul(2),
            owner: HashMap::new(),
            units: Vec::new(),
            watchers: Vec::new(),
            topology: Topology::default(),
            verify: false,
            verify_nanos: 0,
            #[cfg(test)]
            skip_merge_once: false,
        }
    }

    pub(crate) fn set_verify(&mut self, on: bool) {
        self.verify = on;
    }

    /// 取走本刻的合并 / 拆分次数。
    pub(crate) fn take_topology(&mut self) -> Topology {
        std::mem::take(&mut self.topology)
    }

    pub(crate) fn units(&self) -> Vec<Unit> {
        self.units.iter().flatten().cloned().collect()
    }

    /// 触发所在的单元（按忙区块判定）。`None` 是设计 4.3 的第 2 种越界。
    pub(crate) fn unit_for(&self, chunk: ChunkPos) -> Option<&Unit> {
        let index = *self.owner.get(&chunk)?;
        self.units.get(index)?.as_ref()
    }

    /// 单元下标，用来把一次触发的耗时按单元累计。划分会变，所以下标只在
    /// 同一刻内有效；报告每刻清零。
    pub(crate) fn unit_index_for(&self, chunk: ChunkPos) -> Option<usize> {
        self.owner.get(&chunk).copied()
    }

    /// 按本刻忙集合的变化更新划分。`added` / `removed` 通常为空；全空且关注
    /// 关系没变时这里直接返回。
    pub(crate) fn update(
        &mut self,
        busy: &BTreeSet<ChunkPos>,
        watchers: &[(ChunkPos, ChunkPos)],
        added: &[ChunkPos],
        removed: &[ChunkPos],
    ) {
        let watchers_changed = watchers != self.watchers.as_slice();
        if added.is_empty() && removed.is_empty() && !watchers_changed {
            self.verify_nanos = 0;
            return;
        }
        let mut affected: BTreeSet<usize> = BTreeSet::new();
        for &chunk in removed {
            if let Some(index) = self.owner.remove(&chunk) {
                affected.insert(index);
            }
        }
        if watchers_changed {
            // 旧的关注边和新的关注边两端所在的单元都要重算。
            for pairs in [&self.watchers, watchers] {
                for &(a, b) in pairs {
                    if let Some(&index) = self.owner.get(&a) {
                        affected.insert(index);
                    }
                    if let Some(&index) = self.owner.get(&b) {
                        affected.insert(index);
                    }
                }
            }
            self.watchers = watchers.to_vec();
        }
        // 收起受影响的旧单元成员，清空槽位；再把这些成员和新增区块一起放回。
        let mut pending: BTreeSet<ChunkPos> = added.iter().copied().collect();
        for &index in &affected {
            if let Some(Some(unit)) = self.units.get(index) {
                for &chunk in &unit.busy {
                    self.owner.remove(&chunk);
                    if busy.contains(&chunk) {
                        pending.insert(chunk);
                    }
                }
            }
            if let Some(slot) = self.units.get_mut(index) {
                *slot = None;
            }
        }
        for &chunk in removed {
            pending.remove(&chunk);
        }
        let before = self.units.iter().filter(|unit| unit.is_some()).count();
        let written = self.insert_groups(pending, busy);
        let after = self.units.iter().filter(|unit| unit.is_some()).count();
        if after < before {
            self.topology.merges = self.topology.merges.saturating_add((before - after) as u64);
        } else if after > before {
            self.topology.splits = self.topology.splits.saturating_add((after - before) as u64);
        }
        if self.verify {
            let start = std::time::Instant::now();
            let expected = naive_partition(busy, &self.watchers, self.halo);
            self.verify_nanos = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            let mut actual = self.units();
            actual.sort_by(|a, b| a.busy.cmp(&b.busy));
            assert_eq!(actual, expected, "增量划分和全量划分不一致");
        } else {
            self.verify_nanos = 0;
        }
    }

    /// 把一批区块放回划分：从每个候选出发 BFS，相邻的候选 / 已有单元 /
    /// 关注边的另一端都并成一组。返回写下的组数。
    fn insert_groups(&mut self, pending: BTreeSet<ChunkPos>, busy: &BTreeSet<ChunkPos>) -> usize {
        let extra = extra_edges(&self.watchers, busy);
        let mut written = 0;
        for &start in &pending {
            if self.owner.contains_key(&start) {
                continue;
            }
            let mut members = BTreeSet::new();
            let mut queue = VecDeque::from([start]);
            let mut seen: HashSet<ChunkPos> = [start].into_iter().collect();
            let mut absorbed: BTreeSet<usize> = BTreeSet::new();
            while let Some(at) = queue.pop_front() {
                if !members.insert(at) {
                    continue;
                }
                let mut visit = neighbours(at, self.radius);
                if let Some(edges) = extra.get(&at) {
                    visit.extend(edges.iter().copied());
                }
                for other in visit {
                    if !busy.contains(&other) {
                        continue;
                    }
                    if let Some(&index) = self.owner.get(&other) {
                        // 这个邻居属于一个还没动的已有单元：整个单元并进来。
                        #[cfg(test)]
                        if std::mem::take(&mut self.skip_merge_once) {
                            continue;
                        }
                        if absorbed.insert(index)
                            && let Some(Some(unit)) = self.units.get(index)
                        {
                            for &chunk in &unit.busy {
                                if seen.insert(chunk) {
                                    queue.push_back(chunk);
                                }
                            }
                        }
                    } else if seen.insert(other) {
                        queue.push_back(other);
                    }
                }
            }
            for &index in &absorbed {
                if let Some(Some(unit)) = self.units.get(index) {
                    for &chunk in &unit.busy {
                        self.owner.remove(&chunk);
                    }
                }
                if let Some(slot) = self.units.get_mut(index) {
                    *slot = None;
                }
            }
            for &chunk in &members {
                self.owner.remove(&chunk);
            }
            self.write_group(members);
            written += 1;
        }
        // 关注边可能让两个组的范围相交，按和朴素实现同样的规则并起来。
        self.merge_overlapping_units();
        written
    }

    fn write_group(&mut self, members: BTreeSet<ChunkPos>) {
        if members.is_empty() {
            return;
        }
        let area = area_of(&members, self.halo);
        let index = self
            .units
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.units.len());
        let unit = Unit {
            busy: members,
            area,
            halo: self.halo,
        };
        for &chunk in &unit.busy {
            self.owner.insert(chunk, index);
        }
        if index == self.units.len() {
            self.units.push(Some(unit));
        } else {
            self.units[index] = Some(unit);
        }
    }

    /// 范围相交的单元并成一个。只在刚写下多个组时调用，通常一次也不进来。
    fn merge_overlapping_units(&mut self) {
        loop {
            let indices: Vec<usize> = self
                .units
                .iter()
                .enumerate()
                .filter_map(|(index, unit)| unit.as_ref().map(|_| index))
                .collect();
            let mut pair = None;
            'outer: for (i, &a) in indices.iter().enumerate() {
                for &b in &indices[i + 1..] {
                    let (Some(Some(left)), Some(Some(right))) =
                        (self.units.get(a), self.units.get(b))
                    else {
                        continue;
                    };
                    if left.area.iter().any(|chunk| right.area.contains(chunk)) {
                        pair = Some((a, b));
                        break 'outer;
                    }
                }
            }
            let Some((a, b)) = pair else {
                return;
            };
            let left = self.units[a].take().expect("上面判过");
            let right = self.units[b].take().expect("上面判过");
            let mut members = left.busy;
            members.extend(right.busy);
            for &chunk in &members {
                self.owner.remove(&chunk);
            }
            self.write_group(members);
        }
    }

    /// 不同单元的范围互不重叠（设计稿 4.1 的不变量）。测试每刻断言这一条。
    pub(crate) fn areas_disjoint(&self) -> bool {
        let mut seen: HashSet<ChunkPos> = HashSet::new();
        for unit in self.units.iter().flatten() {
            for &chunk in &unit.area {
                if !seen.insert(chunk) {
                    return false;
                }
            }
        }
        true
    }
}

/// 一个区块到某个单元范围的最短切比雪夫距离；在范围内是 0。
/// 越界统计用「碰到范围外区块时离范围多远」的这个数（按区块算）。
pub(crate) fn distance_to_area(unit: &Unit, touched: ChunkPos) -> i32 {
    let mut best = i32::MAX;
    for &chunk in &unit.area {
        let distance = (touched.x.saturating_sub(chunk.x))
            .abs()
            .max((touched.z.saturating_sub(chunk.z)).abs());
        best = best.min(distance);
    }
    best.clamp(0, i32::MAX - 1)
}

/// 挂在 `Region` 上的划分状态。`enabled` 关着时世界循环只看这一个布尔值，
/// 不建任何表、不读环境变量、不碰方块逻辑。
#[derive(Debug, Default)]
pub(crate) struct Partitioning {
    pub(crate) enabled: bool,
    pub(crate) verify: bool,
    pub(crate) busy: BusyChunks,
    /// `halo = 1 / 2` 各维护一份，一次跑同时量两套越界。
    pub(crate) partitions: Vec<Partition>,
    pub(crate) now: i64,
}

impl Partitioning {
    /// 打开统计。`halos` 由调用方给（默认 [1, 2]）；`verify` 打开每刻全量对照。
    pub(crate) fn enable(&mut self, halos: &[i32], verify: bool, quiet_ticks: i64) {
        self.enabled = true;
        self.verify = verify;
        self.busy = BusyChunks::new(quiet_ticks);
        self.partitions = halos
            .iter()
            .map(|&halo| {
                let mut partition = Partition::new(halo);
                partition.set_verify(verify);
                partition
            })
            .collect();
    }

    /// 一个世界刻的划分阶段。返回本刻的耗时（纳秒）。
    ///
    /// 三个阶段仍照常串行执行：这里只更新忙区块、单元和统计，不改执行顺序。
    /// 活动增量由调用方 drain 后传进来，这样「没有变化的那一刻」只付
    /// 三次清空 Vec 和一次过期检查。
    pub(crate) fn tick(
        &mut self,
        now: i64,
        touched_ticks: Vec<ChunkPos>,
        touched_events: Vec<ChunkPos>,
        awake: &BTreeSet<ChunkPos>,
        queued: &BTreeSet<ChunkPos>,
        watchers: &[(ChunkPos, ChunkPos)],
    ) -> u64 {
        let start = std::time::Instant::now();
        self.now = now;
        let added = self
            .busy
            .observe(now, &touched_ticks, &touched_events, awake, queued);
        let removed = self.busy.advance(now);
        let busy = self.busy.chunks();
        for partition in &mut self.partitions {
            partition.update(&busy, watchers, &added, &removed);
        }
        start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    /// 本刻全量对照的耗时（纳秒）。`verify` 关着时是 0。
    pub(crate) fn verify_nanos(&self) -> u64 {
        self.partitions.iter().map(|p| p.verify_nanos).sum()
    }

    /// 本刻的合并 / 拆分次数（halo=1 那份）。
    pub(crate) fn topology(&mut self) -> Topology {
        self.partitions
            .first_mut()
            .map_or(Topology::default(), Partition::take_topology)
    }

    pub(crate) fn units(&self) -> Vec<Unit> {
        self.partitions.first().map_or(Vec::new(), Partition::units)
    }

    pub(crate) fn busy_len(&self) -> usize {
        self.busy.len()
    }
}
