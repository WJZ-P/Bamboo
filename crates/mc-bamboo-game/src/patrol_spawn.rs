//! 岛的一刻里的自定义刷怪器（官方 `ServerChunkCache.tickChunks` 末尾的
//! `level.tickCustomSpawners(spawnEnemies)`）。
//!
//! 目前只有一个 `PatrolSpawner`。官方在自然刷怪和随机刻之后才跑自定义刷怪器，这里放在
//! 随机刻之后、和随机刻同属一个剖析阶段。
//!
//! The island's custom spawners (vanilla `level.tickCustomSpawners(spawnEnemies)` at the
//! end of `ServerChunkCache.tickChunks`). Only the `PatrolSpawner` exists so far.
//! Vanilla runs custom spawners after natural spawning and the random ticks; this runs
//! after the random ticks, in the same profile phase.

use mc_bamboo_blocks::World;
use mc_bamboo_blocks::entity::mob::patrol::{self, PatrolFacts};
use mc_bamboo_data::gamerules::SPAWN_PATROLS;

use crate::region::Region;

/// 跑一遍自定义刷怪器。
///
/// Run the custom spawners once.
pub(crate) fn tick(region: &mut Region) {
    let facts = PatrolFacts {
        // 官方只有在 `SPAWN_MOBS` 为真时才调 `tickCustomSpawners`，`spawnEnemies` 再是
        // `SPAWN_MONSTERS`。
        // Vanilla calls `tickCustomSpawners` only with `SPAWN_MOBS` on; `spawnEnemies`
        // is `SPAWN_MONSTERS` on top of that.
        spawn_enemies: region.global.block_rules.spawn_mobs
            && region.global.block_rules.spawn_monsters,
        spawn_patrols: region.global.rules.bool(SPAWN_PATROLS),
    };
    let mut spawner = region.patrol_spawner;
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        patrol::tick(&mut world, &mut spawner, facts);
        world.into_effects()
    };
    region.patrol_spawner = spawner;
    crate::interaction::send_effects(region, None, &effects);
}
