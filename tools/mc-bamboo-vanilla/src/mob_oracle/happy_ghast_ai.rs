//! 快乐恶魂开 AI 的逐刻对拍：成年自由飘浮、幼年跟雪球、长大换控制 / 目标、
//! 静止超时和玩家骑乘。逐刻比位置、速度、朝向、年龄、尺寸、移动 / 导航目标、
//! 正在跑的目标、大脑快照、家、悬停状态和随机源。
//!
//! Tick-by-tick happy ghast AI replay: adult free floating, a baby following a
//! snowball, the control/goal switch when it grows up, hover timeout and a
//! player riding. Each tick compares position, velocity, rotation, age, size,
//! move/navigation targets, running goals, brain dump, home, hover state and RNG.

use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    ctx::PlayerView,
    mob::{MobType, happy_ghast},
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HappyGhastAiCase {
    name: String,
    first_id: i32,
    seed: i64,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    baby: bool,
    tempt: bool,
    snowball: i32,
    harness: i32,
    sensors: Vec<i64>,
    frames: Vec<HappyGhastAiFrame>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct HappyGhastAiFrame {
    /// `[x,y,z]`（f64 位）。
    pos: [i64; 3],
    /// `[dx,dy,dz]`（f64 位）。
    vel: [i64; 3],
    /// `[yRot,xRot,yHeadRot,yBodyRot]`（f32 位）。
    rot: [i32; 4],
    age: i32,
    dims: [i32; 3],
    /// `MoveControl` 的 `wantedX/Y/Z`（f64 位）。
    wanted: [i64; 3],
    /// `speedModifier`（f64 位）。
    speed: i64,
    moving: bool,
    goals: String,
    /// `[nextNodeIndex, x,y,z, ...]`，没有路径是 `[]`。
    path: Vec<i32>,
    brain: String,
    home: [i32; 4],
    has_home: bool,
    stays_still: bool,
    still_timeout: i32,
    passengers: usize,
    seed: i64,
    player_rot: [i32; 2],
}

impl HappyGhastAiCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                bright_outside: true,
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
            explosion_break: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: vec![oracle_player(
                self.first_id - 1024,
                Vec3::new(6.5, 72.0, 0.5),
                self.snowball,
                self.tempt,
            )],
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            shoulder: Default::default(),
            containers: Default::default(),
            container_open_counts: Default::default(),
        }
    }

    /// 和官方夹具一样的起点：显式种子、朝向、幼年 / 长大 / 悬停 / 挽具的起始状态。
    ///
    /// Starting state: explicit seed, facing, baby / growth / hover / harness.
    fn spawn(&self, world: &mut mc_bamboo_blocks::World<'_>, break_test: Option<&str>) -> i32 {
        let pos = Vec3::new(2.5, 72.0, 0.5);
        let id = world.spawn_entity(Entity::by_mob_type(MobType::HappyGhast, pos));
        world.with_entity_mut(id, &mut |entity| {
            entity.random = JavaRandom::new(self.seed);
            entity.y_rot = 0.0;
            entity.old_position = pos;
            entity.dimensions = (self.baby_dimensions().0, self.baby_dimensions().1);
            if let Some(mob) = entity.kind.mob_mut() {
                mob.uuid = 1_u128.to_be_bytes();
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                mob.persistence_required = true;
                mob.eye_height = self.baby_dimensions().2;
                if self.baby {
                    mob.age = -24_000;
                    // `setBaby(true)` 之后 `babyGhastSetup` 换寻路器、清目标。
                    // `setBaby(true)` runs `babyGhastSetup`: swap navigation, clear goals.
                    happy_ghast::age_boundary_reached(mob);
                } else {
                    // 官方构造出来的成年带着 `registerGoals()` 的三个目标。
                    // A freshly constructed adult carries `registerGoals`' three goals.
                    mob.goals = happy_ghast::goals();
                }
                if self.name == "happy_ghast_ai_grow" {
                    mob.age = -1;
                }
                if self.name == "happy_ghast_ai_still" {
                    // 官方夹具用反射直接写 `serverStillTimeout`，没走
                    // `setServerStillTimeout`，所以同步标志这一刻仍是假。
                    // The official fixture writes the field directly with
                    // reflection, so the synced flag stays false at tick 0.
                    mob.happy_ghast.still_timeout = 10;
                }
                if self.name == "happy_ghast_ai_ride" {
                    mob.equipment.set(
                        EquipmentSlot::Body,
                        mc_bamboo_protocol::item::ItemStack::new(self.harness, 1),
                    );
                }
                // 改坏确认 `happy_ghast_ai_speed`：飞行速度乘错，位置 / 速度立刻分叉。
                // Break check: a wrong flying speed diverges at once.
                if break_test == Some("happy_ghast_ai_speed") {
                    mob.attributes.set_base_for_oracle(
                        mc_bamboo_data::registries::attribute::FLYING_SPEED,
                        0.06,
                    );
                }
                if self.baby {
                    // 洗牌种子和传感器延迟照官方夹具；两边都从一个已存在的
                    // `WALK_TARGET` 起步，这样 `RunOne` 的飞行闲逛不触发。
                    // Shuffle seed and sensor delays follow the official fixture;
                    // both sides start with a WALK_TARGET so `RunOne`'s flying
                    // stroll does not trigger.
                    let state = entity.random.state();
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(state);
                        for (sensor, &delay) in brain.sensors.iter_mut().zip(&self.sensors) {
                            sensor.time_to_tick = delay;
                        }
                        brain.memories.set(
                            mc_bamboo_blocks::entity::mob::brain::memory::MemoryType::WalkTarget,
                            mc_bamboo_blocks::entity::mob::brain::memory::MemoryValue::Walk(
                                mc_bamboo_blocks::entity::mob::brain::memory::WalkTarget::to_point(
                                    Vec3::new(6.5, 72.0, 0.5),
                                    1.25,
                                    2,
                                ),
                            ),
                        );
                    }
                }
            }
        });
        id
    }

    fn baby_dimensions(&self) -> (f32, f32, f32) {
        MobType::HappyGhast.dimensions(self.baby)
    }
}

fn oracle_player(uid: i32, position: Vec3, item: i32, tempt: bool) -> PlayerView {
    PlayerView {
        uid,
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        position,
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(0.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
        game_mode: 0,
        mining_fatigue: None,
        sneaking: false,
        sprinting: false,
        swimming: false,
        flying: false,
        invisible: false,
        explosion_knockback_resistance: 0.0,
        armor_cover: 0.0,
        main_hand: if tempt { item } else { 0 },
        off_hand: 0,
        team: None,
        yaw: 0.0,
        pitch: 0.0,
        combat: Default::default(),
        effects: Default::default(),
        shoulder_left: None,
        shoulder_right: None,
    }
}

impl Report {
    pub(super) fn replay_happy_ghast_ai(&mut self, case: &HappyGhastAiCase) {
        self.cases += 1;
        let break_test = self.break_test.clone();
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);
        // 改坏确认 `happy_ghast_ai_grow`：长大时不换目标表。每个场景开头都重设，
        // 不会漏到别的场景。
        // Break check: keep the baby goal table when growing up. Reset per scene.
        mc_bamboo_blocks::entity::mob::happy_ghast::set_oracle_keep_baby_goals(
            break_test.as_deref() == Some("happy_ghast_ai_grow"),
        );
        let id = case.spawn(
            &mut World::new(&mut level, &mut ticks),
            break_test.as_deref(),
        );
        let _ = level.entities.take_mob_events();
        let _ = level.entities.take_spawn_snapshots();

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(case.start_time + tick as i64);
                if case.name == "happy_ghast_ai_ride" {
                    if tick == 2 {
                        let player_uid =
                            level.players.first().map(|player| player.uid).unwrap_or(0);
                        World::new(&mut level, &mut ticks).with_entity_mut(id, &mut |entity| {
                            if let Some(mob) = entity.kind.mob_mut() {
                                let was_vehicle = !mob.passengers.is_empty();
                                mob.passengers.push(player_uid);
                                happy_ghast::after_add_passenger(
                                    entity.position,
                                    mob,
                                    was_vehicle,
                                    false,
                                );
                            }
                        });
                    }
                    if tick > 2
                        && let Some(player) = level.players.first_mut()
                    {
                        player.yaw = 17.0;
                        player.pitch = -12.0;
                    }
                }
                World::new(&mut level, &mut ticks).tick_entities();
                World::new(&mut level, &mut ticks).drain_effects();
            }
            let _ = level.entities.take_mob_events();

            let label = format!("{} tick {tick}", case.name);
            let Some(ours) = snapshot(&level, id) else {
                self.mismatch(format!("{label}: happy ghast missing in bamboo"));
                continue;
            };
            if ours != *expected {
                self.mismatch(format!("{label}: official {expected:?}, bamboo {ours:?}"));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}

fn snapshot(level: &MobLevel, id: i32) -> Option<HappyGhastAiFrame> {
    let entity = level.entities.get(id)?;
    let mob = entity.kind.mob()?;
    let player = level.players.first()?;
    let mover = &mob.mover;
    let goals = mob.goals.running_names_of(mob).join(" ");
    let path = mob.navigation.path.as_ref().map_or_else(Vec::new, |path| {
        let mut values = vec![path.next_node_index as i32];
        for node in &path.nodes {
            values.extend([node.x, node.y, node.z]);
        }
        values
    });
    let home = mob.home.unwrap_or((BlockPos::new(0, 0, 0), -1));
    Some(HappyGhastAiFrame {
        pos: [
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
        ],
        vel: [
            entity.delta.x.to_bits() as i64,
            entity.delta.y.to_bits() as i64,
            entity.delta.z.to_bits() as i64,
        ],
        rot: [
            entity.y_rot.to_bits() as i32,
            mob.x_rot.to_bits() as i32,
            mob.y_head_rot.to_bits() as i32,
            mob.y_body_rot.to_bits() as i32,
        ],
        age: mob.age,
        dims: [
            entity.dimensions.0.to_bits() as i32,
            entity.dimensions.1.to_bits() as i32,
            mob.eye_height.to_bits() as i32,
        ],
        wanted: [
            mover.wanted_x.to_bits() as i64,
            mover.wanted_y.to_bits() as i64,
            mover.wanted_z.to_bits() as i64,
        ],
        speed: mover.speed_modifier.to_bits() as i64,
        moving: mover.has_wanted(),
        goals,
        path,
        brain: brain::dump(level, &entity),
        home: [home.0.x, home.0.y, home.0.z, home.1],
        has_home: mob.home.is_some(),
        stays_still: mob.happy_ghast.stays_still,
        still_timeout: mob.happy_ghast.still_timeout,
        passengers: mob.passengers.len(),
        seed: entity.random.state(),
        player_rot: [player.yaw.to_bits() as i32, player.pitch.to_bits() as i32],
    })
}
