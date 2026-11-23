//! 鱿鱼 / 发光鱿鱼对拍：逐刻比位置、速度、随机源、氧气、身体和触手的状态、移动向量、
//! 掉落、世界随机源和出站包（配对、同步、实体事件、音效、墨汁粒子）。
//!
//! Squid and glow squid comparison: each tick compares position, velocity, random, air,
//! body and tentacle state, the movement vector, drops, the level random and the outgoing
//! packets (pairing, sync, entity events, sounds, ink particles).

use mc_bamboo_blocks::entity::{
    KindPublic,
    mob::{DamageKind, MobType},
};
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::*;

/// 官方一侧第几刻让牛打鱿鱼一下。
///
/// The tick on which the official side has the cow hit the squid.
const HURT_TICK: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SquidCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    glow: bool,
    hurt: bool,
    baby: bool,
    frames: Vec<SquidFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SquidFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<SquidState>,
    drops: Vec<[i32; 2]>,
    level_seed: i64,
    packets: Vec<String>,
}

/// 一刻结束时鱿鱼的样子，字段和官方一侧的输出一一对应。
///
/// The squid at the end of a tick, field for field with the official output.
#[derive(Debug, Deserialize, PartialEq)]
struct SquidState {
    motion: [i64; 8],
    seed: i64,
    /// 血量位、氧气、`hurtTime`、`deathTime`、`noActionTime`、年龄、变暗计时、着地、在水里。
    state: [i32; 9],
    /// `xBodyRot`、`zBodyRot`、`tentacleMovement`、`tentacleAngle`、`tentacleSpeed`、
    /// `rotateSpeed`、`yBodyRot`、`yHeadRot` 的原始位。
    body: [i32; 8],
    movement: [i64; 3],
}

impl SquidCase {
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
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            natural_spawn: None,
        }
    }

    /// 和官方一侧同样的顺序：先鱿鱼，再（受伤场景）不带 AI 的牛。
    ///
    /// Same order as the official side: the squid, then the AI-less cow of hurt scenes.
    fn spawn(&self, world: &mut World<'_>) -> (i32, Option<i32>) {
        let [x, y, z] = self.start;
        let kind = if self.glow {
            MobType::GlowSquid
        } else {
            MobType::Squid
        };
        let mut entity = Entity::by_mob_type(kind, Vec3::new(x, y, z));
        if self.baby {
            let (width, height, eye) = kind.dimensions(true);
            entity.dimensions = (width, height);
            if let Some(mob) = entity.kind.mob_mut() {
                mob.age = -24000;
                mob.eye_height = eye;
            }
        }
        let squid = world.spawn_entity(entity);
        let cow = self.hurt.then(|| {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(3.5, 61.0, 0.5));
            cow.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(cow)
        });
        (squid, cow)
    }
}

/// `damageSources().mobAttack(cow)`：和 `Mob.doHurtTarget` 造的来源一样。
///
/// `damageSources().mobAttack(cow)`, the same source `Mob.doHurtTarget` builds.
pub(super) fn cow_attack(level: &MobLevel, cow: i32) -> DamageSource {
    let entity = level.entities.get(cow).expect("the cow is alive");
    let mut source = DamageSource::environment(DamageKind::MobAttack).with_non_player_cause(true);
    source.attacker = Some(cow);
    source.direct = Some(cow);
    source.attacker_uuid = entity.kind.mob().map(|mob| mob.uuid);
    source.position = Some(entity.position);
    source
}

pub(super) fn take_hurt_marked(level: &mut MobLevel, ticks: &mut BlockTicks, id: i32) -> bool {
    let mut marked = false;
    World::new(level, ticks).with_entity_mut(id, &mut |entity| {
        if let Some(mob) = entity.kind.mob_mut() {
            marked = std::mem::take(&mut mob.hurt_marked);
        }
    });
    marked
}

fn squid_state(entity: &Entity) -> SquidState {
    let mob = entity.kind.mob().expect("a squid is a mob");
    let (p, v) = (entity.position, entity.delta);
    let squid = &mob.squid;
    SquidState {
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
            mob.air_supply,
            mob.hurt_time,
            mob.death_time,
            mob.no_action_time,
            mob.age,
            squid.dark_ticks,
            i32::from(entity.on_ground),
            i32::from(entity.in_water),
        ],
        body: [
            squid.x_body_rot,
            squid.z_body_rot,
            squid.tentacle_movement,
            squid.tentacle_angle,
            squid.tentacle_speed,
            squid.rotate_speed,
            mob.y_body_rot,
            mob.y_head_rot,
        ]
        .map(|value| value.to_bits() as i32),
        movement: [squid.movement.x, squid.movement.y, squid.movement.z]
            .map(|value| value.to_bits() as i64),
    }
}

impl Report {
    pub(super) fn replay_squid(&mut self, case: &SquidCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("squid");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (squid_id, cow_id) = case.spawn(&mut World::new(&mut level, &mut ticks));
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let mut was_gone = false;
        let mut dropped = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let attack = (tick == HURT_TICK)
                    .then(|| cow_id.map(|cow| cow_attack(&level, cow)))
                    .flatten();
                let mut world = World::new(&mut level, &mut ticks);
                if let Some(source) = attack {
                    world.hurt_mob(squid_id, source, if broken { 2.0 } else { 1.0 });
                }
                world.tick_entities();
            }
            let events = level.entities.take_mob_events();
            let mut packets = event_packets(&events, || sound_source.next_long());
            let public = level.entities.public_at(squid_id).cloned();
            let mut drops = Vec::new();
            for spawned in level.entities.take_spawn_snapshots() {
                match &spawned.kind {
                    KindPublic::Item { stack, .. } => drops.push([stack.item, stack.count]),
                    KindPublic::Mob(_) if spawned.uid == squid_id => {
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
                    // `ServerEntity.sendChanges`：`hurtMarked` 为真时多发一个速度包并清掉标志。
                    // `ServerEntity.sendChanges` sends one more velocity packet for a set
                    // `hurtMarked` and clears the flag.
                    let hurt_marked = take_hurt_marked(&mut level, &mut ticks, squid_id);
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        hurt_marked,
                    ));
                }
                None if !was_gone => packets.push(packet_body(&RemoveEntities {
                    entity_ids: &[squid_id],
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
                .get(squid_id)
                .map(|entity| squid_state(&entity));
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} squid: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            if drops != expected.drops {
                self.mismatch(format!(
                    "{label} drops: official {:?}, bamboo {drops:?}",
                    expected.drops
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
