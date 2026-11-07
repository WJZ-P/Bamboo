//! `CommandBlock`。
//!
//! 这里没有权限系统：物品放不下，方块破坏不了，右键是 `PASS`。
//! 通电和执行命令都在方块实体里，也走不到。

use mc_bamboo_data::Block;

use super::facing_the_view;
use crate::{behaviour::Behaviour, context::PlaceContext, state::BlockState, world::World};

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
}
