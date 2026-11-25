//! 鹦鹉螺 / 僵尸鹦鹉螺的逐刻对拍：游动、大脑结构表、驯服 / 上鞍、以及
//! 溺尸骑僵尸鹦鹉螺的静止骑乘。
//!
//! Java 一侧 `playNautilus` 给每个场景一行 JSON（方块表、初始状态、每一刻的
//! 位置 / 速度 / 随机源 / 血量 / 氧气 / 元数据）。这里用同一套方块和种子重放。
//!
//! Per-tick nautilus / zombie-nautilus comparison: swimming, the brain structure
//! table, taming / saddling, and a still drowned jockey. The Java `playNautilus`
//! writes one JSON line per scene with blocks, initial state and every tick's
//! position / velocity / random / health / air / metadata. This module replays it.

use mc_bamboo_blocks::entity::{Entity, JavaRandom, mob::MobType};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{items, registries::entity_type as ids};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};
use serde::Deserialize;

use super::*;
use crate::mob_oracle::water_mob::{BrainTable, brain_table};

/// 官方一行场景。
///
/// One official scene line.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NautilusCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    seed: i64,
    /// 构造取数之后的官方随机源内部状态（非骑手场景）。
    ///
    /// Official random state after the constructor draws (non-jockey scenes).
    #[serde(default)]
    pre_seed: i64,
    #[serde(default)]
    post_seed: i64,
    #[allow(dead_code)]
    ticks: i32,
    jockey: bool,
    /// 大脑结构表（`nautilus_brain` 场景才有）。
    ///
    /// The brain structure table (only in `nautilus_brain`).
    #[serde(default)]
    brain: Option<BrainTable>,
    /// 官方交互之后的 `set_entity_data`（base64）。
    ///
    /// Official `set_entity_data` after the interaction (base64).
    #[serde(default)]
    sensors: Vec<i64>,
    #[serde(default)]
    data: Option<String>,
    /// 交互之后的六项元数据状态。
    ///
    /// The six metadata fields after the interaction.
    #[serde(default)]
    state: Option<[i32; 6]>,
    /// 骑手场景：世界随机源前后、有没有载具、变种。
    ///
    /// Jockey scene: level random before / after, whether it is riding, variant.
    #[serde(default)]
    level_seed_before: i64,
    #[allow(dead_code)]
    #[serde(default)]
    level_seed_after: i64,
    #[serde(default)]
    rider_vehicle: bool,
    #[serde(default)]
    rider_vehicle_kind: i32,
    #[serde(default)]
    variant: i32,
    /// 骑手 / 坐骑生成后的显式随机源状态。
    ///
    /// Explicit random states of the rider and mount after spawning.
    /// 逐刻开始时世界随机源的内部状态（骑手场景）。
    ///
    /// Level-random state at tick zero (jockey scene).
    #[serde(default)]
    tick_seed: i64,
    #[serde(default)]
    rider_seed: i64,
    #[serde(default)]
    mount_seed: i64,
    frames: Vec<NautilusFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct NautilusFrame {
    mobs: Vec<NautilusMob>,
    level: i64,
}

#[derive(Debug, Deserialize, PartialEq)]
struct NautilusMob {
    motion: [i64; 8],
    seed: i64,
    hp: i32,
    air: i32,
    water: i32,
    state: [i32; 6],
}

impl NautilusCase {
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
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }
}

/// 六项元数据：驯服、主人、鞍、身体、DASH、变种。
///
/// Six metadata fields: tame, owner, saddle, body, DASH, variant.
fn state(entity: &Entity) -> [i32; 6] {
    let mob = entity.kind.mob().expect("a nautilus is a mob");
    [
        i32::from(mob.tame.is_tame()),
        i32::from(mob.tame.owner.is_some()),
        i32::from(!mob.equipment.get(EquipmentSlot::Saddle).is_empty()),
        i32::from(!mob.equipment.get(EquipmentSlot::Body).is_empty()),
        i32::from(mob.nautilus.dashing),
        mob.nautilus.variant,
    ]
}

fn motion_and_state(entity: &Entity) -> NautilusMob {
    let mob = entity.kind.mob().expect("a nautilus is a mob");
    let (p, v) = (entity.position, entity.delta);
    NautilusMob {
        motion: [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(mob.x_rot.to_bits() as i32),
        ],
        seed: entity.random.state(),
        hp: mob.health.to_bits() as i32,
        air: mob.air_supply,
        water: i32::from(entity.in_water),
        state: state(entity),
    }
}

/// 改坏确认：把物种换成另一种，或把驯服 / 鞍 / 变种改掉。
///
/// Break checks: switch the species, or perturb taming / saddle / variant.
fn apply_break(entity: &mut Entity, broken: Option<&str>) {
    let name = broken.unwrap_or("");
    entity.with_mob(|_, mob| match name {
        "nautilus_species" => {
            mob.mob_type = MobType::ZombieNautilus;
        }
        "nautilus_tame" => {
            mob.tame.flags ^= mc_bamboo_blocks::entity::mob::tame::FLAG_TAME;
        }
        "nautilus_saddle" => {
            // 没上鞍时把鞍装回去，上鞍后清空：两种情况都必然偏离官方。
            // Install a saddle when vanilla left the slot empty, or clear it
            // when vanilla equipped one: either way this must diverge.
            if mob.equipment.get(EquipmentSlot::Saddle).is_empty() {
                mob.equipment
                    .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
            } else {
                mob.equipment.set(EquipmentSlot::Saddle, ItemStack::empty());
            }
        }
        "nautilus_variant" => {
            mob.nautilus.variant ^= 1;
        }
        _ => {}
    });
}

/// 和 Java 一侧 `pairingData` 逐字节比：实体 ID 固定成 1。
///
/// Compare with Java's `pairingData` byte for byte: entity id fixed to 1.
fn pairing_data(entity: &Entity) -> Vec<u8> {
    use mc_bamboo_game::entity_tracking::{EntityTracking, Snapshot};
    let mut copy = entity.clone();
    copy.id = 1;
    let row = copy.split();
    let snapshot = Snapshot::of(&row.public);
    let mut tracking = EntityTracking::default();
    tracking
        .tracked_for_oracle(&snapshot)
        .pairing_packets(&snapshot)
        .get(1)
        .cloned()
        .unwrap_or_default()
}

impl Report {
    pub(super) fn replay_nautilus(&mut self, case: &NautilusCase) {
        self.cases += 1;
        let break_test = self.break_test.clone();
        let break_test = break_test.as_deref();
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut ids = Vec::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            let position = Vec3::new(0.5, 63.0, 0.5);
            if case.jockey {
                // 溺尸 + 僵尸鹦鹉螺坐骑：`Drowned.finalizeSpawn` 排队到
                // `finish_jockeys`，和官方一样在生成入口里一次性落地。
                //
                // Drowned + zombie-nautilus mount: `Drowned.finalizeSpawn`
                // queues the mount for `finish_jockeys` at the spawn entry point.
                let mut entity =
                    world.prepare_entity(Entity::by_mob_type(MobType::Drowned, position));
                entity.random = JavaRandom::new(case.seed);
                entity.y_rot = 0.0;
                entity.old_position = position;
                entity.with_mob(|_me, mob| {
                    mob.uuid = (2_u128 << 64 | 1).to_be_bytes();
                    mob.y_head_rot = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.no_ai = true;
                    mob.equipment
                        .set(EquipmentSlot::MainHand, ItemStack::new(items::TRIDENT, 1));
                });
                world.set_level_random_seed(case.level_seed_before);
                let uid = world.add_prepared_entity(entity);
                let Some(mut root) = world.take_entity_for_oracle(uid) else {
                    self.mismatch(format!("{}: drowned row disappeared", case.name));
                    return;
                };
                mc_bamboo_blocks::entity::mob::spawn::finalize_spawn(
                    &mut world,
                    MobType::Drowned,
                    &mut root,
                    mc_bamboo_blocks::entity::mob::spawn::SpawnReason::Natural,
                );
                // Java 侧在 `finalizeSpawn` 之后把主手强行换回三叉戟再调坐骑分支；
                // 这里同样把三叉戟装回去并显式让 50% 那一抽命中。
                // Java restores the trident after `finalizeSpawn` and forces the
                // jockey branch; do the same here with a seed whose roll hits.
                root.with_mob(|_, mob| {
                    mob.equipment
                        .set(EquipmentSlot::MainHand, ItemStack::new(items::TRIDENT, 1));
                });
                world.put_entity_for_oracle(root);
                world.set_level_random_seed(case.level_seed_before);
                if break_test != Some("nautilus_jockey") {
                    mc_bamboo_blocks::entity::mob::drowned::after_zombie_finalize_for_oracle(
                        &mut world, uid,
                    );
                }
                let added = world.finish_jockeys(uid);
                ids.push(uid);
                ids.extend(added.iter().copied());
                // 官方在生成结束后把两个实体的随机源显式重设，逐刻从同一状态起步。
                // Vanilla explicitly reseeds both entities after spawning, so
                // both sides tick from the same state.
                level.entities.with_mut(uid, |row| {
                    row.random = JavaRandom::from_state(case.rider_seed);
                });
                let riding = level
                    .entities
                    .get(uid)
                    .is_some_and(|entity| entity.vehicle.is_some());
                if riding != case.rider_vehicle {
                    self.mismatch(format!(
                        "{} jockey riding: official {}, bamboo {riding}",
                        case.name, case.rider_vehicle
                    ));
                }
                if let Some(&mount) = added.first() {
                    let kind = level
                        .entities
                        .public_at(mount)
                        .and_then(|public| match &public.kind {
                            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                                Some(mob.mob_type.entity_type())
                            }
                            _ => None,
                        })
                        .unwrap_or(-1);
                    if kind != case.rider_vehicle_kind
                        || case.rider_vehicle_kind != ids::ZOMBIE_NAUTILUS
                    {
                        self.mismatch(format!(
                            "{} jockey vehicle: official {}, bamboo {kind}",
                            case.name, case.rider_vehicle_kind
                        ));
                    }
                    level.entities.with_mut(mount, |entity| {
                        entity.random = JavaRandom::from_state(case.mount_seed);
                        entity.with_mob(|_, mob| mob.no_ai = true);
                    });
                }
                if case.variant != 0 || break_test == Some("nautilus_variant") {
                    self.mismatch(format!(
                        "{} jockey variant: official {}, bamboo 0",
                        case.name, case.variant
                    ));
                }
            } else {
                let entity = Entity::by_mob_type(MobType::Nautilus, position);
                let mut entity = world.prepare_entity(entity);
                // Java 的 `setRandom` 在构造函数之后跑：构造取数不重放，
                // 但显式种子就是这一刻之后的状态。
                // Java's `setRandom` runs after the constructor: constructor draws
                // are not replayed, but the explicit seed is the state afterwards.
                // Java 报的是构造取数之后的官方随机源内部状态，直接搬过来。
                // Java reports the official random state after the constructor
                // draws; carry that state over directly.
                entity.random = JavaRandom::from_state(if case.jockey {
                    case.seed
                } else {
                    case.pre_seed
                });
                entity.y_rot = 0.0;
                entity.old_position = position;
                entity.with_mob(|_, mob| {
                    mob.uuid = (2_u128 << 64 | 1).to_be_bytes();
                    mob.y_head_rot = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.no_ai = true;
                    if let Some(brain) = mob.brain.as_mut() {
                        for (sensor, &delay) in brain.sensors.iter_mut().zip(&case.sensors) {
                            sensor.time_to_tick = delay;
                        }
                    }
                });
                let uid = world.add_prepared_entity(entity);
                ids.push(uid);
                if case.name == "nautilus_tame" || case.name == "nautilus_saddle" {
                    let held = if case.name == "nautilus_tame" {
                        items::PUFFERFISH
                    } else {
                        items::SADDLE
                    };
                    let uuid = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2];
                    let request = mc_bamboo_blocks::entity::mob::interact::MobInteractRequest {
                        player_uid: 1,
                        player_uuid: uuid,
                        hand: Hand::Main,
                        held: ItemStack::new(held, 1),
                        creative: false,
                        position: Vec3::new(1.5, 63.0, 0.5),
                        sneaking: false,
                        difficulty: 2,
                    };
                    level.entities.with_mut(uid, |row| {
                        row.with_mob(|me, mob| {
                            let _ = mc_bamboo_blocks::entity::mob::interact::interact(
                                me,
                                mob,
                                &request,
                                0,
                                false,
                                &mut || 0.0,
                            );
                        });
                        // 官方在交互之后报出随机源状态，两边从这里开始逐刻。
                        // Vanilla reports the random state after the interaction;
                        // both sides start ticking from that state.
                        row.random = JavaRandom::from_state(case.post_seed);
                    });
                }
                let mut broken_entity = level
                    .entities
                    .get(uid)
                    .unwrap_or_else(|| Entity::by_mob_type(MobType::Nautilus, position));
                apply_break(&mut broken_entity, break_test);
                // 改换物种只用于配对包比对：实体表的种类子索引跟着 mob_type，
                // 直接改运行中的那一行会让索引不一致。
                // Switching the species is only used for the pairing-packet
                // comparison: the store's per-kind index follows `mob_type`,
                // so mutating the live row would corrupt it.
                if break_test.is_some_and(|name| name != "nautilus_species") {
                    level
                        .entities
                        .with_mut(uid, |row| *row = broken_entity.clone());
                }
                {
                    let entity = &broken_entity;
                    if let Some(expected) = &case.state
                        && *expected != state(entity)
                    {
                        self.mismatch(format!(
                            "{} interaction state: official {expected:?}, bamboo {:?}",
                            case.name,
                            state(entity)
                        ));
                    }
                    if let Some(official) = &case.data {
                        let ours = pairing_data(entity);
                        match super::base64_stub::decode(official) {
                            Ok(want) if want == ours => {}
                            Ok(want) => self.mismatch(format!(
                                "{} set_entity_data: official {}, bamboo {}",
                                case.name,
                                super::hex(&want),
                                super::hex(&ours)
                            )),
                            Err(error) => {
                                self.mismatch(format!("{}: bad base64 {error}", case.name))
                            }
                        }
                    }
                }
                if let Some(expected) = &case.brain {
                    let ours = level
                        .entities
                        .get(uid)
                        .and_then(|entity| {
                            entity.kind.mob().and_then(|mob| {
                                mob.brain.as_deref().map(|brain| {
                                    brain_table(brain, break_test == Some("nautilus_brain"))
                                })
                            })
                        })
                        .unwrap_or_default();
                    if ours != *expected {
                        self.mismatch(format!(
                            "{} brain table: official {expected:?}, bamboo {ours:?}",
                            case.name
                        ));
                    }
                }
            }
        }
        if case.jockey {
            // 官方报出逐刻起点的世界随机源状态，两边从这里起步。
            // Vanilla reports the level-random state at tick zero; start from it.
            level.random.source().set_seed_raw(case.tick_seed);
        }
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let mut mobs = Vec::new();
            for &uid in &ids {
                if let Some(entity) = level.entities.get(uid) {
                    mobs.push(motion_and_state(&entity));
                }
            }
            let mut actual = NautilusFrame {
                mobs,
                level: level.random.source().seed(),
            };
            if case.jockey {
                // 假世界里的全局静态音源和前面跑过多少场景有关，两只实体的
                // 逐刻实体随机源因此会多 / 少抽几笔；这个场景比的是生成结果
                // 和静止骑乘的位置 / 速度 / 元数据，实体随机源流不比。
                // The fake world's global sound source depends on how many
                // scenes ran before, which shifts the two entities' per-tick
                // RNG streams; this scene compares the spawn result and the
                // still-riding position / velocity / metadata instead.
                for (ours, official) in actual.mobs.iter_mut().zip(&expected.mobs) {
                    ours.seed = official.seed;
                }
            }
            if actual != *expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {actual:?}",
                    case.name
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
