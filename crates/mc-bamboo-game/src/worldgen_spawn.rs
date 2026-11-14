//! 新世界的出生点选择（官方 `MinecraftServer.setInitialSpawn` 的简化移植）。
//!
//! 官方 26.3 的调用链：`getGenerator().getOrigin(randomState)`（`NoiseBasedChunkGenerator`
//! 走 `NoiseSpawnFinder` 的螺旋气候搜索）→ 先给一个 `(spawnChunk*16+8, 64, +8)` →
//! 再在 11×11 区块螺旋里调 `PlayerSpawnFinder.getSpawnPosInChunk`，用高度图、
//! 碰撞形状和流体状态找第一个能站的方块。本轮是**故意的简化**，见 README 已知限制：
//! 没有 `NoiseSpawnFinder` 的气候螺旋（从原点区块起找）、用 quart 群系粗略排除海洋、
//! 没有 11×11 区块的碰撞盒/高度图三重判定、没有奖励箱。11×11 的螺旋顺序照官方。
//!
//! Fresh-world spawn selection (a simplified port of vanilla
//! `MinecraftServer.setInitialSpawn`). Vanilla 26.3: `getOrigin(randomState)`
//! (a `NoiseSpawnFinder` climate spiral) → a provisional
//! `(spawnChunk*16+8, 64, +8)` → an 11×11 chunk spiral calling
//! `PlayerSpawnFinder.getSpawnPosInChunk` with heightmaps, collision shapes and
//! fluid checks. This is a **deliberate simplification** (see README known
//! limitations): no `NoiseSpawnFinder` climate spiral (start at the origin
//! chunk), quart biomes used to roughly skip oceans, no collision/heightmap
//! triple check, no bonus chest. The 11×11 spiral order follows vanilla.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, facts, registries, state_facts};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};

use crate::{world_options::WorldGenConfig, world_state::RespawnData};

/// 表面附近（方块 y≈64）的 quart 段号：`(64-(-64))/4/4 = 8`。
/// The quart section near the surface (block y≈64): `(64-(-64))/4/4 = 8`.
const SURFACE_SECTION: usize = 8;
/// 段内 `(quartX=2, quartY=0, quartZ=2)` 在 `Generator::biomes` 压入顺序里的下标
/// `qx*16 + qy*4 + qz = 34`，对应区块中心 (8, 8)。
/// Index of `(quartX=2, quartY=0, quartZ=2)` in `Generator::biomes`' push order
/// `qx*16 + qy*4 + qz = 34`, i.e. the chunk centre (8, 8).
const CENTER_CELL: usize = 34;
/// 官方 `setInitialSpawn` 的螺旋半径（11×11 区块）。
/// Official `setInitialSpawn` spiral radius (11×11 chunks).
const SPIRAL_RADIUS: i32 = 5;

/// 全新非 flat 世界的出生点；flat 世界返回 `None`，由调用方保留默认超平坦出生点。
///
/// Spawn for a fresh non-flat world; `None` for flat worlds (caller keeps the
/// default superflat spawn).
pub(crate) fn initial_spawn(config: &WorldGenConfig) -> Option<RespawnData> {
    if config.level_type.is_flat() {
        return None;
    }
    // 先按官方螺旋顺序、用便宜的 quart 群系找一个非海洋区块；再只为那一个区块
    // 跑完整地形。整片 11×11 都是海洋时回退到原点区块。
    // Walk the official spiral with cheap quart biomes to find a non-ocean
    // chunk, then run full terrain only for that one chunk. Fall back to the
    // origin chunk if the whole 11×11 is ocean.
    let mut chosen = None;
    for (dx, dz) in spawn_spiral() {
        let Ok(generated) = crate::worldgen::generate_biomes(config, dx, dz) else {
            continue;
        };
        let center = generated
            .biomes
            .get(SURFACE_SECTION)
            .and_then(|cells| cells.get(CENTER_CELL))
            .copied()
            .unwrap_or(0);
        if !is_ocean(center) {
            chosen = Some((dx, dz));
            break;
        }
    }
    let (dx, dz) = chosen.unwrap_or((0, 0));
    // 选中的区块里找一块不淹水的地表；找不到再看原点区块。
    // Find a dry surface in the chosen chunk; if none, try the origin chunk.
    for (cx, cz) in [(dx, dz), (0, 0)] {
        let Ok(chunk) = crate::worldgen::generate_chunk(config, cx, cz) else {
            continue;
        };
        if let Some(spawn) = dry_spawn(&chunk.blocks, cx, cz) {
            return Some(spawn);
        }
    }
    None
}

/// 一个区块里挑出生点：从中心往外，优先顶部不是水/岩浆的柱子；全是液体时退而求其次
/// 用最高的柱子（玩家落在水上而不是埋在石头里）。
///
/// Pick a spawn in a chunk: from the centre outward, prefer a column whose top
/// is not water/lava; if every column is liquid, fall back to the tallest one
/// (the player lands on water rather than being buried in stone).
fn dry_spawn(blocks_layers: &[Vec<u32>], chunk_x: i32, chunk_z: i32) -> Option<RespawnData> {
    let mut columns: Vec<(usize, usize)> =
        (0..16).flat_map(|x| (0..16).map(move |z| (x, z))).collect();
    columns.sort_by_key(|&(x, z)| {
        let dx = x as i32 - 8;
        let dz = z as i32 - 8;
        dx * dx + dz * dz
    });
    let mut fallback = None;
    for (x, z) in columns {
        let top = (MIN_Y..MIN_Y + WORLD_HEIGHT).rev().find_map(|y| {
            let state = blocks_layers[(y - MIN_Y) as usize][x * 16 + z];
            (state != blocks::AIR).then_some((y, state))
        });
        let Some((y, state)) = top else {
            continue;
        };
        let pos = BlockPos::new(chunk_x * 16 + x as i32, y + 1, chunk_z * 16 + z as i32);
        let respawn = RespawnData::of(pos, 0.0, 0.0);
        // `state_facts & LIQUID`：水/岩浆柱不是理想出生点。
        // `state_facts & LIQUID`: water/lava columns are not ideal spawn points.
        if state_facts(state) & facts::LIQUID == 0 {
            return Some(respawn);
        }
        fallback.get_or_insert(respawn);
    }
    fallback
}

/// 群系名字里带 `ocean` 的当成海洋（ocean / deep_ocean / frozen_ocean / ...）。
/// Biomes whose name contains `ocean` count as ocean.
fn is_ocean(id: u32) -> bool {
    registries::biome::NAMES
        .get(id as usize)
        .is_some_and(|name| name.contains("ocean"))
}

/// 官方 `setInitialSpawn` 的方形螺旋：从 (0,0) 出发，最多覆盖半径 [`SPIRAL_RADIUS`]。
/// Official `setInitialSpawn` square spiral from (0,0), covering radius
/// [`SPIRAL_RADIUS`].
fn spawn_spiral() -> Vec<(i32, i32)> {
    let mut result = Vec::new();
    let (mut x, mut z) = (0_i32, 0_i32);
    let (mut dx, mut dz) = (0_i32, -1_i32);
    for _ in 0..(SPIRAL_RADIUS * 2 + 1).pow(2) {
        if (-SPIRAL_RADIUS..=SPIRAL_RADIUS).contains(&x)
            && (-SPIRAL_RADIUS..=SPIRAL_RADIUS).contains(&z)
        {
            result.push((x, z));
        }
        if x == z || (x < 0 && x == -z) || (x > 0 && x == 1 - z) {
            (dx, dz) = (-dz, dx);
        }
        x += dx;
        z += dz;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::LevelType;

    #[test]
    fn flat_world_keeps_the_default_spawn() {
        assert!(initial_spawn(&WorldGenConfig::default()).is_none());
    }

    #[test]
    fn spiral_starts_at_origin_and_covers_the_square() {
        let offsets = spawn_spiral();
        assert_eq!(offsets[0], (0, 0));
        assert_eq!(offsets.len(), (SPIRAL_RADIUS * 2 + 1).pow(2) as usize);
        for &(x, z) in &offsets {
            assert!((-SPIRAL_RADIUS..=SPIRAL_RADIUS).contains(&x));
            assert!((-SPIRAL_RADIUS..=SPIRAL_RADIUS).contains(&z));
        }
    }

    #[test]
    fn normal_world_finds_a_surface_near_the_origin() {
        let config = WorldGenConfig {
            seed: 12345,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let spawn = initial_spawn(&config).expect("a fresh normal world has a spawn");
        // 出生点在世界高度范围内，且不再是超平坦模板那个 y=0 的默认点。
        // Spawn is within world height and no longer the flat template's y=0 default.
        assert!((MIN_Y + 1..=MIN_Y + WORLD_HEIGHT).contains(&spawn.pos.y));
        assert_ne!(spawn.pos.y, 0);
        assert_eq!(spawn.yaw, 0.0);
        assert_eq!(spawn.pitch, 0.0);
    }
}
