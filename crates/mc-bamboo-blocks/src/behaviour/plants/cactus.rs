//! `CactusBlock`。`CactusFlowerBlock` 只换了支撑条件，在 [`vegetation`](super::vegetation) 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain},
    state::{AGE_15, BlockState},
    world::{UPDATE_NONE, World},
};

/// 仙人掌最多长到三格高。
const MAX_HEIGHT: i32 = 3;
/// `age` 数到这里就长一格。
const MAX_AGE: i32 = 15;
/// `age` 数到这里时可能在顶上开花。
const FLOWER_AGE: i32 = 8;
/// 开花的概率，已经三格高时大一些。官方拿 `nextDouble()` 和它们比。
const FLOWER_CHANCE: f64 = 0.1;
const TALL_FLOWER_CHANCE: f64 = 0.25;
/// 官方写 `age` 用的标记，260：`UPDATE_NONE`。
const GROWTH_FLAGS: u32 = UPDATE_NONE;

/// `CactusBlock`。
pub(super) struct Cactus;

/// 仙人掌不能贴着实心方块和岩浆。
fn crowds_cactus(world: &World, pos: BlockPos) -> bool {
    world.block_state(pos).is_solid() || world.fluid_state(pos).is_lava()
}

impl Behaviour for Cactus {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let crowded = Direction::HORIZONTAL
            .into_iter()
            .any(|side| crowds_cactus(world, pos.relative(side)));
        let below = world.block_state(pos.below());
        !crowded
            && (below.is_same_block(state) || below.is_tagged(block_tags::SUPPORTS_CACTUS))
            && !world.block_state(pos.above()).is_liquid()
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
        schedule_break_if_unsupported(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }

    /// 上面空着时 `age` 加一。数到 8 时可能开花，数到 15 时往上长一格，最多三格高。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let above = pos.above();
        if !world.is_empty_block(above) {
            return;
        }
        let age = state.get(AGE_15);
        let Some(height) = stack_height(state, world, pos, age) else {
            return;
        };
        if age == FLOWER_AGE && Cactus.can_survive(state.default_state(), world, above) {
            let chance = if height >= MAX_HEIGHT {
                TALL_FLOWER_CHANCE
            } else {
                FLOWER_CHANCE
            };
            if world.random().next_double() <= chance {
                world.set_block_and_update(above, BlockState(blocks::CACTUS_FLOWER));
            }
        } else if age == MAX_AGE && height < MAX_HEIGHT {
            world.set_block_and_update(above, state.default_state());
            let reset = state.with(AGE_15, 0);
            world.set_block(pos, reset, GROWTH_FLAGS);
            world.neighbor_changed_with_state(reset, above, state.block());
        }
        if age < MAX_AGE {
            world.set_block(pos, state.with(AGE_15, age + 1), GROWTH_FLAGS);
        }
    }
}

/// 从这一格往下数，连着的仙人掌有几格，算上自己。已经三格高又数满了 15 时官方直接不长，返回 `None`。
fn stack_height(state: BlockState, world: &World, pos: BlockPos, age: i32) -> Option<i32> {
    let mut height = 1;
    while world
        .block_state(pos.relative_by(Direction::Down, height))
        .is_same_block(state)
    {
        height += 1;
        if height == MAX_HEIGHT && age == MAX_AGE {
            return None;
        }
    }
    Some(height)
}
