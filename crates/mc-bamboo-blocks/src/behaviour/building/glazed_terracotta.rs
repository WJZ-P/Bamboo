//! `GlazedTerracottaBlock`。

use mc_bamboo_data::Block;

use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::PlaceContext,
    state::BlockState,
    world::World,
};

/// `GlazedTerracottaBlock`：朝着玩家。
pub(super) struct GlazedTerracotta;

impl Behaviour for GlazedTerracotta {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }
}
