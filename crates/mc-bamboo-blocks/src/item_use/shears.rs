//! `ShearsItem.useOn`：剪掉还在长的植物头，让它停止生长。
//!
//! 剪南瓜、给蜂巢取蜜脾这些在方块自己的 `useItemOn` 里。

use mc_bamboo_data::{BlockAncestor, sounds::sound_event};

use crate::{context::UseContext, interact::InteractionResult, state::AGE_25, world::World};

/// `GrowingPlantHeadBlock.MAX_AGE`。
const MAX_AGE: i32 = 25;

/// 点中的不是还在长的 `GrowingPlantHeadBlock` 时返回 `None`，官方接着走 `Item.useOn`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> Option<InteractionResult> {
    let pos = ctx.clicked_pos();
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::GrowingPlantHeadBlock) || state.get(AGE_25) == MAX_AGE {
        return None;
    }
    // `useOn`：剪的声音在写方块之前，跳过动手的玩家。
    world.play_block_sound_except(
        pos,
        sound_event("minecraft:block.growing_plant.crop"),
        1.0,
        1.0,
    );
    world.set_block_and_update(pos, state.with(AGE_25, MAX_AGE));
    Some(InteractionResult::Success)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockState,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    #[test]
    fn shears_stop_a_growing_vine() {
        let mut level = TestLevel::new();
        level.set(POS.below(), BlockState(blocks::NETHERRACK));
        let vine = BlockState(blocks::TWISTING_VINES).with(AGE_25, 3);
        level.set(POS, vine);
        let shears = actor_holding(items::SHEARS);
        let outcome = use_item(&mut level, &shears, top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(POS), vine.with(AGE_25, MAX_AGE));

        let again = use_item(&mut level, &shears, top_hit(POS));
        assert_eq!(again.result, InteractionResult::Pass);
    }
}
