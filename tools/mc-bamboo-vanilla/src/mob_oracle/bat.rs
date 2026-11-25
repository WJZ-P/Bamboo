//! 蝙蝠对拍：逐刻比位置、速度、朝向、随机源、血量、标志字节、飞行目标、摔落距离、
//! 世界事件 1025、掉落、世界随机源和出站包（配对、同步、音效）。
//!
//! Bat comparison: each tick compares position, velocity, rotation, random, health, the
//! flag byte, the flight target, fall distance, level event 1025, drops, the level random
//! and the outgoing packets (pairing, sync, sounds).

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::{
    squid::{cow_attack, take_hurt_marked},
    *,
};

/// 官方一侧第几刻让牛打蝙蝠：先 1 点打醒，再 10 点打死。
///
/// The ticks on which the official cow hits the bat: 1 damage to wake it, 10 to kill.
const HURT_TICKS: [(usize, f32); 2] = [(10, 1.0), (60, 10.0)];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BatCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    hurt: bool,
    player_id: i32,
    /// 玩家走近的那一刻；没有玩家时是 -1。
    ///
    /// The tick the player walks up; -1 without a player.
    player_tick: i32,
    player_far: [f64; 3],
    player_near: [f64; 3],
    frames: Vec<BatFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<BatState>,
    drops: Vec<[i32; 2]>,
    level_seed: i64,
    level_events: Vec<[i32; 5]>,
    packets: Vec<String>,
}

/// 一刻结束时蝙蝠的样子，字段和官方一侧的输出一一对应。
///
/// The bat at the end of a tick, field for field with the official output.
#[derive(Debug, Deserialize, PartialEq)]
struct BatState {
    motion: [i64; 8],
    seed: i64,
    /// 血量位、`hurtTime`、`deathTime`、`noActionTime`、倒挂、着地、`yBodyRot`、`yHeadRot`、
    /// `zza` 的原始位。
    state: [i32; 9],
    /// 有没有目标，加目标方块。
    ///
    /// Whether there is a target, then the target block.
    target: [i32; 4],
    fall: i64,
}

pub(super) fn player_view(uid: i32, [x, y, z]: [f64; 3]) -> PlayerView {
    PlayerView {
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        uid,
        yaw: 0.0,
        pitch: 0.0,
        position: Vec3::new(x, y, z),
        delta: Vec3::default(),
        eye_height: 1.62,
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
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
    }
}

impl BatCase {
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
            players: if self.player_tick > 0 {
                vec![player_view(self.player_id, self.player_far)]
            } else {
                Vec::new()
            },
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            natural_spawn: None,
            biome: None,
        }
    }

    /// 和官方一侧同样的顺序：先蝙蝠，再（受伤场景）不带 AI 的牛。
    ///
    /// Same order as the official side: the bat, then the AI-less cow of hurt scenes.
    fn spawn(&self, world: &mut World<'_>) -> (i32, Option<i32>) {
        let [x, y, z] = self.start;
        let bat = world.spawn_entity(Entity::by_mob_type(MobType::Bat, Vec3::new(x, y, z)));
        let cow = self.hurt.then(|| {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(3.5, 61.0, 0.5));
            cow.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(cow)
        });
        (bat, cow)
    }
}

fn bat_state(entity: &Entity) -> BatState {
    let mob = entity.kind.mob().expect("a bat is a mob");
    let (p, v) = (entity.position, entity.delta);
    BatState {
        motion: [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(mob.x_rot.to_bits() as i32),
        ],
        seed: entity.random.state(),
        state: [
            mob.health.to_bits() as i32,
            mob.hurt_time,
            mob.death_time,
            mob.no_action_time,
            i32::from(mob.bat.is_resting()),
            i32::from(entity.on_ground),
            mob.y_body_rot.to_bits() as i32,
            mob.y_head_rot.to_bits() as i32,
            mob.zza.to_bits() as i32,
        ],
        target: mob
            .bat
            .target
            .map_or([0; 4], |target| [1, target.x, target.y, target.z]),
        fall: entity.fall_distance.to_bits() as i64,
    }
}

impl Report {
    pub(super) fn replay_bat(&mut self, case: &BatCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("bat");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (bat_id, cow_id) = case.spawn(&mut World::new(&mut level, &mut ticks));
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let mut was_gone = false;
        let mut dropped = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut level_events = Vec::new();
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                // 改坏确认：玩家晚一刻走近，蝙蝠就晚一刻醒。
                // Break check: the player arrives one tick late, and so does the wake-up.
                if tick as i32 == case.player_tick + i32::from(broken) {
                    level.players[0] = player_view(case.player_id, case.player_near);
                }
                let attack = HURT_TICKS
                    .iter()
                    .find(|(at, _)| *at == tick)
                    .and_then(|&(_, amount)| cow_id.map(|cow| (cow_attack(&level, cow), amount)));
                let mut world = World::new(&mut level, &mut ticks);
                if let Some((source, amount)) = attack {
                    world.hurt_mob(bat_id, source, amount);
                }
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
            let public = level.entities.public_at(bat_id).cloned();
            let mut drops = Vec::new();
            for spawned in level.entities.take_spawn_snapshots() {
                match &spawned.kind {
                    KindPublic::Item { stack, .. } => drops.push([stack.item, stack.count]),
                    KindPublic::Mob(_) if spawned.uid == bat_id => {
                        let snapshot = Snapshot::of(&spawned);
                        let tracked = tracking.tracked_for_oracle(&snapshot);
                        packets.extend(tracked.pairing_packets(&snapshot));
                    }
                    _ => {}
                }
            }
            dropped += drops.len();
            match &public {
                Some(public) if tick != 0 => {
                    let snapshot = Snapshot::of(public);
                    let hurt_marked = take_hurt_marked(&mut level, &mut ticks, bat_id);
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        hurt_marked,
                    ));
                }
                None if !was_gone => packets.push(packet_body(&RemoveEntities {
                    entity_ids: &[bat_id],
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
            let ours = level.entities.get(bat_id).map(|entity| bat_state(&entity));
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} bat: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            if drops != expected.drops {
                self.mismatch(format!(
                    "{label} drops: official {:?}, bamboo {drops:?}",
                    expected.drops
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
        println!(
            "{}: {} ticks, {dropped} drops",
            case.name,
            case.frames.len()
        );
    }
}
