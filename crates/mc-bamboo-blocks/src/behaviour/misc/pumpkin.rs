//! `PumpkinBlock`：剪刀把南瓜刻成雕刻南瓜。种子是掉落物，不在范围里。

use mc_bamboo_core::Axis;
use mc_bamboo_data::{blocks, items};

use crate::{
    behaviour::{Behaviour, Plain},
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `PumpkinBlock`。
pub(super) struct Pumpkin;

impl Behaviour for Pumpkin {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item != items::SHEARS {
            return Plain.use_item_on(state, world, ctx);
        }
        let face = ctx.clicked_face();
        let facing = if face.axis() == Axis::Y {
            ctx.horizontal_direction().opposite()
        } else {
            face
        };
        let carved = BlockState(blocks::CARVED_PUMPKIN).with(HORIZONTAL_FACING, facing);
        world.set_block(ctx.clicked_pos(), carved, UPDATE_ALL_IMMEDIATE);
        InteractionResult::Success
    }
}
