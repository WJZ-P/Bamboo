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
    for chunk in chunks {
        if !region.tick_levels.chunk_ticks_entities(chunk) {
            continue;
        }
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        natural::spawn_for_chunk(&mut world, chunk, &mut state, &params, &categories);
        let effects = world.into_effects();
        crate::interaction::send_effects(region, None, &effects);
    }
}

/// `ChunkGenerator.getSeaLevel()`：超平坦是 `FlatLevelSource` 的 -63，噪声世界读设置。
/// 设置要解析整份世界生成数据，所以只算一次；三个噪声预设的值相同（63）。
///
/// `ChunkGenerator.getSeaLevel()`: -63 for `FlatLevelSource`, the settings value for noise
/// worlds. Reading the settings parses the worldgen data, so it is computed once; the
/// three noise presets share the value (63).
fn sea_level(region: &Region) -> i32 {
    static NOISE: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let level_type = region.global.world_gen.level_type;
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
    use super::*;

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
