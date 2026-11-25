//! 刷怪笼对拍：逐刻比 `SpawnerBlockEntity.serverTick` 之后的倒计时、当前 `SpawnData` 的
//! 实体、这一轮刷出的实体（类型 + 位置）和世界随机源。场景覆盖玩家在范围内外、倒数、
//! `MaxNearbyEntities` 封顶、光照规则挡住、刷怪蛋改实体。
//!
//! Spawner comparison: each tick compares the countdown, the current `SpawnData`
//! entity, the mobs spawned this tick (type + position) and the level random after
//! `SpawnerBlockEntity.serverTick`.

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, KindPublic, LevelRandom,
    World,
    behaviour::machines::spawner::SpawnerBlockEntity,
    entity::{Entity, ctx::PlayerView, mob::MobType},
};
use mc_bamboo_core::{BlockPos, Vec3};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnerCase {
    name: String,
    pos: [i32; 3],
    blocks: Vec<[i32; 4]>,
    player: [f64; 3],
    difficulty: i32,
    entity: String,
    delay: i32,
    pre_place: i32,
    block_light: i32,
    egg: bool,
    frames: Vec<SpawnerFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SpawnerFrame {
    delay: i32,
    entity: String,
    spawned: Vec<[i32; 4]>,
    level_seed: i64,
}

impl SpawnerCase {
    fn kind(&self) -> MobType {
        MobType::by_name(&self.entity).expect("scene entity is modelled")
    }

    fn level(&self, broken: &str) -> MobLevel {
        MobLevel {
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect::<HashMap<_, _>>(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: self.difficulty,
                // 地下刷怪笼：天空光 0、方块光按场景给；暗度 15 让怪物认为够黑。
                // An underground spawner: sky 0, the scene's block light; darken 15.
                sky_darken: 15,
                spawner_light: Some((
                    0,
                    if broken == "spawner_lit" {
                        0
                    } else {
                        self.block_light.max(0)
                    },
                )),
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            entities: EntityStore::new(),
            players: vec![PlayerView {
                uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
                uid: (1 << 24) + 1024,
                yaw: 0.0,
                pitch: 0.0,
                position: Vec3::new(self.player[0], self.player[1], self.player[2]),
                delta: Vec3::default(),
                width: 0.6,
                height: 1.8,
                eye_height: 1.62,
                health: 20.0,
                look_direction: Vec3::view_vector(0.0, 0.0),
                gaze_disguise: false,
                alive: true,
                spectator: false,
                game_mode: 0,
                mining_fatigue: None,
                creative: false,
                flying: false,
                explosion_knockback_resistance: 0.0,
                sneaking: false,
                sprinting: false,
                swimming: false,
                invisible: false,
                armor_cover: 0.0,
                main_hand: 0,
                off_hand: 0,
                combat: Default::default(),
                effects: Default::default(),
                team: None,
            }],
            game: None,
            wrong_was_hurt: false,
            containers: Default::default(),
            explosion_break: 0,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }
}

impl Report {
    pub(super) fn replay_spawner(&mut self, case: &SpawnerCase) {
        self.cases += 1;
        let broken = match self.break_test.as_deref() {
            Some("spawner_delay") => "spawner_delay",
            Some("spawner_lit") => "spawner_lit",
            Some("spawner_entity") => "spawner_entity",
            _ => "",
        };
        let pos = BlockPos::new(case.pos[0], case.pos[1], case.pos[2]);
        let mut level = case.level(broken);
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut spawner = SpawnerBlockEntity::new(pos);
            // `spawner_entity` 改坏确认：故意把实体换成僵尸，让非僵尸场景的 `entity` /
            // `spawned` 类型对不上。
            let kind = if broken == "spawner_entity" {
                MobType::Zombie
            } else {
                case.kind()
            };
            spawner.set_entity_id(kind, &world);
            spawner.set_delay_for_test(if broken == "spawner_delay" {
                case.delay + 1
            } else {
                case.delay
            });
            world.set_block_entity(Box::new(spawner));
            // `MaxNearbyEntities` 封顶场景：预置满额的同种实体。
            // The cap scene pre-fills the area with the exact entity type.
            for i in 0..case.pre_place {
                let x = 1.5 + f64::from(i % 3);
                let z = 1.5 + f64::from(i / 3);
                world.spawn_entity(Entity::by_mob_type(MobType::Zombie, Vec3::new(x, 64.0, z)));
            }
        }
        // 预置的封顶实体不是「这一轮刷出来的」：官方放进 `tracked`，这里清掉出生快照。
        // The pre-filled cap entities are not spawned this tick: the official side puts
        // them in `tracked`, so drop their spawn snapshots here.
        let _ = level.entities.take_spawn_snapshots();
        for (index, expected) in case.frames.iter().enumerate() {
            if index > 0 {
                ticks.set_time(1000 + index as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if case.egg && index == 1 {
                    let new_kind = MobType::Skeleton;
                    let _ =
                        world.with_block_entity::<SpawnerBlockEntity, _>(pos, |spawner, world| {
                            spawner.set_entity_id(new_kind, world);
                        });
                }
                world.tick_block_entities();
            }
            let spawned: Vec<[i32; 4]> = level
                .entities
                .take_spawn_snapshots()
                .into_iter()
                .filter_map(|public| match &public.kind {
                    KindPublic::Mob(mob) => Some([
                        mob.mob_type.entity_type(),
                        (public.position.x * 2.0) as i32,
                        public.position.y as i32,
                        (public.position.z * 2.0) as i32,
                    ]),
                    _ => None,
                })
                .collect();
            let ours = SpawnerFrame {
                delay: level
                    .block_entities
                    .get::<SpawnerBlockEntity>(pos)
                    .map(|spawner| spawner.delay_for_test())
                    .unwrap_or_default(),
                entity: level
                    .block_entities
                    .get::<SpawnerBlockEntity>(pos)
                    .map(|spawner| spawner.entity_for_test())
                    .unwrap_or_default(),
                spawned,
                level_seed: level.random.source().seed(),
            };
            if ours != *expected {
                self.mismatch(format!(
                    "{} tick {index}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }
}
