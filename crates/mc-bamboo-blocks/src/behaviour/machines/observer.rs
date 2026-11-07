//! `ObserverBlock`。
//!
//! 侦测器的脉冲是红石信号：`updateShape` 排的刻和 `tick` 不做，世界里不会有 `powered` 的侦测器。

use mc_bamboo_data::Block;

use crate::{
    behaviour::{Behaviour, common::default_state},
    context::PlaceContext,
    state::{BlockState, FACING},
    world::World,
};

/// `ObserverBlock`。
pub(super) struct Observer;

impl Behaviour for Observer {
    /// 脸朝着玩家看的方向，也就是玩家看到的是它的背面。
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block).with(FACING, ctx.nearest_looking_direction()))
    }
}
