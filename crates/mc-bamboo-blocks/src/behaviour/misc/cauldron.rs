//! `CauldronBlock`、`LayeredCauldronBlock`、`LavaCauldronBlock` 和官方的 `CauldronInteractions`。
//!
//! 官方这张表还有水瓶、旗帜和染色物品的条目，它们都要看物品组件。这里的物品没有组件，
//! 官方默认组件的物品走到这些条目时都是 `TRY_WITH_EMPTY_HAND`，和没有条目一样。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, blocks, items, sounds::sound_event};

use super::speleothem::{cauldron_fill_fluid, stalactite_tip_above};
use crate::{
    behaviour::Behaviour,
    context::UseContext,
    fluid::Fluid,
    interact::InteractionResult,
    state::{BlockState, LEVEL_CAULDRON, block_of_item},
    world::World,
};

/// 三个类的 `useItemOn` 和 `tick` 都在父类 `AbstractCauldronBlock` 里，只有查表的内容不同。
pub(super) struct Cauldron;

/// `MAX_FILL_LEVEL`。
const MAX_FILL_LEVEL: i32 = 3;

/// `LayeredCauldronBlock.lowerFillLevel`。
fn lower_fill_level(state: BlockState, world: &mut World, pos: BlockPos) {
    let level = state.get(LEVEL_CAULDRON) - 1;
    let lowered = if level == 0 {
        BlockState(blocks::CAULDRON)
    } else {
        state.with(LEVEL_CAULDRON, level)
    };
    world.set_block_and_update(pos, lowered);
}

/// 炼药锅交互里的声音都是 `BLOCKS` 来源，音量音高都是 1，`except` 是空。
fn play_sound(world: &mut World, pos: BlockPos, sound: &str) {
    world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
}

/// `emptyBucket` 的结果：换成 `filled`，然后放倒水的声音。
fn fill_with(
    filled: BlockState,
    sound: &str,
    world: &mut World,
    pos: BlockPos,
) -> InteractionResult {
    world.set_block_and_update(pos, filled);
    play_sound(world, pos, sound);
    InteractionResult::Success
}

/// `fillLavaInteraction`、`fillPowderSnowInteraction`：水下倒不进去，但是吃掉这次点击。
fn fill_with_unless_under_water(
    filled: BlockState,
    sound: &str,
    world: &mut World,
    pos: BlockPos,
) -> InteractionResult {
    if world.is_water_at(pos.above()) {
        InteractionResult::Consume
    } else {
        fill_with(filled, sound, world, pos)
    }
}

/// `fillBucket`：水和细雪要满了才能舀，岩浆总是满的。
fn fill_bucket(state: BlockState, world: &mut World, pos: BlockPos) -> InteractionResult {
    let is_layered = state.is(blocks::WATER_CAULDRON) || state.is(blocks::POWDER_SNOW_CAULDRON);
    if !state.is(blocks::LAVA_CAULDRON)
        && !(is_layered && state.get(LEVEL_CAULDRON) == MAX_FILL_LEVEL)
    {
        return InteractionResult::TryEmptyHandInteraction;
    }
    let sound = if state.is(blocks::LAVA_CAULDRON) {
        "minecraft:item.bucket.fill_lava"
    } else if state.is(blocks::POWDER_SNOW_CAULDRON) {
        "minecraft:item.bucket.fill_powder_snow"
    } else {
        "minecraft:item.bucket.fill"
    };
    fill_with(BlockState(blocks::CAULDRON), sound, world, pos)
}

/// `Items.DYED_SHULKER_BOX`：染了色的潜影盒，普通的潜影盒不在里面。
fn is_dyed_shulker_box(item: i32) -> bool {
    item != items::SHULKER_BOX && block_of_item(item).is_a(BlockAncestor::ShulkerBoxBlock)
}

/// `WATER` 表多出来的条目：玻璃瓶和洗潜影盒都用掉一层水。
fn use_water(state: BlockState, world: &mut World, item: i32, pos: BlockPos) -> InteractionResult {
    if state.is(blocks::WATER_CAULDRON) {
        if item == items::GLASS_BOTTLE {
            lower_fill_level(state, world, pos);
            // `WATER` 表里玻璃瓶在用掉一层水之后出声，洗潜影盒不出声。
            play_sound(world, pos, "minecraft:item.bottle.fill");
            return InteractionResult::Success;
        }
        if is_dyed_shulker_box(item) {
            lower_fill_level(state, world, pos);
            return InteractionResult::Success;
        }
    }
    InteractionResult::TryEmptyHandInteraction
}

/// `canReceiveStalactiteDrip`。
pub(super) fn can_receive_drip(state: BlockState, fluid: Fluid) -> bool {
    state.is(blocks::CAULDRON) || state.is(blocks::WATER_CAULDRON) && fluid == Fluid::Water
}

/// `receiveStalactiteDrip`。
fn receive_drip(state: BlockState, world: &mut World, pos: BlockPos, fluid: Fluid) {
    let filled = if !state.is(blocks::CAULDRON) {
        let level = state.get(LEVEL_CAULDRON);
        if level == MAX_FILL_LEVEL {
            return;
        }
        state.with(LEVEL_CAULDRON, level + 1)
    } else if fluid == Fluid::Water {
        BlockState(blocks::WATER_CAULDRON)
    } else {
        BlockState(blocks::LAVA_CAULDRON)
    };
    world.set_block_and_update(pos, filled);
    // 水滴 1047，岩浆滴 1046；已经有水的炼药锅只接水滴。
    if fluid == Fluid::Water {
        world.level_event(1047, pos, 0);
    } else {
        world.level_event(1046, pos, 0);
    }
}

impl Behaviour for Cauldron {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let full_of = |cauldron| BlockState(cauldron).with(LEVEL_CAULDRON, MAX_FILL_LEVEL);
        match ctx.item {
            items::WATER_BUCKET => fill_with(
                full_of(blocks::WATER_CAULDRON),
                "minecraft:item.bucket.empty",
                world,
                pos,
            ),
            items::LAVA_BUCKET => fill_with_unless_under_water(
                BlockState(blocks::LAVA_CAULDRON),
                "minecraft:item.bucket.empty_lava",
                world,
                pos,
            ),
            items::POWDER_SNOW_BUCKET => fill_with_unless_under_water(
                full_of(blocks::POWDER_SNOW_CAULDRON),
                "minecraft:item.bucket.empty_powder_snow",
                world,
                pos,
            ),
            items::BUCKET => fill_bucket(state, world, pos),
            item => use_water(state, world, item, pos),
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let fluid =
            stalactite_tip_above(world, pos).and_then(|tip| cauldron_fill_fluid(world, tip));
        if let Some(fluid) = fluid
            && can_receive_drip(state, fluid)
        {
            receive_drip(state, world, pos, fluid);
        }
    }

    /// 岩浆锅总是满的，官方直接写 3。空锅没有覆盖，读出来是 0。
    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.is_a(BlockAncestor::LayeredCauldronBlock) {
            state.get(LEVEL_CAULDRON)
        } else if state.is_a(BlockAncestor::LavaCauldronBlock) {
            MAX_FILL_LEVEL
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        behaviour::misc::test_support::tick,
        state::{SPELEOTHEM_THICKNESS, SpeleothemThickness, VERTICAL_DIRECTION},
        testing::TestLevel,
    };

    const CAULDRON: BlockPos = BlockPos::new(0, 64, 0);
    const TIP: BlockPos = BlockPos::new(0, 67, 0);

    /// 炼药锅上面隔两格挂着钟乳石，钟乳石上面是 `source`，中间隔着一格石头。
    fn drip_level(cauldron: BlockState, source: BlockState) -> TestLevel {
        let mut level = TestLevel::new();
        level.set(CAULDRON, cauldron);
        let stalactite = BlockState(blocks::POINTED_DRIPSTONE)
            .with(VERTICAL_DIRECTION, Direction::Down)
            .with(SPELEOTHEM_THICKNESS, SpeleothemThickness::Tip);
        level.set(TIP, stalactite);
        level.set(TIP.above(), BlockState(blocks::STONE));
        level.set(TIP.above().above(), source);
        level
    }

    fn dripped(cauldron: u32, source: u32) -> BlockState {
        let mut level = drip_level(BlockState(cauldron), BlockState(source));
        tick(&mut level, CAULDRON);
        level.get(CAULDRON)
    }

    #[test]
    fn water_above_a_stalactite_starts_a_water_cauldron() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::WATER),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn mud_counts_as_water() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::MUD),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn lava_fills_only_an_empty_cauldron() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::LAVA),
            BlockState(blocks::LAVA_CAULDRON)
        );
        assert_eq!(
            dripped(blocks::WATER_CAULDRON, blocks::LAVA),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn water_raises_a_water_cauldron_up_to_full() {
        let level_of = |state: BlockState| state.get(LEVEL_CAULDRON);
        assert_eq!(level_of(dripped(blocks::WATER_CAULDRON, blocks::WATER)), 2);
        let full = BlockState(blocks::WATER_CAULDRON).with(LEVEL_CAULDRON, MAX_FILL_LEVEL);
        let mut level = drip_level(full, BlockState::WATER);
        tick(&mut level, CAULDRON);
        assert_eq!(level.get(CAULDRON), full);
    }

    #[test]
    fn a_solid_block_stops_the_drip() {
        let mut level = drip_level(BlockState(blocks::CAULDRON), BlockState::WATER);
        level.set(CAULDRON.above(), BlockState(blocks::STONE));
        tick(&mut level, CAULDRON);
        assert_eq!(level.get(CAULDRON), BlockState(blocks::CAULDRON));
    }
}
