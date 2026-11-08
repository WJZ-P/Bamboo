//! 放下时面朝玩家的方块：`EndPortalFrameBlock`、`VaultBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, EYE, VAULT_STATE, VaultState},
    world::World,
};

/// `EndPortalFrameBlock`。末影之眼是有自己 `useOn` 的物品，不在这里。
pub(super) struct EndPortalFrame;

impl Behaviour for EndPortalFrame {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx).with(EYE, false))
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(EYE) { 15 } else { 0 }
    }
}

/// `VaultBlock`。钥匙要对照方块实体里的配置和奖励，不在范围里，所以只吃掉点击。
pub(super) struct Vault;

impl Behaviour for Vault {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn use_item_on(
        &self,
        state: BlockState,
        _world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !ctx.hand_is_empty() && state.get(VAULT_STATE) == VaultState::Active {
            InteractionResult::SuccessServer
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }
}
