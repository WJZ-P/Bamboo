//! `DaylightDetectorBlock`。
//!
//! 这个世界是开阔的白天：太阳在正中，亮度是 15，所以官方算出来的信号强度只有两种结果。
//! 官方每秒重新算一次，那是方块实体的计时，这里只在右键时算。

use mc_bamboo_core::BlockPos;

use crate::{
    behaviour::Behaviour,
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, INVERTED, POWER},
    world::{UPDATE_CLIENTS, World},
};

/// 信号强度的上限。
const MAX_SIGNAL: i32 = 15;

/// `DaylightDetectorBlock`。
pub(super) struct DaylightDetector;

/// `updateSignalStrength`。
fn update_signal_strength(state: BlockState, world: &mut World, pos: BlockPos) {
    let brightness = world.raw_brightness(pos);
    let target = if state.get(INVERTED) {
        MAX_SIGNAL - brightness
    } else {
        brightness
    };
    if state.get(POWER) != target {
        world.set_block_and_update(pos, state.with(POWER, target));
    }
}

impl Behaviour for DaylightDetector {
    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let toggled = state.cycle(INVERTED);
        world.set_block(pos, toggled, UPDATE_CLIENTS);
        update_signal_strength(toggled, world, pos);
        InteractionResult::Success
    }
}
