//! `HoneycombItem.useOn`：给铜涂蜡。
//!
//! 告示牌上蜡在 `SignBlock.useItemOn` 里，那里只改方块实体，方块状态不变。

use mc_bamboo_data::block_maps::WAXABLES;

use crate::{
    behaviour::common::block_by_id,
    context::UseContext,
    interact::InteractionResult,
    state::BlockState,
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `useOn`。双箱子的另一半由形状更新跟上。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    let Some(waxed) = waxed(world.block_state(pos)) else {
        return InteractionResult::Pass;
    };
    world.set_block(pos, waxed, UPDATE_ALL_IMMEDIATE);
    InteractionResult::Success
}

/// `getWaxed`：涂蜡后的方块，属性照抄。
fn waxed(state: BlockState) -> Option<BlockState> {
    let block = state.block_id() as i32;
    WAXABLES
        .iter()
        .find(|&&(plain, _)| plain == block)
        .map(|&(_, waxed)| state.copied_onto(block_by_id(waxed)))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        state::{HORIZONTAL_FACING, OPEN},
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    #[test]
    fn honeycomb_waxes_copper_and_keeps_its_properties() {
        let mut level = TestLevel::new();
        let trapdoor = BlockState(blocks::OXIDIZED_COPPER_TRAPDOOR)
            .with(HORIZONTAL_FACING, Direction::East)
            .with(OPEN, true);
        level.set(POS, trapdoor);
        // 铜活板门自己会接住空手的右键，蹲着才轮到蜜脾。
        let mut actor = actor_holding(items::HONEYCOMB);
        actor.secondary_use = true;
        let outcome = use_item(&mut level, &actor, top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            level.get(POS),
            trapdoor.copied_onto(BlockState(blocks::WAXED_OXIDIZED_COPPER_TRAPDOOR).block())
        );
    }

    #[test]
    fn honeycomb_ignores_waxed_and_other_blocks() {
        let mut level = TestLevel::new();
        let actor = actor_holding(items::HONEYCOMB);
        for block in [blocks::WAXED_COPPER_BLOCK, blocks::STONE] {
            level.set(POS, BlockState(block));
            let outcome = use_item(&mut level, &actor, top_hit(POS));
            assert_eq!(outcome.result, InteractionResult::Pass);
            assert_eq!(level.get(POS), BlockState(block));
        }
    }
}
