//! 流体：水和岩浆的流动、桶。对应官方的 `FlowingFluid`、`WaterFluid`、`LavaFluid` 和 `BucketItem`。
//!
//! 流体的计划刻有自己的队列，每一刻在方块的计划刻之后跑，见 [`run_scheduled_ticks`](crate::run_scheduled_ticks)。
//! 世界是主世界。源头转换读 `water_source_conversion` 和 `lava_source_conversion`，
//! 默认是水能、岩浆不能。岩浆不是下界那种快速的岩浆。
//! 岩浆的随机刻会点燃附近能烧的方块，见 [`random_tick`]。

pub mod bucket;
mod flowing;
mod lava;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{blocks, items, registries::fluid};

use crate::{
    state::{BlockState, FluidState, LEVEL},
    world::World,
};

/// `LavaFluid.MIN_LEVEL_CUTOFF`：岩浆至少这么高，水流过来才会凝固它。
const LAVA_MIN_LEVEL_CUTOFF: f32 = 0.444_444_45;

/// 流体的种类。官方每种都有源头和流动两个 `Fluid`，规则相同，这里合成一个。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fluid {
    Water,
    Lava,
}

impl Fluid {
    /// 流体注册表 ID 的种类，空的流体是 `None`。
    pub const fn of_type(fluid: i32) -> Option<Self> {
        match fluid {
            fluid::WATER | fluid::FLOWING_WATER => Some(Self::Water),
            fluid::LAVA | fluid::FLOWING_LAVA => Some(Self::Lava),
            _ => None,
        }
    }

    /// 这个流体状态的种类。
    pub fn of(fluid: FluidState) -> Option<Self> {
        Self::of_type(fluid.fluid())
    }

    /// `isSame`：是同一种流体，源头和流动的不分。
    pub fn is_same(self, fluid: FluidState) -> bool {
        Self::of(fluid) == Some(self)
    }

    /// `getSource()` 的注册表 ID。
    pub const fn source(self) -> i32 {
        match self {
            Self::Water => fluid::WATER,
            Self::Lava => fluid::LAVA,
        }
    }

    /// `getFlowing()` 的注册表 ID。
    pub const fn flowing(self) -> i32 {
        match self {
            Self::Water => fluid::FLOWING_WATER,
            Self::Lava => fluid::FLOWING_LAVA,
        }
    }

    /// `getTickDelay`。
    pub const fn tick_delay(self) -> i32 {
        match self {
            Self::Water => 5,
            Self::Lava => 30,
        }
    }

    /// `getSlopeFindDistance`：找下坡的口子最远看几格。
    const fn slope_find_distance(self) -> i32 {
        match self {
            Self::Water => 4,
            Self::Lava => 2,
        }
    }

    /// `getDropOff`：往旁边流一格少多少。
    const fn drop_off(self) -> i32 {
        match self {
            Self::Water => 1,
            Self::Lava => 2,
        }
    }

    /// `canConvertToSource`：水读 `water_source_conversion`，岩浆读 `lava_source_conversion`。
    ///
    /// `canConvertToSource`: water reads `water_source_conversion`, lava reads `lava_source_conversion`.
    fn can_convert_to_source(self, world: &World) -> bool {
        let rules = world.block_game_rules();
        match self {
            Self::Water => rules.water_source_conversion,
            Self::Lava => rules.lava_source_conversion,
        }
    }

    /// `getBucket`。
    pub const fn bucket(self) -> i32 {
        match self {
            Self::Water => items::WATER_BUCKET,
            Self::Lava => items::LAVA_BUCKET,
        }
    }

    /// `createLegacyBlock` 用的方块，默认状态就是源头。
    const fn liquid_block(self) -> u32 {
        match self {
            Self::Water => blocks::WATER,
            Self::Lava => blocks::LAVA,
        }
    }

    /// `getSource(false)`。
    pub fn source_state(self) -> FluidState {
        BlockState(self.liquid_block()).fluid()
    }

    /// `getFlowing(amount, falling)`，借 `LiquidBlock` 的状态表取：`getLegacyLevel` 反过来就是它。
    fn flowing_state(self, amount: i32, falling: bool) -> FluidState {
        let level = 8 - amount.min(8) + if falling { 8 } else { 0 };
        BlockState(self.liquid_block()).with(LEVEL, level).fluid()
    }
}

/// `Fluids.EMPTY.defaultFluidState()`。
fn empty() -> FluidState {
    BlockState::AIR.fluid()
}

/// `FluidState.getHeight`：上面一格是同一种流体时是满格。
pub fn height(fluid: FluidState, world: &World, pos: BlockPos) -> f32 {
    match Fluid::of(fluid) {
        Some(kind) if kind.is_same(world.fluid_state(pos.above())) => 1.0,
        Some(_) => fluid.own_height(),
        None => 0.0,
    }
}

/// `FluidState.canBeReplacedWith`：`pos` 的流体 `fluid` 能不能被从 `direction` 流过来的 `other` 换掉。
fn can_be_replaced_with(
    fluid: FluidState,
    world: &World,
    pos: BlockPos,
    other: i32,
    direction: Direction,
) -> bool {
    let other_is_water = Fluid::of_type(other) == Some(Fluid::Water);
    match Fluid::of(fluid) {
        None => true,
        // 水只让岩浆从上面压下来。
        Some(Fluid::Water) => direction == Direction::Down && !other_is_water,
        Some(Fluid::Lava) => height(fluid, world, pos) >= LAVA_MIN_LEVEL_CUTOFF && other_is_water,
    }
}

/// `ServerLevel.tickFluid`：到期时这一格的流体还是排刻时那一种才执行。
pub(crate) fn tick(world: &mut World, pos: BlockPos, fluid: i32) {
    let state = world.block_state(pos);
    let current = state.fluid();
    if current.fluid() == fluid
        && let Some(kind) = Fluid::of(current)
    {
        flowing::tick(world, kind, pos, state, current);
    }
}

/// `FluidState.randomTick`：只有岩浆有，见 `lava`。只对 `is_fluid_randomly_ticking` 的状态调用。
pub(crate) fn random_tick(world: &mut World, pos: BlockPos, fluid: FluidState) {
    if Fluid::of(fluid) == Some(Fluid::Lava) {
        lava::random_tick(world, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, run_scheduled_ticks, testing::TestLevel};

    const SOURCE: BlockPos = BlockPos::new(0, 64, 0);

    /// 一块 9×9 的石头地板，上面一层是空气。
    fn floor() -> TestLevel {
        let mut level = TestLevel::new();
        for x in -4..=4 {
            for z in -4..=4 {
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
            }
        }
        level
    }

    /// 像玩家放下一样写入，会触发 `onPlace` 和邻居更新。
    fn place(level: &mut TestLevel, ticks: &mut BlockTicks, pos: BlockPos, state: BlockState) {
        World::new(level, ticks).set_block_and_update(pos, state);
    }

    /// 从下一刻跑到第 `last` 刻。
    fn run_until(level: &mut TestLevel, ticks: &mut BlockTicks, last: i64) {
        for now in ticks.now() + 1..=last {
            run_scheduled_ticks(level, ticks, now);
        }
    }

    fn water(level: i32) -> BlockState {
        BlockState::WATER.with(LEVEL, level)
    }

    #[test]
    fn a_source_spreads_one_step_every_five_ticks() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        place(&mut level, &mut ticks, SOURCE, BlockState::WATER);
        run_until(&mut level, &mut ticks, 4);
        assert!(level.get(SOURCE.north()).is_air());
        run_until(&mut level, &mut ticks, 5);
        for side in Direction::HORIZONTAL {
            assert_eq!(level.get(SOURCE.relative(side)), water(1), "{side:?}");
        }
        run_until(&mut level, &mut ticks, 10);
        assert_eq!(level.get(SOURCE.north().north()), water(2));
    }

    #[test]
    fn water_falls_and_spreads_from_the_bottom() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        let high = SOURCE.above().above();
        place(&mut level, &mut ticks, high, BlockState::WATER);
        run_until(&mut level, &mut ticks, 15);
        // 往下落的水是满格的“正在下落”，落地后往旁边流。
        assert_eq!(level.get(high.below()), water(8));
        assert_eq!(level.get(SOURCE), water(8));
        assert_eq!(level.get(SOURCE.east()), water(1));
        // 悬空的源头下面已经有往下落的水了，这时它也往旁边流。
        assert_eq!(level.get(high.east()), water(1));
    }

    #[test]
    fn two_sources_make_a_third() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        // 一条三格长的沟，水只能往中间那格流。
        for x in -1..=3 {
            level.set(BlockPos::new(x, 64, -1), BlockState(blocks::STONE));
            level.set(BlockPos::new(x, 64, 1), BlockState(blocks::STONE));
        }
        level.set(SOURCE.west(), BlockState(blocks::STONE));
        level.set(BlockPos::new(3, 64, 0), BlockState(blocks::STONE));
        place(&mut level, &mut ticks, SOURCE, BlockState::WATER);
        place(
            &mut level,
            &mut ticks,
            SOURCE.east().east(),
            BlockState::WATER,
        );
        run_until(&mut level, &mut ticks, 10);
        assert_eq!(level.get(SOURCE.east()), BlockState::WATER);
    }

    #[test]
    fn flowing_water_dries_up_without_its_source() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        place(&mut level, &mut ticks, SOURCE, BlockState::WATER);
        run_until(&mut level, &mut ticks, 10);
        place(&mut level, &mut ticks, SOURCE, BlockState::AIR);
        run_until(&mut level, &mut ticks, 60);
        for x in -4..=4 {
            for z in -4..=4 {
                let pos = BlockPos::new(x, 64, z);
                assert!(level.get(pos).is_air(), "{pos:?}");
            }
        }
    }

    #[test]
    fn lava_flowing_down_into_water_makes_stone() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        level.set(SOURCE, BlockState::WATER);
        place(
            &mut level,
            &mut ticks,
            SOURCE.above(),
            BlockState(blocks::LAVA),
        );
        run_until(&mut level, &mut ticks, 30);
        assert_eq!(level.get(SOURCE), BlockState(blocks::STONE));
        assert_eq!(level.get(SOURCE.above()), BlockState(blocks::LAVA));
    }

    #[test]
    fn lava_spreads_slower_and_shorter_than_water() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        place(&mut level, &mut ticks, SOURCE, BlockState(blocks::LAVA));
        run_until(&mut level, &mut ticks, 29);
        assert!(level.get(SOURCE.west()).is_air());
        run_until(&mut level, &mut ticks, 120);
        let lava = |level: i32| BlockState(blocks::LAVA).with(LEVEL, level);
        assert_eq!(level.get(SOURCE.west()), lava(2));
        assert_eq!(level.get(SOURCE.west().west()), lava(4));
        assert_eq!(level.get(SOURCE.west().west().west()), lava(6));
        assert!(level.get(SOURCE.west().west().west().west()).is_air());
    }

    #[test]
    fn water_next_to_lava_turns_it_into_obsidian_or_cobblestone() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        place(&mut level, &mut ticks, SOURCE, BlockState(blocks::LAVA));
        place(&mut level, &mut ticks, SOURCE.north(), BlockState::WATER);
        assert_eq!(level.get(SOURCE), BlockState(blocks::OBSIDIAN));

        let flowing_lava = BlockState(blocks::LAVA).with(LEVEL, 2);
        place(&mut level, &mut ticks, SOURCE.east(), flowing_lava);
        place(
            &mut level,
            &mut ticks,
            SOURCE.east().south(),
            BlockState::WATER,
        );
        assert_eq!(level.get(SOURCE.east()), BlockState(blocks::COBBLESTONE));
    }

    #[test]
    fn waterlogged_blocks_are_not_filled_by_flowing_water() {
        let mut level = floor();
        let mut ticks = BlockTicks::new();
        let slab = BlockState(blocks::OAK_SLAB);
        level.set(SOURCE.east(), slab);
        place(&mut level, &mut ticks, SOURCE, BlockState::WATER);
        run_until(&mut level, &mut ticks, 20);
        assert_eq!(level.get(SOURCE.east()), slab);
    }
}
