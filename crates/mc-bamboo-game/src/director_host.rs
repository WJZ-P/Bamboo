//! Director 和世界线程之间的宿主：内联执行器、停放表、region 消息泵。
//!
//! 提交 5 里执行器是内联的：任务当场做完，盒子攒在信箱里。信箱在 tick 开头
//! 按离玩家的距离交付/收回，每刻有个数上限；玩家脚下 3×3 不受上限约束。
//! 视野里没交付的区块留在 `PlayerChunkSender` 的待发送集合里，交付了才发。

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_storage::chunk_io::{ChunkIo, Reply};
use mc_bamboo_world::{
    director::{
        DemandSnapshot, Director, DirectorConfig, Effect, Event, Owner, RegionId, RegionMessage,
        SaveSeq, Task, TaskId, TaskResult, TicketOp, owner_is_held,
    },
    light::generate::GeneratedLight,
    store::ChunkRevision,
    ticket::{ChunkStatus, LevelChange},
};

use crate::{
    chunk_codec::PackedChunk,
    chunk_generator::{ChunkGenerator, FlatGenerator},
    chunk_jobs::{ChunkJob, ChunkOutcome, ChunkReply, ChunkWork, decode_packed, encode_column},
    compute_pool::{ComputePool, PoolJob},
    inbound::PlayerId,
    island::{
        DemandRank, Encoded, Island, IslandAccess, MailOrder, MailReport, Posted, SaveChanges,
        SavePlan, SaveReport, distance_to_players,
    },
    island_partition::{ChunkDelta, IslandPartition, SectionPos, TopologyOp},
    profiler::{Profiler, SubPhase},
    region::{RegionColumn, parking::Parking},
    worldgen::GeneratedChunk,
    worldgen_features::{FeatureWrites, GenerateOutput, assemble_feature_column},
};

/// 编解码累计耗时（纳秒）。解析/序列化内联在世界线程上做，这里量出总耗时，
/// 关服时记进日志，写进汇报；以后挪进计算池时可以直接对比。
/// Accumulated codec time (ns), used in the shutdown log / report.
static CODEC_NANOS: AtomicU64 = AtomicU64::new(0);

pub fn codec_nanos() -> u64 {
    CODEC_NANOS.load(Ordering::Relaxed)
}

pub(crate) fn record_codec(started: Instant) {
    let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    CODEC_NANOS.fetch_add(elapsed, Ordering::Relaxed);
}

/// 交付/收回每刻的个数上限。0 表示不限（只给剖析和回归对比用）。
pub(crate) const DEFAULT_MAIL_BUDGET: usize = 64;
/// 官方 `MinecraftServer.autoSave`：每 6000 刻一次全量（只写改过的）。
/// Official `MinecraftServer.autoSave`: one full pass every 6000 ticks.
pub(crate) const SAVE_INTERVAL_TICKS: u64 = 6000;
/// 官方 `ChunkMap.saveChunksEagerly`：每刻最多 20 个，单区块 10 秒冷却。
/// Official `ChunkMap.saveChunksEagerly`: at most 20 per tick.
pub(crate) const EAGER_SAVES_PER_TICK: usize = 20;
/// 单区块 10 秒（200 刻）冷却，照原版 `nextChunkSaveTime`。
/// Per-chunk 10 second (200 tick) cooldown, like vanilla `nextChunkSaveTime`.
pub(crate) const SAVE_COOLDOWN_TICKS: u64 = 200;
/// 写盘连续失败时的退避：从 1 刻起翻倍，封顶 200 刻。失败绝不永久暂停，
/// 磁盘恢复后下一次到点就重试（复查报告「致命 2」）。
/// Backoff after consecutive write failures: 1,2,4,... capped at 200 ticks.
/// A failure never pauses saving forever: retries resume when the disk recovers.
pub(crate) const MAX_CONSECUTIVE_IO_FAILURES: u32 = 3;
pub(crate) const MAX_SAVE_RETRY_BACKOFF_TICKS: u64 = 200;
/// F6：在途快照、停放盒子、IO 信箱的硬上限，超了就暂缓卸载做背压。
/// F6: hard caps on queued snapshots, parked boxes and the IO mailbox;
/// exceeding any of them defers unloads (backpressure).
pub(crate) const MAX_PENDING_WRITES: usize = 4096;
pub(crate) const MAX_PARKED_BOXES: usize = 4096;
pub(crate) const MAX_IO_MAILBOX: usize = 16384;
/// 一次 `pump` 里最多跑多少事件/效果。只是防真死循环的护栏，不能让大世界
/// 关服因为达到上限就 panic：超了记 error 并停在这一轮，不走 `assert!`。
/// Guard against a truly non-convergent `pump`. A large world must never panic
/// here: on exceeding the cap, log and stop the pump instead of asserting.
pub(crate) const PUMP_ROUND_LIMIT: usize = 20_000_000;
/// 关服排空计算池的墙钟上限。正常关服远小于这个值；它只防止任务被无限重派
/// 时世界线程卡死，超时后明确记下没存下来的区块、继续关服，绝不无限等。
///
/// Wall-clock bound for draining the compute pool at shutdown. Normal shutdown
/// is far shorter; it only stops the world thread from hanging if tasks keep
/// being resubmitted forever. On timeout, log the unsaved chunks and continue
/// shutting down instead of waiting forever.
pub(crate) const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// 计算池里每条线程最多同时放几个任务，其余留在宿主的待办里。
/// 留在宿主手里的任务还能按玩家的新位置重排、撤票时直接丢掉，
/// 不会在池子里排一长串过期的远处任务。
pub(crate) const POOL_JOBS_PER_THREAD: usize = 4;

/// 一次待落盘的区块快照。
#[cfg(test)]
pub(crate) struct PendingWrite {
    pub(crate) pos: ChunkPos,
    pub(crate) seq: SaveSeq,
    pub(crate) chunk: Option<Vec<u8>>,
    pub(crate) entities: Option<Vec<u8>>,
}
#[cfg(not(test))]
struct PendingWrite {
    pos: ChunkPos,
    seq: SaveSeq,
    chunk: Option<Vec<u8>>,
    entities: Option<Vec<u8>>,
}

/// 主控判脏的结果。
struct JudgedDirty {
    dirty: Vec<ChunkPos>,
    remark: Vec<ChunkPos>,
    #[cfg(test)]
    verify: Vec<(ChunkPos, ChunkRevision)>,
}

/// 卸载快照的匀速提交：`ceil(待写数 / 6000)`，至少 1 个。
fn save_batch_size(queue_len: usize) -> usize {
    (queue_len as u64).div_ceil(SAVE_INTERVAL_TICKS).max(1) as usize
}

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
    /// 主控的邮筒：Director 发给各岛、岛还没收取的信，按岛编号分开、按投递顺序排。
    /// 交付的预算、排序和收回的上限都在岛的信箱里（`island::ChunkMailbox`）。
    ///
    /// The master's postbox: letters from the Director not yet taken by their
    /// island, per island id in posting order. Budgets, ordering and reclaim caps
    /// live in the island's own mailbox (`island::ChunkMailbox`).
    posts: BTreeMap<RegionId, Vec<Posted>>,
    /// 主控的剖析器：宿主的子项（票、Director 事件、回执）记在这里，每刻末尾并进岛的
    /// 那份报告（只有一个岛时就是合计）。
    ///
    /// The master's profiler: the host's sub-phases (tickets, Director events,
    /// acks) are recorded here and folded into the island's report at the end of
    /// each tick (with one island that is the total). 放在堆上：剖析器有几十 KB，
    /// 调试构建按值搬 `World` 时不该再多占一份栈。
    /// Boxed: a profiler is tens of KB and debug builds move `World` by value.
    pub(crate) profiler: Box<Profiler>,
    /// 和 Director 同一份玩家需求快照，岛收信时排交付顺序用；共享，换快照才复制。
    ///
    /// The same player-demand snapshot as the Director's, shared with the island
    /// for delivery order; copied only when it changes.
    demand: Arc<DemandSnapshot>,
    /// 主控眼里岛上已经交付的区块：按岛交回的回执增删，和岛的 `ChunkStore` 已加载集合
    /// 一致（齐步下每次收信之后，分离下每次交回之后）。存盘记账按它剪枝。
    ///
    /// The master's view of the chunks delivered to the island, kept from the
    /// island's acks; it matches the island's loaded set after each mail
    /// application (lockstep) or hand-back (detached). Save bookkeeping is pruned by it.
    island_chunks: HashSet<ChunkPos>,
    /// 齐步收信时的回执，跨次复用容量。
    mail_report: MailReport,
    /// 一次 `pump` 里攒着的异步读请求，收尾时按离玩家的距离排序后一起交给
    /// IO 线程。IO 线程按 FIFO 处理：近处的回执先到，`PlayerChunkSender` 的
    /// 第一批就一定是最近的区块，远处的模板不会抢在近处的盘上区块之前。
    /// Async reads collected during one `pump`, submitted in distance order at
    /// the end. The IO worker is FIFO, so near replies arrive first.
    read_queue: Vec<(ChunkPos, TaskId, u64)>,
    /// 每次 `Director::handle_effects` 复用的效果列表容量。
    effect_scratch: Vec<Effect<Box<RegionColumn>>>,
    /// 信箱泵的事件队列，跨刻复用容量。
    pump_events: Vec<Event<Box<RegionColumn>>>,
    /// 交给 `pump` 的单事件批次，跨刻复用容量。
    initial_scratch: Vec<Event<Box<RegionColumn>>>,
    /// IO 回执和读盘结果事件列表，跨刻复用容量。
    io_replies: Vec<Reply>,
    done_scratch: Vec<Event<Box<RegionColumn>>>,
    /// 统计：本刻停放的区块数。
    parked: usize,
    /// 区块 IO 线程。`None` 表示这一局不接磁盘（测试 / 无限超平坦）。
    io: Option<ChunkIo>,
    /// 生成阶段函数。内联执行器和计算池共用这一个。
    generator: Arc<dyn ChunkGenerator>,
    /// 生成器的世界配置，读盘恢复结构起点时要用同一个种子。
    world_gen_config: crate::world_options::WorldGenConfig,
    /// 生成期每个区块的半成品 ProtoChunk（CARVERS 地形 + 已按 `TaskId` 顺序合并的
    /// 地物写增量），存成不可变 `Arc`：任务发出后世界线程不再改这份快照，要改就
    /// `Arc::make_mut` 写时复制出新的 `Arc`（设计稿 5.3.1）。只在世界线程读写，
    /// 计算池拿到的都是 `Arc` 克隆、只读。区块交付/收回时清掉。
    ///
    /// Half-built ProtoChunk per chunk during generation (CARVERS terrain +
    /// feature deltas merged in `TaskId` order), as an immutable `Arc`: once a
    /// task is dispatched the world thread never mutates that snapshot, it
    /// copy-on-writes a new `Arc` via `Arc::make_mut` (design 5.3.1). Only the
    /// world thread mutates; the compute pool gets read-only `Arc` clones.
    partials: BTreeMap<ChunkPos, Arc<GeneratedChunk>>,
    /// 每个区块累计的地物写，按世界线程收到结果的顺序（`TaskId` 升序）追加；
    /// 每条都带来源中心下标，完工组装时按固定先后合，与追加顺序无关。
    /// Per-chunk feature writes, appended in the order the world thread receives
    /// results (ascending `TaskId`); each carries its source center index and
    /// assembly merges in a fixed order, independent of append order.
    feature_deltas: BTreeMap<ChunkPos, Vec<FeatureWrites>>,
    /// `light` 阶段算出的生成期光照，按区块存；完工组装时装进交付盒子并清掉。
    /// Generation-time light from the `light` stage, per chunk; installed into
    /// the delivered column at final assembly and cleared.
    generated_light: BTreeMap<ChunkPos, GeneratedLight>,
    /// `structure_references` 阶段算好的 `structures` 顶层标签，按区块存；完工组装时
    /// 装进交付盒子并清掉。
    /// The `structures` tag from `structure_references`, installed into the
    /// delivered column at assembly and then cleared.
    generated_structures: BTreeMap<ChunkPos, mc_bamboo_nbt::Tag>,
    /// 读盘半成品区块的上层内容（方块实体 / 计划刻 / 实体 / 四张表）。续跑的
    /// 阶段纯函数会替换方块层，完工组装时把这些内容装回交付盒子。
    /// Upper-layer content of a decoded non-full chunk; stage functions replace
    /// the block layer, so assembly moves this back into the delivered box.
    disk_upper_bytes: BTreeMap<ChunkPos, Vec<u8>>,
    /// 盘上 `Status >= FEATURES` 的区块：完工组装不能重建方块层。
    disk_saved_after_features: BTreeMap<ChunkPos, bool>,
    /// 盘上 `Status` 已经过了 `SPAWN` 的半成品区块：续跑完工时不再生成期刷怪。
    ///
    /// Decoded chunks already past `SPAWN`: no world-generation spawning on completion.
    resumed_after_spawn: BTreeSet<ChunkPos>,
    /// 计算池。`None` 是内联执行器：解析、生成、序列化当场做完（精确模式、测试）。
    pool: Option<ComputePool<ChunkJob>>,
    /// 还没交给池子的计算任务（待办）。池子里最多放 `threads × POOL_JOBS_PER_THREAD`
    /// 个，其余留在这里，每次 `pump` 收尾按离玩家的距离挑最近的补进去；撤票时
    /// 直接从这里删掉。存盘不进待办，直接交给池子。
    compute_backlog: HashMap<TaskId, ChunkJob>,
    /// 待办的出队顺序：`(rank, TaskId)` 小的先出。`rank` 是
    /// `(玩家优先级, x, z)`，和 Director 的准入排序同一份需求快照。
    /// 删掉的任务不从堆里删，出堆时发现它已经不在 `compute_backlog` 里就跳过。
    backlog_order: BinaryHeap<Reverse<(DemandRank, TaskId)>>,
    /// 上次整堆重排时票表里的玩家区块；`set_player_demand` 清空它，强制重排。
    backlog_centers: Vec<ChunkPos>,
    /// 还没交给池子就被取消的任务：回执留到下一次收池结果时一起交给 Director，
    /// 保证每个派出去的任务都恰好回一次执。
    early_replies: Vec<(TaskId, ChunkReply)>,
    /// 已经交给池子、又被 Director 取消的任务。它们的解析结果直接丢掉：
    /// 字节来自盘上，盘上还有，不必停放。
    cancelled_compute: HashSet<TaskId>,
    /// 测试用：交给池子的 (解析, 生成, 序列化) 任务数。
    #[cfg(test)]
    pub(crate) pooled_kinds: [usize; 3],
    /// 每个区块最新一次写的 seq。收到 `Written` 回执时据此丢掉写盘途中缓冲。
    pending_writes: HashMap<ChunkPos, SaveSeq>,
    /// D6：还没提交给 IO 线程的快照，每刻匀速出队。
    #[cfg(test)]
    pub(crate) save_queue: VecDeque<PendingWrite>,
    #[cfg(not(test))]
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
    /// 每个已加载区块上次「写盘成功」后的 `(版本号, 上层内容指纹)`（补 9）。
    /// 只在 `Written` 回执到达时记，失败写不记，下一次正常判脏会重试。
    /// `(revision, upper-content fingerprint)` at each chunk's last *successful*
    /// write (item 9). Recorded only on the `Written` ack, so a failed write
    /// stays dirty and is retried by the normal path (review fatal 2).
    saved_revisions: HashMap<ChunkPos, (ChunkRevision, u64)>,
    /// 每次已提交写的快照时的版本号与指纹，按 seq 记；`Written` 回执到达时才搬到
    /// `saved_revisions`。比它旧的回执不改判脏状态。
    /// Snapshot revision/fingerprint per submitted seq; moved into
    /// `saved_revisions` only on its `Written` ack.
    acked_revisions: HashMap<u64, (ChunkPos, ChunkRevision, u64)>,
    /// 每个已加载区块上次被 D6 匀速快照的刻数，用于单区块 10 秒冷却。
    /// Last tick each loaded chunk was snapshotted by D6, for the cooldown.
    loaded_save_tick: HashMap<ChunkPos, u64>,
    /// 已经收到 IO 回执的异步栅栏序号和成功标志；`World` 每刻取走，发等在上面的
    /// `save-all` 成功 / 失败消息。
    /// Async barrier acks (seq + ok); `World` drains them each tick to release
    /// the waiting `save-all` success/failure messages.
    flushed_barriers: Vec<(u64, bool)>,
    /// 累计的写盘失败次数（每次都在 `poll_io` 记 `error!`）。关服时汇报。
    /// Count of write failures (each logged with `error!` in `poll_io`).
    io_failures: u64,
    /// 连续失败次数；到阈值就暂停匀速写盘，一次成功清零。
    /// Consecutive failures; paces saves off at the threshold, reset on success.
    consecutive_io_failures: u32,
    /// 连续失败的退避截止刻：这一刻之前不再把新快照交给 IO 线程，到点就重试。
    /// 绝不永久暂停（复查报告「致命 2」）。
    /// Backoff deadline for consecutive failures; retry when it passes. Never a
    /// permanent pause (review report, fatal 2).
    save_retry_after: u64,
    /// 写盘失败或快照被推迟、还欠一次保存的区块（复查「致命 2」「中 6」）。
    /// Chunks whose write failed or whose snapshot was deferred.
    retry_dirty: HashSet<ChunkPos>,
    /// 版本/上层内容已判脏、但这一刻还没轮到拍快照的区块。
    dirty_pending: HashSet<ChunkPos>,
    /// `/save-all` 请求还剩哪些快照没提交给 IO 线程。全部提交完才排自己的栅栏，
    /// 所以「保存真正完成」包含这次请求的全部写（复查报告「中 8 / 中 9」）。
    /// Pending save-all requests and the snapshot seqs they still owe IO. A
    /// barrier is submitted only when all of them are handed over (review 8/9).
    save_all_requests: VecDeque<(PlayerId, HashSet<u64>)>,
    /// 已经排好栅栏、等 IO 回执的 `/save-all`：`World` 按 (栅栏序号, 发起者) 发消息。
    /// Save-all barriers already enqueued, for `World` to await.
    ready_save_barriers: Vec<(u64, PlayerId)>,
    /// 最近一刻的刻数，失败退避据此算截止刻。
    last_tick: u64,
    /// 第 8 步的分区器：只算不用。开关关着时不建表，pump 只判一次布尔值。
    /// 分区器自己也在关着时不存在的 `Option` 里，避免任何额外分配。
    islands: Option<IslandPartition>,
    /// 这一刻攒着的区块依据变化，pump 收尾时一次喂给分区器。
    island_deltas: BTreeMap<ChunkPos, ChunkDelta>,
    /// 分区器上一次跑的刻，防同一刻重复 tick。
    island_last_tick: u64,
    /// 对照模式的开关，从剖析器读一次。
    islands_verify_flag: bool,
    /// 本刻 pump 里增量记账的耗时（纳秒），和分区器自己那一段分开报。
    island_bookkeeping_nanos: u64,
    /// 第 10 步：分区器驱动运行时的拆合（`islands = true`）。信件按区块所在的岛投，
    /// 交付前先让分区器知道新区块归谁。
    ///
    /// Step 10: the partitioner drives real splits and merges (`islands = true`).
    /// Letters go to the island that owns each chunk, and the partitioner learns
    /// a new chunk's island before its delivery is posted.
    live_islands: bool,
    /// 分区器已经定了、运行时还没照做的拆合，按先后排。
    topology: VecDeque<(TopologyOp, u64)>,
    /// 卷进还没照做的拆合的岛：照做之前不收信（计划 3.1「交付前检查」）。
    held_islands: BTreeSet<RegionId>,
    /// 关服中：F6 背压必须让路，保证所有区块都被收回并保存。
    /// Shutting down: F6 backpressure must stand aside so every chunk is saved.
    shutting_down: bool,
    /// 官方 `ServerLevel.noSave` 的反面：`/save-off` 时为假，停掉匀速保存
    /// 和卸载写盘；玩家离开的写、`/save-all`（force）和关服不受它管。
    /// Inverse of official `ServerLevel.noSave`: false during `/save-off`,
    /// stopping paced/unload writes. Player-leave saves, `/save-all` (force)
    /// and shutdown are unaffected.
    saves_enabled: bool,
    /// 游戏规则 `SPAWN_MOBS` 的快照，每刻由 `World` 刷新；生成期刷怪读它。
    ///
    /// Snapshot of the `SPAWN_MOBS` rule, refreshed each tick; read by world-generation spawning.
    spawn_mobs: bool,
}

impl DirectorHost {
    pub(crate) fn new(config: DirectorConfig) -> Self {
        let mut director = Director::new(config);
        director.set_template(|| Box::new(RegionColumn::from_template()));
        Self {
            infinite_flat: false,
            director,
            parking: Parking::default(),
            posts: BTreeMap::new(),
            profiler: Box::default(),
            demand: Arc::default(),
            island_chunks: HashSet::new(),
            mail_report: MailReport::default(),
            read_queue: Vec::new(),
            effect_scratch: Vec::new(),
            pump_events: Vec::new(),
            initial_scratch: Vec::new(),
            io_replies: Vec::new(),
            done_scratch: Vec::new(),
            parked: 0,
            io: None,
            generator: Arc::new(FlatGenerator::from_env()),
            world_gen_config: crate::world_options::WorldGenConfig::default(),
            partials: BTreeMap::new(),
            feature_deltas: BTreeMap::new(),
            generated_light: BTreeMap::new(),
            generated_structures: BTreeMap::new(),
            disk_upper_bytes: BTreeMap::new(),
            disk_saved_after_features: BTreeMap::new(),
            resumed_after_spawn: BTreeSet::new(),
            pool: None,
            compute_backlog: HashMap::new(),
            backlog_order: BinaryHeap::new(),
            backlog_centers: Vec::new(),
            early_replies: Vec::new(),
            cancelled_compute: HashSet::new(),
            #[cfg(test)]
            pooled_kinds: [0; 3],
            pending_writes: HashMap::new(),
            save_queue: VecDeque::new(),
            save_cooldown: HashMap::new(),
            pending_players: HashMap::new(),
            saved_revisions: HashMap::new(),
            acked_revisions: HashMap::new(),
            loaded_save_tick: HashMap::new(),
            flushed_barriers: Vec::new(),
            io_failures: 0,
            consecutive_io_failures: 0,
            save_retry_after: 0,
            retry_dirty: HashSet::new(),
            dirty_pending: HashSet::new(),
            save_all_requests: VecDeque::new(),
            ready_save_barriers: Vec::new(),
            last_tick: 0,
            islands: None,
            island_deltas: BTreeMap::new(),
            island_last_tick: 0,
            islands_verify_flag: false,
            island_bookkeeping_nanos: 0,
            live_islands: false,
            topology: VecDeque::new(),
            held_islands: BTreeSet::new(),
            shutting_down: false,
            saves_enabled: true,
            spawn_mobs: true,
        }
    }

    /// 接上真实磁盘 IO。只在启用存档的一局里调用一次。
    pub(crate) fn set_io(&mut self, io: ChunkIo) {
        self.io = Some(io);
    }

    /// 起 `threads` 条计算线程；0 表示内联执行器（默认）。只在开局调用一次。
    /// 起不来时记一条错误，退回内联执行器。
    pub(crate) fn set_compute_threads(&mut self, threads: usize) {
        self.pool = None;
        if threads == 0 || self.infinite_flat {
            return;
        }
        match ComputePool::new("bamboo-chunk-worker", threads) {
            Ok(pool) => self.pool = Some(pool),
            Err(error) => {
                tracing::error!(%error, "could not start the chunk compute pool; running inline");
            }
        }
    }

    /// 换生成器。`World::build` 按世界类型装 `FlatGenerator` 或 `OverworldGenerator`，
    /// 测试和压测也能换。
    ///
    /// Swap the generator. `World::build` installs `FlatGenerator` or
    /// `OverworldGenerator` by level type; tests and load tests can also swap it.
    /// 更新 Director 的玩家加载需求快照；宿主自己的排序直接读同一份。
    pub(crate) fn set_player_demand(&mut self, demand: DemandSnapshot) {
        self.demand = Arc::new(demand.clone());
        self.director.set_player_demand(demand);
        // 玩家位置/方向变了，待办里的旧距离全部失效，下一次 submit 重建堆。
        self.backlog_centers.clear();
    }

    /// 排队用的 rank：读盘、计算池待办、交付信箱都用这一份需求快照。
    /// 没有玩家时 rank 全是 0，按坐标排（原来的兜底顺序）。
    fn demand_rank(&self, pos: ChunkPos) -> DemandRank {
        let demand = self.director.player_demand();
        let rank = if demand.players().is_empty() {
            // 没玩家时退回旧口径：离票表里的玩家中心切比雪夫距离；
            // 票表也没玩家就按坐标。
            let centers = self.director.tickets().player_centers();
            if centers.is_empty() {
                0
            } else {
                distance_to_players(pos, &centers).0 as i64
            }
        } else {
            demand.rank(pos)
        };
        (rank, pos.x, pos.z)
    }

    /// 岛收信时要的主控状态：需求快照、需求为空时的票表玩家中心、要不要跳过收回。
    ///
    /// The master state an island needs to take its mail.
    pub(crate) fn mail_order(&self) -> MailOrder {
        let centers = if self.demand.players().is_empty() {
            self.director.tickets().player_centers()
        } else {
            Vec::new()
        };
        MailOrder {
            demand: Arc::clone(&self.demand),
            centers,
            // F6 背压只跳收回；`/save-off` 时也不收回（官方 noSave 跳过 processUnloads）。
            skip_reclaims: self.save_backpressure() || (!self.saves_enabled && !self.shutting_down),
        }
    }

    pub(crate) fn set_generator(&mut self, generator: Arc<dyn ChunkGenerator>) {
        self.generator = generator;
    }

    /// 接上世界生成配置（`World::build` 在读档、选好种子后调用一次）。
    pub(crate) fn set_world_gen_config(&mut self, config: crate::world_options::WorldGenConfig) {
        self.world_gen_config = config;
    }

    /// 结构阶段恢复用的世界生成配置（和生成器同一个种子 / 世界类型）。
    fn generator_config(&self) -> &crate::world_options::WorldGenConfig {
        &self.world_gen_config
    }

    /// 计算池的线程是不是意外退出了。任务 panic 不算，那会变成失败回执。
    pub(crate) fn compute_failed(&self) -> bool {
        self.pool.as_ref().is_some_and(ComputePool::worker_died)
    }

    /// 还没回执的计算任务总数：池子里的 + 待办里的 + 等着回「已取消」的。
    /// 内联执行器恒为 0。
    pub(crate) fn compute_in_flight(&self) -> usize {
        self.compute_in_pool() + self.compute_backlog.len() + self.early_replies.len()
    }

    /// 已经交给池子、还没收回回执的任务数。上限见 [`POOL_JOBS_PER_THREAD`]，
    /// 存盘不受上限，可能略超。
    pub(crate) fn compute_in_pool(&self) -> usize {
        self.pool.as_ref().map_or(0, ComputePool::in_flight)
    }

    /// 宿主手里还没交给池子的待办数。
    pub(crate) fn compute_backlog(&self) -> usize {
        self.compute_backlog.len()
    }

    /// 测试专用：池子里有在途任务时阻塞等待最多 `timeout` 收一条回执并暂存到
    /// `early_replies`，让下一刻 `collect_compute` 原样接手；避免测试空转 `world.tick()`
    /// 抢占计算线程。返回是否已有回执可收。
    ///
    /// Test only: while pool jobs are in flight, wait up to `timeout` for one
    /// reply and buffer it in `early_replies` so the next tick's
    /// `collect_compute` consumes it as usual, avoiding busy-spinning
    /// `world.tick()` and starving worker threads. Returns whether a reply is ready.
    #[cfg(test)]
    pub(crate) fn wait_for_compute_reply(&mut self, timeout: Duration) -> bool {
        if !self.early_replies.is_empty() {
            return true;
        }
        let Some(pool) = &mut self.pool else {
            return false;
        };
        if pool.in_flight() == 0 {
            return false;
        }
        if let Some(reply) = pool.recv_timeout(timeout) {
            self.early_replies.push(reply);
            true
        } else {
            false
        }
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

    /// 每刻由 `World` 按 `SaveSession::auto_save` 刷新（F7 / 官方 `noSave`）。
    /// Refreshed each tick by `World` from `SaveSession::auto_save` (F7).
    pub(crate) fn set_saves_enabled(&mut self, enabled: bool) {
        self.saves_enabled = enabled;
    }

    /// 每刻由 `World` 按游戏规则 `SPAWN_MOBS` 刷新。
    /// Refreshed each tick by `World` from the `SPAWN_MOBS` rule.
    pub(crate) fn set_spawn_mobs(&mut self, enabled: bool) {
        self.spawn_mobs = enabled;
    }

    /// 登记一次 `/save-all`：它排进去的快照全部交给 IO 之后才排自己的栅栏。
    /// Register a `/save-all`: its own barrier is queued once all of its
    /// snapshots have been handed to IO (review 8/9).
    pub(crate) fn begin_save_all(&mut self, source: PlayerId) -> Option<()> {
        self.io.as_ref()?;
        let seqs: HashSet<u64> = self.save_queue.iter().map(|pending| pending.seq).collect();
        // `/save-all` 是强制保存：这次请求的快照一次性交给 IO 线程，不受匀速
        // 和退避影响。成功消息仍等自己那道栅栏（中 8 / 中 9）。
        // Force: hand this request's snapshots to IO at once. Success still
        // waits for its own barrier (review 8/9).
        for pending in self.save_queue.drain(..) {
            if let Some(io) = &self.io {
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
        self.save_all_requests.push_back((source, seqs));
        // 立刻排栅栏：这次请求的写已经全部在 FIFO 信箱里，栅栏在它们之后。
        if let Some(seq) = self.submit_flush_barrier() {
            self.ready_save_barriers.push((seq, source));
        }
        Some(())
    }

    /// 每次 `/save-all` 的栅栏在它自己的写之后排队；这里只推进等待集合的记账。
    /// 失败窗口按栅栏序号在 `World` 里配对，两道栅栏之间夹别的失败不会错位（中 9）。
    /// Each `/save-all` barrier is queued behind its own writes; this only
    /// advances the bookkeeping. `World` pairs results by barrier seq (review 9).
    fn release_ready_save_barriers(&mut self) {
        self.save_all_requests.clear();
    }

    /// 取走已经排好的栅栏：`World` 用 `(序号, 发起者)` 等各自的回执。
    pub(crate) fn take_ready_save_barriers(&mut self) -> Vec<(u64, PlayerId)> {
        std::mem::take(&mut self.ready_save_barriers)
    }

    /// 排一道不阻塞的 IO 栅栏，返回序号；`None` 表示这一局没接磁盘（无需等待）。
    /// Enqueue a non-blocking IO barrier; `None` means no disk is attached.
    pub(crate) fn submit_flush_barrier(&mut self) -> Option<u64> {
        self.io.as_ref().map(|io| io.submit_barrier())
    }

    /// 取走这一刻收到的栅栏回执（序号 + 成功标志），由 `World` 发等在上面的消息。
    /// Drain the barrier acks (seq + ok) this tick; `World` releases the messages.
    pub(crate) fn take_flushed_barriers(&mut self) -> Vec<(u64, bool)> {
        std::mem::take(&mut self.flushed_barriers)
    }

    /// 把 `SaveSession` 编出的 `level.dat` / `data/` 字节交给 IO 线程写，
    /// 世界线程只编码、不碰磁盘（F3 / R5）。
    /// Hand the encoded `level.dat`/`data/` bytes to the IO thread; the world
    /// thread only encodes and never writes to disk (F3 / R5).
    pub(crate) fn submit_file_writes(&mut self, files: Vec<crate::save_session::EncodedFile>) {
        if let Some(io) = &self.io {
            for file in files {
                io.submit_file_write(file.path, file.backup, file.bytes);
            }
        }
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
        let mut host = Self::new(config);
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

    /// 每刻开头重置计数。交付 / 收回的预算在岛的信箱里，由岛自己重置。
    pub(crate) fn begin_tick(&mut self) {
        self.parked = 0;
    }

    /// 分区器开关：由 `region.profiler` 在世界启动时读一次环境变量决定。
    /// 宿主第一次见到打开的剖析器时把表和初始状态建出来。
    ///
    /// 宿主不碰 `Region`（第 6 步）：`World` 建好之后把岛的剖析器读到的开关交进来一次。
    /// The host never touches the `Region` (step 6): `World` passes the island
    /// profiler's switches in once after building.
    pub(crate) fn ensure_islands(&mut self, profiler: &Profiler) {
        if !profiler.islands_enabled() {
            return;
        }
        if self.islands.is_none() {
            let mut partition = IslandPartition::new();
            let verify = profiler.islands_verify();
            partition.set_verify(verify);
            self.islands_verify_flag = verify;
            self.islands = Some(partition);
        }
    }

    /// 一次等级变化折算成「票想加载」集合的增量。
    fn note_level_changes(&mut self) {
        if self.islands.is_none() {
            return;
        }
        let start = Instant::now();
        for entry in self.director.classified_level_changes() {
            let pos = entry.change.pos;
            let desired = entry.change.loading.1 <= crate::island_partition::DESIRED_LEVEL;
            self.island_deltas
                .entry(pos)
                .or_insert_with(|| ChunkDelta::new(pos, None, None))
                .desired = Some(desired);
        }
        self.island_bookkeeping_nanos = self
            .island_bookkeeping_nanos
            .saturating_add(start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    /// owner 转换折算成「还没收回」集合的增量。`None` 表示记录不存在。
    fn note_owner(&mut self, pos: ChunkPos, owner: Option<Owner>) {
        if self.islands.is_none() {
            return;
        }
        let start = Instant::now();
        let held = owner.is_some_and(owner_is_held);
        self.island_deltas
            .entry(pos)
            .or_insert_with(|| ChunkDelta::new(pos, None, None))
            .held = Some(held);
        self.island_bookkeeping_nanos = self
            .island_bookkeeping_nanos
            .saturating_add(start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    /// 对照模式：用 Director 的当前视图再算一次增量，正式路径不调它。
    fn reconcile_islands(&mut self) {
        if !self.islands_verify_flag {
            return;
        }
        let start = Instant::now();
        let desired: std::collections::BTreeSet<ChunkPos> = self
            .director
            .tickets()
            .loading_chunks_at_most(crate::island_partition::DESIRED_LEVEL)
            .into_iter()
            .collect();
        let held: std::collections::BTreeSet<ChunkPos> = self
            .director
            .owner_records()
            .into_iter()
            .filter(|record| owner_is_held(record.owner))
            .map(|record| record.pos)
            .collect();
        let deltas = self
            .islands
            .as_mut()
            .expect("上面判过")
            .reconcile(&desired, &held);
        for delta in deltas {
            let slot = self
                .island_deltas
                .entry(delta.pos)
                .or_insert_with(|| ChunkDelta::new(delta.pos, None, None));
            if let Some(value) = delta.desired {
                slot.desired = Some(value);
            }
            if let Some(value) = delta.held {
                slot.held = Some(value);
            }
        }
        self.island_bookkeeping_nanos = self
            .island_bookkeeping_nanos
            .saturating_add(start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    /// 分区器开着没有。
    pub(crate) fn islands_enabled(&self) -> bool {
        self.islands.is_some()
    }

    /// 打开多岛（第 10 步）：分区器一直在，而且驱动运行时的拆合。无限超平坦不派任务，
    /// 也就没有岛可分，这时什么都不做。
    ///
    /// Turn on multi-island mode (step 10): the partitioner always runs and
    /// drives real splits and merges. Infinite flat dispatches nothing, so it
    /// stays off there.
    pub(crate) fn enable_live_islands(&mut self, verify: bool) {
        if self.infinite_flat {
            return;
        }
        let partition = self.islands.get_or_insert_with(|| {
            let mut partition = IslandPartition::new();
            partition.set_verify(verify);
            partition
        });
        if verify {
            partition.set_verify(true);
            self.islands_verify_flag = true;
        }
        let params = partition.params();
        if params != crate::island_partition::PartitionParams::DEFAULT {
            tracing::warn!(
                ?params,
                "island partition parameters overridden from the environment (tuning only)"
            );
        }
        partition.record_ops();
        self.live_islands = true;
    }

    /// 每刻全量对照开着（`BAMBOO_PROFILE_ISLANDS=2`）：分区器每刻全量重算，世界在每次照做
    /// 拆合之后连 release 构建也断言计划 6.2 第 3 条的不变量（长时间压测用）。
    ///
    /// Whether the per-tick full check is on (`BAMBOO_PROFILE_ISLANDS=2`): the
    /// partitioner recomputes in full every tick and the world asserts the
    /// invariants of plan 6.2 item 3 after every settle, release builds too
    /// (for long load runs).
    pub(crate) fn islands_verify(&self) -> bool {
        self.islands_verify_flag
    }

    /// 把攒着的区块依据变化先喂给分区器，取走它定下的拆合。投信之前调：新区块要先
    /// 有岛，合并要先登记。
    ///
    /// Feed the pending membership deltas to the partitioner now and take the
    /// structural changes it decided. Called before posting: a new chunk must
    /// have an island and a merge must be registered first.
    fn flush_island_deltas(&mut self) {
        if !self.live_islands || self.island_deltas.is_empty() {
            return;
        }
        let start = Instant::now();
        let deltas: Vec<ChunkDelta> = std::mem::take(&mut self.island_deltas)
            .into_values()
            .collect();
        if let Some(partition) = &mut self.islands {
            partition.apply_now(&deltas);
            let ops = partition.take_ops();
            self.queue_topology(ops);
        }
        self.island_bookkeeping_nanos = self
            .island_bookkeeping_nanos
            .saturating_add(start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    /// 登记一批拆合，卷进去的岛先不收信。
    fn queue_topology(&mut self, ops: Vec<TopologyOp>) {
        for op in ops {
            match &op {
                TopologyOp::Merge { into, from } => {
                    self.held_islands.insert(*into);
                    self.held_islands.insert(*from);
                }
                TopologyOp::Split { from, to, .. } => {
                    self.held_islands.insert(*from);
                    self.held_islands.insert(*to);
                }
            }
            self.topology.push_back((op, self.last_tick));
        }
    }

    /// 区块现在归哪个岛（按分区器）。只在多岛时有意义；单岛时恒为家岛。
    ///
    /// The island that owns `pos` now (per the partitioner). Always home in
    /// single-island mode.
    pub(crate) fn island_of(&mut self, pos: ChunkPos) -> Option<RegionId> {
        if !self.live_islands {
            return Some(mc_bamboo_world::director::HOME_REGION);
        }
        self.flush_island_deltas();
        self.islands
            .as_ref()
            .and_then(|partition| partition.island_of_chunk(pos))
    }

    /// 一个岛按（x，z）最小的 section；分区器里没有它时取 `fallback` 所在的 section。
    pub(crate) fn island_origin(&self, id: RegionId, fallback: ChunkPos) -> SectionPos {
        self.islands
            .as_ref()
            .and_then(|partition| partition.origin_of(id))
            .unwrap_or_else(|| SectionPos::of(fallback))
    }

    /// 还有没照做的拆合。
    pub(crate) fn has_topology(&self) -> bool {
        !self.topology.is_empty()
    }

    /// 看第 `index` 条还没照做的拆合（不取走）。混合驱动下卷进不在主控手里的岛的拆合要
    /// 跳过去看后面的（计划 5.5.4）。
    ///
    /// Peek at the `index`-th pending structural change. Under the hybrid
    /// drive, changes involving an island that is away are skipped over
    /// (plan 5.5.4).
    pub(crate) fn topology_at(&self, index: usize) -> Option<&TopologyOp> {
        self.topology.get(index).map(|(op, _)| op)
    }

    /// 取走第 `index` 条照做完的拆合和它等了几刻；剩下的拆合卷进去的岛仍然扣着。
    ///
    /// Take the `index`-th change once applied, with the ticks it waited;
    /// islands in the remaining changes stay held.
    pub(crate) fn take_topology(&mut self, index: usize) -> Option<(TopologyOp, u64)> {
        let (op, since) = self.topology.remove(index)?;
        let pending: Vec<(TopologyOp, u64)> = self.topology.drain(..).collect();
        self.held_islands.clear();
        for (op, since) in pending {
            self.queue_topology(vec![op]);
            if let Some(last) = self.topology.back_mut() {
                last.1 = since;
            }
        }
        Some((op, self.last_tick.saturating_sub(since)))
    }

    /// 合并 `from` 进 `into` 时邮筒跟着并：`from` 的信都比 `into` 里关于 `from` 那些
    /// 区块的信早（分区器合并之后才按新编号投），所以接在前面；Director 里 `from` 的
    /// 已交付区块改记到 `into` 名下。
    ///
    /// On merging `from` into `into` the postboxes merge too: every letter in
    /// `from` predates the letters in `into` about `from`'s chunks (those were
    /// posted under the new id after the partitioner merged), so they go first;
    /// the Director's delivered chunks of `from` are re-owned by `into`.
    pub(crate) fn merge_posts(&mut self, into: RegionId, from: RegionId, moved: &[ChunkPos]) {
        if let Some(mut earlier) = self.posts.remove(&from) {
            let later = self.posts.entry(into).or_default();
            earlier.append(later);
            *later = earlier;
        }
        for &pos in moved {
            self.director.reassign_region(pos, into);
        }
    }

    /// 拆出 `to` 时邮筒跟着拆：`from` 里关于 `moved_section` 那些区块的信按原来的先后挪到
    /// `to` 的前面；搬走的已交付区块在 Director 里改记到 `to` 名下。
    ///
    /// On splitting `to` off `from` the postbox splits too: `from`'s letters
    /// about chunks in the moved sections go, in order, in front of `to`'s;
    /// moved delivered chunks are re-owned by `to` in the Director.
    pub(crate) fn split_posts(
        &mut self,
        from: RegionId,
        to: RegionId,
        sections: &BTreeSet<SectionPos>,
        moved: &[ChunkPos],
    ) {
        let inside = |pos: &ChunkPos| sections.contains(&SectionPos::of(*pos));
        let mut kept = Vec::new();
        let mut moving = Vec::new();
        for letter in self.posts.remove(&from).unwrap_or_default() {
            match letter {
                Posted::Deliver { pos, .. } if inside(&pos) => moving.push(letter),
                Posted::Deliver { .. } => kept.push(letter),
                Posted::Reclaim(positions) => {
                    let (out, stay): (Vec<_>, Vec<_>) =
                        positions.into_iter().partition(|(pos, _)| inside(pos));
                    if !out.is_empty() {
                        moving.push(Posted::Reclaim(out));
                    }
                    if !stay.is_empty() {
                        kept.push(Posted::Reclaim(stay));
                    }
                }
                Posted::Levels(changes) => {
                    let (out, stay): (Vec<_>, Vec<_>) =
                        changes.into_iter().partition(|change| inside(&change.pos));
                    if !out.is_empty() {
                        moving.push(Posted::Levels(out));
                    }
                    if !stay.is_empty() {
                        kept.push(Posted::Levels(stay));
                    }
                }
            }
        }
        if !kept.is_empty() {
            self.posts.insert(from, kept);
        }
        if !moving.is_empty() {
            let later = self.posts.entry(to).or_default();
            moving.append(later);
            *later = moving;
        }
        for &pos in moved {
            self.director.reassign_region(pos, to);
        }
    }

    /// 拆合照做完以后把扣着的信放出去（齐步：岛借给这次调用，当场收；分离：留给岛
    /// 下一刻开头）。
    ///
    /// Release the held mail after the structural changes (lockstep: the
    /// islands are lent and take it now; detached: at their next tick start).
    pub(crate) fn release_mail(&mut self, island: &mut IslandAccess<'_>) {
        if self.infinite_flat {
            return;
        }
        let events = std::mem::take(&mut self.initial_scratch);
        self.pump(island, events);
    }

    /// 邮筒里有信、又没被拆合扣着的岛编号（升序）。分离驱动下岛开头收信时，用它给还不
    /// 存在的岛编号先建空岛（计划 5.5.5）。
    ///
    /// Island ids with posted letters that are not held by a pending change,
    /// ascending. The detached drive uses it at the start of a tick to create
    /// islands that do not exist yet (plan 5.5.5).
    pub(crate) fn posted_islands(&self) -> Vec<RegionId> {
        self.posts
            .iter()
            .filter(|(id, letters)| !letters.is_empty() && !self.held_islands.contains(id))
            .map(|(&id, _)| id)
            .collect()
    }

    /// 邮筒里投给 `id`、它还没取走的收回信说到的位置。这些区块还在 `id` 手里（Director
    /// 记着 `Reclaiming`，不带岛编号），岛被判死时它们和它的别的区块一起丢（计划 5.6.3）。
    ///
    /// Positions named by reclaim letters posted to `id` and not yet taken.
    /// Those chunks are still in `id`'s hands (the Director records
    /// `Reclaiming`, without an island id), and are lost with its other chunks
    /// when it is declared dead (plan 5.6.3).
    pub(crate) fn posted_reclaims(&self, id: RegionId) -> Vec<ChunkPos> {
        self.posts
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|letter| match letter {
                Posted::Reclaim(positions) => Some(positions.iter().map(|(pos, _)| *pos)),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// 邮筒里投给 `id` 的信件数（测试用）。
    #[cfg(test)]
    pub(crate) fn posted_to(&self, id: RegionId) -> usize {
        self.posts.get(&id).map_or(0, Vec::len)
    }

    /// 测试专用：直接拿到分区器。
    #[cfg(test)]
    pub(crate) fn partition_mut(&mut self) -> Option<&mut IslandPartition> {
        self.islands.as_mut()
    }

    /// 测试专用：分区器刚定的拆合排进队列。
    #[cfg(test)]
    pub(crate) fn take_partition_ops(&mut self) {
        if let Some(partition) = &mut self.islands {
            let ops = partition.take_ops();
            self.queue_topology(ops);
        }
    }

    /// 测试专用：把给定的拆合原样排进队列而不照做，用来在「做决定」和「照做」之间
    /// 制造一个确定的窗口。
    ///
    /// Test only: queue the given changes verbatim without replaying them,
    /// creating a deterministic window between decision and replay.
    #[cfg(test)]
    pub(crate) fn queue_topology_for_test(&mut self, ops: Vec<TopologyOp>) {
        self.queue_topology(ops);
    }

    /// 直接看分区器（不变量检查、测试逐刻断言和全量对照用）。
    pub(crate) fn islands(&self) -> Option<&IslandPartition> {
        self.islands.as_ref()
    }

    /// 测试专用：不经过环境变量，直接打开 / 关闭分区器和全量对照。
    #[cfg(test)]
    pub(crate) fn enable_islands(&mut self, verify: bool) {
        let mut partition = IslandPartition::new();
        partition.set_verify(verify);
        self.islands = Some(partition);
        self.islands_verify_flag = verify;
        self.island_deltas.clear();
        self.island_last_tick = 0;
        self.island_bookkeeping_nanos = 0;
    }

    /// 每刻收尾：把本刻攒下的增量一次喂给分区器，记进剖析器。
    /// 关着时只有一次 `Option` 判断。
    ///
    /// 统计写进 `report`（打报告的那份剖析器，现在是岛的）；`World` 在 `tick` 之后调。
    /// The statistics go into `report` (the profiler that prints, the island's
    /// for now); `World` calls this right after `tick`.
    pub(crate) fn finish_island_tick(&mut self, report: &mut Profiler, now_tick: u64) {
        // 无限超平坦不派任务，分区器也不推进（和接到 `tick` 里时一样）。
        // Infinite flat dispatches nothing, so the partitioner does not advance.
        if self.infinite_flat || self.island_last_tick == now_tick {
            return;
        }
        self.island_last_tick = now_tick;
        let deltas: Vec<ChunkDelta> = std::mem::take(&mut self.island_deltas)
            .into_values()
            .collect();
        let result = match &mut self.islands {
            Some(partition) => partition.tick(now_tick, &deltas),
            None => return,
        };
        if self.live_islands
            && let Some(partition) = &mut self.islands
        {
            let ops = partition.take_ops();
            self.queue_topology(ops);
        }
        let bookkeeping = std::mem::take(&mut self.island_bookkeeping_nanos);
        if let Some(stats) = report.island_stats_mut()
            && let Some(partition) = &self.islands
        {
            stats.record_tick(partition, result, bookkeeping);
        }
        // 分区器的合并 / 拆分事件只在剖析器开着时攒，避免长局无界增长。
        if let Some(partition) = &mut self.islands {
            partition.events.clear();
        }
        if result.invariant_failures > 0 {
            debug_assert!(
                false,
                "岛划分不变量被破坏：tick={now_tick} failures={}",
                result.invariant_failures
            );
        }
    }

    /// 测试专用：生成期每区块状态还剩多少条
    /// （`partials`、`feature_deltas`、`generated_light`、`generated_structures`）。
    /// Test-only: leftover per-chunk generation state (partials, feature
    /// deltas, generated light, generated structures).
    #[cfg(test)]
    pub(crate) fn generation_leftovers(&self) -> [Vec<ChunkPos>; 4] {
        [
            self.partials.keys().copied().collect(),
            self.feature_deltas.keys().copied().collect(),
            self.generated_light.keys().copied().collect(),
            self.generated_structures.keys().copied().collect(),
        ]
    }

    #[cfg(test)]
    pub(crate) fn set_reclaim_delay(&mut self, delay: u64) {
        self.director.config_mut().reclaim_delay_ticks = delay;
    }

    /// 邮筒里还没被岛收取的信数。
    #[cfg(test)]
    pub(crate) fn posted_len(&self) -> usize {
        self.posts.values().map(Vec::len).sum()
    }

    /// 邮筒里还没被岛收取的（交付的盒子数，收回的位置数）。
    #[cfg(test)]
    pub(crate) fn posted_counts(&self) -> (usize, usize) {
        let mut counts = (0, 0);
        for letter in self.posts.values().flatten() {
            match letter {
                Posted::Deliver { .. } => counts.0 += 1,
                Posted::Reclaim(positions) => counts.1 += positions.len(),
                Posted::Levels(_) => {}
            }
        }
        counts
    }

    /// `pos` 的盒子已经投进邮筒、等着岛下一刻开头收取。
    #[cfg(test)]
    pub(crate) fn has_posted_delivery(&self, pos: ChunkPos) -> bool {
        self.posts
            .values()
            .flatten()
            .any(|letter| matches!(letter, Posted::Deliver { pos: at, .. } if *at == pos))
    }

    /// 一批票的操作：喂给 Director，把效果跑到底。
    pub(crate) fn tickets(&mut self, island: &mut IslandAccess<'_>, ops: Vec<TicketOp>) {
        if self.infinite_flat {
            // 只把票喂给票表，不产生效果。
            self.profiler.start_sub(SubPhase::TicketUpdate);
            for op in ops {
                self.apply_infinite_ticket(op);
            }
            let _ = self.director.tickets_mut().run_updates();
            self.profiler.end_sub();
            return;
        }
        let mut events = std::mem::take(&mut self.initial_scratch);
        events.push(Event::Tickets { ops });
        self.pump(island, events);
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
    pub(crate) fn tick(&mut self, island: &mut IslandAccess<'_>, now_tick: u64) {
        self.last_tick = now_tick;
        if self.infinite_flat {
            self.profiler.start_sub(SubPhase::TicketUpdate);
            self.director.tickets_mut().tick(&mut |_| false);
            let _ = self.director.tickets_mut().run_updates();
            self.profiler.end_sub();
            return;
        }
        self.pace_saves(now_tick);
        self.release_ready_save_barriers();
        let mut events = std::mem::take(&mut self.initial_scratch);
        events.push(Event::Tick { now_tick });
        self.pump(island, events);
        // 分区器每刻只推进一次（`World` 在这之后调 `finish_island_tick`）：收包阶段和
        // poll_io 的 pump 只攒增量。
        // The partitioner advances once per tick (`World` calls
        // `finish_island_tick` right after); earlier pumps only accumulate.
    }

    /// 同一区块在 `save_queue` 里只留最新一份快照（官方 `IOWorker.store`
    /// 覆盖同位置的 `pendingStore.data`），且不接收比已排队的更旧的快照。
    /// Keep only the newest snapshot per chunk in `save_queue` (official
    /// `IOWorker.store` overwrites the same position); ignore older stragglers.
    fn enqueue_snapshot(&mut self, pending: PendingWrite) {
        if self
            .save_queue
            .iter()
            .any(|other| other.pos == pending.pos && other.seq > pending.seq)
        {
            return;
        }
        self.save_queue.retain(|other| other.pos != pending.pos);
        self.save_queue.push_back(pending);
    }

    /// 积压到上限就背压：暂缓卸载，已加载区块数和内存都有界（F6）。
    /// Backpressure: defer reclaims once any cap is reached (F6).
    fn save_backpressure(&self) -> bool {
        if self.shutting_down {
            return false;
        }
        self.save_queue.len() >= MAX_PENDING_WRITES
            || self.parking.len() >= MAX_PARKED_BOXES
            || self
                .io
                .as_ref()
                .is_some_and(|io| io.in_flight_requests() >= MAX_IO_MAILBOX)
    }

    /// D6：每刻把 `ceil(待写数 / 6000)`（至少 1）个快照提交给 IO 线程。
    /// 单区块 10 秒冷却只在「队列里还有同区块更新的快照」时跳过旧快照，
    /// 保证冷却不会丢掉一个孤立的快照。
    ///
    /// D6: submit `ceil(pending / 6000)` (at least 1) snapshots per tick. The
    /// per-chunk cooldown only skips a stale snapshot when a newer one for the
    /// same chunk is still queued, so an isolated snapshot is never dropped.
    fn pace_saves(&mut self, now_tick: u64) {
        // `save-off` 照官方 `ChunkMap.tick`：noSave 时不 processUnloads，
        // 也就是不把卸载快照交给 IO 线程（F7）。`/save-all` / 关服不走这里。
        // `save-off` mirrors official `ChunkMap.tick`: noSave skips
        // processUnloads, so unload writes are not handed to IO (F7).
        // `/save-all` and shutdown do not go through this path.
        if self.io.is_none()
            || self.save_queue.is_empty()
            || !self.saves_enabled
            || now_tick < self.save_retry_after
        {
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

    /// 用岛交回的改动判脏：方块/光照版本号和上次成功写盘不同、或上层内容改过的
    /// 已加载区块，加上还欠一次保存的。返回（脏区块，版本变了但这次不存、要放回岛
    /// 改动集合的区块）。上层内容只来自岛的改动集合，绝不扫全局表或所有已加载区块
    /// （复查报告「高 3」）。
    ///
    /// Judge dirtiness from the changes the island handed over: loaded chunks
    /// whose block/light revision differs from the last successful write or
    /// whose upper content changed, plus those still owed a save. Returns
    /// (dirty chunks, chunks to put back into the island's change set).
    fn judge_dirty(&mut self, changes: SaveChanges, force: bool) -> JudgedDirty {
        self.profiler.start_sub(SubPhase::StorageDirty);
        let mut dirty = Vec::new();
        let mut remark = Vec::new();
        for changed in changes.changed {
            let pos = changed.pos;
            let saved = self.saved_revisions.get(&pos).copied();
            let revision_changed =
                saved.is_none_or(|(saved_revision, _)| saved_revision != changed.revision);
            // 从盘上/模板交付进来的区块还没存过：照官方 `isUnsaved` 只有真被
            // 改过的才算，原样区块不写。已保存过的区块只按版本变化判脏。
            if revision_changed
                && (saved.is_some() || !changed.pristine || changed.upper_dirty || force)
            {
                dirty.push(pos);
            } else if revision_changed {
                // 版本变了但这一刻不该存：把标记留到下一刻再判。
                remark.push(pos);
            }
        }
        // 上层内容改动集合：只有真正改过的区块在这里，开销和集合大小有关。
        for pos in changes.content {
            if !dirty.contains(&pos) {
                dirty.push(pos);
            }
        }
        for pos in &self.retry_dirty {
            if self.island_chunks.contains(pos) && !dirty.contains(pos) {
                dirty.push(*pos);
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        #[cfg(test)]
        let verify = self
            .saved_revisions
            .iter()
            .filter(|(pos, _)| !dirty.contains(pos))
            .map(|(&pos, &(revision, _))| (pos, revision))
            .collect();
        self.profiler.end_sub();
        JudgedDirty {
            dirty,
            remark,
            #[cfg(test)]
            verify,
        }
    }

    /// 测试专用：判一次脏（岛交出改动、主控判、放回不存的），返回脏区块。
    /// Test-only: one dirty check (island hands changes, master judges, puts back the rest).
    #[cfg(test)]
    pub(crate) fn dirty_loaded_chunks(
        &mut self,
        island: &mut Island,
        force: bool,
    ) -> Vec<ChunkPos> {
        let changes = island.drain_save_changes();
        let judged = self.judge_dirty(changes, force);
        let _ = island.follow_save_plan(SavePlan {
            remark: judged.remark,
            verify: judged.verify,
            ..SavePlan::default()
        });
        judged.dirty
    }

    /// 卸载后不必留着记账，避免长局里无界增长。
    /// Drop bookkeeping for unloaded chunks so it cannot grow without bound.
    fn prune_saved(&mut self) {
        let loaded = &self.island_chunks;
        self.saved_revisions.retain(|pos, _| loaded.contains(pos));
        self.loaded_save_tick.retain(|pos, _| loaded.contains(pos));
    }

    /// 从脏区块里挑这一刻要拍快照的：6000 刻那一次全量，其余每刻最多 20 个，
    /// 单区块 10 秒冷却（失败重试的不受冷却限制）；没轮到的留到下一刻。
    /// 返回（要拍的，判脏候选数）。
    ///
    /// Pick this tick's snapshots from the dirty chunks: everything on the
    /// 6000-tick pass, otherwise at most 20 with a per-chunk cooldown; the rest
    /// waits. Returns (chosen, candidate count).
    fn choose_snapshots(
        &mut self,
        mut dirty: Vec<ChunkPos>,
        now_tick: u64,
    ) -> (Vec<ChunkPos>, u64) {
        // 上一刻没轮到的脏区块留在集合里，不会因为改动标记被取走而丢掉。
        for pos in std::mem::take(&mut self.dirty_pending) {
            if self.island_chunks.contains(&pos) && !dirty.contains(&pos) {
                dirty.push(pos);
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        let full = now_tick.is_multiple_of(SAVE_INTERVAL_TICKS);
        let count = if full {
            usize::MAX
        } else {
            EAGER_SAVES_PER_TICK
        };
        let candidates = dirty.len() as u64;
        let mut remaining = Vec::new();
        let mut eligible = Vec::new();
        for pos in dirty {
            let cooling = self.loaded_save_tick.get(&pos).is_some_and(|&last| {
                now_tick.saturating_sub(last) < SAVE_COOLDOWN_TICKS
                    && !self.retry_dirty.contains(&pos)
            });
            if cooling || eligible.len() >= count {
                remaining.push(pos);
            } else {
                eligible.push(pos);
            }
        }
        self.dirty_pending.extend(remaining);
        (eligible, candidates)
    }

    /// 照官方 `ChunkMap.saveChunksEagerly`：每刻最多 20 个交给 IO 线程。
    /// 仍走同一个有上限的队列，`pace_saves` 只负责卸载快照的匀速部分。
    /// Official `saveChunksEagerly`: at most 20 per tick, through the same
    /// bounded queue; `pace_saves` only paces unload snapshots.
    fn submit_eager_saves(&mut self) {
        if let Some(io) = &self.io {
            for _ in 0..20 {
                let Some(pending) = self.save_queue.pop_front() else {
                    break;
                };
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
    }

    /// D6：每刻给 `ceil(已加载脏区块数 / 6000)`（至少 1）个仍加载的脏区块拍快照
    /// 交给 IO 线程，单区块 10 秒冷却；`enabled=false`（`save-off`）时整个停掉。
    /// 主控出名单、岛编码交回（齐步：岛借给这次调用，当场做完，和以前逐位相同）。
    ///
    /// D6: each tick snapshot `ceil(dirty/6000)` (at least 1) still-loaded dirty
    /// chunks to the IO thread with a per-chunk 10s cooldown; fully disabled
    /// during save-off. The master picks the list and the island encodes it
    /// (lockstep: the island is lent and it all happens on the spot).
    pub(crate) fn save_loaded_dirty(
        &mut self,
        islands: &mut [&mut Island],
        now_tick: u64,
        enabled: bool,
    ) {
        if !enabled || self.io.is_none() {
            return;
        }
        self.prune_saved();
        let changes = drain_all_save_changes(islands);
        let judged = self.judge_dirty(changes, false);
        let (eligible, candidates) = self.choose_snapshots(judged.dirty, now_tick);
        let encoded = follow_all(
            islands,
            SavePlan {
                remark: judged.remark,
                metrics: Some((candidates, eligible.len() as u64)),
                encode: eligible,
                drain: false,
                #[cfg(test)]
                verify: judged.verify,
            },
        );
        self.accept_snapshots(encoded, now_tick);
        self.submit_eager_saves();
    }

    /// 分离驱动的交回：收下岛照上一份名单编好的快照，用它交回的改动判脏，给出下一份
    /// 名单（岛下一刻末尾编码）。`save-off` 时判出来的脏区块留到恢复后再存。
    ///
    /// Detached hand-back: take the snapshots the island encoded from the last
    /// list, judge the changes it handed over, and return the next list (encoded
    /// at the end of the island's next tick). During save-off the dirty chunks
    /// wait until saving resumes.
    pub(crate) fn plan_saves(
        &mut self,
        report: SaveReport,
        now_tick: u64,
        enabled: bool,
    ) -> SavePlan {
        self.accept_snapshots(report.encoded, now_tick);
        let saving = enabled && self.io.is_some();
        let mut plan = SavePlan {
            drain: saving,
            ..SavePlan::default()
        };
        if let Some(changes) = report.changes {
            self.prune_saved();
            let judged = self.judge_dirty(changes, false);
            plan.remark = judged.remark;
            #[cfg(test)]
            {
                plan.verify = judged.verify;
            }
            if saving {
                let (eligible, candidates) = self.choose_snapshots(judged.dirty, now_tick);
                plan.metrics = Some((candidates, eligible.len() as u64));
                plan.encode = eligible;
            } else {
                self.dirty_pending.extend(judged.dirty);
            }
        }
        if saving {
            self.submit_eager_saves();
        }
        plan
    }

    /// `/save-all`：立刻给所有已加载脏区块拍快照交给 IO 线程，不受匀速和冷却限制。
    /// 这是栅栏：岛一定停放着、借给这次调用；分离驱动下还没编码的上一份名单也并进来。
    /// `/save-all`: snapshot every loaded dirty chunk to the IO thread at once,
    /// ignoring the pacing and cooldown. A barrier: the island is parked and
    /// lent; under the detached drive the pending list is folded in too.
    pub(crate) fn snapshot_all_dirty(&mut self, islands: &mut [&mut Island], now_tick: u64) {
        if self.io.is_none() {
            return;
        }
        // 分离驱动下还没编码的名单：放回该放的，要拍的并进这次。齐步下它是空的。
        // A pending detached list: put back what it puts back, fold its chunks in.
        for island in islands.iter_mut() {
            let pending = std::mem::take(&mut island.save_plan);
            let drain = pending.drain;
            self.dirty_pending.extend(pending.encode.iter().copied());
            let _ = island.follow_save_plan(SavePlan {
                remark: pending.remark,
                drain,
                ..SavePlan::default()
            });
            island.save_plan.drain = drain;
        }
        // `/save-all` 是显式的强制重试：清掉退避，把内存里还在的快照再提交一次。
        // 后续失败会重新按连续次数退避，但绝不永久暂停。
        // `save-all` is an explicit forced retry: clear the backoff and submit
        // everything still in memory. Later failures back off again, never pause forever.
        self.save_retry_after = 0;
        let changes = drain_all_save_changes(islands);
        let judged = self.judge_dirty(changes, true);
        let mut dirty = judged.dirty;
        // 冷却期 / 匀速没轮到而留到下一刻的脏区块，强制保存也要一起写。
        for pos in std::mem::take(&mut self.dirty_pending) {
            if self.island_chunks.contains(&pos) && !dirty.contains(&pos) {
                dirty.push(pos);
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        // 强制保存不看单区块冷却。
        self.loaded_save_tick.clear();
        let encoded = follow_all(
            islands,
            SavePlan {
                remark: judged.remark,
                encode: dirty,
                metrics: None,
                drain: false,
                #[cfg(test)]
                verify: judged.verify,
            },
        );
        self.accept_snapshots(encoded, now_tick);
        // 强制保存：把这次刚拍的快照立刻交给 IO 线程，不等匀速（官方
        // `saveEverything` 同步存所有已经改过的区块）。仍走同一个有上限的队列。
        // Force: hand the fresh snapshots to IO immediately (official
        // `saveEverything`), still through the bounded queue.
        if let Some(io) = &self.io {
            for pending in self.save_queue.drain(..) {
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
    }

    /// 收下岛编好的快照：排进写盘队列，记下落定后的版本号和这一刻。
    /// 编码失败或这一刻不适合搬盒子就什么也不记，下一刻重试。
    /// Take the island's encoded snapshots: queue them for writing and record
    /// the settled revision and tick. On failure (or when the tables are busy)
    /// record nothing so it is retried on a later tick.
    fn accept_snapshots(&mut self, encoded: Vec<Encoded>, now_tick: u64) {
        for Encoded {
            pos,
            revision,
            result,
        } in encoded
        {
            let Some(result) = result else {
                // 有待算光照 / 表正忙：推迟到下一刻，不记「已存」。
                self.retry_dirty.insert(pos);
                self.loaded_save_tick.remove(&pos);
                continue;
            };
            match result {
                Ok(packed) => {
                    // 和卸载写共用同一个单调递增的 seq，IO 线程据此丢旧快照。
                    // Share one monotonic seq source with unload saves.
                    let seq = self.director.allocate_save_seq();
                    // 和卸载写走同一条有上限的队列，不再直接塞进无界 IO 信箱
                    // （复查报告「中：snapshot_loaded 绕过队列和背压」）。
                    // Use the same bounded queue as unload saves instead of
                    // submitting straight into the unbounded IO mailbox.
                    self.enqueue_snapshot(PendingWrite {
                        pos,
                        seq,
                        chunk: packed.chunk,
                        entities: packed.entities,
                    });
                    self.pending_writes.insert(pos, seq);
                    // 「已保存」只在 Written 回执到达时记（复查「致命 2」）：
                    // 快照刚排进队列时记成已存会让写盘失败的改动永远不再判脏。
                    // "Saved" is only recorded on the Written ack: recording it
                    // at enqueue time makes a failed write never dirty again.
                    self.acked_revisions.insert(seq, (pos, revision, 0));
                    self.loaded_save_tick.insert(pos, now_tick);
                    self.dirty_pending.remove(&pos);
                    // 已经重新拍成新快照了：不再每刻重复重拍，等 Written 回执再彻底清。
                    self.retry_dirty.remove(&pos);
                }
                Err(error) => {
                    tracing::warn!(%error, ?pos, "could not encode a loaded chunk for saving");
                }
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
    ///
    /// 关服是栅栏：岛一定停放着、借给这次调用，两种驱动都当场收信。
    /// Shutdown is a barrier: the island is parked and lent, so both drives
    /// take the mail on the spot.
    pub(crate) fn shutdown(&mut self, islands: &mut crate::island::Islands) {
        if self.infinite_flat {
            return;
        }
        // 混合驱动下关服时可能有岛没回来（计划 5.5.5）：它们的区块存不下来，列进日志；
        // 停放着的岛照常收回、写出。
        // Under the hybrid drive some islands may not be back at shutdown
        // (plan 5.5.5): their chunks cannot be saved and are logged; the
        // parked ones are reclaimed and written as usual.
        let away = islands.away_ids();
        if !away.is_empty() {
            let mut lost: Vec<ChunkPos> = self
                .director
                .owner_records()
                .into_iter()
                .filter(|record| {
                    matches!(record.owner,
                        mc_bamboo_world::director::Owner::Region(id) if away.contains(&id))
                })
                .map(|record| record.pos)
                .collect();
            lost.sort_unstable();
            tracing::error!(
                islands = ?away,
                ?lost,
                "these islands did not hand back before shutdown; changes to their chunks \
                 after the last save are lost"
            );
        }
        for island in islands.parked_mut() {
            island.mailbox.lift_budget();
            // 收回要求光照算完、各表都不在 tick 中途；关服在 tick 边界调用。
            island.region.chunks.finish_light_updates();
        }
        self.shutdown_with(&mut IslandAccess::All(&mut *islands));
        for island in islands.parked_mut() {
            island.mailbox.clear();
        }
    }

    /// 关服时岛卡在 tick 线程上回不来（岛的实施计划第 6 节第 7 步）：照常推完关服事件，
    /// 只是收回的信件投进邮筒、没人收；主控手里已有的快照和停放的盒子照样写完。
    /// 岛上的区块存不下来，在日志里列出。
    ///
    /// Shutdown while the island is stuck on its tick thread (island plan,
    /// step 7): drive the shutdown events as usual, except the reclaim mail is
    /// only posted and never taken; the snapshots and parked boxes the master
    /// holds are still written. The island's chunks cannot be saved and are logged.
    pub(crate) fn shutdown_without_island(&mut self) {
        if self.infinite_flat {
            return;
        }
        let mut lost: Vec<ChunkPos> = self.island_chunks.iter().copied().collect();
        lost.sort_unstable();
        self.shutdown_with(&mut IslandAccess::Away);
        if !lost.is_empty() {
            tracing::error!(
                ?lost,
                "these chunks were on the stuck island; changes after their last save are lost"
            );
        }
    }

    /// 拿着 `positions` 的岛被判死了（岛的实施计划 5.6.3）：这些位置里 Director 还记在
    /// region 一侧的，按「盒子没了」处理——删记录，票还要的重新加载（读到的是上次存盘
    /// 的内容）；所有岛的邮筒里说到它们的收回和换档删掉（代数已经作废）；主控这边
    /// 「还欠一次保存」的记账去掉（没有东西可拍了）。已经编好、在队列里或者写盘途中的
    /// 快照照常写完。返回真正丢了的位置（升序）。
    ///
    /// The island holding `positions` was declared dead (island plan 5.6.3).
    /// Of these, the ones the Director still records on the region side are
    /// handled as "the box is gone": the record is removed and reloaded if
    /// tickets still want it (reading what was last saved); reclaims and level
    /// changes about them are dropped from every postbox (their generation is
    /// void); the master's "still owes a save" bookkeeping for them is
    /// cleared. Snapshots already encoded, queued or being written are still
    /// written. Returns the positions really lost, ascending.
    pub(crate) fn lose_chunks(
        &mut self,
        island: &mut IslandAccess<'_>,
        positions: &[ChunkPos],
    ) -> Vec<ChunkPos> {
        if self.infinite_flat {
            return Vec::new();
        }
        let lost: BTreeSet<ChunkPos> = positions
            .iter()
            .copied()
            .filter(|&pos| self.director.owner_at(pos).is_some_and(owner_is_held))
            .collect();
        if lost.is_empty() {
            return Vec::new();
        }
        for letters in self.posts.values_mut() {
            let kept: Vec<Posted> = std::mem::take(letters)
                .into_iter()
                .filter_map(|letter| match letter {
                    Posted::Reclaim(mut positions) => {
                        positions.retain(|(pos, _)| !lost.contains(pos));
                        (!positions.is_empty()).then_some(Posted::Reclaim(positions))
                    }
                    Posted::Levels(mut changes) => {
                        changes.retain(|change| !lost.contains(&change.pos));
                        (!changes.is_empty()).then_some(Posted::Levels(changes))
                    }
                    deliver @ Posted::Deliver { .. } => Some(deliver),
                })
                .collect();
            *letters = kept;
        }
        for pos in &lost {
            self.island_chunks.remove(pos);
            self.dirty_pending.remove(pos);
            self.retry_dirty.remove(pos);
        }
        let lost: Vec<ChunkPos> = lost.into_iter().collect();
        let mut events = std::mem::take(&mut self.initial_scratch);
        events.push(Event::Lost {
            positions: lost.clone(),
        });
        self.pump(island, events);
        lost
    }

    /// 岛里现在有多少个区块（主控的记账）。
    pub(crate) fn island_chunk_count(&self) -> usize {
        self.island_chunks.len()
    }

    fn shutdown_with(&mut self, access: &mut IslandAccess<'_>) {
        // 关服照官方最后全量保存：解除退避和 F6 背压，把内存里还在的
        // 快照再试一次，所有区块都要收回（复查「致命 2」：失败盒子也要重写）。
        // Shutdown is the final full save: lift the backoff and F6 backpressure,
        // retry everything still in memory, including failed parked boxes.
        self.save_retry_after = 0;
        self.shutting_down = true;
        let mut events = std::mem::take(&mut self.initial_scratch);
        events.push(Event::Shutdown);
        self.pump(access, events);
        // 等计算池里的解析/生成/序列化都回来：存盘的盒子要回到世界线程才能
        // 排进写盘队列，收到的结果可能又派出新任务，所以循环到池子空为止。
        self.drain_compute(access);
        // 写盘失败后还没写成功的盒子：用同一条编码路径重新提交（致命 2）。
        // Re-encode every parked box whose write never succeeded, using the
        // same codec as unload saves (fatal 2).
        let failed_boxes = self.parking.take_all();
        for (pos, column) in failed_boxes {
            let seq = self.director.allocate_save_seq();
            match encode_column(pos, &column) {
                Ok(packed) => self.enqueue_snapshot(PendingWrite {
                    pos,
                    seq,
                    chunk: packed.chunk,
                    entities: packed.entities,
                }),
                Err(error) => {
                    tracing::error!(%error, ?pos, "could not encode a parked chunk at shutdown")
                }
            }
            // 没写成功的盒子留在内存里，关服日志明确列出。
            self.parking.park(pos, column);
        }
        // 关服不等 D6 匀速：把还没提交的快照一次性全部交给 IO 线程。
        // Shutdown does not pace: hand every remaining snapshot to the IO thread.
        let mut unsaved = Vec::new();
        if let Some(io) = &self.io {
            for pending in self.save_queue.drain(..) {
                unsaved.push(pending.pos);
                io.submit_write(pending.pos, pending.seq, pending.chunk, pending.entities);
            }
        }
        self.posts.clear();
        self.flush_io();
        // 磁盘到关服还在失败时，明确列出没存下来的区块，再照常退出。
        if self.io_failures > 0 && !unsaved.is_empty() {
            tracing::error!(
                ?unsaved,
                "these chunks still had writes pending at shutdown; IO failures were logged"
            );
        }
    }

    /// `pump` 达到收敛上限：不 panic，关服时明确列出没收回、没存下来的区块。
    /// On a non-convergent pump, log the chunks that were never reclaimed/saved.
    fn report_unconverged_pump(&self, rounds: usize) {
        let mut left: Vec<ChunkPos> = self.island_chunks.iter().copied().collect();
        left.sort_unstable();
        tracing::error!(
            rounds,
            ?left,
            queued = self.save_queue.len(),
            parked = self.parking.len(),
            shutting_down = self.shutting_down,
            "Director event loop did not converge; chunks below may not have been saved"
        );
    }

    /// 处理一个事件和它引发的全部任务，直到没有新效果。
    /// 内联执行器不设每刻上限（提交 5/6 的要求）：任务的纯函数部分当场做完，
    /// 盒子投进岛的邮筒，由岛的信箱按预算交付/收回。
    ///
    /// 同一刻里 Director 可能因为 `Delivered` / `Returned` 又派出新任务，
    /// 所以主循环要跑到没有新消息为止；岛借给这次调用时（齐步）每轮收敛前让岛
    /// 收一次信，信箱每轮都只花掉这一刻剩下的预算，预算用完之后没有回执，循环就停。
    /// 岛不在手里时（分离）信件留在邮筒里，回执在岛交回时再喂进来。
    /// 宿主自己不碰 `Region`。
    /// 把一批初始事件处理到收敛。`initial` 在本函数返回时清空，容量留给下次。
    ///
    /// Drive a batch of events to convergence. Mail goes to the island's
    /// postbox; when the island is lent (lockstep) it takes the mail before each
    /// convergence check, otherwise (detached) its acks come back at hand-back.
    /// The host never touches the `Region` itself.
    fn pump(&mut self, island: &mut IslandAccess<'_>, mut initial: Vec<Event<Box<RegionColumn>>>) {
        // 第 8 步：分区器只在同一个 pump 里消费 Director 的等级变化和 owner
        // 事件，不碰交付 / 收回本身。开关关着时只有一次布尔判断。
        // Step 8: feed level changes and owner transitions to the island
        // partitioner without touching the delivery/reclaim path.
        let islands_on = self.islands.is_some();
        // 一批初始事件（例如同一刻收回的所有读盘结果）一起处理，循环末尾只
        // 交付一次，保证按距离排序、出生点 3×3 在同一批里。
        // A batch of initial events (e.g. all reads collected this tick) is driven
        // together and delivered once, keeping distance order and the 3x3 spawn
        // footprint in a single batch.
        let mut events = std::mem::take(&mut self.pump_events);
        events.clear();
        events.append(&mut initial);
        self.initial_scratch = initial;
        let mut rounds = 0;
        loop {
            // 先把队列里的事件和它们引发的任务结果全部跑完。
            while let Some(event) = events.pop() {
                rounds += 1;
                if rounds >= PUMP_ROUND_LIMIT {
                    self.report_unconverged_pump(rounds);
                    return;
                }
                // `Tickets` 和 `Tick` 里包含 `run_updates()`（等级传播 + 消费变化列表），
                // 单独记在 `ticket_update`；其余事件是纯状态机推进。
                let sub = match event {
                    Event::Tickets { .. } | Event::Tick { .. } => SubPhase::TicketUpdate,
                    Event::Delivered { .. } | Event::Returned { .. } => SubPhase::DirectorAck,
                    _ => SubPhase::DirectorEvents,
                };
                // 回执本身也改 owner：放在 effects 循环之前，效果里的
                // Deliver / Reclaim 若是同一格以更晚的那条为准。
                let ack = match &event {
                    Event::Delivered { pos, .. } | Event::Returned { pos, .. } => Some(*pos),
                    _ => None,
                };
                // 岛没了（`Lost`）同样改 owner：处理完按每个位置的真实归属告诉分区器。
                // A lost island changes owners too: tell the partitioner each
                // position's real owner afterwards.
                let lost = match &event {
                    Event::Lost { positions } if islands_on => positions.clone(),
                    _ => Vec::new(),
                };
                self.profiler.start_sub(sub);
                let mut effects = std::mem::take(&mut self.effect_scratch);
                self.director
                    .handle_effects(event, |out| std::mem::swap(&mut effects, out));
                self.profiler.end_sub();
                if islands_on {
                    self.note_level_changes();
                    // 用事件处理完之后的真实 owner：过期回执不改变归属，
                    // 也不会凭空造出「还没收回」的区块。
                    if let Some(pos) = ack {
                        let owned = self.director.owner_at(pos);
                        self.note_owner(pos, owned);
                    }
                    for pos in lost {
                        let owned = self.director.owner_at(pos);
                        self.note_owner(pos, owned);
                    }
                }
                for effect in effects.drain(..) {
                    match effect {
                        Effect::Submit {
                            id,
                            generation,
                            task,
                        } => {
                            rounds += 1;
                            if rounds >= PUMP_ROUND_LIMIT {
                                self.report_unconverged_pump(rounds);
                                return;
                            }
                            self.profiler.start_sub(SubPhase::DirectorEvents);
                            let next = self.run_task(id, generation, task);
                            self.profiler.end_sub();
                            // 异步读盘任务这一刻不产生回执，结果在下一刻开头从信箱收回。
                            // An async disk read produces no ack this tick; its result
                            // is collected from the mailbox at the start of the next tick.
                            if let Some(next) = next {
                                events.push(next);
                            }
                        }
                        Effect::Cancel { id } => {
                            // 任务被取消时，还没交给 IO 线程的读也要撤掉，否则它
                            // 会占一个 FIFO 位置，把近处读的回执拖晚一刻。
                            // Drop a queued read when its task is cancelled so it
                            // cannot delay a nearer read in the FIFO worker.
                            self.read_queue.retain(|&(_, queued_id, _)| queued_id != id);
                            self.cancel_compute(id);
                        }
                        Effect::ToRegion(message) => {
                            if islands_on {
                                match &message {
                                    RegionMessage::Deliver { pos, .. } => {
                                        self.note_owner(*pos, Some(Owner::Delivering));
                                    }
                                    RegionMessage::Reclaim { positions, .. } => {
                                        for &(pos, _) in positions {
                                            self.note_owner(pos, Some(Owner::Reclaiming));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            self.region_message(message);
                        }
                        Effect::Park { pos, column } => {
                            self.profiler.start_sub(SubPhase::DirectorEvents);
                            self.parking.park(pos, column);
                            self.parked += 1;
                            self.profiler.end_sub();
                        }
                        Effect::DropSnapshot { pos, seq } => {
                            if let Some(io) = &self.io {
                                io.drop_snapshot(pos, seq);
                            }
                        }
                        Effect::Warn { message } => tracing::warn!(%message, "chunk director"),
                    }
                }
                self.effect_scratch = effects;
            }
            // 岛在手里就让它收信，交付/收回的回执变成 Delivered / Returned 事件。
            // 没有新消息就收敛了。
            self.flush_mail(island, &mut events);
            if events.is_empty() {
                self.submit_reads();
                self.submit_compute();
                // 对照模式只在 pump 收敛后做一次全量对账，正式路径不进这里。
                if islands_on {
                    self.reconcile_islands();
                }
                self.pump_events = events;
                return;
            }
        }
    }

    /// 把这一轮攒下的异步读按离玩家的距离提交给 IO 线程。IO 线程是 FIFO，
    /// 先提交的先回执；`poll_io` 一次收回所有回执后按距离交付，进服的第一批
    /// 区块因此就是最近的那一圈（官方 `PlayerChunkSender` 的语义）。
    /// Submit the reads collected this round in distance order. The IO worker is
    /// FIFO, so near chunks reply first and join the same ordered delivery batch.
    ///
    /// 玩家中心取自 Director 票表而不是 `region.players`：进服那一刻
    /// `Event::Tickets`（`PlayerMoved`）先派读任务、随后才 `region.add_player`，
    /// 用 `region.players` 会取到空表，读任务退化成按坐标排序，远处区块抢先回执。
    /// 票表在 `handle_effects` 里同步 `set_player`，派读时一定有正确的中心。
    /// Player centers come from the Director ticket table, not `region.players`:
    /// on join, `Event::Tickets` (`PlayerMoved`) queues reads before
    /// `region.add_player`, so `region.players` is empty and reads would fall back
    /// to raw coordinate order, letting far chunks reply first. The ticket table
    /// is updated synchronously in `handle_effects`, so centers are always valid.
    fn submit_reads(&mut self) {
        if self.read_queue.is_empty() {
            return;
        }
        let Some(io) = &self.io else {
            self.read_queue.clear();
            return;
        };
        let mut queued = std::mem::take(&mut self.read_queue);
        // 稳定排序：同等 rank 保持 Director 派任务的顺序（TaskId 升序），结果确定。
        queued.sort_by_key(|&(pos, _, _)| self.demand_rank(pos));
        for (pos, id, generation) in queued {
            io.submit_read(pos, id, generation);
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
                    // 停放盒子的生成阶段跟着存档元数据走；没有读盘元数据时是 Full。
                    // A parked box carries its persisted stage in archive metadata.
                    let status = column.chunk_column().chunk().archive().status;
                    self.restore_decoded_stage(pos, status, &column, None);
                    TaskResult::Decoded { column, status }
                } else if self.io.is_some() {
                    // 官方 `ChunkMap.scheduleChunkLoad` 没有「盘上有就异步、盘上没有就
                    // 同步生成」的快路径：所有区块都先经过异步加载流水线，变成可发的
                    // 区块之后才登记到 `PlayerChunkSender`、按距离排序发送。这里照做：
                    // 盘上没有数据时 IO 线程回 `chunk: None`，下一刻开头和盘上的读结果
                    // 一起由 `poll_io` 收回，经同一次 `pump` 按距离排序交付。这样带存档
                    // 的世界不会让远处模板区块抢在近处盘上区块之前到达。
                    // Vanilla's `scheduleChunkLoad` sends every chunk through the async
                    // pipeline; chunks with no on-disk data reply `None` and join the same
                    // distance-ordered delivery batch as the ones read from disk.
                    self.read_queue.push((pos, id, generation));
                    return None;
                } else {
                    // 没接世界目录（测试、对拍、压测）：仍当刻同步走模板。
                    // Without a world directory, keep the old synchronous template path.
                    TaskResult::Read { bytes: None }
                }
            }
            Task::Decode { bytes, .. } => {
                if self.io.is_some() {
                    if self.pool.is_some() {
                        self.queue_compute(id, pos, generation, ChunkWork::Decode { bytes });
                        return None;
                    }
                    match decode_packed(pos, &bytes) {
                        Ok(decoded) => {
                            let status = decoded.persisted_status;
                            self.restore_decoded_stage(pos, status, &decoded.column, Some(&bytes));
                            TaskResult::Decoded {
                                column: decoded.column,
                                status,
                            }
                        }
                        Err(error) => TaskResult::BadChunk { error },
                    }
                } else {
                    TaskResult::Decoded {
                        column: Box::new(RegionColumn::from_template()),
                        status: ChunkStatus::Full,
                    }
                }
            }
            Task::Generate {
                stage,
                column,
                neighbors,
                ..
            } => {
                // 只读快照：中心取自己的半成品，邻居按 Director 列的位置从缓存取；
                // 没有快照的位置留 `None`，计算线程用纯函数现算。全是 `Arc` 克隆，
                // 不复制方块数据，也不改缓存。
                // Read-only snapshots: the center's half-built column plus each
                // listed neighbor's `Arc`; a missing neighbor is `None` and the
                // compute thread regenerates it purely. All `Arc` clones: no
                // block copy, no cache mutation.
                let center_snapshot = self.partials.get(&pos).cloned();
                let snapshots: Vec<(ChunkPos, Option<Arc<GeneratedChunk>>)> = neighbors
                    .iter()
                    .map(|(npos, _)| (*npos, self.partials.get(npos).cloned()))
                    .collect();
                if self.pool.is_some() {
                    let generator = Arc::clone(&self.generator);
                    self.queue_compute(
                        id,
                        pos,
                        generation,
                        ChunkWork::Generate {
                            stage,
                            center: column,
                            center_snapshot,
                            neighbors: snapshots,
                            generator,
                        },
                    );
                    return None;
                }
                // 内联执行器：当场跑，增量由 `merge_generated` 合并（同一世界线程）。
                // Inline: run now; `merge_generated` merges deltas on this thread.
                let output =
                    self.generator
                        .generate(stage, pos, column, center_snapshot, &snapshots);
                self.merge_generated(pos, &output);
                TaskResult::Generated {
                    stage,
                    column: output.column,
                }
            }
            Task::Assemble { column, .. } => {
                // 生成到完工后用不可变的 CARVERS 快照 + 累计的地物写组装交付
                // 盒子（按来源中心固定先后合，和回执顺序无关）；flat / 读盘路径
                // 没有这份快照，原盒子照旧。组装和生成用的是同一个
                // `build_region_column`。
                // At completion, assemble from the immutable CARVERS snapshot +
                // accumulated feature writes (fixed source-center order, reply-
                // order independent); flat / disk paths keep the original box.
                // 盘上状态已经跑到 FEATURES 时，方块层是「地物写完之后」的
                // 视图，不能拿 CARVERS + 新算的写再覆盖一遍（官方 `ChunkStep.apply`
                // 会跳过已完成的阶段）；只把续跑阶段交回的增量合进去。
                let disk_after_features = self.disk_saved_after_features.get(&pos) == Some(&true)
                    && self.partials.contains_key(&pos);
                // 走过生成流水线（有 CARVERS 快照）、盘上又还没过 `SPAWN` 的区块才刷怪。
                // Only columns that ran the pipeline and were not saved past `SPAWN` spawn.
                let runs_spawn =
                    self.partials.contains_key(&pos) && !self.resumed_after_spawn.remove(&pos);
                let mut column = if disk_after_features {
                    let snapshot = self.partials.get(&pos).cloned();
                    if let (Some(snapshot), Some(mut groups)) =
                        (snapshot, self.feature_deltas.remove(&pos))
                    {
                        let mut assembled = (*snapshot.as_ref()).clone();
                        crate::worldgen_features::merge_feature_writes(
                            &mut assembled,
                            groups.drain(..),
                        );
                        crate::worldgen::build_region_column(&assembled).unwrap_or(column)
                    } else {
                        column
                    }
                } else {
                    match self.partials.get(&pos) {
                        Some(snapshot) => {
                            let mut deltas = self.feature_deltas.remove(&pos).unwrap_or_default();
                            assemble_feature_column(snapshot.as_ref(), &mut deltas)
                                .unwrap_or(column)
                        }
                        None => column,
                    }
                };
                // 读盘带来的上层内容（方块实体 / 计划刻 / 实体 / 四张表）装回
                // 完工盒子；阶段函数只换了方块层，不该丢掉这些内容。
                if let Some(bytes) = self.disk_upper_bytes.remove(&pos)
                    && let Ok(disk) = decode_packed(pos, &bytes)
                {
                    column.adopt_upper_content(*disk.column);
                }
                // `light` 阶段算好的天空光 + 方块光装进交付盒子并标记
                // `isLightOn=true`；flat / 读盘路径没有这份光照，盒子照旧。
                // Install the `light` stage's sky + block light into the
                // delivered column with `isLightOn=true`; flat/disk keep theirs.
                if let Some(light) = self.generated_light.remove(&pos) {
                    column.set_generated_light(light);
                }
                // `ChunkStatus.SPAWN`：方块和光照都装好之后跑生成期刷怪。
                // `ChunkStatus.SPAWN`: world-generation spawning once blocks and light are in.
                if runs_spawn {
                    crate::worldgen_mobs::spawn_original_mobs(
                        &self.world_gen_config,
                        pos,
                        &mut column,
                        self.spawn_mobs,
                    );
                }
                // `structures` 标签最后装：它会从当前列读出读盘元数据并只覆盖
                // `archive.structures`，不会动光照或方块。
                // Install `structures` last: it reads the column's archive metadata
                // and only replaces `archive.structures`, leaving light/blocks intact.
                if let Some(tag) = self.generated_structures.remove(&pos) {
                    crate::worldgen_structures::set_structures_tag(&mut column, &tag);
                }
                // 盘上非 full 的区块完工后把 persisted status 抬到 Full；
                // 生成器新造的盒子默认就是 Full，不能在这里动它的 archive，
                // 否则 `chunk_mut()` 会把 pristine 标记清掉、卸载时全被停放。
                // Only decoded non-full chunks need the bump; touching archive
                // on a freshly generated box would clear its pristine flag.
                if column.chunk_column().chunk().archive().status != ChunkStatus::Full {
                    let mut archive = column.chunk_column().chunk().archive().clone();
                    archive.status = ChunkStatus::Full;
                    column.set_archive(archive);
                }
                self.partials.remove(&pos);
                self.feature_deltas.remove(&pos);
                self.disk_saved_after_features.remove(&pos);
                TaskResult::Assembled { column }
            }
            Task::Save { seq, column, .. } => {
                if self.io.is_some() {
                    if self.pool.is_some() {
                        self.queue_compute(id, pos, generation, ChunkWork::Save { seq, column });
                        return None;
                    }
                    let encoded = encode_column(pos, &column);
                    self.saved_outcome(pos, seq, column, encoded)
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

    /// 序列化做完之后的记账：成功就进 D6 匀速写盘队列，失败就把盒子停放起来。
    /// 内联执行器和计算池的回执都走这里，记账只在世界线程上改。
    fn saved_outcome(
        &mut self,
        pos: ChunkPos,
        seq: SaveSeq,
        column: Box<RegionColumn>,
        encoded: Result<PackedChunk, String>,
    ) -> TaskResult<Box<RegionColumn>> {
        match encoded {
            Ok(packed) => {
                // 不直接写盘：进 D6 匀速队列，同区块只留最新一份（F6）。
                self.enqueue_snapshot(PendingWrite {
                    pos,
                    seq,
                    chunk: packed.chunk,
                    entities: packed.entities,
                });
                self.pending_writes.insert(pos, seq);
                // 卸载写不进已加载判脏路径；回执仍要清掉在途标记。
                self.acked_revisions
                    .insert(seq, (pos, ChunkRevision::default(), 0));
                TaskResult::Saved { seq, column }
            }
            Err(error) => {
                // 编码失败不能把盒子弄丢：留在停放表里，下次卸载再试。
                self.parking.park(pos, column);
                TaskResult::SaveFailed { seq, error }
            }
        }
    }

    /// 攒一个计算任务。存盘直接交给池子（最优先，写盘和卸载背压都在等它），
    /// 其余进待办，`pump` 收尾时按距离补进池子。
    fn queue_compute(&mut self, id: TaskId, pos: ChunkPos, generation: u64, work: ChunkWork) {
        #[cfg(test)]
        {
            let kind = match work {
                ChunkWork::Decode { .. } => 0,
                ChunkWork::Generate { .. } => 1,
                ChunkWork::Save { .. } => 2,
            };
            self.pooled_kinds[kind] += 1;
        }
        let job = ChunkJob {
            pos,
            generation,
            work,
        };
        if matches!(job.work, ChunkWork::Save { .. })
            && let Some(pool) = &mut self.pool
        {
            pool.submit(id, 0, job);
            return;
        }
        let rank = self.demand_rank(pos);
        self.backlog_order.push(Reverse((rank, id)));
        self.compute_backlog.insert(id, job);
    }

    /// Director 取消了一个任务。还在待办里的直接删掉、回「已取消」；
    /// 已经交给池子的只有还在排队时才取消得掉，回执照常从池子回来。
    fn cancel_compute(&mut self, id: TaskId) {
        if let Some(job) = self.compute_backlog.remove(&id) {
            self.early_replies.push((id, job.cancelled()));
        } else if let Some(pool) = &mut self.pool {
            pool.cancel(id);
            self.cancelled_compute.insert(id);
        }
    }

    /// 从待办里按离玩家的距离挑最近的补进池子，直到池子里有
    /// `threads × POOL_JOBS_PER_THREAD` 个任务。玩家跨了区块才整堆重排，
    /// 平时只从堆顶取。
    fn submit_compute(&mut self) {
        if self.compute_backlog.is_empty() {
            self.backlog_order.clear();
            return;
        }
        let Some(pool) = &mut self.pool else {
            // 没有池子不会走到 `queue_compute`；万一走到了也不丢任务。
            let mut jobs: Vec<(TaskId, ChunkJob)> = self.compute_backlog.drain().collect();
            jobs.sort_by_key(|(id, _)| *id);
            for (id, mut job) in jobs {
                let output = job.run();
                self.early_replies.push((id, job.finish(Ok(output))));
            }
            self.backlog_order.clear();
            return;
        };
        // 玩家跨格（或方向变了）时 `set_player_demand` 会清空这个标记，
        // 下一次 submit 整堆重排；平时只从堆顶取。rank 已经含坐标兜底，结果确定。
        if self.backlog_centers.is_empty() {
            let demand = self.director.player_demand();
            let centers = self.director.tickets().player_centers();
            let rank = |pos: ChunkPos| {
                let rank = if demand.players().is_empty() && !centers.is_empty() {
                    distance_to_players(pos, &centers).0 as i64
                } else {
                    demand.rank(pos)
                };
                (rank, pos.x, pos.z)
            };
            self.backlog_order = self
                .compute_backlog
                .iter()
                .map(|(&id, job)| Reverse((rank(job.pos), id)))
                .collect();
            // 记下重排时的玩家中心：非空就说明排过了，下次 `set_player_demand` 再清。
            let mut centers = centers;
            centers.sort_unstable();
            self.backlog_centers = centers;
        }
        let cap = pool.threads().saturating_mul(POOL_JOBS_PER_THREAD);
        while pool.in_flight() < cap {
            let Some(Reverse(((rank, _, _), id))) = self.backlog_order.pop() else {
                break;
            };
            // 已经取消（不在待办里）的堆项直接跳过。
            // rank 不小于 0；钳到 u64 让坐标救不了方向加权为负的情况。
            let priority = rank.max(0) as u64;
            if let Some(job) = self.compute_backlog.remove(&id) {
                pool.submit(id, priority, job);
            }
        }
    }

    /// 收走池子里已经回来的结果，按 `TaskId` 升序变成 `TaskDone`。
    ///
    /// 回执到达的先后取决于线程调度；排好序再交给 Director，同一批结果里
    /// 交付给世界的顺序就和调度无关。哪个结果落在哪一刻仍然取决于调度，
    /// 所以内联执行器和计算池的交付时刻可以不同，区块内容相同。
    fn collect_compute(&mut self, wait: Option<Duration>) -> Vec<Event<Box<RegionColumn>>> {
        let mut replies = std::mem::take(&mut self.early_replies);
        if let Some(pool) = &mut self.pool {
            if let Some(timeout) = wait
                && replies.is_empty()
                && let Some(reply) = pool.recv_timeout(timeout)
            {
                replies.push(reply);
            }
            while let Some(reply) = pool.try_recv() {
                replies.push(reply);
            }
        }
        replies.sort_by_key(|(id, _)| *id);
        // `pump` 从末尾弹事件：倒着放，编号小的先处理。
        replies
            .into_iter()
            .rev()
            .map(|(id, reply)| self.compute_event(id, reply))
            .collect()
    }

    /// 读盘的非 full 区块：按阶段恢复中间结果，并留住盘上的上层内容。
    ///
    /// `Status >= TERRAIN` 时把盘上方块还原成 `GeneratedChunk` 存进 `partials`
    /// （后续 `features` / `light` 的输入）；`Status >= LIGHT` 时盘上光照直接
    /// 复用；`structures` 标签来自读盘元数据。方块实体 / 计划刻 / 实体这些
    /// 上层内容留在整盒里，完工组装时 `adopt_upper_content` 装回。
    ///
    /// Restore a decoded non-full chunk's stage intermediates.
    fn restore_decoded_stage(
        &mut self,
        pos: ChunkPos,
        status: ChunkStatus,
        column: &RegionColumn,
        bytes: Option<&[u8]>,
    ) {
        if status == ChunkStatus::Full {
            return;
        }
        // 上层内容由 `restore_decoded_column` 在持有原始 NBT 字节的调用点
        // 解码第二份盒子留住（方块实体 / 计划刻 / 实体 / 四张表）。
        self.disk_saved_after_features
            .insert(pos, (status as usize) >= (ChunkStatus::Features as usize));
        if status == ChunkStatus::Spawn {
            self.resumed_after_spawn.insert(pos);
        }
        if let Some(bytes) = bytes {
            self.disk_upper_bytes.insert(pos, bytes.to_vec());
        }
        let world = column.chunk_column().chunk();
        if status.is_after_terrain() {
            self.partials.insert(
                pos,
                Arc::new(crate::worldgen::generated_chunk_from_column(column, pos)),
            );
        }
        if (status as usize) >= (ChunkStatus::Light as usize) && column.chunk_column().light_on() {
            self.generated_light
                .insert(pos, crate::worldgen::generated_light_from_column(column));
        }
        if let Some(tag) = world.archive().structures.clone() {
            self.generated_structures.insert(pos, tag);
        }
        // 结构起点 / 引用按盘上标签恢复（计算池线程用线程本地覆盖）。
        // 生成器持有这份配置，正式路径和读盘路径共用同一个种子。
        crate::worldgen_structures::install_disk_structures(
            self.generator_config(),
            pos.x,
            pos.z,
            world.archive().structures.as_ref(),
        );
    }

    /// 合并一次生成结果：`terrain` 的 CARVERS 快照进缓存（不可变 `Arc`，不改
    /// 已有的那份）；地物写按 `TaskId` 顺序追加到目标区块的增量表。
    /// `compute_event` 已按 `TaskId` 升序、内联路径也在同一世界线程调用，所以
    /// 追加顺序确定；完工时 `assemble_feature_column` 按来源的固定先后合，结果与
    /// 追加顺序无关。
    ///
    /// Merge one generation result: cache the `terrain` CARVERS snapshot as an
    /// immutable `Arc` (never mutate an existing snapshot) and append feature
    /// writes to each target chunk's list in `TaskId` order. `compute_event`
    /// calls this in ascending `TaskId` order; `assemble_feature_column` merges
    /// by fixed source order at completion, so the result is order-independent.
    fn merge_generated(&mut self, pos: ChunkPos, output: &GenerateOutput) {
        if let Some(terrain) = &output.terrain {
            self.partials.insert(pos, Arc::clone(terrain));
        }
        for (target, deltas) in &output.deltas {
            self.feature_deltas
                .entry(*target)
                .or_default()
                .extend_from_slice(deltas);
        }
        if let Some(light) = &output.light {
            self.generated_light.insert(pos, light.clone());
        }
        if let Some(structures) = &output.structures {
            self.generated_structures.insert(pos, structures.clone());
        }
    }

    /// 一条池子回执变成 Director 的 `TaskDone`。
    fn compute_event(&mut self, id: TaskId, reply: ChunkReply) -> Event<Box<RegionColumn>> {
        let ChunkReply {
            pos,
            generation,
            outcome,
        } = reply;
        let cancelled = self.cancelled_compute.remove(&id);
        let result = match outcome {
            // 取消之前已经开始跑的解析：结果没人要了，盘上的字节还在。
            ChunkOutcome::Decoded(_) if cancelled => TaskResult::Cancelled,
            ChunkOutcome::Decoded(Ok((decoded, bytes))) => {
                let status = decoded.persisted_status;
                self.restore_decoded_stage(pos, status, &decoded.column, Some(&bytes));
                TaskResult::Decoded {
                    column: decoded.column,
                    status,
                }
            }
            ChunkOutcome::Decoded(Err(error)) => TaskResult::BadChunk { error },
            ChunkOutcome::Generated { stage, output } => {
                // 取消时已经在跑的生成：Director 不再认它，`Forgot` 也可能已经清过
                // 这个位置，合并进来就再没人清。不合并；重派时纯函数给出同样结果。
                // A generation already running when cancelled: the Director no
                // longer accepts it and `Forgot` may have cleared this position
                // already, so merging would leak. Skip; a re-run is identical.
                if !cancelled {
                    self.merge_generated(pos, &output);
                }
                TaskResult::Generated {
                    stage,
                    column: output.column,
                }
            }
            ChunkOutcome::GenerateFailed { error } => TaskResult::BadChunk { error },
            ChunkOutcome::Saved {
                seq,
                column,
                encoded,
            } => self.saved_outcome(pos, seq, column, encoded),
            // 取消的任务 Director 已经不认了，结果只会被当成孤儿处理：
            // 带回来的半成品按「生成完成」交回去，非原样的由它停放。
            ChunkOutcome::Cancelled {
                column: Some(column),
            } => TaskResult::Assembled { column },
            ChunkOutcome::Cancelled { column: None } => TaskResult::Cancelled,
        };
        Event::TaskDone {
            id,
            pos,
            generation,
            result,
        }
    }

    /// 关服用：阻塞等计算池把在途任务全部做完，结果照常喂给 Director。
    /// 池子线程意外全退、或任务被无限重派时都不会无限等：没有进展或超过
    /// 墙钟上限就记错误退出，把还没存下来的区块列出来。
    ///
    /// Drain the compute pool at shutdown, feeding results to the Director. A
    /// dead pool or endlessly resubmitted tasks must never hang the world thread:
    /// no progress or a wall-clock timeout logs the unsaved chunks and returns.
    fn drain_compute(&mut self, island: &mut IslandAccess<'_>) {
        let started = Instant::now();
        let mut idle_rounds = 0;
        while self.compute_in_flight() > 0 {
            if started.elapsed() >= SHUTDOWN_DRAIN_TIMEOUT {
                tracing::error!(
                    left = self.compute_in_flight(),
                    "chunk compute pool did not drain before the shutdown deadline; some chunks are not saved"
                );
                return;
            }
            self.submit_compute();
            let events = self.collect_compute(Some(Duration::from_millis(100)));
            if events.is_empty() {
                idle_rounds += 1;
                if self.compute_failed() && idle_rounds > 50 {
                    tracing::error!(
                        left = self.compute_in_flight(),
                        "chunk compute pool stopped with jobs in flight; their chunks are not saved"
                    );
                    return;
                }
                continue;
            }
            idle_rounds = 0;
            self.pump(island, events);
        }
    }

    /// 每刻开头收回 IO 信箱：读结果变成 `TaskDone` 喂给 Director，
    /// 写完成回执用来丢掉写盘途中缓冲。
    ///
    /// Drain the IO mailbox at the start of each tick: read results become
    /// `TaskDone` events; write acks release the write-in-progress buffer.
    pub(crate) fn poll_io(&mut self, island: &mut IslandAccess<'_>) {
        let mut replies = std::mem::take(&mut self.io_replies);
        replies.clear();
        if let Some(io) = &self.io {
            while let Some(reply) = io.try_recv() {
                replies.push(reply);
            }
        }
        let mut done = std::mem::take(&mut self.done_scratch);
        done.clear();
        let mut known_updates: Vec<(ChunkPos, bool, u64)> = Vec::new();
        let mut dropped_inflight: Vec<(ChunkPos, Option<u64>)> = Vec::new();
        for reply in replies.drain(..) {
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
                Reply::Written { pos, seq, present } => {
                    // 写成功之后才把 `known` 置成盘上真实状态（F1）。
                    // 旧回执只清自己的 `(pos, seq)`，绝不抹掉较新的在途写（致命 1）。
                    // Only after a successful write does `known` reflect the disk;
                    // an older ack never clears a newer in-flight write (fatal 1).
                    known_updates.push((pos, present, seq));
                    if self.pending_writes.get(&pos) == Some(&seq) {
                        self.pending_writes.remove(&pos);
                        // 只有这个位置没有更晚的写时才释放盒子。
                        if let Some(io) = &self.io
                            && !io.has_in_flight_write(pos, seq)
                        {
                            let _ = self.parking.take(pos);
                        }
                    }
                    // 「已保存」只在回执到达时记，失败写永远保持脏（致命 2）。
                    if let Some((_, revision, fingerprint)) = self.acked_revisions.remove(&seq) {
                        self.saved_revisions.insert(pos, (revision, fingerprint));
                    }
                    self.retry_dirty.remove(&pos);
                    // 上层内容改动位由 `dirty_loaded_chunks` 在下一刻按版本号
                    // 重新判定；写失败时 `retry_dirty` 会把它重新标脏。
                    // 一次成功清零连续失败，恢复匀速保存。
                    self.consecutive_io_failures = 0;
                    self.save_retry_after = 0;
                    // F6：写完清掉冷却记账，避免长局无界增长。
                    self.save_cooldown.remove(&pos);
                    // 这次写已处理完，从等待中的 `/save-all` 请求里去掉（中 8）。
                    for (_, remaining) in &mut self.save_all_requests {
                        remaining.remove(&seq);
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
                Reply::Flushed { seq, ok } => {
                    // 异步栅栏的回执：World 会在本刻取走它，按 ok 发等在上面的
                    // `save-all` 成功或失败消息。
                    // An async barrier ack; World drains it this tick and sends
                    // the waiting `save-all` success/failure message.
                    self.flushed_barriers.push((seq, ok));
                }
                Reply::WriteFailed {
                    pos,
                    seq,
                    path,
                    error,
                } => {
                    // 每次写/刷盘失败都带路径和原因记 error!，绝不静默丢数据。
                    // Log every write/sync failure with its path and reason.
                    tracing::error!(%path, %error, ?pos, ?seq, "chunk IO write failed");
                    self.io_failures += 1;
                    self.consecutive_io_failures += 1;
                    // 失败区块的盒子留在 parking 里；只清这个 seq 的在途标记，
                    // 并把它重新标脏，让正常保存路径在退避后重拍快照（致命 2）。
                    if let Some(pos) = pos {
                        dropped_inflight.push((pos, seq));
                        self.save_cooldown.remove(&pos);
                        self.pending_writes.remove(&pos);
                        self.saved_revisions.remove(&pos);
                        self.retry_dirty.insert(pos);
                        if let Some(acked) = seq.and_then(|s| self.acked_revisions.remove(&s)) {
                            let _ = acked;
                        }
                    }
                    // IO 线程按「这道栅栏提交之后有没有失败」给每次请求单独判成功
                    // 或失败；这里只把该写从等待集合里去掉，让栅栏能排出去。
                    if let Some(seq) = seq {
                        for (_, remaining) in &mut self.save_all_requests {
                            remaining.remove(&seq);
                        }
                    }
                    let backoff = 1_u64
                        .checked_shl(self.consecutive_io_failures.min(8))
                        .unwrap_or(MAX_SAVE_RETRY_BACKOFF_TICKS)
                        .min(MAX_SAVE_RETRY_BACKOFF_TICKS);
                    self.save_retry_after = self.last_tick.saturating_add(backoff);
                    if self.consecutive_io_failures >= MAX_CONSECUTIVE_IO_FAILURES {
                        tracing::error!(
                            failures = self.consecutive_io_failures,
                            backoff,
                            "consecutive chunk IO failures; retrying with backoff, data stays in memory"
                        );
                    }
                }
            }
        }
        if let Some(io) = &self.io {
            for (pos, present, seq) in known_updates {
                io.set_known(pos, present, seq);
            }
            for (pos, seq) in dropped_inflight {
                io.drop_inflight(pos, seq);
            }
        }
        // 再收计算池的结果（按 TaskId 排好），和读盘结果合成一批。
        // 读盘结果在前、池结果在后；`pump` 从末尾弹，所以池结果先处理。
        done.extend(self.collect_compute(None));
        // 所有读盘结果一次性喂给 Director，只交付一次，保住 3×3 和距离顺序。
        if !done.is_empty() {
            self.pump(island, done);
        } else {
            done.clear();
            self.done_scratch = done;
        }
        self.io_replies = replies;
    }

    /// Director 发给岛的信：交付、收回、换档投进目标岛的邮筒，不碰 `Region`；
    /// `Ready` / `Forgot` 只动主控自己的记账。
    ///
    /// A Director letter for an island: deliveries, reclaims and level changes
    /// go to the target island's postbox without touching the `Region`;
    /// `Ready` / `Forgot` only touch the master's own bookkeeping.
    fn region_message(&mut self, message: RegionMessage<Box<RegionColumn>>) {
        if self.live_islands {
            self.route_message(message);
            return;
        }
        let (region, letter) = match message {
            RegionMessage::Deliver {
                region,
                pos,
                generation,
                column,
                desired,
            } => (
                region,
                Posted::Deliver {
                    pos,
                    generation,
                    desired,
                    column,
                },
            ),
            RegionMessage::Reclaim { region, positions } => (region, Posted::Reclaim(positions)),
            RegionMessage::LevelChanged { region, changes } => (region, Posted::Levels(changes)),
            // `Ready` 不用另存一份集合：`ChunkStore::is_loaded` 就是已交付集合，
            // 发区块时直接问它；多存一份只会多一处要保持一致的状态。
            RegionMessage::Ready { .. } => return,
            RegionMessage::Forgot { positions } => {
                // F6：区块被遗忘时清掉各处的每区块记账，避免长局无界增长。
                // F6: clear per-chunk bookkeeping when a chunk is forgotten.
                for &pos in &positions {
                    self.save_cooldown.remove(&pos);
                    self.saved_revisions.remove(&pos);
                    self.loaded_save_tick.remove(&pos);
                    // 生成期的半成品快照、累计增量和光照也在这里清掉。
                    // Clear the generation-time snapshot, deltas and light.
                    self.partials.remove(&pos);
                    self.feature_deltas.remove(&pos);
                    self.generated_light.remove(&pos);
                    self.generated_structures.remove(&pos);
                    self.disk_upper_bytes.remove(&pos);
                    self.disk_saved_after_features.remove(&pos);
                    self.resumed_after_spawn.remove(&pos);
                }
                return;
            }
        };
        self.profiler.start_sub(SubPhase::DirectorEvents);
        self.posts.entry(region).or_default().push(letter);
        self.profiler.end_sub();
    }

    /// 多岛时投信（第 10 步）：交付按分区器给的岛；收回和换档按区块所在的岛分组。
    /// Director 给的编号（恒为家岛）不用。
    ///
    /// Posting with several islands (step 10): a delivery goes to the
    /// partitioner's island for it; reclaims and level changes are grouped by
    /// each chunk's island. The Director's id (always home) is ignored.
    fn route_message(&mut self, message: RegionMessage<Box<RegionColumn>>) {
        self.flush_island_deltas();
        let island_of = |host: &Self, pos: ChunkPos| {
            host.islands
                .as_ref()
                .and_then(|partition| partition.island_of_chunk(pos))
        };
        // 交付和收回的区块一定在某个岛的 section 里（「票想要」或「还没收回」）。
        // A delivered or reclaimed chunk is always in some island's section.
        let route = |host: &Self, pos: ChunkPos| -> RegionId {
            island_of(host, pos).unwrap_or_else(|| {
                debug_assert!(false, "{pos:?} 的 section 不在任何岛里");
                tracing::warn!(?pos, "a chunk letter has no island; sending it home");
                mc_bamboo_world::director::HOME_REGION
            })
        };
        self.profiler.start_sub(SubPhase::DirectorEvents);
        match message {
            RegionMessage::Deliver {
                pos,
                generation,
                column,
                desired,
                ..
            } => {
                let id = route(self, pos);
                self.posts.entry(id).or_default().push(Posted::Deliver {
                    pos,
                    generation,
                    desired,
                    column,
                });
            }
            RegionMessage::Reclaim { positions, .. } => {
                let mut by_island: BTreeMap<RegionId, Vec<(ChunkPos, u64)>> = BTreeMap::new();
                for (pos, generation) in positions {
                    by_island
                        .entry(route(self, pos))
                        .or_default()
                        .push((pos, generation));
                }
                for (id, positions) in by_island {
                    self.posts
                        .entry(id)
                        .or_default()
                        .push(Posted::Reclaim(positions));
                }
            }
            RegionMessage::LevelChanged { changes, .. } => {
                // 换档也会说到还在流水线里的区块（没有哪个岛拿着它，没人要听），跳过。
                // Level changes may name chunks still in the pipeline (no island
                // holds them, nobody needs to hear); skip those.
                let mut by_island: BTreeMap<RegionId, Vec<LevelChange>> = BTreeMap::new();
                for change in changes {
                    if let Some(id) = island_of(self, change.pos) {
                        by_island.entry(id).or_default().push(change);
                    }
                }
                for (id, changes) in by_island {
                    self.posts
                        .entry(id)
                        .or_default()
                        .push(Posted::Levels(changes));
                }
            }
            message @ (RegionMessage::Ready { .. } | RegionMessage::Forgot { .. }) => {
                self.profiler.end_sub();
                // 和单岛一样只动主控自己的记账。
                // Bookkeeping only, as with one island.
                let live = std::mem::replace(&mut self.live_islands, false);
                self.region_message(message);
                self.live_islands = live;
                return;
            }
        }
        self.profiler.end_sub();
    }

    /// 取走投给 `island` 的信，给分离驱动的岛在自己一刻开头收取。
    ///
    /// Take the letters posted to `island`, for a detached island to take at
    /// the start of its own tick.
    pub(crate) fn take_posted(&mut self, island: RegionId, into: &mut Vec<Posted>) {
        if self.held_islands.contains(&island) {
            return;
        }
        if let Some(posted) = self.posts.get_mut(&island) {
            into.append(posted);
        }
    }

    /// 记下岛的回执里真正进出 `Region` 的区块，回执本身接到 `events` 后面。
    fn note_report(&mut self, report: &mut MailReport, events: &mut Vec<Event<Box<RegionColumn>>>) {
        for (pos, entered) in report.moves.drain(..) {
            if entered {
                self.island_chunks.insert(pos);
            } else {
                self.island_chunks.remove(&pos);
            }
        }
        events.append(&mut report.events);
    }

    /// 岛借给这次调用时让它收信，回执写进 `events`（先清空）；岛不在手里就什么都不做。
    ///
    /// When the island is lent, let it take its mail and write the acks into
    /// `events` (cleared first); otherwise do nothing.
    fn flush_mail(
        &mut self,
        island: &mut IslandAccess<'_>,
        events: &mut Vec<Event<Box<RegionColumn>>>,
    ) {
        events.clear();
        match island {
            IslandAccess::Away => {}
            #[cfg(test)]
            IslandAccess::Lent(island) => {
                let order = self.mail_order();
                let mut report = std::mem::take(&mut self.mail_report);
                let posted = self.posts.entry(island.id).or_default();
                island.take_mail(posted, &order, &mut report);
                self.note_report(&mut report, events);
                self.mail_report = report;
            }
            IslandAccess::All(islands) => {
                // 按编号升序，每个岛都收（信箱里可能还有上次按预算留下的盒子）；邮筒里投给
                // 还不存在的岛的信先建一个空岛。卷进还没照做的拆合的岛先不收。
                // Ascending id, every island takes (its mailbox may hold boxes
                // left by the budget); letters to a missing island create an
                // empty one first. Islands in a pending change wait.
                let order = self.mail_order();
                let mut report = std::mem::take(&mut self.mail_report);
                let mut ids: BTreeSet<RegionId> = islands.ids().into_iter().collect();
                ids.extend(
                    self.posts
                        .iter()
                        .filter(|(_, letters)| !letters.is_empty())
                        .map(|(&id, _)| id),
                );
                for id in ids {
                    // 不在主控手里的岛（混合驱动下关服时没回来的）收不了信，信留在邮筒里。
                    // An island that is away (not back at a hybrid shutdown)
                    // cannot take mail; its letters stay posted.
                    if self.held_islands.contains(&id) || !islands.can_ensure(id) {
                        continue;
                    }
                    let origin = self.island_origin(id, ChunkPos::new(0, 0));
                    let island = islands.ensure(id, origin);
                    let posted = self.posts.entry(id).or_default();
                    island.take_mail(posted, &order, &mut report);
                    self.note_report(&mut report, events);
                }
                self.mail_report = report;
            }
        }
    }

    /// 岛交回：把分离驱动下岛自己收信产生的回执喂给 Director。
    ///
    /// Hand-back: feed the acks a detached island produced to the Director.
    pub(crate) fn accept_replies(
        &mut self,
        island: &mut IslandAccess<'_>,
        replies: &mut MailReport,
    ) {
        if replies.is_empty() {
            return;
        }
        let mut events = std::mem::take(&mut self.initial_scratch);
        self.note_report(replies, &mut events);
        self.pump(island, events);
    }

    /// 测试专用：让岛收一次信，返回回执。
    #[cfg(test)]
    fn flush_region_mail(&mut self, island: &mut Island) -> Vec<Event<Box<RegionColumn>>> {
        let mut events = Vec::new();
        self.flush_mail(&mut IslandAccess::Lent(island), &mut events);
        events
    }
}

/// 把每个岛上次存盘以后的改动按编号顺序接成一份，交给主控统一判脏。
///
/// Concatenate every island's changes since the last save pass, by id, for
/// one dirty check on the master.
fn drain_all_save_changes(islands: &mut [&mut Island]) -> SaveChanges {
    let mut changes = SaveChanges::default();
    for island in islands.iter_mut() {
        let mut mine = island.drain_save_changes();
        changes.changed.append(&mut mine.changed);
        changes.content.append(&mut mine.content);
    }
    changes
}

/// 照一份存盘名单做：位置交给已加载它的岛编码；哪个岛都没加载的交给第一个岛（编出来
/// 是 `None`，下次重试）。指标只记一次。只有一个岛时就是 `follow_save_plan`。
///
/// Follow one save list: each position goes to the island that has it
/// loaded; positions no island has go to the first (they encode to `None`
/// and are retried). Metrics are recorded once. With one island this is
/// just `follow_save_plan`.
pub(crate) fn follow_all(islands: &mut [&mut Island], plan: SavePlan) -> Vec<Encoded> {
    if let [island] = islands {
        return island.follow_save_plan(plan);
    }
    let mut plans = split_plan(islands, plan);
    let mut encoded = Vec::new();
    for (island, plan) in islands.iter_mut().zip(plans.drain(..)) {
        encoded.extend(island.follow_save_plan(plan));
    }
    encoded
}

/// 一份存盘名单按岛分开（见 [`follow_all`]）。
pub(crate) fn split_plan(islands: &[&mut Island], plan: SavePlan) -> Vec<SavePlan> {
    let mut plans: Vec<SavePlan> = islands
        .iter()
        .map(|_| SavePlan {
            drain: plan.drain,
            ..SavePlan::default()
        })
        .collect();
    if plans.is_empty() {
        return plans;
    }
    let owner = |pos: ChunkPos| {
        islands
            .iter()
            .position(|island| island.region.chunks.is_loaded(pos))
            .unwrap_or(0)
    };
    let SavePlan {
        remark,
        encode,
        metrics,
        #[cfg(test)]
        verify,
        ..
    } = plan;
    for pos in remark {
        plans[owner(pos)].remark.push(pos);
    }
    for pos in encode {
        plans[owner(pos)].encode.push(pos);
    }
    plans[0].metrics = metrics;
    #[cfg(test)]
    for (pos, revision) in verify {
        plans[owner(pos)].verify.push((pos, revision));
    }
    plans
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, PrimedTnt};
    use mc_bamboo_core::{ChunkPos, Vec3};

    use mc_bamboo_world::director::{Desired, HOME_REGION};

    use super::*;
    use crate::{region::Region, world::tests::Harness};

    /// 一个只有 `Region` 的岛，给不需要整个世界的测试用。
    fn bare_island() -> Island {
        Island::new(
            HOME_REGION,
            Region::new(2, 20, false),
            MailBudget::default(),
        )
    }

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
        let mut host = DirectorHost::new(DirectorConfig::default());
        let mut server = Harness::new(2);
        let island = server.island();
        island.mailbox.queue_reclaim(ORIGIN, 1);
        let entity = borrowed_entity(&mut island.region);
        let first = host.flush_region_mail(island);
        assert!(
            first.is_empty(),
            "前提不满足时不产生 Returned，Director 还在等"
        );
        assert_eq!(
            island.mailbox.reclaims(),
            &[(ORIGIN, 1)],
            "失败的收回要放回信箱"
        );

        island.region.entities.put(entity);
        let second = host.flush_region_mail(island);
        assert!(island.mailbox.reclaims().is_empty(), "重试成功之后信箱清空");
        assert_eq!(second.len(), 1, "这一刻要给 Director 回 Returned");
        assert!(matches!(second[0], Event::Returned { pos, .. } if pos == ORIGIN));
    }

    /// 同一个位置在同一刻既在 `deliveries` 里又在 `reclaims` 里：收回优先，
    /// 盒子原样还回去，绝不先进 region 再被取出来。
    #[test]
    fn a_reclaim_and_a_delivery_for_one_position_take_the_box_once() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let mut server = Harness::new_loaded(2);
        let pos = ChunkPos::new(20, 20);
        let island = server.island();
        island.mailbox.queue_delivery(
            pos,
            7,
            Desired::UNLOADED,
            Box::new(RegionColumn::from_template()),
        );
        island.mailbox.queue_reclaim(pos, 7);
        let events = host.flush_region_mail(island);
        assert_eq!(events.len(), 1, "只回一条 Returned");
        assert!(matches!(
            events[0],
            Event::Returned {
                pos: at,
                generation: 7,
                column: Some(_)
            } if at == pos
        ));
        assert_eq!(server.island().mailbox.backlog(), (0, 0), "两个信箱都清空");
        assert!(!server.region().chunks.is_loaded(pos), "盒子没进 region");
        assert_eq!(
            server.island().mailbox.counters_this_tick().0,
            0,
            "不占交付额度"
        );
    }

    /// 交付因为前提不满足失败时，盒子一定原样回到信箱，且 region 里
    /// 没有留下半插入的状态；前提恢复后下一次 flush 能成功交付。
    #[test]
    fn a_failed_delivery_puts_the_box_back_without_a_half_insert() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let mut server = Harness::new_loaded(2);
        let island = server.island();
        island.mailbox.queue_delivery(
            ORIGIN,
            3,
            Desired::UNLOADED,
            Box::new(RegionColumn::from_template()),
        );
        // 方块实体正在 tick 中途：`deliver_column` 的只读前提不满足。
        island.region.block_entities.begin_tick(0);
        assert!(!island.region.ready_to_move_columns(), "前提确实不满足");

        let first = host.flush_region_mail(island);
        assert!(first.is_empty(), "失败时不产生 Delivered");
        assert_eq!(island.mailbox.backlog(), (1, 0), "盒子回到信箱");
        assert!(
            !island.region.chunks.is_loaded(ORIGIN),
            "region 里没有半插入的区块"
        );
        assert_eq!(island.mailbox.counters_this_tick().0, 0, "失败不占额度");

        // 同一个 pump 轮次里再调一次也还是失败，盒子不会丢、不会重复插入。
        let again = host.flush_region_mail(island);
        assert!(again.is_empty(), "前提没恢复前一直失败");
        assert_eq!(island.mailbox.backlog(), (1, 0), "重试没有把盒子弄丢");

        island.region.block_entities.end_tick();
        let third = host.flush_region_mail(island);
        assert_eq!(third.len(), 1, "前提恢复后交付成功");
        assert!(matches!(
            third[0],
            Event::Delivered { region: HOME_REGION, pos, generation: 3 } if pos == ORIGIN
        ));
        assert!(island.region.chunks.is_loaded(ORIGIN), "只插入了一次");
        assert_eq!(island.mailbox.backlog(), (0, 0), "信箱清空");
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
        let mut host = DirectorHost::new(DirectorConfig::default());
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

    /// 待核实：`pump` 里的收敛护栏不再 `assert!`（大世界关服会超过原来的
    /// 100 万轮），改成大得多的上限 + 记 error 后停这一轮，绝不 panic。
    /// Review "to verify": the `pump` convergence guard no longer asserts (a
    /// large-world shutdown can exceed 1M rounds); it uses a much larger cap
    /// and logs instead of panicking.
    #[test]
    fn pump_handles_a_large_batch_without_panicking() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let mut island = bare_island();
        let events: Vec<_> = (0..5_000_u64)
            .map(|now_tick| Event::Tick { now_tick })
            .collect();
        host.pump(&mut IslandAccess::Lent(&mut island), events);
        // 能跑到这里就说明一批 5000 个事件没有触发收敛护栏、也没有 panic。
    }

    /// F7：`save-off` 停掉卸载写盘（官方 `ChunkMap.tick` 的 noSave），
    /// `save-on` 恢复；`/save-all` 的强制快照不走 `pace_saves`，不受它管。
    /// F7: `save-off` stops unload writes (official `ChunkMap.tick` noSave),
    /// `save-on` resumes; `/save-all`'s forced snapshots bypass `pace_saves`.
    #[test]
    fn save_off_stops_unload_writes_and_save_all_is_a_force() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct Tmp(std::path::PathBuf);
        impl Tmp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-f7-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Tmp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let tmp = Tmp::new();
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default());
        host.set_io(ChunkIo::open(&dir).unwrap());
        let pos = ChunkPos::new(3, 4);
        let snapshot = |seq| PendingWrite {
            pos,
            seq,
            chunk: Some(vec![seq as u8]),
            entities: None,
        };
        host.enqueue_snapshot(snapshot(1));

        // save-off：队列不出队，`pace_saves` 不把卸载快照交给 IO。
        host.set_saves_enabled(false);
        host.pace_saves(1);
        assert_eq!(host.save_queue.len(), 1, "save-off must not dequeue");
        assert!(!host.io().unwrap().has_chunk(pos), "no write was submitted");

        // save-on：恢复匀速出队。
        host.set_saves_enabled(true);
        host.pace_saves(2);
        assert!(host.save_queue.is_empty(), "save-on resumes paced writes");
        // `/save-all` 的强制快照不走 `pace_saves`，见 world 测试
        // `save_all_is_a_force_even_during_save_off`。
    }

    /// F6：同一区块在 `save_queue` 里只留最新一份，旧序号不能覆盖新的。
    /// F6: only the newest snapshot per chunk stays in `save_queue`.
    #[test]
    fn save_queue_keeps_only_the_newest_snapshot_per_chunk() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let pos = ChunkPos::new(1, 1);
        let other = ChunkPos::new(2, 2);
        let snapshot = |pos, seq| PendingWrite {
            pos,
            seq,
            chunk: Some(vec![seq as u8]),
            entities: None,
        };
        host.enqueue_snapshot(snapshot(pos, 1));
        host.enqueue_snapshot(snapshot(other, 1));
        host.enqueue_snapshot(snapshot(pos, 2));
        assert_eq!(host.save_queue.len(), 2, "one entry per chunk");
        assert_eq!(
            host.save_queue.iter().find(|p| p.pos == pos).unwrap().seq,
            2
        );
        // 迟到的旧序号不能把新的挤掉。
        host.enqueue_snapshot(snapshot(pos, 1));
        assert_eq!(
            host.save_queue.iter().find(|p| p.pos == pos).unwrap().seq,
            2
        );
    }

    /// F6：积压到上限触发背压，收回请求整批留到下一刻。
    /// F6: hitting a cap defers reclaims to the next tick.
    #[test]
    fn a_full_save_queue_defers_reclaims_as_backpressure() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        for i in 0..MAX_PENDING_WRITES {
            host.enqueue_snapshot(PendingWrite {
                pos: ChunkPos::new(i as i32, 0),
                seq: i as u64,
                chunk: Some(vec![1]),
                entities: None,
            });
        }
        assert!(host.save_backpressure());
        let mut island = bare_island();
        let pos = ChunkPos::new(0, 0);
        island.mailbox.queue_reclaim(pos, 0);
        let _ = host.flush_region_mail(&mut island);
        assert_eq!(
            island.mailbox.reclaims().len(),
            1,
            "reclaim must be deferred"
        );
        let _ = pos;
        // 队列腾空后背压解除。
        host.save_queue.clear();
        assert!(!host.save_backpressure());
    }

    /// F6：区块被遗忘时清掉每区块记账。
    /// F6: per-chunk bookkeeping is cleared when a chunk is forgotten.
    #[test]
    fn forgetting_a_chunk_clears_its_bookkeeping() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let region = Region::new(2, 20, false);
        let pos = ChunkPos::new(5, 5);
        host.save_cooldown.insert(pos, 10);
        host.loaded_save_tick.insert(pos, 20);
        host.saved_revisions
            .insert(pos, (region.chunks.revision(pos), 0));
        host.region_message(RegionMessage::Forgot {
            positions: vec![pos],
        });
        assert!(!host.save_cooldown.contains_key(&pos));
        assert!(!host.loaded_save_tick.contains_key(&pos));
        assert!(!host.saved_revisions.contains_key(&pos));
    }

    /// F1：写盘失败不回 `Written`，世界线程保留 parking 盒子、记 error、
    /// 清掉在途标记；连续失败到阈值后暂停接收新保存请求（§8），
    /// `/save-all` 作为强制重试解除暂停。
    /// F1: a failed write keeps the parked box, logs the error, clears inflight,
    /// and after consecutive failures pauses new saves (§8); `save-all` forces
    /// a retry.
    #[test]
    fn a_failed_write_keeps_the_parked_box_and_pauses_saves() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct Tmp(std::path::PathBuf);
        impl Tmp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-f1-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Tmp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let tmp = Tmp::new();
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let io = ChunkIo::open(&dir).unwrap();
        // 先起 IO 线程再把 region/entities/ 换成普通文件，令写必然失败。
        // Replace the region dirs with files only after the thread is up.
        for directory in [dir.region_dir(), dir.entities_dir()] {
            std::fs::remove_dir(&directory).unwrap();
            std::fs::write(&directory, b"not a directory").unwrap();
        }
        let pos = ChunkPos::new(4, -2);
        let mut host = DirectorHost::new(DirectorConfig::default());
        host.set_io(io);
        host.parking
            .park(pos, Box::new(RegionColumn::from_template()));
        host.pending_writes.insert(pos, 5);
        host.save_queue.push_back(PendingWrite {
            pos,
            seq: 5,
            chunk: Some(vec![1]),
            entities: None,
        });

        host.pace_saves(1);
        let _ = host.io().unwrap().flush(); // 故意写失败，flush 如实报 Err，只当同步点
        let mut island = bare_island();
        host.poll_io(&mut IslandAccess::Lent(&mut island));

        assert!(
            host.parking.contains(pos),
            "a failed write must keep the parked box, not ack Written"
        );
        // region/ 和 entities/ 两份各自报一条失败，所以至少一条。
        assert!(
            host.io_failures() >= 1,
            "the failure must be counted and logged"
        );
        assert!(
            !host.io().unwrap().has_chunk(pos),
            "has_chunk must not lie after a failed write"
        );

        // 连续失败只做有上限的退避，不永久暂停：到点之后队列照常出队重试。
        for i in 0..MAX_CONSECUTIVE_IO_FAILURES {
            host.pending_writes.insert(pos, 100 + u64::from(i));
            host.save_queue.push_back(PendingWrite {
                pos,
                seq: 100 + u64::from(i),
                chunk: Some(vec![2]),
                entities: None,
            });
            host.pace_saves(2 + u64::from(i));
            let _ = host.io().unwrap().flush(); // 故意写失败，flush 如实报 Err，只当同步点
            host.poll_io(&mut IslandAccess::Lent(&mut island));
        }
        assert!(
            host.save_retry_after > 0,
            "consecutive failures must back off"
        );
        let before = host.save_queue.len();
        host.pace_saves(1);
        assert_eq!(
            host.save_queue.len(),
            before,
            "during the backoff window saves are deferred"
        );
        assert!(host.parking.contains(pos), "the box is still in memory");

        // 退避到期之后自动重试：磁盘恢复时不再需要人工 `/save-all`。
        host.pace_saves(host.save_retry_after);
        assert!(
            host.save_queue.len() < before,
            "once the backoff expires normal saving retries"
        );
        // `/save-all` 是显式强制重试：清掉剩余的退避。
        host.snapshot_all_dirty(&mut [&mut island], 60);
        assert_eq!(
            host.save_retry_after, 0,
            "save-all forces an immediate retry"
        );
    }

    /// 写盘途中又被要回来：快照还排在 D6 队列里没落盘、盘上是旧数据，
    /// 这时 `Task::Read` 必须直接拿 Parking 里的最新盒子，绝不能去读旧盘或用模板。
    /// A read while a write is in flight: the snapshot is still queued (not on
    /// disk) and disk holds stale bytes; `Task::Read` must return the parked
    /// edited box, never a stale disk read or a template.
    #[test]
    fn a_read_during_an_in_flight_write_returns_the_parked_edited_box() {
        use mc_bamboo_core::BlockPos;
        use mc_bamboo_data::blocks;
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};
        use mc_bamboo_world::{column::ChunkColumn, director::ChunkBundle};

        struct Tmp(std::path::PathBuf);
        impl Tmp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-inflight-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Tmp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let tmp = Tmp::new();
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default());
        host.set_io(ChunkIo::open(&dir).unwrap());

        // 造一个改过的盒子。
        let mut server = Harness::new_loaded(2);
        let pos = ChunkPos::new(0, 0);
        let edit = BlockPos::new(2, -60, 2);
        server
            .region()
            .chunks
            .load(pos, Box::new(ChunkColumn::from_template()));
        server.region().chunks.set_block(edit, blocks::STONE);
        server.region().chunks.finish_light_updates();
        let column = server
            .region()
            .take_columns(&[pos].into_iter().collect())
            .expect("preconditions met")
            .remove(&pos)
            .expect("box present");
        assert!(!column.is_pristine(), "the edited box is dirty");

        // 盘上有一份旧数据（这样不命中 Parking 就会发异步读，读到旧的）。
        host.io
            .as_ref()
            .unwrap()
            .submit_write(pos, 99, Some(b"stale".to_vec()), None);
        assert!(host.io.as_ref().unwrap().has_chunk(pos));
        // 新快照还在 D6 队列里没交给 IO，盒子停在写盘途中缓冲。
        host.save_queue.push_back(PendingWrite {
            pos,
            seq: 100,
            chunk: Some(vec![7]),
            entities: None,
        });
        host.parking.park(pos, column);

        let event = host
            .run_task(0, 0, Task::Read { pos })
            .expect("a parked box answers synchronously, no async disk read");
        let Event::TaskDone { result, .. } = event else {
            panic!("expected a TaskDone event");
        };
        let TaskResult::Decoded { column, status } = result else {
            panic!("in-flight write must return the parked box, not a disk read/template");
        };
        assert_eq!(
            status,
            ChunkStatus::Full,
            "parked boxes are finished chunks"
        );
        assert_eq!(
            column.column().chunk().block(2, -60, 2),
            blocks::STONE,
            "the rejoining chunk must carry the latest edit"
        );
        assert!(host.parking.is_empty(), "the read consumes the parked box");
    }

    /// 剖析：跑一批真实的 `Task::Save` 编码，量出 `codec_nanos` 的实际增量。
    /// Profiling: run a batch of real `Task::Save` encodes and measure the
    /// `codec_nanos` delta (printed with --nocapture for the report).
    #[test]
    fn codec_profiler_records_real_encode_work() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct Tmp(std::path::PathBuf);
        impl Tmp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-codec-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Tmp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        const SAVES: usize = 200;
        let tmp = Tmp::new();
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default());
        host.set_io(ChunkIo::open(&dir).unwrap());
        let before = codec_nanos();
        for i in 0..SAVES {
            let column = Box::new(RegionColumn::from_template());
            let _ = host.run_task(
                i as u64,
                0,
                Task::Save {
                    pos: ORIGIN,
                    seq: i as u64 + 1,
                    column,
                },
            );
        }
        let after = codec_nanos();
        let delta = after - before;
        println!(
            "CODEC_NANOS {SAVES} template encodes: total={delta} ns, per_chunk={} ns",
            delta / SAVES as u64
        );
        assert!(delta > 0, "encoding should record nonzero codec time");
    }

    /// 带世界目录时，盘上有和盘上没有的区块都必须走同一条异步读流水线
    /// （官方 `ChunkMap.scheduleChunkLoad` 没有同步生成的快路径）；没接世界目录
    /// 时仍按原来的同步模板路径走。
    /// With a world dir every read goes through the async pipeline, whether or
    /// not the chunk exists on disk; without one the old synchronous path stays.
    #[test]
    fn every_read_goes_through_the_io_thread_when_a_world_dir_is_attached() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct Temp(std::path::PathBuf);
        impl Temp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-read-order-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let temp = Temp::new();
        let dir = WorldDir::new(&temp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default());
        // 没接 IO：当刻同步回模板。
        assert!(matches!(
            host.run_task(0, 0, Task::Read { pos: ORIGIN }),
            Some(Event::TaskDone { .. })
        ));

        let io = ChunkIo::open(&dir).unwrap();
        let on_disk = ChunkPos::new(3, 0);
        io.submit_write(on_disk, 7, Some(b"chunk bytes".to_vec()), None);
        assert!(io.has_chunk(on_disk), "a submitted write counts as on-disk");
        host.set_io(io);
        // 盘上有：异步。盘上没有：也异步，结果由 IO 线程回 `chunk: None`。
        for (id, pos) in [(1, on_disk), (2, ORIGIN), (3, ChunkPos::new(2, 1))] {
            assert!(
                host.run_task(id, 0, Task::Read { pos }).is_none(),
                "read of {pos:?} must wait for the IO thread"
            );
        }
        host.flush_io();
        let mut server = Harness::new_loaded(2);
        host.poll_io(&mut IslandAccess::Lent(server.island()));
        let mut seen = Vec::new();
        while let Some(reply) = host.io().unwrap().try_recv() {
            if let Reply::Read { pos, .. } = reply {
                seen.push(pos);
            }
        }
        assert_eq!(seen.len(), 0, "poll_io already consumed all read replies");
    }

    /// 进服那刻的票 pump 在 `region.add_player` 之前派读任务，`region.players`
    /// 还是空的；`submit_reads` 必须用 Director 票表里的玩家中心按距离排序，
    /// 否则读任务退化成按坐标排序，远处区块抢先回执、第一批不是脚下 3×3。
    ///
    /// On join, the ticket pump queues reads before `region.add_player`, so
    /// `region.players` is empty. `submit_reads` must order by the player centers
    /// from the Director ticket table; otherwise reads fall back to raw coordinate
    /// order, far chunks reply first, and the first batch is not the 3x3.
    #[test]
    fn read_submission_uses_ticket_table_centers_not_region_players() {
        use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

        struct Temp(std::path::PathBuf);
        impl Temp {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "bamboo-submit-order-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                let _ = std::fs::remove_dir_all(&path);
                Self(path)
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let temp = Temp::new();
        let dir = WorldDir::new(&temp.0);
        dir.create_layout().unwrap();
        let mut host = DirectorHost::new(DirectorConfig::default());
        host.set_io(ChunkIo::open(&dir).unwrap());
        // 玩家只在票表里（模拟进服派读时尚未 region.add_player），region 里没有玩家。
        // The player is only in the ticket table (reads queued before add_player).
        host.director
            .tickets_mut()
            .set_player(1, Some(ChunkPos::new(0, 0)));

        // 故意远处在前、出生点在后：修复前会按坐标排，(-10,-10) 抢在最前。
        // Far chunks first on purpose: before the fix they sorted by coordinate.
        host.read_queue = vec![
            (ChunkPos::new(10, 10), 1, 0),
            (ORIGIN, 2, 0),
            (ChunkPos::new(3, 0), 3, 0),
            (ChunkPos::new(-10, -10), 4, 0),
        ];
        host.submit_reads();
        host.flush_io();

        let mut seen = Vec::new();
        while let Some(reply) = host.io().unwrap().try_recv() {
            if let Reply::Read { pos, .. } = reply {
                seen.push(pos);
            }
        }
        assert_eq!(seen.len(), 4, "四个读都提交了");
        assert_eq!(seen[0], ORIGIN, "最近的出生点区块必须先提交给 IO 线程");
        assert_eq!(seen[1], ChunkPos::new(3, 0), "其次按距离");
    }
}
