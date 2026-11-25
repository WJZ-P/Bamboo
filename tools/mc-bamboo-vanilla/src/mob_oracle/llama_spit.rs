//! 羊驼吐口水的逐刻对拍：玩家打羊驼，羊驼回击吐口水，口水飞向玩家。
//! 比羊驼和口水的位置、速度、随机源，羊驼正在跑的目标、血量，口水的同步包和玩家状态。
//!
//! Tick-by-tick llama spit replay: the player hits the llama, the llama spits
//! back and the spit flies at the player. Compares llama and spit motion and
//! random state, the llama's running goals and health, the spit packets and
//! the player's state.

use std::collections::BTreeSet;

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::{projectile::motion, *};

/// 玩家打羊驼的两刻，和 Java 场景相同。
///
/// The two ticks on which the player hits the llama, as in the Java scene.
const HURT_TICKS: [usize; 2] = [5, 80];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LlamaSpitCase {
    name: String,
    first_id: i32,
    player_id: i32,
    player_x: f64,
    seed: i64,
    blocks: Vec<[i32; 4]>,
    shots: usize,
    frames: Vec<SpitFrame>,
    player_states: Vec<[i64; 5]>,
    player_packets: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpitFrame {
    spits: Vec<SpitEntry>,
    packets: Vec<String>,
    llama: [i64; 8],
    llama_seed: i64,
    llama_health: i32,
    goals: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct SpitEntry {
    id: i32,
    motion: [i64; 8],
    seed: i64,
}

impl LlamaSpitCase {
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
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: Some(ReplayPlayers::new(
                2,
                &[ReplayPlayerSetup {
                    id: self.player_id,
                    uuid: (1 << 64) | 2,
                    name: "ArrowOracle".to_owned(),
                    position: Vec3::new(self.player_x, 64.0, 0.5),
                    health: 20.0,
                    seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                    creative: false,
                }],
            )),
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }

    /// 和 Java 一样：先从岛的种子源取一个数，再换成场景的种子；朝向全是 0。
    ///
    /// As in Java: draw one seed from the island source, then use the scene seed;
    /// every rotation is 0.
    fn spawn_llama(&self, world: &mut World<'_>) {
        let mut llama = world.prepare_entity(Entity::by_mob_type(
            MobType::Llama,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        llama.random = JavaRandom::new(self.seed);
        llama.y_rot = 0.0;
        let mob = llama.kind.mob_mut().unwrap();
        mob.uuid = 1_u128.to_be_bytes();
        mob.y_head_rot = 0.0;
        mob.y_body_rot = 0.0;
        world.add_prepared_entity(llama);
    }
}

impl Report {
    pub(super) fn replay_llama_spit(&mut self, case: &LlamaSpitCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        case.spawn_llama(&mut World::new(&mut level, &mut ticks));
        let mut tracking = EntityTracking::default();
        let mut previous = BTreeSet::new();
        let mut shots = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                level.game.as_mut().unwrap().tick_cooldowns();
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if HURT_TICKS.contains(&tick) && self.break_test.as_deref() != Some("llama_spit") {
                    let at = Vec3::new(case.player_x, 64.0, 0.5);
                    world.hurt_mob(
                        case.first_id,
                        DamageSource::player(case.player_id, at, false),
                        1.0,
                    );
                }
                world.tick_entities();
                world.drain_effects();
            }
            let _ = level.entities.take_mob_events();
            let is_spit = |public: &mc_bamboo_blocks::entity::EntityPublic| {
                matches!(public.kind, KindPublic::LlamaSpit(_))
            };
            let active: BTreeSet<_> = level
                .entities
                .ids()
                .into_iter()
                .filter(|&id| level.entities.public_at(id).is_some_and(is_spit))
                .collect();
            let mut packets = Vec::new();
            for &uid in previous.difference(&active) {
                packets.push(packet_body(&RemoveEntities { entity_ids: &[uid] }));
            }
            for public in level.entities.take_spawn_snapshots() {
                if !is_spit(&public) {
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
            let spits: Vec<_> = active
                .iter()
                .map(|&id| {
                    let entity = level.entities.get(id).unwrap();
                    let EntityKind::LlamaSpit(data) = &entity.kind else {
                        unreachable!()
                    };
                    SpitEntry {
                        id,
                        motion: motion(&entity, data.pitch),
                        seed: entity.random.state(),
                    }
                })
                .collect();
            self.check_spits(&label, &expected.spits, &spits);
            self.check_llama(
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
            // `ServerPlayer.doTick` 在实体阶段之后按血量快照补 `set_health`。
            if tick != 0 {
                player.sync_health();
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
            previous = active;
            self.steps += 1;
        }
        if shots != case.shots {
            self.mismatch(format!(
                "{} spits: official {}, bamboo {shots}",
                case.name, case.shots
            ));
        }
        let health = f32::from_bits(level.game.as_ref().unwrap().state(case.player_id)[0] as u32);
        println!(
            "{}: {} ticks, {shots} spits, final player health {health}",
            case.name,
            case.frames.len()
        );
    }

    fn check_spits(&mut self, label: &str, expected: &[SpitEntry], actual: &[SpitEntry]) {
        if expected.len() != actual.len() {
            self.mismatch(format!(
                "{label}: official {} spits, bamboo {}",
                expected.len(),
                actual.len()
            ));
        }
        for (theirs, ours) in expected.iter().zip(actual) {
            if (theirs.id, theirs.seed) != (ours.id, ours.seed) {
                self.mismatch(format!(
                    "{label} spit id/seed: official ({}, {}), bamboo ({}, {})",
                    theirs.id, theirs.seed, ours.id, ours.seed
                ));
            }
            self.check_projectile_motion(label, "spit", &theirs.motion, &ours.motion);
        }
    }

    fn check_llama(&mut self, label: &str, expected: &SpitFrame, llama: &Entity) {
        let mob = llama.kind.mob().unwrap();
        self.check_projectile_motion(label, "llama", &expected.llama, &motion(llama, mob.x_rot));
        let goals = mob
            .goals
            .running_names(mc_bamboo_blocks::entity::mob::MobType::Llama)
            .into_iter()
            .chain(
                mob.target_goals
                    .running_names(mc_bamboo_blocks::entity::mob::MobType::Llama),
            )
            .collect::<Vec<_>>()
            .join(" ");
        let ours = (llama.random.state(), mob.health.to_bits() as i32, goals);
        let theirs = (
            expected.llama_seed,
            expected.llama_health,
            expected.goals.clone(),
        );
        if ours != theirs {
            self.mismatch(format!(
                "{label} llama seed/health/goals: official {theirs:?}, bamboo {ours:?}"
            ));
        }
    }
}
