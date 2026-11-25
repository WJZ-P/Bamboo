//! 幻翼自定义刷怪器对拍：逐刻比生成的幻翼（落点、大小、锚点）、世界随机源和
//! `nextTick`。官方一侧在假 `ServerLevel` 里跑真的 `PhantomSpawner.tick`。
//!
//! Phantom custom-spawner comparison: each tick compares the spawned phantoms
//! (spawn point, size, anchor), the level random and `nextTick`. The official
//! side runs the real `PhantomSpawner.tick` in a fake `ServerLevel`.

use mc_bamboo_blocks::{
    World,
    entity::mob::{
        MobType,
        phantom_spawner::{self, SpawnerPlayer, SpawnerRules},
    },
};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::squid::take_hurt_marked;
use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PhantomSpawnCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    sky_darken: i32,
    sea_level: i32,
    can_see_sky: bool,
    has_sky_light: bool,
    clock_ticks: i64,
    spawn_mobs: bool,
    spawn_monsters: bool,
    spawn_phantoms: bool,
    players: Vec<PhantomSpawnPlayer>,
    /// 在这些刻把 `nextTick` 直接设成 1，制造多次刷怪尝试。
    ///
    /// Ticks at which `nextTick` is set to 1 for extra spawn attempts.
    forces: Vec<i32>,
    frames: Vec<PhantomSpawnFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhantomSpawnPlayer {
    position: [f64; 3],
    spectator: bool,
    time_since_rest: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PhantomSpawnFrame {
    spawns: Vec<[i32; 7]>,
    level_seed: i64,
    next_tick: i32,
}

impl PhantomSpawnCase {
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
                difficulty: 2,
                sky_darken: self.sky_darken,
                sea_level: self.sea_level,
                can_see_sky: self.can_see_sky,
                clock: self.clock_ticks,
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
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            natural_spawn: None,
            biome: None,
        }
    }

    fn players(&self) -> Vec<SpawnerPlayer> {
        self.players
            .iter()
            .map(|player| SpawnerPlayer {
                position: Vec3::new(player.position[0], player.position[1], player.position[2]),
                spectator: player.spectator,
                time_since_rest: player.time_since_rest,
            })
            .collect()
    }

    fn rules(&self) -> SpawnerRules {
        SpawnerRules {
            spawn_mobs: self.spawn_mobs,
            spawn_monsters: self.spawn_monsters,
            spawn_phantoms: self.spawn_phantoms,
            has_sky_light: self.has_sky_light,
        }
    }
}

impl Report {
    pub(super) fn replay_phantom_spawn(&mut self, case: &PhantomSpawnCase) {
        self.cases += 1;
        // 改坏确认：把 `spawn_phantoms` 关掉，整段不取数也不刷怪，官方一侧照常。
        // Break check: turn `spawn_phantoms` off so this draws and spawns nothing.
        let broken = self.break_test.as_deref() == Some("phantom_spawn");
        let players = case.players();
        let mut rules = case.rules();
        if broken {
            rules.spawn_phantoms = false;
        }
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut spawner = phantom_spawner::PhantomSpawner { next_tick: 0 };
        for (tick, expected) in case.frames.iter().enumerate() {
            if case.forces.contains(&(tick as i32)) {
                spawner.next_tick = 1;
            }
            {
                let mut world = World::new(&mut level, &mut ticks);
                spawner.tick(&mut world, &players, rules);
            }
            let mut spawns = Vec::new();
            for snapshot in level.entities.take_spawn_snapshots() {
                let Some(entity) = level.entities.get(snapshot.uid) else {
                    continue;
                };
                let Some(mob) = entity.kind.mob() else {
                    continue;
                };
                if mob.mob_type != MobType::Phantom {
                    continue;
                }
                let pos = entity.block_position();
                let anchor = mob.phantom.anchor.expect("finalizeSpawn sets the anchor");
                spawns.push([
                    pos.x,
                    pos.y,
                    pos.z,
                    mob.phantom.size,
                    anchor.x,
                    anchor.y,
                    anchor.z,
                ]);
            }
            let ours = PhantomSpawnFrame {
                spawns,
                level_seed: level.random.source().seed(),
                next_tick: spawner.next_tick,
            };
            let label = format!("{} tick {tick}", case.name);
            if ours != *expected {
                self.mismatch(format!(
                    "{label} spawner: official {expected:?}, bamboo {ours:?}"
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!(
            "{}: {} ticks, {} spawns",
            case.name,
            case.frames.len(),
            case.frames
                .iter()
                .map(|frame| frame.spawns.len())
                .sum::<usize>()
        );
    }
}

/// 幻翼 AI 对拍：绕锚点盘旋、俯冲、咬中、白天着火。官方一侧在假世界里逐刻跑真的
/// `Phantom`（目标选择器 + 自己的三个目标 + `PhantomMoveControl`）。
///
/// Phantom AI comparison: circling the anchor, swooping, biting and daylight burning.
/// The official side runs the real `Phantom` (its target selector, its three goals and
/// `PhantomMoveControl`) in the fake level, tick by tick.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PhantomCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    anchor: [i32; 3],
    has_player: bool,
    player_id: i32,
    player_pos: [f64; 3],
    sky_darken: i32,
    can_see_sky: bool,
    monsters_burn: bool,
    bright_outside: bool,
    sea_level: i32,
    frames: Vec<PhantomFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhantomFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<PhantomAiState>,
    level_seed: i64,
    level_events: Vec<[i32; 5]>,
    packets: Vec<String>,
}

/// 一刻结束时幻翼的样子，字段和官方一侧的输出一一对应。
///
/// The phantom at the end of a tick, field for field with the official output.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PhantomAiState {
    /// 位置、速度和两个朝向的原始位。
    ///
    /// Position, velocity and both rotations as raw bits.
    motion: [i64; 8],
    /// `yBodyRot`、`yHeadRot` 的原始位。
    ///
    /// Raw bits of `yBodyRot` and `yHeadRot`.
    body: [i32; 2],
    size: i32,
    seed: i64,
    /// 血量位、`hurtTime`、`deathTime`、`noActionTime`、俯冲标志、着地、着火刻数。
    ///
    /// Health bits, `hurtTime`, `deathTime`, `noActionTime`, the swoop flag, on-ground
    /// and the remaining fire ticks.
    state: [i64; 7],
    /// 有没有目标，加目标实体 ID。
    ///
    /// Whether there is a target, then the target entity id.
    target: [i32; 2],
    /// 有没有锚点，加锚点坐标。
    ///
    /// Whether there is an anchor, then its block position.
    anchor: [i32; 4],
    /// `moveTargetPoint` 三个坐标的原始位。
    ///
    /// Raw bits of the three `moveTargetPoint` coordinates.
    #[serde(rename = "move")]
    move_target: [i64; 3],
}

impl PhantomCase {
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
                difficulty: 2,
                sky_darken: self.sky_darken,
                can_see_sky: self.can_see_sky,
                monsters_burn: self.monsters_burn,
                bright_outside: self.bright_outside,
                sea_level: self.sea_level,
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
            players: if self.has_player {
                vec![super::bat::player_view(self.player_id, self.player_pos)]
            } else {
                Vec::new()
            },
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            natural_spawn: None,
            biome: None,
        }
    }
}

fn phantom_ai_state(entity: &Entity) -> PhantomAiState {
    let mob = entity.kind.mob().expect("a phantom is a mob");
    let (position, delta) = (entity.position, entity.delta);
    PhantomAiState {
        motion: [
            position.x.to_bits() as i64,
            position.y.to_bits() as i64,
            position.z.to_bits() as i64,
            delta.x.to_bits() as i64,
            delta.y.to_bits() as i64,
            delta.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(mob.x_rot.to_bits() as i32),
        ],
        body: [
            mob.y_body_rot.to_bits() as i32,
            mob.y_head_rot.to_bits() as i32,
        ],
        size: mob.phantom.size,
        seed: entity.random.state(),
        state: [
            i64::from(mob.health.to_bits() as i32),
            i64::from(mob.hurt_time),
            i64::from(mob.death_time),
            i64::from(mob.no_action_time),
            i64::from(mob.phantom.swooping),
            i64::from(entity.on_ground),
            i64::from(entity.remaining_fire_ticks),
        ],
        target: mob.target.map_or([0, 0], |target| [1, target]),
        anchor: mob
            .phantom
            .anchor
            .map_or([0, 0, 0, 0], |anchor| [1, anchor.x, anchor.y, anchor.z]),
        move_target: [
            mob.phantom.move_target.x.to_bits() as i64,
            mob.phantom.move_target.y.to_bits() as i64,
            mob.phantom.move_target.z.to_bits() as i64,
        ],
    }
}

impl Report {
    pub(super) fn replay_phantom(&mut self, case: &PhantomCase) {
        self.cases += 1;
        // 改坏确认：出生点高一格，第一刻的位置就对不上。
        // Break check: spawn one block higher, so the very first frame differs.
        let broken = self.break_test.as_deref() == Some("phantom_ai");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let phantom_id = {
            let mut world = World::new(&mut level, &mut ticks);
            let start = Vec3::new(
                case.start[0],
                case.start[1] + if broken { 1.0 } else { 0.0 },
                case.start[2],
            );
            let mut entity = Entity::by_mob_type(MobType::Phantom, start);
            let [x, y, z] = case.anchor;
            entity.with_mob(|_, mob| mob.phantom.anchor = Some(BlockPos::new(x, y, z)));
            world.spawn_entity(entity)
        };
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let mut was_gone = false;
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut level_events = Vec::new();
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                for effect in world.drain_effects() {
                    if let mc_bamboo_blocks::Effect::LevelEvent {
                        event, pos, data, ..
                    } = effect
                    {
                        level_events.push([event, pos.x, pos.y, pos.z, data]);
                    }
                }
            }
            let events = level.entities.take_mob_events();
            let mut packets = event_packets(&events, || sound_source.next_long());
            let public = level.entities.public_at(phantom_id).cloned();
            for spawned in level.entities.take_spawn_snapshots() {
                if spawned.uid != phantom_id {
                    continue;
                }
                let snapshot = Snapshot::of(&spawned);
                let tracked = tracking.tracked_for_oracle(&snapshot);
                packets.extend(tracked.pairing_packets(&snapshot));
            }
            match &public {
                Some(public) if tick != 0 => {
                    let snapshot = Snapshot::of(public);
                    let hurt_marked = take_hurt_marked(&mut level, &mut ticks, phantom_id);
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        hurt_marked,
                    ));
                }
                None if !was_gone => packets.push(packet_body(&RemoveEntities {
                    entity_ids: &[phantom_id],
                })),
                _ => {}
            }
            was_gone = public.is_none();
            let label = format!("{} tick {tick}", case.name);
            if was_gone != expected.gone {
                self.mismatch(format!(
                    "{label} gone: official {}, bamboo {was_gone}",
                    expected.gone
                ));
            }
            let ours = level
                .entities
                .get(phantom_id)
                .map(|entity| phantom_ai_state(&entity));
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} phantom: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            if level_events != expected.level_events {
                self.mismatch(format!(
                    "{label} level events: official {:?}, bamboo {level_events:?}",
                    expected.level_events
                ));
            }
            let level_seed = level.random.source().seed();
            if level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{label} level random: official {}, bamboo {level_seed}",
                    expected.level_seed
                ));
            }
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                &packets,
                &expected.packets,
            );
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
