//! 世界的一块区域：区块、里面的玩家，以及这些玩家的出站链路。
//!
//! 这里的方法拿不到 [`Global`](crate::global::Global)，全服的事情由
//! [`World`](crate::World) 串起来。现在只有一个 `Region`，将来有多个时，
//! 玩家和它的链路一起搬家。

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockTicks, LevelRandom, RedstoneMemory, entity::Entities,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::registries::{attribute, dimension_type, world_clock};
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_protocol::packet::play::{
    AttributeModifier, AttributeOperation, AttributeSnapshot, EntityData, GameEvent, GameMode,
    InitializeBorder, Login, PlayerAbilities, SetDefaultSpawnPosition, SetEntityData, SetTime,
    UpdateAttributes,
};
use mc_bamboo_world::{flat, light::LightChanges, poi::PoiStore, store::ChunkStore};

use crate::{
    chunk_encoding::{ChunkCache, encode_template},
    chunk_tracking,
    entity_tracking::EntityTracking,
    experience::Experience,
    global::{Clock, GameTime},
    inbound::{JoinRequest, PlayerId},
    interaction::{BlockChanges, level::RegionLevel},
    links::Links,
    menu::Menu,
    movement,
    player::Player,
    player_grid::PlayerGrid,
    profiler::Profiler,
    secure_chat::ServiceProfileKeys,
    tick_levels::TickLevels,
    world_state::RespawnData,
};

pub mod chunk_content;
mod column;
pub(crate) use column::take_pristine_cost;
#[cfg(test)]
mod column_tests;
pub(crate) mod content_snapshot;
pub(crate) mod dirty;
pub(crate) mod island_split;
pub(crate) mod parking;
pub(crate) mod partition;
pub(crate) mod unit;

// 区块平面和多 region 是后面的步骤，现在还没有调用方。
pub use crate::command_block::{ArchiveCommandBlock, CommandBlockData};
pub use crate::container::ContainerEntry;
pub use crate::container::ContainerKind;
#[allow(unused_imports)]
pub use column::{NotReady, PackedContainer, RegionColumn, RegionColumnParts, RegionColumnView};
pub(crate) use island_split::IslandPart;
pub(crate) use island_split::{IslandIdentity, IslandRandom};

pub(crate) const DIMENSION: &str = "minecraft:overworld";
/// 区块还没就绪时的临时位置：`PlayerSpawnFinder.fixupSpawnHeight` 那一段的高度，
/// 只用于登记玩家和挂票，不发位置包、不是最终落点。进服 / 重生的最终位置
/// 统一由 [`crate::spawn_landing::landing`] 算。
///
/// Temporary position while chunks are unready: the height from
/// `PlayerSpawnFinder.fixupSpawnHeight`. It only registers the player and its
/// tickets; no position packet is sent and it is never the final landing. The
/// final join / respawn position always comes from
/// [`crate::spawn_landing::landing`].
pub(crate) fn temporary_position(region: &Region) -> Vec3 {
    let pos = region.global.spawn.pos;
    let y = if region.global.world_gen.level_type.is_flat() {
        flat::surface_y()
    } else {
        crate::spawn_landing::noise_sea_level(region.global.world_gen.level_type) + 1
    };
    Vec3::new(f64::from(pos.x) + 0.5, f64::from(y), f64::from(pos.z) + 0.5)
}

/// 官方 `ServerPlayer.onUpdateAbilities`：能力按当前游戏模式推。
pub(crate) fn abilities_for(game_mode: GameMode) -> PlayerAbilities {
    let creative = matches!(game_mode, GameMode::Creative | GameMode::Spectator);
    PlayerAbilities {
        invulnerable: creative,
        flying: game_mode == GameMode::Spectator,
        can_fly: creative,
        instant_build: game_mode == GameMode::Creative,
        flying_speed: ABILITIES.flying_speed,
        walking_speed: ABILITIES.walking_speed,
    }
}

pub(crate) const ABILITIES: PlayerAbilities = PlayerAbilities {
    invulnerable: true,
    flying: false,
    can_fly: true,
    instant_build: true,
    flying_speed: 0.05,
    walking_speed: 0.1,
};

/// 创造模式的触及距离加成完全由服务端下发，客户端代码里没有这两个数。
///
/// The creative reach bonus is sent by the server. The client code does not contain these numbers.
pub(crate) const CREATIVE_ATTRIBUTES: [AttributeSnapshot<'static>; 2] = [
    AttributeSnapshot {
        attribute: attribute::BLOCK_INTERACTION_RANGE,
        base: 4.5,
        modifiers: &[AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }],
    },
    AttributeSnapshot {
        attribute: attribute::ENTITY_INTERACTION_RANGE,
        base: 3.0,
        modifiers: &[AttributeModifier {
            id: "minecraft:creative_mode_entity_range",
            amount: 2.0,
            operation: AttributeOperation::AddValue,
        }],
    },
];

/// 岛的时钟（模拟时间）。计划刻队列的 `now`、方块实体的休眠时钟、容器重查、讲台、
/// 传送确认、红石缓存都以它为准（岛的实施计划 5.1）。岛建立时取当时的
/// [`GameTime`]，之后每刻加一；精确模式下两者永远相等，但仍不是一回事：
/// [`GameTime`] 归主控，昼夜、`/time`、存盘的 `Time` 读它。
///
/// 内部用 `i64`：合并时被吸收方的时刻要平移 `吸收方 − 被吸收方`，可能平移到 0 以下。
///
/// The island clock (simulation time). Scheduled ticks, block-entity sleep,
/// container rechecks, lecterns, teleport acks and the redstone cache all use
/// it. It starts at the current [`GameTime`] and advances by one per tick, so
/// the two are always equal in exact mode, but [`GameTime`] belongs to the
/// master. Stored as `i64` because a merge may shift instants below zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RegionTick(i64);

impl RegionTick {
    /// 岛建立时的初值：当时的世界时间。
    ///
    /// The initial value when an island is created: the world time then.
    pub(crate) fn starting_at(time: GameTime) -> Self {
        Self(time.0)
    }

    /// 现在的刻号，给方块逻辑当 `getGameTime()` 用。
    pub(crate) fn game_time(self) -> i64 {
        self.0
    }

    /// 从 `earlier` 到现在过了多少 tick。
    pub(crate) fn since(self, earlier: Self) -> u64 {
        u64::try_from(self.0.saturating_sub(earlier.0)).unwrap_or(0)
    }

    /// `later` 个 tick 之后。
    pub(crate) fn plus(self, later: u64) -> Self {
        Self(
            self.0
                .saturating_add(i64::try_from(later).unwrap_or(i64::MAX)),
        )
    }

    /// 平移 `delta` 刻，岛合并时用。
    ///
    /// Shifted by `delta` ticks, for island merges.
    pub(crate) fn rebased(self, delta: i64) -> Self {
        Self(self.0.saturating_add(delta))
    }

    /// 是否已经到了 `at` 这一刻。这一 tick 开始时增长，所以到期的刻在自己这一 tick 跑。
    pub(crate) fn reached(self, at: Self) -> bool {
        self.0 >= at.0
    }

    /// `at` 距现在还有多少刻，已经到期的算 0。搬区块时把绝对时刻换算成相对值用。
    pub(crate) fn remaining(self, at: Self) -> u64 {
        u64::try_from(at.0.saturating_sub(self.0)).unwrap_or(0)
    }
}

/// [`Region::rebase_clock_parts`] 平移哪几类。正式路径只用 [`Self::ALL`]。
///
/// Which classes [`Region::rebase_clock_parts`] shifts; production uses [`Self::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockParts {
    pub(crate) clock: bool,
    pub(crate) block_ticks: bool,
    pub(crate) redstone: bool,
    pub(crate) block_entities: bool,
    pub(crate) containers: bool,
    pub(crate) lecterns: bool,
    pub(crate) teleports: bool,
    pub(crate) entities: bool,
}

impl ClockParts {
    pub(crate) const ALL: Self = Self {
        clock: true,
        block_ticks: true,
        redstone: true,
        block_entities: true,
        containers: true,
        lecterns: true,
        teleports: true,
        entities: true,
    };
}

/// 世界的一块区域。`players` 和 `links` 是两个字段，遍历玩家时可以写链路。
pub(crate) struct Region {
    /// 已经进入世界的玩家。被踢的玩家留在这里，直到连接层的 `Leave` 到达。
    pub(crate) players: BTreeMap<PlayerId, Player>,
    /// 玩家在哪一格，广播找附近的人用。和 `players` 一起增删，见 [`Region::insert_player`]。
    ///
    /// Which cell each player is in, for broadcasts. Kept in step with `players`; see
    /// [`Region::insert_player`].
    pub(crate) player_grid: PlayerGrid,
    pub(crate) links: Links,
    /// 这一刻里方块侧 / 实体侧记下的进度触发事件，在玩家 tick 末尾统一 dispatch。
    /// 借用被拆开的地方（比如 `HurtParts`）只往这里压事件，不另起一套事件系统。
    pub(crate) pending_triggers: Vec<crate::advancement_triggers::PendingEvent>,
    pub(crate) chunks: ChunkStore,
    /// POI 段：`SectionPos → PoiSection`，按 `ChunkPos` 分脏。和 `chunks` 平级，
    /// 改动不进 `ChunkRevision`，也不进区块包缓存；单独存到 `poi/`。
    pub(crate) poi: PoiStore,
    /// 每个已交付区块的等级档位和由此算出的 tick 集合。只从 Director 的消息更新。
    pub(crate) tick_levels: TickLevels,
    /// 没改过的区块共用的编码结果。
    pub(crate) template_body: Vec<u8>,
    /// 改过的区块的编码结果，区块变了就重新编码。
    pub(crate) chunk_cache: ChunkCache,
    /// 这一 tick 改过的方块，等广播阶段发出去。
    pub(crate) block_changes: BlockChanges,
    /// 这一 tick 开头算完光照以后变了的段，等广播阶段发出去。
    pub(crate) light_changes: BTreeMap<ChunkPos, LightChanges>,
    /// 排队的计划刻。
    pub(crate) block_ticks: BlockTicks,
    /// 有内容的方块实体，按位置索引。只在这个区域里存在，没有持久化。
    pub(crate) containers: crate::pos_map::PosMap<ContainerEntry>,
    /// 方块实体从物品上带来的组件，官方 `BlockEntity.collectComponents` 的结果，
    /// 见 [`crate::block_items`]。
    pub(crate) block_items: crate::pos_map::PosMap<ComponentPatch>,
    /// 每名玩家打开的菜单。物品栏菜单不在里面，它没有状态要同步。
    pub(crate) menus: BTreeMap<PlayerId, Menu>,
    /// 讲台上的书和打开着的讲台菜单。
    ///
    /// Books on lecterns, and the lectern menus currently open.
    pub(crate) lecterns: crate::lectern::Tables,
    /// 活塞这类会动的方块实体。实体算阶段要按位置读盖子开着的潜影盒。
    pub(crate) block_entities: BlockEntities,
    /// 等这一刻末尾处理的方块事件，官方的 `Level.blockEvents`。
    block_events: BlockEvents,
    /// 方块逻辑的随机源，官方的 `Level.random` 和 `randValue`。
    pub(crate) random: LevelRandom,
    /// 红石缓存，一个 region 一份；开关从 `bamboo.toml` 读，默认关。
    pub(crate) redstone_cache: mc_bamboo_blocks::cache::RedstoneCache,
    /// 自然刷怪要不要按玩家算上限（`bamboo.toml` 的 `spawn_per_player_cap`）；
    /// 关掉只剩官方的全局上限。
    ///
    /// Whether natural spawning caps each category per player
    /// (`spawn_per_player_cap` in `bamboo.toml`); off keeps only the vanilla
    /// global cap.
    pub(crate) spawn_per_player_cap: bool,
    /// 每只生物每刻最多处理几次推挤（0 = 不限）。
    pub(crate) max_entity_collisions: usize,
    /// 三档降频的两个距离边界（0 = 关，精确模式）和每刻唤醒名额的倍率。
    pub(crate) mob_throttle_distance: i32,
    pub(crate) mob_sleep_distance: i32,
    pub(crate) mob_wakeups_scale: usize,
    /// 独立寻路阶段（实验，缺省关）和它的节点预算 / 失败冷却。
    pub(crate) mob_pathfinding_phase: bool,
    pub(crate) mob_pathfinding_budget: u32,
    pub(crate) mob_pathfinding_fail_cooldown: bool,
    /// 被困村民与睡觉村民大脑降频配置（缺省关）。
    pub(crate) villager_lobotomize: bool,
    pub(crate) villager_lobotomize_interval: i32,
    pub(crate) villager_lobotomize_wait_trade_lock: bool,
    pub(crate) villager_sleep_brain_interval: i32,
    /// `ServerLevel.customSpawners` 里的 `PatrolSpawner`。每岛一份，和官方的每维度一份
    /// 对应，岛拆分时两边各带自己的计时器。
    ///
    /// The `PatrolSpawner` of `ServerLevel.customSpawners`. One per island, matching
    /// vanilla's one per dimension; a split carries the timer with each side.
    pub(crate) patrol_spawner: mc_bamboo_blocks::entity::mob::patrol::PatrolSpawner,
    /// 战利品表的 `random_sequence`。平坦世界的种子是 0，和 `login` 里的 `hashed_seed` 一样。
    ///
    /// Loot `random_sequence`s. The flat world seed is 0, the same value as `hashed_seed` in `login`.
    pub(crate) loot_sequences: mc_bamboo_loot::RandomSequences,
    /// 音效包的随机种子，官方的 `Level.soundSeedGenerator`。和世界随机分开，只影响客户端挑哪个变体。
    ///
    /// Seed for sound packets, vanilla `Level.soundSeedGenerator`. Separate from the world
    /// random; it only picks which sound variant the client plays.
    pub(crate) sound_seed_generator: LevelRandom,
    /// 下落的方块和点燃的 TNT。世界线程独占，见 `docs/folia.md`。
    ///
    /// Falling blocks and primed TNT. The world thread owns them; see `docs/folia.md`.
    pub(crate) entities: Entities,
    /// 这些实体和玩家之间的配对。
    ///
    /// Pairing between those entities and players.
    pub(crate) entity_tracking: EntityTracking,
    /// 比较器的输出、书架最近动的格子、红石火把最近的开关，还有讲台的页码。
    ///
    /// Comparator output, bookshelf slots, redstone-torch toggles, and lectern pages.
    pub(crate) redstone: RedstoneMemory,
    pub(crate) tick: RegionTick,
    /// 服务端视距，已经 clamp 到合法范围。
    pub(crate) view_distance: i32,
    max_players: i32,
    /// 写进 `login` 包的那一位，官方取的是 `MinecraftServer.usesAuthentication()`。
    pub(crate) online_mode: bool,
    /// 配置里的 `enforce-secure-profile`。真正强制还要正版验证和公钥都在。
    ///
    /// Configured `enforce-secure-profile`. Enforcement also needs online mode and service keys.
    pub(crate) enforce_secure_profile: bool,
    /// Mojang 的 `playerCertificateKeys`。连接任务负责填充。
    ///
    /// Mojang's `playerCertificateKeys`. The connection task fills them in.
    pub(crate) service_keys: Arc<ServiceProfileKeys>,
    /// 测试把聊天时钟钉死，两次同样的输入才有同样的字节。`None` 用系统时间，和官方 `Instant.now` 一样。
    ///
    /// Tests pin the chat clock so the same input produces the same bytes.
    /// `None` uses the system clock, matching official `Instant.now`.
    pub(crate) clock_millis: Option<i64>,
    /// 主控发布的全局状态快照（岛的实施计划第 6 节第 5a 步）：游戏规则、天气、时间、
    /// 世界边界、难度、出生点、计分板等。岛里只读这一份，权威副本在 `Global`。
    ///
    /// The global-state snapshot the master publishes (island plan, section 6,
    /// step 5a): rules, weather, time, border, difficulty, spawn, scoreboard and
    /// so on. The island only reads this; the authoritative copy is on `Global`.
    pub(crate) global: crate::global::GlobalView,
    /// 官方 `server.properties` 的 `broadcast-console-to-ops`：控制台命令反馈要不要
    /// 广播给在线 OP。`WorldConfig` 启动时带进来，和游戏规则分开、运行中不变。
    ///
    /// Official `server.properties` `broadcast-console-to-ops`: whether console command
    /// feedback is broadcast to online ops. Set from `WorldConfig` at startup; separate
    /// from game rules and fixed for the run.
    pub(crate) broadcast_console_to_ops: bool,
    /// 命令方块里保存的命令。不执行，也不存档。
    ///
    /// Commands stored in command blocks. They are not executed and not saved.
    pub(crate) command_blocks: crate::pos_map::PosMap<crate::command_block::CommandBlockData>,
    /// 这一 tick 里改过的管理名单，tick 末尾统一发布并写盘。
    pub(crate) admin_changes: crate::admin::AdminChanges,
    /// 每名玩家最近一次动作的时刻，单位毫秒。官方 `ServerPlayer.lastActionTime`。
    pub(crate) last_action_millis: BTreeMap<PlayerId, i64>,
    /// 每名玩家的对端 IP 文本（不含端口），供 `ban-ip` 和登录复查用。
    pub(crate) peer_ips: BTreeMap<PlayerId, String>,
    /// 这个 region 每刻的耗时和数量指标。世界线程独占，一直开着。
    pub(crate) profiler: Profiler,
    /// 第 2 步的划分状态：默认关，只在调试开关下接进世界循环。
    pub(crate) partitioning: crate::region::partition::Partitioning,
    /// 「上次保存之后改过没有」的标记，见 [`dirty`]。
    pub(crate) dirty: dirty::Dirty,
    /// 官方 `ServerLevel.sleepStatus`：多少活跃玩家、多少人在睡觉。
    pub(crate) sleep: crate::sleep::SleepStatus,
    /// `PhantomSpawner.nextTick`：自定义刷怪器每刻的倒数，跨刻保留。
    ///
    /// `PhantomSpawner.nextTick`: the custom spawner's countdown, kept across ticks.
    pub(crate) phantom_spawner: mc_bamboo_blocks::entity::mob::phantom_spawner::PhantomSpawner,
}

/// 每次启动不同的种子，官方用 `RandomSupport.generateUniqueSeed`，这里用启动时刻的纳秒数。
pub(crate) fn unique_seed() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as i64)
}

// 世界线程以后要能把 `Region` 交给线程池，所以它必须能在线程之间移动。
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Region>();
};

/// 进服时世界线程替这名玩家定好的东西：游戏模式、全局状态里的难度包、出生点和种子哈希，
/// 以及按权威 OP 名单查出的权限和存档里的经验。
///
/// What the world thread settled for a joining player: the game mode, the difficulty frames,
/// spawn and seed hash from the world state, the permissions from the authoritative op list,
/// and the saved experience.
pub(crate) struct JoinSettings<'a> {
    pub(crate) game_mode: GameMode,
    pub(crate) difficulty_frames: &'a [u8],
    pub(crate) spawn: RespawnData,
    pub(crate) hashed_seed: i64,
    pub(crate) permission_level: u8,
    pub(crate) is_op: bool,
    pub(crate) experience: Experience,
    /// 官方 `PrepareSpawnTask.start`：只有新玩家（`SavedPosition.position()` 为空）
    /// 才跑 `PlayerSpawnFinder.findSpawn`；老玩家用存档里的精确位置。
    /// Official `PrepareSpawnTask.start`: only a new player (empty
    /// `SavedPosition.position()`) runs `PlayerSpawnFinder.findSpawn`; a
    /// returning player keeps the exact saved position.
    pub(crate) new_player: bool,
}

impl Region {
    /// 测试里直接把玩家挪到某处，网格跟着更新。
    ///
    /// Test helper: moves a player directly and updates the grid.
    #[cfg(test)]
    pub(crate) fn move_player(&mut self, id: PlayerId, position: Vec3) {
        let Self {
            players,
            player_grid,
            ..
        } = self;
        if let Some(player) = players.get_mut(&id) {
            player.set_position(player_grid, position);
        }
    }

    /// 让玩家进入世界：放进玩家表，也记进网格。
    ///
    /// Puts a player into the world: the player table and the grid.
    pub(crate) fn insert_player(&mut self, player: Player) {
        self.player_grid.place(player.id, player.position());
        self.players.insert(player.id, player);
    }

    /// 测试和 oracle 专用：把区块表切回「无限超平坦」（每个位置都隐式存在）。
    /// 正式构造出来的 region 一律用 [`ChunkStore::loaded`](mc_bamboo_world::store::ChunkStore::loaded)。
    pub(crate) fn use_infinite_flat_chunks(&mut self) {
        self.chunks = mc_bamboo_world::store::ChunkStore::flat();
        self.tick_levels.disable();
    }

    /// 测试专用：直接构造一个 region，用「无限超平坦」的退化实现。
    /// 正式世界的 region 由 `World::build` 走 [`Self::with_keys`]（已加载模式）。
    pub(crate) fn new(view_distance: i32, max_players: i32, online_mode: bool) -> Self {
        let mut region = Self::with_keys(
            view_distance,
            max_players,
            online_mode,
            false,
            crate::secure_chat::empty_keys(),
        );
        region.use_infinite_flat_chunks();
        region
    }

    /// 正版验证和公钥从 `World::with_service_keys` 进来。测试走 [`Self::new`]。
    ///
    /// Online mode and the profile keys arrive through `World::with_service_keys`. Tests use [`Self::new`].
    pub(crate) fn with_keys(
        view_distance: i32,
        max_players: i32,
        online_mode: bool,
        enforce_secure_profile: bool,
        service_keys: Arc<ServiceProfileKeys>,
    ) -> Self {
        let chunks = ChunkStore::loaded();
        let template_body = encode_template(&ChunkStore::flat());
        Self {
            players: BTreeMap::new(),
            player_grid: PlayerGrid::default(),
            links: Links::default(),
            pending_triggers: Vec::new(),
            tick_levels: TickLevels::new(chunks.is_loaded_mode()),
            chunks,
            poi: PoiStore::new(),
            template_body,
            chunk_cache: ChunkCache::default(),
            block_changes: BlockChanges::default(),
            light_changes: BTreeMap::new(),
            block_ticks: BlockTicks::new(),
            containers: crate::pos_map::PosMap::default(),
            block_items: crate::pos_map::PosMap::default(),
            menus: BTreeMap::new(),
            lecterns: crate::lectern::Tables::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            random: LevelRandom::new(unique_seed()),
            redstone_cache: mc_bamboo_blocks::cache::RedstoneCache::new(
                mc_bamboo_blocks::cache::CacheConfig::default(),
            ),
            spawn_per_player_cap: true,
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
            patrol_spawner: mc_bamboo_blocks::entity::mob::patrol::PatrolSpawner::default(),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            sound_seed_generator: LevelRandom::new(unique_seed()),
            entities: Entities::default(),
            entity_tracking: EntityTracking::default(),
            redstone: RedstoneMemory::default(),
            tick: RegionTick::default(),
            view_distance,
            max_players,
            online_mode,
            enforce_secure_profile,
            service_keys,
            clock_millis: None,
            global: crate::global::GlobalView::default(),
            broadcast_console_to_ops: true,
            command_blocks: crate::pos_map::PosMap::default(),
            admin_changes: crate::admin::AdminChanges::default(),
            last_action_millis: BTreeMap::new(),
            peer_ips: BTreeMap::new(),
            profiler: Profiler::default(),
            partitioning: crate::region::partition::Partitioning::default(),
            dirty: dirty::Dirty::default(),
            sleep: crate::sleep::SleepStatus::default(),
            phantom_spawner: Default::default(),
        }
    }

    /// 未签名消息和过期判断用的现在。测试可以钉死。
    ///
    /// Now, for unsigned messages and expiry checks. Tests can pin it.
    pub(crate) fn now_millis(&self) -> i64 {
        self.clock_millis
            .unwrap_or_else(crate::secure_chat::now_millis)
    }

    /// 改用世界共享的实体 ID 分配器。只在建好、还没有实体时调用一次。
    ///
    /// Switch to the world's shared entity-id allocator. Called once, right
    /// after construction and before any entity exists.
    pub(crate) fn use_entity_ids(&mut self, ids: mc_bamboo_blocks::EntityIds) {
        debug_assert!(
            self.entities.is_empty(),
            "switching allocators under live entities"
        );
        self.entities = Entities::with_ids(ids);
    }

    pub(crate) fn advance_tick(&mut self) {
        self.tick.0 += 1;
        // 这一刻交付的区块已经进 region：照官方 `PrepareSpawnTask.tick`
        // 完成挂着的落位搜索，位置包排在下一刻的客户端确认之前。
        // Chunks delivered this tick are now in the region: like vanilla
        // `PrepareSpawnTask.tick`, finish pending landings so the position
        // packet precedes next tick's client confirmation.
        crate::spawn_landing::finish_pending(self);
    }

    /// 岛建立时把时钟定在当时的世界时间；计划刻队列的时钟一起对上。
    ///
    /// Set the island clock to the world time when the island is created, and
    /// line the scheduled-tick clock up with it.
    pub(crate) fn start_clock(&mut self, time: GameTime) {
        self.tick = RegionTick::starting_at(time);
        self.block_ticks.set_time(self.tick.game_time());
    }

    /// 岛合并前把被吸收方的全部绝对时刻平移 `delta = 吸收方.now − 被吸收方.now`
    /// （岛的实施计划 5.1）。平移之后再跑，等于这个岛的时钟本来就差 `delta` 时跑。
    /// 只在 tick 边界调用。
    ///
    /// 平移的七类：计划刻（含搁置表、活塞的 `lastTicked`、每区块最近排刻时刻和时钟）、
    /// 红石火把开关记录、方块实体表的睡眠日程和每个方块实体自己存的时刻（漏斗、
    /// 熔炉、酿造台、营火）、容器的 `recheck_at`、讲台待发的格子、玩家传送的
    /// `sent_at`，以及岛的时钟本身。红石缓存直接丢掉重建（结果和解释执行逐位相同，
    /// 只是暂时没有记录）；单元划分的统计不平移，合并时重建。
    ///
    /// Shift every absolute instant of the absorbed island by
    /// `delta = absorber.now − absorbed.now` before a merge, so that running
    /// afterwards equals running on a clock that was always `delta` apart.
    /// Tick boundary only. The redstone cache is dropped (replay is
    /// result-identical); partition statistics are rebuilt on merge instead.
    ///
    /// `delta` 为 0 时每一类都是恒等的（全是同加一个数），不逐条走：拆合时两边的时钟
    /// 通常相同，逐条走一遍等于每次合并都把被吸收方整个扫一遍。红石缓存照样换空。
    ///
    /// With `delta == 0` every class is the identity (all are additive), so
    /// nothing is walked: both clocks usually agree in a merge and walking
    /// would scan the whole absorbed island each time. The redstone cache is
    /// still replaced with an empty one.
    pub(crate) fn rebase_clock(&mut self, delta: i64) {
        if delta == 0 {
            self.redstone_cache =
                mc_bamboo_blocks::cache::RedstoneCache::new(*self.redstone_cache.config());
            return;
        }
        self.rebase_clock_parts(delta, ClockParts::ALL);
    }

    /// 同 [`Self::rebase_clock`]，按 `parts` 只平移其中几类。给改坏确认用。
    ///
    /// Like [`Self::rebase_clock`] but only the selected classes; for break-it checks.
    pub(crate) fn rebase_clock_parts(&mut self, delta: i64, parts: ClockParts) {
        if parts.clock {
            self.tick = self.tick.rebased(delta);
        }
        if parts.block_ticks {
            self.block_ticks.rebase(delta);
        }
        if parts.redstone {
            self.redstone.rebase(delta);
        }
        if parts.block_entities {
            self.block_entities.rebase(delta);
        }
        if parts.containers {
            for entry in self.containers.values_mut() {
                if let Some(at) = &mut entry.recheck_at {
                    *at = at.rebased(delta);
                }
            }
        }
        if parts.lecterns {
            self.lecterns.rebase(delta);
        }
        if parts.teleports {
            for player in self.players.values_mut() {
                player.teleport.sent_at = player.teleport.sent_at.rebased(delta);
            }
        }
        if parts.entities {
            self.entities.rebase(delta);
        }
        self.redstone_cache =
            mc_bamboo_blocks::cache::RedstoneCache::new(*self.redstone_cache.config());
    }

    /// 游戏层直接改了按位置索引的上层表：把那一格所在区块标脏。
    /// 宁多勿少，多存一次没有错，漏存一次才是错。
    /// Game-layer direct write to a positional upper-layer table: mark the chunk.
    pub(crate) fn touch(&mut self, pos: BlockPos) {
        self.dirty.mark(pos.chunk());
        // 同一个入口同时报告「这一格内容变了」：以后新写的容器路径只要标脏，
        // 关注这格的睡眠漏斗也会醒，不用再记一份清单。
        self.block_entities.notify_cell(pos);
        // 双箱两半合成一张容器时，内容变化要报告两格：漏斗可能只关注其中一半。
        if crate::container::kind_in(&self.chunks, pos)
            == Some(crate::container::ContainerKind::Chest)
            && let Ok(Some(chest)) = crate::container::chest_shape_with(&self.chunks, pos, true)
        {
            self.block_entities.notify_cell(chest.first);
            self.block_entities.notify_cell(chest.second);
        }
    }

    /// 测试专用：直接排一条方块事件。
    #[cfg(test)]
    pub(crate) fn push_block_event_for_test(&mut self, event: mc_bamboo_blocks::BlockEvent) {
        self.block_events.push(event);
    }

    /// 划分阶段取走本刻排过方块事件的区块，并报告还排着事件的区块。
    /// 两个入口都在 region 里，因为 `block_events` 表只对它可见。
    pub(crate) fn drain_block_event_activity(&mut self) -> Vec<ChunkPos> {
        self.block_events.drain_touched_chunks()
    }

    pub(crate) fn queued_block_event_chunks(&self) -> std::collections::BTreeSet<ChunkPos> {
        self.block_events.queued_chunks()
    }

    /// 划分开关关着时清掉第 1 步攒下的本刻活动清单，不让它们无限增长。
    pub(crate) fn clear_activity_touched(&mut self) {
        self.block_ticks.clear_touched_chunks();
        self.block_events.clear_touched_chunks();
    }

    /// 方块逻辑要的世界和计划刻队列。它们借用不同的字段，所以要一起拿。
    pub(crate) fn block_level(&mut self) -> (RegionLevel<'_>, &mut BlockTicks) {
        let (level, ticks, _) = self.split_level(false);
        (level, ticks)
    }

    /// 带红石缓存的方块逻辑入口：只有游戏 tick 的两段用得到。
    pub(crate) fn block_level_with_cache(
        &mut self,
    ) -> (
        RegionLevel<'_>,
        &mut BlockTicks,
        &mut mc_bamboo_blocks::cache::RedstoneCache,
    ) {
        self.split_level(false)
    }

    /// 实体逻辑读的那份规则：已发布的方块快照加 Bamboo 自己的挤压上限配置。
    /// 只改一个字段，不重算整份规则（这条路径每刻要跑很多次）。
    pub(crate) fn entity_game_rules(&self) -> mc_bamboo_blocks::BlockGameRules {
        let mut rules = self.global.block_rules;
        rules.max_entity_collisions = self.max_entity_collisions;
        rules.mob_throttle_distance = self.mob_throttle_distance;
        rules.mob_sleep_distance = self.mob_sleep_distance;
        rules.mob_wakeups_scale = self.mob_wakeups_scale;
        rules.mob_pathfinding_phase = self.mob_pathfinding_phase;
        rules.mob_pathfinding_budget = self.mob_pathfinding_budget;
        rules.mob_pathfinding_fail_cooldown = self.mob_pathfinding_fail_cooldown;
        rules.villager_lobotomize = self.villager_lobotomize;
        rules.villager_lobotomize_interval = self.villager_lobotomize_interval;
        rules.villager_lobotomize_wait_trade_lock = self.villager_lobotomize_wait_trade_lock;
        rules.villager_sleep_brain_interval = self.villager_sleep_brain_interval;
        rules
    }

    /// 精确驱动的实体循环用的世界：额外借出连接、音效种子和规则，生物打玩家时
    /// 能在自己的 tick 里当场走玩家受伤的正式入口（[`RegionLevel::player_side`]）。
    pub(crate) fn entity_level(&mut self) -> (RegionLevel<'_>, &mut BlockTicks) {
        let (level, ticks, _) = self.split_level(true);
        (level, ticks)
    }

    fn split_level(
        &mut self,
        with_players: bool,
    ) -> (
        RegionLevel<'_>,
        &mut BlockTicks,
        &mut mc_bamboo_blocks::cache::RedstoneCache,
    ) {
        let entity_rules = self.entity_game_rules();
        let player_side = with_players.then(|| crate::interaction::level::PlayerSide {
            links: &mut self.links,
            sound_seeds: &self.sound_seed_generator,
            rules: &self.global.rules,
            pending_triggers: &mut self.pending_triggers,
        });
        let level = RegionLevel {
            chunks: &mut self.chunks,
            poi: &mut self.poi,
            changes: &mut self.block_changes,
            block_items: &mut self.block_items,
            break_tool: mc_bamboo_protocol::item::ItemStack::empty(),
            players: &mut self.players,
            random: &self.random,
            redstone: &mut self.redstone,
            block_entities: &mut self.block_entities,
            block_events: &mut self.block_events,
            containers: &mut self.containers,
            entities: &mut self.entities,
            loot_sequences: &self.loot_sequences,
            block_rules: entity_rules,
            block_weather: self.global.block_weather,
            sky_darken: self.global.sky_darken,
            villager_activity: self.global.villager_activity,
            baby_villager_activity: self.global.baby_villager_activity,
            clock_ticks: self.global.clock.total_ticks,
            scoreboard: &self.global.scoreboard,
            world_seed: self.global.seed,
            world_gen: &self.global.world_gen,
            sea_level: if self.global.world_gen.level_type.is_flat() {
                -63
            } else {
                63
            },
            difficulty: self.global.difficulty,
            world_border: self.global.world_border.clone(),
            command_blocks: &mut self.command_blocks,
            tick_levels: &self.tick_levels,
            dirty: &mut self.dirty,
            chunk_profiler: self
                .profiler
                .chunk_timing_enabled()
                .then_some(&self.profiler),
            partition_profiler: self.profiler.partition_enabled().then_some(&self.profiler),
            partitions: &self.partitioning.partitions,
            player_side,
        };
        (level, &mut self.block_ticks, &mut self.redstone_cache)
    }

    /// 进服开头：`login`、`change_difficulty`、`player_abilities`、权限事件、命令树、
    /// `player_position`。官方 `PlayerList.placeNewPlayer` 把难度包放在 login 后面，
    /// **不发** `set_experience`：那一包由 `ServerPlayer.doTick` 在进服这一刻
    /// （`lastSentExp` 仍是构造时的 -99999999）补发。
    ///
    /// The start of join: `login`, `change_difficulty`, `player_abilities`, the
    /// permission event, the command tree, then `player_position`. Official
    /// `placeNewPlayer` puts the difficulty packet right after `login` and does
    /// **not** send `set_experience`: `ServerPlayer.doTick` sends it on the join
    /// tick because `lastSentExp` is still -99999999.
    pub(crate) fn add_player(&mut self, request: JoinRequest, settings: JoinSettings<'_>) {
        let JoinSettings {
            game_mode,
            difficulty_frames,
            spawn,
            hashed_seed: state_seed_hash,
            permission_level,
            is_op,
            experience,
            new_player,
        } = settings;
        // 属性已经进了 Tab 名单，这里用不到。
        let JoinRequest {
            id,
            uuid,
            name,
            properties: _,
            client,
            permission_level: _,
            is_op: _,
            peer_ip,
            online_at_login: _,
            player_data: _,
            player_advancements: _,
            sink,
            inbox,
        } = request;
        self.links.add(id, sink);
        self.peer_ips.insert(id, peer_ip);
        let (is_flat, sea_level) = crate::spawn_landing::search_from(self).level_flags();

        let login = Login {
            entity_id: id.0,
            dimensions: &[DIMENSION],
            max_players: self.max_players,
            view_distance: self.view_distance,
            simulation_distance: self.view_distance,
            dimension_type: dimension_type::OVERWORLD,
            dimension: DIMENSION,
            hashed_seed: state_seed_hash,
            game_mode,
            is_flat,
            sea_level,
            online_mode: self.online_mode,
            enforces_secure_chat: crate::secure_chat::enforces(self),
            // 官方 `PlayerList.placeNewPlayer`：规则开着就不显示死亡界面。
            // Vanilla `PlayerList.placeNewPlayer`: with the rule on, the
            // client does not show the death screen.
            show_death_screen: !self
                .global
                .rules
                .bool(mc_bamboo_data::gamerules::IMMEDIATE_RESPAWN),
        };
        self.links.send(id, &login);
        // 官方在 login 后立刻同步难度；帧已经由 `Global` 编码好，广播时也用它。
        self.links.send_frames(id, difficulty_frames);
        self.links.send(id, &ABILITIES);
        // 官方在能力包之后、传送之前发权限事件和命令树。
        //
        // Official sends the permission event and the command tree after abilities and before the teleport.
        crate::command::send_session(&mut self.links, id, permission_level, id.0);

        // `PrepareSpawnTask`：区块就绪时直接算精确落点并发 `player_position`；
        // 没就绪只放一个临时位置，等票加载完由 `finish_pending` 补发位置包。
        // `PrepareSpawnTask`: compute the exact landing and send
        // `player_position` when chunks are ready; otherwise use a temporary
        // position and let `finish_pending` complete it after ticket delivery.
        let (position, ready) = if new_player {
            match crate::spawn_landing::landing(self, None, None, false) {
                Ok((landing, _)) => (landing.position, true),
                Err(crate::spawn_landing::NotReady) => (temporary_position(self), false),
            }
        } else {
            // `SavedPosition.position()`：方块中心 +0.5，y 用存档里的精确值。
            // `SavedPosition.position()`: block centre +0.5 with the saved y.
            (
                Vec3::new(
                    f64::from(spawn.pos.x) + 0.5,
                    f64::from(spawn.pos.y),
                    f64::from(spawn.pos.z) + 0.5,
                ),
                true,
            )
        };
        let mut player = Player::new(id, uuid, name, game_mode, client, position);
        player.experience = experience;
        player.permission_level = permission_level;
        player.is_op = is_op;
        player.inbox = inbox;
        player.yaw = spawn.yaw;
        player.pitch = spawn.pitch;
        if ready {
            movement::teleport(
                &mut player,
                &mut self.links,
                &mut self.player_grid,
                self.tick,
                position,
            );
        } else {
            player.spawn_pending = Some(crate::spawn_landing::PendingLanding::join());
        }
        self.insert_player(player);
    }

    /// 进服第 6 步：只有服务端会告诉客户端自己的皮肤外层、主手和触及距离。
    pub(crate) fn send_own_entity_data(&mut self, id: PlayerId) {
        let Some(player) = self.players.get(&id) else {
            return;
        };
        let items = [
            EntityData::MainHand(player.client.main_hand),
            EntityData::ModelCustomisation(player.client.model_customisation),
        ];
        self.links.send(
            id,
            &SetEntityData {
                entity_id: id.0,
                items: &items,
            },
        );
        self.links.send(
            id,
            &UpdateAttributes {
                entity_id: id.0,
                attributes: &CREATIVE_ATTRIBUTES,
            },
        );
    }

    /// 官方 `PlayerList.sendLevelInfo`：`initialize_border` 开头，然后 `set_time`、
    /// `set_default_spawn_position`、下雨时的三个 `game_event`，最后 `game_event` 13
    /// 和 `set_chunk_cache_center`。`world_spawn` 是 `LevelData` 里的世界出生点，
    /// 只用于 `set_default_spawn_position`；区块中心取 `player.chunk_pos()`（玩家位置）。
    ///
    /// Official `PlayerList.sendLevelInfo`: `initialize_border` first, then `set_time`,
    /// `set_default_spawn_position`, the three rain `game_event`s, and finally
    /// `game_event` 13 and `set_chunk_cache_center`. `world_spawn` is the world spawn
    /// from `LevelData`, used only for `set_default_spawn_position`; the chunk cache
    /// center comes from `player.chunk_pos()` (the player position).
    ///
    /// 时间、时钟、出生点、天气和边界都读岛上的全局快照。
    /// Time, clock, spawn, weather and the border come from the island's global snapshot.
    pub(crate) fn start_loading_chunks(&mut self, id: PlayerId) {
        let (time, world_spawn, weather) =
            (self.global.time, self.global.spawn, self.global.weather);
        let clocks = self.global.clocks();
        let border = &self.global.world_border;
        self.links.send(
            id,
            &InitializeBorder {
                center_x: border.center_x(),
                center_z: border.center_z(),
                old_size: border.size(),
                new_size: border.lerp_target(),
                lerp_time: border.lerp_time(),
                absolute_max_size: border.absolute_max_size(),
                warning_blocks: border.warning_blocks(),
                warning_time: border.warning_time(),
            },
        );
        self.send_time(id, time, &clocks);
        self.links.send(
            id,
            &SetDefaultSpawnPosition {
                dimension: DIMENSION,
                pos: world_spawn.pos,
                yaw: world_spawn.yaw,
                pitch: world_spawn.pitch,
            },
        );
        // 官方 `PlayerList.sendLevelInfo` 按 `Level.isRaining()`（渐变雨强 > 0.2）判断，
        // 两个强度包带的是当前强度，不是固定的 0 或 1。
        if weather.is_raining() {
            self.links.send(
                id,
                &GameEvent {
                    event: GameEvent::START_RAINING,
                    param: 0.0,
                },
            );
            self.links.send(
                id,
                &GameEvent {
                    event: GameEvent::RAIN_LEVEL_CHANGE,
                    param: weather.rain_level,
                },
            );
            self.links.send(
                id,
                &GameEvent {
                    event: GameEvent::THUNDER_LEVEL_CHANGE,
                    param: weather.rain_level * weather.thunder_level,
                },
            );
        }
        // 客户端收不到它就永远停在加载画面。
        self.links.send(
            id,
            &GameEvent {
                event: GameEvent::LEVEL_CHUNKS_LOAD_START,
                param: 0.0,
            },
        );
        let now = self.profiler.now();
        if let Some(player) = self.players.get_mut(&id) {
            player.chunks.sender.set_now(now);
            chunk_tracking::update_player_view(player, &mut self.links, self.view_distance);
        }
    }

    /// 把主世界时钟的当前状态发给所有人。`/time` 改了主世界之后调用。
    ///
    /// Broadcasts the overworld clock. Called after `/time` changes it.
    pub(crate) fn broadcast_time(&mut self, time: GameTime, clock: Clock, advance_time: bool) {
        self.broadcast_clock(time, world_clock::OVERWORLD, clock, advance_time);
    }

    /// 把指定注册表 ID 的时钟发给所有人：官方 `ServerClockManager.modifyClock`
    /// 只带被改的那一个时钟。
    ///
    /// Broadcasts one clock by registry id: official `ServerClockManager.modifyClock`
    /// sends only the clock that changed.
    pub(crate) fn broadcast_clock(
        &mut self,
        time: GameTime,
        clock_id: i32,
        clock: Clock,
        advance_time: bool,
    ) {
        let state = clock_state(clock_id, clock, advance_time);
        self.links.broadcast(&crate::links::encode_frames(&SetTime {
            game_time: time.0,
            clocks: &[state],
        }));
    }

    /// 官方 `ServerClockManager.createFullSyncPacket`：进服和 `advance_time` 改变时
    /// 发全部时钟。
    ///
    /// Official `ServerClockManager.createFullSyncPacket`: send every clock on
    /// join and whenever `advance_time` changes.
    pub(crate) fn broadcast_all_clocks(
        &mut self,
        time: GameTime,
        clocks: &[(&'static str, Clock)],
        advance_time: bool,
    ) {
        let states: Vec<_> = clocks
            .iter()
            .map(|(name, clock)| {
                let id = match *name {
                    "minecraft:the_end" => world_clock::THE_END,
                    _ => world_clock::OVERWORLD,
                };
                clock_state(id, *clock, advance_time)
            })
            .collect();
        self.links.broadcast(&crate::links::encode_frames(&SetTime {
            game_time: time.0,
            clocks: &states,
        }));
    }

    /// 官方 `forceGameTimeSynchronization`：每 20 刻只发游戏时间，时钟表为空。
    ///
    /// Official `forceGameTimeSynchronization`: every 20 ticks, send only the
    /// game time with an empty clock map.
    pub(crate) fn broadcast_game_time(&mut self, time: GameTime) {
        self.links.broadcast(&crate::links::encode_frames(&SetTime {
            game_time: time.0,
            clocks: &[],
        }));
    }

    fn send_time(&mut self, id: PlayerId, time: GameTime, clocks: &[(&'static str, Clock)]) {
        let advance_time = self.global.time_advances();
        let states: Vec<_> = clocks
            .iter()
            .map(|(name, clock)| {
                let id = match *name {
                    "minecraft:the_end" => world_clock::THE_END,
                    _ => world_clock::OVERWORLD,
                };
                clock_state(id, *clock, advance_time)
            })
            .collect();
        self.links.send(
            id,
            &SetTime {
                game_time: time.0,
                clocks: &states,
            },
        );
    }

    /// 连接层报来的对端 IP。
    pub(crate) fn peer_ip(&self, id: PlayerId) -> Option<&str> {
        self.peer_ips.get(&id).map(String::as_str)
    }

    /// 登录包里的玩家上限。
    ///
    /// The player cap written into the login packet.
    pub(crate) fn max_players(&self) -> i32 {
        self.max_players
    }
}

/// 官方 `ClockInstance.packNetworkState` 的一项。
///
/// One entry of official `ClockInstance.packNetworkState`.
fn clock_state(
    clock_id: i32,
    clock: Clock,
    advance_time: bool,
) -> mc_bamboo_protocol::packet::play::ClockState {
    mc_bamboo_protocol::packet::play::ClockState {
        clock: clock_id,
        ticks: clock.total_ticks,
        partial: clock.partial_tick,
        rate: clock.network_rate(advance_time),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_clock_measures_elapsed_ticks() {
        let mut region = Region::new(2, 20, false);
        let start = region.tick;

        for _ in 0..21 {
            region.advance_tick();
        }

        assert_eq!(region.tick.since(start), 21);
        assert_eq!(start.since(region.tick), 0);
    }

    #[test]
    fn a_scheduled_tick_runs_on_its_own_tick() {
        let mut region = Region::new(2, 20, false);
        let start = region.tick;
        let at = start.plus(5);

        for _ in 0..4 {
            region.advance_tick();
            assert!(!region.tick.reached(at));
        }
        region.advance_tick();
        assert!(region.tick.reached(at));
    }

    /// 生物的平移脚本：几头牛随机在「现在」记下受伤、卡住检查和重算时刻，
    /// 每刻观测距这三个时刻过了几刻。`shift` 给出时第 `mid` 刻后按 `parts`
    /// 平移岛的时钟。
    ///
    /// Mob rebase script: a few cows randomly stamp their damage, stuck-check
    /// and recompute times at "now"; each tick observes how long ago each was.
    fn mob_script(seed: u64, base: i64, shift: Option<(i64, i64, ClockParts)>) -> Vec<[i64; 18]> {
        use mc_bamboo_blocks::entity::Entity;
        use mc_bamboo_core::Vec3;

        let mut state = seed | 1;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut region = Region::new(2, 20, false);
        region.start_clock(crate::global::GameTime(base));
        let cows: Vec<_> = (0..3)
            .map(|i| {
                let mut cow = Entity::cow(Vec3::new(f64::from(i) * 4.0, -60.0, 0.0));
                let now = region.tick.game_time();
                let mob = cow.kind.mob_mut().expect("cow is a mob");
                mob.last_damage_stamp = now;
                mob.navigation.last_timeout_check = now;
                mob.navigation.time_last_recompute = now;
                // 降频的三个时刻也是岛时钟的绝对刻号，拆岛平移要跟着走。
                mob.throttle.last_decision = now;
                mob.throttle.last_active = now;
                mob.throttle.awake_until = now;
                region.entities.spawn(cow)
            })
            .collect();
        let mut out = Vec::new();
        for step in 1..=150_i64 {
            region.advance_tick();
            let now = region.tick.game_time();
            let mut row = [0; 18];
            for (i, &uid) in cows.iter().enumerate() {
                let roll = next();
                let seen = region.entities.with_mut(uid, |cow| {
                    let mob = cow.kind.mob_mut().expect("cow is a mob");
                    match roll % 7 {
                        0 => mob.last_damage_stamp = now,
                        1 => mob.navigation.last_timeout_check = now,
                        2 => mob.navigation.time_last_recompute = now,
                        _ => {}
                    }
                    [
                        now - mob.last_damage_stamp,
                        now - mob.navigation.last_timeout_check,
                        now - mob.navigation.time_last_recompute,
                        now - mob.throttle.last_decision,
                        now - mob.throttle.last_active,
                        now - mob.throttle.awake_until,
                    ]
                });
                row[i * 6..i * 6 + 6].copy_from_slice(&seen.expect("cow is stored"));
            }
            out.push(row);
            if let Some((mid, delta, parts)) = shift
                && step == mid
            {
                region.rebase_clock_parts(delta, parts);
            }
        }
        out
    }

    /// 岛合并平移时钟后，生物记着的绝对时刻跟着平移；漏掉生物会被发现。
    ///
    /// After a clock rebase, mob time stamps move along; leaving mobs out is caught.
    #[test]
    fn mob_time_stamps_follow_a_clock_rebase() {
        let mut caught = false;
        for seed in 1..=8_u64 {
            let base = 1_000 + (seed as i64) * 37;
            let delta = 5_000 + (seed as i64) * 11;
            let apart = mob_script(seed, base + delta, None);
            assert_eq!(
                mob_script(seed, base, Some((70, delta, ClockParts::ALL))),
                apart,
                "seed {seed}"
            );
            let broken = ClockParts {
                entities: false,
                ..ClockParts::ALL
            };
            caught |= mob_script(seed, base, Some((70, delta, broken))) != apart;
        }
        assert!(caught, "leaving mobs out of the rebase went unnoticed");
    }

    #[test]
    fn creative_attributes_match_the_official_bytes() {
        let mut frames = Vec::new();
        mc_bamboo_protocol::packet::ClientboundPacket::write_body(
            &UpdateAttributes {
                entity_id: 1,
                attributes: &CREATIVE_ATTRIBUTES,
            },
            &mut frames,
        );

        let mut expected = vec![0x01, 0x02, 0x08];
        expected.extend_from_slice(&4.5f64.to_be_bytes());
        expected.extend_from_slice(&[0x01, 0x23]);
        expected.extend_from_slice(b"minecraft:creative_mode_block_range");
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.extend_from_slice(&[0x00, 0x0d]);
        expected.extend_from_slice(&3.0f64.to_be_bytes());
        expected.extend_from_slice(&[0x01, 0x24]);
        expected.extend_from_slice(b"minecraft:creative_mode_entity_range");
        expected.extend_from_slice(&2.0f64.to_be_bytes());
        expected.push(0x00);
        assert_eq!(expected.len(), 113);
        assert_eq!(frames, expected);
    }
}
