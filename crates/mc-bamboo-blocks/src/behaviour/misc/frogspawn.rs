//! `FrogspawnBlock`：放下时排孵化的计划刻，到时孵出 2 到 5 只蝌蚪。
//!
//! `FrogspawnBlock`: placing schedules the hatch tick, which releases two to five tadpoles.

use mc_bamboo_core::{BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::tags::{block_tags, fluid_tags};

use crate::{
    behaviour::{Behaviour, Plain},
    entity::{
        Entity,
        mob::{MobType, spawn},
    },
    state::BlockState,
    world::World,
};

/// `FrogspawnBlock`。
pub(super) struct Frogspawn;

/// `spawnTadpoles`：`nextInt(2, 6)` 只，每只的 x、z 偏移各取一个夹到
/// `[0.2, 0.7999999970197678]` 的 `nextDouble`，朝向 `nextInt(1, 361)`，放在方块下面半格，
/// 设成不会消失。
///
/// `spawnTadpoles`: `nextInt(2, 6)` tadpoles, each with x and z offsets from a
/// `nextDouble` clamped to `[0.2, 0.7999999970197678]` and a `nextInt(1, 361)` yaw, half a
/// block below the spawn and persistent.
fn spawn_tadpoles(world: &mut World, pos: BlockPos) {
    let amount = world.random().next_int_between(2, 6);
    for _ in 0..amount {
        // `EntityTypes.TADPOLE.create` 在取位置的随机数之前。
        // `EntityTypes.TADPOLE.create` comes before the position draws.
        let mut tadpole =
            world.prepare_entity(Entity::by_mob_type(MobType::Tadpole, Vec3::default()));
        let offset = || {
            mth::clamp(
                world.random().next_double(),
                f64::from(0.2_f32),
                0.799_999_997_019_767_8,
            )
        };
        let x = f64::from(pos.x) + offset();
        let z = f64::from(pos.z) + offset();
        let yaw = world.random().next_int_between(1, 361);
        spawn::snap_to(
            &mut tadpole,
            Vec3::new(x, f64::from(pos.y) - 0.5, z),
            yaw as f32,
            0.0,
        );
        tadpole.with_mob(|_, mob| mob.persistence_required = true);
        world.add_prepared_entity(tadpole);
    }
}

/// `mayPlaceOn`：水面上，再往上没有流体。
fn may_place_on(world: &World, pos: BlockPos) -> bool {
    let supports = world
        .fluid_state(pos)
        .is_tagged(fluid_tags::SUPPORTS_FROGSPAWN)
        || world
            .block_state(pos)
            .is_tagged(block_tags::SUPPORTS_FROGSPAWN);
    supports && world.fluid_state(pos.above()).is_empty()
}

impl Behaviour for Frogspawn {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        may_place_on(world, pos.below())
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        // `getFrogspawnHatchDelay`：`nextInt(3600, 12000)`。孵化本身不在范围里，这个数要照取。
        let delay = world.random().next_int_between(3600, 12000);
        world.schedule_tick(pos, state.block(), delay);
    }

    /// `tick`：站不住就碎掉，否则孵化——方块碎掉、放孵化声、`spawnTadpoles`。
    ///
    /// `tick`: break when unsupported, otherwise hatch — the block breaks, the hatch
    /// sound plays and `spawnTadpoles` runs.
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let hatches = self.can_survive(state, world, pos);
        world.destroy_block(pos, false, 512);
        if !hatches {
            return;
        }
        world.play_block_sound(
            pos,
            mc_bamboo_data::sounds::sound_event("minecraft:block.frogspawn.hatch"),
            1.0,
            1.0,
        );
        spawn_tadpoles(world, pos);
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if self.can_survive(state, world, pos) {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        } else {
            BlockState::AIR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, entity::EntityKind, testing::TestLevel};
    use mc_bamboo_data::blocks;

    const SPAWN_POS: BlockPos = BlockPos::new(0, 63, 0);

    /// 水面上的蛙卵到点孵化：方块没了，出来 2 到 5 只不会消失的蝌蚪，落在方块下面半格、
    /// 水平偏移夹在 `[0.2, 0.8)` 里。
    ///
    /// Frogspawn on water hatches when its tick fires: the block is gone and two to five
    /// persistent tadpoles appear half a block below it, their horizontal offsets clamped
    /// to `[0.2, 0.8)`.
    #[test]
    fn hatching_releases_two_to_five_persistent_tadpoles() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let spawn = BlockState(blocks::FROGSPAWN);
        level.set(SPAWN_POS.below(), BlockState(blocks::WATER));
        level.set(SPAWN_POS, spawn);
        let mut world = World::new(&mut level, &mut ticks);
        spawn.tick(&mut world, SPAWN_POS);
        drop(world);

        assert!(level.get(SPAWN_POS).is_air());
        let tadpoles = level.entities();
        assert!((2..=5).contains(&tadpoles.len()), "{}", tadpoles.len());
        for entity in tadpoles.iter() {
            let EntityKind::Mob(mob) = &entity.kind else {
                panic!("蛙卵只孵出生物");
            };
            assert_eq!(mob.mob_type, MobType::Tadpole);
            assert!(mob.persistence_required);
            assert_eq!(entity.position.y, f64::from(SPAWN_POS.y) - 0.5);
            let offset = entity.position.x - f64::from(SPAWN_POS.x);
            assert!((f64::from(0.2_f32)..0.8).contains(&offset), "{offset}");
        }
    }

    /// 下面的水没了：到点只是碎掉，不孵化。
    ///
    /// With the water gone the tick only breaks the block.
    #[test]
    fn unsupported_frogspawn_breaks_without_hatching() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let spawn = BlockState(blocks::FROGSPAWN);
        level.set(SPAWN_POS.below(), BlockState(blocks::STONE));
        level.set(SPAWN_POS, spawn);
        let mut world = World::new(&mut level, &mut ticks);
        spawn.tick(&mut world, SPAWN_POS);
        drop(world);

        assert!(level.get(SPAWN_POS).is_air());
        assert!(level.entities().is_empty());
    }
}
