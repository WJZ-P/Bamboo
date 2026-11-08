//! `PotentSulfurBlock`：状态只看上下两格，喷发的倒计时在方块实体里，不在范围里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, registries::fluid, sounds::sound_event, tags::block_tags};

use crate::{
    behaviour::{Behaviour, common::default_state},
    context::PlaceContext,
    state::{BlockState, POTENT_SULFUR_STATE, PotentSulfurState},
    world::World,
};

/// `PotentSulfurBlock`。
pub(super) struct PotentSulfur;

/// `isSourceIfFluid`。
fn is_source_if_fluid(state: BlockState) -> bool {
    let fluid = state.fluid();
    fluid.is_empty() || fluid.is_source()
}

/// `validBlockState`。
fn valid_state(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    use PotentSulfurState as S;

    if !world.fluid_state(pos.above()).is(fluid::WATER) {
        return state.with(POTENT_SULFUR_STATE, S::Dry);
    }
    let below = world.block_state(pos.below());
    let source = is_source_if_fluid(below);
    if below.is_tagged(block_tags::CAUSES_CONTINUOUS_GEYSER_ERUPTIONS) && source {
        state.with(POTENT_SULFUR_STATE, S::Continuous)
    } else if !below.is_tagged(block_tags::CAUSES_PERIODIC_GEYSER_ERUPTIONS) || !source {
        state.with(POTENT_SULFUR_STATE, S::Wet)
    } else if state.get(POTENT_SULFUR_STATE) == S::Erupting {
        state
    } else {
        state.with(POTENT_SULFUR_STATE, S::Dormant)
    }
}

impl Behaviour for PotentSulfur {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(valid_state(default_state(block), world, ctx.clicked_pos()))
    }

    /// `onPlace`：放下时就在喷，先放喷发的声音（方块事件在方块实体里，不在范围）。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        let sound = match state.get(POTENT_SULFUR_STATE) {
            PotentSulfurState::Continuous => {
                Some("minecraft:block.potent_sulfur.geyser_continuous_eruption")
            }
            PotentSulfurState::Erupting => Some("minecraft:block.potent_sulfur.geyser_eruption"),
            PotentSulfurState::Dormant | PotentSulfurState::Wet | PotentSulfurState::Dry => None,
        };
        if let Some(sound) = sound {
            world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
        }
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        valid_state(state, world, pos)
    }
}
