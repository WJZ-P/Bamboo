//! 满桶和空桶。
//!
//! 满桶倒得出去时换成空桶。鱼桶只倒水，`checkExtraContent` 里的生物不生成。
//! 硫磺立方桶的流体是空的，只放倒空的声音，不放方块。倒不出去时套一层默认抛出。
//! 空桶舀起来就换成舀到的物品种类（不要那一堆上的组件）；舀不到时只抛一次，不套一层。
//!
//! Filled and empty buckets.
//!
//! A filled bucket that can pour becomes an empty bucket. A fish bucket pours water only.
//! The mob in `checkExtraContent` is not spawned. A sulfur-cube bucket has an empty fluid:
//! it plays the empty sound and places no block. A pour that fails nests a default throw.
//! An empty bucket that picks something up becomes a fresh stack of that item type, without
//! the picked stack's components. A miss throws once and does not nest.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use super::{Shot, consume_with_remainder, nested_default, throw_one};
use crate::{
    behaviour::common::item_of, fluid::bucket::empty_from_dispenser, state::BlockState,
    world::World,
};

pub(super) fn is_filled(item: i32) -> bool {
    matches!(
        item,
        items::LAVA_BUCKET
            | items::WATER_BUCKET
            | items::POWDER_SNOW_BUCKET
            | items::SALMON_BUCKET
            | items::COD_BUCKET
            | items::PUFFERFISH_BUCKET
            | items::TROPICAL_FISH_BUCKET
            | items::AXOLOTL_BUCKET
            | items::SULFUR_CUBE_BUCKET
            | items::TADPOLE_BUCKET
    )
}

pub(super) fn is_empty_bucket(item: i32) -> bool {
    item == items::BUCKET
}

pub(super) fn filled(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let target = pos.relative(facing);
    if pour(world, stack.item, target) {
        let left =
            consume_with_remainder(world, pos, facing, stack, ItemStack::new(items::BUCKET, 1));
        Shot::plain(left)
    } else {
        Shot::plain(nested_default(world, pos, facing, stack))
    }
}

pub(super) fn empty(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let state = world.block_state(target);
    if state.is_a(BlockAncestor::BucketPickup) {
        // `pickupBlock(null, ...)`。发射器不放 `getPickupSound`。
        //
        // `pickupBlock(null, ...)`. A dispenser does not play `getPickupSound`.
        let picked = state.pickup_block(world, target, false);
        if picked != items::AIR {
            let left = consume_with_remainder(world, pos, facing, stack, ItemStack::new(picked, 1));
            return Shot::plain(left);
        }
    }
    Shot::plain(throw_one(world, stack))
}

/// `DispensibleContainerItem.emptyContents`。细雪桶是 `SolidBucketItem`，不走 `BucketItem`。
fn pour(world: &mut World, item: i32, target: BlockPos) -> bool {
    if item == items::POWDER_SNOW_BUCKET {
        return pour_powder_snow(world, target);
    }
    empty_from_dispenser(world, item, target)
}

/// `SolidBucketItem.emptyContents`：在边界内且是空方块才放下，声音不跳过任何人。
///
/// `SolidBucketItem.emptyContents`: place only when the cell is in bounds and empty.
/// The sound skips nobody.
fn pour_powder_snow(world: &mut World, pos: BlockPos) -> bool {
    if !world.is_in_world_bounds(pos) || !world.is_empty_block(pos) {
        return false;
    }
    world.set_block_and_update(pos, BlockState(blocks::POWDER_SNOW));
    if let Some(sound) = item_of(items::POWDER_SNOW_BUCKET).and_then(|item| item.place_sound) {
        let sound = u16::try_from(sound).expect("音效事件的注册表 ID 在 u16 范围内");
        world.play_block_sound(pos, sound, 1.0, 1.0);
    }
    true
}
