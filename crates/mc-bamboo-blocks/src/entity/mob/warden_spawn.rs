//! `SpawnUtil.trySpawnMob` 的监守者召唤：尖啸体警告到 4 级时照官方取点。
//!
//! 参数是 `WARDEN, TRIGGERED, getBlockPos(), 20, 5, 6, ON_TOP_OF_COLLIDER, false`：
//! 20 次尝试，每次在水平 ±5 取点、垂直先抬 6 格，再向下最多 ±6 格找可站的地面。

use mc_bamboo_core::{BlockPos, Direction, Vec3};

use crate::{
    entity::{Entity, EntityUid},
    shape,
    world::World,
};

use super::{MobType, spawn, warden, warden_ai};

/// `WARDEN_SPAWN_ATTEMPTS`。
const ATTEMPTS: i32 = 20;
/// `WARDEN_SPAWN_RANGE_XZ`。
const RANGE_XZ: i32 = 5;
/// `WARDEN_SPAWN_RANGE_Y`（也是向下搜索的格数）。
const RANGE_Y: i32 = 6;

/// `trySummonWarden`：警告等级到 4 才取点，返回新监守者的实体 ID。
///
/// `trySummonWarden`: only at warning level 4; returns the new warden's entity id.
pub fn try_summon_warden(
    world: &mut World<'_>,
    origin: BlockPos,
    warning_level: i32,
) -> Option<EntityUid> {
    if warning_level < 4 {
        return None;
    }
    for _ in 0..ATTEMPTS {
        // `Mth.randomBetweenInclusive(random, -5, 5)`：`min + nextInt(max - min + 1)`。
        // `Mth.randomBetweenInclusive(random, -5, 5)`: `min + nextInt(max - min + 1)`.
        let dx = -RANGE_XZ + world.random().next_int(2 * RANGE_XZ + 1);
        let dz = -RANGE_XZ + world.random().next_int(2 * RANGE_XZ + 1);
        let mut pos = BlockPos::new(origin.x + dx, origin.y + RANGE_Y, origin.z + dz);
        if !world.world_border().is_within_bounds_pos(pos) {
            continue;
        }
        if !move_to_possible_spawn_position(world, &mut pos) {
            continue;
        }
        // `SpawnUtil` 里 `mob.checkSpawnRules(levelAccessor, reason)`（Warden 没覆写，恒真）
        // 和 `mob.checkSpawnObstruction(levelReader)`（Warden 覆写成
        // `super.checkSpawnObstruction() && noCollision(getSpawnAABB(...))`）。
        // `SpawnUtil`: `mob.checkSpawnRules` (not overridden, always true) and
        // `mob.checkSpawnObstruction` (warden adds `noCollision(getSpawnAABB(...))`).
        if !spawn_obstruction_clear(world, pos) {
            continue;
        }
        // `EntityType.create(level, null, pos, TRIGGERED, false, false)`：
        // 实体脚落在 `pos` 的格心（y 不加 0.5），`finalizeSpawn` 置 EMERGING 姿态。
        // `EntityType.create(...)`: the mob's feet sit at the centre of `pos`
        // (no 0.5 on y); `finalizeSpawn` sets the EMERGING pose.
        let mut entity =
            world.prepare_entity(Entity::by_mob_type(MobType::Warden, Vec3::default()));
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        );
        // `EntityType.create` 的 `snapTo(..., wrapDegrees(nextFloat() * 360), 0)`。
        // `EntityType.create`'s `snapTo(..., wrapDegrees(nextFloat() * 360), 0)`.
        let yaw = mc_bamboo_core::mth::wrap_degrees(world.random().next_float() * 360.0);
        spawn::snap_to(&mut entity, center, yaw, 0.0);
        // `Mob.finalizeSpawn`：`triangle(0, 0.11485)` 两次 `nextDouble()`（共四步线性
        // 同余）加一次 `nextFloat()` 决定左撇子。记忆和装备由下面显式设置。
        // `Mob.finalizeSpawn`: `triangle(0, 0.11485)` draws two `nextDouble()` values
        // (four LCG steps) plus one `nextFloat()` for left-handedness.
        world.random().next_double();
        world.random().next_double();
        world.random().next_float();
        entity.with_mob(|_, mob| {
            // `Warden.finalizeSpawn(TRIGGERED)`：DIG_COOLDOWN 1200、IS_EMERGING 134、
            // EMERGING 姿态（Java 侧 `EntityType.create` 不跑 finalizeSpawn，对拍场景要显式设）。
            // `Warden.finalizeSpawn(TRIGGERED)`: DIG_COOLDOWN 1200, IS_EMERGING 134 and
            // the EMERGING pose (Java's `EntityType.create` skips finalizeSpawn, so the
            // oracle scene sets these explicitly too).
            mob.warden.pose = warden::WardenPose::Emerging;
            if let Some(brain) = mob.brain.as_mut() {
                use crate::entity::mob::brain::memory::{MemoryType, MemoryValue};
                brain.memories.set_with_ttl(
                    MemoryType::IsEmerging,
                    MemoryValue::Unit,
                    i64::from(warden_ai::EMERGE_DURATION),
                );
                brain.memories.set_with_ttl(
                    MemoryType::DigCooldown,
                    MemoryValue::Unit,
                    i64::from(warden_ai::DIGGING_COOLDOWN),
                );
            }
        });
        let uid = world.add_prepared_entity(entity);
        // `SpawnUtil` 里 `mob.playAmbientSound()`：刚生成的愤怒为 CALM，环境音是
        // `WARDEN_AMBIENT`、音量 `getSoundVolume()` = 4.0、音高取实体声音音高（简化为 1.0）。
        // `SpawnUtil`: `mob.playAmbientSound()` — a fresh warden is CALM, so the ambient
        // is `WARDEN_AMBIENT` at `getSoundVolume()` = 4.0 (pitch simplified to 1.0).
        world.play_sound(
            center,
            mc_bamboo_data::sounds::sound_event("minecraft:entity.warden.ambient"),
            mc_bamboo_data::sounds::HOSTILE,
            4.0,
            1.0,
            false,
        );
        return Some(uid);
    }
    None
}

/// `Warden.checkSpawnObstruction` 的 `noCollision` 部分：脚上方 2.9 格（宽 0.9 落在
/// 同一格列）里不能有带碰撞的方块。`checkSpawnRules` 对 TRIGGERED 恒真。
///
/// The `noCollision` part of `Warden.checkSpawnObstruction`: the 2.9-tall box above the
/// feet (0.9 wide, inside the same column) must contain no colliding blocks.
fn spawn_obstruction_clear(world: &World<'_>, pos: BlockPos) -> bool {
    for dy in 0..3 {
        let state = world.block_state(BlockPos::new(pos.x, pos.y + dy, pos.z));
        if !mc_bamboo_data::collision_shape(state.0).is_empty() {
            return false;
        }
    }
    true
}

/// `SpawnUtil.moveToPossibleSpawnPosition` 配 `ON_TOP_OF_COLLIDER`：
/// 从起点向下最多 `2 * RANGE_Y + 1` 格，找「脚下格顶面完整、上方格无碰撞」的位置。
///
/// `SpawnUtil.moveToPossibleSpawnPosition` with `ON_TOP_OF_COLLIDER`: walk down
/// at most `2 * RANGE_Y + 1` blocks looking for a full top face below an empty block.
fn move_to_possible_spawn_position(world: &World<'_>, pos: &mut BlockPos) -> bool {
    for _ in 0..=2 * RANGE_Y {
        *pos = pos.relative(Direction::Down);
        let stand = pos.relative(Direction::Up);
        let below = world.block_state(*pos);
        let above = world.block_state(stand);
        let above_empty = mc_bamboo_data::collision_shape(above.0).is_empty();
        let below_full =
            shape::is_face_full(mc_bamboo_data::collision_shape(below.0), Direction::Up);
        if above_empty && below_full {
            *pos = stand;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, testing::TestLevel};

    #[test]
    fn a_level_four_warning_summons_a_warden_on_stone() {
        let mut level = TestLevel::new();
        let stone = crate::BlockState(mc_bamboo_data::blocks::STONE);
        for x in -12..=12 {
            for z in -12..=12 {
                level.set(BlockPos::new(x, 63, z), stone);
            }
        }
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let origin = BlockPos::new(0, 64, 0);
        assert!(try_summon_warden(&mut world, origin, 3).is_none());
        let uid = try_summon_warden(&mut world, origin, 4).expect("a warden is summoned");
        let position = world.entity(uid).expect("warden in world").position;
        // 抬 6 格后向下找到石头顶面：站在 64 层。
        assert_eq!(position.y, 64.0);
        assert!((position.x - 0.5).abs() <= 5.0);
        assert!((position.z - 0.5).abs() <= 5.0);
    }

    #[test]
    fn no_ground_means_no_summon() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        assert!(
            try_summon_warden(&mut world, BlockPos::new(0, 64, 0), 4).is_none(),
            "an empty world has no full top face to stand on"
        );
    }
}
