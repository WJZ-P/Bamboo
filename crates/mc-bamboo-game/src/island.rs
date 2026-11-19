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
    director::{DemandSnapshot, Desired, Event, HOME_REGION, RegionId},
    store::ChunkRevision,
    ticket::LevelChange,
};

use crate::{
    chunk_codec::PackedChunk,
    director_host::MailBudget,
    global_port::GlobalEffect,
    island_partition::SectionPos,
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

/// `island_threads` 缺省值的下限。多岛缺省打开，一条线程时卡住的慢岛占着它，别的岛都得
/// 排在它后面（慢岛只拖慢自己的前提不成立），所以缺省至少 2 条（岛的实施计划第 10 步
/// d 段的测量：`islands-heavy-4` 一条线程时四个岛一起掉到重岛的 TPS，两条起只有重岛掉）。
///
/// The lower bound of the `island_threads` default. Multi-island mode is on
/// by default, and with one thread a slow island holds it while every other
/// island queues behind (so a slow island no longer only slows itself);
/// hence at least 2 (island plan, step 10 part d: in `islands-heavy-4` one
/// thread drags all four islands down to the heavy one's TPS, from two
/// threads up only the heavy island drops).
pub const MIN_DEFAULT_ISLAND_THREADS: usize = 2;

/// `island_threads` 缺省值的上限：岛再多也只按这么多条线程并行，多出来的岛进公共队列
/// 轮流跑；线程栈是保留的地址空间，空着的线程不占 CPU。
///
/// The upper bound of the `island_threads` default: past this many islands
/// run in turns from the shared queue; idle threads cost no CPU and their
/// stacks are only reserved address space.
pub const MAX_DEFAULT_ISLAND_THREADS: usize = 8;

/// 没有显式给出时的 `island_threads`：逻辑核数的一半，夹在
/// [`MIN_DEFAULT_ISLAND_THREADS`] 和 [`MAX_DEFAULT_ISLAND_THREADS`] 之间。另一半留给主控
/// 线程、连接层的运行时、区块计算池和 IO 线程；拿不到核数时按 2。
///
/// The `island_threads` used when none is given: half the logical cores,
/// clamped between [`MIN_DEFAULT_ISLAND_THREADS`] and
/// [`MAX_DEFAULT_ISLAND_THREADS`]. The other half is left to the master
/// thread, the connection runtime, the chunk compute pool and the IO thread;
/// 2 when the core count is unknown.
pub fn recommended_island_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
    (cores / 2).clamp(MIN_DEFAULT_ISLAND_THREADS, MAX_DEFAULT_ISLAND_THREADS)
}

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

/// 没有显式选择时的 `island_threads`：环境变量优先，没有就是 [`recommended_island_threads`]。
/// 要求逐位一致的测试、对拍和探针都显式给 0，不靠这个缺省值。
///
/// The `island_threads` used when nothing is chosen explicitly: the
/// environment wins, otherwise [`recommended_island_threads`]. Bit-exact tests,
/// oracles and probes give 0 explicitly instead of relying on it.
pub fn default_island_threads() -> usize {
    island_threads_from_env().unwrap_or_else(recommended_island_threads)
}

/// `islands` 没有显式给出时的值：开（岛的实施计划第 6 节第 10 步验收后打开，第 9 节第 7 条）。
/// 要求逐位一致的对拍、探针和测试显式给 `false`。
///
/// The `islands` value when none is given: on (island plan, step 10,
/// turned on once accepted; section 9, item 7). Bit-exact oracles, probes
/// and tests give `false` explicitly.
pub const DEFAULT_ISLANDS: bool = true;

/// 改 `islands` 缺省值的环境变量（`0` / `1`、`false` / `true`）：对拍、探针和测试用它
/// 整进程切换。
///
/// The environment variable that overrides the `islands` default (`0` / `1`,
/// `false` / `true`), for oracles, probes and tests to switch a whole process.
pub const ISLANDS_ENV: &str = "BAMBOO_ISLANDS";

/// 环境变量给的 `islands`。没设或者认不出来时是 `None`。
///
/// The `islands` given by the environment, or `None` when unset or unreadable.
pub fn islands_from_env() -> Option<bool> {
    let value = std::env::var(ISLANDS_ENV).ok()?;
    match value.trim() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => {
            tracing::warn!(%value, "ignoring an invalid {ISLANDS_ENV}");
            None
        }
    }
}

/// `island_dead_seconds` 没有显式给出时的值：一个岛的同一刻在 tick 线程上连续跑超过
/// 这么多秒就判为死岛（岛的实施计划 5.6.1）。正常的慢以 2 秒为界（落后 2 秒以内追赶），
/// 30 秒是它的 15 倍；又比官方看门狗的 60 秒和连接层在途配额用完的两三分钟短。
///
/// The `island_dead_seconds` value when none is given: an island whose tick
/// keeps running on its thread for more than this many seconds is declared
/// dead (island plan 5.6.1). Ordinary slowness ends at 2 seconds (the
/// catch-up window); 30 is fifteen times that, and shorter than the vanilla
/// watchdog's 60 seconds and the few minutes a connection's in-flight quota lasts.
pub const DEFAULT_ISLAND_DEAD_SECONDS: u64 = 30;

/// `island_dead_seconds` 的下限：再短就会把一次很重的 tick 当成卡死。
///
/// The lower bound of `island_dead_seconds`; anything shorter would take one
/// heavy tick for a stuck island.
pub const MIN_ISLAND_DEAD_SECONDS: u64 = 5;

/// `island_dead_seconds` 的上限（一小时）：再长就和不判死没有区别。
///
/// The upper bound of `island_dead_seconds` (an hour); longer is no
/// different from never.
pub const MAX_ISLAND_DEAD_SECONDS: u64 = 3600;

/// 在运行时盖过 `island_dead_seconds` 的环境变量（秒，在 [`MIN_ISLAND_DEAD_SECONDS`] 和
/// [`MAX_ISLAND_DEAD_SECONDS`] 之间）。
///
/// The environment variable that overrides `island_dead_seconds` at run time
/// (seconds, between [`MIN_ISLAND_DEAD_SECONDS`] and [`MAX_ISLAND_DEAD_SECONDS`]).
pub const ISLAND_DEAD_SECONDS_ENV: &str = "BAMBOO_ISLAND_DEAD_SECONDS";

/// 环境变量给的 `island_dead_seconds`。没设或者不是上下限之间的整数时是 `None`。
///
/// The `island_dead_seconds` given by the environment, or `None` when unset
/// or not an integer within the bounds.
pub fn island_dead_seconds_from_env() -> Option<u64> {
    let value = std::env::var(ISLAND_DEAD_SECONDS_ENV).ok()?;
    match value.trim().parse::<u64>() {
        Ok(seconds) if (MIN_ISLAND_DEAD_SECONDS..=MAX_ISLAND_DEAD_SECONDS).contains(&seconds) => {
            Some(seconds)
        }
        _ => {
            tracing::warn!(%value, "ignoring an invalid {ISLAND_DEAD_SECONDS_ENV}");
            None
        }
    }
}

/// 没有显式选择时的判死时限：环境变量优先，没有就是 [`DEFAULT_ISLAND_DEAD_SECONDS`]。
///
/// The dead-island timeout used when nothing is chosen explicitly: the
/// environment wins, otherwise [`DEFAULT_ISLAND_DEAD_SECONDS`].
pub fn default_island_dead_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(
        island_dead_seconds_from_env().unwrap_or(DEFAULT_ISLAND_DEAD_SECONDS),
    )
}

/// 没有显式选择时的 `islands`：环境变量优先，没有就是 [`DEFAULT_ISLANDS`]。
///
/// The `islands` used when nothing is chosen explicitly: the environment
/// wins, otherwise [`DEFAULT_ISLANDS`].
pub fn default_islands() -> bool {
    islands_from_env().unwrap_or(DEFAULT_ISLANDS)
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
    #[cfg(test)]
    Lent(&'a mut Island),
    /// 全部岛都停放着、借给这次调用（世界用这一种；只给一个岛的 `Lent` 留给单测）：
    /// 每个岛当场收自己的信，投给还不存在的岛的信先建一个空岛（第 10 步）。
    ///
    /// Every island is parked and lent (what the world uses; `Lent` with one
    /// island is for unit tests): each takes its own mail on the spot, and mail
    /// for a missing island creates an empty one first (step 10).
    All(&'a mut Islands),
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
    /// 每一刻开头都睡这么久（真的慢岛，给烟雾测试用），一直留着。
    Lag(std::time::Duration),
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
pub(crate) struct IslandSlot {
    island: Option<Box<Island>>,
    /// 岛不在主控手里时它的玩家编号（升序）。岛不在时没人能进出它（进服、离开、搬玩家都
    /// 要它停放着），所以这份名单在它回来之前一直是对的；主控靠它知道「这名玩家在一个
    /// 没停放的岛上」（计划 5.5.1）。停放时是空的。
    ///
    /// The island's player ids (ascending) while it is out of the master's
    /// hands. Nobody can enter or leave an island that is away (joins, leaves
    /// and moves need it parked), so the list stays right until it is back;
    /// the master uses it to know that a player is on an island that is not
    /// parked (plan 5.5.1). Empty while parked.
    away_players: Vec<crate::inbound::PlayerId>,
    /// 岛不在主控手里时这一次派出的情况（见 [`Away`]）。停放时是 `None`。
    ///
    /// This dispatch's details while the island is away (see [`Away`]);
    /// `None` while parked.
    away: Option<Away>,
    /// 这个岛欠几刻：主控每次派出时它不在就加一，追赶时每多跑一刻减一（计划 5.5.1）。
    /// 只有混合驱动用；别的驱动下一直是 0。
    ///
    /// Ticks this island owes: one more each time the master dispatches while
    /// it is away, one less per catch-up tick (plan 5.5.1). Only the hybrid
    /// drive uses it; always 0 otherwise.
    pub(crate) debt: u32,
}

/// 一次派出时主控记下的东西（岛的实施计划 5.6）。岛要是再也不回来，主控靠它知道丢了
/// 什么。
///
/// What the master notes at a dispatch (island plan 5.6); if the island
/// never returns, this is how the master knows what was lost.
pub(crate) struct Away {
    /// 派出号：只增不减，岛回来时拿它对槽位。岛的编号会被复用，不能拿来认岛。
    ///
    /// The dispatch ticket: ever increasing, matched against the slot when
    /// the island returns. Island ids are reused and cannot identify one.
    pub(crate) ticket: u64,
    /// 派出时岛的时钟。
    pub(crate) clock: i64,
    /// 派出的时刻（量交接耗时用）。
    ///
    /// When it was dispatched (for the hand-over timing).
    pub(crate) dispatched: std::time::Instant,
    /// 派出时 Director 还不知道在这个岛手里的位置：信箱里没交付的盒子和没收回的位置，
    /// 以及这一刻开头收信产生、还没交给 Director 的回执（刚交付的、刚收回的盒子）。
    /// 已经确认交付的区块 Director 自己记着（`Owner::Region(这个岛)`），不用抄。
    ///
    /// Positions the Director does not yet know to be in this island's hands
    /// at dispatch: undelivered boxes and unreclaimed positions in its
    /// mailbox, and the acks from this tick's mail not yet handed to the
    /// Director (boxes just delivered or just reclaimed). Chunks already
    /// acknowledged are on the Director's own record (`Owner::Region(this)`).
    pub(crate) unacked: Vec<ChunkPos>,
}

/// 被判死的岛留给主控的东西（[`Islands::bury`]）。
///
/// What a dead island leaves the master ([`Islands::bury`]).
pub(crate) struct Buried {
    /// 它上面的玩家。
    pub(crate) players: Vec<crate::inbound::PlayerId>,
    /// 派出时记下的情况。
    pub(crate) away: Away,
    /// 它欠的刻数。
    pub(crate) debt: u32,
}

impl IslandSlot {
    pub(crate) fn new(island: Box<Island>) -> Self {
        Self {
            island: Some(island),
            away_players: Vec::new(),
            away: None,
            debt: 0,
        }
    }

    /// 岛不在主控手里时这一次派出的派出号。
    ///
    /// The ticket of the current dispatch while the island is away.
    pub(crate) fn away_ticket(&self) -> Option<u64> {
        self.away.as_ref().map(|away| away.ticket)
    }

    /// 岛在不在主控手里。
    pub(crate) fn is_home(&self) -> bool {
        self.island.is_some()
    }

    /// 拿走岛，交给 tick 线程。记下它的玩家编号和这次派出的情况（[`Away`]）。
    ///
    /// Take the island for a tick thread, remembering its player ids and this
    /// dispatch's details ([`Away`]).
    pub(crate) fn take(&mut self, ticket: u64) -> Box<Island> {
        let island = self.island.take().expect("岛已经交出去了");
        self.away_players = island.region.players.keys().copied().collect();
        let mut unacked = island.mailbox.positions();
        unacked.extend(
            island
                .replies
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Delivered { pos, .. } | Event::Returned { pos, .. } => Some(*pos),
                    _ => None,
                }),
        );
        self.away = Some(Away {
            ticket,
            clock: island.region.tick.game_time(),
            dispatched: std::time::Instant::now(),
            unacked,
        });
        island
    }

    /// 拿走停放着的岛，但它的玩家不记进「不在时的名单」：主控把全部岛临时拼成一个时
    /// （[`Islands::unify`]），这个岛的玩家跟着 `Region` 并进了发起人的岛，在那边找得到。
    ///
    /// Take the parked island without recording its players as away: while
    /// the master temporarily joins every island into one
    /// ([`Islands::unify`]) its players are on the sender's island with the
    /// `Region` and are found there.
    fn lift(&mut self) -> Box<Island> {
        self.island.take().expect("只能拼停放着的岛")
    }

    /// 岛交回来了。
    pub(crate) fn put(&mut self, island: Box<Island>) {
        debug_assert!(self.island.is_none(), "岛已经在主控手里");
        self.away_players.clear();
        self.away = None;
        self.island = Some(island);
    }

    /// 停放着的岛；不在主控手里时是 `None`。
    ///
    /// The parked island, or `None` while it is away.
    pub(crate) fn parked(&self) -> Option<&Island> {
        self.island.as_deref()
    }

    pub(crate) fn parked_mut(&mut self) -> Option<&mut Island> {
        self.island.as_deref_mut()
    }

    /// 这名玩家在这个岛上（岛不在时看派出时记下的名单）。
    ///
    /// Whether the player is on this island (by the list taken at dispatch
    /// while it is away).
    fn holds_player(&self, player: crate::inbound::PlayerId) -> bool {
        match &self.island {
            Some(island) => island.region.players.contains_key(&player),
            None => self.away_players.binary_search(&player).is_ok(),
        }
    }
}

impl std::ops::Deref for IslandSlot {
    type Target = Island;

    fn deref(&self) -> &Island {
        self.island
            .as_deref()
            .expect("岛正在 tick 线程上跑，主控碰不到它")
    }
}

impl std::ops::DerefMut for IslandSlot {
    fn deref_mut(&mut self) -> &mut Island {
        self.island
            .as_deref_mut()
            .expect("岛正在 tick 线程上跑，主控碰不到它")
    }
}

/// 主控手里的全部岛（岛的实施计划第 6 节第 10 步）。编号 [`HOME_REGION`] 的岛（「家岛」）
/// 一直在，可以是空的：只有一个岛时（`islands = false`，或者多岛模式下所有人都在一起）
/// 它就是唯一的岛，和以前的 `World.island` 一样用（`Deref` 到它的槽位）。别的岛由分区器
/// 按需建出来，空了就拆掉。按编号升序遍历，所以齐步驱动下各岛的先后是确定的。
///
/// Every island the master holds (island plan, step 10). The home island
/// ([`HOME_REGION`]) always exists and may be empty: with one island
/// (`islands = false`, or everyone together in multi-island mode) it is the
/// only one and is used like the old `World.island` (`Deref` to its slot).
/// Other islands are created on demand by the partitioner and dropped when
/// empty. Iteration is by ascending id, so the lockstep order is fixed.
pub(crate) struct Islands {
    slots: BTreeMap<RegionId, IslandSlot>,
    /// 这一刻（全局游戏时间）已经为哪些起源 section 派生过新岛的随机源
    /// （[`Self::note_derivation`]）。
    ///
    /// Origin sections that already had a new island's random sources derived
    /// at this game time ([`Self::note_derivation`]).
    derived: (i64, Vec<SectionPos>),
    /// 下一个派出号（[`Away::ticket`]）。
    next_ticket: u64,
    /// 空白岛：开服时从家岛拆出来的空 `Region`，从不派出、不进岛表，只在家岛被判死时
    /// 当模板，换一个空的家岛进槽位（岛的实施计划 5.6.2「家岛」）。只有混合驱动留它。
    ///
    /// The blank island: an empty `Region` split off home at start-up, never
    /// dispatched and not in the table, used only as the template for a fresh
    /// empty home when home is declared dead (island plan 5.6.2). Kept under
    /// the hybrid drive only.
    blueprint: Option<Box<Island>>,
    /// 测试专用：借玩家时一个都不借（改坏确认）。
    #[cfg(test)]
    pub(crate) lend_nothing: bool,
    /// 测试专用：借玩家时不按时钟差平移（改坏确认）。
    #[cfg(test)]
    pub(crate) lend_unshifted: bool,
    /// 测试专用：要借岛的命令退回只借玩家（只看得见发起人的岛的方块，改坏确认）。
    #[cfg(test)]
    pub(crate) world_view_off: bool,
    /// 测试专用：按需借的命令强制走全拼（等价性对照的另一半）。
    #[cfg(test)]
    pub(crate) borrow_all_islands: bool,
    /// 测试专用：拼了几次（确认命令真的走了这条路）。
    #[cfg(test)]
    pub(crate) unified: usize,
    /// 最近一次借了几个岛（含发起人岛；0 表示没有拼）。
    pub(crate) last_borrowed: usize,
    /// 测试专用：故意改坏按需借（等价性对照的改坏确认）。
    #[cfg(test)]
    pub(crate) borrow_break: BorrowBreak,
}

impl Islands {
    pub(crate) fn new(home: Box<Island>) -> Self {
        debug_assert_eq!(home.id, HOME_REGION);
        Self {
            slots: BTreeMap::from([(HOME_REGION, IslandSlot::new(home))]),
            derived: (i64::MIN, Vec::new()),
            next_ticket: 1,
            blueprint: None,
            #[cfg(test)]
            lend_nothing: false,
            #[cfg(test)]
            lend_unshifted: false,
            #[cfg(test)]
            world_view_off: false,
            #[cfg(test)]
            borrow_all_islands: false,
            #[cfg(test)]
            unified: 0,
            last_borrowed: 0,
            #[cfg(test)]
            borrow_break: BorrowBreak::None,
        }
    }

    /// 领一个派出号。
    ///
    /// Allocate a dispatch ticket.
    pub(crate) fn next_ticket(&mut self) -> u64 {
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        ticket
    }

    /// 留一份空白岛（见 [`Self::blueprint`] 字段）。家岛停放着、在 tick 边界时调用一次。
    ///
    /// Keep a blank island (see the `blueprint` field); called once with
    /// home parked at a tick boundary.
    pub(crate) fn keep_blueprint(&mut self) {
        let home = &mut **self.slots.get_mut(&HOME_REGION).expect("家岛一直在");
        let random = home
            .region
            .derive_island_random(home.region.global.seed, SectionPos::of(ChunkPos::new(0, 0)));
        let Ok(region) = home.region.split_off_island(
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            random,
        ) else {
            tracing::warn!("could not keep a blank island; a dead home island cannot be replaced");
            return;
        };
        self.blueprint = Some(Box::new(Self::fresh(HOME_REGION, region, home)));
    }

    /// 空白岛的 `Region`（主控换家岛之前给它重发全局快照）。
    pub(crate) fn blueprint_region_mut(&mut self) -> Option<&mut Region> {
        self.blueprint
            .as_deref_mut()
            .map(|island| &mut island.region)
    }

    /// 拿着派出号 `ticket` 的岛（它不在主控手里）。
    ///
    /// The island that is away on dispatch `ticket`.
    pub(crate) fn island_of_ticket(&self, ticket: u64) -> Option<RegionId> {
        self.slots
            .iter()
            .find(|(_, slot)| slot.away_ticket() == Some(ticket))
            .map(|(&id, _)| id)
    }

    /// 岛 `id` 不在主控手里时这一次派出的派出号。
    pub(crate) fn away_ticket(&self, id: RegionId) -> Option<u64> {
        self.slots.get(&id).and_then(IslandSlot::away_ticket)
    }

    /// 不在主控手里的岛 `id` 这一次派出时记下的情况。
    ///
    /// What was noted when island `id`, now away, was dispatched.
    pub(crate) fn away(&self, id: RegionId) -> Option<&Away> {
        self.slots.get(&id).and_then(|slot| slot.away.as_ref())
    }

    /// 把不在主控手里的岛 `id` 判死（岛的实施计划 5.6.2）：拿掉它的槽位，返回派出时记下的
    /// 东西。之后「全部岛停放」不再算它；它哪天回来了，派出号对不上任何槽位，主控丢弃。
    /// 家岛不能没有：从空白岛拆一份空的换进槽位，时钟设成它没卡住的话现在该有的时钟
    /// （派出时的时钟 + 1 + 欠的刻数），随机源按世界种子、原点 section 和现在的全局游戏
    /// 时间派生。岛停放着、或者不存在时返回 `None`。
    ///
    /// Declare island `id`, which is away, dead (island plan 5.6.2): drop its
    /// slot and return what was noted at dispatch. It no longer counts for
    /// "every island parked", and if it ever returns its ticket matches no
    /// slot and the master discards it. Home must exist: a fresh empty one is
    /// split off the blank island with the clock home would have had if it
    /// had not stuck (clock at dispatch + 1 + ticks owed) and random sources
    /// derived from the world seed, the origin section and the current game
    /// time. `None` if the island is parked or missing.
    pub(crate) fn bury(&mut self, id: RegionId) -> Option<Buried> {
        if self.slots.get(&id).is_none_or(IslandSlot::is_home) {
            return None;
        }
        let slot = self.slots.remove(&id).expect("刚看过");
        let buried = Buried {
            players: slot.away_players,
            away: slot.away.expect("不在主控手里的岛记着这次派出"),
            debt: slot.debt,
        };
        if id == HOME_REGION {
            let blueprint = self
                .blueprint
                .as_deref_mut()
                .expect("混合驱动留着空白岛，家岛判死时拿它换一个空的");
            let clock = buried.away.clock + 1 + i64::from(buried.debt);
            let delta = clock - blueprint.region.tick.game_time();
            if delta != 0 {
                blueprint.region.rebase_clock(delta);
            }
            let random = blueprint.region.derive_island_random(
                blueprint.region.global.seed,
                SectionPos::of(ChunkPos::new(0, 0)),
            );
            let region = blueprint
                .region
                .split_off_island(
                    &std::collections::BTreeSet::new(),
                    &std::collections::BTreeSet::new(),
                    random,
                )
                .unwrap_or_else(|_| panic!("空白岛上拆不出空的家岛"));
            let island = Box::new(Self::fresh(HOME_REGION, region, blueprint));
            self.slots.insert(HOME_REGION, IslandSlot::new(island));
        }
        Some(buried)
    }

    /// 岛的个数（含空的家岛）。
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// 不止一个岛：主控的帧和全局改动要扇出到别的岛。
    ///
    /// More than one island: master frames and global changes fan out.
    pub(crate) fn several(&self) -> bool {
        self.slots.len() > 1
    }

    /// 全部岛的编号，升序。
    pub(crate) fn ids(&self) -> Vec<RegionId> {
        self.slots.keys().copied().collect()
    }

    pub(crate) fn contains(&self, id: RegionId) -> bool {
        self.slots.contains_key(&id)
    }

    /// 停放着的岛。岛在 tick 线程上跑时 panic（主控碰不到它）。
    pub(crate) fn get(&self, id: RegionId) -> Option<&Island> {
        self.slots.get(&id).map(|slot| &**slot)
    }

    pub(crate) fn get_mut(&mut self, id: RegionId) -> Option<&mut Island> {
        self.slots.get_mut(&id).map(|slot| &mut **slot)
    }

    pub(crate) fn slot_mut(&mut self, id: RegionId) -> Option<&mut IslandSlot> {
        self.slots.get_mut(&id)
    }

    /// 全部停放着的岛，按编号升序。
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Island> {
        self.slots.values().map(|slot| &**slot)
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Island> {
        self.slots.values_mut().map(|slot| &mut **slot)
    }

    /// 全部岛都在主控手里。
    pub(crate) fn all_parked(&self) -> bool {
        self.slots.values().all(IslandSlot::is_home)
    }

    /// 这名玩家在哪个岛上。岛不在主控手里时按它派出时的玩家名单算。
    ///
    /// The island a player is on; an island that is away answers from the
    /// player list taken when it was dispatched.
    pub(crate) fn island_of_player(&self, player: crate::inbound::PlayerId) -> Option<RegionId> {
        self.slots
            .iter()
            .find(|(_, slot)| slot.holds_player(player))
            .map(|(&id, _)| id)
    }

    /// 已加载 `pos` 的停放着的岛（不在主控手里的岛看不到）。
    ///
    /// The parked island that has `pos` loaded (islands that are away are not seen).
    pub(crate) fn island_of_chunk(&self, pos: ChunkPos) -> Option<RegionId> {
        self.slots
            .iter()
            .find(|(_, slot)| {
                slot.parked()
                    .is_some_and(|island| island.region.chunks.is_loaded(pos))
            })
            .map(|(&id, _)| id)
    }

    /// 岛 `id` 停放着，并且它收信产生的回执都已经交给主控（可以拆合）。
    ///
    /// Island `id` is parked and every ack from its mail has gone to the
    /// master (it may be split or merged).
    pub(crate) fn acks_handed_back(&self, id: RegionId) -> bool {
        self.slots
            .get(&id)
            .and_then(IslandSlot::parked)
            .is_some_and(|island| island.replies.is_empty())
    }

    /// 岛 `id` 在表里并且停放着。
    ///
    /// Island `id` exists and is parked.
    pub(crate) fn is_parked(&self, id: RegionId) -> bool {
        self.slots.get(&id).is_some_and(IslandSlot::is_home)
    }

    /// 停放着的岛，按编号升序（混合驱动下有的岛可能不在主控手里，计划 5.5）。
    ///
    /// The parked islands by ascending id (under the hybrid drive some may be
    /// away, plan 5.5).
    pub(crate) fn parked(&self) -> impl Iterator<Item = &Island> {
        self.slots.values().filter_map(IslandSlot::parked)
    }

    pub(crate) fn parked_mut(&mut self) -> impl Iterator<Item = &mut Island> {
        self.slots.values_mut().filter_map(IslandSlot::parked_mut)
    }

    /// 停放着的岛的编号，升序。
    pub(crate) fn parked_ids(&self) -> Vec<RegionId> {
        self.slots
            .iter()
            .filter(|(_, slot)| slot.is_home())
            .map(|(&id, _)| id)
            .collect()
    }

    /// 不在主控手里的岛的编号，升序。
    ///
    /// Ids of the islands that are away, ascending.
    pub(crate) fn away_ids(&self) -> Vec<RegionId> {
        self.slots
            .iter()
            .filter(|(_, slot)| !slot.is_home())
            .map(|(&id, _)| id)
            .collect()
    }

    /// 全部在线玩家的编号（含不在主控手里的岛上的），升序。
    ///
    /// Every online player's id, players of islands that are away included, ascending.
    pub(crate) fn player_ids(&self) -> Vec<crate::inbound::PlayerId> {
        let mut ids: Vec<crate::inbound::PlayerId> = self
            .slots
            .values()
            .flat_map(|slot| match slot.parked() {
                Some(island) => island.region.players.keys().copied().collect::<Vec<_>>(),
                None => slot.away_players.clone(),
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    /// 不在主控手里的岛上的全部玩家。
    ///
    /// Every player on the islands that are away.
    pub(crate) fn away_players(&self) -> std::collections::BTreeSet<crate::inbound::PlayerId> {
        self.slots
            .values()
            .filter(|slot| !slot.is_home())
            .flat_map(|slot| slot.away_players.iter().copied())
            .collect()
    }

    /// 岛 `id` 欠几刻（计划 5.5.1）。
    pub(crate) fn debt(&self, id: RegionId) -> u32 {
        self.slots.get(&id).map_or(0, |slot| slot.debt)
    }

    /// 建空岛时照着抄的岛：家岛停放着就是它，不然是编号最小的停放着的岛；一个停放的
    /// 都没有时是 `None`（计划 5.5.1「新岛的模板」）。
    ///
    /// The island a new empty island is copied from: home when parked,
    /// otherwise the lowest parked id, or `None` when nothing is parked
    /// (plan 5.5.1).
    pub(crate) fn template(&self) -> Option<RegionId> {
        if self.is_parked(HOME_REGION) {
            return Some(HOME_REGION);
        }
        self.parked_ids().first().copied()
    }

    /// 岛 `id` 现在能直接用：已经在并且停放着，或者还不在、但有模板能建出来。
    ///
    /// Island `id` is usable now: present and parked, or absent with a
    /// template to create it from.
    pub(crate) fn can_ensure(&self, id: RegionId) -> bool {
        match self.slots.get(&id) {
            Some(slot) => slot.is_home(),
            None => self.template().is_some(),
        }
    }

    /// 编号为 `id` 的岛；没有就建一个空的（时钟、配置、全局快照、区块包模板照家岛抄，
    /// 随机源按世界种子和岛的起源 section 派生，计划 3.3「岛的随机源」第 1 条）。
    /// 只在 tick 边界、全部岛停放时调用。
    ///
    /// The island `id`, creating an empty one if missing (clock, settings,
    /// global snapshot and chunk template copied from home; random sources
    /// derived from the world seed and the origin section, plan 3.3). Tick
    /// boundary only, with every island parked.
    pub(crate) fn ensure(&mut self, id: RegionId, origin: SectionPos) -> &mut Island {
        if !self.slots.contains_key(&id) {
            let template = self.template().expect("建空岛要有一个停放着的岛当模板");
            let home = &mut **self.slots.get_mut(&template).expect("模板在表里");
            let time = home.region.global.time.0;
            let random = home
                .region
                .derive_island_random(home.region.global.seed, origin);
            let region = home
                .region
                .split_off_island(
                    &std::collections::BTreeSet::new(),
                    &std::collections::BTreeSet::new(),
                    random,
                )
                .unwrap_or_else(|_| panic!("tick 边界上建不出空岛 {id}"));
            let mut island = Box::new(Self::fresh(id, region, home));
            island.state = IslandState::Parked;
            self.slots.insert(id, IslandSlot::new(island));
            self.note_derivation(origin, time);
        }
        self.get_mut(id).expect("刚建好")
    }

    /// 照 `like` 的执行配置给一个新建的 `Region` 套上岛这一层。
    fn fresh(id: RegionId, region: Region, like: &Island) -> Island {
        let mut island = Island::new(id, region, like.mailbox.budget);
        island.entity_threads = like.entity_threads;
        island.last_worker = like.last_worker;
        island
    }

    /// 记下这一刻为 `origin` 派生了一个新岛的随机源。派生的输入是（世界种子，起源
    /// section，建岛时的全局游戏时间），同一刻同一起源再派生一次会拿到完全相同的随机源，
    /// 两个岛的实体种子、牛的 UUID 就会重复（计划 3.3「岛的随机源」第 1 条）。分区器有
    /// 拆分迟滞，正常运行碰不到；测试和调试构建里断言，正式构建不 panic。
    ///
    /// Record that a new island's random sources were derived for `origin` at
    /// game time `time`. Deriving again for the same origin in the same tick
    /// would yield identical sources (repeated entity seeds and cow UUIDs; plan
    /// 3.3). The partitioner's split delay keeps this from happening; tests and
    /// debug builds assert it, release builds do not panic.
    pub(crate) fn note_derivation(&mut self, origin: SectionPos, time: i64) {
        if self.derived.0 != time {
            self.derived = (time, Vec::new());
        }
        debug_assert!(
            !self.derived.1.contains(&origin),
            "同一刻（游戏时间 {time}）为起源 {origin:?} 派生了两次新岛的随机源"
        );
        self.derived.1.push(origin);
    }

    pub(crate) fn insert(&mut self, island: Box<Island>) {
        let previous = self.slots.insert(island.id, IslandSlot::new(island));
        debug_assert!(previous.is_none(), "岛的编号重复");
    }

    /// 拿走一个停放着的岛（拆合时）。家岛不能拿走。
    pub(crate) fn remove(&mut self, id: RegionId) -> Option<Box<Island>> {
        debug_assert_ne!(id, HOME_REGION, "家岛不能拿走");
        self.slots
            .remove(&id)
            .map(|slot| slot.island.expect("只能拿走停放着的岛"))
    }

    /// 拆掉空了的岛（没有区块、玩家、信件、实体）。家岛留着。
    ///
    /// Drop islands that became empty (no chunks, players, mail or entities).
    /// Home stays.
    pub(crate) fn drop_empty(&mut self) -> Vec<RegionId> {
        let empty: Vec<RegionId> = self
            .slots
            .iter()
            .filter(|(id, slot)| **id != HOME_REGION && slot.is_home() && slot.is_empty())
            .map(|(&id, _)| id)
            .collect();
        for id in &empty {
            self.slots.remove(id);
        }
        empty
    }

    /// 家岛（停放着）。
    #[cfg(test)]
    pub(crate) fn home_mut(&mut self) -> &mut Island {
        self.get_mut(HOME_REGION).expect("家岛一直在")
    }

    /// 全部停放着的岛，按编号升序，给要一次处理所有岛的主控调用（存盘名单）。
    pub(crate) fn all_mut(&mut self) -> Vec<&mut Island> {
        self.iter_mut().collect()
    }

    /// 主控要在岛停放时执行一件跟别的岛上的玩家有关的事（经 `World::on_master` 这个唯一
    /// 入口：全局类命令、控制台命令、交回的全局改动、进出服的全局部分、跨岛命令、聊天）：
    /// 先把 `borrow` 说的那些岛上的玩家连同跟着玩家走的七类表（`region/island_split.rs`
    /// 的 `PlayerTables`）整套借到 `into` 上，事情做完再按原样还回去（[`Self::scatter`]）。
    /// 借过来的玩家和本来就在这里的玩家一起按编号排，所以选择器、给 OP 的广播、每名接收者
    /// 的签名聊天状态都和所有人在一个岛上时一样。这些岛这时都停放着，主控写进借来的链路
    /// 就等于岛按顺序收下主控的出站信件（计划 4.3）。只有一个岛时什么都不做。混合驱动下
    /// 只借停放着的岛（不在主控手里的岛的那一份走延后投递，计划 5.5.2）；两岛时钟不同时
    /// 玩家身上的绝对时刻按时钟差平移。
    ///
    /// The master does something involving players of other islands while
    /// the islands are parked (through the single entry `World::on_master`:
    /// global or console commands, handed-back global changes, the global part
    /// of joins and leaves, cross-island commands, chat): first borrow the
    /// players of the islands named by `borrow`, with all seven
    /// player-following tables (`PlayerTables` in `region/island_split.rs`),
    /// onto `into`, and give them back unchanged afterwards
    /// ([`Self::scatter`]). Borrowed players sort with the local ones by id, so
    /// selectors, broadcasts to ops and every receiver's signed-chat state work
    /// as if everyone were on one island. Those islands are parked, so writing
    /// into a borrowed link equals the island taking the master's outbound
    /// mail in order (plan 4.3). A no-op with one island.
    pub(crate) fn gather(&mut self, into: RegionId, borrow: &Borrow) -> Gathered {
        let mut gathered = Gathered(Vec::new());
        if self.slots.len() <= 1 || !self.slots.contains_key(&into) {
            return gathered;
        }
        #[cfg(test)]
        if self.lend_nothing {
            return gathered;
        }
        let others: Vec<RegionId> = match borrow {
            Borrow::All | Borrow::World | Borrow::Islands(_) => self
                .parked_ids()
                .into_iter()
                .filter(|&id| id != into)
                .collect(),
            Borrow::PlayersOf(players) => {
                let mut ids: Vec<RegionId> = players
                    .iter()
                    .filter_map(|&player| self.island_of_player(player))
                    .filter(|&id| id != into && self.is_parked(id))
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                ids
            }
        };
        for id in others {
            let lent = {
                let region = &mut self.get_mut(id).expect("在表里").region;
                let players: std::collections::BTreeSet<_> =
                    region.players.keys().copied().collect();
                if players.is_empty() {
                    continue;
                }
                region.lend_players(&players)
            };
            let ids = lent.ids();
            let shift = self.lend_shift();
            self.get_mut(into)
                .expect("在表里")
                .region
                .take_players(lent, shift);
            gathered.0.push((id, ids));
        }
        gathered
    }

    /// 把 [`Self::gather`] 借来的玩家（七类表整套）还回原来的岛。原来的岛不在了（不会发生：
    /// 借着的时候不拆合）就留在 `into`。
    ///
    /// Give the players borrowed by [`Self::gather`] (all seven tables) back to
    /// their islands; if an island vanished meanwhile (cannot happen: nothing
    /// splits or merges while borrowed) they stay on `into`.
    pub(crate) fn scatter(&mut self, into: RegionId, gathered: Gathered) {
        for (id, ids) in gathered.0 {
            let set: std::collections::BTreeSet<_> = ids.iter().copied().collect();
            let lent = self
                .get_mut(into)
                .expect("在表里")
                .region
                .lend_players(&set);
            let shift = self.lend_shift();
            match self.get_mut(id) {
                Some(island) => island.region.take_players(lent, shift),
                None => self
                    .get_mut(into)
                    .expect("在表里")
                    .region
                    .take_players(lent, shift),
            }
        }
    }

    /// 主控要在一条命令看到的世界里执行它（计划 4.5 / 4.5.1）：把 `islands` 里 `into`
    /// 以外的每个岛的 `Region` 整个并进 `into`，命令在拼好的 `Region` 上执行，看到的方块、
    /// 方块实体、容器、实体和玩家都和拼起这几个岛时一样；做完用 [`Self::divide`] 按原样
    /// 拆回去。每个被并进来的岛记下它的区块集合、玩家集合、四个随机源的状态和剖析器、
    /// 和 `into` 的时钟差；岛这一层的东西（区块信箱、存盘名单和报告、排队的全局改动、
    /// 回执、欠刻）留在槽位里不动。`islands` 只含 `into` 时完全不拼。
    /// [`Self::unify`] 是「全部岛」的同一段路径（全拼兜底）。
    /// 全部岛都要停放着（断言）。只有一个岛时什么都不做。
    ///
    /// 什么都没有的岛（没有区块也没有玩家，比如空着的家岛）不并：拆的时候按区块和玩家
    /// 分，它没有这两样，并进来的东西就回不去了。
    ///
    /// 光照：排着还没开始算的任务跟着区块走，不用先算完；哪个岛正好有算到一半的轮次
    /// 就先把它算完（见计划 4.5「光照」）。
    ///
    /// The master runs a command on the whole world (plan 4.5): every other
    /// island's `Region` is poured into `into`, the command runs on the joined
    /// `Region` and sees the blocks, block entities, containers, entities and
    /// players exactly as on a single island, and [`Self::divide`] splits them
    /// back afterwards. For each absorbed island its chunk set, player set,
    /// random-source state, profiler and clock difference are kept; its
    /// island-level state (chunk mailbox, save lists, queued global changes,
    /// acks, ticks owed) stays untouched in its slot. Every island must be
    /// parked (asserted). A no-op with one island. Islands with neither
    /// chunks nor players (an empty home) are left alone, since the split
    /// back goes by chunks and players. Light tasks queued but not started
    /// travel with their chunks; a round that is half done is finished first.
    pub(crate) fn unify(&mut self, into: RegionId) -> Unified {
        let all: std::collections::BTreeSet<RegionId> = self.ids().into_iter().collect();
        self.unify_subset(into, &all)
    }

    /// 只拼 `islands` 里这几个岛（计划 4.5.1）；[`Self::unify`] 传全部岛。
    ///
    /// Join only the islands in `islands` (plan 4.5.1); [`Self::unify`] passes
    /// every island.
    pub(crate) fn unify_subset(
        &mut self,
        into: RegionId,
        islands: &std::collections::BTreeSet<RegionId>,
    ) -> Unified {
        let mut unified = Unified(Vec::new());
        if self.slots.len() <= 1 || islands.len() <= 1 {
            return unified;
        }
        assert!(self.all_parked(), "要借岛的命令等全部岛停放");
        #[cfg(test)]
        {
            self.unified += 1;
        }
        self.last_borrowed = islands.len();
        let others: Vec<RegionId> = self
            .ids()
            .into_iter()
            .filter(|&id| id != into && islands.contains(&id))
            .collect();
        for id in others {
            let region = &mut self.get_mut(id).expect("在表里").region;
            let chunks: std::collections::BTreeSet<ChunkPos> =
                region.chunks.loaded_positions().into_iter().collect();
            let players: std::collections::BTreeSet<_> = region.players.keys().copied().collect();
            crate::region::island_split::lap("unify record");
            if chunks.is_empty() && players.is_empty() {
                continue;
            }
            if !region.chunks.ready_to_split() {
                region.chunks.finish_light_updates();
            }
            let island = self.slots.get_mut(&id).expect("在表里").lift();
            let (shell, region) = island.into_shell();
            let target = &mut self.get_mut(into).expect("发起人的岛在表里").region;
            if !target.chunks.ready_to_split() {
                target.chunks.finish_light_updates();
            }
            let delta = target.tick.game_time() - region.tick.game_time();
            let identity = target.absorb_island_keeping(region, delta);
            crate::region::island_split::lap("unify absorbed");
            unified.0.push(UnifiedIsland {
                shell,
                chunks,
                players,
                identity,
                delta,
            });
        }
        unified
    }

    /// [`Self::unify`] 的逆操作：把每个并进来的岛按记下的区块和玩家拆回去，换上它自己的
    /// 随机源状态和剖析器，时钟平移回去，放回槽位。命令期间新出现的东西按位置（方块、
    /// 计划刻、方块实体、容器、实体、待广播的变化）或者按玩家（缓冲里写好的包、菜单）
    /// 各回各的岛；被命令送到别的岛的位置上的玩家回他原来的岛，之后由
    /// `World::relocate_players` 搬。拆不回去说明不在 tick 边界，panic。
    ///
    /// The inverse of [`Self::unify`]: split every absorbed island back by its
    /// recorded chunks and players, give it its own random-source state and
    /// profiler, rebase its clock back and seat it. What the command created
    /// goes by position or by player to the island that owns it; a player the
    /// command sent onto another island's ground returns to their old island
    /// and is moved later by `World::relocate_players`. Failing to split
    /// means this is not a tick boundary and panics.
    pub(crate) fn divide(&mut self, into: RegionId, unified: Unified) {
        #[cfg(test)]
        let break_identity = self.borrow_break == BorrowBreak::MissIdentity;
        #[cfg_attr(not(test), allow(unused_mut))]
        for (mut part, index) in unified.0.into_iter().zip(0usize..).collect::<Vec<_>>() {
            let _ = index;
            #[cfg(test)]
            if break_identity && index == 0 {
                // 故意不把一个岛的时钟平移回去（改坏确认）。
                part.delta = part.delta.wrapping_add(1);
            }
            let UnifiedIsland {
                shell,
                chunks,
                mut players,
                identity,
                delta,
            } = part;
            let id = shell.id;
            let joined = &mut self.get_mut(into).expect("发起人的岛在表里").region;
            players.retain(|player| joined.players.contains_key(player));
            crate::region::island_split::lap("divide prepare");
            let mut region = joined
                .split_off_island_restoring(&chunks, &players, identity)
                .unwrap_or_else(|_| panic!("命令执行完以后拆不回岛 {id}（不在 tick 边界）"));
            if delta != 0 {
                region.rebase_clock(-delta);
            }
            let island = Box::new(Island::from_shell(shell, region));
            self.slots.get_mut(&id).expect("槽位留着").put(island);
            crate::region::island_split::lap("divide seated");
        }
    }

    /// 借玩家时要不要按时钟差平移（正式构建恒为真）。
    fn lend_shift(&self) -> bool {
        #[cfg(test)]
        if self.lend_unshifted {
            return false;
        }
        true
    }
}

/// 测试专用：故意漏借一类东西，验证等价性对照真的能抓到（计划 4.5.1 第 7 节）。
///
/// Test-only: deliberately miss a class so the equivalence checks must fail.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BorrowBreak {
    None,
    /// 规划器漏借一个涉及的岛。
    MissIsland,
    /// 选择器筛查漏一个岛的实体。
    MissSelector,
    /// 不借 `R` 外的玩家。
    MissPlayers,
    /// `/tp` 非玩家实体到别的岛时漏借目的地岛。
    MissDestination,
    /// 规划期漏查 `/summon` NBT 里的显式 UUID。
    MissUuid,
    /// `divide` 少还原一个岛的随机源。
    MissIdentity,
}

/// 主控借玩家时要哪些岛上的玩家（借玩家的唯一入口 `World::on_master` 的参数）。以后
/// 各岛不再每刻一起停放时（计划 5.2），入口可以按它判断要等哪些岛。
///
/// Whose players the master borrows (the parameter of the single borrowing
/// entry `World::on_master`). Once islands no longer park together every tick
/// (plan 5.2), the entry can use it to know which islands to wait for.
pub(crate) enum Borrow {
    /// 全部岛上的玩家：选择器（`@a`、玩家名）、全服广播（Tab 名单、进出服消息、聊天）、
    /// 给 OP 的反馈。
    ///
    /// Every island's players: selectors, server-wide broadcasts, feedback to ops.
    All,
    /// 整个世界：全部岛的 `Region` 临时拼成一个（[`Islands::unify`]），命令里的方块坐标、
    /// 容器、实体和读方块的玩家命令都和整个世界一个岛时一样（计划 4.5）。现在只作按需借
    /// 算不出范围时的兜底（计划 4.5.1）。要全部岛停放。
    ///
    /// The whole world: every island's `Region` is temporarily joined
    /// ([`Islands::unify`]) so block positions, containers, entities and
    /// player commands that read blocks behave as on a single island (plan
    /// 4.5). Now only the fallback when on-demand borrowing cannot work out
    /// the island set (plan 4.5.1). Needs every island parked.
    World,
    /// 只把这几座岛的 `Region` 并进发起人的岛，其余岛只借玩家（计划 4.5.1）。
    /// 集合恒含发起人的岛；只有一座岛时完全不拼。
    ///
    /// Join only these islands' `Region`s into the sender's; the rest lend
    /// only their players (plan 4.5.1). The set always contains the sender's
    /// island; with a single island nothing is joined.
    Islands(std::collections::BTreeSet<RegionId>),
    /// 只要这几名玩家所在的岛。
    ///
    /// Only the islands these players are on.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "现在每个用途都要全部岛 / every use needs all islands today"
        )
    )]
    PlayersOf(Vec<crate::inbound::PlayerId>),
}

/// [`Islands::unify`] 并进发起人的岛的全部岛，等着 [`Islands::divide`] 拆回去。
///
/// Every island [`Islands::unify`] poured into the sender's, waiting for
/// [`Islands::divide`].
pub(crate) struct Unified(Vec<UnifiedIsland>);

/// 一个并进去的岛留在外面的部分。
struct UnifiedIsland {
    /// 岛这一层的东西（不跟着 `Region` 并进去）。
    shell: IslandShell,
    /// 拼之前它已加载的区块。
    chunks: std::collections::BTreeSet<ChunkPos>,
    /// 拼之前它的玩家。
    players: std::collections::BTreeSet<crate::inbound::PlayerId>,
    /// 它自己的随机源状态和剖析器。
    identity: crate::region::IslandIdentity,
    /// 发起人的岛的时钟减它的时钟（并进去时平移了这么多，拆回来时平移回去）。
    delta: i64,
}

/// 一个岛除了 `Region` 以外的全部字段（见 [`Island::into_shell`]）。
///
/// Every field of an island except its `Region` (see [`Island::into_shell`]).
struct IslandShell {
    id: RegionId,
    entity_threads: usize,
    effects: Vec<GlobalEffect>,
    mailbox: ChunkMailbox,
    replies: MailReport,
    save_plan: SavePlan,
    save_report: SaveReport,
    last_worker: usize,
    state: IslandState,
    #[cfg(test)]
    test_hook: Option<IslandTestHook>,
}

/// [`Islands::gather`] 借走的玩家：岛编号和那个岛的玩家编号。
pub(crate) struct Gathered(Vec<(RegionId, Vec<crate::inbound::PlayerId>)>);

impl std::ops::Deref for Islands {
    type Target = IslandSlot;

    fn deref(&self) -> &IslandSlot {
        self.slots.get(&HOME_REGION).expect("家岛一直在")
    }
}

impl std::ops::DerefMut for Islands {
    fn deref_mut(&mut self) -> &mut IslandSlot {
        self.slots.get_mut(&HOME_REGION).expect("家岛一直在")
    }
}

impl Island {
    /// 岛上什么都没有了：没有已加载的区块、玩家、实体，信箱和存盘名单也空着。
    ///
    /// Nothing left: no loaded chunk, player or entity, empty mailbox and save lists.
    pub(crate) fn is_empty(&self) -> bool {
        self.region.players.is_empty()
            && self.region.chunks.loaded_count().unwrap_or(0) == 0
            && self.region.entities.len() == 0
            && self.mailbox.deliveries.is_empty()
            && self.mailbox.reclaims.is_empty()
            && self.effects.is_empty()
            && self.replies.is_empty()
            && self.save_plan.encode.is_empty()
            && self.save_plan.remark.is_empty()
            && self.save_report.encoded.is_empty()
            && self.save_report.changes.is_none()
    }

    /// 把岛拆成 `Region` 和其余的字段。逐个字段解构：以后给 `Island` 加字段而没决定它在
    /// 「拼成一个」时留在哪边，编译就不过。
    ///
    /// Take the island apart into its `Region` and everything else, field by
    /// field, so a new `Island` field without a decision for the joined view
    /// does not compile.
    fn into_shell(self: Box<Self>) -> (IslandShell, Region) {
        let Self {
            id,
            region,
            entity_threads,
            effects,
            mailbox,
            replies,
            save_plan,
            save_report,
            last_worker,
            state,
            #[cfg(test)]
            test_hook,
        } = *self;
        (
            IslandShell {
                id,
                entity_threads,
                effects,
                mailbox,
                replies,
                save_plan,
                save_report,
                last_worker,
                state,
                #[cfg(test)]
                test_hook,
            },
            region,
        )
    }

    /// [`Self::into_shell`] 的逆操作。
    fn from_shell(shell: IslandShell, region: Region) -> Self {
        let IslandShell {
            id,
            entity_threads,
            effects,
            mailbox,
            replies,
            save_plan,
            save_report,
            last_worker,
            state,
            #[cfg(test)]
            test_hook,
        } = shell;
        Self {
            id,
            region,
            entity_threads,
            effects,
            mailbox,
            replies,
            save_plan,
            save_report,
            last_worker,
            state,
            #[cfg(test)]
            test_hook,
        }
    }

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
            Some(IslandTestHook::Lag(time)) => {
                std::thread::sleep(time);
                self.test_hook = Some(IslandTestHook::Lag(time));
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

    /// 岛拆分（岛的实施计划第 6 节第 9 步）：拆出 `chunks` 和 `players`，造出编号为 `id`
    /// 的新岛。`Region` 的部分见 [`Region::split_off_island`]；岛这一层再分区块信箱、存盘
    /// 名单和存盘报告。只在岛停放、交回的回执已经处理完时调用（断言）；排队的全局改动
    /// 留在原岛。和信箱交付前一样先把攒着的光照算完。新岛停放着，执行配置照抄。
    /// 运行时用 [`Self::split_off_island_boxed`]（新岛直接在堆上）。
    ///
    /// Island split: carve `chunks` and `players` off into a new island `id`.
    /// See [`Region::split_off_island`] for the region; this level also splits
    /// the chunk mailbox, the save plan and the save report. Parked islands
    /// only, after the acks were processed; queued global changes stay. The
    /// run time uses [`Self::split_off_island_boxed`].
    #[cfg(test)]
    pub(crate) fn split_off_island(
        &mut self,
        id: RegionId,
        chunks: &std::collections::BTreeSet<ChunkPos>,
        players: &std::collections::BTreeSet<crate::inbound::PlayerId>,
        random: crate::region::IslandRandom,
    ) -> Result<Self, crate::region::NotReady> {
        self.split_off_island_boxed(id, chunks, players, random)
            .map(|island| *island)
    }

    /// 岛合并：把停放着的 `other` 倒进自己。`delta = 自己的时钟 − other 的时钟`，见
    /// [`Region::absorb_island`]。区块信箱、存盘名单和报告、排队的全局改动接在后面。
    /// 运行时用 [`Self::absorb_island_boxed`]。
    ///
    /// Island merge: pour the parked `other` into this island, with
    /// `delta = self.now − other.now` (see [`Region::absorb_island`]). The run
    /// time uses [`Self::absorb_island_boxed`].
    #[cfg(test)]
    pub(crate) fn absorb_island(&mut self, other: Self, delta: i64) {
        self.absorb_island_boxed(Box::new(other), delta);
    }

    /// 同 [`Self::split_off_island`]，新岛直接放在堆上（主控线程的栈余量薄，岛很大）。
    ///
    /// Like [`Self::split_off_island`], with the new island boxed (the master
    /// thread's stack is thin and an island is large).
    pub(crate) fn split_off_island_boxed(
        &mut self,
        id: RegionId,
        chunks: &std::collections::BTreeSet<ChunkPos>,
        players: &std::collections::BTreeSet<crate::inbound::PlayerId>,
        random: crate::region::IslandRandom,
    ) -> Result<Box<Self>, crate::region::NotReady> {
        self.split_off_island_skipping(id, chunks, players, random, None)
    }

    /// 同 [`Self::split_off_island_boxed`]；`skip` 给出时故意不搬那一类（只给改坏确认用）。
    ///
    /// Like [`Self::split_off_island_boxed`]; `skip` leaves one class behind
    /// on purpose (break-it checks only).
    pub(crate) fn split_off_island_skipping(
        &mut self,
        id: RegionId,
        chunks: &std::collections::BTreeSet<ChunkPos>,
        players: &std::collections::BTreeSet<crate::inbound::PlayerId>,
        random: crate::region::IslandRandom,
        skip: Option<crate::region::IslandPart>,
    ) -> Result<Box<Self>, crate::region::NotReady> {
        assert_eq!(self.state, IslandState::Parked, "只能拆停放着的岛");
        assert!(self.replies.is_empty(), "交回还没处理完就拆岛");
        self.region.chunks.finish_light_updates();
        crate::region::island_split::lap("light");
        let region = self
            .region
            .split_off_island_parts(chunks, players, random, skip)?;
        let mut out = Box::new(Self::new(id, region, self.mailbox.budget));
        out.mailbox = self.mailbox.split_off(chunks);
        out.save_plan = self.save_plan.split_off(chunks);
        out.save_report = self.save_report.split_off(chunks);
        crate::region::island_split::lap("Mailbox/SavePlan/SaveReport");
        out.entity_threads = self.entity_threads;
        out.last_worker = self.last_worker;
        Ok(out)
    }

    /// 同 [`Self::absorb_island`]，被吸收方在堆上。
    ///
    /// Like [`Self::absorb_island`], with the absorbed island boxed.
    pub(crate) fn absorb_island_boxed(&mut self, other: Box<Self>, delta: i64) {
        assert!(
            self.state == IslandState::Parked && other.state == IslandState::Parked,
            "只能合停放着的岛"
        );
        assert!(other.replies.is_empty(), "交回还没处理完就合岛");
        let mut other = other;
        self.region.chunks.finish_light_updates();
        other.region.chunks.finish_light_updates();
        self.mailbox.absorb(std::mem::replace(
            &mut other.mailbox,
            ChunkMailbox::new(self.mailbox.budget),
        ));
        self.save_plan.absorb(std::mem::take(&mut other.save_plan));
        self.save_report
            .absorb(std::mem::take(&mut other.save_report));
        self.effects.append(&mut other.effects);
        crate::region::island_split::lap("merge light/Mailbox/SavePlan/SaveReport");
        let Self { region, .. } = *other;
        self.region.absorb_island(region, delta);
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

impl SavePlan {
    /// 岛拆分：名单里的位置按区块分；两个剖析指标留在原岛，是否交回改动两边相同。
    ///
    /// Island split: positions split by chunk; the metrics stay, `drain` is copied.
    pub(crate) fn split_off(&mut self, chunks: &std::collections::BTreeSet<ChunkPos>) -> Self {
        let inside = |pos: &ChunkPos| chunks.contains(pos);
        let (remark, kept) = std::mem::take(&mut self.remark)
            .into_iter()
            .partition(inside);
        self.remark = kept;
        let (encode, kept) = std::mem::take(&mut self.encode)
            .into_iter()
            .partition(inside);
        self.encode = kept;
        #[cfg(test)]
        let verify = {
            let (verify, kept) = std::mem::take(&mut self.verify)
                .into_iter()
                .partition(|(pos, _)| chunks.contains(pos));
            self.verify = kept;
            verify
        };
        Self {
            remark,
            encode,
            metrics: None,
            drain: self.drain,
            #[cfg(test)]
            verify,
        }
    }

    /// 岛合并：名单接起来。「放回改动集合」的那份绝不能丢，否则那些区块不再判脏。
    ///
    /// Island merge: lists are concatenated; the remark list must never be lost.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.remark.extend(other.remark);
        self.encode.extend(other.encode);
        self.metrics = self.metrics.or(other.metrics);
        self.drain |= other.drain;
        #[cfg(test)]
        self.verify.extend(other.verify);
    }
}

impl SaveReport {
    /// 岛拆分：编好的快照和交回的改动按区块分。
    ///
    /// Island split: encoded snapshots and handed-back changes split by chunk.
    pub(crate) fn split_off(&mut self, chunks: &std::collections::BTreeSet<ChunkPos>) -> Self {
        let (encoded, kept) = std::mem::take(&mut self.encoded)
            .into_iter()
            .partition(|encoded| chunks.contains(&encoded.pos));
        self.encoded = kept;
        let changes = self.changes.as_mut().map(|changes| {
            let (changed, kept) = std::mem::take(&mut changes.changed)
                .into_iter()
                .partition(|changed| chunks.contains(&changed.pos));
            changes.changed = kept;
            let (content, kept) = std::mem::take(&mut changes.content)
                .into_iter()
                .partition(|pos| chunks.contains(pos));
            changes.content = kept;
            SaveChanges { changed, content }
        });
        Self { encoded, changes }
    }

    /// 岛合并：接起来。
    ///
    /// Island merge: concatenated.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.encoded.extend(other.encoded);
        match (&mut self.changes, other.changes) {
            (Some(mine), Some(theirs)) => {
                mine.changed.extend(theirs.changed);
                mine.content.extend(theirs.content);
            }
            (mine @ None, theirs) => *mine = theirs,
            (Some(_), None) => {}
        }
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

    /// 岛拆分：位置落在 `chunks` 里的待交付盒子和待收回位置搬进一份新信箱（岛的实施计划
    /// 3.3「区块信箱里的信」）。预算照抄；本刻的计数从零开始，新岛下一刻开头本来也要重置。
    ///
    /// Island split: boxes and reclaims for positions in `chunks` move to a new
    /// mailbox. The budget is copied; this tick's counters start from zero.
    pub(crate) fn split_off(&mut self, chunks: &std::collections::BTreeSet<ChunkPos>) -> Self {
        let mut out = Self::new(self.budget);
        out.budget_left = self.budget_left;
        for pos in chunks {
            if let Some(entry) = self.deliveries.remove(pos) {
                out.deliveries.insert(*pos, entry);
            }
        }
        let (moved, kept) = std::mem::take(&mut self.reclaims)
            .into_iter()
            .partition(|(pos, _)| chunks.contains(pos));
        self.reclaims = kept;
        out.reclaims = moved;
        out
    }

    /// 岛合并：另一份信箱里的盒子和收回请求并进来，按原来的先后接在后面。
    ///
    /// Island merge: the other mailbox's boxes and reclaims are appended.
    pub(crate) fn absorb(&mut self, other: Self) {
        for (pos, entry) in other.deliveries {
            let previous = self.deliveries.insert(pos, entry);
            debug_assert!(previous.is_none(), "两个岛的信箱里有同一个位置的盒子");
        }
        self.reclaims.extend(other.reclaims);
    }

    /// 待交付和待收回的位置（拆岛时它们跟着区块走）。
    pub(crate) fn positions(&self) -> Vec<ChunkPos> {
        self.deliveries
            .keys()
            .copied()
            .chain(self.reclaims.iter().map(|(pos, _)| *pos))
            .collect()
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
