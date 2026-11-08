//! `NyliumBlock`、`NetherrackBlock`：被盖住的菌岩退回下界岩，骨粉让下界岩长出旁边的菌岩。
//!
//! 骨粉在菌岩上长下界植物要世界生成的地物，不做。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, tags::block_tags};

use crate::{behaviour::Behaviour, state::BlockState, world::World};

/// 挡光到这个程度（官方的 `getMaxLightLevel`）菌岩就活不下去。
const MAX_LIGHT: i32 = 15;

/// `NyliumBlock`。
pub(super) struct Nylium;

impl Behaviour for Nylium {
    /// `canBeNylium`：上面挡住了光就变回下界岩。
    fn random_tick(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let above = world.block_state(pos.above());
        if above.light_dampening_from_plain_block_below() >= MAX_LIGHT {
            world.set_block_and_update(pos, BlockState(blocks::NETHERRACK));
        }
    }
}

/// `NetherrackBlock`。
pub(super) struct Netherrack;

/// 周围 3×3×3 里的菌岩：有没有、有没有诡异的、有没有绯红的。
fn nearby_nylium(world: &World, pos: BlockPos) -> (bool, bool, bool) {
    let (mut any, mut warped, mut crimson) = (false, false, false);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let state = world.block_state(pos.offset(x, y, z));
                if state.is_tagged(block_tags::NYLIUM) {
                    any = true;
                    warped |= state.is(blocks::WARPED_NYLIUM);
                    crimson |= state.is(blocks::CRIMSON_NYLIUM);
                }
            }
        }
    }
    (any, warped, crimson)
}

impl Behaviour for Netherrack {
    /// 上面透光、旁边有菌岩。
    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.block_state(pos.above()).propagates_skylight_down() && nearby_nylium(world, pos).0
    }

    /// 两种都有时随机挑一种。
    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let nylium = match nearby_nylium(world, pos) {
            (_, true, true) => {
                if world.random().next_boolean() {
                    blocks::WARPED_NYLIUM
                } else {
                    blocks::CRIMSON_NYLIUM
                }
            }
            (_, true, false) => blocks::WARPED_NYLIUM,
            (_, false, true) => blocks::CRIMSON_NYLIUM,
            (_, false, false) => return,
        };
        world.set_block_and_update(pos, BlockState(nylium));
    }
}
