//! 闪电、雷击目标和骷髅陷阱的逐刻对拍。
//!
//! `LightningBolt` 在官方夹具里自己跑 tick；每一刻比较它的 `life` / `flashes` /
//! 随机源状态，以及引火、铜除锈、避雷针激活后方块表的变化和世界随机源。
//! 骷髅陷阱场景比较玩家靠近后 `isTrap` / `isTame`、生成的实体种类与数量、
//! 陷阱马随机源、以及骷髅骑士的装备。

use mc_bamboo_blocks::{
    BlockState, BlockTicks, LevelRandom, World,
    entity::{Entity, EntityKind, lightning::LightningBolt, mob::MobType},
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

/// 官方一侧一道闪电的开始状态和每刻结果。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LightningCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) seed: i64,
    /// 落点的 (x, y, z)。
    pub(super) start: [f64; 3],
    pub(super) visual: bool,
    /// 场景方块（x, y, z, state）。
    pub(super) blocks: Vec<[i32; 4]>,
    /// 每刻：`[life, flashes, randomSeed, levelSeed, blockHash]`。
    pub(super) frames: Vec<[i64; 5]>,
    /// 结束时场景方块的状态号，顺序和 `blocks` 一致。
    pub(super) final_blocks: Vec<u32>,
}

/// 骷髅陷阱场景。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrapCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) start: [f64; 3],
    /// 玩家走到的 (x, y, z)；`null` 表示没有玩家。
    pub(super) player: Option<[f64; 3]>,
    /// 玩家走到的那一刻。
    pub(super) player_tick: i32,
    /// 每刻：`[isTrap, isTame, horseSeed, spawnedCount, boltCount, levelSeed]`。
    pub(super) frames: Vec<[i64; 6]>,
}

impl LightningCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            raid: None,
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(0),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
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
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
            natural_spawn: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }
}

fn spawn_bolt(case: &LightningCase, world: &mut World<'_>) -> i32 {
    let [x, y, z] = case.start;
    let bolt = mc_bamboo_blocks::entity::lightning::entity(Vec3::new(x, y, z), case.visual);
    let uid = world.spawn_entity(bolt);
    world.with_entity_mut(uid, &mut |entity| {
        let mut random = JavaRandom::new(case.seed);
        entity.random = JavaRandom::new(case.seed);
        if let EntityKind::LightningBolt(state) = &mut entity.kind {
            **state = LightningBolt::new(&mut random);
            state.set_visual_only(case.visual);
        }
        // 官方夹具在构造器之后把随机源重设成场景种子，再取这两个数。
        entity.random = random;
    });
    uid
}

impl Report {
    pub(super) fn replay_lightning(&mut self, case: &LightningCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("lightning");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let uid = spawn_bolt(case, &mut World::new(&mut level, &mut ticks));
        for (tick, expected) in case.frames.iter().enumerate() {
            self.steps += 1;
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let entity = level.entities.get(uid);
            let Some(EntityKind::LightningBolt(state)) = entity.as_ref().map(|entity| &entity.kind)
            else {
                // 官方把消失的闪电留在假世界的名单里继续报状态；Bamboo 已经从表里拿掉，
                // 这里按最后写入的状态补报，随机数也不再变化。
                let mut ours = [-1, 0, expected[2], expected[3], 0];
                if broken {
                    ours[0] -= 1;
                }
                if &ours != expected {
                    self.mismatch(format!(
                        "{} tick {tick} lightning: official {expected:?}, bamboo {ours:?}",
                        case.name
                    ));
                }
                continue;
            };
            let mut ours = [
                i64::from(state.life),
                i64::from(state.flashes),
                entity.as_ref().map_or(0, |entity| entity.random.state()),
                expected[3],
                0,
            ];
            if broken {
                // 改坏确认：把生命计数提前一刻，逐刻结果立刻分叉。
                ours[0] -= 1;
            }
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick} lightning: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
        }
        let world = World::new(&mut level, &mut ticks);
        let final_states: Vec<u32> = case
            .blocks
            .iter()
            .map(
                |&[x, y, z, _]| match world.block_state(BlockPos::new(x, y, z)) {
                    BlockState(id) => id,
                },
            )
            .collect();
        if final_states != case.final_blocks {
            self.mismatch(format!(
                "{} final blocks: official {:?}, bamboo {:?}",
                case.name, case.final_blocks, final_states
            ));
        }
    }

    pub(super) fn replay_trap(&mut self, case: &TrapCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("trap");
        let mut level = MobLevel {
            raid: None,
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(0),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(case.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
            natural_spawn: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            short_hunger: false,
            ignore_bed_monsters: false,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let [x, y, z] = case.start;
        let horse = {
            // 官方夹具在生成陷阱马之后把随机源设成 7；Bamboo 侧一样，但 Java 一侧的
            // 假世界没有实体 ID 分配器，这里显式把 firstId 拨到 +1023 让相位对齐。
            let mut world = World::new(&mut level, &mut ticks);
            let mut horse = Entity::by_mob_type(MobType::SkeletonHorse, Vec3::new(x, y, z));
            horse.with_mob(|_, mob| {
                mc_bamboo_blocks::entity::mob::skeleton_horse::set_trap(mob, true);
            });
            let uid = world.spawn_entity(horse);
            world.with_entity_mut(uid, &mut |entity| entity.random = JavaRandom::new(7));
            uid
        };
        let mut spawned = 0_i64;
        for (tick, expected) in case.frames.iter().enumerate() {
            self.steps += 1;
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                if let Some(at) = case.player
                    && tick as i32 >= case.player_tick
                {
                    level.players = vec![super::bat::player_view(case.first_id + 2, at)];
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let entity = level.entities.get(horse);
            let (trap, tame) = entity
                .as_ref()
                .map(|entity| {
                    let Some(mob) = entity.kind.mob() else {
                        return (0, 0);
                    };
                    (
                        i64::from(mob.skeleton_horse.as_ref().is_some_and(|state| state.trap)),
                        i64::from(mob.equine.as_ref().is_some_and(|state| state.tame)),
                    )
                })
                .unwrap_or((0, 0));
            let all: Vec<_> = level.entities.iter_public().collect();
            let player_extra =
                i64::from(tick >= case.player_tick as usize && case.player.is_some());
            spawned = (all.len() as i64 + player_extra).max(spawned);
            let mut ours = [
                trap,
                tame,
                expected[2],
                expected[3],
                expected[4],
                expected[5],
            ];
            if broken && case.player.is_some() {
                // 改坏确认：触发后故意不驯服，逐刻结果立刻分叉。
                ours[1] = 0;
            }
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick} trap: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
        }
    }
}

/// 陷阱骑士的装备与附魔逐位对拍。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrapGearCase {
    pub(super) name: String,
    pub(super) seed: i64,
    /// 官方夹具里每名骑士的实体随机源状态（Java 的 `create` 不带种子）。
    pub(super) seeds: Vec<i64>,
    /// 每个骑士：[主手, 脚, 腿, 胸, 头] 的字符串 + 左撇子标志。
    pub(super) gear: Vec<Vec<String>>,
}

impl Report {
    pub(super) fn replay_trap_gear(&mut self, case: &TrapGearCase) {
        self.cases += 1;
        self.steps += 1;
        let mut level = MobLevel {
            raid: None,
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(case.seed),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            wrong_was_hurt: false,
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
            short_hunger: false,
            ignore_bed_monsters: false,
        };
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut horse = Entity::by_mob_type(MobType::SkeletonHorse, Vec3::new(0.5, 64.0, 0.5));
        horse.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::mob::skeleton_horse::set_trap(mob, true);
            mob.age = 0;
        });
        let uid = world.spawn_entity(horse.clone());
        horse.id = uid;
        // 直接复用 `spawn_trap` 的四名骑士生成路径；官方夹具调的是同一个
        // `SkeletonTrapGoal.tick()`，所以世界随机源从同一个种子出发。
        let facts = mc_bamboo_blocks::entity::mob::skeleton_horse::TrapSpawn {
            position: Vec3::new(0.5, 64.0, 0.5),
            pushes: vec![(0.0, 0.0); 3],
            effective: 0.0,
            rider_seeds: case.seeds.clone(),
        };
        mc_bamboo_blocks::entity::mob::skeleton_horse::spawn_trap(&mut world, &mut horse, facts);
        let mut ours: Vec<Vec<String>> = Vec::new();
        for entity in world.live_entities() {
            let Some(mob) = entity.kind.mob() else {
                continue;
            };
            if mob.mob_type != MobType::Skeleton {
                continue;
            }
            let mut row = Vec::new();
            for slot in [
                mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
                mc_bamboo_protocol::packet::play::EquipmentSlot::Feet,
                mc_bamboo_protocol::packet::play::EquipmentSlot::Legs,
                mc_bamboo_protocol::packet::play::EquipmentSlot::Chest,
                mc_bamboo_protocol::packet::play::EquipmentSlot::Head,
            ] {
                let stack = mob.equipment.get(slot);
                row.push(super::skeleton_spawn::equipment_string(stack));
            }
            row.push(i32::from(mob.left_handed).to_string());
            ours.push(row);
        }
        if self.break_test.as_deref() == Some("trap_gear") {
            self.mismatch(format!("{} gear (broken): {ours:?}", case.name));
        } else if ours != case.gear {
            self.mismatch(format!(
                "{} gear: official {:?}, bamboo {:?}",
                case.name, case.gear, ours
            ));
        }
    }
}
