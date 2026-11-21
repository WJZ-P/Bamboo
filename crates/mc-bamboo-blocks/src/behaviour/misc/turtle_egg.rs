//! `TurtleEggBlock`：放置、踩碎和随机刻孵化。踩碎由移动那边的 `step_on` 处理。
//!
//! `TurtleEggBlock`: placing, trampling and random-tick hatching. Trampling comes from
//! the movement side's `step_on`.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    entity::Entity,
    entity::mob::{MobType, living, spawn},
    state::{BlockState, EGGS, HATCH},
    world::World,
};

/// `TurtleEggBlock`。
pub(super) struct TurtleEgg;

/// `MAX_EGGS`。
const MAX_EGGS: i32 = 4;

impl Behaviour for TurtleEgg {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            let eggs = state.get(EGGS) + 1;
            Some(state.with(EGGS, eggs.min(MAX_EGGS)))
        } else {
            Some(default_state(block))
        }
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let adds_an_egg = !ctx.is_secondary_use_active()
            && ctx.item == state.block().item
            && state.get(EGGS) < MAX_EGGS;
        adds_an_egg || Plain.can_be_replaced(state, ctx, world)
    }

    /// `onPlace`：放在沙子上时发 2012 号粒子事件。
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if world.block_state(pos.below()).is_tagged(block_tags::SAND) {
            world.level_event(2012, pos, 15);
        }
    }

    /// `randomTick`：先按孵化概率取一个 `nextFloat`，再在沙子上推进 `HATCH`；
    /// 第三阶段孵出 `EGGS` 只小海龟。
    ///
    /// `randomTick`: one `nextFloat` for the hatch chance, then advance `HATCH` on sand;
    /// the third stage hatches `EGGS` baby turtles.
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        // 环境属性 `TURTLE_EGG_HATCH_CHANCE` 白天是 0.002；`Timelines` 在夜里把它顶到 1.0。
        // The environment attribute is 0.002 by day; `Timelines` raises it to 1.0 at night.
        let day_time = world.game_time().rem_euclid(24_000);
        let chance = if (21_062..21_905).contains(&day_time) {
            1.0
        } else {
            0.002
        };
        if !(chance > 0.0 && world.random().next_float() < chance) {
            return;
        }
        if !world.block_state(pos.below()).is_tagged(block_tags::SAND) {
            return;
        }
        let hatch = state.get(HATCH);
        let pitch = 0.9 + world.random().next_float() * 0.2;
        if hatch < 2 {
            world.play_block_sound(
                pos,
                mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.egg_crack"),
                0.7,
                pitch,
            );
            world.set_block(pos, state.with(HATCH, hatch + 1), 2);
            return;
        }
        world.play_block_sound(
            pos,
            mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.egg_hatch"),
            0.7,
            pitch,
        );
        world.remove_block(pos);
        let eggs = state.get(EGGS);
        let data = state.0 as i32;
        for index in 0..eggs {
            world.level_event(2001, pos, data);
            // `EntityTypes.TURTLE.create` 照样分配 ID 和实体随机种子，否则孵出来的
            // 小海龟全带 0 号 ID、共用一个构造随机源。构造取的是实体自己的随机源，
            // 不是 `level.random`。
            // `EntityTypes.TURTLE.create` allocates an id and an entity random seed;
            // without it every hatchling carries id 0 and shares one constructor
            // random. The draws use the entity's own random, not `level.random`.
            let mut turtle =
                world.prepare_entity(Entity::by_mob_type(MobType::Turtle, Vec3::default()));
            turtle.with_mob(|me, mob| {
                living::set_age(me, mob, -24_000);
                mob.turtle.home_pos = pos;
            });
            spawn::snap_to(
                &mut turtle,
                Vec3::new(
                    f64::from(pos.x) + 0.3 + f64::from(index) * 0.2,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.3,
                ),
                0.0,
                0.0,
            );
            world.add_prepared_entity(turtle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, entity::EntityKind, testing::TestLevel};
    use mc_bamboo_data::blocks;

    const EGG_POS: BlockPos = BlockPos::new(0, 62, 0);

    /// 夜里 `Timelines` 把孵化概率顶到 1.0，不用赌白天那 0.002 的 `nextFloat`。
    ///
    /// At night `Timelines` raises the chance to 1.0, so the test need not gamble
    /// on the daytime 0.002 draw.
    fn night_egg(eggs: i32, hatch: i32) -> (TestLevel, BlockTicks, BlockState) {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        ticks.set_time(21_500);
        let egg = BlockState(blocks::TURTLE_EGG)
            .with(EGGS, eggs)
            .with(HATCH, hatch);
        level.set(EGG_POS.below(), BlockState(blocks::SAND));
        level.set(EGG_POS, egg);
        (level, ticks, egg)
    }

    #[test]
    fn a_cracking_egg_advances_hatch_without_spawning() {
        let (mut level, mut ticks, egg) = night_egg(2, 0);
        let mut world = World::new(&mut level, &mut ticks);
        egg.random_tick(&mut world, EGG_POS);
        drop(world);
        assert_eq!(level.get(EGG_POS).get(HATCH), 1);
        assert!(level.entities().is_empty());
    }

    #[test]
    fn hatching_spawns_one_baby_per_egg_with_distinct_ids() {
        let (mut level, mut ticks, egg) = night_egg(3, 2);
        let mut world = World::new(&mut level, &mut ticks);
        egg.random_tick(&mut world, EGG_POS);
        drop(world);

        assert!(!level.get(EGG_POS).is_same_block(egg));
        let mut turtles: Vec<_> = level.entities().iter().collect();
        assert_eq!(turtles.len(), 3);
        turtles.sort_by(|a, b| a.position.x.total_cmp(&b.position.x));
        let mut ids = Vec::new();
        for (index, turtle) in turtles.iter().enumerate() {
            let EntityKind::Mob(mob) = &turtle.kind else {
                panic!("a hatchling is a mob");
            };
            assert_eq!(mob.mob_type, MobType::Turtle);
            assert_eq!(mob.age, -24_000);
            assert_eq!(mob.turtle.home_pos, EGG_POS);
            let x = f64::from(EGG_POS.x) + 0.3 + index as f64 * 0.2;
            assert!((turtle.position.x - x).abs() < 1.0e-9);
            assert!((turtle.position.y - f64::from(EGG_POS.y)).abs() < 1.0e-9);
            assert!((turtle.position.z - (f64::from(EGG_POS.z) + 0.3)).abs() < 1.0e-9);
            ids.push(turtle.id);
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 3, "each hatchling gets its own allocated id");
    }

    #[test]
    fn an_egg_off_sand_never_advances() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        ticks.set_time(21_500);
        let egg = BlockState(blocks::TURTLE_EGG).with(EGGS, 2).with(HATCH, 2);
        level.set(EGG_POS, egg);
        let mut world = World::new(&mut level, &mut ticks);
        egg.random_tick(&mut world, EGG_POS);
        drop(world);
        assert_eq!(level.get(EGG_POS), egg);
        assert!(level.entities().is_empty());
    }
}
