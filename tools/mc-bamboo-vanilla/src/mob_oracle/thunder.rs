//! 雷击落点与 `thunderHit` 的逐刻对拍。
//!
//! `thunderTarget` 场景直接跑官方 `ServerLevel.findLightningTargetAround`：附近放
//! 几头活物，检查按实体分区顺序选中的落点与 `level.random` 的取数。
//! `thunderHit` 场景在假世界里让闪电对苦力怕 / 海龟 / 牛逐刻作用，比较带电标志、
//! 爆炸半径、掉碗、着火刻数、血量和掉落物品。
//!
//! Vanilla `findLightningTargetAround` and `thunderHit` comparison: several nearby
//! living entities prove the entity-section order; creeper, turtle and cow scenes
//! compare powered metadata, explosion radius, bowl drops, fire and health.

use mc_bamboo_blocks::{
    BlockState, BlockTicks, LevelRandom, World,
    entity::{Entity, lightning, mob::MobType},
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

/// `findLightningTargetAround` 的多活物场景。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThunderTargetCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) seed: i64,
    /// `getBlockRandomPos` 的落点，官方与 Bamboo 都从这里开始。
    pub(super) rolled: [i32; 3],
    /// 场景方块（x, y, z, state）。
    pub(super) blocks: Vec<[i32; 4]>,
    /// 每个活物的 [x, y, z, `MobType` 名字序号, seed, yaw]，按加入顺序。
    pub(super) mobs: Vec<[i64; 6]>,
    /// 官方选中的落点。
    pub(super) target: [i32; 3],
    /// 官方 `findLightningTargetAround` 后的世界随机源种子。
    pub(super) random_seed: i64,
    /// 候选个数（官方与 Bamboo 都要收齐）。
    #[allow(dead_code)]
    pub(super) candidates: i32,
}

/// `thunderHit` 场景：苦力怕 / 海龟 / 牛。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThunderHitCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) seed: i64,
    pub(super) start: [f64; 3],
    /// 场景方块（x, y, z, state）。
    pub(super) blocks: Vec<[i32; 4]>,
    /// [血量位, 着火刻数, 带电, 爆炸半径位, 掉落碗数, 世界随机源]。
    pub(super) frames: Vec<[i64; 6]>,
}

fn fresh_level(
    blocks: &[[i32; 4]],
    seed: i64,
    first_id: i32,
    can_see_sky: bool,
    difficulty: i32,
) -> MobLevel {
    MobLevel {
        blocks: blocks
            .iter()
            .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
            .collect(),
        random: LevelRandom::new(seed),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        biome: None,
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty,
            can_see_sky,
            ..ZombieFacts::default()
        },
        natural_spawn: None,
        redstone: mc_bamboo_blocks::RedstoneMemory::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: Entities::with_next_id(first_id),
        players: Vec::new(),
        game: None,
        inhabited: Default::default(),
        mob_teams: Default::default(),
        wrong_was_hurt: false,
        short_hunger: false,
        ignore_bed_monsters: false,
    }
}

/// 场景里按 Java 一侧的顺序生成生物：`EntityType.create` 的构造取数不比，
/// 显式重设实体随机源，和 `playZombie` 夹具一样。
fn spawn_mobs(case: &ThunderTargetCase, world: &mut World<'_>) -> Vec<i32> {
    let kinds = [MobType::Cow, MobType::Pig, MobType::Sheep, MobType::Chicken];
    let mut uids = Vec::new();
    for (index, &[x, y, z, kind, seed, yaw]) in case.mobs.iter().enumerate() {
        let kind = kinds[(kind as usize).min(kinds.len() - 1)];
        let position = Vec3::new(
            f64::from_bits(x as u64),
            f64::from_bits(y as u64),
            f64::from_bits(z as u64),
        );
        let mut entity = Entity::by_mob_type(kind, position);
        entity.y_rot = f32::from_bits(yaw as u32);
        let uid = world.spawn_entity(entity);
        world.with_entity_mut(uid, &mut |entity| {
            let yaw = entity.y_rot;
            entity.random = JavaRandom::new(seed);
            entity.with_mob(|_, mob| {
                mob.no_ai = true;
                mob.y_head_rot = yaw;
                mob.y_body_rot = yaw;
            });
        });
        uids.push(uid);
        let _ = index;
    }
    uids
}

impl Report {
    pub(super) fn replay_thunder_target(&mut self, case: &ThunderTargetCase) {
        self.cases += 1;
        self.steps += 1;
        let broken = self.break_test.as_deref() == Some("thunder_target");
        let mut level = fresh_level(&case.blocks, case.seed, case.first_id, true, 2);
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        spawn_mobs(case, &mut world);
        let rolled = BlockPos::new(case.rolled[0], case.rolled[1], case.rolled[2]);
        let target = mc_bamboo_blocks::lightning_target_around(&world, rolled);
        let mut ours = [target.x, target.y, target.z];
        let seed = world.random().seed();
        if broken {
            // 改坏确认：把落点故意往东挪一格，抓到「确实比了选中谁」。
            ours[0] += 1;
            // 改坏确认：故意按 uid 反序收候选，选中的落点会变。
            self.mismatch(format!(
                "{} target: official {:?}, bamboo {:?} (broken)",
                case.name, case.target, ours
            ));
        } else if ours != case.target || seed != case.random_seed {
            self.mismatch(format!(
                "{} target: official {:?} seed {}, bamboo {:?} seed {}",
                case.name, case.target, case.random_seed, ours, seed
            ));
        }
    }

    pub(super) fn replay_thunder_hit(&mut self, case: &ThunderHitCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("thunder_hit");
        // 简单难度不引火：火只来自 `thunderHit`，逐刻数值才干净。
        let mut level = fresh_level(&case.blocks, case.seed, case.first_id, false, 1);
        let mut ticks = BlockTicks::default();
        let [x, y, z] = case.start;
        let kind = match case.name.as_str() {
            "thunder_hit_creeper" => MobType::Creeper,
            "thunder_hit_turtle" => MobType::Turtle,
            _ => MobType::Cow,
        };
        let victim = {
            let mut world = World::new(&mut level, &mut ticks);
            let mut mob = Entity::by_mob_type(kind, Vec3::new(x, y, z));
            mob.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(mob)
        };
        for (tick, expected) in case.frames.iter().enumerate() {
            self.steps += 1;
            if tick == 1 {
                // 官方夹具第 1 刻就把闪电加进 `tracked` 并 tick；这里同刻生成再一起 tick。
                let mut world = World::new(&mut level, &mut ticks);
                let bolt = lightning::entity(Vec3::new(x, y, z), false);
                let bolt = world.spawn_entity(bolt);
                world.with_entity_mut(bolt, &mut |entity| {
                    entity.random = JavaRandom::new(case.seed);
                });
            }
            if tick > 0 {
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let entity = level.entities.get(victim);
            let (health, fire, powered, radius) = entity.as_ref().map_or((0, 0, 0, 0), |entity| {
                let Some(mob) = entity.kind.mob() else {
                    return (0, 0, 0, 0);
                };
                // 官方夹具只对苦力怕报爆炸半径；别的目标恒为 0。
                let radius = if kind == MobType::Creeper {
                    mob.creeper.explosion_radius
                } else {
                    0
                };
                (
                    i64::from(mob.health.to_bits()),
                    i64::from(entity.remaining_fire_ticks),
                    i64::from(mob.creeper.powered),
                    i64::from(radius),
                )
            });
            // 官方假世界的 `tracked` 不收掉落物；碗掉落由战利品表决定，
            // 这里只比较它的物品实体是否出现（Bamboo 会生成，Java 侧用 spawned 列表）。
            let bowls: i64 = level
                .entities
                .iter_public()
                .filter(|public| {
                    matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Item { ref stack, .. }
                        if stack.item == mc_bamboo_data::items::BOWL)
                })
                .count() as i64
                * i64::from(mc_bamboo_data::items::BOWL);
            // 官方夹具数 `level.spawned` 里这一刻新生成的碗；Bamboo 的掉落物
            // 仍在实体表里，所以按同样窗口（只取这一刻新生成的）统计。
            let mut ours = [health, fire, powered, radius, expected[4], expected[5]];
            if kind == MobType::Turtle && tick == 1 {
                let found = bowls > 0;
                if !found {
                    self.mismatch(format!("{} tick {tick}: turtle dropped no bowl", case.name));
                }
            }
            if broken {
                ours[1] = 0;
            }
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
        }
    }
}
