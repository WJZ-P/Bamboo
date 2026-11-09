//! `DispenserBlock.dispenseFrom` 和 `DispenseItemBehavior.bootStrap`。
//!
//! 生成实体的行为不建实体。船和矿车在放得下的时候只扣物品、放一次 1000 和 2000；
//! 放不下时走官方的失败分支，也就是再套一层 `DefaultDispenseItemBehavior.dispense`。
//! 箭、刷怪蛋、盔甲架走官方在生成失败时的分支。TNT 在 `tnt_explodes` 为真时生成点燃的实体，见各文件。
//!
//! `DispenserBlock.dispenseFrom` and `DispenseItemBehavior.bootStrap`.
//!
//! Behaviours that spawn an entity do not create one, except primed TNT. A boat or minecart that
//! can be placed only consumes the item and plays 1000 then 2000 once. When it cannot, the
//! official failure branch runs, which nests another `DefaultDispenseItemBehavior.dispense`.
//! Arrows, spawn eggs and armour stands follow the branch official uses when the spawn fails.
//! TNT spawns a primed entity when `tnt_explodes` is true.

mod bone;
mod bottle;
mod bucket;
mod entity;
mod equipment;
mod ignite;
mod place;
mod wax;

#[cfg(test)]
mod tests;

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    container::{copy_with_count, same_item_same_components, stack_limit},
    dispenser::{limited, occupied_slots, random_slot, spawn_item_random, without_one},
    hopper::{query, set_changed},
    state::{BlockState, FACING},
    world::World,
};

/// 一次 `execute` 的结果。`success` 为 `None` 时是 `DefaultDispenseItemBehavior`，声音总是 1000。
///
/// The result of one `execute`. `success: None` is `DefaultDispenseItemBehavior`, whose sound
/// is always 1000.
pub(super) struct Shot {
    pub stack: ItemStack,
    pub success: Option<bool>,
}

impl Shot {
    pub(super) fn plain(stack: ItemStack) -> Self {
        Self {
            stack,
            success: None,
        }
    }

    pub(super) fn optional(stack: ItemStack, success: bool) -> Self {
        Self {
            stack,
            success: Some(success),
        }
    }
}

/// `DispenserBlock.dispenseFrom`。
///
/// `DispenserBlock.dispenseFrom`.
pub fn dispense_from(world: &mut World, pos: BlockPos, state: BlockState) {
    // 官方找不到方块实体时只打一条警告，不报 1001。
    // Official only logs a warning when the block entity is missing. No 1001.
    let Some(occupied) = occupied_slots(world, pos) else {
        return;
    };
    let slot = random_slot(world, &occupied);
    if slot < 0 {
        world.level_event(1001, pos, 0);
        return;
    }
    let slot = slot as usize;
    let Some(item) = query(world, pos, &mut |container| container.get(slot)) else {
        return;
    };
    if item.is_empty() {
        return;
    }
    let facing = state.get(FACING);
    let shot = dispatch(world, pos, facing, item);
    let result = finish(world, pos, facing, shot);
    world.with_container(pos, &mut |container| {
        container.set(slot, limited(result.clone()));
    });
    set_changed(world, pos);
}

/// `DefaultDispenseItemBehavior.dispense` 末尾的声音和冒烟。可选行为失败时声音是 1001，冒烟照样有。
///
/// The sound and smoke at the end of `DefaultDispenseItemBehavior.dispense`. An optional
/// behaviour that failed uses 1001. The smoke still plays.
fn finish(world: &mut World, pos: BlockPos, facing: Direction, shot: Shot) -> ItemStack {
    let sound = match shot.success {
        Some(false) => 1001,
        _ => 1000,
    };
    world.level_event(sound, pos, 0);
    world.level_event(2000, pos, facing.id());
    shot.stack
}

/// `DefaultDispenseItemBehavior.execute`：按 `spawnItem` 的速度生成一件物品实体。
///
/// 位置是 `DispenserBlock.getDispensePosition`，也就是发射器方块的中心沿朝向外 0.7 格。
/// 速度随机数只取一遍，生成实体用的是物品自己的随机源，不碰 `level.random`。
///
/// `DefaultDispenseItemBehavior.execute`: spawn one item at the `spawnItem` velocity.
/// The position is `DispenserBlock.getDispensePosition`, 0.7 blocks out from the block centre.
/// The velocity draws happen once; the entity's own random source is separate from `level.random`.
pub(super) fn throw_one(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> ItemStack {
    let velocity = spawn_item_random(world, facing);
    world.spawn_dispensed_item_with_velocity(
        copy_with_count(&stack, 1),
        facing,
        dispense_position(pos, facing),
        velocity,
    );
    without_one(&stack)
}

/// `DispenserBlock.getDispensePosition`：中心沿朝向出去 0.7 格。
///
/// `DispenserBlock.getDispensePosition`: 0.7 blocks out from the block centre.
fn dispense_position(pos: BlockPos, facing: Direction) -> Vec3 {
    let (step_x, step_y, step_z) = facing.normal();
    Vec3::new(
        f64::from(pos.x) + 0.5 + 0.7 * f64::from(step_x),
        f64::from(pos.y) + 0.5 + 0.7 * f64::from(step_y),
        f64::from(pos.z) + 0.5 + 0.7 * f64::from(step_z),
    )
}

/// 内层的 `defaultDispenseItemBehavior.dispense`：先抛，再 1000，再 2000。外层还会再放一遍。
///
/// The inner `defaultDispenseItemBehavior.dispense`: throw, then 1000, then 2000.
/// The outer `dispense` plays them again.
pub(super) fn nested_default(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> ItemStack {
    let stack = throw_one(world, pos, facing, stack);
    world.level_event(1000, pos, 0);
    world.level_event(2000, pos, facing.id());
    stack
}

/// `consumeWithRemainder`。余下的那件先塞回发射器，塞不下再抛，并多放一次 1000 和 2000。
///
/// `consumeWithRemainder`. The remainder is inserted back into the dispenser first.
/// What does not fit is thrown, with an extra 1000 and 2000.
pub(super) fn consume_with_remainder(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
    remainder: ItemStack,
) -> ItemStack {
    let left = without_one(&stack);
    if left.is_empty() {
        return remainder;
    }
    let leftover = insert_item(world, pos, remainder);
    if !leftover.is_empty() {
        let velocity = spawn_item_random(world, facing);
        world.spawn_dispensed_item_with_velocity(
            leftover,
            facing,
            dispense_position(pos, facing),
            velocity,
        );
        world.level_event(1000, pos, 0);
        world.level_event(2000, pos, facing.id());
    }
    left
}

/// `DispenserBlockEntity.insertItem`。塞进去的时候格子里还是没减过数量的那一堆。
///
/// `DispenserBlockEntity.insertItem`. The slots still hold the unshrunk stack.
fn insert_item(world: &mut World, pos: BlockPos, mut stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        return stack;
    }
    let limit = stack_limit(&stack);
    let mut moved = false;
    world.with_container(pos, &mut |container| {
        for slot in 0..container.size() {
            if stack.is_empty() {
                break;
            }
            let target = container.get(slot);
            if !target.is_empty() && !same_item_same_components(&stack, &target) {
                continue;
            }
            let room = limit - target.count;
            if room <= 0 {
                continue;
            }
            let transfer = stack.count.min(room);
            if target.is_empty() {
                container.set(slot, limited(copy_with_count(&stack, transfer)));
            } else {
                let mut grown = target;
                grown.count += transfer;
                container.set(slot, limited(grown));
            }
            stack = without_count(&stack, transfer);
            moved = true;
        }
    });
    if moved {
        set_changed(world, pos);
    }
    stack
}

/// 从这一堆里拿走 `count` 个。拿光了就是空的。
fn without_count(stack: &ItemStack, count: i32) -> ItemStack {
    if stack.count <= count {
        ItemStack::empty()
    } else {
        copy_with_count(stack, stack.count - count)
    }
}

/// `getDispenseMethod`：注册表优先，然后是可装备、硫磺立方能吞的、带实体数据的刷怪蛋。
///
/// `getDispenseMethod`: the registry first, then equippable, sulfur-cube swallowable, then a
/// spawn egg that carries entity data.
fn dispatch(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let item = stack.item;
    if bone::handles(item) {
        return bone::execute(world, pos, facing, stack);
    }
    if bucket::is_filled(item) {
        return bucket::filled(world, pos, facing, stack);
    }
    if bucket::is_empty_bucket(item) {
        return bucket::empty(world, pos, facing, stack);
    }
    if ignite::handles(item) {
        return ignite::execute(world, pos, facing, stack);
    }
    if place::handles(item) {
        return place::execute(world, pos, facing, stack);
    }
    if bottle::handles(item) {
        return bottle::execute(world, pos, facing, stack);
    }
    if wax::handles(item) {
        return wax::execute(world, pos, facing, stack);
    }
    if entity::handles(item) {
        return entity::execute(world, pos, facing, stack);
    }
    // 注册表没有这项时，可装备比刷怪蛋优先。官方 `getDefaultDispenseMethod`。
    //
    // When the registry has no entry, equippable wins over a spawn egg.
    // Official `getDefaultDispenseMethod`.
    if equipment::is_equippable(&stack) {
        return equipment::execute(world, pos, facing, stack);
    }
    if entity::is_spawn_egg(item) {
        return entity::spawn_egg(world, pos, facing, stack);
    }
    // `SulfurCubeBlockDispenseItemBehavior`：没有硫磺立方时就是 `super.execute`，一次默认抛出。
    //
    // `SulfurCubeBlockDispenseItemBehavior`: with no sulfur cube this is `super.execute`,
    // one default throw.
    Shot::plain(throw_one(world, pos, facing, stack))
}
