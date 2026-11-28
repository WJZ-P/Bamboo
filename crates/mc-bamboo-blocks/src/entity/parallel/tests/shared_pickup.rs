//! 并行驱动里两个生物同一刻看中同一堆掉落物：物品只能进其中一个的背包。
//!
//! 算阶段两边看到的都是阶段开始时的快照，所以「拿走」必须在落地时对着物品当前的状态
//! 重新判定；在算阶段就把东西加进背包的话，两边各拿一份。
//!
//! Two mobs wanting the same ground stack in one tick of the parallel driver: it may
//! enter only one inventory.
//!
//! Both compute against the snapshot from the start of the phase, so taking the stack
//! must be decided again at commit time against the item's current state. Adding it to
//! the inventory during compute hands a copy to each.

use mc_bamboo_data::items;

use super::*;
use crate::entity::{EntityKind, mob::MobType};

const BREAD: i32 = 5;

fn bread_everywhere(entities: &[Entity]) -> i32 {
    entities
        .iter()
        .map(|entity| match &entity.kind {
            EntityKind::Mob(mob) => mob.villager.inventory.count_item(items::BREAD),
            EntityKind::Item(ground) if !entity.removed => ground.stack.count,
            _ => 0,
        })
        .sum()
}

#[test]
fn two_villagers_do_not_both_take_the_same_stack() {
    let blocks = flat_blocks();
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for x in [0.3, 0.9] {
            world.spawn_entity(Entity::by_mob_type(
                MobType::Villager,
                Vec3::new(x, 65.0, 0.5),
            ));
        }
        world.spawn_entity(Entity::ground_item_with_delta(
            Vec3::new(0.6, 65.0, 0.5),
            ItemStack::new(items::BREAD, BREAD),
            Vec3::default(),
        ));
    }
    for _ in 0..3 {
        let mut world = World::new(&mut level, &mut ticks);
        tick_entities_parallel(&blocks, &mut world, &InlineExecutor);
    }
    let after = World::new(&mut level, &mut ticks).live_entities();

    assert_eq!(bread_everywhere(&after), BREAD);
}

#[test]
fn two_foxes_do_not_both_take_the_same_stack() {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;

    let blocks = flat_blocks();
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for x in [0.3, 0.9] {
            world.spawn_entity(Entity::by_mob_type(MobType::Fox, Vec3::new(x, 65.0, 0.5)));
        }
        world.spawn_entity(Entity::ground_item_with_delta(
            Vec3::new(0.6, 65.0, 0.5),
            ItemStack::new(items::BREAD, BREAD),
            Vec3::default(),
        ));
    }
    {
        let mut world = World::new(&mut level, &mut ticks);
        tick_entities_parallel(&blocks, &mut world, &InlineExecutor);
    }
    let after = World::new(&mut level, &mut ticks).live_entities();

    let bread: i32 = after
        .iter()
        .map(|entity| match &entity.kind {
            EntityKind::Mob(mob) => {
                let held = mob.equipment.get(EquipmentSlot::MainHand);
                if held.item == items::BREAD {
                    held.count
                } else {
                    0
                }
            }
            EntityKind::Item(ground) if !entity.removed && ground.stack.item == items::BREAD => {
                ground.stack.count
            }
            _ => 0,
        })
        .sum();
    assert_eq!(bread, BREAD);
}
