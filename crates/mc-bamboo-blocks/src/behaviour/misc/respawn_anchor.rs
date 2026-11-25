//! `RespawnAnchorBlock`。
//!
//! 主世界里用充能的重生锚空手右键会爆炸（`RespawnAnchorBlock.explode`，强度 5、带火、
//! `bad_respawn_point` 伤害、`BLOCK` 交互）；Bamboo 只有主世界，所以充能后空手右键必爆。

use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::{items, sounds::sound_event};

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
            let pos = ctx.clicked_pos();
            world.set_block_and_update(pos, charged);
            // `charge`：方块中心出声，`null` 作 `except`。
            world.play_block_sound(
                pos,
                sound_event("minecraft:block.respawn_anchor.charge"),
                1.0,
                1.0,
            );
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
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(RESPAWN_ANCHOR_CHARGES) == 0 {
            return InteractionResult::Pass;
        }
        // 主世界用充能的重生锚会爆炸（Bamboo 只有主世界，`canSetSpawn` 恒假）。
        // `Vec3.atCenterOf(pos)`，强度 5、带火，伤害类型 `bad_respawn_point`。
        let pos = ctx.clicked_pos();
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        world.explode_respawn_anchor(center, pos);
        InteractionResult::SuccessServer
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

    /// 主世界用充能的重生锚空手右键会爆炸：重生锚被移除，旁边的石头被炸掉。
    #[test]
    fn a_charged_anchor_explodes() {
        let pos = BlockPos::new(0, 64, 0);
        let charged = BlockState(blocks::RESPAWN_ANCHOR).with(RESPAWN_ANCHOR_CHARGES, 1);
        let mut level = TestLevel::new();
        level.set(pos, charged);
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            level.set(pos.offset(dx, 0, dz), BlockState(blocks::STONE));
        }
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        // 重生锚被移除。
        assert!(level.get(pos).is_air());
        // 至少一块相邻的石头被炸掉。
        let broken = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .any(|(dx, dz)| level.get(pos.offset(dx, 0, dz)).is_air());
        assert!(broken, "the blast should break the surrounding stone");
    }

    /// 没充能的重生锚空手右键什么也不发生。
    #[test]
    fn an_uncharged_anchor_does_not_explode() {
        let pos = BlockPos::new(0, 64, 0);
        let uncharged = BlockState(blocks::RESPAWN_ANCHOR);
        let mut level = TestLevel::new();
        level.set(pos, uncharged);
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert_eq!(level.get(pos), uncharged);
    }
}
