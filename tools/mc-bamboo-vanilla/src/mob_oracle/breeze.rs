//! 旋风人逐刻对拍：官方 `Breeze` 与 Bamboo 的旋风人在同一场景下逐刻比较姿态、
//! 大脑记忆、坐标 / 速度 / 朝向、两种随机源。
//!
//! Tick-by-tick comparison of the official `Breeze` against Bamboo's: pose, brain
//! memories, position / velocity / rotation, and both random sources.

use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BreezeCase {
    name: String,
    first_id: i32,
    #[allow(dead_code)]
    start_time: i64,
    #[allow(dead_code)]
    ticks: i32,
    player: [f64; 3],
    sensors: Vec<i64>,
    mob_seed: i64,
    level_seed: i64,
    frames: Vec<BreezeFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BreezeFrame {
    pose: String,
    brain: String,
    motion: [i64; 8],
    mob_seed: i64,
    level_seed: i64,
    removed: bool,
}

fn pose_name(pose: mc_bamboo_blocks::entity::mob::breeze::Pose) -> &'static str {
    use mc_bamboo_blocks::entity::mob::breeze::Pose as P;
    match pose {
        P::Standing => "STANDING",
        P::Shooting => "SHOOTING",
        P::Inhaling => "INHALING",
        P::Sliding => "SLIDING",
        P::LongJumping => "LONG_JUMPING",
    }
}

fn mob_level(case: &BreezeCase) -> MobLevel {
    let stone = BlockState(mc_bamboo_data::blocks::STONE);
    let mut blocks = HashMap::new();
    for x in -15..=15 {
        for z in -15..=15 {
            blocks.insert(BlockPos::new(x, 63, z), stone);
        }
    }
    MobLevel {
        raid: None,
        blocks,
        random: LevelRandom::new(1),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: 2,
            ..ZombieFacts::default()
        },
        natural_spawn: None,
        biome: None,
        redstone: mc_bamboo_blocks::RedstoneMemory::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        game_event_listeners: Default::default(),
        warden_warnings: Default::default(),
        darkness_ticks: 0,
        poi: mc_bamboo_world::poi::PoiStore::new(),
        shoulder: Default::default(),
        entities: Entities::with_next_id(case.first_id),
        players: Vec::new(),
        game: Some(ReplayPlayers::new(
            2,
            &[ReplayPlayerSetup {
                id: case.first_id + 1,
                uuid: (1 << 64) | 2,
                name: "BreezeOracle".to_owned(),
                position: Vec3::new(case.player[0], case.player[1], case.player[2]),
                health: 20.0,
                seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                creative: false,
            }],
        )),
        inhabited: HashMap::new(),
        mob_teams: HashMap::new(),
        wrong_was_hurt: false,
        short_hunger: false,
        ignore_bed_monsters: false,
        explosion_break: 0,
        containers: Default::default(),
        container_open_counts: Default::default(),
    }
}

impl Report {
    pub(super) fn replay_breeze(&mut self, case: &BreezeCase) {
        use mc_bamboo_blocks::entity::{Entity, JavaRandom, mob::MobType};

        self.cases += 1;
        // 这些字段驱动 Java 的场景，Rust 侧由 `frames` 直接决定逐刻长度。
        let _ = case.level_seed;
        let broken = self.break_test.clone();
        let broken = broken.as_deref();
        mc_bamboo_blocks::entity::wind_charge::set_oracle_no_margin(
            broken == Some("breeze_margin"),
        );
        let mut level = mob_level(case);
        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);

        let breeze_id = {
            let mut world = World::new(&mut level, &mut ticks);
            let entity = Entity::by_mob_type(MobType::Breeze, Vec3::new(0.5, 64.0, 0.5));
            let id = world.spawn_entity(entity);
            // 官方场景在生成后显式设随机源、传感器延迟和洗牌种子；这里照做。
            world.with_entity_mut(id, &mut |entity| {
                entity.random = JavaRandom::from_state(case.mob_seed);
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(0.5, 64.0, 0.5);
                let yaw = entity.y_rot;
                entity.with_mob(|me, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                    mob.x_rot = 0.0;
                    mob.y_rot_o = yaw;
                    mob.x_rot_o = 0.0;
                    mob.y_head_rot = yaw;
                    mob.y_head_rot_o = yaw;
                    mob.y_body_rot = yaw;
                    mob.y_body_rot_o = yaw;
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(me.random.state());
                        for (index, sensor) in brain.sensors.iter_mut().enumerate() {
                            if let Some(&delay) = case.sensors.get(index) {
                                sensor.time_to_tick = delay;
                            }
                        }
                    }
                });
            });
            id
        };

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let now = case.start_time + tick as i64;
                ticks.set_time(now);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            // `brain::dump` 用 `MobLevel.players` 解析 `EntityTracker`；真实受伤链路
            // 的玩家状态在 `game` 里，这里每刻同步一份只读视图。
            if let Some(views) = level.game.as_ref().map(|game| game.views()) {
                level.players = views;
            }

            let Some(entity) = level.entities.get(breeze_id) else {
                if !expected.removed {
                    self.mismatch(format!(
                        "{} tick {tick}: breeze missing (official removed={})",
                        case.name, expected.removed
                    ));
                }
                self.steps += 1;
                if self.mismatches.len() >= self.show {
                    return;
                }
                continue;
            };
            let mob = entity.kind.mob().expect("breeze is a mob");
            let mut pose = pose_name(mob.breeze.pose).to_owned();
            let mut brain = super::brain::dump(&level, &entity);
            let (p, v) = (entity.position, entity.delta);
            let mut motion = [
                p.x.to_bits() as i64,
                p.y.to_bits() as i64,
                p.z.to_bits() as i64,
                v.x.to_bits() as i64,
                v.y.to_bits() as i64,
                v.z.to_bits() as i64,
                i64::from(entity.y_rot.to_bits() as i32),
                i64::from(mob.x_rot.to_bits() as i32),
            ];
            let mob_seed = entity.random.state();
            let level_seed = level.random.source().seed();
            let removed = entity.removed;

            if broken == Some("breeze_pose") && tick == 3 {
                pose = "STANDING".to_owned();
            }
            if broken == Some("breeze_brain") && tick == 3 {
                brain.push('x');
            }
            if broken == Some("breeze_motion") && tick == 3 {
                motion[0] = motion[0].wrapping_add(1);
            }

            if pose != expected.pose {
                self.mismatch(format!(
                    "{} tick {tick} pose: official {:?}, bamboo {:?}",
                    case.name, expected.pose, pose
                ));
            }
            if brain != expected.brain {
                self.mismatch(format!(
                    "{} tick {tick} brain: official {:?}, bamboo {:?}",
                    case.name, expected.brain, brain
                ));
            }
            if motion != expected.motion {
                self.mismatch(format!(
                    "{} tick {tick} motion: official {:?}, bamboo {:?}",
                    case.name, expected.motion, motion
                ));
            }
            if mob_seed != expected.mob_seed {
                self.mismatch(format!(
                    "{} tick {tick} mob seed: official {}, bamboo {mob_seed}",
                    case.name, expected.mob_seed
                ));
            }
            if level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level seed: official {}, bamboo {level_seed}",
                    case.name, expected.level_seed
                ));
            }
            if removed != expected.removed {
                self.mismatch(format!(
                    "{} tick {tick} removed: official {}, bamboo {removed}",
                    case.name, expected.removed
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
