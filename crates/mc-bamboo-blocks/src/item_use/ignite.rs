//! `FlintAndSteelItem` 和 `FireChargeItem`：点燃营火、蜡烛和蜡烛蛋糕，点不着时在点中的面前生火。
//!
//! 两者只差写方块的标记，打火石多一个 `UPDATE_IMMEDIATE`。火点亮传送门框架在火的 `onPlace` 里。
//! 点 TNT 在 `TntBlock.useItemOn` 里，轮不到这里。

use mc_bamboo_data::tags::block_tags;

use crate::{
    behaviour::misc::fire,
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, LIT, WATERLOGGED},
    world::{UPDATE_ALL, UPDATE_ALL_IMMEDIATE, World},
};

/// `FlintAndSteelItem.useOn`。音效的音调取一个随机数。
pub(super) fn flint_and_steel(world: &mut World, ctx: &UseContext) -> InteractionResult {
    ignite(world, ctx, UPDATE_ALL_IMMEDIATE, 1)
}

/// `FireChargeItem.useOn`：`setBlockAndUpdate`。音效的音调取两个随机数。
pub(super) fn fire_charge(world: &mut World, ctx: &UseContext) -> InteractionResult {
    ignite(world, ctx, UPDATE_ALL, 2)
}

/// 点着点中的方块，或者在点中的面前生火。火也放不下时是 `FAIL`。
///
/// 音效不做，但官方在写方块之前为音调取了 `sound_draws` 个随机数，火放下时又要取数排计划刻，
/// 所以这几个数照取。
fn ignite(world: &mut World, ctx: &UseContext, flags: u32, sound_draws: usize) -> InteractionResult {
    let pos = ctx.clicked_pos();
    let state = world.block_state(pos);
    if can_light(state) {
        draw_sound_pitch(world, sound_draws);
        world.set_block(pos, state.with(LIT, true), flags);
        return InteractionResult::Success;
    }
    let fire_pos = pos.relative(ctx.clicked_face());
    if !fire::can_be_placed_at(world, fire_pos, ctx.horizontal_direction()) {
        return InteractionResult::Fail;
    }
    draw_sound_pitch(world, sound_draws);
    let fire_state = fire::state_at(world, fire_pos);
    world.set_block(fire_pos, fire_state, flags);
    InteractionResult::Success
}

/// 音效音调的 `nextFloat`。
fn draw_sound_pitch(world: &World, draws: usize) {
    for _ in 0..draws {
        world.random().next_float();
    }
}

/// `CampfireBlock.canLight`、`CandleBlock.canLight`、`CandleCakeBlock.canLight`：
/// 还没点着，营火和蜡烛还不能泡在水里。
fn can_light(state: BlockState) -> bool {
    let unlit = state.try_get(LIT) == Some(false);
    let dry = state.try_get(WATERLOGGED) == Some(false);
    let needs_dry = state.is_tagged(block_tags::CAMPFIRES) || state.is_tagged(block_tags::CANDLES);
    (needs_dry && unlit && dry) || (state.is_tagged(block_tags::CANDLE_CAKES) && unlit)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockHit,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn flint_and_steel_on(level: &mut TestLevel, hit: BlockHit) -> InteractionResult {
        use_item(level, &actor_holding(items::FLINT_AND_STEEL), hit).result
    }

    #[test]
    fn fire_goes_in_front_of_the_clicked_face() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        assert_eq!(
            flint_and_steel_on(&mut level, top_hit(POS)),
            InteractionResult::Success
        );
        assert_eq!(level.get(POS.above()), BlockState(blocks::FIRE));
    }

    #[test]
    fn fire_on_soul_sand_is_soul_fire() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::SOUL_SAND));
        flint_and_steel_on(&mut level, top_hit(POS));
        assert_eq!(level.get(POS.above()), BlockState(blocks::SOUL_FIRE));
    }

    #[test]
    fn fire_that_cannot_stay_fails() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::STONE));
        let side = BlockHit {
            face: Direction::North,
            ..top_hit(POS)
        };
        assert_eq!(
            flint_and_steel_on(&mut level, side),
            InteractionResult::Fail
        );
        assert!(level.get(POS.north()).is_air());
    }

    #[test]
    fn campfires_light_unless_they_are_under_water() {
        let mut level = TestLevel::new();
        let campfire = BlockState(blocks::CAMPFIRE).with(LIT, false);
        level.set(POS, campfire);
        flint_and_steel_on(&mut level, top_hit(POS));
        assert_eq!(level.get(POS), campfire.with(LIT, true));

        let drowned = campfire.with(WATERLOGGED, true);
        level.set(POS, drowned);
        flint_and_steel_on(&mut level, top_hit(POS));
        assert_eq!(level.get(POS), drowned);
    }

    #[test]
    fn a_fire_charge_lights_a_candle_cake() {
        let mut level = TestLevel::new();
        level.set(POS.below(), BlockState(blocks::STONE));
        level.set(POS, BlockState(blocks::CANDLE_CAKE));
        let outcome = use_item(&mut level, &actor_holding(items::FIRE_CHARGE), top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            level.get(POS),
            BlockState(blocks::CANDLE_CAKE).with(LIT, true)
        );
    }

    #[test]
    fn fire_inside_an_obsidian_frame_opens_a_portal() {
        let mut level = TestLevel::new();
        let obsidian = BlockState(blocks::OBSIDIAN);
        for x in 1..=2 {
            level.set(BlockPos::new(x, 64, 0), obsidian);
            level.set(BlockPos::new(x, 68, 0), obsidian);
        }
        for y in 65..=67 {
            level.set(BlockPos::new(0, y, 0), obsidian);
            level.set(BlockPos::new(3, y, 0), obsidian);
        }
        flint_and_steel_on(&mut level, top_hit(BlockPos::new(1, 64, 0)));
        assert!(level.get(BlockPos::new(2, 67, 0)).is(blocks::NETHER_PORTAL));
    }
}
