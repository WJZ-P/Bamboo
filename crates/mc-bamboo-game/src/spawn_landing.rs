//! 新玩家进服与无床 / 锚重生的落脚点，照官方 `PlayerSpawnFinder.findSpawn`。
//!
//! 26.3 的两条链共用一个搜索：进服是 `PrepareSpawnTask.start` →
//! `PlayerSpawnFinder.findSpawn(level, worldRespawn.pos)`；重生若无可用个人出生点，
//! 是 `PlayerList.respawn` → `TeleportTransition.missingRespawnBlock` /
//! `createDefault` → `findAdjustedSharedSpawnPos` → `ServerPlayer.adjustSpawnLocation`
//! → 同一个 `findSpawn`。`/spawnpoint` 的强制个人点在方块允许时直接落地，
//! 不经过搜索（见 [`crate::player_respawn`]）。
//!
//! 搜索本身：`getGameType() == ADVENTURE` 直接 `fixupSpawnHeight`；否则半径取
//! `max(0, respawn_radius)` 与世界边界距离 `floor(getDistanceToBorder)` 的较小值，
//! 距离不大于 1 时半径为 1。候选数 `min(1024, (2r+1)^2)`，步长是和候选数互素的
//! `getCoprime`，起点偏移由 `RandomSource.createThreadLocalInstance().nextInt(count)`
//! 给出；依次同步加载候选区块并调 `getLevelRespawnPos` + `noCollisionNoLiquid`，
//! 第一个通过的列 `Vec3.atBottomCenterOf` 就是落点，全部失败再 `fixupSpawnHeight`。
//!
//! 区块没就绪时本模块返回 `NotReady`，调用方等现有区块票把搜索覆盖的区块加载完，
//! 再重新跑同一个搜索；不在这里同步生成区块、也不拿兜底位置当最终落点。
//!
//! Join and bed/anchor-free respawn landings, following vanilla
//! `PlayerSpawnFinder.findSpawn`. Both paths share the same search: join uses
//! `PrepareSpawnTask.start`, while a respawn without a usable personal spawn
//! reaches it through `TeleportTransition.missingRespawnBlock` /
//! `createDefault` → `findAdjustedSharedSpawnPos` → `adjustSpawnLocation`.
//! A forced `/spawnpoint` lands directly when its two blocks allow respawning
//! (see [`crate::player_respawn`]).
//!
//! The search: `ADVENTURE` goes straight to `fixupSpawnHeight`; otherwise the
//! radius is `min(max(0, respawn_radius), floor(distanceToBorder))`, with a
//! minimum of 1 when the border distance is at most 1. `candidateCount` is
//! `min(1024, (2r+1)^2)`, the stride is `getCoprime`, and the start offset is
//! `RandomSource.createThreadLocalInstance().nextInt(count)`. Each candidate
//! column goes through `getLevelRespawnPos` + `noCollisionNoLiquid`; the first
//! success yields `Vec3.atBottomCenterOf`, and `fixupSpawnHeight` is the
//! all-candidates-failed fallback.
//!
//! An unready target chunk yields `NotReady`; the caller waits for the existing
//! chunk tickets to deliver the covered chunks and then reruns this same
//! search. No synchronous generation happens here, and the fallback is never
//! used as the final landing for an unready chunk.

use mc_bamboo_blocks::entity::JavaRandom;
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{blocks, collision_shape, facts, registries, state_facts, state_fluid};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, chunk::HeightmapKind, store::ChunkStore};

use crate::{gamerules::GameRuleSet, world_options::LevelType, world_state::RespawnData};

/// 官方 `PlayerSpawnFinder` 的候选数上限。
/// The official `PlayerSpawnFinder` candidate cap.
const MAX_CANDIDATES: i32 = 1024;

/// 一次搜索的输入。
/// Inputs of one search.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Search<'a> {
    pub(crate) chunks: &'a ChunkStore,
    pub(crate) rules: &'a GameRuleSet,
    /// `WorldData.isFlatWorld()`；login / respawn 包也用它。
    /// `WorldData.isFlatWorld()`; also used by the login / respawn packets.
    pub(crate) level_type: LevelType,
    pub(crate) border: &'a mc_bamboo_blocks::WorldBorder,
    pub(crate) spawn: RespawnData,
    /// `WorldData.getGameType()`：冒险模式走 fixup 分支。
    /// `WorldData.getGameType()`: adventure goes through the fixup branch.
    pub(crate) default_game_mode: mc_bamboo_protocol::packet::play::GameMode,
}

impl Search<'_> {
    /// `ServerLevel.isFlat()` 与 `ChunkGenerator.getSeaLevel()` 的当前值。
    /// Current values of `ServerLevel.isFlat()` and `ChunkGenerator.getSeaLevel()`.
    pub(crate) fn level_flags(&self) -> (bool, i32) {
        if self.level_type.is_flat() {
            (true, mc_bamboo_world::flat::SEA_LEVEL)
        } else {
            // flat 的 `FlatLevelSource.getSeaLevel()` 是 -63；三个噪声预设的
            // `NoiseGeneratorSettings.seaLevel()` 都是 63。
            // `FlatLevelSource.getSeaLevel()` is -63; all three noise presets
            // have `NoiseGeneratorSettings.seaLevel() == 63`.
            (false, noise_sea_level(self.level_type))
        }
    }
}

/// `NoiseBasedChunkGenerator.getSeaLevel()`：设置里的 `sea_level`。
/// `NoiseBasedChunkGenerator.getSeaLevel()`: the settings' `sea_level`.
pub(crate) fn noise_sea_level(level_type: LevelType) -> i32 {
    let Some(id) = level_type.settings_id() else {
        return mc_bamboo_world::flat::SEA_LEVEL;
    };
    mc_bamboo_worldgen::Worldgen::shared()
        .ok()
        .and_then(|worldgen| worldgen.settings(id).map(|settings| settings.sea_level))
        .unwrap_or(63)
}

/// 搜索还没法完成：候选区块没加载。调用方要等区块就绪后重试。
/// The search cannot finish yet: a candidate chunk is not loaded. The caller
/// must wait for chunk readiness and retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotReady;

/// 等待区块就绪时挂在玩家上的状态。`join` 还没发 `player_position`，
/// `respawn` 已经发过重生包，就绪后补一次 `player_position`。
///
/// Per-player state while chunks are pending. `join` has not sent
/// `player_position` yet; `respawn` already sent the respawn sequence and
/// completes it with one `player_position` once ready.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PendingLanding {
    pub(crate) join: bool,
    /// 重生的个人点（进服恒为 `None`）；等待期间固定，避免规则/命令改了它。
    /// The respawn's personal spawn (always `None` on join); fixed while
    /// waiting so a later command cannot change it.
    pub(crate) personal: Option<RespawnData>,
    /// 已等待的相对 tick 数，超时后照已加载的区块算一次。
    /// Relative ticks waited; after the timeout compute with loaded chunks.
    pub(crate) waited: i32,
}

impl PendingLanding {
    pub(crate) fn join() -> Self {
        Self {
            join: true,
            personal: None,
            waited: 0,
        }
    }

    pub(crate) fn respawn(personal: Option<RespawnData>) -> Self {
        Self {
            join: false,
            personal,
            waited: 0,
        }
    }
}

/// 一次搜索的结果，供对拍逐项比较。
/// One search's result, so the oracle can compare every item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Landing {
    pub(crate) position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// 命中的候选列；`fixupSpawnHeight` 兜底时是 `None`。
    /// The winning candidate column; `None` for the `fixupSpawnHeight` fallback.
    pub(crate) candidate: Option<BlockPos>,
    /// 官方 `PlayerSpawnFinder` 实际用过的半径。
    /// The radius actually used by vanilla `PlayerSpawnFinder`.
    pub(crate) radius: i32,
    /// 候选序列的起点偏移（官方 `offset`）。
    /// Start offset of the candidate walk (vanilla `offset`).
    pub(crate) offset: i32,
    /// 完成时的随机源内部状态（对拍用）。
    /// Internal random state on completion (for the oracle).
    pub(crate) random_state: i64,
}

impl Landing {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn position_tuple(self) -> (f64, f64, f64) {
        (self.position.x, self.position.y, self.position.z)
    }
}

/// 对拍用：按官方 `PlayerSpawnFinder` 的构造顺序重放一次候选序列，
/// 返回 `(radius, offset, 随机源末态)`。冒险 / 强制个人点分支本身不取随机数，
/// 但 Java 对拍仍用这个口径报告这几个量。
/// Oracle helper: replay the official candidate walk's RNG draws and return
/// `(radius, offset, final RNG state)`. Adventure / forced personal branches
/// do not draw, but the Java oracle reports these values with the same seed.
#[cfg(feature = "oracle-samples")]
pub(crate) fn oracle_candidate_walk(search: &Search<'_>, seed: i64) -> (i32, i32, i64) {
    let wanted = search
        .rules
        .int(mc_bamboo_data::gamerules::RESPAWN_RADIUS)
        .max(0);
    let distance = search
        .border
        .distance_to_border(f64::from(search.spawn.pos.x), f64::from(search.spawn.pos.z));
    let border = distance.floor() as i32;
    let mut radius = wanted.min(border);
    if border <= 1 {
        radius = 1;
    }
    let side = radius.saturating_mul(2).saturating_add(1);
    let count = MAX_CANDIDATES.min(side.saturating_mul(side)).max(1);
    let mut random = JavaRandom::from_state(seed);
    let offset = random.next_int_bounded(count);
    (radius, offset, random.state())
}

/// 官方 `RandomSource.createThreadLocalInstance()` 的播种链：
/// `ThreadLocalRandom.current().nextLong()` → `SingleThreadedRandomSource.setSeed`
/// （xor 0x5DEECE66D 再掩 48 位）。对拍把 `ThreadLocalRandom` 的内部 `rnd`
/// 固定成 `thread_seed`，这里算出 `JavaRandom` 应有的初始状态。
///
/// Vanilla's `RandomSource.createThreadLocalInstance()` seeding chain:
/// `ThreadLocalRandom.current().nextLong()` → `SingleThreadedRandomSource.setSeed`
/// (xor 0x5DEECE66D then mask 48 bits). The oracle pins `ThreadLocalRandom`'s
/// internal `rnd` to `thread_seed`; this computes the matching `JavaRandom`
/// initial state.
#[cfg(feature = "oracle-samples")]
pub(crate) fn oracle_initial_seed(thread_seed: i64) -> i64 {
    const MULTIPLIER: u64 = 0x5_DEEC_E66D;
    const MASK: u64 = (1 << 48) - 1;
    // 对拍把 `PlayerSpawnFinder` 的随机源直接用固定种子构造：
    // `SingleThreadedRandomSource.setSeed` xor MULTIPLIER 得到 48 位初态。
    // The oracle constructs the official RNG from a fixed seed;
    // `SingleThreadedRandomSource.setSeed` xors MULTIPLIER for its state.
    ((thread_seed as u64 ^ MULTIPLIER) & MASK) as i64
}

/// 官方 `PlayerSpawnFinder.findSpawn` 的结果。给 `seed` 时随机源可复现
/// （生产代码传 `None`，照官方从线程本地随机源取不确定的种子；对拍传固定值）。
///
/// The result of vanilla `PlayerSpawnFinder.findSpawn`. A supplied `seed` makes
/// the random source reproducible; production passes `None` and uses the
/// official thread-local random, while the oracle passes a fixed seed.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn find_spawn(search: &Search<'_>, seed: Option<i64>) -> Result<Landing, NotReady> {
    find_spawn_impl(search, seed, false)
}

/// `find_spawn` 带超时开关：超时时把还没交付的候选列当作没有可站方块，
/// 用当时已加载的部分照官方顺序算一次（只影响噪声世界）。
/// `find_spawn` with a timeout switch: after the timeout, an undelivered
/// candidate column is treated as having no standable block and the search
/// runs over the loaded part (noise worlds only).
fn find_spawn_impl(
    search: &Search<'_>,
    seed: Option<i64>,
    allow_partial: bool,
) -> Result<Landing, NotReady> {
    let pos = search.spawn.pos;
    // 冒险模式：`findSpawn` 开头的 `completedFuture(fixupSpawnHeight(...))`。
    // Adventure: `findSpawn` starts with `completedFuture(fixupSpawnHeight(...))`.
    if search.default_game_mode == mc_bamboo_protocol::packet::play::GameMode::Adventure {
        let fixed = fixup_spawn_height(search, pos, allow_partial)?;
        return Ok(Landing {
            position: bottom_center(fixed),
            yaw: search.spawn.yaw,
            pitch: search.spawn.pitch,
            candidate: None,
            radius: 0,
            offset: 0,
            random_state: 0,
        });
    }
    let wanted = search
        .rules
        .int(mc_bamboo_data::gamerules::RESPAWN_RADIUS)
        .max(0);
    let distance = search
        .border
        .distance_to_border(f64::from(pos.x), f64::from(pos.z));
    let border = distance.floor() as i32;
    let mut radius = wanted.min(border);
    if border <= 1 {
        radius = 1;
    }
    let side = radius.saturating_mul(2).saturating_add(1);
    let count = MAX_CANDIDATES.min(side.saturating_mul(side)).max(1);
    let coprime = if count <= 16 { count - 1 } else { 17 };
    // 对拍传进来的 `seed` 是官方 `SingleThreadedRandomSource` 的 48 位内部
    // 状态；生产传 `None` 时用新的 `new Random(seed)`。
    // The oracle's `seed` is the official 48-bit internal state; production
    // passes `None` and constructs `new Random(seed)`.
    let mut random = match seed {
        Some(state) => JavaRandom::from_state(state),
        None => JavaRandom::new(thread_local_seed()),
    };
    let offset = random.next_int_bounded(count);
    let mut unready = false;
    for index in 0..count {
        let slot = (offset.wrapping_add(coprime.wrapping_mul(index))) % count;
        let x = pos.x.wrapping_add(slot % side).wrapping_sub(radius);
        let z = pos.z.wrapping_add(slot / side).wrapping_sub(radius);
        // 官方 `scheduleCandidate` 会先加载候选区块；未交付的噪声列不能读，
        // 记下来让调用方等区块（flat 的模板在哪一格都一样，直接读模板）。
        // Vanilla `scheduleCandidate` loads the candidate chunk first. An
        // undelivered noise column cannot be read, so remember it and let the
        // caller wait; a flat template is identical everywhere and readable.
        if !search.chunks.is_loaded(ChunkPos::new(x >> 4, z >> 4))
            && !search.level_type.is_flat()
            && !allow_partial
        {
            unready = true;
            continue;
        }
        if let Some(found) = level_respawn_pos(search, x, z, allow_partial)?
            && no_collision_no_liquid(search, found, allow_partial)?
        {
            return Ok(Landing {
                position: bottom_center(found),
                yaw: search.spawn.yaw,
                pitch: search.spawn.pitch,
                candidate: Some(found),
                radius,
                offset,
                random_state: random.state(),
            });
        }
    }
    if unready {
        return Err(NotReady);
    }
    // `scheduleNext` 走完 count 个候选后，最后一个是
    // `fixupSpawnHeight(level, spawnSuggestion)` 的无条件兜底。
    // After `count` candidates `scheduleNext` unconditionally falls back to
    // `fixupSpawnHeight(level, spawnSuggestion)`.
    let fixed = fixup_spawn_height(search, pos, allow_partial)?;
    Ok(Landing {
        position: bottom_center(fixed),
        yaw: search.spawn.yaw,
        pitch: search.spawn.pitch,
        candidate: None,
        radius,
        offset,
        random_state: random.state(),
    })
}

/// 官方 `RandomSource.createThreadLocalInstance()` 用
/// `ThreadLocalRandom.current().nextLong()`；这里用进程内单调 + 时钟播种，
/// 保持逐次不同且不和世界的随机序列混在一起。
///
/// Vanilla's `RandomSource.createThreadLocalInstance()` uses
/// `ThreadLocalRandom.current().nextLong()`. A process-wide monotonic counter
/// plus the clock keeps successive searches distinct without sharing world RNG.
fn thread_local_seed() -> i64 {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static COUNTER: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (nanos ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed)) as i64
}

/// `Vec3.atBottomCenterOf`：y 贴着方块底，x / z 在方块中心。
/// `Vec3.atBottomCenterOf`: y at the block's bottom, x / z at its centre.
pub(crate) fn bottom_center(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    )
}

/// 读一个方块状态。超平坦模板在所有未交付位置都一样，直接读 store 的模板；
/// 噪声世界的未交付列在超时后可当空气，否则返回 `NotReady`。
/// Read one block state. A superflat template is identical at unloaded
/// positions, so read the store's template; an unloaded noise column is air
/// after the timeout, or `NotReady` otherwise.
fn state_at(search: &Search<'_>, pos: BlockPos, allow_partial: bool) -> Result<u32, NotReady> {
    if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&pos.y) {
        return Ok(blocks::AIR);
    }
    if let Some(state) = search.chunks.block(pos) {
        return Ok(state);
    }
    if search.level_type.is_flat() {
        let chunk = search.chunks.chunk(pos.chunk());
        return Ok(chunk.block((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize));
    }
    if allow_partial {
        return Ok(blocks::AIR);
    }
    Err(NotReady)
}

/// `FluidState.isEmpty()`。
/// `FluidState.isEmpty()`.
fn fluid_is_empty(state: u32) -> bool {
    state_fluid(state).fluid == registries::fluid::EMPTY
}

/// `Block.isFaceFull(getCollisionShape(...), UP)`：碰撞形状的顶面覆盖整格。
/// `Block.isFaceFull(getCollisionShape(...), UP)`: the collision shape's top
/// face covers the whole block face.
fn face_full_up(state: u32) -> bool {
    collision_shape(state).iter().any(|box_| {
        box_.min_x <= 0.0
            && box_.max_x >= 1.0
            && box_.min_z <= 0.0
            && box_.max_z >= 1.0
            && (box_.max_y - 1.0).abs() < 1.0e-7
    })
}

/// `LevelChunk.getHeight(type, x, z)`：读区块里一直维护着的那张高度图，
/// 返回最高的方块 y（不是它上面一格）。未交付的超平坦列读模板的高度图。
/// `LevelChunk.getHeight(type, x, z)`: read the chunk's maintained heightmap
/// and return the topmost block's y (not the free block above). An unloaded
/// flat column reads the template's heightmap.
fn heightmap_y(
    search: &Search<'_>,
    kind: HeightmapKind,
    x: i32,
    z: i32,
    allow_partial: bool,
) -> Result<i32, NotReady> {
    let chunk = ChunkPos::new(x >> 4, z >> 4);
    if !search.chunks.is_loaded(chunk) && !search.level_type.is_flat() {
        return if allow_partial {
            Ok(MIN_Y - 1)
        } else {
            Err(NotReady)
        };
    }
    Ok(search
        .chunks
        .chunk(chunk)
        .heightmap_y(kind, (x & 15) as usize, (z & 15) as usize)
        - 1)
}

/// `PlayerDimensions.makeBoundingBox` 的碰撞形状判据：只要有形状就算挡住玩家。
/// `PlayerDimensions.makeBoundingBox`: any collision shape blocks the player.
fn has_collision(state: u32) -> bool {
    !collision_shape(state).is_empty() || !fluid_is_empty(state)
}

/// `PlayerSpawnFinder.getLevelRespawnPos` 的一列判定。
/// `PlayerSpawnFinder.getLevelRespawnPos` for one column.
fn level_respawn_pos(
    search: &Search<'_>,
    x: i32,
    z: i32,
    allow_partial: bool,
) -> Result<Option<BlockPos>, NotReady> {
    let top = heightmap_y(search, HeightmapKind::MotionBlocking, x, z, allow_partial)?;
    if top < MIN_Y {
        return Ok(None);
    }
    let surface = heightmap_y(search, HeightmapKind::WorldSurface, x, z, allow_partial)?;
    let floor = heightmap_y(search, HeightmapKind::OceanFloor, x, z, allow_partial)?;
    // 水面高于海底（`surface > ocean_floor`）时这一列站不住。
    // A liquid column whose surface sits above its ocean floor cannot hold a spawn.
    if surface <= top && surface > floor {
        return Ok(None);
    }
    // 从 topY+1 往下：先撞到流体就放弃，先撞到满碰撞顶面就站在它上面。
    // From topY+1 down: a fluid aborts; the first full collision top face wins.
    let mut y = top + 1;
    while y >= MIN_Y {
        let state = state_at(search, BlockPos::new(x, y, z), allow_partial)?;
        if !fluid_is_empty(state) {
            return Ok(None);
        }
        if face_full_up(state) {
            return Ok(Some(BlockPos::new(x, y + 1, z)));
        }
        y -= 1;
    }
    Ok(None)
}

/// `PlayerSpawnFinder.noCollisionNoLiquid`：玩家站在 `pos` 时脚和头两格
/// 都没有碰撞形状、没有流体，等价官方 `CollisionGetter.noCollision(null, AABB, true)`。
///
/// `PlayerSpawnFinder.noCollisionNoLiquid`: neither the feet nor the head block
/// has a collision shape or fluid, equivalent to vanilla
/// `CollisionGetter.noCollision(null, AABB, true)`.
fn no_collision_no_liquid(
    search: &Search<'_>,
    pos: BlockPos,
    allow_partial: bool,
) -> Result<bool, NotReady> {
    for y in [pos.y, pos.y + 1] {
        let state = state_at(search, BlockPos::new(pos.x, y, pos.z), allow_partial)?;
        if has_collision(state) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `PlayerSpawnFinder.fixupSpawnHeight`：从暂定点往上找能站的位置，再贴回地面。
/// `PlayerSpawnFinder.fixupSpawnHeight`: move up to a standable position, then
/// settle back onto the ground.
fn fixup_spawn_height(
    search: &Search<'_>,
    start: BlockPos,
    allow_partial: bool,
) -> Result<BlockPos, NotReady> {
    let standable = |y: i32| -> Result<bool, NotReady> {
        for at in [y, y + 1] {
            let state = state_at(search, BlockPos::new(start.x, at, start.z), allow_partial)?;
            if has_collision(state) {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let max_y = MIN_Y + WORLD_HEIGHT - 1;
    let mut y = start.y;
    while !standable(y)? && y < max_y {
        y += 1;
    }
    y -= 1;
    while standable(y)? && y > MIN_Y {
        y -= 1;
    }
    Ok(BlockPos::new(start.x, y + 1, start.z))
}

/// `Region` 上取一次搜索输入。
/// Collect a search's inputs from a `Region`.
pub(crate) fn search_from(region: &crate::region::Region) -> Search<'_> {
    Search {
        chunks: &region.chunks,
        rules: &region.global.rules,
        level_type: region.global.world_gen.level_type,
        border: &region.global.world_border,
        spawn: region.global.spawn,
        default_game_mode: region.global.default_game_mode,
    }
}

/// 官方 `PlayerSpawnFinder.findSpawn` 里实际生效的半径（搜索失败时给调用方挂票用）。
/// The radius actually used by vanilla `PlayerSpawnFinder.findSpawn` (so the
/// caller can add tickets for the covered chunks).
pub(crate) fn search_radius(search: &Search<'_>) -> i32 {
    let wanted = search
        .rules
        .int(mc_bamboo_data::gamerules::RESPAWN_RADIUS)
        .max(0);
    let pos = search.spawn.pos;
    let distance = search
        .border
        .distance_to_border(f64::from(pos.x), f64::from(pos.z));
    let border = distance.floor() as i32;
    if border <= 1 { 1 } else { wanted.min(border) }
}

/// 一次搜索覆盖到的区块（给调用方挂票 / 判就绪用）。
/// Chunks covered by one search, for the caller's tickets / readiness check.
pub(crate) fn covered_chunks(search: &Search<'_>, radius: i32) -> Vec<ChunkPos> {
    let pos = search.spawn.pos;
    let half = radius.min(256);
    let mut out = Vec::new();
    for x in (pos.x.saturating_sub(half))..=(pos.x.saturating_add(half)) {
        for z in (pos.z.saturating_sub(half))..=(pos.z.saturating_add(half)) {
            let chunk = ChunkPos::new(x >> 4, z >> 4);
            if !out.contains(&chunk) {
                out.push(chunk);
            }
        }
    }
    out
}

/// 进服 / 重生共用的入口：照官方先判 `/spawnpoint` 的强制个人点，再走
/// `findSpawn(worldSpawn)`。返回 `NotReady` 时由调用方等区块就绪后重试。
/// `missing` 表示个人点方块失效，要发 `NO_RESPAWN_BLOCK_AVAILABLE`。
///
/// Shared join / respawn entry: vanilla checks a forced `/spawnpoint` first,
/// then falls back to `findSpawn(worldSpawn)`. `NotReady` means the caller must
/// wait for chunk readiness and retry. `missing` means the personal spawn
/// block is unusable, so `NO_RESPAWN_BLOCK_AVAILABLE` has to be sent.
pub(crate) fn landing(
    region: &crate::region::Region,
    personal: Option<RespawnData>,
    seed: Option<i64>,
    allow_partial: bool,
) -> Result<(Landing, bool), NotReady> {
    if let Some(personal) = personal
        && forced_personal_ready(region, personal)
    {
        // `findRespawnAndUseSpawnBlock` 的 forced 分支：两格能重生就直接落地，
        // 位置是 `pos + (0.5, 0.1, 0.5)`，不做散布。
        // The forced branch of `findRespawnAndUseSpawnBlock`: when both blocks
        // allow respawning, land at `pos + (0.5, 0.1, 0.5)` with no search.
        return Ok((
            Landing {
                position: Vec3::new(
                    f64::from(personal.pos.x) + 0.5,
                    f64::from(personal.pos.y) + 0.1,
                    f64::from(personal.pos.z) + 0.5,
                ),
                yaw: personal.yaw,
                pitch: personal.pitch,
                candidate: None,
                radius: 0,
                offset: 0,
                random_state: 0,
            },
            false,
        ));
    }
    // 个人点存在但方块不允许、或根本没有个人点：世界出生点 + `findSpawn`。
    // A blocked personal spawn, or no personal spawn: world spawn + `findSpawn`.
    let search = search_from(region);
    let landing = find_spawn_impl(&search, seed, allow_partial)?;
    Ok((landing, personal.is_some()))
}

/// 供对拍用：直接在一份 `Search` 上跑共用的落位逻辑（不需要完整 `Region`）。
/// For the oracle: run the shared landing logic on a bare `Search`.
#[cfg(any(test, feature = "oracle-samples"))]
pub(crate) fn landing_from_search(
    search: &Search<'_>,
    personal: Option<RespawnData>,
    seed: Option<i64>,
) -> Result<(Landing, bool), NotReady> {
    if let Some(personal) = personal
        && personal_spawn_ready(search, personal)
    {
        return Ok((
            Landing {
                position: Vec3::new(
                    f64::from(personal.pos.x) + 0.5,
                    f64::from(personal.pos.y) + 0.1,
                    f64::from(personal.pos.z) + 0.5,
                ),
                yaw: personal.yaw,
                pitch: personal.pitch,
                candidate: None,
                radius: 0,
                offset: 0,
                random_state: 0,
            },
            false,
        ));
    }
    let landing = find_spawn(search, seed)?;
    Ok((landing, personal.is_some()))
}

/// 个人点的两格能不能重生（官方 `isPossibleToRespawnInThis`）。
/// Whether a personal spawn's two blocks allow respawning.
#[cfg(any(test, feature = "oracle-samples"))]
pub(crate) fn personal_spawn_ready(search: &Search<'_>, spawn: RespawnData) -> bool {
    let feet = state_at(search, spawn.pos, false).unwrap_or(blocks::STONE);
    let head = state_at(search, spawn.pos.offset(0, 1, 0), false).unwrap_or(blocks::STONE);
    respawn_block_allowed(feet) && respawn_block_allowed(head)
}

/// `Block.isPossibleToRespawnInThis`：不是固体也不是液体。
/// `Block.isPossibleToRespawnInThis`: neither solid nor liquid.
fn respawn_block_allowed(state: u32) -> bool {
    let facts = state_facts(state);
    facts & facts::SOLID == 0 && facts & mc_bamboo_data::facts::LIQUID == 0
}

/// 官方 `findRespawnAndUseSpawnBlock` 的 forced 分支：个人点和它上面一格
/// 都能重生时才直接落地。没加载的区块按不可用处理（回退到世界出生点）。
/// The forced branch of vanilla `findRespawnAndUseSpawnBlock`: both the
/// personal position and the block above must allow respawning. An unloaded
/// chunk is treated as unusable (falls back to the world spawn).
fn forced_personal_ready(region: &crate::region::Region, spawn: RespawnData) -> bool {
    let search = search_from(region);
    let feet = state_at(&search, spawn.pos, false).unwrap_or(blocks::STONE);
    let head = state_at(&search, spawn.pos.offset(0, 1, 0), false).unwrap_or(blocks::STONE);
    respawn_block_allowed(feet) && respawn_block_allowed(head)
}

/// 官方 `TeleportTransition.missingRespawnBlock`：强制个人点的两格不能重生，
/// 回退到世界出生点时为真（发 `NO_RESPAWN_BLOCK_AVAILABLE` 且清个人点）。
/// Vanilla `TeleportTransition.missingRespawnBlock`: a forced personal spawn
/// whose two blocks cannot hold a respawn falls back to the world spawn; send
/// `NO_RESPAWN_BLOCK_AVAILABLE` and clear the personal spawn.
pub(crate) fn personal_spawn_missing(
    region: &crate::region::Region,
    personal: Option<RespawnData>,
) -> bool {
    personal.is_some_and(|spawn| !forced_personal_ready(region, spawn))
}

/// 每刻检查等区块的玩家：候选区块就绪时重跑精确搜索并落位。
/// 超时（40 刻，2 秒）也照当时已加载的区块算一次并记日志，不让人永远卡住。
///
/// Each tick, finish pending landings: once the covered chunks are ready,
/// rerun the exact search and place the player. On timeout (40 ticks / 2 s)
/// compute with the chunks available then and log it, so nobody waits forever.
pub(crate) fn finish_pending(region: &mut crate::region::Region) {
    use crate::inbound::PlayerId;
    use tracing::warn;

    /// 官方 `PrepareSpawnTask` 没有超时，但服务端不能让客户端永远停在加载界面。
    /// Vanilla `PrepareSpawnTask` has no timeout, but a server cannot leave a
    /// client on the loading screen forever.
    const MAX_WAIT_TICKS: i32 = 40;

    let pending: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.spawn_pending.is_some())
        .map(|(&id, _)| id)
        .collect();
    for id in pending {
        let Some(pending) = region.players.get(&id).and_then(|p| p.spawn_pending) else {
            continue;
        };
        let landing = {
            let search = search_from(region);
            let radius = search_radius(&search);
            let ready = search.level_type.is_flat()
                || covered_chunks(&search, radius)
                    .iter()
                    .all(|&pos| region.chunks.is_loaded(pos));
            if !ready && pending.waited + 1 < MAX_WAIT_TICKS {
                if let Some(player) = region.players.get_mut(&id)
                    && let Some(state) = &mut player.spawn_pending
                {
                    state.waited += 1;
                }
                continue;
            }
            if !ready {
                warn!(
                    player = id.0,
                    waited_ticks = pending.waited + 1,
                    "spawn landing timed out; using the search over loaded chunks"
                );
            }
            match landing(region, pending.personal, None, !ready) {
                Ok((landing, _)) => landing,
                Err(NotReady) => {
                    // 超时后仍有未加载列：不拿兜底位置当最终落点，继续等下一刻。
                    // Even after the timeout, an unloaded column cannot produce
                    // a final landing; keep waiting rather than place wrongly.
                    if let Some(player) = region.players.get_mut(&id)
                        && let Some(state) = &mut player.spawn_pending
                    {
                        state.waited += 1;
                    }
                    continue;
                }
            }
        };
        finish_one(region, id, pending.join, landing);
    }
}

/// 一名玩家的落位：重生补 `player_position`（和官方 `connection.teleport` 一致），
/// 进服走 `movement::teleport`；两者都更新服务端坐标和 `player_grid`，
/// 跨岛搬运照 1b 的路径由主控按玩家所在区块完成。
///
/// Place one player: respawn appends `player_position` (like vanilla
/// `connection.teleport`), join uses `movement::teleport`. Both update the
/// server-side position and grid; cross-island relocation uses the 1b path.
fn finish_one(
    region: &mut crate::region::Region,
    id: crate::inbound::PlayerId,
    join: bool,
    landing: Landing,
) {
    use mc_bamboo_protocol::packet::play::PlayerPosition;

    let position = landing.position;
    let yaw = landing.yaw;
    let pitch = landing.pitch;
    if join {
        let tick = region.tick;
        if let Some(player) = region.players.get_mut(&id) {
            player.yaw = yaw;
            player.pitch = pitch;
            crate::movement::teleport(
                player,
                &mut region.links,
                &mut region.player_grid,
                tick,
                position,
            );
        }
    } else {
        region.links.send(
            id,
            &PlayerPosition {
                teleport_id: 1,
                x: position.x,
                y: position.y,
                z: position.z,
                yaw,
                pitch,
                relative: 0,
            },
        );
        crate::command::place_player_at_respawn(region, id, position, yaw, pitch);
    }
    if let Some(player) = region.players.get_mut(&id) {
        player.spawn_pending = None;
        player.health_sync = crate::player_hurt::HealthSync::of(player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::WorldBorder;
    use mc_bamboo_data::blocks;

    fn flat<'a>(
        chunks: &'a ChunkStore,
        rules: &'a GameRuleSet,
        spawn: BlockPos,
        radius: i32,
    ) -> Search<'a> {
        // 测试专用：借用一份静态边界，够大，半径不被它裁小。
        // Test-only: a static, huge border so the radius is not clamped.
        static BORDER: std::sync::LazyLock<WorldBorder> =
            std::sync::LazyLock::new(WorldBorder::default);
        let _ = radius;
        Search {
            chunks,
            rules,
            level_type: LevelType::Flat,
            border: &BORDER,
            spawn: RespawnData::of(spawn, 0.0, 0.0),
            default_game_mode: mc_bamboo_protocol::packet::play::GameMode::Creative,
        }
    }

    #[test]
    fn radius_zero_finds_the_exact_spawn_column() {
        let rules = GameRuleSet::vanilla();
        let mut assigned = rules.clone();
        let _ = assigned.assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
        let chunks = ChunkStore::flat();
        let landing = find_spawn(
            &flat(&chunks, &assigned, BlockPos::new(8, 0, 8), 0),
            Some(1),
        )
        .expect("infinite flat chunks are ready");
        assert_eq!(landing.position_tuple(), (8.5, -60.0, 8.5));
        assert_eq!(landing.candidate, Some(BlockPos::new(8, -60, 8)));
        assert_eq!(landing.radius, 0);
        assert_eq!(landing.offset, 0);
    }

    #[test]
    fn default_radius_finds_a_grass_surface_near_the_spawn() {
        let rules = GameRuleSet::vanilla();
        let chunks = ChunkStore::flat();
        let landing = find_spawn(&flat(&chunks, &rules, BlockPos::new(8, 0, 8), 10), Some(42))
            .expect("infinite flat chunks are ready");
        assert_eq!(landing.radius, 10);
        assert_eq!(landing.position.y, -60.0);
        let pos = landing.candidate.expect("flat has standable columns");
        assert!((-2..=18).contains(&pos.x) && (-2..=18).contains(&pos.z));
        assert_eq!(pos.y, -60);
        // 草方块在落脚点正下方。/ Grass is directly below the landing column.
        assert_eq!(
            chunks.block(BlockPos::new(pos.x, -61, pos.z)),
            Some(blocks::GRASS_BLOCK)
        );
    }

    /// 区块没就绪时返回 `NotReady`；全部就绪后同一个搜索给出同一个精确落点。
    /// An unready chunk yields `NotReady`; once delivered the same search gives
    /// the same exact landing.
    #[test]
    fn unready_noise_chunk_defers_until_the_same_landing_is_available() {
        use mc_bamboo_world::column::ChunkColumn;
        let mut rules = GameRuleSet::vanilla();
        let _ = rules.assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
        static BORDER: std::sync::LazyLock<WorldBorder> =
            std::sync::LazyLock::new(WorldBorder::default);
        let mut chunks = ChunkStore::loaded();
        let spawn = RespawnData::of(BlockPos::new(40, -60, 40), 0.0, 0.0);
        let unready = Search {
            chunks: &chunks,
            rules: &rules,
            level_type: LevelType::Normal,
            border: &BORDER,
            spawn,
            default_game_mode: mc_bamboo_protocol::packet::play::GameMode::Creative,
        };
        assert_eq!(find_spawn(&unready, Some(7)), Err(NotReady));
        // 走正式的区块交付路径：种子无关的模板就是噪声世界这一格的等价内容。
        // Deliver through the real chunk path; the template stands in for the
        // generated column (the algorithm only reads its heightmaps / blocks).
        chunks.load(ChunkPos::new(2, 2), Box::new(ChunkColumn::from_template()));
        let ready = Search {
            chunks: &chunks,
            rules: &rules,
            level_type: LevelType::Normal,
            border: &BORDER,
            spawn,
            default_game_mode: mc_bamboo_protocol::packet::play::GameMode::Creative,
        };
        let landing = find_spawn(&ready, Some(7)).expect("交付后能算落点");
        assert_eq!(landing.position_tuple(), (40.5, -60.0, 40.5));
        assert_eq!(landing.candidate, Some(BlockPos::new(40, -60, 40)));
        assert_eq!(landing.radius, 0);
    }

    /// 冒险模式：findSpawn 直接 fixupSpawnHeight，不受半径 / 随机源影响。
    /// Adventure: findSpawn goes straight to fixupSpawnHeight, with no scatter.
    #[test]
    fn adventure_skips_the_scatter() {
        let mut rules = GameRuleSet::vanilla();
        let _ = rules.assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "10");
        let chunks = ChunkStore::flat();
        let mut search = flat(&chunks, &rules, BlockPos::new(8, 0, 8), 10);
        search.default_game_mode = mc_bamboo_protocol::packet::play::GameMode::Adventure;
        let landing = find_spawn(&search, Some(42)).expect("chunks ready");
        assert_eq!(landing.position_tuple(), (8.5, -60.0, 8.5));
        assert_eq!(landing.radius, 0);
        assert_eq!(landing.offset, 0);
    }

    /// 强制个人点两格可重生时直接落地；被实心方块堵住时回退到搜索。
    /// A forced personal spawn lands directly when its two blocks allow
    /// respawning, and falls back to the search when blocked.
    #[test]
    fn forced_personal_spawn_uses_the_official_direct_branch() {
        let mut rules = GameRuleSet::vanilla();
        let _ = rules.assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
        let mut chunks = ChunkStore::flat();
        let personal = RespawnData::of(BlockPos::new(4, -60, 4), 0.0, 0.0);
        assert!(personal_spawn_ready(
            &flat(&chunks, &rules, BlockPos::new(8, 0, 8), 0),
            personal
        ));
        let (direct, missing) = landing_from_search(
            &flat(&chunks, &rules, BlockPos::new(8, 0, 8), 0),
            Some(personal),
            Some(1),
        )
        .expect("chunks ready");
        assert!(!missing);
        assert_eq!(direct.position_tuple(), (4.5, -59.9, 4.5));
        // 个人点被石头堵住：官方回退到世界出生点 + findSpawn，并发 missing 包。
        chunks.set_block(BlockPos::new(4, -60, 4), blocks::STONE);
        let (fallback, missing) = landing_from_search(
            &flat(&chunks, &rules, BlockPos::new(8, 0, 8), 0),
            Some(personal),
            Some(1),
        )
        .expect("chunks ready");
        assert!(missing);
        assert_eq!(fallback.position_tuple(), (8.5, -60.0, 8.5));
    }
}
