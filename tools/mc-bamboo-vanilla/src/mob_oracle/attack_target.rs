//! 攻击目标逐刻对拍：目标选择、位移、攻击后的血量和死亡。
//!
//! 官方一侧在假世界里直接构造攻击者与 NoAI 的受害者，逐刻 `commonTick` + `tick`
//! 并记录双方的位置 / 速度 / 血量 / 死亡 / 目标；这里用同一份方块和种子重放，按原始位比较。
//! 受害者设 NoAI 是为了让官方村民的大脑不逐刻取数，也让受害者不自己走开。
//!
//! Per-tick comparison of attack targets: selection, movement, victim health and
//! death. Vanilla constructs the attacker and a NoAI victim in a fake level; the
//! replay uses the same blocks and seeds and compares raw bits. The victim is NoAI
//! so the official villager brain draws nothing per tick and the victim stays put.

use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    mob::{
        MobType,
        goals::{Goal, TargetType},
    },
};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::*;

/// 场景编号，两侧共用：沿用牛场景的 1 / 7 / 41 / 52 / 53 / 60，新增 70 起。
fn scene_kind(kind: i32) -> MobType {
    match kind {
        1 => MobType::Zombie,
        7 => MobType::Fox,
        41 => MobType::Spider,
        52 => MobType::Drowned,
        53 => MobType::Pillager,
        60 => MobType::Turtle,
        70 => MobType::Villager,
        71 => MobType::IronGolem,
        72 => MobType::Axolotl,
        73 => MobType::Cod,
        75 => MobType::TropicalFish,
        _ => unreachable!("unknown attack-target kind {kind}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AttackTargetCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    difficulty: i32,
    bright_outside: bool,
    sky_darken: i32,
    attacker: AttackTargetMob,
    victim: AttackTargetMob,
    /// 中途落下的墙：（刻、x、y、z、方块状态号）。
    wall: Option<[i32; 5]>,
    /// 墙的每一格，官方一侧在 `wall[0]` 那一刻铺上去。
    wall_blocks: Vec<[i32; 4]>,
    frames: Vec<AttackTargetFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttackTargetMob {
    kind: i32,
    position: [i64; 3],
    yaw: i32,
    seed: i64,
    baby: bool,
    no_ai: bool,
    /// 狐狸品种：0 红狐、1 雪狐。
    fox_variant: i32,
    /// 主手放一把弩。
    crossbow: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AttackTargetFrame {
    attacker: MobFrame,
    victim: MobFrame,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct MobFrame {
    motion: [i64; 6],
    health: i32,
    hurt_time: i32,
    death_time: i32,
    removed: i32,
    target: i32,
}

impl AttackTargetCase {
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
                difficulty: self.difficulty,
                sky_darken: self.sky_darken,
                bright_outside: self.bright_outside,
                monsters_burn: false,
                can_see_sky: false,
                sea_level: -63,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            wrong_was_hurt: false,
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }

    fn spawn(
        &self,
        level: &mut MobLevel,
        ticks: &mut BlockTicks,
        break_test: Option<&str>,
    ) -> (i32, i32) {
        let attacker = self.spawn_one(level, ticks, &self.attacker);
        let victim = self.spawn_one(level, ticks, &self.victim);
        match break_test {
            // 改坏确认：把村民目标换成玩家目标，等价于搜索返回 None。
            // Deliberate breakage: turn the villager goal into a player goal, so it
            // finds nothing (the official side still finds the villager).
            Some("villager_target_none") => {
                level.entities.with_mut(attacker, |entity| {
                    let Some(mob) = entity.kind.mob_mut() else {
                        return;
                    };
                    for wrapped in &mut mob.target_goals.goals {
                        if let Goal::NearestAttackableTarget { kind, .. } = &mut wrapped.goal
                            && *kind == TargetType::Villager
                        {
                            *kind = TargetType::Player;
                        }
                    }
                });
            }
            // 改坏确认：把 mustSee 取反，隔墙之后视线记忆会让目标被错误地放弃。
            // Deliberate breakage: invert mustSee so the sight memory wrongly drops
            // the target after the wall appears.
            Some("must_see_inverted") => {
                level.entities.with_mut(attacker, |entity| {
                    let Some(mob) = entity.kind.mob_mut() else {
                        return;
                    };
                    for wrapped in &mut mob.target_goals.goals {
                        if let Goal::NearestAttackableTarget { must_see, .. } = &mut wrapped.goal {
                            *must_see = !*must_see;
                        }
                    }
                });
            }
            _ => {}
        }
        (attacker, victim)
    }

    fn spawn_one(
        &self,
        level: &mut MobLevel,
        ticks: &mut BlockTicks,
        setup: &AttackTargetMob,
    ) -> i32 {
        let position = Vec3::new(
            f64::from_bits(setup.position[0] as u64),
            f64::from_bits(setup.position[1] as u64),
            f64::from_bits(setup.position[2] as u64),
        );
        let mut entity = Entity::by_mob_type(scene_kind(setup.kind), position);
        entity.old_position = position;
        let id = World::new(level, ticks).spawn_entity(entity);
        // 官方一侧在构造后把实体随机源换成同一颗种子并重放构造取数，这里照做。
        // The official side reseeds after construction and replays the constructor
        // draws; mirror that here.
        level.entities.with_mut(id, |entity| {
            entity.random = JavaRandom::new(setup.seed);
            // 官方构造后把随机源换成场景种子并重放 UUID（两个 nextLong）和朝向
            // （一个 nextFloat），这里照做，之后逐刻的取数才和官方同一步。
            // The official side reseeds and replays the UUID (two nextLong) and
            // facing (one nextFloat) draws; mirror them so later draws line up.
            let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
            let _ = entity.random.next_float();
            entity.y_rot = f32::from_bits(setup.yaw as u32);
            let Some(mob) = entity.kind.mob_mut() else {
                return;
            };
            mob.y_head_rot = entity.y_rot;
            mob.y_body_rot = entity.y_rot;
            mob.no_ai = setup.no_ai;
            if setup.baby {
                mob.age = -24000;
            }
            if setup.crossbow {
                mob.equipment.set(
                    EquipmentSlot::MainHand,
                    mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::CROSSBOW, 1),
                );
            }
            if mob.mob_type == MobType::Fox {
                mc_bamboo_blocks::entity::mob::fox::set_target_goals(mob);
                if setup.fox_variant == 1 {
                    mob.fox.variant = mc_bamboo_blocks::entity::mob::fox::Variant::Snow;
                }
            }
        });
        id
    }
}

/// 官方 `Mob.getTarget()` 在目标死后读到 null；Bamboo 的原始 `target` 字段仍留着 uid，
/// 这里按目标是否还活着折算，免得把纯字段差异算成行为差异。
///
/// Vanilla's `Mob.getTarget()` reads null once the target dies, while Bamboo keeps the
/// raw uid in `target`. Fold that away so a stale field is not reported as a behaviour
/// difference.
fn effective_target(level: &MobLevel, mob: &mc_bamboo_blocks::entity::mob::Mob) -> i32 {
    mob.target
        .and_then(|uid| {
            let other = level.entities.get(uid)?;
            let target = other.kind.mob()?;
            (!other.removed && target.health > 0.0).then_some(uid)
        })
        .unwrap_or(-1)
}

fn frame_of(level: &MobLevel, id: i32) -> MobFrame {
    let Some(entity) = level.entities.get(id) else {
        return MobFrame {
            motion: [0; 6],
            health: 0,
            hurt_time: 0,
            death_time: 0,
            removed: 1,
            target: -1,
        };
    };
    let mob = entity.kind.mob().expect("an attack-target mob");
    MobFrame {
        motion: [
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
            entity.delta.x.to_bits() as i64,
            entity.delta.y.to_bits() as i64,
            entity.delta.z.to_bits() as i64,
        ],
        health: mob.health.to_bits() as i32,
        hurt_time: mob.hurt_time,
        death_time: mob.death_time,
        removed: i32::from(entity.removed),
        target: effective_target(level, mob),
    }
}

/// 官方把移除后的实体仍留在对象里（位置、死亡计时都还在），Bamboo 已经从实体表删掉，
/// 所以移除之后只比"还在不在"，不比其余字段；和 `golem` 场景同一个口径。
///
/// Vanilla keeps a removed entity object (position and death timer intact) while Bamboo
/// has dropped it from the store, so post-removal frames compare only the removed flag.
fn compare_frame(
    report: &mut Report,
    label: &str,
    side: &str,
    expected: &MobFrame,
    ours: &MobFrame,
) {
    if expected.removed == 1 {
        if ours.removed != 1 {
            report.mismatch(format!(
                "{label} {side}: official removed, bamboo still present"
            ));
        }
    } else if ours != expected {
        report.mismatch(format!(
            "{label} {side}: official {expected:?}, bamboo {ours:?}"
        ));
    }
}

impl Report {
    pub(super) fn replay_attack_target(&mut self, case: &AttackTargetCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (attacker, victim) = case.spawn(&mut level, &mut ticks, self.break_test.as_deref());
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                if let Some([at, ..]) = case.wall
                    && tick == at as usize
                {
                    for &[x, y, z, state] in &case.wall_blocks {
                        level
                            .blocks
                            .insert(BlockPos::new(x, y, z), BlockState(state as u32));
                    }
                }
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let ours = AttackTargetFrame {
                attacker: frame_of(&level, attacker),
                victim: frame_of(&level, victim),
            };
            let label = format!("{} tick {tick}", case.name);
            compare_frame(self, &label, "attacker", &expected.attacker, &ours.attacker);
            compare_frame(self, &label, "victim", &expected.victim, &ours.victim);
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
