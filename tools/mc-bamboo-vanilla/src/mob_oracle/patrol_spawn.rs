//! `PatrolSpawner` 场景：固定平地、一名玩家、固定种子，逐刻跑官方 `PatrolSpawner`
//! 和 Bamboo 的 [`patrol::tick`]，比较每次刷出来的巡逻队成员。
//!
//! PatrolSpawner scenes: flat ground, one player and a fixed seed; run vanilla's
//! `PatrolSpawner` and Bamboo's [`patrol::tick`] tick by tick and compare every spawned
//! patrol member.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LevelRandom, World,
    entity::{
        Entities, Entity,
        ctx::PlayerView,
        mob::patrol::{self, PatrolFacts, PatrolSpawner},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::packet::play::EquipmentSlot;
use serde::Deserialize;

use super::{MobLevel, Report, ZombieFacts};

/// Java 一侧发送的场景。`steps` 只含真的刷出成员的刻。
///
/// The scene sent by the Java side. `steps` holds only ticks that really spawned.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    name: String,
    seed: i64,
    ticks: i32,
    difficulty: i32,
    bright: bool,
    spawn_enemies: bool,
    spawn_patrols: bool,
    end_world_seed: i64,
    end_next_tick: i32,
    steps: Vec<Step>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    tick: i64,
    world_seed: i64,
    /// 每名成员：类型、x*2、y、z*2、朝向位、队长、主手、头盔、巡逻中、有巡逻目标、左撇子。
    ///
    /// Each member: type, x*2, y, z*2, yaw bits, leader, main hand, helmet, patrolling,
    /// has a patrol target, left-handed.
    spawned: Vec<[i32; 11]>,
}

#[derive(Default)]
pub(super) struct Coverage {
    pub(super) cases: usize,
    pub(super) ticks: usize,
    members: usize,
    leaders: usize,
    per_scene: std::collections::BTreeMap<String, usize>,
}

impl Coverage {
    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "PATROL SPAWN: {} cases, {} ticks, {} members ({} leaders)",
                self.cases, self.ticks, self.members, self.leaders
            );
            let scenes: Vec<String> = self
                .per_scene
                .iter()
                .map(|(name, count)| format!("{name}={count}"))
                .collect();
            println!("PATROL SPAWN per scene: {}", scenes.join(" "));
        }
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.cases == 0 {
            return Ok(());
        }
        if self.per_scene.get("patrol_day").copied().unwrap_or(0) == 0 {
            return Err("the patrol fixture did not spawn any members".to_owned());
        }
        Ok(())
    }
}

impl Report {
    pub(super) fn replay_patrol_spawn(&mut self, case: &Case) -> Result<(), String> {
        self.cases += 1;
        self.patrol_spawns.cases += 1;
        let mut level = fixture(case);
        let mut ticks = BlockTicks::new();
        ticks.set_time(0);
        let facts = PatrolFacts {
            spawn_enemies: case.spawn_enemies,
            spawn_patrols: case.spawn_patrols,
        };
        let mut spawner = PatrolSpawner::default();
        let mut ours = Vec::new();
        let mut end_world_seed = level.random.source().seed();
        for tick in 0..case.ticks {
            ticks.set_time(i64::from(tick));
            let present = level.entities.len();
            let mut world = World::new(&mut level, &mut ticks);
            patrol::tick(&mut world, &mut spawner, facts);
            end_world_seed = world.random().seed();
            let fresh: Vec<Entity> = level.entities.iter().skip(present).collect();
            if !fresh.is_empty() {
                ours.push(Step {
                    tick: i64::from(tick),
                    world_seed: end_world_seed,
                    spawned: fresh.iter().map(row).collect(),
                });
            }
        }
        if end_world_seed != case.end_world_seed || spawner.next_tick != case.end_next_tick {
            self.mismatch(format!(
                "{} seed {}: official ends at seed {} timer {}, Bamboo at seed {} timer {}",
                case.name,
                case.seed,
                case.end_world_seed,
                case.end_next_tick,
                end_world_seed,
                spawner.next_tick
            ));
        }
        self.steps += 1;
        self.patrol_spawns.ticks += usize::try_from(case.ticks).unwrap_or(0);
        for step in &ours {
            *self
                .patrol_spawns
                .per_scene
                .entry(case.name.clone())
                .or_default() += step.spawned.len();
            self.patrol_spawns.members += step.spawned.len();
            self.patrol_spawns.leaders +=
                step.spawned.iter().filter(|member| member[5] != 0).count();
        }
        if ours != case.steps {
            self.mismatch(format!(
                "{} seed {}: official {:?}, Bamboo {ours:?}",
                case.name, case.seed, case.steps
            ));
        }
        Ok(())
    }
}

/// 一名成员的比较行。
///
/// The comparison row of one member.
fn row(entity: &Entity) -> [i32; 11] {
    let mob = entity.kind.mob().expect("patrol members are mobs");
    [
        mob.mob_type.entity_type(),
        (entity.position.x * 2.0) as i32,
        entity.position.y as i32,
        (entity.position.z * 2.0) as i32,
        entity.y_rot.to_bits() as i32,
        i32::from(mob.raider.patrol_leader),
        mob.equipment.get(EquipmentSlot::MainHand).item,
        mob.equipment.get(EquipmentSlot::Head).item,
        i32::from(mob.raider.patrolling),
        i32::from(mob.raider.patrol_target.is_some()),
        i32::from(mob.left_handed),
    ]
}

/// 和 Java 夹具相同的平地、玩家和随机源。
///
/// The same flat ground, player and random source as the Java fixture.
fn fixture(case: &Case) -> MobLevel {
    let mut cells = std::collections::HashMap::new();
    for x in -64..64 {
        for z in -64..64 {
            cells.insert(BlockPos::new(x, -63, z), BlockState(blocks::GRASS_BLOCK));
        }
    }
    MobLevel {
        blocks: cells,
        random: LevelRandom::new(case.seed),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: case.difficulty,
            sky_darken: if case.bright { 0 } else { 11 },
            bright_outside: case.bright,
            clock: 0,
            sea_level: -63,
            ..ZombieFacts::default()
        },
        natural_spawn: None,
        biome: None,
        redstone: Default::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        game_event_listeners: Default::default(),
        warden_warnings: Default::default(),
        darkness_ticks: 0,
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: Entities::with_next_id(2),
        players: vec![player()],
        game: None,
        inhabited: Default::default(),
        mob_teams: Default::default(),
        ignore_bed_monsters: false,
        short_hunger: false,
        wrong_was_hurt: false,
        explosion_break: 0,
    }
}

fn player() -> PlayerView {
    PlayerView {
        shoulder_left: None,
        shoulder_right: None,
        uid: 1,
        yaw: 0.0,
        pitch: 0.0,
        uuid: None,
        position: Vec3::new(0.5, -62.0, 0.5),
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
    }
}
