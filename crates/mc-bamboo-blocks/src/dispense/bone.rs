//! 发射器里的 `BoneMealItem`。
//!
//! `growCrop` 和 `growWaterPlant` 在官方里自己 `shrink(1)`。这里的催熟函数不减数量，
//! 成功时由发射器减一个，并在目标格发 `levelEvent(1505, 15)`。两个函数看的是同一格，
//! 作物成功就不再长海草。
//!
//! `BoneMealItem` from a dispenser.
//!
//! Official `growCrop` and `growWaterPlant` call `shrink(1)` themselves. The grow functions
//! here do not, so the dispenser shrinks one on success and emits `levelEvent(1505, 15)` at
//! the target. Both functions look at the same cell. A crop success does not also grow seagrass.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use super::Shot;
use crate::{
    dispenser::without_one,
    item_use::{grow_crop, grow_water_plant},
    world::World,
};

pub(super) fn handles(item: i32) -> bool {
    item == items::BONE_MEAL
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let target = pos.relative(facing);
    // `!growCrop && !growWaterPlant`：前一个为真时第二个不调用。
    //
    // `!growCrop && !growWaterPlant`: the second call is skipped when the first returns true.
    let grew = if grow_crop(world, target) {
        true
    } else {
        grow_water_plant(world, target)
    };
    if !grew {
        return Shot::optional(stack, false);
    }
    world.level_event(1505, target, 15);
    Shot::optional(without_one(&stack), true)
}
