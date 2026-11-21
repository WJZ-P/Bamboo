use std::collections::BTreeSet;

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};
use mc_bamboo_protocol::{
    frame::packet_body,
    item::ItemStack,
    packet::play::{EquipmentSlot, RemoveEntities},
};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArrowPickupCase {
    count: i32,
    pickup: u8,
    grounded: bool,
    shake: u8,
    creative: bool,
    full_inventory: i32,
    removed: bool,
    arrows: i32,
    remaining: i32,
}

impl Report {
    pub(super) fn replay_arrow_pickup(&mut self, case: &ArrowPickupCase) {
        use mc_bamboo_blocks::entity::arrow::Pickup;
        self.cases += 1;
        let pickup = match case.pickup {
            0 => Pickup::Disallowed,
            1 => Pickup::Allowed,
            2 => Pickup::CreativeOnly,
            value => panic!("unrecognized pickup mode {value}"),
        };
        let actual = mc_bamboo_game::replay_arrow_pickup(
            case.count,
            pickup,
            case.grounded,
            case.shake,
            case.creative,
            (case.full_inventory >= 0).then_some(case.full_inventory),
        );
        let expected = (case.removed, case.arrows, case.remaining);
        if actual != expected {
            self.mismatch(format!(
                "arrow pickup {case:?}: official {expected:?}, bamboo {actual:?}"
            ));
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProjectileCase {
    name: String,
    difficulty: i32,
    wall: bool,
    skeleton: bool,
    first_id: i32,
    player_id: i32,
    player_x: f64,
    blocks: Vec<[i32; 4]>,
    frames: Vec<ProjectileFrame>,
    player_states: Vec<[i64; 5]>,
    player_packets: Vec<Vec<String>>,
    shots: usize,
    embeds: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectileFrame {
    arrows: Vec<ArrowFrame>,
    packets: Vec<String>,
    shooter: Option<[i64; 8]>,
    shooter_seed: Option<i64>,
    aggressive: Option<bool>,
    drawing: Option<bool>,
    player_thermal: Option<[i32; 2]>,
    targets: Option<[i32; 2]>,
    /// 本刻的熄火音效：x/y/z 的原始位、音量、音高、声源序号。
    ///
    /// Extinguish sounds this tick: raw x/y/z bits, volume, pitch, and source ordinal.
    extinguished: Option<Vec<[i64; 6]>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ArrowFrame {
    id: i32,
    motion: [i64; 8],
    ground: bool,
    shake: u8,
    seed: i64,
    fire: Option<i32>,
}

impl ProjectileCase {
    fn flaming(&self) -> bool {
        self.name.starts_with("arrow_flame_")
    }

    fn piercing(&self) -> bool {
        self.name.starts_with("arrow_piercing_")
    }

    fn overlaps_flight(&self) -> bool {
        self.name == "arrow_block_overlaps_flight"
    }

    fn starts_without_gravity(&self) -> bool {
        self.name == "arrow_no_gravity"
    }

    fn water(&self) -> bool {
        self.name.starts_with("arrow_water_")
    }

    fn water_tail(&self) -> bool {
        self.name == "arrow_water_after_pierce"
    }

    fn level(&self) -> MobLevel {
        MobLevel {
            natural_spawn: None,
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
                difficulty: self.difficulty,
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: Some(ReplayPlayers::new(
                self.difficulty,
                &[ReplayPlayerSetup {
                    id: self.player_id,
                    uuid: (1 << 64) | 2,
                    name: "ArrowOracle".to_owned(),
                    position: Vec3::new(self.player_x, 64.0, 0.5),
                    health: 20.0,
                    seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                }],
            )),
            wrong_was_hurt: false,
        }
    }

    fn spawn(&self, world: &mut World<'_>, mutate: bool, break_pierce: bool) {
        if self.skeleton {
            let mut skeleton = world.prepare_entity(Entity::by_mob_type(
                MobType::Skeleton,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            skeleton.random = JavaRandom::new(42);
            skeleton.y_rot = 0.0;
            let mob = skeleton.kind.mob_mut().unwrap();
            mob.uuid = 1_u128.to_be_bytes();
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            if !mutate {
                mob.equipment.set(
                    EquipmentSlot::MainHand,
                    ItemStack::new(mc_bamboo_data::items::BOW, 1),
                );
            }
            world.add_prepared_entity(skeleton);
        } else {
            let y = if self.water() {
                65.25
            } else if self.piercing() {
                65.0
            } else {
                65.5
            };
            let mut arrow = world.prepare_entity(Entity::arrow(Vec3::new(0.5, y, 0.5)));
            if let EntityKind::Arrow(data) = &mut arrow.kind {
                data.no_gravity = self.starts_without_gravity();
                if self.piercing() || self.water_tail() {
                    data.pierce = i8::from(!break_pierce);
                    data.base_damage = 0.5;
                }
            }
            if !self.overlaps_flight() {
                let power = if self.water() {
                    7.0
                } else if self.piercing() {
                    6.0
                } else {
                    1.6
                };
                arrow.shoot_arrow(
                    Vec3::new(1.0, if mutate { 0.25 } else { 0.0 }, 0.0),
                    power,
                    0.0,
                );
            }
            world.add_prepared_entity(arrow);
            if self.piercing() {
                for x in [3.5, 5.5] {
                    let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5));
                    target.with_mob(|_, mob| mob.no_ai = true);
                    world.spawn_entity(target);
                }
            }
        }
    }
}

impl Report {
    pub(super) fn replay_projectile(&mut self, case: &ProjectileCase) {
        self.cases += 1;
        let mut level = case.level();
        if self.break_test.as_deref() == Some("arrow_water") {
            // 改坏确认：拿掉水面，熄火音效和火焰刻都应该对不上。
            // Break self-test: drop the water so the extinguish sound and fire ticks must differ.
            level.blocks.remove(&BlockPos::new(3, 65, 0));
            level.blocks.remove(&BlockPos::new(4, 65, 0));
        }
        level.game.as_mut().unwrap().set_arrow_target(
            case.player_id,
            case.name.ends_with("_rejected"),
            case.flaming().then_some([37, 80]),
        );
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        case.spawn(
            &mut World::new(&mut level, &mut ticks),
            self.break_test.as_deref() == Some("projectile"),
            self.break_test.as_deref() == Some("arrow_piercing"),
        );
        let mut tracking = EntityTracking::default();
        let mut previous = BTreeSet::new();
        let mut shots = 0;
        let mut embeds = 0;
        let mut packet_count = 0;
        let extinguish_sound = i32::from(mc_bamboo_data::sounds::sound_event(
            "minecraft:entity.generic.extinguish_fire",
        ));
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut extinguished = Vec::new();
            if tick != 0 {
                if (case.flaming() || case.water()) && tick == 1 {
                    level
                        .entities
                        .with_mut(case.first_id, |arrow| arrow.remaining_fire_ticks = 200);
                }
                if case.wall && tick == 12 {
                    level.blocks.remove(&BlockPos::new(7, 65, 0));
                }
                if case.overlaps_flight() && tick == 2 {
                    level.blocks.insert(
                        BlockPos::new(0, 65, 0),
                        BlockState(mc_bamboo_data::blocks::STONE),
                    );
                }
                if case.starts_without_gravity() && tick == 10 {
                    level.entities.with_mut(case.first_id, |entity| {
                        if let EntityKind::Arrow(data) = &mut entity.kind {
                            data.no_gravity = false;
                        }
                    });
                }
                if case.name == "arrow_despawn" && tick == 6 {
                    level.entities.with_mut(case.first_id, |entity| {
                        if let EntityKind::Arrow(data) = &mut entity.kind {
                            data.life = 1198;
                        }
                    });
                }
                level.game.as_mut().unwrap().tick_cooldowns();
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                extinguished.extend(world.drain_effects().iter().filter_map(|effect| {
                    let mc_bamboo_blocks::Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        pitch,
                        ..
                    } = effect
                    else {
                        return None;
                    };
                    (*sound == extinguish_sound).then(|| {
                        [
                            x.to_bits() as i64,
                            y.to_bits() as i64,
                            z.to_bits() as i64,
                            i64::from(volume.to_bits()),
                            i64::from(pitch.to_bits()),
                            i64::from(source.id()),
                        ]
                    })
                }));
            }
            if let Some(thermal) = expected.player_thermal {
                let actual = level
                    .game
                    .as_ref()
                    .unwrap()
                    .arrow_target_thermal(case.player_id);
                if actual != thermal {
                    self.mismatch(format!("{} tick {tick} player fire/frozen: official {thermal:?}, bamboo {actual:?}", case.name));
                }
            }
            if let Some(targets) = expected.targets {
                let actual = [case.first_id + 1, case.first_id + 2].map(|id| {
                    level
                        .entities
                        .get(id)
                        .unwrap()
                        .kind
                        .mob()
                        .unwrap()
                        .health
                        .to_bits() as i32
                });
                if actual != targets {
                    self.mismatch(format!("{} tick {tick} target health bits: official {targets:?}, bamboo {actual:?}", case.name));
                }
            }
            let mut packets = Vec::new();
            let active: BTreeSet<_> = level
                .entities
                .ids()
                .into_iter()
                .filter(|&id| {
                    level
                        .entities
                        .public_at(id)
                        .is_some_and(|public| matches!(public.kind, KindPublic::Arrow(_)))
                })
                .collect();
            for &uid in previous.difference(&active) {
                packets.push(packet_body(&RemoveEntities { entity_ids: &[uid] }));
            }
            let spawns = level.entities.take_spawn_snapshots();
            for public in spawns {
                if !matches!(public.kind, KindPublic::Arrow(_)) {
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
            let arrows: Vec<_> = active
                .iter()
                .map(|&id| {
                    let entity = level.entities.get(id).unwrap();
                    let EntityKind::Arrow(data) = &entity.kind else {
                        unreachable!()
                    };
                    ArrowFrame {
                        id,
                        motion: motion(&entity, data.pitch),
                        ground: data.in_ground,
                        shake: data.shake_time,
                        seed: entity.random.state(),
                        fire: case.water().then_some(entity.remaining_fire_ticks),
                    }
                })
                .collect();
            embeds += arrows.iter().filter(|arrow| arrow.ground).count();
            let label = format!("{} tick {tick}", case.name);
            self.check_arrow_frames(&label, &expected.arrows, &arrows);
            if let Some(expected_effects) = &expected.extinguished
                && *expected_effects != extinguished
            {
                self.mismatch(format!(
                    "{label} extinguish sounds: official {expected_effects:?}, bamboo {extinguished:?}"
                ));
            }
            if let Some(expected_motion) = expected.shooter {
                let shooter = level.entities.get(case.first_id).unwrap();
                let mob = shooter.kind.mob().unwrap();
                self.check_projectile_motion(
                    &label,
                    "shooter",
                    &expected_motion,
                    &motion(&shooter, mob.x_rot),
                );
                let ours = (
                    shooter.random.state(),
                    mob.aggressive,
                    mob.using_item_flags != 0,
                );
                let theirs = (
                    expected.shooter_seed.unwrap(),
                    expected.aggressive.unwrap(),
                    expected.drawing.unwrap(),
                );
                if ours != theirs {
                    self.mismatch(format!("{label} shooter seed/aggressive/drawing: official {theirs:?}, bamboo {ours:?}"));
                }
            }
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
            packet_count += packets.len() + player_packets.len();
            previous = active;
            self.steps += 1;
        }
        if (shots, embeds) != (case.shots, case.embeds) {
            self.mismatch(format!(
                "{} coverage: official {} shots / {} embedded ticks, bamboo {shots} / {embeds}",
                case.name, case.shots, case.embeds
            ));
        }
        let health = f32::from_bits(level.game.as_ref().unwrap().state(case.player_id)[0] as u32);
        println!(
            "{}: {} ticks, {shots} arrows, {embeds} embedded ticks, {packet_count} compared packets, final player health {health}",
            case.name,
            case.frames.len()
        );
    }

    fn check_arrow_frames(&mut self, label: &str, expected: &[ArrowFrame], actual: &[ArrowFrame]) {
        if expected.len() != actual.len() {
            self.mismatch(format!(
                "{label}: official {} arrows, bamboo {}",
                expected.len(),
                actual.len()
            ));
        }
        for (theirs, ours) in expected.iter().zip(actual) {
            if (
                theirs.id,
                theirs.ground,
                theirs.shake,
                theirs.seed,
                theirs.fire,
            ) != (ours.id, ours.ground, ours.shake, ours.seed, ours.fire)
            {
                self.mismatch(format!("{label} arrow id/ground/shake/seed/fire: official ({}, {}, {}, {}, {:?}), bamboo ({}, {}, {}, {}, {:?})",
                    theirs.id, theirs.ground, theirs.shake, theirs.seed, theirs.fire, ours.id, ours.ground, ours.shake, ours.seed, ours.fire));
            }
            self.check_projectile_motion(label, "arrow", &theirs.motion, &ours.motion);
        }
    }

    fn check_projectile_motion(
        &mut self,
        label: &str,
        entity: &str,
        expected: &[i64; 8],
        actual: &[i64; 8],
    ) {
        let names = ["x", "y", "z", "dx", "dy", "dz", "yaw", "pitch"];
        if let Some(i) = (0..8).find(|&i| expected[i] != actual[i]) {
            let value = |bits| {
                if i < 6 {
                    f64::from_bits(bits as u64)
                } else {
                    f64::from(f32::from_bits(bits as u32))
                }
            };
            self.mismatch(format!(
                "{label} {entity} {}: official {}, bamboo {}",
                names[i],
                value(expected[i]),
                value(actual[i])
            ));
        }
    }
}

fn motion(entity: &Entity, pitch: f32) -> [i64; 8] {
    let p = entity.position;
    let v = entity.delta;
    [
        p.x.to_bits() as i64,
        p.y.to_bits() as i64,
        p.z.to_bits() as i64,
        v.x.to_bits() as i64,
        v.y.to_bits() as i64,
        v.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
        i64::from(pitch.to_bits() as i32),
    ]
}
