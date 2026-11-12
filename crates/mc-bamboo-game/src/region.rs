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
    BlockEntities, BlockEvents, BlockGameRules, BlockTicks, LevelRandom, RedstoneMemory,
    WorldBorder, entity::Entities,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::registries::{attribute, dimension_type, world_clock};
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_protocol::packet::play::{
    AttributeModifier, AttributeOperation, AttributeSnapshot, EntityData, GameEvent, GameMode,
    InitializeBorder, Login, PlayerAbilities, SetDefaultSpawnPosition, SetEntityData, SetTime,
    UpdateAttributes,
};
use mc_bamboo_world::{flat, light::LightChanges, store::ChunkStore};

use crate::{
    chunk_encoding::{ChunkCache, encode_template},
    chunk_tracking,
    container::ContainerEntry,
    entity_tracking::EntityTracking,
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
    world_state::{RespawnData, Weather},
};

pub mod chunk_content;
mod column;
pub(crate) use column::take_pristine_cost;
#[cfg(test)]
mod column_tests;
pub(crate) mod parking;

// 区块平面和多 region 是后面的步骤，现在还没有调用方。
#[allow(unused_imports)]
pub use column::{NotReady, RegionColumn, RegionColumnParts, RegionColumnView};

pub(crate) const DIMENSION: &str = "minecraft:overworld";
/// 默认世界出生点在 `world_state`。进服位置只取它的水平坐标，y 仍按超平坦地表算：
/// 官方还会在 `respawn_radius` 里随机找落脚点，这里没有死亡重生也不做随机。
///
/// The default world spawn lives in [`WorldState`]. Join uses its horizontal position and
/// keeps the superflat surface as y; official also searches for a spot within
/// `respawn_radius`, which this server does not implement.
fn spawn_position(spawn: RespawnData) -> Vec3 {
    Vec3::new(
        f64::from(spawn.pos.x) + 0.5,
        f64::from(flat::surface_y()),
        f64::from(spawn.pos.z) + 0.5,
    )
}

const ABILITIES: PlayerAbilities = PlayerAbilities {
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

/// 调度用的时钟。和 [`GameTime`] 同步增长，但不是一回事，不能互相比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RegionTick(u64);

impl RegionTick {
    /// 从 `earlier` 到现在过了多少 tick。
    pub(crate) fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// `later` 个 tick 之后。
    pub(crate) fn plus(self, later: u64) -> Self {
        Self(self.0.saturating_add(later))
    }

    /// 是否已经到了 `at` 这一刻。这一 tick 开始时增长，所以到期的刻在自己这一 tick 跑。
    pub(crate) fn reached(self, at: Self) -> bool {
        self.0 >= at.0
    }

    /// `at` 距现在还有多少刻，已经到期的算 0。搬区块时把绝对时刻换算成相对值用。
    pub(crate) fn remaining(self, at: Self) -> u64 {
        at.0.saturating_sub(self.0)
    }
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
    pub(crate) chunks: ChunkStore,
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
    pub(crate) containers: BTreeMap<BlockPos, ContainerEntry>,
    /// 方块实体从物品上带来的组件，官方 `BlockEntity.collectComponents` 的结果，
    /// 见 [`crate::block_items`]。
    pub(crate) block_items: BTreeMap<BlockPos, ComponentPatch>,
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
    /// 方块逻辑读的游戏规则快照。权威副本在 `Global`，改规则时当场写过来。
    ///
    /// The game-rule snapshot block logic reads. The authoritative copy is on `Global`
    /// and is copied here as soon as a rule changes.
    pub(crate) block_rules: BlockGameRules,
    /// 方块逻辑读的天气快照。权威副本在 `Global`，每 tick 推进后写过来。
    ///
    /// The weather snapshot block logic reads. The authoritative copy is on `Global`
    /// and is copied here after each tick's weather advance.
    pub(crate) block_weather: mc_bamboo_blocks::BlockWeather,
    /// 这一维度的世界边界快照。权威副本在 `Global`，每刻推进渐变后、以及命令改完
    /// 边界后写过来。方块逻辑、移动校验和实体碰撞都读这一份。
    ///
    /// This dimension's world-border snapshot. The authoritative copy is on `Global`
    /// and is copied here after each tick's lerp step and after a border command.
    /// Block logic, movement checks and entity collisions all read this copy.
    pub(crate) world_border: WorldBorder,
    /// `GameRules.SEND_COMMAND_FEEDBACK`。
    pub(crate) send_command_feedback: bool,
    /// `GameRules.LOG_ADMIN_COMMANDS`。
    pub(crate) log_admin_commands: bool,
    /// `GameRules.COMMAND_BLOCKS_WORK`。
    pub(crate) command_blocks_work: bool,
    /// 命令方块里保存的命令。不执行，也不存档。
    ///
    /// Commands stored in command blocks. They are not executed and not saved.
    pub(crate) command_blocks: BTreeMap<BlockPos, crate::command_block::CommandBlockData>,
    /// 这一 tick 里改过的管理名单，tick 末尾统一发布并写盘。
    pub(crate) admin_changes: crate::admin::AdminChanges,
    /// 每名玩家最近一次动作的时刻，单位毫秒。官方 `ServerPlayer.lastActionTime`。
    pub(crate) last_action_millis: BTreeMap<PlayerId, i64>,
    /// 每名玩家的对端 IP 文本（不含端口），供 `ban-ip` 和登录复查用。
    pub(crate) peer_ips: BTreeMap<PlayerId, String>,
    /// 这个 region 每刻的耗时和数量指标。世界线程独占，一直开着。
    pub(crate) profiler: Profiler,
}

/// 每次启动不同的种子，官方用 `RandomSupport.generateUniqueSeed`，这里用启动时刻的纳秒数。
fn unique_seed() -> i64 {
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
/// 以及按权威 OP 名单查出的权限。
///
/// What the world thread settled for a joining player: the game mode, the difficulty frames,
/// spawn and seed hash from the world state, and the permissions from the authoritative op list.
pub(crate) struct JoinSettings<'a> {
    pub(crate) game_mode: GameMode,
    pub(crate) difficulty_frames: &'a [u8],
    pub(crate) spawn: RespawnData,
    pub(crate) hashed_seed: i64,
    pub(crate) permission_level: u8,
    pub(crate) is_op: bool,
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
        let rules = crate::gamerules::GameRuleSet::vanilla();
        Self {
            players: BTreeMap::new(),
            player_grid: PlayerGrid::default(),
            links: Links::default(),
            tick_levels: TickLevels::new(chunks.is_loaded_mode()),
            chunks,
            template_body,
            chunk_cache: ChunkCache::default(),
            block_changes: BlockChanges::default(),
            light_changes: BTreeMap::new(),
            block_ticks: BlockTicks::new(),
            containers: BTreeMap::new(),
            block_items: BTreeMap::new(),
            menus: BTreeMap::new(),
            lecterns: crate::lectern::Tables::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            random: LevelRandom::new(unique_seed()),
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
            block_rules: rules.block_snapshot(),
            block_weather: mc_bamboo_blocks::BlockWeather::CLEAR,
            world_border: WorldBorder::default(),
            send_command_feedback: rules.bool(mc_bamboo_data::gamerules::SEND_COMMAND_FEEDBACK),
            log_admin_commands: rules.bool(mc_bamboo_data::gamerules::LOG_ADMIN_COMMANDS),
            command_blocks_work: rules.bool(mc_bamboo_data::gamerules::COMMAND_BLOCKS_WORK),
            command_blocks: BTreeMap::new(),
            admin_changes: crate::admin::AdminChanges::default(),
            last_action_millis: BTreeMap::new(),
            peer_ips: BTreeMap::new(),
            profiler: Profiler::default(),
        }
    }

    /// 未签名消息和过期判断用的现在。测试可以钉死。
    ///
    /// Now, for unsigned messages and expiry checks. Tests can pin it.
    pub(crate) fn now_millis(&self) -> i64 {
        self.clock_millis
            .unwrap_or_else(crate::secure_chat::now_millis)
    }

    pub(crate) fn advance_tick(&mut self) {
        self.tick.0 += 1;
    }

    /// 方块逻辑要的世界和计划刻队列。它们借用不同的字段，所以要一起拿。
    pub(crate) fn block_level(&mut self) -> (RegionLevel<'_>, &mut BlockTicks) {
        let level = RegionLevel {
            chunks: &mut self.chunks,
            changes: &mut self.block_changes,
            block_items: &mut self.block_items,
            players: &mut self.players,
            random: &self.random,
            redstone: &mut self.redstone,
            block_entities: &mut self.block_entities,
            block_events: &mut self.block_events,
            containers: &mut self.containers,
            entities: &mut self.entities,
            loot_sequences: &self.loot_sequences,
            block_rules: self.block_rules,
            block_weather: self.block_weather,
            world_border: self.world_border.clone(),
            command_blocks: &mut self.command_blocks,
            tick_levels: &self.tick_levels,
        };
        (level, &mut self.block_ticks)
    }

    /// 进服开头：`login`、`change_difficulty`、`player_abilities`、权限事件、命令树、
    /// `player_position`。官方 `PlayerList.placeNewPlayer` 把难度包放在 login 后面。
    ///
    /// The start of join: `login`, `change_difficulty`, `player_abilities`, the permission
    /// event, the command tree, then `player_position`. Official `placeNewPlayer` puts
    /// the difficulty packet right after `login`.
    pub(crate) fn add_player(&mut self, request: JoinRequest, settings: JoinSettings<'_>) {
        let JoinSettings {
            game_mode,
            difficulty_frames,
            spawn,
            hashed_seed: state_seed_hash,
            permission_level,
            is_op,
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
            sink,
        } = request;
        self.links.add(id, sink);
        self.peer_ips.insert(id, peer_ip);

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
            is_flat: true,
            sea_level: flat::SEA_LEVEL,
            online_mode: self.online_mode,
            enforces_secure_chat: crate::secure_chat::enforces(self),
        };
        self.links.send(id, &login);
        // 官方在 login 后立刻同步难度；帧已经由 `Global` 编码好，广播时也用它。
        self.links.send_frames(id, difficulty_frames);
        self.links.send(id, &ABILITIES);
        // 官方在能力包之后、传送之前发权限事件和命令树。
        //
        // Official sends the permission event and the command tree after abilities and before the teleport.
        crate::command::send_session(&mut self.links, id, permission_level, id.0);

        let position = spawn_position(spawn);
        let mut player = Player::new(id, uuid, name, game_mode, client, position);
        player.permission_level = permission_level;
        player.is_op = is_op;
        player.yaw = spawn.yaw;
        player.pitch = spawn.pitch;
        movement::teleport(
            &mut player,
            &mut self.links,
            &mut self.player_grid,
            self.tick,
            position,
        );
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
    /// 和 `set_chunk_cache_center`。
    ///
    /// Official `PlayerList.sendLevelInfo`: `initialize_border` first, then `set_time`,
    /// `set_default_spawn_position`, the three rain `game_event`s, and finally
    /// `game_event` 13 and `set_chunk_cache_center`.
    pub(crate) fn start_loading_chunks(
        &mut self,
        id: PlayerId,
        time: GameTime,
        clock: Clock,
        spawn: RespawnData,
        weather: Weather,
        border: &WorldBorder,
    ) {
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
        self.send_time(id, time, clock);
        self.links.send(
            id,
            &SetDefaultSpawnPosition {
                dimension: DIMENSION,
                pos: spawn.pos,
                yaw: spawn.yaw,
                pitch: spawn.pitch,
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
    pub(crate) fn broadcast_time(&mut self, time: GameTime, clock: Clock) {
        self.broadcast_clock(time, world_clock::OVERWORLD, clock);
    }

    /// 把指定注册表 ID 的时钟发给所有人。末地时钟不进登录时的那一包，改了才发。
    ///
    /// Broadcasts one clock by registry id. The End clock is absent from the login packet
    /// and is sent only after a command changes it.
    pub(crate) fn broadcast_clock(&mut self, time: GameTime, clock_id: i32, clock: Clock) {
        let packet = SetTime {
            game_time: time.0,
            clock: clock_id,
            clock_ticks: clock.total_ticks,
            clock_partial: clock.partial_tick,
            clock_rate: clock.network_rate(),
        };
        self.links.broadcast(&crate::links::encode_frames(&packet));
    }

    fn send_time(&mut self, id: PlayerId, time: GameTime, clock: Clock) {
        self.links.send(id, &time_packet(time, clock));
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

/// 主世界时钟的 `set_time` 包。登录时只带这一项。
///
/// The overworld `set_time` packet. Login sends only this clock.
fn time_packet(time: GameTime, clock: Clock) -> SetTime {
    SetTime {
        game_time: time.0,
        clock: world_clock::OVERWORLD,
        clock_ticks: clock.total_ticks,
        clock_partial: clock.partial_tick,
        clock_rate: clock.network_rate(),
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
