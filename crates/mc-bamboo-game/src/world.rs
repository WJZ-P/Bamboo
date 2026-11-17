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
    DemandSnapshot, DirectorConfig, PipelineKind, PlayerDemand, TicketOp,
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
    inventory, lectern, light_updates, menu, movement,
    pacing::{Pacer, WallTiming},
    player::shows_hat,
    player_save::{game_mode_from_id, restore_inventory, runtime_player_data},
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
/// 区块计算池线程数的上限。
pub const MAX_CHUNK_WORKER_THREADS: usize = 64;

const SERVER_SHUTDOWN: &str = "multiplayer.disconnect.server_shutdown";

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
    /// 红石缓存（实验）：结果和官方逐位一致，对大型红石电路有效。默认关。
    pub redstone_cache: bool,
    /// 区块计算池的线程数。0 = 内联执行器（解析、生成、序列化在世界线程上做）；
    /// ≥ 1 = 常驻后台线程。区块内容和线程数无关，只有交付的时刻可能不同。
    pub chunk_worker_threads: usize,
    /// Director 信箱每刻交付/收回的区块数上限，0 表示不限。
    pub mail_budget: MailBudget,
    /// 世界目录。`None` 表示不存档（测试、oracle、压测都走这条路）。
    ///
    /// The world directory. `None` disables saving (tests, oracle, load tests).
    pub world_dir: Option<PathBuf>,
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
            redstone_cache: false,
            chunk_worker_threads: 0,
            mail_budget: MailBudget::default(),
            world_dir: None,
            world_gen: crate::world_options::WorldGenConfig::default(),
            broadcast_console_to_ops: true,
            bench: None,
        }
    }
}

/// 所有游戏状态的唯一所有者：主控一份，岛一份（现在只有一个岛）。
///
/// 第 1 步只把字段按以后的归属分开，调用顺序和行为都不变
/// （`docs/island-implementation-plan.md` 第 2 节、第 6 节第 1 步）。
///
/// The sole owner of all game state: one master and one island (only one
/// island for now). Step 1 only regroups fields by their future owner; call
/// order and behaviour are unchanged.
pub struct World {
    pub(crate) master: Master,
    pub(crate) island: Island,
}

/// 主控：全局状态、入站通道、节拍、Director 宿主、票中心、存盘会话和压测钩子。
/// 以后跑在主控线程上，固定 20 TPS。
///
/// The master: global state, inbound channel, pacing, the Director host,
/// ticket centres, the save session, and load-test hooks. It will later run on
/// the master thread at a fixed 20 TPS.
pub(crate) struct Master {
    pub(crate) global: Global,
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
}

/// 岛：一个 `Region` 和只属于它的执行配置。以后可以整个交给 tick 线程。
///
/// An island: one `Region` plus the execution settings that belong to it.
/// It will later be handed whole to a tick thread.
pub(crate) struct Island {
    pub(crate) region: Region,
    /// 实体阶段的线程数。0 = 精确驱动；≥ 1 = 并行驱动。
    pub(crate) entity_threads: usize,
    /// 排队实现下这一刻攒下、等交回时由主控应用的全局改动。直接实现下一直是空的。
    ///
    /// Global changes collected this tick under the queued port, applied by the
    /// master at hand-back. Always empty under the direct port.
    pub(crate) effects: Vec<GlobalEffect>,
}

impl World {
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
        let staged = !infinite_flat && !world_gen.level_type.is_flat();
        if let Some(save) = save.as_mut() {
            save.set_world_gen(world_gen.clone());
        }
        // 种子进 `WorldState`（`hashed_seed` 进 login 包、天气），随机序列在岛上。
        // `level.dat` 里没有建模的 seed 字段，`apply_level_data` 不动它。
        // Put the seed into `WorldState` (hashed_seed for the login packet,
        // weather); the random sequences live on the island. `level.dat` has no
        // modelled seed field, so `apply_level_data` leaves it alone.
        global.state_mut().seed = world_gen.seed;
        region.random_sequences = mc_bamboo_loot::RandomSequences::new(world_gen.seed);
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
                inbox,
                refused_inboxes: Vec::new(),
                pacer: Pacer,
                authoritative_admin,
                host: if infinite_flat {
                    DirectorHost::new_infinite_flat(Default::default())
                } else {
                    DirectorHost::new(
                        director_config,
                        MailBudget {
                            deliveries: config.mail_budget.deliveries.min(MAX_MAIL_BUDGET),
                            reclaims: config.mail_budget.reclaims.min(MAX_MAIL_BUDGET),
                        },
                    )
                },
                ticket_centers: std::collections::BTreeMap::new(),
                player_directions: std::collections::BTreeMap::new(),
                player_demand: None,
                director_tick: 0,
                save,
                startup_error,
                pending_save_success: Vec::new(),
                bench_hooks: config.bench,
                bench_ready: false,
                port_mode: PortMode::default_for_world(),
            },
            island: Island {
                region,
                entity_threads: config.entity_threads.min(MAX_ENTITY_THREADS),
                effects: Vec::new(),
            },
        };
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
            world.island.region.use_infinite_flat_chunks();
        }
        if config.redstone_cache {
            world.island.region.redstone_cache =
                mc_bamboo_blocks::cache::RedstoneCache::new(mc_bamboo_blocks::cache::CacheConfig {
                    enabled: true,
                    ..Default::default()
                });
        }
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
        world.island.region.start_clock(time);
        world.master.global.publish(&mut world.island.region);
        world.island.region.profiler.on_world_start();
        if world.island.region.profiler.partition_enabled() {
            let verify = world.island.region.profiler.partition_verify();
            world.island.region.partitioning.enable(
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
        self.master.host.tickets(&mut self.island.region, ops);
    }

    /// 每刻在玩家跟踪阶段之前调用：只在玩家跨区块时发 `PlayerMoved`。
    fn sync_player_tickets(&mut self) {
        let mut moves = Vec::new();
        let mut changed = false;
        for (&id, player) in &self.island.region.players {
            let at = player.chunk_pos();
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
        let mut left = Vec::new();
        for &id in self.master.ticket_centers.keys() {
            if !self.island.region.players.contains_key(&id) {
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
        self.master.director_tick += 1;
        // 刷新 `noSave` 反面：`/save-off` 当刻就停掉卸载写盘（F7）。
        // Refresh the inverse of `noSave`: `/save-off` stops unload writes
        // from this tick on (F7).
        // 没接 SaveSession（测试 / oracle）时保持匀速写盘开启；接了就照 `noSave`。
        let saves_enabled = self
            .master
            .save
            .as_ref()
            .is_none_or(|save| save.auto_save());
        self.master.host.set_saves_enabled(saves_enabled);
        self.master
            .host
            .tick(&mut self.island.region, self.master.director_tick);
    }

    /// 阻塞运行，直到收到 [`Inbound::Shutdown`] 或者所有发送端都没了。
    pub fn run(mut self) {
        if let Some(message) = &self.master.startup_error {
            error!(%message, "world startup failed; not running");
            return;
        }
        let mut timing = WallTiming;
        let mut schedule = Schedule::starting_at(self.island.region.profiler.now());
        loop {
            let expected = schedule.expected_start();
            let actual = self.island.region.profiler.now();
            let skipped = schedule.skipped_catch_up();
            let catch_up = schedule.catching_up();
            self.island
                .region
                .profiler
                .begin_scheduled_tick(expected, actual, skipped, catch_up);
            if self.tick().is_break() {
                break;
            }
            schedule.after_tick(self.island.region.profiler.now());
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
        self.master.host.counters_this_tick().0
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
        self.master.global.publish(&mut self.island.region);
        self.apply_ticket_ops(
            positions
                .iter()
                .map(|&pos| TicketOp::Forced { pos, forced: true }),
        );
    }

    /// 测量脚手架用：某个区块是否已经交付进 region。
    /// Benchmark-only: whether a chunk has been delivered into the region.
    #[doc(hidden)]
    pub fn is_chunk_loaded(&self, pos: mc_bamboo_core::ChunkPos) -> bool {
        self.island.region.chunks.is_loaded(pos)
    }

    /// 对拍脚手架用：一个已交付区块的全部方块状态，顺序是 y 从下到上、x、z
    /// 各 0..16（和官方对拍输出的顺序一致）。没交付返回 `None`。
    /// Oracle scaffolding: every block state of a delivered chunk in y, x, z
    /// order (the official oracle's order); `None` if not delivered.
    #[doc(hidden)]
    pub fn chunk_blocks_unpaced(&self, pos: mc_bamboo_core::ChunkPos) -> Option<Vec<u32>> {
        use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
        if !self.island.region.chunks.is_loaded(pos) {
            return None;
        }
        let mut out = Vec::with_capacity(WORLD_HEIGHT as usize * 256);
        for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
            for x in 0..16 {
                for z in 0..16 {
                    let at = mc_bamboo_core::BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z);
                    out.push(self.island.region.chunks.block(at)?);
                }
            }
        }
        Some(out)
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
        self.island.region.profiler.begin_tick();
        self.record_tick_counts();
        let flow = self.tick_phases();
        // 三阶段这一刻的耗时由 `lap` 顺手记下，划分统计拿它当尾段占比的分母。
        // 开关关着时这里只判一次布尔值。
        if self.island.region.profiler.partition_enabled() {
            let three = self.island.region.profiler.three_phase_nanos_this_tick();
            let total = self.island.region.profiler.all_phase_nanos_this_tick();
            if let Some(stats) = self.island.region.profiler.partition_stats_mut() {
                stats.finish_tick(three, total);
            }
        }
        self.island.region.profiler.end_tick();
        flow
    }

    /// 每刻开始时的数量指标，趁状态还没变先记下来。到期的计划刻数是
    /// 队列里 `trigger_tick <= now + 1` 的那些（岛的时钟在计划刻之前加一）。
    /// 队列按到期时刻排好了，所以只看开头到期的那一段，不扫整条队列。
    fn record_tick_counts(&mut self) {
        let next_time = self.island.region.tick.game_time() + 1;
        let due = self
            .island
            .region
            .block_ticks
            .pending()
            .take_while(move |tick| tick.trigger_tick <= next_time)
            .count()
            + self
                .island
                .region
                .block_ticks
                .pending_fluids()
                .take_while(move |tick| tick.trigger_tick <= next_time)
                .count();
        let profiler = &mut self.island.region.profiler;
        profiler.record(
            Metric::OnlinePlayers,
            self.island.region.players.len() as u64,
        );
        profiler.record(Metric::Entities, self.island.region.entities.len() as u64);
        profiler.record(Metric::DueScheduledTicks, due as u64);
        profiler.record(
            Metric::LoadedChunks,
            self.island.region.chunks.loaded_count().unwrap_or(0) as u64,
        );
    }

    /// 划分阶段：只算不用，三个阶段照常串行。开关关着时只判一次布尔值。
    ///
    /// 忙区块的活动增量来自第 1 步的 drain 接口，只处理状态变了的区块；
    /// 两套 halo 的单元各更新一次，统计打进剖析器。
    fn run_partition_stage(&mut self, now: i64) {
        if !self.island.region.partitioning.enabled {
            // 第 1 步的活动清单是「本刻」的，没有消费者也要每刻清一次。
            self.island.region.clear_activity_touched();
            return;
        }
        let start = std::time::Instant::now();
        let touched_ticks = self.island.region.block_ticks.drain_touched_chunks();
        let touched_events = self.island.region.drain_block_event_activity();
        let awake = self.island.region.block_entities.awake_chunks();
        let queued = self.island.region.queued_block_event_chunks();
        let watchers = crate::region::partition::watcher_pairs(&self.island.region.block_entities);
        let inner = self.island.region.partitioning.tick(
            now,
            touched_ticks,
            touched_events,
            &awake,
            &queued,
            &watchers,
        );
        let nanos = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let _ = inner;
        let verify_nanos = self.island.region.partitioning.verify_nanos();
        let topology = self.island.region.partitioning.topology();
        let busy = self.island.region.partitioning.busy_len();
        let units = self.island.region.partitioning.units();
        if let Some(stats) = self.island.region.profiler.partition_stats_mut() {
            stats.record_partition(nanos, verify_nanos);
            stats.record_units(busy, &units, topology.merges, topology.splits);
        }
        // 范围不重叠是设计稿 4.1 的不变量；生产路径只记数，不因为客户端
        // 给的极端坐标 panic（随机测试每刻断言同一条）。
        let disjoint = self
            .island
            .region
            .partitioning
            .partitions
            .iter()
            .all(crate::region::partition::Partition::areas_disjoint);
        if let Some(stats) = self.island.region.profiler.partition_stats_mut() {
            stats.record_disjoint_check(disjoint);
        }
    }

    /// 每刻一次的压测回调。`None` 时只做一次 `Option` 判断。
    fn run_bench_hooks(&mut self) {
        let Some(hooks) = self.master.bench_hooks else {
            return;
        };
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
        self.master.global.publish(&mut self.island.region);
        if finished {
            self.master.bench_ready = true;
            tracing::info!("bench scenario ready");
        }
    }

    /// 拆成 `tick` 和这一段只是为了保证提前返回时也能收尾；顺序仍然是原来的顺序。
    fn tick_phases(&mut self) -> ControlFlow<()> {
        self.master.host.begin_tick();
        // 上一刻发出去的读盘请求，结果在这一刻开头经信箱收回。
        // Read requests dispatched last tick are collected at the start of this tick.
        self.master.host.poll_io(&mut self.island.region);
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
        self.master.global.publish(&mut self.island.region);
        self.process_inbox()?;
        // 压测输入跑在收包阶段，等价于一名管理员在世界线程上翻拉杆 / 补物品；
        // 它的耗时记进 `inbox`，不进后面三个方块侧阶段。默认配置下这里是空操作。
        self.run_bench_hooks();
        self.consume_save_commands();
        self.sync_player_tickets();
        self.island.region.profiler.lap(Phase::Inbox);
        self.tick_ticket_timeouts();
        let (delivered, reclaimed) = self.master.host.counters_this_tick();
        let profiler = &mut self.island.region.profiler;
        profiler.record(Metric::ChunksDelivered, delivered as u64);
        profiler.record(Metric::ChunksReclaimed, reclaimed as u64);
        profiler.record(Metric::ParkedChunks, self.master.host.parked_count() as u64);
        profiler.record(
            Metric::ComputeInFlight,
            self.master.host.compute_in_pool() as u64,
        );
        profiler.record(
            Metric::ComputeBacklog,
            self.master.host.compute_backlog() as u64,
        );
        profiler.lap(Phase::Delivery);
        light_updates::run_light_updates(&mut self.island.region);
        self.island.region.profiler.lap(Phase::LightUpdates);

        // 官方 `ServerLevel.tick` 开头的 "world border" 阶段在天气和时间之前。
        // 渐变推进后把新快照发布给 region，这一刻的方块逻辑和实体碰撞都读它。
        //
        // Vanilla `ServerLevel.tick`'s "world border" phase comes before weather and time.
        // After the lerp step, publish the new snapshot; this tick's block logic and
        // entity collisions read it.
        self.master.global.tick_world_border();
        self.master.global.publish(&mut self.island.region);
        // 官方 `ServerLevel.tick` 的天气阶段在 `tickTime` 之前；广播帧排在时间包之前发出。
        let weather = weather::advance(&mut self.master.global, &self.island.region);
        if !weather.is_empty() {
            self.island.region.links.broadcast(&weather);
        }
        self.master.global.advance_time();
        // 天气和时间推进完再发布一次，这一刻的方块逻辑读它。
        // Publish again after weather and time; this tick's block logic reads it.
        self.master.global.publish(&mut self.island.region);
        self.island.region.advance_tick();
        self.island.region.profiler.lap(Phase::WeatherAndTime);
        // 划分阶段串行、只算不用；三个阶段的执行顺序和行为不变。读岛的时钟。
        self.run_partition_stage(self.island.region.tick.game_time());
        self.island.region.profiler.lap(Phase::Partition);
        // 官方 `ServerLevel.tick` 的顺序：方块和流体的计划刻、`chunkSource.tick` 里的随机刻、
        // 方块事件、实体、方块实体。计划刻和随机刻里生成的下落方块和 TNT 同一 tick 就动。
        //
        // Vanilla `ServerLevel.tick` order: block and fluid ticks, random ticks inside
        // `chunkSource.tick`, block events, entities, then block entities. Falling blocks and
        // primed TNT spawned by block or random ticks move in the same game tick.
        self.island
            .region
            .profiler
            .begin_chunk_phase(ChunkPhase::ScheduledTicks);
        self.island.region.profiler.begin_partition_phase();
        interaction::run_block_ticks(&mut self.island.region);
        self.island.region.profiler.end_partition_phase();
        self.island.region.profiler.end_chunk_phase();
        self.island.region.profiler.lap(Phase::ScheduledTicks);
        interaction::run_random_ticks(&mut self.island.region);
        self.island.region.profiler.lap(Phase::RandomTicks);
        self.island
            .region
            .profiler
            .begin_chunk_phase(ChunkPhase::BlockEvents);
        self.island.region.profiler.begin_partition_phase();
        interaction::run_block_events(&mut self.island.region);
        self.island.region.profiler.end_partition_phase();
        self.island.region.profiler.end_chunk_phase();
        self.island.region.profiler.lap(Phase::BlockEvents);
        interaction::run_entity_ticks(self.island.entity_threads, &mut self.island.region);
        if self.island.entity_threads == 0 {
            self.island.region.profiler.lap(Phase::Entities);
        }
        self.island
            .region
            .profiler
            .begin_chunk_phase(ChunkPhase::BlockEntities);
        self.island.region.profiler.begin_partition_phase();
        interaction::run_block_entity_ticks(&mut self.island.region);
        self.island.region.profiler.end_partition_phase();
        self.island.region.profiler.end_chunk_phase();
        self.island.region.profiler.lap(Phase::BlockEntities);
        // 玩家自己每 tick 的挖掘进度。官方在实体 tick 里跑 `ServerPlayerGameMode.tick`，
        // 这里跟着玩家 tick 放在随机刻之后。
        interaction::tick_mining(&mut self.island.region);
        // 方块实体的 tick：告示牌在这里清掉走远的编辑者。
        sign::tick_editors(&mut self.island.region);
        // 容器开合人数的重查和菜单同步。
        container::tick(&mut self.island.region);
        menu::tick(&mut self.island.region);
        lectern::tick(&mut self.island.region);
        chat::tick_spam_throttlers(&mut self.island.region);
        crate::drops::tick_drop_throttle(&mut self.island.region);
        crate::command::admin::tick_idle(&mut self.island.region);
        // 官方 `ServerPlayer.doTick` 的六个只读准则：岛算变了的值，主控写计分板。
        //
        // The six read-only criteria from official `ServerPlayer.doTick`: the island
        // computes the changes, the master writes the scoreboard.
        let (mut port, region) = self.island_port();
        crate::scoreboard::tick_criteria(&mut port, region);
        self.island.region.profiler.lap(Phase::PlayerMisc);

        entity_sync::update_derived_state(&mut self.island.region);
        chunk_tracking::update_views(&mut self.island.region);
        self.island.region.profiler.lap(Phase::Views);
        chunk_broadcast::broadcast_changes(&mut self.island.region);
        self.island.region.profiler.lap(Phase::BlockChangeBroadcast);
        entity_sync::sync_entities(&mut self.island.region);
        entity_tracking::sync(&mut self.island.region);
        entity_sync::send_equipment_changes(&mut self.island.region);
        interaction::send_block_changed_acks(&mut self.island.region);
        if let Some(frames) = self.master.global.tick_latency_broadcast() {
            self.island.region.links.broadcast(&frames);
        }
        self.island
            .region
            .profiler
            .lap(Phase::EntitySyncAndTracking);
        chunk_tracking::send_chunks(&mut self.island.region);
        movement::finish_tick(&mut self.island.region);
        self.island.region.profiler.lap(Phase::ChunkSending);
        self.hand_back();
        let flush_bytes = self.island.region.links.flush();
        self.island
            .region
            .profiler
            .record(Metric::FlushBytes, flush_bytes);
        self.island.region.profiler.lap(Phase::Flush);
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
            // 自动保存照官方 autoSave：只入队，不 join、不排阻塞栅栏（R5 / F3）。
            // Autosave like official autoSave: enqueue only, never join or block.
            self.save_all_players();
            let files = self
                .master
                .save
                .as_mut()
                .map(|save| save.encode_saves(&self.master.global, crate::admin::now_millis()));
            match files {
                Some(Ok(files)) => self.master.host.submit_file_writes(files),
                Some(Err(error)) => warn!(%error, "could not encode level data for autosave"),
                None => {}
            }
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
        self.master.host.save_loaded_dirty(
            &mut self.island.region,
            self.master.director_tick,
            save_enabled,
        );
        if self.master.global.admin.stop_requested {
            let flow = self.shut_down();
            self.island.region.profiler.lap(Phase::TickTail);
            return flow;
        }
        // 名单变更在这一 tick 末尾统一发布：一条命令改多个目标只写一次盘。
        let changes = crate::command::admin::take_changes(&mut self.island.region);
        if changes.any() {
            self.publish_admin(changes);
        }
        self.island.region.profiler.lap(Phase::TickTail);
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
        let mut batch = Vec::new();
        let disconnected = self.master.inbox.drain_into(&mut batch);
        let mut players = Vec::new();
        for message in &batch {
            if let Inbound::Join(request) = &message.message {
                request.inbox.drain_into(&mut players);
            }
        }
        for player in self.island.region.players.values() {
            player.inbox.drain_into(&mut players);
        }
        for (_, inbox) in &self.master.refused_inboxes {
            inbox.drain_into(&mut players);
        }
        batch.append(&mut players);
        crate::inbound::merge(&mut batch);
        (batch, disconnected)
    }

    /// 按序号处理这一 tick 之前收到的所有消息。
    fn process_inbox(&mut self) -> ControlFlow<()> {
        // 先全部取出再处理：配额要处理完才归还，连接任务补不进新消息，所以这一步一定会结束。
        let (messages, disconnected) = self.collect_inbound();

        self.island
            .region
            .profiler
            .record(Metric::InboundMessages, messages.len() as u64);
        for Sequenced { message, .. } in messages {
            match message {
                Inbound::Join(request) => self.on_join(*request),
                Inbound::Request {
                    player,
                    request,
                    in_flight,
                } => {
                    self.on_request(player, request);
                    drop(in_flight);
                    // 玩家请求只经端口写全局状态，端口应用时自己重发快照。
                    // Player requests write global state only through the port,
                    // which republishes the snapshot itself when applying.
                    continue;
                }
                Inbound::Latency { player, millis } => {
                    self.master.global.set_latency(player, millis)
                }
                Inbound::Leave { player } => self.on_leave(player),
                Inbound::WhitelistReload(reload) => {
                    self.master.global.admin.whitelist = reload.entries;
                    crate::command::admin::after_reload(
                        &mut self.master.global,
                        &mut self.island.region,
                    );
                }
                // 连接层的控制台线程读到的命令，在世界线程里用服务器控制台来源执行。
                // A console line read by the connection layer runs here with the server-console source.
                Inbound::ConsoleCommand(line) => {
                    crate::command::console::on_console_command(
                        &mut self.master.global,
                        &mut self.island.region,
                        &line,
                    );
                }
                Inbound::Shutdown => return self.shut_down(),
            }
            // 主控队列的消息（进服、延迟、名单重载、控制台命令）直接改 `Global`：
            // 每条处理完重发快照，下一条读到的和直接读 `Global` 一样。
            // Master-queue messages (join, latency, list reload, console commands)
            // write `Global` directly: republish after each so the next message
            // reads exactly what `Global` holds.
            self.master.global.publish(&mut self.island.region);
        }
        if disconnected {
            return self.shut_down();
        }
        ControlFlow::Continue(())
    }

    /// 把进服的 10 步串起来，全部写进这名玩家的同一条出站缓冲。`coverage.md` 4.1。
    fn on_join(&mut self, mut request: JoinRequest) {
        let id = request.id;
        if self.island.region.players.contains_key(&id) {
            warn!(player = id.0, "ignoring a second join with the same id");
            return;
        }
        // 连接层检查过一次，但封禁命令可能在两次检查之间改了名单。
        // 世界线程按自己的权威副本复查，理由和登录阶段完全一样。
        let refusal = if self.master.authoritative_admin {
            self.master.global.admin.login_refusal(
                request.uuid,
                &request.peer_ip,
                self.island.region.now_millis(),
                request.online_at_login,
                self.island.region.max_players() as u32,
            )
        } else {
            None
        };
        if let Some(refusal) = refusal {
            let reason = crate::command::admin::refusal_message(&refusal);
            self.island.region.links.add(id, request.sink);
            self.island.region.links.kick(id, &reason);
            self.master.refused_inboxes.push((id, request.inbox));
            return;
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
            .unwrap_or(self.island.region.global.default_game_mode);
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
            None => self.island.region.global.spawn,
        };
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

        // 进服序列里读的世界设置都来自岛上的快照（岛的实施计划 4.1：进服由岛做）。
        // The join sequence reads world settings from the island's snapshot (plan 4.1).
        let difficulty_frames = self.island.region.global.difficulty_frames();
        let spawn = join_spawn.pos;
        let spawn_chunk = ChunkPos::new(spawn.x >> 4, spawn.z >> 4);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: Some(spawn_chunk),
        }]);
        self.island.region.add_player(
            request,
            JoinSettings {
                game_mode,
                difficulty_frames: &difficulty_frames,
                spawn: join_spawn,
                hashed_seed: self.island.region.global.hashed_seed(),
                permission_level,
                is_op,
            },
        );
        self.island
            .region
            .last_action_millis
            .insert(id, self.island.region.now_millis());

        if let Some(frames) = self.master.global.tab_list() {
            self.island.region.links.send_frames(id, &frames);
        }
        let frames = self.master.global.add_player(id, entry);
        self.island.region.links.broadcast(&frames);

        self.island.region.send_own_entity_data(id);

        if let Some(frames) = self.master.global.joined_message(id) {
            // 官方 `PlayerList.broadcastSystemMessage` 逐玩家看可见性；进服消息跳过本人。
            //
            // Official `PlayerList.broadcastSystemMessage` checks visibility per player;
            // the join message skips the joining player.
            crate::chat::broadcast_system_message(&mut self.island.region, &frames, Some(id));
        }

        // 官方 `ServerScoreboard.getStartTrackingPackets` 和 `CustomBossEvents.onPlayerConnect`
        // 都在进服时只发给这名玩家。
        //
        // Official `ServerScoreboard.getStartTrackingPackets` and
        // `CustomBossEvents.onPlayerConnect` both go only to the joining player.
        let frames = self.master.global.scoreboard().join_frames();
        self.island.region.links.send_frames(id, &frames);
        self.master
            .global
            .boss_bars_mut()
            .on_join(&mut self.island.region, id, uuid);

        // `set_default_spawn_position` 发的是世界出生点（`LevelData`），不是玩家存档里的位置。
        // 玩家位置走上面的 `JoinSettings.spawn`（存档），区块中心走 `player.chunk_pos()`。
        // `set_default_spawn_position` carries the world spawn from `LevelData`, not
        // the saved player position. The player's position comes from
        // `JoinSettings.spawn` above (the save), and the chunk center from `player.chunk_pos()`.
        self.island.region.start_loading_chunks(id);

        // 物品栏 / 末影箱 / 朝向等运行时字段在玩家建好之后写回去。
        if let Some(data) = &restored
            && let Some(player) = self.island.region.players.get_mut(&id)
        {
            restore_inventory(player, data);
        }
    }

    /// 消费 `save-all` / `save-on` / `save-off` 压进来的请求（命令动作不碰 IO）。
    fn consume_save_commands(&mut self) {
        let requests = std::mem::take(&mut self.master.global.save_requests);
        for request in requests {
            match request {
                SaveCommand::SaveAll { flush, source } => {
                    // 照官方 saveEverything：先给所有脏的已加载区块拍快照交给 IO 线程，
                    // 再存玩家，最后把 level.dat / data/ 的字节交给 IO 线程。
                    // 世界线程只编码、不碰磁盘，也不阻塞等 IO（R5 / F3）。
                    // Like official saveEverything: snapshot dirty loaded chunks,
                    // save players, then hand level.dat/data bytes to the IO
                    // thread. The world thread only encodes, never blocks on IO.
                    self.master
                        .host
                        .snapshot_all_dirty(&mut self.island.region, self.master.director_tick);
                    self.save_all_players();
                    let files = self.master.save.as_mut().map(|save| {
                        save.encode_saves(&self.master.global, crate::admin::now_millis())
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
                        continue;
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

    /// 发 `commands.save.success`（广播给 OP），和命令侧原来的一致。
    /// Send `commands.save.success` (broadcast to ops), as the command did.
    fn send_save_success(&mut self, source: PlayerId) {
        crate::command::feedback::success(
            &mut self.island.region,
            source,
            Component::translate("commands.save.success", []),
            true,
        );
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
        let failure = crate::command::feedback::fail("commands.save.failed");
        crate::command::feedback::report(&mut self.island.region, source, "save-all", &failure);
    }

    /// 存档一名在线玩家。文件写交给 IO 线程，世界线程不等待。
    fn save_one_player(&mut self, id: PlayerId) {
        let encoded = self
            .island
            .region
            .players
            .get(&id)
            .and_then(encode_player_save);
        if let Some((uuid, bytes)) = encoded {
            self.master.host.submit_player_write(&uuid, bytes);
        }
    }

    /// 存档所有在线玩家（自动保存、关服、`save-all`）。
    fn save_all_players(&mut self) {
        // 先把所有存档编出来，避免遍历 `region.players` 时又可变借 `host`。
        // Encode every save first so we don't hold a borrow of `region.players`
        // while mutably borrowing `host`.
        let encoded: Vec<(String, Vec<u8>)> = self
            .island
            .region
            .players
            .values()
            .filter_map(encode_player_save)
            .collect();
        for (uuid, bytes) in encoded {
            self.master.host.submit_player_write(&uuid, bytes);
        }
    }

    /// 处理离开，顺序见 `coverage.md` 4.2。可以重复调用：世界里没有这名玩家时直接忽略，
    /// 连接任务还没发 `Join` 就结束时走的就是这条路。
    fn on_leave(&mut self, id: PlayerId) {
        self.master
            .refused_inboxes
            .retain(|(refused, _)| *refused != id);
        if !self.island.region.players.contains_key(&id) {
            return;
        }
        // 连接已经结束了，后面的包都不用写给它。
        self.island.region.links.remove(id);
        self.island.region.last_action_millis.remove(&id);
        self.island.region.peer_ips.remove(&id);

        // 「离开了游戏」和 Tab 名单是全局的，经端口交给主控（岛的实施计划 4.1）。
        // "Left the game" and the tab list are global; they go to the master
        // through the port (island plan 4.1).
        let (mut port, region) = self.island_port();
        port.submit(region, GlobalEffect::LeaveMessage { player: id });
        entity_sync::on_leave(&mut self.island.region, id);
        entity_tracking::on_leave(&mut self.island.region, id);
        // 玩家断开时官方不回 `container_close`，光标上的东西直接丢弃。
        menu::on_leave(&mut self.island.region, id);
        lectern::on_leave(&mut self.island.region, id);
        // 官方 `PlayerList.remove` 在移除玩家前先存盘；`PlayerList.save` /
        // `saveAll` 都不看 `noSave`，所以 `/save-off` 期间离开照存（F7）。
        // Official `PlayerList.remove` saves before removing; `PlayerList.save`
        // / `saveAll` ignore `noSave`, so a leave during `/save-off` still saves.
        self.save_one_player(id);
        self.island.region.players.remove(&id);
        self.island.region.player_grid.remove(id);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(id.0 as u32),
            to: None,
        }]);
        let (mut port, region) = self.island_port();
        port.submit(region, GlobalEffect::Left { player: id });
    }

    /// 把一条请求交给负责它的模块。被踢的玩家要等 `Leave` 才离开世界，这段时间它的请求都忽略。
    fn on_request(&mut self, player: PlayerId, request: PlayRequest) {
        if !self.island.region.links.is_open(player) {
            return;
        }
        crate::command::admin::note_action(&mut self.island.region, player, &request);
        match &request {
            PlayRequest::ChatCommand { command, signed } => {
                let command = command.clone();
                let signed = signed.clone();
                let (mut port, region) = self.island_port();
                command::on_player_command(&mut port, region, player, &command, signed.as_ref());
                return;
            }
            PlayRequest::CommandSuggestion { id, command } => {
                let (transaction, command) = (*id, command.clone());
                command::on_suggestion(&mut self.island.region, player, transaction, &command);
                return;
            }
            PlayRequest::ChatSession(session) => {
                let session = session.clone();
                let (mut port, region) = self.island_port();
                if let Some(update) = secure_chat::on_session(region, player, session) {
                    port.submit(region, GlobalEffect::ChatSession { player, update });
                }
                return;
            }
            _ => {}
        }
        let region = &mut self.island.region;
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
                self.on_client_information(player, information);
            }
            PlayRequest::Chat {
                message,
                signature,
                time_millis,
                salt,
                last_seen,
            } => {
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
            }
            PlayRequest::ChatAck { offset } => secure_chat::on_ack(region, player, offset),
            // 聊天命令、建议和会话更新在上面处理过了，这里只是让匹配穷尽。
            //
            // Chat commands, suggestions, and session updates are handled above. This arm only makes the match exhaustive.
            PlayRequest::ChatSession(_)
            | PlayRequest::ChatCommand { .. }
            | PlayRequest::CommandSuggestion { .. } => {}
            PlayRequest::ClientCommand { action } => {
                if action
                    == mc_bamboo_protocol::packet::play::ClientCommandKind::RequestGameRuleValues
                {
                    crate::gamerules::on_request_values(&mut self.island.region, player);
                } else {
                    chat::on_client_command(&mut self.island.region, player, action);
                }
            }
            PlayRequest::ChangeGameMode { mode } => self.on_change_game_mode(player, mode),
            PlayRequest::SetGameRule { entries } => {
                let (mut port, region) = self.island_port();
                crate::gamerules::on_set_packet(&mut port, region, player, entries);
            }
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
            PlayRequest::PlayerCommand { action } => {
                entity_sync::on_player_command(region, player, action);
            }
            PlayRequest::PlayerAbilities { flags } => {
                entity_sync::on_player_abilities(region, player, flags);
            }
            PlayRequest::Punch => entity_sync::on_punch(region, player),
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
            PlayRequest::KeepAlive(_) | PlayRequest::PingRequest(_) | PlayRequest::Other(_) => {}
        }
    }

    /// `handleChangeGameMode`。权限不够时只记日志。
    ///
    /// `handleChangeGameMode`. Without permission this only logs.
    fn on_change_game_mode(&mut self, id: PlayerId, mode: GameMode) {
        let Some((allowed, name)) = self.island.region.players.get(&id).map(|player| {
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
        let (mut port, region) = self.island_port();
        command::change_own_mode(&mut port, region, id, mode);
    }

    /// 客户端的设置变了。视距由下一次跟踪阶段生效，皮肤外层和主手由实体同步发出去。
    fn on_client_information(&mut self, id: PlayerId, information: ClientInformation) {
        let Some(player) = self.island.region.players.get_mut(&id) else {
            return;
        };
        player.client = information;

        let show = player.shows_hat();
        let (mut port, region) = self.island_port();
        port.submit(region, GlobalEffect::Hat { player: id, show });
    }

    /// 岛写全局状态的口子（按 [`Master::port_mode`]）和岛本身。
    ///
    /// The island's port for global writes (per [`Master::port_mode`]) and the
    /// island itself.
    fn island_port(&mut self) -> (GlobalPort<'_>, &mut Region) {
        let port = match self.master.port_mode {
            PortMode::Direct => GlobalPort::Direct(&mut self.master.global),
            PortMode::Queued => GlobalPort::Queued(&mut self.island.effects),
        };
        (port, &mut self.island.region)
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
        if self.island.effects.is_empty() {
            return;
        }
        let effects = std::mem::take(&mut self.island.effects);
        crate::global_port::apply_all(&mut self.master.global, &mut self.island.region, effects);
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
        // 还没交回的全局改动先应用，存盘看到的是最新的。
        // Apply global changes not yet handed back so the save sees them.
        self.hand_back();
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
        let files = self
            .master
            .save
            .as_mut()
            .map(|save| save.encode_saves(&self.master.global, crate::admin::now_millis()));
        match files {
            Some(Ok(files)) => self.master.host.submit_file_writes(files),
            Some(Err(error)) => warn!(%error, "could not encode level data on shutdown"),
            None => {}
        }
        // 收回全部已加载区块、提交全部积压写，然后排空 IO 线程并 sync_all：
        // 这是唯一允许世界线程阻塞等 IO 的地方。
        // Reclaim every loaded chunk, hand over every queued write, then drain
        // and sync_all: the only place the world thread may block on IO.
        self.master.host.shutdown(&mut self.island.region);
        if let Some(save) = &mut self.master.save {
            save.close();
        }
        info!(
            codec_nanos = crate::director_host::codec_nanos(),
            io_failures = self.master.host.io_failures(),
            "chunk codec time and IO failures on this run"
        );
        let reason = Component::translate(SERVER_SHUTDOWN, []);
        self.island.region.links.kick_all(&reason);
        ControlFlow::Break(())
    }
}

/// 编出一名在线玩家的存档字节（纯函数，不碰 `self`，避免和 `host` 的可变借用冲突）。
/// 返回带连字符的 uuid 和 gzip 字节，交给 [`World::save_all_players`] 提交给 IO 线程。
/// Encode one live player's save (pure, so it does not conflict with the mutable
/// borrow of `host`). Returns the hyphenated uuid and gzip bytes for submission.
fn encode_player_save(player: &crate::player::Player) -> Option<(String, Vec<u8>)> {
    let data = runtime_player_data(player);
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
        let behind = now.saturating_duration_since(self.next_tick);
        self.skipped_catch_up = behind > MAX_CATCH_UP;
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
#[path = "world/island_scale_tests.rs"]
mod island_scale_tests;
#[cfg(test)]
mod loaded_tests;
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
