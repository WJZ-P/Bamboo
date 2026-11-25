//! `SnifferEggBlock`：放置排孵化的计划刻，计划刻推进孵化阶段，第三阶段孵出幼年嗅探兽。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::tags::block_tags;

use crate::{
    behaviour::Behaviour,
    entity::{
        Entity,
        mob::{MobType, living, spawn},
    },
    state::{BlockState, HATCH},
    world::World,
};

/// 普通情况下孵化要多少刻。
const HATCH_TIME: i32 = 24_000;
/// 底下是加速孵化的方块时要多少刻。
const BOOSTED_HATCH_TIME: i32 = 12_000;
/// 孵化分三个阶段，每个阶段排一次计划刻。
const HATCH_STAGES: i32 = 3;
/// 每次计划刻在阶段时长上多加的随机刻数的上界（不含）。
const RANDOM_EXTRA_DELAY: i32 = 300;
/// `MAX_HATCH_LEVEL`：到这一级就孵出嗅探兽。
const MAX_HATCH_LEVEL: i32 = 2;
/// `getBabyStartAge`：孵出来的是幼年。
const BABY_START_AGE: i32 = -48_000;

/// `SnifferEggBlock`。
pub(super) struct SnifferEgg;

/// `hatchBoost`：底下的方块能加速孵化。
fn hatch_boost(world: &World, pos: BlockPos) -> bool {
    world
        .block_state(pos.below())
        .is_tagged(block_tags::SNIFFER_EGG_HATCH_BOOST)
}

impl Behaviour for SnifferEgg {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        let boosted = hatch_boost(world, pos);
        if boosted {
            world.level_event(3009, pos, 0);
        }
        let hatch_time = if boosted {
            BOOSTED_HATCH_TIME
        } else {
            HATCH_TIME
        };
        let delay = hatch_time / HATCH_STAGES + world.random().next_int(RANDOM_EXTRA_DELAY);
        world.schedule_tick(pos, state.block(), delay);
    }

    /// `tick`：`HATCH` 到第三阶段放孵出声、拿掉方块、生一只幼年嗅探兽；否则放碎蛋声、
    /// 推进一阶段。
    ///
    /// `tick`: the third `HATCH` stage plays the hatch sound, removes the block and spawns
    /// a baby sniffer; otherwise the crack sound plays and `HATCH` advances by one.
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let hatch = state.get(HATCH);
        // 官方 crack / hatch 音都是音量 0.7、音高 `0.9 + random.nextFloat() * 0.2`。
        // Vanilla crack / hatch sounds are volume 0.7, pitch `0.9 + random.nextFloat() * 0.2`.
        let pitch = 0.9 + world.random().next_float() * 0.2;
        if hatch < MAX_HATCH_LEVEL {
            world.play_block_sound(
                pos,
                mc_bamboo_data::sounds::sound_event("minecraft:block.sniffer_egg.crack"),
                0.7,
                pitch,
            );
            world.set_block(pos, state.with(HATCH, hatch + 1), 2);
            return;
        }
        world.play_block_sound(
            pos,
            mc_bamboo_data::sounds::sound_event("minecraft:block.sniffer_egg.hatch"),
            0.7,
            pitch,
        );
        world.remove_block(pos);
        // `EntityTypes.SNIFFER.create` 分配 ID 和实体随机种子；`snapTo` 的朝向取世界随机源。
        let mut sniffer =
            world.prepare_entity(Entity::by_mob_type(MobType::Sniffer, Vec3::default()));
        sniffer.with_mob(|me, mob| {
            living::set_age(me, mob, BABY_START_AGE);
        });
        let yaw = mc_bamboo_core::mth::wrap_degrees(world.random().next_float() * 360.0);
        spawn::snap_to(
            &mut sniffer,
            Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ),
            yaw,
            0.0,
        );
        world.add_prepared_entity(sniffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, Effect, entity::EntityKind, run_scheduled_ticks, testing::TestLevel};
    use mc_bamboo_data::blocks;

    const EGG_POS: BlockPos = BlockPos::new(0, 64, 0);

    /// 摆好蛋（走完 `onPlace` 排孵化计划刻）并保留计划刻队列。
    /// Place an egg (running `onPlace` and its scheduled hatch tick), keeping the queue.
    fn place_egg(below: u32) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(EGG_POS.below(), BlockState(below));
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block_and_update(EGG_POS, BlockState(blocks::SNIFFER_EGG));
        }
        (level, ticks)
    }

    /// 前两次计划刻只是碎蛋推进 `HATCH`，不孵出生物。
    /// The first two ticks only crack the egg and advance `HATCH`, no baby spawns.
    #[test]
    fn cracking_advances_hatch_without_spawning() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        for hatch in 0..2 {
            let egg = BlockState(blocks::SNIFFER_EGG).with(HATCH, hatch);
            level.set(EGG_POS, egg);
            let mut world = World::new(&mut level, &mut ticks);
            egg.tick(&mut world, EGG_POS);
            let effects = world.drain_effects();
            drop(world);
            assert_eq!(level.get(EGG_POS).get(HATCH), hatch + 1);
            assert!(level.entities().is_empty());
            assert!(effects.iter().any(|effect| matches!(
                *effect,
                Effect::Sound { sound, .. }
                    if sound == i32::from(mc_bamboo_data::sounds::sound_event(
                        "minecraft:block.sniffer_egg.crack"
                    ))
            )));
        }
    }

    /// 第三次计划刻移除方块、孵出一只幼年嗅探兽，落在蛋的中心。
    /// The third tick removes the block and spawns one baby sniffer at the egg centre.
    #[test]
    fn hatching_spawns_one_baby_sniffer() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let egg = BlockState(blocks::SNIFFER_EGG).with(HATCH, 2);
        level.set(EGG_POS, egg);
        let mut world = World::new(&mut level, &mut ticks);
        egg.tick(&mut world, EGG_POS);
        let effects = world.drain_effects();
        drop(world);

        assert!(level.get(EGG_POS).is_air());
        assert!(effects.iter().any(|effect| matches!(
            *effect,
            Effect::Sound { sound, .. }
                if sound == i32::from(mc_bamboo_data::sounds::sound_event(
                    "minecraft:block.sniffer_egg.hatch"
                ))
        )));
        let babies: Vec<_> = level.entities().iter().collect();
        assert_eq!(babies.len(), 1);
        let EntityKind::Mob(mob) = &babies[0].kind else {
            panic!("a hatchling is a mob");
        };
        assert_eq!(mob.mob_type, MobType::Sniffer);
        assert_eq!(mob.age, BABY_START_AGE);
        assert!((babies[0].position.x - f64::from(EGG_POS.x) - 0.5).abs() < 1.0e-9);
        assert!((babies[0].position.y - f64::from(EGG_POS.y) - 0.5).abs() < 1.0e-9);
        assert!((babies[0].position.z - f64::from(EGG_POS.z) - 0.5).abs() < 1.0e-9);
    }

    /// 苔藓块把第一次碎蛋计划刻从 8000+ 提前到 4000+：5000 刻时苔藓已碎、普通还没。
    /// Moss halves the first crack tick from 8000+ to 4000+: at tick 5000 moss has
    /// cracked and the plain block has not.
    #[test]
    fn moss_boost_schedules_the_first_crack_sooner() {
        let (mut plain, mut plain_ticks) = place_egg(blocks::DIRT);
        run_scheduled_ticks(&mut plain, &mut plain_ticks, 5000);
        assert_eq!(plain.get(EGG_POS).get(HATCH), 0);
        assert!(plain.entities().is_empty());

        let (mut moss, mut moss_ticks) = place_egg(blocks::MOSS_BLOCK);
        run_scheduled_ticks(&mut moss, &mut moss_ticks, 5000);
        assert_eq!(moss.get(EGG_POS).get(HATCH), 1);
        assert!(moss.entities().is_empty());
    }
}
