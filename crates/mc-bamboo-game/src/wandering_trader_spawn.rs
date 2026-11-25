//! 岛的一刻里的流浪商人刷新器（官方 `ServerLevel.tickCustomSpawners`）。
//!
//! 官方把 `WanderingTraderSpawner` 登记在主世界的 `customSpawners` 里，排在
//! `CatSpawner` / `VillageSiege` 之后；Bamboo 只建模这三个自定义刷怪器里的
//! 幻翼、巡逻队和流浪商人，位置仍在随机刻之后、方块事件和实体之前。
//! `SPAWN_WANDERING_TRADERS` 由方块层再查一次；每 1200 刻改一次的存档计时
//! 通过 [`GlobalPort`](crate::global_port::GlobalPort) 交给主控，写进
//! `data/minecraft/wandering_trader.dat`。
//!
//! The island tick for the vanilla `WanderingTraderSpawner` (part of
//! `ServerLevel.tickCustomSpawners`). Vanilla registers it after the cat and
//! village-siege spawners; Bamboo models phantoms, patrols and this one, all
//! still after random ticks and before block events and entities. The saved
//! timer, changed every 1200 ticks, goes through `GlobalPort` to the master
//! for `data/minecraft/wandering_trader.dat`.

use mc_bamboo_blocks::{
    World,
    entity::mob::wandering_trader_spawner::{WanderingTraderData, WanderingTraderSpawner},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_data::gamerules::SPAWN_WANDERING_TRADERS;

use crate::{
    global_port::{GlobalEffect, GlobalPort},
    region::Region,
};

/// 跑一遍流浪商人刷新器，把改过的存档计时和生成副作用发布出去。
///
/// Run the wandering-trader spawner once and publish the saved timer and effects.
pub(crate) fn tick(region: &mut Region, port: &mut GlobalPort<'_>) {
    let enabled = region.global.rules.bool(SPAWN_WANDERING_TRADERS);
    // `getRandomPlayer` 的 `isAlive()` 过滤：只把活着的玩家交给刷新器，
    // `nextInt(players.size())` 的世界随机源取数由方块层在 `spawn` 里做。
    let players: Vec<Vec3> = region
        .players
        .values()
        .filter(|player| !player.health.is_dead_or_dying())
        .map(|player| player.position())
        .collect();
    let state = region.wandering_trader_spawner;
    let mut spawner = WanderingTraderSpawner::with_random_seed(
        region.global.wandering_trader_data,
        state.tick_delay,
        state.random_seed,
    );
    // `Region` 存的是 LCG 的内部种子（`LegacyRandom::seed()`），不是 `setSeed`
    // 的输入种子；恢复内部状态后取数序列才接着上一刻。
    spawner.random = mc_bamboo_blocks::random::LegacyRandom::from_raw_seed(state.random_seed);
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        spawner.tick(&mut world, &players, enabled);
        world.into_effects()
    };
    region.wandering_trader_spawner.tick_delay = spawner.tick_delay;
    region.wandering_trader_spawner.random_seed = spawner.random.seed();
    let data: WanderingTraderData = spawner.data;
    if data != region.global.wandering_trader_data {
        region.global.wandering_trader_data = data;
        port.submit(region, GlobalEffect::WanderingTraderData(data));
    }
    crate::interaction::send_effects(region, None, &effects);
}
