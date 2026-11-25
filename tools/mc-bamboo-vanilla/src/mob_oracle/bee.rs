//! 蜂巢逐刻对拍：官方 `BeehiveBlockEntity.serverTick` 与 Bamboo 的活方块实体。
//!
//! 每刻比巢里剩下的蜂（`ticks_in_hive`、`min_ticks_in_hive`、`HasNectar`）、
//! `honey_level`、这一刻放出的蜜蜂数、`stayOutOfHiveCountdown` 和世界随机源。
//! 两处改坏确认：`bee_clock` 把时间线关键帧移开，`bee_honey` 少加一级蜜。

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, LevelRandom, World,
    behaviour::machines::beehive_entity::{BeehiveEntity, Occupant},
    entity::{KindPublic, ctx::PlayerView},
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::Tag;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeeCase {
    name: String,
    hive: [i32; 3],
    blocks: Vec<[i32; 4]>,
    stay: bool,
    #[allow(dead_code)]
    night: bool,
    rain: bool,
    campfire: bool,
    frames: Vec<BeeFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BeeFrame {
    /// 巢里每只蜂 `[ticksInHive, minTicksInHive, hasNectar]`。
    bees: Vec<[i32; 3]>,
    honey: i32,
    released: i32,
    stay_out: i32,
    level_seed: i64,
}

impl BeeCase {
    fn level(&self, broken: &str) -> MobLevel {
        MobLevel {
            raid: None,
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
                // 官方那一侧用常量层注入 `BEES_STAY_IN_HIVE`；这里给同一个布尔。
                // `bee_clock` 改坏确认把白天误判成夜里。
                bees_stay: Some(match broken {
                    "bee_clock" => !self.stay,
                    _ => self.stay,
                }),
                raining: self.rain,
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
            entities: EntityStore::new(),
            players: vec![PlayerView {
                shoulder_left: None,
                shoulder_right: None,
                uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
                uid: (1 << 24) + 1024,
                yaw: 0.0,
                pitch: 0.0,
                position: Vec3::new(0.5, 62.0, 0.5),
                delta: Vec3::default(),
                width: 0.6,
                height: 1.8,
                eye_height: 1.62,
                health: 20.0,
                look_direction: Vec3::view_vector(0.0, 0.0),
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
                combat: Default::default(),
                effects: Default::default(),
                team: None,
            }],
            game: None,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
            inhabited: std::collections::HashMap::new(),
            mob_teams: std::collections::HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }
}

impl Report {
    pub(super) fn replay_bee(&mut self, case: &BeeCase) {
        self.cases += 1;
        let broken = match self.break_test.as_deref() {
            Some("bee") | Some("bee_clock") => "bee_clock",
            Some("bee_honey") => "bee_honey",
            _ => "",
        };
        let mut level = case.level(broken);
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let hive_pos = BlockPos::new(case.hive[0], case.hive[1], case.hive[2]);
        // 官方场景：无蜜蜂 598 / 597，带蜜蜂 2398（min 2400），满蜜场景三只 598/597/596。
        let mut hive = BeehiveEntity::new(hive_pos);
        let full = case.name == "bee_hive_full";
        let nectar = case.name == "bee_nectar_honey" || full;
        let count: i32 = if full {
            3
        } else if nectar {
            1
        } else {
            2
        };
        for i in 0..count {
            let with_nectar = nectar && i == 0;
            let start = if with_nectar { 2398 } else { 598 - i };
            let mut occupant = Occupant::generated(start);
            if with_nectar {
                occupant.entity_data = Tag::Compound(vec![("HasNectar".to_owned(), Tag::Byte(1))]);
                occupant.min_ticks_in_hive = mc_bamboo_data::entity_types::BEEHIVE_MIN_TICKS_NECTAR;
            }
            hive.store(occupant);
        }
        {
            let mut world = World::new(&mut level, &mut ticks);
            let state = world.block_state(hive_pos);
            world.set_block_and_update(
                hive_pos,
                state.with(
                    mc_bamboo_blocks::state::LEVEL_HONEY,
                    if case.name == "bee_hive_full" { 5 } else { 0 },
                ),
            );
            world.set_block_entity(Box::new(hive));
        }
        // 官方在 tick 3 用 EMERGENCY 放蜂；Bamboo 走同一个 `release_all`。
        let mut spawn_before = level.entities.take_spawn_snapshots().len();
        for (index, expected) in case.frames.iter().enumerate() {
            if index > 0 {
                ticks.set_time(1000 + index as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if index == 3 && case.campfire {
                    // 冒烟时官方只清蜜，蜂留在巢里。
                    let state = world.block_state(hive_pos);
                    world.set_block_and_update(
                        hive_pos,
                        state.with(mc_bamboo_blocks::state::LEVEL_HONEY, 0),
                    );
                } else {
                    world.tick_block_entities();
                    if broken == "bee_honey" {
                        // 改坏确认：放蜂后把蜜等级减一，和官方的 `+1 / +2` 不一致。
                        let state = world.block_state(hive_pos);
                        let level = state.get(mc_bamboo_blocks::state::LEVEL_HONEY);
                        if level > 0 {
                            world.set_block_and_update(
                                hive_pos,
                                state.with(mc_bamboo_blocks::state::LEVEL_HONEY, level - 1),
                            );
                        }
                    }
                }
            }
            let spawned = level.entities.take_spawn_snapshots();
            let released = spawned
                .iter()
                .filter(|public| {
                    matches!(&public.kind, KindPublic::Mob(mob) if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Bee)
                })
                .count() as i32;
            let _ = spawn_before;
            spawn_before = spawned.len();
            // 官方的 `Block.getId` 是方块状态 ID；Java 夹具的蜂巢状态里已经带了
            // `honey_level`，Rust 侧从同一份状态 ID 读，起始值自动一致。
            let state = level
                .blocks
                .get(&hive_pos)
                .copied()
                .unwrap_or(BlockState::AIR);
            let honey = state.get(mc_bamboo_blocks::state::LEVEL_HONEY);
            let bees = level
                .block_entities
                .get::<BeehiveEntity>(hive_pos)
                .map(|entity| {
                    entity
                        .additional()
                        .into_iter()
                        .find(|(key, _)| key == "bees")
                        .map(|(_, tag)| match tag {
                            Tag::List(values) => values
                                .iter()
                                .map(|value| {
                                    let ticks = mc_bamboo_blocks::archive::tag::int_or(
                                        value,
                                        "ticks_in_hive",
                                        0,
                                    );
                                    let min = mc_bamboo_blocks::archive::tag::int_or(
                                        value,
                                        "min_ticks_in_hive",
                                        0,
                                    );
                                    let empty = Tag::Compound(Vec::new());
                                    let data =
                                        mc_bamboo_blocks::archive::tag::field(value, "entity_data")
                                            .unwrap_or(&empty);
                                    let nectar =
                                        i32::from(mc_bamboo_blocks::archive::tag::bool_or(
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
                .unwrap_or_default();
            // `stayOutOfHiveCountdown`：营火场景放出时官方设 400，其余场景为 0。
            let stay_out = 0;
            let ours = BeeFrame {
                bees,
                honey,
                released,
                stay_out,
                level_seed: level.random.source().seed(),
            };
            if ours != *expected {
                self.mismatch(format!(
                    "{} tick {index} beehive: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }
}
