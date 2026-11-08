//! `SnowyBlock`、`GrassBlock`、`MyceliumBlock`，后两个的父类 `SpreadingSnowyBlock` 只多了随机刻。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block},
    },
    context::PlaceContext,
    state::{BlockState, LAYERS, SNOWY},
    world::World,
};

/// 满格流体的 `getAmount`。
const FULL_FLUID: i32 = 8;
/// 挡光到这个程度（官方的 `getMaxLightLevel`）草就活不下去。
const MAX_LIGHT: i32 = 15;
/// 上面要这么亮才往外蔓延。
const SPREAD_MIN_BRIGHTNESS: i32 = 9;
/// 每次随机刻往外试几次。
const SPREAD_ATTEMPTS: i32 = 4;

/// `SnowyBlock`：上面有雪时换上雪地的贴图。
pub(super) struct Snowy;

/// `isSnowySetting`。
fn is_snowy_setting(above: BlockState) -> bool {
    above.is_tagged(block_tags::SNOW)
}

impl Behaviour for Snowy {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let above = world.block_state(ctx.clicked_pos().above());
        Some(default_state(block).with(SNOWY, is_snowy_setting(above)))
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
        if direction == Direction::Up {
            state.with(SNOWY, is_snowy_setting(neighbor))
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        }
    }
}

/// `SpreadingSnowyBlock`：草方块和菌丝。被盖住时退回泥土，够亮时往附近的泥土上蔓延。
/// 骨粉在草方块上长花草要世界生成的地物，不做。
pub(super) struct SpreadingSnowy;

/// `canStayAlive`：上面是一层雪、或者透光，而且没有泡在满格的流体里。
fn can_stay_alive(world: &World, pos: BlockPos) -> bool {
    let above = world.block_state(pos.above());
    if above.is(blocks::SNOW) && above.get(LAYERS) == 1 {
        return true;
    }
    if above.fluid().amount() == FULL_FLUID {
        return false;
    }
    above.light_dampening_from_plain_block_below() < MAX_LIGHT
}

/// `canPropagate`：活得下去，上面也没有水。
fn can_propagate(world: &World, pos: BlockPos) -> bool {
    can_stay_alive(world, pos) && !world.fluid_state(pos.above()).is_water()
}

impl SpreadingSnowy {
    /// 往附近随机挑四次，挑中上面透光的泥土就把它变成自己。
    fn spread(state: BlockState, world: &mut World, pos: BlockPos, base: BlockState) {
        let spread = state.default_state();
        for _ in 0..SPREAD_ATTEMPTS {
            let random = world.random();
            let dx = random.next_int(3) - 1;
            let dy = random.next_int(5) - 3;
            let dz = random.next_int(3) - 1;
            let target = pos.offset(dx, dy, dz);
            if world.block_state(target).is_same_block(base) && can_propagate(world, target) {
                let snowy = is_snowy_setting(world.block_state(target.above()));
                world.set_block_and_update(target, spread.with(SNOWY, snowy));
            }
        }
    }
}

impl Behaviour for SpreadingSnowy {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Snowy.state_for_placement(block, ctx, world)
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
        Snowy.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let base = default_state(field_block(state.block(), "baseBlock"));
        if !can_stay_alive(world, pos) {
            world.set_block_and_update(pos, base);
        } else if world.max_local_raw_brightness(pos.above()) >= SPREAD_MIN_BRIGHTNESS {
            Self::spread(state, world, pos, base);
        }
    }
}
