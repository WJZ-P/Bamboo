//! 村民的场景：生成时的群系品种、存档往返和死亡掉落。
//!
//! 期 2 的村民没有大脑，生成场景只比较 `VillagerData` / finalized / 尺寸和血量；
//! 构造期的传感器延迟取数不在这一期，所以不比随机源。
//!
//! Villager scenes: the biome type at spawn, save round trips and death drops. Phase 2
//! has no villager brain, so the spawn scene compares `VillagerData` / finalized /
//! dimensions / health and not the random source.

use super::*;
use mc_bamboo_blocks::entity::mob::MobType;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerSpawnCase {
    biome: String,
    kind: String,
    profession: String,
    level: i32,
    finalized: bool,
    dimensions: [f32; 2],
    health: f32,
    follow_range: f64,
    left_handed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerSaveCase {
    name: String,
    nbt: String,
    reloaded: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerDeathCase {
    by_player: bool,
    removed: bool,
    items: Vec<String>,
    experience: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerBrainCase {
    name: String,
    first_id: i32,
    clock_start: i64,
    baby: bool,
    profession: String,
    has_job_site: bool,
    has_meeting_point: bool,
    hurt_tick: i32,
    has_zombie: bool,
    mob_seed: i64,
    sensor_delay: i64,
    brain_table: super::water_mob::BrainTable,
    frames: Vec<VillagerBrainFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VillagerBrainFrame {
    brain: String,
    motion: [i64; 8],
    seed: i64,
    level_seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerPoiCase {
    name: String,
    first_id: i32,
    clock_start: i64,
    mob_seed: i64,
    sensor_delay: i64,
    workstation: [i32; 3],
    first_claim_tick: usize,
    phase3_until_tick: usize,
    poi_checks: PoiTicketChecks,
    brain_table: super::water_mob::BrainTable,
    frames: Vec<VillagerPoiFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PoiTicketChecks {
    workstation: [i32; 4],
    bed: [i32; 5],
    bell: [i32; 4],
    bell_village_dist: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VillagerPoiFrame {
    brain: String,
    motion: [i64; 8],
    seed: i64,
    level_seed: i64,
    poi_free_tickets: i32,
    profession: String,
}

/// 村民自己的键，加上原样保留的 `Offers` / `Inventory` 和通用的 `Brain`。
const SAVE_KEYS: &[&str] = &[
    "VillagerData",
    "VillagerDataFinalized",
    "FoodLevel",
    "Gossips",
    "Xp",
    "LastRestock",
    "LastGossipDecay",
    "RestocksToday",
    "Offers",
    "Inventory",
    "Brain",
];

impl Report {
    pub(super) fn replay_villager_spawn(&mut self, case: &VillagerSpawnCase) {
        self.cases += 1;
        let Some(biome) = mc_bamboo_data::registries::biome::by_name(&case.biome) else {
            self.mismatch(format!("{}: 官方群系不认识", case.biome));
            return;
        };
        let level = MobLevel {
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: Some(biome),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut level = level;
        let mut world = World::new(&mut level, &mut ticks);
        let uid = match mc_bamboo_blocks::entity::mob::spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            None,
        ) {
            Ok(uid) => uid,
            Err(error) => {
                self.mismatch(format!("{}: summon failed: {error:?}", case.biome));
                return;
            }
        };
        if self.break_test.as_deref() == Some("villager_spawn") {
            // 改坏确认：把品种固定成平原，群系场景应当立刻报不一致。
            world.with_entity_mut(uid, &mut |entity| {
                entity.with_mob(|_, mob| {
                    mob.villager.kind =
                        mc_bamboo_data::villager::type_id("minecraft:plains").unwrap_or(0);
                });
            });
        }
        let Some(entity) = world.entity(uid) else {
            self.mismatch(format!("{}: summon vanished", case.biome));
            return;
        };
        let Some(mob) = entity.kind.mob() else {
            self.mismatch(format!("{}: summon not a mob", case.biome));
            return;
        };
        let kind = mc_bamboo_data::villager::type_name(mob.villager.kind).unwrap_or("");
        let profession = mc_bamboo_data::villager::profession(mob.villager.profession)
            .map_or("", |entry| entry.name);
        let dimensions = mob.dimensions();
        let follow_range = mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
        let actual = (
            kind,
            profession,
            mob.villager.level,
            mob.villager.finalized,
            [dimensions.0, dimensions.1],
            mob.health,
            follow_range,
            mob.left_handed,
        );
        let expected = (
            case.kind.as_str(),
            case.profession.as_str(),
            case.level,
            case.finalized,
            case.dimensions,
            case.health,
            case.follow_range,
            case.left_handed,
        );
        if actual != expected {
            self.mismatch(format!(
                "villager spawn {}: official {expected:?}, bamboo {actual:?}",
                case.biome
            ));
        }
    }

    pub(super) fn replay_villager_save(&mut self, case: &VillagerSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: SNBT does not parse: {error:?}", case.name));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{}: load failed: {error:?}", case.name));
            return;
        }
        if self.break_test.as_deref() == Some("villager_save") {
            // 改坏确认：等级加一，存档对拍应当报 `VillagerData` 不一致。
            entity.with_mob(|_, mob| mob.villager.level += 1);
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: save failed: {error:?}", case.name));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!(
                    "{}: reloaded SNBT does not parse: {error:?}",
                    case.name
                ));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected = field(&reloaded, key).map(canonical);
            let actual = field(&ours, key).map(canonical);
            if expected != actual {
                self.mismatch(format!(
                    "{} {key}: official {expected:?}, bamboo {actual:?}",
                    case.name
                ));
            }
        }
    }

    pub(super) fn replay_villager_death(&mut self, case: &VillagerDeathCase) {
        self.cases += 1;
        let mut level = MobLevel {
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut source =
            DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::Generic);
        if case.by_player {
            source =
                DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::PlayerAttack);
            source.attacker_is_player = true;
        }
        let id;
        {
            let mut world = World::new(&mut level, &mut ticks);
            id = world.spawn_entity(Entity::by_mob_type(
                MobType::Villager,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            world.hurt_mob(id, source, 100.0);
        }
        for tick in 0..25 {
            ticks.set_time(1001 + tick);
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
        }
        let removed = level.entities.public_at(id).is_none();
        if removed != case.removed {
            self.mismatch(format!(
                "villager death by_player {}: removed official {}, bamboo {removed}",
                case.by_player, case.removed
            ));
        }
        let mut items: Vec<String> = level
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .ground()
                    .map(|ground| format!("minecraft:itemx{}", ground.stack.count))
            })
            .collect();
        items.sort();
        if items != case.items {
            self.mismatch(format!(
                "villager death by_player {}: items official {:?}, bamboo {items:?}",
                case.by_player, case.items
            ));
        }
        let experience: i32 = level
            .entities
            .iter()
            .filter_map(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::ExperienceOrb(orb) => Some(orb.value),
                _ => None,
            })
            .sum();
        if experience != case.experience {
            self.mismatch(format!(
                "villager death by_player {}: experience official {}, bamboo {experience}",
                case.by_player, case.experience
            ));
        }
    }

    pub(super) fn replay_villager_brain(&mut self, case: &VillagerBrainCase) {
        use mc_bamboo_blocks::entity::{
            JavaRandom,
            mob::{
                brain::memory::{MemoryType, MemoryValue},
                villager_ai,
            },
        };
        use mc_bamboo_protocol::components::value::GlobalPos;

        self.cases += 1;
        let break_test = self.break_test.clone();
        let mut blocks = std::collections::HashMap::new();
        let stone = BlockState(mc_bamboo_data::blocks::STONE);
        for x in -15..=15 {
            for z in -15..=15 {
                blocks.insert(BlockPos::new(x, 63, z), stone);
            }
        }
        let mut level = MobLevel {
            blocks,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                clock: case.clock_start,
                difficulty: 2,
                can_see_sky: true,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(case.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);

        let update_schedule_clock = |level: &mut MobLevel, clock: i64| {
            // 改坏确认 `villager_schedule_keyframe`：关键帧错一刻（时钟减 1）。
            let effective = if break_test.as_deref() == Some("villager_schedule_keyframe") {
                clock - 1
            } else {
                clock
            };
            level.facts.clock = effective;
            level.facts.villager_activity =
                Some(mc_bamboo_blocks::sky::villager_activity(effective));
            level.facts.baby_villager_activity =
                Some(mc_bamboo_blocks::sky::baby_villager_activity(effective));
        };
        update_schedule_clock(&mut level, case.clock_start);

        let villager_id;
        {
            let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
            entity.with_mob(|me, mob| {
                if case.profession == "farmer" {
                    mob.villager.profession =
                        mc_bamboo_data::villager::profession_id("minecraft:farmer").unwrap_or(0);
                    mob.villager.xp = 1;
                    villager_ai::refresh_brain_offline(me, mob);
                }
                if case.baby {
                    let (width, height, eye) = MobType::Villager.dimensions(true);
                    me.dimensions = (width, height);
                    mob.age = -24_000;
                    mob.eye_height = eye;
                    villager_ai::refresh_brain_offline(me, mob);
                }
                if let Some(brain) = mob.brain.as_mut() {
                    if case.has_job_site {
                        brain.memories.set(
                            MemoryType::JobSite,
                            MemoryValue::GlobalPos(GlobalPos {
                                dimension: "minecraft:the_nether".to_owned(),
                                pos: BlockPos::new(3, 64, 0),
                            }),
                        );
                    }
                    if case.has_meeting_point {
                        brain.memories.set(
                            MemoryType::MeetingPoint,
                            MemoryValue::GlobalPos(GlobalPos {
                                dimension: "minecraft:the_nether".to_owned(),
                                pos: BlockPos::new(0, 64, 3),
                            }),
                        );
                    }
                }
            });
            let mut world = World::new(&mut level, &mut ticks);
            entity.with_mob(|_, mob| {
                villager_ai::register_brain_goals(mob, &world, 1000);
            });
            villager_id = world.spawn_entity(entity);
            // `spawn_entity` 会跑 `init_mob_state`（改朝向、随机源、传感器延迟和洗牌种子），
            // 所以在 spawn 之后显式对齐官方场景的初值。
            world.with_entity_mut(villager_id, &mut |entity| {
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(0.5, 64.0, 0.5);
                entity.random = JavaRandom::new(case.mob_seed);
                entity.with_mob(|me, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.y_head_rot = 0.0;
                    mob.y_head_rot_o = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_body_rot_o = 0.0;
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(me.random.state());
                        for sensor in &mut brain.sensors {
                            sensor.time_to_tick = case.sensor_delay;
                        }
                    }
                });
            });

            if case.has_zombie {
                let z_id = world.spawn_entity(Entity::by_mob_type(
                    MobType::Zombie,
                    Vec3::new(4.5, 64.0, 0.5),
                ));
                world.with_entity_mut(z_id, &mut |zombie| {
                    zombie.y_rot = 0.0;
                    zombie.old_position = Vec3::new(4.5, 64.0, 0.5);
                    zombie.random = JavaRandom::new(101);
                    zombie.with_mob(|_, mob| {
                        mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
                        mob.no_ai = true;
                        mob.x_rot = 0.0;
                        mob.y_rot_o = 0.0;
                        mob.x_rot_o = 0.0;
                        mob.y_head_rot = 0.0;
                        mob.y_head_rot_o = 0.0;
                        mob.y_body_rot = 0.0;
                        mob.y_body_rot_o = 0.0;
                    });
                });
            }
        }

        // 1. 核对大脑结构表（传感器、行为表、活动进入条件、退出清记忆）。
        let broken_table = break_test.as_deref() == Some("villager_brain_table");
        let ours_table = level
            .entities
            .get(villager_id)
            .and_then(|e| {
                e.kind
                    .mob()
                    .and_then(|m| m.brain.as_deref())
                    .map(|b| super::water_mob::brain_table(b, broken_table))
            })
            .unwrap_or_default();
        if ours_table != case.brain_table {
            self.mismatch(format!(
                "{} brain table: official {:?}, bamboo {ours_table:?}",
                case.name, case.brain_table
            ));
        }

        // 2. 逐刻重放并核对大脑状态、坐标/速度/朝向、实体随机源与世界随机源。
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let tick_i64 = tick as i64;
                ticks.set_time(1000 + tick_i64);
                update_schedule_clock(&mut level, case.clock_start + tick_i64);
                let mut world = World::new(&mut level, &mut ticks);
                if tick as i32 == case.hurt_tick {
                    let source = DamageSource::environment(
                        mc_bamboo_blocks::entity::mob::DamageKind::Generic,
                    );
                    world.hurt_mob(villager_id, source, 2.0);
                }
                if break_test.as_deref() == Some("villager_panic_invert") {
                    // 改坏确认：反转恐慌触发条件（有伤害/敌对时擦掉，平静时强行注入伤害）。
                    world.with_entity_mut(villager_id, &mut |entity| {
                        entity.with_mob(|_, mob| {
                            let hurt_or_hostile = mob.last_damage_source.is_some()
                                || mob.brain.as_deref().is_some_and(|b| {
                                    b.memories.has(MemoryType::HurtBy)
                                        || b.memories.has(MemoryType::NearestHostile)
                                });
                            if hurt_or_hostile {
                                mob.last_damage_source = None;
                                if let Some(b) = mob.brain.as_mut() {
                                    b.memories.erase(MemoryType::HurtBy);
                                    b.memories.erase(MemoryType::NearestHostile);
                                }
                            } else {
                                mob.last_damage_source = Some(DamageSource::environment(
                                    mc_bamboo_blocks::entity::mob::DamageKind::Generic,
                                ));
                                mob.last_damage_stamp = 1000 + tick_i64;
                            }
                        });
                    });
                }
                world.tick_entities();
                if break_test.as_deref() == Some("villager_raid_rng") {
                    // 改坏确认：破坏 `SetRaidStatus` 每刻对世界随机源的取数。
                    let _ = world.random().next_int(20);
                }
            }
            let Some(entity) = level.entities.get(villager_id) else {
                self.mismatch(format!("{} tick {tick}: villager missing", case.name));
                return;
            };
            let mob = entity.kind.mob().expect("villager is mob");
            let ours_brain = super::brain::dump(&level, &entity);
            if ours_brain != expected.brain {
                self.mismatch(format!(
                    "{} tick {tick} brain: official {:?}, bamboo {:?}",
                    case.name, expected.brain, ours_brain
                ));
            }
            let (p, v) = (entity.position, entity.delta);
            let ours_motion = [
                p.x.to_bits() as i64,
                p.y.to_bits() as i64,
                p.z.to_bits() as i64,
                v.x.to_bits() as i64,
                v.y.to_bits() as i64,
                v.z.to_bits() as i64,
                i64::from(entity.y_rot.to_bits() as i32),
                i64::from(mob.x_rot.to_bits() as i32),
            ];
            if ours_motion != expected.motion {
                self.mismatch(format!(
                    "{} tick {tick} motion: official {:?}, bamboo {:?}",
                    case.name, expected.motion, ours_motion
                ));
            }
            let ours_seed = entity.random.state();
            if ours_seed != expected.seed {
                self.mismatch(format!(
                    "{} tick {tick} mob seed: official {}, bamboo {ours_seed}",
                    case.name, expected.seed
                ));
            }
            let ours_level_seed = level.random.source().seed();
            if ours_level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level random: official {}, bamboo {ours_level_seed}",
                    case.name, expected.level_seed
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }

    /// 村民第 4 期 POI 夹具接口校验与最小 `AcquirePoi` 场景：
    /// 1) 核对工作站、床（仅床头）、钟在 `MobLevel.poi` 里的登记、`take`/`release` 票数和村庄距离；
    /// 2) 放一个堆肥桶和一只无业成年村民，跑真大脑到 `AcquirePoi` 认领前（`0..=phase3_until_tick`）
    ///    全量逐刻比较，并在 `first_claim_tick` 核对第 3 期排期随机数；第 4 期接上真实认领行为后
    ///    可直接放开 `phase3_until_tick` 截断做全 61 刻逐刻比较。
    ///
    /// Villager Phase 4 POI fixture verification and minimal `AcquirePoi` scene:
    /// verifies workstation/bed/bell registration and `take`/`release` tickets on `MobLevel.poi`,
    /// then compares the real-brain scene tick-by-tick through `phase3_until_tick` (plus
    /// scheduling RNG at `first_claim_tick`). Phase 4 removes the cap for full-trace parity.
    pub(super) fn replay_villager_poi(&mut self, case: &VillagerPoiCase) {
        use mc_bamboo_blocks::entity::mob::villager_ai;
        use mc_bamboo_core::Direction;
        use mc_bamboo_data::blocks;
        use mc_bamboo_world::light::SectionPos;

        self.cases += 1;

        // 1. 夹具工作站 / 床 / 钟 POI 登记与真实 take / release 票数校验。
        {
            let ws_pos = BlockPos::new(4, 64, 0);
            let bed_head = BlockPos::new(0, 64, 4);
            let bed_foot = BlockPos::new(0, 64, 5);
            let bell_pos = BlockPos::new(-4, 64, 0);
            let mut blocks = std::collections::HashMap::new();
            blocks.insert(ws_pos, BlockState(blocks::COMPOSTER));
            let bed_head_state = BlockState(blocks::RED_BED)
                .with(mc_bamboo_blocks::state::FACING, Direction::North)
                .with(
                    mc_bamboo_blocks::state::BED_PART,
                    mc_bamboo_blocks::state::BedPart::Head,
                );
            let bed_foot_state = BlockState(blocks::RED_BED)
                .with(mc_bamboo_blocks::state::FACING, Direction::North)
                .with(
                    mc_bamboo_blocks::state::BED_PART,
                    mc_bamboo_blocks::state::BedPart::Foot,
                );
            blocks.insert(bed_head, bed_head_state);
            blocks.insert(bed_foot, bed_foot_state);
            blocks.insert(bell_pos, BlockState(blocks::BELL));

            let mut check_level = MobLevel {
                blocks,
                random: LevelRandom::new(1),
                loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
                looting: Vec::new(),
                biome: None,
                rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
                facts: ZombieFacts {
                    difficulty: 2,
                    ..ZombieFacts::default()
                },
                natural_spawn: None,
                redstone: mc_bamboo_blocks::RedstoneMemory::default(),
                block_entities: BlockEntities::default(),
                block_events: BlockEvents::default(),
                game_event_listeners: Default::default(),
                warden_warnings: Default::default(),
                darkness_ticks: 0,
                poi: mc_bamboo_world::poi::PoiStore::new(),
                entities: Entities::with_next_id(case.first_id),
                players: Vec::new(),
                game: None,
                inhabited: Default::default(),
                mob_teams: Default::default(),
                ignore_bed_monsters: false,
                short_hunger: false,
                wrong_was_hurt: false,
                explosion_break: 0,
                shoulder: Default::default(),
            };
            check_level.register_poi_from_blocks();

            let free_or_absent = |level: &MobLevel, pos: BlockPos| -> i32 {
                if level.poi.get_type(pos).is_some() {
                    level.poi.free_tickets(pos)
                } else {
                    -1
                }
            };
            let ws_init = free_or_absent(&check_level, ws_pos);
            let _ = check_level
                .poi
                .take(|_| true, |_, p| p == ws_pos, ws_pos, 1);
            let ws_after1 = free_or_absent(&check_level, ws_pos);
            let ws_take2 = check_level
                .poi
                .take(|_| true, |_, p| p == ws_pos, ws_pos, 1)
                .is_some();
            let _ = check_level.poi.release(ws_pos);
            let ws_after_rel = free_or_absent(&check_level, ws_pos);

            let bed_init = free_or_absent(&check_level, bed_head);
            let bed_foot_tickets = free_or_absent(&check_level, bed_foot);
            let _ = check_level
                .poi
                .take(|_| true, |_, p| p == bed_head, bed_head, 1);
            let bed_after1 = free_or_absent(&check_level, bed_head);
            let bed_take2 = check_level
                .poi
                .take(|_| true, |_, p| p == bed_head, bed_head, 1)
                .is_some();
            let _ = check_level.poi.release(bed_head);
            let bed_after_rel = free_or_absent(&check_level, bed_head);

            let bell_init = free_or_absent(&check_level, bell_pos);
            let _ = check_level
                .poi
                .take(|_| true, |_, p| p == bell_pos, bell_pos, 1);
            let bell_after1 = free_or_absent(&check_level, bell_pos);
            let _ = check_level
                .poi
                .take(|_| true, |_, p| p == bell_pos, bell_pos, 1);
            let bell_after2 = free_or_absent(&check_level, bell_pos);
            let _ = check_level.poi.release(bell_pos);
            let bell_after_rel = free_or_absent(&check_level, bell_pos);
            let bell_village_dist = check_level
                .poi
                .sections_to_village(SectionPos::of(bell_pos));

            let ours_checks = PoiTicketChecks {
                workstation: [ws_init, ws_after1, i32::from(ws_take2), ws_after_rel],
                bed: [
                    bed_init,
                    bed_foot_tickets,
                    bed_after1,
                    i32::from(bed_take2),
                    bed_after_rel,
                ],
                bell: [bell_init, bell_after1, bell_after2, bell_after_rel],
                bell_village_dist,
            };
            if ours_checks != case.poi_checks {
                self.mismatch(format!(
                    "{} poiChecks: official {:?}, bamboo {:?}",
                    case.name, case.poi_checks, ours_checks
                ));
            }
        }

        // 2. 最小真大脑场景：平地 + 工作站 + 无业成年村民。
        let ws_pos = BlockPos::new(
            case.workstation[0],
            case.workstation[1],
            case.workstation[2],
        );
        let mut blocks = std::collections::HashMap::new();
        for x in -15..=15 {
            for z in -15..=15 {
                blocks.insert(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
            }
        }
        blocks.insert(ws_pos, BlockState(blocks::COMPOSTER));

        let mut level = MobLevel {
            blocks,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                clock: case.clock_start,
                difficulty: 2,
                can_see_sky: true,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(case.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };
        level.register_poi_from_blocks();

        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let set_clock = |level: &mut MobLevel, clock: i64| {
            level.facts.clock = clock;
            level.facts.villager_activity = Some(mc_bamboo_blocks::sky::villager_activity(clock));
            level.facts.baby_villager_activity =
                Some(mc_bamboo_blocks::sky::baby_villager_activity(clock));
        };
        set_clock(&mut level, case.clock_start);

        let villager_id;
        {
            let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
            let mut world = World::new(&mut level, &mut ticks);
            entity.with_mob(|_, mob| {
                villager_ai::register_brain_goals(mob, &world, 1000);
            });
            villager_id = world.spawn_entity(entity);
            world.with_entity_mut(villager_id, &mut |entity| {
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(0.5, 64.0, 0.5);
                entity.random = JavaRandom::new(case.mob_seed);
                entity.with_mob(|me, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.y_head_rot = 0.0;
                    mob.y_head_rot_o = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_body_rot_o = 0.0;
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(me.random.state());
                        for sensor in &mut brain.sensors {
                            sensor.time_to_tick = case.sensor_delay;
                        }
                    }
                });
            });
        }

        let ours_table = level
            .entities
            .get(villager_id)
            .and_then(|e| {
                e.kind
                    .mob()
                    .and_then(|m| m.brain.as_deref())
                    .map(|b| super::water_mob::brain_table(b, false))
            })
            .unwrap_or_default();
        if ours_table != case.brain_table {
            self.mismatch(format!(
                "{} brain table: official {:?}, bamboo {ours_table:?}",
                case.name, case.brain_table
            ));
        }

        // 全量逐刻比较所有 61 刻（含 AcquirePoi 认领工作站、走向工作站与成为农民）。
        let _ = case.phase3_until_tick;
        let mut prev_prof = level
            .entities
            .get(villager_id)
            .and_then(|e| e.kind.mob().map(|m| m.villager.profession))
            .unwrap_or(0);
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let tick_i64 = tick as i64;
                ticks.set_time(1000 + tick_i64);
                set_clock(&mut level, case.clock_start + tick_i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                world.with_entity_mut(villager_id, &mut |entity| {
                    let seed = entity.random.state();
                    entity.with_mob(|_, mob| {
                        if mob.villager.profession != prev_prof {
                            prev_prof = mob.villager.profession;
                            if let Some(brain) = mob.brain.as_mut() {
                                brain.seed_shuffles(seed);
                            }
                        }
                    });
                });
            }
            let Some(entity) = level.entities.get(villager_id) else {
                self.mismatch(format!("{} tick {tick}: villager missing", case.name));
                return;
            };
            let mob = entity.kind.mob().expect("villager is mob");
            let ours_level_seed = level.random.source().seed();
            if ours_level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level random: official {}, bamboo {ours_level_seed}",
                    case.name, expected.level_seed
                ));
            }
            let ours_brain = super::brain::dump(&level, &entity);
            if ours_brain != expected.brain {
                self.mismatch(format!(
                    "{} tick {tick} brain: official {:?}, bamboo {:?}",
                    case.name, expected.brain, ours_brain
                ));
            }
            let (p, v) = (entity.position, entity.delta);
            let ours_motion = [
                p.x.to_bits() as i64,
                p.y.to_bits() as i64,
                p.z.to_bits() as i64,
                v.x.to_bits() as i64,
                v.y.to_bits() as i64,
                v.z.to_bits() as i64,
                i64::from(entity.y_rot.to_bits() as i32),
                i64::from(mob.x_rot.to_bits() as i32),
            ];
            if ours_motion != expected.motion {
                self.mismatch(format!(
                    "{} tick {tick} motion: official {:?}, bamboo {:?}",
                    case.name, expected.motion, ours_motion
                ));
            }
            let ours_seed = entity.random.state();
            if ours_seed != expected.seed {
                self.mismatch(format!(
                    "{} tick {tick} mob seed: official {}, bamboo {ours_seed}",
                    case.name, expected.seed
                ));
            }
            let ours_tickets = if level.poi.get_type(ws_pos).is_some() {
                level.poi.free_tickets(ws_pos)
            } else {
                -1
            };
            if ours_tickets != expected.poi_free_tickets {
                self.mismatch(format!(
                    "{} tick {tick} poiFreeTickets: official {}, bamboo {ours_tickets}",
                    case.name, expected.poi_free_tickets
                ));
            }
            let ours_prof = mc_bamboo_data::villager::profession(mob.villager.profession)
                .map_or("", |entry| entry.name);
            if ours_prof != expected.profession {
                self.mismatch(format!(
                    "{} tick {tick} profession: official {:?}, bamboo {:?}",
                    case.name, expected.profession, ours_prof
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!(
            "{}: {} ticks compared (official AcquirePoi claimed at tick {})",
            case.name,
            case.frames.len(),
            case.first_claim_tick,
        );
    }

    /// 村民第 4 期真实 POI 全量对拍：多候选失败重试、抢工作站、拆站失业、夜间上床与天亮醒来、
    /// 听钟声藏身、农民收割播种与捡种子，附带 5 处 `--break-self-test` 改坏确认。
    pub(super) fn replay_villager_phase4(&mut self, case: &VillagerPhase4Case) {
        use mc_bamboo_blocks::entity::mob::{
            brain::memory::{MemoryType, MemoryValue},
            villager_ai,
        };
        use mc_bamboo_blocks::state::{BED_PART, BedPart, HORIZONTAL_FACING, MOISTURE, OCCUPIED};
        use mc_bamboo_core::Direction;
        use mc_bamboo_data::{blocks, items, villager as villager_data};
        use mc_bamboo_protocol::components::value::GlobalPos;
        use mc_bamboo_protocol::item::ItemStack;

        self.cases += 1;
        let break_test = self.break_test.clone();
        let name = case.name.as_str();
        let mut blocks_map = std::collections::HashMap::new();
        for x in -15..=15 {
            for z in -15..=15 {
                blocks_map.insert(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
            }
        }
        let mut watch_pois = Vec::new();
        let mut watch_blocks = Vec::new();

        match name {
            "villager_p4_retry" => {
                let blocked = BlockPos::new(3, 64, 0);
                blocks_map.insert(blocked, BlockState(blocks::COMPOSTER));
                for (wx, wy, wz) in [
                    (2, 64, 0),
                    (4, 64, 0),
                    (3, 64, 1),
                    (3, 64, -1),
                    (2, 65, 0),
                    (4, 65, 0),
                    (3, 65, 1),
                    (3, 65, -1),
                    (3, 66, 0),
                ] {
                    blocks_map.insert(BlockPos::new(wx, wy, wz), BlockState(blocks::STONE));
                }
                watch_pois.push(blocked);
                watch_pois.push(BlockPos::new(5, 64, 0));
            }
            "villager_p4_compete" => {
                let ws = BlockPos::new(4, 64, 0);
                blocks_map.insert(ws, BlockState(blocks::COMPOSTER));
                watch_pois.push(ws);
            }
            "villager_p4_unemploy" => {
                let ws = BlockPos::new(3, 64, 0);
                blocks_map.insert(ws, BlockState(blocks::COMPOSTER));
                watch_pois.push(ws);
                watch_blocks.push(ws);
            }
            "villager_p4_sleep_wake" => {
                let bed = BlockPos::new(3, 64, 0);
                let foot = BlockPos::new(3, 64, 1);
                let occupied = break_test.as_deref() == Some("villager_p4_no_sleep");
                blocks_map.insert(
                    bed,
                    BlockState(blocks::RED_BED)
                        .with(HORIZONTAL_FACING, Direction::North)
                        .with(BED_PART, BedPart::Head)
                        .with(OCCUPIED, occupied),
                );
                blocks_map.insert(
                    foot,
                    BlockState(blocks::RED_BED)
                        .with(HORIZONTAL_FACING, Direction::North)
                        .with(BED_PART, BedPart::Foot)
                        .with(OCCUPIED, occupied),
                );
                watch_pois.push(bed);
                watch_blocks.push(bed);
            }
            "villager_p4_bell" => {
                let bed = BlockPos::new(-4, 64, 0);
                let foot = BlockPos::new(-4, 64, 1);
                let bell = BlockPos::new(6, 64, 0);
                blocks_map.insert(
                    bed,
                    BlockState(blocks::RED_BED)
                        .with(HORIZONTAL_FACING, Direction::North)
                        .with(BED_PART, BedPart::Head),
                );
                blocks_map.insert(
                    foot,
                    BlockState(blocks::RED_BED)
                        .with(HORIZONTAL_FACING, Direction::North)
                        .with(BED_PART, BedPart::Foot),
                );
                blocks_map.insert(bell, BlockState(blocks::BELL));
                watch_pois.push(bed);
                watch_pois.push(bell);
            }
            "villager_p4_farmer" => {
                let ws = BlockPos::new(0, 64, 4);
                blocks_map.insert(ws, BlockState(blocks::COMPOSTER));
                let wet_farm = BlockState(blocks::FARMLAND).with(MOISTURE, 7);
                blocks_map.insert(BlockPos::new(2, 63, 0), wet_farm);
                blocks_map.insert(BlockPos::new(3, 63, 0), wet_farm);
                let ripe_wheat = BlockState(blocks::WHEAT).with(mc_bamboo_blocks::state::AGE_7, 7);
                blocks_map.insert(BlockPos::new(2, 64, 0), ripe_wheat);
                watch_pois.push(ws);
                watch_blocks.push(BlockPos::new(2, 64, 0));
                watch_blocks.push(BlockPos::new(3, 64, 0));
            }
            _ => {}
        }

        let mut rules = mc_bamboo_blocks::BlockGameRules::vanilla();
        if name == "villager_p4_farmer" && break_test.as_deref() == Some("villager_p4_no_harvest") {
            rules.mob_griefing = false;
        }
        let mut level = MobLevel {
            blocks: blocks_map,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules,
            facts: ZombieFacts {
                clock: case.clock_start,
                difficulty: 2,
                can_see_sky: true,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(case.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };
        if !(name == "villager_p4_retry" && break_test.as_deref() == Some("villager_p4_no_take")) {
            level.register_poi_from_blocks();
        }

        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let set_clock = |level: &mut MobLevel, clock: i64| {
            level.facts.clock = clock;
            level.facts.villager_activity = Some(mc_bamboo_blocks::sky::villager_activity(clock));
            level.facts.baby_villager_activity =
                Some(mc_bamboo_blocks::sky::baby_villager_activity(clock));
        };
        set_clock(&mut level, case.clock_start);

        let mut villager_ids = Vec::new();
        let spawn_villager =
            |level: &mut MobLevel, ticks: &mut BlockTicks, pos: Vec3, seed: i64, idx: u8| {
                let mut entity = Entity::by_mob_type(MobType::Villager, pos);
                let mut world = World::new(level, ticks);
                entity.with_mob(|_, mob| {
                    villager_ai::register_brain_goals(mob, &world, 1000);
                });
                let id = world.spawn_entity(entity);
                world.with_entity_mut(id, &mut |entity| {
                    entity.y_rot = 0.0;
                    entity.old_position = pos;
                    entity.random = JavaRandom::new(seed);
                    entity.with_mob(|me, mob| {
                        mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1 + idx];
                        mob.x_rot = 0.0;
                        mob.y_rot_o = 0.0;
                        mob.x_rot_o = 0.0;
                        mob.y_head_rot = 0.0;
                        mob.y_head_rot_o = 0.0;
                        mob.y_body_rot = 0.0;
                        mob.y_body_rot_o = 0.0;
                        if let Some(brain) = mob.brain.as_mut() {
                            brain.seed_shuffles(me.random.state());
                            for sensor in &mut brain.sensors {
                                sensor.time_to_tick = case.sensor_delay;
                            }
                        }
                    });
                });
                id
            };

        if name == "villager_p4_compete" {
            villager_ids.push(spawn_villager(
                &mut level,
                &mut ticks,
                Vec3::new(-0.5, 64.0, 0.5),
                71,
                0,
            ));
            villager_ids.push(spawn_villager(
                &mut level,
                &mut ticks,
                Vec3::new(13.5, 64.0, 0.5),
                73,
                1,
            ));
        } else if name == "villager_p4_farmer" {
            let farmer_prof = villager_data::profession_id("minecraft:farmer").unwrap();
            let ws = BlockPos::new(0, 64, 4);
            let _ = level.poi.take(|_| true, |_, p| p == ws, ws, 1);
            let pos = Vec3::new(1.5, 64.0, 0.5);
            let mut entity = Entity::by_mob_type(MobType::Villager, pos);
            let mut world = World::new(&mut level, &mut ticks);
            entity.with_mob(|_, mob| {
                mob.villager.set_profession(farmer_prof);
                let mut brain = villager_ai::brain(false, farmer_prof);
                brain.memories.set(
                    MemoryType::JobSite,
                    MemoryValue::GlobalPos(GlobalPos {
                        dimension: "minecraft:overworld".to_owned(),
                        pos: ws,
                    }),
                );
                mob.brain = Some(Box::new(brain));
                mob.villager
                    .inventory
                    .add_item(ItemStack::new(items::WHEAT_SEEDS, 4));
                villager_ai::register_brain_goals(mob, &world, 1000);
            });
            let id = world.spawn_entity(entity);
            world.with_entity_mut(id, &mut |entity| {
                entity.y_rot = 0.0;
                entity.old_position = pos;
                entity.random = JavaRandom::new(71);
                entity.with_mob(|me, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.y_head_rot = 0.0;
                    mob.y_head_rot_o = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_body_rot_o = 0.0;
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.seed_shuffles(me.random.state());
                        for sensor in &mut brain.sensors {
                            sensor.time_to_tick = case.sensor_delay;
                        }
                    }
                });
            });
            villager_ids.push(id);
            let mut dropped = Entity::ground_item_with_delta(
                Vec3::new(1.5, 64.0, 1.5),
                ItemStack::new(items::WHEAT_SEEDS, 2),
                Vec3::default(),
            );
            dropped.id = case.first_id + 10;
            world.add_prepared_entity(dropped);
        } else {
            villager_ids.push(spawn_villager(
                &mut level,
                &mut ticks,
                Vec3::new(0.5, 64.0, 0.5),
                71,
                0,
            ));
        }

        let mut prev_profs: Vec<i32> = villager_ids
            .iter()
            .map(|&id| {
                level
                    .entities
                    .get(id)
                    .and_then(|e| e.kind.mob().map(|m| m.villager.profession))
                    .unwrap_or(0)
            })
            .collect();

        let mut clock_offset = 0_i64;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let tick_i64 = tick as i64;
                ticks.set_time(1000 + tick_i64);
                if name == "villager_p4_sleep_wake" && tick == 60 {
                    clock_offset = 12_000 - 60;
                }
                set_clock(&mut level, case.clock_start + tick_i64 + clock_offset);

                if name == "villager_p4_retry"
                    && tick == 25
                    && break_test.as_deref() != Some("villager_p4_no_take")
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.set_block_and_update(
                        BlockPos::new(5, 64, 0),
                        BlockState(blocks::COMPOSTER),
                    );
                } else if name == "villager_p4_unemploy"
                    && tick == 55
                    && break_test.as_deref() != Some("villager_p4_no_unemploy")
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.set_block_and_update(BlockPos::new(3, 64, 0), BlockState::AIR);
                } else if name == "villager_p4_bell"
                    && tick == 10
                    && break_test.as_deref() != Some("villager_p4_no_bell")
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    mc_bamboo_blocks::behaviour::attached::bell::ring(
                        &mut world,
                        BlockPos::new(6, 64, 0),
                    );
                }

                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                for (i, &vid) in villager_ids.iter().enumerate() {
                    world.with_entity_mut(vid, &mut |entity| {
                        let seed = entity.random.state();
                        entity.with_mob(|_, mob| {
                            if mob.villager.profession != prev_profs[i] {
                                prev_profs[i] = mob.villager.profession;
                                if let Some(brain) = mob.brain.as_mut() {
                                    brain.seed_shuffles(seed);
                                }
                            }
                        });
                    });
                }
            }

            let ours_level_seed = level.random.source().seed();
            if ours_level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level random: official {}, bamboo {ours_level_seed}",
                    case.name, expected.level_seed
                ));
            }
            for (i, (&vid, exp_v)) in villager_ids.iter().zip(&expected.villagers).enumerate() {
                let Some(entity) = level.entities.get(vid) else {
                    self.mismatch(format!("{} tick {tick} v{i}: missing", case.name));
                    return;
                };
                let mob = entity.kind.mob().expect("villager is mob");
                let ours_brain = super::brain::dump(&level, &entity);
                if ours_brain != exp_v.brain {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} brain: official {:?}, bamboo {:?}",
                        case.name, exp_v.brain, ours_brain
                    ));
                }
                let (p, v) = (entity.position, entity.delta);
                let ours_motion = [
                    p.x.to_bits() as i64,
                    p.y.to_bits() as i64,
                    p.z.to_bits() as i64,
                    v.x.to_bits() as i64,
                    v.y.to_bits() as i64,
                    v.z.to_bits() as i64,
                    i64::from(entity.y_rot.to_bits() as i32),
                    i64::from(mob.x_rot.to_bits() as i32),
                ];
                if ours_motion != exp_v.motion {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} motion: official {:?}, bamboo {:?}",
                        case.name, exp_v.motion, ours_motion
                    ));
                }
                let ours_seed = entity.random.state();
                if ours_seed != exp_v.seed {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} seed: official {}, bamboo {ours_seed}",
                        case.name, exp_v.seed
                    ));
                }
                let ours_prof = villager_data::profession(mob.villager.profession)
                    .map_or("", |entry| entry.name);
                if ours_prof != exp_v.profession {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} profession: official {:?}, bamboo {:?}",
                        case.name, exp_v.profession, ours_prof
                    ));
                }
                let ours_sleep = mob.villager.sleeping_pos.map(|p| [p.x, p.y, p.z]);
                if ours_sleep != exp_v.sleeping_pos {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} sleepingPos: official {:?}, bamboo {:?}",
                        case.name, exp_v.sleeping_pos, ours_sleep
                    ));
                }
                let ours_inv = [
                    mob.villager.inventory.count_item(items::WHEAT_SEEDS),
                    mob.villager.inventory.count_item(items::WHEAT),
                ];
                if ours_inv != exp_v.inv {
                    self.mismatch(format!(
                        "{} tick {tick} v{i} inv: official {:?}, bamboo {:?}",
                        case.name, exp_v.inv, ours_inv
                    ));
                }
            }
            let ours_pois: Vec<i32> = watch_pois
                .iter()
                .map(|&pos| {
                    if level.poi.get_type(pos).is_some() {
                        level.poi.free_tickets(pos)
                    } else {
                        -1
                    }
                })
                .collect();
            if ours_pois != expected.poi_tickets {
                self.mismatch(format!(
                    "{} tick {tick} poiTickets: official {:?}, bamboo {:?}",
                    case.name, expected.poi_tickets, ours_pois
                ));
            }
            let ours_blocks: Vec<u32> = watch_blocks
                .iter()
                .map(|&pos| level.blocks.get(&pos).copied().unwrap_or(BlockState::AIR).0)
                .collect();
            if ours_blocks != expected.blocks {
                self.mismatch(format!(
                    "{} tick {tick} blocks: official {:?}, bamboo {:?}",
                    case.name, expected.blocks, ours_blocks
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks compared", case.name, case.frames.len());
    }
}

/// 村民第 4 期真实 POI 全量对拍用例。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerPhase4Case {
    pub name: String,
    pub clock_start: i64,
    pub sensor_delay: i64,
    pub first_id: i32,
    pub frames: Vec<VillagerPhase4Frame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerPhase4Frame {
    pub level_seed: i64,
    pub villagers: Vec<VillagerPhase4Entity>,
    pub poi_tickets: Vec<i32>,
    pub blocks: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerPhase4Entity {
    pub brain: String,
    pub motion: [i64; 8],
    pub seed: i64,
    pub profession: String,
    pub sleeping_pos: Option<[i32; 3]>,
    pub inv: [i32; 2],
}

/// 复合标签的键按名字排序，便于和官方的哈希表顺序比较。
fn canonical(tag: &mc_bamboo_nbt::Tag) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries
                .iter()
                .map(|(name, value)| (name.clone(), canonical(value)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Tag::Compound(sorted)
        }
        Tag::List(items) => Tag::List(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn field<'a>(tag: &'a mc_bamboo_nbt::Tag, key: &str) -> Option<&'a mc_bamboo_nbt::Tag> {
    match tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

/// 村民交易接进大脑的逐刻对拍用例：打开/关闭交易、工作站补货、交易后升级。
///
/// Villager-trading tick-by-tick case: opening/closing a trade, restocking at a
/// workstation, and levelling up after a trade.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerTradeCase {
    pub name: String,
    pub clock_start: i64,
    pub sensor_delay: i64,
    pub first_id: i32,
    pub player_id: i32,
    pub event_tick: i32,
    pub close_tick: i32,
    pub frames: Vec<VillagerTradeFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerTradeFrame {
    brain: String,
    motion: [i64; 8],
    seed: i64,
    level_seed: i64,
    xp: i32,
    level: i32,
    regen: i32,
    health: i64,
    offers: [i64; 4],
    trading: i32,
}

fn trade_offer(uses: i32) -> mc_bamboo_protocol::merchant::MerchantOffer {
    use mc_bamboo_protocol::merchant::{ItemCost, MerchantOffer};
    MerchantOffer {
        base_cost_a: ItemCost::new(mc_bamboo_data::items::EMERALD, 1),
        cost_b: None,
        result: ItemStack::new(mc_bamboo_data::items::APPLE, 1),
        uses,
        max_uses: 12,
        reward_exp: true,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: 0.05,
        xp: 1,
    }
}

impl Report {
    /// 逐刻重放交易场景，附带 5 处 `--break-self-test` 改坏确认：
    /// `vt_no_follow` / `vt_no_close` / `vt_no_restock` / `vt_no_levelup` / `vt_no_orb_rng`。
    pub(super) fn replay_villager_trade(&mut self, case: &VillagerTradeCase) {
        use mc_bamboo_blocks::entity::mob::villager_ai;
        use mc_bamboo_data::registries::mob_effect;

        self.cases += 1;
        let break_test = self.break_test.clone();
        let break_test = break_test.as_deref();
        let name = case.name.as_str();
        let follow = name == "villager_trade_follow";
        let restock = name == "villager_trade_restock";
        let levelup = name == "villager_trade_levelup";

        let mut blocks_map = std::collections::HashMap::new();
        for x in -15..=15 {
            for z in -15..=15 {
                blocks_map.insert(
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }

        let rules = mc_bamboo_blocks::BlockGameRules::vanilla();
        let players = if follow {
            vec![super::bat::player_view(case.player_id, [2.5, 64.0, 0.5])]
        } else {
            Vec::new()
        };
        let mut level = MobLevel {
            blocks: blocks_map,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules,
            facts: ZombieFacts {
                clock: case.clock_start,
                difficulty: 2,
                can_see_sky: true,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(case.first_id),
            players,
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        };

        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let set_clock = |level: &mut MobLevel, clock: i64| {
            level.facts.clock = clock;
            level.facts.villager_activity = Some(mc_bamboo_blocks::sky::villager_activity(clock));
            level.facts.baby_villager_activity =
                Some(mc_bamboo_blocks::sky::baby_villager_activity(clock));
        };
        set_clock(&mut level, case.clock_start);

        let pos = Vec3::new(0.5, 64.0, 0.5);
        let mut entity = Entity::by_mob_type(MobType::Villager, pos);
        let mut world = World::new(&mut level, &mut ticks);
        entity.with_mob(|_, mob| {
            villager_ai::register_brain_goals(mob, &world, 1000);
        });
        let vid = world.spawn_entity(entity);
        world.with_entity_mut(vid, &mut |entity| {
            entity.y_rot = 0.0;
            entity.old_position = pos;
            entity.random = JavaRandom::new(71);
            entity.with_mob(|me, mob| {
                mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_head_rot_o = 0.0;
                mob.y_body_rot = 0.0;
                mob.y_body_rot_o = 0.0;
                mob.villager.offers = Some(if restock {
                    vec![trade_offer(3), trade_offer(1)]
                } else {
                    vec![trade_offer(0)]
                });
                if levelup {
                    mob.villager.xp = 9;
                    mob.health = 10.0;
                }
                if let Some(brain) = mob.brain.as_mut() {
                    brain.seed_shuffles(me.random.state());
                    for sensor in &mut brain.sensors {
                        sensor.time_to_tick = case.sensor_delay;
                    }
                }
            });
        });

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let now = 1000 + tick as i64;
                ticks.set_time(now);
                set_clock(&mut level, case.clock_start + tick as i64);
                let day = (case.clock_start + tick as i64) / 24_000;

                if tick as i32 == case.event_tick {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(vid, &mut |entity| {
                        entity.with_mob(|me, mob| {
                            if follow {
                                if break_test != Some("vt_no_follow") {
                                    mob.villager.set_trading_player(Some(case.player_id));
                                }
                            } else if restock {
                                if break_test != Some("vt_no_restock")
                                    && mob.villager.should_restock(now, day)
                                {
                                    mob.villager.restock(now);
                                }
                            } else if break_test != Some("vt_no_levelup") {
                                let mut rng = me.random;
                                let orb_base = if break_test == Some("vt_no_orb_rng") {
                                    3
                                } else {
                                    3 + rng.next_int_bounded(4)
                                };
                                me.random = rng;
                                let _ = mc_bamboo_blocks::entity::mob::villager::notify_trade(
                                    mob,
                                    2,
                                    orb_base,
                                    || vec![trade_offer(0), trade_offer(0)],
                                );
                            }
                        });
                    });
                }
                if tick as i32 == case.close_tick && break_test != Some("vt_no_close") {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(vid, &mut |entity| {
                        entity.with_mob(|_, mob| mob.villager.stop_trading());
                    });
                }

                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }

            let Some(entity) = level.entities.get(vid) else {
                self.mismatch(format!("{} tick {tick}: missing villager", case.name));
                return;
            };
            let mob = entity.kind.mob().expect("villager is mob");
            let ours_brain = super::brain::dump(&level, &entity);
            if ours_brain != expected.brain {
                self.mismatch(format!(
                    "{} tick {tick} brain: official {:?}, bamboo {:?}",
                    case.name, expected.brain, ours_brain
                ));
            }
            let (p, v) = (entity.position, entity.delta);
            let ours_motion = [
                p.x.to_bits() as i64,
                p.y.to_bits() as i64,
                p.z.to_bits() as i64,
                v.x.to_bits() as i64,
                v.y.to_bits() as i64,
                v.z.to_bits() as i64,
                i64::from(entity.y_rot.to_bits() as i32),
                i64::from(mob.x_rot.to_bits() as i32),
            ];
            if ours_motion != expected.motion {
                self.mismatch(format!(
                    "{} tick {tick} motion: official {:?}, bamboo {:?}",
                    case.name, expected.motion, ours_motion
                ));
            }
            let ours_seed = entity.random.state();
            if ours_seed != expected.seed {
                self.mismatch(format!(
                    "{} tick {tick} seed: official {}, bamboo {ours_seed}",
                    case.name, expected.seed
                ));
            }
            let ours_level_seed = level.random.source().seed();
            if ours_level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} levelSeed: official {}, bamboo {ours_level_seed}",
                    case.name, expected.level_seed
                ));
            }
            let ours_xp = mob.villager.xp;
            if ours_xp != expected.xp {
                self.mismatch(format!(
                    "{} tick {tick} xp: official {}, bamboo {ours_xp}",
                    case.name, expected.xp
                ));
            }
            if mob.villager.level != expected.level {
                self.mismatch(format!(
                    "{} tick {tick} level: official {}, bamboo {}",
                    case.name, expected.level, mob.villager.level
                ));
            }
            let ours_regen = mob
                .effects
                .get(mob_effect::REGENERATION)
                .map_or(-1, |effect| effect.duration);
            if ours_regen != expected.regen {
                self.mismatch(format!(
                    "{} tick {tick} regen: official {}, bamboo {ours_regen}",
                    case.name, expected.regen
                ));
            }
            let ours_health = i64::from(mob.health.to_bits() as i32);
            if ours_health != expected.health {
                self.mismatch(format!(
                    "{} tick {tick} health: official {}, bamboo {ours_health}",
                    case.name, expected.health
                ));
            }
            let offers = mob.villager.offers.as_deref().unwrap_or(&[]);
            let ours_offers = [
                offers.len() as i64,
                offers.iter().map(|o| i64::from(o.uses)).sum(),
                offers.iter().map(|o| i64::from(o.demand)).sum(),
                offers.iter().map(|o| i64::from(o.special_price_diff)).sum(),
            ];
            if ours_offers != expected.offers {
                self.mismatch(format!(
                    "{} tick {tick} offers: official {:?}, bamboo {:?}",
                    case.name, expected.offers, ours_offers
                ));
            }
            let ours_trading = i32::from(mob.villager.is_trading());
            if ours_trading != expected.trading {
                self.mismatch(format!(
                    "{} tick {tick} trading: official {}, bamboo {ours_trading}",
                    case.name, expected.trading
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks compared", case.name, case.frames.len());
    }
}
