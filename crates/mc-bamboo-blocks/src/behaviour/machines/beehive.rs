//! `BeehiveBlock`。
//!
//! 蜜蜂在方块实体里，这里的蜂巢总是空的：`updateShape` 和 `playerWillDestroy` 里放蜜蜂、
//! 掉落的部分没有东西要做。采蜜后不管有没有烟，蜜都清零，有烟只是不放蜜蜂。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, items,
    sounds::{self, sound_event},
};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{Behaviour, Plain, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    item_exchange::ItemExchange,
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
            // 剪刀先 `dropHoneycomb`。战利品表是常量 3 个蜜脾，走自己的 `random_sequence`，
            // 不碰 `level.random`；`Block.popResource` 为该堆取三次位置偏移，然后生成物品实体。
            // 接着放剪刀声、扣耐久，再 `releaseBeesAndResetHoneyLevel`（这里只把蜜清零）。
            // 玻璃瓶不掉蜜脾，先把瓶子的消耗和蜜瓶交给游戏层，再放装瓶声，同样清零。
            //
            // Shears call `dropHoneycomb` first. The constant three honeycomb use their own
            // `random_sequence`, not `level.random`; `Block.popResource` draws three position
            // offsets and spawns the item. Then the shear sound, `hurtAndBreak(1)`, and
            // `releaseBeesAndResetHoneyLevel` (which here only clears the honey). A glass bottle
            // drops no honeycomb: record the bottle exchange, then play the fill sound and clear.
            if ctx.item == items::SHEARS {
                world.pop_resource(pos, ItemStack::new(items::HONEYCOMB, 3));
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
                world.set_item_exchange(ItemExchange::BottleHoney);
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
