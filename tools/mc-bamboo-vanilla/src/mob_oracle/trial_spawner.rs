//! 试炼刷怪笼完整流程逐刻对拍：等玩家 → 激活刷怪 → 怪被打死 → 等待弹奖励 → 弹出 →
//! 冷却。逐刻比状态机（state/ominous）、`registered_players` / `current_mobs` 数量、
//! 冷却计时、刷出的怪（类型 + 精确位置）、弹出的奖励物品、世界事件（3011/3012/3013/
//! 3014）和世界随机源。场景从 `waiting_for_players` 起步，避开还没建模的
//! `inactive → waiting_for_players` 显示实体（`getOrCreateDisplayEntity`）。
//!
//! Full-flow trial spawner parity: wait for players → spawn → kill the mob → wait for
//! reward ejection → eject → cooldown. Each tick compares the state machine
//! (state/ominous), the `registered_players` / `current_mobs` counts, cooldown timers,
//! the mobs spawned (type + exact position), the ejected items, the level events and
//! the level random. Scenes start in `waiting_for_players` to skip the not-yet-modelled
//! display entity of the `inactive → waiting_for_players` transition.

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, KindPublic, LevelRandom,
    World,
    behaviour::trial_spawner::{
        Config, FullConfig, StateData, TrialSpawner, TrialSpawnerBlockEntity, Weighted,
    },
    entity::ctx::PlayerView,
    state::TrialSpawnerState,
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::Tag;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrialSpawnerCase {
    name: String,
    pos: [i32; 3],
    blocks: Vec<[i32; 4]>,
    player: [f64; 3],
    difficulty: i32,
    entity: String,
    total_mobs: i32,
    simultaneous_mobs: i32,
    cooldown_length: i32,
    player_range: i32,
    kill_tick: i32,
    frames: Vec<TrialSpawnerFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TrialSpawnerFrame {
    state: i32,
    ominous: bool,
    registered: i32,
    mobs: i32,
    cooldown_ends_at: i64,
    next_mob_spawns_at: i64,
    total_mobs_spawned: i32,
    /// 这一刻刷出来的怪：`[entity_type, x 位, y 位, z 位]`。
    ///
    /// The mobs spawned this tick as `[entity_type, x bits, y bits, z bits]`.
    spawned: Vec<[i64; 4]>,
    /// 这一刻弹出的奖励：`[item, count]`。
    ///
    /// The rewards ejected this tick as `[item, count]`.
    ejected: Vec<[i32; 2]>,
    /// 世界事件：`[type, x, y, z, data]`。
    events: Vec<[i32; 5]>,
    seed: i64,
}

fn state_ordinal(state: TrialSpawnerState) -> i32 {
    match state {
        TrialSpawnerState::Inactive => 0,
        TrialSpawnerState::WaitingForPlayers => 1,
        TrialSpawnerState::Active => 2,
        TrialSpawnerState::WaitingForRewardEjection => 3,
        TrialSpawnerState::EjectingReward => 4,
        TrialSpawnerState::Cooldown => 5,
    }
}

impl TrialSpawnerCase {
    fn level(&self, broken: &str) -> MobLevel {
        let mut player = PlayerView {
            uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
            uid: (1 << 24) + 1024,
            yaw: 0.0,
            pitch: 0.0,
            position: Vec3::new(self.player[0], self.player[1], self.player[2]),
            delta: Vec3::default(),
            eye_height: 1.62,
            width: 0.6,
            height: 1.8,
            health: 20.0,
            look_direction: Vec3::view_vector(0.0, 0.0),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            combat: Default::default(),
            effects: Default::default(),
            team: None,
            shoulder_left: None,
            shoulder_right: None,
        };
        if broken == "trial_spawner_far_player" {
            player.position = Vec3::new(self.player[0] + 30.0, self.player[1], self.player[2]);
        }
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
                can_see_sky: true,
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
            players: vec![player],
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
            shoulder: Default::default(),
        }
    }

    fn spawner(&self, broken: &str) -> TrialSpawner {
        let kind = if broken == "trial_spawner_wrong_entity" {
            "minecraft:skeleton"
        } else {
            self.entity.as_str()
        };
        let spawn_data = Tag::Compound(vec![(
            "entity".to_owned(),
            Tag::Compound(vec![("id".to_owned(), Tag::String(kind.to_owned()))]),
        )]);
        let normal = Config {
            spawn_range: 4,
            total_mobs: self.total_mobs as f32,
            simultaneous_mobs: self.simultaneous_mobs as f32,
            total_mobs_added_per_player: 0.0,
            simultaneous_mobs_added_per_player: 0.0,
            ticks_between_spawn: 40,
            spawn_potentials: vec![Weighted::new(spawn_data, 1)],
            loot_tables_to_eject: vec![Weighted::new(
                "minecraft:spawners/trial_chamber/key".to_owned(),
                1,
            )],
            items_to_drop_when_ominous:
                "minecraft:spawners/trial_chamber/items_to_drop_when_ominous".to_owned(),
            raw: Tag::Compound(Vec::new()),
        };
        let config = FullConfig {
            normal: normal.clone(),
            ominous: normal,
            target_cooldown_length: if broken == "trial_spawner_long_cooldown" {
                self.cooldown_length + 1
            } else {
                self.cooldown_length
            },
            required_player_range: self.player_range,
            normal_raw: None,
            ominous_raw: None,
        };
        TrialSpawner {
            state: TrialSpawnerState::WaitingForPlayers,
            ominous: false,
            config,
            data: StateData::default(),
        }
    }
}

impl Report {
    pub(super) fn replay_trial_spawner(&mut self, case: &TrialSpawnerCase) {
        self.cases += 1;
        let broken = self.break_test.clone().unwrap_or_default();
        let pos = BlockPos::new(case.pos[0], case.pos[1], case.pos[2]);
        let mut level = case.level(&broken);
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let spawner = case.spawner(&broken);
            world.set_block_entity(Box::new(TrialSpawnerBlockEntity::new(pos, spawner)));
        }
        let _ = level.entities.take_spawn_snapshots();

        for (index, expected) in case.frames.iter().enumerate() {
            let mut ours_events: Vec<[i32; 5]> = Vec::new();
            if index > 0 {
                ticks.set_time(1000 + index as i64);
                // 到点把已刷出的怪全部清掉，模拟被打死。
                // 改坏确认 `trial_spawner_no_kill`：故意不清怪，让状态机永远停在 active。
                if case.kill_tick > 0
                    && index == case.kill_tick as usize
                    && broken != "trial_spawner_no_kill"
                {
                    let doomed: Vec<i32> = level
                        .entities
                        .iter_public()
                        .filter(|public| matches!(&public.kind, KindPublic::Mob(_)))
                        .map(|public| public.uid)
                        .collect();
                    for uid in doomed {
                        let _ =
                            mc_bamboo_blocks::level::LevelEntities::discard_entity(&mut level, uid);
                    }
                }
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.tick_block_entities();
                    for effect in world.into_effects() {
                        if let mc_bamboo_blocks::Effect::LevelEvent {
                            event, pos, data, ..
                        } = effect
                        {
                            ours_events.push([event, pos.x, pos.y, pos.z, data]);
                        }
                    }
                }
            }
            let spawned_now = level.entities.take_spawn_snapshots();
            let mut spawned: Vec<[i64; 4]> = Vec::new();
            let mut ejected: Vec<[i32; 2]> = Vec::new();
            for public in &spawned_now {
                match &public.kind {
                    KindPublic::Mob(mob) => spawned.push([
                        i64::from(mob.mob_type.entity_type()),
                        public.position.x.to_bits() as i64,
                        public.position.y.to_bits() as i64,
                        public.position.z.to_bits() as i64,
                    ]),
                    KindPublic::Item { stack, .. } => ejected.push([stack.item, stack.count]),
                    _ => {}
                }
            }
            let spawner = level
                .block_entities
                .get::<TrialSpawnerBlockEntity>(pos)
                .map(|be| be.trial_spawner().clone())
                .unwrap_or_default();
            let ours = TrialSpawnerFrame {
                state: state_ordinal(spawner.state),
                ominous: spawner.ominous,
                registered: spawner.data.detected_players.len() as i32,
                mobs: spawner.data.current_mobs.len() as i32,
                cooldown_ends_at: spawner.data.cooldown_ends_at,
                next_mob_spawns_at: spawner.data.next_mob_spawns_at,
                total_mobs_spawned: spawner.data.total_mobs_spawned,
                spawned,
                ejected,
                events: ours_events,
                seed: level.random.source().seed(),
            };
            if ours != *expected {
                self.mismatch(format!(
                    "{} tick {index}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
