//! 刷怪候选区块：官方 `ChunkMap.collectSpawningChunks`。
//!
//! 一个区块入选要同时满足：
//! - `DistanceManager.naturalSpawnChunkCounter` 里有它：离某个计入距离管理器的玩家所在区块
//!   切比雪夫距离不超过 8；
//! - `ChunkHolder.getTickingChunk()` 不为空：能跑方块逻辑；
//! - `anyPlayerCloseEnoughForSpawningInternal`：有一名不是旁观的玩家，区块中心
//!   `(x * 16 + 8, z * 16 + 8)` 到他的水平距离平方小于 16384。
//!
//! 第三条里的玩家一定也满足第一条（离区块中心不到 128 格，所在区块的切比雪夫距离就不超过 8），
//! 所以结果就是「能跑方块逻辑、且有非旁观玩家离中心不到 128 格」的区块。
//! 官方按哈希表的顺序收集再打乱；这里按坐标排序返回，`InhabitedTime` 的累加和顺序无关。
//! 以后做自然刷怪时，打乱用的世界随机数要在调用方照官方取。
//!
//! The natural-spawn candidate chunks of vanilla `ChunkMap.collectSpawningChunks`:
//! block-ticking chunks whose centre is within 128 blocks of a non-spectator player.

use std::collections::BTreeSet;

use mc_bamboo_core::{ChunkPos, Vec3};

use crate::region::Region;

/// `naturalSpawnChunkCounter` 的最大距离（区块）。
const SPAWN_CHUNK_RADIUS: i32 = 8;
/// `ChunkMap.playerIsCloseEnoughForSpawning` 的距离平方上限（128 格）。
const SPAWN_DISTANCE_SQR: f64 = 16_384.0;

/// 区块中心到 `(x, z)` 的水平距离平方，官方 `ChunkMap.euclideanDistanceSquared`。
fn distance_sqr(chunk: ChunkPos, x: f64, z: f64) -> f64 {
    let dx = f64::from(chunk.x * 16 + 8) - x;
    let dz = f64::from(chunk.z * 16 + 8) - z;
    dx * dx + dz * dz
}

/// 刷怪候选区块的纯计算：`players` 是（位置，是不是旁观），`ticks_blocks` 回答一个区块
/// 能不能跑方块逻辑（官方 `getTickingChunk() != null`）。按坐标升序返回。
/// 游戏和对拍用同一份。
///
/// The candidate computation shared by the game and the oracle, sorted by coordinate.
pub fn spawning_chunk_candidates(
    players: impl IntoIterator<Item = (Vec3, bool)>,
    ticks_blocks: impl Fn(ChunkPos) -> bool,
) -> Vec<ChunkPos> {
    let mut chunks = BTreeSet::new();
    for (position, spectator) in players {
        if spectator {
            continue;
        }
        let (x, z) = (position.x, position.z);
        let center = ChunkPos::containing(x, z);
        for dx in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
            for dz in -SPAWN_CHUNK_RADIUS..=SPAWN_CHUNK_RADIUS {
                let chunk = ChunkPos::new(center.x.saturating_add(dx), center.z.saturating_add(dz));
                if distance_sqr(chunk, x, z) < SPAWN_DISTANCE_SQR && ticks_blocks(chunk) {
                    chunks.insert(chunk);
                }
            }
        }
    }
    chunks.into_iter().collect()
}

/// 这一刻区域里的刷怪候选区块，按坐标升序。
///
/// This tick's natural-spawn candidate chunks in a region, sorted by coordinate.
pub(crate) fn spawning_chunks(region: &Region) -> Vec<ChunkPos> {
    let players = region.players.values().map(|player| {
        (
            player.position(),
            player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Spectator,
        )
    });
    spawning_chunk_candidates(players, |chunk| {
        region.tick_levels.chunk_ticks_blocks(chunk)
            && (!region.tick_levels.enabled() || region.chunks.is_loaded(chunk))
    })
}

/// `ServerChunkCache.tickSpawningChunk` 的第一句：每个候选区块的 `InhabitedTime` 加一。
/// 官方只在 `TickRateManager.runsNormally()` 时走到这里；Bamboo 没有暂停 / 冻结刻，恒为真。
///
/// Adds one tick of `InhabitedTime` to every candidate chunk.
pub(crate) fn increment_inhabited_time(region: &mut Region) {
    for chunk in spawning_chunks(region) {
        region.chunks.increment_inhabited_time(chunk);
    }
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
        let region = region_with(GameMode::Survival, Vec3::new(8.0, -60.0, 8.0));
        let chunks = spawning_chunks(&region);
        // 中心 (8, 8) 到 (8 + 16 * 8, 8) 正好 128 格，不算；7 个区块远的算。
        assert!(chunks.contains(&ChunkPos::new(7, 0)));
        assert!(!chunks.contains(&ChunkPos::new(8, 0)));
        assert!(chunks.contains(&ChunkPos::new(5, 5)));
        assert!(!chunks.contains(&ChunkPos::new(6, 6)));
        assert!(chunks.contains(&ChunkPos::new(0, 0)));
    }

    #[test]
    fn spectators_add_no_candidates() {
        let region = region_with(GameMode::Spectator, Vec3::new(8.0, -60.0, 8.0));
        assert!(spawning_chunks(&region).is_empty());
    }

    /// 累加只记在单独的表里：区块版本号不变（区块包缓存不失效），模板区块不被物化，
    /// 也不进存盘集合。
    #[test]
    fn incrementing_inhabited_time_leaves_the_chunk_untouched() {
        let mut region = region_with(GameMode::Survival, Vec3::new(8.0, -60.0, 8.0));
        let chunk = ChunkPos::new(0, 0);
        let revision = region.chunks.revision(chunk);
        let _ = region.chunks.take_changed_chunks();
        increment_inhabited_time(&mut region);
        increment_inhabited_time(&mut region);
        assert_eq!(region.chunks.inhabited_time(chunk), 2);
        assert_eq!(region.chunks.revision(chunk), revision);
        assert!(region.chunks.is_template(chunk));
        assert!(region.chunks.take_changed_chunks().is_empty());
    }
}
