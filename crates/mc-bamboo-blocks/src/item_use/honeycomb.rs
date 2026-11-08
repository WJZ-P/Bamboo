//! `HoneycombItem.useOn`：给铜涂蜡。
//!
//! 告示牌上蜡在 `SignBlock.useItemOn` 里，那里只改方块实体，方块状态不变。

use mc_bamboo_data::{BlockAncestor, block_maps::WAXABLES, sounds::sound_event};

use crate::{
    behaviour::{common::block_by_id, machines::chest::connected_direction},
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, CHEST_TYPE, ChestType},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `useOn`。双箱子的另一半由形状更新跟上。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    let old = world.block_state(pos);
    let Some(waxed) = waxed(old) else {
        return InteractionResult::Pass;
    };
    world.set_block(pos, waxed, UPDATE_ALL_IMMEDIATE);
    // `useOn`：先发 3003 的光照，再出声，两样都跳过动手的玩家。
    world.level_event_except(3003, pos, 0);
    world.play_block_sound_except(
        pos,
        sound_event("minecraft:item.honeycomb.wax_on"),
        1.0,
        1.0,
    );
    // 双箱子的另一半也要发一遍，声音的坐标仍然是点中的这一半，和官方一致。
    if old.is_a(BlockAncestor::ChestBlock) && old.get(CHEST_TYPE) != ChestType::Single {
        let neighbour = pos.relative(connected_direction(old));
        world.level_event_except(3003, neighbour, 0);
        world.play_block_sound_except(
            pos,
            sound_event("minecraft:item.honeycomb.wax_on"),
            1.0,
            1.0,
        );
    }
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
        level::Effect,
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

    /// 双箱子的另一半也发一遍光照和声音，声音的坐标还是点中的这一半。
    #[test]
    fn waxing_half_of_a_double_chest_reaches_both_halves() {
        let mut level = TestLevel::new();
        let half = |chest_type| {
            BlockState(blocks::COPPER_CHEST)
                .with(HORIZONTAL_FACING, Direction::North)
                .with(CHEST_TYPE, chest_type)
        };
        level.set(POS, half(ChestType::Left));
        level.set(POS.east(), half(ChestType::Right));
        let mut actor = actor_holding(items::HONEYCOMB);
        actor.secondary_use = true;
        let outcome = use_item(&mut level, &actor, top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(POS).is(blocks::WAXED_COPPER_CHEST));
        assert!(level.get(POS.east()).is(blocks::WAXED_COPPER_CHEST));

        let events: Vec<(i32, BlockPos)> = outcome
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::LevelEvent { event, pos, .. } => Some((*event, *pos)),
                _ => None,
            })
            .collect();
        assert_eq!(events, [(3003, POS), (3003, POS.east())]);
        let sound = sound_event("minecraft:item.honeycomb.wax_on");
        let sounds: Vec<(i32, f64, f64)> = outcome
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Sound { sound, x, z, .. } => Some((*sound, *x, *z)),
                _ => None,
            })
            .collect();
        assert_eq!(
            sounds,
            [(i32::from(sound), 0.5, 0.5), (i32::from(sound), 0.5, 0.5)]
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
