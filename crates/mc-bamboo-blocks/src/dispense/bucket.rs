//! 满桶和空桶。
//!
//! 满桶倒得出去时换成空桶。鱼桶只倒水，生物不生成，但 `EntityType.create` 和
//! `finalizeSpawn` 要从 `level.random` 取的数照取。硫磺立方桶的流体是空的，只放倒空的
//! 声音，不放方块，生成用的随机数同样照取。倒不出去时套一层默认抛出。
//! 空桶舀起来就换成舀到的物品种类（不要那一堆上的组件）；舀不到时只抛一次，不套一层。
//!
//! Filled and empty buckets.
//!
//! A filled bucket that can pour becomes an empty bucket. A fish bucket pours water only.
//! The mob in `checkExtraContent` is not spawned, but the `level.random` draws inside
//! `EntityType.create` and `finalizeSpawn` still happen. A sulfur-cube bucket has an empty
//! fluid: it plays the empty sound, places no block, and takes the same spawn draws.
//! A pour that fails nests a default throw.
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
        // `checkExtraContent` 在倒成功之后。生物不生成，随机数在 `consumeWithRemainder` 之前取。
        //
        // `checkExtraContent` runs after a successful pour. The mob is not spawned. The random
        // draws happen before `consumeWithRemainder`.
        if is_mob(stack.item) {
            bucket_mob_spawn_random(world, stack.item);
        }
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
    Shot::plain(throw_one(world, facing, stack))
}

/// `DispensibleContainerItem.emptyContents`。细雪桶是 `SolidBucketItem`，不走 `BucketItem`。
fn pour(world: &mut World, item: i32, target: BlockPos) -> bool {
    if item == items::POWDER_SNOW_BUCKET {
        return pour_powder_snow(world, target);
    }
    empty_from_dispenser(world, item, target)
}

fn is_mob(item: i32) -> bool {
    matches!(
        item,
        items::SALMON_BUCKET
            | items::COD_BUCKET
            | items::PUFFERFISH_BUCKET
            | items::TROPICAL_FISH_BUCKET
            | items::AXOLOTL_BUCKET
            | items::SULFUR_CUBE_BUCKET
            | items::TADPOLE_BUCKET
    )
}

/// `MobBucketItem.spawn` 里 `EntityType.create` 对 `level.random` 的取数。
///
/// 先是 `snapTo` 的一次 `nextFloat`。美西螈在 `EntitySpawnReason.BUCKET` 时
/// `finalizeSpawn` 直接返回。别的都进 `Mob.finalizeSpawn`：一次 `triangle` 再一次
/// `nextFloat`。热带鱼接着按 `nextFloat() < 0.9` 选常见变种或随机花纹。
/// 硫磺立方覆盖了 `setSpawnSize`，不再取体型的随机数。鲑鱼的体型用实体自己的随机源。
///
/// The `level.random` draws in `EntityType.create` from `MobBucketItem.spawn`.
///
/// `snapTo` draws one `nextFloat` first. An axolotl's `finalizeSpawn` returns immediately
/// for `EntitySpawnReason.BUCKET`. The others reach `Mob.finalizeSpawn`: one `triangle`,
/// then one `nextFloat`. A tropical fish then picks a common variant or a random pattern
/// from `nextFloat() < 0.9`. The sulfur cube overrides `setSpawnSize` and does not roll a
/// size. A salmon's size uses the entity's own random.
fn bucket_mob_spawn_random(world: &mut World, item: i32) {
    let _yaw = world.random().next_float();
    if item == items::AXOLOTL_BUCKET {
        return;
    }
    // `Mob.finalizeSpawn` 里的 `0.11485000000000001` 是官方写死的 spread。
    //
    // The spread `0.11485000000000001` is the literal in `Mob.finalizeSpawn`.
    let _follow = world.random().triangle(0.0, 0.114_850_000_000_000_01);
    let _hand = world.random().next_float();
    if item == items::TROPICAL_FISH_BUCKET {
        tropical_fish_variant(world);
    }
}

/// `TropicalFish.finalizeSpawn` 在 `super` 之后的品种分支。
///
/// `COMMON_VARIANTS` 有 22 个，`Pattern.values()` 有 12 个，`DyeColor.values()` 有 16 个。
/// 这三个长度是 26.3 的枚举，生成器没有导出。
///
/// The variant branch after `super` in `TropicalFish.finalizeSpawn`.
///
/// `COMMON_VARIANTS` has 22 entries, `Pattern.values()` has 12, and `DyeColor.values()` has
/// 16. Those lengths are the 26.3 enums. The generator does not export them.
fn tropical_fish_variant(world: &mut World) {
    const COMMON_VARIANTS: i32 = 22;
    const PATTERNS: i32 = 12;
    const DYE_COLORS: i32 = 16;
    // `nextFloat() < 0.9`：`0.9` 是 double，float 先提升再比。
    //
    // `nextFloat() < 0.9`: `0.9` is a double, so the float is widened before the compare.
    if f64::from(world.random().next_float()) < 0.9 {
        let _variant = world.random().next_int(COMMON_VARIANTS);
    } else {
        let _pattern = world.random().next_int(PATTERNS);
        let _base = world.random().next_int(DYE_COLORS);
        let _spots = world.random().next_int(DYE_COLORS);
    }
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
