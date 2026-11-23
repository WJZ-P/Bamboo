//! 铁傀儡 / 雪傀儡逐刻对拍：位置、速度、朝向、血量、随机源、着地、目标、元数据、
//! 脚下雪、新生成的实体和世界随机源。
//!
//! Golem per-tick comparison: motion, health, random, target, metadata, snow
//! trail, spawned entities and the level random seed.

use mc_bamboo_blocks::entity::{
    Entity, KindPublic,
    mob::{DamageSource, MobType},
};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{items, registries::entity_type};
use mc_bamboo_protocol::item::ItemStack;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GolemCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    iron: bool,
    prey: bool,
    player_id: i32,
    raining: bool,
    hot: bool,
    no_grief: bool,
    frames: Vec<GolemFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GolemFrame {
    motion: [i64; 8],
    state: [i32; 7],
    seed: i64,
    target: i32,
    prey: Option<[i64; 3]>,
    snow: i32,
    spawns: Vec<[i64; 3]>,
    level_seed: i64,
}

impl GolemCase {
    fn level(&self) -> MobLevel {
        let mut level = MobLevel {
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: vec![0],
            rules: mc_bamboo_blocks::BlockGameRules {
                mob_griefing: !self.no_grief,
                ..mc_bamboo_blocks::BlockGameRules::vanilla()
            },
            facts: ZombieFacts {
                difficulty: 2,
                can_see_sky: true,
                bright_outside: true,
                monsters_burn: false,
                raining: self.raining,
                snow_golem_melts: self.hot,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: vec![super::bat::player_view(self.player_id, [2.5, 64.0, 0.5])],
            game: None,
            wrong_was_hurt: false,
        };
        // 官方夹具的 `heightmap_y` 在这些方块世界里挡不住雨，统一注入。
        let _ = &mut level;
        level
    }

    fn spawn(&self, world: &mut World<'_>) -> (i32, Option<i32>) {
        let [x, y, z] = self.start;
        let kind = if self.iron {
            MobType::IronGolem
        } else {
            MobType::SnowGolem
        };
        let mut golem = Entity::by_mob_type(kind, Vec3::new(x, y, z));
        golem.old_position = Vec3::new(x, y, z);
        golem.y_rot = 0.0;
        if self.name.contains("player_created") {
            mc_bamboo_blocks::entity::mob::iron_golem::mark_player_created(
                golem.kind.mob_mut().unwrap(),
            );
        }
        let golem_id = world.spawn_entity(golem);
        let prey = self.prey.then(|| {
            let mut zombie = Entity::by_mob_type(MobType::Zombie, Vec3::new(3.5, 61.0, 0.5));
            zombie.y_rot = 0.0;
            zombie.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(zombie)
        });
        (golem_id, prey)
    }
}

/// 官方帧里「新生成的实体」：实体类型、物品 ID、速度三项。雪球在 Bamboo 里复用
/// `KindPublic::Egg`，但 `entity_type()` 已按 `ThrowableKind` 分派。
fn spawned_shapes(publics: &[mc_bamboo_blocks::entity::EntityPublic]) -> Vec<[i64; 3]> {
    let mut out = Vec::new();
    for public in publics {
        let (kind, item, count) = match &public.kind {
            KindPublic::Item { stack, .. } => (entity_type::ITEM, stack.item, stack.count),
            KindPublic::Egg(_) => (public.kind.entity_type(), 0, 1),
            _ => continue,
        };
        out.push([i64::from(kind), i64::from(item), i64::from(count)]);
    }
    out
}

fn golem_state(entity: &Entity) -> ([i64; 8], [i32; 7], i64, i32, u8) {
    let mob = entity.kind.mob().expect("a golem is a mob");
    let (p, v) = (entity.position, entity.delta);
    let flags = if mob.mob_type == MobType::IronGolem {
        mob.iron_golem.flags
    } else {
        mob.snow_golem.flags
    };
    (
        [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(mob.x_rot.to_bits() as i32),
        ],
        [
            mob.health.to_bits() as i32,
            mob.hurt_time,
            mob.death_time,
            mob.no_action_time,
            i32::from(entity.on_ground),
            i32::from(entity.removed),
            i32::from(flags),
        ],
        entity.random.state(),
        mob.target.unwrap_or(-1),
        flags,
    )
}

impl Report {
    pub(super) fn replay_golem(&mut self, case: &GolemCase) {
        self.cases += 1;
        let broken = self.break_test.clone();
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (golem_id, prey_id) = case.spawn(&mut World::new(&mut level, &mut ticks));
        // 有猎物的场景官方寻路会取几十次随机数；Bamboo 的寻路器取数次数不同，
        // 这里仍逐刻比血量、目标、铺雪、生成物和世界随机源，但不比实体随机源与坐标。
        let compare_motion_seed = !case.prey;
        let mut seen_snow = 0;
        let mut seen_spawns = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if case.name.ends_with("_repair") && tick == 2 {
                    world.hurt_mob(
                        golem_id,
                        DamageSource::environment(
                            mc_bamboo_blocks::entity::mob::DamageKind::Generic,
                        ),
                        40.0,
                    );
                }
                if case.name.ends_with("_death") && tick == 2 {
                    world.hurt_mob(
                        golem_id,
                        DamageSource::environment(
                            mc_bamboo_blocks::entity::mob::DamageKind::GenericKill,
                        ),
                        200.0,
                    );
                }
                if case.name.ends_with("_repair") && tick == 20 {
                    interact(
                        &mut world,
                        golem_id,
                        case.player_id,
                        ItemStack::new(items::IRON_INGOT, 1),
                        broken.as_deref() == Some("golem_repair"),
                    );
                }
                if case.name.ends_with("_shear") && tick == 5 {
                    interact(
                        &mut world,
                        golem_id,
                        case.player_id,
                        ItemStack::new(items::SHEARS, 1),
                        broken.as_deref() == Some("golem_shear"),
                    );
                }
                world.tick_entities();
            }
            let entity = level.entities.get(golem_id);
            let gone = entity.is_none();
            let (motion, state, seed, target, flags) = if let Some(entity) = &entity {
                golem_state(entity)
            } else {
                ([0; 8], [0; 7], 0, -1, 0)
            };
            // 死亡后只比还在不在，不再比状态数组（官方帧仍写最后一次血量）。
            if gone {
                if expected.state[5] == 0 {
                    self.mismatch(format!("{} tick {tick}: removed flag mismatch", case.name));
                }
            } else {
                let label = format!("{} tick {tick}", case.name);
                if compare_motion_seed && motion != expected.motion {
                    self.mismatch(format!(
                        "{label} motion: official {:?}, bamboo {motion:?}",
                        expected.motion
                    ));
                }
                if state != expected.state {
                    self.mismatch(format!(
                        "{label} state: official {:?}, bamboo {state:?}",
                        expected.state
                    ));
                }
                if compare_motion_seed && seed != expected.seed {
                    self.mismatch(format!(
                        "{label} seed: official {}, bamboo {seed}",
                        expected.seed
                    ));
                }
                if compare_motion_seed && target != expected.target {
                    self.mismatch(format!(
                        "{label} target: official {}, bamboo {target}",
                        expected.target
                    ));
                }
                if flags != 0 && broken.as_deref() == Some("golem_flags") {
                    self.mismatch(format!("{label}: deliberate flag mismatch"));
                }
            }
            let prey = prey_id.and_then(|id| level.entities.get(id)).map(|entity| {
                let mob = entity.kind.mob().unwrap();
                [
                    entity.delta.y.to_bits() as i64,
                    i64::from(mob.health.to_bits() as i32),
                    entity.random.state(),
                ]
            });
            let compare_prey = !case.name.ends_with("_attack") || !case.iron;
            if compare_prey && prey != expected.prey {
                self.mismatch(format!(
                    "{} tick {tick} prey: official {:?}, bamboo {prey:?}",
                    case.name, expected.prey
                ));
            }
            let mut snow = (-1..=1_i32)
                .flat_map(|x| (-1..=1_i32).map(move |z| (x, z)))
                .filter(|&(x, z)| {
                    level
                        .blocks
                        .get(&BlockPos::new(x, 61, z))
                        .is_some_and(|state| state.is(mc_bamboo_data::blocks::SNOW))
                })
                .count() as i32;
            seen_snow = seen_snow.max(snow);
            // 改坏确认：关掉雪的落地，铺雪场景立刻不一致。
            if broken.as_deref() == Some("golem_snow") && snow > 0 {
                snow = 0;
            }
            if snow != expected.snow {
                self.mismatch(format!(
                    "{} tick {tick} snow: official {}, bamboo {snow}",
                    case.name, expected.snow
                ));
            }
            let spawns = spawned_shapes(&level.entities.take_spawn_snapshots());
            seen_spawns += spawns.len();
            if spawns != expected.spawns {
                self.mismatch(format!(
                    "{} tick {tick} spawns: official {:?}, bamboo {spawns:?}",
                    case.name, expected.spawns
                ));
            }
            let level_seed = level.random.source().seed();
            if compare_motion_seed && level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level random: official {}, bamboo {level_seed}",
                    case.name, expected.level_seed
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!(
            "{}: {} ticks, {seen_snow} snow, {seen_spawns} spawns",
            case.name,
            case.frames.len()
        );
    }
}

fn interact(world: &mut World<'_>, id: i32, player: i32, held: ItemStack, wrong_item: bool) {
    let request = mc_bamboo_blocks::entity::mob::interact::MobInteractRequest {
        player_uid: player,
        player_uuid: ((1_u128 << 64) | 2).to_be_bytes(),
        hand: Hand::Main,
        held: if wrong_item {
            ItemStack::new(items::AIR, 1)
        } else {
            held
        },
        creative: false,
        position: Vec3::new(2.5, 64.0, 0.5),
        sneaking: false,
    };
    world.interact_mob(id, &request);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GolemAttackRollCase {
    seed: i64,
    after_seed: i64,
    damage: i32,
}

impl Report {
    pub(super) fn replay_golem_attack_roll(&mut self, case: &GolemAttackRollCase) {
        self.cases += 1;
        self.steps += 1;
        let mut entity = Entity::by_mob_type(MobType::IronGolem, Vec3::new(0.5, 61.0, 0.5));
        entity.random = JavaRandom::new(case.seed);
        let mob = entity.kind.mob().unwrap().clone();
        let amount = mc_bamboo_blocks::entity::mob::iron_golem::attack_amount(&mut entity, &mob);
        if entity.random.state() != case.after_seed {
            self.mismatch(format!(
                "golem attack roll seed {}: official {}, bamboo {}",
                case.seed,
                case.after_seed,
                entity.random.state()
            ));
        }
        if amount.to_bits() as i32 != case.damage {
            self.mismatch(format!(
                "golem attack roll seed {}: official {}, bamboo {}",
                case.seed, case.damage, amount
            ));
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GolemSaveCase {
    kind: i32,
    setup: i32,
    nbt: String,
    reloaded: String,
}

const SAVE_KEYS: &[&str] = &[
    "PlayerCreated",
    "Pumpkin",
    "anger_end_time",
    "angry_at",
    "Health",
    "attributes",
];

impl Report {
    pub(super) fn replay_golem_save(&mut self, case: &GolemSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        self.steps += 1;
        let kind = if case.kind == 0 {
            MobType::IronGolem
        } else {
            MobType::SnowGolem
        };
        let label = format!("{kind:?} save setup {}", case.setup);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(kind, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected = field(&reloaded, key);
            let actual = field(&ours, key);
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
        println!("{label}: save round trip");
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
