//! `RespawnAnchorBlock`。
//!
//! 主世界里用充能的重生锚官方会爆炸，爆炸不在范围里：这里吃掉点击，方块留在原地。

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::items;

use crate::{
    behaviour::Behaviour,
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, RESPAWN_ANCHOR_CHARGES},
    world::World,
};

/// `RespawnAnchorBlock`。
pub(super) struct RespawnAnchor;

/// `MAX_CHARGES`。
const MAX_CHARGES: i32 = 4;

/// `canBeCharged`。
fn can_be_charged(state: BlockState) -> bool {
    state.get(RESPAWN_ANCHOR_CHARGES) < MAX_CHARGES
}

/// `getScaledChargeLevel`：官方算的是 `floor(charges / 4.0 * maximum)`，充能非负，整数除法一样。
fn scaled_charge_level(state: BlockState, maximum: i32) -> i32 {
    state.get(RESPAWN_ANCHOR_CHARGES) * maximum / MAX_CHARGES
}

impl Behaviour for RespawnAnchor {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item == items::GLOWSTONE && can_be_charged(state) {
            let charges = state.get(RESPAWN_ANCHOR_CHARGES) + 1;
            let charged = state.with(RESPAWN_ANCHOR_CHARGES, charges);
            world.set_block_and_update(ctx.clicked_pos(), charged);
            return InteractionResult::Success;
        }
        // 主手点不了，让副手的荧石来充能。
        if ctx.hand == Hand::Main && ctx.actor.off_hand == items::GLOWSTONE && can_be_charged(state)
        {
            InteractionResult::Pass
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(RESPAWN_ANCHOR_CHARGES) == 0 {
            InteractionResult::Pass
        } else {
            InteractionResult::SuccessServer
        }
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        scaled_charge_level(state, 15)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::testing::{TestLevel, actor_holding, top_hit, use_item};

    /// 官方这里爆炸，oracle 比不了，这里固定下来：什么也不发生，但是点击被吃掉。
    #[test]
    fn a_charged_anchor_does_not_explode() {
        let pos = BlockPos::new(0, 64, 0);
        let charged = BlockState(blocks::RESPAWN_ANCHOR).with(RESPAWN_ANCHOR_CHARGES, 1);
        let mut level = TestLevel::new();
        level.set(pos, charged);
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert_eq!(level.get(pos), charged);
    }
}
