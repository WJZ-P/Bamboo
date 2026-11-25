//! 监守者逐刻对拍：官方 `Warden` 与 Bamboo 的监守者在同一场景下逐刻比较姿态、
//! 愤怒、大脑记忆、坐标 / 速度 / 朝向、两种随机源、玩家血量和黑暗效果。
//!
//! Tick-by-tick comparison of the official `Warden` against Bamboo's: pose, anger,
//! brain memories, position / velocity / rotation, both random sources, player health
//! and the darkness effect.

use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WardenCase {
    name: String,
    first_id: i32,
    start_time: i64,
    emerge: bool,
    player: bool,
    anger: i32,
    attack_target: bool,
    erase_sonic: bool,
    erase_dig: bool,
    vibration: Option<Vec<i32>>,
    vibration2: Option<Vec<i32>>,
    #[serde(default)]
    summon: bool,
    sensors: Vec<i64>,
    mob_seed: i64,
    level_seed: i64,
    frames: Vec<WardenFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WardenFrame {
    pose: String,
    anger: i32,
    anger_level: String,
    brain: String,
    motion: [i64; 8],
    mob_seed: i64,
    level_seed: i64,
    player_health: i32,
    player_effects: Vec<i32>,
    removed: bool,
}

/// 场景初始状态：石头地面、世界随机源、一名可选的玩家（走真实受伤链路）。
///
/// Scene initial state: a stone floor, the level random and an optional player
/// (driven through the real hurt pipeline).
fn mob_level(case: &WardenCase) -> MobLevel {
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
        game: case.player.then(|| {
            ReplayPlayers::new(
                2,
                &[ReplayPlayerSetup {
                    id: case.first_id + 1,
                    uuid: (1 << 64) | 2,
                    name: "WardenOracle".to_owned(),
                    position: Vec3::new(2.5, 64.0, 0.5),
                    health: 20.0,
                    seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                    creative: false,
                }],
            )
        }),
        inhabited: HashMap::new(),
        mob_teams: HashMap::new(),
        wrong_was_hurt: false,
        short_hunger: false,
        ignore_bed_monsters: false,
        explosion_break: 0,
    }
}

fn pose_name(pose: mc_bamboo_blocks::entity::mob::warden::WardenPose) -> &'static str {
    use mc_bamboo_blocks::entity::mob::warden::WardenPose as P;
    match pose {
        P::Standing => "STANDING",
        P::Emerging => "EMERGING",
        P::Digging => "DIGGING",
        P::Roaring => "ROARING",
        P::Sniffing => "SNIFFING",
    }
}

fn anger_level_name(level: mc_bamboo_blocks::entity::mob::warden::AngerLevel) -> &'static str {
    use mc_bamboo_blocks::entity::mob::warden::AngerLevel as A;
    match level {
        A::Calm => "CALM",
        A::Agitated => "AGITATED",
        A::Angry => "ANGRY",
    }
}

impl Report {
    pub(super) fn replay_warden(&mut self, case: &WardenCase) {
        use mc_bamboo_blocks::{
            entity::{
                Entity, JavaRandom,
                mob::{
                    MobType,
                    brain::memory::{MemoryType, MemoryValue},
                    warden::WardenPose,
                },
            },
            game_event::{GameEvent, vibration::VibrationInfo},
        };
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        // 这个字段驱动 Java 的场景，Rust 侧由 `frames` 直接决定逐刻长度。
        let _ = case.level_seed;
        let broken = self.break_test.clone();
        let broken = broken.as_deref();
        let mut level = mob_level(case);
        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);

        let warden_id = {
            let mut world = World::new(&mut level, &mut ticks);
            let id = if case.summon {
                // `SpawnUtil.trySpawnMob(WARDEN, TRIGGERED, ..., ON_TOP_OF_COLLIDER)`。
                mc_bamboo_blocks::entity::mob::warden_spawn::try_summon_warden(
                    &mut world,
                    BlockPos::new(0, 64, 0),
                    4,
                )
                .expect("a warden is summoned")
            } else {
                let mut entity = Entity::by_mob_type(MobType::Warden, Vec3::new(0.5, 64.0, 0.5));
                entity.with_mob(|_, mob| {
                    if case.emerge {
                        mob.warden.pose = WardenPose::Emerging;
                    }
                    if let Some(brain) = mob.brain.as_mut() {
                        if case.emerge {
                            brain.memories.set_with_ttl(
                                MemoryType::IsEmerging,
                                MemoryValue::Unit,
                                134,
                            );
                            brain.memories.set_with_ttl(
                                MemoryType::DigCooldown,
                                MemoryValue::Unit,
                                1200,
                            );
                        }
                        if case.anger > 0 {
                            // `Warden.increaseAngerAt(player, anger, false)`：刷钻地冷却、加愤怒。
                            // `Warden.increaseAngerAt(player, anger, false)`: refresh the
                            // dig cooldown and add anger.
                            if brain.memories.has(MemoryType::DigCooldown) {
                                brain.memories.set_with_ttl(
                                    MemoryType::DigCooldown,
                                    MemoryValue::Unit,
                                    1200,
                                );
                            }
                            let player = case.first_id + 1;
                            mob.warden.anger.increase_anger(
                                player,
                                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
                                case.anger,
                                true,
                            );
                        }
                        if case.attack_target {
                            // `Warden.setAttackTarget`。
                            brain.memories.erase(MemoryType::RoarTarget);
                            brain.memories.set(
                                MemoryType::AttackTarget,
                                MemoryValue::Entity(case.first_id + 1),
                            );
                            brain.memories.erase(MemoryType::CantReachWalkTargetSince);
                            brain.memories.set_with_ttl(
                                MemoryType::SonicBoomCooldown,
                                MemoryValue::Unit,
                                200,
                            );
                        }
                        if case.erase_sonic {
                            brain.memories.erase(MemoryType::SonicBoomCooldown);
                        }
                        if case.erase_dig {
                            brain.memories.erase(MemoryType::DigCooldown);
                        }
                    }
                });
                world.spawn_entity(entity)
            };
            // 官方场景在生成后显式设 ID / UUID / 实体随机源 / 传感器延迟。
            // The official scene pins the id, uuid, entity random seed and sensor
            // delays after spawning.
            world.with_entity_mut(id, &mut |entity| {
                if !case.summon {
                    entity.y_rot = 0.0;
                    entity.old_position = Vec3::new(0.5, 64.0, 0.5);
                }
                entity.random = JavaRandom::from_state(case.mob_seed);
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
                // 官方场景直接调 `onReceiveVibration`，这里注入一个已到达的振动，
                // 让本刻的 `custom_server_ai_step` 立刻回调。
                // The official scene calls `onReceiveVibration` directly: inject an
                // already-arrived vibration so this tick's step invokes the callback.
                let arriving = [&case.vibration, &case.vibration2]
                    .into_iter()
                    .flatten()
                    .find(|vib| vib[0] == tick as i32);
                if let Some(vib) = arriving {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(warden_id, &mut |entity| {
                        entity.with_mob(|_, mob| {
                            if let Some(listener) = mob.entity_listener.as_mut() {
                                let player = case.first_id + 1;
                                listener.vibration.current = Some(VibrationInfo {
                                    event: GameEvent::Step,
                                    distance: 5.0,
                                    pos: Vec3::new(
                                        f64::from(vib[1]) + 0.5,
                                        f64::from(vib[2]) + 0.5,
                                        f64::from(vib[3]) + 0.5,
                                    ),
                                    source_player: (vib[4] == 1).then_some(player),
                                    source_entity: (vib[4] == 1).then_some(player),
                                    projectile_owner: (vib[5] == 1).then_some(player),
                                    source_uuid: None,
                                    projectile_owner_uuid: None,
                                });
                                listener.vibration.travel = 0;
                            }
                        });
                    });
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            // `brain::dump` 用 `MobLevel.players` 解析 `EntityTracker`；真实受伤链路
            // 的玩家状态在 `game` 里，这里每刻同步一份只读视图。
            if let Some(views) = level.game.as_ref().map(|game| game.views()) {
                level.players = views;
            }

            let Some(entity) = level.entities.get(warden_id) else {
                if !expected.removed {
                    self.mismatch(format!(
                        "{} tick {tick}: warden missing (official removed={})",
                        case.name, expected.removed
                    ));
                }
                self.steps += 1;
                if self.mismatches.len() >= self.show {
                    return;
                }
                continue;
            };
            let mob = entity.kind.mob().expect("warden is a mob");
            let mut pose = pose_name(mob.warden.pose).to_owned();
            let mut anger = mob.warden.client_anger_level;
            let anger_level = anger_level_name(mob.warden.anger.anger_level()).to_owned();
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
            let mut player_health = level
                .game
                .as_ref()
                .map_or(-1, |game| game.state(case.first_id + 1)[0] as i32);
            let mut player_effects: Vec<i32> = Vec::new();
            if level.darkness_ticks > 0 {
                player_effects.push(level.darkness_ticks);
            }
            if let Some(game) = &level.game {
                for effect in game.effects(case.first_id + 1) {
                    player_effects.push(effect[1]);
                }
            }
            player_effects.sort_unstable();
            let removed = entity.removed;

            if broken == Some("warden_pose") && tick == 3 {
                pose = "STANDING".to_owned();
            }
            if broken == Some("warden_anger") && tick == 3 {
                anger = anger.wrapping_add(1);
            }
            if broken == Some("warden_brain") && tick == 3 {
                brain.push('x');
            }
            if broken == Some("warden_motion") && tick == 3 {
                motion[0] = motion[0].wrapping_add(1);
            }
            if broken == Some("warden_health") && tick == 3 {
                player_health = player_health.wrapping_add(1);
            }
            if broken == Some("warden_darkness") && tick == 3 {
                player_effects.push(1);
                player_effects.sort_unstable();
            }

            if pose != expected.pose {
                self.mismatch(format!(
                    "{} tick {tick} pose: official {:?}, bamboo {:?}",
                    case.name, expected.pose, pose
                ));
            }
            if anger != expected.anger {
                self.mismatch(format!(
                    "{} tick {tick} anger: official {}, bamboo {anger}",
                    case.name, expected.anger
                ));
            }
            if anger_level != expected.anger_level {
                self.mismatch(format!(
                    "{} tick {tick} anger level: official {:?}, bamboo {:?}",
                    case.name, expected.anger_level, anger_level
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
            if player_health != expected.player_health {
                self.mismatch(format!(
                    "{} tick {tick} player health: official {}, bamboo {player_health}",
                    case.name, expected.player_health
                ));
            }
            if player_effects != expected.player_effects {
                self.mismatch(format!(
                    "{} tick {tick} player effects: official {:?}, bamboo {:?}",
                    case.name, expected.player_effects, player_effects
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

/// 监守者存档对拍：`anger` 与 `listener` 两个键照官方 codec 往返。
///
/// Warden save parity: round-trip the `anger` and `listener` keys against the
/// official codec.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WardenSaveCase {
    filled: bool,
    nbt: String,
    reloaded: String,
}

const WARDEN_SAVE_KEYS: &[&str] = &[
    "anger", "listener", "Health", "Pos", "Motion", "Rotation", "UUID",
];

impl Report {
    pub(super) fn replay_warden_save(&mut self, case: &WardenSaveCase) {
        use mc_bamboo_blocks::entity::{Entity, mob::MobType, mob::nbt};
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        let label = format!("warden save filled {}", case.filled);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::Warden, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        let broken = self.break_test.as_deref() == Some("warden_save_anger") && case.filled;
        for key in WARDEN_SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let mut actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            if broken && *key == "anger" {
                actual = None;
            }
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
    }
}
