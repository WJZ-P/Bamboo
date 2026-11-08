//! 方块实体被移除时的副作用，对应官方 `BlockEntity.preRemoveSideEffects`。
//!
//! 世界里没有方块实体，容器内容按空算；只有状态就是内容镜像的 `chiseled_bookshelf` 和 `jukebox`
//! 要看状态。官方即使内容为空，`Containers.dropItemStack` 也会先取三个 `nextDouble`，
//! 数漏了后面的随机刻和计划刻就全错开，所以槽位数量要照抄官方容器的 `getContainerSize`。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockClass, registries, sounds::sound_event, tags::block_tags};

use crate::{
    state::{
        BlockState, HAS_RECORD, Property, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED, SLOT_2_OCCUPIED,
        SLOT_3_OCCUPIED, SLOT_4_OCCUPIED, SLOT_5_OCCUPIED,
    },
    world::World,
};

/// `ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES`。
const SLOT_OCCUPIED: [Property<bool>; 6] = [
    SLOT_0_OCCUPIED,
    SLOT_1_OCCUPIED,
    SLOT_2_OCCUPIED,
    SLOT_3_OCCUPIED,
    SLOT_4_OCCUPIED,
    SLOT_5_OCCUPIED,
];

/// `Containers.dropItemStack` 在堆是空的时候取的三对 double：掉落点的 `x/y/z`。
fn drop_empty_stack(world: &mut World) {
    for _ in 0..3 {
        world.random().next_double();
    }
}

/// 一格空的槽位：`dropContents` 对每一格都调用 `dropItemStack`，空堆只取掉落点。
fn drop_empty_slot(world: &mut World) {
    drop_empty_stack(world);
}

/// 一本 `chiseled_bookshelf` 里的书：掉落点之后，`split(nextInt(21) + 10)` 一次就分完，
/// 再是 `setDeltaMovement` 里的三个 `triangle`，每个取两个 double。
fn drop_book(world: &mut World) {
    drop_empty_stack(world);
    world.random().next_int(21);
    for _ in 0..6 {
        world.random().next_double();
    }
}

/// 官方容器实体的 `getContainerSize`，内容为空时槽位一个不少。
fn drop_empty_slots(world: &mut World, slots: i32) {
    for _ in 0..slots {
        drop_empty_slot(world);
    }
}

/// `BlockEntity.preRemoveSideEffects`。
pub(crate) fn pre_remove_side_effects(state: BlockState, world: &mut World) {
    let Some(entity) = state.block().block_entity_type else {
        return;
    };
    use registries::block_entity_type as E;

    match entity {
        // `ChestBlockEntity`、`TrappedChestBlockEntity`（铜箱子也是这个实体）、`BarrelBlockEntity`。
        E::CHEST | E::TRAPPED_CHEST | E::BARREL => drop_empty_slots(world, 27),
        // `AbstractFurnaceBlockEntity`：三格燃料，经验来自空的 `recipesUsed`，不取数。
        E::FURNACE | E::SMOKER | E::BLAST_FURNACE => drop_empty_slots(world, 3),
        E::HOPPER | E::BREWING_STAND => drop_empty_slots(world, 5),
        E::DISPENSER | E::DROPPER | E::CRAFTER => drop_empty_slots(world, 9),
        E::SHELF => drop_empty_slots(world, 3),
        E::DECORATED_POT => drop_empty_slots(world, 1),
        E::CAMPFIRE => drop_empty_slots(world, 4),
        E::CHISELED_BOOKSHELF => {
            for slot in SLOT_OCCUPIED {
                if state.get(slot) {
                    drop_book(world);
                } else {
                    drop_empty_slot(world);
                }
            }
        }
        // `JukeboxBlockEntity.popOutTheItem` 用 `offsetRandomXZ` 取两个 `nextFloat`。
        // 潜影盒把这个方法覆盖成空的。
        E::JUKEBOX if state.get(HAS_RECORD) => {
            world.random().next_float();
            world.random().next_float();
        }
        _ => {}
    }
}

/// `BlockEntity.setRemoved`：这个位置在 `preRemoveSideEffects` 之后，不管跳过副作用与否都会走到。
/// 目前只有信标在这一步出声。
pub(crate) fn removed_side_effects(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.block().block_entity_type == Some(registries::block_entity_type::BEACON) {
        world.play_block_sound(
            pos,
            sound_event("minecraft:block.beacon.deactivate"),
            1.0,
            1.0,
        );
    }
}

/// `state.shouldChangedStateKeepBlockEntity(old)`：新状态的方块决定旧的方块实体留不留。
pub(crate) fn keeps_block_entity(state: BlockState, old: BlockState) -> bool {
    match state.class() {
        BlockClass::CopperChestBlock | BlockClass::WeatheringCopperChestBlock => {
            old.is_tagged(block_tags::COPPER_CHESTS)
        }
        BlockClass::CopperGolemStatueBlock | BlockClass::WeatheringCopperGolemStatueBlock => {
            old.is_tagged(block_tags::COPPER_GOLEM_STATUES)
        }
        _ => false,
    }
}
