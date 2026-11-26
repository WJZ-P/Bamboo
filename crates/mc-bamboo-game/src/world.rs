//! 世界线程的主循环：20 TPS，每 tick 先处理队列里的消息，再按固定顺序推进各个系统。

use std::{
    ops::ControlFlow,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::packets::play::serverbound;
use mc_bamboo_protocol::{
    packet::play::{ClientInformation, GameMode, PlayRequest},
    text::Component,
};
use mc_bamboo_storage::{
    chunk_io::ChunkIo,
    player_data::PlayerData,
    world_dir::{encode_player_data, player_uuid_string},
};
use mc_bamboo_world::director::{
    DemandSnapshot, DirectorConfig, HOME_REGION, PipelineKind, PlayerDemand, TicketOp,
};

use crate::director_host::{DirectorHost, MailBudget};
use tracing::{debug, error, info, warn};

use crate::{
    book, chat, chunk_broadcast,
    chunk_generator::OverworldGenerator,
    chunk_tracking::{self, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE},
    command, container, entity_sync, entity_tracking,
    global::{Global, RosterEntry},
    global_port::{GlobalEffect, GlobalPort, PortMode},
    inbound::{Inbound, InboundReceiver, JoinRequest, PlayerId, PlayerInbox, Sequenced},
    interaction::{self, UseItemOn},
    inventory,
    island::{Borrow, BorrowClass, Island, IslandAccess, IslandDrive, Islands, Posted},
    island_pool::{IslandPool, MAX_ISLAND_THREADS, Wait},
    lectern, light_updates, menu, movement,
    pacing::{Pacer, WallTiming},
    player::shows_hat,
    player_save::{
        game_mode_from_id, restore_experience_seed, restore_inventory, runtime_player_data,
    },
    profiler::{ChunkPhase, Metric, Phase},
    region::{JoinSettings, Region},
    save_session::{SaveCommand, SaveSession},
    secure_chat::{self, ServiceProfileKeys},
    sign, weather,
    world_state::RespawnData,
};

const TICK: Duration = Duration::from_millis(50);
/// 落后超过这个时长就不追了，直接跳过去。短暂的卡顿靠连续跑 tick 追上。
const MAX_CATCH_UP: Duration = Duration::from_secs(2);
/// 信箱预算和实体线程数的上限，防住配置文件里的巨值。
pub const MAX_MAIL_BUDGET: usize = 1 << 16;
pub const MAX_ENTITY_THREADS: usize = 256;
/// `max_entity_collisions` 的合法上限（只是防配置写坏，Paper 缺省是 8）。
pub const MAX_ENTITY_COLLISIONS: usize = 4096;
/// 三档降频两个距离的合法上限（只是防配置写坏；推荐值 32 / 64）。
pub const MAX_MOB_DISTANCE: usize = 4096;
/// `mob_wakeups_scale` 的合法上限，和 `mc_bamboo_blocks` 里的常量一致。
pub const MAX_MOB_WAKEUPS_SCALE: usize = mc_bamboo_blocks::entity::mob::throttle::MAX_WAKEUPS_SCALE;
/// 区块计算池线程数的上限。
pub const MAX_CHUNK_WORKER_THREADS: usize = 64;
/// 独立寻路阶段每岛每刻节点预算的合法上限（只是防配置写坏）。
pub const MAX_PATHFINDING_BUDGET: u32 = 1 << 20;
/// 村民大脑降频间隔上限（刻），与 `mc_bamboo_blocks` 保持一致。
pub const MAX_VILLAGER_BRAIN_INTERVAL: i32 =
    mc_bamboo_blocks::entity::mob::throttle::MAX_VILLAGER_BRAIN_INTERVAL;

const SERVER_SHUTDOWN: &str = "multiplayer.disconnect.server_shutdown";
/// 收到关服请求之后最多再等一个还在跑的岛多久。正常的一刻是几毫秒到几十毫秒，
/// 卡顿的一刻也不过秒级；过了这么久还没回来就当它卡死了。
///
/// How long to keep waiting for a running island after a shutdown request. A
/// normal tick takes milliseconds and even a lag spike seconds; past this the
/// island is treated as stuck.
const ISLAND_STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// 等岛交回时每隔这么久醒一次，看有没有关服请求、要不要报告。
/// 不影响正常交回的延迟：岛一交回，等待就立刻结束。
///
/// While waiting for the island, wake this often to look for a shutdown
/// request or to report; a normal hand-back ends the wait at once.
const ISLAND_WAIT_SLICE: Duration = Duration::from_millis(100);
/// 岛一刻跑了这么久还没回来就记一条警告，之后每隔这么久再记一条。
///
/// Warn when an island tick has run this long, and again at this interval.
const ISLAND_SLOW_REPORT: Duration = Duration::from_secs(10);

/// 多阶段生成、又没显式配 `chunk_worker_threads` 时的默认线程数：取 CPU 核数的一半，
/// 夹在 1..=4。真生成每区块几十毫秒，留几条线程就不至于把 20 TPS 的世界线程拖住；
/// 上限 4 是照 31 GB 内存、同时还有别的队员在跑的机器留的余量。
///
/// Default worker threads for staged generation when `chunk_worker_threads` is
/// unset: half the CPUs, clamped to 1..=4. Real generation costs tens of ms per
/// chunk, so a few threads keep it off the 20 TPS world thread; the cap of 4
/// leaves room on a machine shared with other workers.
fn default_staged_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(1, 4))
        .unwrap_or(2)
}

/// 世界的启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldConfig {
    /// 服务端视距，合法范围是 2 到 32。
    pub view_distance: i32,
    /// 写进 `login` 包的玩家上限。满员由连接层判断，世界线程不看它。
    pub max_players: i32,
    /// 服务端有没有开正版验证。只写进 `login` 包，验证本身在连接层做。
    pub online_mode: bool,
    /// 官方 `enforce-secure-profile`，默认打开。没开正版验证、或者公钥还没取到时不生效。
    ///
    /// Official `enforce-secure-profile`, on by default. It does nothing until online mode is on
    /// and the profile keys have been fetched.
    pub enforce_secure_profile: bool,
    /// 启动时读到的管理名单，成为世界线程里的权威副本。
    pub admin: crate::admin::AdminSnapshot,
    /// 实体阶段的线程数。0 = 精确驱动（和官方逐位一致）；≥ 1 = 并行驱动。
    pub entity_threads: usize,
    /// 每只生物每刻最多处理几次推挤（0 = 不限，精确模式）。
    pub max_entity_collisions: usize,
    /// 三档降频的两个距离边界（0 = 关，精确模式）：降频档起点、休眠档起点。
    pub mob_throttle_distance: i32,
    pub mob_sleep_distance: i32,
    /// 每刻唤醒名额的倍率（1 = Paper 的缺省值）。
    pub mob_wakeups_scale: usize,
    /// 独立寻路阶段（实验，缺省关）：请求挂起一刻，阶段末尾统一去重 / 排优先级 / 按预算算。
    /// 不是官方游戏规则，不进存档、不同步客户端。
    pub mob_pathfinding_phase: bool,
    /// 每个岛每刻最多展开的 A* 节点数；0 = 用缺省预算。只在独立寻路阶段打开时有意义。
    pub mob_pathfinding_budget: u32,
    /// 寻路失败冷却（Paper 的连续 10 次失败后 40 刻不再试）。
    pub mob_pathfinding_fail_cooldown: bool,
    /// 被困村民降频（Purpur lobotomize，缺省关）：间隔与是否等待交易锁定。
    pub villager_lobotomize: bool,
    pub villager_lobotomize_interval: i32,
    pub villager_lobotomize_wait_trade_lock: bool,
    /// 睡觉村民大脑运行间隔（刻，0 = 关，精确模式）。
    pub villager_sleep_brain_interval: i32,
    /// 红石缓存（实验）：结果和官方逐位一致，对大型红石电路有效。默认关。
    pub redstone_cache: bool,
    /// 区块计算池的线程数。0 = 内联执行器（解析、生成、序列化在世界线程上做）；
    /// ≥ 1 = 常驻后台线程。区块内容和线程数无关，只有交付的时刻可能不同。
    pub chunk_worker_threads: usize,
    /// 世界生成两份全进程缓存的上限和空闲释放秒数（`bamboo.toml` 的三个键）。
    /// 只影响内存和速度，不影响生成结果。
    ///
    /// Limits and idle-release delay of the two process-wide worldgen caches
    /// (three `bamboo.toml` keys). They affect memory and speed, never output.
    pub worldgen_cache: crate::worldgen_cache::WorldgenCacheSettings,
    /// 自然刷怪按玩家算上限（默认开）；关掉只剩官方的全局上限。
    ///
    /// Per-player natural spawn caps (on by default); off keeps only the
    /// vanilla global cap.
    pub spawn_per_player_cap: bool,
    /// Director 信箱每刻交付/收回的区块数上限，0 表示不限。
    pub mail_budget: MailBudget,
    /// 岛的 tick 线程数（岛的实施计划第 6 节第 7 步）。0 = 齐步：岛在世界线程上内联跑，
    /// 精确模式，逐位一致；≥ 1 = 分离：岛的一刻在这么多条 tick 线程上跑，和主控的
    /// Director 推进并行，区块交付晚一刻。缺省见 [`crate::default_island_threads`]；
    /// 要求逐位一致的地方显式给 0。
    ///
    /// Island tick threads (island plan, section 6, step 7). 0 is lockstep: the
    /// island runs inline on the world thread, exact mode, bit for bit. 1 or
    /// more is detached: the island's tick runs on that many tick threads in
    /// parallel with the master's Director step, and chunks arrive one tick
    /// later. See [`crate::default_island_threads`] for the default; bit-exact
    /// callers give 0 explicitly.
    pub island_threads: usize,
    /// 多岛（岛的实施计划第 6 节第 10 步）：分区器把相距很远的玩家群分成各自的岛，
    /// 岛按需拆开、合并，各岛分别推进。`false` = 整个世界一个岛（精确模式、对拍、要求
    /// 逐位一致的测试）。缺省见 [`crate::default_islands`]；要求逐位一致的地方显式给 `false`。
    ///
    /// Multi-island mode (island plan, step 10): the partitioner puts distant
    /// groups of players on their own islands, splits and merges them on
    /// demand, and each island advances on its own. `false` keeps the whole
    /// world one island (exact mode, oracles, bit-exact tests). See
    /// [`crate::default_islands`] for the default; bit-exact callers give `false`.
    pub islands: bool,
    /// 多岛的混合驱动下，一个岛的同一刻在 tick 线程上连续跑超过这么久就判为死岛
    /// （`docs/island-implementation-plan.md` 5.6）：它的玩家被断开，它的区块从上次存盘
    /// 重新加载，世界接着跑。缺省见 [`crate::default_island_dead_timeout`]（30 秒）。
    ///
    /// Under the multi-island hybrid drive, an island whose tick keeps
    /// running on its thread for longer than this is declared dead
    /// (`docs/island-implementation-plan.md` 5.6): its players are
    /// disconnected, its chunks reload from the last save and the world goes
    /// on. See [`crate::default_island_dead_timeout`] for the default (30 s).
    pub island_dead_timeout: Duration,
    /// 世界目录。`None` 表示不存档（测试、oracle、压测都走这条路）。
    ///
    /// The world directory. `None` disables saving (tests, oracle, load tests).
    pub world_dir: Option<PathBuf>,
    /// 启动时给世界规则表里的一条整数规则定值（只用 `respawn_radius`）。
    /// `None` 用官方缺省；需要固定出生点的嵌入式测试显式设成 0。
    /// An optional initial integer game rule value for the world (only
    /// `respawn_radius` is used). `None` keeps the official default; embedded
    /// tests that need a fixed spawn explicitly set 0.
    pub spawn_radius: Option<i32>,
    /// 世界生成参数（种子、世界类型、flat 预设、是否生成结构）。
    /// 默认超平坦、种子 0，保持嵌入式测试的现有行为。
    ///
    /// World generation parameters (seed, level type, flat preset, structures).
    /// Defaults to superflat / seed 0 to keep embedded-test behaviour.
    pub world_gen: crate::world_options::WorldGenConfig,
    /// 官方 `server.properties` 的 `broadcast-console-to-ops`，控制台命令反馈广播给 OP 用。
    ///
    /// Official `server.properties` `broadcast-console-to-ops`, for console-feedback
    /// broadcasts to ops.
    pub broadcast_console_to_ops: bool,
    /// 压测场景接线，只给 `mc-bamboo-loadtest serve` 用，默认 `None`。
    /// `None` 时每刻只多一次 `Option` 判断，游戏行为完全不变。
    ///
    /// Load-test scenario hooks, used only by `mc-bamboo-loadtest serve`.
    /// `None` keeps the world loop unchanged apart from one `Option` check.
    #[doc(hidden)]
    pub bench: Option<crate::bench::BenchHooks>,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            view_distance: 8,
            max_players: 20,
            online_mode: false,
            enforce_secure_profile: true,
            admin: crate::admin::AdminSnapshot::default(),
            entity_threads: 0,
            max_entity_collisions: 0,
            mob_throttle_distance: 0,
            mob_sleep_distance: 0,
            mob_wakeups_scale: 1,
            mob_pathfinding_phase: false,
            mob_pathfinding_budget: 0,
            mob_pathfinding_fail_cooldown: false,
            villager_lobotomize: false,
            villager_lobotomize_interval: 100,
            villager_lobotomize_wait_trade_lock: true,
            villager_sleep_brain_interval: 0,
            redstone_cache: false,
            chunk_worker_threads: 0,
            worldgen_cache: crate::worldgen_cache::WorldgenCacheSettings::default(),
            spawn_per_player_cap: true,
            mail_budget: MailBudget::default(),
            island_threads: crate::island::default_island_threads(),
            islands: crate::island::default_islands(),
            island_dead_timeout: crate::island::default_island_dead_timeout(),
            world_dir: None,
            world_gen: crate::world_options::WorldGenConfig::flat_template(),
            spawn_radius: None,
            broadcast_console_to_ops: true,
            bench: None,
        }
    }
}

/// 所有游戏状态的唯一所有者：主控一份，岛一份（现在只有一个岛）。
///
/// 第 1 步把字段按以后的归属分开（`docs/island-implementation-plan.md` 第 2 节、第 6 节
/// 第 1 步）；第 7 步起岛放在 [`IslandSlot`] 里，分离驱动下每刻整个交给 tick 线程跑完再交回。
///
/// The sole owner of all game state: one master and one island (only one
/// island for now). Step 1 regrouped the fields by owner; since step 7 the
/// island lives in an [`IslandSlot`] and, under the detached drive, is handed
/// whole to a tick thread every tick and handed back.
pub struct World {
    pub(crate) master: Master,
    /// 全部岛。`Deref` 到家岛：只有一个岛时（`islands = false`）它就是唯一的岛。
    ///
    /// Every island. Derefs to the home island, the only one with `islands = false`.
    pub(crate) islands: Islands,
}

/// 主控：全局状态、入站通道、节拍、Director 宿主、票中心、存盘会话和压测钩子。
/// 跑在世界线程（主控线程）上，20 TPS。只有一个岛时主控每刻等岛交回才进下一段串行的活
/// （收包、天气要停放的岛），所以主控和岛共用一个节拍；岛落后时两者一起按 2 秒内连续
/// 追赶（岛的实施计划 5.2、第 6 节第 7 步）。
///
/// The master: global state, inbound channel, pacing, the Director host,
/// ticket centres, the save session, and load-test hooks. It runs on the
/// world (master) thread at 20 TPS. With one island the master waits for the
/// hand-back before its next serial part (inbound messages and weather need
/// the parked island), so master and island share one schedule and catch up
/// together within 2 seconds (island plan 5.2, step 7).
pub(crate) struct Master {
    pub(crate) global: Global,
    /// 世界目录，写进度档案时用；`None` 表示不存档。
    pub(crate) world_dir: Option<std::path::PathBuf>,
    /// 上一刻全部岛交回的睡眠计数；主控在这一刻开头、天气之后据此决定跳早。
    /// 官方 `ServerLevel.tick` 的 `areEnoughSleeping` / `areEnoughDeepSleeping`
    /// 也是在天气之后、时间推进之前读上一刻的玩家状态。
    ///
    /// Sleep counts from all islands at the previous hand-back. The master
    /// uses them at the start of this tick, after weather and before time,
    /// exactly where `ServerLevel.tick` reads the previous tick's players.
    pub(crate) sleep: crate::sleep::SleepCounts,
    /// 主控队列（岛的实施计划第 6 节第 4 步）。玩家自己的队列挂在玩家上。
    inbox: InboundReceiver,
    /// 进服被拒的连接还会发请求、最后发 `Leave`：它们的队列留在这里照常取空、
    /// 按序号处理（请求因为链路已关而被忽略），收到 `Leave` 时扔掉。
    /// 这样连接层看到的和所有消息走一条通道时一样，发送不会失败。
    ///
    /// Queues of connections refused at join. They may still send requests and
    /// finally `Leave`; the queues are drained and handled by number as usual
    /// (the requests are ignored because the link is closed) and dropped on
    /// `Leave`, so the connection sees the same as with a single channel.
    refused_inboxes: Vec<(PlayerId, PlayerInbox)>,
    /// 睡到下一刻用的节拍器。
    pacer: Pacer,
    /// 只有连接层起的世界才用权威 OP 名单重算进服权限。
    /// 直接嵌 `World::new` 的测试/工具按 `JoinRequest` 传入的等级走。
    authoritative_admin: bool,
    /// 区块平面的 Director 和它的宿主（内联执行器、停放表、region 消息）。
    pub(crate) host: DirectorHost,
    /// 上一刻每名玩家所在的区块，用来只在跨区块时发 `PlayerMoved`。
    ticket_centers: std::collections::BTreeMap<PlayerId, ChunkPos>,
    /// 飞行中的末影珍珠：`uid -> (上次挂票的区块, 距下次续票的刻数)`。
    /// 官方 `ThrownEnderpearl.ticketTimer` 落在实体上，这里放在主控的票表里。
    ///
    /// In-flight ender pearls: `uid -> (chunk last ticketed, ticks to refresh)`.
    /// Vanilla stores `ticketTimer` on the entity; the master keeps it here.
    ender_pearl_tickets: std::collections::BTreeMap<i32, (ChunkPos, i32)>,
    /// 每名玩家上一次跨区块的方向（只取符号）。需求快照里方向未知时退化为纯距离。
    player_directions: std::collections::BTreeMap<PlayerId, (i32, i32)>,
    /// 上次喂给 Director 的需求快照，位置或方向变了才重算。
    player_demand: Option<DemandSnapshot>,
    /// Director 的逻辑刻数，每次 `tick_ticket_timeouts` 加一。
    pub(crate) director_tick: u64,
    /// 存档会话。`None` 表示这一局不存档。
    save: Option<SaveSession>,
    /// 启动阶段打开存档失败的原因；非空时 `run` 直接拒绝启动。
    startup_error: Option<String>,
    /// 等着某道 IO 栅栏回执再发成功消息的 `save-all flush`：`(栅栏序号, 玩家)`。
    /// `save-all flush` success messages waiting on an IO barrier: `(seq, player)`.
    pending_save_success: Vec<(u64, PlayerId)>,
    /// 压测回调的「场景搭好了没有」和回调本身；默认配置下不被读取。
    bench_hooks: Option<crate::bench::BenchHooks>,
    bench_ready: bool,
    /// 岛写全局状态用哪种实现（岛的实施计划第 6 节第 5b 步）。缺省直接实现；
    /// 单元测试可以整体切成排队实现。
    ///
    /// Which port implementation island writes to global state use (island
    /// plan, section 6, step 5b). Direct by default; unit tests can switch the
    /// whole run to the queued one.
    pub(crate) port_mode: PortMode,
    /// 齐步还是分离（[`IslandDrive`]），由 `island_threads` 定。
    pub(crate) drive: IslandDrive,
    /// 分离驱动的 tick 线程池；齐步时是 `None`。
    ///
    /// The detached drive's tick-thread pool; `None` in lockstep.
    pool: Option<IslandPool>,
    /// 实体并行算阶段的常驻线程池。启动时按 `entity_threads` 建好，所有岛共用；
    /// 精确模式（`entity_threads = 0`）和单线程时为 `None`。
    entity_pool: Option<mc_bamboo_blocks::entity::parallel::WorkerPool>,
    /// 等岛交回期间先取出来的主控队列消息（看有没有关服请求），下一刻照常按序号处理。
    ///
    /// Master-queue messages taken out while waiting for the island (to see a
    /// shutdown request); they are handled by number next tick as usual.
    held_inbound: Vec<Sequenced>,
    /// 收到关服请求之后最多再等岛交回多久；过了就不等它，能存的存下来、报告后退出。
    ///
    /// How long to keep waiting for the island after a shutdown request; past
    /// that the master stops without it, saving what it can and reporting.
    pub(crate) island_stop_timeout: Duration,
    /// 一个岛的同一刻在 tick 线程上连续跑超过这么久就判死（混合驱动，岛的实施计划
    /// 5.6.1）。
    ///
    /// An island whose tick runs on its thread for longer than this is
    /// declared dead (hybrid drive, island plan 5.6.1).
    pub(crate) island_dead_timeout: Duration,
    /// 这一刻主控等岛交回、什么也没干的时间。
    island_wait: Duration,
    /// 分离驱动下岛还在跑时收到的 `/save-all`：等岛交回、停放之后再过栅栏。
    ///
    /// `/save-all` requests that arrived while a detached island was running:
    /// they pass the barrier once the island has handed back and parked.
    deferred_save_all: Vec<(bool, PlayerId)>,
    /// 分离驱动下岛开头收信用的信件列表，跨刻复用容量。
    posted_scratch: Vec<Posted>,
    /// 多岛开着（`WorldConfig::islands`，且不是无限超平坦）。
    ///
    /// Multi-island mode is on (`WorldConfig::islands`, and not infinite flat).
    islands_mode: bool,
    /// 正在跨岛的玩家：已经站到别的岛的区块上、原来的岛还没把他的视野和配对清干净，
    /// 记着从哪一刻（`director_tick`）开始等（[`World::relocate_players`]）。
    ///
    /// Players crossing islands: already standing on another island's chunk,
    /// waiting for the old island to clear their view and pairings, with the
    /// tick (`director_tick`) the wait began.
    transit: std::collections::BTreeMap<PlayerId, u64>,
    /// 落到别的岛的区块上、还没搬过去的实体所在的岛（[`World::move_stray_entities`]）：那一刻
    /// 它或者目的地的回执还没交回，拆不了，留到下一次照做拆合时再搬。
    ///
    /// Islands with entities that landed on another island's chunk and are not
    /// moved yet: that island or the destination still had acks out, so the
    /// move waits for the next settle.
    stray_sources: std::collections::BTreeSet<mc_bamboo_world::director::RegionId>,
    /// 混合驱动的状态（计划 5.5）：每个不在手里的岛的出站队列、排着队的事、正在进服的
    /// 玩家、留给不在的岛的存盘名单等。别的驱动下一直是空的。
    ///
    /// The hybrid drive's state (plan 5.5): the outbound queue of every island
    /// that is away, held work, players still joining, save lists kept for
    /// absent islands, and so on. Always empty under the other drives.
    pub(crate) hybrid: island_hybrid::HybridState,
    /// 主控自己的剖析器。混合驱动下打报告的就是它：主控的各段记在这里，各岛的样本在岛
    /// 交回时（子段）和打报告前（其余）并进来，每个岛另起一行（计划第 2 节第 26 行）。
    /// 别的驱动下打报告的那份跟着家岛，这一份只在家岛不在主控手里时（关服时没等到卡住
    /// 的岛）接住主控这一段，不进报告。
    ///
    /// The master's own profiler. Under the hybrid drive it is the reporting
    /// one: the master's parts are recorded here and every island's samples
    /// are folded in at hand-back (sub-phases) and before each report (the
    /// rest), with a line per island (plan section 2, row 26). Under the
    /// other drives the reporting profiler travels with the home island and
    /// this one only catches the master's part while home is away (stuck at
    /// shutdown), unreported.
    profiler: Box<crate::profiler::Profiler>,
    /// 测试专用：运行时拆岛时故意漏搬这一类（改坏确认）。
    #[cfg(test)]
    pub(crate) split_skip: Option<crate::region::IslandPart>,
    /// 测试专用：照做拆分时故意按「瞬时位置」而不是跟踪的视野中心给玩家分边
    /// （改坏确认，模拟用做决定那一刻的名单）。正式构建里不存在。
    ///
    /// Test only: assign players to a split by instantaneous position instead
    /// of by their tracked view centre (break-it check, modelling the
    /// decision-time roster). Absent in production builds.
    #[cfg(test)]
    pub(crate) split_by_position: bool,
    /// 测试专用：拆合之后故意弄坏新生成实体的状态（改坏确认，见
    /// `world/island_runtime_tests.rs`）。`LoseEntity` 丢掉一个实体，`LoseTracking`
    /// 清掉一个新实体的追踪配对。
    ///
    /// Test only: corrupt freshly spawned entities after a split or merge
    /// (break-it checks, see `world/island_runtime_tests.rs`). `LoseEntity`
    /// drops one entity; `LoseTracking` clears one fresh entity's pairing.
    #[cfg(test)]
    pub(crate) split_break: Option<crate::world::island_runtime::FreshEntityBreak>,
    /// 测试专用：把第一次跳早决定推迟一刻（模拟「岛末尾判断、交回执行」的旧时机）。
    /// 正式构建里不存在。
    ///
    /// Test-only: delay the first morning-skip decision by one tick, to model
    /// the old "decide at island end, apply at hand-back" timing.
    #[cfg(test)]
    pub(crate) sleep_break_late: bool,
}

/// 对拍脚手架用：一个区块的 `PostProcessing`（每段 packed short），交付盒子里的和
/// 写盘再读回的各一份。
/// Oracle scaffolding: a chunk's `PostProcessing` (packed shorts per section),
/// as delivered and after a save round trip.
#[doc(hidden)]
pub type PostProcessingTables = (Vec<Vec<u16>>, Vec<Vec<u16>>);

/// 对拍脚手架用的一条计划刻：`(坐标, 是否流体, 注册表 ID, 触发时刻, 优先级)`。
/// Oracle scaffolding: one scheduled tick `(pos, is_fluid, id, trigger, priority)`.
#[doc(hidden)]
pub type ScheduledTickRow = (mc_bamboo_core::BlockPos, bool, i64, i64, i32);
impl World {
    /// 存盘时把各停放岛的 `Raids` 按岛编号汇总给 `raids.dat`。岛不在主控手里时
    /// 只收停放着的那些（和区块快照一致：没交回的岛不进这次存盘）。
    ///
    /// Collect every parked island's `Raids` for `raids.dat`. Islands still away
    /// from the master are skipped, like their chunk snapshots.
    fn raid_tables(&self) -> Vec<(u32, mc_bamboo_blocks::entity::raid::Raids)> {
        // `parked()` 只遍历停放着的岛；关服时岛还卡在 tick 线程上，用 `iter()`
        // 会因为 `IslandSlot` 的 Deref 断言而 panic。
        // `parked()` only visits islands at home; during shutdown an island can
        // still be away on its tick thread, where `iter()`'s Deref would panic.
        self.islands
            .parked()
            .map(|island| (island.id, island.region.raids.clone()))
            .collect()
    }

    /// 创建一个空世界。`config` 的视距超出合法范围时取最接近的值。
    pub fn new(config: WorldConfig, inbox: InboundReceiver) -> Self {
        Self::with_service_keys(config, inbox, secure_chat::empty_keys())
    }

    /// `keys` 和连接任务共用。世界线程只读，不去取。
    ///
    /// `keys` is shared with the connection tasks. The world thread only reads them.
    pub fn with_service_keys(
        config: WorldConfig,
        inbox: InboundReceiver,
        keys: Arc<ServiceProfileKeys>,
    ) -> Self {
        Self::build(config, inbox, keys, None, false)
    }

    /// 连接层把启动时读到的名单和发布变更的通道交给世界线程。
    /// 名单变更作为 owned 消息发给连接层，世界线程不会等它。
    pub fn with_admin(
        config: WorldConfig,
        inbox: InboundReceiver,
        keys: Arc<ServiceProfileKeys>,
        admin: crate::admin::AdminInit,
    ) -> Self {
        Self::build(config, inbox, keys, Some(admin), false)
    }

    fn build(
        config: WorldConfig,
        inbox: InboundReceiver,
        keys: Arc<ServiceProfileKeys>,
        admin: Option<crate::admin::AdminInit>,
        infinite_flat: bool,
    ) -> Self {
        let view_distance = config
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);
        let island_threads = config.island_threads.min(MAX_ISLAND_THREADS);
        // 启动阶段同步打开存档：建目录、拿锁、读 level.dat。失败就拒绝启动，
        // 不让世界线程带着一个没锁住的世界目录继续跑。
        // Open the save synchronously at startup: create the layout, take the lock,
        // read level.dat. On failure, refuse to start instead of running unlocked.
        let (mut save, startup_error) = match (&config.world_dir, infinite_flat) {
            (Some(root), false) => match SaveSession::open(root) {
                Ok((session, _source)) => (Some(session), None),
                Err(error) => {
                    let message = format!("could not open the world directory: {error}");
                    error!(%message, "refusing to start");
                    (None, Some(message))
                }
            },
            _ => (None, None),
        };
        let mut global = Global::default();
        let authoritative_admin = admin.is_some();
        if let Some(init) = admin {
            global.admin = init.snapshot.into_state();
            global.admin_messages = init.messages;
        }
        let mut region = Region::with_keys(
            view_distance,
            config.max_players,
            config.online_mode,
            config.enforce_secure_profile,
            keys,
        );
        // 实体 ID 整个世界共用一个分配器（岛的实施计划第 6 节第 2 步），在任何实体
        // 出现之前接上。现在只有一个岛，句柄只在它的实体表里；以后新建的岛从这张表
        // 取同一个句柄（`EntityStore::ids_handle`）。
        // One entity-id allocator for the whole world (island plan step 2),
        // attached before any entity exists. With one island the handle lives
        // only in its store; later islands clone it from there.
        region.use_entity_ids(mc_bamboo_blocks::EntityIds::new());
        // 控制台命令反馈的广播开关来自 `server.properties`，启动时带进世界线程。
        // The console-feedback broadcast flag comes from `server.properties` at startup.
        region.broadcast_console_to_ops = config.broadcast_console_to_ops;
        // 启动时把读到的 `level.dat` 世界设置、`Time`（游戏时间）和 `data/` 的
        // 游戏规则 / 世界边界 / 世界时钟套回运行时；边界的 `restore` 用存档里的
        // 游戏时间。全新世界没有 `level.dat`，运行时默认值才是权威（不能拿
        // `LevelData::default()` 的出生点覆盖它），时间也从 0 开始。
        // Apply loaded `level.dat` world settings and `Time` (game time) plus
        // `data/` game rules / border / world clocks back onto runtime; `restore`
        // uses the saved game time. A fresh world has no `level.dat`, so runtime
        // defaults are authoritative (do not overwrite them with
        // `LevelData::default()`'s spawn) and game time starts at 0.
        let fresh_world = save
            .as_ref()
            .is_none_or(|session| !session.has_loaded_level());
        if let Some(save) = &save {
            let game_time = if save.has_loaded_level() {
                crate::save_data::apply_level_data(global.state_mut(), save.level());
                global.restore_game_time(save.level().time);
                save.level().time
            } else {
                global.time().0
            };
            save.startup().apply(&mut global, &mut region, game_time);
        }
        // 有 `world_gen_settings.dat` 就以存档为准：空 `level-seed` 的随机种子在
        // 首次保存时写进存档，重启后必须用同一个（官方行为），不能重新摇一次。
        // `server.properties` 的世界类型只对新世界生效，改它对老存档无效。
        // A saved `world_gen_settings.dat` wins: the random seed for an empty
        // `level-seed` is written on first save, and a restart must reuse it
        // (vanilla behaviour) instead of rolling a new one. `level-type` from
        // `server.properties` only applies to a fresh world.
        let world_gen = save
            .as_ref()
            .and_then(|session| session.loaded_world_gen())
            .cloned()
            .unwrap_or_else(|| config.world_gen.clone());
        let staged = !infinite_flat
            && (!world_gen.level_type.is_flat()
                || world_gen.generate_structures
                || world_gen.flat != crate::world_options::FlatSettings::default());
        if let Some(save) = save.as_mut() {
            save.set_world_gen(world_gen.clone());
        }
        // 种子进 `WorldState`（`hashed_seed` 进 login 包、天气），按名随机序列在主控上
        // （岛的实施计划 3.3「岛的随机源」第 2 条）。`level.dat` 里没有建模的 seed 字段，
        // `apply_level_data` 不动它。
        // Put the seed into `WorldState` (hashed_seed for the login packet,
        // weather); the named random sequences live on the master (island plan
        // 3.3). `level.dat` has no modelled seed field, so `apply_level_data`
        // leaves it alone.
        global.state_mut().seed = world_gen.seed;
        if let Some(radius) = config.spawn_radius {
            let _ = global.rules_mut().assign(
                mc_bamboo_data::gamerules::RESPAWN_RADIUS,
                &radius.to_string(),
            );
        }
        global.random_sequences = mc_bamboo_loot::RandomSequences::new(world_gen.seed);
        // `ServerLevel.isFlat()` / `ChunkGenerator.getSeaLevel()` 和落脚点搜索
        // 都从发布的全局快照读这份参数（存档优先，见上面的 world_gen）。
        // `ServerLevel.isFlat()`, `ChunkGenerator.getSeaLevel()` and the landing
        // search all read this from the published snapshot (a save wins).
        global.world_gen = world_gen.clone();
        // 全新世界（没有 level.dat）按官方 `MinecraftServer.setInitialSpawn` 选出生点；
        // 读档时用存档里的出生点（已由 `apply_level_data` 套回）。超平坦保持不变。
        // A fresh world (no level.dat) picks a spawn like `setInitialSpawn`;
        // a loaded world keeps the saved spawn (already applied). Flat is unchanged.
        if fresh_world
            && staged
            && let Some(spawn) = crate::worldgen_spawn::initial_spawn(&world_gen)
        {
            global.state_mut().spawn = spawn;
        }
        let director_config = DirectorConfig {
            pipeline: if staged {
                PipelineKind::Staged
            } else {
                PipelineKind::Flat
            },
            ..DirectorConfig::default()
        };
        let mut world = Self {
            master: Master {
                global,
                world_dir: config.world_dir.clone(),
                sleep: crate::sleep::SleepCounts::default(),
                inbox,
                refused_inboxes: Vec::new(),
                pacer: Pacer,
                authoritative_admin,
                host: if infinite_flat {
                    DirectorHost::new_infinite_flat(Default::default())
                } else {
                    DirectorHost::new(director_config)
                },
                ticket_centers: std::collections::BTreeMap::new(),
                ender_pearl_tickets: std::collections::BTreeMap::new(),
                player_directions: std::collections::BTreeMap::new(),
                player_demand: None,
                director_tick: 0,
                save,
                startup_error,
                pending_save_success: Vec::new(),
                bench_hooks: config.bench,
                bench_ready: false,
                port_mode: PortMode::default_for_world(),
                drive: IslandDrive::from_threads(island_threads),
                pool: None,
                entity_pool: None,
                held_inbound: Vec::new(),
                island_stop_timeout: ISLAND_STOP_TIMEOUT,
                island_dead_timeout: config.island_dead_timeout,
                island_wait: Duration::ZERO,
                deferred_save_all: Vec::new(),
                posted_scratch: Vec::new(),
                islands_mode: config.islands && !infinite_flat,
                transit: std::collections::BTreeMap::new(),
                stray_sources: std::collections::BTreeSet::new(),
                hybrid: island_hybrid::HybridState::default(),
                profiler: Box::default(),
                #[cfg(test)]
                split_skip: None,
                #[cfg(test)]
                split_by_position: false,
                #[cfg(test)]
                split_break: None,
                #[cfg(test)]
                sleep_break_late: false,
            },
            islands: {
                let budget = MailBudget {
                    deliveries: config.mail_budget.deliveries.min(MAX_MAIL_BUDGET),
                    reclaims: config.mail_budget.reclaims.min(MAX_MAIL_BUDGET),
                };
                // 直接在堆上建：`Region` 很大，调试构建里按值多搬一次就多占一份栈。
                // Build on the heap: `Region` is large and every by-value move
                // costs stack in debug builds.
                let mut island = Box::new(Island::new(HOME_REGION, region, budget));
                island.entity_threads = config.entity_threads.min(MAX_ENTITY_THREADS);
                Islands::new(island)
            },
        };
        // 实体算阶段的常驻池：启动时建好，按 `entity_threads` 派活。起不来就退回
        // 精确模式并拒绝启动，不让世界线程每刻临时起线程。
        let entity_threads = world.islands.entity_threads;
        if entity_threads > 1 {
            match mc_bamboo_blocks::entity::parallel::WorkerPool::new(
                "bamboo-entity",
                entity_threads,
            ) {
                Ok(pool) => world.master.entity_pool = Some(pool),
                Err(error) => {
                    let message = format!("could not start the entity worker pool: {error}");
                    error!(%message, "refusing to start");
                    world.master.startup_error = Some(message);
                    world.islands.entity_threads = 0;
                }
            }
        }
        if island_threads > 0 {
            match IslandPool::new(island_threads, run_detached_island) {
                Ok(pool) => {
                    debug!(
                        island_threads,
                        "the island ticks on its own threads, beside the master (detached drive)"
                    );
                    world.master.pool = Some(pool);
                }
                Err(error) => {
                    let message = format!("could not start the island tick threads: {error}");
                    error!(%message, "refusing to start");
                    world.master.startup_error = Some(message);
                }
            }
        }
        world
            .master
            .host
            .director_mut()
            .tickets_mut()
            .set_view_distance(view_distance);
        world
            .master
            .host
            .director_mut()
            .tickets_mut()
            .set_simulation_distance(crate::interaction::SIMULATION_DISTANCE);
        if infinite_flat {
            world.islands.region.use_infinite_flat_chunks();
        }
        if config.redstone_cache {
            world.islands.region.redstone_cache =
                mc_bamboo_blocks::cache::RedstoneCache::new(mc_bamboo_blocks::cache::CacheConfig {
                    enabled: true,
                    ..Default::default()
                });
        }
        world.islands.region.spawn_per_player_cap = config.spawn_per_player_cap;
        world.islands.region.max_entity_collisions =
            config.max_entity_collisions.min(MAX_ENTITY_COLLISIONS);
        world.islands.region.mob_throttle_distance = config
            .mob_throttle_distance
            .clamp(0, MAX_MOB_DISTANCE as i32);
        world.islands.region.mob_sleep_distance =
            config.mob_sleep_distance.clamp(0, MAX_MOB_DISTANCE as i32);
        world.islands.region.mob_wakeups_scale =
            config.mob_wakeups_scale.min(MAX_MOB_WAKEUPS_SCALE);
        world.islands.region.mob_pathfinding_phase = config.mob_pathfinding_phase;
        world.islands.region.mob_pathfinding_budget =
            config.mob_pathfinding_budget.min(MAX_PATHFINDING_BUDGET);
        world.islands.region.mob_pathfinding_fail_cooldown = config.mob_pathfinding_fail_cooldown;
        world.islands.region.villager_lobotomize = config.villager_lobotomize;
        world.islands.region.villager_lobotomize_interval = config
            .villager_lobotomize_interval
            .clamp(1, MAX_VILLAGER_BRAIN_INTERVAL);
        world.islands.region.villager_lobotomize_wait_trade_lock =
            config.villager_lobotomize_wait_trade_lock;
        world.islands.region.villager_sleep_brain_interval = config
            .villager_sleep_brain_interval
            .clamp(0, MAX_VILLAGER_BRAIN_INTERVAL);
        // 读盘恢复结构起点时要和生成器用同一个种子 / 世界类型。
        world.master.host.set_world_gen_config(world_gen.clone());
        crate::worldgen_cache::configure(config.worldgen_cache);
        if staged {
            // 真生成比超平坦模板慢得多，默认就用计算池把它挪出世界线程；配置文件
            // 显式给了 `chunk_worker_threads` 就照给的值（0 = 仍然内联）。
            // Real generation is far slower than the superflat template, so by
            // default push it off the world thread with the compute pool; an
            // explicit `chunk_worker_threads` value is honoured (0 = still inline).
            world
                .master
                .host
                .set_generator(Arc::new(OverworldGenerator::new(world_gen.clone())));
        }
        let compute_threads = if staged && config.chunk_worker_threads == 0 {
            default_staged_worker_threads()
        } else {
            config.chunk_worker_threads
        };
        world
            .master
            .host
            .set_compute_threads(compute_threads.min(MAX_CHUNK_WORKER_THREADS));
        // 启用存档时起一条 IO 线程并接进 Director 宿主；起不来也拒绝启动。
        // With saving enabled, start one IO thread and attach it to the host.
        if let Some(save) = &world.master.save {
            match ChunkIo::open(save.dir()) {
                Ok(io) => world.master.host.set_io(io),
                Err(error) => {
                    let message = format!("could not start the chunk IO thread: {error}");
                    error!(%message, "refusing to start");
                    world.master.startup_error = Some(message);
                }
            }
        }
        // 岛的时钟从世界时间起算（岛的实施计划 5.1）。放在启动数据都套上之后，
        // 从存档恢复的游戏时间自然对得上。
        // The island clock starts at the world time (island plan 5.1), after
        // all startup data is applied so the restored game time is picked up.
        let time = world.master.global.time();
        world.islands.region.start_clock(time);
        world.master.hybrid.max_players = world.islands.region.max_players();
        world.master.global.publish(&mut world.islands.region);
        world.islands.region.profiler.on_world_start();
        world
            .master
            .host
            .ensure_islands(&world.islands.region.profiler);
        if world.master.islands_mode {
            let verify = world.islands.region.profiler.islands_verify();
            world.master.host.enable_live_islands(verify);
            // 多岛时天气（自然推进和 `/weather` 的时长）用主控自己的随机源（计划 3.3
            // 「岛的随机源」第 4 条、8.2 第 6 条），不碰任何岛的。
            // With several islands the weather (its cycle and `/weather`
            // durations) uses the master's own random source (plan 3.3, 8.2 item 6).
            world.master.global.weather_random = Some(mc_bamboo_blocks::random::LevelRandom::new(
                crate::region::unique_seed(),
            ));
        }
        if world.hybrid() {
            // 混合驱动留一份空白岛：家岛被判死时拿它换一个空的家岛（计划 5.6.2）。
            // The hybrid drive keeps a blank island to replace a dead home
            // with an empty one (plan 5.6.2).
            world.islands.keep_blueprint();
            // 混合驱动下打报告的是主控自己的剖析器，开关和报告间隔照家岛的。
            // Under the hybrid drive the master's own profiler reports, with
            // the home island's switches and report interval.
            *world.master.profiler = world.islands.region.profiler.fresh_like();
        }
        if world.islands.region.profiler.partition_enabled() {
            let verify = world.islands.region.profiler.partition_verify();
            world.islands.region.partitioning.enable(
                &crate::partition_stats::HALOS,
                verify,
                crate::region::partition::QUIET_TICKS,
            );
        }
        world
    }

    /// **测试和 oracle 专用**：创建一个「无限超平坦」世界，每个区块位置都隐式存在。
    ///
    /// 正式入口（[`new`](Self::new)、[`with_admin`](Self::with_admin)）一律用已加载模式，
    /// 区块由 Director 按 ticket 交付。这个退化实现只给不需要加载语义的单元测试、
    /// 集成测试和 oracle 用；不要在生产代码里调用。
    pub fn new_infinite_flat_for_tests(config: WorldConfig, inbox: InboundReceiver) -> Self {
        Self::build(config, inbox, secure_chat::empty_keys(), None, true)
    }

    /// 打报告的剖析器（见 [`report_profiler`]）。
    fn profiler(&mut self) -> &mut crate::profiler::Profiler {
        let hybrid = self.hybrid();
        report_profiler(&mut self.islands, &mut self.master.profiler, hybrid)
    }

    /// 票表的只读视图，给测试核对玩家票和等级用。
    #[cfg(test)]
    pub(crate) fn tickets(&self) -> &mc_bamboo_world::ticket::TicketTable {
        self.master.host.director().tickets()
    }

    /// 测试专用：直接改票表，跳过 `TicketOp` 的事件路径。
    #[cfg(test)]
    pub(crate) fn tickets_mut(&mut self) -> &mut mc_bamboo_world::ticket::TicketTable {
        self.master.host.director_mut().tickets_mut()
    }

    /// 测试专用：走一批 `TicketOp`。
    #[cfg(test)]
    pub(crate) fn apply_ticket(&mut self, op: TicketOp) {
        self.apply_ticket_ops([op]);
    }

    /// 一批票的操作。已加载模式交给 Director（会派加载/卸载任务），
    /// 无限超平坦模式只记票、不派任务。
    pub(crate) fn apply_ticket_ops(&mut self, ops: impl IntoIterator<Item = TicketOp>) {
        let ops: Vec<TicketOp> = ops.into_iter().collect();
        if ops.is_empty() {
            return;
        }
        for op in &ops {
            if let TicketOp::PlayerMoved { id, to } = op {
                let player = PlayerId(*id as i32);
                match to {
                    Some(pos) => self.master.ticket_centers.insert(player, *pos),
                    None => self.master.ticket_centers.remove(&player),
                };
            }
        }
        self.master.host.tickets(
            &mut island_access(self.master.drive, &mut self.islands),
            ops,
        );
        self.settle_islands();
    }

    /// 每刻在玩家跟踪阶段之前调用：只在玩家跨区块时发 `PlayerMoved`。
    fn sync_player_tickets(&mut self) {
        let mut moves = Vec::new();
        let mut changed = false;
        // 所有岛上的玩家按编号排（只有一个岛时就是那个岛的玩家表的顺序）。
        // Every island's players by id (one island: its own table order).
        // 混合驱动下只看停放着的岛；不在主控手里的岛上的玩家这一刻不动票。
        // Under the hybrid drive only parked islands are read; players on
        // islands that are away keep their tickets this tick.
        let mut players: Vec<(PlayerId, ChunkPos)> = self
            .islands
            .parked()
            .flat_map(|island| {
                island
                    .region
                    .players
                    .iter()
                    .map(|(&id, player)| (id, player.chunk_pos()))
            })
            .collect();
        players.sort_unstable_by_key(|&(id, _)| id);
        for &(id, at) in &players {
            if self.master.ticket_centers.get(&id) == Some(&at) {
                continue;
            }
            if let Some(previous) = self.master.ticket_centers.insert(id, at) {
                let direction = ((at.x - previous.x).signum(), (at.z - previous.z).signum());
                if direction != (0, 0) {
                    self.master.player_directions.insert(id, direction);
                }
            }
            changed = true;
            moves.push(TicketOp::PlayerMoved {
                id: u64::from(id.0 as u32),
                to: Some(at),
            });
        }
        // 票还在、人不在了的才算离开：不在主控手里的岛上的玩家和正在进服（票已经挂上、
        // 目标岛还没停放）的玩家都还在（计划 5.5.2）。
        // Only players who are really gone count as left: players on islands
        // that are away and players still joining (ticket placed, target
        // island not parked yet) are present (plan 5.5.2).
        let online = self.islands.player_ids();
        let mut left = Vec::new();
        for &id in self.master.ticket_centers.keys() {
            if online.binary_search(&id).is_err() && !self.master.hybrid.joining.contains(&id) {
                left.push(id);
            }
        }
        for id in left {
            self.master.ticket_centers.remove(&id);
            self.master.player_directions.remove(&id);
            changed = true;
            moves.push(TicketOp::PlayerMoved {
                id: u64::from(id.0 as u32),
                to: None,
            });
        }
        if !moves.is_empty() {
            self.apply_ticket_ops(moves);
        }
        // `ThrownEnderpearl.tick` 的区块票：把本刻所有停放岛上的飞行珍珠收集起来，
        // 跨区块 / 计时到点重挂，消失的撤票。珍珠静滞室靠它继续 tick。
        let mut live = std::collections::BTreeMap::new();
        for island in self.islands.parked() {
            live.extend(crate::ender_pearl::live_pearls(&island.region));
        }
        let pearl_ops =
            crate::ender_pearl::sync_tickets(&live, &mut self.master.ender_pearl_tickets);
        if !pearl_ops.is_empty() {
            self.apply_ticket_ops(pearl_ops);
        }
        if changed {
            self.publish_player_demand();
        }
    }

    /// 把「每名玩家在哪个区块、朝哪边走」喂给 Director 和宿主。
    /// 只在跨格或有人进出时调，位置没变时不重排任何队列。
    fn publish_player_demand(&mut self) {
        let players: Vec<PlayerDemand> = self
            .master
            .ticket_centers
            .iter()
            .map(|(&id, &center)| PlayerDemand {
                id: u64::from(id.0 as u32),
                center,
                direction: self
                    .master
                    .player_directions
                    .get(&id)
                    .copied()
                    .unwrap_or((0, 0)),
            })
            .collect();
        let demand = DemandSnapshot::new(players);
        if self.master.player_demand.as_ref() == Some(&demand) {
            return;
        }
        self.master.player_demand = Some(demand.clone());
        self.master.host.set_player_demand(demand);
    }

    /// 超时票每刻递减。`is_busy` 现在恒为假（区块都隐式存在），
    /// 提交 5 接 Director 后按「有在途任务/未交付」填。
    fn tick_ticket_timeouts(&mut self) {
        self.master
            .director_step(&mut island_access(self.master.drive, &mut self.islands));
        self.finish_partition_tick();
        self.settle_islands();
    }

    /// 分区器这一刻收尾，统计写进岛的剖析器（打报告的那份）。
    fn finish_partition_tick(&mut self) {
        if self.master.host.islands_enabled() {
            let hybrid = self.hybrid();
            self.master.host.finish_island_tick(
                report_profiler(&mut self.islands, &mut self.master.profiler, hybrid),
                self.master.director_tick,
            );
        }
    }

    /// 阻塞运行，直到收到 [`Inbound::Shutdown`] 或者所有发送端都没了。
    ///
    /// 节拍按截止时间排：到点就跑一刻（分离驱动下这一刻里把岛派给 tick 线程），落后 2 秒
    /// 以内连续追赶，再多就放弃追赶、从现在重新计时（`Schedule`）。
    ///
    /// Run until [`Inbound::Shutdown`] or until every sender is gone. Ticks
    /// are scheduled by deadline (the detached drive dispatches the island to
    /// a tick thread within the tick); up to 2 seconds behind is caught up back
    /// to back, beyond that the schedule restarts from now (`Schedule`).
    pub fn run(mut self) {
        if let Some(message) = &self.master.startup_error {
            error!(%message, "world startup failed; not running");
            return;
        }
        let mut timing = WallTiming;
        let mut schedule = Schedule::starting_at(self.profiler().now());
        // 混合驱动（计划 5.5）：每刻只等岛到这一刻按计划的开始时刻 + 50 ms；主控自己
        // 落后了不连续补刻。
        // The hybrid drive (plan 5.5): each tick waits for islands only until
        // its scheduled start + 50 ms, and the master does not catch up.
        let hybrid = self.hybrid();
        loop {
            let expected = schedule.expected_start();
            self.master.hybrid.deadline = hybrid.then(|| expected + TICK);
            let actual = self.profiler().now();
            let skipped = schedule.skipped_catch_up();
            let catch_up = schedule.catching_up();
            self.profiler()
                .begin_scheduled_tick(expected, actual, skipped, catch_up);
            if self.tick().is_break() {
                break;
            }
            let now = self.profiler().now();
            if hybrid {
                schedule.after_tick_without_catch_up(now);
            } else {
                schedule.after_tick(now);
            }
            self.master
                .pacer
                .wait_until(schedule.deadline(), &mut timing);
        }
    }

    /// 测量脚手架用：不睡节拍器地推进一 tick，返回这一刻交付的区块数。
    /// 除了不睡觉，走的仍是 [`Self::tick`] 的完整流水线。
    ///
    /// Benchmark-only: advance one tick without pacing, returning the number of
    /// chunks delivered this tick. Runs the exact same pipeline as [`Self::tick`].
    #[doc(hidden)]
    pub fn tick_unpaced(&mut self) -> usize {
        let _ = self.tick();
        self.islands
            .parked()
            .map(|island| island.mailbox.counters_this_tick().0)
            .sum()
    }

    /// 测量脚手架用：加一批强加载票，并关掉随机刻（避免比较期间草蔓延改方块）。
    /// Benchmark-only: force-load a set of chunks and disable random ticks.
    #[doc(hidden)]
    pub fn force_chunks_unpaced(&mut self, positions: &[mc_bamboo_core::ChunkPos]) {
        use mc_bamboo_world::director::TicketOp;
        // 改权威副本再发布：快照每刻都会从 `Global` 重新发布。
        // Change the authoritative copy and publish: the snapshot is republished every tick.
        self.master
            .global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::RANDOM_TICK_SPEED, "0");
        self.publish_global();
        self.apply_ticket_ops(
            positions
                .iter()
                .map(|&pos| TicketOp::Forced { pos, forced: true }),
        );
    }

    /// 对拍按加载等级交付区块。33 能单独检查完工组装，32 触发半径一的后处理。
    /// Request a loading tier independently of simulation for generation comparisons.
    #[doc(hidden)]
    pub fn load_chunks_unpaced(&mut self, positions: &[ChunkPos], level: i32) {
        use mc_bamboo_world::ticket::TicketType;
        self.apply_ticket_ops(positions.iter().map(|&pos| TicketOp::Add {
            ty: TicketType::PlayerLoading,
            pos,
            level,
        }));
    }

    /// 测量脚手架用：某个区块是否已经交付进 region。
    /// Benchmark-only: whether a chunk has been delivered into the region.
    #[doc(hidden)]
    pub fn is_chunk_loaded(&self, pos: mc_bamboo_core::ChunkPos) -> bool {
        self.islands.island_of_chunk(pos).is_some()
    }

    /// 对拍脚手架用：一个已交付区块的全部方块状态，顺序是 y 从下到上、x、z
    /// 各 0..16（和官方对拍输出的顺序一致）。没交付返回 `None`。
    /// Oracle scaffolding: every block state of a delivered chunk in y, x, z
    /// order (the official oracle's order); `None` if not delivered.
    #[doc(hidden)]
    pub fn chunk_blocks_unpaced(&self, pos: mc_bamboo_core::ChunkPos) -> Option<Vec<u32>> {
        use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
        let island = self.islands.get(self.islands.island_of_chunk(pos)?)?;
        let mut out = Vec::with_capacity(WORLD_HEIGHT as usize * 256);
        for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
            for x in 0..16 {
                for z in 0..16 {
                    let at = mc_bamboo_core::BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z);
                    out.push(island.region.chunks.block(at)?);
                }
            }
        }
        Some(out)
    }

    /// 对拍脚手架用：已交付区块的 `PostProcessing`（每段 packed short，按标记顺序），
    /// 以及把它按存档路径只读编码、再解码回来后的同一张表。没交付或表正忙时
    /// 返回 `None`；存档里没有这个键时对应一侧是空列表。
    /// Oracle scaffolding: a delivered chunk's `PostProcessing` (packed shorts per
    /// section, mark order) plus the same table after a read-only save encode and
    /// decode. `None` if not delivered or mid-tick; an absent key yields an empty
    /// list on that side.
    #[doc(hidden)]
    pub fn chunk_post_processing_unpaced(
        &self,
        pos: mc_bamboo_core::ChunkPos,
    ) -> Option<Result<PostProcessingTables, String>> {
        use crate::chunk_codec::ChunkCodec;
        let island = self.islands.get(self.islands.island_of_chunk(pos)?)?;
        let column = island.region.chunks.column_clone(pos)?;
        let delivered =
            crate::worldgen::post_processing_sections(column.chunk().archive()).unwrap_or_default();
        let packed = match island.region.encode_loaded_column(pos)? {
            Ok(packed) => packed,
            Err(error) => return Some(Err(error)),
        };
        let reloaded = match crate::chunk_codec::InlineCodec.decode(pos, &packed) {
            Ok(decoded) => decoded,
            Err(error) => return Some(Err(error)),
        };
        let reloaded = crate::worldgen::post_processing_sections(
            reloaded.column.chunk_column().chunk().archive(),
        )
        .unwrap_or_default();
        Some(Ok((delivered, reloaded)))
    }

    /// 对拍脚手架用：已加载区块现在写盘会写出的区块 NBT（未压缩的根复合标签）。
    /// `None` 表示没加载或表正忙。
    ///
    /// Oracle scaffolding: the chunk NBT a save would write right now (the
    /// uncompressed root compound); `None` when not loaded or busy.
    #[doc(hidden)]
    pub fn chunk_nbt_unpaced(
        &self,
        pos: mc_bamboo_core::ChunkPos,
    ) -> Option<Result<mc_bamboo_nbt::Tag, String>> {
        let island = self.islands.get(self.islands.island_of_chunk(pos)?)?;
        let packed = match island.region.encode_loaded_column(pos)? {
            Ok(packed) => packed,
            Err(error) => return Some(Err(error)),
        };
        let bytes = packed.chunk?;
        Some(
            mc_bamboo_nbt::read_named(&bytes, mc_bamboo_nbt::Accounter::uncompressed_quota())
                .map(|(_, entries)| mc_bamboo_nbt::Tag::Compound(entries))
                .map_err(|error| format!("cannot read encoded chunk: {error}")),
        )
    }

    /// 对拍脚手架用：区块现在能不能发给玩家（`send_chunks` 用的同一个判断）。
    /// Oracle scaffolding: whether the chunk may be sent now (`send_chunks`' check).
    #[doc(hidden)]
    pub fn chunk_ready_to_send_unpaced(&self, pos: mc_bamboo_core::ChunkPos) -> bool {
        self.islands
            .island_of_chunk(pos)
            .and_then(|id| self.islands.get(id))
            .is_some_and(|island| island.region.tick_levels.ready_to_send(pos))
    }

    /// 对拍脚手架用：区块的完工后处理是否已经做完。
    /// Oracle scaffolding: whether the chunk's generation post-processing has run.
    #[doc(hidden)]
    pub fn chunk_post_processed_unpaced(&self, pos: mc_bamboo_core::ChunkPos) -> bool {
        self.islands
            .island_of_chunk(pos)
            .and_then(|id| self.islands.get(id))
            .is_some_and(|island| island.region.tick_levels.prepared(pos))
    }

    /// 对拍脚手架用：落在 `chunks` 里的排队计划刻，连同所在岛计划刻队列的当前时钟。
    /// 每条是 `(坐标, 是否流体, 注册表 ID, 触发时刻, 优先级)`，方块的在前，各自按执行顺序。
    ///
    /// Oracle scaffolding: the scheduled ticks inside `chunks`, plus the island
    /// queue's current clock. Each entry is `(pos, is_fluid, registry id, trigger,
    /// priority)`, block ticks first, each in execution order.
    #[doc(hidden)]
    pub fn scheduled_ticks_unpaced(
        &self,
        chunks: &[mc_bamboo_core::ChunkPos],
    ) -> Option<(i64, Vec<ScheduledTickRow>)> {
        let island = self
            .islands
            .get(self.islands.island_of_chunk(*chunks.first()?)?)?;
        let ticks = &island.region.block_ticks;
        let inside = |pos: mc_bamboo_core::BlockPos| {
            chunks.contains(&mc_bamboo_core::ChunkPos::new(pos.x >> 4, pos.z >> 4))
        };
        let mut rows: Vec<ScheduledTickRow> = ticks
            .pending()
            .filter(|tick| inside(tick.pos))
            .map(|tick| {
                (
                    tick.pos,
                    false,
                    tick.block as i64,
                    tick.trigger_tick,
                    tick.priority as i32,
                )
            })
            .collect();
        rows.extend(
            ticks
                .pending_fluids()
                .filter(|tick| inside(tick.pos))
                .map(|tick| {
                    (
                        tick.pos,
                        true,
                        i64::from(tick.fluid),
                        tick.trigger_tick,
                        tick.priority as i32,
                    )
                }),
        );
        Some((ticks.now(), rows))
    }

    /// 测量脚手架用：计算池里还有没有在途任务。
    /// Benchmark-only: whether the compute pool still has work in flight.
    #[doc(hidden)]
    pub fn compute_in_flight(&self) -> usize {
        self.master.host.compute_in_flight()
    }

    /// 推进一 tick。顺序有约束：光照和官方一样在处理完请求之后、下一 tick 之前算，计划刻、随机刻和
    /// 方块实体的 tick 紧跟在时间增长之后（官方 `ServerLevel.tick` 的顺序），新区块排在区块中心和
    /// forget 后面，每个区块的光照变化紧挨在它的方块变化前面，方块变化排在 ack 前面，
    /// 区块批次放在最后，这样发出去的内容是这一 tick 结束时的世界。
    fn tick(&mut self) -> ControlFlow<()> {
        // 官方 `MinecraftServer.tickServer` 在 `tickChildren` 之前 `++tickCount`；
        // 周期时间同步按这个开服以来的刻数计相位，和存档里的 `gameTime` 无关。
        //
        // Official `MinecraftServer.tickServer` does `++tickCount` before
        // `tickChildren`; the periodic time sync uses this since-launch count,
        // independent of the saved `gameTime`.
        self.master.global.increment_server_ticks();
        self.profiler().begin_tick();
        self.master.island_wait = Duration::ZERO;
        self.record_tick_counts();
        let flow = self.tick_phases();
        let hybrid = self.hybrid();
        // 家岛不在主控手里（关服时没等到卡住的岛）：剖析器跟着岛走了，这一刻不再收尾。
        // 混合驱动下打报告的是主控自己的，照常收尾。
        // The home island is away (stuck at shutdown): its profiler went with
        // it. Under the hybrid drive the master's own reports and finishes as usual.
        if !hybrid && !self.islands.is_home() {
            return flow;
        }
        // 三阶段这一刻的耗时由 `lap` 顺手记下，划分统计拿它当尾段占比的分母。
        // 开关关着时这里只判一次布尔值。
        if self.profiler().partition_enabled() {
            let three = self.profiler().three_phase_nanos_this_tick();
            let total = self.profiler().all_phase_nanos_this_tick();
            if let Some(stats) = self.profiler().partition_stats_mut() {
                stats.finish_tick(three, total);
            }
        }
        // 主控（宿主）这一刻记的子段并进报告：只有一个岛时就是合计。
        // Fold the master's (host's) sub-phases into the report: with one
        // island that is the total.
        report_profiler(&mut self.islands, &mut self.master.profiler, hybrid)
            .absorb_sub_totals(&mut self.master.host.profiler);
        if self.master.drive == IslandDrive::Detached {
            // 主控线程这一刻自己干活的时间：整刻减去等岛交回的那段。
            // The master thread's own work this tick: the whole tick minus the
            // wait for the island.
            let wait = self.master.island_wait;
            let profiler = self.profiler();
            let busy = profiler.tick_elapsed().saturating_sub(wait);
            profiler.record_duration(Metric::MasterBusy, busy);
        }
        if hybrid && self.master.profiler.report_due() {
            self.fold_island_profiles();
        }
        self.profiler().end_tick();
        flow
    }

    /// 混合驱动下打报告之前：停放着的岛的样本并进主控的剖析器（之后清零），每个岛另记一行
    /// （这一段跑完了几刻、自己的 TPS、每刻多久、丢了几刻、还欠几刻；刻数和耗时是主控在岛
    /// 落座时记的，岛不在手里也算得到）。不在主控手里的岛的各阶段样本留到下一份报告。
    ///
    /// Before a report under the hybrid drive: fold every parked island's
    /// samples into the master's profiler (and clear them), with one line per
    /// island (ticks finished in the period, its own TPS, time per tick,
    /// ticks dropped and owed; ticks and times are recorded by the master as
    /// islands are seated, so islands that are away count too). Islands that
    /// are away keep their phase samples for the next report.
    fn fold_island_profiles(&mut self) {
        use std::fmt::Write as _;
        let seconds = self.master.profiler.since_report().as_secs_f64();
        let mut text = format!(
            "islands (over {seconds:.2} s; ticks = island ticks finished, busy = ms per island tick):\n"
        );
        let mut ids: std::collections::BTreeSet<mc_bamboo_world::director::RegionId> =
            self.islands.ids().into_iter().collect();
        ids.extend(self.master.profiler.island_runs().keys().copied());
        let ms = |nanos: u64| nanos as f64 / 1e6;
        for id in ids {
            let (ticks, mean, p50, p99, max, dropped) =
                match self.master.profiler.island_runs().get(&id) {
                    Some(runs) => {
                        let busy = &runs.busy;
                        let mean = if busy.count() == 0 {
                            0.0
                        } else {
                            busy.sum() as f64 / busy.count() as f64 / 1e6
                        };
                        (
                            busy.count(),
                            mean,
                            ms(busy.p50()),
                            ms(busy.p99()),
                            ms(busy.max()),
                            runs.dropped,
                        )
                    }
                    None => (0, 0.0, 0.0, 0.0, 0.0, 0),
                };
            let tps = if seconds > 0.0 {
                ticks as f64 / seconds
            } else {
                0.0
            };
            let state = if !self.islands.contains(id) {
                "gone".to_owned()
            } else if let Some(island) = self
                .islands
                .slot_mut(id)
                .and_then(crate::island::IslandSlot::parked_mut)
            {
                let region = &mut island.region;
                let state = format!(
                    "chunks={} players={} entities={}",
                    region.chunks.loaded_count().unwrap_or(0),
                    region.players.len(),
                    region.entities.len(),
                );
                self.master.profiler.merge(&region.profiler);
                region.profiler.reset_samples();
                state
            } else {
                "away".to_owned()
            };
            let _ = writeln!(
                text,
                "  island {id:<4} {state} ticks={ticks} tps={tps:.2} busy mean={mean:.3} p50={p50:.3} p99={p99:.3} max={max:.3} dropped={dropped} owed={}",
                self.islands.debt(id),
            );
        }
        self.master.profiler.append_to_report(&text);
    }

    /// 每刻开始时的数量指标，趁状态还没变先记下来。到期的计划刻数是
    /// 队列里 `trigger_tick <= now + 1` 的那些（岛的时钟在计划刻之前加一）。
    /// 队列按到期时刻排好了，所以只看开头到期的那一段，不扫整条队列。
    fn record_tick_counts(&mut self) {
        // 多岛时是全部岛的合计。
        // The sum over every island with several.
        let (mut players, mut entities, mut due, mut loaded) = (0, 0, 0, 0);
        for island in self.islands.parked() {
            let region = &island.region;
            let next_time = region.tick.game_time() + 1;
            due += region
                .block_ticks
                .pending()
                .take_while(move |tick| tick.trigger_tick <= next_time)
                .count()
                + region
                    .block_ticks
                    .pending_fluids()
                    .take_while(move |tick| tick.trigger_tick <= next_time)
                    .count();
            players += region.players.len();
            entities += region.entities.len();
            loaded += region.chunks.loaded_count().unwrap_or(0);
        }
        let profiler = self.profiler();
        profiler.record(Metric::OnlinePlayers, players as u64);
        profiler.record(Metric::Entities, entities as u64);
        profiler.record(Metric::DueScheduledTicks, due as u64);
        profiler.record(Metric::LoadedChunks, loaded as u64);
    }

    /// 每刻一次的压测回调。`None` 时只做一次 `Option` 判断。
    fn run_bench_hooks(&mut self) {
        let Some(hooks) = self.master.bench_hooks else {
            return;
        };
        // 压测回调按位置找停放着的岛（`BenchWorld`），不在主控手里的岛这一刻碰不到。
        // The hooks find parked islands by position (`BenchWorld`); islands
        // that are away are out of reach this tick.
        let ready = self.master.bench_ready;
        let finished = {
            let mut bench = crate::bench::BenchWorld::new(self);
            if ready {
                (hooks.tick)(&mut bench);
                false
            } else {
                (hooks.setup)(&mut bench)
            }
        };
        // 压测回调可能经命令改了 `Global`。
        // The hooks may have changed `Global` through a command.
        self.publish_global();
        if finished {
            self.master.bench_ready = true;
            tracing::info!("bench scenario ready");
        }
    }

    /// 拆成 `tick` 和这一段只是为了保证提前返回时也能收尾。
    ///
    /// 齐步（`island_threads = 0`）在世界线程上照原来的顺序一行不差地跑。分离时一刻分三段：
    ///
    /// 1. 岛停放在主控手里：岛先收上一刻投来的信；主控收入站消息（玩家请求、进服、控制台
    ///    命令），推进边界、天气和时间。这几步都同时碰 `Global` 和岛（请求可能直接写全局，
    ///    天气用岛的随机源），只有一个岛时在这里串行做。
    /// 2. 岛交给 tick 线程跑它的一刻（光照、岛的时钟、方块、实体、视图、广播、发区块，最后
    ///    照上次的名单编存盘快照）；主控同时推进 Director，只投信、不碰岛。
    /// 3. 岛交回、停放：主控收下回执和排队的全局改动、发延迟广播、刷出站缓冲，给下一份
    ///    存盘名单，处理 `/save-all`、关服和名单变更。主控的帧都在岛停放时写进玩家的缓冲，
    ///    和岛写的帧不抢；第 3 段写的和岛这一刻写的同一次刷出（岛的实施计划 4.3）。
    ///
    /// Split from `tick` only so an early return still finishes the tick.
    /// Lockstep (`island_threads = 0`) runs the original order line for line
    /// on the world thread. Detached runs three parts: (1) with the island
    /// parked, it takes last tick's mail and the master handles the inbound
    /// messages and advances the border, weather and time (all of which touch
    /// both `Global` and the island); (2) the island runs its tick on a tick
    /// thread while the master advances the Director, posting mail only;
    /// (3) the island parks again and the master takes the acks and queued
    /// global changes, sends the latency update, flushes, plans the next save
    /// and handles `/save-all`, shutdown and list changes. Master frames are
    /// written only while the island is parked, so they never race the
    /// island's, and part 3's frames go out in the same flush (plan 4.3).
    fn tick_phases(&mut self) -> ControlFlow<()> {
        let hybrid = self.hybrid();
        if hybrid {
            // 混合驱动第 ⓪ 段：收下已经交回的岛（计划 5.5.1）。
            // Hybrid part 0: take in the islands that are back (plan 5.5.1).
            self.begin_hybrid_tick()?;
        }
        self.master.host.begin_tick();
        for island in self.islands.parked_mut() {
            island.mailbox.begin_tick();
        }
        let detached = self.master.drive == IslandDrive::Detached;
        if detached {
            self.start_island_ticks();
        }
        self.inbound_phase()?;
        if hybrid {
            self.advance_world_clock();
            self.profiler().lap(Phase::WeatherAndTime);
            self.run_hybrid_round()?;
            self.finish_tick()?;
            // 第 ④ 段：还欠刻的岛在剩下的时间里追赶。
            // Part 4: islands that owe ticks catch up in the time left.
            return self.catch_up();
        } else if detached {
            self.advance_world_clock();
            self.profiler().lap(Phase::WeatherAndTime);
            self.run_detached_tick()?;
        } else {
            self.tick_ticket_timeouts();
            self.record_delivery_metrics();
            self.profiler().lap(Phase::Delivery);
            for island in self.islands.iter_mut() {
                crate::generation_post_process::prepare_ticking_chunks(&mut island.region);
                light_updates::run_light_updates(&mut island.region);
            }
            self.profiler().lap(Phase::LightUpdates);
            self.advance_world_clock();
            for island in self.islands.iter_mut() {
                island.region.advance_tick();
            }
            self.profiler().lap(Phase::WeatherAndTime);
            if self.islands.several() {
                // 齐步多岛：按编号一个接一个跑完各岛的一刻（单线程，确定的先后）。主控的
                // 全局改动和延迟广播交回时再做，扇出给所有岛。
                // Lockstep with several islands: run each island's tick in id
                // order on this thread. Global changes and the latency update
                // happen at hand-back and fan out to every island.
                for id in self.islands.ids() {
                    let island = self.islands.get_mut(id).expect("在表里");
                    let home = id == HOME_REGION;
                    if !home {
                        island.region.profiler.begin_tick();
                    }
                    run_island_simulation(island, None, self.master.entity_pool.as_ref());
                    if !home {
                        island.region.profiler.end_tick();
                    }
                }
            } else {
                let port_mode = self.master.port_mode;
                let entity_pool = self.master.entity_pool.as_ref();
                run_island_simulation(
                    &mut self.islands,
                    Some((&mut self.master.global, port_mode)),
                    entity_pool,
                );
            }
        }
        self.finish_tick()
    }

    /// 第 1 段：收 IO 和计算池的结果、发 `/save-all` 的回执，处理这一刻的入站消息、
    /// 压测回调和存盘命令，按玩家的新位置挂票。
    ///
    /// Part 1: collect IO and compute results, release `/save-all` replies,
    /// handle this tick's inbound messages, bench hooks and save commands, and
    /// move tickets to the players' new positions.
    fn inbound_phase(&mut self) -> ControlFlow<()> {
        // 上一刻发出去的读盘请求，结果在这一刻开头经信箱收回。
        // Read requests dispatched last tick are collected at the start of this tick.
        self.master
            .host
            .poll_io(&mut island_access(self.master.drive, &mut self.islands));
        // 每次 `/save-all` 等自己那道栅栏；请求里全部快照提交完之后才登记等待。
        // Each `/save-all` waits for its own barrier, registered only after all
        // of its snapshots have been handed to IO (review 8/9).
        for (seq, source) in self.master.host.take_ready_save_barriers() {
            self.master.pending_save_success.push((seq, source));
        }
        let flushed = self.master.host.take_flushed_barriers();
        self.release_flushed_save_success(flushed);
        // IO 线程 panic 或通道断开是致命错误（设计稿 §14 / R12）：记日志，走关服流程，
        // 尽量把还能存的存下来，不挂死。`shut_down` 里的同步写（level.dat / data/）不依赖
        // IO 线程，失败的写也不会让 flush 无限等待（通道已断立刻返回）。
        // A dead IO thread is fatal (design §14 / R12): log, shut down, save what
        // can still be saved, never hang. `shut_down`'s synchronous writes do not
        // depend on the IO thread, and a disconnected channel makes flush return
        // immediately instead of blocking.
        if self.master.host.io_failed() {
            error!("chunk IO thread stopped unexpectedly; shutting down to avoid silent data loss");
            return self.shut_down();
        }
        if self.master.host.compute_failed() {
            error!("chunk compute thread stopped unexpectedly; shutting down");
            return self.shut_down();
        }
        // 每刻开头给岛发布一份全局快照（岛的实施计划第 6 节第 5a 步）。
        // Publish this tick's global snapshot to the island (island plan, step 5a).
        self.publish_global();
        self.process_inbox()?;
        // 压测输入跑在收包阶段，等价于一名管理员在世界线程上翻拉杆 / 补物品；
        // 它的耗时记进 `inbox`，不进后面三个方块侧阶段。默认配置下这里是空操作。
        self.run_bench_hooks();
        self.consume_save_commands();
        self.sync_player_tickets();
        // 这一刻玩家票刚挂上；区块在下一段的 Director 步进里交付。
        // 已交付 / 新交付的候选区块就绪后，`Region::advance_tick` 与邮筒
        // 收信处完成落位（见那里）。
        for island in self.islands.parked_mut() {
            crate::spawn_landing::finish_pending(&mut island.region);
        }
        // 区块就绪后由 `Region::advance_tick` 完成落位（见那里）。
        // `Region::advance_tick` finishes the landing once chunks are ready.
        self.profiler().lap(Phase::Inbox);
        ControlFlow::Continue(())
    }

    /// 这一刻信箱交付 / 收回了多少，宿主的停放表和计算池有多少。
    fn record_delivery_metrics(&mut self) {
        let (delivered, reclaimed) = self
            .islands
            .parked()
            .map(|island| island.mailbox.counters_this_tick())
            .fold((0, 0), |(a, b), (c, d)| (a + c, b + d));
        let parked = self.master.host.parked_count() as u64;
        let in_pool = self.master.host.compute_in_pool() as u64;
        let backlog = self.master.host.compute_backlog() as u64;
        let profiler = self.profiler();
        profiler.record(Metric::ChunksDelivered, delivered as u64);
        profiler.record(Metric::ChunksReclaimed, reclaimed as u64);
        profiler.record(Metric::ParkedChunks, parked);
        profiler.record(Metric::ComputeInFlight, in_pool);
        profiler.record(Metric::ComputeBacklog, backlog);
    }

    /// 世界边界、天气和时间推进一刻，前后各发布一次快照。
    ///
    /// 官方 `MinecraftServer.tickChildren` / `ServerLevel.tick` 的开头：
    /// clocks 阶段推进世界时钟 → 周期时间同步 → world border → 天气 →
    /// 睡眠统计（够了就跳早、叫醒、清天气）→ `updateSkyBrightness` →
    /// `tickTime`（游戏时间 +1）。跳早用的是上一刻全部岛交回的计数，
    /// 在岛这一刻开跑之前做，多岛读同一份结果。
    ///
    /// Official `MinecraftServer.tickChildren` / `ServerLevel.tick` prefix:
    /// clocks tick → periodic time sync → world border → weather → sleep
    /// bookkeeping (skip night, wake, clear weather) → `updateSkyBrightness`
    /// → `tickTime`. The skip uses the counts all islands handed back last
    /// tick and happens before any island runs this tick.
    fn advance_world_clock(&mut self) {
        // 跳早标志只在决定跳早的这一刻为真；下一刻重新算。
        self.master.global.sleep_skip = false;
        // `MinecraftServer.tickChildren` 的 "clocks" 阶段在 `ServerLevel.tick` 之前。
        self.master.global.tick_clocks();
        // `forceGameTimeSynchronization` 在 levels 循环之前，包里是这一刻
        // `tickTime` 加一之前的游戏时间。先取出来，后面游戏时间才 +1。
        let sync_time = self.master.global.time_sync_packet_time();

        self.master.global.tick_world_border();
        self.publish_global();
        // 官方 `ServerLevel.tick` 的天气阶段在 `tickTime` 之前；广播帧排在时间包之前发出。
        // 多岛时天气用主控自己的随机源，不碰任何岛的（计划 3.3「岛的随机源」第 4 条）。
        // Vanilla's weather phase precedes `tickTime`. With several islands
        // the weather uses the master's own random source (plan 3.3).
        let weather = match self.master.global.weather_random.take() {
            Some(random) => {
                let frames = weather::advance(&mut self.master.global, &random);
                self.master.global.weather_random = Some(random);
                frames
            }
            None => weather::advance(&mut self.master.global, &self.islands.region.random),
        };
        if !weather.is_empty() {
            self.broadcast_all(&weather);
        }

        // 官方 `ServerLevel.tick`：`players_sleeping_percentage` 加上
        // `areEnoughSleeping` / `areEnoughDeepSleeping` 决定跳早；
        // `moveToTimeMarker` 要 `advance_time` 为真，`wakeUpAllPlayers` 无条件，
        // `resetWeatherCycle` 要 `advance_weather` 为真且正在下雨。
        //
        // Official `ServerLevel.tick`: `players_sleeping_percentage` plus
        // `areEnoughSleeping` / `areEnoughDeepSleeping` decide the skip.
        // `moveToTimeMarker` needs `advance_time`; `wakeUpAllPlayers` is
        // unconditional; `resetWeatherCycle` needs `advance_weather` and rain.
        let rules = self.master.global.rules();
        let percentage = rules.int(mc_bamboo_data::gamerules::PLAYERS_SLEEPING_PERCENTAGE);
        let advance_time = rules.bool(mc_bamboo_data::gamerules::ADVANCE_TIME);
        let advance_weather = rules.bool(mc_bamboo_data::gamerules::ADVANCE_WEATHER);
        let should_skip = self.master.sleep.should_skip(percentage);
        // 改坏确认：第一次成立时故意不做，把决定推到下一刻。
        #[cfg(test)]
        let should_skip = if should_skip && self.master.sleep_break_late {
            self.master.sleep_break_late = false;
            false
        } else {
            should_skip
        };
        if should_skip {
            let clock_moved =
                crate::sleep::skip_night(&mut self.master.global, advance_time, advance_weather);
            if clock_moved {
                // `ServerClockManager.modifyClock` 只发被改的那一个时钟。
                let time = self.master.global.time();
                let clock = self.master.global.clocks()[0].1;
                let frames =
                    crate::links::encode_frames(&mc_bamboo_protocol::packet::play::SetTime {
                        game_time: time.0,
                        clocks: &[mc_bamboo_protocol::packet::play::ClockState {
                            clock: mc_bamboo_data::registries::world_clock::OVERWORLD,
                            ticks: clock.total_ticks,
                            partial: clock.partial_tick,
                            rate: clock.network_rate(advance_time),
                        }],
                    });
                self.broadcast_all(&frames);
            }
        }

        // `ServerLevel.tickTime`：游戏时间照常加一；世界时钟已在开头推进过。
        self.master.global.increment_game_time();
        if let Some(time) = sync_time {
            for island in self.islands.parked_mut() {
                island.region.broadcast_game_time(time);
            }
            // 不在主控手里的岛：进它的出站队列，落地时发。
            // Islands that are away: queued, sent when they land.
            let away = self.islands.away_ids();
            if !away.is_empty() {
                let frames =
                    crate::links::encode_frames(&mc_bamboo_protocol::packet::play::SetTime {
                        game_time: time.0,
                        clocks: &[],
                    });
                self.post_fan_out(&away, vec![crate::links::FanOut::All(frames.into())]);
            }
        }
        // 天气、跳早和时间推进完再发布一次，这一刻的方块逻辑读它。
        // Publish again after weather, skip and time; this tick's block logic reads it.
        self.publish_global();
    }

    /// 给每个岛发布一份全局快照（只有一个岛时就是原来那一次）。
    ///
    /// Publish the global snapshot to every island (once with one island).
    fn publish_global(&mut self) {
        // 混合驱动下只发给停放着的岛；岛每次派出之前都会再发一次（计划 5.5.5）。
        // Under the hybrid drive only parked islands get it; every island is
        // published to again right before it is dispatched (plan 5.5.5).
        for island in self.islands.parked_mut() {
            self.master.global.publish(&mut island.region);
        }
    }

    /// 主控的帧广播给所有岛的所有玩家（岛都停放着，等于各岛按顺序收下这封出站信）。
    ///
    /// Broadcast a master frame to every player on every island (the islands
    /// are parked, so this equals each island taking the outbound letter).
    fn broadcast_all(&mut self, frames: &[u8]) {
        for island in self.islands.parked_mut() {
            island.region.links.broadcast(frames);
        }
        // 不在主控手里的岛：进它的出站队列，落地时发（计划 5.5.5）。
        // Islands that are away: queued, sent when they land (plan 5.5.5).
        let away = self.islands.away_ids();
        if !away.is_empty() {
            self.post_fan_out(&away, vec![crate::links::FanOut::All(frames.into())]);
        }
    }

    /// **借玩家的唯一入口**：主控在 `into` 这个岛上做一件涉及别的岛上的玩家的事，`borrow`
    /// 说的那些岛上的玩家先整套借过来（[`Islands::gather`]），做完还回去、重发全局快照，再把
    /// 落到别的岛已加载区块上的实体搬过去（[`Self::move_stray_entities`]：`/give` 掉在远处
    /// 玩家脚下的东西、`/loot spawn` 到别的岛的位置）。只有一个岛时就是直接做。
    ///
    /// 命令、聊天、会话更新、改规则、改模式的实现只拿到 `into` 的 `Region`，不直接碰别的岛；
    /// 现在齐步驱动和分离驱动每刻都有一次全部岛停放，这里当场借、当场做。以后各岛不再一起
    /// 停放时（计划 5.2）只改这里：要的岛没停放就排队，等它停放再做。调用方里**依赖当场
    /// 执行**的（结果要和同一名玩家后面的包保持先后、或者要接着往同一条缓冲里写）在调用处
    /// 标了「当场执行」。
    ///
    /// **The single entry for borrowing players**: the master does something
    /// on island `into` that involves players of other islands. The players of
    /// the islands named by `borrow` are borrowed whole first
    /// ([`Islands::gather`]), returned afterwards, the snapshot republished,
    /// and entities that landed on another island's loaded chunk are moved
    /// there ([`Self::move_stray_entities`]). With one island it just runs.
    /// Commands, chat, chat sessions, game rules and game modes only see
    /// `into`'s `Region` and never touch other islands directly. Today every
    /// island parks once per tick under both drives, so this borrows and runs
    /// at once; if islands stop parking together (plan 5.2) only this entry
    /// changes (queue until the islands it needs are parked). Callers that
    /// **rely on running at once** (ordering against the same player's later
    /// packets, or writing on into the same buffer) are marked at the call site.
    fn on_master<R>(
        &mut self,
        into: mc_bamboo_world::director::RegionId,
        borrow: crate::island::Borrow,
        work: impl FnOnce(&mut Global, &mut Region) -> R,
    ) -> R {
        if !self.islands.several() {
            let region = &mut self.islands.get_mut(into).expect("岛在表里").region;
            return work(&mut self.master.global, region);
        }
        // 混合驱动下有的岛可能不在主控手里（计划 5.5.2）：只借停放着的岛，给不在的岛的那
        // 一份（给所有人的帧、要逐个接收者判断的投递）按先后收集下来，做完进它们的出站
        // 队列。必须看得见全部玩家的事（命令、改规则包、控制台）由调用方先等全部岛停放
        // （[`Self::try_held`]），到这里时没有不在的岛。
        // Under the hybrid drive some islands may be away (plan 5.5.2): only
        // parked islands lend their players, and the share of the absent ones
        // (frames for everyone, deliveries decided per receiver) is collected
        // in order and queued for them afterwards. Work that must see every
        // player (commands, game-rule packets, the console) waits for every
        // island before coming here ([`Self::try_held`]).
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut borrow = borrow;
        #[cfg(test)]
        if self.islands.world_view_off && matches!(borrow, Borrow::World(_) | Borrow::Islands(..)) {
            borrow = Borrow::All;
        }
        #[cfg(test)]
        if self.islands.borrow_all_islands
            && let Borrow::Islands(_, class) = &borrow
        {
            borrow = Borrow::World(*class);
        }
        // 按需借（计划 4.5.1）：先取「不在主控手里」的岛名单，再 lift 借进来的岛；
        // lift 过的槽位看起来也不在，晚取会多投一份 fan-out 帧。
        // On-demand borrowing (plan 4.5.1): take the away-island list before
        // lifting the borrowed islands; lifted slots look away too, so taking
        // it later would post an extra fan-out copy.
        let away = self.islands.away_ids();
        let absent = self.islands.away_players();
        if matches!(borrow, Borrow::World(_) | Borrow::Islands(..)) {
            // 全拼兜底走原来的三段；按需借只并算出来的那几个岛，其余岛只借玩家。
            // Full-join fallback keeps the old three stages; on-demand joins
            // only the planned islands and lends players from the rest.
            let joining = Instant::now();
            let unified = match &borrow {
                Borrow::World(_) => self.islands.unify(into),
                Borrow::Islands(ids, _) => self.islands.unify_subset(into, ids),
                _ => unreachable!("上面已经筛过"),
            };
            let joined = joining.elapsed();
            // `R` 外的岛把玩家（七类表）整套借过来：成功反馈要发给所有 OP，
            // 音效和等级事件也按全部玩家的距离筛（计划 4.5.1）。
            // Islands outside `R` lend their whole player tables: success
            // feedback reaches every op, and effects are filtered against all
            // players (plan 4.5.1).
            #[cfg(test)]
            let gathered = if self.islands.borrow_break == crate::island::BorrowBreak::MissPlayers {
                self.islands.gather(into, &Borrow::PlayersOf(Vec::new()))
            } else {
                self.islands.gather(into, &Borrow::All)
            };
            #[cfg(not(test))]
            let gathered = self.islands.gather(into, &Borrow::All);
            let region = &mut self.islands.get_mut(into).expect("岛在表里").region;
            if !away.is_empty() {
                region.links.begin_fan_out(absent);
            }
            let working = Instant::now();
            let out = work(&mut self.master.global, region);
            let worked = working.elapsed();
            let fan_out = if away.is_empty() {
                Vec::new()
            } else {
                region.links.end_fan_out()
            };
            self.islands.scatter(into, gathered);
            let dividing = Instant::now();
            self.islands.divide(into, unified);
            let divided = dividing.elapsed();
            let (borrowed, class, fallback) = match &borrow {
                Borrow::World(class) => (self.islands.ids().len() as u64, *class, true),
                Borrow::Islands(ids, class) => (ids.len() as u64, *class, false),
                _ => (0, BorrowClass::Other, false),
            };
            #[cfg(test)]
            {
                self.islands.last_borrowed = borrowed as usize;
                self.islands.last_fallback = fallback;
            }
            let profiler = self.profiler();
            profiler.record_duration(Metric::IslandUnify, joined);
            profiler.record_duration(Metric::IslandWorldCommand, worked);
            profiler.record_duration(Metric::IslandDivide, divided);
            profiler.record(Metric::IslandBorrowedIslands, borrowed);
            if fallback {
                profiler.record(Metric::IslandWorldCommandFallbacks, 1);
            }
            // 每类命令各记一条「借了几个岛」/「兜底了一次」（计划 4.5.1 第 8 节）。
            // One borrowed-island sample and one fallback sample per class
            // (plan 4.5.1, section 8).
            let borrowed_metric = match class {
                BorrowClass::Other => None,
                BorrowClass::Gamemode => Some(Metric::IslandBorrowedGamemode),
                BorrowClass::Hurt => Some(Metric::IslandBorrowedHurt),
                BorrowClass::Effect => Some(Metric::IslandBorrowedEffect),
                BorrowClass::Execute => Some(Metric::IslandBorrowedExecute),
            };
            if let Some(metric) = borrowed_metric {
                profiler.record(metric, borrowed);
                if fallback {
                    let metric = match class {
                        BorrowClass::Gamemode => Metric::IslandFallbackGamemode,
                        BorrowClass::Hurt => Metric::IslandFallbackHurt,
                        BorrowClass::Effect => Metric::IslandFallbackEffect,
                        _ => Metric::IslandFallbackExecute,
                    };
                    profiler.record(metric, 1);
                }
            }
            self.publish_global();
            self.post_fan_out(&away, fan_out);
            return out;
        }
        let away = match borrow {
            Borrow::All => away,
            Borrow::PlayersOf(_) => Vec::new(),
            Borrow::World(_) | Borrow::Islands(..) => unreachable!("上面已经返回"),
        };
        let gathered = self.islands.gather(into, &borrow);
        let region = &mut self.islands.get_mut(into).expect("岛在表里").region;
        if !away.is_empty() {
            region.links.begin_fan_out(absent);
        }
        let out = work(&mut self.master.global, region);
        let fan_out = if away.is_empty() {
            Vec::new()
        } else {
            region.links.end_fan_out()
        };
        self.islands.scatter(into, gathered);
        self.publish_global();
        self.move_stray_entities(into);
        self.post_fan_out(&away, fan_out);
        out
    }

    /// 分离驱动的第 2 段：岛交给 tick 线程跑它的一刻，主控同时推进 Director，再等岛交回。
    /// 岛在线程上 panic 时在主控线程上重新抛出，走世界线程原来的崩溃处理；关服时岛卡住
    /// 回不来就不等它（[`Self::shut_down_without_island`]）。
    ///
    /// Detached part 2: hand the island to a tick thread, advance the Director
    /// meanwhile, then wait for the hand-back. A panic on the tick thread is
    /// re-raised here, on the world thread's existing crash path; an island
    /// stuck at shutdown is left behind ([`Self::shut_down_without_island`]).
    fn run_detached_tick(&mut self) -> ControlFlow<()> {
        // 多岛时每个岛各交给一条 tick 线程（尽量回到上次那条），全部交回再往下走。
        // With several islands each goes to a tick thread (its last one when
        // possible), and the master continues once all are back.
        let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
        let dispatched = Instant::now();
        let mut away = 0;
        for id in self.islands.ids() {
            let ticket = self.islands.next_ticket();
            let slot = self.islands.slot_mut(id).expect("岛在表里");
            let mut island = slot.take(ticket);
            island.dispatch();
            let hint = island.last_worker;
            if let Err(mut island) = pool.dispatch(hint, island, ticket) {
                island.park();
                slot.put(island);
                panic!("the island tick threads are gone");
            }
            away += 1;
        }
        // 岛在 tick 线程上跑；这里只碰主控自己的状态，信件只投进邮筒。
        // The islands run on tick threads; only master state is touched
        // here and mail is only posted.
        let master_started = Instant::now();
        self.master.director_step(&mut IslandAccess::Away);
        let master_done = Instant::now();
        let mut backs = Vec::with_capacity(away);
        while backs.len() < away {
            match self.wait_for_island(dispatched) {
                Some(back) => backs.push(back),
                None => {
                    for back in backs {
                        let mut island = back.island;
                        island.park();
                        let id = island.id;
                        if let Some(slot) = self.islands.slot_mut(id) {
                            slot.put(island);
                        }
                    }
                    return self.shut_down_without_island();
                }
            }
        }
        let resumed = Instant::now();
        let mut first_panic = None;
        let mut started_first = resumed;
        let mut finished_last = dispatched;
        let mut home_busy = Duration::ZERO;
        for back in backs {
            let crate::island_pool::HandBack {
                mut island,
                ticket: _,
                worker,
                started,
                finished,
                queued: _,
                panic,
            } = *back;
            island.park();
            island.last_worker = worker;
            started_first = started_first.min(started);
            finished_last = finished_last.max(finished);
            let busy = finished.saturating_duration_since(started);
            if island.id == HOME_REGION {
                home_busy = busy;
            } else {
                island
                    .region
                    .profiler
                    .record_duration(Metric::IslandBusy, busy);
            }
            let id = island.id;
            self.islands
                .slot_mut(id)
                .expect("交出去的岛回到自己的槽位")
                .put(island);
            if first_panic.is_none() {
                first_panic = panic;
            }
        }
        if let Some(payload) = first_panic {
            error!("an island tick thread panicked; taking the world thread's crash path");
            std::panic::resume_unwind(payload);
        }
        self.master.island_wait = resumed.saturating_duration_since(master_done);
        let profiler = &mut self.profiler();
        profiler.restart_lap();
        let director = master_done.saturating_duration_since(master_started);
        profiler.record_phase(
            Phase::Delivery,
            director.as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        profiler.record_duration(Metric::IslandBusy, home_busy);
        // 交接本身：派出到线程开跑，加上岛跑完（主控自己的活也做完）到主控接着干活。
        // The hand-over itself: dispatch to start, plus island done (and the
        // master's own work done) to the master resuming.
        let handoff = started_first.saturating_duration_since(dispatched)
            + resumed.saturating_duration_since(finished_last.max(master_done));
        profiler.record_duration(Metric::IslandHandoff, handoff);
        self.finish_partition_tick();
        self.record_delivery_metrics();
        ControlFlow::Continue(())
    }

    /// 等岛交回。岛一交回就返回；一直不回来时每隔一段报告一次，收到关服请求之后再等
    /// `island_stop_timeout`，还不回来就返回 `None`（当它卡死了）。tick 线程全没了
    /// （岛不会再回来）当成崩溃。
    ///
    /// Wait for the hand-back. Returns as soon as the island is back; while it
    /// is not, report periodically, and after a shutdown request wait
    /// `island_stop_timeout` more before returning `None` (stuck). Losing every
    /// tick thread (the island never comes back) is a crash.
    fn wait_for_island(
        &mut self,
        dispatched: Instant,
    ) -> Option<Box<crate::island_pool::HandBack>> {
        let mut next_report = ISLAND_SLOW_REPORT;
        let mut stop_requested: Option<Instant> = None;
        loop {
            let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
            match pool.wait(ISLAND_WAIT_SLICE) {
                Wait::Back(back) => return Some(back),
                Wait::Lost => {
                    panic!("every island tick thread is gone; the island cannot come back")
                }
                Wait::Pending => {}
            }
            let running = dispatched.elapsed();
            if running >= next_report {
                warn!(
                    seconds = running.as_secs(),
                    "the island tick is still running"
                );
                next_report += ISLAND_SLOW_REPORT;
            }
            if stop_requested.is_none() && self.master.shutdown_requested() {
                stop_requested = Some(Instant::now());
            }
            if let Some(since) = stop_requested
                && since.elapsed() >= self.master.island_stop_timeout
            {
                return None;
            }
        }
    }

    /// 汇总全部已停放岛的睡眠计数，供主控在下一刻开头决定跳早。
    ///
    /// Sum the sleep counts of every parked island for the master's decision
    /// at the start of the next tick.
    pub(super) fn aggregate_sleep(&mut self) {
        let mut sleep = crate::sleep::SleepCounts::default();
        for island in self.islands.parked() {
            sleep.add(crate::sleep::counts_of(&island.region));
        }
        self.master.sleep = sleep;
    }

    /// 第 3 段（两种驱动共用）：交回、刷出站缓冲、自动保存和匀速存盘、`/save-all`、
    /// 关服检查和名单变更。
    ///
    /// Part 3 (both drives): hand-back, flush, autosave and paced saves,
    /// `/save-all`, the stop check and list changes.
    fn finish_tick(&mut self) -> ControlFlow<()> {
        let detached = self.master.drive == IslandDrive::Detached;
        // 齐步单岛时延迟广播已经在岛的同步阶段里原位发过了。
        // Lockstep with one island already sent the latency update in place.
        let latency_here = detached || self.islands.several();
        self.hand_back();
        // 官方 `ServerLevel.tick` 开头读的是上一刻末尾的 `SleepStatus` 和
        // `sleepCounter`；这里把全部已交回岛的计数汇总，下一刻开头用。
        //
        // Official `ServerLevel.tick` reads the previous tick's `SleepStatus`
        // and `sleepCounter`; sum the counts from all handed-back islands for
        // the start of the next tick.
        self.aggregate_sleep();
        // 混合驱动：这一轮的岛落地以后，排着队的事能做的先做（都按时的时候队列是空的）。
        // Hybrid: once this round's islands have landed, run what was held
        // (nothing is held when every island is on time).
        self.drain_held();
        if latency_here {
            // 延迟广播是主控的帧：齐步时在岛的同步阶段里原位发；分离时岛交回之后发，
            // 和这一刻岛写的帧同一次刷出。多岛时发给所有岛。
            // The latency update is a master frame: lockstep sends it in place in
            // the island's sync phase; detached sends it after the hand-back, in
            // the same flush as the island's frames; to every island with several.
            if let Some(frames) = self.master.global.tick_latency_broadcast() {
                self.broadcast_all(&frames);
            }
        }
        let flush_bytes: u64 = self
            .islands
            .parked_mut()
            .map(|island| island.region.links.flush())
            .sum();
        self.profiler().record(Metric::FlushBytes, flush_bytes);
        self.profiler().lap(Phase::Flush);
        // 运行中每 6000 tick 自动保存一次（官方 `autoSave`，`save-on` 控制开关）。
        // Run-time autosave every 6000 ticks (official `autoSave`, gated by save-on).
        if self.master.director_tick > 0
            && self.master.director_tick.is_multiple_of(6_000)
            && self
                .master
                .save
                .as_ref()
                .is_some_and(|save| save.auto_save())
        {
            // 混合驱动下有岛不在主控手里时，等全部岛停放再存（计划 5.5.5）。
            // Under the hybrid drive, wait for every island when some are away (plan 5.5.5).
            self.run_or_hold(Box::new(Self::autosave));
        }
        // 运行中匀速给仍加载的脏区块拍快照写盘（D6），`save-off` 时停。
        // 区块平时由这条平摊的 IO 保存，不再等 6000 刻集中刷，世界线程也不等待。
        // Paced run-time snapshots of still-loaded dirty chunks (D6), off during
        // save-off. Chunks are saved by this flat IO stream rather than a
        // 6000-tick burst; the world thread never waits.
        let save_enabled = self
            .master
            .save
            .as_ref()
            .is_some_and(|save| save.auto_save());
        match self.master.drive {
            IslandDrive::Lockstep => self.master.host.save_loaded_dirty(
                &mut self.islands.all_mut(),
                self.master.director_tick,
                save_enabled,
            ),
            IslandDrive::Detached => {
                // 岛在 tick 线程上这一刻的最后一件事是照上次交回时的名单编码、取走新的改动；
                // 主控收下快照、判脏，给出下一份名单（多岛时报告接成一份、名单按岛分开）。
                // The island's last step on its tick thread encoded the list from
                // the last hand-back and took the new changes; the master takes
                // the snapshots, judges, and gives the next list (with several
                // islands the reports are joined and the list split by island).
                // 混合驱动下：落后的岛落地时交出来、存在主控手里的报告先接上；名单里归属
                // 的岛不在主控手里的那部分留着，等它落地（计划 5.5.5）。
                // Under the hybrid drive the reports stashed when lagging islands
                // landed come first, and the part of the list owned by islands
                // that are away is kept until they land (plan 5.5.5).
                let mut report = std::mem::take(&mut self.master.hybrid.reports);
                for island in self.islands.parked_mut() {
                    report.absorb(std::mem::take(&mut island.save_report));
                }
                let plan =
                    self.master
                        .host
                        .plan_saves(report, self.master.director_tick, save_enabled);
                self.assign_save_plan(plan);
                for (flush, source) in std::mem::take(&mut self.master.deferred_save_all) {
                    self.run_or_hold(Box::new(move |world: &mut Self| {
                        world.save_all(flush, source);
                    }));
                }
            }
        }
        if self.master.global.admin.stop_requested {
            let flow = self.shut_down();
            self.profiler().lap(Phase::TickTail);
            return flow;
        }
        // 名单变更在这一 tick 末尾统一发布：一条命令改多个目标只写一次盘。
        let mut changes = std::mem::take(&mut self.master.hybrid.admin);
        for island in self.islands.parked_mut() {
            changes.merge(crate::command::admin::take_changes(&mut island.region));
        }
        if changes.any() {
            self.publish_admin(changes);
        }
        // 这一刻的拆合、跨岛的玩家在全部岛停放、交回处理完之后做（分离驱动的唯一时点）。
        // This tick's structural changes and island crossings, once every
        // island is parked and handed back (the only point under the detached drive).
        self.settle_islands();
        self.profiler().lap(Phase::TickTail);
        ControlFlow::Continue(())
    }

    /// 把名单快照交给连接层写盘。通道不阻塞，满了只记日志。
    fn publish_admin(&self, changes: crate::admin::AdminChanges) {
        let Some(sender) = &self.master.global.admin_messages else {
            return;
        };
        let update = crate::admin::AdminUpdate {
            snapshot: self.master.global.admin.snapshot(),
            changes,
        };
        if sender
            .send(crate::admin::AdminMessage::Update(update))
            .is_err()
        {
            warn!("admin list update could not reach the connection layer");
        }
    }

    /// 取出这一刻要处理的全部入站消息，按序号归并成一条（岛的实施计划第 6 节第 4 步）。
    /// 返回主控队列的发端是不是已经全没了。
    ///
    /// 先取主控队列，再取玩家的队列：任何一条在某条主控消息之前发出的玩家消息，
    /// 这时一定已经在玩家队列里了（同一账号的 `Leave` 在下一次 `Join` 之前发出）。
    /// 这一批里 `Join` 带来的新玩家的队列也一起取，进服后同一刻的请求和现在一样同一刻处理。
    ///
    /// Collect this tick's inbound messages and merge them by number (island plan,
    /// section 6, step 4). Returns whether every master sender is gone. The master
    /// queue is drained first, then the player queues: any player message sent
    /// before a drained master message is already in its queue (an account's
    /// `Leave` is sent before its next `Join`). Queues brought in by this batch's
    /// `Join`s are drained too, so requests right after a join still run this tick.
    fn collect_inbound(&mut self) -> (Vec<Sequenced>, bool) {
        // 等岛交回时先取出来的主控消息排在前面（它们到得更早），归并按序号排，顺序不变。
        // Messages taken out while waiting for the island come first (they
        // arrived earlier); the merge orders by number anyway.
        let mut batch = std::mem::take(&mut self.master.held_inbound);
        let disconnected = self.master.inbox.drain_into(&mut batch);
        let mut players = Vec::new();
        for message in &batch {
            if let Inbound::Join(request) = &message.message {
                request.inbox.drain_into(&mut players);
            }
        }
        // 只取停放着的岛上的玩家队列：不在主控手里的岛上的玩家的消息留在队列里，岛落地
        // 以后再取（计划 5.5.2）。
        // Only queues of players on parked islands are drained; messages of
        // players on islands that are away wait in their queues (plan 5.5.2).
        for island in self.islands.parked() {
            for player in island.region.players.values() {
                player.inbox.drain_into(&mut players);
            }
        }
        for (_, inbox) in &self.master.refused_inboxes {
            inbox.drain_into(&mut players);
        }
        self.master.hybrid.drain_joining(&mut players);
        batch.append(&mut players);
        crate::inbound::merge(&mut batch);
        (batch, disconnected)
    }

    /// 按序号处理这一 tick 之前收到的所有消息。
    fn process_inbox(&mut self) -> ControlFlow<()> {
        // 先全部取出再处理：配额要处理完才归还，连接任务补不进新消息，所以这一步一定会结束。
        let (messages, disconnected) = self.collect_inbound();

        self.profiler()
            .record(Metric::InboundMessages, messages.len() as u64);
        for Sequenced { message, .. } in messages {
            if matches!(message, Inbound::Shutdown) {
                return self.shut_down();
            }
            // 每条消息现在能做就做；它要的岛没停放时排进来源自己的队列（混合驱动，
            // 计划 5.5.2）。别的驱动下全部岛都停放着，永远当场做。
            // Each message runs now if it can; when an island it needs is not
            // parked it queues behind its source (hybrid drive, plan 5.5.2).
            // Under the other drives every island is parked and it always runs.
            let request = matches!(message, Inbound::Request { .. });
            self.accept_message(message);
            if request {
                // 玩家请求只经端口写全局状态，端口应用时自己重发快照。
                // Player requests write global state only through the port,
                // which republishes the snapshot itself when applying.
                continue;
            }
            // 主控队列的消息（进服、延迟、名单重载、控制台命令）直接改 `Global`：
            // 每条处理完重发快照，下一条读到的和直接读 `Global` 一样。
            // Master-queue messages (join, latency, list reload, console commands)
            // write `Global` directly: republish after each so the next message
            // reads exactly what `Global` holds.
            self.publish_global();
        }
        if disconnected {
            return self.shut_down();
        }
        ControlFlow::Continue(())
    }

    /// 进服的前一半（只碰主控）：重复编号、名单复查、权限、读玩家存档，然后按出生点
    /// 挂上玩家票。返回 `None` 表示进服到此为止（重复或者被拒）。后一半见
    /// [`Self::finish_join`]；两半之间目标岛可能还没停放（混合驱动，计划 5.5.2），
    /// 这名玩家记在「正在进服」里，票不会被当成离开撤掉。
    ///
    /// The first half of a join (master only): duplicate id, list re-check,
    /// permissions, the player save, then the player ticket at the spawn.
    /// `None` means the join ends here (duplicate or refused). The second
    /// half is [`Self::finish_join`]; in between the target island may not be
    /// parked yet (hybrid drive, plan 5.5.2), and the player is recorded as
    /// joining so the ticket is not dropped as a leave.
    fn begin_join(&mut self, mut request: JoinRequest) -> Option<Box<island_hybrid::PendingJoin>> {
        let id = request.id;
        if self.islands.island_of_player(id).is_some() || self.master.hybrid.joining.contains(&id) {
            warn!(player = id.0, "ignoring a second join with the same id");
            return None;
        }
        // 进服序列里读的世界设置都来自全局快照（岛的实施计划 4.1：进服由岛做）；每条主控
        // 消息处理完都重发过，所以这里现取的一份和岛上那份相同。
        // The join sequence reads world settings from the global snapshot
        // (plan 4.1); it is republished after every master message, so a
        // fresh view equals the islands' copy.
        let view = self.master.global.view();
        // 连接层检查过一次，但封禁命令可能在两次检查之间改了名单。
        // 世界线程按自己的权威副本复查，理由和登录阶段完全一样。
        let refusal = if self.master.authoritative_admin {
            self.master.global.admin.login_refusal(
                request.uuid,
                &request.peer_ip,
                self.clock_millis(),
                request.online_at_login,
                self.max_players() as u32,
            )
        } else {
            None
        };
        if let Some(refusal) = refusal {
            let reason = crate::command::admin::refusal_message(&refusal);
            if self.islands.is_home() {
                self.islands.region.links.add(id, request.sink);
                self.islands.region.links.kick(id, &reason);
            } else {
                // 家岛不在主控手里：用一份临时的链路发完拒绝原因就关。
                // The home island is away: a temporary link sends the reason and closes.
                let mut links = crate::links::Links::default();
                links.add(id, request.sink);
                links.kick(id, &reason);
            }
            self.master.refused_inboxes.push((id, request.inbox));
            return None;
        }
        let (permission_level, is_op) = if self.master.authoritative_admin {
            let op = self.master.global.admin.op(request.uuid);
            (op.map_or(0, |entry| entry.level), op.is_some())
        } else {
            (request.permission_level, request.is_op)
        };
        // 官方 `CachedUserNameToIdResolver.add` 在进服时记下档案。
        self.master
            .global
            .admin
            .remember_player(&crate::admin::NameAndId {
                uuid: request.uuid,
                name: request.name.clone(),
            });

        // 恢复玩家存档：位置取整到方块、朝向、游戏模式、物品栏。
        // 位置的小数部分丢掉（不额外补发位置包），见 README 已知限制。
        // 离开时的写交给 IO 线程异步做，重进可能早于落盘：优先用世界线程
        // 记下的那份"已交给 IO、还没写完"的内存副本，而不是连接层读到的盘上旧字节。
        // Restore from the player save: block-rounded position, rotation, game
        // mode and inventory. The sub-block fraction is dropped (no extra
        // position packet); see README known limitations. A leave is written
        // asynchronously, so a rejoin can beat the disk: prefer the world
        // thread's in-flight in-memory copy over the connection-layer disk bytes.
        let uuid_string = player_uuid_string(request.uuid.as_bytes());
        let player_bytes = self
            .master
            .host
            .pending_player(&uuid_string)
            .map(<[u8]>::to_vec)
            .or_else(|| request.player_data.take());
        let restored = player_bytes.and_then(|bytes| match PlayerData::from_gzip(&bytes) {
            Ok(data) => Some(data),
            Err(error) => {
                warn!(%error, "could not decode player data; treating as a new player");
                None
            }
        });
        let game_mode = restored
            .as_ref()
            .map(|data| game_mode_from_id(data.game_type))
            .unwrap_or(view.default_game_mode);
        let join_spawn = match &restored {
            Some(data) => RespawnData::of(
                BlockPos::new(
                    data.position[0] as i32,
                    data.position[1] as i32,
                    data.position[2] as i32,
                ),
                data.rotation[0],
                data.rotation[1],
            ),
            None => view.spawn,
        };
        let difficulty_frames = view.difficulty_frames();
        let spawn = join_spawn.pos;
        let spawn_chunk = ChunkPos::new(spawn.x >> 4, spawn.z >> 4);
        self.master.hybrid.joining.insert(id);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: Some(spawn_chunk),
        }]);
        Some(Box::new(island_hybrid::PendingJoin {
            request,
            permission_level,
            is_op,
            restored,
            game_mode,
            join_spawn,
            difficulty_frames,
            spawn_chunk,
        }))
    }

    /// 进服的后一半：把进服的 10 步串起来，全部写进这名玩家的同一条出站缓冲
    /// （`coverage.md` 4.1）。目标岛不在主控手里、或者要新建却没有停放着的岛可以照抄时
    /// 原样退回，等它停放再做（混合驱动，计划 5.5.2）。
    ///
    /// The second half of a join: the ten join steps, all written into this
    /// player's one outbound buffer. If the target island is away, or must be
    /// created with no parked island to copy, the join is handed back to wait
    /// (hybrid drive, plan 5.5.2).
    fn finish_join(
        &mut self,
        pending: Box<island_hybrid::PendingJoin>,
    ) -> Result<(), Box<island_hybrid::PendingJoin>> {
        // 多岛时进服就是到达（计划 4.1）：玩家的票已经挂上，分区器给出出生点所在的岛
        // （可能刚建的空岛），玩家直接放进那个岛。单岛时就是家岛。
        // With several islands a join is an arrival (plan 4.1): the ticket is
        // placed, the partitioner names the spawn's island (maybe a new empty
        // one) and the player goes straight there. Home with one island.
        let target = self
            .master
            .host
            .island_of(pending.spawn_chunk)
            .unwrap_or(HOME_REGION);
        if !self.islands.can_ensure(target) {
            return Err(pending);
        }
        let island_hybrid::PendingJoin {
            mut request,
            permission_level,
            is_op,
            restored,
            game_mode,
            join_spawn,
            difficulty_frames,
            spawn_chunk,
        } = *pending;
        let id = request.id;
        let request_advancements = request.player_advancements.clone();
        self.master.hybrid.joining.remove(&id);
        let uuid = request.uuid;
        let entry = RosterEntry {
            uuid,
            name: request.name.clone(),
            // 皮肤只有名单要用，直接搬过来。
            properties: std::mem::take(&mut request.properties),
            game_mode,
            latency: 0,
            show_hat: shows_hat(&request.client),
            chat_session: None,
        };
        // 混合驱动下留一个第二出口：这名玩家所在的岛要是卡死被判死，主控靠它把他断开
        // （计划 5.6.2）。
        // Under the hybrid drive keep a second exit: if this player's island
        // gets stuck and is declared dead, the master disconnects them through
        // it (plan 5.6.2).
        if self.hybrid()
            && let Some(exit) = request.sink.second_handle()
        {
            self.master.hybrid.death.exits.insert(id, exit);
        }
        let origin = self.master.host.island_origin(target, spawn_chunk);
        let region = &mut self.islands.ensure(target, origin).region;
        let hashed_seed = region.global.hashed_seed();
        // 官方 `Player.readAdditionalSaveData` 把 `XpP` / `XpLevel` / `XpTotal` 直接读进
        // 玩家，再由 `placeNewPlayer` 发第一包 `set_experience`。
        //
        // Official `Player.readAdditionalSaveData` reads `XpP` / `XpLevel` /
        // `XpTotal` into the player, then `placeNewPlayer` sends the first
        // `set_experience` packet.
        let join_experience = restored
            .as_ref()
            .map(|data| crate::experience::Experience {
                level: data.xp_level,
                total: data.xp_total,
                progress: data.xp_p,
            })
            .unwrap_or_default();
        region.add_player(
            request,
            JoinSettings {
                game_mode,
                difficulty_frames: &difficulty_frames,
                spawn: join_spawn,
                hashed_seed,
                permission_level,
                is_op,
                experience: join_experience,
                new_player: restored.is_none(),
            },
        );
        let now = region.now_millis();
        region.last_action_millis.insert(id, now);

        // Tab 名单、进服消息、计分板和 Boss 栏是主控的帧：别的岛上的玩家借过来一起收。
        // 当场执行：这些帧和后面的 `start_loading_chunks` 写同一条缓冲，先后是进服序列的一部分。
        // 混合驱动下不在主控手里的岛上的玩家晚一点收到名单和进服消息（延后投递）。
        // The tab list, join message, scoreboard and boss bars are master
        // frames: players on other islands are borrowed to receive them too.
        // Runs at once: these frames and `start_loading_chunks` below share one
        // buffer, and their order is part of the join sequence. Under the
        // hybrid drive players on islands that are away get the list entry and
        // the join message later (deferred delivery).
        self.on_master(target, Borrow::All, |global, region| {
            if let Some(frames) = global.tab_list() {
                region.links.send_frames(id, &frames);
            }
            let frames = global.add_player(id, entry);
            region.links.broadcast(&frames);

            region.send_own_entity_data(id);

            if let Some(frames) = global.joined_message(id) {
                // 官方 `PlayerList.broadcastSystemMessage` 逐玩家看可见性；进服消息跳过本人。
                //
                // Official `PlayerList.broadcastSystemMessage` checks visibility per player;
                // the join message skips the joining player.
                crate::chat::broadcast_system_message(region, &frames, Some(id));
            }

            // 官方 `ServerScoreboard.getStartTrackingPackets` 和 `CustomBossEvents.onPlayerConnect`
            // 都在进服时只发给这名玩家。
            //
            // Official `ServerScoreboard.getStartTrackingPackets` and
            // `CustomBossEvents.onPlayerConnect` both go only to the joining player.
            let frames = global.scoreboard().join_frames();
            region.links.send_frames(id, &frames);
            global.boss_bars_mut().on_join(region, id, uuid);
        });
        let region = &mut self.islands.get_mut(target).expect("刚放进去").region;

        // `set_default_spawn_position` 发的是世界出生点（`LevelData`），不是玩家存档里的位置。
        // 玩家位置走上面的 `JoinSettings.spawn`（存档），区块中心走 `player.chunk_pos()`。
        // `set_default_spawn_position` carries the world spawn from `LevelData`, not
        // the saved player position. The player's position comes from
        // `JoinSettings.spawn` above (the save), and the chunk center from `player.chunk_pos()`.
        region.start_loading_chunks(id);

        // `XpSeed == 0`（没有存档或存档里写 0）时按官方从玩家随机源抽一次种子；
        // 保存过非 0 种子时不取随机数。两种情况的后续随机序列都和官方一致。
        //
        // Official draws a seed from the player's random source only when
        // `XpSeed == 0` (no save, or a save with zero); a saved non-zero seed
        // consumes no random number. Either way the following sequence matches.
        if let Some(player) = region.players.get_mut(&id) {
            restore_experience_seed(player, restored.as_ref());
        }
        // 物品栏 / 末影箱 / 朝向等运行时字段在玩家建好之后写回去。
        if let Some(data) = &restored
            && let Some(player) = region.players.get_mut(&id)
        {
            restore_inventory(player, data);
            crate::player_effects::send_active(region, id);
        }
        // `loadAndSpawnEnderPearls`：把存下来的飞行珍珠放回实体表并注册到玩家身上。
        if let Some(data) = &restored {
            crate::ender_pearl::load(region, id, data);
            // `PrepareSpawnTask` 的 `loadAndSpawnParentVehicle`：把根载具连整棵
            // 乘客树放回世界，按 `Attach` 重新骑上。
            if let Some(tag) = &data.root_vehicle {
                crate::riding::load_root_vehicle(region, id, tag);
            }
        }
        // 官方 `PlayerList.placeNewPlayer` 构造 `PlayerAdvancements` 后由首 tick 发全量包。
        if let Some(player) = region.players.get_mut(&id) {
            player.advancements = crate::advancements::join_state(request_advancements.as_deref());
        }
        Ok(())
    }

    /// 消费 `save-all` / `save-on` / `save-off` 压进来的请求（命令动作不碰 IO）。
    fn consume_save_commands(&mut self) {
        let requests = std::mem::take(&mut self.master.global.save_requests);
        for request in requests {
            match request {
                SaveCommand::SaveAll { flush, source } => {
                    if self.master.drive == IslandDrive::Detached {
                        // 岛还在跑：等它这一刻交回、停放之后再过栅栏（第 2 节第 23 行）。
                        // The island is running: pass the barrier once it parks.
                        self.master.deferred_save_all.push((flush, source));
                    } else {
                        self.save_all(flush, source);
                    }
                }
                SaveCommand::SaveOn => {
                    if let Some(save) = &mut self.master.save {
                        save.set_auto_save(true);
                    }
                }
                SaveCommand::SaveOff => {
                    if let Some(save) = &mut self.master.save {
                        save.set_auto_save(false);
                    }
                }
            }
        }
    }

    /// 照官方 saveEverything：先给所有脏的已加载区块拍快照交给 IO 线程，
    /// 再存玩家，最后把 level.dat / data/ 的字节交给 IO 线程。
    /// 世界线程只编码、不碰磁盘，也不阻塞等 IO（R5 / F3）。岛这时一定停放着。
    /// Like official saveEverything: snapshot dirty loaded chunks, save
    /// players, then hand level.dat/data bytes to the IO thread. The world
    /// thread only encodes, never blocks on IO. The island is parked here.
    fn save_all(&mut self, flush: bool, source: PlayerId) {
        debug_assert!(self.islands.all_parked());
        self.master
            .host
            .snapshot_all_dirty(&mut self.islands.all_mut(), self.master.director_tick);
        self.save_all_players();
        let raid_tables = self.raid_tables();
        let master = &mut self.master;
        let files = master.save.as_mut().map(|save| {
            save.encode_saves(&master.global, &raid_tables, crate::admin::now_millis())
        });
        let encoded = match files {
            Some(Ok(files)) => {
                self.master.host.submit_file_writes(files);
                true
            }
            Some(Err(error)) => {
                // 编码就失败了：绝不能报成功，直接发 commands.save.failed（F9）。
                warn!(%error, "save-all could not encode level data");
                false
            }
            None => true,
        };
        if !encoded {
            self.send_save_failure(source);
            return;
        }
        // 这次请求的全部快照都交给 IO 线程之后才排一道自己的栅栏。
        // 阻塞的 flush() 只允许出现在关服；flush 标志按官方解析保留。
        // Queue this request's own barrier after all of its snapshots
        // reach IO. Blocking flush() remains shutdown-only.
        debug!(flush, "save-all handed its writes to the bounded queue");
        if self.master.host.begin_save_all(source).is_none() {
            // 这一局没接磁盘：没有 IO 要等，直接发。
            self.send_save_success(source);
        } else {
            // 取回刚登记好的栅栏，等自己的 IO 回执（中 9）。
            for (seq, owner) in self.master.host.take_ready_save_barriers() {
                self.master.pending_save_success.push((seq, owner));
            }
        }
    }

    /// 发 `commands.save.success`（广播给 OP），和命令侧原来的一致。
    /// Send `commands.save.success` (broadcast to ops), as the command did.
    fn send_save_success(&mut self, source: PlayerId) {
        // 发起人和别的岛上的 OP 都要看得见：等全部岛停放（计划 5.5.2）。
        // The sender and ops on other islands must all be visible: wait for
        // every island (plan 5.5.2).
        self.run_or_hold(Box::new(move |world: &mut Self| {
            let island = world.island_for_feedback(source);
            world.on_master(island, Borrow::All, |_, region| {
                crate::command::feedback::success(
                    region,
                    source,
                    Component::translate("commands.save.success", []),
                    true,
                );
            });
        }));
    }

    /// 收回已到达的 IO 栅栏回执，按回执的 `ok` 发等在上面的 `save-all` 消息：
    /// 成功才发 `commands.save.success`，失败发 `commands.save.failed`（F9 /
    /// 官方 `SaveAllCommand` 的 `ERROR_FAILED`）。两者都只在保存真正完成后发。
    /// Release waiting `save-all` messages from the IO barrier acks: success
    /// only sends `commands.save.success`, a failed window sends
    /// `commands.save.failed` (F9 / official `SaveAllCommand.ERROR_FAILED`).
    /// Both are sent only after the save really completed.
    fn release_flushed_save_success(&mut self, flushed: Vec<(u64, bool)>) {
        if flushed.is_empty() {
            return;
        }
        let mut remaining = Vec::new();
        for (seq, source) in std::mem::take(&mut self.master.pending_save_success) {
            if let Some(&(_, ok)) = flushed.iter().find(|(got, _)| *got == seq) {
                if ok {
                    self.send_save_success(source);
                } else {
                    error!(?source, "save-all failed: an IO write in the window failed");
                    self.send_save_failure(source);
                }
            } else {
                remaining.push((seq, source));
            }
        }
        self.master.pending_save_success = remaining;
    }

    /// 发 `commands.save.failed`（红色系统消息），照官方 `SaveAllCommand`。
    /// Send `commands.save.failed` (red system message) like official
    /// `SaveAllCommand` when `saveEverything` returns false.
    fn send_save_failure(&mut self, source: PlayerId) {
        self.run_or_hold(Box::new(move |world: &mut Self| {
            let failure = crate::command::feedback::fail("commands.save.failed");
            let island = world.island_for_feedback(source);
            world.on_master(island, Borrow::All, |_, region| {
                crate::command::feedback::report(region, source, "save-all", &failure);
            });
        }));
    }

    /// 存档一名在线玩家。文件写交给 IO 线程，世界线程不等待。
    fn save_one_player(&mut self, id: PlayerId) {
        // 官方 `PlayerList.remove`：存盘发生在 `stopRiding` 之前，所以这时
        // `RootVehicle` 还能看到玩家骑着的整棵树。
        let root_vehicle = self
            .islands
            .parked()
            .find_map(|island| crate::riding::root_vehicle_save(&island.region, id));
        let player = self
            .islands
            .parked()
            .find_map(|island| island.region.players.get(&id));
        let Some(player) = player else {
            return;
        };
        let encoded = encode_player_save(player, root_vehicle);
        let text = crate::advancements::encode_save(player);
        if let Some((uuid, bytes)) = encoded {
            self.master.host.submit_player_write(&uuid, bytes);
        }
        if let Some(root) = &self.master.world_dir
            && let Err(error) =
                crate::save_session::save_player_advancements(root, player.uuid.as_bytes(), &text)
        {
            tracing::warn!(%error, "could not save player advancements");
        }
    }

    /// 存档所有在线玩家（自动保存、关服、`save-all`）。
    fn save_all_players(&mut self) {
        // 先把所有存档编出来，避免遍历 `region.players` 时又可变借 `host`。
        // Encode every save first so we don't hold a borrow of `region.players`
        // while mutably borrowing `host`.
        // 所有岛的玩家按编号排，和只有一个岛时的顺序一样。
        // Every island's players by id, the same order as with one island.
        let mut players: Vec<&crate::player::Player> = self
            .islands
            .parked()
            .flat_map(|island| island.region.players.values())
            .collect();
        players.sort_unstable_by_key(|player| player.id);
        let roots: std::collections::BTreeMap<PlayerId, Option<mc_bamboo_nbt::Tag>> = self
            .islands
            .parked()
            .flat_map(|island| {
                island
                    .region
                    .players
                    .keys()
                    .map(|id| (*id, crate::riding::root_vehicle_save(&island.region, *id)))
                    .collect::<Vec<_>>()
            })
            .collect();
        let encoded: Vec<([u8; 16], String, Vec<u8>)> = players
            .into_iter()
            .filter_map(|player| {
                let root_vehicle = roots.get(&player.id).cloned().flatten();
                let (uuid, bytes) = encode_player_save(player, root_vehicle)?;
                Some((*player.uuid.as_bytes(), uuid, bytes))
            })
            .collect();
        // 进度档案：官方 `PlayerAdvancements.save` 是 UTF-8 文本，不做 `_old` 轮换。
        let advancement_text: Vec<([u8; 16], String)> = self
            .islands
            .parked()
            .flat_map(|island| island.region.players.values())
            .map(|player| {
                (
                    *player.uuid.as_bytes(),
                    crate::advancements::encode_save(player),
                )
            })
            .collect();
        for (_, uuid, bytes) in encoded {
            self.master.host.submit_player_write(&uuid, bytes);
        }
        if let Some(root) = &self.master.world_dir {
            for (uuid, text) in advancement_text {
                if let Err(error) =
                    crate::save_session::save_player_advancements(root, &uuid, &text)
                {
                    tracing::warn!(%error, "could not save player advancements");
                }
            }
        }
    }

    /// 处理离开，顺序见 `coverage.md` 4.2。可以重复调用：世界里没有这名玩家时直接忽略，
    /// 连接任务还没发 `Join` 就结束时走的就是这条路。
    fn on_leave(&mut self, id: PlayerId) {
        self.master
            .refused_inboxes
            .retain(|(refused, _)| *refused != id);
        self.master.transit.remove(&id);
        self.master.hybrid.death.exits.remove(&id);
        let Some(island) = self.islands.island_of_player(id) else {
            return;
        };
        let region = &mut self.islands.get_mut(island).expect("在表里").region;
        // 连接已经结束了，后面的包都不用写给它。
        region.links.remove(id);
        region.last_action_millis.remove(&id);
        region.peer_ips.remove(&id);

        // 「离开了游戏」和 Tab 名单是全局的，经端口交给主控（岛的实施计划 4.1）；多岛时
        // 经借玩家的入口当场发给所有岛的玩家，和一个岛时的先后相同。
        // "Left the game" and the tab list are global; they go to the master
        // through the port (island plan 4.1), with several islands through the
        // borrowing entry, at once and in the single-island order.
        self.with_players(island, Borrow::All, |port, region| {
            port.submit(region, GlobalEffect::LeaveMessage { player: id });
        });
        let region = &mut self.islands.get_mut(island).expect("在表里").region;
        entity_sync::on_leave(region, id);
        entity_tracking::on_leave(region, id);
        // 玩家断开时官方不回 `container_close`，光标上的东西直接丢弃。
        menu::on_leave(region, id);
        lectern::on_leave(region, id);
        // 官方 `PlayerList.remove` 在移除玩家前先存盘；`PlayerList.save` /
        // `saveAll` 都不看 `noSave`，所以 `/save-off` 期间离开照存（F7）。
        // Official `PlayerList.remove` saves before removing; `PlayerList.save`
        // / `saveAll` ignore `noSave`, so a leave during `/save-off` still saves.
        // `saveEnderPearls` 用当下还在飞的珍珠，先把快照刷到这一刻。
        crate::ender_pearl::refresh_snapshot(region, id);
        self.save_one_player(id);
        let region = &mut self.islands.get_mut(island).expect("在表里").region;
        // 存完之后按 `UNLOADED_WITH_PLAYER` 把珍珠从世界里移除。
        crate::ender_pearl::remove_on_leave(region, id);
        crate::player_fishing::remove_on_leave(region, id);
        // 官方 `PlayerList.remove`：只有一名玩家乘客时先 `stopRiding`，再把根载具
        // 和整棵树按 `UNLOADED_WITH_PLAYER` 移除；存档里已经留了 `RootVehicle`。
        if let Some(vehicle) = region.players.get(&id).and_then(|p| p.riding.vehicle) {
            let root = crate::riding::root_vehicle(region, vehicle);
            if crate::riding::player_passenger_count(region, root) == 1 {
                crate::riding::stop_riding(region, id);
                let mut tree = vec![root];
                let mut all = Vec::new();
                while let Some(uid) = tree.pop() {
                    all.push(uid);
                    tree.extend(
                        crate::riding::passengers(region, uid)
                            .into_iter()
                            .filter(|&p| p >= mc_bamboo_blocks::entity::FIRST_ENTITY_ID),
                    );
                }
                for uid in all {
                    region.entities.remove(uid);
                }
            }
        }
        region.players.remove(&id);
        region.player_grid.remove(id);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: None,
        }]);
        // 撤票可能已经让这个岛合并进别的岛；端口交给玩家原来那片区域现在所属的岛。
        // Dropping the ticket may have merged the island away; use whichever
        // island holds it now.
        let island = if self.islands.contains(island) {
            island
        } else {
            // 合并只发生在停放着的岛之间，吸收方一定停放着；家岛停放着就是它。
            // Merges only join parked islands, so the absorber is parked; home if it is.
            self.islands.template().unwrap_or(HOME_REGION)
        };
        self.with_players(island, Borrow::All, |port, region| {
            port.submit(region, GlobalEffect::Left { player: id });
        });
    }

    /// 把一条请求交给负责它的模块。被踢的玩家要等 `Leave` 才离开世界，这段时间它的请求都忽略。
    fn on_request(&mut self, player: PlayerId, request: PlayRequest) {
        let Some(island) = self.islands.island_of_player(player) else {
            return;
        };
        if !self.region_of(island).links.is_open(player) {
            return;
        }
        crate::command::admin::note_action(self.region_of(island), player, &request);
        match &request {
            PlayRequest::ChatCommand { command, signed } => {
                let command = command.clone();
                let signed = signed.clone();
                // 当场执行：命令的结果（传送、改模式、物品）要排在这名玩家后面的包之前。
                // Runs at once: the outcome (teleport, mode, items) must precede
                // this player's later packets.
                // 碰方块、容器、实体或者读目标脚下方块的命令要整个世界，别的只借玩家
                // （计划 4.5）。
                // Commands touching blocks, containers or entities, or reading
                // the block under a target, need the whole world; the rest only
                // borrow players (plan 4.5).
                let borrow = command::borrow::plan_borrow(&self.islands, island, player, &command);
                self.with_players(island, borrow, |port, region| {
                    command::on_player_command(port, region, player, &command, signed.as_ref());
                });
                return;
            }
            PlayRequest::CommandSuggestion { id, command } => {
                let (transaction, command) = (*id, command.clone());
                // 补全玩家名要看得见所有岛的玩家（和一个岛时一样）。
                // Name completion must see every island's players, as on one island.
                self.with_players(island, Borrow::All, |_, region| {
                    command::on_suggestion(region, player, transaction, &command);
                });
                return;
            }
            PlayRequest::ChatSession(session) => {
                let session = session.clone();
                // 当场执行：新会话决定这名玩家后面的聊天怎么验签。
                // Runs at once: the new session decides how this player's later
                // chat is verified.
                self.with_players(island, Borrow::All, |port, region| {
                    if let Some(update) = secure_chat::on_session(region, player, session) {
                        port.submit(region, GlobalEffect::ChatSession { player, update });
                    }
                });
                return;
            }
            PlayRequest::Chat {
                message,
                signature,
                time_millis,
                salt,
                last_seen,
            } => {
                // 聊天经主控扇出：所有岛的玩家都借过来，每名接收者的签名聊天状态照常由
                // `secure_chat/deliver.rs` 逐个打包（计划第 2 节第 12 行）。
                // Chat fans out through the master: every island's players are
                // borrowed and each receiver's signed-chat state is packed by
                // `secure_chat/deliver.rs` as usual (plan section 2, row 12).
                let (message, signature, time_millis, salt, last_seen) = (
                    message.clone(),
                    signature.clone(),
                    *time_millis,
                    *salt,
                    *last_seen,
                );
                // 当场执行：签名链和确认按这名玩家的包的先后走。
                // Runs at once: the signature chain and acks follow this player's
                // packet order.
                self.with_players(island, Borrow::All, |_, region| {
                    if chat::on_chat(
                        region,
                        player,
                        chat::ChatMessage {
                            text: message,
                            signature: signature.map(|bytes| *bytes),
                            time_millis,
                            salt,
                            last_seen: &last_seen,
                        },
                    ) {
                        chat::detect_chat_rate_spam(region, player);
                    }
                });
                return;
            }
            PlayRequest::SetGameRule { entries } => {
                let entries = entries.clone();
                self.with_players(island, Borrow::All, |port, region| {
                    crate::gamerules::on_set_packet(port, region, player, entries);
                });
                return;
            }
            _ => {}
        }
        let region = &mut self.islands.get_mut(island).expect("在表里").region;
        match request {
            PlayRequest::AcceptTeleportation {
                id,
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                let position = Vec3::new(x, y, z);
                movement::on_accept_teleportation(region, player, id, position, (yaw, pitch));
            }
            PlayRequest::MovePlayer {
                position,
                rotation,
                on_ground,
                horizontal_collision,
            } => movement::on_move_player(
                region,
                player,
                position,
                rotation,
                on_ground,
                horizontal_collision,
            ),
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick,
            } => chunk_tracking::on_chunk_batch_received(region, player, desired_chunks_per_tick),
            PlayRequest::ClientInformation(information) => {
                self.on_client_information(island, player, information);
            }
            PlayRequest::ChatAck { offset } => secure_chat::on_ack(region, player, offset),
            // 聊天命令、建议和会话更新在上面处理过了，这里只是让匹配穷尽。
            //
            // Chat commands, suggestions, and session updates are handled above. This arm only makes the match exhaustive.
            PlayRequest::ChatSession(_)
            | PlayRequest::Chat { .. }
            | PlayRequest::SetGameRule { .. }
            | PlayRequest::ChatCommand { .. }
            | PlayRequest::CommandSuggestion { .. } => {}
            PlayRequest::ClientCommand { action } => {
                if action
                    == mc_bamboo_protocol::packet::play::ClientCommandKind::RequestGameRuleValues
                {
                    crate::gamerules::on_request_values(region, player);
                } else {
                    chat::on_client_command(region, player, action);
                }
            }
            PlayRequest::ChangeGameMode { mode } => self.on_change_game_mode(island, player, mode),
            PlayRequest::SetCommandBlock {
                pos,
                command,
                mode,
                track_output,
                conditional,
                automatic,
            } => crate::command_block::on_set(
                region,
                player,
                crate::command_block::CommandBlockEdit {
                    pos,
                    command,
                    mode,
                    track_output,
                    conditional,
                    automatic,
                },
            ),
            PlayRequest::PlayerInput { flags } => {
                entity_sync::on_player_input(region, player, flags);
            }
            PlayRequest::PlayerCommand { action, data } => {
                entity_sync::on_player_command(region, player, action, data);
            }
            PlayRequest::MoveVehicle {
                position,
                yaw,
                pitch,
                on_ground,
            } => crate::riding::on_move_vehicle(region, player, position, yaw, pitch, on_ground),
            PlayRequest::PaddleBoat { left, right } => {
                crate::riding::set_paddle(region, player, left, right);
            }
            PlayRequest::PlayerAbilities { flags } => {
                entity_sync::on_player_abilities(region, player, flags);
            }
            PlayRequest::Punch => entity_sync::on_punch(region, player),
            PlayRequest::Attack { entity_id } => {
                crate::player_hurt::on_attack(region, player, entity_id);
            }
            PlayRequest::Interact {
                entity_id,
                hand,
                using_secondary_action,
                ..
            } => interaction::on_interact_entity(
                region,
                player,
                entity_id,
                hand,
                using_secondary_action,
            ),
            PlayRequest::SetCarriedItem { slot } => {
                inventory::on_set_carried_item(region, player, slot);
            }
            PlayRequest::SetCreativeModeSlot { slot, stack } => {
                inventory::on_set_creative_mode_slot(region, player, slot, stack);
            }
            PlayRequest::PickItemFromBlock { pos, include_data } => {
                inventory::on_pick_item_from_block(region, player, pos, include_data);
            }
            PlayRequest::PlayerAction {
                action,
                pos,
                face,
                sequence,
            } => interaction::on_player_action(region, player, action, pos, face, sequence),
            PlayRequest::UseItemOn {
                hand,
                pos,
                face,
                cursor,
                inside,
                sequence,
                ..
            } => {
                let packet = UseItemOn {
                    hand,
                    pos,
                    face,
                    cursor,
                    inside,
                    sequence,
                };
                interaction::on_use_item_on(region, player, packet);
            }
            PlayRequest::UseItem {
                hand,
                sequence,
                yaw,
                pitch,
            } => interaction::on_use_item(region, player, hand, sequence, (yaw, pitch)),
            PlayRequest::ContainerClick {
                container_id,
                state_id,
                slot,
                button,
                input,
                changed_slots,
                carried,
            } => menu::on_click(
                region,
                player,
                menu::ClickPacket {
                    container_id,
                    state_id,
                    slot,
                    button,
                    input,
                    changed_slots,
                    carried,
                },
            ),
            PlayRequest::SelectTrade { slot } => menu::on_select_trade(region, player, slot),
            PlayRequest::ContainerClose { .. } => {
                menu::on_close_request(region, player);
                lectern::on_player_close(region, player);
            }
            PlayRequest::RenameItem { name } => menu::on_rename(region, player, &name),
            PlayRequest::ContainerButtonClick {
                container_id,
                button_id,
            } => {
                menu::on_button(region, player, container_id, button_id);
                lectern::on_button(region, player, container_id, button_id);
            }
            PlayRequest::ContainerSlotStateChanged {
                container_id,
                slot_id,
                new_state,
            } => menu::on_slot_state_changed(region, player, container_id, slot_id, new_state),
            PlayRequest::SignUpdate { pos, lines, slot } => {
                sign::on_sign_update(region, player, pos, lines, slot);
            }
            PlayRequest::EditBook { slot, pages, title } => {
                book::on_edit_book(region, player, slot, pages, title);
            }
            // 连接任务把这两种转过来。其余 `Other` 它自己丢掉。
            //
            // The connection forwards these two. It drops every other `Other` packet.
            PlayRequest::Other(id) if id == serverbound::CLIENT_TICK_END => {
                movement::on_client_tick_end(region, player);
            }
            PlayRequest::Other(id) if id == serverbound::PLAYER_LOADED => {
                movement::on_player_loaded(region, player);
            }
            PlayRequest::SeenAdvancements(seen) => {
                crate::advancements::on_seen_advancements(region, player, &seen);
            }
            PlayRequest::KeepAlive(_) | PlayRequest::PingRequest(_) | PlayRequest::Other(_) => {}
        }
    }

    /// `handleChangeGameMode`。权限不够时只记日志。
    ///
    /// `handleChangeGameMode`. Without permission this only logs.
    fn on_change_game_mode(
        &mut self,
        island: mc_bamboo_world::director::RegionId,
        id: PlayerId,
        mode: GameMode,
    ) {
        let Some((allowed, name)) = self.region_of(island).players.get(&id).map(|player| {
            (
                player.permission_level >= mc_bamboo_command::permission::GAMEMASTERS,
                player.name.clone(),
            )
        }) else {
            return;
        };
        if !allowed {
            warn!(
                player = %name,
                ?mode,
                "tried to change game mode without required permissions"
            );
            return;
        }
        // 当场执行：模式决定这名玩家后面的包（飞行、挖掘）怎么处理。
        // Runs at once: the mode decides how this player's later packets
        // (flight, digging) are handled.
        self.with_players(island, Borrow::All, |port, region| {
            command::change_own_mode(port, region, id, mode);
        });
    }

    /// 客户端的设置变了。视距由下一次跟踪阶段生效，皮肤外层和主手由实体同步发出去。
    fn on_client_information(
        &mut self,
        island: mc_bamboo_world::director::RegionId,
        id: PlayerId,
        information: ClientInformation,
    ) {
        let Some(player) = self.region_of(island).players.get_mut(&id) else {
            return;
        };
        player.client = information;

        let show = player.shows_hat();
        self.with_players(island, Borrow::All, |port, region| {
            port.submit(region, GlobalEffect::Hat { player: id, show });
        });
    }

    /// 岛写全局状态的口子（按 [`Master::port_mode`]）和岛本身。
    ///
    /// The island's port for global writes (per [`Master::port_mode`]) and the
    /// island itself.
    fn island_port(
        &mut self,
        island: mc_bamboo_world::director::RegionId,
    ) -> (GlobalPort<'_>, &mut Region) {
        // 只有一个岛时才用：多岛时一律经借玩家的入口 [`Self::with_players`]。
        // One island only: with several everything goes through [`Self::with_players`].
        debug_assert!(!self.islands.several());
        let island = self.islands.get_mut(island).expect("岛在表里");
        let port = match self.master.port_mode {
            PortMode::Direct => GlobalPort::Direct(&mut self.master.global),
            PortMode::Queued => GlobalPort::Queued(&mut island.effects),
        };
        (port, &mut island.region)
    }

    /// 停放着的岛 `island` 的 `Region`。
    fn region_of(&mut self, island: mc_bamboo_world::director::RegionId) -> &mut Region {
        &mut self.islands.get_mut(island).expect("岛在表里").region
    }

    /// 收包阶段处理一条和别的岛上的玩家有关的请求（命令、聊天、会话、规则、模式、帽子）。
    /// 只有一个岛时就是原来那样经端口；多岛时经借玩家的唯一入口 [`Self::on_master`]，用直接
    /// 端口当场执行（计划 4.4：全局类在主控执行；可能跨岛的选择器看得见所有岛的玩家），
    /// 帧经借来的链路发给各岛的玩家。
    ///
    /// Handle a request that involves players of other islands (commands,
    /// chat, chat sessions, game rules, game mode, hat) during the inbound
    /// phase. With one island it goes through the port as before; with
    /// several it goes through the single borrowing entry [`Self::on_master`]
    /// and runs with the direct port (plan 4.4), frames reaching each island's
    /// players through the borrowed links.
    fn with_players(
        &mut self,
        island: mc_bamboo_world::director::RegionId,
        borrow: crate::island::Borrow,
        work: impl FnOnce(&mut GlobalPort<'_>, &mut Region),
    ) {
        if !self.islands.several() {
            let (mut port, region) = self.island_port(island);
            work(&mut port, region);
            return;
        }
        self.on_master(island, borrow, |global, region| {
            let mut port = GlobalPort::Direct(global);
            work(&mut port, region);
        });
    }

    /// 岛交回：主控按提交顺序应用排队实现攒下的全局改动。放在岛这一刻的活做完、
    /// 出站缓冲刷出之前，主控产生的帧（命令反馈、Tab 名单）和岛的帧同一刻发出。
    /// 直接实现下队列是空的，这里什么都不做。
    ///
    /// Island hand-back: the master applies the global changes the queued port
    /// collected, in submission order. It sits after the island's work for the
    /// tick and before the outbound flush, so frames the master produces
    /// (command feedback, the tab list) go out in the same tick as the island's.
    /// Under the direct port the queue is empty and this does nothing.
    fn hand_back(&mut self) {
        // 混合驱动下只有停放着的岛，每个先做完落地的前几步（它不在期间排给它的出站投递、
        // 留给它的存盘名单），再交回（计划 5.5.1）；别的驱动下这两样一直是空的。
        // Under the hybrid drive only parked islands, each first finishing
        // its landing (the outbound deliveries queued while it was away, the
        // save list kept for it) and then handing back (plan 5.5.1); both are
        // always empty under the other drives.
        for id in self.islands.parked_ids() {
            self.land_island(id);
        }
    }

    /// 一个岛的交回（见 [`Self::hand_back`]）。多岛时全局改动的帧借全部玩家扇出。
    ///
    /// One island's hand-back (see [`Self::hand_back`]); with several islands
    /// the frames of global changes fan out through every borrowed player.
    fn hand_back_island(&mut self, id: mc_bamboo_world::director::RegionId) {
        // 分离驱动下岛这一刻开头收信产生的回执交给 Director；齐步下回执当场就给了。
        // Under the detached drive, feed the acks from this tick's mail to the
        // Director; in lockstep they were handed over on the spot.
        let Some(island) = self.islands.get_mut(id) else {
            return;
        };
        if !island.replies.is_empty() {
            let mut replies = std::mem::take(&mut island.replies);
            self.master.host.accept_replies(
                &mut island_access(self.master.drive, &mut self.islands),
                &mut replies,
            );
            if let Some(island) = self.islands.get_mut(id) {
                island.replies = replies;
            }
        }
        let Some(island) = self.islands.get_mut(id) else {
            return;
        };
        if island.effects.is_empty() {
            return;
        }
        let effects = std::mem::take(&mut island.effects);
        self.on_master(id, Borrow::All, |global, region| {
            crate::global_port::apply_all(global, region, effects);
        });
    }

    /// 分离驱动下岛这一刻的开头：收下主控上一刻投来的信，交付 / 收回，回执留到交回时
    /// 给主控。岛这时还停放着，在收包之前收信，和第 6 步的时点相同。
    ///
    /// The start of a detached island's tick: take the mail the master posted
    /// last tick, deliver and reclaim, and keep the acks for the hand-back. The
    /// island is still parked; mail is taken before the inbound messages, as in step 6.
    fn start_island_ticks(&mut self) {
        // 停放着的岛都收；邮筒里投给还不存在的岛编号的信（只有强加载票、没有玩家的新岛）
        // 先建一个空岛再收，和齐步驱动借岛收信时一样（计划 5.5.5）。不在主控手里的岛的信
        // 留在邮筒里。
        // Every parked island takes its mail; letters posted to an island id
        // that does not exist yet (a new island with only a forced ticket and
        // no player) create an empty island first, as lockstep does when it
        // lends the islands (plan 5.5.5). Mail of islands that are away stays posted.
        let mut ids: std::collections::BTreeSet<mc_bamboo_world::director::RegionId> =
            self.islands.parked_ids().into_iter().collect();
        if self.master.islands_mode {
            #[cfg(test)]
            let create = !self.master.hybrid.breaks.no_islands_for_mail;
            #[cfg(not(test))]
            let create = true;
            if create {
                ids.extend(
                    self.master
                        .host
                        .posted_islands()
                        .into_iter()
                        .filter(|&id| !self.islands.contains(id) && self.islands.can_ensure(id)),
                );
            }
        }
        for id in ids {
            self.take_island_mail(id);
        }
    }

    /// 一个停放着的岛（不存在就先建空岛）收下邮筒里投给它的信，回执留到交回。
    ///
    /// One parked island (created empty if missing) takes the letters posted
    /// to it; the acks wait for the hand-back.
    fn take_island_mail(&mut self, id: mc_bamboo_world::director::RegionId) {
        let mut posted = std::mem::take(&mut self.master.posted_scratch);
        let order = self.master.host.mail_order();
        let origin = self.master.host.island_origin(id, ChunkPos::new(0, 0));
        let island = self.islands.ensure(id, origin);
        self.master.host.take_posted(island.id, &mut posted);
        let mut replies = std::mem::take(&mut island.replies);
        island.take_mail(&mut posted, &order, &mut replies);
        // 区块刚交付进 region：照官方 `PrepareSpawnTask.tick` 当场完成
        // 挂着没落位的进服 / 重生，位置包排在客户端确认包之前。
        // Chunks just landed: like vanilla `PrepareSpawnTask.tick`, finish
        // pending landings so the position packet precedes the confirmation.
        crate::spawn_landing::finish_pending(&mut island.region);
        island.replies = replies;
        self.master.posted_scratch = posted;
    }

    /// 按官方 `stopServer` 顺序关服：先存玩家、再存区块（后续步骤接入 IO 线程后补），
    /// 然后写 `level.dat` / `data/` / `chunk_tickets.dat`，最后释放锁、踢人。
    /// 任何一步出错都只记日志，不让关服挂死。
    ///
    /// Shut down in the official `stopServer` order: save players, then chunks
    /// (added with the IO thread in a later step), then write `level.dat`, `data/`
    /// and `chunk_tickets.dat`, release the lock, and finally kick everyone.
    /// Every step only logs on error so shutdown can never hang.
    fn shut_down(&mut self) -> ControlFlow<()> {
        // 关服是栅栏：岛停下来交回。还没交回的回执和全局改动先应用，存盘看到的是最新的。
        // Shutdown is a barrier: the island stops and hands back. Apply acks and
        // global changes not yet handed back so the save sees them.
        // 混合驱动：先把不在主控手里的岛收回来，总共最多等 `island_stop_timeout`；到点还
        // 没回来的不等了，停放着的岛照常存（计划 5.5.5）。
        // Hybrid drive: recall the islands that are away first, waiting at
        // most `island_stop_timeout` in total; those still out are left behind
        // and the parked ones are saved as usual (plan 5.5.5).
        if self.hybrid() {
            self.recall_for_shutdown();
        }
        for island in self.islands.parked_mut() {
            island.park();
        }
        self.hand_back();
        self.drain_held();
        // 还没照做的拆合先做完，关服的收回信件才能投到对的岛（「关服时有岛在拆合」）。
        // Finish pending structural changes first so the shutdown reclaims
        // reach the right islands (shutdown during a split or merge).
        self.settle_islands();
        // 官方顺序：玩家 → 区块 → level.dat / data/ → 解锁。
        // level.dat/data 的字节也在关服的阻塞栅栏之前交给 IO 线程，
        // 世界线程只编码，不逐文件同步写（F3 / R5）。
        // Official order: players → chunks → level.dat/data → unlock. The
        // level.dat/data bytes are queued before the shutdown's blocking barrier;
        // the world thread only encodes, never writes file-by-file (F3 / R5).
        // 关服前照官方 `stopServer` 把 `noSave` 复位：这一刻全量保存，
        // 不受 `/save-off` 影响（F7）。
        // Like official `stopServer`, reset `noSave` before the final full
        // save so `/save-off` cannot suppress it (F7).
        if let Some(save) = &mut self.master.save {
            save.set_auto_save(true);
        }
        self.master.host.set_saves_enabled(true);
        self.save_all_players();
        let raid_tables = self.raid_tables();
        let master = &mut self.master;
        let files = master.save.as_mut().map(|save| {
            save.encode_saves(&master.global, &raid_tables, crate::admin::now_millis())
        });
        match files {
            Some(Ok(files)) => self.master.host.submit_file_writes(files),
            Some(Err(error)) => warn!(%error, "could not encode level data on shutdown"),
            None => {}
        }
        // 收回全部已加载区块、提交全部积压写，然后排空 IO 线程并 sync_all：
        // 这是唯一允许世界线程阻塞等 IO 的地方。
        // Reclaim every loaded chunk, hand over every queued write, then drain
        // and sync_all: the only place the world thread may block on IO.
        self.master.host.shutdown(&mut self.islands);
        if let Some(save) = &mut self.master.save {
            save.close();
        }
        info!(
            codec_nanos = crate::director_host::codec_nanos(),
            io_failures = self.master.host.io_failures(),
            "chunk codec time and IO failures on this run"
        );
        let reason = Component::translate(SERVER_SHUTDOWN, []);
        for island in self.islands.parked_mut() {
            island.region.links.kick_all(&reason);
        }
        ControlFlow::Break(())
    }

    /// 关服时岛卡在 tick 线程上回不来：不再等它（岛的实施计划第 6 节第 7 步）。
    /// 全局状态（`level.dat`、`data/`）照常编码写盘，主控手里已有的区块快照和停放的盒子
    /// 交给 IO 线程写完，释放世界目录的锁，报告存不下来的东西。岛上的区块和玩家自上次
    /// 存盘以来的改动存不下来；连接层在世界线程结束后自己告诉玩家关服了。
    ///
    /// Shutdown while the island is stuck on its tick thread (island plan,
    /// step 7): stop waiting. Global state (`level.dat`, `data/`) is encoded and
    /// written as usual, the chunk snapshots and parked boxes the master holds
    /// are written by the IO thread, the world lock is released, and what could
    /// not be saved is reported. The island's chunks and players lose their
    /// changes since the last save; the connection layer tells the players once
    /// the world thread has ended.
    fn shut_down_without_island(&mut self) -> ControlFlow<()> {
        error!(
            timeout_secs = self.master.island_stop_timeout.as_secs_f64(),
            island_chunks = self.master.host.island_chunk_count(),
            "the island did not hand back after the shutdown request; stopping without it, \
             its chunks and players keep only what was saved before"
        );
        if let Some(save) = &mut self.master.save {
            save.set_auto_save(true);
        }
        self.master.host.set_saves_enabled(true);
        let raid_tables = self.raid_tables();
        let master = &mut self.master;
        let files = master.save.as_mut().map(|save| {
            save.encode_saves(&master.global, &raid_tables, crate::admin::now_millis())
        });
        match files {
            Some(Ok(files)) => self.master.host.submit_file_writes(files),
            Some(Err(error)) => warn!(%error, "could not encode level data on shutdown"),
            None => {}
        }
        self.master.host.shutdown_without_island();
        if let Some(save) = &mut self.master.save {
            save.close();
        }
        ControlFlow::Break(())
    }
}

/// 打报告的剖析器：混合驱动下是主控自己的（家岛也可能落后，主控的各段不能跟着它走）；
/// 别的驱动下跟着家岛，家岛不在主控手里时给主控那份（不进报告）。
///
/// The reporting profiler: the master's own under the hybrid drive (home
/// may lag too, and the master's parts must not depend on it); otherwise the
/// home island's, or the master's unreported one while home is away.
fn report_profiler<'a>(
    islands: &'a mut Islands,
    master: &'a mut crate::profiler::Profiler,
    hybrid: bool,
) -> &'a mut crate::profiler::Profiler {
    if !hybrid && islands.is_home() {
        &mut islands.region.profiler
    } else {
        master
    }
}

/// 划分阶段：只算不用，三个阶段照常串行。开关关着时只判一次布尔值。
///
/// 忙区块的活动增量来自第 1 步的 drain 接口，只处理状态变了的区块；
/// 两套 halo 的单元各更新一次，统计打进剖析器。
fn run_partition_stage(region: &mut Region, now: i64) {
    if !region.partitioning.enabled {
        // 第 1 步的活动清单是「本刻」的，没有消费者也要每刻清一次。
        region.clear_activity_touched();
        return;
    }
    let start = std::time::Instant::now();
    let touched_ticks = region.block_ticks.drain_touched_chunks();
    let touched_events = region.drain_block_event_activity();
    let awake = region.block_entities.awake_chunks();
    let queued = region.queued_block_event_chunks();
    let watchers = crate::region::partition::watcher_pairs(&region.block_entities);
    let inner = region.partitioning.tick(
        now,
        touched_ticks,
        touched_events,
        &awake,
        &queued,
        &watchers,
    );
    let nanos = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    let _ = inner;
    let verify_nanos = region.partitioning.verify_nanos();
    let topology = region.partitioning.topology();
    let busy = region.partitioning.busy_len();
    let units = region.partitioning.units();
    if let Some(stats) = region.profiler.partition_stats_mut() {
        stats.record_partition(nanos, verify_nanos);
        stats.record_units(busy, &units, topology.merges, topology.splits);
    }
    // 范围不重叠是设计稿 4.1 的不变量；生产路径只记数，不因为客户端
    // 给的极端坐标 panic（随机测试每刻断言同一条）。
    let disjoint = region
        .partitioning
        .partitions
        .iter()
        .all(crate::region::partition::Partition::areas_disjoint);
    if let Some(stats) = region.profiler.partition_stats_mut() {
        stats.record_disjoint_check(disjoint);
    }
}

/// 岛自己的一刻，从划分阶段到发区块：方块、实体、玩家杂项、视图、方块广播、实体同步。
/// 只碰岛。`master` 是齐步时借来的主控全局状态：准则照它的口子写，延迟广播在原来的
/// 位置发，和以前逐位相同；分离时是 `None`，准则排进岛的效果队列、交回时由主控应用，
/// 延迟广播由主控在交回之后发。
///
/// The island's own tick from the partition stage to chunk sending: blocks,
/// entities, player bits, views, block broadcasts and entity sync. It touches
/// only the island. `master` is the master's global state lent in lockstep:
/// criteria go through its port and the latency update is sent in place, bit
/// for bit as before. Detached passes `None`: criteria are queued on the
/// island for the master to apply at hand-back, and the master sends the
/// latency update after the hand-back.
fn run_island_simulation(
    island: &mut Island,
    master: Option<(&mut Global, PortMode)>,
    entity_pool: Option<&mc_bamboo_blocks::entity::parallel::WorkerPool>,
) {
    let (mut global, port_mode) = match master {
        Some((global, mode)) => (Some(global), mode),
        None => (None, PortMode::Queued),
    };
    // 官方 `ServerLevel.tick` 开头（天气之后、时间推进之前）决定跳早，
    // `wakeUpAllPlayers` 在这一刻的玩家 tick 之前叫醒所有人。主控已经把
    // 标志发布到全局快照里，岛在自己一刻开头照它做。
    //
    // Official `ServerLevel.tick` decides the skip at its start (after
    // weather, before time) and wakes everyone before player ticks. The
    // master published the flag; the island applies it at its tick start.
    crate::sleep::wake_from_skip(&mut island.region);
    // 划分阶段串行、只算不用；三个阶段的执行顺序和行为不变。读岛的时钟。
    let now = island.region.tick.game_time();
    run_partition_stage(&mut island.region, now);
    island.region.profiler.lap(Phase::Partition);
    // 官方 `ServerLevel.tick` 的顺序：方块和流体的计划刻、`chunkSource.tick` 里的随机刻、
    // 方块事件、实体、方块实体。计划刻和随机刻里生成的下落方块和 TNT 同一 tick 就动。
    //
    // Vanilla `ServerLevel.tick` order: block and fluid ticks, random ticks inside
    // `chunkSource.tick`, block events, entities, then block entities. Falling blocks and
    // primed TNT spawned by block or random ticks move in the same game tick.
    let entity_threads = island.entity_threads;
    let region = &mut island.region;
    region
        .profiler
        .begin_chunk_phase(ChunkPhase::ScheduledTicks);
    region.profiler.begin_partition_phase();
    interaction::run_block_ticks(region);
    region.profiler.end_partition_phase();
    region.profiler.end_chunk_phase();
    region.profiler.lap(Phase::ScheduledTicks);
    // 官方 `chunkSource.tick` 先刷怪再跑随机刻，位置照它。
    // Vanilla `chunkSource.tick` spawns before the random ticks; keep it that way.
    crate::natural_spawn::tick(region);
    interaction::run_random_ticks(region);
    // 官方 `tickChunks` 在自然刷怪和逐区块随机刻之后调 `tickCustomSpawners`；主世界的
    // 自定义刷怪器按 `MinecraftServer.createLevels` 里的顺序是 `PhantomSpawner`、
    // `PatrolSpawner`，再往后的猫、村庄围攻、流浪商人；Bamboo 只建模前后三个，
    // 刷新器仍按这个相对顺序跑，且都在随机刻之后、方块事件和实体之前。
    // Vanilla `tickChunks` calls `tickCustomSpawners` after natural spawning and the
    // per-chunk random ticks. The overworld's custom spawners run in
    // `MinecraftServer.createLevels` order: `PhantomSpawner`, `PatrolSpawner`,
    // then the cat, village-siege and wandering-trader spawners. Bamboo models
    // the first two and the last; they keep that relative order, all after the
    // random ticks and before block events and entities.
    crate::phantom_spawn::tick(region);
    crate::patrol_spawn::tick(region);
    {
        let mut port = match (port_mode, global.as_deref_mut()) {
            (PortMode::Direct, Some(global)) => GlobalPort::Direct(global),
            _ => GlobalPort::Queued(&mut island.effects),
        };
        crate::wandering_trader_spawn::tick(region, &mut port);
    }
    // 官方 `ServerLevel.tick` 的 "raid" 阶段：自定义刷怪器之后跑袭击状态机。
    // Vanilla's "raid" phase of `ServerLevel.tick`: after the custom spawners.
    crate::raid::tick(region);
    region.profiler.lap(Phase::RandomTicks);
    region.profiler.begin_chunk_phase(ChunkPhase::BlockEvents);
    region.profiler.begin_partition_phase();
    interaction::run_block_events(region);
    region.profiler.end_partition_phase();
    region.profiler.end_chunk_phase();
    region.profiler.lap(Phase::BlockEvents);
    // `ThrownEnderpearl.tick` 开头按 `ender_pearls_vanish_on_death` 丢弃死掉主人的珍珠。
    // 放在实体阶段之前，和官方「珍珠自己的 tick 先检查」同一位置。
    //
    // Drop a dead owner's pearls before the entity phase, like the start of the
    // pearl's own `ThrownEnderpearl.tick`.
    let vanish = region.entity_game_rules().ender_pearls_vanish_on_death;
    crate::ender_pearl::remove_dead_owners_pearls(region, vanish);
    interaction::run_entity_ticks(entity_threads, region, entity_pool);
    // 乘客的 `rideTick`：载具这一刻的位置定了，骑手的位置跟上来，要下车的在这里下车。
    // 官方把乘客紧跟在载具后面 tick，这里放在实体阶段之后、玩家自己的 tick 之前。
    //
    // The passenger's `rideTick`: the vehicle position is settled for this tick, so
    // riders follow it and dismounts land here. Vanilla ticks passengers right
    // after the vehicle; this runs after the entity phase and before the player's
    // own tick.
    crate::riding::tick(region);
    if island.entity_threads == 0 {
        region.profiler.lap(Phase::Entities);
    }
    region.profiler.begin_chunk_phase(ChunkPhase::BlockEntities);
    region.profiler.begin_partition_phase();
    interaction::run_block_entity_ticks(region);
    region.profiler.end_partition_phase();
    region.profiler.end_chunk_phase();
    region.profiler.lap(Phase::BlockEntities);
    // 玩家属性表：每刻按装备 / 效果 / 游戏模式重算修饰符。挖掘、攻击、效果、同步
    // 都在这一刻之后读它，和官方实体 tick 里先 `detectEquipmentUpdates` 一致。
    //
    // Rebuild the player attribute modifiers from equipment / effects / game mode
    // before mining, attack, effects and sync read them, like vanilla's entity
    // tick running `detectEquipmentUpdates` first.
    crate::player_attributes::rebuild(region);
    // `LivingEntity.baseTick` 的 `tickEffects`、`collectEquipmentChanges` 和
    // `onChangedBlock`：TICK 组件、换靴子停机/立即重跑、跨格重跑 location 效果。
    // 官方 `commonTick` 在 `ServerPlayer.tick` 之前推 `tickCount`，这里比
    // `player_hurt::tick` 的自增早一刻，模块内自行加一。
    crate::player_enchantments::tick(region);
    // 玩家自己每 tick 的挖掘进度。官方在实体 tick 里跑 `ServerPlayerGameMode.tick`，
    // 这里跟着玩家 tick 放在随机刻之后。
    interaction::tick_mining(region);
    // 方块实体的 tick：告示牌在这里清掉走远的编辑者。
    sign::tick_editors(region);
    // 容器开合人数的重查和菜单同步。
    container::tick(region);
    menu::tick(region);
    lectern::tick(region);
    chat::tick_spam_throttlers(region);
    crate::drops::tick_drop_throttle(region);
    // `Player.aiStep`：攻击冷却计数每刻加一，主手换物品时清零。
    // 放在玩家 tick 开头，和官方实体 tick 在 `ServerPlayer.doTick` 之前一致。
    //
    // `Player.aiStep`: increment the attack-cooldown ticker each tick and reset
    // it on a main-hand item swap. Runs at the start of the player tick, like
    // vanilla's entity tick before `ServerPlayer.doTick`.
    crate::player_attack::tick(region);
    // `LivingEntity.tick` 的 `push` 段：自旋攻击计时和撞到实体时的 `Player.attack`。
    //
    // The `push` section of `LivingEntity.tick`: the spin-attack timer and the
    // `Player.attack` on contact.
    crate::player_auto_spin::tick(region);
    // `Player.tick` 的 `cooldowns.tick()`、清掉已经离表的珍珠和每刻的存档快照。
    crate::ender_pearl::tick(region);
    crate::player_fishing::tick(region);
    // 官方 `ServerPlayer.doTick` 里 `set_health` 在 `set_experience` 之前。
    // 伤害冷却递减、按需补 `set_health`。
    //
    // In `ServerPlayer.doTick` `set_health` comes before `set_experience`:
    // tick the hurt cooldowns and send health if the snapshot moved.
    crate::player_hurt::tick(region);
    // `LivingEntity.baseTick` 的 `tickEffects`：在受伤冷却之后跑持续效果，
    // 治疗和伤害走同一个 `hurt_player` 入口。
    crate::player_effects::tick(region);
    // `Entity.baseTick` / `LivingEntity.baseTick` 的环境伤害：着火、溺水、
    // 卡墙、虚空、仙人掌 / 甜浆果 / 岩浆块、细雪冻伤。
    crate::player_environment::tick_environment(region);
    // `LivingEntity.updateUsingItem`：吃东西的剩余刻数和完成结算。
    crate::player_consume::tick(region);
    // `LivingEntity.updateUsingItem` 的弓 / 弩分支。
    //
    // The bow and crossbow branch of `LivingEntity.updateUsingItem`.
    crate::player_weapon::tick(region);
    crate::player_glide::tick(region);
    // 移动 / 跳跃 / 攻击 / 挖掘累计的饥饿消耗在 tick 末尾落到 FoodData。
    crate::player_exhaustion::flush(region);
    // 官方 `ServerPlayer.doTick`：总经验变了就给这名玩家补一包 `set_experience`。
    //
    // Official `ServerPlayer.doTick`: a changed total sends one `set_experience`.
    crate::experience::sync_dirty(region);
    // 官方 `ServerPlayer.doTick`：进度触发器、首包/增量包都在玩家 tick 末尾 flush。
    crate::advancements::tick(region);
    crate::command::admin::tick_idle(region);
    // 官方 `ServerPlayer.doTick` 的六个只读准则：岛算变了的值，主控写计分板。
    //
    // The six read-only criteria from official `ServerPlayer.doTick`: the
    // island computes, the master writes the scoreboard.
    {
        let mut port = match (port_mode, global.as_deref_mut()) {
            (PortMode::Direct, Some(global)) => GlobalPort::Direct(global),
            _ => GlobalPort::Queued(&mut island.effects),
        };
        crate::scoreboard::tick_criteria(&mut port, &mut island.region);
    }
    // `ServerPlayer.doTick` 每刻推进 `sleepCounter`；主控在交回后读这份计数。
    crate::sleep::tick_sleep_counters(&mut island.region);
    let region = &mut island.region;
    region.profiler.lap(Phase::PlayerMisc);

    // `ServerPlayer.handleShoulderEntities`：离肩条件在派生状态之前跑，元数据能看到清空。
    crate::player_shoulder::tick(region);
    entity_sync::update_derived_state(region);
    chunk_tracking::update_views(region);
    region.profiler.lap(Phase::Views);
    chunk_broadcast::broadcast_changes(region);
    region.profiler.lap(Phase::BlockChangeBroadcast);
    entity_sync::sync_entities(region);
    // 玩家属性：全量（配对后 / 首刻）或逐刻脏属性，发给自己和全部观察者。
    // 放在 sync_entities 之后，配对新增的观察者已经进了 seen_by。
    //
    // Player attributes: full (after pairing / first tick) or per-tick dirty,
    // sent to the player and all observers. After sync_entities so newly
    // paired observers are already in seen_by.
    crate::player_attributes::sync_dirty(region);
    entity_tracking::sync(region);
    entity_sync::send_equipment_changes(region);
    interaction::send_block_changed_acks(region);
    if let Some(global) = global
        && let Some(frames) = global.tick_latency_broadcast()
    {
        region.links.broadcast(&frames);
    }
    region.profiler.lap(Phase::EntitySyncAndTracking);
    chunk_tracking::send_chunks(region);
    movement::finish_tick(region);
    region.profiler.lap(Phase::ChunkSending);
}

/// 分离驱动下岛在 tick 线程上跑的一刻：光照、岛的时钟，[`run_island_simulation`]，
/// 最后照上次交回时的名单编存盘快照、取走新的改动，交回时给主控。只碰岛自己。
///
/// A detached island's tick on its tick thread: light, the island clock,
/// [`run_island_simulation`], and finally the save snapshots from the list
/// given at the last hand-back plus the new changes, for the master at
/// hand-back. It touches only the island.
fn run_detached_island(island: &mut Island) {
    #[cfg(test)]
    island.run_test_hook();
    // 派出去之前的那段（主控收包、交接）不算进任何阶段，交接本身另记 `island_handoff`。
    // The time before dispatch is no phase; the hand-over goes to `island_handoff`.
    island.region.profiler.restart_lap();
    crate::generation_post_process::prepare_ticking_chunks(&mut island.region);
    light_updates::run_light_updates(&mut island.region);
    island.region.profiler.lap(Phase::LightUpdates);
    island.region.advance_tick();
    run_island_simulation(island, None, None);
    island.save_report = island.save_pass();
}

impl Master {
    /// 主控每刻推进 Director 一次：超时票递减、到点的任务、匀速存盘的队列。分离驱动下
    /// 这一步和岛的一刻并行（`island` 是 `Away`），信件只投进邮筒。
    ///
    /// The master's Director step, once per tick: ticket timeouts, due tasks,
    /// the paced save queue. Under the detached drive it runs beside the
    /// island's tick (`island` is `Away`) and mail is only posted.
    fn director_step(&mut self, island: &mut IslandAccess<'_>) {
        self.director_tick += 1;
        // 刷新 `noSave` 反面：`/save-off` 当刻就停掉卸载写盘（F7）。
        // Refresh the inverse of `noSave`: `/save-off` stops unload writes
        // from this tick on (F7).
        // 没接 SaveSession（测试 / oracle）时保持匀速写盘开启；接了就照 `noSave`。
        let saves_enabled = self.save.as_ref().is_none_or(|save| save.auto_save());
        self.host.set_saves_enabled(saves_enabled);
        // 生成期刷怪读游戏规则 `SPAWN_MOBS`。
        // World-generation spawning reads the `SPAWN_MOBS` rule.
        let spawn_mobs = self
            .global
            .rules()
            .bool(mc_bamboo_data::gamerules::SPAWN_MOBS);
        self.host.set_spawn_mobs(spawn_mobs);
        self.host.tick(island, self.director_tick);
    }

    /// 等岛交回期间看一眼主控队列：有关服请求、或者发端全没了。取出来的消息留着，
    /// 下一刻照常处理。
    ///
    /// While waiting for the island, look at the master queue for a shutdown
    /// request or closed senders. Messages taken out are kept for next tick.
    fn shutdown_requested(&mut self) -> bool {
        let closed = self.inbox.drain_into(&mut self.held_inbound);
        closed
            || self
                .held_inbound
                .iter()
                .any(|message| matches!(message.message, Inbound::Shutdown))
    }
}

/// 主控这一次调用怎么拿岛：齐步借出去、信件当场收取；分离只投信，岛下一刻开头收取。
///
/// How the master reaches the island for one call: lockstep lends it and the
/// mail is taken on the spot; detached only posts, for the island's next tick.
fn island_access(drive: IslandDrive, islands: &mut Islands) -> IslandAccess<'_> {
    match drive {
        IslandDrive::Lockstep => {
            debug_assert!(islands.all_parked());
            IslandAccess::All(islands)
        }
        IslandDrive::Detached => IslandAccess::Away,
    }
}

/// 编出一名在线玩家的存档字节（纯函数，不碰 `self`，避免和 `host` 的可变借用冲突）。
/// 返回带连字符的 uuid 和 gzip 字节，交给 [`World::save_all_players`] 提交给 IO 线程。
/// Encode one live player's save (pure, so it does not conflict with the mutable
/// borrow of `host`). Returns the hyphenated uuid and gzip bytes for submission.
fn encode_player_save(
    player: &crate::player::Player,
    root_vehicle: Option<mc_bamboo_nbt::Tag>,
) -> Option<(String, Vec<u8>)> {
    let mut data = runtime_player_data(player);
    data.root_vehicle = root_vehicle;
    match encode_player_data(&data) {
        Ok(bytes) => Some((player_uuid_string(player.uuid.as_bytes()), bytes)),
        Err(error) => {
            warn!(%error, "could not encode player data");
            None
        }
    }
}

/// 按绝对时间排下一 tick，sleep 的误差和 tick 本身的耗时都不会累积。
/// 第一刻立刻跑，`next_tick` 是第二刻的起点，之后每刻按 50ms 网格推进。
struct Schedule {
    next_tick: Instant,
    /// 本刻按计划应该几点开始；追赶时它落后于实际开始时刻。
    expected_start: Instant,
    /// 上一次 `after_tick` 有没有因为落后太多而放弃追赶。
    skipped_catch_up: bool,
    /// 上一次 `after_tick` 要不要连续跑（没睡）。
    catching_up: bool,
}

impl Schedule {
    /// 第一 tick 在 `now` 立刻开始，第二刻不早于 `now + TICK`。
    fn starting_at(now: Instant) -> Self {
        Self {
            next_tick: now + TICK,
            expected_start: now,
            skipped_catch_up: false,
            catching_up: false,
        }
    }

    /// 这一刻应该几点开始。
    fn expected_start(&self) -> Instant {
        self.expected_start
    }

    /// `after_tick` 算出的下一刻截止时刻。
    fn deadline(&self) -> Instant {
        self.expected_start
    }

    fn skipped_catch_up(&self) -> bool {
        self.skipped_catch_up
    }

    fn catching_up(&self) -> bool {
        self.catching_up
    }

    /// 一 tick 结束时调用，返回到下一 tick 开始还要等多久。已经落后时返回 0，连续跑 tick 追上。
    fn after_tick(&mut self, now: Instant) -> Duration {
        self.advance(now, MAX_CATCH_UP)
    }

    /// 同 [`Self::after_tick`]，但不连续补刻（混合驱动的主控，计划 5.5.1）：落后超过一刻
    /// 就从现在重新计时。
    ///
    /// Like [`Self::after_tick`] without making ticks up (the hybrid drive's
    /// master, plan 5.5.1): more than one tick behind restarts from now.
    fn after_tick_without_catch_up(&mut self, now: Instant) -> Duration {
        self.advance(now, TICK)
    }

    fn advance(&mut self, now: Instant, limit: Duration) -> Duration {
        let behind = now.saturating_duration_since(self.next_tick);
        self.skipped_catch_up = behind > limit;
        if self.skipped_catch_up {
            warn!(
                "Can't keep up! Running {}ms behind, skipping {} ticks",
                behind.as_millis(),
                behind.as_millis() / TICK.as_millis()
            );
            self.next_tick = now;
        }
        let wait = self.next_tick.saturating_duration_since(now);
        self.catching_up = wait.is_zero();
        self.expected_start = self.next_tick;
        self.next_tick += TICK;
        wait
    }
}

#[cfg(test)]
#[path = "world/tests/compute_pool_tests.rs"]
mod compute_pool_tests;
#[cfg(test)]
#[path = "world/entity_player_tests.rs"]
mod entity_player_tests;
#[cfg(test)]
#[path = "world/island_alone_tests.rs"]
mod island_alone_tests;
#[cfg(test)]
#[path = "world/island_borrow_tests.rs"]
mod island_borrow_tests;
#[cfg(test)]
#[path = "world/island_command_tests.rs"]
mod island_command_tests;
#[cfg(test)]
#[path = "world/island_crossing_tests.rs"]
mod island_crossing_tests;
#[path = "world/island_death.rs"]
mod island_death;
#[cfg(test)]
#[path = "world/island_death_tests.rs"]
mod island_death_tests;
#[cfg(test)]
#[path = "world/tests/island_drive_tests.rs"]
mod island_drive_tests;
#[path = "world/island_hybrid.rs"]
mod island_hybrid;
#[cfg(test)]
#[path = "world/island_hybrid_tests.rs"]
mod island_hybrid_tests;
#[cfg(test)]
#[path = "world/island_partition_tests.rs"]
mod island_partition_tests;
#[cfg(test)]
#[path = "world/tests/island_pool_tests.rs"]
mod island_pool_tests;
#[path = "world/island_runtime.rs"]
mod island_runtime;
#[cfg(test)]
#[path = "world/island_runtime_tests.rs"]
mod island_runtime_tests;
#[cfg(test)]
#[path = "world/island_scale_tests.rs"]
mod island_scale_tests;
#[cfg(test)]
#[path = "world/island_split_bench.rs"]
mod island_split_bench;
#[cfg(test)]
#[path = "world/island_split_tests.rs"]
mod island_split_tests;
#[cfg(test)]
mod loaded_tests;
#[cfg(test)]
#[path = "world/player_death_tests.rs"]
mod player_death_tests;
#[cfg(test)]
#[path = "world/tests/review_probe.rs"]
mod review_probe;
#[cfg(test)]
#[path = "world/staged_tests.rs"]
mod staged_tests;
#[cfg(test)]
mod storage_diff_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod ticket_tests;
