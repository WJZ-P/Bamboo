//! 海豚 / 美西螈对拍：逐刻比位置、速度、朝向、随机源、氧气、年龄、着地 / 在水里、
//! 各自的元数据状态和尺寸，以及世界随机源。
//!
//! 两边都设 `NoAI`，显式给同一个实体随机源种子、不重放构造取数；这样官方的
//! goalSelector / Brain 不跑，逐刻只剩 `baseTick`（氧气与环境音取数）和
//! `Dolphin.tick()` / `Axolotl.baseTick()` 的 NoAI 分支，可以逐位比较。
//!
//! Dolphin / axolotl comparison: each tick compares position, velocity, rotation,
//! random, air, age, on-ground / in-water, the per-mob metadata state and size, plus
//! the world random.
//!
//! Both sides set `NoAI`, get the same explicit entity-random seed and skip constructor
//! draws; vanilla's goalSelector / Brain does not run, so only `baseTick` (air and
//! ambient-sound draws) and the NoAI branches of `Dolphin.tick()` /
//! `Axolotl.baseTick()` remain, comparable bit for bit.
//!
//! 场景还逐字核对官方 `goalSelector` 的注册表（优先级、类名、标志位掩码）：目标不在
//! NoAI 夹具里逐刻跑，但注册次序和标志位必须和官方一致，这是「把占位目标换成真实现」
//! 的可核对部分。
//!
//! Each case also checks the official `goalSelector` registry (priority, class name,
//! flag mask) character for character. The goals are not ticked in this NoAI fixture,
//! but their registration order and flags must match vanilla — this is the checkable
//! part of replacing the placeholder goals with real implementations.

use mc_bamboo_blocks::entity::mob::MobType;
use mc_bamboo_blocks::entity::mob::goals::GoalSelector;

use super::*;

/// 两边共用的显式实体随机源种子。
///
/// The explicit entity-random seed shared by both sides.
const SCENE_SEED: i64 = 0x5EED_5EED;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WaterMobCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    dolphin: bool,
    baby: bool,
    variant: i32,
    /// 官方 `goalSelector` 注册表：优先级、类名、标志位掩码。
    ///
    /// The official `goalSelector` registry: priority, class name, flag mask.
    #[serde(default)]
    registered: Vec<(i32, String, i32)>,
    frames: Vec<WaterMobFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaterMobFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<WaterMobState>,
    level_seed: i64,
}

/// 一刻结束时海豚 / 美西螈的样子，字段和官方一侧一一对应。
///
/// The mob at the end of a tick, field for field with the official output.
#[derive(Debug, Deserialize, PartialEq)]
struct WaterMobState {
    motion: [i64; 8],
    seed: i64,
    /// 血量、氧气、hurtTime、deathTime、noActionTime、age、着地、在水里、
    /// 以及三个元数据状态（海豚：gotFish / moistness / 0；美西螈：variant / playingDead / fromBucket）。
    ///
    /// Health, air, hurtTime, deathTime, noActionTime, age, on-ground, in-water and
    /// three metadata fields (dolphin: gotFish / moistness / 0; axolotl: variant /
    /// playingDead / fromBucket).
    state: [i32; 11],
    /// 宽、高、眼高的原始位。
    ///
    /// Raw bits of width, height and eye height.
    dimensions: [i32; 3],
}

impl WaterMobCase {
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
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            natural_spawn: None,
        }
    }

    fn mob_type(&self) -> MobType {
        if self.dolphin {
            MobType::Dolphin
        } else {
            MobType::Axolotl
        }
    }

    /// 和官方一侧同样的顺序建这只生物，然后把随机源设成显式种子（构造取数不重放）。
    ///
    /// Build the mob like the official side, then set the explicit seed (no constructor
    /// draws replayed).
    fn spawn(&self, world: &mut World<'_>, broken: bool, broken_goals: bool) -> i32 {
        let [x, y, z] = self.start;
        let kind = self.mob_type();
        let entity = Entity::by_mob_type(kind, Vec3::new(x, y, z));
        let id = world.spawn_entity(entity);
        world.with_entity_mut(id, &mut |entity| {
            entity.random = JavaRandom::new(SCENE_SEED);
            entity.y_rot = 0.0;
            entity.old_position = Vec3::new(x, y, z);
            if let Some(mob) = entity.kind.mob_mut() {
                mob.uuid = (2_u128 << 64 | 1).to_be_bytes();
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                mob.no_ai = true;
                // 改坏确认：把注册表清空，官方仍登记着十四个目标，逐刻比较应报错。
                // Break check: clear the registry; vanilla still registers fourteen
                // goals, so the comparison must fail.
                if broken_goals {
                    mob.goals.goals.clear();
                }
                if self.baby {
                    let (width, height, eye) = kind.dimensions(true);
                    entity.dimensions = (width, height);
                    mob.age = -24000;
                    mob.eye_height = eye;
                }
                if !self.dolphin {
                    mob.axolotl.variant = self.variant;
                    // 改坏确认：装死标志偏离官方，比较应报错。
                    // Break check: perturb playing-dead so the comparison fails.
                    if broken {
                        mob.axolotl.playing_dead = true;
                    }
                } else if broken {
                    // 改坏确认：`gotFish` 偏离官方。
                    // Break check: perturb gotFish.
                    mob.dolphin.got_fish = true;
                }
            }
        });
        id
    }
}

fn water_mob_state(entity: &Entity, dolphin: bool) -> WaterMobState {
    let mob = entity.kind.mob().expect("a water mob is a mob");
    let (p, v) = (entity.position, entity.delta);
    let custom = if dolphin {
        [i32::from(mob.dolphin.got_fish), mob.dolphin.moistness, 0]
    } else {
        [
            mob.axolotl.variant,
            i32::from(mob.axolotl.playing_dead),
            i32::from(mob.axolotl.from_bucket),
        ]
    };
    WaterMobState {
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
        state: [
            mob.health.to_bits() as i32,
            mob.air_supply,
            mob.hurt_time,
            mob.death_time,
            mob.no_action_time,
            mob.age,
            i32::from(entity.on_ground),
            i32::from(entity.in_water),
            custom[0],
            custom[1],
            custom[2],
        ],
        dimensions: [
            entity.dimensions.0.to_bits() as i32,
            entity.dimensions.1.to_bits() as i32,
            mob.eye_height.to_bits() as i32,
        ],
    }
}

/// 官方 `goalSelector` 注册表的每一行：优先级、类名、标志位掩码。
/// `Flag` 的枚举序和官方 `Goal.Flag` 一致，掩码两边逐位相同。
///
/// Each row of the official `goalSelector` registry: priority, class name, flag mask.
/// `Flag`'s ordinal order matches vanilla `Goal.Flag`, so the masks are bit-equal.
fn goal_rows(selector: &GoalSelector) -> Vec<(i32, String, i32)> {
    selector
        .goals
        .iter()
        .map(|wrapped| {
            let flags = wrapped
                .goal
                .flags()
                .iter()
                .map(|flag| 1_i32 << (*flag as i32))
                .sum();
            (wrapped.priority, wrapped.goal.name().to_owned(), flags)
        })
        .collect()
}

impl Report {
    pub(super) fn replay_water_mob(&mut self, case: &WaterMobCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("water_mob");
        let broken_goals = self.break_test.as_deref() == Some("dolphin_goals");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let id = case.spawn(
            &mut World::new(&mut level, &mut ticks),
            broken,
            broken_goals,
        );
        let ours_registered = level
            .entities
            .get(id)
            .map(|entity| {
                entity
                    .kind
                    .mob()
                    .map(|mob| goal_rows(&mob.goals))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if ours_registered != case.registered {
            self.mismatch(format!(
                "{} registered goals: official {:?}, bamboo {ours_registered:?}",
                case.name, case.registered
            ));
        }
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let public = level.entities.public_at(id).cloned();
            let gone = public.is_none();
            let label = format!("{} tick {tick}", case.name);
            if gone != expected.gone {
                self.mismatch(format!(
                    "{label} gone: official {}, bamboo {gone}",
                    expected.gone
                ));
            }
            let ours = level
                .entities
                .get(id)
                .map(|entity| water_mob_state(&entity, case.dolphin));
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} state: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            let level_seed = level.random.source().seed();
            if level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{label} level random: official {}, bamboo {level_seed}",
                    expected.level_seed
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
