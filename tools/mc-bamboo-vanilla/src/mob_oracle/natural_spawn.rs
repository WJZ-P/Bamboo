//! 用官方刷怪表的单个条目、固定区块顺序重放自然生成的一刻。
//!
//! Replay natural spawning with a single official table entry and fixed chunk order.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LevelRandom, World,
    entity::{
        Entities, Entity,
        ctx::PlayerView,
        mob::{
            MobType,
            natural::{self, SpawnParams, SpawnState},
        },
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{
    blocks,
    registries::{biome, entity_type},
    spawning::{self, MobCategory, MobSpawnerDef},
};
use mc_bamboo_protocol::packet::play::EquipmentSlot;
use serde::Deserialize;

use super::{MobLevel, Report, ZombieFacts};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    name: String,
    entity_type: i32,
    biome: String,
    seed: i64,
    difficulty: i32,
    sky: i32,
    block: i32,
    darken: i32,
    thunder: bool,
    player_x: f64,
    population: usize,
    spawnable_chunks: i32,
    local_cap: bool,
    nearby_respawn: bool,
    halloween: bool,
    steps: Vec<Step>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    time: i64,
    count: i32,
    world_seed: i64,
    spawned: Vec<[i32; 8]>,
}

pub(super) struct SceneFacts {
    pub(super) biome: i32,
    pub(super) category: MobCategory,
    pub(super) entry: &'static MobSpawnerDef,
    pub(super) sky: i32,
    pub(super) block: i32,
    pub(super) thunder: bool,
}

#[derive(Default)]
pub(super) struct Coverage {
    cases: usize,
    pub(super) ticks: usize,
    cows: usize,
    skeletons: usize,
    zombies: usize,
    without_spawn: usize,
    cap_off: usize,
    per_scene: std::collections::BTreeMap<String, usize>,
}

impl Coverage {
    fn record(&mut self, case: &Case, step: &Step) {
        self.ticks += 1;
        *self.per_scene.entry(case.name.clone()).or_default() += step.spawned.len();
        self.without_spawn += usize::from(step.spawned.is_empty());
        self.cows += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::COW)
            .count();
        self.skeletons += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::SKELETON)
            .count();
        self.zombies += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::ZOMBIE || row[0] == entity_type::HUSK)
            .count();
        if case.name == "cow_no_local_cap" {
            self.cap_off += step.spawned.len();
        }
    }

    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "NATURAL SPAWN: {} cases, {} ticks, {} cows, {} skeletons, {} zombies or husks, {} empty ticks, {} spawns with local cap bypassed",
                self.cases,
                self.ticks,
                self.cows,
                self.skeletons,
                self.zombies,
                self.without_spawn,
                self.cap_off
            );
            let scenes: Vec<String> = self
                .per_scene
                .iter()
                .map(|(name, count)| format!("{name}={count}"))
                .collect();
            println!("NATURAL SPAWN per scene: {}", scenes.join(" "));
        }
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.cases > 0
            && (self.cows == 0 || self.skeletons == 0 || self.zombies == 0 || self.cap_off == 0)
        {
            return Err(
                "natural spawn fixtures did not exercise every mob and the local cap bypass".into(),
            );
        }
        Ok(())
    }
}

impl Report {
    pub(super) fn replay_natural_spawn(&mut self, case: &Case) -> Result<(), String> {
        self.cases += 1;
        self.natural_spawns.cases += 1;
        let kind = MobType::by_entity_type(case.entity_type).ok_or_else(|| {
            format!(
                "unsupported natural spawn fixture type {}",
                case.entity_type
            )
        })?;
        let broken = self.break_test.as_deref() == Some("natural_spawn");
        let mut level = fixture(case, kind, broken)?;
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for _ in 0..case.population {
                world.spawn_entity(Entity::by_mob_type(kind, Vec3::new(8.5, -45.0, 8.5)));
            }
        }
        let category = kind.mob_category();
        let params = SpawnParams {
            spawnable_chunk_count: case.spawnable_chunks,
            spawn_enemies: category == MobCategory::Monster,
            per_player_cap: case.local_cap,
            respawn_pos: Some(BlockPos::new(
                if case.nearby_respawn { 8 } else { 10000 },
                -62,
                8,
            )),
            sea_level: -63,
        };
        for official in &case.steps {
            ticks.set_time(official.time);
            let ours = spawn_tick(&mut level, &mut ticks, category, &params);
            self.steps += 1;
            self.natural_spawns.record(case, &ours);
            if &ours != official {
                self.mismatch(format!(
                    "{} seed {} tick {}: official {official:?}, Bamboo {ours:?}",
                    case.name, case.seed, official.time
                ));
            }
        }
        Ok(())
    }
}

fn spawn_tick(
    level: &mut MobLevel,
    ticks: &mut BlockTicks,
    category: MobCategory,
    params: &SpawnParams,
) -> Step {
    let time = ticks.now();
    let present = level.entities.len();
    let mut world = World::new(level, ticks);
    let mut state = SpawnState::create(&world, params.spawnable_chunk_count);
    let mut categories = state.filtered_categories(params.spawn_enemies, time % 400 == 0);
    categories.retain(|&value| value == category);
    for chunk in [ChunkPos::new(0, 0), ChunkPos::new(1, 0)] {
        natural::spawn_for_chunk(&mut world, chunk, &mut state, params, &categories);
    }
    let world_seed = world.random().seed();
    let spawned = level.entities.iter().skip(present).map(spawn_row).collect();
    Step {
        time,
        count: state.count(category),
        world_seed,
        spawned,
    }
}

fn spawn_row(entity: Entity) -> [i32; 8] {
    let mob = entity.kind.mob().expect("natural spawns are mobs");
    [
        mob.mob_type.entity_type(),
        (entity.position.x * 2.0) as i32,
        entity.position.y as i32,
        (entity.position.z * 2.0) as i32,
        entity.y_rot.to_bits() as i32,
        i32::from(mob.is_baby()),
        i32::from(mob.left_handed),
        mob.equipment.get(EquipmentSlot::MainHand).item,
    ]
}

fn fixture(case: &Case, kind: MobType, broken: bool) -> Result<MobLevel, String> {
    let biome = biome::NAMES
        .iter()
        .position(|name| *name == case.biome)
        .ok_or_else(|| format!("unknown natural spawn fixture biome {}", case.biome))?
        as i32;
    let category = kind.mob_category();
    let entry = spawning::for_biome_id(biome)
        .mobs_to_spawn(category)
        .iter()
        .find(|entry| entry.entity_type == case.entity_type)
        .ok_or_else(|| format!("{} has no spawn entry for {}", case.biome, case.entity_type))?;
    // 尸壳夹具多一层 -60 的玻璃顶，挡住官方的骆驼尸壳分支（见 `MobOracle.java`）。
    // The husk fixture adds a glass roof at -60 to block vanilla's camel-husk branch.
    let roof = kind == MobType::Husk;
    let blocks = (-16..48)
        .flat_map(|x| {
            (-16..32).flat_map(move |z| {
                let floor = (BlockPos::new(x, -63, z), BlockState(blocks::GRASS_BLOCK));
                let glass = (BlockPos::new(x, -60, z), BlockState(blocks::GLASS));
                std::iter::once(floor).chain(roof.then_some(glass))
            })
        })
        .collect();
    let player = PlayerView {
        uid: 1,
        uuid: None,
        position: Vec3::new(case.player_x, -62.0, 8.5),
        eye_height: 1.62,
        alive: true,
        spectator: false,
        creative: false,
        sneaking: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
    };
    Ok(MobLevel {
        blocks,
        random: LevelRandom::new(case.seed),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: case.difficulty,
            sky_darken: case.darken,
            can_see_sky: case.sky == 15,
            raining: case.thunder,
            halloween: case.halloween,
            ..ZombieFacts::default()
        },
        natural_spawn: Some(SceneFacts {
            biome,
            category,
            entry,
            sky: if broken && case.name == "cow_day" {
                0
            } else {
                case.sky
            },
            block: if broken && case.name == "skeleton_night_2" {
                15
            } else {
                case.block
            },
            thunder: case.thunder,
        }),
        redstone: Default::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        entities: Entities::with_next_id(2),
        players: vec![player],
        game: None,
        wrong_was_hurt: false,
    })
}
