//! 全新噪声世界的出生点，照官方 `MinecraftServer.setInitialSpawn` 的整条链。
//!
//! 26.3 的链：`chunkSource.getGenerator().getOrigin(randomState)`（主世界走
//! `NoiseBasedChunkGenerator` → `NoiseSpawnFinder` 的气候螺旋，见
//! `mc_bamboo_worldgen::spawn`）→ 先给一个 `(spawnChunk*16+8, 64, +8)` 的暂定出生点 →
//! 在 11×11 区块螺旋里调 `PlayerSpawnFinder.getSpawnPosInChunk`，用
//! `MOTION_BLOCKING` / `WORLD_SURFACE` / `OCEAN_FLOOR` 三张高度图、流体状态和碰撞形状
//! 找第一个能站的方块 → 找到就覆盖暂定点。`getSpawnHeight` 是基类默认的 64，
//! `generateBonusChest()` 对专用服务端默认 false，所以本轮不放奖励箱。
//!
//! 对拍只比较到 `terrain`（CARVERS）的地形：地物/结构还没接进游戏，见 README 已知限制。
//! `fixupSpawnHeight` 是 `PlayerSpawnFinder.findSpawn`（玩家进服/重生搜索）的兜底，
//! `setInitialSpawn` 本身不调它；这里只在 11×11 整片找不到落点这一罕见情形用到。
//!
//! Fresh noise-world spawn, following vanilla `MinecraftServer.setInitialSpawn`.
//! Chain: `getGenerator().getOrigin(randomState)` (overworld → `NoiseBasedChunkGenerator`
//! → `NoiseSpawnFinder` climate spiral, see `mc_bamboo_worldgen::spawn`) → a provisional
//! `(spawnChunk*16+8, 64, +8)` → an 11×11 chunk spiral calling
//! `PlayerSpawnFinder.getSpawnPosInChunk` with the `MOTION_BLOCKING` / `WORLD_SURFACE`
//! / `OCEAN_FLOOR` heightmaps, fluid state and collision shapes → the first standable
//! block overrides the provisional. `getSpawnHeight` is the base default 64 and
//! `generateBonusChest()` defaults to false on a dedicated server, so no bonus chest
//! is placed. The oracle compares only `terrain` (CARVERS) blocks; features /
//! structures are not wired yet (README known limitations). `fixupSpawnHeight`
//! belongs to `PlayerSpawnFinder.findSpawn` (join/respawn search), not
//! `setInitialSpawn`; here it is used only in the rare all-ocean fallback.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, facts, registries, state_facts, state_fluid};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};

use crate::{world_options::WorldGenConfig, world_state::RespawnData};

/// 官方 `setInitialSpawn` 的方形螺旋半径（11×11 区块）。
/// Official `setInitialSpawn` square spiral radius (11×11 chunks).
const SPIRAL_RADIUS: i32 = 5;

/// 一次出生点选择的中间结果，供 oracle 分别核对出生区块和最终出生点。
/// Intermediate result of one spawn selection, so the oracle can check the
/// spawn chunk and the final spawn position separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnSelection {
    pub(crate) origin: (i32, i32),
    pub(crate) pos: BlockPos,
}

/// 全新非 flat 世界的出生点；flat 返回 `None`，调用方保留默认超平坦出生点。
/// Spawn for a fresh non-flat world; `None` for flat (caller keeps the default).
pub(crate) fn initial_spawn(config: &WorldGenConfig) -> Option<RespawnData> {
    select_spawn(config).map(|selection| RespawnData::of(selection.pos, 0.0, 0.0))
}

/// `MinecraftServer.setInitialSpawn` 的完整计算，返回出生区块和最终出生点。
/// The full `MinecraftServer.setInitialSpawn` computation, returning the spawn
/// chunk and the final spawn position.
pub(crate) fn select_spawn(config: &WorldGenConfig) -> Option<SpawnSelection> {
    if config.level_type.is_flat() {
        return None;
    }
    let settings_id = config.level_type.settings_id()?;
    let worldgen = mc_bamboo_worldgen::Worldgen::shared().ok()?;
    let origin =
        mc_bamboo_worldgen::spawn::spawn_origin(worldgen, settings_id, config.seed).ok()?;
    let (cx, cz) = origin;

    // 官方先给一个暂定出生点；`getSpawnHeight` 是基类默认 64（>= minY，不触发
    // WORLD_SURFACE 兜底）。`spawnChunk.getWorldPosition().offset(8, 64, 8)`。
    // Vanilla sets a provisional spawn first; `getSpawnHeight` is the base default
    // 64 (>= minY, so the WORLD_SURFACE fallback is not taken).
    let provisional = BlockPos::new(cx * 16 + 8, 64, cz * 16 + 8);

    // 11×11 螺旋里第一个能站的方块覆盖暂定点；整片找不到就用 fixup 后的暂定点。
    // The first standable block in the 11×11 spiral overrides the provisional;
    // if the whole area fails, fall back to the fixup'd provisional.
    for (dx, dz) in spawn_spiral() {
        if let Some(pos) = get_spawn_pos_in_chunk(config, cx + dx, cz + dz) {
            return Some(SpawnSelection { origin, pos });
        }
    }
    let pos = fixup_spawn_height(config, provisional);
    Some(SpawnSelection { origin, pos })
}

/// `isAir`：空气、洞穴空气、虚空空气。
/// `isAir`: air, cave air and void air.
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `FluidState.isEmpty()`：只有空流体才算没有流体。
/// `FluidState.isEmpty()`: only the empty fluid counts as "no fluid".
fn fluid_is_empty(state: u32) -> bool {
    state_fluid(state).fluid == registries::fluid::EMPTY
}

/// `Heightmap.Types.MOTION_BLOCKING.isOpaque`。
/// `MOTION_BLOCKING` heightmap predicate.
fn motion_blocking(state: u32) -> bool {
    state_facts(state) & facts::MOTION_BLOCKING != 0 || !fluid_is_empty(state)
}

/// `Heightmap.Types.OCEAN_FLOOR.isOpaque`（`MATERIAL_MOTION_BLOCKING`，水不算）。
/// `OCEAN_FLOOR` heightmap predicate (`MATERIAL_MOTION_BLOCKING`, no fluids).
fn ocean_floor(state: u32) -> bool {
    state_facts(state) & facts::MOTION_BLOCKING != 0
}

/// `Block.isFaceFull(getCollisionShape(...), UP)`：CARVERS 地形的固体都是完整方块。
/// `Block.isFaceFull(..., UP)`: CARVERS terrain solids are all full cubes.
fn is_face_full_up(state: u32) -> bool {
    state_facts(state) & facts::COLLISION_FULL_BLOCK != 0
}

/// 一列里最高的、满足 `opaque` 的方块 y（等价 `chunk.getHeight(...)` = getHighestTaken）；
/// 整列没有时返回 `MIN_Y - 1`。
/// Topmost y satisfying `opaque` in a column (equals `getHeight`/`getHighestTaken`);
/// `MIN_Y - 1` when the whole column is empty.
fn topmost(blocks: &[Vec<u32>], index: usize, opaque: fn(u32) -> bool) -> i32 {
    for y in (MIN_Y..MIN_Y + WORLD_HEIGHT).rev() {
        if opaque(blocks[(y - MIN_Y) as usize][index]) {
            return y;
        }
    }
    MIN_Y - 1
}

/// `PlayerSpawnFinder.getLevelRespawnPos` 对某一列的判定。
/// `PlayerSpawnFinder.getLevelRespawnPos` for one column.
fn get_level_respawn_pos(
    blocks: &[Vec<u32>],
    world_x: i32,
    world_z: i32,
    index: usize,
) -> Option<BlockPos> {
    let state_at = |y: i32| -> u32 {
        if (MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            blocks[(y - MIN_Y) as usize][index]
        } else {
            blocks::AIR
        }
    };
    let top_y = topmost(blocks, index, motion_blocking);
    let surface = topmost(blocks, index, |s| !is_air(s));
    let floor = topmost(blocks, index, ocean_floor);
    if top_y < MIN_Y {
        return None;
    }
    // 水面高于海底（surface > ocean_floor）时这一列站不住，直接排除。
    // A column whose surface sits above its ocean floor (a liquid column) cannot
    // hold a spawn; reject it.
    if surface <= top_y && surface > floor {
        return None;
    }
    // 从 topY+1 往下：先撞到流体就放弃，先撞到满碰撞顶面就站在它上面。
    // From topY+1 down: a fluid aborts; the first full collision top face wins.
    let mut y = top_y + 1;
    while y >= MIN_Y {
        let state = state_at(y);
        if !fluid_is_empty(state) {
            return None;
        }
        if is_face_full_up(state) {
            return Some(BlockPos::new(world_x, y + 1, world_z));
        }
        y -= 1;
    }
    None
}

/// `PlayerSpawnFinder.getSpawnPosInChunk`：区块内按 x 外、z 内递增找第一个能站的列。
/// `PlayerSpawnFinder.getSpawnPosInChunk`: scan columns x-major, z-minor, return
/// the first standable one.
fn get_spawn_pos_in_chunk(config: &WorldGenConfig, cx: i32, cz: i32) -> Option<BlockPos> {
    let chunk = crate::worldgen::generate_chunk(config, cx, cz).ok()?;
    for local_x in 0..16_i32 {
        for local_z in 0..16_i32 {
            let index = (local_x * 16 + local_z) as usize;
            if let Some(pos) =
                get_level_respawn_pos(&chunk.blocks, cx * 16 + local_x, cz * 16 + local_z, index)
            {
                return Some(pos);
            }
        }
    }
    None
}

/// `PlayerSpawnFinder.fixupSpawnHeight`：从暂定点往上找到能站的位置，再贴回地面。
/// 对正常的螺旋结果（脚下是满实心方块、头顶是空气）是恒等变换，只在整片找不到
/// 落点时才用来兜底。`noCollisionNoLiquid` 用 CARVERS 地形近似：当前格和上一格
/// 都不是流体、当前格没有满碰撞形状。
///
/// `PlayerSpawnFinder.fixupSpawnHeight`: move up to a standable position, then
/// settle back onto the ground. It is the identity for a normal spiral result
/// (full solid below, air above) and is only used for the rare fallback.
/// `noCollisionNoLiquid` is approximated on CARVERS terrain: the feet block and
/// the block above are not fluid and the feet block has no full collision shape.
fn fixup_spawn_height(config: &WorldGenConfig, start: BlockPos) -> BlockPos {
    let chunk = match crate::worldgen::generate_chunk(config, start.x >> 4, start.z >> 4) {
        Ok(chunk) => chunk,
        Err(_) => return start,
    };
    let index = (start.x & 15) as usize * 16 + (start.z & 15) as usize;
    let state_at = |y: i32| -> u32 {
        if (MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            chunk.blocks[(y - MIN_Y) as usize][index]
        } else {
            blocks::AIR
        }
    };
    // 近似 `noCollisionNoLiquid(pos)`：脚下方块和头顶方块都没有碰撞/流体。
    // Approximate `noCollisionNoLiquid(pos)`: neither the feet nor the head block
    // has a collision shape or a fluid.
    let standable = |y: i32| -> bool {
        let feet = state_at(y);
        let head = state_at(y + 1);
        fluid_is_empty(feet)
            && fluid_is_empty(head)
            && !is_face_full_up(feet)
            && !is_face_full_up(head)
    };
    let max_y = MIN_Y + WORLD_HEIGHT - 1;
    let mut y = start.y;
    while !standable(y) && y < max_y {
        y += 1;
    }
    y -= 1;
    while standable(y) && y > MIN_Y {
        y -= 1;
    }
    BlockPos::new(start.x, y + 1, start.z)
}

/// 官方 `setInitialSpawn` 的方形螺旋：从 (0,0) 出发，最多覆盖半径 [`SPIRAL_RADIUS`]，
/// 共 `Mth.square(11) = 121` 个偏移。
/// Official `setInitialSpawn` square spiral from (0,0), radius [`SPIRAL_RADIUS`],
/// `Mth.square(11) = 121` offsets.
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

    fn normal(seed: i64) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        }
    }

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
        let config = normal(12345);
        let spawn = initial_spawn(&config).expect("a fresh normal world has a spawn");
        // 出生点在世界高度范围内，且不再是超平坦模板那个 y=0 的默认点。
        // Spawn is within world height and no longer the flat template's y=0 default.
        assert!((MIN_Y + 1..=MIN_Y + WORLD_HEIGHT).contains(&spawn.pos.y));
        assert_ne!(spawn.pos.y, 0);
        assert_eq!(spawn.yaw, 0.0);
        assert_eq!(spawn.pitch, 0.0);
    }

    #[test]
    fn solid_terrain_blocks_are_face_full_and_fluids_are_not() {
        // CARVERS 地形的固体是完整方块，水/空气不是（决定 getLevelRespawnPos）。
        // CARVERS terrain solids are full cubes; water/air are not.
        for &solid in &[
            blocks::GRASS_BLOCK,
            blocks::STONE,
            blocks::SAND,
            blocks::DIRT,
        ] {
            assert!(is_face_full_up(solid), "{solid}");
            assert!(fluid_is_empty(solid), "{solid}");
        }
        assert!(!is_face_full_up(blocks::WATER));
        assert!(!fluid_is_empty(blocks::WATER));
        assert!(!is_face_full_up(blocks::AIR));
    }
}
