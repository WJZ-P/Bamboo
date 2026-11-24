//! 守卫者逐刻对拍：游走、激光、尖刺、远古效果和扑腾。
//!
//! 官方一侧在假世界里直接构造守卫者（和它的目标），逐刻 `commonTick` + `tick`
//! 记录位置 / 速度 / 朝向 / 血量 / 氧气 / 目标 / 元数据；这里用同一份方块和种子
//! 重放，按原始位比较。
//!
//! Per-tick comparison of guardians: swimming, laser, thorns, the elder effect and
//! flopping. Vanilla constructs the guardian in a fake level; the replay uses the
//! same blocks and seeds and compares raw bits.

use mc_bamboo_blocks::entity::{Entity, JavaRandom, mob::MobType};
use mc_bamboo_core::Vec3;

use super::*;

/// 场景编号，两侧共用：90 普通守卫者、91 远古守卫者。
fn scene_kind(kind: i32) -> MobType {
    match kind {
        90 => MobType::Guardian,
        91 => MobType::ElderGuardian,
        _ => unreachable!("unknown guardian kind {kind}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GuardianCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    difficulty: i32,
    sky_darken: i32,
    guardian: GuardianSetup,
    frames: Vec<GuardianFrameLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianSetup {
    kind: i32,
    position: [i64; 3],
    seed: i64,
    yaw: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GuardianFrameLine {
    guardian: GuardianFrame,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GuardianFrame {
    /// 位置 xyz、速度 xyz 的原始位。
    motion: [i64; 6],
    y_rot: i32,
    y_head_rot: i32,
    y_body_rot: i32,
    health: i32,
    air: i32,
    hurt_time: i32,
    death_time: i32,
    removed: i32,
    /// `getTarget()`：无目标 -1。
    target: i32,
    /// `DATA_ID_ATTACK_TARGET`。
    attack_target: i32,
    /// `DATA_ID_MOVING`。
    moving: i32,
    #[serde(default)]
    look_cooldown: i32,
    #[serde(default)]
    wanted_x: i64,
    #[serde(default)]
    wanted_z: i64,
    #[serde(default)]
    random_state: i64,
    #[serde(default)]
    no_action_time: i32,
}

impl GuardianCase {
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
                bright_outside: false,
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

    fn spawn(&self, level: &mut MobLevel, ticks: &mut BlockTicks) -> i32 {
        let setup = &self.guardian;
        let position = Vec3::new(
            f64::from_bits(setup.position[0] as u64),
            f64::from_bits(setup.position[1] as u64),
            f64::from_bits(setup.position[2] as u64),
        );
        let mut entity = Entity::by_mob_type(scene_kind(setup.kind), position);
        entity.old_position = position;
        let id = World::new(level, ticks).spawn_entity(entity);
        // 官方构造后把随机源换成场景种子，重放 UUID（两个 nextLong）和朝向
        // （一个 nextFloat）；守卫者构造那次 tail `nextFloat` 用的是重置前的
        // 构造随机源，两侧都不重放它。
        // The official side reseeds after construction and replays the UUID (two
        // nextLong) and facing (one nextFloat) draws. The constructor's tail
        // `nextFloat` uses the pre-reset source on both sides and is not replayed.
        level.entities.with_mut(id, |entity| {
            entity.random = JavaRandom::new(setup.seed);
            let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
            let _ = entity.random.next_float();
            entity.y_rot = f32::from_bits(setup.yaw as u32);
            let Some(mob) = entity.kind.mob_mut() else {
                return;
            };
            mob.y_head_rot = entity.y_rot;
            mob.y_body_rot = entity.y_rot;
        });
        id
    }
}

fn frame_of(level: &MobLevel, id: i32) -> GuardianFrame {
    let Some(entity) = level.entities.get(id) else {
        return GuardianFrame {
            motion: [0; 6],
            y_rot: 0,
            y_head_rot: 0,
            y_body_rot: 0,
            health: 0,
            air: 0,
            hurt_time: 0,
            death_time: 0,
            removed: 1,
            target: -1,
            attack_target: 0,
            moving: 0,
            look_cooldown: 0,
            wanted_x: 0,
            wanted_z: 0,
            random_state: 0,
            no_action_time: 0,
        };
    };
    let mob = entity.kind.mob().expect("a guardian");
    GuardianFrame {
        motion: [
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
            entity.delta.x.to_bits() as i64,
            entity.delta.y.to_bits() as i64,
            entity.delta.z.to_bits() as i64,
        ],
        y_rot: entity.y_rot.to_bits() as i32,
        y_head_rot: mob.y_head_rot.to_bits() as i32,
        y_body_rot: mob.y_body_rot.to_bits() as i32,
        health: mob.health.to_bits() as i32,
        air: mob.air_supply,
        hurt_time: mob.hurt_time,
        death_time: mob.death_time,
        removed: i32::from(entity.removed),
        target: mob.target.unwrap_or(-1),
        attack_target: mob.guardian.attack_target,
        moving: i32::from(mob.guardian.moving),
        look_cooldown: mob.look.look_at_cooldown,
        wanted_x: mob.look.wanted_x.to_bits() as i64,
        wanted_z: mob.look.wanted_z.to_bits() as i64,
        random_state: entity.random.state(),
        no_action_time: mob.no_action_time,
    }
}

impl Report {
    pub(super) fn replay_guardian(&mut self, case: &GuardianCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let guardian = case.spawn(&mut level, &mut ticks);
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let ours = frame_of(&level, guardian);
            let label = format!("{} tick {tick}", case.name);
            if expected.guardian.removed == 1 {
                if ours.removed != 1 {
                    self.mismatch(format!("{label}: official removed, bamboo still present"));
                }
            } else if ours != expected.guardian {
                self.mismatch(format!(
                    "{label}: official {:?}, bamboo {:?}",
                    expected.guardian, ours
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
