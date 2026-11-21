//! 女巫投掷喷溅药水的逐刻对拍：覆盖远距缓慢 / 剧毒 / 伤害、近距虚弱 / 伤害与自溅免伤、
//! 以及范围溅射同时命中牛（受伤）和僵尸（亡灵反向回血）。逐刻比较女巫和喷溅药水的运动、
//! 随机源、女巫目标和血量、药水同步包、2002 / 2007 与 1053 / 1054 世界事件、玩家血量 /
//! 效果 / 出站包，以及周围生物的血量与效果。
//!
//! Tick-by-tick witch splash-potion replay: covers far slowness/poison/harming, near
//! weakness/harming plus self-splash immunity, and AoE hitting both a cow (damaged) and a
//! zombie (undead healed). Compares witch and potion motion and RNG state, witch goals and
//! health, potion sync packets, 2002/2007 and 1053/1054 level events, player health/effects/
//! packets, and nearby mob health/effects.

use std::collections::BTreeSet;

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::{projectile::motion, *};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitchThrowCase {
    name: String,
    first_id: i32,
    player_id: i32,
    player_x: f64,
    player_health: i32,
    seed: i64,
    with_extras: bool,
    blocks: Vec<[i32; 4]>,
    shots: usize,
    frames: Vec<WitchFrame>,
    player_states: Vec<[i64; 5]>,
    player_packets: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WitchFrame {
    potions: Vec<PotionEntry>,
    packets: Vec<String>,
    level_events: Vec<[i32; 5]>,
    witch: [i64; 8],
    witch_seed: i64,
    witch_health: i32,
    goals: String,
    player_effects: Vec<[i32; 3]>,
    extras: Vec<ExtraEntry>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct PotionEntry {
    id: i32,
    motion: [i64; 8],
    seed: i64,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ExtraEntry {
    id: i32,
    health: i32,
    effects: Vec<[i32; 3]>,
}

impl WitchThrowCase {
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
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: Some(ReplayPlayers::new(
                2,
                &[ReplayPlayerSetup {
                    id: self.player_id,
                    uuid: (1 << 64) | 2,
                    name: "WitchOracle".to_owned(),
                    position: Vec3::new(self.player_x, 64.0, 0.5),
                    health: f32::from_bits(self.player_health as u32),
                    seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                }],
            )),
            wrong_was_hurt: false,
        }
    }

    fn spawn_mobs(&self, world: &mut World<'_>) {
        let mut witch = world.prepare_entity(Entity::by_mob_type(
            MobType::Witch,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        witch.random = JavaRandom::new(self.seed);
        witch.y_rot = 0.0;
        let mob = witch.kind.mob_mut().unwrap();
        mob.uuid = 1_u128.to_be_bytes();
        mob.y_head_rot = 0.0;
        mob.y_body_rot = 0.0;
        world.add_prepared_entity(witch);

        if self.with_extras {
            let mut cow = world.prepare_entity(Entity::by_mob_type(
                MobType::Cow,
                Vec3::new(self.player_x, 64.0, 2.0),
            ));
            cow.random = JavaRandom::new(77);
            cow.y_rot = 0.0;
            let mob = cow.kind.mob_mut().unwrap();
            mob.uuid = 2_u128.to_be_bytes();
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.no_ai = true;
            world.add_prepared_entity(cow);

            let mut zombie = world.prepare_entity(Entity::by_mob_type(
                MobType::Zombie,
                Vec3::new(self.player_x, 64.0, -1.0),
            ));
            zombie.random = JavaRandom::new(88);
            zombie.y_rot = 0.0;
            let mob = zombie.kind.mob_mut().unwrap();
            mob.uuid = 3_u128.to_be_bytes();
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.health = 10.0;
            mob.no_ai = true;
            world.add_prepared_entity(zombie);
        }
    }
}

impl Report {
    pub(super) fn replay_witch_throw(&mut self, case: &WitchThrowCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        case.spawn_mobs(&mut World::new(&mut level, &mut ticks));
        let mut tracking = EntityTracking::default();
        let mut previous = BTreeSet::new();
        let mut shots = 0;
        let mut total_events = 0;
        let break_witch = self.break_test.as_deref() == Some("witch_throw");
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut level_events = Vec::new();
            if tick != 0 {
                level.game.as_mut().unwrap().tick_cooldowns();
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if break_witch && tick == 1 {
                    // 改坏确认：把玩家改成满血前先清掉女巫的目标能力（设为 no_ai），
                    // 确认夹具抓得到女巫没投掷 / 没触发世界事件。
                    // Break self-test: disable witch AI so no potions throw and checks fail.
                    world.with_entity_mut(case.first_id, &mut |entity| {
                        if let Some(mob) = entity.kind.mob_mut() {
                            mob.no_ai = true;
                        }
                    });
                }
                world.tick_entities();
                for effect in world.drain_effects() {
                    if let mc_bamboo_blocks::Effect::LevelEvent {
                        event, pos, data, ..
                    } = effect
                    {
                        level_events.push([event, pos.x, pos.y, pos.z, data]);
                    }
                }
            }
            total_events += level_events.len();
            let _ = level.entities.take_mob_events();
            let is_potion = |public: &mc_bamboo_blocks::entity::EntityPublic| {
                matches!(public.kind, KindPublic::SplashPotion(_))
            };
            let active: BTreeSet<_> = level
                .entities
                .ids()
                .into_iter()
                .filter(|&id| level.entities.public_at(id).is_some_and(is_potion))
                .collect();
            let mut packets = Vec::new();
            for &uid in previous.difference(&active) {
                packets.push(packet_body(&RemoveEntities { entity_ids: &[uid] }));
            }
            for public in level.entities.take_spawn_snapshots() {
                if !is_potion(&public) {
                    continue;
                }
                let snapshot = Snapshot::of(&public);
                packets.extend(
                    tracking
                        .tracked_for_oracle(&snapshot)
                        .pairing_packets(&snapshot),
                );
                shots += 1;
            }
            if tick != 0 {
                for &uid in &active {
                    let snapshot = Snapshot::of(level.entities.public_at(uid).unwrap());
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        snapshot.sync_velocity,
                    ));
                }
            }
            let label = format!("{} tick {tick}", case.name);
            let potions: Vec<_> = active
                .iter()
                .map(|&id| {
                    let entity = level.entities.get(id).unwrap();
                    let EntityKind::SplashPotion(data) = &entity.kind else {
                        unreachable!()
                    };
                    PotionEntry {
                        id,
                        motion: motion(&entity, data.x_rot),
                        seed: entity.random.state(),
                    }
                })
                .collect();
            self.check_potions(&label, &expected.potions, &potions);
            if expected.level_events != level_events {
                self.mismatch(format!(
                    "{label} level events: official {:?}, bamboo {level_events:?}",
                    expected.level_events
                ));
            }
            self.check_witch(
                &label,
                expected,
                &level.entities.get(case.first_id).unwrap(),
            );
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                &packets,
                &expected.packets,
            );
            let player = level.game.as_mut().unwrap();
            let state = player.state(case.player_id);
            if state != case.player_states[tick] {
                self.mismatch(format!(
                    "{label} player: official {:?}, bamboo {state:?}",
                    case.player_states[tick]
                ));
            }
            let player_effects = player.effects(case.player_id);
            if player_effects != expected.player_effects {
                self.mismatch(format!(
                    "{label} player effects: official {:?}, bamboo {player_effects:?}",
                    expected.player_effects
                ));
            }
            let player_packets: Vec<_> = player
                .take_packets(case.player_id)
                .into_iter()
                .filter(|body| matches!(body.first(), Some(43 | 67 | 68 | 69 | 106)))
                .collect();
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &format!("{} player", case.name),
                tick,
                &player_packets,
                &case.player_packets[tick],
            );
            let extras: Vec<_> = expected
                .extras
                .iter()
                .map(|entry| {
                    let entity = level.entities.get(entry.id).unwrap();
                    let mob = entity.kind.mob().unwrap();
                    ExtraEntry {
                        id: entry.id,
                        health: mob.health.to_bits() as i32,
                        effects: mob
                            .effects
                            .entries()
                            .iter()
                            .map(|effect| [effect.effect, effect.duration, effect.amplifier])
                            .collect(),
                    }
                })
                .collect();
            if extras != expected.extras {
                self.mismatch(format!(
                    "{label} extras: official {:?}, bamboo {extras:?}",
                    expected.extras
                ));
            }
            previous = active;
            self.steps += 1;
        }
        if shots != case.shots {
            self.mismatch(format!(
                "{} potions: official {}, bamboo {shots}",
                case.name, case.shots
            ));
        }
        let health = f32::from_bits(level.game.as_ref().unwrap().state(case.player_id)[0] as u32);
        let final_effects = level.game.as_ref().unwrap().effects(case.player_id);
        let extra_hp: Vec<f32> = case
            .frames
            .last()
            .map(|frame| {
                frame
                    .extras
                    .iter()
                    .map(|entry| f32::from_bits(entry.health as u32))
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "{}: {} ticks, {shots} potions, {total_events} level events, final player health {health}, effects {final_effects:?}, extras hp {extra_hp:?}",
            case.name,
            case.frames.len()
        );
    }

    fn check_potions(&mut self, label: &str, expected: &[PotionEntry], actual: &[PotionEntry]) {
        if expected.len() != actual.len() {
            self.mismatch(format!(
                "{label}: official {} potions, bamboo {}",
                expected.len(),
                actual.len()
            ));
        }
        for (theirs, ours) in expected.iter().zip(actual) {
            if (theirs.id, theirs.seed) != (ours.id, ours.seed) {
                self.mismatch(format!(
                    "{label} potion id/seed: official ({}, {}), bamboo ({}, {})",
                    theirs.id, theirs.seed, ours.id, ours.seed
                ));
            }
            self.check_projectile_motion(label, "potion", &theirs.motion, &ours.motion);
        }
    }

    fn check_witch(&mut self, label: &str, expected: &WitchFrame, witch: &Entity) {
        let mob = witch.kind.mob().unwrap();
        self.check_projectile_motion(label, "witch", &expected.witch, &motion(witch, mob.x_rot));
        let goals = mob
            .goals
            .running_names()
            .into_iter()
            .chain(mob.target_goals.running_names())
            .collect::<Vec<_>>()
            .join(" ");
        let ours = (witch.random.state(), mob.health.to_bits() as i32, goals);
        let expected_goals = expected.goals.replace(
            "NearestAttackableWitchTargetGoal",
            "NearestAttackableTargetGoal",
        );
        let theirs = (expected.witch_seed, expected.witch_health, expected_goals);
        if ours != theirs {
            self.mismatch(format!(
                "{label} witch seed/health/goals: official {theirs:?}, bamboo {ours:?}"
            ));
        }
    }
}
