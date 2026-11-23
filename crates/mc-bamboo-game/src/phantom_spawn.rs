//! 幻翼自定义刷怪器的一刻（官方 `ServerLevel.tickCustomSpawners`）。
//!
//! `ServerChunkCache.tickChunks` 在逐区块自然刷怪和随机刻之后调用
//! `tickCustomSpawners(spawnEnemies)`，主世界的 `customSpawners` 里就有
//! `PhantomSpawner`。这里插在 [`crate::natural_spawn::tick`] 和随机刻之后、方块事件和实体
//! 之前，位置照它。玩家 `TIME_SINCE_REST` 由 [`crate::sleep::tick_sleep_counters`] 每刻
//! 推进，这里只读。
//!
//! The phantom custom spawner's tick (vanilla `ServerLevel.tickCustomSpawners`).
//! `ServerChunkCache.tickChunks` calls `tickCustomSpawners(spawnEnemies)` after the
//! per-chunk natural spawning and the random ticks; the overworld's `customSpawners`
//! include `PhantomSpawner`. This runs right after [`crate::natural_spawn::tick`] and
//! the random ticks, before block events and entities. `TIME_SINCE_REST` is advanced
//! each tick by [`crate::sleep::tick_sleep_counters`]; this only reads it.

use mc_bamboo_blocks::{
    World,
    entity::mob::phantom_spawner::{self, SpawnerPlayer, SpawnerRules},
};
use mc_bamboo_protocol::packet::play::GameMode;

use crate::region::Region;

/// `PhantomSpawner.tick(level, spawnEnemies)`。`spawn_mobs` / `spawn_monsters` /
/// `spawn_phantoms` 三道规则都在 [`SpawnerRules`] 里由方块层再查一遍。
///
/// `PhantomSpawner.tick(level, spawnEnemies)`: the three rules are re-checked by the
/// block layer through [`SpawnerRules`].
pub(crate) fn tick(region: &mut Region) {
    let rules = region.global.block_rules;
    let spawn = SpawnerRules {
        spawn_mobs: rules.spawn_mobs,
        spawn_monsters: rules.spawn_monsters,
        spawn_phantoms: rules.spawn_phantoms,
        // 主世界 `DimensionType.hasSkyLight()` 恒真；自定义刷怪器只登记在主世界。
        // The overworld's `DimensionType.hasSkyLight()` is always true; the custom
        // spawner is only registered there.
        has_sky_light: true,
    };
    let players: Vec<SpawnerPlayer> = region
        .players
        .values()
        .map(|player| SpawnerPlayer {
            position: player.position(),
            spectator: player.game_mode == GameMode::Spectator,
            time_since_rest: player.time_since_rest,
        })
        .collect();
    let mut next_tick = region.phantom_spawner.next_tick;
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut spawner = phantom_spawner::PhantomSpawner { next_tick };
        spawner.tick(&mut world, &players, spawn);
        next_tick = spawner.next_tick;
        world.into_effects()
    };
    region.phantom_spawner.next_tick = next_tick;
    crate::interaction::send_effects(region, None, &effects);
}
