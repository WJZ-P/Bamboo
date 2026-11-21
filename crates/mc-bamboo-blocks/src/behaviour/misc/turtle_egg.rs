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
            let mut turtle = Entity::by_mob_type(MobType::Turtle, Vec3::default());
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
