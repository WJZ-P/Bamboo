//! `emptyAllLivingFromHive` 与 `angerNearbyBees` 的逐刻对拍。
//!
//! 官方侧用真的 `BeehiveBlockEntity` / `Bee` / `BeehiveBlock` 跑两种方法；
//! Bamboo 侧按同一套顺序重放：先 `angerNearbyBees`（每只无目标的蜂取一次
//! 世界随机源），再放出三只蜂、按玩家到每只蜂的距离设目标或 400 刻倒计时；
//! 挖巢场景的两段顺序相反。逐刻比每只蜂的 target / 倒计时和世界随机源。
//! 一处改坏确认少取一次随机数。

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, LevelRandom, World,
    behaviour::machines::beehive_entity::BeehiveEntity,
    entity::{Entity, ctx::PlayerView},
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BeeReleaseCase {
    name: String,
    players: i32,
    campfire: bool,
    destroy: bool,
    frames: Vec<BeeReleaseFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct BeeReleaseFrame {
    /// 每只蜂 `[目标玩家下标（-1 为无）, stayOutOfHiveCountdown]`。
    bees: Vec<[i32; 2]>,
    level_seed: i64,
}

fn player(index: i32) -> PlayerView {
    PlayerView {
        uuid: Some(((1_u128 << 64) | (2 + index as u128)).to_be_bytes()),
        uid: (1 << 24) + 1024 + index,
        yaw: 0.0,
        pitch: 0.0,
        position: Vec3::new(1.5 + f64::from(index), 62.0, 0.5),
        delta: Vec3::default(),
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(0.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
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
    }
}

impl BeeReleaseCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            blocks: {
                let mut map = HashMap::from([(
                    BlockPos::new(0, 62, 0),
                    BlockState(mc_bamboo_data::blocks::BEEHIVE)
                        .with(mc_bamboo_blocks::state::LEVEL_HONEY, 5),
                )]);
                if self.campfire {
                    map.insert(
                        BlockPos::new(0, 61, 0),
                        BlockState(mc_bamboo_data::blocks::CAMPFIRE)
                            .with(mc_bamboo_blocks::state::LIT, true),
                    );
                }
                map
            },
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
            entities: EntityStore::new(),
            players: (0..self.players).map(player).collect(),
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }
}

impl Report {
    pub(super) fn replay_bee_release(&mut self, case: &BeeReleaseCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("bee_release");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let hive_pos = BlockPos::new(0, 62, 0);
        // 两只已在世界里的蜂：近（2.5, 62, 0.5）与远（22.5, 62, 0.5）。
        let mut ids = Vec::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for (i, x) in [2.5_f64, 22.5_f64].into_iter().enumerate() {
                let mut bee = Entity::by_mob_type(
                    mc_bamboo_blocks::entity::mob::MobType::Bee,
                    Vec3::new(x, 62.0, 0.5),
                );
                bee.with_mob(|_, mob| mob.no_ai = true);
                ids.push(world.spawn_entity(bee));
                let _ = i;
            }
            // 蜂巢方块实体里放三只蜂。
            let mut hive = BeehiveEntity::new(hive_pos);
            for i in 0..3 {
                hive.store(
                    mc_bamboo_blocks::behaviour::machines::beehive_entity::Occupant::generated(
                        599 - i,
                    ),
                );
            }
            world.set_block_entity(Box::new(hive));
        }
        let mut released: Vec<i32> = Vec::new();
        for (index, expected) in case.frames.iter().enumerate() {
            if index == 1 {
                ticks.set_time(1001);
                let mut world = World::new(&mut level, &mut ticks);
                if !case.campfire && !case.destroy {
                    anger_nearby(&mut world, &ids, case.players, broken);
                }
                release_all(
                    &mut world,
                    hive_pos,
                    &case.level().players[0],
                    case.campfire,
                    &mut released,
                );
                if case.destroy {
                    let mut all = ids.clone();
                    all.extend(released.iter().copied());
                    anger_nearby(&mut world, &all, case.players, broken);
                }
            }
            let mut ours = Vec::new();
            let mut all = ids.clone();
            all.extend(released.iter().copied());
            for id in &all {
                let Some(entity) = level.entities.get(*id) else {
                    continue;
                };
                let Some(mob) = entity.kind.mob() else {
                    continue;
                };
                let target = mob.target.map(|uid| uid - ((1 << 24) + 1024)).unwrap_or(-1);
                ours.push([target, mob.bee.stay_out_of_hive]);
            }
            let frame = BeeReleaseFrame {
                bees: ours,
                level_seed: level.random.source().seed(),
            };
            if frame != *expected {
                self.mismatch(format!(
                    "{} tick {index} bee release: official {expected:?}, bamboo {frame:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }
}

/// `BeehiveBlock.angerNearbyBees`：范围内的蜂按 uid 升序，每只无目标的蜂
/// 取一次 `world.random.nextInt(玩家数)`；玩家列表为空时不取。
fn anger_nearby(world: &mut World, ids: &[i32], player_count: i32, broken: bool) {
    if player_count <= 0 {
        return;
    }
    let mut players: Vec<_> = (0..player_count).map(|i| (1 << 24) + 1024 + i).collect();
    players.sort_unstable();
    for (index, id) in ids.iter().enumerate() {
        let inside_and_no_target = world
            .level()
            .entity_public(*id)
            .and_then(|public| match public.kind {
                mc_bamboo_blocks::entity::KindPublic::Mob(_) => Some(public.position),
                _ => None,
            })
            .is_some_and(|pos| {
                pos.x >= -8.0
                    && pos.x < 9.0
                    && pos.y >= 56.0
                    && pos.y < 69.0
                    && pos.z >= -8.0
                    && pos.z < 9.0
            })
            && !world
                .level()
                .entity_public(*id)
                .and_then(|public| match public.kind {
                    mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.has_target),
                    _ => None,
                })
                .unwrap_or(true);
        if !inside_and_no_target {
            continue;
        }
        // 官方对范围内每只蜂都取一次；改坏确认故意跳过第一只。
        if broken && index == 0 {
            continue;
        }
        let pick = world.random().next_int(player_count) as usize;
        let target = players.get(pick).copied();
        if let Some(target) = target {
            world.with_entity_mut(*id, &mut |entity| {
                entity.with_mob(|_, mob| mob.target = Some(target));
            });
        }
    }
}

/// `emptyAllLivingFromHive`：放出三只蜂，玩家到每只蜂 4 格内才设目标 / 400 刻。
fn release_all(
    world: &mut World,
    hive: BlockPos,
    player: &PlayerView,
    sedated: bool,
    released: &mut Vec<i32>,
) {
    let state =
        BlockState(mc_bamboo_data::blocks::BEEHIVE).with(mc_bamboo_blocks::state::LEVEL_HONEY, 5);
    // 先记下已有的 ID，`release_all` 之后用生成快照拿到新蜂。
    let before: std::collections::BTreeSet<i32> = world.level().entity_uids().into_iter().collect();
    let _ = world.with_block_entity::<BeehiveEntity, _>(hive, |entity, world| {
        entity.release_all(
            world,
            state,
            mc_bamboo_blocks::behaviour::machines::beehive_entity::ReleaseStatus::Emergency,
            Some(*player),
        );
    });
    for public in world.level().entity_uids() {
        if !before.contains(&public) && !released.contains(&public) {
            released.push(public);
        }
    }
    let _ = sedated;
}
