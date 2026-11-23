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

/// `TurtleEggBlock.decreaseEggs`：先放碎蛋声（音高也取世界随机数），最后一枚用
/// `destroyBlock(pos, false)`，否则少一枚并发 2001 事件（带减之前的状态）。
///
/// `TurtleEggBlock.decreaseEggs`: the break sound first (its pitch is a world-random
/// draw), then `destroyBlock(pos, false)` for the last egg, otherwise one egg fewer and a
/// 2001 level event carrying the state before the decrement.
pub(crate) fn decrease_eggs(world: &mut World, pos: BlockPos, state: BlockState) {
    let pitch = 0.9 + world.random().next_float() * 0.2;
    world.play_block_sound(
        pos,
        mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.egg_break"),
        0.7,
        pitch,
    );
    let eggs = state.get(EGGS);
    let data = state.0 as i32;
    if eggs <= 1 {
        world.destroy_block(pos, false, crate::world::UPDATE_LIMIT);
    } else {
        world.set_block(pos, state.with(EGGS, eggs - 1), 2);
        // `gameEvent(BLOCK_DESTROY)` 在 `levelEvent` 之前；游戏事件还没有监听者，
        // 这里只把客户端的 2001 事件发出去，参数带减之前的状态。
        // `gameEvent(BLOCK_DESTROY)` precedes `levelEvent`; game events have no
        // listeners yet, so only the client 2001 event with the old state is emitted.
        world.level_event(2001, pos, data);
    }
}

/// `TurtleEggBlock.playerDestroy` 在 `super.playerDestroy`（战利品由游戏层生成）之后的
/// 那一步：挖掉一摞蛋时只少一枚。调用方已经移除过方块，也按 `hasCorrectToolForDrops`
/// 判断过。
///
/// The step of `TurtleEggBlock.playerDestroy` after `super.playerDestroy` (the game layer
/// spawns the loot): breaking part of a clutch removes one egg. The caller has already
/// removed the block and checked `hasCorrectToolForDrops`.
pub fn player_destroy(world: &mut World, pos: BlockPos, state: BlockState) {
    if state.is(mc_bamboo_data::blocks::TURTLE_EGG) {
        decrease_eggs(world, pos, state);
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

    /// 玩家挖掉一摞 4 枚蛋时方块已经被移除，`playerDestroy` 只放回 3 枚，
    /// 并留下碎蛋声和带减之前状态的 2001 事件。
    ///
    /// Breaking a clutch of four: the block is already removed, and `playerDestroy` puts
    /// three eggs back, leaving a break sound and a 2001 event carrying the old state.
    #[test]
    fn player_destroy_puts_back_one_egg_fewer() {
        let (mut level, mut ticks, egg) = night_egg(4, 0);
        level.set(EGG_POS, BlockState(blocks::AIR));
        let mut world = World::new(&mut level, &mut ticks);
        player_destroy(&mut world, EGG_POS, egg);
        let effects = world.drain_effects();
        drop(world);
        let after = level.get(EGG_POS);
        assert!(after.is_same_block(egg));
        assert_eq!(after.get(EGGS), 3);
        assert_eq!(after.get(HATCH), 0);
        assert!(effects.iter().any(|effect| matches!(
            *effect,
            crate::Effect::LevelEvent {
                event: 2001,
                pos,
                data,
                except_initiator: false
            } if pos == EGG_POS && data == egg.0 as i32
        )));
        assert!(effects.iter().any(|effect| matches!(
            *effect,
            crate::Effect::Sound {
                sound,
                pitch,
                ..
            } if i32::from(sound) == i32::from(mc_bamboo_data::sounds::sound_event(
                "minecraft:entity.turtle.egg_break"
            )) && (0.9..1.1).contains(&pitch)
        )));
    }

    /// 最后一枚被挖掉时什么也不留下，`destroyBlock(pos, false)` 只发一次破碎粒子。
    ///
    /// Breaking the last egg leaves nothing behind; `destroyBlock(pos, false)` only
    /// emits the break particles.
    #[test]
    fn player_destroy_of_the_last_egg_leaves_air() {
        let (mut level, mut ticks, egg) = night_egg(1, 0);
        level.set(EGG_POS, BlockState(blocks::AIR));
        let mut world = World::new(&mut level, &mut ticks);
        player_destroy(&mut world, EGG_POS, egg);
        let effects = world.drain_effects();
        drop(world);
        assert!(level.get(EGG_POS).is_air());
        assert!(effects.iter().any(|effect| matches!(
            *effect,
            crate::Effect::Sound { sound, .. } if i32::from(sound) == i32::from(
                mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.egg_break")
            )
        )));
        assert!(!effects.iter().any(|effect| matches!(
            *effect,
            crate::Effect::LevelEvent {
                event: 2001,
                pos,
                ..
            } if pos == EGG_POS
        )));
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
