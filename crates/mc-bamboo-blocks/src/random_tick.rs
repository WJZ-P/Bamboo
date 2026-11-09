//! 随机刻：官方 `ServerLevel.tickChunk` 里和方块有关的部分。
//!
//! 每个被模拟的区块每刻先掷 `speed` 次天气的骰子，再在每个有随机刻方块的区块段里随机挑 `speed` 格，
//! 调用这一格的 `randomTick` 和它的流体的 `randomTick`。挑格子用的是 `Level.randValue` 那个线性同余，
//! 方块自己的逻辑用 `Level.random`，两个都在 [`LevelRandom`](crate::random::LevelRandom) 里。

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{BlockAncestor, blocks};

use crate::{
    fluid,
    interact::{InteractionResult, Outcome, finish},
    level::Level,
    state::{BlockState, LAYERS, LEVEL_CAULDRON},
    ticks::BlockTicks,
    weather::Precipitation,
    world::World,
};

/// 游戏规则 `random_tick_speed` 的默认值：每个区块段每刻挑几格。
///
/// 运行时的速度从 [`crate::Level::block_game_rules`] 读。这个常数只是默认值，
/// 必须和生成表里的 `GameRules.RANDOM_TICK_SPEED` 一样。
///
/// Runtime speed comes from [`crate::Level::block_game_rules`]. This constant is only the
/// default and must match the generated `GameRules.RANDOM_TICK_SPEED`.
pub const DEFAULT_RANDOM_TICK_SPEED: i32 = 3;
/// 每次掷骰子有这么多分之一的机会碰一下天气。
const PRECIPITATION_CHANCE: i32 = 48;
/// `getBlockRandomPos` 的 `yMask`：区块段的 16 层。
const SECTION_Y_MASK: i32 = 15;
/// `ServerLevel.tickThunder` 的雷击骰子。
const LIGHTNING_CHANCE: i32 = 100_000;

/// `ServerLevel.tickChunk(chunk, tickSpeed)`：一个区块这一刻的随机刻。
pub fn tick_chunk(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    chunk: ChunkPos,
    speed: i32,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let origin = BlockPos::new(chunk.x << 4, 0, chunk.z << 4);
    roll_precipitation(&mut world, origin, speed);
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

/// `ServerLevel.tickThunder(chunk)`：打雷时每个实体 ticking 区块掷一次骰子。
///
/// 官方命中后还要找闪电落点、看难度和刷怪规则取随机数，最后生成闪电实体或陷阱马。
/// Bamboo 没有闪电实体，只取官方要取的随机数：落点候选里的 `nextInt(list.size())` 和
/// 陷阱马的 `nextDouble()` 都不生成实体，这里按「没有实体」的分支走。
pub fn tick_thunder(level: &mut dyn Level, ticks: &mut BlockTicks, chunk: ChunkPos) -> Outcome {
    let world = World::new(level, ticks);
    if world.is_raining() && world.is_thundering() {
        let origin = BlockPos::new(chunk.x << 4, 0, chunk.z << 4);
        if world.random().next_int(LIGHTNING_CHANCE) == 0 {
            // `findLightningTargetAround` 先取一格；没有实体候选时不额外取数。
            let _target = world.block_random_pos(origin, SECTION_Y_MASK);
        }
    }
    finish(world, InteractionResult::Pass)
}

/// `tickChunk` 开头的 `tickPrecipitation`：骰子掷中时挑一格看要不要结冰、积雪。
///
/// 晴天时只剩挑格子这一步，它会推进 `randValue`，后面挑的格子要和官方一样就不能省。
fn roll_precipitation(world: &mut World, origin: BlockPos, speed: i32) {
    for _ in 0..speed {
        if world.random().next_int(PRECIPITATION_CHANCE) == 0 {
            let pos = world.block_random_pos(origin, SECTION_Y_MASK);
            tick_precipitation(world, pos);
        }
    }
}

/// `ServerLevel.tickPrecipitation(pos)`，照官方顺序：结冰、积雪、炼药锅接雨接雪。
fn tick_precipitation(world: &mut World, pos: BlockPos) {
    let top = BlockPos::new(pos.x, world.level().heightmap_y(pos), pos.z);
    let below = top.below();
    // 官方 `shouldFreeze` 和 `handlePrecipitation` 的温度看 below，`shouldSnow` 看 top。
    let below_facts = world.level().precipitation_facts(below);
    let top_facts = world.level().precipitation_facts(top);
    if should_freeze(world, below, below_facts) {
        world.set_block_and_update(below, BlockState(blocks::ICE));
    }
    if !world.is_raining() {
        return;
    }
    let max_snow = world.block_game_rules().max_snow_accumulation_height;
    if max_snow > 0 && should_snow(world, top, top_facts) {
        let state = world.block_state(top);
        if state.is(blocks::SNOW) {
            let layers = state.get(LAYERS);
            if layers < max_snow.min(8) {
                // 官方还要 `pushEntitiesUp`，Bamboo 没有实体可以推。
                world.set_block_and_update(top, state.with(LAYERS, layers + 1));
            }
        } else {
            world.set_block_and_update(top, BlockState(blocks::SNOW));
        }
    }
    if below_facts.precipitation != Precipitation::None {
        handle_precipitation(world, below, below_facts.precipitation);
    }
}

/// `Biome.shouldFreeze(level, pos)`：暖得下雨不结冰，方块光太亮不结冰，必须是一格水源，
/// 而且四边也都是水（官方要求水块四周有水）。
fn should_freeze(world: &World, pos: BlockPos, facts: crate::weather::PrecipitationFacts) -> bool {
    if facts.warm_enough_to_rain || !in_world(world, pos) || world.block_light(pos) >= 10 {
        return false;
    }
    let state = world.block_state(pos);
    if !state.fluid().is_water() || !state.is_a(BlockAncestor::LiquidBlock) {
        return false;
    }
    [pos.west(), pos.east(), pos.north(), pos.south()]
        .into_iter()
        .all(|at| world.is_water_at(at))
}

/// `Biome.shouldSnow(level, pos)`：只有雪群系、方块光暗、空气或雪层、雪层放得住。
fn should_snow(world: &World, pos: BlockPos, facts: crate::weather::PrecipitationFacts) -> bool {
    if facts.precipitation != Precipitation::Snow || !in_world(world, pos) {
        return false;
    }
    if world.block_light(pos) >= 10 {
        return false;
    }
    let state = world.block_state(pos);
    if !state.is_air() && !state.is(blocks::SNOW) {
        return false;
    }
    BlockState(blocks::SNOW).can_survive(world, pos)
}

/// `Block.handlePrecipitation`：只有空锅和分层锅会接，别的方块是空实现。
fn handle_precipitation(world: &mut World, pos: BlockPos, kind: Precipitation) {
    let chance = match kind {
        Precipitation::Rain => 0.05_f32,
        Precipitation::Snow => 0.1_f32,
        Precipitation::None => return,
    };
    // `CauldronBlock.shouldHandlePrecipitation` 和 `LayeredCauldronBlock` 的第一件事都是取数。
    let state = world.block_state(pos);
    if !state.is(blocks::CAULDRON)
        && !state.is(blocks::WATER_CAULDRON)
        && !state.is(blocks::POWDER_SNOW_CAULDRON)
    {
        return;
    }
    if world.random().next_float() >= chance {
        return;
    }
    if state.is(blocks::CAULDRON) {
        let filled = if kind == Precipitation::Rain {
            BlockState(blocks::WATER_CAULDRON)
        } else {
            BlockState(blocks::POWDER_SNOW_CAULDRON)
        };
        world.set_block_and_update(pos, filled);
        return;
    }
    let level = state.get(LEVEL_CAULDRON);
    let layered = if kind == Precipitation::Rain {
        state.is(blocks::WATER_CAULDRON)
    } else {
        state.is(blocks::POWDER_SNOW_CAULDRON)
    };
    if level < 3 && layered {
        world.set_block_and_update(pos, state.cycle(LEVEL_CAULDRON));
    }
}

fn in_world(world: &World, pos: BlockPos) -> bool {
    world.is_inside_build_height(pos.y)
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
