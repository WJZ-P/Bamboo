//! 岛的拆合（岛的实施计划第 6 节第 9 步、3.3 节）：把一个岛按区块集合和玩家集合整批
//! 拆成两个，或者把两个岛整批合成一个。还不在运行时调用，第 10 步由分区器驱动。
//!
//! 每一类按位置或按玩家存的数据怎么分、怎么并、要不要平移时间，逐类写在计划 3.3 的
//! 「岛上每一类数据的拆合」表里；这里的 [`IslandPart`] 和那张表一一对应。拆的时候
//! 新的 `Region` 用结构体字面量逐个字段给出、合的时候把被吸收方逐个字段解构，
//! 以后给 `Region` 加字段而忘了在这里决定怎么拆合，编译就不过。
//!
//! 两条前提（调用方保证，这里断言）：两部分的已加载区块切比雪夫距离至少 3（实体 tick
//! 要看半径 2 的邻居都已交付；分区器给的间隔至少 8 个区块），以及视野不跨岛（计划
//! 4.2：实体的观察者、玩家的观察者都和它在同一个岛）。
//!
//! Island split and merge (island plan, section 6 step 9 and 3.3): split one
//! island into two by a chunk set and a player set, or merge two into one, in
//! batches. Not called at run time yet; step 10 drives it from the
//! partitioner. Each class of positional or per-player state is listed in the
//! plan's 3.3 table and matches [`IslandPart`] here. The split builds the new
//! `Region` with a struct literal and the merge destructures the absorbed one
//! field by field, so a new field without a split/merge decision does not
//! compile. Preconditions (asserted): the two parts' loaded chunks are at
//! least 3 apart, and no view crosses islands (plan 4.2).

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::{LevelRandom, entity::Entities};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_loot::RandomSequences;
use mc_bamboo_world::store::ChunkStore;

#[cfg(test)]
use super::ClockParts;
use super::{Region, column::NotReady};
use crate::{
    chunk_encoding::ChunkCache, inbound::PlayerId, interaction::BlockChanges,
    island_partition::SectionPos,
};

/// 两部分的已加载区块之间至少要隔开的切比雪夫距离：方块逻辑看半径 1、实体看半径 2 的
/// 邻居是否都已交付（`TickLevels`），邻域不能跨两部分。
///
/// The minimum Chebyshev distance between the two parts' loaded chunks:
/// entity ticking looks at radius 2, so no neighbourhood may span both parts.
pub(crate) const ISLAND_GAP: i32 = 3;

/// 新岛的四个随机源（岛的实施计划第 2 节第 15 行、6.2）。拆的时候由调用方给出，运行时
/// 用 [`IslandRandom::derive`]：只由（世界种子，岛的起源 section，建岛时的全局游戏时间）
/// 派生（计划 3.3「岛的随机源」第 1 条）。合并时吸收方留自己的，被吸收方的丢掉。
/// `/random` 和 `/loot` 的按名序列不在岛上，在主控（同一节第 2 条）。
///
/// The new island's four random sources (plan section 2 row 15, 6.2). The
/// caller supplies them on a split; at run time [`IslandRandom::derive`] takes
/// them from (world seed, the island's origin section, the global game time
/// at creation) only (plan 3.3 item 1). A merge keeps the absorber's and
/// drops the absorbed island's. The named sequences of `/random` and `/loot`
/// live on the master (item 2).
pub(crate) struct IslandRandom {
    /// 官方 `Level.random` 和 `randValue`。
    pub(crate) random: LevelRandom,
    /// 官方 `Level.soundSeedGenerator`。
    pub(crate) sound_seed_generator: LevelRandom,
    /// 战利品表的 `random_sequence`。
    pub(crate) loot_sequences: RandomSequences,
    /// 实体种子源的种子：这个岛上生成的实体从 `JavaRandom::new(这个数)` 依次取种子
    /// （`EntityStore::set_entity_seeds`）。
    ///
    /// The seed of the entity seed source: entities spawned on this island draw
    /// their seeds from `JavaRandom::new(this)` in turn.
    pub(crate) entity_seeds: i64,
}

impl IslandRandom {
    /// 只由（`base`，岛的起源 section，建岛时的全局游戏时间 `time`）派生，和拆合的先后、
    /// 原岛的随机状态都无关。加上建岛时刻是因为同一块地方拆出来、合回去、再拆出来时
    /// 起源 section 相同：只看起源的话新岛会把上一次那串随机数从头重放一遍，实体的种子
    /// 重复，牛的 UUID（取自实体自己的随机源）就和上一次那头撞上，`/summon` 和刷怪蛋失败。
    /// 同一刻同一起源只派生一次（`Islands::note_derivation` 断言）。战利品的按名序列沿用
    /// 原来的世界种子，`salt` 换成派生值（总设计稿 6.6「按世界种子、序列名、region 起源
    /// section 派生」）。
    ///
    /// Derived from (`base`, origin section, global game time `time` at
    /// creation) only, independent of the split and merge history and of the
    /// parent's random state. The creation time is included because the same
    /// area split off, merged back and split off again has the same origin:
    /// without it the new island would replay the previous run of numbers,
    /// repeating entity seeds and so cow UUIDs (taken from the entity's own
    /// random), making `/summon` and spawn eggs fail. One derivation per
    /// origin per tick (asserted by `Islands::note_derivation`).
    pub(crate) fn derive(base: i64, origin: SectionPos, time: i64, loot_world_seed: i64) -> Self {
        let stream = |index: u64| -> i64 {
            let key = (u64::from(origin.x as u32) << 32) | u64::from(origin.z as u32);
            splitmix(
                splitmix(base as u64 ^ index.wrapping_mul(0xA24B_AED4_963E_E407))
                    ^ key
                    ^ splitmix(time as u64),
            ) as i64
        };
        let sequences = |world_seed: i64, index: u64| {
            let mut sequences = RandomSequences::new(world_seed);
            sequences.set_seed_defaults(stream(index) as i32, true, true);
            sequences
        };
        Self {
            random: LevelRandom::new(stream(1)),
            sound_seed_generator: LevelRandom::new(stream(2)),
            loot_sequences: sequences(loot_world_seed, 3),
            entity_seeds: stream(4),
        }
    }
}

/// 一个岛「只属于它自己」、不跟着区块或玩家走的东西：四个随机源现在的状态和它的剖析器。
/// 平常的合并留吸收方的、丢被吸收方的；主控把全部岛临时拼成一个执行命令时（计划 4.5）
/// 把每个被并进来的岛的这一份存着，拆回去时原样换上，别的岛的命令不会推进它的随机源。
/// 红石缓存和区块包缓存不在这里：拼着的时候它们收不到失效通知，只能清空。
///
/// What belongs to one island alone and follows neither chunks nor players:
/// the state of its four random sources and its profiler. A normal merge
/// keeps the absorber's and drops the absorbed island's; when the master
/// temporarily joins every island into one to run a command (plan 4.5) it
/// keeps each absorbed island's and puts it back on the split, so another
/// island's command never advances its random sources. The redstone cache
/// and the chunk packet cache are not here: they would miss invalidations
/// while joined and can only be cleared.
pub(crate) struct IslandIdentity {
    random: LevelRandom,
    sound_seed_generator: LevelRandom,
    loot_sequences: RandomSequences,
    entity_seeds: mc_bamboo_blocks::entity::JavaRandom,
    profiler: crate::profiler::Profiler,
}

/// SplitMix64 的一步，把相近的输入搅开。
fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 拆分时要按位置或按玩家分走的每一类数据（计划 3.3 的表）。正式路径一类不落；
/// 测试用 [`Region::split_off_island_parts`] 故意漏掉一类，确认核对能报出来。
///
/// Every class of state a split moves by position or by player (plan 3.3).
/// Production moves all of them; tests skip one on purpose to make sure the
/// ownership check reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IslandPart {
    /// 区块表：方块、两层光照、版本号、原样起点、已加载登记、存盘改动集合。
    Chunks,
    /// 每区块的 tick 档位和三个 tick 集合、待交出的成员变化。
    TickLevels,
    /// 计划刻（主队列、搁置表、活塞记录、每区块标记）。
    BlockTicks,
    /// 方块事件队列。
    BlockEvents,
    /// 方块实体和它们的运行顺序、睡眠日程、关注表。
    BlockEntities,
    /// 红石记忆（比较器、书架、唱片机、合成器、讲台页码、火把记录）。
    Redstone,
    /// 容器内容。
    Containers,
    /// 方块实体从物品带来的组件。
    BlockItems,
    /// 命令方块里的命令。
    CommandBlocks,
    /// 讲台上的书。
    Books,
    /// 实体表。
    Entities,
    /// 实体表里还没交给同步层的生成快照（这一刻同步之后才生成的实体，下一刻按它建
    /// 跟踪状态）。跟着实体走。
    SpawnSnapshots,
    /// 实体表里离场生物留下、还没交给同步层的事件（实体行已经离表）。按生物离场时
    /// 所在的区块走。
    DepartedMobEvents,
    /// 实体和玩家的配对。
    EntityTracking,
    /// 上层内容的待存标记。
    Dirty,
    /// 还没广播的光照变化。
    LightChanges,
    /// 还没广播的方块变化。
    BlockChanges,
    /// 区块包缓存（拆走的部分在原岛丢掉，新岛从空的开始）。
    ChunkCache,
    /// 玩家表。
    Players,
    /// 玩家网格。
    PlayerGrid,
    /// 出站链路和缓冲。
    Links,
    /// 打开的菜单。
    Menus,
    /// 讲台视图和待发格子。
    LecternViews,
    /// 对端 IP。
    PeerIps,
    /// 最后操作时刻。
    LastAction,
}

impl IslandPart {
    /// 全部类别，测试逐类改坏用。
    #[cfg(test)]
    pub(crate) const ALL: [Self; 25] = [
        Self::Chunks,
        Self::TickLevels,
        Self::BlockTicks,
        Self::BlockEvents,
        Self::BlockEntities,
        Self::Redstone,
        Self::Containers,
        Self::BlockItems,
        Self::CommandBlocks,
        Self::Books,
        Self::Entities,
        Self::SpawnSnapshots,
        Self::DepartedMobEvents,
        Self::EntityTracking,
        Self::Dirty,
        Self::LightChanges,
        Self::BlockChanges,
        Self::ChunkCache,
        Self::Players,
        Self::PlayerGrid,
        Self::Links,
        Self::Menus,
        Self::LecternViews,
        Self::PeerIps,
        Self::LastAction,
    ];
}

/// 按键把一张表分成两份：`moves` 为真的搬出去。一遍扫完，和要搬多少无关。
fn split_map<K: Ord, V>(table: &mut BTreeMap<K, V>, moves: impl Fn(&K) -> bool) -> BTreeMap<K, V> {
    let (moved, kept) = std::mem::take(table)
        .into_iter()
        .partition(|(key, _)| moves(key));
    *table = kept;
    moved
}

/// 按玩家把一张表分成两份。玩家只有几个，逐个取。
fn split_players<V>(
    table: &mut BTreeMap<PlayerId, V>,
    players: &BTreeSet<PlayerId>,
) -> BTreeMap<PlayerId, V> {
    players
        .iter()
        .filter_map(|id| Some((*id, table.remove(id)?)))
        .collect()
}

/// 断言 `moved` 里的已加载区块和 `stays` 里的已加载区块切比雪夫距离至少 [`ISLAND_GAP`]。
fn assert_gap(
    moved: impl IntoIterator<Item = ChunkPos>,
    is_moved: impl Fn(ChunkPos) -> bool,
    stays: impl Fn(ChunkPos) -> bool,
) {
    let reach = ISLAND_GAP - 1;
    for pos in moved {
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let near = ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz));
                assert!(
                    is_moved(near) || !stays(near),
                    "岛的两部分靠得太近：{pos:?} 和 {near:?} 都已加载（要隔开至少 {ISLAND_GAP} 个区块）"
                );
            }
        }
    }
}

impl Region {
    /// 按 [`IslandRandom::derive`] 派生新岛的随机源：建岛时刻取这个岛的全局快照里的
    /// 游戏时间（主控每刻发布，所有岛相同），战利品序列沿用这个岛的世界种子。
    ///
    /// Derive a new island's random sources: the creation time is the game time
    /// in this island's global snapshot (published by the master every tick,
    /// the same on every island); the loot sequences keep this island's world seed.
    pub(crate) fn derive_island_random(&self, base: i64, origin: SectionPos) -> IslandRandom {
        IslandRandom::derive(
            base,
            origin,
            self.global.time.0,
            self.loot_sequences.world_seed(),
        )
    }

    /// 换上一份新的四个随机源（空着的岛被当作新岛接手一块时，计划 3.3「岛的随机源」
    /// 第 1 条：新岛的随机源只由世界种子和它的起源 section 派生）。
    ///
    /// Install a fresh set of the four random sources (an empty island taking
    /// over a part as a new island, plan 3.3 item 1: a new island's sources come
    /// from the world seed and its origin section only).
    pub(crate) fn replace_random(&mut self, random: IslandRandom) {
        let IslandRandom {
            random,
            sound_seed_generator,
            loot_sequences,
            entity_seeds,
        } = random;
        self.random = random;
        self.sound_seed_generator = sound_seed_generator;
        self.loot_sequences = loot_sequences;
        self.entities.set_entity_seeds(entity_seeds);
    }

    /// 岛拆分：把位置落在 `chunks` 里的一切和 `players` 的一切整批拆出去，造出新岛的
    /// `Region`。`chunks` 是新岛拥有的全部区块位置（已加载的、还在信箱里的都算）；
    /// 两边时钟相同，不平移。只在 tick 边界调用，前提不满足时返回 [`NotReady`]、岛不变。
    ///
    /// 新岛照抄原岛的时钟、全局快照、配置和区块包模板；红石缓存、单元划分、剖析器
    /// 从空的开始（缓存和统计，丢掉只损失命中）；管理名单的本刻改动留在原岛
    /// （它们是全局的，由主控在交回时取走）。随机源由调用方给（[`IslandRandom`]）。
    ///
    /// Island split: move everything positioned in `chunks` and everything of
    /// `players` into a new island's `Region` in one batch. `chunks` is every
    /// chunk position the new island owns, loaded or still in the mailbox. Both
    /// clocks are equal, so nothing shifts. Tick boundary only; otherwise
    /// [`NotReady`] with the island untouched. The new island copies the clock,
    /// the global snapshot, configuration and the chunk template; the redstone
    /// cache, unit partitioning and profiler start empty; this tick's admin
    /// list changes stay. The caller supplies the random sources.
    pub(crate) fn split_off_island(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
        players: &BTreeSet<PlayerId>,
        random: IslandRandom,
    ) -> Result<Region, NotReady> {
        self.split_off_island_parts(chunks, players, random, None)
    }

    /// 同 [`Self::split_off_island`]；`skip` 给出时故意不搬那一类（只给改坏确认用）。
    ///
    /// Like [`Self::split_off_island`]; `skip` leaves one class behind on
    /// purpose (break-it checks only).
    pub(crate) fn split_off_island_parts(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
        players: &BTreeSet<PlayerId>,
        random: IslandRandom,
        skip: Option<IslandPart>,
    ) -> Result<Region, NotReady> {
        assert!(self.chunks.is_loaded_mode(), "只有已加载模式能拆岛");
        if !self.ready_to_split_island() {
            return Err(NotReady);
        }
        let store = &self.chunks;
        assert_gap(
            chunks.iter().copied().filter(|&pos| store.is_loaded(pos)),
            |pos| chunks.contains(&pos),
            |pos| store.is_loaded(pos),
        );
        for id in players {
            assert!(
                self.players.contains_key(id),
                "要搬的玩家 {id:?} 不在这个岛上"
            );
        }
        // 视野不跨岛：搬走的玩家只被搬走的玩家看见，留下的只被留下的看见。
        debug_assert!(self.players.iter().all(|(id, player)| {
            if players.contains(id) {
                player.sync.seen_by().is_subset(players)
            } else {
                player.sync.seen_by().is_disjoint(players)
            }
        }));
        let moves = |part: IslandPart| skip != Some(part);
        let inside = |pos: &BlockPos| chunks.contains(&pos.chunk());
        lap("checks");

        // 前提都检查过了，下面每一步都不会失败。
        let new_chunks = if moves(IslandPart::Chunks) {
            self.chunks
                .split_off(chunks)
                .unwrap_or_else(|_| unreachable!("前提已经检查过"))
        } else {
            ChunkStore::loaded()
        };
        lap("Chunks");
        let tick_levels = if moves(IslandPart::TickLevels) {
            self.tick_levels.split_off(chunks)
        } else {
            crate::tick_levels::TickLevels::new(self.tick_levels.enabled())
        };
        lap("TickLevels");
        let block_ticks = if moves(IslandPart::BlockTicks) {
            self.block_ticks.split_off_island(chunks)
        } else {
            self.block_ticks.split_off_island(&BTreeSet::new())
        };
        lap("BlockTicks");
        let block_events = if moves(IslandPart::BlockEvents) {
            self.block_events.split_off(chunks)
        } else {
            self.block_events.split_off(&BTreeSet::new())
        };
        lap("BlockEvents");
        let block_entities = self
            .block_entities
            .split_off_island(if moves(IslandPart::BlockEntities) {
                chunks
            } else {
                const { &BTreeSet::new() }
            })
            .unwrap_or_else(|_| unreachable!("前提已经检查过"));
        lap("BlockEntities");
        let redstone = if moves(IslandPart::Redstone) {
            self.redstone.split_off(chunks)
        } else {
            self.redstone.split_off(&BTreeSet::new())
        };
        lap("Redstone");
        let containers = if moves(IslandPart::Containers) {
            split_map(&mut self.containers, inside)
        } else {
            BTreeMap::new()
        };
        lap("Containers");
        let block_items = if moves(IslandPart::BlockItems) {
            split_map(&mut self.block_items, inside)
        } else {
            BTreeMap::new()
        };
        lap("BlockItems");
        let command_blocks = if moves(IslandPart::CommandBlocks) {
            split_map(&mut self.command_blocks, inside)
        } else {
            BTreeMap::new()
        };
        lap("CommandBlocks");
        let mut entities = if moves(IslandPart::Entities) {
            self.entities
                .split_off(chunks, random.entity_seeds)
                .unwrap_or_else(|_| unreachable!("前提已经检查过"))
        } else {
            let mut empty = Entities::with_ids(self.entities.ids_handle().clone());
            empty.set_entity_seeds(random.entity_seeds);
            empty
        };
        if !moves(IslandPart::SpawnSnapshots) {
            self.entities.reclaim_spawn_snapshots(&mut entities);
        }
        if !moves(IslandPart::DepartedMobEvents) {
            self.entities.reclaim_departed_events(&mut entities);
        }
        lap("Entities");
        let moved_entities: BTreeSet<i32> = entities.ids().into_iter().collect();
        let entity_tracking = if moves(IslandPart::EntityTracking) {
            self.entity_tracking.split_off(&moved_entities, players)
        } else {
            crate::entity_tracking::EntityTracking::default()
        };
        lap("EntityTracking");
        let dirty = if moves(IslandPart::Dirty) {
            self.dirty.split_off(chunks)
        } else {
            super::dirty::Dirty::default()
        };
        lap("Dirty");
        let light_changes = if moves(IslandPart::LightChanges) {
            split_map(&mut self.light_changes, |pos| chunks.contains(pos))
        } else {
            BTreeMap::new()
        };
        lap("LightChanges");
        let mut block_changes = BlockChanges::default();
        if moves(IslandPart::BlockChanges) {
            let (moved, kept) = std::mem::take(&mut self.block_changes.changed)
                .into_iter()
                .partition(|pos| inside(pos));
            self.block_changes.changed = kept;
            block_changes.changed = moved;
        }
        lap("BlockChanges");
        if moves(IslandPart::ChunkCache) {
            self.chunk_cache.retain(|pos| !chunks.contains(&pos));
        }
        lap("ChunkCache");
        let lecterns = self.lecterns.split_off(
            if moves(IslandPart::Books) {
                chunks
            } else {
                const { &BTreeSet::new() }
            },
            if moves(IslandPart::LecternViews) {
                players
            } else {
                const { &BTreeSet::new() }
            },
        );
        lap("Lecterns");
        let none = const { &BTreeSet::new() };
        let pick = |part: IslandPart| if moves(part) { players } else { none };
        let new_players = split_players(&mut self.players, pick(IslandPart::Players));
        let player_grid = self.player_grid.split_off(pick(IslandPart::PlayerGrid));
        let links = self.links.split_off(pick(IslandPart::Links));
        let menus = split_players(&mut self.menus, pick(IslandPart::Menus));
        let peer_ips = split_players(&mut self.peer_ips, pick(IslandPart::PeerIps));
        let last_action_millis =
            split_players(&mut self.last_action_millis, pick(IslandPart::LastAction));
        lap("Players");

        let IslandRandom {
            random,
            sound_seed_generator,
            loot_sequences,
            entity_seeds: _,
        } = random;
        Ok(Region {
            players: new_players,
            player_grid,
            links,
            chunks: new_chunks,
            tick_levels,
            template_body: self.template_body.clone(),
            chunk_cache: ChunkCache::default(),
            block_changes,
            light_changes,
            block_ticks,
            containers,
            block_items,
            menus,
            lecterns,
            block_entities,
            block_events,
            random,
            redstone_cache: mc_bamboo_blocks::cache::RedstoneCache::new(
                *self.redstone_cache.config(),
            ),
            loot_sequences,
            sound_seed_generator,
            entities,
            entity_tracking,
            redstone,
            tick: self.tick,
            view_distance: self.view_distance,
            max_players: self.max_players,
            online_mode: self.online_mode,
            enforce_secure_profile: self.enforce_secure_profile,
            service_keys: self.service_keys.clone(),
            clock_millis: self.clock_millis,
            global: self.global.clone(),
            broadcast_console_to_ops: self.broadcast_console_to_ops,
            command_blocks,
            admin_changes: crate::admin::AdminChanges::default(),
            last_action_millis,
            peer_ips,
            profiler: self.profiler.fresh_like(),
            partitioning: self.partitioning.fresh_like(),
            dirty,
        })
    }

    /// 岛合并：把 `other` 整批倒进自己。`delta = 自己的时钟 − other 的时钟`：先按它平移
    /// 被吸收方的全部绝对时刻（[`Self::rebase_clock`]，计划 5.1），平移之后两边时钟
    /// 相同，再逐类并。吸收方的时钟、随机源、全局快照、剖析器留下；被吸收方的红石缓存、
    /// 区块包缓存丢掉，剖析器的样本并进吸收方的；单元划分重建。只在两个岛都在 tick 边界时调用（断言）。
    ///
    /// Island merge: pour `other` into this island. `delta = self.now −
    /// other.now`: first shift every absolute instant of the absorbed island
    /// ([`Self::rebase_clock`], plan 5.1) so both clocks agree, then merge class
    /// by class. The absorber keeps its clock, random sources, global snapshot
    /// and profiler; the absorbed redstone cache and chunk cache are dropped
    /// and its profiler's samples join the absorber's; unit partitioning is rebuilt. Both at a tick boundary.
    pub(crate) fn absorb_island(&mut self, other: Region, delta: i64) {
        let identity = self.absorb_island_keeping(other, delta);
        // 被吸收方还没报告的样本并进吸收方的，不白丢（剖析器不影响行为）。
        // The absorbed island's unreported samples join the absorber's
        // instead of being lost (profilers never affect behaviour).
        self.profiler.merge(&identity.profiler);
    }

    /// 同 [`Self::absorb_island`]，但把被吸收方「只属于它自己」的东西交还给调用方，而不是
    /// 丢掉（见 [`IslandIdentity`]）。主控把全部岛临时拼成一个执行命令时用：做完按原样
    /// 拆回去，再用 [`Self::restore_identity`] 换上（计划 4.5）。
    ///
    /// Like [`Self::absorb_island`], but hands back what belongs to the
    /// absorbed island alone instead of dropping it (see [`IslandIdentity`]).
    /// Used when the master temporarily joins every island into one to run a
    /// command; afterwards the islands are split back and get theirs again
    /// through [`Self::restore_identity`] (plan 4.5).
    pub(crate) fn absorb_island_keeping(
        &mut self,
        mut other: Region,
        delta: i64,
    ) -> IslandIdentity {
        self.assert_mergeable(&other);
        other.rebase_clock(delta);
        lap("merge rebase");
        self.merge_rebased(other)
    }

    /// [`Self::absorb_island_keeping`] 的逆操作：把 `chunks` 和 `players` 拆出去，新岛
    /// 换上存下来的随机源状态和剖析器，而不是派生一份新的。
    ///
    /// The inverse of [`Self::absorb_island_keeping`]: split `chunks` and
    /// `players` off and give the new island its saved random-source state and
    /// profiler instead of freshly derived ones.
    pub(crate) fn split_off_island_restoring(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
        players: &BTreeSet<PlayerId>,
        identity: IslandIdentity,
    ) -> Result<Region, NotReady> {
        if !self.ready_to_split_island() {
            return Err(NotReady);
        }
        let IslandIdentity {
            random,
            sound_seed_generator,
            loot_sequences,
            entity_seeds,
            profiler,
        } = identity;
        let mut region = self.split_off_island(
            chunks,
            players,
            IslandRandom {
                random,
                sound_seed_generator,
                loot_sequences,
                entity_seeds: 0,
            },
        )?;
        region.entities.set_entity_seed_source(entity_seeds);
        region.profiler = profiler;
        Ok(region)
    }

    /// 同 [`Self::absorb_island`]，平移只做 `clock` 选中的几类（只给改坏确认用）。
    ///
    /// Like [`Self::absorb_island`] shifting only the selected classes
    /// (break-it checks only).
    #[cfg(test)]
    pub(crate) fn absorb_island_parts(&mut self, mut other: Region, delta: i64, clock: ClockParts) {
        self.assert_mergeable(&other);
        other.rebase_clock_parts(delta, clock);
        let _ = self.merge_rebased(other);
    }

    fn assert_mergeable(&self, other: &Region) {
        assert!(
            self.chunks.is_loaded_mode() && other.chunks.is_loaded_mode(),
            "只有已加载模式能合岛"
        );
        assert!(
            self.ready_to_split_island() && other.ready_to_split_island(),
            "只能在 tick 边界合岛"
        );
    }

    /// 被吸收方已经平移到吸收方的时钟，逐类并进来。返回被吸收方自己的随机源和剖析器
    /// （平常的合并把它们丢掉）。
    fn merge_rebased(&mut self, other: Region) -> IslandIdentity {
        debug_assert_eq!(other.tick, self.tick, "平移之后两边的时钟要相同");
        debug_assert_eq!(
            other.block_ticks.now(),
            self.block_ticks.now(),
            "平移之后两边计划刻的时钟要相同"
        );
        let mine = &self.chunks;
        assert_gap(
            other.chunks.loaded_positions(),
            |_| false,
            |pos| mine.is_loaded(pos),
        );
        lap("merge checks");
        let Region {
            players,
            player_grid,
            links,
            chunks,
            tick_levels,
            template_body: _,
            chunk_cache: _,
            block_changes,
            light_changes,
            block_ticks,
            containers,
            block_items,
            menus,
            lecterns,
            block_entities,
            block_events,
            random,
            redstone_cache: _,
            loot_sequences,
            sound_seed_generator,
            entities,
            entity_tracking,
            redstone,
            tick: _,
            view_distance: _,
            max_players: _,
            online_mode: _,
            enforce_secure_profile: _,
            service_keys: _,
            clock_millis: _,
            global: _,
            broadcast_console_to_ops: _,
            command_blocks,
            admin_changes,
            last_action_millis,
            peer_ips,
            profiler,
            partitioning: _,
            dirty,
        } = other;
        let identity = IslandIdentity {
            random,
            sound_seed_generator,
            loot_sequences,
            entity_seeds: entities.entity_seed_source(),
            profiler,
        };
        self.chunks
            .absorb(chunks)
            .unwrap_or_else(|_| unreachable!("前提已经检查过"));
        lap("merge Chunks");
        self.tick_levels.absorb(tick_levels);
        lap("merge TickLevels");
        self.block_ticks.absorb(block_ticks);
        lap("merge BlockTicks");
        self.block_events.absorb(block_events);
        lap("merge BlockEvents");
        self.block_entities.absorb_island(block_entities);
        lap("merge BlockEntities");
        self.redstone.absorb(redstone);
        lap("merge Redstone");
        self.containers.extend(containers);
        self.block_items.extend(block_items);
        self.command_blocks.extend(command_blocks);
        lap("merge Containers/BlockItems/CommandBlocks");
        self.entities.absorb(entities);
        lap("merge Entities");
        self.entity_tracking.absorb(entity_tracking);
        lap("merge EntityTracking");
        self.dirty.absorb(dirty);
        self.light_changes.extend(light_changes);
        self.block_changes.changed.extend(block_changes.changed);
        lap("merge Dirty/Light/BlockChanges");
        for (id, player) in players {
            let previous = self.players.insert(id, player);
            assert!(previous.is_none(), "玩家 {id:?} 在两个岛上");
        }
        self.player_grid.absorb(player_grid);
        self.links.absorb(links);
        self.menus.extend(menus);
        self.lecterns.absorb(lecterns);
        self.peer_ips.extend(peer_ips);
        self.last_action_millis.extend(last_action_millis);
        self.admin_changes.merge(admin_changes);
        lap("merge Players");
        self.partitioning = self.partitioning.fresh_like();
        identity
    }
}

/// 一批玩家在一个岛上的全部「跟着玩家走」的表（计划 3.3 表里按玩家分的七类：玩家、
/// 玩家网格、出站链路、菜单、讲台视图和待发格子、对端 IP、最后操作时刻）。主控借玩家
/// （[`Region::lend_players`]）时整套借出、整套还回，和拆合时搬的是同一套，少一类都会
/// 让借来的玩家在发起人的岛上留下东西（比如 `/tp` 写进网格的登记）。
///
/// Every player-following table of a batch of players on one island (the
/// seven per-player classes of plan 3.3: players, player grid, outbound
/// links, menus, lectern views and pending slots, peer IPs, last action).
/// The master's borrowing ([`Region::lend_players`]) moves the whole set out
/// and back, the same set a split moves; missing one would leave something of
/// a borrowed player on the sender's island (such as a grid entry written by
/// `/tp`).
pub(crate) struct PlayerTables {
    players: BTreeMap<PlayerId, crate::player::Player>,
    player_grid: crate::player_grid::PlayerGrid,
    links: crate::links::Links,
    menus: BTreeMap<PlayerId, crate::menu::Menu>,
    lecterns: crate::lectern::Tables,
    peer_ips: BTreeMap<PlayerId, String>,
    last_action_millis: BTreeMap<PlayerId, i64>,
    /// 借出时岛的时钟。玩家身上有按岛时钟记的绝对时刻（传送确认的 `sent_at`、讲台待发
    /// 格子的 `deliver_at`），放进另一个岛时按两岛时钟差平移（计划 5.5.2）。
    ///
    /// The lending island's clock. Players carry absolute instants on the
    /// island clock (`sent_at` of an unconfirmed teleport, `deliver_at` of a
    /// pending lectern slot); they are rebased by the clock difference when
    /// the tables enter another island (plan 5.5.2).
    clock: i64,
}

impl PlayerTables {
    /// 借出来的玩家，按编号升序。
    pub(crate) fn ids(&self) -> Vec<PlayerId> {
        self.players.keys().copied().collect()
    }
}

impl Region {
    /// 把 `players` 的七类表整套借出去（见 [`PlayerTables`]）。不在这个岛上的编号跳过。
    ///
    /// Lend the seven tables of `players` (see [`PlayerTables`]); ids not on
    /// this island are skipped.
    pub(crate) fn lend_players(&mut self, players: &BTreeSet<PlayerId>) -> PlayerTables {
        PlayerTables {
            players: split_players(&mut self.players, players),
            player_grid: self.player_grid.split_off(players),
            links: self.links.split_off(players),
            menus: split_players(&mut self.menus, players),
            lecterns: self.lecterns.split_off(&BTreeSet::new(), players),
            peer_ips: split_players(&mut self.peer_ips, players),
            last_action_millis: split_players(&mut self.last_action_millis, players),
            clock: self.tick.game_time(),
        }
    }

    /// 收下借来（或还回来）的玩家表。两个岛的时钟不同时（混合驱动下慢岛落后，计划 5.5），
    /// 玩家身上按岛时钟记的两样绝对时刻按时钟差平移到这个岛的时钟（见
    /// [`PlayerTables::clock`]）；还回去时再平移回来，一来一回分毫不差。
    ///
    /// Take borrowed (or returned) player tables. When the clocks differ (a
    /// slow island lags under the hybrid drive, plan 5.5), the two absolute
    /// instants players carry are rebased onto this island's clock (see
    /// [`PlayerTables::clock`]); returning them rebases back exactly.
    ///
    /// `shift` 为假时故意不平移（只给改坏确认用，正式路径恒为真）。
    ///
    /// `shift = false` skips the rebase on purpose (break-it checks only;
    /// always true on production paths).
    pub(crate) fn take_players(&mut self, tables: PlayerTables, shift: bool) {
        let PlayerTables {
            mut players,
            player_grid,
            links,
            menus,
            mut lecterns,
            peer_ips,
            last_action_millis,
            clock,
        } = tables;
        let delta = self.tick.game_time() - clock;
        if delta != 0 && shift {
            lecterns.rebase(delta);
            for player in players.values_mut() {
                player.teleport.sent_at = player.teleport.sent_at.rebased(delta);
            }
        }
        for (id, player) in players {
            let previous = self.players.insert(id, player);
            assert!(previous.is_none(), "玩家 {id:?} 在两个岛上");
        }
        self.player_grid.absorb(player_grid);
        self.links.absorb(links);
        self.menus.extend(menus);
        self.lecterns.absorb(lecterns);
        self.peer_ips.extend(peer_ips);
        self.last_action_millis.extend(last_action_millis);
    }
}

#[cfg(test)]
pub(crate) use laps::lap;

/// 正式构建：不记。
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn lap(_: &'static str) {}

/// 测试专用：拆合（和拼 / 拆整个世界）每一段各花了多久，微基准按段打印。正式构建里
/// [`lap`] 是空函数。
///
/// Test only: how long each stage of a split or merge (and of joining /
/// dividing the world) took, printed per stage by the micro-benchmarks. In
/// production builds [`lap`] is an empty function.
#[cfg(test)]
pub(crate) mod laps {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    type Laps = Option<(Instant, Vec<(&'static str, Duration)>)>;

    thread_local! {
        static LAPS: RefCell<Laps> = const { RefCell::new(None) };
    }

    /// 开始记：之后每次 [`lap`] 记下从上一次到现在的时长。
    pub(crate) fn start() {
        LAPS.with(|laps| *laps.borrow_mut() = Some((Instant::now(), Vec::new())));
    }

    /// 记一段（没在记时什么都不做）。
    pub(crate) fn lap(name: &'static str) {
        LAPS.with(|laps| {
            if let Some((last, out)) = laps.borrow_mut().as_mut() {
                let now = Instant::now();
                out.push((name, now - *last));
                *last = now;
            }
        });
    }

    /// 停下并取走记下的各段。
    pub(crate) fn take() -> Vec<(&'static str, Duration)> {
        LAPS.with(|laps| {
            laps.borrow_mut()
                .take()
                .map(|(_, out)| out)
                .unwrap_or_default()
        })
    }
}
