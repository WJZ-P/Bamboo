//! Director 的状态、事件和输出。
//!
//! 一次 [`Director::handle`] 是纯的：只读自己的状态和事件，只往返回的 [`Effect`]
//! 列表里写。宿主负责真正跑任务、搬盒子、发消息。

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap},
};

use mc_bamboo_core::ChunkPos;

use crate::ticket::{
    ChunkStatus, LevelChange, TicketTable, TicketType, generation_status, is_block_ticking,
    is_entity_ticking,
};

use super::priority::{DemandKind, DemandSnapshot};
use super::pyramid::{PipelineKind, neighbor_view};

/// 一个完工的区块束。Director 不打开它，只整包搬运。
///
/// 宿主用 `Box<RegionColumn>` 实现；纯状态机的测试用一个数字。
/// `is_pristine` 判断盒子内容是否还和生成器输出一样、也没有上层内容：
/// 这样的盒子不需要停放，卸载时直接丢掉，下次重新生成。
pub trait ChunkBundle: Send + std::fmt::Debug {
    fn is_pristine(&self) -> bool;
}

/// 任务的编号，单调递增，不复用。取消和完成都靠它认领。
pub type TaskId = u64;
/// 一份快照的编号，单调递增。
pub type SaveSeq = u64;
/// region 的编号。现在只有一个 region（0），类型上留着给分区器。
pub type RegionId = u32;

/// 家岛：单岛时唯一的岛，Director 发出的交付、收回、换档都带这个编号。多岛时（第 10 步）
/// 宿主不看这个编号，交付按分区器给的岛投，收回和换档按区块所在的岛分组；
/// `Delivered` 回执带回真正收下的岛，`Owner::Region` 记的就是它，拆合时宿主改记。
///
/// The home island: the only one with a single island, and the id on every
/// delivery, reclaim and level change the Director sends. With several
/// islands (step 10) the host ignores it: deliveries go to the partitioner's
/// island, reclaims and level changes are grouped by each chunk's island, the
/// `Delivered` ack carries the island that really took the chunk (what
/// `Owner::Region` records), and the host re-owns chunks on splits and merges.
pub const HOME_REGION: RegionId = 0;

/// 区块现在归谁。任何时刻一份盒子恰好在一个地方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    /// 在 Director / 执行器手里（读盘、解析、生成、组装）。
    Pipeline,
    /// `Deliver` 已发出，等 region 的 `Delivered` 回执。
    Delivering,
    /// 已交付给 region。
    Region(RegionId),
    /// `Reclaim` 已发出，等 region 在 tick 边界 `Returned`。
    Reclaiming,
    /// 已从 region 收回，快照在途。
    Saving,
    /// 没票了但还有在途任务，等它们收尾后删记录。
    Draining,
}

/// 想要的等级。`target` 是 `generation_status(loading_level)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desired {
    pub loading_level: i32,
    pub target: Option<ChunkStatus>,
    pub simulation_level: i32,
}

impl Desired {
    pub fn new(loading_level: i32, simulation_level: i32) -> Self {
        Self {
            loading_level,
            target: generation_status(loading_level),
            simulation_level,
        }
    }

    pub const UNLOADED: Self = Self {
        loading_level: crate::ticket::UNLOADED_LEVEL,
        target: None,
        simulation_level: crate::ticket::SIMULATION_MAX_LEVEL,
    };
}

/// 一条等级变化被归到哪一类。只有非 `Noop` 的才需要动区块表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelClassification {
    Noop,
    Load,
    Unload,
    Promote,
    Demote,
    LevelOnly,
}

/// 最近一次 `run_updates` 里的一条变化，连同 `classify` 的结果。
/// 分区器用它判断「票想要加载的区块」集合怎么变（含 `Promote` / `Demote`）。
///
/// One level change together with its classification, for the island
/// partitioner to update the set of ticket-desired chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedLevelChange {
    pub change: LevelChange,
    pub class: LevelClassification,
}

/// `Owner` 是不是「盒子还没收回」的三类之一。
///
/// Whether an owner still holds a delivered (or in-flight) chunk box.
pub fn owner_is_held(owner: Owner) -> bool {
    matches!(
        owner,
        Owner::Delivering | Owner::Region(_) | Owner::Reclaiming
    )
}

/// 一条记录的归属，给分区器补全「还没收回的区块」用。
///
/// A chunk record's owner, used by the island partitioner to cover chunks
/// whose boxes have not been reclaimed yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerRecord {
    pub pos: ChunkPos,
    pub owner: Owner,
}

/// 一个等级属于 31 / 32 / 33 划分的哪一档。34 以上（含无票）统一算第四档。
fn tick_tier(level: i32) -> u8 {
    // 33 是完工但不 tick，32 是方块 tick，31 及以内是实体 tick。
    if level > 33 {
        3
    } else if is_entity_ticking(level) {
        0
    } else if is_block_ticking(level) {
        1
    } else {
        2
    }
}

/// 任一套等级有没有跨过 31 / 32 / 33 的 tick 边界。
fn tick_changed(change: LevelChange) -> bool {
    let (old_level, new_level) = change.loading;
    let (old_sim, new_sim) = change.simulation;
    tick_tier(old_level) != tick_tier(new_level) || tick_tier(old_sim) != tick_tier(new_sim)
}

/// `LevelChange` 的过滤：把 34..44 的等级量化到四个生成阶段档，0..=33 是第五档。
///
/// 加载和模拟两套等级都要看：任一套跨过 31/32/33 边界就要通知 region，
/// 否则「按等级决定谁 tick」收不到模拟等级自己的变化。视距 10 时玩家跨一格有约
/// 一千条变化，绝大多数不跨边界；`Noop` 不产生任务或消息，处理量只和真正要动作的
/// 区块数成正比。
pub fn classify(change: LevelChange) -> LevelClassification {
    let (old_level, new_level) = change.loading;
    let old = generation_status(old_level);
    let new = generation_status(new_level);
    if new.is_none() {
        LevelClassification::Unload
    } else if old.is_none() {
        LevelClassification::Load
    } else if old_level <= 33 && new_level <= 33 {
        // 两边都是完工区块：只可能换 tick 级别，不重跑流水线。
        if tick_changed(change) {
            LevelClassification::LevelOnly
        } else {
            LevelClassification::Noop
        }
    } else if new_level > old_level {
        LevelClassification::Demote
    } else if new_level < old_level && old != new {
        LevelClassification::Promote
    } else {
        LevelClassification::Noop
    }
}

/// 票的增减。`Director` 自己把它喂给 `TicketTable`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketOp {
    PlayerMoved {
        id: u64,
        to: Option<ChunkPos>,
    },
    ViewDistance(i32),
    SimulationDistance(i32),
    Add {
        ty: TicketType,
        pos: ChunkPos,
        level: i32,
    },
    Remove {
        ty: TicketType,
        pos: ChunkPos,
        level: i32,
    },
    Forced {
        pos: ChunkPos,
        forced: bool,
    },
    WithRadius {
        ty: TicketType,
        pos: ChunkPos,
        radius: i32,
        add: bool,
    },
}

/// 输入事件。
#[derive(Debug)]
pub enum Event<C> {
    /// 一批票的操作，Director 处理完后自己 `run_updates()`。
    Tickets { ops: Vec<TicketOp> },
    /// 后台任务完成。`generation` 不匹配时状态不变，结果里的盒子回停放表。
    TaskDone {
        id: TaskId,
        pos: ChunkPos,
        generation: u64,
        result: TaskResult<C>,
    },
    /// region 在 tick 边界交还了区块束。`None` 表示该位置本来就没有盒子。
    Returned {
        pos: ChunkPos,
        generation: u64,
        column: Option<C>,
    },
    /// region 确认收到交付。`region` 是收下它的岛。
    ///
    /// The island `region` confirms it took the delivery.
    Delivered {
        region: RegionId,
        pos: ChunkPos,
        generation: u64,
    },
    /// 拿着这些区块的岛没了（它的 tick 线程卡死、被判死，盒子拿不回来；岛的实施计划
    /// 5.6.3）。归属还在 region 一侧（`Delivering` / `Region` / `Reclaiming`）的记录照
    /// 「收回但没有盒子」处理：删掉记录，票还想要它就当场按新加载重建、重新读盘。
    /// 别的状态的记录不动。
    ///
    /// The island holding these chunks is gone (its tick thread is stuck and
    /// it was declared dead, so the boxes cannot come back; island plan
    /// 5.6.3). Records still owned on the region side (`Delivering`, `Region`,
    /// `Reclaiming`) are treated as returned without a box: the record is
    /// removed and, if tickets still want the chunk, recreated as a fresh
    /// load that reads from disk again. Records in other states are left alone.
    Lost { positions: Vec<ChunkPos> },
    /// 每刻推进：超时票递减、收回延迟、限速补充。
    Tick { now_tick: u64 },
    /// 关服：收回全部区块、写完所有快照。
    Shutdown,
}

/// 任务结果。`Save` 完成时把盒子**随结果交还**，票若已回来可直接重新交付，
/// 不用再走一遍读盘/解码。
#[derive(Debug)]
pub enum TaskResult<C> {
    Read {
        bytes: Option<Vec<u8>>,
    },
    /// 从盘上读回的盒子。`status` 是盘上 `Status` 对应的阶段：
    /// `full` 直接交付，其余从该阶段接着跑生成流水线。
    ///
    /// A box read from disk. `status` is its on-disk stage: `full` is delivered
    /// directly; earlier stages resume generation from there.
    Decoded {
        column: C,
        status: ChunkStatus,
    },
    Generated {
        stage: ChunkStatus,
        column: C,
    },
    Assembled {
        column: C,
    },
    BadChunk {
        error: String,
    },
    /// `column` 是原盒子；没票时 Director 发 `Effect::Park` 让宿主停放。
    Saved {
        seq: SaveSeq,
        column: C,
    },
    SaveFailed {
        seq: SaveSeq,
        error: String,
    },
    Cancelled,
}

/// 派给执行器的任务。全部是 owned 数据，可以搬到计算池和 IO 线程。
#[derive(Debug)]
pub enum Task<C> {
    /// 从停放表/磁盘取盒子。`bytes` 是宿主解释的不透明数据；
    /// `Read` 的结果 `None` 表示没有这个区块，要走生成流水线。
    Read {
        pos: ChunkPos,
    },
    Decode {
        pos: ChunkPos,
        bytes: Vec<u8>,
    },
    /// `column` 是中心的半成品，`None` 表示执行器按模板造一个。
    /// `neighbors` 是这一步需要的邻居只读视图：`(位置, 邻居至少到了的阶段)`，
    /// 半径和阶段由 `pyramid::neighbor_view` 给出。宿主按位置把不可变快照
    /// （`Arc`）挂到计算任务上，Director 本身不碰盒子数据。
    ///
    /// `column` is the center's half-built box (`None` means build from a
    /// template). `neighbors` lists the read-only neighbor view as
    /// `(pos, minimum stage)`, from `pyramid::neighbor_view`; the host attaches
    /// the immutable `Arc` snapshots to the compute job.
    Generate {
        pos: ChunkPos,
        stage: ChunkStatus,
        column: Option<C>,
        neighbors: Vec<(ChunkPos, ChunkStatus)>,
        write_radius: i32,
    },
    Assemble {
        pos: ChunkPos,
        column: C,
    },
    /// 序列化 + 停放。完成时把盒子原样交还。
    Save {
        pos: ChunkPos,
        seq: SaveSeq,
        column: C,
    },
}

impl<C> Task<C> {
    /// 这次任务会写到离中心多远的区块。写范围不相交的任务可以同时跑。
    pub fn write_radius(&self) -> i32 {
        match self {
            Self::Generate { write_radius, .. } => *write_radius,
            _ => 0,
        }
    }

    pub fn pos(&self) -> ChunkPos {
        match self {
            Self::Read { pos }
            | Self::Decode { pos, .. }
            | Self::Generate { pos, .. }
            | Self::Assemble { pos, .. }
            | Self::Save { pos, .. } => *pos,
        }
    }
}

/// 发给 region 的信箱消息。交付、收回、换档带目标岛的编号，宿主按它投进那个岛的信箱。
///
/// Mailbox messages for a region. Deliveries, reclaims and level changes carry
/// the target island; the host posts them into that island's mailbox.
#[derive(Debug)]
pub enum RegionMessage<C> {
    Deliver {
        region: RegionId,
        pos: ChunkPos,
        generation: u64,
        column: C,
        desired: Desired,
    },
    /// 请 region 在 tick 边界交回这些区块，带各自的 generation。
    Reclaim {
        region: RegionId,
        positions: Vec<(ChunkPos, u64)>,
    },
    /// 只在 `Full` 档内换了 tick 级别，盒子不用搬。
    LevelChanged {
        region: RegionId,
        changes: Vec<LevelChange>,
    },
    /// 已交付、可以进入玩家待发送集合的区块。
    Ready { positions: Vec<ChunkPos> },
    /// 区块不再被跟踪。
    Forgot { positions: Vec<ChunkPos> },
}

/// 一次 `handle` 产生的效果。宿主按顺序执行，不重排。
#[derive(Debug)]
pub enum Effect<C> {
    Submit {
        id: TaskId,
        /// 派发时这条记录的代数，宿主回 `TaskDone` 时原样带回来。
        generation: u64,
        task: Task<C>,
    },
    Cancel {
        id: TaskId,
    },
    ToRegion(RegionMessage<C>),
    /// 没票的快照落地：宿主把整个盒子放进停放表（不经过 NBT 编码）。
    /// `is_pristine` 的盒子不会走到这里。
    Park {
        pos: ChunkPos,
        column: C,
    },
    /// 设计稿 5.5：同一区块有新快照时丢掉还没写完的旧快照。
    DropSnapshot {
        pos: ChunkPos,
        seq: SaveSeq,
    },
    Warn {
        message: String,
    },
}

/// 可调参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectorConfig {
    /// 区块失去最后一张票后在 region 里逗留多少刻。票回来就什么都不用做。
    ///
    /// 逗留的区块等级已是「未加载」，不参与任何 tick；会 tick 的区块周围一圈
    /// 一定有票（`player_loading` 等级 31，向外一圈 32 才是方块 tick 边界），
    /// 所以方块逻辑碰不到这些逗留的区块。缺省 100 刻。
    pub reclaim_delay_ticks: u64,
    /// 流水线形状。`Flat` 是现在唯一的生成器。
    pub pipeline: PipelineKind,
    /// 每刻最多新派多少个任务。0 表示不限。
    pub max_submits_per_tick: usize,
    /// 同时在途的任务总数上限。`usize::MAX` 表示不限。
    pub max_in_flight: usize,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            reclaim_delay_ticks: 100,
            pipeline: PipelineKind::Flat,
            max_submits_per_tick: 0,
            max_in_flight: usize::MAX,
        }
    }
}

/// 每个区块一条记录。
#[derive(Debug)]
struct ChunkRecord {
    desired: Desired,
    owner: Owner,
    /// 已经到的阶段。只有 `Owner::Pipeline` 时有意义。
    stage: ChunkStatus,
    /// 至多一个在途任务。
    task: Option<TaskId>,
    /// 每次交付 / 收回 / 重新加载 +1，用来丢弃过期的完成消息。
    generation: u64,
    /// 最后一次排存盘的快照序号；`Saved` 的 seq 小于它就忽略。
    snapshot: Option<SaveSeq>,
    /// 失去最后一张票的时刻，收回延迟用。`None` 表示还有票。
    unloaded_at: Option<u64>,
    /// 这条记录只是别人的生成依赖，不进 region、不发给玩家。
    dependency: bool,
}

/// 一个还没派出去的任务（写范围冲突或配额用完）。
#[derive(Debug)]
struct PendingTask<C> {
    pos: ChunkPos,
    generation: u64,
    seq: Option<SaveSeq>,
    task: Task<C>,
}

/// 一个在途任务。
#[derive(Debug)]
struct InFlight {
    pos: ChunkPos,
    generation: u64,
    write_radius: i32,
}

/// 逗留到期的队列项。`BinaryHeap` 按 `at` 大顶堆，用 `Reverse` 变成小顶堆：
/// 最早失去票的位置先出来。同刻的按坐标排，输出的效果序列仍然确定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReclaimDue {
    /// `Reverse` 让最早到期的最先弹出。
    at: std::cmp::Reverse<u64>,
    pos: ChunkPos,
}

/// 一次 `drive` 最多检查多少个被挡住的任务；其余留在堆里等下一次。
const MAX_SKIPS_PER_DRIVE: usize = 16;

/// 待派任务的排序键：离最近玩家的 rank（带方向加权），同一档按
/// 「这条流水线已经走到哪一步」把快完工的往前放，最后按 TaskId 兜底。
/// 键只由 `(需求快照, 位置, 阶段, 编号)` 决定；任务完成、配额变化都不改它。
type QueueKey = (i64, u8, TaskId);

/// 堆里的一项：`(排序键, 任务编号)`。TaskId 已在键里作最后的兜底。
type QueueEntry = (QueueKey, TaskId);

/// 区块平面的 Director。见模块文档。
#[derive(Debug)]
pub struct Director<C> {
    tickets: TicketTable,
    chunks: BTreeMap<ChunkPos, ChunkRecord>,
    in_flight: BTreeMap<TaskId, InFlight>,
    /// 等着派出去的任务，按 TaskId 排序。
    pending: BTreeMap<TaskId, PendingTask<C>>,
    config: DirectorConfig,
    next_task: TaskId,
    next_seq: SaveSeq,
    now_tick: u64,
    /// 待排队的 `Ready` / `Forgot`，按坐标去重。
    pending_ready: BTreeSet<ChunkPos>,
    pending_forgot: BTreeSet<ChunkPos>,
    /// 等着排 `Read` 的记录。只有刚进 `Full` 档、还没派任务的记录才在里面，
    /// 每次 `drive` 只取这一批，不扫整张记录表。
    queued_reads: BTreeSet<ChunkPos>,
    /// 全部待派任务的小顶堆（`Reverse` 包大顶堆）。只在
    /// 新任务加入、玩家快照变化或者有任务被挡住后重试时才碰它。
    pending_order: BinaryHeap<Reverse<QueueEntry>>,
    /// 需求快照的代数；变了之后堆里的键整堆重算一次。
    demand_epoch: u64,
    /// 堆里当前键用的是哪一代需求。
    heap_epoch: u64,
    /// 逗留到期的队列：按 `unloaded_at` 排，队头不到期就不用再往后看。
    /// 同一个位置可能有旧条目（`unloaded_at` 改过），取出时和记录里的值对一下。
    reclaim_due: BinaryHeap<ReclaimDue>,
    /// 本刻已经派出去的任务数。
    submitted_this_tick: usize,
    /// 当前玩家需求和每玩家在途配额。宿主每刻更新，纯状态机测试默认是空快照。
    demand: DemandSnapshot,
    /// 每名玩家当前在途的「加载存档」和「新生成」任务数，按 `(玩家编号, 类别)` 记。
    demand_in_flight: HashMap<(u64, DemandKind), usize>,
    /// 每个在途任务归到哪名玩家、属于哪一类，`on_task_done` 收回时减计数。
    task_owners: HashMap<TaskId, (Option<u64>, DemandKind)>,
    /// 超时票的 `is_busy` 直接查这张表，不每刻重建。
    busy: BTreeSet<ChunkPos>,
    /// 已交付或正在交付的记录数，随 `Owner` 变化增量维护。
    loaded: usize,
    /// 统计：过滤掉的等级变化 / 真正动作的区块。
    filtered: u64,
    acted: u64,
    shutting_down: bool,
    /// 每次 `handle` 复用的效果列表容量。
    effects: Vec<Effect<C>>,
    /// 一次等级变化里「跨了 tick 档」的位置，跨次复用容量。
    level_changed: Vec<LevelChange>,
    /// 票表变化列表的只读副本，跨次复用容量。
    changes_scratch: Vec<LevelChange>,
    /// 本刻带着分类的等级变化，供宿主在同一次 pump 里读（分区器用）。
    classified_changes: Vec<ClassifiedLevelChange>,
    /// 新排的读任务，跨次 `drive` 复用容量。
    reads: Vec<(ChunkPos, u64)>,
    /// 构造一个「生成器输出」的盒子。宿主设置；测试可以用 `C::default`。
    template: Option<fn() -> C>,
}

impl<C: ChunkBundle> Default for Director<C> {
    fn default() -> Self {
        Self::new(DirectorConfig::default())
    }
}

impl<C: ChunkBundle> Director<C> {
    pub fn new(config: DirectorConfig) -> Self {
        Self {
            tickets: TicketTable::new(),
            chunks: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            pending: BTreeMap::new(),
            config,
            next_task: 1,
            next_seq: 1,
            now_tick: 0,
            pending_ready: BTreeSet::new(),
            pending_forgot: BTreeSet::new(),
            queued_reads: BTreeSet::new(),
            pending_order: BinaryHeap::new(),
            demand_epoch: 0,
            heap_epoch: 0,
            reclaim_due: BinaryHeap::new(),
            submitted_this_tick: 0,
            demand: DemandSnapshot::default(),
            demand_in_flight: HashMap::new(),
            task_owners: HashMap::new(),
            busy: BTreeSet::new(),
            loaded: 0,
            filtered: 0,
            acted: 0,
            shutting_down: false,
            effects: Vec::new(),
            level_changed: Vec::new(),
            changes_scratch: Vec::new(),
            classified_changes: Vec::new(),
            reads: Vec::new(),
            template: None,
        }
    }

    pub fn tickets(&self) -> &TicketTable {
        &self.tickets
    }

    /// 更新玩家加载需求的快照。玩家跨格才该调。
    ///
    /// 换快照只推一次代数并标脏；下一次真正要派任务时才整堆重算一次，
    /// 不是每刻、更不是每个事件重排。在途任务仍记在原来的玩家名下。
    pub fn set_player_demand(&mut self, demand: DemandSnapshot) {
        if self.demand == demand {
            return;
        }
        self.demand = demand;
        self.demand_epoch = self.demand_epoch.wrapping_add(1);
    }

    /// 当前的需求快照，宿主的读盘、计算池和交付队列用同一个 rank 排序。
    pub fn player_demand(&self) -> &DemandSnapshot {
        &self.demand
    }

    /// 每刻只重置派出配额。任务一直留在优先级堆里，不需要整表重排。
    pub fn begin_tick(&mut self) {
        self.submitted_this_tick = 0;
    }

    pub fn tickets_mut(&mut self) -> &mut TicketTable {
        &mut self.tickets
    }

    pub fn config(&self) -> &DirectorConfig {
        &self.config
    }

    /// 测试里改流水线配置（例如把收回逗留调短，好在几十刻内看收回）。
    pub fn config_mut(&mut self) -> &mut DirectorConfig {
        &mut self.config
    }

    /// 统计用的计数：(过滤掉的等级变化, 真正动作的区块)。
    pub fn counters(&self) -> (u64, u64) {
        (self.filtered, self.acted)
    }

    /// 最近一次 `run_updates` 的全部等级变化及分类，按 (x, z) 升序。
    /// 宿主每次 `handle_effects` 后取走；下一次调用会清空这份列表。
    ///
    /// All level changes from the latest `run_updates`, with their
    /// classification, sorted by (x, z). Consumed by the host after each
    /// `handle_effects`; the next call clears this list.
    pub fn classified_level_changes(&self) -> &[ClassifiedLevelChange] {
        &self.classified_changes
    }

    /// 一条记录当前的归属；没有记录时是 `None`。宿主在回执事件后用它
    /// 做增量的 owner 记账，过期回执不会凭空造出「还没收回」的区块。
    ///
    /// Current owner of one record, or `None`. The host uses it after an
    /// ack so a stale ack cannot invent a held chunk.
    pub fn owner_at(&self, pos: ChunkPos) -> Option<Owner> {
        self.chunks.get(&pos).map(|record| record.owner)
    }

    /// 所有记录及其归属，按 (x, z) 升序。对照模式拿它做全量对账，
    /// 正式路径走增量的 owner 事件。
    ///
    /// Every chunk record and its owner, sorted by (x, z). Used by the
    /// reference-check mode; the live path is incremental.
    pub fn owner_records(&self) -> Vec<OwnerRecord> {
        self.chunks
            .iter()
            .map(|(&pos, record)| OwnerRecord {
                pos,
                owner: record.owner,
            })
            .collect()
    }

    /// 岛拆合以后，已交付区块改记到新的岛名下（岛的实施计划第 10 步）。只改
    /// `Owner::Region` 的编号，别的状态都不动；不是 `Region` 的记录不变。返回改了没有。
    ///
    /// After an island split or merge, re-own a delivered chunk by its new
    /// island (island plan, step 10). Only the id of `Owner::Region` changes;
    /// records in any other state are left alone. Returns whether it changed.
    pub fn reassign_region(&mut self, pos: ChunkPos, region: RegionId) -> bool {
        match self.chunks.get_mut(&pos) {
            Some(record) if matches!(record.owner, Owner::Region(_)) => {
                record.owner = Owner::Region(region);
                true
            }
            _ => false,
        }
    }

    /// 从和卸载存档同一个计数器里领一个写盘序号。运行中给「仍加载的脏区块」
    /// 拍快照也用这个序号，交给 IO 线程后按 seq 大小丢掉同区块的旧快照，
    /// 和卸载写共用一条单调递增的序列，不会互相盖掉。
    ///
    /// Allocate a save seq from the same counter as unload saves. Paced snapshots
    /// of still-loaded dirty chunks use it too, so the IO thread's per-chunk
    /// "keep the newest seq" rule shares one monotonic sequence across both paths.
    pub fn allocate_save_seq(&mut self) -> SaveSeq {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    pub fn now_tick(&self) -> u64 {
        self.now_tick
    }

    /// 还有没有在途任务或待派任务。关服时用它判断能不能收尾。
    pub fn is_settled(&self) -> bool {
        self.in_flight.is_empty()
            && self.pending.is_empty()
            && self.loaded == 0
            && self.busy.is_empty()
    }

    /// 已交付或正在交付的区块数。
    pub fn loaded_count(&self) -> usize {
        self.loaded
    }

    /// 供随机测试逐步断言不变量。
    pub fn debug_state(&self) -> DebugState<'_, C> {
        DebugState { director: self }
    }

    /// 改一条记录的归属，顺便维护 `loaded` 计数和超时票用的忙集合。
    /// 所有写 `owner` 的地方都走这里。
    fn set_owner(&mut self, pos: ChunkPos, owner: Owner) {
        let Some(record) = self.chunks.get_mut(&pos) else {
            return;
        };
        let was_loaded = matches!(
            record.owner,
            Owner::Delivering | Owner::Region(_) | Owner::Reclaiming
        );
        let is_loaded = matches!(
            owner,
            Owner::Delivering | Owner::Region(_) | Owner::Reclaiming
        );
        record.owner = owner;
        if was_loaded && !is_loaded {
            self.loaded -= 1;
        } else if !was_loaded && is_loaded {
            self.loaded += 1;
        }
        self.refresh_busy(pos);
    }

    /// 一条记录此刻是否「还没准备好存盘」。`TicketTable::tick` 每刻只问有超时票的位置。
    fn is_busy_record(record: &ChunkRecord) -> bool {
        record.task.is_some()
            || matches!(
                record.owner,
                Owner::Pipeline
                    | Owner::Delivering
                    | Owner::Saving
                    | Owner::Reclaiming
                    | Owner::Draining
            )
    }

    /// 记录状态变了之后重算这条记录的忙标记。
    fn refresh_busy(&mut self, pos: ChunkPos) {
        let busy = self.chunks.get(&pos).is_some_and(Self::is_busy_record);
        if busy {
            self.busy.insert(pos);
        } else {
            self.busy.remove(&pos);
        }
    }

    /// 改失去最后一张票的时刻，同时按到期时刻排进队列；票回来时把它撤掉。
    fn set_unloaded_at(&mut self, pos: ChunkPos, at: Option<u64>) {
        if let Some(record) = self.chunks.get_mut(&pos) {
            record.unloaded_at = at;
        }
        if let Some(at) = at {
            self.reclaim_due.push(ReclaimDue {
                at: std::cmp::Reverse(at),
                pos,
            });
        }
    }

    /// 删一条记录，把它的增量索引一起清掉。
    fn remove_record(&mut self, pos: ChunkPos) {
        if let Some(record) = self.chunks.remove(&pos)
            && matches!(
                record.owner,
                Owner::Delivering | Owner::Region(_) | Owner::Reclaiming
            )
        {
            self.loaded -= 1;
        }
        self.busy.remove(&pos);
        self.queued_reads.remove(&pos);
        self.pending_forgot.insert(pos);
    }

    /// 一个没人要的盒子：不是原样就回停放表，原样的才可以丢。
    ///
    /// 从停放表/磁盘读出来的盒子一旦被判为过期、被取消或者票已经撤了，
    /// 宿主那边已经没有它了，丢掉就等于丢玩家改动。
    fn park_orphan(out: &mut Vec<Effect<C>>, pos: ChunkPos, column: C) {
        if !column.is_pristine() {
            out.push(Effect::Park { pos, column });
        }
    }

    /// 把结果里带着的盒子取出来，交给 `park_orphan`。
    fn park_orphan_result(out: &mut Vec<Effect<C>>, pos: ChunkPos, result: TaskResult<C>) {
        let column = match result {
            TaskResult::Decoded { column, .. }
            | TaskResult::Generated { column, .. }
            | TaskResult::Assembled { column }
            | TaskResult::Saved { column, .. } => Some(column),
            TaskResult::Read { .. }
            | TaskResult::BadChunk { .. }
            | TaskResult::SaveFailed { .. }
            | TaskResult::Cancelled => None,
        };
        if let Some(column) = column {
            Self::park_orphan(out, pos, column);
        }
    }

    /// 处理一个事件，返回它产生的效果。测试和不复用容量的调用方走这里。
    pub fn handle(&mut self, event: Event<C>) -> Vec<Effect<C>> {
        let mut out = Vec::new();
        self.handle_effects(event, |effects| std::mem::swap(&mut out, effects));
        out
    }

    /// 处理一个事件，把效果列表交给 `emit`。
    ///
    /// 效果列表在 Director 里复用：`emit` 返回后清空，下一次 `handle_effects`
    /// 用同一块容量。宿主必须在 `emit` 里把效果消费完或换走。
    pub fn handle_effects(&mut self, event: Event<C>, emit: impl FnOnce(&mut Vec<Effect<C>>)) {
        self.classified_changes.clear();
        let mut out = std::mem::take(&mut self.effects);
        out.clear();
        match event {
            Event::Tickets { ops } => {
                for op in ops {
                    self.apply_ticket(op);
                }
                self.consume_level_changes(&mut out);
            }
            Event::TaskDone {
                id,
                pos,
                generation,
                result,
            } => {
                self.on_task_done(&mut out, id, pos, generation, result);
            }
            Event::Returned {
                pos,
                generation,
                column,
            } => self.on_returned(&mut out, pos, generation, column),
            Event::Delivered {
                region,
                pos,
                generation,
            } => self.on_delivered(region, pos, generation),
            Event::Lost { positions } => self.on_lost(&mut out, positions),
            Event::Tick { now_tick } => {
                self.now_tick = now_tick;
                self.begin_tick();
                self.tick_timeouts();
                self.consume_level_changes(&mut out);
                self.expire_reclaim_delay(&mut out);
            }
            Event::Shutdown => {
                self.shutting_down = true;
                self.reclaim_all(&mut out);
            }
        }
        self.drive(&mut out);
        emit(&mut out);
        out.clear();
        self.effects = out;
    }

    // ---- 票 ----

    fn apply_ticket(&mut self, op: TicketOp) {
        match op {
            TicketOp::PlayerMoved { id, to } => self.tickets.set_player(id, to),
            TicketOp::ViewDistance(v) => self.tickets.set_view_distance(v),
            TicketOp::SimulationDistance(d) => self.tickets.set_simulation_distance(d),
            TicketOp::Add { ty, pos, level } => self.tickets.add(ty, pos, level),
            TicketOp::Remove { ty, pos, level } => self.tickets.remove(ty, pos, level),
            TicketOp::Forced { pos, forced } => self.tickets.set_forced(pos, forced),
            TicketOp::WithRadius {
                ty,
                pos,
                radius,
                add,
            } => {
                if add {
                    self.tickets.add_with_radius(ty, pos, radius);
                } else {
                    self.tickets.remove_with_radius(ty, pos, radius);
                }
            }
        }
    }

    /// 把 `run_updates()` 的输出量化，只有真正要动作的区块才碰 `chunks`。
    fn consume_level_changes(&mut self, out: &mut Vec<Effect<C>>) {
        // `run_updates` 返回的列表已经按 (x, z) 排好；这里按只读切片遍历，
        // 不再克隆一份，也不再排一次。
        let mut level_changed = std::mem::take(&mut self.level_changed);
        level_changed.clear();
        let mut changes = std::mem::take(&mut self.changes_scratch);
        changes.clear();
        let mut classified = std::mem::take(&mut self.classified_changes);
        classified.clear();
        classified.extend(
            self.tickets
                .run_updates()
                .iter()
                .map(|&change| ClassifiedLevelChange {
                    change,
                    class: classify(change),
                }),
        );
        changes.extend(classified.iter().map(|entry| entry.change));
        for &ClassifiedLevelChange { change, class } in &classified {
            // 任一套等级跨了 tick 档，都要告诉 region：交付中的区块撤票时会从 31
            // 一步跳到 34 以上，归到 Unload/Demote 而不是 LevelOnly，但 region 里
            // 那份档位照样要降下来（它还会等 100 刻才被收回）。没交付的位置也报，
            // region 只记下档位，等 Deliver 时用最新值。
            if tick_changed(change) {
                level_changed.push(change);
            }
            // 只碰报出来的位置：半成生成记录的等级变化时同样在 `run_updates()`
            // 的列表里，用变化里的两个新分量直接更新，不必再查两次票表。
            self.refresh_desired(change);
            match class {
                LevelClassification::Noop => self.filtered += 1,
                LevelClassification::LevelOnly => self.acted += 1,
                class => {
                    self.acted += 1;
                    self.apply_level_change(out, change, class);
                }
            }
        }
        if !level_changed.is_empty() {
            let changes = std::mem::take(&mut level_changed);
            out.push(Effect::ToRegion(RegionMessage::LevelChanged {
                region: HOME_REGION,
                changes,
            }));
        }
        self.level_changed = level_changed;
        self.changes_scratch = changes;
        self.classified_changes = classified;
    }

    /// 一条报出来的等级变化：把记录里的两个等级数字同步到票表的新值，
    /// 并维护失去票的时刻。依赖记录也在这里更新，但它们不因没票而排队收回。
    fn refresh_desired(&mut self, change: LevelChange) {
        let pos = change.pos;
        let desired = Desired::new(change.loading.1, change.simulation.1);
        let Some(record) = self.chunks.get_mut(&pos) else {
            return;
        };
        record.desired = desired;
        let dependency = record.dependency;
        let unloaded = record.unloaded_at.is_some();
        if !dependency && desired.target.is_none() && !unloaded {
            self.set_unloaded_at(pos, Some(self.now_tick));
        } else if !dependency && desired.target.is_some() && unloaded {
            self.set_unloaded_at(pos, None);
        }
        if dependency && desired.target.is_none() && !unloaded {
            // 依赖记录随流水线的依赖存在，由 `expire_reclaim_delay`
            // 在没票且没有在途任务时删掉，不进 region 的收回路径。
            self.set_unloaded_at(pos, Some(self.now_tick));
        }
    }

    fn apply_level_change(
        &mut self,
        out: &mut Vec<Effect<C>>,
        change: LevelChange,
        class: LevelClassification,
    ) {
        let pos = change.pos;
        let desired = Desired::new(change.loading.1, change.simulation.1);
        match class {
            LevelClassification::Load => {
                // 等级 34..44 只是生成依赖带，不读盘也不交付。
                let dependency = desired.target != Some(ChunkStatus::Full);
                let record = self.chunks.entry(pos).or_insert_with(|| ChunkRecord {
                    desired,
                    owner: Owner::Pipeline,
                    stage: ChunkStatus::Empty,
                    task: None,
                    generation: 1,
                    snapshot: None,
                    unloaded_at: None,
                    dependency,
                });
                record.desired = desired;
                // 已经存在的依赖记录（由 `ensure_dependencies` 按所需阶段建的）
                // 不能退化回 `Empty`，否则生成阶段会以为邻居还没到那个阶段。
                if !(dependency && record.dependency) {
                    record.dependency = dependency;
                }
                let was_draining = record.owner == Owner::Draining;
                let had_unloaded_at = record.unloaded_at.is_some();
                if had_unloaded_at {
                    self.set_unloaded_at(pos, None);
                }
                if was_draining {
                    self.set_owner(pos, Owner::Pipeline);
                }
                self.queue_read_if_needed(pos);
            }
            LevelClassification::Unload => {
                if !self.chunks.contains_key(&pos) {
                    return;
                }
                // 在途的读盘/生成任务直接取消；回来的结果靠
                // `on_task_done` 把非原样的盒子送回停放表。
                let pipeline = self.chunks[&pos].owner == Owner::Pipeline;
                if pipeline {
                    self.cancel_task(out, pos);
                    self.set_owner(pos, Owner::Draining);
                }
            }
            LevelClassification::Promote | LevelClassification::Demote => {
                let Some(record) = self.chunks.get_mut(&pos) else {
                    return;
                };
                if desired.target == Some(ChunkStatus::Full) {
                    // 进了 `Full` 档：这条记录现在要真正加载和交付。
                    record.dependency = false;
                } else if matches!(record.owner, Owner::Pipeline) {
                    // 退回半成品档：不应继续读盘/生成。
                    record.dependency = true;
                }
                self.queue_read_if_needed(pos);
            }
            LevelClassification::LevelOnly | LevelClassification::Noop => {}
        }
    }

    /// 一条记录刚刚可以开始加载流水线时排进 `Read` 队列。
    fn queue_read_if_needed(&mut self, pos: ChunkPos) {
        let Some(record) = self.chunks.get(&pos) else {
            return;
        };
        if record.owner == Owner::Pipeline
            && record.task.is_none()
            && !record.dependency
            && record.desired.target == Some(ChunkStatus::Full)
        {
            self.queued_reads.insert(pos);
        }
    }

    fn cancel_task(&mut self, out: &mut Vec<Effect<C>>, pos: ChunkPos) {
        let Some(record) = self.chunks.get_mut(&pos) else {
            return;
        };
        if let Some(id) = record.task.take() {
            if self.in_flight.remove(&id).is_some() {
                out.push(Effect::Cancel { id });
            }
            self.pending.remove(&id);
            self.release_task_owner(id);
            self.forget_priority_id(id);
        }
        self.refresh_busy(pos);
        self.queued_reads.remove(&pos);
    }

    fn tick_timeouts(&mut self) {
        self.tickets.tick(&mut |pos| self.busy.contains(&pos));
    }

    /// 逗留到期才发 `Reclaim`。票在延迟内回来了就什么都不做。
    ///
    /// 到期队列按 `unloaded_at` 排好，每刻只弹到期的那些；旧条目（票回来过、
    /// 又撤了）和记录里的时刻对不上就丢掉，没到期的留在堆里下次再看。
    fn expire_reclaim_delay(&mut self, out: &mut Vec<Effect<C>>) {
        let delay = if self.shutting_down {
            0
        } else {
            self.config.reclaim_delay_ticks
        };
        let mut reclaim = Vec::new();
        let mut drop = Vec::new();
        let mut requeue = Vec::new();
        while let Some(ReclaimDue { at, pos }) = self.reclaim_due.peek().copied() {
            let at = at.0;
            if self.now_tick.saturating_sub(at) < delay {
                break;
            }
            self.reclaim_due.pop();
            let Some(record) = self.chunks.get(&pos) else {
                continue;
            };
            let at_matches = record.unloaded_at == Some(at);
            let owner = record.owner;
            let generation = record.generation;
            let no_task = record.task.is_none();
            if !at_matches {
                // 票回来过：这个时刻作废；若又撤了票，新条目已经在队列里。
                continue;
            }
            match owner {
                Owner::Region(_) | Owner::Delivering => {
                    self.set_owner(pos, Owner::Reclaiming);
                    reclaim.push((pos, generation));
                }
                Owner::Draining | Owner::Pipeline if no_task => {
                    // 没进过 region 的区块：没有盒子要收回，直接删记录。
                    drop.push(pos);
                }
                _ => {
                    // 盒子还在流水线或存盘里：这次不能收回，等状态变了再看。
                    requeue.push(ReclaimDue {
                        at: std::cmp::Reverse(at),
                        pos,
                    });
                }
            }
        }
        // 重新按原时刻排回去，下一刻还会检查，不会永远卡住。
        self.reclaim_due.extend(requeue);
        for pos in drop {
            self.remove_record(pos);
        }
        if !reclaim.is_empty() {
            out.push(Effect::ToRegion(RegionMessage::Reclaim {
                region: HOME_REGION,
                positions: reclaim,
            }));
        }
    }

    fn reclaim_all(&mut self, out: &mut Vec<Effect<C>>) {
        let mut reclaim = Vec::new();
        let mut cancels = Vec::new();
        let mut drop = Vec::new();
        let positions: Vec<ChunkPos> = self.chunks.keys().copied().collect();
        for pos in positions {
            let Some(record) = self.chunks.get_mut(&pos) else {
                continue;
            };
            let owner = record.owner;
            let generation = record.generation;
            let id = record.task.take();
            record.desired = Desired::UNLOADED;
            self.set_unloaded_at(pos, Some(self.now_tick));
            match owner {
                Owner::Region(_) | Owner::Delivering => {
                    self.set_owner(pos, Owner::Reclaiming);
                    reclaim.push((pos, generation));
                }
                Owner::Pipeline | Owner::Draining => {
                    if let Some(id) = id {
                        cancels.push(id);
                    }
                    drop.push(pos);
                }
                _ => {}
            }
        }
        for id in cancels {
            if self.in_flight.remove(&id).is_some() {
                out.push(Effect::Cancel { id });
            }
            self.pending.remove(&id);
            self.release_task_owner(id);
            self.forget_priority_id(id);
        }
        for pos in drop {
            self.remove_record(pos);
        }
        if !reclaim.is_empty() {
            out.push(Effect::ToRegion(RegionMessage::Reclaim {
                region: HOME_REGION,
                positions: reclaim,
            }));
        }
    }

    // ---- 任务完成 ----

    fn on_task_done(
        &mut self,
        out: &mut Vec<Effect<C>>,
        id: TaskId,
        pos: ChunkPos,
        generation: u64,
        result: TaskResult<C>,
    ) {
        let Some(in_flight) = self.in_flight.remove(&id) else {
            self.release_task_owner(id);
            Self::park_orphan_result(out, pos, result);
            return;
        };
        if in_flight.pos != pos || in_flight.generation != generation {
            Self::park_orphan_result(out, pos, result);
            return;
        }
        let Some(record) = self.chunks.get_mut(&pos) else {
            Self::park_orphan_result(out, pos, result);
            return;
        };
        if record.generation != generation || record.task != Some(id) {
            Self::park_orphan_result(out, pos, result);
            return;
        }
        record.task = None;
        self.refresh_busy(pos);
        self.release_task_owner(id);
        // 不管这个任务占不占配额，都可能有被挡住的任务可以再试。
        match result {
            TaskResult::Read { bytes: Some(bytes) } => {
                self.enqueue(out, pos, generation, None, Task::Decode { pos, bytes });
            }
            TaskResult::Read { bytes: None } => self.advance_stage(out, pos, generation, None),
            TaskResult::Decoded { column, status } => {
                // 官方 `ChunkStep.apply` 只跑 `persistedStatus` 之后的阶段：
                // 把记录推进到盘上的阶段，再从下一阶段接着生成；`full` 直接交付。
                // Vanilla `ChunkStep.apply` only runs stages after the persisted
                // one: advance the record to the on-disk stage and resume there.
                if let Some(record) = self.chunks.get_mut(&pos) {
                    record.stage = status;
                }
                if status == ChunkStatus::Full {
                    self.deliver(out, pos, generation, column);
                } else {
                    self.advance_stage(out, pos, generation, Some(column));
                }
            }
            TaskResult::Assembled { column } => self.deliver(out, pos, generation, column),
            TaskResult::Generated { stage, column } => {
                if let Some(record) = self.chunks.get_mut(&pos) {
                    record.stage = stage;
                }
                self.advance_stage(out, pos, generation, Some(column));
            }
            TaskResult::BadChunk { error } => {
                out.push(Effect::Warn {
                    message: format!("bad chunk at ({}, {}): {error}", pos.x, pos.z),
                });
                self.advance_stage(out, pos, generation, None);
            }
            TaskResult::Saved { seq, column } => self.on_saved(out, pos, seq, column),
            TaskResult::SaveFailed { seq, error } => {
                out.push(Effect::Warn {
                    message: format!("saving ({}, {}) failed: {error}", pos.x, pos.z),
                });
                self.finish_save(out, pos, seq, None);
            }
            TaskResult::Cancelled => {}
        }
    }

    /// 生成流水线的下一步。没有下一阶段就组装（组装完成才交付）。
    fn advance_stage(
        &mut self,
        out: &mut Vec<Effect<C>>,
        pos: ChunkPos,
        generation: u64,
        column: Option<C>,
    ) {
        let Some(record) = self.chunks.get(&pos) else {
            return;
        };
        let stages = self.config.pipeline.stages();
        let next = stages
            .iter()
            .copied()
            .find(|stage| *stage as usize > record.stage as usize);
        let Some(stage) = next else {
            if let Some(column) = column {
                self.enqueue(out, pos, generation, None, Task::Assemble { pos, column });
            } else {
                self.deliver_from_template(out, pos, generation);
            }
            return;
        };
        let step = self.config.pipeline.step(stage);
        for &(needed, radius) in step.requires {
            self.ensure_dependencies(pos, radius, needed);
        }
        // 邻居只读视图：按 `neighbor_view` 的半径把 3×3（去掉中心）列进任务，
        // 宿主再用不可变快照填进来。和 `ensure_dependencies` 一样用饱和加减，
        // 坐标到 i32 边界也不会溢出。
        // Read-only neighbor view: list the radius-1 3x3 (minus the center) per
        // `neighbor_view`; the host fills in immutable snapshots. Saturating
        // offsets like `ensure_dependencies`.
        let mut neighbors = Vec::new();
        if let Some((radius, needed)) = neighbor_view(stage) {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    neighbors.push((
                        ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz)),
                        needed,
                    ));
                }
            }
        }
        self.enqueue(
            out,
            pos,
            generation,
            None,
            Task::Generate {
                pos,
                stage,
                column,
                neighbors,
                write_radius: step.write_radius,
            },
        );
    }

    /// `Flat` 流水线由执行器造盒子，Director 只需要一个「交付」事件。
    /// 用 `Assemble` 让执行器把模板盒子交回来，状态和读盘/生成路径一致。
    fn deliver_from_template(&mut self, out: &mut Vec<Effect<C>>, pos: ChunkPos, generation: u64) {
        // `column: None` 的 `Generate` 是让执行器从模板造盒子的那一步；
        // 流水线结束时已经没有下一阶段，这里补一次组装。
        self.enqueue(
            out,
            pos,
            generation,
            None,
            Task::Assemble {
                pos,
                column: self.make_template(),
            },
        );
    }

    /// 由宿主实现的模板构造。纯状态机测试通过 `C: Default` 得到；
    /// 真实宿主用 `ChunkBundle::from_template`，见 `Director::with_template`。
    fn make_template(&self) -> C {
        self.template.as_ref().expect("宿主没有提供模板盒子")()
    }

    fn ensure_dependencies(&mut self, pos: ChunkPos, radius: i32, needed: ChunkStatus) {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let neighbor = ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz));
                let desired = Desired::new(
                    self.tickets.loading_level(neighbor),
                    self.tickets.simulation_level(neighbor),
                );
                let entry = self.chunks.entry(neighbor).or_insert_with(|| ChunkRecord {
                    desired,
                    owner: Owner::Pipeline,
                    stage: needed,
                    task: None,
                    generation: 1,
                    snapshot: None,
                    unloaded_at: None,
                    dependency: true,
                });
                // 同一个位置可能被多个阶段以不同要求依赖：取最靠后的阶段，
                // 后建的记录不能把已经满足的阶段退回去了。
                if (entry.stage as usize) < (needed as usize) {
                    entry.stage = needed;
                }
                entry.dependency = true;
                entry.desired = desired;
                let had_unloaded_at = entry.unloaded_at.is_some();
                if had_unloaded_at {
                    self.set_unloaded_at(neighbor, None);
                }
                // 依赖记录随票走：等级退到「无票」时由 `consume_level_changes`
                // 设 `unloaded_at`，到期在 `expire_reclaim_delay` 里直接删掉。
            }
        }
    }

    fn deliver(&mut self, out: &mut Vec<Effect<C>>, pos: ChunkPos, generation: u64, column: C) {
        let Some(record) = self.chunks.get(&pos) else {
            Self::park_orphan(out, pos, column);
            return;
        };
        if record.desired.target.is_none() {
            // 交付途中票撤了：不进 region，非原样的盒子回停放表。
            Self::park_orphan(out, pos, column);
            return;
        }
        let desired = record.desired;
        self.set_owner(pos, Owner::Delivering);
        self.set_unloaded_at(pos, None);
        out.push(Effect::ToRegion(RegionMessage::Deliver {
            region: HOME_REGION,
            pos,
            generation,
            column,
            desired,
        }));
    }

    fn on_delivered(&mut self, region: RegionId, pos: ChunkPos, generation: u64) {
        if let Some(record) = self.chunks.get_mut(&pos)
            && record.generation == generation
            && record.owner == Owner::Delivering
        {
            self.set_owner(pos, Owner::Region(region));
            self.pending_ready.insert(pos);
        }
    }

    /// [`Event::Lost`]：region 一侧的盒子没了。代数接着往上数，之后迟到的 `Delivered` /
    /// `Returned` 都对不上；重建的记录和等级刚进加载带时新建的一样（44 以内但不到
    /// `Full` 的是生成依赖，`Full` 的排进读盘队列，读盘照常先看停放表和写盘途中的缓冲）。
    ///
    /// [`Event::Lost`]: the region-side box is gone. The generation keeps
    /// counting so late `Delivered` / `Returned` acks no longer match; a
    /// recreated record is what a fresh load creates (a generation dependency
    /// below `Full`, queued for a read at `Full`, and reads look at parked
    /// boxes and in-flight writes first as always).
    fn on_lost(&mut self, out: &mut Vec<Effect<C>>, positions: Vec<ChunkPos>) {
        for pos in positions {
            let Some(record) = self.chunks.get(&pos) else {
                continue;
            };
            if !owner_is_held(record.owner) {
                continue;
            }
            let (desired, generation) = (record.desired, record.generation);
            self.cancel_task(out, pos);
            self.remove_record(pos);
            if desired.target.is_none() || self.shutting_down {
                continue;
            }
            self.chunks.insert(
                pos,
                ChunkRecord {
                    desired,
                    owner: Owner::Pipeline,
                    stage: ChunkStatus::Empty,
                    task: None,
                    generation: generation.wrapping_add(1),
                    snapshot: None,
                    unloaded_at: None,
                    dependency: desired.target != Some(ChunkStatus::Full),
                },
            );
            self.refresh_busy(pos);
            self.queue_read_if_needed(pos);
        }
    }

    // ---- 收回与存盘 ----

    fn on_returned(
        &mut self,
        out: &mut Vec<Effect<C>>,
        pos: ChunkPos,
        generation: u64,
        column: Option<C>,
    ) {
        let Some(record) = self.chunks.get_mut(&pos) else {
            return;
        };
        if record.generation != generation || record.owner != Owner::Reclaiming {
            return;
        }
        self.set_owner(pos, Owner::Saving);
        let record = self.chunks.get_mut(&pos).expect("记录还在");
        record.generation = record.generation.wrapping_add(1);
        let next = record.generation;
        let Some(column) = column else {
            self.remove_record(pos);
            return;
        };
        if column.is_pristine() && record.desired.target.is_none() {
            // 没改过的盒子不用停放，直接丢掉，下次重新生成。
            self.remove_record(pos);
            return;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        if let Some(old) = record.snapshot.replace(seq) {
            out.push(Effect::DropSnapshot { pos, seq: old });
        }
        self.enqueue(out, pos, next, Some(seq), Task::Save { pos, seq, column });
    }

    /// `Save` 完成：票回来了就直接重新交付盒子，不经过读盘；仍没票就停放/丢弃。
    fn on_saved(&mut self, out: &mut Vec<Effect<C>>, pos: ChunkPos, seq: SaveSeq, column: C) {
        let Some(record) = self.chunks.get_mut(&pos) else {
            return;
        };
        if record.snapshot.is_some_and(|current| seq < current) {
            return;
        }
        record.snapshot = None;
        if record.desired.target.is_some() {
            let (generation, desired) = (record.generation, record.desired);
            self.set_owner(pos, Owner::Delivering);
            self.set_unloaded_at(pos, None);
            out.push(Effect::ToRegion(RegionMessage::Deliver {
                region: HOME_REGION,
                pos,
                generation,
                column,
                desired,
            }));
        } else {
            self.finish_save(out, pos, seq, Some(column));
        }
    }

    fn finish_save(
        &mut self,
        out: &mut Vec<Effect<C>>,
        pos: ChunkPos,
        seq: SaveSeq,
        column: Option<C>,
    ) {
        let Some(record) = self.chunks.get(&pos) else {
            return;
        };
        let _ = seq;
        if record.desired.target.is_some() {
            // 存盘失败且票回来了：重新走读盘。
            self.enqueue(out, pos, record.generation, None, Task::Read { pos });
            return;
        }
        if let Some(column) = column.filter(|column| !column.is_pristine()) {
            out.push(Effect::Park { pos, column });
        }
        self.remove_record(pos);
    }

    // ---- 派任务 ----

    fn enqueue(
        &mut self,
        out: &mut Vec<Effect<C>>,
        pos: ChunkPos,
        generation: u64,
        seq: Option<SaveSeq>,
        task: Task<C>,
    ) {
        let _ = out;
        let id = self.next_task;
        self.next_task += 1;
        if self.chunks.contains_key(&pos) {
            if let Some(record) = self.chunks.get_mut(&pos) {
                record.task = Some(id);
            }
            self.refresh_busy(pos);
            self.queued_reads.remove(&pos);
        }
        self.pending.insert(
            id,
            PendingTask {
                pos,
                generation,
                seq,
                task,
            },
        );
        self.schedule_task(id);
    }

    /// 一个任务属于哪种每玩家限速。`Save` / `Assemble` 是收尾路径，不占配额。
    fn demand_kind(task: &Task<C>) -> DemandKind {
        match task {
            Task::Read { .. } | Task::Decode { .. } => DemandKind::Load,
            Task::Generate { .. } => DemandKind::Generate,
            Task::Assemble { .. } | Task::Save { .. } => DemandKind::Unbounded,
        }
    }

    /// 派出任务时登记它归哪名玩家；返回该任务能不能过每玩家在途上限。
    fn claim_task_owner(&mut self, id: TaskId, pos: ChunkPos, kind: DemandKind) -> bool {
        if matches!(kind, DemandKind::Unbounded) || self.demand.players().is_empty() {
            return true;
        }
        let Some(player) = self.demand.owner(pos) else {
            return true;
        };
        let cap = kind.cap(&self.demand);
        if cap == 0
            || self
                .demand_in_flight
                .get(&(player, kind))
                .copied()
                .unwrap_or(0)
                >= cap
        {
            return false;
        }
        *self.demand_in_flight.entry((player, kind)).or_default() += 1;
        self.task_owners.insert(id, (Some(player), kind));
        true
    }

    /// 任务完成或取消时释放每玩家计数。每个任务只登记一次，不存在的条目是空操作。
    fn release_task_owner(&mut self, id: TaskId) {
        let Some((Some(player), kind)) = self.task_owners.remove(&id) else {
            return;
        };
        let slot = self.demand_in_flight.entry((player, kind)).or_default();
        *slot = slot.saturating_sub(1);
        if *slot == 0 {
            self.demand_in_flight.remove(&(player, kind));
        }
        // 配额释放了，上一轮被挡住的任务可以再试。
    }

    /// 把一个待派任务放进小顶堆。键只算一次；玩家快照变了才整堆重算。
    fn schedule_task(&mut self, id: TaskId) {
        let key = self.task_key(id);
        self.pending_order.push(Reverse((key, id)));
    }

    /// 一个任务的排序键：rank、阶段进度、TaskId 兜底。
    fn task_key(&self, id: TaskId) -> QueueKey {
        let Some(pending) = self.pending.get(&id) else {
            return (i64::MAX, u8::MAX, id);
        };
        let stage = self
            .chunks
            .get(&pending.pos)
            .map_or(0, |record| record.stage as usize as u8);
        (self.demand.rank(pending.pos), u8::MAX - stage, id)
    }

    /// 玩家快照变了才整堆重算键。堆里只有待派任务，且只在
    /// `set_player_demand` 标脏后的第一次派单做，不是每刻。
    fn refresh_heap_if_needed(&mut self) {
        if self.heap_epoch == self.demand_epoch {
            return;
        }
        let mut heap = std::mem::take(&mut self.pending_order);
        let mut fresh = Vec::with_capacity(heap.len());
        while let Some(Reverse((_, id))) = heap.pop() {
            if self.pending.contains_key(&id) {
                fresh.push(id);
            }
        }
        for id in fresh {
            let key = self.task_key(id);
            self.pending_order.push(Reverse((key, id)));
        }
        self.heap_epoch = self.demand_epoch;
    }

    /// 队列里被取消的任务不用从堆里删：出堆时 `pending` 里查不到就跳过。
    fn forget_priority_id(&mut self, _id: TaskId) {}

    /// 每处理完一个事件，把能推进的状态往前推：
    /// 新记录排 `Read`，待派任务按写范围排他派出去，攒着的 Ready/Forgot 发掉。
    fn drive(&mut self, out: &mut Vec<Effect<C>>) {
        // 新加载、还没有在途/待派任务的记录排 Read。
        // 只有目标阶段是 `Full`（等级 ≤ 33）的区块才要读盘/生成并交付；
        // 等级 34..44 的区块是依赖圈里的半成品，由 `ensure_dependencies`
        // 建记录，不进 region、不发区块包。
        // 只排这一批新记录，不扫整张记录表。`enqueue` 会把它从队列里摘掉。
        let mut reads = std::mem::take(&mut self.reads);
        reads.clear();
        // 新排的读任务先按玩家优先级排，TaskId 因此也是「急的先发」，
        // 后面所有按 TaskId 兜底的地方都跟着这个顺序走。
        let mut queued: Vec<ChunkPos> =
            std::mem::take(&mut self.queued_reads).into_iter().collect();
        queued.sort_by_key(|&pos| (self.demand.rank(pos), pos.x, pos.z));
        for pos in queued {
            if self.chunks.get(&pos).is_some_and(|record| {
                record.owner == Owner::Pipeline
                    && record.task.is_none()
                    && !record.dependency
                    && record.desired.target == Some(ChunkStatus::Full)
            }) {
                let generation = self.chunks[&pos].generation;
                reads.push((pos, generation));
            }
        }
        for (pos, generation) in reads.drain(..) {
            self.enqueue(out, pos, generation, None, Task::Read { pos });
        }
        self.reads = reads;

        let can_submit = (self.config.max_submits_per_tick == 0
            || self.submitted_this_tick < self.config.max_submits_per_tick)
            && self.in_flight.len() < self.config.max_in_flight;
        if !can_submit {
            self.flush_notifications(out);
            return;
        }
        self.refresh_heap_if_needed();

        let cap = self.config.max_in_flight;
        // 被写范围/配额挡住的任务在本轮结束时原样放回堆；一次 drive 最多
        // 检查 MAX_SKIPS_PER_DRIVE 个，其余仍留在堆里，下一次 drive 自然再看。
        // 堆按优先级出，所以配额一释放，最急的任务当刻就能继续。
        let mut skipped: Vec<Reverse<(QueueKey, TaskId)>> = Vec::new();
        let mut skips = 0;
        loop {
            if (self.config.max_submits_per_tick > 0
                && self.submitted_this_tick >= self.config.max_submits_per_tick)
                || self.in_flight.len() >= cap
            {
                break;
            }
            let Some(Reverse((key, id))) = self.pending_order.pop() else {
                break;
            };
            if !self.pending.contains_key(&id) {
                continue;
            }
            // 阶段推进后键可能过期；重算一次再放回，不重复消费这个任务。
            let fresh = self.task_key(id);
            if fresh != key {
                skipped.push(Reverse((fresh, id)));
                skips += 1;
                if skips >= MAX_SKIPS_PER_DRIVE {
                    break;
                }
                continue;
            }
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            // 记录已经被删或代数变了，任务作废。
            let valid = self.chunks.get(&pending.pos).is_some_and(|record| {
                record.task == Some(id) && record.generation == pending.generation
            });
            if !valid {
                self.pending.remove(&id);
                continue;
            }
            let radius = pending.task.write_radius();
            let pos = pending.pos;
            let kind = Self::demand_kind(&pending.task);
            let overlaps = self
                .in_flight
                .values()
                .any(|other| write_ranges_overlap(other.pos, other.write_radius, pos, radius));
            // 公平靠每名玩家的在途硬上限：满了就跳过这个任务。
            if overlaps || !self.claim_task_owner(id, pos, kind) {
                skipped.push(Reverse((key, id)));
                skips += 1;
                if skips >= MAX_SKIPS_PER_DRIVE {
                    break;
                }
                continue;
            }
            let pending = self.pending.remove(&id).expect("上面已经确认存在");
            self.in_flight.insert(
                id,
                InFlight {
                    pos,
                    generation: pending.generation,
                    write_radius: radius,
                },
            );
            self.submitted_this_tick += 1;
            out.push(Effect::Submit {
                id,
                generation: pending.generation,
                task: pending.task,
            });
            let _ = pending.seq;
        }
        self.pending_order.extend(skipped);
        self.flush_notifications(out);
    }

    fn flush_notifications(&mut self, out: &mut Vec<Effect<C>>) {
        if !self.pending_ready.is_empty() {
            let positions = std::mem::take(&mut self.pending_ready)
                .into_iter()
                .collect();
            out.push(Effect::ToRegion(RegionMessage::Ready { positions }));
        }
        if !self.pending_forgot.is_empty() {
            let positions = std::mem::take(&mut self.pending_forgot)
                .into_iter()
                .collect();
            out.push(Effect::ToRegion(RegionMessage::Forgot { positions }));
        }
    }

    /// 由宿主注入模板盒子的构造器。精确模式/测试可以直接用 `C: Default`。
    pub fn set_template(&mut self, make: fn() -> C) {
        self.template = Some(make);
    }
}

/// 两个写范围（切比雪夫方块）是否相交。
fn write_ranges_overlap(a: ChunkPos, ar: i32, b: ChunkPos, br: i32) -> bool {
    (a.x - b.x).abs() <= ar.saturating_add(br) && (a.z - b.z).abs() <= ar.saturating_add(br)
}

/// 供测试检查的记录快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugRecord {
    pub desired: Desired,
    pub owner: Owner,
    pub stage: ChunkStatus,
    pub task: Option<TaskId>,
    pub generation: u64,
    pub snapshot: Option<SaveSeq>,
    pub unloaded_at: Option<u64>,
    pub dependency: bool,
}

/// 随机测试用它逐步断言不变量。
pub struct DebugState<'a, C> {
    director: &'a Director<C>,
}

impl<C: ChunkBundle> DebugState<'_, C> {
    /// 每条记录的公开快照，按坐标排序。
    pub fn records(&self) -> BTreeMap<ChunkPos, DebugRecord> {
        self.director
            .chunks
            .iter()
            .map(|(&pos, record)| {
                (
                    pos,
                    DebugRecord {
                        desired: record.desired,
                        owner: record.owner,
                        stage: record.stage,
                        task: record.task,
                        generation: record.generation,
                        snapshot: record.snapshot,
                        unloaded_at: record.unloaded_at,
                        dependency: record.dependency,
                    },
                )
            })
            .collect()
    }

    /// 在途任务：`id → (pos, generation, write_radius)`。
    pub fn in_flight(&self) -> BTreeMap<TaskId, (ChunkPos, u64, i32)> {
        self.director
            .in_flight
            .iter()
            .map(|(&id, task)| (id, (task.pos, task.generation, task.write_radius)))
            .collect()
    }

    /// 等待派出的任务：`id → pos`。
    pub fn pending(&self) -> BTreeMap<TaskId, ChunkPos> {
        self.director
            .pending
            .iter()
            .map(|(&id, task)| (id, task.pos))
            .collect()
    }

    pub fn tickets(&self) -> &TicketTable {
        self.director.tickets()
    }
}
