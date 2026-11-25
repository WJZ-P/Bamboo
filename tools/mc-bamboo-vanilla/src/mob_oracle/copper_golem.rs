//! 铜傀儡逐刻对拍：官方假世界里跑真 `CopperGolem.tick()` / 大脑 / 搬运行为，
//! Bamboo 用同一个方块 / 容器场景重放，逐刻比方块、容器槽位、氧化等级与计时、
//! 状态、主手物品、大脑快照和世界随机源。
//!
//! Copper golem per-tick parity: the official fake level runs the real
//! `CopperGolem.tick()` / brain / transport behaviour; Bamboo replays the same
//! block and container scene and compares blocks, chest slots, weathering stage and
//! clock, state, held stack, brain dump and level random every tick.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LevelRandom,
    entity::{
        Entities, Entity,
        mob::{MobType, copper_golem},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

const FIRST_ID: i32 = 1 << 24;
const START_TIME: i64 = 1000;
const ENTITY_RANDOM_SEED: i64 = 0x0DDB_1A5E_5BAD_5EED;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CopperGolemCase {
    name: String,
    frames: Vec<CopperGolemFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CopperGolemFrame {
    blocks: Vec<i32>,
    chests: Vec<Vec<[i32; 2]>>,
    open_counts: Vec<i32>,
    golem: Vec<i64>,
    brain: Vec<String>,
    /// 官方本刻开始时的世界随机源状态，逐刻注入 Bamboo。
    #[serde(default)]
    start_seed: i64,
    /// 官方本刻结束时的世界随机源状态，逐刻逐位比较。
    level_seed: i64,
    spawns: Vec<(String, i32)>,
}

impl CopperGolemCase {
    fn level(&self) -> MobLevel {
        let mut blocks = HashMap::new();
        for x in -4..=8 {
            for z in -4..=8 {
                blocks.insert(
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }
        let build = BlockPos::new(2, 64, 0);
        if self.name == "copper_golem_build" {
            blocks.insert(build, BlockState(mc_bamboo_data::blocks::COPPER_BLOCK));
            blocks.insert(
                build.above(),
                BlockState(mc_bamboo_data::blocks::CARVED_PUMPKIN).with(
                    mc_bamboo_blocks::state::HORIZONTAL_FACING,
                    mc_bamboo_core::Direction::North,
                ),
            );
        }
        let source = BlockPos::new(3, 63, 0);
        let dest = BlockPos::new(4, 63, 0);
        let mut containers = HashMap::new();
        if self.name == "copper_golem_transport" || self.name == "copper_golem_no_target" {
            blocks.insert(
                source,
                BlockState(mc_bamboo_data::blocks::COPPER_CHEST).with(
                    mc_bamboo_blocks::state::HORIZONTAL_FACING,
                    mc_bamboo_core::Direction::North,
                ),
            );
            let mut slots = vec![ItemStack::new(mc_bamboo_data::items::STONE, 5)];
            slots.resize(27, ItemStack::empty());
            containers.insert(source, slots);
        }
        if self.name == "copper_golem_transport" {
            blocks.insert(
                dest,
                BlockState(mc_bamboo_data::blocks::CHEST).with(
                    mc_bamboo_blocks::state::HORIZONTAL_FACING,
                    mc_bamboo_core::Direction::North,
                ),
            );
            let mut slots = vec![ItemStack::new(mc_bamboo_data::items::STONE, 2)];
            slots.resize(27, ItemStack::empty());
            containers.insert(dest, slots);
        }
        MobLevel {
            blocks,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                can_see_sky: true,
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
            entities: Entities::with_next_id(FIRST_ID),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            containers,
            container_open_counts: Default::default(),
            explosion_break: 0,
        }
    }
}

impl Report {
    pub(super) fn replay_copper_golem(&mut self, case: &CopperGolemCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        let build = BlockPos::new(2, 64, 0);
        let source = BlockPos::new(3, 63, 0);
        let dest = BlockPos::new(4, 63, 0);
        let statue = BlockPos::new(1, 64, 0);

        let golem_id = {
            let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::new(3.5, 64.0, 0.5));
            entity.with_mob(|me, mob| {
                me.y_rot = 0.0;
                me.old_position = Vec3::new(3.5, 64.0, 0.5);
                mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3];
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                mob.y_head_rot_o = 0.0;
                mob.y_body_rot_o = 0.0;
                if case.name == "copper_golem_oxidize" {
                    mob.copper_golem.next_weathering_tick = START_TIME + 3;
                } else if case.name == "copper_golem_wax" {
                    mob.copper_golem.next_weathering_tick = copper_golem::IGNORE_WEATHERING_TICK;
                } else if case.name == "copper_golem_statue" {
                    mob.copper_golem.weather_state = copper_golem::WeatherState::Oxidized;
                    mob.copper_golem.next_weathering_tick = 0;
                }
            });
            entity.random = mc_bamboo_blocks::entity::JavaRandom::new(ENTITY_RANDOM_SEED);
            let mut world = World::new(&mut level, &mut ticks);
            let id = world.spawn_entity(entity);
            // `spawn_entity` 会跑 `init_mob_state`（重设随机源、传感器延迟和洗牌种子），
            // 所以在其之后显式对齐官方场景的初值。
            world.with_entity_mut(id, &mut |entity| {
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(3.5, 64.0, 0.5);
                entity.random = mc_bamboo_blocks::entity::JavaRandom::new(ENTITY_RANDOM_SEED);
                entity.with_mob(|me, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3];
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.y_head_rot = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_head_rot_o = 0.0;
                    mob.y_body_rot_o = 0.0;
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(me.random.state());
                        for sensor in &mut brain.sensors {
                            sensor.time_to_tick = 1;
                        }
                        let memory = mc_bamboo_blocks::entity::mob::brain::memory::MemoryType::TransportItemsCooldownTicks;
                        if case.name == "copper_golem_transport"
                            || case.name == "copper_golem_no_target"
                        {
                            brain.memories.erase(memory);
                        } else {
                            brain.memories.set(
                                memory,
                                mc_bamboo_blocks::entity::mob::brain::memory::MemoryValue::Int(65),
                            );
                        }
                    }
                });
            });
            id
        };

        let mut spawned_this_tick: Vec<(String, i32)> = Vec::new();
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(START_TIME + tick as i64);
                level.random.source().set_seed_raw(expected.start_seed);
                if case.name == "copper_golem_statue" && tick == 1 {
                    let mut world = World::new(&mut level, &mut ticks);
                    let Some(mut golem) = world.take_entity_for_oracle(golem_id) else {
                        return;
                    };
                    mc_bamboo_blocks::entity::ctx::apply(
                        mc_bamboo_blocks::entity::ctx::Command::CopperGolemStatue {
                            pos: statue,
                            pose: 3,
                            facing: mc_bamboo_core::Direction::South,
                        },
                        &mut golem,
                        &mut world,
                    );
                    world.put_entity_for_oracle(golem);
                    world.remove_entity(golem_id);
                }
                if case.name == "copper_golem_build" && tick == 1 {
                    // 官方 `EntityTickList` 在刻末才把新生成的实体并进名单，
                    // 所以这一刻生成的小傀儡要到下一刻才 tick。
                    let mut world = World::new(&mut level, &mut ticks);
                    // 官方 `EntityTickList` 在刻末才把新生成的实体并进名单，
                    // 所以这一刻生成的小傀儡要到下一刻才 tick。
                    world.begin_entity_tick();
                    // 官方夹具直接调 `CarvedPumpkinBlock.onPlace`；Bamboo 侧同一步。
                    mc_bamboo_blocks::behaviour::machines::pumpkin::place_for_oracle(
                        &mut world,
                        build.above(),
                    );
                    // 刻末合并 pending：`Level::end_entity_tick` 由下一次
                    // `World::tick_entities` 开头补做，效果相同。
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            if tick == 1 {
                spawned_this_tick = copper_spawns(&level, golem_id);
            }
            let mut ours = copper_frame(
                &level,
                golem_id,
                &[build, build.above(), source, dest, statue],
            );
            ours.start_seed = expected.start_seed;
            ours.level_seed = level.random.source().seed();
            ours.spawns = if tick == 1 {
                spawned_this_tick.clone()
            } else {
                Vec::new()
            };
            // 改坏确认：故意改错搬运场景的某一项，证明逐刻比较没放宽。
            match self.break_test.as_deref() {
                Some("cg_transport") => {
                    if !ours.chests.is_empty() && !ours.chests[0].is_empty() {
                        ours.chests[0][0][1] += 1;
                    }
                }
                Some("cg_brain") => {
                    ours.brain.push(String::from("!"));
                }
                Some("cg_weather") => {
                    if ours.golem.len() > 1 {
                        ours.golem[1] += 1;
                    }
                }
                Some("cg_build") if case.name == "copper_golem_build" => {
                    ours.blocks[0] = ours.blocks[0].wrapping_add(1);
                }
                _ => {}
            }
            if ours != *expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }
}

fn copper_frame(level: &MobLevel, golem_id: i32, cells: &[BlockPos]) -> CopperGolemFrame {
    let blocks = cells
        .iter()
        .map(|&pos| level.block_state(pos).0 as i32)
        .collect();
    let open_counts = vec![
        level.container_open_count(BlockPos::new(3, 63, 0)),
        level.container_open_count(BlockPos::new(4, 63, 0)),
    ];
    let mut chests = Vec::new();
    for pos in [BlockPos::new(3, 63, 0), BlockPos::new(4, 63, 0)] {
        let slots = level.slots_at(pos);
        if slots.is_empty() {
            chests.push(Vec::new());
        } else {
            chests.push(
                slots
                    .iter()
                    .map(|stack| {
                        if stack.is_empty() {
                            [0, 0]
                        } else {
                            [stack.item, stack.count]
                        }
                    })
                    .collect(),
            );
        }
    }
    let golem = match level.entities.get(golem_id) {
        Some(entity) => {
            let mob = entity.kind.mob().expect("copper golem is a mob");
            let held = mob
                .equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand);
            vec![
                0,
                i64::from(mob.copper_golem.weather_state.id()),
                i64::from(mob.copper_golem.state.id()),
                mob.copper_golem.next_weathering_tick,
                i64::from(if held.is_empty() { 0 } else { held.item }),
                i64::from(held.count),
                i64::from(mob.health.to_bits() as i32),
                entity.position.x.to_bits() as i64,
            ]
        }
        None => vec![1, 0, 0, 0, 0, 0, 0, 0],
    };
    let brain = level
        .entities
        .get(golem_id)
        .map(|entity| copper_brain(&entity))
        .unwrap_or_default();
    let _ = (level, golem_id);
    CopperGolemFrame {
        blocks,
        chests,
        open_counts,
        golem,
        brain,
        start_seed: 0,
        level_seed: level.random.source().seed(),
        spawns: Vec::new(),
    }
}

/// 这一刻新生成的实体（Java 夹具每刻清空 `level.spawned`）。
fn copper_spawns(level: &MobLevel, golem_id: i32) -> Vec<(String, i32)> {
    level
        .entities
        .iter_public()
        .filter(|public| public.uid != golem_id)
        .map(|public| {
            (
                MobType::by_entity_type(public.kind.entity_type())
                    .map(|ty| ty.name().trim_start_matches("minecraft:").to_owned())
                    .unwrap_or_default(),
                public.uid,
            )
        })
        .collect()
}

/// 铜傀儁大脑摘要，和 Java 侧 `copperBrainSummary` 逐项对齐。
fn copper_brain(entity: &Entity) -> Vec<String> {
    use mc_bamboo_blocks::entity::mob::brain::memory::MemoryType;
    let Some(brain) = entity.kind.mob().and_then(|mob| mob.brain.as_deref()) else {
        return Vec::new();
    };
    let mut activities: Vec<String> = brain
        .active_names()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    activities.sort();
    let mut running: Vec<String> = brain.running_names();
    running.sort();
    let cooldown = brain
        .memories
        .int(MemoryType::TransportItemsCooldownTicks)
        .map(|value| value.to_string())
        .unwrap_or_default();
    let positions = |kind| {
        brain
            .memories
            .global_pos_list(kind)
            .map(|list| {
                let mut out: Vec<String> = list
                    .iter()
                    .map(|gp| format!("{}:{}:{}", gp.pos.x, gp.pos.y, gp.pos.z))
                    .collect();
                out.sort();
                out.join(";")
            })
            .unwrap_or_default()
    };
    let walk = brain
        .memories
        .walk_target()
        .map(|walk| match walk.target {
            mc_bamboo_blocks::entity::mob::brain::memory::PositionTracker::Block {
                pos, ..
            } => {
                format!("{};{};{}", pos.x, pos.y, pos.z)
            }
            _ => "?".to_owned(),
        })
        .unwrap_or_default();
    let look = brain
        .memories
        .look_target()
        .map(|look| match look {
            mc_bamboo_blocks::entity::mob::brain::memory::PositionTracker::Block {
                pos, ..
            } => {
                format!("{};{};{}", pos.x, pos.y, pos.z)
            }
            _ => "?".to_owned(),
        })
        .unwrap_or_default();
    vec![
        activities.join(","),
        running.join(","),
        cooldown,
        positions(MemoryType::VisitedBlockPositions),
        positions(MemoryType::UnreachableTransportBlockPositions),
        walk,
        look,
    ]
}
