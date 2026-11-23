//! 岛的一刻里的自然生成阶段（官方 `ServerChunkCache.tickChunks` 的刷怪部分）。
//!
//! 官方在 `chunkSource.tick` 里先重算计数、再打乱刷怪区块列表、逐区块刷怪，最后才跑随机刻；
//! 这里插在随机刻之前，位置和官方一致。打乱用的是岛自己的世界随机源（`Region.random`），
//! 和官方用 `level.getRandom()` 打乱 `spawningChunks` 一样。官方按
//! `Long2ByteOpenHashMap` 的哈希顺序遍历区块，Bamboo 按坐标顺序收集，所以只有列表内容和
//! 计数可比较，打乱后的具体顺序不同（`docs/multi-region-tick-design.md:58`）。
//!
//! The natural-spawn stage of an island's tick (the spawn part of vanilla
//! `ServerChunkCache.tickChunks`). Vanilla rebuilds the counts, shuffles the spawning
//! chunk list, spawns per chunk and only then runs random ticks; this stage sits right
//! before the random ticks. The list is shuffled with the island's own world random, as
//! vanilla shuffles `spawningChunks` with `level.getRandom()`. Vanilla collects chunks in
//! `Long2ByteOpenHashMap` hash order and Bamboo in coordinate order, so only the list's
//! contents and the counts compare; the shuffled order differs.

use std::collections::BTreeSet;

use mc_bamboo_blocks::World;
use mc_bamboo_blocks::entity::mob::natural::{self, SpawnParams, SpawnState};
use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::gamerules::SPECTATORS_GENERATE_CHUNKS;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::region::Region;

/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`：`getNaturalSpawnChunkCount` 只算玩家周围 8 个
/// 区块内的区块（切比雪夫距离）。
///
/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`: `getNaturalSpawnChunkCount` only counts chunks
/// within 8 chunks (Chebyshev) of a player.
const SPAWN_CHUNK_RADIUS: i32 = 8;

/// `ServerChunkCache.tickSpawningChunk` 第一句的候选集合：官方 `ChunkMap.collectSpawningChunks`
/// 逐刻给这批区块的 `InhabitedTime` 加一（在打乱和自然刷怪之前）。
///
/// 三个条件照字节码：候选区块来自 `naturalSpawnChunkCounter`（最大距离 8 的玩家区块方形）、
/// 有 ticking 区块（`getTickingChunk() != null`）、有非旁观玩家离区块中心水平距离平方小于
/// 16384。第三条里那名玩家所在的区块必然在前 8 格的方形内，所以纯计算只需
/// 「ticks_blocks」和玩家（位置、是否旁观）两张表。按坐标升序返回；官方按哈希顺序，
/// 但 `InhabitedTime` 的累加和顺序无关。
///
/// The candidate chunks of vanilla `ServerChunkCache.tickSpawningChunk`, whose first
/// statement increments `InhabitedTime` for each one. Game and oracle share this.
pub fn spawning_chunk_candidates(
    players: impl IntoIterator<Item = (mc_bamboo_core::Vec3, bool)>,
    ticks_blocks: impl Fn(ChunkPos) -> bool,
) -> Vec<ChunkPos> {
    let mut found = BTreeSet::new();
    for (position, spectator) in players {
        if spectator {
            continue;
        }
        for chunk in chunk_square(
            ChunkPos::containing(position.x, position.z),
            SPAWN_CHUNK_RADIUS,
        ) {
            if ticks_blocks(chunk) && natural::is_close_to_spawn_chunk(position, chunk) {
                found.insert(chunk);
            }
        }
    }
    found.into_iter().collect()
}

/// 这一刻用于 `InhabitedTime` 累加的刷怪候选区块（按坐标升序）。
/// 无限超平坦模式没有区块等级，照下面自然刷怪的视距门槛补齐；已加载模式由
/// `tick_levels` 决定哪些区块能跑方块逻辑。
///
/// This tick's spawning-chunk candidates for the `InhabitedTime` increment.
pub(crate) fn inhabited_time_chunks(region: &Region) -> Vec<ChunkPos> {
    let radius = crate::interaction::SIMULATION_DISTANCE.min(region.view_distance + 1);
    let players = region
        .players
        .values()
        .map(|player| (player.position(), player.game_mode == GameMode::Spectator));
    spawning_chunk_candidates(players, |chunk| {
        if !region.chunks.is_loaded(chunk) || !region.tick_levels.chunk_ticks_blocks(chunk) {
            return false;
        }
        // 无限模式没有等级，照 `spawning_chunks` 的老口径按玩家视距收窄。
        region.tick_levels.enabled()
            || region.players.values().any(|player| {
                player.game_mode != GameMode::Spectator
                    && in_chunk_square(player.chunk_pos(), chunk, radius)
            })
    })
}

/// 随机刻之外的自然生成。`spawn_mobs` 关掉时整段不做，一个随机数都不取（官方这时仍会
/// 打乱刷怪区块列表，这条差异见 README 的已知差异）。
///
/// Natural spawning beside the random ticks. With `spawn_mobs` off nothing runs and no
/// random number is drawn (vanilla still shuffles the chunk list then; see the known
/// differences in the README).
pub(crate) fn tick(region: &mut Region) {
    if !region.global.block_rules.spawn_mobs {
        return;
    }
    let spawn_enemies = region.global.block_rules.spawn_monsters;
    let candidates = spawn_candidate_chunks(region);
    let spawnable_chunk_count = candidates.len() as i32;
    if spawnable_chunk_count == 0 {
        return;
    }
    let params = SpawnParams {
        spawnable_chunk_count,
        spawn_enemies,
        per_player_cap: region.spawn_per_player_cap,
        respawn_pos: Some(region.global.spawn.pos),
        sea_level: sea_level(region),
    };
    let mut state = {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        SpawnState::create(&world, spawnable_chunk_count)
    };
    let spawn_persistent = region.tick.game_time() % 400 == 0;
    let categories = state.filtered_categories(spawn_enemies, spawn_persistent);
    let mut chunks = spawning_chunks(region, &candidates);
    // 官方即使没有可刷的类别也照样收集并打乱刷怪区块列表（`tickChunks` 里打乱在
    // 类别判断之外），所以这里也每刻取这一串随机数。
    //
    // Vanilla collects and shuffles the spawning chunks even when no category can
    // spawn (the shuffle sits outside the category check), so the draws happen
    // every tick here too.
    {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::shuffle(world.random(), &mut chunks);
    }
    if categories.is_empty() || chunks.is_empty() {
        return;
    }
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        for chunk in chunks {
            if !world.level().chunk_ticks_entities(chunk) {
                continue;
            }
            natural::spawn_for_chunk(&mut world, chunk, &mut state, &params, &categories);
        }
        world.into_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

/// `ChunkGenerator.getSeaLevel()`：超平坦是 `FlatLevelSource` 的 -63，噪声世界读设置。
/// 设置要解析整份世界生成数据，所以只算一次；三个噪声预设的值相同（63）。
///
/// `ChunkGenerator.getSeaLevel()`: -63 for `FlatLevelSource`, the settings value for noise
/// worlds. Reading the settings parses the worldgen data, so it is computed once; the
/// three noise presets share the value (63).
fn sea_level(region: &Region) -> i32 {
    sea_level_of(&region.global)
}

/// 同 [`sea_level`]，给只拿得到全局快照的调用方（实体算阶段的只读视图）。
///
/// Same as [`sea_level`], for callers that only hold the global snapshot (the entity
/// compute phase's read-only view).
pub(crate) fn sea_level_of(global: &crate::global::GlobalView) -> i32 {
    static NOISE: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let level_type = global.world_gen.level_type;
    if level_type.is_flat() {
        return mc_bamboo_world::flat::SEA_LEVEL;
    }
    *NOISE.get_or_init(|| crate::spawn_landing::noise_sea_level(level_type))
}

/// `ChunkMap.collectSpawningChunks`：从候选区块中收集已达方块 tick 等级、且区块中心距
/// 非旁观玩家不到 128 格的区块。实体 tick 的门槛在打乱之后检查。
///
/// `ChunkMap.collectSpawningChunks`: collect block-ticking candidates whose centres
/// are less than 128 blocks from a non-spectator. Check entity ticking after shuffling.
fn spawning_chunks(region: &Region, candidates: &BTreeSet<ChunkPos>) -> Vec<ChunkPos> {
    let radius = crate::interaction::SIMULATION_DISTANCE.min(region.view_distance + 1);
    candidates
        .iter()
        .copied()
        .filter(|&chunk| {
            region.chunks.is_loaded(chunk)
                && region.tick_levels.chunk_ticks_blocks(chunk)
                && region.players.values().any(|player| {
                    player.game_mode != GameMode::Spectator
                        && (region.tick_levels.enabled()
                            || in_chunk_square(player.chunk_pos(), chunk, radius))
                        && natural::is_close_to_spawn_chunk(player.position(), chunk)
                })
        })
        .collect()
}

/// `DistanceManager.getNaturalSpawnChunkCount()`：玩家周围 8 个区块（切比雪夫距离，
/// 也就是 17×17 的方形）里的区块个数，按区块去重。官方的追踪器从玩家所在区块出发、
/// 每跨一格（含对角）加一，所以是方形而不是菱形；它也只看玩家票，不看区块是否加载，
/// 旁观者仅在 `spectators_generate_chunks` 开启时贡献玩家票。
///
/// `DistanceManager.getNaturalSpawnChunkCount()`: the number of chunks within 8
/// chunks of a player, counted once per chunk. The vanilla tracker grows by one per
/// step in all eight directions, so the shape is a square, not a diamond. It counts
/// player tickets only, includes spectators only when they generate chunks, and
/// does not look at load state.
fn spawn_candidate_chunks(region: &Region) -> BTreeSet<ChunkPos> {
    let spectators_generate = region.global.rules.bool(SPECTATORS_GENERATE_CHUNKS);
    region
        .players
        .values()
        .filter(|player| spectators_generate || player.game_mode != GameMode::Spectator)
        .flat_map(|player| chunk_square(player.chunk_pos(), SPAWN_CHUNK_RADIUS))
        .collect()
}

fn in_chunk_square(center: ChunkPos, chunk: ChunkPos, radius: i32) -> bool {
    (center.x - chunk.x).abs().max((center.z - chunk.z).abs()) <= radius
}

/// 以 `center` 为中心、切比雪夫半径 `radius` 的区块，含对角。
///
/// The chunks within Chebyshev `radius` of `center`, diagonals included.
fn chunk_square(center: ChunkPos, radius: i32) -> impl Iterator<Item = ChunkPos> {
    (-radius..=radius).flat_map(move |dx| {
        (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
    })
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use super::*;
    use crate::{inbound::PlayerId, player::Player};

    fn region_with(mode: GameMode, at: Vec3) -> Region {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            mode,
            ClientInformation::default(),
            at,
        );
        region.players.insert(id, player);
        region
    }

    #[test]
    fn candidates_are_the_chunks_whose_centre_is_within_128_blocks() {
        let chunks = spawning_chunk_candidates([(Vec3::new(8.0, -60.0, 8.0), false)], |_| true);
        // 中心 (8, 8) 到 (8 + 16 * 8, 8) 正好 128 格，不算；7 个区块远的算。
        assert!(chunks.contains(&ChunkPos::new(7, 0)));
        assert!(!chunks.contains(&ChunkPos::new(8, 0)));
        assert!(chunks.contains(&ChunkPos::new(5, 5)));
        assert!(!chunks.contains(&ChunkPos::new(6, 6)));
        assert!(chunks.contains(&ChunkPos::new(0, 0)));
        assert!(!chunks.contains(&ChunkPos::new(9, 0)), "8 格方形以外不算");
    }

    #[test]
    fn spectators_and_non_ticking_chunks_add_no_candidates() {
        assert!(
            spawning_chunk_candidates([(Vec3::new(8.0, -60.0, 8.0), true)], |_| true).is_empty()
        );
        assert!(
            spawning_chunk_candidates([(Vec3::new(8.0, -60.0, 8.0), false)], |_| false).is_empty()
        );
    }

    /// 累加只记在单独的表里：区块版本号不变（区块包缓存不失效），模板区块不被物化，
    /// 也不进存盘集合。
    #[test]
    fn incrementing_inhabited_time_leaves_the_chunk_untouched() {
        let mut region = region_with(GameMode::Survival, Vec3::new(8.0, -60.0, 8.0));
        let chunk = ChunkPos::new(0, 0);
        let revision = region.chunks.revision(chunk);
        let _ = region.chunks.take_changed_chunks();
        for _ in 0..2 {
            for pos in inhabited_time_chunks(&region) {
                region.chunks.increment_inhabited_time(pos);
            }
        }
        assert_eq!(region.chunks.inhabited_time(chunk), 2);
        assert_eq!(region.chunks.revision(chunk), revision);
        assert!(region.chunks.is_template(chunk));
        assert!(region.chunks.take_changed_chunks().is_empty());
    }

    #[test]
    fn the_spawn_chunk_square_is_seventeen_by_seventeen() {
        let chunks: Vec<ChunkPos> = chunk_square(ChunkPos::new(0, 0), SPAWN_CHUNK_RADIUS).collect();
        // 官方的追踪器八方向传播，所以是 17×17 的方形，不是 8 格的菱形。
        // The vanilla tracker propagates in eight directions, so this is a 17 by 17
        // square, not an 8-step diamond.
        assert_eq!(chunks.len(), 289);
        assert!(chunks.contains(&ChunkPos::new(8, 8)));
        assert!(!chunks.contains(&ChunkPos::new(9, 0)));
    }
}
