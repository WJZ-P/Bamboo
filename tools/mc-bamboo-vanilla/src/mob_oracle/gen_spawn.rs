//! 用官方的群系刷怪表（平原、针叶林、雪地针叶林，另有蘑菇岛和冰冻海洋的少量 misc 场景）
//! 和公式地形重放生成期刷怪（`NaturalSpawner.spawnMobsForChunkGeneration`）。
//!
//! 装饰随机源的取数必须完全一致（结束时比一次指纹）；Bamboo 建模了的生物比类型、坐标、朝向。
//! 幼年、左撇子、兔子 / 狐狸 / 狼的品种明细和马科的属性花色取区域随机源，只在这一区块
//! 前面没有生成过 Bamboo 没建模的动物时比较：那些动物在官方那边会跑自己的 `finalizeSpawn`。
//!
//! Replay world-generation spawning with the official biome tables (plains, taiga,
//! snowy taiga, plus a few mushroom-field and frozen-ocean misc scenes) over a formula
//! terrain. The decoration random must match exactly (a fingerprint at the end); every
//! mob Bamboo models compares type, position and facing. Baby, left-handedness, the
//! rabbit / fox / wolf variant details and the equine attributes come from the region
//! random and are compared only while no unmodelled animal spawned before in the chunk,
//! since vanilla runs those animals' `finalizeSpawn`.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LegacyRandom, LevelRandom, World,
    entity::{
        Entities, Entity,
        mob::{
            Mob, MobType,
            natural_gen::{GenSpawn, spawn_mobs_for_chunk_generation},
        },
    },
    random::{RandomSource, XoroshiroRandom},
};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{
    blocks,
    registries::biome,
    spawning::{self, MobCategory},
};
use serde::Deserialize;

use super::{
    MobLevel, Report, ZombieFacts,
    natural_spawn::{SceneFacts, spawn_floor},
};

#[derive(Debug, Deserialize)]
pub(super) struct Case {
    name: String,
    biome: String,
    seed: i64,
    sky: i32,
    block: i32,
    terrain: bool,
    /// 每个样本是 `[行, 农场生物的品种与叫声]`；非农场生物的第二项为 null。
    ///
    /// Each sample is `[row, farm spawn variants]`; the second item is null for
    /// mobs that are not farm animals.
    spawned: Vec<(Vec<i64>, Option<[String; 2]>)>,
    /// 和 `spawned` 一一对应：`MobOracle.genSpawnDetail`。
    details: Vec<String>,
    random: i64,
    region: i64,
}

#[derive(Default)]
pub(super) struct Coverage {
    pub(super) cases: usize,
    /// 成功比对过生成品种的牛、猪、羊、鸡，按此顺序计数。
    ///
    /// Cows, pigs, sheep and chickens whose spawn variants matched, in that order.
    farms: [usize; 4],
    equines: usize,
    others: usize,
    simulated: usize,
    details: usize,
    /// 哞菇、北极熊各生成了多少只。
    /// Spawned mooshrooms and polar bears.
    mooshrooms: usize,
    bears: usize,
    /// 蘑菇岛和冰冻海洋对拍的例数。
    /// Mushroom-field and frozen-ocean fixture cases.
    misc_cases: usize,
    panda_cases: usize,
    pandas_compared: usize,
    /// 兔子、狐狸、狼各生成了多少只，以及其中的幼崽和叼着东西的狐狸。
    /// Spawned rabbits, foxes and wolves, plus babies among them and foxes with an item.
    wild: [usize; 3],
    wild_babies: usize,
    fox_items: usize,
}

impl Coverage {
    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "GEN SPAWN: {} cases, {} equines, {} wild animals, {} other animals, {} simulated animals, {} mooshrooms, {} polar bears, {} mobs with finalize details compared",
                self.cases,
                self.equines,
                self.wild.iter().sum::<usize>(),
                self.others,
                self.simulated,
                self.mooshrooms,
                self.bears,
                self.details
            );
            println!(
                "GEN SPAWN: matched spawn variants for {} cows, {} pigs, {} sheep and {} chickens",
                self.farms[0], self.farms[1], self.farms[2], self.farms[3]
            );
            println!(
                "GEN SPAWN: {} rabbits, {} foxes, {} wolves, {} babies among them, {} foxes holding an item",
                self.wild[0], self.wild[1], self.wild[2], self.wild_babies, self.fox_items
            );
            if self.panda_cases > 0 {
                println!(
                    "GEN SPAWN: {} pandas with both genes compared",
                    self.pandas_compared
                );
            }
        }
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.cases == 0 {
            return Ok(());
        }
        if self.panda_cases > 0 {
            return if self.pandas_compared > 0 {
                Ok(())
            } else {
                Err("jungle world-gen fixtures did not compare any panda genes".into())
            };
        }
        if self.misc_cases > 0 {
            return if self.mooshrooms > 0 && self.bears > 0 && self.simulated == 0 {
                Ok(())
            } else {
                Err("misc world-gen fixtures did not spawn both animals".into())
            };
        }
        if self.farms.contains(&0) {
            return Err(format!(
                "gen spawn fixtures must compare variants for cows, pigs, sheep and chickens; got {:?}",
                self.farms
            ));
        }
        if self.equines == 0 || self.details == 0 {
            return Err("gen spawn fixtures did not exercise equines and finalize details".into());
        }
        if self.wild.contains(&0) || self.wild_babies == 0 || self.fox_items == 0 {
            return Err("gen spawn fixtures did not exercise rabbits, foxes and wolves".into());
        }
        Ok(())
    }

    fn record_farm(&mut self, kind: MobType) {
        let farm = match kind {
            MobType::Cow => 0,
            MobType::Pig => 1,
            MobType::Sheep => 2,
            MobType::Chicken => 3,
            _ => return,
        };
        self.farms[farm] += 1;
    }
}

impl Report {
    pub(super) fn replay_gen_spawn(&mut self, case: &Case) {
        self.cases += 1;
        self.gen_spawns.cases += 1;
        let broken = self.break_test.as_deref() == Some("gen_spawn");
        let biome_id = biome::NAMES
            .iter()
            .position(|name| *name == case.biome)
            .expect("known fixture biome") as i32;
        self.gen_spawns.misc_cases += usize::from(matches!(
            biome_id,
            biome::MUSHROOM_FIELDS | biome::FROZEN_OCEAN
        ));
        self.gen_spawns.panda_cases += usize::from(biome_id == biome::BAMBOO_JUNGLE);
        let mut level = fixture(case, biome_id);
        let mut ticks = BlockTicks::new();
        let decoration = mc_bamboo_worldgen::random::WorldgenRandom::new(
            mc_bamboo_worldgen::random::Random::Legacy(LegacyRandom::new(0)),
        )
        .set_decoration_seed(case.seed, 0, 0);
        let random = LegacyRandom::new(decoration);
        let region = XoroshiroRandom::from_seed(case.seed.wrapping_mul(31).wrapping_add(7));
        let table = spawning::for_biome_id(biome_id);
        let probability = if broken && case.name == "gen_flat" {
            table.creature_spawn_probability * 2.0
        } else {
            table.creature_spawn_probability
        };
        let mobs = {
            let mut world = World::new(&mut level, &mut ticks);
            spawn_mobs_for_chunk_generation(
                &mut world,
                &GenSpawn {
                    chunk: ChunkPos::new(0, 0),
                    mobs: table.mobs_to_spawn(MobCategory::Creature),
                    probability,
                    random: &random,
                    region_random: &region,
                },
            )
        };
        self.compare_gen_spawn(case, &mobs, &random, &region);
    }

    fn compare_gen_spawn(
        &mut self,
        case: &Case,
        mobs: &[Entity],
        random: &LegacyRandom,
        region: &XoroshiroRandom,
    ) {
        let label = format!("{} seed {}", case.name, case.seed);
        if case.spawned.len() != case.details.len() {
            self.mismatch(format!(
                "{label}: spawn rows and variant details differ in count"
            ));
            return;
        }
        let mut in_sync = true;
        let mut ours = mobs.iter();
        for ((row, variants), detail) in case.spawned.iter().zip(&case.details) {
            let kind = i32::try_from(row[0]).ok().and_then(MobType::by_entity_type);
            let Some(kind) = kind else {
                self.gen_spawns.simulated += 1;
                in_sync = false;
                continue;
            };
            self.gen_spawns.mooshrooms += usize::from(kind == MobType::Mooshroom);
            self.gen_spawns.bears += usize::from(kind == MobType::PolarBear);
            if kind.is_equine() {
                self.gen_spawns.equines += 1;
            } else {
                self.gen_spawns.others += 1;
            }
            let Some(entity) = ours.next() else {
                self.mismatch(format!("{label}: official {row:?} missing in Bamboo"));
                return;
            };
            let mob = entity.kind.mob().expect("generated mobs are mobs");
            let place = placement(entity);
            if place[..] != row[..5] {
                self.mismatch(format!(
                    "{label}: official {row:?}, Bamboo placement {place:?}"
                ));
            }
            if in_sync {
                self.gen_spawns.details += 1;
                self.gen_spawns.record_wild(mob);
                let ours_details = details(entity);
                self.gen_spawns.pandas_compared += usize::from(kind == MobType::Panda);
                let ours_detail = finalize_detail(mob);
                if ours_details[..] != row[5..] || &ours_detail != detail {
                    self.mismatch(format!(
                        "{label}: official {row:?} {detail:?}, Bamboo details {ours_details:?} {ours_detail:?}"
                    ));
                }
                let ours_variants = super::spawn_variants(mob);
                if &ours_variants != variants {
                    self.mismatch(format!(
                        "{label}: official spawn variants {variants:?}, Bamboo {ours_variants:?}"
                    ));
                } else if variants.is_some() {
                    self.gen_spawns.record_farm(kind);
                }
            }
        }
        if let Some(extra) = ours.next() {
            self.mismatch(format!(
                "{label}: Bamboo spawned an extra mob at {:?}",
                extra.position
            ));
        }
        let fingerprint = random.next_long();
        if fingerprint != case.random {
            self.mismatch(format!(
                "{label}: decoration random official {}, Bamboo {fingerprint}",
                case.random
            ));
        }
        if in_sync && region.next_long() != case.region {
            self.mismatch(format!("{label}: region random drifted"));
        }
    }
}

impl Coverage {
    fn record_wild(&mut self, mob: &Mob) {
        let slot = match mob.mob_type {
            MobType::Rabbit => 0,
            MobType::Fox => 1,
            MobType::Wolf => 2,
            _ => return,
        };
        self.wild[slot] += 1;
        self.wild_babies += usize::from(mob.is_baby());
        self.fox_items += usize::from(slot == 1 && mouth_item(mob) != mc_bamboo_data::items::AIR);
    }
}

fn mouth_item(mob: &Mob) -> i32 {
    mob.equipment
        .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
        .item
}

/// 和 `MobOracle.genSpawnDetail` 同一个格式。
///
/// The same format as `MobOracle.genSpawnDetail`.
fn finalize_detail(mob: &Mob) -> String {
    match mob.mob_type {
        MobType::Rabbit => format!("rabbit|{}", mob.rabbit.variant.id()),
        MobType::Fox => format!("fox|{}|{}", mob.fox.variant.id(), mouth_item(mob)),
        MobType::Wolf => format!(
            "wolf|{}|{}",
            mob.wolf.variant_name(),
            mob.wolf.sound_variant_name()
        ),
        MobType::Panda => format!(
            "panda|{}|{}",
            mob.panda.main_gene.id(),
            mob.panda.hidden_gene.id()
        ),
        _ => String::new(),
    }
}

/// 类型、坐标原始位和朝向原始位。
///
/// Type, raw coordinate bits and raw yaw bits.
fn placement(entity: &Entity) -> [i64; 5] {
    let mob = entity.kind.mob().expect("generated mobs are mobs");
    [
        i64::from(mob.mob_type.entity_type()),
        entity.position.x.to_bits() as i64,
        entity.position.y.to_bits() as i64,
        entity.position.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
    ]
}

/// 幼年、左撇子；马科再加 `super::equine::state`。
///
/// Baby and left-handedness, plus `super::equine::state` for equines.
fn details(entity: &Entity) -> Vec<i64> {
    let mob = entity.kind.mob().expect("generated mobs are mobs");
    let mut details = vec![i64::from(mob.is_baby()), i64::from(mob.left_handed)];
    if mob.mob_type.is_equine() {
        details.extend(super::equine::state(entity));
    }
    details
}

/// 和 `MobOracle.genColumn` 同一个坐标公式。
fn gen_column(x: i32, z: i32, terrain: bool, floor: BlockState) -> Vec<(BlockPos, BlockState)> {
    let top = if terrain {
        -63 + (x * 7 + z * 13).rem_euclid(3)
    } else {
        -63
    };
    let mut column: Vec<_> = (-63..top)
        .map(|y| (BlockPos::new(x, y, z), BlockState(blocks::STONE)))
        .collect();
    let ground = if terrain && (x + 2 * z).rem_euclid(9) == 0 {
        BlockState(blocks::WATER)
    } else if terrain && (x * 3 + z * 5).rem_euclid(11) == 0 {
        BlockState(blocks::SAND)
    } else {
        floor
    };
    column.push((BlockPos::new(x, top, z), ground));
    let above = BlockPos::new(x, top + 1, z);
    if terrain && (5 * x + z).rem_euclid(13) == 0 {
        column.push((above, BlockState(blocks::OAK_LEAVES)));
    } else if terrain && (x - z).rem_euclid(4) == 0 {
        column.push((above, BlockState(blocks::SHORT_GRASS)));
    }
    column
}

fn fixture(case: &Case, biome_id: i32) -> MobLevel {
    let floor = spawn_floor(biome_id);
    let blocks = (-16..32)
        .flat_map(|x| (-16..32).flat_map(move |z| gen_column(x, z, case.terrain, floor)))
        .collect();
    let entry = spawning::for_biome_id(biome_id).mobs_to_spawn(MobCategory::Creature);
    MobLevel {
        blocks,
        random: LevelRandom::new(0),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        biome: None,
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: 2,
            ..ZombieFacts::default()
        },
        natural_spawn: Some(SceneFacts {
            biome: biome_id,
            category: MobCategory::Creature,
            entry: &entry[0],
            sky: case.sky,
            block: case.block,
            thunder: false,
        }),
        redstone: Default::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        game_event_listeners: Default::default(),
        warden_warnings: Default::default(),
        darkness_ticks: 0,
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: Entities::with_next_id(2),
        players: Vec::new(),
        game: None,
        inhabited: Default::default(),
        mob_teams: Default::default(),
        ignore_bed_monsters: false,
        short_hunger: false,
        wrong_was_hurt: false,
        containers: Default::default(),
        explosion_break: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::Coverage;

    #[test]
    fn gen_spawn_requires_each_farm_species_in_variant_comparisons() {
        let mut coverage = Coverage {
            cases: 1,
            farms: [1; 4],
            equines: 1,
            details: 8,
            wild: [1; 3],
            wild_babies: 1,
            fox_items: 1,
            ..Coverage::default()
        };
        assert!(coverage.check().is_ok());
        for i in 0..coverage.farms.len() {
            coverage.farms[i] = 0;
            assert!(coverage.check().is_err());
            coverage.farms[i] = 1;
        }
        assert!(Coverage::default().check().is_ok());
    }
}
