//! 自然生成（官方 `NaturalSpawner`）。
//!
//! 每刻从本岛实体表重算 [`SpawnState`]：按 `MobCategory` 计数、算刷怪代价
//! （`PotentialCalculator`）和每个玩家附近的计数（`LocalMobCapCalculator` 的等价物）。
//! 然后按类别挑出还能刷的类别（全局上限 `maxInstancesPerChunk * spawnableChunkCount / 289`），
//! 对刷怪区块逐个类别取一个随机起点、在 3 组 × 4 次的尝试里按群系的加权表挑一种生物，
//! 检查距离、光照、方块和碰撞条件，最后 `finalizeSpawn` 并按组生成。
//!
//! 随机数的取数顺序照官方：`getRandomPosWithin` 三次、每组一次 `nextFloat` 定上限、
//! 每步两次 `nextInt(6)`、挑表 `nextInt(总权重)`、组大小 `count.sample`、生成规则里的
//! `nextInt(32)` / `nextInt(8)`、朝向 `nextFloat`、`finalizeSpawn` 各取数。
//!
//! Natural spawning, vanilla `NaturalSpawner`. Each tick rebuilds a [`SpawnState`] from
//! the island's entity table (per-category counts, spawn potential, per-player counts),
//! filters the categories that still have room under the global cap, and for every
//! spawning chunk tries three groups of attempts: pick a weighted spawn entry from the
//! biome table, check distance/light/block/collision rules, then `finalizeSpawn` and
//! spawn the group. Random draws follow vanilla order.

use std::collections::BTreeMap;

use mc_bamboo_core::{Axis, BlockPos, ChunkPos, Direction, LightLayer, Vec3, mth};
use mc_bamboo_data::blocks::{CACTUS, LAVA, LAVA_CAULDRON, MAGMA_BLOCK, POWDER_SNOW};
use mc_bamboo_data::spawning::{self, MobCategory, MobSpawnSettings, MobSpawnerDef};
use mc_bamboo_data::tags::block_tags;
use mc_bamboo_data::{BlockAncestor, light_emission};

use super::{MobType, spawn};
use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, PlayerView},
        store::KindPublic,
    },
    shape::{Aabb, CollisionContext},
    state::{BlockState, LIT, SupportType},
    world::World,
};

/// `NaturalSpawner.MIN_SPAWN_DISTANCE` 的平方：离玩家 24 格以内不刷。
pub const MIN_SPAWN_DISTANCE_SQR: f64 = 576.0;

/// `NaturalSpawner.SPAWN_DISTANCE_CHUNK`：`LocalMobCapCalculator` 只看 8 个区块内的玩家。
pub const SPAWN_DISTANCE_CHUNK: i32 = 8;

/// `NaturalSpawner.MAGIC_NUMBER`，全局上限按 `max * chunks / 289`。
pub const MAGIC_NUMBER: i32 = 289;

/// 每个类别对一个区块最多试 3 组。
const GROUP_ATTEMPTS: i32 = 3;

/// 每步候选点的水平漂移上限（`nextInt(6) - nextInt(6)`）。
const STEP_RANGE: i32 = 6;

/// 刷怪表里的一项，按加权表挑出来以后要能回查。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpawnEntry {
    entity_type: i32,
    weight: i32,
    min: i32,
    max: i32,
}

impl SpawnEntry {
    fn of(entry: &MobSpawnerDef) -> Self {
        Self {
            entity_type: entry.entity_type,
            weight: entry.weight,
            min: entry.min,
            max: entry.max,
        }
    }
}

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
            // `Mob.requiresCustomPersistence()` 只认载具和拴绳，Bamboo 都没有。
            // Vanilla `requiresCustomPersistence()` needs a vehicle or a leash; neither exists.
            if mob.persistence_required {
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
            .filter(|player| player_is_close_to_chunk(**player, chunk))
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
    /// `spawn_mobs`。
    pub spawn_mobs: bool,
    /// `spawn_monsters`（官方的 `spawnEnemies`）。
    pub spawn_enemies: bool,
    /// 按玩家的上限（`bamboo.toml` 的 `spawn_per_player_cap`）：关掉时只查全局上限。
    ///
    /// The per-player cap (`spawn_per_player_cap` in `bamboo.toml`); when off only the
    /// global cap applies.
    pub per_player_cap: bool,
    /// `getRespawnData()` 的位置，离它 24 格以内不刷。
    pub respawn_pos: Option<BlockPos>,
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
    let start = random_pos_within(world, chunk);
    if start.y > world.min_y() {
        spawn_category_for_position(world, category, chunk, start, state, params);
    }
}

/// `NaturalSpawner.getRandomPosWithin`：先取 x、z，再用 `WORLD_SURFACE` 高度图取 y。
fn random_pos_within(world: &World, chunk: ChunkPos) -> BlockPos {
    let random = world.random();
    let x = chunk.x * 16 + random.next_int(16);
    let z = chunk.z * 16 + random.next_int(16);
    let top_empty_y = world.world_surface_y(BlockPos::new(x, world.min_y(), z)) + 1;
    let min_y = world.min_y();
    let span = (top_empty_y - min_y + 1).max(1);
    let y = min_y + random.next_int(span);
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
        let mut current: Option<SpawnEntry> = None;
        let mut group: Option<spawn::SpawnGroup> = None;
        let mut max = mth::ceil(f64::from(world.random().next_float()) * 4.0);
        let mut group_size = 0;
        let mut step = 0;
        while step < max {
            step += 1;
            let random = world.random();
            x += random.next_int(STEP_RANGE) - random.next_int(STEP_RANGE);
            z += random.next_int(STEP_RANGE) - random.next_int(STEP_RANGE);
            let pos = BlockPos::new(x, y_start, z);
            let xx = f64::from(x) + 0.5;
            let zz = f64::from(z) + 0.5;
            let Some(nearest) = nearest_player_distance_sqr(world, xx, f64::from(y_start), zz)
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
                max = sample_count(world, entry);
            }
            let entry = current.expect("上面刚填过");
            let kind = MobType::by_entity_type(entry.entity_type).expect("刷怪表只留已支持的类型");
            if !is_valid_spawn_position_for_type(world, category, entry, kind, pos, nearest, state)
            {
                continue;
            }
            let Some(mut entity) = create_mob(world, kind) else {
                return;
            };
            let yaw = world.random().next_float() * 360.0;
            spawn::snap_to(&mut entity, Vec3::new(xx, f64::from(y_start), zz), yaw, 0.0);
            if !is_valid_position_for_mob(world, &entity, kind, nearest) {
                continue;
            }
            spawn::finalize_spawn_with_group(world, kind, &mut entity, &mut group);
            cluster_size += 1;
            group_size += 1;
            world.add_prepared_entity(entity);
            state.after_spawn(world, kind, pos);
            if cluster_size >= max_spawn_cluster_size(kind) {
                return;
            }
            if is_max_group_size_reached(kind, group_size) {
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
        let dx = f64::from(pos.x) + 0.5 - f64::from(respawn.x);
        let dy = f64::from(pos.y) - f64::from(respawn.y);
        let dz = f64::from(pos.z) + 0.5 - f64::from(respawn.z);
        if dx * dx + dy * dy + dz * dz < MIN_SPAWN_DISTANCE_SQR {
            return false;
        }
    }
    let chunk_pos = chunk_at(pos);
    chunk_pos == chunk || world.level().chunk_ticks_entities(chunk_pos)
}

/// `NaturalSpawner.isValidSpawnPostitionForType`。
fn is_valid_spawn_position_for_type(
    world: &mut World,
    category: MobCategory,
    entry: SpawnEntry,
    kind: MobType,
    pos: BlockPos,
    nearest_player_distance_sqr: f64,
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
    if !check_spawn_rules(world, kind, pos) {
        return false;
    }
    if !state.can_spawn(world, kind, pos) {
        return false;
    }
    no_collision(world, spawn_aabb(kind, pos))
}

/// `NaturalSpawner.canSpawnMobAt`：这一点的群系表里还有这一项。
fn can_spawn_mob_at(
    world: &World,
    category: MobCategory,
    entry: SpawnEntry,
    pos: BlockPos,
) -> bool {
    mobs_at(world, category, pos)
        .iter()
        .any(|candidate| SpawnEntry::of(candidate) == entry)
}

/// `NaturalSpawner.mobsAt`（下界要塞那一支不建模）。
fn mobs_at(world: &World, category: MobCategory, pos: BlockPos) -> &'static [MobSpawnerDef] {
    settings_at(world, pos).mobs_to_spawn(category)
}

/// `level.environmentAttributes().getValue(NATURAL_MOB_SPAWNS, pos)`：主世界就是群系的表。
fn settings_at(world: &World, pos: BlockPos) -> MobSpawnSettings {
    spawning::for_biome_id(world.biome_id(pos))
}

/// `NaturalSpawner.getRandomSpawnMobAt`，外加「挑到 Bamboo 没有的生物时当作没有刷怪数据」。
///
/// `NaturalSpawner.getRandomSpawnMobAt`, plus "a pick Bamboo cannot create counts as no
/// spawn data": only the modelled mob types can spawn.
fn random_spawn_mob_at(
    world: &mut World,
    category: MobCategory,
    pos: BlockPos,
) -> Option<SpawnEntry> {
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
    let picked = pick(table, world.random())?;
    MobType::by_entity_type(picked.entity_type)?;
    Some(SpawnEntry::of(picked))
}

/// `IntProvider.sample` 的取数入口，参数是已经挑中的那一项。
fn sample_count(world: &World, entry: SpawnEntry) -> i32 {
    let def = MobSpawnerDef {
        entity_type: entry.entity_type,
        weight: entry.weight,
        min: entry.min,
        max: entry.max,
    };
    sample(&def, world.random())
}

/// 官方 `WeightedList.getRandom`：`nextInt(总权重)` 之后按累减权重挑一项。
/// 表为空或总权重为 0 时返回 `None` 且不取随机数（官方 `selector == null` 直接返回空）。
///
/// Vanilla `WeightedList.getRandom`: draw `nextInt(totalWeight)` and walk the
/// cumulative weights. An empty table or a zero total returns `None` without a draw.
pub fn pick<'a>(
    table: &'a [MobSpawnerDef],
    random: &crate::random::LegacyRandom,
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
pub fn sample(entry: &MobSpawnerDef, random: &crate::random::LegacyRandom) -> i32 {
    let span = entry.max - entry.min + 1;
    if span <= 1 {
        entry.min
    } else {
        entry.min + random.next_int(span)
    }
}

/// `SpawnPlacements.isSpawnPositionOk`：牛、僵尸、尸壳都是 `ON_GROUND`。
fn spawn_placement_ok(world: &World, kind: MobType, pos: BlockPos) -> bool {
    if !world.world_border().is_within_bounds_pos(pos) {
        return false;
    }
    let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    if !is_valid_spawn_below(world, below) {
        return false;
    }
    is_valid_empty_spawn_block(world, kind, pos) && is_valid_empty_spawn_block(world, kind, above)
}

/// `BlockState.isValidSpawn`：`isFaceSturdy(UP)` 且发光小于 14。
fn is_valid_spawn_below(world: &World, pos: BlockPos) -> bool {
    world.is_face_sturdy(pos, Direction::Up, SupportType::Full)
        && light_emission(world.block_state(pos).0) < 14
}

/// `NaturalSpawner.isValidEmptySpawnBlock`。
fn is_valid_empty_spawn_block(world: &World, kind: MobType, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
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
    !is_block_dangerous(kind, state)
}

/// `EntityType.isBlockDangerous`：烧着的方块、凋灵玫瑰、甜浆果丛、仙人掌、细雪。
fn is_block_dangerous(kind: MobType, state: BlockState) -> bool {
    if kind.fire_immune() {
        return false;
    }
    is_burning_block(state)
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
fn check_spawn_rules(world: &World, kind: MobType, pos: BlockPos) -> bool {
    if !kind.allowed_in_peaceful() && world.level().difficulty_id() == 0 {
        return false;
    }
    match kind {
        MobType::Cow => {
            let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
            world
                .block_state(below)
                .is_tagged(block_tags::ANIMALS_SPAWNABLE_ON)
                && world.raw_brightness(pos) > 8
        }
        MobType::Zombie => {
            is_dark_enough_to_spawn(world, pos) && valid_mob_spawn_block(world, kind, pos)
        }
        MobType::Husk => {
            is_dark_enough_to_spawn(world, pos)
                && valid_mob_spawn_block(world, kind, pos)
                && world.level().can_see_sky(pos)
        }
    }
}

/// `Monster.isDarkEnoughToSpawn`。
fn is_dark_enough_to_spawn(world: &World, pos: BlockPos) -> bool {
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
fn valid_mob_spawn_block(world: &World, kind: MobType, pos: BlockPos) -> bool {
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    if kind.fire_immune() {
        return true;
    }
    is_valid_spawn_below(world, below)
}

/// `NaturalSpawner.isValidPositionForMob`。
fn is_valid_position_for_mob(
    world: &World,
    entity: &Entity,
    kind: MobType,
    nearest_player_distance_sqr: f64,
) -> bool {
    let despawn = f64::from(kind.mob_category().facts().despawn_distance);
    if nearest_player_distance_sqr > despawn * despawn && kind.removes_when_far_away() {
        return false;
    }
    // `Mob.checkSpawnRules` 在 `Mob` 基类里恒真；`checkSpawnObstruction` 要求不泡在液体里
    // 且没有实体挡住（Bamboo 的 `is_unobstructed` 只看玩家）。
    //
    // `Mob.checkSpawnRules` is true on the base class; `checkSpawnObstruction` needs no
    // liquid in the box and nothing in the way (Bamboo's check only knows players).
    !contains_any_liquid(world, entity.bounding_box())
        && world.level().is_unobstructed(&[entity.bounding_box()])
}

/// `EntityType.getSpawnAABB(x, y, z)`：以 (x, y, z) 为脚底中心的身体盒。
fn spawn_aabb(kind: MobType, pos: BlockPos) -> Aabb {
    let (width, height, _) = kind.dimensions(false);
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
fn contains_any_liquid(world: &World, box_: Aabb) -> bool {
    let min = BlockPos::new(
        box_.min(Axis::X).floor() as i32,
        box_.min(Axis::Y).floor() as i32,
        box_.min(Axis::Z).floor() as i32,
    );
    let max = BlockPos::new(
        box_.max(Axis::X).ceil() as i32,
        box_.max(Axis::Y).ceil() as i32,
        box_.max(Axis::Z).ceil() as i32,
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

/// `Level.noCollision(AABB)`：方块和实体都不能挡。
fn no_collision(world: &World, box_: Aabb) -> bool {
    world
        .block_collisions(box_, CollisionContext::Empty)
        .is_empty()
        && world.level().is_unobstructed(&[box_])
}

/// `EntityType.create(level, NATURAL)`：Bamboo 只构造自己建模的几种。
fn create_mob(world: &mut World, kind: MobType) -> Option<Entity> {
    Some(world.prepare_entity(Entity::by_mob_type(kind, Vec3::default())))
}

/// `Mob.getMaxSpawnClusterSize()`。
fn max_spawn_cluster_size(kind: MobType) -> i32 {
    let _ = kind;
    4
}

/// `Mob.isMaxGroupSizeReached`。
fn is_max_group_size_reached(kind: MobType, group_size: i32) -> bool {
    let _ = (kind, group_size);
    false
}

/// `ChunkMap.playerIsCloseEnoughForSpawning`：不是旁观者，离区块中心小于 128 格。
fn player_is_close_to_chunk(player: PlayerView, chunk: ChunkPos) -> bool {
    if player.spectator {
        return false;
    }
    let cx = f64::from(chunk.x * 16 + 8);
    let cz = f64::from(chunk.z * 16 + 8);
    let dx = cx - player.position.x;
    let dz = cz - player.position.z;
    dx * dx + dz * dz < 16384.0
}

/// `Level.getNearestPlayer(x, y, z, -1.0, false)` 的距离平方：非旁观者、非创造。
fn nearest_player_distance_sqr(world: &World, x: f64, y: f64, z: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    world.for_each_player(&mut |player| {
        if player.spectator || player.creative {
            return;
        }
        let dx = player.position.x - x;
        let dy = player.position.y - y;
        let dz = player.position.z - z;
        let distance = dx * dx + dy * dy + dz * dz;
        best = Some(best.map_or(distance, |current| current.min(distance)));
    });
    best
}

/// 群系标签查询，只有刷怪相关的两个标签用得到。
fn biome_tag_contains(name: &str, biome: i32) -> bool {
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
            uid,
            position,
            eye_height: 1.62,
            alive: true,
            spectator: false,
            creative: false,
            sneaking: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
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
            spawn_mobs: true,
            spawn_enemies: true,
            per_player_cap,
            respawn_pos: None,
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
    fn cows_spawn_on_open_grass_at_day() {
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
        for entity in level.entities().iter() {
            let mob = entity.kind.mob().expect("只会有生物");
            assert_eq!(mob.mob_type, MobType::Cow, "白天只支持牛刷出来");
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
    fn zombies_can_spawn_when_it_is_dark() {
        let mut level = TestLevel::new();
        grass_platform(&mut level);
        level.light = Some((0, 0));
        level.players.push(player(1, Vec3::new(8.5, 64.0, 40.5)));
        let params = params(289 * 4, true);
        let chunk = ChunkPos::new(0, 0);
        let mut zombies = 0;
        for _ in 0..3000 {
            zombies += run_tick(&mut level, chunk, &params);
            if zombies > 0 {
                break;
            }
        }
        assert!(zombies > 0, "黑暗里应该刷出怪物");
        for entity in level.entities().iter() {
            let Some(mob) = entity.kind.mob() else {
                continue;
            };
            assert!(
                mob.mob_type.mob_category() == MobCategory::Monster,
                "黑暗里只会刷出怪物"
            );
            assert_eq!(mob.mob_type.entity_type(), types::ZOMBIE);
        }
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
