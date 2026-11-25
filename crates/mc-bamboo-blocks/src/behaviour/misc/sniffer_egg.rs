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
        let pitch = 0.7 + world.random().next_float() * 0.2;
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
