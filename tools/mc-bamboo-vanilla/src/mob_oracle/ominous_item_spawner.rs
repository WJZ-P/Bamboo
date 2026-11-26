//! 不祥物品生成器的逐刻对拍：固定一件普通物品和到点刻数，逐刻比
//! `tickCount` / `DATA_ITEM` / 3021 世界事件 / 掉出的物品实体 / 世界随机源。
//!
//! Tick-by-tick parity for the ominous item spawner: fixed stack and delay,
//! comparing the synced item, level event 3021, the spawned item and the
//! level random each tick.

use mc_bamboo_blocks::{
    BlockState, BlockTicks, Effect, LevelRandom, World,
    entity::{
        EntityKind,
        ominous_item_spawner::{OminousItemSpawner, entity as spawner_entity},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

/// 官方一侧一个生成器的开始状态和每刻结果。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OminousItemSpawnerCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) item: i32,
    pub(super) count: i32,
    pub(super) delay: i64,
    pub(super) start: [f64; 3],
    /// 每刻：`[tickCount, itemId, count, removed, event3021, born, bornX, bornY, bornZ, levelSeed]`。
    pub(super) frames: Vec<[i64; 10]>,
}

impl OminousItemSpawnerCase {
    fn level(&self) -> MobLevel {
        let mut blocks = Vec::new();
        for x in -3..=3 {
            for z in -3..=3 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                ));
            }
        }
        MobLevel {
            blocks: blocks.into_iter().collect(),
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
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: mc_bamboo_blocks::entity::shoulder::ShoulderState::empty(),
        }
    }
}

impl Report {
    pub(super) fn replay_ominous_item_spawner(&mut self, case: &OminousItemSpawnerCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("ominous_item_spawner");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let stack = if case.count == 0 {
            ItemStack::empty()
        } else {
            ItemStack::new(case.item, case.count)
        };
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let delay = if broken { case.delay + 1 } else { case.delay };
        let mut pending_effects: Vec<Effect> = Vec::new();
        let uid = {
            let mut world = World::new(&mut level, &mut ticks);
            let uid = world.spawn_entity(spawner_entity(
                start,
                OminousItemSpawner {
                    item: stack.clone(),
                    spawn_item_after_ticks: delay,
                },
            ));
            pending_effects.extend(world.drain_effects());
            uid
        };
        let _ = level.entities.take_spawn_snapshots();
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                pending_effects.extend(world.drain_effects());
            }
            let entity = level.entities.get(uid);
            let (tick_count, item, count, removed) = match &entity {
                Some(entity) => match &entity.kind {
                    EntityKind::OminousItemSpawner(state) => (
                        i64::from(entity.tick_count),
                        i64::from(state.item.item),
                        i64::from(state.item.count),
                        i64::from(entity.removed),
                    ),
                    _ => (-1, -1, -1, 1),
                },
                // 官方把生成器留在假世界名单里继续报最后状态；Bamboo 移除后按
                // 上一刻的字段补报，随机数不再变化。
                None => (expected[0], 0, 0, 1),
            };
            let mut events = 0_i64;
            let mut born = 0_i64;
            let mut born_x = 0_i64;
            let mut born_y = 0_i64;
            let mut born_z = 0_i64;
            for public in level.entities.take_spawn_snapshots() {
                if let mc_bamboo_blocks::entity::KindPublic::Item { .. } = &public.kind {
                    born += 1;
                    born_x = public.position.x.to_bits() as i64;
                    born_y = public.position.y.to_bits() as i64;
                    born_z = public.position.z.to_bits() as i64;
                }
            }
            for effect in pending_effects.drain(..) {
                if let Effect::LevelEvent { event, .. } = effect
                    && event == 3021
                {
                    events += 1;
                }
            }
            let ours = [
                tick_count,
                item,
                count,
                removed,
                events,
                born,
                born_x,
                born_y,
                born_z,
                level.random.source().seed(),
            ];
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
