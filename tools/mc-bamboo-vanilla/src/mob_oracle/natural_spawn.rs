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
    /// `WorldGenLevel.getSeed()`：史莱姆区块读它。
    ///
    /// `WorldGenLevel.getSeed()`, read for slime chunks.
    #[serde(default)]
    level_seed: i64,
    /// 草方块地面的高度；沼泽的史莱姆场景在 50 到 66 之间叠了五层。
    ///
    /// The grass floor heights; swamp slime scenes stack five floors from 50 to 66.
    #[serde(default = "default_floors")]
    floors: Vec<i32>,
    #[serde(default)]
    water_depth: i32,
    #[serde(default = "flat_sea_level")]
    sea_level: i32,
    /// 主世界时钟，月相读它。
    ///
    /// The overworld clock, read for the moon phase.
    #[serde(default)]
    clock: i64,
    /// 水生夹具：三层水（-64..=-62）代替草地。
    ///
    /// Aquatic fixture: three water layers (-64..=-62) replace the grass.
    #[serde(default)]
    aquatic: bool,
    /// 美西螈夹具：三层水里最底下那层是黏土。
    ///
    /// Axolotl fixture: the bottom of the three "water" layers is clay.
    #[serde(default)]
    clay_floor: bool,
    /// 海底神殿夹具：用 `minecraft:monument` 的覆盖表，水面以上到水柱铺水。
    ///
    /// Ocean monument fixture: use the monument override table and fill the
    /// water column up to the sea level.
    #[serde(default)]
    monument: bool,
    /// 神殿夹具在水面盖一层石头，挡住 `canSeeSkyFromBelowWater`。
    ///
    /// The monument fixture puts a stone roof at sea level to block sky visibility.
    #[serde(default)]
    monument_roof: bool,
    steps: Vec<Step>,
}

fn default_floors() -> Vec<i32> {
    vec![-63]
}

fn flat_sea_level() -> i32 {
    -63
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    time: i64,
    count: i32,
    world_seed: i64,
    spawned: Vec<[i32; 12]>,
    /// 每只刷出来的生物的花色和叫声（`MobOracle.spawnVariants`）。
    ///
    /// Each spawned mob's variant and sound (`MobOracle.spawnVariants`).
    variants: Vec<Option<[String; 2]>>,
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
    /// 刷出来的猪、羊、鸡（新加的农场动物）。
    ///
    /// Spawned pigs, sheep and chickens (the newer farm animals).
    farm: [usize; 3],
    skeletons: usize,
    zombies: usize,
    parched: usize,
    slimes: usize,
    squid: usize,
    equines: usize,
    witches: usize,
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
        for (count, kind) in
            self.farm
                .iter_mut()
                .zip([entity_type::PIG, entity_type::SHEEP, entity_type::CHICKEN])
        {
            *count += step.spawned.iter().filter(|row| row[0] == kind).count();
        }
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
        self.parched += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::PARCHED)
            .count();
        self.slimes += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::SLIME)
            .count();
        self.squid += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::SQUID || row[0] == entity_type::GLOW_SQUID)
            .count();
        self.equines += step
            .spawned
            .iter()
            .filter(|row| MobType::by_entity_type(row[0]).is_some_and(MobType::is_equine))
            .count();
        self.witches += step
            .spawned
            .iter()
            .filter(|row| row[0] == entity_type::WITCH)
            .count();
        if case.name == "cow_no_local_cap" {
            self.cap_off += step.spawned.len();
        }
    }

    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "NATURAL SPAWN: {} cases, {} ticks, {} cows, {:?} pigs/sheep/chickens, {} skeletons, {} zombies or husks, {} parched, {} slimes, {} squid or glow squid, {} equines, {} witches, {} empty ticks, {} spawns with local cap bypassed",
                self.cases,
                self.ticks,
                self.cows,
                self.farm,
                self.skeletons,
                self.zombies,
                self.parched,
                self.slimes,
                self.squid,
                self.equines,
                self.witches,
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
        if let Some(&deep) = self.per_scene.get("drowned_deep_ocean")
            && (deep == 0
                || self
                    .per_scene
                    .get("drowned_shallow_river")
                    .copied()
                    .unwrap_or(0)
                    == 0)
        {
            return Err("natural spawn fixtures did not spawn drowned in ocean and river".into());
        }
        if self.per_scene.contains_key("witch_night") && self.witches == 0 {
            return Err("natural spawn fixtures did not spawn any witches".into());
        }
        // 流浪者和沼骸的正例必须真的刷出生物，否则找天 / 黑暗规则没被覆盖。
        // The stray and bogged positive scenes must really spawn, or the sky / darkness
        // rules went untested.
        for (scene, creature) in [
            ("stray_night", "strays"),
            ("bogged_night", "bogged"),
            ("parched_night", "parched"),
        ] {
            if let Some(&count) = self.per_scene.get(scene)
                && count == 0
            {
                return Err(format!(
                    "natural spawn fixture {scene} did not spawn any {creature}"
                ));
            }
        }
        // 焦骸：夜里看得见天必须刷，白天 / 没天 / 和平必须不刷。
        // Parched: open sky at night must spawn; day, no sky and peaceful must not.
        for name in ["parched_day", "parched_no_sky", "parched_peaceful"] {
            if self.per_scene.get(name) != Some(&0) {
                return Err(format!(
                    "natural spawn fixture {name} must not spawn parched"
                ));
            }
        }
        // 蝙蝠：有顶的暗场景必须刷，没顶的必须不刷。
        // Bats: the roofed dark scene must spawn and the open one must not.
        if let Some(&dark) = self.per_scene.get("bat_dark")
            && (dark == 0 || self.per_scene.get("bat_open") != Some(&0))
        {
            return Err(
                "bat natural spawn fixtures must spawn under a roof but not in the open".into(),
            );
        }
        // 熊猫单独的一组：亮场景必须刷、暗场景必须不刷。
        // The panda-only group: the bright scene must spawn and the dim one must not.
        if let Some(&day) = self.per_scene.get("panda_day") {
            return if day > 0 && self.per_scene.get("panda_dim") == Some(&0) {
                Ok(())
            } else {
                Err("panda natural spawn fixtures must spawn at raw brightness 15 but not 8".into())
            };
        }
        // 神殿正例必须真的刷出守卫者，和平难度不能刷；两个露天/封顶场景都要有结果。
        // The monument positives must spawn guardians, peaceful must not, and both
        // the open-sky and roofed scenes must be exercised.
        if let Some(&count) = self.per_scene.get("monument_guardian_sky")
            && (count == 0
                || self.per_scene.get("monument_guardian_peaceful") != Some(&0)
                || self.per_scene.get("monument_guardian") == Some(&0))
        {
            return Err(
                "monument guardian natural spawn fixtures did not exercise the spawn rules".into(),
            );
        }
        for name in [
            "fish_cod",
            "fish_salmon",
            "fish_tropical",
            "fish_tropical_lush",
            "fish_puffer",
            "dolphin_ocean",
            "axolotl_lush",
        ] {
            if self.per_scene.get(name) == Some(&0) {
                return Err(format!("natural spawn fixture {name} produced no mobs"));
            }
        }
        let covered_misc = ["mooshroom_day", "polar_bear_day"]
            .iter()
            .all(|name| self.per_scene.get(*name).is_some_and(|&count| count > 0));
        let mobs = [
            self.cows,
            self.skeletons,
            self.zombies,
            self.slimes,
            self.squid,
            self.equines,
        ];
        let full = self.per_scene.contains_key("cow_day");
        if self.cases > 0
            && full
            && (mobs.contains(&0) || self.farm.contains(&0) || self.cap_off == 0 || !covered_misc)
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
        let category = if kind == MobType::Ocelot {
            MobCategory::Monster
        } else {
            kind.mob_category()
        };
        let params = SpawnParams {
            spawnable_chunk_count: case.spawnable_chunks,
            spawn_enemies: category == MobCategory::Monster,
            per_player_cap: case.local_cap,
            respawn_pos: Some(BlockPos::new(
                if case.nearby_respawn { 8 } else { 10000 },
                case.floors[0] + 1,
                8,
            )),
            // 改坏确认：海平面降一格，鱿鱼的「不高于海平面」就不成立。
            // Break check: one block lower and the squid's "at or below sea level" fails.
            sea_level: case.sea_level - i32::from(broken && case.name == "squid_ocean"),
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
    let fresh: Vec<Entity> = level.entities.iter().skip(present).collect();
    let variants = fresh
        .iter()
        .map(|entity| super::spawn_variants(entity.kind.mob().expect("natural spawns are mobs")))
        .collect();
    Step {
        time,
        count: state.count(category),
        world_seed,
        spawned: fresh.into_iter().map(spawn_row).collect(),
        variants,
    }
}

fn spawn_row(entity: Entity) -> [i32; 12] {
    let mob = entity.kind.mob().expect("natural spawns are mobs");
    let [main_gene, hidden_gene] = if mob.mob_type == MobType::Panda {
        [mob.panda.main_gene.id(), mob.panda.hidden_gene.id()]
    } else {
        [-1; 2]
    };
    let tropical = mob.mob_type == MobType::TropicalFish;
    [
        mob.mob_type.entity_type(),
        (entity.position.x * 2.0) as i32,
        entity.position.y as i32,
        (entity.position.z * 2.0) as i32,
        // 大小不是 1 的史莱姆朝向来自构造函数的随机源，官方那边不带种子，两边都记 0。
        // The yaw of a slime whose size is not 1 comes from the constructor random, unseeded
        // in vanilla; both sides record 0.
        if mob.mob_type == MobType::Slime && mob.cube.size != 1 {
            0
        } else {
            entity.y_rot.to_bits() as i32
        },
        // 史莱姆不会是幼年，这一列改放大小。
        // Slimes are never babies; this column carries their size instead.
        if mob.mob_type == MobType::Slime {
            mob.cube.size
        } else {
            i32::from(mob.is_baby())
        },
        i32::from(mob.left_handed),
        mob.equipment.get(EquipmentSlot::MainHand).item,
        // 溺尸记副手；热带鱼记花纹；其余为 0。
        // Drowned carry the offhand item; tropical fish carry their variant; else 0.
        if mob.mob_type == MobType::Drowned {
            mob.equipment.get(EquipmentSlot::OffHand).item
        } else if tropical {
            mob.fish.variant
        } else {
            0
        },
        main_gene,
        hidden_gene,
        // 热带鱼花纹和美西螈花色共用最后一列（官方夹具的 12 列布局）。
        // Tropical fish patterns and axolotl variants share the last column
        // (the vanilla fixture's 12-column layout).
        if tropical {
            mob.fish.variant
        } else if mob.mob_type == MobType::Axolotl {
            mob.axolotl.variant
        } else {
            0
        },
    ]
}

fn fixture(case: &Case, kind: MobType, broken: bool) -> Result<MobLevel, String> {
    let biome = biome::NAMES
        .iter()
        .position(|name| *name == case.biome)
        .ok_or_else(|| format!("unknown natural spawn fixture biome {}", case.biome))?
        as i32;
    // 豹猫实体类别是 `Creature`，但在丛林刷怪表里排在 `Monster` 槽位。
    // Ocelots have category `Creature` yet live in the `Monster` table of jungle biomes.
    let category = if kind == MobType::Ocelot {
        MobCategory::Monster
    } else {
        kind.mob_category()
    };
    // 神殿夹具用生成的官方覆盖表（守卫者 ×2–4），其余夹具从群系表里挑单一条目。
    // The monument fixture uses the generated official override (guardians ×2–4);
    // the rest pick one entry from the biome table.
    let entry = if case.monument {
        spawning::structure_spawn_override("minecraft:monument", category)
            .and_then(|table| {
                table
                    .iter()
                    .find(|entry| entry.entity_type == case.entity_type)
            })
            .ok_or("monument guardian override is missing")?
    } else {
        spawning::for_biome_id(biome)
            .mobs_to_spawn(category)
            .iter()
            .find(|entry| entry.entity_type == case.entity_type)
            .ok_or_else(|| format!("{} has no spawn entry for {}", case.biome, case.entity_type))?
    };
    // 尸壳夹具多一层 -60 的玻璃顶，挡住官方的骆驼尸壳分支（见 `MobOracle.java`）。
    // The husk fixture adds a glass roof at -60 to block vanilla's camel-husk branch.
    // 蝙蝠夹具：石头地面，除了 `_open` 都有同样的玻璃顶。
    // The bat fixture: a stone floor, and the same glass roof except in `_open`.
    let bat = kind == MobType::Bat;
    let roof = kind == MobType::Husk || (bat && !case.name.ends_with("_open"));
    let floor_state = if bat {
        BlockState(blocks::STONE)
    } else if case.name == "ocelot_jungle_leaves" {
        BlockState(blocks::OAK_LEAVES)
    } else {
        spawn_floor(biome)
    };
    let aquatic = case.aquatic;
    let monument = case.monument;
    let monument_roof = case.monument_roof;
    let floors = case.floors.clone();
    let water_depth = case.water_depth;
    let blocks = (-16..48)
        .flat_map(|x| {
            let floors = floors.clone();
            (-16..32).flat_map(move |z| {
                let mut column = Vec::new();
                if monument {
                    for y in -61..=-56 {
                        let state = if monument_roof && y == -56 {
                            blocks::STONE
                        } else {
                            blocks::WATER
                        };
                        column.push((BlockPos::new(x, y, z), BlockState(state)));
                    }
                }
                for &y in &floors {
                    column.push((BlockPos::new(x, y, z), BlockState(blocks::GRASS_BLOCK)));
                }
                for dy in 1..=water_depth {
                    column.push((
                        BlockPos::new(x, floors[0] + dy, z),
                        BlockState(blocks::WATER),
                    ));
                }
                if aquatic {
                    for y in -64..=-62 {
                        // 美西螈夹具把最底下的水换成黏土，`checkAxolotlSpawnRules` 才通过。
                        // The axolotl fixture swaps the bottom water layer for clay so
                        // `checkAxolotlSpawnRules` passes.
                        let state = if case.clay_floor && y == -64 {
                            blocks::CLAY
                        } else {
                            blocks::WATER
                        };
                        column.push((BlockPos::new(x, y, z), BlockState(state)));
                    }
                } else {
                    column.push((BlockPos::new(x, -63, z), floor_state));
                    if roof {
                        column.push((BlockPos::new(x, -60, z), BlockState(blocks::GLASS)));
                    }
                }
                column
            })
        })
        .collect();
    let player = PlayerView {
        uid: 1,
        yaw: 0.0,
        pitch: 0.0,
        uuid: None,
        position: Vec3::new(case.player_x, f64::from(case.floors[0] + 1), 8.5),
        delta: Vec3::default(),
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::default(),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
    };
    Ok(MobLevel {
        blocks,
        random: LevelRandom::new(case.seed),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        biome: None,
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: case.difficulty,
            sky_darken: case.darken,
            // 改坏自检：把流浪者正例的找天判据拿掉，官方仍刷、Bamboo 不刷，
            // 用来证明流浪者夹具确实覆盖 `Stray.checkStraySpawnRules` 的找天一支。
            // Break-self-test: drop the stray positive scene's open-sky fact, so vanilla
            // still spawns and Bamboo does not — proving the fixture covers the sky gate.
            can_see_sky: case.sky == 15 && !(broken && case.name == "stray_night"),
            raining: case.thunder,
            halloween: case.halloween,
            clock: case.clock,
            world_seed: case.level_seed,
            sea_level: case.sea_level,
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
            block: if broken
                && matches!(
                    case.name.as_str(),
                    "skeleton_night_2" | "drowned_shallow_river" | "bat_dim"
                ) {
                15
            } else {
                case.block
            },
            thunder: case.thunder,
        }),
        redstone: Default::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: Entities::with_next_id(2),
        players: vec![player],
        game: None,
        inhabited: Default::default(),
        mob_teams: Default::default(),
        ignore_bed_monsters: false,
        short_hunger: false,
        wrong_was_hurt: false,
    })
}

pub(super) fn spawn_floor(biome: i32) -> BlockState {
    BlockState(match biome {
        biome::MUSHROOM_FIELDS => blocks::MYCELIUM,
        biome::FROZEN_OCEAN => blocks::ICE,
        _ => blocks::GRASS_BLOCK,
    })
}
