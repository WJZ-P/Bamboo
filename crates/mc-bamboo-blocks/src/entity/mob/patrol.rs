//! `PatrolSpawner`（官方 `net.minecraft.world.level.levelgen.PatrolSpawner`）。
//!
//! 每隔 `12000 + nextInt(1200)` 刻、白天、五分之一概率、离随机玩家 24–47 格处，
//! 按区域难度刷一支掠夺者巡逻队：队长带灾厄旗帜、随机巡逻目标，其余是跟随者。
//! 假世界没有村庄数据，[`tick`] 按任务要求跳过 `isCloseToVillage`，见文档。
//!
//! `PatrolSpawner` (vanilla `net.minecraft.world.level.levelgen.PatrolSpawner`):
//! every `12000 + nextInt(1200)` ticks, in daylight, one chance in five, 24–47
//! blocks from a random player, spawn a patrol sized by the regional difficulty —
//! a leader with the ominous banner and a random patrol target, plus followers.
//! Test worlds have no villages, so [`tick`] skips `isCloseToVillage` as required.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::registries::biome;

use super::{MobType, natural, pillager, raider, spawn};
use crate::{
    World,
    entity::{Entity, ctx::BlockView},
};

/// `PatrolSpawner.nextTick`，开局是 0（第一刻就重排计时）。
///
/// `PatrolSpawner.nextTick`, 0 at world start (the first tick re-arms the timer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatrolSpawner {
    pub next_tick: i32,
}

/// 官方 `tick(level, spawnEnemies)` 里从游戏规则读出的两个布尔量。
/// 其余事实（白天、区域难度、玩家、群系、区块加载）都从 [`Level`](crate::Level) 取。
///
/// The two game-rule booleans of vanilla `tick(level, spawnEnemies)`. All other
/// facts (daylight, regional difficulty, players, biome, chunk loading) come from
/// [`Level`](crate::Level).
#[derive(Debug, Clone, Copy)]
pub struct PatrolFacts {
    /// `spawn_monsters`（传入 `tickCustomSpawners` 的 `spawnEnemies`）。
    ///
    /// `spawn_monsters` (the `spawnEnemies` passed to `tickCustomSpawners`).
    pub spawn_enemies: bool,
    /// `GameRules.SPAWN_PATROLS`。
    ///
    /// `GameRules.SPAWN_PATROLS`.
    pub spawn_patrols: bool,
}

/// `hasChunksAt(spawnPos ± 10)` 的方块半径。
const CHUNK_MARGIN: i32 = 10;
/// `EARLY_GAME` 时间线：主世界时钟到这一刻才允许巡逻队生成（约开服前 5 天）。
///
/// The `EARLY_GAME` timeline: patrols are allowed only from this overworld-clock
/// tick on (roughly the first five in-game days).
pub const EARLY_GAME_END: i64 = 120000;

/// `PatrolSpawner.tick(level, spawnEnemies)`。
///
/// 取数顺序照官方：`nextInt(1200)` 重排计时、`nextInt(5)` 定这一次是否尝试、
/// `nextInt(playerCount)` 选玩家、`x/z` 各两次（`nextInt(24)` 再 `nextBoolean()`），
/// 每个成员落地之后 `nextInt(5) - nextInt(5)` 平移，队长另外用实体随机源取两次
/// `nextInt(1000)` 找巡逻目标，`finalizeSpawn` 用世界随机源。`isCloseToVillage`
/// 被跳过（Bamboo 没有村庄），位置由 [`crate::Level::can_pillager_patrol_spawn`]
/// 和 [`crate::Level::patrol_chunks_loaded`] 负责。
///
/// Vanilla `PatrolSpawner.tick(level, spawnEnemies)`. Draws follow the official
/// order: `nextInt(1200)` re-arms the timer, `nextInt(5)` decides the attempt,
/// `nextInt(playerCount)` picks a player, two draws per axis (`nextInt(24)` then
/// `nextBoolean()`), a `nextInt(5) - nextInt(5)` shift after each member, two
/// `nextInt(1000)` from the entity random for the leader's patrol target, and
/// `finalizeSpawn` from the world random. `isCloseToVillage` is skipped (Bamboo
/// has no villages); the position gates live in
/// [`crate::Level::can_pillager_patrol_spawn`] and
/// [`crate::Level::patrol_chunks_loaded`].
pub fn tick(world: &mut World, spawner: &mut PatrolSpawner, facts: PatrolFacts) {
    if !facts.spawn_enemies || !facts.spawn_patrols {
        return;
    }
    spawner.next_tick -= 1;
    if spawner.next_tick > 0 {
        return;
    }
    // `world.random()` 返回带内部可变性的 `&LegacyRandom`，但它是绑在 `&World` 上的；
    // 每次取数现取现用，免得这个不可变借用跨过下面 `spawn_member(&mut world)`。
    // `world.random()` returns a `&LegacyRandom` tied to `&World`; fetch it per draw
    // so no immutable borrow survives into `spawn_member(&mut world)` below.
    spawner.next_tick += 12000 + world.random().next_int(1200);
    if !world.level().is_bright_outside() {
        return;
    }
    if world.random().next_int(5) != 0 {
        return;
    }
    // `level.players()`：全部玩家都算进 playerCount，旁观者被选中后跳过。
    // `level.players()`: every player counts toward playerCount; a picked
    // spectator is skipped afterwards.
    let mut players: Vec<(Vec3, bool)> = Vec::new();
    BlockView::for_each_player(world, &mut |player| {
        players.push((player.position, player.spectator));
    });
    if players.is_empty() {
        return;
    }
    let index = world.random().next_int(players.len() as i32) as usize;
    let (position, spectator) = players[index];
    if spectator {
        return;
    }
    // `isCloseToVillage(player.blockPosition(), 2)`：Bamboo 没有村庄检测，按任务跳过。
    // `isCloseToVillage(player.blockPosition(), 2)`: no village detection in Bamboo,
    // skipped as the task requires.
    let base = BlockPos::containing(position);
    let x = (24 + world.random().next_int(24)) * sign(world.random().next_boolean());
    let z = (24 + world.random().next_int(24)) * sign(world.random().next_boolean());
    let mut pos = BlockPos::new(base.x + x, base.y, base.z + z);
    if !world.level().patrol_chunks_loaded(pos, CHUNK_MARGIN) {
        return;
    }
    if !world.level().can_pillager_patrol_spawn(pos) {
        return;
    }
    let effective = BlockView::effective_difficulty(world, pos);
    let group_size = effective.ceil() as i32 + 1;
    for index in 0..group_size {
        pos = with_no_leaves_height(world, pos);
        let leader = index == 0;
        if leader && !spawn_member(world, pos, true) {
            break;
        }
        if !leader {
            spawn_member(world, pos, false);
        }
        // `spawnPos.move(nextInt(5) - nextInt(5), 0, nextInt(5) - nextInt(5))`：
        // 下一名成员平移，队长失败 break 之前不取这两个数。
        // `spawnPos.move(nextInt(5) - nextInt(5), 0, nextInt(5) - nextInt(5))`:
        // shift for the next member; not drawn when the leader breaks out.
        pos = BlockPos::new(
            pos.x + world.random().next_int(5) - world.random().next_int(5),
            pos.y,
            pos.z + world.random().next_int(5) - world.random().next_int(5),
        );
    }
}

/// `(nextBoolean() ? -1 : 1)`。
const fn sign(negative: bool) -> i32 {
    if negative { -1 } else { 1 }
}

/// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`，复用巡逻目标的高度图。
///
/// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`, reusing the patrol
/// goal's heightmap helper.
fn with_no_leaves_height(world: &World, pos: BlockPos) -> BlockPos {
    BlockPos::new(
        pos.x,
        raider::no_leaves_height(
            |at| world.block_state(at),
            world.min_y(),
            world.max_y(),
            pos.x,
            pos.z,
        ),
        pos.z,
    )
}

/// `PatrolSpawner.spawnPatrolMember(level, pos, random, isLeader)`。
///
/// 先 `NaturalSpawner.isValidEmptySpawnBlock`，再
/// `PatrollingMonster.checkPatrollingMonsterSpawnRules`（方块光 ≤ 8 且脚下能站）。
/// 队长在 `finalizeSpawn` 之前设队长标志并 `findPatrolTarget`（实体随机源两次
/// `nextInt(1000)`）；`setPos` 用整数坐标，`finalizeSpawn` 用 `PATROL` 原因。
///
/// Vanilla `PatrolSpawner.spawnPatrolMember(level, pos, random, isLeader)`:
/// `NaturalSpawner.isValidEmptySpawnBlock` then
/// `PatrollingMonster.checkPatrollingMonsterSpawnRules` (block light ≤ 8 and a
/// valid footing). The leader is flagged and `findPatrolTarget` (two `nextInt(1000)`
/// from the entity random) before `finalizeSpawn`; `setPos` uses integer
/// coordinates and `finalizeSpawn` uses the `PATROL` reason.
fn spawn_member(world: &mut World, pos: BlockPos, leader: bool) -> bool {
    if !natural::patrol_spawn_block_ok(world, pos) {
        return false;
    }
    if !pillager::check_spawn_rules(world, pos) {
        return false;
    }
    let mut entity = world.prepare_entity(Entity::by_mob_type(MobType::Pillager, Vec3::default()));
    if leader {
        entity.with_mob(|me, mob| {
            mob.raider.patrol_leader = true;
            mob.raider.patrolling = true;
            raider::find_patrol_target(me, mob);
        });
    }
    spawn::snap_to(
        &mut entity,
        Vec3::new(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)),
        0.0,
        0.0,
    );
    spawn::finalize_spawn(
        world,
        MobType::Pillager,
        &mut entity,
        spawn::SpawnReason::Patrol,
    );
    world.add_prepared_entity(entity);
    true
}

/// `can_pillager_patrol_spawn` 的默认实现里用到的两个事实（供真实世界调用）：
/// 蘑菇岛群系关掉巡逻队，且主世界时钟要过了 [`EARLY_GAME_END`]。
///
/// The two facts behind the real `can_pillager_patrol_spawn` (for the game level):
/// mushroom fields disable patrols and the overworld clock must pass
/// [`EARLY_GAME_END`].
pub const fn can_patrol_spawn_at(clock_ticks: i64, biome_id: i32) -> bool {
    clock_ticks >= EARLY_GAME_END && biome_id != biome::MUSHROOM_FIELDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockState, BlockTicks, testing::TestLevel};
    use mc_bamboo_data::{blocks, items, registries::entity_type};
    use mc_bamboo_protocol::packet::play::EquipmentSlot;

    /// 与官方一样的平地：y=63 草、y=64 空气，开阔白天、一个玩家在原点。
    ///
    /// A flat world like vanilla's: grass at y=63, air at y=64, open daylight,
    /// one player at the origin.
    fn level(seed: i64) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        for x in -64..64 {
            for z in -64..64 {
                level.set(BlockPos::new(x, 62, z), BlockState(blocks::DIRT));
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK));
            }
        }
        level.light = Some((15, 0));
        level.random = crate::LevelRandom::new(seed);
        level.players.push(crate::entity::ctx::PlayerView {
            main_arm_right: true,
            fall_flying: false,
            shoulder_left: None,
            shoulder_right: None,
            uid: 1,
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            position: Vec3::new(0.5, 64.0, 0.5),
            delta: Vec3::default(),
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::default(),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            combat: Default::default(),
            effects: Default::default(),
        });
        (level, BlockTicks::new())
    }

    const F: PatrolFacts = PatrolFacts {
        spawn_enemies: true,
        spawn_patrols: true,
    };

    /// 用某个种子跑一刻，返回（关卡、计时器）。世界随机源是 java.util.Random，
    /// 遍历找能通过 `nextInt(5)==0` 的种子。
    ///
    /// Run one tick with a seed and return (level, spawner). The world random is
    /// java.util.Random, so scan seeds for one passing `nextInt(5)==0`.
    fn run(seed: i64, facts: PatrolFacts, dark: bool) -> (TestLevel, PatrolSpawner) {
        let (mut level, mut ticks) = level(seed);
        if dark {
            level.sky_darken = 4;
        }
        let mut spawner = PatrolSpawner::default();
        {
            let mut world = World::new(&mut level, &mut ticks);
            tick(&mut world, &mut spawner, facts);
        }
        (level, spawner)
    }

    /// 队伍：一名戴灾厄旗帜、有巡逻目标的队长，其余拿弩的跟随者，全是掠夺者。
    #[test]
    fn a_patrol_is_a_leader_with_followers() {
        let mut chosen = None;
        for seed in 1..2000_i64 {
            if !run(seed, F, false).0.entities().is_empty() {
                chosen = Some(seed);
                break;
            }
        }
        let seed = chosen.expect("some seed must pass the 1-in-5 gate");
        let (spawned_level, _) = run(seed, F, false);
        let entities = spawned_level.entities();
        // 组大小由 `effective_difficulty` 决定；区域难度至少 2，所以至少 3 只。
        // The group size follows `effective_difficulty`; regional difficulty is
        // at least 2, so at least three members.
        let (mut check, mut ticks) = level(seed);
        let expected = {
            let world = World::new(&mut check, &mut ticks);
            BlockView::effective_difficulty(&world, BlockPos::new(0, 64, 0)).ceil() as i32 + 1
        };
        assert_eq!(entities.len(), expected as usize);
        let mut leaders = 0;
        for entity in entities.iter() {
            let mob = entity.kind.mob().expect("patrol members are pillagers");
            assert_eq!(mob.mob_type.entity_type(), entity_type::PILLAGER);
            assert_eq!(
                mob.equipment.get(EquipmentSlot::MainHand).item,
                items::CROSSBOW
            );
            assert!(mob.raider.patrolling);
            assert!(mob.raider.can_join_raid);
            if mob.raider.patrol_leader {
                leaders += 1;
                assert_eq!(
                    mob.equipment.get(EquipmentSlot::Head).item,
                    items::WHITE_BANNER,
                    "leader wears the ominous banner"
                );
                assert!(mob.raider.patrol_target.is_some());
            } else {
                assert!(mob.raider.patrol_target.is_none());
            }
        }
        assert_eq!(leaders, 1, "exactly one leader");
    }

    /// 关掉 `spawn_enemies` / `spawn_patrols` 时整刻不刷、不取随机数、计时器不动。
    #[test]
    fn gated_ticks_spawn_nothing_and_draw_nothing() {
        for facts in [
            PatrolFacts {
                spawn_enemies: false,
                spawn_patrols: true,
            },
            PatrolFacts {
                spawn_enemies: true,
                spawn_patrols: false,
            },
        ] {
            let (level, spawner) = run(7, facts, false);
            assert_eq!(level.entities().len(), 0);
            assert_eq!(
                level.random.source().seed(),
                crate::LevelRandom::new(7).source().seed()
            );
            assert_eq!(spawner.next_tick, 0);
        }
    }

    /// 天黑时只在重排计时那一支取 `nextInt(1200)`，之后整段跳过。
    #[test]
    fn dark_outside_still_draws_the_timer() {
        let (level, spawner) = run(7, F, true);
        assert_eq!(level.entities().len(), 0);
        assert_ne!(
            level.random.source().seed(),
            crate::LevelRandom::new(7).source().seed(),
            "the timer re-arm draws nextInt(1200) before the daylight gate"
        );
        assert!((11999..=13199).contains(&spawner.next_tick));
    }

    /// 计时器第一刻重排成 `12000 + nextInt(1200) - 1`，之后每刻只减一。
    #[test]
    fn the_timer_counts_down_between_attempts() {
        let (_, mut spawner) = run(3, F, false);
        let armed = spawner.next_tick;
        assert!(
            (11999..=13199).contains(&armed),
            "12000 + r - 1, got {armed}"
        );
        let (mut level, mut ticks) = level(3);
        let before = level.random.source().seed();
        {
            let mut world = World::new(&mut level, &mut ticks);
            tick(&mut world, &mut spawner, F);
        }
        assert_eq!(spawner.next_tick, armed - 1);
        assert_eq!(
            level.random.source().seed(),
            before,
            "a plain countdown draws nothing"
        );
    }

    /// 早期世界时间线和蘑菇岛都关掉巡逻队。
    #[test]
    fn early_game_and_mushroom_fields_disable_patrols() {
        assert!(!can_patrol_spawn_at(0, biome::PLAINS));
        assert!(!can_patrol_spawn_at(EARLY_GAME_END - 1, biome::PLAINS));
        assert!(can_patrol_spawn_at(EARLY_GAME_END, biome::PLAINS));
        assert!(!can_patrol_spawn_at(
            EARLY_GAME_END + 1,
            biome::MUSHROOM_FIELDS
        ));
    }

    /// `nextTick` 为正时整段跳过，连玩家表都不看。
    #[test]
    fn a_positive_timer_skips_everything() {
        let (mut level, mut ticks) = level(9);
        let before = level.random.source().seed();
        let mut spawner = PatrolSpawner { next_tick: 100 };
        {
            let mut world = World::new(&mut level, &mut ticks);
            tick(&mut world, &mut spawner, F);
        }
        assert_eq!(spawner.next_tick, 99);
        assert_eq!(level.random.source().seed(), before);
        assert_eq!(level.entities().len(), 0);
    }
}
