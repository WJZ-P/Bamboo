//! `DaylightDetectorBlock`。
//!
//! 世界时钟停在正午，太阳在正中，官方按太阳角度打的折扣是 1，信号强度就是这一格的天空光，反转时是 15 减它。
//! 只看天空光，火把照不亮它。官方每秒重新算一次，那是方块实体的计时，这里只在右键时算。

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
    let brightness = world.effective_sky_brightness(pos);
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

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        state.get(POWER)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::testing::{TestLevel, actor_holding, top_hit, use_item};

    const DETECTOR: BlockPos = BlockPos::new(0, 64, 0);

    fn toggle(level: &mut TestLevel) -> BlockState {
        use_item(level, &actor_holding(items::AIR), top_hit(DETECTOR));
        level.get(DETECTOR)
    }

    #[test]
    fn the_signal_follows_sky_light_and_ignores_torches() {
        let mut level = TestLevel::new();
        level.set(DETECTOR, BlockState(blocks::DAYLIGHT_DETECTOR));
        level.light = Some((6, 14));

        let inverted = toggle(&mut level);
        assert!(inverted.get(INVERTED));
        assert_eq!(inverted.get(POWER), 9);

        let normal = toggle(&mut level);
        assert!(!normal.get(INVERTED));
        assert_eq!(normal.get(POWER), 6);
    }
}
