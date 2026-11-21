//! 用官方平原刷怪表和公式地形重放生成期刷怪（`NaturalSpawner.spawnMobsForChunkGeneration`）。
//!
//! 装饰随机源的取数必须完全一致（结束时比一次指纹）；生成的牛和马科比类型、坐标、朝向。
//! 幼年、左撇子和马科的属性花色取区域随机源，只在这一区块前面没有生成过 Bamboo
//! 还没建模的动物时比较：那些动物在官方那边会跑自己的 `finalizeSpawn`。
//!
//! Replay world-generation spawning with the official plains table over a formula
//! terrain. The decoration random must match exactly (a fingerprint at the end);
//! spawned cows and equines compare type, position and facing. Baby, left-handedness
//! and equine attributes and variants come from the region random and are compared
//! only while no animal Bamboo does not model yet spawned before in the chunk, since
//! vanilla runs those animals' `finalizeSpawn`.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LegacyRandom, LevelRandom, World,
    entity::{
        Entities, Entity,
        mob::{
            MobType,
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

use super::{MobLevel, Report, ZombieFacts, natural_spawn::SceneFacts};

#[derive(Debug, Deserialize)]
pub(super) struct Case {
    name: String,
    seed: i64,
    sky: i32,
    block: i32,
    terrain: bool,
    spawned: Vec<Vec<i64>>,
    random: i64,
    region: i64,
}

#[derive(Default)]
pub(super) struct Coverage {
    pub(super) cases: usize,
    cows: usize,
    equines: usize,
    others: usize,
    details: usize,
}

impl Coverage {
    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "GEN SPAWN: {} cases, {} cows, {} equines, {} other animals, {} mobs with finalize details compared",
                self.cases, self.cows, self.equines, self.others, self.details
            );
        }
    }

    pub(super) fn check(&self) -> Result<(), String> {
        let missing = self.cows == 0 || self.equines == 0 || self.others == 0;
        if self.cases > 0 && (missing || self.details == 0) {
            return Err("gen spawn fixtures did not exercise cows and other animals".into());
        }
        Ok(())
    }
}

impl Report {
    pub(super) fn replay_gen_spawn(&mut self, case: &Case) {
        self.cases += 1;
        self.gen_spawns.cases += 1;
        let broken = self.break_test.as_deref() == Some("gen_spawn");
        let mut level = fixture(case);
        let mut ticks = BlockTicks::new();
        let decoration = mc_bamboo_worldgen::random::WorldgenRandom::new(
            mc_bamboo_worldgen::random::Random::Legacy(LegacyRandom::new(0)),
        )
        .set_decoration_seed(case.seed, 0, 0);
        let random = LegacyRandom::new(decoration);
        let region = XoroshiroRandom::from_seed(case.seed.wrapping_mul(31).wrapping_add(7));
        let table = spawning::for_biome_id(biome::PLAINS);
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
        let mut in_sync = true;
        let mut ours = mobs.iter();
        for row in &case.spawned {
            let Some(kind) = modelled(row[0]) else {
                self.gen_spawns.others += 1;
                in_sync = false;
                continue;
            };
            if kind == MobType::Cow {
                self.gen_spawns.cows += 1;
            } else {
                self.gen_spawns.equines += 1;
            }
            let Some(entity) = ours.next() else {
                self.mismatch(format!("{label}: official {row:?} missing in Bamboo"));
                return;
            };
            let place = placement(entity);
            if place[..] != row[..5] {
                self.mismatch(format!(
                    "{label}: official {row:?}, Bamboo placement {place:?}"
                ));
            }
            if in_sync {
                self.gen_spawns.details += 1;
                let details = details(entity);
                if details[..] != row[5..] {
                    self.mismatch(format!(
                        "{label}: official {row:?}, Bamboo details {details:?}"
                    ));
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

/// Bamboo 已经建模、会真的生成的种类。
///
/// Kinds Bamboo models and really spawns.
fn modelled(entity: i64) -> Option<MobType> {
    let kind = MobType::by_entity_type(i32::try_from(entity).ok()?)?;
    (kind == MobType::Cow || kind.is_equine()).then_some(kind)
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

/// 幼年、左撇子；马科再加 [`super::equine::state`]。
///
/// Baby and left-handedness, plus [`super::equine::state`] for equines.
fn details(entity: &Entity) -> Vec<i64> {
    let mob = entity.kind.mob().expect("generated mobs are mobs");
    let mut details = vec![i64::from(mob.is_baby()), i64::from(mob.left_handed)];
    if mob.mob_type.is_equine() {
        details.extend(super::equine::state(entity));
    }
    details
}

/// 和 `MobOracle.genColumn` 同一个坐标公式。
fn gen_column(x: i32, z: i32, terrain: bool) -> Vec<(BlockPos, BlockState)> {
    let top = if terrain {
        -63 + (x * 7 + z * 13).rem_euclid(3)
    } else {
        -63
    };
    let mut column: Vec<_> = (-63..top)
        .map(|y| (BlockPos::new(x, y, z), BlockState(blocks::STONE)))
        .collect();
    let ground = if terrain && (x + 2 * z).rem_euclid(9) == 0 {
        blocks::WATER
    } else if terrain && (x * 3 + z * 5).rem_euclid(11) == 0 {
        blocks::SAND
    } else {
        blocks::GRASS_BLOCK
    };
    column.push((BlockPos::new(x, top, z), BlockState(ground)));
    let above = BlockPos::new(x, top + 1, z);
    if terrain && (5 * x + z).rem_euclid(13) == 0 {
        column.push((above, BlockState(blocks::OAK_LEAVES)));
    } else if terrain && (x - z).rem_euclid(4) == 0 {
        column.push((above, BlockState(blocks::SHORT_GRASS)));
    }
    column
}

fn fixture(case: &Case) -> MobLevel {
    let blocks = (-16..32)
        .flat_map(|x| (-16..32).flat_map(move |z| gen_column(x, z, case.terrain)))
        .collect();
    let entry = spawning::for_biome_id(biome::PLAINS).mobs_to_spawn(MobCategory::Creature);
    MobLevel {
        blocks,
        random: LevelRandom::new(0),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: 2,
            ..ZombieFacts::default()
        },
        natural_spawn: Some(SceneFacts {
            biome: biome::PLAINS,
            category: MobCategory::Creature,
            entry: &entry[0],
            sky: case.sky,
            block: case.block,
            thunder: false,
        }),
        redstone: Default::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        entities: Entities::with_next_id(2),
        players: Vec::new(),
        game: None,
        wrong_was_hurt: false,
    }
}
