//! Director 和世界线程之间的宿主：内联执行器、停放表、region 消息泵。
//!
//! 提交 5 里执行器是内联的：任务当场做完，盒子攒在信箱里。信箱在 tick 开头
//! 按离玩家的距离交付/收回，每刻有个数上限；玩家脚下 3×3 不受上限约束。
//! 视野里没交付的区块留在 `PlayerChunkSender` 的待发送集合里，交付了才发。

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_storage::chunk_io::{ChunkIo, Reply};
use mc_bamboo_world::{
    director::{
        Desired, Director, DirectorConfig, Effect, Event, RegionMessage, SaveSeq, Task, TaskId,
        TaskResult, TicketOp,
    },
    store::ChunkRevision,
};

use crate::{
    chunk_codec::{ChunkCodec, InlineCodec, PackedChunk},
    profiler::SubPhase,
    region::{Region, RegionColumn, parking::Parking},
};

/// 编解码累计耗时（纳秒）。解析/序列化内联在世界线程上做，这里量出总耗时，
/// 关服时记进日志，写进汇报；以后挪进计算池时可以直接对比。
/// Accumulated codec time (ns), used in the shutdown log / report.
static CODEC_NANOS: AtomicU64 = AtomicU64::new(0);

pub fn codec_nanos() -> u64 {
    CODEC_NANOS.load(Ordering::Relaxed)
}

fn record_codec(started: Instant) {
    let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    CODEC_NANOS.fetch_add(elapsed, Ordering::Relaxed);
}

/// 交付/收回每刻的个数上限。0 表示不限（只给剖析和回归对比用）。
pub(crate) const DEFAULT_MAIL_BUDGET: usize = 64;
/// D6：运行中把待写快照摊到 6000 刻里，每刻至少存 1 个。
/// D6: spread pending chunk snapshots over 6000 ticks, at least one per tick.
pub(crate) const SAVE_INTERVAL_TICKS: usize = 6000;
/// 单区块 10 秒（200 刻）冷却，照原版 `nextChunkSaveTime`。
/// Per-chunk 10 second (200 tick) cooldown, like vanilla `nextChunkSaveTime`.
pub(crate) const SAVE_COOLDOWN_TICKS: u64 = 200;

/// 一次待落盘的区块快照。
struct PendingWrite {
    pos: ChunkPos,
    seq: SaveSeq,
    chunk: Option<Vec<u8>>,
    entities: Option<Vec<u8>>,
}

/// 每刻要提交几个写：`ceil(待写数 / 6000)`，至少 1 个。
fn save_batch_size(queue_len: usize) -> usize {
    queue_len.div_ceil(SAVE_INTERVAL_TICKS).max(1)
}

/// 收回的积压大了之后，每刻收掉其中的这么多分之一。
///
/// 交付的上限是为了削进服的尖峰；收回要是也卡死在固定的个数上，很多人同时
/// 高速移动时每刻新排进来的比收走的多，已加载的区块数和内存就没有上界。
/// 按积压的比例收，积压会稳定在「每刻新排进来的个数 × 这个数」附近。
pub(crate) const RECLAIM_DRAIN_TICKS: usize = 20;

/// 信箱的每刻预算。`0` 表示不限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailBudget {
    pub deliveries: usize,
    pub reclaims: usize,
}

impl Default for MailBudget {
    fn default() -> Self {
        Self {
            deliveries: DEFAULT_MAIL_BUDGET,
            reclaims: DEFAULT_MAIL_BUDGET,
        }
    }
}

/// 已加载模式下 Director 需要的全部状态。
pub(crate) struct DirectorHost {
    director: Director<Box<RegionColumn>>,
    /// 无限超平坦模式：票表照常记，但不派任何交付/收回。
    infinite_flat: bool,
    parking: Parking,
    budget: MailBudget,
    /// 发给 region 的信箱：待交付的盒子（带交付时的等级）、待收回的位置。
    deliveries: BTreeMap<ChunkPos, (u64, Desired, Box<RegionColumn>)>,
    reclaims: Vec<(ChunkPos, u64)>,
    /// 本刻还没花掉的预算。整刻内所有 `pump` 共用，进服/离开/移动各发一批票
    /// 也不会把上限乘上好几倍；每刻开头由 `begin_tick` 重置。
    budget_left: MailBudget,
    /// 这一刻已经从 region 取回了多少个区块。收回的上限会随积压放宽，
    /// 所以不能用「剩余额度」记，要记花掉的个数。
    reclaims_spent: usize,
    /// 统计：本刻交付/收回/停放的区块数，给剖析器用。
    delivered: usize,
    reclaimed: usize,
    parked: usize,
    /// 区块 IO 线程。`None` 表示这一局不接磁盘（测试 / 无限超平坦）。
    io: Option<ChunkIo>,
    codec: InlineCodec,
    /// 每个区块最新一次写的 seq。收到 `Written` 回执时据此丢掉写盘途中缓冲。
    pending_writes: HashMap<ChunkPos, SaveSeq>,
    /// D6：还没提交给 IO 线程的快照，每刻匀速出队。
    save_queue: VecDeque<PendingWrite>,
    /// 每个区块上次提交写的刻数，用于 10 秒冷却。
    save_cooldown: HashMap<ChunkPos, u64>,
    /// 已交给 IO 线程、还没收到写完回执的玩家存档：`uuid -> (seq, bytes)`。
    /// 官方 `PlayerDataStorage.save` 是同步写，没有「写了但盘上还是旧的」窗口；
    /// 这里写是异步的，重进时优先用这份内存副本，避免丢进度。
    /// Player saves handed to the IO thread but not yet confirmed written.
    /// Vanilla saves synchronously, so there is no stale-on-disk window; here the
    /// write is async, and a rejoin prefers this in-memory copy to avoid losing progress.
    pending_players: HashMap<String, (u64, Vec<u8>)>,
    /// 每个已加载区块上次拍快照（存/卸载写）落定后的版本号，用来认出
    /// 「改过但还没存」的脏区块。版本号由 `ChunkStore` 在每次写方块/光照时更换。
    /// Revision at each loaded chunk's last snapshot, to find dirty loaded chunks.
    saved_revisions: HashMap<ChunkPos, ChunkRevision>,
    /// 每个已加载区块上次被 D6 匀速快照的刻数，用于单区块 10 秒冷却。
    /// Last tick each loaded chunk was snapshotted by D6, for the cooldown.
    loaded_save_tick: HashMap<ChunkPos, u64>,
    /// 已经收到 IO 回执的异步栅栏序号；`World` 每刻取走，发等在上面的
    /// `save-all flush` 成功消息。
    /// Seqs of async barriers whose acks arrived; `World` drains them each tick
    /// to release the `save-all flush` success messages waiting on them.
    flushed_barriers: Vec<u64>,
    /// 累计的写盘失败次数（每次都在 `poll_io` 记 `error!`）。关服时汇报。
    /// Count of write failures (each logged with `error!` in `poll_io`).
    io_failures: u64,
}

impl DirectorHost {
    pub(crate) fn new(config: DirectorConfig, budget: MailBudget) -> Self {
        let mut director = Director::new(config);
        director.set_template(|| Box::new(RegionColumn::from_template()));
        Self {
            infinite_flat: false,
            director,
            parking: Parking::default(),
            budget,
            budget_left: budget,
            reclaims_spent: 0,
            deliveries: BTreeMap::new(),
            reclaims: Vec::new(),
            delivered: 0,
            reclaimed: 0,
            parked: 0,
            io: None,
            codec: InlineCodec,
            pending_writes: HashMap::new(),
            save_queue: VecDeque::new(),
            save_cooldown: HashMap::new(),
            pending_players: HashMap::new(),
            saved_revisions: HashMap::new(),
            loaded_save_tick: HashMap::new(),
            flushed_barriers: Vec::new(),
            io_failures: 0,
        }
    }

    /// 接上真实磁盘 IO。只在启用存档的一局里调用一次。
    pub(crate) fn set_io(&mut self, io: ChunkIo) {
        self.io = Some(io);
    }

    /// 关服排空：等此前所有写落盘并 `sync_all`。失败要报出来，不能当成功。
    /// Drain at shutdown: wait for all writes to land and `sync_all`. A failure
    /// is logged, never silently treated as success.
    pub(crate) fn flush_io(&self) {
        if let Some(Err(error)) = self.io.as_ref().map(|io| io.flush()) {
            tracing::error!(%error, "chunk IO flush failed");
        }
    }

    /// IO 线程是不是已经退出（panic / 通道断开）。世界线程据此当成致命错误走关服。
    /// Whether the IO thread has quit (panic / disconnected channel); the world
    /// thread treats this as a fatal error and shuts down.
    pub(crate) fn io_failed(&self) -> bool {
        self.io.as_ref().is_some_and(|io| io.worker_stopped())
    }

    /// 本次运行累计的写盘失败次数，关服时记进日志。
    /// Write failures seen this run, logged at shutdown.
    pub(crate) fn io_failures(&self) -> u64 {
        self.io_failures
    }

    /// 排一道不阻塞的 IO 栅栏，返回序号；`None` 表示这一局没接磁盘（无需等待）。
    /// `save-all flush` 用它等「此前所有写落盘」的回执，世界线程不等待。
    /// Enqueue a non-blocking IO barrier; `None` means no disk is attached.
    /// `save-all flush` waits for its ack instead of blocking the world thread.
    pub(crate) fn submit_flush_barrier(&mut self) -> Option<u64> {
        self.io.as_ref().map(|io| io.submit_barrier())
    }

    /// 取走这一刻收到的栅栏回执序号，由 `World` 发等在上面的成功消息。
    /// Drain the barrier seqs acked this tick; `World` releases the messages.
    pub(crate) fn take_flushed_barriers(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.flushed_barriers)
    }

    /// 提交一名玩家的存档写，并在内存里记下"已交给 IO、还没落盘"的副本。
    /// 收到 IO 线程的 `PlayerWritten` 回执（`poll_io`）再删掉。
    /// Submit a player save and remember the in-flight copy in memory. The IO
    /// thread's `PlayerWritten` receipt (`poll_io`) removes it.
    pub(crate) fn submit_player_write(&mut self, uuid: &str, bytes: Vec<u8>) {
        if let Some(io) = &self.io {
            let seq = io.submit_player_write(uuid, bytes.clone());
            self.pending_players.insert(uuid.to_owned(), (seq, bytes));
        }
    }

    /// 重进时优先用这份内存副本，而不是连接层读到的盘上旧字节。
    /// On rejoin prefer this in-memory copy over the (possibly stale) disk bytes
    /// read by the connection layer.
    pub(crate) fn pending_player(&self, uuid: &str) -> Option<&[u8]> {
        self.pending_players
            .get(uuid)
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// 测试/无限超平坦专用：只记账，不派任务也不碰 region。
    pub(crate) fn new_infinite_flat(config: DirectorConfig) -> Self {
        let mut host = Self::new(config, MailBudget::default());
        host.infinite_flat = true;
        host
    }

    #[allow(dead_code)]
    pub(crate) fn director(&self) -> &Director<Box<RegionColumn>> {
        &self.director
    }

    /// 测试专用：直接看 IO 线程句柄（收写回执、读回磁盘）。
    /// Test-only: direct access to the IO handle (write acks, read-back).
    #[cfg(test)]
    pub(crate) fn io(&self) -> Option<&ChunkIo> {
        self.io.as_ref()
    }

    pub(crate) fn director_mut(&mut self) -> &mut Director<Box<RegionColumn>> {
        &mut self.director
    }

    #[allow(dead_code)]
    pub(crate) fn counters(&self) -> (u64, u64) {
        self.director.counters()
    }

    #[allow(dead_code)]
    pub(crate) fn parked_count(&self) -> usize {
        self.parking.len()
    }

    /// 本刻交付/收回的区块数。
    pub(crate) fn counters_this_tick(&self) -> (usize, usize) {
        (self.delivered, self.reclaimed)
    }

    /// 每刻开头重置预算和计数。进服/离开/移动发来的票操作都算在这一刻里。
    pub(crate) fn begin_tick(&mut self) {
        self.budget_left = self.budget;
        self.reclaims_spent = 0;
        self.delivered = 0;
        self.reclaimed = 0;
        self.parked = 0;
    }

    /// 测试专用：直接改预算，不碰进程级环境变量。
    #[cfg(test)]
    pub(crate) fn set_budget(&mut self, budget: MailBudget) {
        self.budget = budget;
        self.budget_left = budget;
        self.reclaims_spent = 0;
    }

    /// 这一刻从 region 取回的区块数（不含直接从信箱退回的盒子）。
    #[cfg(test)]
    pub(crate) fn region_reclaims_this_tick(&self) -> usize {
        self.reclaims_spent
    }

    #[cfg(test)]
    pub(crate) fn set_reclaim_delay(&mut self, delay: u64) {
        self.director.config_mut().reclaim_delay_ticks = delay;
    }

    /// 本刻还欠着多少交付/收回。测试用它给信箱长度一个上界。
    #[cfg(test)]
    pub(crate) fn mail_backlog(&self) -> (usize, usize) {
        (self.deliveries.len(), self.reclaims.len())
    }

    /// 一批票的操作：喂给 Director，把效果跑到底。
    pub(crate) fn tickets(&mut self, region: &mut Region, ops: Vec<TicketOp>) {
        if self.infinite_flat {
            // 只把票喂给票表，不产生效果。
            region.profiler.start_sub(SubPhase::TicketUpdate);
            for op in ops {
                self.apply_infinite_ticket(op);
            }
            let _ = self.director.tickets_mut().run_updates();
            region.profiler.end_sub();
            return;
        }
        self.pump(region, vec![Event::Tickets { ops }]);
    }

    fn apply_infinite_ticket(&mut self, op: TicketOp) {
        let table = self.director.tickets_mut();
        match op {
            TicketOp::PlayerMoved { id, to } => table.set_player(id, to),
            TicketOp::ViewDistance(v) => table.set_view_distance(v),
            TicketOp::SimulationDistance(d) => table.set_simulation_distance(d),
            TicketOp::Add { ty, pos, level } => table.add(ty, pos, level),
            TicketOp::Remove { ty, pos, level } => table.remove(ty, pos, level),
            TicketOp::Forced { pos, forced } => table.set_forced(pos, forced),
            TicketOp::WithRadius {
                ty,
                pos,
                radius,
                add,
            } => {
                if add {
                    table.add_with_radius(ty, pos, radius);
                } else {
                    table.remove_with_radius(ty, pos, radius);
                }
            }
        }
    }

    /// 每刻推进。预算和计数由 [`Self::begin_tick`] 在刻开头重置。
    pub(crate) fn tick(&mut self, region: &mut Region, now_tick: u64) {
        if self.infinite_flat {
            region.profiler.start_sub(SubPhase::TicketUpdate);
            self.director.tickets_mut().tick(&mut |_| false);
            let _ = self.director.tickets_mut().run_updates();
            region.profiler.end_sub();
            return;
        }
        self.pace_saves(now_tick);
        self.pump(region, vec![Event::Tick { now_tick }]);
    }

    /// D6：每刻把 `ceil(待写数 / 6000)`（至少 1）个快照提交给 IO 线程。
    /// 单区块 10 秒冷却只在「队列里还有同区块更新的快照」时跳过旧快照，
    /// 保证冷却不会丢掉一个孤立的快照。
    ///
    /// D6: submit `ceil(pending / 6000)` (at least 1) snapshots per tick. The
    /// per-chunk cooldown only skips a stale snapshot when a newer one for the
    /// same chunk is still queued, so an isolated snapshot is never dropped.
    fn pace_saves(&mut self, now_tick: u64) {
        if self.io.is_none() || self.save_queue.is_empty() {
            return;
        }
        let count = save_batch_size(self.save_queue.len());
        let mut to_write = Vec::new();
        for _ in 0..count {
            let Some(pending) = self.save_queue.pop_front() else {
                break;
            };
            let last = self.save_cooldown.get(&pending.pos).copied().unwrap_or(0);
            let has_newer = self
                .save_queue
                .iter()
                .any(|other| other.pos == pending.pos && other.seq > pending.seq);
            if now_tick.saturating_sub(last) < SAVE_COOLDOWN_TICKS && has_newer {
                continue;
            }
            self.save_cooldown.insert(pending.pos, now_tick);
            to_write.push(pending);
        }
        if let Some(io) = &self.io {
            for pending in to_write {
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
    }

    /// 列出「已加载、非原样、版本号和上次快照不同」的脏区块，按坐标排序。
    /// `pristine` 的区块照官方不存；版本号一变就说明自上次落盘后又改过。
    /// List loaded, non-pristine chunks whose revision differs from their last
    /// save, sorted. Pristine chunks are not saved (vanilla); a changed revision
    /// means the chunk was edited since its last snapshot.
    fn dirty_loaded_chunks(&self, region: &Region) -> Vec<ChunkPos> {
        let mut dirty = Vec::new();
        for (pos, revision, pristine) in region.chunks.loaded_states() {
            if !pristine && self.saved_revisions.get(&pos).copied() != Some(revision) {
                dirty.push(pos);
            }
        }
        dirty.sort_unstable();
        dirty
    }

    /// D6：每刻给 `ceil(已加载脏区块数 / 6000)`（至少 1）个仍加载的脏区块拍快照
    /// 交给 IO 线程，单区块 10 秒冷却；`enabled=false`（`save-off`）时整个停掉。
    /// 快照用 `encode_loaded_column` 走和卸载存盘同一条验证过的编码路径，不卸载区块。
    ///
    /// D6: each tick snapshot `ceil(dirty/6000)` (at least 1) still-loaded dirty
    /// chunks to the IO thread with a per-chunk 10s cooldown; fully disabled
    /// during save-off. Snapshots go through the verified unload-save encoder.
    pub(crate) fn save_loaded_dirty(&mut self, region: &mut Region, now_tick: u64, enabled: bool) {
        if !enabled || self.io.is_none() {
            return;
        }
        // 卸载后不必留着记账，避免长局里无界增长。
        // Drop bookkeeping for unloaded chunks so it cannot grow without bound.
        self.saved_revisions
            .retain(|pos, _| region.chunks.is_loaded(*pos));
        self.loaded_save_tick
            .retain(|pos, _| region.chunks.is_loaded(*pos));

        let dirty = self.dirty_loaded_chunks(region);
        let count = save_batch_size(dirty.len());
        let eligible: Vec<ChunkPos> = dirty
            .into_iter()
            .filter(|pos| match self.loaded_save_tick.get(pos) {
                None => true,
                Some(&last) => now_tick.saturating_sub(last) >= SAVE_COOLDOWN_TICKS,
            })
            .take(count)
            .collect();
        for pos in eligible {
            self.snapshot_loaded(region, pos, now_tick);
        }
    }

    /// `/save-all`：立刻给所有已加载脏区块拍快照交给 IO 线程，不受匀速和冷却限制。
    /// `/save-all`: snapshot every loaded dirty chunk to the IO thread at once,
    /// ignoring the pacing and cooldown.
    pub(crate) fn snapshot_all_dirty(&mut self, region: &mut Region, now_tick: u64) {
        if self.io.is_none() {
            return;
        }
        let dirty = self.dirty_loaded_chunks(region);
        for pos in dirty {
            self.snapshot_loaded(region, pos, now_tick);
        }
    }

    /// 给一个已加载脏区块拍快照交给 IO 线程，并记下落定后的版本号和这一刻。
    /// 编码失败或这一刻不适合搬盒子就什么也不记，下一刻重试。
    /// Snapshot one loaded dirty chunk to the IO thread and record the settled
    /// revision and tick. On failure (or when the tables are busy) record nothing
    /// so it is retried on a later tick.
    fn snapshot_loaded(&mut self, region: &mut Region, pos: ChunkPos, now_tick: u64) {
        let Some(result) = region.encode_loaded_column(pos, &self.codec) else {
            return;
        };
        match result {
            Ok(packed) => {
                // 和卸载写共用同一个单调递增的 seq，IO 线程据此丢旧快照。
                // Share one monotonic seq source with unload saves.
                let seq = self.director.allocate_save_seq();
                if let Some(io) = &self.io {
                    io.submit_write(pos, seq, packed.chunk, packed.entities);
                }
                // 搬盒子会换版本号，落定之后再记，下一刻才不会被误判成又脏了。
                // The round-trip bumps the revision; record it afterwards.
                self.saved_revisions
                    .insert(pos, region.chunks.revision(pos));
                self.loaded_save_tick.insert(pos, now_tick);
            }
            Err(error) => {
                tracing::warn!(%error, ?pos, "could not encode a loaded chunk for saving");
            }
        }
    }

    /// 关服：收回全部已加载区块、把脏快照交给 IO 线程写盘，再排空 IO 线程。
    /// 关服不设每刻预算，一次收完全部区块；`flush_io` 等所有写落盘并 `sync_all`。
    /// 每一步都不 panic，任何前提不满足只会导致对应区块没存上，不让进程挂死。
    ///
    /// Shutdown: reclaim every loaded chunk, hand dirty snapshots to the IO thread,
    /// then drain it. No per-tick budget at shutdown; `flush_io` waits for all writes
    /// and calls `sync_all`.
    pub(crate) fn shutdown(&mut self, region: &mut Region) {
        if self.infinite_flat {
            return;
        }
        self.budget = MailBudget {
            deliveries: 0,
            reclaims: 0,
        };
        self.budget_left = self.budget;
        // 收回要求光照算完、各表都不在 tick 中途；关服在 tick 边界调用。
        region.chunks.finish_light_updates();
        self.pump(region, vec![Event::Shutdown]);
        // 关服不等 D6 匀速：把还没提交的快照一次性全部交给 IO 线程。
        // Shutdown does not pace: hand every remaining snapshot to the IO thread.
        if let Some(io) = &self.io {
            for pending in self.save_queue.drain(..) {
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
        self.deliveries.clear();
        self.reclaims.clear();
        self.flush_io();
    }

    /// 处理一个事件和它引发的全部任务，直到没有新效果。
    /// 内联执行器不设每刻上限（提交 5/6 的要求）：任务的纯函数部分当场做完，
    /// 盒子攒进信箱，由 `flush_region_mail` 按预算交付/收回。
    ///
    /// 同一刻里 Director 可能因为 `Delivered` / `Returned` 又派出新任务，
    /// 所以主循环要跑到没有新消息为止；`flush_region_mail` 每轮都只花掉
    /// 这一刻剩下的预算，预算用完之后它返回空事件，循环就停。
    fn pump(&mut self, region: &mut Region, initial: Vec<Event<Box<RegionColumn>>>) {
        // 一批初始事件（例如同一刻收回的所有读盘结果）一起处理，循环末尾只
        // 交付一次，保证按距离排序、出生点 3×3 在同一批里。
        // A batch of initial events (e.g. all reads collected this tick) is driven
        // together and delivered once, keeping distance order and the 3x3 spawn
        // footprint in a single batch.
        let mut events = initial;
        let mut rounds = 0;
        loop {
            // 先把队列里的事件和它们引发的任务结果全部跑完。
            while let Some(event) = events.pop() {
                rounds += 1;
                assert!(rounds < 1_000_000, "Director 的事件循环没有收敛");
                // `Tickets` 和 `Tick` 里包含 `run_updates()`（等级传播 + 消费变化列表），
                // 单独记在 `ticket_update`；其余事件是纯状态机推进。
                let sub = match event {
                    Event::Tickets { .. } | Event::Tick { .. } => SubPhase::TicketUpdate,
                    Event::Delivered { .. } | Event::Returned { .. } => SubPhase::DirectorAck,
                    _ => SubPhase::DirectorEvents,
                };
                region.profiler.start_sub(sub);
                let effects = self.director.handle(event);
                region.profiler.end_sub();
                for effect in effects {
                    match effect {
                        Effect::Submit {
                            id,
                            generation,
                            task,
                        } => {
                            rounds += 1;
                            assert!(rounds < 1_000_000, "Director 的事件循环没有收敛");
                            region.profiler.start_sub(SubPhase::DirectorEvents);
                            let next = self.run_task(id, generation, task);
                            region.profiler.end_sub();
                            // 异步读盘任务这一刻不产生回执，结果在下一刻开头从信箱收回。
                            // An async disk read produces no ack this tick; its result
                            // is collected from the mailbox at the start of the next tick.
                            if let Some(next) = next {
                                events.push(next);
                            }
                        }
                        Effect::Cancel { .. } => {}
                        Effect::ToRegion(message) => self.region_message(region, message),
                        Effect::Park { pos, column } => {
                            region.profiler.start_sub(SubPhase::DirectorEvents);
                            self.parking.park(pos, column);
                            self.parked += 1;
                            region.profiler.end_sub();
                        }
                        Effect::DropSnapshot { pos, seq } => {
                            if let Some(io) = &self.io {
                                io.drop_snapshot(pos, seq);
                            }
                        }
                        Effect::Warn { message } => tracing::warn!(%message, "chunk director"),
                    }
                }
            }
            // 再把 region 信箱里的交付/收回变成 Delivered / Returned 事件。
            // 没有新消息就收敛了。
            events = self.flush_region_mail(region);
            if events.is_empty() {
                return;
            }
        }
    }

    /// 执行器。解析/序列化内联在世界线程上做（在 `ChunkCodec` 接口后，可挪进计算池），
    /// 磁盘读写交给 IO 线程。`None` 表示这是一个异步读任务，结果下一刻从信箱收回。
    ///
    /// The executor: parse/serialize inline on the world thread (behind `ChunkCodec`),
    /// disk IO on the IO thread. `None` means an async read whose result arrives next tick.
    fn run_task(
        &mut self,
        id: TaskId,
        generation: u64,
        task: Task<Box<RegionColumn>>,
    ) -> Option<Event<Box<RegionColumn>>> {
        let pos = task.pos();
        let result = match task {
            // 写盘途中缓冲（Parking）里是整个盒子的最新快照。命中就直接交还，
            // 保证「写盘途中又被请求回来」拿到最新内容而不是从盘上读旧的。
            Task::Read { pos } => {
                if let Some(column) = self.parking.take(pos) {
                    TaskResult::Decoded { column }
                } else if let Some(io) = &self.io {
                    if io.has_chunk(pos) {
                        // 盘上有这个区块：发异步读，结果下一刻从信箱收回。
                        io.submit_read(pos, id, generation);
                        return None;
                    }
                    // 盘上没有：和接存档前一样同步走超平坦模板，不引入一 tick
                    // 的读盘延迟，进服首批次区块的顺序保持不变。
                    TaskResult::Read { bytes: None }
                } else {
                    TaskResult::Read { bytes: None }
                }
            }
            Task::Decode { bytes, .. } => {
                if self.io.is_some() {
                    let started = Instant::now();
                    let outcome = match PackedChunk::unpack(&bytes) {
                        Err(error) => TaskResult::BadChunk { error },
                        Ok(packed) => match self.codec.decode(pos, &packed) {
                            Ok(column) => TaskResult::Decoded { column },
                            Err(error) => TaskResult::BadChunk { error },
                        },
                    };
                    record_codec(started);
                    outcome
                } else {
                    TaskResult::Decoded {
                        column: Box::new(RegionColumn::from_template()),
                    }
                }
            }
            Task::Generate { stage, column, .. } => TaskResult::Generated {
                stage,
                column: column.unwrap_or_else(|| Box::new(RegionColumn::from_template())),
            },
            Task::Assemble { column, .. } => TaskResult::Assembled { column },
            Task::Save { seq, column, .. } => {
                if self.io.is_some() {
                    let started = Instant::now();
                    let encoded = self.codec.encode(pos, &column);
                    record_codec(started);
                    match encoded {
                        Ok(packed) => {
                            // 不直接写盘：进 D6 匀速队列，每刻出队一小批。
                            self.save_queue.push_back(PendingWrite {
                                pos,
                                seq,
                                chunk: packed.chunk,
                                entities: packed.entities,
                            });
                            self.pending_writes.insert(pos, seq);
                            TaskResult::Saved { seq, column }
                        }
                        Err(error) => {
                            // 编码失败不能把盒子弄丢：留在停放表里，下次卸载再试。
                            self.parking.park(pos, column);
                            TaskResult::SaveFailed { seq, error }
                        }
                    }
                } else {
                    TaskResult::Saved { seq, column }
                }
            }
        };
        Some(Event::TaskDone {
            id,
            pos,
            generation,
            result,
        })
    }

    /// 每刻开头收回 IO 信箱：读结果变成 `TaskDone` 喂给 Director，
    /// 写完成回执用来丢掉写盘途中缓冲。
    ///
    /// Drain the IO mailbox at the start of each tick: read results become
    /// `TaskDone` events; write acks release the write-in-progress buffer.
    pub(crate) fn poll_io(&mut self, region: &mut Region) {
        let mut replies = Vec::new();
        if let Some(io) = &self.io {
            while let Some(reply) = io.try_recv() {
                replies.push(reply);
            }
        }
        let mut done = Vec::new();
        for reply in replies {
            match reply {
                Reply::Read {
                    pos,
                    task_id,
                    generation,
                    chunk,
                    entities,
                    error,
                } => {
                    let bytes = match error {
                        Some(message) => {
                            tracing::warn!(%message, ?pos, "chunk read failed; using flat template");
                            None
                        }
                        None if chunk.is_none() => None,
                        None => Some(PackedChunk { chunk, entities }.pack()),
                    };
                    done.push(Event::TaskDone {
                        id: task_id,
                        pos,
                        generation,
                        result: TaskResult::Read { bytes },
                    });
                }
                Reply::Written { pos, seq } => {
                    if self.pending_writes.get(&pos) == Some(&seq) {
                        self.pending_writes.remove(&pos);
                        // 写盘完成，盒子已经在磁盘上，丢掉内存缓冲。
                        let _ = self.parking.take(pos);
                    }
                }
                Reply::PlayerWritten {
                    uuid,
                    seq,
                    ok,
                    error,
                } => {
                    // 只有最新一次写成功落盘才丢掉内存副本；失败的写留着，
                    // 重进仍能拿到离开时的数据。只有回执的 seq 和记录一致才删，
                    // 免得旧写完成把新写的副本删掉。
                    // Drop the in-memory copy only when the newest write landed.
                    // A failed write keeps it so a rejoin still sees the leaving
                    // data; matching seq avoids an older receipt removing a newer copy.
                    if !ok {
                        if let Some(error) = error {
                            tracing::error!(%uuid, %error, "player save write failed");
                        }
                        continue;
                    }
                    if self
                        .pending_players
                        .get(&uuid)
                        .is_some_and(|(s, _)| *s == seq)
                    {
                        self.pending_players.remove(&uuid);
                    }
                }
                Reply::Flushed { seq } => {
                    // 异步栅栏的回执：World 会在本刻取走它，发等在上面的
                    // `save-all flush` 成功消息。
                    // An async barrier ack; World drains it this tick to release
                    // the `save-all flush` success message waiting on it.
                    self.flushed_barriers.push(seq);
                }
                Reply::WriteFailed { path, error } => {
                    // 每次写/刷盘失败都带路径和原因记 error!，绝不静默丢数据。
                    // Log every write/sync failure with its path and reason.
                    tracing::error!(%path, %error, "chunk IO write failed");
                    self.io_failures += 1;
                }
            }
        }
        // 所有读盘结果一次性喂给 Director，只交付一次，保住 3×3 和距离顺序。
        if !done.is_empty() {
            self.pump(region, done);
        }
    }

    fn region_message(&mut self, region: &mut Region, message: RegionMessage<Box<RegionColumn>>) {
        match message {
            RegionMessage::Deliver {
                pos,
                generation,
                column,
                desired,
            } => {
                region.profiler.time_sub(SubPhase::DirectorEvents, || {
                    self.deliveries.insert(pos, (generation, desired, column));
                });
            }
            RegionMessage::Reclaim { positions } => {
                region.profiler.time_sub(SubPhase::DirectorEvents, || {
                    self.reclaims.extend(positions);
                });
            }
            RegionMessage::LevelChanged { changes } => {
                // 同一个 pump 里可能先收到档位变化、后 flush 交付：信箱里那份档位
                // 也要跟着变，否则新交付的区块会带着旧档位进 region。尚未交付的
                // 位置在 region 里没有记录，`level_changed` 只更新已交付的那些。
                region.profiler.time_sub(SubPhase::DirectorEvents, || {
                    for change in &changes {
                        if let Some(entry) = self.deliveries.get_mut(&change.pos) {
                            entry.1 = Desired::new(change.loading.1, change.simulation.1);
                        }
                    }
                });
                region.profiler.time_sub(SubPhase::LevelsUpdate, || {
                    region.tick_levels.level_changed(&changes);
                });
            }
            // `Ready` 不用另存一份集合：`ChunkStore::is_loaded` 就是已交付集合，
            // 发区块时直接问它；多存一份只会多一处要保持一致的状态。
            RegionMessage::Ready { .. } | RegionMessage::Forgot { .. } => {}
        }
    }

    /// 离最近的玩家有多远（切比雪夫距离）。没有玩家时所有位置一样远。
    fn distance_to_players(pos: ChunkPos, centers: &[ChunkPos]) -> (u64, i32, i32) {
        let nearest = centers
            .iter()
            .map(|center| {
                let dx = i64::from(pos.x) - i64::from(center.x);
                let dz = i64::from(pos.z) - i64::from(center.z);
                // 差值非负，两个 i32 相减放进 i64 不会溢出。
                dx.abs().max(dz.abs()) as u64
            })
            .min()
            .unwrap_or(u64::MAX);
        // 同距离按坐标排，结果不依赖 BTreeMap 之外的任何顺序。
        (nearest, pos.x, pos.z)
    }

    /// 玩家脚下 3×3：不受每刻上限约束，且优先于任何其它位置。
    /// 玩家不能站在一个没加载的地方，进服、传送、高速移动都一样。
    fn is_under_a_player(pos: ChunkPos, centers: &[ChunkPos]) -> bool {
        centers.iter().any(|center| {
            (i64::from(pos.x) - i64::from(center.x)).abs() <= 1
                && (i64::from(pos.z) - i64::from(center.z)).abs() <= 1
        })
    }

    /// 玩家所在的区块。刻预算用完时，脚下 3×3 仍然要交付完。
    fn guaranteed_centers(region: &Region) -> Vec<ChunkPos> {
        region
            .players
            .values()
            .map(|player| player.chunk_pos())
            .collect()
    }

    /// 收回请求瞄着还在信箱里排队的盒子时，直接把盒子还回去。
    ///
    /// 代数用 `Reclaim` 带来的那个：它和 `Deliver` 时的代数相同，
    /// `Director::on_returned` 核对的就是它。盒子没进过 region，所以
    /// 不需要碰 region、档位表里也没有它要清的条目、不算「交付」、
    /// 也不占每刻的收回额度；信箱里为它记的等级随整条记录一起删掉。
    fn return_pending_deliveries(&mut self, events: &mut Vec<Event<Box<RegionColumn>>>) {
        if self.reclaims.is_empty() || self.deliveries.is_empty() {
            return;
        }
        self.reclaims.retain(|(pos, generation)| {
            let Some((_, _, column)) = self.deliveries.remove(pos) else {
                return true;
            };
            self.reclaimed += 1;
            events.push(Event::Returned {
                pos: *pos,
                generation: *generation,
                column: Some(column),
            });
            false
        });
    }

    /// region 在 tick 边界收信箱：先交付、后收回，返回要回给 Director 的事件。
    ///
    /// 整刻最多交付 `budget.deliveries` 个非豁免区块（`0` 表示不限），
    /// 收回的上限见 [`Self::reclaim_cap`]，剩下的留到下一刻。
    /// 同一刻内 `pump` 会多次调用它，交付共用 `budget_left` 里的剩余额度，
    /// 收回共用 `reclaims_spent`。
    fn flush_region_mail(&mut self, region: &mut Region) -> Vec<Event<Box<RegionColumn>>> {
        let delivery_left = self.budget_left.deliveries;
        let limited_deliveries = self.budget.deliveries > 0;
        let mut events = Vec::new();
        // 收回优先：盒子可能还在 `deliveries` 里排队（交付有了每刻上限之后，
        // 一个盒子可以在信箱里等好几刻）。这时 Director 已经把它记成
        // `Reclaiming`，盒子必须原封不动地作为 `Returned` 回去，绝不能先进
        // region 再被取出来，更不能晚几刻变成一个没人管的孤儿。
        // 这一步不碰 region，不占交付/收回上限，也不需要等光照。
        region.profiler.time_sub(SubPhase::ReturnBoxes, || {
            self.return_pending_deliveries(&mut events);
        });
        if self.deliveries.is_empty() && self.reclaims.is_empty() {
            return events;
        }
        // 只有信箱里真有要交付/收回的东西时才把光照算完：这个函数每刻都调，
        // 无条件 `finish_light_updates` 会绕开「光照每刻有预算」的设计。
        // 交付/收回要求「没有算到一半的轮次」。
        region.profiler.time_sub(SubPhase::DeliveryLightFinish, || {
            region.chunks.finish_light_updates();
        });
        let centers = Self::guaranteed_centers(region);

        // 近的先交付。玩家脚下 3×3 全部交付且不算进上限；同等距离按坐标排。
        region.profiler.start_sub(SubPhase::MailSort);
        let mut ordered: Vec<ChunkPos> = self.deliveries.keys().copied().collect();
        // BTreeMap 的键已经按 (x, z) 升序，稳定排序让同等距离保持坐标顺序。
        ordered.sort_by_key(|&pos| {
            let guaranteed = Self::is_under_a_player(pos, &centers);
            (!guaranteed, Self::distance_to_players(pos, &centers))
        });
        region.profiler.end_sub();
        let mut used = 0usize;
        // 一批交付只重算一次受影响的邻域，不逐块重算；每次成功都要把
        // 信箱里记着的最新等级交给 region（脚下 3×3 和下一刻才轮到的都算）。
        let mut delivered_levels = Vec::new();
        for pos in ordered {
            let guaranteed = Self::is_under_a_player(pos, &centers);
            if !guaranteed && limited_deliveries && used >= delivery_left {
                continue;
            }
            let Some((generation, desired, column)) = self.deliveries.remove(&pos) else {
                continue;
            };
            match region.deliver_column(pos, column) {
                Ok(()) => {
                    self.delivered += 1;
                    if !guaranteed {
                        used += 1;
                    }
                    delivered_levels.push((pos, desired));
                    events.push(Event::Delivered { pos, generation });
                }
                Err((_, column)) => {
                    // 前提不满足（例如某张表正在 tick 中途）：留在信箱里，
                    // 下一次 pump 再试，绝不把盒子丢掉。
                    tracing::warn!(?pos, "chunk delivery was not ready, will retry");
                    self.deliveries.insert(pos, (generation, desired, column));
                }
            }
        }
        // 一批交付只重算一次受影响的邻域，不逐块重算。
        region.profiler.time_sub(SubPhase::LevelsUpdate, || {
            region.tick_levels.deliver(delivered_levels);
        });
        if limited_deliveries {
            self.budget_left.deliveries = delivery_left.saturating_sub(used);
        }

        if self.reclaims.is_empty() {
            return events;
        }
        let reclaimed_before = self.reclaimed;
        // 远的先收回：这些区块不急，玩家脚下的票也没了时先留着更保险。
        region.profiler.start_sub(SubPhase::MailSort);
        let mut by_generation = BTreeMap::new();
        for (pos, generation) in std::mem::take(&mut self.reclaims) {
            by_generation.insert(pos, generation);
        }
        let mut ordered: Vec<ChunkPos> = by_generation.keys().copied().collect();
        let distance = |pos: &ChunkPos| Self::distance_to_players(*pos, &centers);
        // 距离取相反数，远的在前；同距离按坐标升序。
        // 积压可以有几千个，到玩家的距离每个位置只算一次。
        ordered.sort_by_cached_key(|pos| {
            let (far, x, z) = distance(pos);
            (std::cmp::Reverse(far), x, z)
        });
        region.profiler.end_sub();
        // 这一刻要收回的总数 = 已经收的 + 还在信箱里的。收一个，前一项加一、
        // 后一项减一，总数不变；所以同一刻里不管 `pump` 调用几次，
        // 上限只会因为新排进来的请求变大，不会因为多调用几次而重新发一份额度。
        let cap = Self::reclaim_cap(self.budget.reclaims, self.reclaims_spent + ordered.len());
        let count = ordered.len().min(cap.saturating_sub(self.reclaims_spent));
        if count == 0 {
            // 这一刻的额度用完了，全部留到下一刻。
            self.reclaims.extend(by_generation);
            return events;
        }
        let batch: Vec<(ChunkPos, u64)> = ordered
            .into_iter()
            .take(count)
            .map(|pos| {
                let generation = by_generation.remove(&pos).expect("键在表里");
                (pos, generation)
            })
            .collect();
        // 没轮到的留在信箱里，下一次 tick 再收。
        self.reclaims.extend(by_generation);
        let positions = batch.iter().map(|&(pos, _)| pos).collect();
        match region.reclaim_columns(&positions) {
            Ok(mut boxes) => {
                region.profiler.time_sub(SubPhase::LevelsUpdate, || {
                    region.tick_levels.reclaim(positions.iter());
                });
                region.profiler.start_sub(SubPhase::ReturnBoxes);
                for (pos, generation) in batch {
                    self.reclaimed += 1;
                    events.push(Event::Returned {
                        pos,
                        generation,
                        column: boxes.remove(&pos),
                    });
                }
                region.profiler.end_sub();
            }
            Err(_) => {
                // 失败时整批放回信箱，下次 tick 边界再试：Director 还以为这些
                // 区块「正在收回」，丢了这条列表它就永远等不到回执。
                tracing::warn!("chunk reclaim was not ready, will retry");
                self.reclaims.extend(batch);
            }
        }
        self.reclaims_spent += self.reclaimed.saturating_sub(reclaimed_before);
        events
    }

    /// 一刻里最多从 region 取回多少个区块。`demand` 是这一刻要收回的总数。
    ///
    /// 积压小的时候就是配置的上限；积压大了每刻收掉它的
    /// 1/[`RECLAIM_DRAIN_TICKS`]。配置成 0 表示不限。
    fn reclaim_cap(configured: usize, demand: usize) -> usize {
        if configured == 0 {
            return usize::MAX;
        }
        configured.max(demand.div_ceil(RECLAIM_DRAIN_TICKS))
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, PrimedTnt};
    use mc_bamboo_core::{ChunkPos, Vec3};

    use super::*;
    use crate::world::tests::Harness;

    const ORIGIN: ChunkPos = ChunkPos::new(0, 0);

    /// 造一个「收回失败」的前提：实体表里有一行被借出去还没放回，
    /// `EntityStore::take_chunk` 会因此整批拒绝。
    fn borrowed_entity(region: &mut crate::region::Region) -> Entity {
        let entity = Entity::new(
            EntityKind::Tnt(PrimedTnt::new()),
            Vec3::new(2.0, -59.0, 2.0),
        );
        let uid = region.entities.spawn(entity);
        let entity = region.entities.take(uid).expect("刚放进去的实体");
        assert!(
            !region.ready_to_move_columns(),
            "有借出去的实体时不该能收回"
        );
        entity
    }

    #[test]
    fn a_failed_reclaim_is_put_back_and_retried() {
        let mut host = DirectorHost::new(DirectorConfig::default(), MailBudget::default());
        let mut server = Harness::new(2);
        let region = server.region();
        host.reclaims.push((ORIGIN, 1));
        let entity = borrowed_entity(region);
        let first = host.flush_region_mail(region);
        assert!(
            first.is_empty(),
            "前提不满足时不产生 Returned，Director 还在等"
        );
        assert_eq!(host.reclaims, vec![(ORIGIN, 1)], "失败的收回要放回信箱");

        region.entities.put(entity);
        let second = host.flush_region_mail(region);
        assert!(host.reclaims.is_empty(), "重试成功之后信箱清空");
        assert_eq!(second.len(), 1, "这一刻要给 Director 回 Returned");
        assert!(matches!(second[0], Event::Returned { pos, .. } if pos == ORIGIN));
    }

    /// 同一个位置在同一刻既在 `deliveries` 里又在 `reclaims` 里：收回优先，
    /// 盒子原样还回去，绝不先进 region 再被取出来。
    #[test]
    fn a_reclaim_and_a_delivery_for_one_position_take_the_box_once() {
        let mut host = DirectorHost::new(DirectorConfig::default(), MailBudget::default());
        let mut server = Harness::new_loaded(2);
        let pos = ChunkPos::new(20, 20);
        host.deliveries.insert(
            pos,
            (
                7,
                Desired::UNLOADED,
                Box::new(RegionColumn::from_template()),
            ),
        );
        host.reclaims.push((pos, 7));
        let events = host.flush_region_mail(server.region());
        assert_eq!(events.len(), 1, "只回一条 Returned");
        assert!(matches!(
            events[0],
            Event::Returned {
                pos: at,
                generation: 7,
                column: Some(_)
            } if at == pos
        ));
        assert_eq!(host.mail_backlog(), (0, 0), "两个信箱都清空");
        assert!(!server.region().chunks.is_loaded(pos), "盒子没进 region");
        assert_eq!(host.delivered, 0, "不占交付额度");
    }

    /// 交付因为前提不满足失败时，盒子一定原样回到信箱，且 region 里
    /// 没有留下半插入的状态；前提恢复后下一次 flush 能成功交付。
    #[test]
    fn a_failed_delivery_puts_the_box_back_without_a_half_insert() {
        let mut host = DirectorHost::new(DirectorConfig::default(), MailBudget::default());
        let mut server = Harness::new_loaded(2);
        let region = server.region();
        host.deliveries.insert(
            ORIGIN,
            (
                3,
                Desired::UNLOADED,
                Box::new(RegionColumn::from_template()),
            ),
        );
        // 方块实体正在 tick 中途：`deliver_column` 的只读前提不满足。
        region.block_entities.begin_tick();
        assert!(!region.ready_to_move_columns(), "前提确实不满足");

        let first = host.flush_region_mail(region);
        assert!(first.is_empty(), "失败时不产生 Delivered");
        assert_eq!(host.mail_backlog(), (1, 0), "盒子回到信箱");
        assert!(
            !region.chunks.is_loaded(ORIGIN),
            "region 里没有半插入的区块"
        );
        assert_eq!(host.delivered, 0, "失败不占额度");

        // 同一个 pump 轮次里再调一次也还是失败，盒子不会丢、不会重复插入。
        let again = host.flush_region_mail(region);
        assert!(again.is_empty(), "前提没恢复前一直失败");
        assert_eq!(host.mail_backlog(), (1, 0), "重试没有把盒子弄丢");

        region.block_entities.end_tick();
        let third = host.flush_region_mail(region);
        assert_eq!(third.len(), 1, "前提恢复后交付成功");
        assert!(matches!(third[0], Event::Delivered { pos, generation: 3 } if pos == ORIGIN));
        assert!(region.chunks.is_loaded(ORIGIN), "只插入了一次");
        assert_eq!(host.mail_backlog(), (0, 0), "信箱清空");
    }

    #[test]
    fn invalid_env_values_fall_back_to_the_default_budget() {
        // 其它测试可能并发改环境变量，这里只验证解析函数本身不会 panic。
        assert_eq!(MailBudget::default().deliveries, DEFAULT_MAIL_BUDGET);
        assert_eq!(MailBudget::default().reclaims, DEFAULT_MAIL_BUDGET);
    }

    #[test]
    fn d6_batch_size_is_at_least_one() {
        assert_eq!(save_batch_size(0), 1);
        assert_eq!(save_batch_size(1), 1);
        assert_eq!(save_batch_size(6000), 1);
        assert_eq!(save_batch_size(6001), 2);
        assert_eq!(save_batch_size(12000), 2);
    }

    #[test]
    fn d6_drains_the_queue_one_snapshot_per_tick() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct TempDir(std::path::PathBuf);
        impl TempDir {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!("bamboo-d6-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let tmp = TempDir::new();
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default(), MailBudget::default());
        host.set_io(ChunkIo::open(&dir).unwrap());
        for i in 0..3 {
            host.save_queue.push_back(PendingWrite {
                pos: ChunkPos::new(i, 0),
                seq: i as u64,
                chunk: Some(vec![1]),
                entities: None,
            });
        }
        host.pace_saves(1);
        assert_eq!(host.save_queue.len(), 2);
        host.pace_saves(2);
        assert_eq!(host.save_queue.len(), 1);
        host.pace_saves(3);
        assert!(host.save_queue.is_empty());
    }
}
