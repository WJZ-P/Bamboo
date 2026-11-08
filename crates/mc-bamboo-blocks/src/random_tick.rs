//! 随机刻：官方 `ServerLevel.tickChunk` 里和方块有关的部分。
//!
//! 每个被模拟的区块每刻先掷 `speed` 次天气的骰子，再在每个有随机刻方块的区块段里随机挑 `speed` 格，
//! 调用这一格的 `randomTick` 和它的流体的 `randomTick`。挑格子用的是 `Level.randValue` 那个线性同余，
//! 方块自己的逻辑用 `Level.random`，两个都在 [`LevelRandom`](crate::random::LevelRandom) 里。

use mc_bamboo_core::{BlockPos, ChunkPos};

use crate::{
    fluid,
    interact::{InteractionResult, Outcome, finish},
    level::Level,
    ticks::BlockTicks,
    world::World,
};

/// 游戏规则 `random_tick_speed` 的默认值：每个区块段每刻挑几格。
pub const DEFAULT_RANDOM_TICK_SPEED: i32 = 3;
/// 每次掷骰子有这么多分之一的机会碰一下天气。
const PRECIPITATION_CHANCE: i32 = 48;
/// `getBlockRandomPos` 的 `yMask`：区块段的 16 层。
const SECTION_Y_MASK: i32 = 15;

/// `ServerLevel.tickChunk(chunk, tickSpeed)`：一个区块这一刻的随机刻。
pub fn tick_chunk(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    chunk: ChunkPos,
    speed: i32,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let origin = BlockPos::new(chunk.x << 4, 0, chunk.z << 4);
    roll_precipitation(&world, origin, speed);
    if speed > 0 {
        let first = world.min_y() >> 4;
        let last = world.max_y() >> 4;
        for section_y in first..=last {
            if world.is_randomly_ticking_section(chunk, section_y) {
                let section_origin = BlockPos::new(origin.x, section_y << 4, origin.z);
                tick_section(&mut world, section_origin, speed);
            }
        }
    }
    finish(world, InteractionResult::Pass)
}

/// `tickChunk` 开头的 `tickPrecipitation`：骰子掷中时挑一格看要不要结冰、积雪。
///
/// 世界只有平原，平原在任何高度都暖得下雨，水不会结冰；天永远是晴的，不会积雪，也没有雨落进炼药锅。
/// 所以掷中时只剩挑格子这一步，它会推进 `randValue`，后面挑的格子要和官方一样就不能省。
fn roll_precipitation(world: &World, origin: BlockPos, speed: i32) {
    for _ in 0..speed {
        if world.random().next_int(PRECIPITATION_CHANCE) == 0 {
            world.block_random_pos(origin, SECTION_Y_MASK);
        }
    }
}

/// 一个区块段里挑 `speed` 格做随机刻。
fn tick_section(world: &mut World, origin: BlockPos, speed: i32) {
    for _ in 0..speed {
        let pos = world.block_random_pos(origin, SECTION_Y_MASK);
        tick_one(world, pos);
    }
}

/// 选中的一格：方块的 `randomTick`，再是开始时那个状态的流体的 `randomTick`。
fn tick_one(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if state.is_randomly_ticking() {
        state.random_tick(world, pos);
    }
    if state.is_fluid_randomly_ticking() {
        fluid::random_tick(world, pos, state.fluid());
    }
}

/// 只对 `pos` 这一格做一次随机刻，和 `tickChunk` 选中这一格时做的事相同。oracle 用它逐个方块比较。
pub fn random_tick(level: &mut dyn Level, ticks: &mut BlockTicks, pos: BlockPos) -> Outcome {
    let mut world = World::new(level, ticks);
    tick_one(&mut world, pos);
    finish(world, InteractionResult::Pass)
}
