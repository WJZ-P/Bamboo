//! `ChorusFlowerBlock`、`ChorusPlantBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block, pipe_property},
    },
    context::PlaceContext,
    state::{AGE_5, BlockState},
    world::{UPDATE_CLIENTS, World},
};

/// `age` 是 5 的花枯死了，不再长；4 的花不再分叉。
const DEAD_AGE: i32 = 5;
/// 往下最多再数几节柱子。
const MAX_PILLAR_CHECK: i32 = 4;

/// `ChorusFlowerBlock`。
pub(super) struct ChorusFlower;

/// 下面是空气时，这朵花要贴着且只贴着一根紫颂植株，其余方向都要是空气。
fn hangs_from_one_plant(plant: BlockState, world: &World, pos: BlockPos) -> bool {
    let mut plants = 0;
    for side in Direction::HORIZONTAL {
        let neighbor = world.block_state(pos.relative(side));
        if neighbor.is_same_block(plant) {
            plants += 1;
        } else if !neighbor.is_air() {
            return false;
        }
    }
    plants == 1
}

impl Behaviour for ChorusFlower {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let plant = default_state(field_block(state.block(), "plant"));
        let below = world.block_state(pos.below());
        below.is_same_block(plant)
            || below.is_tagged(block_tags::SUPPORTS_CHORUS_FLOWER)
            || below.is_air() && hangs_from_one_plant(plant, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if direction != Direction::Up {
            schedule_break_if_unsupported(state, world, pos);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }

    /// 上面空着时往上长，长不了就往旁边分叉，分不了叉就枯死。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let above = pos.above();
        let age = state.get(AGE_5);
        if !world.is_empty_block(above) || above.y > world.max_y() || age >= DEAD_AGE {
            return;
        }
        let plant = default_state(field_block(state.block(), "plant"));
        let (grow_upwards, on_support) = wants_to_grow_up(plant, world, pos);
        if grow_upwards
            && all_neighbors_empty(world, above, None)
            && world.is_empty_block(above.above())
        {
            become_plant(plant, world, pos);
            place_grown_flower(state, world, above, age);
        } else if age < DEAD_AGE - 1 {
            branch(state, plant, world, pos, on_support);
        } else {
            place_dead_flower(state, world, pos);
        }
    }
}

/// 要不要往上长，以及下面是不是一根直接立在支撑方块上的柱子（这时分叉多试一次）。
fn wants_to_grow_up(plant: BlockState, world: &World, pos: BlockPos) -> (bool, bool) {
    let below = world.block_state(pos.below());
    if below.is_tagged(block_tags::SUPPORTS_CHORUS_FLOWER) || below.is_air() {
        return (true, false);
    }
    if !below.is_same_block(plant) {
        return (false, false);
    }
    let mut height = 1;
    let mut on_support = false;
    for _ in 0..MAX_PILLAR_CHECK {
        let test = world.block_state(pos.relative_by(Direction::Down, height + 1));
        if !test.is_same_block(plant) {
            on_support = test.is_tagged(block_tags::SUPPORTS_CHORUS_FLOWER);
            break;
        }
        height += 1;
    }
    let bound = if on_support { 5 } else { 4 };
    let grow = height < 2 || height <= world.random().next_int(bound);
    (grow, on_support)
}

/// 往随机的水平方向试着分叉，一个都没分出去时枯死。
fn branch(flower: BlockState, plant: BlockState, world: &mut World, pos: BlockPos, on_support: bool) {
    let attempts = world.random().next_int(4) + i32::from(on_support);
    let age = flower.get(AGE_5);
    let mut branched = false;
    for _ in 0..attempts {
        let direction = world.random().horizontal_direction();
        let target = pos.relative(direction);
        if world.is_empty_block(target)
            && world.is_empty_block(target.below())
            && all_neighbors_empty(world, target, Some(direction.opposite()))
        {
            place_grown_flower(flower, world, target, age + 1);
            branched = true;
        }
    }
    if branched {
        become_plant(plant, world, pos);
    } else {
        place_dead_flower(flower, world, pos);
    }
}

/// 花变成紫颂植株，按六个邻居连好。
fn become_plant(plant: BlockState, world: &mut World, pos: BlockPos) {
    let connected = state_with_connections(world, pos, plant);
    world.set_block(pos, connected, UPDATE_CLIENTS);
}

/// `placeGrownFlower`。
fn place_grown_flower(flower: BlockState, world: &mut World, pos: BlockPos, age: i32) {
    world.set_block(pos, flower.default_state().with(AGE_5, age), UPDATE_CLIENTS);
}

/// `placeDeadFlower`。
fn place_dead_flower(flower: BlockState, world: &mut World, pos: BlockPos) {
    world.set_block(pos, flower.default_state().with(AGE_5, DEAD_AGE), UPDATE_CLIENTS);
}

/// `allNeighborsEmpty`：四个水平邻居都是空气，`ignore` 那一面不看。
fn all_neighbors_empty(world: &World, pos: BlockPos, ignore: Option<Direction>) -> bool {
    Direction::HORIZONTAL
        .into_iter()
        .filter(|&side| Some(side) != ignore)
        .all(|side| world.is_empty_block(pos.relative(side)))
}

/// `ChorusPlantBlock.getStateWithConnections`。
fn state_with_connections(world: &World, pos: BlockPos, plant: BlockState) -> BlockState {
    Direction::ALL.into_iter().fold(plant, |state, side| {
        let neighbor = world.block_state(pos.relative(side));
        state.with(pipe_property(side), connects_to(plant, neighbor, side))
    })
}

/// `ChorusPlantBlock`。
pub(super) struct ChorusPlant;

/// 这一侧的邻居和紫颂植株连在一起。只有下面可以是支撑方块。
fn connects_to(plant: BlockState, neighbor: BlockState, side: Direction) -> bool {
    neighbor.is_same_block(plant)
        || neighbor.is(blocks::CHORUS_FLOWER)
        || side == Direction::Down && neighbor.is_tagged(block_tags::SUPPORTS_CHORUS_PLANT)
}

/// `below` 能托住紫颂植株。
fn holds_plant(plant: BlockState, below: BlockState) -> bool {
    below.is_same_block(plant) || below.is_tagged(block_tags::SUPPORTS_CHORUS_PLANT)
}

impl Behaviour for ChorusPlant {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(state_with_connections(
            world,
            ctx.clicked_pos(),
            default_state(block),
        ))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        let above_and_below = !world.block_state(pos.above()).is_air() && !below.is_air();
        for side in Direction::HORIZONTAL {
            let neighbor_pos = pos.relative(side);
            if world.block_state(neighbor_pos).is_same_block(state) {
                if above_and_below {
                    return false;
                }
                if holds_plant(state, world.block_state(neighbor_pos.below())) {
                    return true;
                }
            }
        }
        holds_plant(state, below)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if !state.can_survive(world, pos) {
            world.schedule_tick(pos, state.block(), 1);
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        state.with(
            pipe_property(direction),
            connects_to(state, neighbor, direction),
        )
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}
