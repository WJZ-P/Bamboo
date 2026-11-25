//! 蜜蜂找巢候选表的有界逐刻对拍（复用 NoAI 夹具思想，不跑真目标链 / 寻路）。
//!
//! 官方一侧用 `PoiManager.getInRange(#bee_home, blockPosition, 20, ANY)` 建有序
//! 候选，按 `BeehiveBlockEntity.isFull()` 过滤满巢，再按
//! `Comparator.comparingDouble(blockPos.distSqr(pos))` 稳定排序；Bamboo 一侧调
//! `bee_goals::nearby_hives_with_space`（生产路径）复算。逐刻改哪些巢满，比整张
//! 有序候选表：POI 遍历序、等距并列、满巢换巢、整数距离排序都在这里钉死。
//! 三处改坏确认分别把排序改回浮点、漏掉满巢过滤、把半径 20 改成 19。

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, LevelRandom, World,
    behaviour::machines::beehive_entity::{BeehiveEntity, Occupant},
    entity::ctx::BlockView,
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeeLocateCase {
    name: String,
    center: [i32; 3],
    bee_pos: [f64; 3],
    /// 每个巢 `[x, y, z, 全局方块状态号, kind]`，`kind` 0 = beehive、1 = bee_nest。
    hives: Vec<[i64; 5]>,
    frames: Vec<BeeLocateFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeeLocateFrame {
    /// 每刻每巢是否满（1 = `BeehiveBlockEntity.isFull()`，3 只蜂）。
    full: Vec<i32>,
    /// 官方算出的整张有序候选表。
    candidates: Vec<[i32; 3]>,
}

fn hive_pos(hive: &[i64; 5]) -> BlockPos {
    BlockPos::new(hive[0] as i32, hive[1] as i32, hive[2] as i32)
}

/// 官方 `BlockPos.distSqr(Vec3i)`：整数坐标差的平方和。
fn int_dist_sqr(from: BlockPos, to: BlockPos) -> f64 {
    let dx = f64::from(to.x - from.x);
    let dy = f64::from(to.y - from.y);
    let dz = f64::from(to.z - from.z);
    dx * dx + dy * dy + dz * dz
}

/// 改坏确认用的旧排序键：浮点位置到巢中心 (+0.5) 的距离平方。
fn float_dist_sqr(bee: Vec3, pos: BlockPos) -> f64 {
    let dx = (f64::from(pos.x) + 0.5) - bee.x;
    let dy = (f64::from(pos.y) + 0.5) - bee.y;
    let dz = (f64::from(pos.z) + 0.5) - bee.z;
    dx * dx + dy * dy + dz * dz
}

/// 这一刻的方块实体表：每个 POI 巢都挂一个 `BeehiveEntity`（官方
/// `doesHiveHaveSpace` 要求方块实体存在）；满巢放 3 只蜂让 `is_full()` 为真。
fn block_entities_for(case: &BeeLocateCase, frame: &BeeLocateFrame) -> BlockEntities {
    let mut table = BlockEntities::default();
    for (i, hive) in case.hives.iter().enumerate() {
        let mut hive_entity = BeehiveEntity::new(hive_pos(hive));
        if frame.full.get(i).copied().unwrap_or(0) != 0 {
            for _ in 0..3 {
                hive_entity.store(Occupant::generated(599));
            }
        }
        table.insert(Box::new(hive_entity));
    }
    table
}

/// 正常路径直接调生产函数；三处改坏确认在这里用同一套 `BlockView` 复算成错的候选表，
/// 交给 `Report` 的比较器抓出来。
fn compute_candidates(
    world: &World<'_>,
    origin: BlockPos,
    bee_pos: Vec3,
    broken: Option<&str>,
) -> Vec<BlockPos> {
    match broken {
        Some("bee_locate_float_sort") => {
            let mut hives = world.beehives_near(origin, 20);
            hives.retain(|pos| !world.beehive_full(*pos));
            hives.sort_by(|a, b| {
                float_dist_sqr(bee_pos, *a).total_cmp(&float_dist_sqr(bee_pos, *b))
            });
            hives
        }
        Some("bee_locate_no_filter") => {
            let mut hives = world.beehives_near(origin, 20);
            hives.sort_by(|a, b| int_dist_sqr(origin, *a).total_cmp(&int_dist_sqr(origin, *b)));
            hives
        }
        Some("bee_locate_radius19") => {
            let mut hives = world.beehives_near(origin, 19);
            hives.retain(|pos| !world.beehive_full(*pos));
            hives.sort_by(|a, b| int_dist_sqr(origin, *a).total_cmp(&int_dist_sqr(origin, *b)));
            hives
        }
        _ => mc_bamboo_blocks::entity::mob::bee_goals::nearby_hives_with_space(origin, world),
    }
}

impl Report {
    pub(super) fn replay_bee_locate(&mut self, case: &BeeLocateCase) {
        self.cases += 1;
        let broken = self.break_test.clone();
        let origin = BlockPos::new(case.center[0], case.center[1], case.center[2]);
        let bee_pos = Vec3::new(case.bee_pos[0], case.bee_pos[1], case.bee_pos[2]);
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
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
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: EntityStore::new(),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            containers: Default::default(),
            explosion_break: 0,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
        };
        for hive in &case.hives {
            level
                .blocks
                .insert(hive_pos(hive), BlockState(hive[3] as u32));
        }
        level.register_poi_from_blocks();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        for (index, frame) in case.frames.iter().enumerate() {
            level.block_entities = block_entities_for(case, frame);
            let ours = {
                let world = World::new(&mut level, &mut ticks);
                compute_candidates(&world, origin, bee_pos, broken.as_deref())
            };
            let ours: Vec<[i32; 3]> = ours.iter().map(|pos| [pos.x, pos.y, pos.z]).collect();
            if ours != frame.candidates {
                self.mismatch(format!(
                    "{} tick {index}: official {:?}, bamboo {:?}",
                    case.name, frame.candidates, ours
                ));
            }
            self.steps += 1;
        }
    }
}
