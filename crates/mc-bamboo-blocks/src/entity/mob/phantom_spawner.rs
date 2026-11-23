//! 幻翼自定义刷怪器：官方 `PhantomSpawner`（`CustomSpawner`）。
//!
//! 主世界每一刻跑一次：先倒数 `nextTick`，到点了才在夜里 / 雷暴、露天、区域难度够高、
//! 玩家 `TIME_SINCE_REST` 够久时，在玩家头顶抽一个落点、按难度抽一组的数量，逐只
//! `finalizeSpawn` 后放进世界。取数顺序和 `PhantomSpawner.tick` 完全一致。
//!
//! The phantom custom spawner: vanilla `PhantomSpawner` (a `CustomSpawner`). It runs once
//! per overworld tick: count down `nextTick`, then at night or in a thunderstorm, under
//! open sky, with high enough regional difficulty and a long enough `TIME_SINCE_REST`,
//! roll a spawn point above the player, roll a group size from the difficulty and add
//! each phantom after `finalizeSpawn`. The random-draw order matches `PhantomSpawner.tick`.

use mc_bamboo_core::{BlockPos, Vec3};

use super::{MobType, natural, spawn};
use crate::{entity::ctx::BlockView, world::World};

/// 一名玩家在刷怪器眼里的样子（`ServerPlayer` 的几项）。
///
/// A player as the spawner sees it (a few `ServerPlayer` fields).
#[derive(Debug, Clone, Copy)]
pub struct SpawnerPlayer {
    /// 脚底位置，官方 `player.blockPosition()` 由它取整。
    ///
    /// Feet position; vanilla `player.blockPosition()` floors it.
    pub position: Vec3,
    /// `player.isSpectator()`。
    pub spectator: bool,
    /// `Stats.CUSTOM.get(Stats.TIME_SINCE_REST)` 的当前值。
    ///
    /// Current `Stats.CUSTOM.get(Stats.TIME_SINCE_REST)` value.
    pub time_since_rest: i32,
}

/// 刷怪器每刻要读的几条游戏规则 / 维度事实。
///
/// The game rules and dimension facts the spawner reads each tick.
#[derive(Debug, Clone, Copy)]
pub struct SpawnerRules {
    /// `GameRules.SPAWN_MOBS`（`ServerChunkCache.tickChunks` 的 `doMobSpawning`）。
    pub spawn_mobs: bool,
    /// `GameRules.SPAWN_MONSTERS`（传进 `tick` 的 `spawnEnemies`）。
    pub spawn_monsters: bool,
    /// `GameRules.SPAWN_PHANTOMS`。
    pub spawn_phantoms: bool,
    /// `level.dimensionType().hasSkyLight()`；主世界恒真。
    ///
    /// `level.dimensionType().hasSkyLight()`; always true for the overworld.
    pub has_sky_light: bool,
}

/// `PhantomSpawner.nextTick`：初值 0，第一次 tick 就抽下一次的间隔。
///
/// `PhantomSpawner.nextTick`: starts at 0, so the first tick rolls an interval.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhantomSpawner {
    pub next_tick: i32,
}

impl PhantomSpawner {
    /// `PhantomSpawner.tick(level, spawnEnemies)`。`spawn_mobs` 关掉时官方根本不调
    /// `tickCustomSpawners`，这里也整段不做、一个随机数都不取。
    ///
    /// `PhantomSpawner.tick(level, spawnEnemies)`: with `spawn_mobs` off vanilla
    /// never calls `tickCustomSpawners`, so this does nothing and draws nothing.
    pub fn tick(&mut self, world: &mut World, players: &[SpawnerPlayer], rules: SpawnerRules) {
        if !rules.spawn_mobs || !rules.spawn_monsters || !rules.spawn_phantoms {
            return;
        }
        self.next_tick -= 1;
        if self.next_tick > 0 {
            return;
        }
        self.next_tick += (60 + world.random().next_int(60)) * 20;
        // `getSkyDarken() >= 5 || !hasSkyLight()`：夜里 / 雷暴，或没有天空光的维度。
        // `getSkyDarken() >= 5 || !hasSkyLight()`: night/thunder, or a skylight-less dimension.
        if world.sky_darken() < 5 && rules.has_sky_light {
            return;
        }
        for player in players {
            if player.spectator {
                continue;
            }
            let feet = BlockPos::containing(player.position);
            // `!hasSkyLight() || playerPos.y >= seaLevel && canSeeSky(playerPos)`。
            if rules.has_sky_light && (feet.y < world.sea_level() || !world.can_see_sky(feet)) {
                continue;
            }
            // `difficulty.isHarderThan(random.nextFloat() * 3.0F)`：先取数再比较。
            // `difficulty.isHarderThan(random.nextFloat() * 3.0F)`: draw first, then compare.
            if world.effective_difficulty(feet) <= world.random().next_float() * 3.0 {
                continue;
            }
            // `Mth.clamp(stats.getValue(TIME_SINCE_REST), 1, MAX)`。
            let value = player.time_since_rest.clamp(1, i32::MAX);
            if world.random().next_int(value) < 72_000 {
                continue;
            }
            // `playerPos.above(20 + nextInt(15)).east(-10 + nextInt(21)).south(-10 + nextInt(21))`
            // 的取数顺序：`above` 先算 y，再算 x，最后算 z。
            // Draw order: `above` draws the y offset first, then x, then z.
            let dy = 20 + world.random().next_int(15);
            let dx = -10 + world.random().next_int(21);
            let dz = -10 + world.random().next_int(21);
            let pos = BlockPos::new(feet.x + dx, feet.y + dy, feet.z + dz);
            if !natural::is_valid_empty_spawn_block_for(world, MobType::Phantom, pos) {
                continue;
            }
            // `groupSize = 1 + nextInt(difficulty.getDifficulty().getId() + 1)`。
            let group_size = 1 + world.random().next_int(world.difficulty() + 1);
            for _ in 0..group_size {
                let Some(mut entity) = natural::create_mob(world, MobType::Phantom) else {
                    break;
                };
                // `phantom.snapTo(spawnPos, 0.0F, 0.0F) = Vec3.atBottomCenterOf(spawnPos)`。
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
                // `phantom.finalizeSpawn(level, difficulty, NATURAL, null)`：锚点上移 5 格、
                // 大小设 0，再走 `Mob.finalizeSpawn`（世界随机源取 3 个数）。
                spawn::finalize_spawn(
                    world,
                    MobType::Phantom,
                    &mut entity,
                    spawn::SpawnReason::Natural,
                );
                world.add_prepared_entity(entity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, testing::TestLevel};

    fn rules(spawn_mobs: bool) -> SpawnerRules {
        SpawnerRules {
            spawn_mobs,
            spawn_monsters: true,
            spawn_phantoms: true,
            // 跳过夜晚 / 露天 / 海平面的判断，只测抽点和生成。
            // Skip the night / open-sky / sea-level checks to test the rolls alone.
            has_sky_light: false,
        }
    }

    #[test]
    fn a_disabled_spawner_draws_nothing() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let before = level.random.source().seed();
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut spawner = PhantomSpawner::default();
            spawner.tick(
                &mut world,
                &[SpawnerPlayer {
                    position: Vec3::new(8.5, 64.0, 8.5),
                    spectator: false,
                    time_since_rest: i32::MAX,
                }],
                rules(false),
            );
            assert_eq!(spawner.next_tick, 0);
        }
        assert_eq!(level.random.source().seed(), before);
        assert_eq!(level.entities().len(), 0);
    }

    #[test]
    fn a_rested_player_never_spawns() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut spawner = PhantomSpawner::default();
            // `clamp(1, ...)` 让 `nextInt(1) == 0 < 72000`，绝不刷怪。
            // `clamp(1, ...)` makes `nextInt(1) == 0 < 72000`, so nothing spawns.
            spawner.tick(
                &mut world,
                &[SpawnerPlayer {
                    position: Vec3::new(8.5, 64.0, 8.5),
                    spectator: false,
                    time_since_rest: 0,
                }],
                rules(true),
            );
            // `nextTick--` 先变成 -1，再加 `(60 + nextInt(60)) * 20`，落在 1199..=2379。
            // `nextTick--` makes it -1 first, then adds `(60 + nextInt(60)) * 20`: 1199..=2379.
            assert!((1199..=2379).contains(&spawner.next_tick));
        }
        assert_eq!(level.entities().len(), 0);
    }

    #[test]
    fn spawned_phantoms_are_size_zero_with_an_anchor_five_above() {
        let mut any = false;
        for seed in 0..32_i64 {
            let mut level = TestLevel::new();
            level.random = crate::random::LevelRandom::new(seed);
            let mut ticks = BlockTicks::new();
            {
                let mut world = World::new(&mut level, &mut ticks);
                let mut spawner = PhantomSpawner::default();
                spawner.tick(
                    &mut world,
                    &[SpawnerPlayer {
                        position: Vec3::new(8.5, 64.0, 8.5),
                        spectator: false,
                        time_since_rest: i32::MAX,
                    }],
                    rules(true),
                );
            }
            for snap in level.entities_mut_for_test().take_spawn_snapshots() {
                let entity = level.entities().get(snap.uid).expect("just spawned");
                let mob = entity.kind.mob().expect("a phantom is a mob");
                assert_eq!(mob.mob_type, MobType::Phantom);
                assert_eq!(mob.phantom.size, 0);
                let feet = entity.block_position();
                assert_eq!(
                    mob.phantom.anchor,
                    Some(BlockPos::new(feet.x, feet.y + 5, feet.z))
                );
                assert_eq!(entity.position.x, f64::from(feet.x) + 0.5);
                assert_eq!(entity.position.z, f64::from(feet.z) + 0.5);
                // 落点在玩家头顶 20..=34、水平 ±10。
                assert!((84..=98).contains(&feet.y));
                assert!((-2..=18).contains(&feet.x));
                assert!((-2..=18).contains(&feet.z));
                any = true;
            }
        }
        assert!(any, "no seed spawned a phantom");
    }
}
