//! `LavaFluid.randomTick`：岩浆点燃上面和旁边能烧的方块。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{block_maps::IGNITED_BY_LAVA, tags::block_tags};

use crate::{behaviour::misc::fire, state::BlockState, world::World};

/// 往上找几次可以点火的空气：0 次时改成在同一层旁边点。
const MAX_PASSES: i32 = 3;
/// 同一层旁边试几格。
const SIDE_ATTEMPTS: i32 = 3;

/// `LavaFluid.randomTick`。
pub(super) fn random_tick(world: &mut World, pos: BlockPos) {
    if !world.can_spread_fire_around(pos) {
        return;
    }
    let passes = world.random().next_int(MAX_PASSES);
    if passes > 0 {
        ignite_above(world, pos, passes);
    } else {
        ignite_beside(world, pos);
    }
}

/// 一步步往上斜着走，碰到旁边有能烧的方块的空气就点火，碰到挡火的方块就停。
fn ignite_above(world: &mut World, pos: BlockPos, passes: i32) {
    let mut test = pos;
    for _ in 0..passes {
        test = random_step(world, test, 1);
        if !world.is_loaded(test) {
            return;
        }
        let state = world.block_state(test);
        if state.is_air() {
            if has_flammable_neighbours(world, test) {
                let fire = fire::state_at(world, test);
                world.set_block_and_update(test, fire);
                return;
            }
        } else if state.is_tagged(block_tags::BLOCKS_LAVA_FIRE_SPREAD) {
            return;
        }
    }
}

/// 同一层的旁边挑三格，能烧的方块上面是空气时在上面点火。
fn ignite_beside(world: &mut World, pos: BlockPos) {
    for _ in 0..SIDE_ATTEMPTS {
        let test = random_step(world, pos, 0);
        if !world.is_loaded(test) {
            return;
        }
        if world.is_empty_block(test.above()) && is_flammable(world, test) {
            // 官方按下面这一格算火的状态，不是按点火的那一格。
            let fire = fire::state_at(world, test);
            world.set_block_and_update(test.above(), fire);
        }
    }
}

/// `pos.offset(nextInt(3) - 1, dy, nextInt(3) - 1)`，先取 x 再取 z。
fn random_step(world: &World, pos: BlockPos, dy: i32) -> BlockPos {
    let dx = world.random().next_int(3) - 1;
    let dz = world.random().next_int(3) - 1;
    pos.offset(dx, dy, dz)
}

fn has_flammable_neighbours(world: &World, pos: BlockPos) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| is_flammable(world, pos.relative(side)))
}

/// `isFlammable`：所有区块都在内存里，没有“区块没加载”的分支。
fn is_flammable(world: &World, pos: BlockPos) -> bool {
    ignited_by_lava(world.block_state(pos))
}

/// `ignitedByLava`。
fn ignited_by_lava(state: BlockState) -> bool {
    let id = i32::try_from(state.block_id()).expect("方块 ID 放得进 int");
    IGNITED_BY_LAVA.binary_search(&id).is_ok()
}
