//! `SulfurCubeBlockDispenseItemBehavior.dispenseBlock`：前方一格里的第一只
//! 硫磺方块怪吞下这一堆的一个，成功就把这堆减一。
//!
//! `SulfurCubeBlockDispenseItemBehavior.dispenseBlock`: the first sulfur cube in
//! the cell in front swallows one item from the stack; on success the stack shrinks.

use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    entity::{
        KindPublic,
        mob::{MobType, sulfur_cube},
    },
    shape::Aabb,
    world::World,
};

fn cell_box(pos: BlockPos) -> Aabb {
    Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x) + 1.0,
        f64::from(pos.y) + 1.0,
        f64::from(pos.z) + 1.0,
    )
}

/// 官方 `getEntitiesOfClass(SulfurCube.class, new AABB(pos))` 逐个调
/// `equipItem(stack)`；`equipItem` 成功就 `stack.shrink(1)` 并返回真。
/// 这里用公开实体表找目标，再就地改那一只的 BODY 槽和原型派生值。
///
/// Vanilla walks `getEntitiesOfClass(SulfurCube.class, new AABB(pos))` calling
/// `equipItem(stack)` on each; the first success shrinks the stack by one.
pub(super) fn dispense_block(world: &mut World, pos: BlockPos, stack: &ItemStack) -> bool {
    let cell = cell_box(pos);
    let mut candidates = Vec::new();
    world.level().for_each_entity_in(cell, &mut |public| {
        if let KindPublic::Mob(mob) = &public.kind
            && mob.mob_type == MobType::SulfurCube
        {
            candidates.push(public.uid);
        }
    });
    for uid in candidates {
        let mut equipped = false;
        world.with_listed_entity_mut(uid, &mut |entity| {
            entity.with_mob(|_, mob| {
                if sulfur_cube::equip_item(mob, stack) {
                    equipped = true;
                }
            });
        });
        if equipped {
            return true;
        }
    }
    false
}

/// 前方一格里有没有硫磺方块怪（测试 / 发射器夹具用）。
#[cfg(test)]
pub(super) fn has_cube(world: &World, pos: BlockPos) -> bool {
    let mut found = false;
    let cell = cell_box(pos);
    world.level().for_each_entity_in(cell, &mut |public| {
        if let KindPublic::Mob(mob) = &public.kind
            && mob.mob_type == MobType::SulfurCube
        {
            found = true;
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::{BlockTicks, entity::Entity, testing::TestLevel};

    #[test]
    fn a_swallowable_item_is_absorbed_by_a_cube_in_front() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.spawn_entity(Entity::by_mob_type(
            MobType::SulfurCube,
            Vec3::new(2.5, 64.0, 0.5),
        ));
        let stack = ItemStack::new(mc_bamboo_data::items::DIRT, 1);
        assert!(dispense_block(&mut world, BlockPos::new(2, 64, 0), &stack));
        assert!(has_cube(&world, BlockPos::new(2, 64, 0)));
    }

    #[test]
    fn no_cube_means_the_item_is_not_consumed() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let stack = ItemStack::new(mc_bamboo_data::items::DIRT, 1);
        assert!(!dispense_block(&mut world, BlockPos::new(2, 64, 0), &stack));
    }
}
