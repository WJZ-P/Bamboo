//! `FlowingFluid` 和两个子类覆盖的部分：流动、下落、干涸、生成新的源头、冲掉挡路的方块，岩浆碰到水。

use std::collections::HashMap;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, blocks, tags::block_tags};

use super::{Fluid, can_be_replaced_with, empty, height};
use crate::{
    shape,
    state::{BlockState, FluidState},
    world::World,
};

/// `getSlopeDistance` 没找到口子时的距离。
const NO_SLOPE: i32 = 1000;
/// 从三个方向都有同种源头的格子往下流时，还会往旁边流。
const SOURCES_TO_KEEP_SPREADING: usize = 3;
/// 往下落的流体往旁边流时当作这么多。
const FALLING_SPREAD_AMOUNT: i32 = 7;

/// `FlowingFluid.tick`。
pub(super) fn tick(
    world: &mut World,
    kind: Fluid,
    pos: BlockPos,
    mut state: BlockState,
    mut fluid: FluidState,
) {
    if !fluid.is_source() {
        let current = world.block_state(pos);
        let new = new_liquid(world, kind, pos, current);
        let delay = spread_delay(world, kind, pos, fluid, new);
        if new.is_empty() {
            fluid = new;
            state = BlockState::AIR;
            world.set_block_and_update(pos, state);
        } else if new != fluid {
            fluid = new;
            state = fluid.legacy_block();
            world.set_block_and_update(pos, state);
            world.schedule_fluid_tick_with_delay(pos, fluid.fluid(), delay);
        }
    }
    spread(world, kind, pos, state, fluid);
}

/// `getSpreadDelay`。岩浆变高时官方有四分之三的概率慢四倍。
fn spread_delay(
    world: &World,
    kind: Fluid,
    pos: BlockPos,
    old: FluidState,
    new: FluidState,
) -> i32 {
    let delay = kind.tick_delay();
    let rising = kind == Fluid::Lava
        && !old.is_empty()
        && !new.is_empty()
        && !old.is_falling()
        && !new.is_falling()
        && height(new, world, pos) > height(old, world, pos);
    if rising && slow_lava(world) {
        delay * 4
    } else {
        delay
    }
}

/// `level.getRandom().nextInt(4) != 0`：涨起来的岩浆四分之三的机会慢四倍。
fn slow_lava(world: &World) -> bool {
    world.random().next_int(4) != 0
}

/// `spread`：能往下就往下，不能时往旁边。
fn spread(world: &mut World, kind: Fluid, pos: BlockPos, state: BlockState, fluid: FluidState) {
    if fluid.is_empty() {
        return;
    }
    let below_pos = pos.below();
    let below = world.block_state(below_pos);
    let below_fluid = below.fluid();
    if can_maybe_pass_through(
        world,
        kind,
        (pos, state),
        Direction::Down,
        (below_pos, below),
    ) {
        let new_below = new_liquid(world, kind, below_pos, below);
        let new_type = new_below.fluid();
        if can_be_replaced_with(below_fluid, world, below_pos, new_type, Direction::Down)
            && can_hold_specific_fluid(world, below_pos, below, new_type)
        {
            spread_to(world, kind, below_pos, below, Direction::Down, new_below);
            if source_neighbor_count(world, kind, pos) >= SOURCES_TO_KEEP_SPREADING {
                spread_to_sides(world, kind, pos, fluid, state);
            }
            return;
        }
    }
    if fluid.is_source() || !is_water_hole(world, kind, (pos, state), (below_pos, below)) {
        spread_to_sides(world, kind, pos, fluid, state);
    }
}

/// `spreadToSides`。
fn spread_to_sides(
    world: &mut World,
    kind: Fluid,
    pos: BlockPos,
    fluid: FluidState,
    state: BlockState,
) {
    let amount = if fluid.is_falling() {
        FALLING_SPREAD_AMOUNT
    } else {
        fluid.amount() - kind.drop_off()
    };
    if amount <= 0 {
        return;
    }
    for (direction, new) in spread_targets(world, kind, pos, state) {
        let neighbor_pos = pos.relative(direction);
        let neighbor = world.block_state(neighbor_pos);
        spread_to(world, kind, neighbor_pos, neighbor, direction, new);
    }
}

/// `getNewLiquid`：按四周和上面的同种流体，这一格应该是什么流体。
fn new_liquid(world: &World, kind: Fluid, pos: BlockPos, state: BlockState) -> FluidState {
    let mut highest = 0;
    let mut sources = 0;
    for direction in Direction::HORIZONTAL {
        let neighbor_pos = pos.relative(direction);
        let neighbor = world.block_state(neighbor_pos);
        let neighbor_fluid = neighbor.fluid();
        if kind.is_same(neighbor_fluid)
            && can_pass_through_wall(world, direction, (pos, state), (neighbor_pos, neighbor))
        {
            if neighbor_fluid.is_source() {
                sources += 1;
            }
            highest = highest.max(neighbor_fluid.amount());
        }
    }

    if sources >= 2 && kind.can_convert_to_source() {
        let below = world.block_state(pos.below());
        if below.is_solid() || is_source_of(kind, below.fluid()) {
            return kind.source_state();
        }
    }

    let above_pos = pos.above();
    let above = world.block_state(above_pos);
    if kind.is_same(above.fluid())
        && can_pass_through_wall(world, Direction::Up, (pos, state), (above_pos, above))
    {
        return kind.flowing_state(8, true);
    }

    let amount = highest - kind.drop_off();
    if amount <= 0 {
        empty()
    } else {
        kind.flowing_state(amount, false)
    }
}

/// `canPassThroughWall`：两格的碰撞形状在相接的面上合起来没有堵死。
///
/// 官方先用 `==` 比较 `Shapes.block()` 和 `Shapes.empty()` 两个单例，结果的缓存也只是加速。
/// 比的是同一个形状时，合并面的判定给出相同的答案，所以这里只做合并面的判定。
fn can_pass_through_wall(
    world: &World,
    direction: Direction,
    (source_pos, source): (BlockPos, BlockState),
    (target_pos, target): (BlockPos, BlockState),
) -> bool {
    let empty_context = shape::CollisionContext::Empty;
    let target_shape = target.collision_boxes(world, target_pos, empty_context);
    let source_shape = source.collision_boxes(world, source_pos, empty_context);
    !shape::merged_face_occludes(&source_shape, &target_shape, direction)
}

/// `spreadTo`，加上 `LavaFluid` 的覆盖：岩浆往下碰到水，水变石头，岩浆不过去。
fn spread_to(
    world: &mut World,
    kind: Fluid,
    pos: BlockPos,
    state: BlockState,
    direction: Direction,
    target: FluidState,
) {
    if kind == Fluid::Lava && direction == Direction::Down && world.fluid_state(pos).is_water() {
        if state.is_a(BlockAncestor::LiquidBlock) {
            world.set_block_and_update(pos, BlockState(blocks::STONE));
        }
        // `LavaFluid.fizz`。
        world.level_event(1501, pos, 0);
        return;
    }
    if state.is_a(BlockAncestor::LiquidBlockContainer) {
        state.place_liquid(world, pos, target);
        return;
    }
    // `beforeDestroyingBlock`：水让方块掉落成物品，这里没有实体；岩浆冲掉方块时嘶一声。
    if !state.is_air() && kind == Fluid::Lava {
        world.level_event(1501, pos, 0);
    }
    world.set_block_and_update(pos, target.legacy_block());
}

/// `getSpread`：往四周流的目标，只留去口子最近的方向。顺序是官方 `EnumMap` 的 `Direction` 顺序。
fn spread_targets(
    world: &World,
    kind: Fluid,
    pos: BlockPos,
    state: BlockState,
) -> Vec<(Direction, FluidState)> {
    let mut lowest = NO_SLOPE;
    let mut found: [Option<FluidState>; 6] = [None; 6];
    let mut slopes = SlopeSearch::new(pos);
    for direction in Direction::HORIZONTAL {
        let test_pos = pos.relative(direction);
        let test = world.block_state(test_pos);
        if !can_maybe_pass_through(world, kind, (pos, state), direction, (test_pos, test)) {
            continue;
        }
        let new = new_liquid(world, kind, test_pos, test);
        if !can_hold_specific_fluid(world, test_pos, test, new.fluid()) {
            continue;
        }
        let distance = if slopes.is_hole(world, kind, test_pos) {
            0
        } else {
            slopes.distance(world, kind, test_pos, 1, direction.opposite(), test)
        };
        if distance < lowest {
            found = [None; 6];
        }
        if distance <= lowest {
            if can_be_replaced_with(test.fluid(), world, test_pos, new.fluid(), direction) {
                found[direction.id() as usize] = Some(new);
            }
            lowest = distance;
        }
    }
    Direction::ALL
        .into_iter()
        .filter_map(|direction| Some((direction, found[direction.id() as usize]?)))
        .collect()
}

/// `SpreadContext`：一次 `getSpread` 里读过的方块和判过的口子。
///
/// 键只有相对的水平坐标，因为找下坡的路都在同一层。世界在这期间不变，缓存只是加速。
struct SlopeSearch {
    origin: BlockPos,
    states: HashMap<(i32, i32), BlockState>,
    holes: HashMap<(i32, i32), bool>,
}

impl SlopeSearch {
    fn new(origin: BlockPos) -> Self {
        Self {
            origin,
            states: HashMap::new(),
            holes: HashMap::new(),
        }
    }

    fn key(&self, pos: BlockPos) -> (i32, i32) {
        (pos.x - self.origin.x, pos.z - self.origin.z)
    }

    fn block_state(&mut self, world: &World, pos: BlockPos) -> BlockState {
        *self
            .states
            .entry(self.key(pos))
            .or_insert_with(|| world.block_state(pos))
    }

    fn is_hole(&mut self, world: &World, kind: Fluid, pos: BlockPos) -> bool {
        let key = self.key(pos);
        if let Some(&hole) = self.holes.get(&key) {
            return hole;
        }
        let state = self.block_state(world, pos);
        let below_pos = pos.below();
        let below = world.block_state(below_pos);
        let hole = is_water_hole(world, kind, (pos, state), (below_pos, below));
        self.holes.insert(key, hole);
        hole
    }

    /// `getSlopeDistance`：从 `pos` 出发，不回头，几步能走到往下的口子。
    fn distance(
        &mut self,
        world: &World,
        kind: Fluid,
        pos: BlockPos,
        pass: i32,
        from: Direction,
        state: BlockState,
    ) -> i32 {
        let mut lowest = NO_SLOPE;
        for direction in Direction::HORIZONTAL {
            if direction == from {
                continue;
            }
            let test_pos = pos.relative(direction);
            let test = self.block_state(world, test_pos);
            if !can_pass_through(world, kind, (pos, state), direction, (test_pos, test)) {
                continue;
            }
            if self.is_hole(world, kind, test_pos) {
                return pass;
            }
            if pass < kind.slope_find_distance() {
                let distance =
                    self.distance(world, kind, test_pos, pass + 1, direction.opposite(), test);
                lowest = lowest.min(distance);
            }
        }
        lowest
    }
}

/// `isWaterHole`：从 `top` 能流进下面的 `bottom`。
fn is_water_hole(
    world: &World,
    kind: Fluid,
    top: (BlockPos, BlockState),
    bottom: (BlockPos, BlockState),
) -> bool {
    if !can_pass_through_wall(world, Direction::Down, top, bottom) {
        return false;
    }
    let (bottom_pos, bottom_state) = bottom;
    kind.is_same(bottom_state.fluid())
        || can_hold_fluid(world, bottom_pos, bottom_state, kind.flowing())
}

/// `canPassThrough`：找下坡时，流动的这种流体能进这一格。
fn can_pass_through(
    world: &World,
    kind: Fluid,
    source: (BlockPos, BlockState),
    direction: Direction,
    test: (BlockPos, BlockState),
) -> bool {
    let (test_pos, test_state) = test;
    can_maybe_pass_through(world, kind, source, direction, test)
        && can_hold_specific_fluid(world, test_pos, test_state, kind.flowing())
}

/// `canMaybePassThrough`。
fn can_maybe_pass_through(
    world: &World,
    kind: Fluid,
    source: (BlockPos, BlockState),
    direction: Direction,
    test: (BlockPos, BlockState),
) -> bool {
    let (_, test_state) = test;
    !is_source_of(kind, test_state.fluid())
        && can_hold_any_fluid(test_state)
        && can_pass_through_wall(world, direction, source, test)
}

/// `isSourceBlockOfThisType`。
fn is_source_of(kind: Fluid, fluid: FluidState) -> bool {
    kind.is_same(fluid) && fluid.is_source()
}

/// `sourceNeighborCount`。
fn source_neighbor_count(world: &World, kind: Fluid, pos: BlockPos) -> usize {
    Direction::HORIZONTAL
        .into_iter()
        .filter(|&direction| is_source_of(kind, world.fluid_state(pos.relative(direction))))
        .count()
}

/// `canHoldAnyFluid`：流体能装进去，或者能冲掉。
fn can_hold_any_fluid(state: BlockState) -> bool {
    state.is_a(BlockAncestor::LiquidBlockContainer)
        || state.is_tagged(block_tags::WASHED_AWAY_BY_FLUIDS)
}

/// `canHoldFluid`。
fn can_hold_fluid(world: &World, pos: BlockPos, state: BlockState, fluid: i32) -> bool {
    can_hold_any_fluid(state) && can_hold_specific_fluid(world, pos, state, fluid)
}

/// `canHoldSpecificFluid`：容器自己说了算，别的方块都行。流体自己流过来，不是玩家。
fn can_hold_specific_fluid(world: &World, pos: BlockPos, state: BlockState, fluid: i32) -> bool {
    !state.is_a(BlockAncestor::LiquidBlockContainer)
        || state.can_place_liquid(world, pos, fluid, false)
}
