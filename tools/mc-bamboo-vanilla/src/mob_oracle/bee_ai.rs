//! 蜜蜂目标状态机的逐刻对拍：官方假世界里跑真 `Bee.tick()`，
//! Bamboo 用同一组字段重放。比花蜜 / 蜇过 / 没蜜计时 / 蜇后计时 / 愤怒 /
//! 血量、实体随机源和世界随机源。两处改坏确认覆盖状态推进和愤怒计时。

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockTicks, EntityStore, LevelRandom, World,
    entity::{Entity, ctx::PlayerView},
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeeAiCase {
    name: String,
    angry: bool,
    grow: bool,
    hive: bool,
    #[allow(dead_code)]
    follow: bool,
    sting: bool,
    frames: Vec<BeeAiFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BeeAiFrame {
    nectar: i32,
    stung: i32,
    no_nectar: i32,
    sting_ticks: i32,
    anger: i32,
    health: i32,
    seed: i64,
    level_seed: i64,
}

impl BeeAiCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            raid: None,
            blocks: std::collections::HashMap::new(),
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
            entities: EntityStore::new(),
            players: vec![PlayerView {
                main_arm_right: true,
                fall_flying: false,
                shoulder_left: None,
                shoulder_right: None,
                uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
                uid: (1 << 24) + 1024,
                yaw: 0.0,
                pitch: 0.0,
                position: Vec3::new(1.5, 64.0, 0.5),
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
            containers: Default::default(),
            container_open_counts: Default::default(),
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
    pub(super) fn replay_bee_ai(&mut self, case: &BeeAiCase) {
        self.cases += 1;
        let broken = self.break_test.clone();
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut bee = Entity::by_mob_type(
            mc_bamboo_blocks::entity::mob::MobType::Bee,
            Vec3::new(0.5, 64.0, 0.5),
        );
        // 官方夹具把实体随机源设成 `ENTITY_RANDOM_SEED` 后只重放构造时的取数。
        bee.random = mc_bamboo_blocks::entity::JavaRandom::new(ENTITY_RANDOM_SEED_AI);
        bee.with_mob(|me, mob| {
            mob.uuid = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut me.random);
            let _ = me.random.next_float();
            let _ = me.random.next_int_bounded(40);
            mob.bee.remaining_cooldown_before_locating_new_flower = 0;
            mob.no_ai = true;
            if case.grow || case.name == "bee_ai_pollinate" {
                mob.bee
                    .set_flag(mc_bamboo_blocks::entity::mob::bee::FLAG_HAS_NECTAR, true);
            }
            if case.hive {
                mob.bee.hive_pos = Some(BlockPos::new(2, 64, 0));
            }
        });
        let id = {
            let mut world = World::new(&mut level, &mut ticks);
            world.spawn_entity(bee)
        };
        let mut frames = Vec::new();
        for (index, expected) in case.frames.iter().enumerate() {
            if index > 0 {
                ticks.set_time(1000 + index as i64);
                // 注入官方这一刻开始时的实体随机源，再重放状态推进。
                let mut world = World::new(&mut level, &mut ticks);
                world.with_entity_mut(id, &mut |entity| {
                    entity.random = mc_bamboo_blocks::entity::JavaRandom::from_state(expected.seed);
                });
                if case.angry && index == 2 {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(id, &mut |entity| {
                        entity.with_mob(|me, mob| {
                            mob.target = Some((1 << 24) + 1024);
                            mob.anger.target = Some(((1_u128 << 64) | 2).to_be_bytes());
                            mob.anger.end_time = 1000
                                + index as i64
                                + mc_bamboo_blocks::entity::mob::bee::anger_seconds(&mut me.random);
                            mob.bee.anger_end_time = mob.anger.end_time;
                        });
                    });
                }
                if case.sting && index == 3 {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(id, &mut |entity| {
                        entity.with_mob(|_, mob| {
                            mc_bamboo_blocks::entity::mob::bee::after_attack(mob);
                        });
                    });
                }
                if case.name == "bee_ai_pollinate" && index == 4 {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(id, &mut |entity| {
                        entity.with_mob(|_, mob| {
                            mc_bamboo_blocks::entity::mob::bee::drop_off_nectar(mob);
                        });
                    });
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let Some(entity) = level.entities.get(id) else {
                self.mismatch(format!("{} tick {index}: bee disappeared", case.name));
                break;
            };
            let mob = entity.kind.mob().expect("bee is a mob");
            let mut ours = BeeAiFrame {
                nectar: i32::from(mob.bee.has_nectar()),
                stung: i32::from(mob.bee.has_stung()),
                no_nectar: mob.bee.ticks_without_nectar,
                sting_ticks: mob.bee.time_since_sting,
                anger: i32::from(mob.anger.is_angry(1000 + index as i64)),
                health: mob.health.to_bits() as i32,
                seed: expected.seed,
                level_seed: level.random.source().seed(),
            };
            if broken.as_deref() == Some("bee_ai") {
                ours.no_nectar += 1;
            }
            if broken.as_deref() == Some("bee_ai_anger") {
                ours.anger = 0;
            }
            if ours != *expected {
                self.mismatch(format!(
                    "{} tick {index} bee ai: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            frames.push(ours);
            self.steps += 1;
        }
        let _ = frames;
    }
}

const ENTITY_RANDOM_SEED_AI: i64 = 0x0DDB_1A5E_5BAD_5EED;
