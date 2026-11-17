//! 岛和它的区块信箱（岛的实施计划第 6 节第 6 步）。
//!
//! 岛是一个 [`Region`] 加上只属于它的执行状态，可以整个交给别的线程（`Island: Send`）。
//! 它有两种所有权状态：**停放**时在主控手里，主控可以把它借给一次调用；**运行**时它在
//! 跑自己的一刻，主控碰不到它。
//!
//! Director 发给岛的交付、收回和换档不再由宿主直接写进 `Region`，而是先投进主控的
//! 邮筒（[`Posted`]），再由岛自己的 [`ChunkMailbox`] 收下、按预算交付 / 收回，把回执
//! （`Delivered` / `Returned`）交还主控。什么时候收取由驱动决定（[`IslandDrive`]）：
//!
//! - **齐步**（`island_threads = 0`）：岛停放着、借给主控，宿主每轮 `pump` 收敛前当场让岛
//!   收取，和以前直接写 `Region` 的时点、顺序逐位相同（精确模式）。
//! - **分离**（`island_threads ≥ 1`，缺省）：宿主只投信，岛在自己下一刻开头收取，回执在这一刻
//!   交回时给主控。区块因此比齐步晚一刻交付。第 7 步起岛这一刻的主体在 tick 线程上跑，
//!   和主控的 Director 推进并行（`island_pool.rs`、`World::tick_phases`）。
//!
//! The island and its chunk mailbox (island plan, section 6, step 6). An island
//! is a [`Region`] plus the execution state that belongs only to it, and can be
//! handed whole to another thread (`Island: Send`). It is either **parked**
//! (held by the master, which may lend it to one call) or **running** its own
//! tick (out of the master's reach). Director deliveries, reclaims and level
//! changes are no longer written into the `Region` by the host: they are posted
//! ([`Posted`]) and the island's own [`ChunkMailbox`] takes them, delivers and
//! reclaims within the budget, and hands the acks back. When it takes them is
//! the drive's choice ([`IslandDrive`]): lockstep (`island_threads = 0`) applies
//! them on the spot at the same points as before (exact mode); detached (the
//! default) applies them at the start of the island's next tick, so chunks
//! arrive one tick later, and since step 7 runs the island's tick on a tick
//! thread in parallel with the master's Director step.

use std::{collections::BTreeMap, sync::Arc};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{
    director::{DemandSnapshot, Desired, Event, RegionId},
    store::ChunkRevision,
    ticket::LevelChange,
};

use crate::{
    chunk_codec::PackedChunk,
    director_host::MailBudget,
    global_port::GlobalEffect,
    profiler::{Metric, SubPhase},
    region::{Region, RegionColumn},
};

/// 收回的积压大了之后，每刻收掉其中的这么多分之一。
///
/// 交付的上限是为了削进服的尖峰；收回要是也卡死在固定的个数上，很多人同时
/// 高速移动时每刻新排进来的比收走的多，已加载的区块数和内存就没有上界。
/// 按积压的比例收，积压会稳定在「每刻新排进来的个数 × 这个数」附近。
pub(crate) const RECLAIM_DRAIN_TICKS: usize = 20;

/// 排队用的 rank：`(玩家优先级, x, z)`，小的先出。
pub(crate) type DemandRank = (i64, i32, i32);

/// `island_threads` 没有显式给出时的值（岛的实施计划第 6 节第 7 步按测量定）。
///
/// The `island_threads` value when none is given (island plan, step 7, by measurement).
pub const DEFAULT_ISLAND_THREADS: usize = 1;

/// 改 `island_threads` 缺省值的环境变量：对拍、探针和测试用它整进程切到精确模式（`0`）。
///
/// The environment variable that overrides the `island_threads` default:
/// oracles, probes and tests use it to put a whole process in exact mode (`0`).
pub const ISLAND_THREADS_ENV: &str = "BAMBOO_ISLAND_THREADS";

/// 环境变量给的 `island_threads`。没设或者不是非负整数时是 `None`。
///
/// The `island_threads` given by the environment, or `None` when unset or not
/// a non-negative integer.
pub fn island_threads_from_env() -> Option<usize> {
    let value = std::env::var(ISLAND_THREADS_ENV).ok()?;
    match value.trim().parse::<usize>() {
        Ok(threads) => Some(threads.min(crate::island_pool::MAX_ISLAND_THREADS)),
        Err(_) => {
            tracing::warn!(%value, "ignoring an invalid {ISLAND_THREADS_ENV}");
            None
        }
    }
}

/// 没有显式选择时的 `island_threads`：环境变量优先，没有就是 [`DEFAULT_ISLAND_THREADS`]。
/// 要求逐位一致的测试、对拍和探针都显式给 0，不靠这个缺省值。
///
/// The `island_threads` used when nothing is chosen explicitly: the
/// environment wins, otherwise [`DEFAULT_ISLAND_THREADS`]. Bit-exact tests,
/// oracles and probes give 0 explicitly instead of relying on it.
pub fn default_island_threads() -> usize {
    island_threads_from_env().unwrap_or(DEFAULT_ISLAND_THREADS)
}

/// 主控和岛之间怎么交接，由 `island_threads` 决定：0 是齐步，≥ 1 是分离。
///
/// How the master and the island hand over, decided by `island_threads`:
/// 0 is lockstep, 1 or more is detached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IslandDrive {
    /// 齐步（`island_threads = 0`）：岛一直在主控线程上，借给主控，信箱在原来的时点
    /// 当场应用，和官方逐位一致（精确模式）。
    ///
    /// Lockstep (`island_threads = 0`): the island stays on the master thread,
    /// lent to the master, and mail is applied on the spot, bit for bit (exact mode).
    Lockstep,
    /// 分离（`island_threads ≥ 1`）：岛的一刻在 tick 线程上跑，和主控的 Director 推进
    /// 并行；信箱在岛下一刻开头应用，区块交付晚一刻。
    ///
    /// Detached (`island_threads ≥ 1`): the island's tick runs on a tick
    /// thread in parallel with the master's Director step; mail is applied at
    /// the start of the island's next tick, so chunks arrive one tick later.
    Detached,
}

impl IslandDrive {
    pub(crate) fn from_threads(threads: usize) -> Self {
        if threads == 0 {
            Self::Lockstep
        } else {
            Self::Detached
        }
    }
}

/// 岛的所有权状态。
///
/// The island's ownership state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IslandState {
    /// 在主控手里，可以借出、拆合、编码存盘快照。
    ///
    /// Held by the master: it may be lent, split or merged.
    Parked,
    /// 正在跑自己的一刻；主控发给它的信件只能投进邮筒。
    ///
    /// Running its own tick; the master can only post mail to it.
    Running,
}

/// 主控这一次调用能不能直接用岛。
///
/// Whether the master may use the island directly during one call.
pub(crate) enum IslandAccess<'a> {
    /// 岛停放着、借给这次调用（齐步，或者关服、`/save-all` 这类栅栏）：信件当场收取。
    ///
    /// The parked island is lent to this call (lockstep, or a barrier such as
    /// shutdown or `/save-all`): mail is taken on the spot.
    Lent(&'a mut Island),
    /// 岛不在主控手里（分离驱动）：信件留在邮筒里，岛下一刻开头取走。
    ///
    /// The island is out of reach (detached drive): mail stays posted until the
    /// island's next tick.
    Away,
}

/// 一个岛：一个 `Region`、只属于它的执行配置和它的信箱。可以整个交给 tick 线程。
///
/// An island: one `Region`, the execution settings that belong to it, and its
/// mailbox. It can be handed whole to a tick thread.
pub(crate) struct Island {
    /// 岛的编号，和 Director 里的 `Owner::Region(id)` 一致。
    pub(crate) id: RegionId,
    pub(crate) region: Region,
    /// 实体阶段的线程数。0 = 精确驱动；≥ 1 = 并行驱动。
    pub(crate) entity_threads: usize,
    /// 排队实现下这一刻攒下、等交回时由主控应用的全局改动。直接实现下一直是空的。
    ///
    /// Global changes collected this tick under the queued port, applied by the
    /// master at hand-back. Always empty under the direct port.
    pub(crate) effects: Vec<GlobalEffect>,
    /// 区块信箱。
    pub(crate) mailbox: ChunkMailbox,
    /// 分离驱动下岛自己收信产生的回执，交回时给主控。齐步下一直是空的。
    ///
    /// Acks the island produced while taking its own mail under the detached
    /// drive, handed to the master at hand-back. Always empty in lockstep.
    pub(crate) replies: MailReport,
    /// 主控上次交回时给的存盘名单，分离驱动下岛在这一刻末尾照着编码。齐步下一直是空的。
    ///
    /// The save list the master gave at the last hand-back; a detached island
    /// encodes it at the end of this tick. Always empty in lockstep.
    pub(crate) save_plan: SavePlan,
    /// 分离驱动下这一刻末尾照名单编好的快照和新的改动，交回时给主控。齐步下一直是空的。
    ///
    /// The snapshots encoded from the list and the new changes at the end of
    /// a detached tick, for the master at hand-back. Always empty in lockstep.
    pub(crate) save_report: SaveReport,
    /// 上次跑它的 tick 线程，下次尽量还交给它。
    ///
    /// The tick thread that ran it last; it goes back there when possible.
    pub(crate) last_worker: usize,
    pub(crate) state: IslandState,
    /// 测试专用：下一刻在 tick 线程上开头要做的事（卡住、panic）。
    #[cfg(test)]
    pub(crate) test_hook: Option<IslandTestHook>,
}

/// 测试专用：岛的下一刻在 tick 线程上开头做的事。
#[cfg(test)]
pub(crate) enum IslandTestHook {
    /// 记下跑这一刻的线程名。
    RecordThread(std::sync::Arc<std::sync::Mutex<Option<String>>>),
    /// 卡住，直到这个标志被置上。
    Stall(std::sync::Arc<std::sync::atomic::AtomicBool>),
    Panic,
}

// 岛要能整个交给 tick 线程（第 7 步）。
// The island must be movable to a tick thread (step 7).
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Island>();
};

/// 主控手里放岛的地方。岛停放时在这里（在堆上，交给 tick 线程只搬一个指针）；
/// 派出去跑的时候整个被拿走，主控这段时间只拿得到自己的状态。停放时可以像
/// `Island` 一样直接用（`Deref`）。
///
/// Where the master keeps the island. A parked island lives here (on the heap,
/// so handing it to a tick thread moves one pointer); while it runs it is taken
/// out whole and the master only holds its own state. A parked island is used
/// like an `Island` through `Deref`.
pub(crate) struct IslandSlot(Option<Box<Island>>);

impl IslandSlot {
    pub(crate) fn new(island: Box<Island>) -> Self {
        Self(Some(island))
    }

    /// 岛在不在主控手里。
    pub(crate) fn is_home(&self) -> bool {
        self.0.is_some()
    }

    /// 拿走岛，交给 tick 线程。
    pub(crate) fn take(&mut self) -> Box<Island> {
        self.0.take().expect("岛已经交出去了")
    }

    /// 岛交回来了。
    pub(crate) fn put(&mut self, island: Box<Island>) {
        debug_assert!(self.0.is_none(), "岛已经在主控手里");
        self.0 = Some(island);
    }
}

impl std::ops::Deref for IslandSlot {
    type Target = Island;

    fn deref(&self) -> &Island {
        self.0
            .as_deref()
            .expect("岛正在 tick 线程上跑，主控碰不到它")
    }
}

impl std::ops::DerefMut for IslandSlot {
    fn deref_mut(&mut self) -> &mut Island {
        self.0
            .as_deref_mut()
            .expect("岛正在 tick 线程上跑，主控碰不到它")
    }
}

impl Island {
    pub(crate) fn new(id: RegionId, region: Region, budget: MailBudget) -> Self {
        Self {
            id,
            region,
            entity_threads: 0,
            effects: Vec::new(),
            mailbox: ChunkMailbox::new(budget),
            replies: MailReport::default(),
            save_plan: SavePlan::default(),
            save_report: SaveReport::default(),
            last_worker: 0,
            state: IslandState::Parked,
            #[cfg(test)]
            test_hook: None,
        }
    }

    /// 测试专用：跑掉下一刻的钩子。
    #[cfg(test)]
    pub(crate) fn run_test_hook(&mut self) {
        match self.test_hook.take() {
            Some(IslandTestHook::RecordThread(name)) => {
                *name.lock().unwrap() = std::thread::current().name().map(str::to_owned);
            }
            Some(IslandTestHook::Stall(release)) => {
                while !release.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            Some(IslandTestHook::Panic) => panic!("岛的测试 panic"),
            None => {}
        }
    }

    /// 停放 → 运行。
    pub(crate) fn dispatch(&mut self) {
        debug_assert_eq!(self.state, IslandState::Parked, "岛已经在跑");
        self.state = IslandState::Running;
    }

    /// 运行 → 停放（交回）。
    pub(crate) fn park(&mut self) {
        self.state = IslandState::Parked;
    }

    /// 收下邮筒里的信，按 `order` 交付、收回，回执写进 `report`。
    ///
    /// Take the posted mail, deliver and reclaim per `order`, and write the
    /// acks into `report`.
    pub(crate) fn take_mail(
        &mut self,
        posted: &mut Vec<Posted>,
        order: &MailOrder,
        report: &mut MailReport,
    ) {
        self.mailbox.receive(&mut self.region, posted);
        self.mailbox.apply(self.id, &mut self.region, order, report);
    }

    /// 取走上次存盘之后改过的已加载区块（判脏的原料），交给主控判。
    ///
    /// 方块 / 光照看 `ChunkStore` 版本号变过的集合，上层内容看 `Region::dirty`
    /// 和计划刻的增删；都只碰改动集合，不扫全部已加载区块（复查报告「高 3」）。
    ///
    /// Take the loaded chunks changed since the last save pass (the raw
    /// material for the master's dirty check). Only the change sets are read,
    /// never every loaded chunk (review high 3).
    pub(crate) fn drain_save_changes(&mut self) -> SaveChanges {
        let region = &mut self.region;
        region.profiler.start_sub(SubPhase::StorageDirty);
        let mut changes = SaveChanges::default();
        for pos in region.chunks.take_changed_chunks() {
            if !region.chunks.is_loaded(pos) {
                continue;
            }
            changes.changed.push(ChangedChunk {
                pos,
                revision: region.chunks.revision(pos),
                pristine: region.chunks.is_pristine(pos),
                upper_dirty: region.dirty.pending_contains(pos)
                    || region.block_ticks.dirty_contains(pos),
            });
        }
        // 计划刻的增删单独记在 BlockTicks 里，一并取走。
        for pos in region.block_ticks.take_dirty() {
            region.dirty.mark(pos);
        }
        changes.content = region
            .dirty
            .take()
            .into_iter()
            .filter(|&pos| region.chunks.is_loaded(pos))
            .collect();
        region.profiler.end_sub();
        changes
    }

    /// 照主控的名单做：版本变了但这次不存的区块放回改动集合，记指标，按顺序编码。
    /// 编码走和卸载存盘同一条 `encode_loaded_column`，不卸载、不改状态。
    ///
    /// Follow the master's list: put back chunks that changed but are not
    /// saved this time, record the metrics, and encode in order through the
    /// same `encode_loaded_column` as unload saves, without changing state.
    pub(crate) fn follow_save_plan(&mut self, plan: SavePlan) -> Vec<Encoded> {
        let SavePlan {
            remark,
            encode,
            metrics,
            #[cfg(test)]
            verify,
            ..
        } = plan;
        let region = &mut self.region;
        for pos in remark {
            region.chunks.mark_changed(pos);
        }
        #[cfg(test)]
        {
            // 校验器：被判成「没改过」的已保存区块，重新编码必须和上次写出的字节一致。
            for (pos, revision) in verify {
                if region.chunks.revision(pos) == revision {
                    let immutable: &Region = region;
                    immutable.dirty.verify_clean(immutable, pos);
                }
            }
        }
        if let Some((candidates, snapshots)) = metrics {
            region
                .profiler
                .record(Metric::StorageCandidates, candidates);
            region.profiler.record(Metric::StorageSnapshots, snapshots);
        }
        encode
            .into_iter()
            .map(|pos| {
                region.profiler.start_sub(SubPhase::StorageEncode);
                let result = region.encode_loaded_column(pos);
                region.profiler.end_sub();
                Encoded {
                    pos,
                    revision: region.chunks.revision(pos),
                    result,
                }
            })
            .collect()
    }

    /// 分离驱动下这一刻末尾的存盘：照上次交回时的名单编码，再按名单要求把新的改动
    /// 一起交回。
    ///
    /// The detached save step at the end of the tick: encode the list given at
    /// the last hand-back, then hand back the new changes if the list asks.
    pub(crate) fn save_pass(&mut self) -> SaveReport {
        let plan = std::mem::take(&mut self.save_plan);
        let drain = plan.drain;
        let encoded = self.follow_save_plan(plan);
        let changes = drain.then(|| self.drain_save_changes());
        SaveReport { encoded, changes }
    }
}

/// 主控交给岛的存盘名单（岛的实施计划第 2 节第 23 行：「主控出名单、岛编码交回」）。
///
/// The save list the master gives an island (island plan, section 2, row 23).
#[derive(Default)]
pub(crate) struct SavePlan {
    /// 版本号变了、但这一次不该存的区块：放回岛的改动集合，下次再判。
    pub(crate) remark: Vec<ChunkPos>,
    /// 要编码的区块，按这个顺序编、按这个顺序交回。
    pub(crate) encode: Vec<ChunkPos>,
    /// 编码前记的两个剖析指标（判脏候选数、这次拍的快照数）；`None` 不记。
    pub(crate) metrics: Option<(u64, u64)>,
    /// 编完之后要不要把新的改动交回来（分离驱动；齐步由主控当场要）。
    pub(crate) drain: bool,
    /// 测试专用：主控判成「没改过」的已存区块和它们存时的版本号，岛复核没漏标脏。
    #[cfg(test)]
    pub(crate) verify: Vec<(ChunkPos, ChunkRevision)>,
}

/// 一个版本号变过的已加载区块。
pub(crate) struct ChangedChunk {
    pub(crate) pos: ChunkPos,
    pub(crate) revision: ChunkRevision,
    /// 还保持生成器原样（官方 `isUnsaved` 为假，原样区块不写）。
    pub(crate) pristine: bool,
    /// 上层内容或计划刻也改过。
    pub(crate) upper_dirty: bool,
}

/// 上次存盘之后的改动：版本号变过的区块和上层内容改过的区块，都只含已加载的。
#[derive(Default)]
pub(crate) struct SaveChanges {
    pub(crate) changed: Vec<ChangedChunk>,
    pub(crate) content: Vec<ChunkPos>,
}

/// 岛编好的一份快照。`result` 为 `None` 表示没加载或表正忙，下次重试。
pub(crate) struct Encoded {
    pub(crate) pos: ChunkPos,
    pub(crate) revision: ChunkRevision,
    pub(crate) result: Option<Result<PackedChunk, String>>,
}

/// 岛交回的存盘报告。
#[derive(Default)]
pub(crate) struct SaveReport {
    pub(crate) encoded: Vec<Encoded>,
    pub(crate) changes: Option<SaveChanges>,
}

/// 主控投给岛的一封信，按投递的顺序收取。
///
/// One letter the master posts to an island, taken in posting order.
pub(crate) enum Posted {
    Deliver {
        pos: ChunkPos,
        generation: u64,
        desired: Desired,
        column: Box<RegionColumn>,
    },
    Reclaim(Vec<(ChunkPos, u64)>),
    Levels(Vec<LevelChange>),
}

/// 岛收信时要用的主控状态：玩家需求（排交付顺序）和背压。主控每次让岛收信时给一份。
///
/// The master state the island needs to take its mail: the player demand (for
/// delivery order) and backpressure. The master hands one over each time.
pub(crate) struct MailOrder {
    pub(crate) demand: Arc<DemandSnapshot>,
    /// 需求快照里没有玩家时退回的旧口径：票表里的玩家中心。
    pub(crate) centers: Vec<ChunkPos>,
    /// 存盘积压到上限或 `/save-off` 时不收回。
    pub(crate) skip_reclaims: bool,
}

impl MailOrder {
    /// 排队用的 rank。没有玩家时 rank 全是 0，按坐标排（原来的兜底顺序）。
    pub(crate) fn rank(&self, pos: ChunkPos) -> DemandRank {
        let rank = if self.demand.players().is_empty() {
            // 没玩家时退回旧口径：离票表里的玩家中心切比雪夫距离；
            // 票表也没玩家就按坐标。
            if self.centers.is_empty() {
                0
            } else {
                distance_to_players(pos, &self.centers).0 as i64
            }
        } else {
            self.demand.rank(pos)
        };
        (rank, pos.x, pos.z)
    }
}

/// 离最近的玩家有多远（切比雪夫距离）。没有玩家时所有位置一样远。
pub(crate) fn distance_to_players(pos: ChunkPos, centers: &[ChunkPos]) -> (u64, i32, i32) {
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

/// 玩家不能站在一个没加载的地方，进服、传送、高速移动都一样。
fn is_under_a_player(pos: ChunkPos, centers: &[ChunkPos]) -> bool {
    centers.iter().any(|center| {
        (i64::from(pos.x) - i64::from(center.x)).abs() <= 1
            && (i64::from(pos.z) - i64::from(center.z)).abs() <= 1
    })
}

/// 岛收一次信的结果：给 Director 的回执，和这次真正进出 `Region` 的区块。
///
/// The result of taking mail once: acks for the Director, and the chunks that
/// actually entered or left the `Region`.
#[derive(Default)]
pub(crate) struct MailReport {
    pub(crate) events: Vec<Event<Box<RegionColumn>>>,
    /// 按发生顺序：`true` 是交付进来，`false` 是从 region 收回。
    pub(crate) moves: Vec<(ChunkPos, bool)>,
}

impl MailReport {
    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty() && self.moves.is_empty()
    }
}

/// 岛的区块信箱：收下的、还没交付的盒子和还没收回的位置，以及每刻的预算。
///
/// The island's chunk mailbox: boxes taken but not yet delivered, positions
/// not yet reclaimed, and the per-tick budget.
pub(crate) struct ChunkMailbox {
    /// 待交付的盒子（带交付时的等级）。
    deliveries: BTreeMap<ChunkPos, (u64, Desired, Box<RegionColumn>)>,
    /// 待收回的位置。
    reclaims: Vec<(ChunkPos, u64)>,
    budget: MailBudget,
    /// 本刻还没花掉的交付额度。整刻内所有收信共用，进服 / 离开 / 移动各发一批票
    /// 也不会把上限乘上好几倍；每刻开头由 `begin_tick` 重置。
    budget_left: MailBudget,
    /// 这一刻已经从 region 取回了多少个区块。收回的上限会随积压放宽，
    /// 所以不能用「剩余额度」记，要记花掉的个数。
    reclaims_spent: usize,
    /// 统计：本刻交付 / 收回的区块数，给剖析器用。
    delivered: usize,
    reclaimed: usize,
}

impl ChunkMailbox {
    pub(crate) fn new(budget: MailBudget) -> Self {
        Self {
            deliveries: BTreeMap::new(),
            reclaims: Vec::new(),
            budget,
            budget_left: budget,
            reclaims_spent: 0,
            delivered: 0,
            reclaimed: 0,
        }
    }

    /// 每刻开头重置预算和计数。进服 / 离开 / 移动发来的票操作都算在这一刻里。
    pub(crate) fn begin_tick(&mut self) {
        self.budget_left = self.budget;
        self.reclaims_spent = 0;
        self.delivered = 0;
        self.reclaimed = 0;
    }

    /// 关服不设每刻预算，一次收完全部区块。
    pub(crate) fn lift_budget(&mut self) {
        self.budget = MailBudget {
            deliveries: 0,
            reclaims: 0,
        };
        self.budget_left = self.budget;
    }

    /// 本刻交付 / 收回的区块数。
    pub(crate) fn counters_this_tick(&self) -> (usize, usize) {
        (self.delivered, self.reclaimed)
    }

    /// 关服收尾：信箱里没交付的盒子不再有人要。
    pub(crate) fn clear(&mut self) {
        self.deliveries.clear();
        self.reclaims.clear();
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

    /// 本刻还欠着多少交付 / 收回。测试用它给信箱长度一个上界。
    #[cfg(test)]
    pub(crate) fn backlog(&self) -> (usize, usize) {
        (self.deliveries.len(), self.reclaims.len())
    }

    /// 测试专用：直接放一个待交付的盒子。
    #[cfg(test)]
    pub(crate) fn queue_delivery(
        &mut self,
        pos: ChunkPos,
        generation: u64,
        desired: Desired,
        column: Box<RegionColumn>,
    ) {
        self.deliveries.insert(pos, (generation, desired, column));
    }

    /// 测试专用：直接放一个待收回的位置。
    #[cfg(test)]
    pub(crate) fn queue_reclaim(&mut self, pos: ChunkPos, generation: u64) {
        self.reclaims.push((pos, generation));
    }

    /// 测试专用：`pos` 的盒子收下了、还在等交付。
    #[cfg(test)]
    pub(crate) fn has_delivery(&self, pos: ChunkPos) -> bool {
        self.deliveries.contains_key(&pos)
    }

    /// 测试专用：待收回的位置。
    #[cfg(test)]
    pub(crate) fn reclaims(&self) -> &[(ChunkPos, u64)] {
        &self.reclaims
    }

    /// 按投递顺序收下邮筒里的信。换档当场记进 `TickLevels`（只影响已交付的区块），
    /// 信箱里还没交付的盒子也换成新档位，否则新交付的区块会带着旧档位进 region。
    ///
    /// Take the posted letters in posting order. Level changes are applied to
    /// `TickLevels` at once (they only touch delivered chunks) and to boxes still
    /// waiting here, so a later delivery does not enter with a stale tier.
    pub(crate) fn receive(&mut self, region: &mut Region, posted: &mut Vec<Posted>) {
        for letter in posted.drain(..) {
            match letter {
                Posted::Deliver {
                    pos,
                    generation,
                    desired,
                    column,
                } => {
                    self.deliveries.insert(pos, (generation, desired, column));
                }
                Posted::Reclaim(positions) => self.reclaims.extend(positions),
                Posted::Levels(changes) => {
                    for change in &changes {
                        if let Some(entry) = self.deliveries.get_mut(&change.pos) {
                            entry.1 = Desired::new(change.loading.1, change.simulation.1);
                        }
                    }
                    region.profiler.time_sub(SubPhase::LevelsUpdate, || {
                        region.tick_levels.level_changed(&changes);
                    });
                }
            }
        }
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
        let deliveries = &mut self.deliveries;
        let mut returned = 0;
        self.reclaims.retain(|(pos, generation)| {
            let Some((_, _, column)) = deliveries.remove(pos) else {
                return true;
            };
            returned += 1;
            events.push(Event::Returned {
                pos: *pos,
                generation: *generation,
                column: Some(column),
            });
            false
        });
        self.reclaimed += returned;
    }

    /// 在 tick 边界交付和收回：先把收回请求瞄着的排队盒子退回，再交付，最后收回。
    ///
    /// 整刻最多交付 `budget.deliveries` 个非豁免区块（`0` 表示不限），
    /// 收回的上限见 [`Self::reclaim_cap`]，剩下的留到下一刻。
    /// 同一刻内会多次调用，交付共用 `budget_left` 里的剩余额度，
    /// 收回共用 `reclaims_spent`。
    ///
    /// Deliver and reclaim at the tick boundary within the per-tick budget;
    /// leftovers wait for the next tick.
    pub(crate) fn apply(
        &mut self,
        island: RegionId,
        region: &mut Region,
        order: &MailOrder,
        report: &mut MailReport,
    ) {
        let delivery_left = self.budget_left.deliveries;
        let limited_deliveries = self.budget.deliveries > 0;
        // 收回优先：盒子可能还在 `deliveries` 里排队（交付有了每刻上限之后，
        // 一个盒子可以在信箱里等好几刻）。这时 Director 已经把它记成
        // `Reclaiming`，盒子必须原封不动地作为 `Returned` 回去，绝不能先进
        // region 再被取出来，更不能晚几刻变成一个没人管的孤儿。
        // 这一步不碰 region，不占交付/收回上限，也不需要等光照。
        region.profiler.start_sub(SubPhase::ReturnBoxes);
        self.return_pending_deliveries(&mut report.events);
        region.profiler.end_sub();
        if self.deliveries.is_empty() && self.reclaims.is_empty() {
            return;
        }
        // 只有信箱里真有要交付/收回的东西时才把光照算完：这个函数每刻都调，
        // 无条件 `finish_light_updates` 会绕开「光照每刻有预算」的设计。
        // 交付/收回要求「没有算到一半的轮次」。
        region.profiler.start_sub(SubPhase::DeliveryLightFinish);
        region.chunks.finish_light_updates();
        region.profiler.end_sub();
        let centers: Vec<ChunkPos> = region
            .players
            .values()
            .map(|player| player.chunk_pos())
            .collect();

        // 近的先交付。玩家脚下 3×3 全部交付且不算进上限；同等 rank 按坐标排。
        region.profiler.start_sub(SubPhase::MailSort);
        let mut ordered: Vec<ChunkPos> = self.deliveries.keys().copied().collect();
        // BTreeMap 的键已经按 (x, z) 升序，稳定排序让同等 rank 保持坐标顺序。
        ordered.sort_by_key(|&pos| {
            let guaranteed = is_under_a_player(pos, &centers);
            (!guaranteed, order.rank(pos))
        });
        region.profiler.end_sub();
        let mut used = 0usize;
        // 一批交付只重算一次受影响的邻域，不逐块重算；每次成功都要把
        // 信箱里记着的最新等级交给 region（脚下 3×3 和下一刻才轮到的都算）。
        let mut delivered_levels = Vec::new();
        for pos in ordered {
            let guaranteed = is_under_a_player(pos, &centers);
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
                    report.moves.push((pos, true));
                    report.events.push(Event::Delivered {
                        region: island,
                        pos,
                        generation,
                    });
                }
                Err((_, column)) => {
                    // 前提不满足（例如某张表正在 tick 中途）：留在信箱里，
                    // 下一次收信再试，绝不把盒子丢掉。
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

        // F6 背压只跳收回，交付（含玩家脚下 3×3）照常（复查「中 4」）。
        // 官方 `ChunkMap.tick` 在 `noSave` 时也跳过整个 processUnloads，
        // 所以 save-off 期间连收回都不做，区块保持加载（复查「中 7」）。
        // Backpressure skips only reclaims, never deliveries (review 4).
        // Official `ChunkMap.tick` skips processUnloads under noSave, so
        // save-off also keeps chunks loaded (review 7).
        if self.reclaims.is_empty() || order.skip_reclaims {
            return;
        }
        let reclaimed_before = self.reclaimed;
        // 远的先收回：这些区块不急，玩家脚下的票也没了时先留着更保险。
        region.profiler.start_sub(SubPhase::MailSort);
        let mut by_generation = BTreeMap::new();
        for (pos, generation) in std::mem::take(&mut self.reclaims) {
            by_generation.insert(pos, generation);
        }
        let mut ordered: Vec<ChunkPos> = by_generation.keys().copied().collect();
        // 距离取相反数，远的在前；同距离按坐标升序。
        // 积压可以有几千个，到玩家的距离每个位置只算一次。
        ordered.sort_by_cached_key(|pos| {
            let (far, x, z) = distance_to_players(*pos, &centers);
            (std::cmp::Reverse(far), x, z)
        });
        region.profiler.end_sub();
        // 这一刻要收回的总数 = 已经收的 + 还在信箱里的。收一个，前一项加一、
        // 后一项减一，总数不变；所以同一刻里不管收信几次，
        // 上限只会因为新排进来的请求变大，不会因为多调用几次而重新发一份额度。
        let cap = Self::reclaim_cap(self.budget.reclaims, self.reclaims_spent + ordered.len());
        let count = ordered.len().min(cap.saturating_sub(self.reclaims_spent));
        if count == 0 {
            // 这一刻的额度用完了，全部留到下一刻。
            self.reclaims.extend(by_generation);
            return;
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
                    report.moves.push((pos, false));
                    report.events.push(Event::Returned {
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
