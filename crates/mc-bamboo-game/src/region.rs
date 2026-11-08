//! 世界的一块区域：区块、里面的玩家，以及这些玩家的出站链路。
//!
//! 这里的方法拿不到 [`Global`](crate::global::Global)，全服的事情由
//! [`World`](crate::World) 串起来。现在只有一个 `Region`，将来有多个时，
//! 玩家和它的链路一起搬家。

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_blocks::{BlockEntities, BlockEvents, BlockTicks, LevelRandom, RedstoneMemory};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::registries::{attribute, dimension_type, world_clock};
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_protocol::packet::play::{
    AttributeModifier, AttributeOperation, AttributeSnapshot, EntityData, GameMode,
    LevelChunksLoadStart, Login, PlayerAbilities, SetEntityData, SetTime, UpdateAttributes,
};
use mc_bamboo_world::{flat, light::LightChanges, store::ChunkStore};

use crate::{
    chunk_encoding::{ChunkCache, encode_template},
    chunk_tracking,
    container::ContainerEntry,
    global::GameTime,
    inbound::{JoinRequest, PlayerId},
    interaction::{BlockChanges, level::RegionLevel},
    links::Links,
    menu::Menu,
    movement,
    player::Player,
    player_grid::PlayerGrid,
};

const DIMENSION: &str = "minecraft:overworld";
/// 出生点在 (0, 0) 区块的中心。
const SPAWN_X: f64 = 8.5;
const SPAWN_Z: f64 = 8.5;
/// 正午。世界时钟的流速是 0，所以时间一直停在这里。
const NOON_TICKS: i64 = 6000;

const ABILITIES: PlayerAbilities = PlayerAbilities {
    invulnerable: true,
    flying: false,
    can_fly: true,
    instant_build: true,
    flying_speed: 0.05,
    walking_speed: 0.1,
};

/// 创造模式的触及距离加成完全由服务端下发，客户端代码里没有这两个数。
const CREATIVE_ATTRIBUTES: [AttributeSnapshot<'static>; 2] = [
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
    /// 活塞这类会动的方块实体。
    block_entities: BlockEntities,
    /// 等这一刻末尾处理的方块事件，官方的 `Level.blockEvents`。
    block_events: BlockEvents,
    /// 方块逻辑的随机源，官方的 `Level.random` 和 `randValue`。
    pub(crate) random: LevelRandom,
    /// 音效包的随机种子，官方的 `Level.soundSeedGenerator`。和世界随机分开，只影响客户端挑哪个变体。
    pub(crate) sound_seed_generator: LevelRandom,
    /// 比较器的输出、书架最近动的格子和红石火把最近的开关。
    redstone: RedstoneMemory,
    pub(crate) tick: RegionTick,
    /// 服务端视距，已经 clamp 到合法范围。
    pub(crate) view_distance: i32,
    max_players: i32,
    /// 写进 `login` 包的那一位，官方取的是 `MinecraftServer.usesAuthentication()`。
    online_mode: bool,
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

    pub(crate) fn new(view_distance: i32, max_players: i32, online_mode: bool) -> Self {
        let chunks = ChunkStore::flat();
        let template_body = encode_template(&chunks);
        Self {
            players: BTreeMap::new(),
            player_grid: PlayerGrid::default(),
            links: Links::default(),
            chunks,
            template_body,
            chunk_cache: ChunkCache::default(),
            block_changes: BlockChanges::default(),
            light_changes: BTreeMap::new(),
            block_ticks: BlockTicks::new(),
            containers: BTreeMap::new(),
            block_items: BTreeMap::new(),
            menus: BTreeMap::new(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            random: LevelRandom::new(unique_seed()),
            sound_seed_generator: LevelRandom::new(unique_seed()),
            redstone: RedstoneMemory::default(),
            tick: RegionTick::default(),
            view_distance,
            max_players,
            online_mode,
        }
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
            players: &self.players,
            random: &self.random,
            redstone: &mut self.redstone,
            block_entities: &mut self.block_entities,
            block_events: &mut self.block_events,
            containers: &mut self.containers,
        };
        (level, &mut self.block_ticks)
    }

    /// 进服第 1 到 3 步：`login`、`player_abilities`、`player_position`。
    pub(crate) fn add_player(&mut self, request: JoinRequest, game_mode: GameMode) {
        // 属性已经进了 Tab 名单，这里用不到。
        let JoinRequest {
            id,
            uuid,
            name,
            properties: _,
            client,
            sink,
        } = request;
        self.links.add(id, sink);

        let login = Login {
            entity_id: id.0,
            dimensions: &[DIMENSION],
            max_players: self.max_players,
            view_distance: self.view_distance,
            simulation_distance: self.view_distance,
            dimension_type: dimension_type::OVERWORLD,
            dimension: DIMENSION,
            hashed_seed: 0,
            game_mode,
            is_flat: true,
            sea_level: flat::SEA_LEVEL,
            online_mode: self.online_mode,
        };
        self.links.send(id, &login);
        self.links.send(id, &ABILITIES);

        let spawn = Vec3::new(SPAWN_X, f64::from(flat::surface_y()), SPAWN_Z);
        let mut player = Player::new(id, uuid, name, game_mode, client, spawn);
        movement::teleport(
            &mut player,
            &mut self.links,
            &mut self.player_grid,
            self.tick,
            spawn,
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

    /// 进服第 8 到 10 步：`set_time`、`game_event` 13、`set_chunk_cache_center`。
    /// 跟踪范围里的区块都进 `pending`，这一 tick 末尾发第一批。
    pub(crate) fn start_loading_chunks(&mut self, id: PlayerId, time: GameTime) {
        let time = SetTime {
            game_time: time.0,
            clock: world_clock::OVERWORLD,
            clock_ticks: NOON_TICKS,
            clock_rate: 0.0,
        };
        self.links.send(id, &time);
        // 客户端收不到它就永远停在加载画面。
        self.links.send(id, &LevelChunksLoadStart);
        if let Some(player) = self.players.get_mut(&id) {
            chunk_tracking::update_player_view(player, &mut self.links, self.view_distance);
        }
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
