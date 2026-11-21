//! 生成期刷怪（官方 `NaturalSpawner.spawnMobsForChunkGeneration`）。
//!
//! 新区块走到 `spawn` 阶段时，按查询点群系的 `CREATURE` 加权表和
//! `CREATURE_WORLD_GEN_SPAWN_PROBABILITY` 刷一批动物。两个随机源分开：循环、选表、
//! 组大小、起点、漂移和朝向取装饰种子的 `WorldgenRandom`；`finalizeSpawn` 取
//! `WorldGenRegion.random`。所以还没建模的动物只要照官方规则模拟放置，装饰随机源的
//! 取数就不会错开，后面真正生成的牛落点和官方一致。
//!
//! World-generation spawning (vanilla `NaturalSpawner.spawnMobsForChunkGeneration`).
//! The loop, table pick, group size, start, drift and facing draw from the
//! decoration-seeded `WorldgenRandom`; `finalizeSpawn` draws from
//! `WorldGenRegion.random`. Simulating the placement of unmodelled animals therefore
//! keeps the decoration random in step, so the cows that do spawn land where vanilla
//! puts them.

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{
    entity_types::FACTS, facts, registries::entity_type, spawning::MobSpawnerDef, tags::block_tags,
};

use super::{MobType, goals, natural, spawn};
use crate::{entity::Entity, random::RandomSource, shape::Aabb, world::World};

/// 一次生成期刷怪的输入。
///
/// The inputs of one world-generation spawn pass.
pub struct GenSpawn<'a> {
    /// 中心区块。
    pub chunk: ChunkPos,
    /// 查询点群系的 `getMobsToSpawn(CREATURE)`。
    pub mobs: &'a [MobSpawnerDef],
    /// 查询点的 `CREATURE_WORLD_GEN_SPAWN_PROBABILITY`。
    pub probability: f32,
    /// `setDecorationSeed` 之后的 `WorldgenRandom`。
    pub random: &'a dyn RandomSource,
    /// `WorldGenRegion.random`，给 `finalizeSpawn`。
    pub region_random: &'a dyn RandomSource,
}

/// 生成期刷怪用的放置事实：`SpawnPlacements` 登记的放置方式、高度图以及身体尺寸。
///
/// Placement facts for generation spawning: the placement type and spawn rule
/// registered in `SpawnPlacements`, the heightmap, and the body size.
#[derive(Debug, Clone, Copy)]
struct Placement {
    entity_type: i32,
    width: f32,
    height: f32,
    /// `ON_GROUND` 为真；狐狸的 `NO_RESTRICTIONS` 为假，位置恒可用。
    ///
    /// True for `ON_GROUND`; false for the fox's `NO_RESTRICTIONS`.
    on_ground: bool,
    heightmap_mask: u64,
    /// Bamboo 建模了的种类；`None` 时只模拟放置、推进随机数，不生成实体。
    kind: Option<MobType>,
}

/// 查已翻译的生成期放置规则；鹦鹉只模拟放置，不生成实体。
///
/// Look up translated world-generation placement rules; parrots are simulated, not spawned.
fn placement(entity: i32) -> Option<Placement> {
    let animal = [
        entity_type::CHICKEN,
        entity_type::COW,
        entity_type::DONKEY,
        entity_type::HORSE,
        entity_type::LLAMA,
        entity_type::MULE,
        // 哞菇的放置方式和高度图一样，规则换成 `checkMushroomSpawnRules`（见 [`spawn_rules`]）。
        // Mooshrooms share the placement and heightmap; [`spawn_rules`] swaps the rule.
        entity_type::MOOSHROOM,
        // 熊猫用普通动物规则，放置方式和高度图一样。
        // Pandas use the plain animal rule and the same placement and heightmap.
        entity_type::PANDA,
        entity_type::PIG,
        // 北极熊同样的放置方式和高度图，规则是 `checkPolarBearSpawnRules`。
        // Polar bears share the placement and heightmap; the rule is `checkPolarBearSpawnRules`.
        entity_type::POLAR_BEAR,
        entity_type::SHEEP,
    ]
    .contains(&entity);
    let on_ground = match entity {
        _ if animal => true,
        entity_type::RABBIT | entity_type::WOLF | entity_type::PARROT => true,
        entity_type::FOX => false,
        _ => return None,
    };
    let facts = &FACTS[entity as usize];
    Some(Placement {
        entity_type: entity,
        width: facts.width,
        height: facts.height,
        on_ground,
        heightmap_mask: if entity == entity_type::PARROT {
            facts::MOTION_BLOCKING
        } else {
            facts::MOTION_BLOCKING_NO_LEAVES
        },
        kind: MobType::by_entity_type(entity),
    })
}
/// `NaturalSpawner.spawnMobsForChunkGeneration`（`SPAWN_MOBS` 由调用方查）。返回生成的生物，
/// 调用方把它们放进区块的实体盒。遇到规则没翻译的种类就停止这个区块，后面的取数无法对齐。
///
/// `NaturalSpawner.spawnMobsForChunkGeneration` (the caller checks `SPAWN_MOBS`).
/// Returns the spawned mobs for the chunk's entity box. A kind whose rules are not
/// translated stops the chunk, since later draws could not stay aligned.
pub fn spawn_mobs_for_chunk_generation(world: &mut World, input: &GenSpawn<'_>) -> Vec<Entity> {
    let mut spawned = Vec::new();
    if input.mobs.is_empty() {
        return spawned;
    }
    let random = input.random;
    let xo = input.chunk.x * 16;
    let zo = input.chunk.z * 16;
    while random.next_float() < input.probability {
        let Some(entry) = natural::pick(input.mobs, random) else {
            continue;
        };
        let count = natural::sample(entry, random);
        let start_x = xo + random.next_int(16);
        let start_z = zo + random.next_int(16);
        let Some(placement) = placement(entry.entity_type) else {
            return spawned;
        };
        let mut cursor = (start_x, start_z);
        let mut group = None;
        for _ in 0..count {
            spawn_one(
                world,
                input,
                placement,
                (start_x, start_z),
                &mut cursor,
                &mut group,
                &mut spawned,
            );
        }
    }
    spawned
}

/// 一只最多试 4 次。规则不过时直接进下一次、不漂移（官方的 `continue`）；
/// 落脚点不行或者朝向已经抽过，就按 `nextInt(5) - nextInt(5)` 漂移，出了区块就从起点重取。
///
/// Up to four attempts per mob. Failed rules retry without drifting (vanilla's
/// `continue`); otherwise drift by `nextInt(5) - nextInt(5)`, redrawing from the start
/// while outside the chunk.
fn spawn_one(
    world: &mut World,
    input: &GenSpawn<'_>,
    placement: Placement,
    start: (i32, i32),
    cursor: &mut (i32, i32),
    group: &mut Option<spawn::SpawnGroup>,
    spawned: &mut Vec<Entity>,
) {
    let random = input.random;
    let xo = input.chunk.x * 16;
    let zo = input.chunk.z * 16;
    let mut success = false;
    let mut attempts = 0;
    while !success && attempts < 4 {
        attempts += 1;
        let (x, z) = *cursor;
        let pos = BlockPos::new(
            x,
            top_non_colliding_y(world, x, z, placement.heightmap_mask),
            z,
        );
        if !placement.on_ground
            || natural::on_ground_position_ok(world, placement.entity_type, false, pos)
        {
            let width = f64::from(placement.width);
            let fx = clamp(
                f64::from(x),
                f64::from(xo) + width,
                f64::from(xo) + 16.0 - width,
            );
            let fz = clamp(
                f64::from(z),
                f64::from(zo) + width,
                f64::from(zo) + 16.0 - width,
            );
            let at = Vec3::new(fx, f64::from(pos.y), fz);
            let area = body(placement, at);
            if !natural::no_collision(world, area)
                || !spawn_rules(world, placement, BlockPos::containing(at))
            {
                continue;
            }
            let yaw = random.next_float() * 360.0;
            success = match placement.kind {
                Some(kind) => match place_mob(world, input, kind, at, yaw, group) {
                    Some(entity) => {
                        spawned.push(entity);
                        true
                    }
                    None => false,
                },
                None => simulated_mob_fits(world, area, BlockPos::containing(at)),
            };
        }
        *cursor = drift(random, *cursor, start, xo, zo);
    }
}

/// `SpawnPlacements.checkSpawnRules`：已建模的动物用各自的规则。
///
/// `SpawnPlacements.checkSpawnRules`: use each modelled animal's own rule.
fn spawn_rules(world: &World, placement: Placement, pos: BlockPos) -> bool {
    match placement.kind {
        Some(kind) => natural::check_spawn_rules(world, kind, pos),
        None if placement.entity_type == entity_type::PARROT => {
            natural::bright_spawn_rules(world, pos, block_tags::PARROTS_SPAWNABLE_ON)
        }
        None => natural::animal_spawn_rules(world, pos),
    }
}

/// `x += nextInt(5) - nextInt(5)`、`z` 同理；出了区块就从起点重取，直到落回区块里。
///
/// Drift both axes; while outside the chunk, redraw around the start.
fn drift(
    random: &dyn RandomSource,
    (mut x, mut z): (i32, i32),
    (start_x, start_z): (i32, i32),
    xo: i32,
    zo: i32,
) -> (i32, i32) {
    x += random.next_int(5) - random.next_int(5);
    z += random.next_int(5) - random.next_int(5);
    while x < xo || x >= xo + 16 || z < zo || z >= zo + 16 {
        x = start_x + random.next_int(5) - random.next_int(5);
        z = start_z + random.next_int(5) - random.next_int(5);
    }
    (x, z)
}

/// `Mth.clamp(double, double, double)`。
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min { min } else { value.min(max) }
}

/// `getSpawnAABB` 和 `EntityDimensions.makeBoundingBox`：以脚底中心为原点的身体盒。
///
/// `getSpawnAABB` / `EntityDimensions.makeBoundingBox`: the body box around the feet.
fn body(placement: Placement, at: Vec3) -> Aabb {
    let half = f64::from(placement.width / 2.0);
    Aabb::new(
        at.x - half,
        at.y,
        at.z - half,
        at.x + half,
        at.y + f64::from(placement.height),
        at.z + half,
    )
}

/// `getTopNonCollidingPos`：主世界没有天花板，按该生物注册的高度图查找。
///
/// `getTopNonCollidingPos`: use the registered heightmap; the overworld has no ceiling.
fn top_non_colliding_y(world: &World, x: i32, z: i32, heightmap_mask: u64) -> i32 {
    for y in (world.min_y()..=world.max_y()).rev() {
        let pos = BlockPos::new(x, y, z);
        let state = world.block_state(pos);
        let blocks = mc_bamboo_data::state_facts(state.0) & heightmap_mask != 0;
        if blocks || !world.fluid_state(pos).is_empty() {
            return y + 1;
        }
    }
    world.min_y()
}

/// 建模了的种类：创建、`snapTo`、`checkSpawnRules` 与 `checkSpawnObstruction`，再用
/// 区域随机源跑 `finalizeSpawn`。
///
/// A modelled kind: create, `snapTo`, `checkSpawnRules` and `checkSpawnObstruction`,
/// then `finalizeSpawn` on the region random.
fn place_mob(
    world: &mut World,
    input: &GenSpawn<'_>,
    kind: MobType,
    at: Vec3,
    yaw: f32,
    group: &mut Option<spawn::SpawnGroup>,
) -> Option<Entity> {
    let mut entity = natural::create_mob(world, kind)?;
    spawn::snap_to(&mut entity, at, yaw, 0.0);
    let feet = entity.block_position();
    let fits = natural::pathfinder_spawn_rules(world, &entity, feet)
        && !natural::contains_any_liquid(world, entity.bounding_box())
        && natural::is_unobstructed_by_entities(world, entity.bounding_box());
    if !fits {
        return None;
    }
    let biome = world.biome_id(feet);
    spawn::finalize_generated(world, kind, &mut entity, biome, input.region_random, group);
    Some(entity)
}

/// 没建模的动物：`PathfinderMob.checkSpawnRules`（`Animal` 的走路代价）加
/// `checkSpawnObstruction`。生成期区域里查不到实体，所以只看液体。
///
/// An unmodelled animal: `PathfinderMob.checkSpawnRules` with the `Animal` walk target
/// value, plus `checkSpawnObstruction`. A generation region sees no entities, so only
/// liquid matters.
fn simulated_mob_fits(world: &World, area: Aabb, feet: BlockPos) -> bool {
    goals::animal_walk_target_value(world, feet) >= 0.0
        && !natural::contains_any_liquid(world, area)
}
