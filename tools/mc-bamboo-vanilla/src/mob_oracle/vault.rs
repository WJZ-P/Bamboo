//! 宝库完整流程逐刻对拍：探测玩家激活 → 插钥匙解锁 → 弹出奖励 → 回到未激活。
//! 逐刻比方块状态、`ServerData`（领奖玩家 / 恢复时刻 / 待弹物品）、`SharedData`
//! （展示物品 / 已连接玩家 / 粒子范围）、世界事件（3015/3016/3017）、声音、
//! 弹出的奖励物品和世界随机源。
//!
//! Full-flow vault parity: detect a player → open with a key → eject rewards →
//! deactivate. Each tick compares the block state, `ServerData`, `SharedData`,
//! level events, sounds, the ejected items and the level random.

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, Effect, EntityStore, KindPublic,
    LevelRandom, World,
    behaviour::vault::{
        ServerData, SharedData, Vault, VaultBlockEntity, VaultWorld, default_config,
    },
    entity::ctx::PlayerView,
    state::VaultState,
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

const START_TIME: i64 = 1000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VaultCase {
    name: String,
    pos: [i32; 3],
    blocks: Vec<[i32; 4]>,
    player: [f64; 3],
    insert_tick: i32,
    frames: Vec<VaultFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct VaultFrame {
    state: i32,
    /// 领过奖的玩家 UUID（`[i32;4]`），两侧都排序。
    rewarded: Vec<[i32; 4]>,
    resumes_at: i64,
    /// 待弹出物品（`[item,count]`，保序）。
    items_to_eject: Vec<[i32; 2]>,
    total_ejections: i32,
    /// 当前展示物品（`[item,count]`，空为 `[0,0]`）。
    display: [i32; 2],
    /// 范围内且未领奖的玩家 UUID（`[i32;4]`），两侧都排序。
    connected: Vec<[i32; 4]>,
    particles_bits: i64,
    events: Vec<[i32; 5]>,
    /// `[soundId, volumeBits, pitchBits]`。
    sounds: Vec<[i64; 3]>,
    ejected: Vec<[i32; 2]>,
    seed: i64,
}

fn state_ordinal(state: VaultState) -> i32 {
    match state {
        VaultState::Inactive => 0,
        VaultState::Active => 1,
        VaultState::Unlocking => 2,
        VaultState::Ejecting => 3,
    }
}

/// `[u8;16]` → 4 个大端 `i32`，和 Java 侧的 `UUID` 拆法一致。
fn uuid_ints(uuid: [u8; 16]) -> [i32; 4] {
    [
        i32::from_be_bytes([uuid[0], uuid[1], uuid[2], uuid[3]]),
        i32::from_be_bytes([uuid[4], uuid[5], uuid[6], uuid[7]]),
        i32::from_be_bytes([uuid[8], uuid[9], uuid[10], uuid[11]]),
        i32::from_be_bytes([uuid[12], uuid[13], uuid[14], uuid[15]]),
    ]
}

fn stack_pair(stack: &ItemStack) -> [i32; 2] {
    if stack.is_empty() {
        [0, 0]
    } else {
        [stack.item, stack.count]
    }
}

fn sorted_uuids(uuids: &[[u8; 16]]) -> Vec<[i32; 4]> {
    let mut rows: Vec<[i32; 4]> = uuids.iter().map(|&u| uuid_ints(u)).collect();
    rows.sort();
    rows
}

impl VaultCase {
    fn level(&self, broken: &str) -> MobLevel {
        let mut player = PlayerView {
            main_arm_right: true,
            fall_flying: false,
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
        if broken == "vault_far_player" {
            player.position = Vec3::new(self.player[0] + 30.0, self.player[1], self.player[2]);
        }
        MobLevel {
            raid: None,
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
                difficulty: 2,
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
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
        }
    }
}

impl Report {
    pub(super) fn replay_vault(&mut self, case: &VaultCase) {
        self.cases += 1;
        let broken = self.break_test.clone().unwrap_or_default();
        let pos = BlockPos::new(case.pos[0], case.pos[1], case.pos[2]);
        let mut level = case.level(&broken);
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let vault = Vault {
                state: VaultState::Inactive,
                config: default_config(),
                server: ServerData::default(),
                shared: SharedData::default(),
            };
            world.set_block_entity(Box::new(VaultBlockEntity::new(pos, vault)));
        }
        let _ = level.entities.take_spawn_snapshots();

        let player_uuid = ((1_u128 << 64) | 2).to_be_bytes();
        let key = default_config().key_item;
        let wrong_key = ItemStack::new(1, 1);

        for (index, expected) in case.frames.iter().enumerate() {
            let mut ours_events: Vec<[i32; 5]> = Vec::new();
            let mut ours_sounds: Vec<[i64; 3]> = Vec::new();
            if index > 0 {
                // 改坏确认 `vault_clock_skew`：把整条时间轴挪一刻，状态和展示轮换都会偏。
                let skew = i64::from(broken == "vault_clock_skew");
                ticks.set_time(START_TIME + index as i64 + skew);
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.tick_block_entities();
                    // 改坏确认 `vault_no_unlock`：故意不插钥匙，宝库永远停在 ACTIVE。
                    // `vault_wrong_key`：插一把错误钥匙，只发失败音、不解锁。
                    if index == case.insert_tick as usize && broken != "vault_no_unlock" {
                        let insert_key = if broken == "vault_wrong_key" {
                            wrong_key.clone()
                        } else {
                            key.clone()
                        };
                        world.with_block_entity::<VaultBlockEntity, _>(pos, |be, world| {
                            let mut ctx = VaultWorld::new(world, pos);
                            be.vault_mut().try_insert_key(
                                pos,
                                false,
                                player_uuid,
                                &insert_key,
                                &mut ctx,
                            );
                        });
                    }
                    for effect in world.drain_effects() {
                        match effect {
                            Effect::LevelEvent {
                                event, pos, data, ..
                            } => ours_events.push([event, pos.x, pos.y, pos.z, data]),
                            Effect::Sound {
                                sound,
                                volume,
                                pitch,
                                ..
                            } => ours_sounds.push([
                                i64::from(sound),
                                i64::from(volume.to_bits()),
                                i64::from(pitch.to_bits()),
                            ]),
                            _ => {}
                        }
                    }
                }
            }
            let spawned_now = level.entities.take_spawn_snapshots();
            let mut ejected: Vec<[i32; 2]> = Vec::new();
            for public in &spawned_now {
                if let KindPublic::Item { stack, .. } = &public.kind {
                    ejected.push(stack_pair(stack));
                }
            }
            let vault = level
                .block_entities
                .get::<VaultBlockEntity>(pos)
                .map(|be| be.vault().clone())
                .unwrap_or_else(|| Vault {
                    state: VaultState::Inactive,
                    config: default_config(),
                    server: ServerData::default(),
                    shared: SharedData::default(),
                });
            let ours = VaultFrame {
                state: state_ordinal(vault.state),
                rewarded: sorted_uuids(&vault.server.rewarded_players),
                resumes_at: vault.server.state_updating_resumes_at,
                items_to_eject: vault.server.items_to_eject.iter().map(stack_pair).collect(),
                total_ejections: vault.server.total_ejections_needed,
                display: stack_pair(&vault.shared.display_item),
                connected: sorted_uuids(&vault.shared.connected_players),
                particles_bits: vault.shared.connected_particles_range.to_bits() as i64,
                events: ours_events,
                sounds: ours_sounds,
                ejected,
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
