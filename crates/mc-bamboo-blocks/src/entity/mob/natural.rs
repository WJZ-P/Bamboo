//! 自然生成（官方 `NaturalSpawner`）。
//!
//! 每刻从本岛实体表重算 [`SpawnState`]：按 `MobCategory` 计数、算刷怪代价
//! （`PotentialCalculator`）和每个玩家附近的计数（`LocalMobCapCalculator` 的等价物）。
//! 然后按类别挑出还能刷的类别（全局上限 `maxInstancesPerChunk * spawnableChunkCount / 289`），
//! 对刷怪区块逐个类别取一个随机起点、在 3 组 × 4 次的尝试里按群系的加权表挑一种生物，
//! 检查距离、光照、方块和碰撞条件，最后 `finalizeSpawn` 并按组生成。
//!
//! 随机数的取数顺序照官方：`getRandomPosWithin` 三次、每组一次 `nextFloat` 定上限、
//! 每步四次 `nextInt(6)`、挑表 `nextInt(总权重)`、组大小 `count.sample`、生成规则里的
//! `nextInt(32)` / `nextInt(8)`、朝向 `nextFloat`、`finalizeSpawn` 各取数。
//!
//! Natural spawning, vanilla `NaturalSpawner`. Each tick rebuilds a [`SpawnState`] from
//! the island's entity table (per-category counts, spawn potential, per-player counts),
//! filters the categories that still have room under the global cap, and for every
//! spawning chunk tries three groups of attempts: pick a weighted spawn entry from the
//! biome table, check distance/light/block/collision rules, then `finalizeSpawn` and
//! spawn the group. Random draws follow vanilla order.

use std::collections::BTreeMap;

use mc_bamboo_core::{Axis, BlockPos, ChunkPos, LightLayer, Vec3, mth};
use mc_bamboo_data::BlockAncestor;
use mc_bamboo_data::blocks::{CACTUS, LAVA, LAVA_CAULDRON, MAGMA_BLOCK, POWDER_SNOW};
use mc_bamboo_data::spawning::{self, MobCategory, MobSpawnSettings, MobSpawnerDef};
use mc_bamboo_data::tags::block_tags;

#[cfg(test)]
use super::timing::record_structure_query;
use super::{MobType, spawn};
use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, PlayerView},
        store::KindPublic,
    },
    shape::{Aabb, CollisionContext},
    state::{BlockState, LIT},
    world::World,
};

/// `NaturalSpawner.MIN_SPAWN_DISTANCE` 的平方：离玩家 24 格以内不刷。
pub const MIN_SPAWN_DISTANCE_SQR: f64 = 576.0;

/// `NaturalSpawner.MAGIC_NUMBER`，全局上限按 `max * chunks / 289`。
pub const MAGIC_NUMBER: i32 = 289;

/// 每个类别对一个区块最多试 3 组。
const GROUP_ATTEMPTS: i32 = 3;

/// 每步候选点的水平漂移上限（`nextInt(6) - nextInt(6)`）。
const STEP_RANGE: i32 = 6;

/// `PotentialCalculator.PointCharge`。
#[derive(Debug, Clone, Copy)]
struct PointCharge {
    pos: BlockPos,
    charge: f64,
}

impl PointCharge {
    /// `PointCharge.getPotentialChange`。
    fn potential_change(self, pos: BlockPos) -> f64 {
        let dx = f64::from(self.pos.x - pos.x);
        let dy = f64::from(self.pos.y - pos.y);
        let dz = f64::from(self.pos.z - pos.z);
        let dist_sqr = dx * dx + dy * dy + dz * dz;
        if dist_sqr == 0.0 {
            f64::INFINITY
        } else {
            self.charge / dist_sqr.sqrt()
        }
    }
}

/// 一刻的刷怪计数，官方 `NaturalSpawner.SpawnState`。
///
/// One tick's spawn bookkeeping, vanilla `NaturalSpawner.SpawnState`.
pub struct SpawnState {
    spawnable_chunk_count: i32,
    counts: [i32; 8],
    potential: Vec<PointCharge>,
    /// 每名玩家每类的计数，`LocalMobCapCalculator.playerMobCounts`。
    player_counts: BTreeMap<EntityUid, [i32; 8]>,
    /// 这一刻的玩家快照，`ChunkMap.getPlayersCloseForSpawning` 用它。
    players: Vec<PlayerView>,
    /// 上一个查询点，`SpawnState.lastCheckedPos` / `lastCheckedType` / `lastCharge`。
    last_checked_pos: Option<BlockPos>,
    last_checked_type: Option<MobType>,
    last_charge: f64,
}

impl SpawnState {
    /// `NaturalSpawner.createState`：遍历本岛实体，按类别计数。
    ///
    /// `spawnable_chunk_count` 是 `DistanceManager.getNaturalSpawnChunkCount()` 的等价物，
    /// 由调用方按玩家附近的区块算好（见 `docs/mob-entity-design.md` 4.6）。
    ///
    /// Vanilla `createState`: walk the island's entities and count them per category.
    /// `spawnable_chunk_count` mirrors `getNaturalSpawnChunkCount()`; the caller derives
    /// it from the chunks around players.
    pub fn create(world: &World, spawnable_chunk_count: i32) -> Self {
        let mut state = Self {
            spawnable_chunk_count,
            counts: [0; 8],
            potential: Vec::new(),
            player_counts: BTreeMap::new(),
            players: Vec::new(),
            last_checked_pos: None,
            last_checked_type: None,
            last_charge: 0.0,
        };
        world.for_each_player(&mut |player| {
            if !player.spectator {
                state.players.push(*player);
            }
        });
        let entities = world.level().entities_table();
        for uid in entities.ids() {
            let Some(public) = entities.public_at(uid) else {
                continue;
            };
            if public.removed {
                continue;
            }
            let KindPublic::Mob(mob) = &public.kind else {
                continue;
            };
            // `Mob.requiresCustomPersistence()` 只认载具和拴绳，Bamboo 都没有；鱼从桶里
            // 放出来的那种、搬着方块的末影人要算进来。
            // Vanilla `requiresCustomPersistence()` needs a vehicle or a leash; fish
            // released from a bucket and endermen carrying a block count as well.
            if mob.requires_custom_persistence() {
                continue;
            }
            let kind = mob.mob_type;
            let category = kind.mob_category();
            if category == MobCategory::Misc {
                continue;
            }
            let pos = BlockPos::containing(public.position);
            if let Some(cost) = settings_at(world, pos).spawn_cost(kind.entity_type()) {
                state.add_charge(pos, cost.charge);
            }
            state.add_mob(chunk_at(pos), category);
            state.counts[category.index()] += 1;
        }
        state
    }

    /// `canSpawnForCategoryGlobal`。
    pub fn can_spawn_for_category_global(&self, category: MobCategory) -> bool {
        let max = category.facts().max_instances * self.spawnable_chunk_count / MAGIC_NUMBER;
        self.counts[category.index()] < max
    }

    /// `getFilteredSpawningCategories`。`spawn_persistent` 是 `gameTime % 400 == 0`。
    pub fn filtered_categories(
        &self,
        spawn_enemies: bool,
        spawn_persistent: bool,
    ) -> Vec<MobCategory> {
        MobCategory::SPAWNING
            .into_iter()
            .filter(|category| {
                let facts = category.facts();
                (spawn_enemies || facts.friendly)
                    && (spawn_persistent || !facts.persistent)
                    && self.can_spawn_for_category_global(*category)
            })
            .collect()
    }

    /// `canSpawnForCategoryLocal`：附近没有玩家时为假，和官方一样。
    fn can_spawn_for_category_local(&self, category: MobCategory, chunk: ChunkPos) -> bool {
        self.players_close_to(chunk).into_iter().any(|player| {
            self.player_counts
                .get(&player.uid)
                .is_none_or(|counts| counts[category.index()] < category.facts().max_instances)
        })
    }

    fn add_charge(&mut self, pos: BlockPos, charge: f64) {
        if charge != 0.0 {
            self.potential.push(PointCharge { pos, charge });
        }
    }

    /// `PotentialCalculator.getPotentialEnergyChange`。
    fn potential_energy_change(&self, pos: BlockPos, charge: f64) -> f64 {
        if charge == 0.0 {
            return 0.0;
        }
        let sum: f64 = self
            .potential
            .iter()
            .map(|point| point.potential_change(pos))
            .sum();
        sum * charge
    }

    /// `LocalMobCapCalculator.addMob`：这个区块附近的每名玩家，这一类的计数 +1。
    fn add_mob(&mut self, chunk: ChunkPos, category: MobCategory) {
        for player in self.players_close_to(chunk) {
            self.player_counts.entry(player.uid).or_insert([0; 8])[category.index()] += 1;
        }
    }

    /// 区块附近的玩家（`ChunkMap.getPlayersCloseForSpawning`）。
    fn players_close_to(&self, chunk: ChunkPos) -> Vec<PlayerView> {
        self.players
            .iter()
            .filter(|player| is_close_to_spawn_chunk(player.position, chunk))
            .copied()
            .collect()
    }

    /// `SpawnState.canSpawn`：刷怪代价的检查。通过时把这一点记下来给 `after_spawn` 用。
    fn can_spawn(&mut self, world: &World, kind: MobType, pos: BlockPos) -> bool {
        self.last_checked_pos = Some(pos);
        self.last_checked_type = Some(kind);
        let Some(cost) = settings_at(world, pos).spawn_cost(kind.entity_type()) else {
            self.last_charge = 0.0;
            return true;
        };
        self.last_charge = cost.charge;
        self.potential_energy_change(pos, cost.charge) <= cost.energy_budget
    }

    /// `SpawnState.afterSpawn`。
    fn after_spawn(&mut self, world: &World, kind: MobType, pos: BlockPos) {
        let charge = if self.last_checked_pos == Some(pos) && self.last_checked_type == Some(kind) {
            self.last_charge
        } else {
            settings_at(world, pos)
                .spawn_cost(kind.entity_type())
                .map_or(0.0, |cost| cost.charge)
        };
        self.add_charge(pos, charge);
        let category = kind.mob_category();
        self.counts[category.index()] += 1;
        self.add_mob(chunk_at(pos), category);
    }

    /// 当前各类别的计数，测试和报告用。
    pub fn count(&self, category: MobCategory) -> i32 {
        self.counts[category.index()]
    }
}

fn chunk_at(pos: BlockPos) -> ChunkPos {
    ChunkPos::new(pos.x >> 4, pos.z >> 4)
}

/// 一刻的刷怪参数，`ServerChunkCache.tickChunks` 从游戏规则和玩家表算出来的部分。
///
/// One tick's spawn parameters, the part `ServerChunkCache.tickChunks` derives from
/// game rules and the player table.
#[derive(Debug, Clone, Copy)]
pub struct SpawnParams {
    /// `DistanceManager.getNaturalSpawnChunkCount()`。
    pub spawnable_chunk_count: i32,
    /// `spawn_monsters`（官方的 `spawnEnemies`）。
    pub spawn_enemies: bool,
    /// 按玩家的上限（`bamboo.toml` 的 `spawn_per_player_cap`）：关掉时只查全局上限。
    ///
    /// The per-player cap (`spawn_per_player_cap` in `bamboo.toml`); when off only the
    /// global cap applies.
    pub per_player_cap: bool,
    /// `getRespawnData()` 的位置，离它 24 格以内不刷。
    pub respawn_pos: Option<BlockPos>,
    /// `LevelAccessor.getSeaLevel()`：水生生物的生成规则读它。
    ///
    /// `LevelAccessor.getSeaLevel()`, read by the aquatic spawn rules.
    pub sea_level: i32,
}

/// `ServerChunkCache.tickSpawningChunk` → `NaturalSpawner.spawnForChunk`：对一个刷怪区块
/// 逐个类别试着生成。调用方负责只传能 tick 实体的区块。
///
/// `ServerChunkCache.tickSpawningChunk` → `NaturalSpawner.spawnForChunk`: try every
/// remaining category on one spawning chunk. The caller passes only entity-ticking chunks.
pub fn spawn_for_chunk(
    world: &mut World,
    chunk: ChunkPos,
    state: &mut SpawnState,
    params: &SpawnParams,
    categories: &[MobCategory],
) {
    // `ServerLevel.canSpawnEntitiesInChunk` 的另一半：区块在世界上边界内。
    // The other half of `ServerLevel.canSpawnEntitiesInChunk`: the chunk is in bounds.
    if !world.world_border().is_within_bounds_chunk(chunk) {
        return;
    }
    for &category in categories {
        if params.per_player_cap && !state.can_spawn_for_category_local(category, chunk) {
            continue;
        }
        spawn_category_for_chunk(world, category, chunk, state, params);
    }
}

/// `NaturalSpawner.spawnCategoryForChunk`。
fn spawn_category_for_chunk(
    world: &mut World,
    category: MobCategory,
    chunk: ChunkPos,
    state: &mut SpawnState,
    params: &SpawnParams,
) {
    let min_y = world.min_y();
    let start = random_pos_within(world.random(), chunk, min_y, |x, z| {
        world.world_surface_y(BlockPos::new(x, min_y, z))
    });
    if start.y > world.min_y() {
        spawn_category_for_position(world, category, chunk, start, state, params);
    }
}

/// `NaturalSpawner.getRandomPosWithin`：先取 x、z，再从世界底部到第一个空位（含）抽 y。
///
/// Vanilla `getRandomPosWithin`: draw x and z, then draw y from the world bottom
/// through the first available height (inclusive).
pub fn random_pos_within(
    random: &crate::random::LegacyRandom,
    chunk: ChunkPos,
    min_y: i32,
    first_available_y: impl FnOnce(i32, i32) -> i32,
) -> BlockPos {
    let x = chunk.x * 16 + random.next_int(16);
    let z = chunk.z * 16 + random.next_int(16);
    let y = random.next_int_between_inclusive(min_y, first_available_y(x, z));
    BlockPos::new(x, y, z)
}

/// `NaturalSpawner.spawnCategoryForPosition`：3 组，每组最多 4 次尝试。
fn spawn_category_for_position(
    world: &mut World,
    category: MobCategory,
    chunk: ChunkPos,
    start: BlockPos,
    state: &mut SpawnState,
    params: &SpawnParams,
) {
    let y_start = start.y;
    if world.block_state(start).is_redstone_conductor() {
        return;
    }
    let mut cluster_size = 0;
    for _group in 0..GROUP_ATTEMPTS {
        let mut x = start.x;
        let mut z = start.z;
        let mut current = None;
        let mut group: Option<spawn::SpawnGroup> = None;
        let mut max = mth::ceil(f64::from(world.random().next_float()) * 4.0);
        let mut step = 0;
        while step < max {
            step += 1;
            let random = world.random();
            x += random.next_int(STEP_RANGE) - random.next_int(STEP_RANGE);
            z += random.next_int(STEP_RANGE) - random.next_int(STEP_RANGE);
            let pos = BlockPos::new(x, y_start, z);
            let xx = f64::from(x) + 0.5;
            let zz = f64::from(z) + 0.5;
            let Some(nearest) =
                nearest_player_distance_sqr(&state.players, xx, f64::from(y_start), zz)
            else {
                continue;
            };
            if !is_right_distance_to_player_and_spawn_point(world, chunk, pos, nearest, params) {
                continue;
            }
            if current.is_none() {
                let Some(entry) = random_spawn_mob_at(world, category, pos) else {
                    break;
                };
                current = Some(entry);
                max = sample(entry, world.random());
            }
            let entry = current.expect("上面刚填过");
            // 保留未实现生物的权重和组大小抽取，但不能为它创建实体。
            // Preserve the weight and group-size draw of unimplemented mobs, but do not create them.
            let Some(kind) = MobType::by_entity_type(entry.entity_type) else {
                break;
            };
            let facts = (nearest, params.sea_level);
            if !is_valid_spawn_position_for_type(world, category, entry, kind, pos, facts, state) {
                continue;
            }
            let Some(mut entity) = create_mob(world, kind) else {
                return;
            };
            let yaw = world.random().next_float() * 360.0;
            spawn::snap_to(&mut entity, Vec3::new(xx, f64::from(y_start), zz), yaw, 0.0);
            if !is_valid_position_for_mob(world, &entity, kind, nearest, params.sea_level) {
                continue;
            }
            spawn::finalize_spawn_with_group(world, kind, &mut entity, &mut group);
            cluster_size += 1;
            let group_reached = is_max_group_size_reached(kind, entity.kind.mob());
            let uid = world.add_prepared_entity(entity);
            world.finish_jockeys(uid);
            state.after_spawn(world, kind, pos);
            if cluster_size >= max_spawn_cluster_size(kind) {
                return;
            }
            if group_reached {
                break;
            }
        }
    }
}

/// `NaturalSpawner.isRightDistanceToPlayerAndSpawnPoint`。
fn is_right_distance_to_player_and_spawn_point(
    world: &World,
    chunk: ChunkPos,
    pos: BlockPos,
    nearest_player_distance_sqr: f64,
    params: &SpawnParams,
) -> bool {
    if nearest_player_distance_sqr <= MIN_SPAWN_DISTANCE_SQR {
        return false;
    }
    if let Some(respawn) = params.respawn_pos {
        // `Vec3i.distToCenterSqr` 把 `BlockPos` 当中心（每个轴上加 0.5）来算。
        // `Vec3i.distToCenterSqr` treats the `BlockPos` as a centre (plus 0.5 per axis).
        let dx = f64::from(pos.x) + 0.5 - (f64::from(respawn.x) + 0.5);
        let dy = f64::from(pos.y) - (f64::from(respawn.y) + 0.5);
        let dz = f64::from(pos.z) + 0.5 - (f64::from(respawn.z) + 0.5);
        if dx * dx + dy * dy + dz * dz < MIN_SPAWN_DISTANCE_SQR {
            return false;
        }
    }
    let chunk_pos = chunk_at(pos);
    chunk_pos == chunk
        || (world.level().chunk_ticks_entities(chunk_pos)
            && world.world_border().is_within_bounds_chunk(chunk_pos))
}

/// `NaturalSpawner.isValidSpawnPostitionForType`。
fn is_valid_spawn_position_for_type(
    world: &mut World,
    category: MobCategory,
    entry: &MobSpawnerDef,
    kind: MobType,
    pos: BlockPos,
    (nearest_player_distance_sqr, sea_level): (f64, i32),
    state: &mut SpawnState,
) -> bool {
    if kind.mob_category() == MobCategory::Misc {
        return false;
    }
    let despawn = f64::from(kind.mob_category().facts().despawn_distance);
    if !kind.can_spawn_far_from_player() && nearest_player_distance_sqr > despawn * despawn {
        return false;
    }
    if !can_spawn_mob_at(world, category, entry, pos) {
        return false;
    }
    if !spawn_placement_ok(world, kind, pos) {
        return false;
    }
    if !check_spawn_rules(world, kind, pos, sea_level) {
        return false;
    }
    no_collision(world, spawn_aabb(kind, pos)) && state.can_spawn(world, kind, pos)
}

/// `NaturalSpawner.canSpawnMobAt` 比较 `SpawnerData`；权重属于列表，不参与比较。
///
/// Vanilla `canSpawnMobAt` compares `SpawnerData`; the list owns the weight.
fn can_spawn_mob_at(
    world: &World,
    category: MobCategory,
    entry: &MobSpawnerDef,
    pos: BlockPos,
) -> bool {
    mobs_at(world, category, pos).iter().any(|candidate| {
        (candidate.entity_type, candidate.min, candidate.max)
            == (entry.entity_type, entry.min, entry.max)
    })
}

/// `ChunkGenerator.getMobsAt`：`NaturalSpawner.mobsAt` 先查结构刷怪覆盖（前哨站、
/// 女巫小屋等），落在结构覆盖盒里时用结构的怪物表，否则用群系表。下界要塞那一支
/// 不建模。覆盖表是静态数据，查表和取权重走的是同一条链。
///
/// `ChunkGenerator.getMobsAt`: `NaturalSpawner.mobsAt` consults the structure spawn
/// override first (outposts, swamp huts, ...); inside an override box the structure's
/// monster list replaces the biome table. The nether-fortress branch is not modelled.
/// The override table is static data, so drawing and support checks share one list.
fn mobs_at(world: &World, category: MobCategory, pos: BlockPos) -> &'static [MobSpawnerDef] {
    structure_spawn_override_timed(world, pos, category)
        .unwrap_or_else(|| world.level().natural_spawn_table(pos, category))
}

/// 正式构建里就是一次虚调用；测试构建里顺手记一次查询次数和耗时。
///
/// One virtual call in production; the test build also records the query and its cost.
#[cfg(not(test))]
#[inline(always)]
fn structure_spawn_override_timed(
    world: &World,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    world.level().structure_spawn_override(pos, category)
}

#[cfg(test)]
#[inline(always)]
fn structure_spawn_override_timed(
    world: &World,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    let started = std::time::Instant::now();
    let table = world.level().structure_spawn_override(pos, category);
    record_structure_query!(started.elapsed().as_nanos() as u64);
    table
}

/// `level.environmentAttributes().getValue(NATURAL_MOB_SPAWNS, pos)`：主世界就是群系的表。
fn settings_at(world: &World, pos: BlockPos) -> MobSpawnSettings {
    spawning::for_biome_id(world.biome_id(pos))
}

/// `NaturalSpawner.getRandomSpawnMobAt`：先抽完整刷怪表，再由调用方决定能否生成。
///
/// Vanilla `getRandomSpawnMobAt`: draw from the full table before checking support.
fn random_spawn_mob_at(
    world: &mut World,
    category: MobCategory,
    pos: BlockPos,
) -> Option<&'static MobSpawnerDef> {
    if category == MobCategory::WaterAmbient
        && biome_tag_contains(
            "minecraft:reduced_water_ambient_spawns",
            world.biome_id(pos),
        )
        && world.random().next_float() < 0.98
    {
        return None;
    }
    let table = mobs_at(world, category, pos);
    pick(table, world.random())
}

/// 官方 `WeightedList.getRandom`：`nextInt(总权重)` 之后按累减权重挑一项。
/// 表为空或总权重为 0 时返回 `None` 且不取随机数（官方 `selector == null` 直接返回空）。
///
/// Vanilla `WeightedList.getRandom`: draw `nextInt(totalWeight)` and walk the
/// cumulative weights. An empty table or a zero total returns `None` without a draw.
pub fn pick<'a>(
    table: &'a [MobSpawnerDef],
    random: &dyn crate::random::RandomSource,
) -> Option<&'a MobSpawnerDef> {
    let total: i32 = table.iter().map(|entry| entry.weight).sum();
    if total <= 0 {
        return None;
    }
    let mut selection = random.next_int(total);
    for entry in table {
        selection -= entry.weight;
        if selection < 0 {
            return Some(entry);
        }
    }
    None
}

/// `IntProvider.sample`：官方刷怪表里只有常量和均匀分布。
///
/// `IntProvider.sample`: the vanilla spawn tables only use constants and uniform ranges.
pub fn sample(entry: &MobSpawnerDef, random: &dyn crate::random::RandomSource) -> i32 {
    let span = entry.max - entry.min + 1;
    if span <= 1 {
        entry.min
    } else {
        entry.min + random.next_int(span)
    }
}

/// 已支持的自然生成生物使用 `SpawnPlacements` 的 `ON_GROUND`。
///
/// The modelled natural spawns use the `ON_GROUND` placement.
fn spawn_placement_ok(world: &World, kind: MobType, pos: BlockPos) -> bool {
    // 溺尸登记的是 `SpawnPlacementTypes.IN_WATER`，其余已支持的都是 `ON_GROUND`。
    // Drowned register `SpawnPlacementTypes.IN_WATER`; the rest use `ON_GROUND`.
    if kind == MobType::Drowned {
        return super::drowned::placement_ok(world, pos);
    }
    // 水生生物登记的是 `SpawnPlacementTypes.IN_WATER`。
    // Aquatic mobs are registered with `SpawnPlacementTypes.IN_WATER`.
    if kind.spawns_in_water() {
        return super::water::in_water_position_ok(world, pos);
    }
    // 狐狸和幻翼注册的是 `NO_RESTRICTIONS`，位置恒可用。
    // Foxes and phantoms are registered with `NO_RESTRICTIONS`, which accepts every position.
    matches!(kind, MobType::Fox | MobType::Phantom)
        || on_ground_position_ok_for(
            world,
            kind.entity_type(),
            kind.fire_immune(),
            kind.immune_to(),
            pos,
        )
}

/// `SpawnPlacementTypes.ON_GROUND.isSpawnPositionOk`：按实体类型检查地面和空位。
///
/// `SpawnPlacementTypes.ON_GROUND.isSpawnPositionOk`: check the footing and empty
/// space for the type.
pub(super) fn on_ground_position_ok(
    world: &World,
    entity_type: i32,
    fire_immune: bool,
    pos: BlockPos,
) -> bool {
    on_ground_position_ok_for(world, entity_type, fire_immune, None, pos)
}

/// 同上，另带 `EntityType.immuneTo` 的方块标签。
///
/// As above, with the `EntityType.immuneTo` block tag.
fn on_ground_position_ok_for(
    world: &World,
    entity_type: i32,
    fire_immune: bool,
    immune_to: Option<mc_bamboo_data::BlockTag>,
    pos: BlockPos,
) -> bool {
    if !world.world_border().is_within_bounds_pos(pos) {
        return false;
    }
    let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    if !world.block_state(below).is_valid_spawn_surface(entity_type) {
        return false;
    }
    is_valid_empty_spawn_block(world, fire_immune, immune_to, pos)
        && is_valid_empty_spawn_block(world, fire_immune, immune_to, above)
}

/// `NaturalSpawner.isValidEmptySpawnBlock` 对单独一格：自定义刷怪器在半空中放幻翼时
/// 只查落点自己，不查落脚方块。
///
/// `NaturalSpawner.isValidEmptySpawnBlock` for one cell: the custom spawner drops
/// phantoms in mid-air and checks the cell itself, not the block below.
pub(crate) fn is_valid_empty_spawn_block_for(world: &World, kind: MobType, pos: BlockPos) -> bool {
    is_valid_empty_spawn_block(world, kind.fire_immune(), kind.immune_to(), pos)
}

/// `NaturalSpawner.isValidEmptySpawnBlock`，给掠夺者巡逻队的落点判定用。
/// 掠夺者不防火、没有 `immuneTo` 标签，两种参数都是默认分支。
///
/// `NaturalSpawner.isValidEmptySpawnBlock` for a patrol landing spot. Pillagers
/// are not fire-immune and have no `immuneTo` tag, so both flags take the default.
pub(super) fn patrol_spawn_block_ok(world: &World, pos: BlockPos) -> bool {
    is_valid_empty_spawn_block(world, false, MobType::Pillager.immune_to(), pos)
}

/// `NaturalSpawner.isValidEmptySpawnBlock`。
fn is_valid_empty_spawn_block(
    world: &World,
    fire_immune: bool,
    immune_to: Option<mc_bamboo_data::BlockTag>,
    pos: BlockPos,
) -> bool {
    let state = world.block_state(pos);
    if state.is_air() {
        return true;
    }
    if state.is_collision_shape_full_block() {
        return false;
    }
    if state.is_signal_source() {
        return false;
    }
    if !world.fluid_state(pos).is_empty() {
        return false;
    }
    if state.is_tagged(block_tags::PREVENT_MOB_SPAWNING_INSIDE) {
        return false;
    }
    // `EntityType.isBlockDangerous` 开头的 `immuneTo`：标签里的方块直接不算危险。
    // The `immuneTo` check at the head of `EntityType.isBlockDangerous`.
    if immune_to.is_some_and(|tag| state.is_tagged(tag)) {
        return true;
    }
    !is_block_dangerous(fire_immune, state)
}

/// `EntityType.isBlockDangerous`：烧着的方块、凋灵玫瑰、甜浆果丛、仙人掌、细雪。
/// 防火的类型只是不怕烧着的方块，其余几种照样危险；`immuneTo` 标签由调用方先查。
///
/// Fire-immune types only ignore burning blocks; the rest stay dangerous. The caller
/// checks the `immuneTo` tag first.
fn is_block_dangerous(fire_immune: bool, state: BlockState) -> bool {
    (!fire_immune && is_burning_block(state))
        || state.0 == mc_bamboo_data::blocks::WITHER_ROSE
        || state.0 == mc_bamboo_data::blocks::SWEET_BERRY_BUSH
        || state.0 == CACTUS
        || state.0 == POWDER_SNOW
}

/// `NodeEvaluator.isBurningBlock`。
fn is_burning_block(state: BlockState) -> bool {
    state.is_tagged(block_tags::FIRE)
        || state.0 == LAVA
        || state.0 == MAGMA_BLOCK
        || (state.is_a(BlockAncestor::CampfireBlock) && state.get(LIT))
        || state.0 == LAVA_CAULDRON
}

/// `SpawnPlacements.checkSpawnRules`：`NATURAL` 走两条分支，动物看亮度和地面，
/// 怪物看黑暗和可生成方块（尸壳还要看得见天）。
pub(super) fn check_spawn_rules(
    world: &World,
    kind: MobType,
    pos: BlockPos,
    sea_level: i32,
) -> bool {
    if !kind.allowed_in_peaceful() && world.level().difficulty_id() == 0 {
        return false;
    }
    match kind {
        MobType::Squid | MobType::GlowSquid => {
            super::squid::check_spawn_rules(world, kind, pos, sea_level)
        }
        MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish => {
            super::fish::check_spawn_rules(world, kind, pos, sea_level)
        }
        // `Dolphin` 用 `checkSurfaceAgeableWaterCreatureSpawnRules`（和鱿鱼同源）。
        // The dolphin uses `checkSurfaceAgeableWaterCreatureSpawnRules` (same as squid).
        MobType::Dolphin => super::dolphin::check_spawn_rules(world, sea_level, pos),
        // `Axolotl.checkAxolotlSpawnRules`：脚下是 `axolotls_spawnable_on`（黏土）。
        // `Axolotl.checkAxolotlSpawnRules`: the block below is in
        // `axolotls_spawnable_on` (clay).
        MobType::Axolotl => world
            .block_state(pos.below())
            .is_tagged(block_tags::AXOLOTLS_SPAWNABLE_ON),
        // `Turtle.checkTurtleSpawnRules`：海平面 +4 以下、沙子上、亮度足够。
        // `Turtle.checkTurtleSpawnRules`: below sea level + 4, on sand and bright.
        MobType::Turtle => {
            pos.y < sea_level + 4
                && world.block_state(pos.below()).is_tagged(block_tags::SAND)
                && world.raw_brightness(pos) > 8
        }
        // `Rabbit.checkRabbitSpawnRules`、`Fox.checkFoxSpawnRules`、`Wolf.checkWolfSpawnRules`：
        // 只是脚下的标签各不相同。
        // The three only differ in the tag of the block below.
        MobType::Rabbit => bright_spawn_rules(world, pos, block_tags::RABBITS_SPAWNABLE_ON),
        MobType::Fox => bright_spawn_rules(world, pos, block_tags::FOXES_SPAWNABLE_ON),
        MobType::Wolf => bright_spawn_rules(world, pos, block_tags::WOLVES_SPAWNABLE_ON),
        // `Armadillo.checkArmadilloSpawnRules`。
        // `Armadillo.checkArmadilloSpawnRules`.
        MobType::Armadillo => bright_spawn_rules(world, pos, block_tags::ARMADILLO_SPAWNABLE_ON),
        // `Goat.checkGoatSpawnRules`。
        // `Goat.checkGoatSpawnRules`.
        MobType::Goat => bright_spawn_rules(world, pos, block_tags::GOATS_SPAWNABLE_ON),
        // `Frog.checkFrogSpawnRules`。
        // `Frog.checkFrogSpawnRules`.
        MobType::Frog => bright_spawn_rules(world, pos, block_tags::FROGS_SPAWNABLE_ON),
        // `Camel.checkCamelSpawnRules`：`#camels_spawnable_on` + 亮度。
        // `Camel.checkCamelSpawnRules`: `#camels_spawnable_on` plus brightness.
        MobType::Camel => bright_spawn_rules(world, pos, block_tags::CAMELS_SPAWNABLE_ON),
        // 蝌蚪没有登记 `SpawnPlacements`：没有规则，恒真。
        // Tadpoles have no `SpawnPlacements` entry: no rule, always true.
        MobType::Tadpole => true,
        MobType::Cow
        | MobType::Pig
        | MobType::Sheep
        | MobType::Chicken
        | MobType::Horse
        | MobType::Donkey
        | MobType::Mule
        | MobType::Llama
        | MobType::SkeletonHorse
        | MobType::ZombieHorse
        | MobType::Cat => animal_spawn_rules(world, pos),
        MobType::Mooshroom => super::mooshroom::spawn_rules(world, pos),
        MobType::PolarBear => super::polar_bear::spawn_rules(world, pos),
        MobType::Panda => super::panda::spawn_rules(world, pos),
        MobType::Ocelot => super::ocelot::spawn_rules(world.random()),
        // `Parrot.checkParrotSpawnRules`：`#parrots_spawnable_on` 加亮度。
        // `Parrot.checkParrotSpawnRules`: `#parrots_spawnable_on` plus brightness.
        MobType::Parrot => super::parrot::spawn_rules(world, pos),
        MobType::Zombie
        | MobType::ZombieVillager
        | MobType::Skeleton
        | MobType::Bogged
        | MobType::Witch
        | MobType::Creeper
        | MobType::Spider
        | MobType::CaveSpider
        | MobType::Enderman => {
            is_dark_enough_to_spawn(world, pos) && valid_mob_spawn_block(world, kind, pos)
        }
        // `Stray.checkStraySpawnRules`：怪物规则之后再要求看得见天；找天只读方块，不取随机数。
        // `Stray.checkStraySpawnRules`: the monster rules, then open sky; the sky search
        // only reads blocks and draws nothing.
        MobType::Stray => {
            is_dark_enough_to_spawn(world, pos)
                && valid_mob_spawn_block(world, kind, pos)
                && super::stray::sees_sky_through_powder_snow(world, pos)
        }
        // `PatrollingMonster.checkPatrollingMonsterSpawnRules`：方块光大于 8 不刷，其余走
        // `checkAnyLightMonsterSpawnRules`（不看天色，只看脚下能不能站；和平难度在函数开头
        // 已经挡掉）。
        // `PatrollingMonster.checkPatrollingMonsterSpawnRules`: no spawn above block light
        // 8, otherwise `checkAnyLightMonsterSpawnRules` (no darkness test, only the footing;
        // peaceful is rejected at the top of this function).
        MobType::Pillager => super::pillager::check_spawn_rules(world, pos),
        MobType::Slime => super::slime::check_spawn_rules(world, pos),
        MobType::Bat => super::bat::check_spawn_rules(world, pos),
        // 幻翼登记的是 `Mob::checkMobSpawnRules`；它不在群系刷怪表里，靠 `PhantomSpawner`。
        // Phantoms register `Mob::checkMobSpawnRules`; they are in no biome table and
        // come from the `PhantomSpawner`.
        MobType::Phantom => valid_mob_spawn_block(world, kind, pos),
        // 蜜蜂不在任何群系刷怪表里，也没有 `SpawnPlacements` 规则。
        // Bees are in no biome spawn table and have no `SpawnPlacements` rule.
        MobType::Bee => true,
        // 蠹虫和末影螨都不在群系刷怪表里：刷怪笼 / `/summon` / 刷怪蛋才生成。
        // 官方 `checkSilverfishSpawnRules` / `checkEndermiteSpawnRules` 只在刷怪笼
        // （`EntitySpawnReason.isSpawner`）或 5 格内没有玩家时允许，这里不给自然刷怪用。
        //
        // Neither silverfish nor endermites appear in biome spawn tables. The vanilla
        // spawn rules only allow spawners or places with no player within five blocks;
        // neither path goes through natural spawning here.
        MobType::Silverfish | MobType::Endermite => valid_mob_spawn_block(world, kind, pos),
        MobType::Drowned => super::drowned::check_spawn_rules(world, pos),
        MobType::Husk => {
            is_dark_enough_to_spawn(world, pos)
                && valid_mob_spawn_block(world, kind, pos)
                && world.level().can_see_sky(pos)
        }
        // 村民不在任何群系刷怪表里，只由结构 / 命令 / 刷怪蛋生成。
        // Villagers are in no biome spawn table; structures, commands and eggs make them.
        MobType::Villager => true,
        // 守卫者的自然生成规则（`checkGuardianSpawnRules` / 海底神殿）属于后续段落；
        // 这里先按"不在群系刷怪表"处理，由命令 / 刷怪蛋 / 结构生成。
        // Guardian natural-spawn rules (`checkGuardianSpawnRules` / ocean monuments) belong to
        // a later phase; treat them as absent from biome tables for now (commands, eggs,
        // structures).
        // `SulfurCube.checkSulfurCubeSpawnRules` 恒真：没有亮度 / 群系门槛。
        // `SulfurCube.checkSulfurCubeSpawnRules` is always true: no light or biome gate.
        MobType::SulfurCube => true,
        MobType::Guardian | MobType::ElderGuardian => true,
        // 两种傀儡都不在群系刷怪表里，只保留命令 / 方块图案生成时共用的规则。
        // Neither golem is in a biome spawn table; only the shared rule remains.
        MobType::IronGolem | MobType::SnowGolem => animal_spawn_rules(world, pos),
    }
}

/// `Animal.checkAnimalSpawnRules`：脚下在 `animals_spawnable_on` 里，且 `getRawBrightness(pos, 0) > 8`。
///
/// `Animal.checkAnimalSpawnRules`: the block below is `animals_spawnable_on` and
/// `getRawBrightness(pos, 0) > 8`.
pub(super) fn animal_spawn_rules(world: &World, pos: BlockPos) -> bool {
    bright_spawn_rules(world, pos, block_tags::ANIMALS_SPAWNABLE_ON)
}

/// 脚下的方块在 `spawnable_on` 里，且 `Animal.isBrightEnoughToSpawn`
/// （`getRawBrightness(pos, 0) > 8`）。
///
/// The block below is in `spawnable_on` and `Animal.isBrightEnoughToSpawn` holds
/// (`getRawBrightness(pos, 0) > 8`).
pub(super) fn bright_spawn_rules(
    world: &World,
    pos: BlockPos,
    spawnable_on: mc_bamboo_data::BlockTag,
) -> bool {
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    world.block_state(below).is_tagged(spawnable_on) && world.raw_brightness(pos) > 8
}

/// `Monster.isDarkEnoughToSpawn`。
pub(super) fn is_dark_enough_to_spawn(world: &World, pos: BlockPos) -> bool {
    let random = world.random();
    if world.brightness(LightLayer::Sky, pos) > random.next_int(32) {
        return false;
    }
    // 主世界 `monster_spawn_block_light_limit` 是 0：方块光不能有。
    if world.brightness(LightLayer::Block, pos) > 0 {
        return false;
    }
    let brightness = if world.is_thundering() {
        world.max_local_raw_brightness_with(pos, 10)
    } else {
        world.max_local_raw_brightness(pos)
    };
    // 主世界 `monster_spawn_light_level` 是 `uniform(0, 7)`。
    brightness <= random.next_int(8)
}

/// `Mob.checkMobSpawnRules` 里的 `isValidSpawn`，`NATURAL` 不是刷怪笼。
pub(super) fn valid_mob_spawn_block(world: &World, kind: MobType, pos: BlockPos) -> bool {
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    if kind.fire_immune() {
        return true;
    }
    world
        .block_state(below)
        .is_valid_spawn_surface(kind.entity_type())
}

/// `NaturalSpawner.isValidPositionForMob`。
fn is_valid_position_for_mob(
    world: &World,
    entity: &Entity,
    kind: MobType,
    nearest_player_distance_sqr: f64,
    sea_level: i32,
) -> bool {
    let despawn = f64::from(kind.mob_category().facts().despawn_distance);
    // 新生成的鱼还没有桶标记和名字，`removeWhenFarAway` 为真；新生成的豹猫 `tickCount == 0`，为假。
    // A fresh fish has no bucket flag or name (`removeWhenFarAway` is true); a fresh ocelot
    // has `tickCount == 0` (false).
    // 官方把最近玩家距离传给 `removeWhenFarAway(distSqr)`；巡逻中的袭击者按它判定 128 格。
    // Vanilla passes the nearest-player distance to `removeWhenFarAway(distSqr)`; a
    // patrolling raider tests the 128-block threshold against it.
    let removes = entity.kind.mob().is_some_and(|mob| {
        mob.removes_when_far_away_at(nearest_player_distance_sqr, entity.tick_count)
    });
    if nearest_player_distance_sqr > despawn * despawn && removes {
        return false;
    }
    if kind == MobType::Ocelot {
        return pathfinder_spawn_rules(world, entity, entity.block_position())
            && super::ocelot::check_spawn_obstruction(world, entity, sea_level);
    }
    // `Drowned.checkSpawnObstruction` 只检查实体遮挡；水生生物（`AgeableWaterCreature`
    // 和 `WaterAnimal`）也只剩 `isUnobstructed`，不查液体；其余生物还检查盒内液体。
    //
    // `Drowned.checkSpawnObstruction` checks only entity obstruction; aquatic mobs
    // (`AgeableWaterCreature` and `WaterAnimal`) likewise keep `isUnobstructed` alone,
    // while ordinary mobs also reject liquid in the box.
    let bounds = entity.bounding_box();
    pathfinder_spawn_rules(world, entity, entity.block_position())
        && (kind == MobType::Drowned
            || kind.is_ageable_water_creature()
            || kind.is_water_animal()
            // 美西螈的 `checkSpawnObstruction` 也只剩 `isUnobstructed`，不查液体。
            // The axolotl's `checkSpawnObstruction` is `isUnobstructed` alone, no liquid check.
            || kind == MobType::Axolotl
            || !contains_any_liquid(world, bounds))
        && is_unobstructed_by_entities(world, bounds)
}

/// `EntityGetter.isUnobstructed(entity)`：盒子里没有未移除、`blocksBuilding` 的实体。
/// 新实体还没进表，不会挡住自己；玩家由 `Level::is_unobstructed` 负责。
///
/// `EntityGetter.isUnobstructed(entity)`: no live `blocksBuilding` entity overlaps the box.
/// The new mob is not in the table yet, so it cannot block itself; players are checked
/// through `Level::is_unobstructed`.
pub(super) fn is_unobstructed_by_entities(world: &World, box_: Aabb) -> bool {
    let mut blocked = false;
    world
        .level()
        .entities_table()
        .for_each_in(box_, &mut |public| {
            blocked |= !public.removed && blocks_building(&public.kind);
        });
    !blocked && world.level().is_unobstructed(&[box_])
}

/// `Entity.blocksBuilding`：生物（`LivingEntity`）、TNT 和下落方块为真；物品、经验球、箭为假。
///
/// `Entity.blocksBuilding`: true for living entities, primed TNT and falling blocks;
/// false for items, experience orbs and arrows.
fn blocks_building(kind: &KindPublic) -> bool {
    matches!(
        kind,
        KindPublic::Mob(_) | KindPublic::Tnt { .. } | KindPublic::FallingBlock { .. }
    )
}

/// `PathfinderMob.checkSpawnRules`：`getWalkTargetValue(pos) >= 0`。牛看脚下是不是草方块，
/// 否则按亮度算路径代价；僵尸取负的代价。
///
/// `PathfinderMob.checkSpawnRules`: `getWalkTargetValue(pos) >= 0`. Cows look at the
/// block below and otherwise use the light-based path cost; monsters negate that cost.
pub(super) fn pathfinder_spawn_rules(world: &World, entity: &Entity, pos: BlockPos) -> bool {
    let Some(mob) = entity.kind.mob() else {
        return true;
    };
    super::goals::walk_target_value(mob, world, pos) >= 0.0
}

/// `EntityType.getSpawnAABB(x, y, z)`：以 (x, y, z) 为脚底中心的身体盒。
pub(super) fn spawn_aabb(kind: MobType, pos: BlockPos) -> Aabb {
    let (mut width, mut height, _) = kind.dimensions(false);
    // `EntityType.spawnDimensionsScale`：只有史莱姆不是 1。
    // `EntityType.spawnDimensionsScale`: 1 for everything but slimes.
    if kind == MobType::Slime {
        width *= super::slime::SPAWN_DIMENSIONS_SCALE;
        height *= super::slime::SPAWN_DIMENSIONS_SCALE;
    }
    let half = f64::from(width / 2.0);
    let x = f64::from(pos.x) + 0.5;
    let y = f64::from(pos.y);
    let z = f64::from(pos.z) + 0.5;
    Aabb::new(
        x - half,
        y,
        z - half,
        x + half,
        y + f64::from(height),
        z + half,
    )
}

/// `Level.containsAnyLiquid(AABB)`。
pub(super) fn contains_any_liquid(world: &World, box_: Aabb) -> bool {
    let min = BlockPos::new(
        box_.min(Axis::X).floor() as i32,
        box_.min(Axis::Y).floor() as i32,
        box_.min(Axis::Z).floor() as i32,
    );
    let max = BlockPos::new(
        box_.max(Axis::X).floor() as i32,
        box_.max(Axis::Y).floor() as i32,
        box_.max(Axis::Z).floor() as i32,
    );
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                if !world.fluid_state(BlockPos::new(x, y, z)).is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// `CollisionGetter.noCollision(AABB)`：没有方块碰撞，也没有 `canBeCollidedWith` 的硬碰撞实体
/// （船、矿车、潜影贝）。玩家和生物不算硬碰撞体；来源为空时不查世界边界。
///
/// `CollisionGetter.noCollision(AABB)`: no block collision and no hard-colliding entity
/// (boats, minecarts, shulkers). Players and mobs are not hard colliders; a null source
/// skips the world border.
pub(super) fn no_collision(world: &World, box_: Aabb) -> bool {
    if !world
        .block_collisions(box_, CollisionContext::Empty)
        .is_empty()
    {
        return false;
    }
    let mut blocked = false;
    world.level().entities_table().for_each_in(
        box_.inflate(1.0e-7, 1.0e-7, 1.0e-7),
        &mut |public| {
            blocked |= crate::entity::store::is_hard_colliding(public);
        },
    );
    !blocked
}

/// `EntityType.create(level, NATURAL)`：Bamboo 只构造自己建模的几种。
pub(super) fn create_mob(world: &mut World, kind: MobType) -> Option<Entity> {
    Some(world.prepare_entity(Entity::by_mob_type(kind, Vec3::default())))
}

/// `Mob.getMaxSpawnClusterSize()`，按物种使用各自的簇上限。
///
/// `Mob.getMaxSpawnClusterSize()` with per-species cluster limits.
fn max_spawn_cluster_size(kind: MobType) -> i32 {
    if kind.is_water_animal() {
        super::fish::max_school_size(kind)
    } else if kind.is_equine() {
        6
    } else if kind == MobType::Wolf {
        // `Wolf.getMaxSpawnClusterSize` 是 8，其余用 `Mob` 的 4。
        // `Wolf.getMaxSpawnClusterSize` is 8; the rest use the `Mob` default of 4.
        8
    } else if kind == MobType::Pillager {
        // `Pillager.getMaxSpawnClusterSize` 是 1。
        // `Pillager.getMaxSpawnClusterSize` is 1.
        1
    } else {
        4
    }
}

/// `Mob.isMaxGroupSizeReached`：只有热带鱼覆盖成「不在鱼群里就停」。
///
/// `Mob.isMaxGroupSizeReached`: only tropical fish override it, to `!isSchool`.
fn is_max_group_size_reached(kind: MobType, mob: Option<&super::Mob>) -> bool {
    kind == MobType::TropicalFish && mob.is_some_and(|mob| !mob.fish.is_school)
}

/// `ChunkMap.playerIsCloseEnoughForSpawning` 的几何条件：水平距区块中心小于 128 格。
///
/// The geometric part of `ChunkMap.playerIsCloseEnoughForSpawning`: horizontal
/// distance from the chunk centre is strictly less than 128 blocks.
pub fn is_close_to_spawn_chunk(position: Vec3, chunk: ChunkPos) -> bool {
    let cx = f64::from(chunk.x * 16 + 8);
    let cz = f64::from(chunk.z * 16 + 8);
    let dx = cx - position.x;
    let dz = cz - position.z;
    dx * dx + dz * dz < 16384.0
}

/// `Level.getNearestPlayer(x, y, z, -1.0, false)` 的距离平方：只排除旁观者，
/// 创造模式玩家照官方一样算数。[`SpawnState`] 在这一刻开头已经滤掉旁观者存好了视图，
/// 直接遍历切片，不在每个候选步上重新经虚调用构造玩家视图。
///
/// The squared distance of `Level.getNearestPlayer(x, y, z, -1.0, false)`: only
/// spectators are skipped, creative players count as in vanilla. [`SpawnState`]
/// already holds the non-spectator player views for this tick.
fn nearest_player_distance_sqr(players: &[PlayerView], x: f64, y: f64, z: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for player in players {
        let dx = player.position.x - x;
        let dy = player.position.y - y;
        let dz = player.position.z - z;
        let distance = dx * dx + dy * dy + dz * dz;
        best = Some(best.map_or(distance, |current| current.min(distance)));
    }
    best
}

/// 群系标签查询，只有刷怪相关的两个标签用得到。
pub(super) fn biome_tag_contains(name: &str, biome: i32) -> bool {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .filter(|group| group.registry == "minecraft:worldgen/biome")
        .flat_map(|group| group.tags.iter())
        .any(|tag| tag.name == name && tag.ids.contains(&biome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::blocks;
    use mc_bamboo_data::registries::entity_type as types;

    use crate::{BlockState, BlockTicks, entity::Entity, testing::TestLevel};

    fn player(uid: i32, position: Vec3) -> PlayerView {
        PlayerView {
            uuid: None,
            uid,
            yaw: 0.0,
            pitch: 0.0,
            position,
            delta: Vec3::default(),
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::default(),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            combat: Default::default(),
            effects: Default::default(),
        }
    }

    /// 一块 3×3 区块的草地，顶面在 y=63。
    fn grass_platform(level: &mut TestLevel) {
        for x in -24..40 {
            for z in -24..40 {
                level.set(BlockPos::new(x, 62, z), BlockState(blocks::DIRT));
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK));
            }
        }
    }

    fn params(spawnable_chunk_count: i32, per_player_cap: bool) -> SpawnParams {
        SpawnParams {
            spawnable_chunk_count,
            spawn_enemies: true,
            per_player_cap,
            respawn_pos: None,
            sea_level: 63,
        }
    }

    /// 跑一刻：建 state、挑类别、对给定区块刷怪。
    fn run_tick(level: &mut TestLevel, chunk: ChunkPos, params: &SpawnParams) -> i32 {
        let mut ticks = BlockTicks::new();
        let mut state = {
            let world = World::new(level, &mut ticks);
            SpawnState::create(&world, params.spawnable_chunk_count)
        };
        let spawn_persistent = world_time(&mut ticks) % 400 == 0;
        let categories = state.filtered_categories(params.spawn_enemies, spawn_persistent);
        if categories.is_empty() {
            return 0;
        }
        let before = level.entities().len();
        let mut world = World::new(level, &mut ticks);
        spawn_for_chunk(&mut world, chunk, &mut state, params, &categories);
        (level.entities().len() - before) as i32
    }

    fn world_time(_ticks: &mut BlockTicks) -> i64 {
        0
    }

    #[test]
    fn create_counts_mobs_per_category() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        level.players.push(player(1, Vec3::new(8.5, 64.0, 8.5)));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.spawn_entity(Entity::cow(Vec3::new(8.5, 64.0, 8.5)));
        world.spawn_entity(Entity::by_mob_type(
            MobType::Zombie,
            Vec3::new(9.5, 64.0, 9.5),
        ));
        let state = SpawnState::create(&world, 289);
        assert_eq!(state.count(MobCategory::Creature), 1);
        assert_eq!(state.count(MobCategory::Monster), 1);
        assert_eq!(state.count(MobCategory::WaterCreature), 0);
    }

    #[test]
    fn spawn_obstruction_ignores_liquid_beyond_the_bounding_box() {
        let mut level = TestLevel::new();
        let liquid = BlockPos::new(1, 64, 0);
        level.set(liquid, BlockState(blocks::WATER));
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut level, &mut ticks);
        assert!(!world.fluid_state(liquid).is_empty());
        assert!(!contains_any_liquid(
            &world,
            Aabb::new(0.1, 64.0, 0.1, 0.9, 65.4, 0.9)
        ));
        assert!(contains_any_liquid(
            &world,
            Aabb::new(0.1, 64.0, 0.1, 1.0, 65.4, 0.9)
        ));
    }

    #[test]
    fn farm_animals_spawn_on_open_grass_at_day() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        // 玩家在 32 格外：过了 24 格的生成距离，又足够近让人算「附近」。
        level.players.push(player(1, Vec3::new(8.5, 64.0, 40.5)));
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        let mut spawned = 0;
        for _ in 0..3000 {
            spawned += run_tick(&mut level, chunk, &params);
            if spawned > 0 {
                break;
            }
        }
        assert!(spawned > 0, "平原草地上应该刷出牛");
        // 平原的 `CREATURE` 表里建模了的有牛、猪、羊、鸡，加上马科合并带进来的马和驴；
        // 白天不会有怪物。
        // The modelled plains creatures are cows, pigs, sheep and chickens, plus the
        // horses and donkeys from the equine merge; no monster spawns by day.
        for entity in level.entities().iter() {
            let mob = entity.kind.mob().expect("只会有生物");
            // 平原白天的 `CREATURE` 表里建模了的就这六种（含马、驴）。
            // These six are the modelled daytime plains `CREATURE` entries (horses and
            // donkeys included).
            assert!(
                matches!(
                    mob.mob_type,
                    MobType::Cow
                        | MobType::Pig
                        | MobType::Sheep
                        | MobType::Chicken
                        | MobType::Horse
                        | MobType::Donkey
                ),
                "白天只刷出平原的被动动物，实际是 {:?}",
                mob.mob_type
            );
        }
    }

    #[test]
    fn no_spawn_within_twenty_four_blocks_of_a_player() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        // 玩家站在区块中心：整个区块都在 24 格的保护区里。
        level.players.push(player(1, Vec3::new(8.5, 64.0, 8.5)));
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        for _ in 0..200 {
            assert_eq!(run_tick(&mut level, chunk, &params), 0);
        }
    }

    #[test]
    fn no_spawn_without_a_player() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        for _ in 0..50 {
            assert_eq!(run_tick(&mut level, chunk, &params), 0);
        }
    }

    #[test]
    fn creative_players_still_keep_mobs_away() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        // 官方的 `getNearestPlayer(..., false)` 只排除旁观者，创造模式玩家照样算最近的玩家。
        // Vanilla's `getNearestPlayer(..., false)` only skips spectators, so a creative
        // player is still the nearest player.
        let mut creative = player(1, Vec3::new(8.5, 64.0, 8.5));
        creative.creative = true;
        level.players.push(creative);
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        for _ in 0..200 {
            assert_eq!(run_tick(&mut level, chunk, &params), 0);
        }
    }

    #[test]
    fn cows_need_grass_or_bright_light_to_stay() {
        let mut level = TestLevel::new();
        level.set(BlockPos::new(0, 62, 0), BlockState(blocks::DIRT));
        // 亮度 15：`getLightLevelDependentMagicValue` 给 1，路径代价 0.5，牛可以留下。
        // Light 15: the magic value is 1, so the light path cost is 0.5 and a cow stays.
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut level, &mut ticks);
        let cow = Entity::cow(Vec3::new(0.5, 63.0, 0.5));
        assert!(pathfinder_spawn_rules(
            &world,
            &cow,
            BlockPos::new(0, 63, 0)
        ));

        let mut dark = TestLevel::new();
        dark.set(BlockPos::new(0, 62, 0), BlockState(blocks::DIRT));
        dark.light = Some((9, 0));
        // 亮度 9 在泥土上：代价转负，`PathfinderMob.checkSpawnRules` 不通过。
        // Light 9 over dirt: the cost turns negative and `PathfinderMob.checkSpawnRules`
        // fails.
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut dark, &mut ticks);
        assert!(!pathfinder_spawn_rules(
            &world,
            &cow,
            BlockPos::new(0, 63, 0)
        ));
    }

    #[test]
    fn the_spawn_surface_follows_is_valid_spawn() {
        // 草方块能当落脚块；树叶和玻璃换掉了 `isValidSpawn` 的默认实现，不能。
        // Grass is a valid footing block; leaves and glass replace the default
        // `isValidSpawn` and are not.
        let cow = MobType::Cow.entity_type();
        let bear = MobType::PolarBear.entity_type();
        assert!(BlockState(blocks::GRASS_BLOCK).is_valid_spawn_surface(cow));
        assert!(!BlockState(blocks::OAK_LEAVES).is_valid_spawn_surface(cow));
        assert!(!BlockState(blocks::GLASS).is_valid_spawn_surface(cow));
        for ice in [blocks::ICE, blocks::FROSTED_ICE] {
            assert!(BlockState(ice).is_valid_spawn_surface(bear));
            assert!(!BlockState(ice).is_valid_spawn_surface(cow));
        }
    }

    #[test]
    fn monsters_need_darkness_to_stay() {
        let mut lit = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut lit, &mut ticks);
        let zombie = Entity::zombie(Vec3::new(0.5, 63.0, 0.5));
        // 亮度 15 下 `Monster.getWalkTargetValue` 是负的。
        // At light 15 `Monster.getWalkTargetValue` is negative.
        assert!(!pathfinder_spawn_rules(
            &world,
            &zombie,
            BlockPos::new(0, 63, 0)
        ));

        let mut dark = TestLevel::new();
        dark.light = Some((0, 0));
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut dark, &mut ticks);
        assert!(pathfinder_spawn_rules(
            &world,
            &zombie,
            BlockPos::new(0, 63, 0)
        ));
    }

    #[test]
    fn the_global_cap_stops_the_category() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        level.players.push(player(1, Vec3::new(8.5, 64.0, 40.5)));
        // `10 * 1 / 289` 是 0：一个区块的计数下，创造类一格也刷不出来。
        let params = params(1, true);
        let chunk = ChunkPos::new(0, 0);
        for _ in 0..50 {
            assert_eq!(run_tick(&mut level, chunk, &params), 0);
        }
    }

    #[test]
    fn zombies_and_skeletons_can_spawn_when_it_is_dark() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        level.light = Some((0, 0));
        level.players.push(player(1, Vec3::new(8.5, 64.0, 40.5)));
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        let mut zombies = false;
        let mut skeletons = false;
        for _ in 0..3000 {
            run_tick(&mut level, chunk, &params);
            for entity in level.entities().iter() {
                let mob = entity.kind.mob().expect("natural spawn must be a mob");
                assert_eq!(mob.mob_type.mob_category(), MobCategory::Monster);
                zombies |= mob.mob_type.entity_type() == types::ZOMBIE;
                skeletons |= mob.mob_type.entity_type() == types::SKELETON;
            }
            if zombies && skeletons {
                break;
            }
        }
        assert!(zombies, "黑暗里应该刷出僵尸");
        assert!(skeletons, "黑暗里应该刷出骷髅");
    }

    /// 苦力怕、蜘蛛和末影人走同一条怪物分支，黑暗里也会自然刷出来。
    ///
    /// Creepers, spiders and endermen take the same monster branch and spawn in the dark.
    #[test]
    fn creepers_spiders_and_endermen_spawn_in_the_dark() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        level.light = Some((0, 0));
        level.players.push(player(1, Vec3::new(8.5, 64.0, 40.5)));
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        let wanted = [types::CREEPER, types::SPIDER, types::ENDERMAN];
        let mut seen = [false; 3];
        // 平原的末影人权重只有 10（苦力怕、蜘蛛各 100），要多跑一些刻；找齐就停。
        // Plains endermen weigh only 10 (creepers and spiders 100), so allow many
        // ticks; the loop stops once all three appeared.
        for _ in 0..40000 {
            run_tick(&mut level, chunk, &params);
            let spawned: Vec<_> = level
                .entities()
                .iter_public()
                .map(|entity| (entity.uid, entity.kind.entity_type()))
                .collect();
            for (uid, entity_type) in spawned {
                if let Some(index) = wanted.iter().position(|&id| id == entity_type) {
                    seen[index] = true;
                }
                // 刷出来的都清掉，怪物上限就不会被别的种类占满。
                // Clear every spawn so other kinds never fill the monster cap.
                level.entities_mut_for_test().remove(uid);
            }
            if seen.iter().all(|&found| found) {
                break;
            }
        }
        assert_eq!(seen, [true; 3], "黑暗里应该刷出苦力怕、蜘蛛和末影人");
    }

    #[test]
    fn shuffle_matches_java_fisher_yates() {
        // `java.util.Random(42)` 打乱 0..8 的结果，由 `Collections.shuffle` 的同款算法给出：
        // 从末尾往前走，取 `nextInt(index + 1)` 交换。
        let random = crate::random::LegacyRandom::new(42);
        let mut items: Vec<u8> = (0..8).collect();
        crate::random::shuffle(&random, &mut items);
        assert_eq!(items.len(), 8);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        // 同一个种子必须给出同一个排列。
        let random = crate::random::LegacyRandom::new(42);
        let mut again: Vec<u8> = (0..8).collect();
        crate::random::shuffle(&random, &mut again);
        assert_eq!(items, again);
    }
}
