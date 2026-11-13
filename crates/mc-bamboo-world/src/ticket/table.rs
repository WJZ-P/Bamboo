//! `TicketTable`：给定票的增减，算出每个区块的加载等级和模拟等级。

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::Entry},
    hash::{BuildHasherDefault, Hasher},
};

use mc_bamboo_core::ChunkPos;

use super::{
    level::{
        DEFAULT_SIMULATION_DISTANCE, FULL_CHUNK_LEVEL, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE,
        PLAYER_TICKET_LEVEL, SIMULATION_MAX_LEVEL, TicketType, UNLOADED_LEVEL,
    },
    profile::{
        FOLD, Guard, LOADING_BFS, LOADING_COLLECT, LOADING_RELAX, LOADING_SEEDS, MERGE, PENDING,
        SIMULATION_BFS, SIMULATION_COLLECT, SIMULATION_RELAX, SIMULATION_SEEDS,
    },
    spread::Plane,
};

/// 区块坐标的便宜哈希。等级表有几十万项，坐标是连续的小整数，
/// 标准库的 SipHash 每次查找太贵；这里把两个 32 位坐标混成一个 64 位值。
#[derive(Default)]
pub struct ChunkHasher(u64);

impl Hasher for ChunkHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        // ChunkPos 只会通过 write_i32 进来；保留通用路径以免 Hash 实现变化时出错。
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_i32(&mut self, value: i32) {
        let mixed = (value as u32 as u64)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .rotate_left(29);
        self.0 = (self.0 ^ mixed).wrapping_mul(0xff51_afd7_ed55_8ccd);
        self.0 ^= self.0 >> 33;
    }
}

/// `HashMap<ChunkPos, _>` 用的构造器。
pub type ChunkHasherBuilder = BuildHasherDefault<ChunkHasher>;

/// 一张票，对应官方 `Ticket` 的三个字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticket {
    pub ty: TicketType,
    pub level: i32,
    /// 剩余刻数；没有超时的票恒为 0，`add` 时重置为 `ty.timeout()`。
    pub ticks_left: i32,
}

impl Ticket {
    pub fn new(ty: TicketType, level: i32) -> Self {
        Self {
            ty,
            level,
            ticks_left: ty.timeout() as i32,
        }
    }
}

/// 一条要存盘的票（`TicketStorage.packTickets` 的一项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SavedTicket {
    pub pos: ChunkPos,
    pub ty: TicketType,
    pub level: i32,
    pub ticks_left: i32,
}

/// 一次 `run_updates` 里某个区块的等级变化，两个分量分别是 (旧, 新)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LevelChange {
    pub pos: ChunkPos,
    pub loading: (i32, i32),
    pub simulation: (i32, i32),
}

/// 两张平面的等级表。
///
/// 线程、区块内容、IO 都不归这里管。调用方（以后的 Director）负责把客户端发来的
/// 视距夹到合法范围；本模块自己也会再夹一次，越界输入不会 panic。
///
/// 不变量：
/// - `tickets` 里同一位置不会有两张 `(ty, level)` 相同的票；
/// - 平面的源等级分别是 `tickets` 中带加载 / 模拟标志的最小等级；
/// - 平面等级是这个源集合经过 8 邻 +1 取最小后的不动点；
/// - 每个区块的 `player_loading` 引用计数等于视距方形覆盖它的玩家数；
///   `player_simulation` 引用计数等于站在该区块的玩家数；
/// - 任何公开输出都经过排序，不依赖哈希图的遍历顺序。
#[derive(Debug)]
pub struct TicketTable {
    tickets: HashMap<ChunkPos, Vec<Ticket>, ChunkHasherBuilder>,
    loading: Plane,
    simulation: Plane,
    /// 位置 → 该位置同标志票的最小等级（用各自的 unloaded 表示没有）。
    loading_sources: HashMap<ChunkPos, i32, ChunkHasherBuilder>,
    simulation_sources: HashMap<ChunkPos, i32, ChunkHasherBuilder>,
    /// 攒着的源变化，等 `run_updates` 一起传播。
    pending: Vec<(ChunkPos, i32, i32, bool)>,
    /// 按平面分好的源变化，跨轮复用容量。
    loading_changes: Vec<(ChunkPos, i32, i32)>,
    simulation_changes: Vec<(ChunkPos, i32, i32)>,
    /// 攒着的等级变化，`run_updates` 返回它。
    changes: Vec<LevelChange>,
    players: BTreeMap<u64, ChunkPos>,
    /// 每个区块有多少名玩家的视距覆盖它。
    player_count: HashMap<ChunkPos, u32, ChunkHasherBuilder>,
    /// 每个区块有多少名玩家站在里面。
    simulation_count: HashMap<ChunkPos, u32, ChunkHasherBuilder>,
    view_distance: i32,
    simulation_distance: i32,
    /// 有带超时票的位置，避免每刻扫全部票。
    timed: BTreeSet<ChunkPos>,
}

impl Default for TicketTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TicketTable {
    /// 空表：视距 10、模拟距离 10，照 `DistanceManager` 的初值。
    pub fn new() -> Self {
        Self {
            tickets: HashMap::default(),
            loading: Plane::new(UNLOADED_LEVEL),
            simulation: Plane::new(SIMULATION_MAX_LEVEL),
            loading_sources: HashMap::default(),
            simulation_sources: HashMap::default(),
            pending: Vec::new(),
            loading_changes: Vec::new(),
            simulation_changes: Vec::new(),
            changes: Vec::new(),
            players: BTreeMap::new(),
            player_count: HashMap::default(),
            simulation_count: HashMap::default(),
            view_distance: 10,
            simulation_distance: DEFAULT_SIMULATION_DISTANCE,
            timed: BTreeSet::new(),
        }
    }

    /// 全服视距（`ChunkMap.setServerViewDistance` 夹到 2..=32）。
    pub const fn view_distance(&self) -> i32 {
        self.view_distance
    }

    /// 模拟距离。
    pub const fn simulation_distance(&self) -> i32 {
        self.simulation_distance
    }

    /// 加载等级：无票是 45。
    pub fn loading_level(&self, pos: ChunkPos) -> i32 {
        self.loading.level(pos)
    }

    /// 模拟等级：无票是 33。
    pub fn simulation_level(&self, pos: ChunkPos) -> i32 {
        self.simulation.level(pos)
    }

    /// 等级表里存了多少个非默认等级的区块（两张平面合计）。
    pub fn level_count(&self) -> usize {
        self.loading.len() + self.simulation.len()
    }

    /// 传播用的辅助结构占了多少个块；公开给测试检查长期移动不会让内存一直涨。
    #[doc(hidden)]
    pub fn scratch_blocks(&self) -> usize {
        self.loading.scratch_blocks() + self.simulation.scratch_blocks()
    }

    /// 全部有票的区块，按坐标排序。
    pub fn ticket_positions(&self) -> Vec<ChunkPos> {
        let mut out: Vec<_> = self.tickets.keys().copied().collect();
        out.sort_unstable();
        out
    }

    /// 某个位置的全部票，按 (类型, 等级) 排序。
    pub fn tickets_at(&self, pos: ChunkPos) -> Vec<Ticket> {
        let Some(list) = self.tickets.get(&pos) else {
            return Vec::new();
        };
        let mut out = list.clone();
        out.sort_unstable();
        out
    }

    /// 有没有带"保持维度活跃"标志的票（`TicketStorage.shouldKeepDimensionActive`）。
    pub fn keep_dimension_active(&self) -> bool {
        self.tickets
            .values()
            .flat_map(|list| list.iter())
            .any(|ticket| ticket.ty.keeps_dimension_active())
    }

    /// 全部要存盘的票，按 (坐标, 类型, 等级) 排序。
    pub fn persistent_tickets(&self) -> Vec<SavedTicket> {
        let mut out = Vec::new();
        for (&pos, list) in &self.tickets {
            for ticket in list {
                if ticket.ty.persists() {
                    out.push(SavedTicket {
                        pos,
                        ty: ticket.ty,
                        level: ticket.level,
                        ticks_left: ticket.ticks_left,
                    });
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// 把一批存盘的票装回去（`TicketStorage.fromPacked` 的等价物）。
    ///
    /// 已经存在的同 (类型, 等级) 票按官方 `addTicket` 只重置超时，不重复插入。
    /// 非存盘类型会被忽略。
    pub fn restore_tickets(&mut self, saved: impl IntoIterator<Item = SavedTicket>) {
        let mut items: Vec<_> = saved.into_iter().filter(|s| s.ty.persists()).collect();
        items.sort_unstable();
        for item in items {
            self.add_with_ticks(item.ty, item.pos, item.level, item.ticks_left);
        }
    }

    /// 加一张票。`add` 同 (类型, 等级) 的票只重置超时（官方 `TicketStorage.addTicket`）。
    pub fn add(&mut self, ty: TicketType, pos: ChunkPos, level: i32) {
        self.add_with_ticks(ty, pos, level, ty.timeout() as i32);
    }

    /// `TicketStorage.addTicketWithRadius`：`level = 33 - radius`。
    pub fn add_with_radius(&mut self, ty: TicketType, pos: ChunkPos, radius: i32) {
        self.add(ty, pos, FULL_CHUNK_LEVEL.saturating_sub(radius));
    }

    /// `TicketStorage.removeTicketWithRadius`。
    pub fn remove_with_radius(&mut self, ty: TicketType, pos: ChunkPos, radius: i32) {
        self.remove(ty, pos, FULL_CHUNK_LEVEL.saturating_sub(radius));
    }

    /// 撤一张票；只删第一张同 (类型, 等级) 的票（官方 `TicketStorage.removeTicket`）。
    pub fn remove(&mut self, ty: TicketType, pos: ChunkPos, level: i32) {
        let Some(list) = self.tickets.get_mut(&pos) else {
            return;
        };
        let Some(index) = list
            .iter()
            .position(|ticket| ticket.ty == ty && ticket.level == level)
        else {
            return;
        };
        list.remove(index);
        let empty = list.is_empty();
        if empty {
            self.tickets.remove(&pos);
        }
        self.recompute_sources(pos);
        self.refresh_timed(pos);
    }

    /// `/forceload`：`TicketStorage.updateChunkForced`。
    pub fn set_forced(&mut self, pos: ChunkPos, forced: bool) {
        if forced {
            self.add(TicketType::Forced, pos, PLAYER_TICKET_LEVEL);
        } else {
            self.remove(TicketType::Forced, pos, PLAYER_TICKET_LEVEL);
        }
    }

    /// `TicketStorage.getForceLoadedChunks`，按坐标排序。
    pub fn forced_chunks(&self) -> Vec<ChunkPos> {
        let mut out: Vec<_> = self
            .tickets
            .iter()
            .filter(|(_, list)| list.iter().any(|ticket| ticket.ty == TicketType::Forced))
            .map(|(&pos, _)| pos)
            .collect();
        out.sort_unstable();
        out
    }

    /// 全服视距变化：`ChunkMap.setServerViewDistance` → `PlayerTicketTracker.updateViewDistance`。
    pub fn set_view_distance(&mut self, distance: i32) {
        let distance = distance.clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
        if distance == self.view_distance {
            return;
        }
        let old_distance = self.view_distance;
        self.view_distance = distance;
        // 每名玩家的方形只在两个半径之间差几圈，不重算内部。
        let centers: Vec<ChunkPos> = self.players.values().copied().collect();
        for center in centers {
            if distance > old_distance {
                self.adjust_ring(center, old_distance + 1, distance, true);
            } else {
                self.adjust_ring(center, distance + 1, old_distance, false);
            }
        }
    }

    /// `DistanceManager.updateSimulationDistance` → `TicketStorage.replaceTicketLevelOfType`：
    /// 把**全部** `player_simulation` 票换成新等级，不只是玩家所在区块的那些。
    pub fn set_simulation_distance(&mut self, distance: i32) {
        // 官方没夹这个值，这里夹到 [0, 31] 以免负数等级或溢出。
        let distance = distance.clamp(0, PLAYER_TICKET_LEVEL);
        if distance == self.simulation_distance {
            return;
        }
        let level = PLAYER_TICKET_LEVEL - distance;
        self.simulation_distance = distance;
        let positions: Vec<(ChunkPos, Vec<i32>)> = self
            .ticket_positions()
            .into_iter()
            .map(|pos| {
                let levels: Vec<i32> = self
                    .tickets_at(pos)
                    .into_iter()
                    .filter(|ticket| ticket.ty == TicketType::PlayerSimulation)
                    .map(|ticket| ticket.level)
                    .collect();
                (pos, levels)
            })
            .filter(|(_, levels)| !levels.is_empty())
            .collect();
        for (pos, levels) in positions {
            for old in levels {
                self.remove(TicketType::PlayerSimulation, pos, old);
            }
            self.add(TicketType::PlayerSimulation, pos, level);
        }
    }

    /// 玩家进服、离开或跨区块。`None` 表示离开。
    ///
    /// 与官方不同的是这里同步给满整个方形视野：官方的 `player_loading` 票增删走
    /// `ThrottlingChunkTaskDispatcher`（每刻 4 个在途），属于加载调度而不是票的语义，
    /// 归以后的 Director。稳定状态与官方逐位一致。
    pub fn set_player(&mut self, id: u64, section: Option<ChunkPos>) {
        let previous = self.players.get(&id).copied();
        match (previous, section) {
            (None, None) | (Some(_), Some(_)) if previous == section => {}
            (Some(old), Some(new)) => {
                self.players.insert(id, new);
                self.move_player(old, new);
            }
            (Some(old), None) => {
                self.players.remove(&id);
                self.adjust_square(old, self.view_distance, false);
                self.decrement_simulation(old);
            }
            (None, Some(new)) => {
                self.players.insert(id, new);
                self.adjust_square(new, self.view_distance, true);
                self.increment_simulation(new);
            }
            (None, None) => {}
        }
    }

    /// 每刻的超时递减和清理，照 `TicketStorage.purgeStaleTickets`。
    ///
    /// `is_busy(pos)` 为真表示"这个区块存在、还没准备好存盘"（对应官方
    /// `ChunkHolder.isReadyForSaving() == false` 且 holder 非 null）。这种区块上
    /// 不带 `CAN_EXPIRE_IF_UNLOADED` 的超时票这一刻**不递减**。
    pub fn tick(&mut self, is_busy: &mut dyn FnMut(ChunkPos) -> bool) {
        let positions: Vec<ChunkPos> = self.timed.iter().copied().collect();
        for pos in positions {
            let busy = is_busy(pos);
            let expired: Vec<(TicketType, i32)> = {
                let Some(list) = self.tickets.get_mut(&pos) else {
                    continue;
                };
                let mut expired = Vec::new();
                for ticket in list.iter_mut() {
                    if !ticket.ty.has_timeout() || (busy && !ticket.ty.can_expire_if_unloaded()) {
                        continue;
                    }
                    ticket.ticks_left -= 1;
                    if ticket.ticks_left < 0 {
                        expired.push((ticket.ty, ticket.level));
                    }
                }
                expired
            };
            for (ty, level) in expired {
                self.remove(ty, pos, level);
            }
            self.refresh_timed(pos);
        }
    }

    /// 所有区块都可存盘时的便捷方法（官方空闲时的行为）。
    pub fn tick_idle(&mut self) {
        self.tick(&mut |_| false);
    }

    /// 把积压的源变化传播完，返回这次等级变了的区块。
    ///
    /// 顺序按 (x, z) 排序，且恰好包含等级变了的区块，不多不少。
    /// 返回的切片只在下一次 `run_updates` 之前有效；长期持有请复制。
    pub fn run_updates(&mut self) -> &[LevelChange] {
        self.changes.clear();
        if self.pending.is_empty() {
            self.loading.note_idle_round();
            self.simulation.note_idle_round();
            return &self.changes;
        }
        let _pending_guard = Guard::new(PENDING);
        let mut loading_changes = std::mem::take(&mut self.loading_changes);
        let mut simulation_changes = std::mem::take(&mut self.simulation_changes);
        loading_changes.clear();
        simulation_changes.clear();
        for &(pos, old, new, simulation) in &self.pending {
            if simulation {
                simulation_changes.push((pos, old, new));
            } else {
                loading_changes.push((pos, old, new));
            }
        }
        self.pending.clear();
        drop(_pending_guard);
        let loading = self.loading.update(
            &loading_changes,
            [
                LOADING_BFS,
                LOADING_SEEDS,
                LOADING_RELAX,
                LOADING_COLLECT,
                FOLD,
            ],
        );
        let simulation = self.simulation.update(
            &simulation_changes,
            [
                SIMULATION_BFS,
                SIMULATION_SEEDS,
                SIMULATION_RELAX,
                SIMULATION_COLLECT,
                FOLD,
            ],
        );
        let _merge_guard = Guard::new(MERGE);
        // 两个分量里没变的那一个也要写 (新, 新)，调用方直接拿 (旧, 新) 用。
        // 两张平面的变化列表都已按 (x, z) 排好，线性归并即可，不用有序映射。
        let mut a = loading.into_iter().peekable();
        let mut b = simulation.into_iter().peekable();
        loop {
            let (pos, loading_change, simulation_change) = match (a.peek(), b.peek()) {
                (Some(&(left, _, _)), Some(&(right, _, _))) => {
                    match (left.x, left.z).cmp(&(right.x, right.z)) {
                        std::cmp::Ordering::Less => {
                            let (pos, old, new) = a.next().unwrap();
                            (pos, Some((old, new)), None)
                        }
                        std::cmp::Ordering::Greater => {
                            let (pos, old, new) = b.next().unwrap();
                            (pos, None, Some((old, new)))
                        }
                        std::cmp::Ordering::Equal => {
                            let (pos, lo, ln) = a.next().unwrap();
                            let (_, so, sn) = b.next().unwrap();
                            (pos, Some((lo, ln)), Some((so, sn)))
                        }
                    }
                }
                (Some(_), None) => {
                    let (pos, old, new) = a.next().unwrap();
                    (pos, Some((old, new)), None)
                }
                (None, Some(_)) => {
                    let (pos, old, new) = b.next().unwrap();
                    (pos, None, Some((old, new)))
                }
                (None, None) => break,
            };
            let loading = loading_change.unwrap_or_else(|| {
                let level = self.loading.level(pos);
                (level, level)
            });
            let simulation = simulation_change.unwrap_or_else(|| {
                let level = self.simulation.level(pos);
                (level, level)
            });
            self.changes.push(LevelChange {
                pos,
                loading,
                simulation,
            });
        }
        drop(_merge_guard);
        self.loading_changes = loading_changes;
        self.simulation_changes = simulation_changes;
        &self.changes
    }

    /// 还有没有等着传播的票变化。
    pub fn has_pending_updates(&self) -> bool {
        !self.pending.is_empty()
    }

    // ---- 内部 ----

    fn add_with_ticks(&mut self, ty: TicketType, pos: ChunkPos, level: i32, ticks_left: i32) {
        let list = self.tickets.entry(pos).or_default();
        if let Some(existing) = list
            .iter_mut()
            .find(|ticket| ticket.ty == ty && ticket.level == level)
        {
            existing.ticks_left = ty.timeout() as i32;
            return;
        }
        list.push(Ticket {
            ty,
            level,
            ticks_left: if ty.has_timeout() {
                ticks_left.max(0)
            } else {
                0
            },
        });
        self.recompute_sources(pos);
        self.refresh_timed(pos);
    }

    /// 一个位置的票变了，重算它作为两张平面源的等级。
    fn recompute_sources(&mut self, pos: ChunkPos) {
        let (loading, simulation) = self.min_levels(pos);
        self.set_source(pos, loading, false);
        self.set_source(pos, simulation, true);
    }

    fn min_levels(&self, pos: ChunkPos) -> (i32, i32) {
        let mut loading = UNLOADED_LEVEL;
        let mut simulation = UNLOADED_LEVEL;
        if let Some(list) = self.tickets.get(&pos) {
            for ticket in list {
                if ticket.ty.loads() {
                    loading = loading.min(ticket.level.max(0));
                }
                if ticket.ty.simulates() {
                    simulation = simulation.min(ticket.level.max(0));
                }
            }
        }
        (loading, simulation)
    }

    fn set_source(&mut self, pos: ChunkPos, level: i32, simulation: bool) {
        let unloaded = if simulation {
            SIMULATION_MAX_LEVEL
        } else {
            UNLOADED_LEVEL
        };
        let sources = if simulation {
            &mut self.simulation_sources
        } else {
            &mut self.loading_sources
        };
        let old = sources.get(&pos).copied().unwrap_or(unloaded);
        let new = level.min(unloaded);
        if old == new {
            return;
        }
        if new == unloaded {
            sources.remove(&pos);
        } else {
            sources.insert(pos, new);
        }
        self.pending.push((pos, old, new, simulation));
    }

    fn refresh_timed(&mut self, pos: ChunkPos) {
        let has = self
            .tickets
            .get(&pos)
            .is_some_and(|list| list.iter().any(|ticket| ticket.ty.has_timeout()));
        if has {
            self.timed.insert(pos);
        } else {
            self.timed.remove(&pos);
        }
    }

    /// 玩家跨区块：只处理两个方形视野的差集（一排进、一排出），不重算整个视野。
    fn move_player(&mut self, old: ChunkPos, new: ChunkPos) {
        let dx = (new.x as i64 - old.x as i64).unsigned_abs();
        let dz = (new.z as i64 - old.z as i64).unsigned_abs();
        let radius = self.view_distance;
        if dx > 2 * radius as u64 || dz > 2 * radius as u64 {
            // 两个方形不相交：整个旧方形出、整个新方形进。
            self.adjust_square(old, radius, false);
            self.adjust_square(new, radius, true);
        } else {
            // 按 x 逐列比较两方形的 z 区间，只有边界列/行会进出。
            for_each_diff(old, new, radius, |pos, entering| {
                self.adjust_one(pos, entering);
            });
        }
        self.decrement_simulation(old);
        self.increment_simulation(new);
    }

    /// 加/撤一整个方形（玩家进服/离开，或者两个视野完全不相交）。
    fn adjust_square(&mut self, center: ChunkPos, radius: i32, add: bool) {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                self.adjust_one(
                    ChunkPos::new(center.x.saturating_add(dx), center.z.saturating_add(dz)),
                    add,
                );
            }
        }
    }

    /// 加/撤半径 `inner..=outer` 的方环（视距变化）。四个角只算一次。
    fn adjust_ring(&mut self, center: ChunkPos, inner: i32, outer: i32, add: bool) {
        for radius in inner..=outer {
            for d in -radius..=radius {
                for &pos in &[
                    ChunkPos::new(center.x.saturating_add(d), center.z.saturating_sub(radius)),
                    ChunkPos::new(center.x.saturating_add(d), center.z.saturating_add(radius)),
                ] {
                    self.adjust_one(pos, add);
                }
            }
            // 竖边跳过两个端点，它们已经算在横边里了。
            for d in (-radius + 1)..radius {
                for &pos in &[
                    ChunkPos::new(center.x.saturating_sub(radius), center.z.saturating_add(d)),
                    ChunkPos::new(center.x.saturating_add(radius), center.z.saturating_add(d)),
                ] {
                    self.adjust_one(pos, add);
                }
            }
        }
    }

    /// 引用计数 0↔1 时才真的增删 `player_loading` 票。
    fn adjust_one(&mut self, pos: ChunkPos, add: bool) {
        if add {
            match self.player_count.entry(pos) {
                Entry::Vacant(entry) => {
                    entry.insert(1);
                    self.add(TicketType::PlayerLoading, pos, PLAYER_TICKET_LEVEL);
                }
                Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            }
        } else {
            let remove = match self.player_count.entry(pos) {
                Entry::Occupied(mut entry) => {
                    *entry.get_mut() = entry.get().saturating_sub(1);
                    if *entry.get() == 0 {
                        entry.remove();
                        true
                    } else {
                        false
                    }
                }
                Entry::Vacant(_) => false,
            };
            if remove {
                self.remove(TicketType::PlayerLoading, pos, PLAYER_TICKET_LEVEL);
            }
        }
    }

    fn increment_simulation(&mut self, pos: ChunkPos) {
        let count = self.simulation_count.entry(pos).or_insert(0);
        *count += 1;
        if *count == 1 {
            self.add(
                TicketType::PlayerSimulation,
                pos,
                PLAYER_TICKET_LEVEL - self.simulation_distance,
            );
        }
    }

    fn decrement_simulation(&mut self, pos: ChunkPos) {
        let remove = match self.simulation_count.entry(pos) {
            Entry::Occupied(mut entry) => {
                *entry.get_mut() = entry.get().saturating_sub(1);
                if *entry.get() == 0 {
                    entry.remove();
                    true
                } else {
                    false
                }
            }
            Entry::Vacant(_) => false,
        };
        if remove {
            self.remove(
                TicketType::PlayerSimulation,
                pos,
                PLAYER_TICKET_LEVEL - self.simulation_distance,
            );
        }
    }
}

/// 枚举两个同半径方形的差集，对每一项调用 `visit(pos, entering)`。
///
/// 按 x 逐列比较旧方形和新方形的 z 区间；两个区间相同就整列跳过，
/// 不同的列只枚举区间差出来的那些 z。视距 10 时跨一格只访问约 42 个区块，
/// 而不是整个 21×21 视野。
fn for_each_diff(old: ChunkPos, new: ChunkPos, radius: i32, mut visit: impl FnMut(ChunkPos, bool)) {
    let x_min = old.x.min(new.x).saturating_sub(radius);
    let x_max = old.x.max(new.x).saturating_add(radius);
    for x in x_min..=x_max {
        let in_old = x.saturating_sub(old.x).abs() <= radius;
        let in_new = x.saturating_sub(new.x).abs() <= radius;
        let old_lo = old.z.saturating_sub(radius);
        let old_hi = old.z.saturating_add(radius);
        let new_lo = new.z.saturating_sub(radius);
        let new_hi = new.z.saturating_add(radius);
        if in_old && in_new && old_lo == new_lo && old_hi == new_hi {
            continue;
        }
        let z_min = if in_old && in_new {
            old_lo.min(new_lo)
        } else if in_old {
            old_lo
        } else {
            new_lo
        };
        let z_max = if in_old && in_new {
            old_hi.max(new_hi)
        } else if in_old {
            old_hi
        } else {
            new_hi
        };
        for z in z_min..=z_max {
            let a = in_old && z >= old_lo && z <= old_hi;
            let b = in_new && z >= new_lo && z <= new_hi;
            if a != b {
                visit(ChunkPos::new(x, z), b);
            }
        }
    }
}
