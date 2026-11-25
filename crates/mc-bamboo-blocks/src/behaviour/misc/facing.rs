//! 放下时面朝玩家的方块：`EndPortalFrameBlock`、`VaultBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{
        Behaviour,
        common::facing_the_player,
        vault::{VaultBlockEntity, VaultWorld},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, EYE, OMINOUS, VAULT_STATE, VaultState},
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

/// `VaultBlock`。空手或非 `ACTIVE` 交给空手交互，其余情况把钥匙交给
/// [`VaultBlockEntity::vault_mut`] 的 `try_insert_key`，返回 `SUCCESS_SERVER`。
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
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.hand_is_empty() || state.try_get(VAULT_STATE) != Some(VaultState::Active) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        // `VaultBlock.useItemOn` 只在服务端跑 `Server.tryInsertKey`；方块层拿不到
        // `ServerLevel` 的区分，但 `World` 只在服务端跑方块行为，这里直接驱动。
        let pos = ctx.clicked_pos();
        let ominous = state.try_get(OMINOUS).unwrap_or(false);
        let key = ctx
            .stack
            .cloned()
            .unwrap_or_else(|| ItemStack::new(ctx.item, 1));
        world.with_block_entity::<VaultBlockEntity, _>(pos, |vault, world| {
            let mut vctx = VaultWorld::new(world, pos);
            vault
                .vault_mut()
                .try_insert_key(pos, ominous, ctx.actor.uuid, &key, &mut vctx);
        });
        InteractionResult::SuccessServer
    }
}
