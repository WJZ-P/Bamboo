//! `CommandBlock`。
//!
//! 放置和破坏看 `canUseGameMasterBlocks`。右键有权限时打开编辑界面。
//! 通电和执行命令还没做。
//!
//! Placement and breaking follow `canUseGameMasterBlocks`. A permitted right-click opens
//! the editor. Power and command execution are not implemented yet.

use mc_bamboo_data::Block;

use super::facing_the_view;
use crate::{
    behaviour::Behaviour,
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    level::Effect,
    state::BlockState,
    world::World,
};

/// `CommandBlock`。
pub(super) struct Command;

impl Behaviour for Command {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx))
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        // `CommandBlock.useWithoutItem`：有权限才 `openCommandBlock` 并返回 `SUCCESS`。
        //
        // `CommandBlock.useWithoutItem` calls `openCommandBlock` and returns `SUCCESS` only
        // when the player may use game-master blocks.
        if ctx.actor.can_use_game_master_blocks {
            world.push_effect(Effect::OpenCommandBlock {
                pos: ctx.clicked_pos(),
            });
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }
}
