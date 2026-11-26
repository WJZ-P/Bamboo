//! 尸壳自然生成的骆驼尸壳骑士分支对拍：世界随机源和生成出来的实体表逐位比较。
//!
//! 官方夹具直接调 `Husk.finalizeSpawn(NATURAL, null)`；这里按同样的种子和方块
//! 重放 Bamboo 的 `finalize_spawn_with_group` + `finish_jockeys`。
//!
//! Replay of the camel-husk jockey branch of natural husks: the level random
//! seed and the spawned entity table are compared bit for bit.

use mc_bamboo_blocks::{
    BlockState, EntityStore, LevelRandom,
    entity::{
        Entity,
        mob::{MobType, spawn},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::packet::play::EquipmentSlot;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    #[allow(dead_code, reason = "只用于调试信息")]
    name: String,
    seed: i64,
    blocked: bool,
    group: bool,
    world_seed: i64,
    initial_seed: i64,
    rows: Vec<[i64; 8]>,
}

impl Case {
    fn level(&self) -> MobLevel {
        let mut level_blocks = Vec::new();
        for x in -6..=6 {
            for z in -6..=6 {
                level_blocks.push((BlockPos::new(x, 63, z), BlockState(blocks::STONE)));
                if self.blocked {
                    level_blocks.push((BlockPos::new(x, 65, z), BlockState(blocks::GLASS)));
                }
            }
        }
        MobLevel {
            raid: None,
            blocks: level_blocks.into_iter().collect(),
            random: LevelRandom::new(self.seed),
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
            block_entities: mc_bamboo_blocks::BlockEntities::default(),
            block_events: mc_bamboo_blocks::BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: EntityStore::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
        }
    }
}

impl Report {
    pub(super) fn replay_husk_jockey(&mut self, case: &Case) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = mc_bamboo_blocks::BlockTicks::default();
        let mut group = None;
        let mut rows = Vec::new();
        let world_seed = {
            let mut world = World::new(&mut level, &mut ticks);
            if world.random().seed() != case.initial_seed {
                self.mismatch(format!(
                    "{} initial seed: official {}, bamboo {}",
                    case.name,
                    case.initial_seed,
                    world.random().seed()
                ));
            }
            for index in 0..if case.group { 2 } else { 1 } {
                let mut entity =
                    world.prepare_entity(Entity::by_mob_type(MobType::Husk, Vec3::default()));
                // 官方夹具的 `snapTo(x, y, z, 0, 0)`：位置、上一刻位置和朝向一起设。
                // The fixture's `snapTo(x, y, z, 0, 0)`: position, old position
                // and facing are all set.
                entity.position = Vec3::new(0.5 + f64::from(index) * 3.0, 64.0, 0.5);
                entity.old_position = entity.position;
                entity.y_rot = 0.0;
                entity.with_mob(|_, mob| {
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                });
                spawn::finalize_spawn_with_group(
                    &mut world,
                    MobType::Husk,
                    &mut entity,
                    &mut group,
                );
                let uid = world.add_prepared_entity(entity);
                world.finish_jockeys(uid);
            }
            world.random().seed()
        };
        for entity in level.entities.iter() {
            let Some(mob) = entity.kind.mob() else {
                continue;
            };
            // 骆驼尸壳的朝向在官方夹具里来自不带种子的构造随机源，两边都记 0。
            // A camel husk's facing comes from the unseeded constructor random in
            // the fixture; both sides record zero.
            let yaw = if mob.mob_type == MobType::CamelHusk {
                0
            } else {
                entity.y_rot.to_bits() as i32
            };
            rows.push([
                i64::from(mob.mob_type.entity_type()),
                entity.position.x.to_bits() as i64,
                entity.position.y.to_bits() as i64,
                entity.position.z.to_bits() as i64,
                i64::from(yaw),
                i64::from(mob.health.to_bits() as i32),
                i64::from(mob.equipment.get(EquipmentSlot::MainHand).item),
                i64::from(mob.is_baby()),
            ]);
        }
        // 官方夹具的 `level.spawned` 是生成顺序（坐骑、骑手、尸壳），Bamboo 的实体表
        // 按 ID 排；按类型和位置排序后比较，不受两边落地顺序影响。
        // The official fixture lists spawn order (mount, rider, husk) while
        // Bamboo's table is id-ordered; sort by type and position.
        rows.sort();
        // 改坏确认：把世界随机源往后推一步，骑士分支必须报种子不一致。
        // Breakage check: advance the level random one step; the jockey case
        // must report a seed mismatch.
        if self.break_test.as_deref() == Some("husk_jockey") {
            let _ = level.random.source().next_float();
        }
        let mut expected = case.rows.clone();
        expected.sort();
        let world_seed = if self.break_test.as_deref() == Some("husk_jockey") {
            level.random.source().seed()
        } else {
            world_seed
        };
        if rows != expected {
            self.mismatch(format!(
                "{} entities: official {expected:?}, bamboo {rows:?}",
                case.name
            ));
        }
        if world_seed != case.world_seed {
            self.mismatch(format!(
                "{} world seed: official {}, bamboo {world_seed}",
                case.name, case.world_seed
            ));
        }
        self.steps += 1;
    }
}
