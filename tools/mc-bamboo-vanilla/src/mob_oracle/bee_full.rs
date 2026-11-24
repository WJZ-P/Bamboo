//! 蜜蜂全量逐刻对拍：真实 POI、真实方块实体、真目标链（不开 NoAi）、
//! 真实飞行寻路与移动控制、真实环境属性与出站包。
//!
//! Full-tick bee oracle: real POI, real block entities, real goal chain (no NoAi),
//! real flying navigation/move control, real environment attributes, and outbound packets.

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, LevelRandom, World,
    behaviour::machines::beehive_entity::{BeehiveEntity, Occupant},
    entity::{Entity, KindPublic, mob::MobType},
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_game::entity_tracking::{EntityTracking, Snapshot, event_packets};
use mc_bamboo_nbt::Tag;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeeFullCase {
    name: String,
    first_id: i32,
    start_time: i64,
    clock_start: i64,
    raining: bool,
    blocks: Vec<[i32; 4]>,
    hives: Vec<HiveSetup>,
    bees: Vec<BeeSetup>,
    frames: Vec<BeeFullFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HiveSetup {
    pos: [i32; 3],
    honey: i32,
    occupants: Vec<[i32; 3]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeeSetup {
    x: i64,
    y: i64,
    z: i64,
    has_nectar: bool,
    ticks_without_nectar: i32,
    stay_out: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeeFullFrame {
    bees: Vec<BeeEntityFrame>,
    hives: Vec<HiveFrame>,
    level_seed: i64,
    packets: Vec<String>,
    sync_flags: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BeeEntityFrame {
    id: i32,
    x: i64,
    y: i64,
    z: i64,
    dx: i64,
    dy: i64,
    dz: i64,
    yr: i32,
    xr: i32,
    hr: i32,
    br: i32,
    hp: i32,
    g: bool,
    flags: u8,
    anger_end: i64,
    target: i32,
    hive_pos: Option<[i32; 3]>,
    flower_pos: Option<[i32; 3]>,
    no_nectar: i32,
    stay_out: i32,
    crops: i32,
    hive_cd: i32,
    flower_cd: i32,
    goals: String,
    seed: i64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct HiveFrame {
    bees: Vec<[i32; 3]>,
    honey: i32,
}

impl Report {
    pub(super) fn replay_bee_full(&mut self, case: &BeeFullCase) {
        self.cases += 1;
        let mut level = MobLevel {
            blocks: case
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                clock: case.clock_start,
                difficulty: 2,
                can_see_sky: true,
                raining: case.raining,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: EntityStore::with_next_id(case.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
        };
        level.register_poi_from_blocks();

        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);

        let hive_positions: Vec<BlockPos> = case
            .hives
            .iter()
            .map(|h| BlockPos::new(h.pos[0], h.pos[1], h.pos[2]))
            .collect();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for h in &case.hives {
                let pos = BlockPos::new(h.pos[0], h.pos[1], h.pos[2]);
                let mut be = BeehiveEntity::new(pos);
                for &[t, min_t, nectar] in &h.occupants {
                    let mut occ = Occupant::generated(t);
                    occ.min_ticks_in_hive = min_t;
                    if nectar != 0 {
                        occ.entity_data =
                            Tag::Compound(vec![("HasNectar".to_owned(), Tag::Byte(1))]);
                    }
                    be.store(occ);
                }
                let state = world.block_state(pos);
                let _ = world.set_block_and_update(
                    pos,
                    state.with(mc_bamboo_blocks::state::LEVEL_HONEY, h.honey),
                );
                world.set_block_entity(Box::new(be));
            }
        }

        let mut live_ids: Vec<i32> = Vec::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for b in &case.bees {
                let pos = Vec3::new(
                    f64::from_bits(b.x as u64),
                    f64::from_bits(b.y as u64),
                    f64::from_bits(b.z as u64),
                );
                let entity = Entity::by_mob_type(MobType::Bee, pos);
                let id = world.spawn_entity(entity);
                world.with_entity_mut(id, &mut |e| {
                    e.old_position = pos;
                    e.with_mob(|me, mob| {
                        mob.y_rot_o = me.y_rot;
                        mob.x_rot_o = mob.x_rot;
                        if b.has_nectar {
                            mob.bee.set_flag(
                                mc_bamboo_blocks::entity::mob::bee::FLAG_HAS_NECTAR,
                                true,
                            );
                        }
                        mob.bee.ticks_without_nectar = b.ticks_without_nectar;
                        mob.bee.stay_out_of_hive = b.stay_out;
                    });
                });
                live_ids.push(id);
            }
        }

        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let _ = level.entities.take_spawn_snapshots();
        let _ = level.entities.take_mob_events();

        for (tick, expected) in case.frames.iter().enumerate() {
            let mut ours_packets: Vec<Vec<u8>> = Vec::new();
            if tick == 0 {
                for (i, &id) in live_ids.iter().enumerate() {
                    let Some(public) = level.entities.public_at(id) else {
                        continue;
                    };
                    let mut snapshot = Snapshot::of(public);
                    apply_flags(
                        &mut snapshot,
                        expected
                            .sync_flags
                            .get(i)
                            .map(String::as_str)
                            .unwrap_or("00000"),
                    );
                    let tracked = tracking.tracked_for_oracle(&snapshot);
                    ours_packets.extend(tracked.pairing_packets(&snapshot));
                }
            } else {
                ticks.set_time(case.start_time + tick as i64);
                level.facts.clock = case.clock_start + tick as i64;
                let (entity_effects, block_effects) = {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.tick_entities();
                    let entity_effects = world.into_effects();
                    let mut world = World::new(&mut level, &mut ticks);
                    world.tick_block_entities();
                    let block_effects = world.into_effects();
                    (entity_effects, block_effects)
                };
                let mut spawned = level.entities.take_spawn_snapshots();
                spawned.retain(|p| !tracking.has_tracked_for_oracle(p.uid));
                for public in &spawned {
                    if matches!(&public.kind, KindPublic::Mob(m) if m.mob_type == MobType::Bee) {
                        live_ids.push(public.uid);
                    }
                    let snapshot = Snapshot::of(public);
                    let tracked = tracking.tracked_for_oracle(&snapshot);
                    ours_packets.extend(tracked.pairing_packets(&snapshot));
                }
                let events = level.entities.take_mob_events();
                ours_packets.extend(event_packets(&events, || sound_source.next_long()));
                for effect in entity_effects.into_iter().chain(block_effects) {
                    if let mc_bamboo_blocks::Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        pitch,
                        seed,
                        ..
                    } = effect
                    {
                        ours_packets.push(mc_bamboo_protocol::frame::packet_body(
                            &mc_bamboo_protocol::packet::play::Sound {
                                sound,
                                inline: None,
                                source,
                                x,
                                y,
                                z,
                                volume,
                                pitch,
                                seed: seed.unwrap_or_else(|| sound_source.next_long()),
                            },
                        ));
                    }
                }
                let mut gone = Vec::new();
                live_ids.retain(|&id| {
                    if level.entities.public_at(id).is_none() {
                        gone.push(id);
                        false
                    } else {
                        true
                    }
                });
                if !gone.is_empty() {
                    ours_packets.push(mc_bamboo_protocol::frame::packet_body(
                        &mc_bamboo_protocol::packet::play::RemoveEntities { entity_ids: &gone },
                    ));
                }
                live_ids.sort_unstable();
                for (i, &id) in live_ids.iter().enumerate() {
                    let Some(public) = level.entities.public_at(id) else {
                        continue;
                    };
                    let mut snapshot = Snapshot::of(public);
                    let bits = expected
                        .sync_flags
                        .get(i)
                        .map(String::as_str)
                        .unwrap_or("00000");
                    let (needs_sync, _sync_pos, sync_vel) = apply_flags(&mut snapshot, bits);
                    let tracked = tracking.tracked_for_oracle(&snapshot);
                    ours_packets.extend(tracked.step(&snapshot, needs_sync, sync_vel));
                }
            }

            // 比较每只活蜂状态。
            let mut ours_bees = Vec::new();
            for &id in &live_ids {
                let Some(entity) = level.entities.get(id) else {
                    continue;
                };
                let mob = entity.kind.mob().expect("bee is a mob");
                ours_bees.push(BeeEntityFrame {
                    id,
                    x: entity.position.x.to_bits() as i64,
                    y: entity.position.y.to_bits() as i64,
                    z: entity.position.z.to_bits() as i64,
                    dx: entity.delta.x.to_bits() as i64,
                    dy: entity.delta.y.to_bits() as i64,
                    dz: entity.delta.z.to_bits() as i64,
                    yr: entity.y_rot.to_bits() as i32,
                    xr: mob.x_rot.to_bits() as i32,
                    hr: mob.y_head_rot.to_bits() as i32,
                    br: mob.y_body_rot.to_bits() as i32,
                    hp: mob.health.to_bits() as i32,
                    g: entity.on_ground,
                    flags: mob.bee.flags,
                    anger_end: mob.bee.anger_end_time,
                    target: mob.target.unwrap_or(-1),
                    hive_pos: mob.bee.hive_pos.map(|p| [p.x, p.y, p.z]),
                    flower_pos: mob.bee.saved_flower_pos.map(|p| [p.x, p.y, p.z]),
                    no_nectar: mob.bee.ticks_without_nectar,
                    stay_out: mob.bee.stay_out_of_hive,
                    crops: mob.bee.crops_grown,
                    hive_cd: mob.bee.remaining_cooldown_before_locating_new_hive,
                    flower_cd: mob.bee.remaining_cooldown_before_locating_new_flower,
                    goals: mob.goals.running_names_of(mob).join(" "),
                    seed: entity.random.state(),
                });
            }
            if ours_bees != expected.bees {
                self.mismatch(format!(
                    "{} tick {tick} bees: official {:?}, bamboo {:?}",
                    case.name, expected.bees, ours_bees
                ));
            }

            // 比较每个蜂巢方块实体状态。
            let mut ours_hives = Vec::new();
            for &pos in &hive_positions {
                let honey = level
                    .blocks
                    .get(&pos)
                    .copied()
                    .unwrap_or(BlockState::AIR)
                    .get(mc_bamboo_blocks::state::LEVEL_HONEY);
                let bees = read_hive_occupants(&level, pos);
                ours_hives.push(HiveFrame { bees, honey });
            }
            if ours_hives != expected.hives {
                self.mismatch(format!(
                    "{} tick {tick} hives: official {:?}, bamboo {:?}",
                    case.name, expected.hives, ours_hives
                ));
            }

            // 比较世界随机源。
            let ours_level_seed = level.random.source().seed();
            if ours_level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} levelSeed: official {}, bamboo {ours_level_seed}",
                    case.name, expected.level_seed
                ));
            }

            // 比较出站包。
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                &ours_packets,
                &expected.packets,
            );

            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}

fn read_hive_occupants(level: &MobLevel, pos: BlockPos) -> Vec<[i32; 3]> {
    level
        .block_entities
        .get::<BeehiveEntity>(pos)
        .map(|entity| {
            entity
                .additional()
                .into_iter()
                .find(|(key, _)| key == "bees")
                .map(|(_, tag)| match tag {
                    Tag::List(values) => values
                        .iter()
                        .map(|value| {
                            let ticks =
                                mc_bamboo_blocks::archive::tag::int_or(value, "ticks_in_hive", 0);
                            let min = mc_bamboo_blocks::archive::tag::int_or(
                                value,
                                "min_ticks_in_hive",
                                0,
                            );
                            let empty = Tag::Compound(Vec::new());
                            let data = mc_bamboo_blocks::archive::tag::field(value, "entity_data")
                                .unwrap_or(&empty);
                            let nectar = i32::from(mc_bamboo_blocks::archive::tag::bool_or(
                                data,
                                "HasNectar",
                                false,
                            ));
                            [ticks, min, nectar]
                        })
                        .collect(),
                    _ => Vec::new(),
                })
                .unwrap_or_default()
        })
        .unwrap_or_default()
}
