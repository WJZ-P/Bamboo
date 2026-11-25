//! 流浪商人刷新器：官方 `WanderingTraderSpawner`（`CustomSpawner`）。
//!
//! 每 1200 刻把 `spawnDelay` 减 1200；减到不正时重置为 24000、把 `spawnChance`
//! 从 25 依次加到 75，再按 **加之前** 的概率判 `nextInt(100)`。命中后在随机玩家
//! 附近（优先 48 格内的村庄集会点 POI`meeting`）找一个落点，生成一名流浪商人和最多
//! 两只拴在它身上的行商羊驼。失败时概率留在递增后的值，下一次再试。
//!
//! The vanilla `WanderingTraderSpawner` (`CustomSpawner`). Every 1200 ticks it
//! subtracts 1200 from `spawnDelay`; when that reaches zero it resets to 24000,
//! raises `spawnChance` from 25 towards 75 in steps of 25 and rolls `nextInt(100)`
//! against the **old** chance. On success it finds a spawn position near a random
//! player (preferring a `meeting` POI within 48 blocks), spawns a wandering trader
//! with up to two leashed trader llamas, and resets the chance to 25.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::tags;

use super::{MobType, leash, natural, spawn, wandering_trader};
use crate::{
    entity::{Entity, ctx::BlockView},
    random::LegacyRandom,
    world::World,
};

/// `DEFAULT_TICK_DELAY`：每 1200 刻才检查一次存档计时。
///
/// `DEFAULT_TICK_DELAY`: the saved-data timer is checked every 1200 ticks.
pub const TICK_DELAY: i32 = 1200;
/// `DEFAULT_SPAWN_DELAY`：一次尝试之间的 24000 刻（20 分钟）。
///
/// `DEFAULT_SPAWN_DELAY`: 24000 ticks (20 minutes) between attempts.
pub const DEFAULT_SPAWN_DELAY: i32 = 24000;
/// `MIN_SPAWN_CHANCE` / `MAX_SPAWN_CHANCE` / `SPAWN_CHANCE_INCREASE`。
pub const MIN_SPAWN_CHANCE: i32 = 25;
pub const MAX_SPAWN_CHANCE: i32 = 75;
pub const SPAWN_CHANCE_INCREASE: i32 = 25;
/// `SPAWN_ONE_IN_X_CHANCE`：即使概率命中，还有一次十分之一的门。
pub const SPAWN_ONE_IN_X_CHANCE: i32 = 10;
/// `NUMBER_OF_SPAWN_ATTEMPTS`：找落点的尝试次数。
pub const NUMBER_OF_SPAWN_ATTEMPTS: usize = 10;
/// 取点半径（玩家 / 集会点附近 48 格；羊驼附近 4 格）。
const SPAWN_RADIUS: i32 = 48;
const LLAMA_RADIUS: i32 = 4;

/// `WanderingTraderData`：`data/minecraft/wandering_trader.dat` 里的两个字段。
/// 只有这两个字段存档；`tick_delay` 和随机源是刷新器自己的内存状态。
///
/// `WanderingTraderData`: the two fields of `data/minecraft/wandering_trader.dat`.
/// Only these persist; `tick_delay` and the random source are in-memory state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WanderingTraderData {
    pub spawn_delay: i32,
    pub spawn_chance: i32,
}

impl Default for WanderingTraderData {
    fn default() -> Self {
        Self {
            spawn_delay: DEFAULT_SPAWN_DELAY,
            spawn_chance: MIN_SPAWN_CHANCE,
        }
    }
}

impl WanderingTraderData {
    /// 官方 `WanderingTraderData.CODEC` 的两个 `optionalFieldOf` 缺省值。
    ///
    /// The two `optionalFieldOf` defaults of `WanderingTraderData.CODEC`.
    pub fn tag(self) -> (i32, i32) {
        (self.spawn_delay, self.spawn_chance)
    }

    /// 从存档的两个字段构造；缺字段或坏值由调用方按官方 `resultOrPartial` 兜底。
    ///
    /// Build from the two saved fields; the caller applies the codec defaults.
    pub fn from_tag(spawn_delay: i32, spawn_chance: i32) -> Self {
        Self {
            spawn_delay,
            spawn_chance,
        }
    }
}

/// 刷新器的内存 + 存档状态。`random` 官方是 `RandomSource.create()`，也就是
/// `LegacyRandomSource`（48 位线性同余）；对拍时注入同种子的一条 `LegacyRandom`。
///
/// The spawner's in-memory and saved state. Vanilla's `random` is
/// `RandomSource.create()`, a `LegacyRandomSource` (48-bit LCG); the oracle
/// injects a seeded `LegacyRandom` with the same algorithm.
#[derive(Debug)]
pub struct WanderingTraderSpawner {
    /// `WanderingTraderSpawner.tickDelay`，构造时 1200、不存档。
    pub tick_delay: i32,
    /// `WanderingTraderData`，存档。
    pub data: WanderingTraderData,
    /// 刷新器自己的随机源（和世界随机源分开）。
    pub random: LegacyRandom,
}

impl Default for WanderingTraderSpawner {
    fn default() -> Self {
        Self {
            tick_delay: TICK_DELAY,
            data: WanderingTraderData::default(),
            random: LegacyRandom::new(0),
        }
    }
}

impl WanderingTraderSpawner {
    /// 按存档值和固定种子建一个刷新器（对拍 / 读档用）。
    ///
    /// Build a spawner from saved data and a fixed seed (oracle / load).
    pub fn with_random_seed(data: WanderingTraderData, tick_delay: i32, seed: i64) -> Self {
        Self {
            tick_delay,
            data,
            random: LegacyRandom::new(seed),
        }
    }

    /// `WanderingTraderSpawner.tick(level, spawnEnemies)`。
    /// `enabled` 是 `GameRules.SPAWN_WANDERING_TRADERS`；关掉时整段不跑、不取数。
    /// 返回这次有没有真的生成商人（`spawn` 的返回值）。
    ///
    /// `WanderingTraderSpawner.tick(level, spawnEnemies)`. `enabled` is the
    /// `SPAWN_WANDERING_TRADERS` game rule; with it off nothing runs and no
    /// random numbers are drawn. Returns whether a trader was spawned.
    pub fn tick(&mut self, world: &mut World, players: &[Vec3], enabled: bool) -> bool {
        if !enabled {
            return false;
        }
        self.tick_delay -= 1;
        if self.tick_delay > 0 {
            return false;
        }
        self.tick_delay = TICK_DELAY;
        let delay = self.data.spawn_delay - TICK_DELAY;
        self.data.spawn_delay = delay;
        if delay > 0 {
            return false;
        }
        self.data.spawn_delay = DEFAULT_SPAWN_DELAY;
        let old_chance = self.data.spawn_chance;
        self.data.spawn_chance =
            (old_chance + SPAWN_CHANCE_INCREASE).clamp(MIN_SPAWN_CHANCE, MAX_SPAWN_CHANCE);
        // 官方用 clamp 之前的 `oldChance` 判定；25→50 还是按 25% 抽。
        // Vanilla rolls against `oldChance`, before the clamp: 25→50 still rolls 25%.
        if self.random.next_int(100) > old_chance {
            return false;
        }
        let spawned = self.spawn(world, players);
        if spawned {
            self.data.spawn_chance = MIN_SPAWN_CHANCE;
        }
        spawned
    }

    /// `WanderingTraderSpawner.spawn(level)`。
    ///
    /// `WanderingTraderSpawner.spawn(level)`.
    fn spawn(&mut self, world: &mut World, players: &[Vec3]) -> bool {
        // `getRandomPlayer()`：Bamboo 没有官方的随机玩家列表，调用方按官方的
        // `level.getRandom().nextInt(players.size())` 之外的方式提供；没有玩家时
        // 官方返回 true（重置概率）。
        if players.is_empty() {
            return true;
        }
        if self.random.next_int(SPAWN_ONE_IN_X_CHANCE) != 0 {
            return false;
        }
        let player_pos = BlockPos::containing(players[0]);
        // `PoiManager.find(holder.is(PoiTypes.MEETING), pos -> true, pos, 48, ANY)`：
        // 找不到村庄钟就退回玩家位置。
        // `PoiManager.find(...)`: fall back to the player position when no bell exists.
        let meeting = mc_bamboo_data::poi::poi_type_id("meeting");
        let target = meeting
            .and_then(|kind| {
                BlockView::poi_find_closest(world, player_pos, SPAWN_RADIUS, kind, 2, &|_| true)
            })
            .unwrap_or(player_pos);
        let Some(pos) = self.find_spawn_position_near(world, target, SPAWN_RADIUS) else {
            return false;
        };
        if !has_enough_space(world, pos) {
            return false;
        }
        // `#without_wandering_trader_spawns`：标签里目前只有 `the_void`。
        // `#without_wandering_trader_spawns`: currently only `the_void`.
        if in_without_trader_biome(world, pos) {
            return false;
        }
        let trader_uid = {
            let mut entity = world.prepare_entity(Entity::by_mob_type(
                MobType::WanderingTrader,
                Vec3::default(),
            ));
            spawn::snap_to(
                &mut entity,
                Vec3::new(
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.5,
                ),
                0.0,
                0.0,
            );
            spawn::finalize_spawn(
                world,
                MobType::WanderingTrader,
                &mut entity,
                spawn::SpawnReason::Event,
            );
            if let Some(mob) = entity.kind.mob_mut() {
                mob.wandering_trader.despawn_delay = wandering_trader::SPAWNED_DESPAWN_DELAY;
                mob.wandering_trader.wander_target = Some(target);
                mob.home = Some((target, 16));
            }
            let uuid = entity.kind.mob().map(|mob| mob.uuid);
            (world.add_prepared_entity(entity), uuid)
        };
        let (trader_uid, trader_uuid) = trader_uid;
        for _ in 0..2 {
            self.try_to_spawn_llama_for(world, pos, trader_uid, trader_uuid);
        }
        true
    }

    /// `tryToSpawnLlamaFor(level, trader, 4)`：在商人 4 格内找落点、生成行商羊驼、
    /// 拴到商人身上。
    ///
    /// `tryToSpawnLlamaFor(level, trader, 4)`: find a position within 4 blocks,
    /// spawn a trader llama and leash it to the trader.
    fn try_to_spawn_llama_for(
        &mut self,
        world: &mut World,
        trader_pos: BlockPos,
        trader_uid: i32,
        trader_uuid: Option<[u8; 16]>,
    ) {
        let Some(pos) = self.find_spawn_position_near(world, trader_pos, LLAMA_RADIUS) else {
            return;
        };
        let mut entity =
            world.prepare_entity(Entity::by_mob_type(MobType::TraderLlama, Vec3::default()));
        spawn::snap_to(
            &mut entity,
            Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ),
            0.0,
            0.0,
        );
        spawn::finalize_spawn(
            world,
            MobType::TraderLlama,
            &mut entity,
            spawn::SpawnReason::Event,
        );
        if let (Some(mob), Some(uuid)) = (entity.kind.mob_mut(), trader_uuid) {
            // `TraderLlama.finalizeSpawn(EVENT)` 强制成年；`setLeashedTo(trader, true)`
            // 只改牵引数据，广播由同步层按 `LeashLink` 事件发。
            mob.age = 0;
            leash::set_leashed_to(mob, trader_uid, leash::LeashDelayed::Entity(uuid));
        }
        world.add_prepared_entity(entity);
    }

    /// `findSpawnPositionNear(level, pos, radius)`：10 次 `x/z` 各一次 `nextInt(2R)`，
    /// 高度取 `getHeight(MOTION_BLOCKING_NO_LEAVES)`，落点按官方
    /// `SpawnPlacements` 的 `ON_GROUND` 判定。
    ///
    /// `findSpawnPositionNear`: ten attempts, one `nextInt(2R)` for x and z each,
    /// the motion-blocking heightmap for y, and the `ON_GROUND` placement check.
    fn find_spawn_position_near(
        &self,
        world: &World,
        center: BlockPos,
        radius: i32,
    ) -> Option<BlockPos> {
        for _ in 0..NUMBER_OF_SPAWN_ATTEMPTS {
            let x = center.x + self.random.next_int(radius * 2) - radius;
            let z = center.z + self.random.next_int(radius * 2) - radius;
            let y = world.level().heightmap_y(BlockPos::new(x, center.y, z));
            let candidate = BlockPos::new(x, y, z);
            if natural::on_ground_position_ok(
                world,
                MobType::WanderingTrader.entity_type(),
                false,
                candidate,
            ) {
                return Some(candidate);
            }
        }
        None
    }
}

/// `hasEnoughSpace`：从 `pos` 到 `pos.offset(1, 2, 1)` 的每一格碰撞形状都得是空的。
///
/// `hasEnoughSpace`: every block from `pos` to `pos.offset(1, 2, 1)` must have
/// an empty collision shape.
fn has_enough_space(world: &World, pos: BlockPos) -> bool {
    for x in pos.x..=pos.x + 1 {
        for y in pos.y..=pos.y + 2 {
            for z in pos.z..=pos.z + 1 {
                let state = world.block_state(BlockPos::new(x, y, z));
                if !matches!(
                    mc_bamboo_data::collision_shape_kind(state.0),
                    mc_bamboo_data::CollisionShapeKind::Empty
                ) {
                    return false;
                }
            }
        }
    }
    true
}

/// `#minecraft:without_wandering_trader_spawns` 的群系标签检查。
///
/// The `#minecraft:without_wandering_trader_spawns` biome tag check.
fn in_without_trader_biome(world: &World, pos: BlockPos) -> bool {
    let id = world.biome_id(pos);
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:worldgen/biome")
        .and_then(|group| {
            group
                .tags
                .iter()
                .find(|tag| tag.name == "minecraft:without_wandering_trader_spawns")
        })
        .is_some_and(|tag| tag.ids.contains(&id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockState, BlockTicks, testing::TestLevel};
    use mc_bamboo_data::blocks;

    /// 一片平地，地面在 y=63，方块表正好覆盖 ±64 的取点范围。
    fn flat_level() -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        for x in -64..64 {
            for z in -64..64 {
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK));
            }
        }
        (level, BlockTicks::new())
    }

    #[test]
    fn the_timer_counts_down_in_1200_tick_steps() {
        let (mut level, mut ticks) = flat_level();
        let mut spawner = WanderingTraderSpawner::with_random_seed(
            WanderingTraderData::from_tag(0, MIN_SPAWN_CHANCE),
            TICK_DELAY,
            0,
        );
        // 前 1199 刻只动 tickDelay，存档值不变。
        for _ in 0..1199 {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(!spawner.tick(&mut world, &[], true));
        }
        assert_eq!(spawner.tick_delay, 1);
        assert_eq!(spawner.data.spawn_delay, 0);
        // 规则关掉时整段不动。
        {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(!spawner.tick(&mut world, &[], false));
        }
        assert_eq!(spawner.tick_delay, 1);
        // 第 1200 刻：delay 减到 -1200 后重置 24000，概率加到 50，
        // random.nextInt(100) 用旧值 25 判定；没有玩家时 spawn 返回 true，概率重置 25。
        {
            let mut world = World::new(&mut level, &mut ticks);
            let _ = spawner.tick(&mut world, &[], true);
        }
        assert_eq!(spawner.tick_delay, TICK_DELAY);
        assert_eq!(spawner.data.spawn_delay, DEFAULT_SPAWN_DELAY);
        assert_eq!(
            spawner.data.spawn_chance,
            MIN_SPAWN_CHANCE + SPAWN_CHANCE_INCREASE
        );
    }

    #[test]
    fn a_successful_attempt_resets_the_chance() {
        let (mut level, mut ticks) = flat_level();
        // 固定一条「前两次门都通过」的随机源：先找一个种子让
        // `nextInt(100) <= 25` 且 `nextInt(10) == 0`，再用它重放整次 tick。
        let seed = (0..4096_i64)
            .find(|&seed| {
                let r = LegacyRandom::new(seed);
                r.next_int(100) <= MIN_SPAWN_CHANCE && r.next_int(SPAWN_ONE_IN_X_CHANCE) == 0
            })
            .expect("存在一个通过两道概率门的种子");
        let mut spawner = WanderingTraderSpawner::with_random_seed(
            WanderingTraderData::from_tag(0, MIN_SPAWN_CHANCE),
            1,
            seed,
        );
        // tick 里两道门和 `spawn` 里的一到三次取点共用同一条随机源；
        // 重放整次 tick，检查刷新成功后概率重置。
        let before = level.entities().len();
        let spawned = {
            let mut world = World::new(&mut level, &mut ticks);
            spawner.tick(&mut world, &[Vec3::new(0.5, 64.0, 0.5)], true)
        };
        assert!(spawned, "种子 {seed} 这次尝试应该刷出商人");
        assert_eq!(spawner.data.spawn_chance, MIN_SPAWN_CHANCE);
        let fresh: Vec<Entity> = level.entities().iter().skip(before).collect();
        assert_eq!(fresh.len(), 3, "一名商人和两只羊驼");
        assert_eq!(
            fresh[0].kind.mob().unwrap().mob_type,
            MobType::WanderingTrader
        );
        let trader = fresh[0].kind.mob().unwrap();
        assert_eq!(
            trader.wandering_trader.despawn_delay,
            wandering_trader::SPAWNED_DESPAWN_DELAY
        );
        assert_eq!(trader.home, Some((BlockPos::new(0, 64, 0), 16)));
        for llama in &fresh[1..] {
            let mob = llama.kind.mob().unwrap();
            assert_eq!(mob.mob_type, MobType::TraderLlama);
            assert_eq!(mob.age, 0);
            assert_eq!(leash::leash_holder(mob), Some(fresh[0].id));
        }
    }

    #[test]
    fn a_disabled_rule_draws_nothing() {
        let (mut level, mut ticks) = flat_level();
        let mut spawner =
            WanderingTraderSpawner::with_random_seed(WanderingTraderData::from_tag(0, 75), 1, 7);
        let seed_before = spawner.random.seed();
        {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(!spawner.tick(&mut world, &[Vec3::new(0.5, 64.0, 0.5)], false));
        }
        assert_eq!(spawner.random.seed(), seed_before);
        assert_eq!(spawner.tick_delay, 1);
        assert_eq!(spawner.data.spawn_delay, 0);
    }
}
