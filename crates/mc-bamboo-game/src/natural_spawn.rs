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

use mc_bamboo_protocol::packet::play::GameMode;

use crate::region::Region;

/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`：`getNaturalSpawnChunkCount` 只算玩家周围 8 个
/// 区块内（曼哈顿距离）的已加载区块。
///
/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`: `getNaturalSpawnChunkCount` only counts loaded
/// chunks within 8 chunks (Manhattan) of a player.
const SPAWN_CHUNK_RADIUS: i32 = 8;

/// 随机刻之外的自然生成。`spawn_mobs` 关掉时整段不做，一个随机数都不取。
///
/// Natural spawning beside the random ticks. With `spawn_mobs` off nothing runs and no
/// random number is drawn.
pub(crate) fn tick(region: &mut Region) {
    if !region.global.block_rules.spawn_mobs {
        return;
    }
    let spawn_enemies = region.global.block_rules.spawn_monsters;
    let spawnable_chunk_count = spawnable_chunk_count(region);
    if spawnable_chunk_count == 0 {
        return;
    }
    let params = SpawnParams {
        spawnable_chunk_count,
        spawn_mobs: true,
        spawn_enemies,
        per_player_cap: region.spawn_per_player_cap,
        respawn_pos: Some(region.global.spawn.pos),
    };
    // `createState` 读这一刻的实体表和玩家表，两样都借自 region。
    let mut state = {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        SpawnState::create(&world, spawnable_chunk_count)
    };
    let spawn_persistent = region.tick.game_time() % 400 == 0;
    let categories = state.filtered_categories(spawn_enemies, spawn_persistent);
    if categories.is_empty() {
        return;
    }
    let mut chunks = spawning_chunks(region);
    if chunks.is_empty() {
        return;
    }
    {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        natural::shuffle(&mut chunks, world.random());
    }
    for chunk in chunks {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        natural::spawn_for_chunk(&mut world, chunk, &mut state, &params, &categories);
        let effects = world.into_effects();
        crate::interaction::send_effects(region, None, &effects);
    }
}

/// `chunkMap.collectSpawningChunks`：能 tick 实体的区块。无限超平坦模式没有等级，
/// 按每名玩家模拟距离内的方形收集（和随机刻的兜底一致）。
///
/// `chunkMap.collectSpawningChunks`: the entity-ticking chunks. Infinite superflat has
/// no levels, so the player squares inside the simulation distance are used, as the
/// random-tick fallback does.
fn spawning_chunks(region: &Region) -> Vec<ChunkPos> {
    if region.tick_levels.enabled() {
        let mut chunks: Vec<ChunkPos> = region
            .tick_levels
            .entity_chunks()
            .into_iter()
            .filter(|chunk| region.chunks.is_loaded(*chunk))
            .collect();
        chunks.sort();
        return chunks;
    }
    let radius = crate::interaction::SIMULATION_DISTANCE.min(region.view_distance + 1);
    let mut chunks = BTreeSet::new();
    for player in region.players.values() {
        let center = player.chunk_pos();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                chunks.insert(ChunkPos::new(center.x + dx, center.z + dz));
            }
        }
    }
    chunks.into_iter().collect()
}

/// `DistanceManager.getNaturalSpawnChunkCount()`：玩家周围 8 个区块（曼哈顿距离）里
/// 已加载的区块个数，按区块去重。
///
/// `DistanceManager.getNaturalSpawnChunkCount()`: the number of loaded chunks within
/// 8 chunks (Manhattan) of a player, counted once per chunk.
fn spawnable_chunk_count(region: &Region) -> i32 {
    let mut chunks = BTreeSet::new();
    for player in region.players.values() {
        if player.game_mode == GameMode::Spectator {
            continue;
        }
        let center = player.chunk_pos();
        for dx in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
            for dz in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                if dx.abs() + dz.abs() > SPAWN_CHUNK_RADIUS {
                    continue;
                }
                let chunk = ChunkPos::new(center.x + dx, center.z + dz);
                if region.chunks.is_loaded(chunk) {
                    chunks.insert(chunk);
                }
            }
        }
    }
    chunks.len() as i32
}
