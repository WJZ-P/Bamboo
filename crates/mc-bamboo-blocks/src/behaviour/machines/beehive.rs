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
            // 采蜜的声音在写方块之前，坐标是玩家所在的位置。剪刀用 `null` 作 `except`，
            // 玻璃瓶跳过动手的玩家。剪刀在掉蜜脾之后、清零之前 `hurtAndBreak(1)`。
            // 掉落表的随机数还没做。玻璃瓶官方会 `shrink(1)` 并给出蜂蜜瓶，这里还不消耗瓶子。
            //
            // The harvest sound plays before the block write, at the player's position.
            // Shears pass `null` as `except`; a bottle skips the player who used it.
            // Shears call `hurtAndBreak(1)` after the honeycomb drop and before the reset.
            // The loot random draws are not implemented. A glass bottle `shrink(1)`s and yields a
            // honey bottle in vanilla; the bottle is not consumed here yet.
            if ctx.item == items::SHEARS {
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
