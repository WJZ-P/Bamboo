//! `BeehiveBlock`。
//!
//! 蜜蜂在方块实体里，这里的蜂巢总是空的：`updateShape` 和 `playerWillDestroy` 里放蜜蜂、
//! 掉落的部分没有东西要做。采蜜后不管有没有烟，蜜都清零，有烟只是不放蜜蜂。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, items,
    sounds::{self, sound_event},
};

use crate::{
    behaviour::{Behaviour, Plain, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, LEVEL_HONEY},
    world::World,
};

/// `BeehiveBlock.MAX_HONEY_LEVELS`。
const MAX_HONEY_LEVELS: i32 = 5;

/// `BeehiveBlock`。
pub(super) struct Beehive;

impl Behaviour for Beehive {
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
        let harvests = ctx.item == items::SHEARS || ctx.item == items::GLASS_BOTTLE;
        if state.get(LEVEL_HONEY) >= MAX_HONEY_LEVELS && harvests {
            let pos = ctx.clicked_pos();
            // 剪刀先 `dropHoneycomb`。战利品表走自己的 `random_sequence`，不碰 `level.random`。
            // `Block.popResource` 在物品实体之前取三次 `nextDouble`。物品实体不生成，这三次照取。
            // 然后是剪刀声、`hurtAndBreak(1)`，再 `releaseBeesAndResetHoneyLevel`。
            // 蜂巢是空的：没有烟时也不放蜜蜂，`emptyAllLivingFromHive` 不取随机数，只把蜜清零。
            // 玻璃瓶不掉蜜脾，所以不取那三次。`releaseBeesAndResetHoneyLevel` 同样只清零。
            // 瓶子的 `shrink(1)` 和蜂蜜瓶还没做。
            //
            // Shears call `dropHoneycomb` first. The loot table uses its own `random_sequence`
            // and does not touch `level.random`. `Block.popResource` draws three `nextDouble`s
            // before the item entity. The entity is not spawned; the three draws still happen.
            // Then the shear sound, `hurtAndBreak(1)`, then `releaseBeesAndResetHoneyLevel`.
            // The hive is empty: even without smoke no bees are released, and
            // `emptyAllLivingFromHive` draws nothing. Only the honey is cleared.
            // A glass bottle does not drop honeycomb, so it does not draw those three.
            // `releaseBeesAndResetHoneyLevel` only clears the honey. The bottle's `shrink(1)`
            // and the honey bottle are not done yet.
            if ctx.item == items::SHEARS {
                let _ = world.random().next_double();
                let _ = world.random().next_double();
                let _ = world.random().next_double();
                world.play_sound(
                    ctx.actor.position,
                    sound_event("minecraft:block.beehive.shear"),
                    sounds::BLOCKS,
                    1.0,
                    1.0,
                    false,
                );
                crate::durability::hurt_hand(world, ctx.hand, 1);
            } else {
                world.play_sound(
                    ctx.actor.position,
                    sound_event("minecraft:item.bottle.fill"),
                    sounds::BLOCKS,
                    1.0,
                    1.0,
                    true,
                );
            }
            world.set_block_and_update(pos, state.with(LEVEL_HONEY, 0));
            return InteractionResult::Success;
        }
        Plain.use_item_on(state, world, ctx)
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        state.get(LEVEL_HONEY)
    }
}
