//! 鱼对拍：逐刻比位置、速度、朝向、随机源、氧气、花纹 / 膨胀尺寸、掉落、世界随机源和
//! 出站包（配对、同步、实体事件、音效、河豚的蜇刺）。
//!
//! Fish comparison: each tick compares position, velocity, rotation, random, air, the
//! variant / puff state and size, drops, the level random and the outgoing packets
//! (pairing, sync, entity events, sounds, the pufferfish sting).

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_protocol::{frame::packet_body, packet::play::RemoveEntities};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FishCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    kind: String,
    puff_state: i32,
    /// 河豚蜇刺场景里的牛和它挨打的刻。
    cow: bool,
    frames: Vec<FishFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FishFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<FishState>,
    drops: Vec<[i32; 2]>,
    level_seed: i64,
    packets: Vec<String>,
}

/// 一刻结束时鱼的样子，字段和官方一侧的输出一一对应。
///
/// The fish at the end of a tick, field for field with the official output.
#[derive(Debug, Deserialize, PartialEq)]
struct FishState {
    motion: [i64; 8],
    seed: i64,
    /// 血量位、氧气、`hurtTime`、`deathTime`、`noActionTime`、着地、在水里、花纹、膨胀。
    state: [i32; 9],
    /// 宽、高、眼高的原始位。
    dimensions: [i32; 3],
    /// 牛的血量位和中毒剩余刻数（没有牛时为 0）。
    cow: [i32; 2],
}

impl FishCase {
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
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            natural_spawn: None,
        }
    }

    fn mob_type(&self) -> MobType {
        match self.kind.as_str() {
            "cod" => MobType::Cod,
            "salmon" => MobType::Salmon,
            "tropical_fish" => MobType::TropicalFish,
            _ => MobType::Pufferfish,
        }
    }

    /// 和官方一侧同样的顺序：先鱼，再（河豚场景）不带 AI 的牛。
    ///
    /// Same order as the official side: the fish, then the AI-less cow of puffer scenes.
    fn spawn(&self, world: &mut World<'_>) -> (i32, Option<i32>) {
        let [x, y, z] = self.start;
        let kind = self.mob_type();
        let mut entity = Entity::by_mob_type(kind, Vec3::new(x, y, z));
        // 官方一侧用 `EntityTypes.create(COMMAND)`，不跑 `finalizeSpawn`：鲑鱼保持
        // medium（构造函数的缺省），热带鱼保持 KOB 白/白，河豚不鼓。
        // The official side creates with `EntityTypes.create(COMMAND)` and no
        // `finalizeSpawn`: salmon stay medium, tropical fish KOB white/white and the
        // pufferfish stays deflated.
        entity.with_mob(|me, mob| {
            mob.fish.puff_state = self.puff_state;
            mc_bamboo_blocks::entity::mob::fish::refresh_dimensions(me, mob);
        });
        let fish = world.spawn_entity(entity);
        // 牛的位置和官方一侧的 `cow.setPos(0.9, 61.0, 0.5)` 一致。
        // The cow stands where the official side's `cow.setPos(0.9, 61.0, 0.5)` puts it.
        let cow = self.cow.then(|| {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(0.9, 61.0, 0.5));
            cow.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(cow)
        });
        (fish, cow)
    }
}

fn fish_state(entity: &Entity, cow: Option<&Entity>) -> FishState {
    let mob = entity.kind.mob().expect("a fish is a mob");
    let (p, v) = (entity.position, entity.delta);
    let cow = cow.map_or([0, 0], |cow| {
        let other = cow.kind.mob().expect("the cow is a mob");
        [
            other.health.to_bits() as i32,
            other
                .effects
                .get(mc_bamboo_data::registries::mob_effect::POISON)
                .map_or(0, |entry| entry.duration),
        ]
    });
    FishState {
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
            i32::from(entity.on_ground),
            i32::from(entity.in_water),
            mob.fish.variant,
            mob.fish.puff_state,
        ],
        dimensions: [
            entity.dimensions.0.to_bits() as i32,
            entity.dimensions.1.to_bits() as i32,
            mob.eye_height.to_bits() as i32,
        ],
        cow,
    }
}

impl Report {
    pub(super) fn replay_fish(&mut self, case: &FishCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (fish_id, cow_id) = case.spawn(&mut World::new(&mut level, &mut ticks));
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let mut was_gone = false;
        let mut dropped = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let events = level.entities.take_mob_events();
            let mut packets = event_packets(&events, || sound_source.next_long());
            let public = level.entities.public_at(fish_id).cloned();
            let mut drops = Vec::new();
            for spawned in level.entities.take_spawn_snapshots() {
                match &spawned.kind {
                    KindPublic::Item { stack, .. } => drops.push([stack.item, stack.count]),
                    KindPublic::Mob(_) if spawned.uid == fish_id => {
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
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        false,
                    ));
                }
                None if !was_gone => packets.push(packet_body(&RemoveEntities {
                    entity_ids: &[fish_id],
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
            let cow = match (cow_id, &public) {
                (Some(cow), Some(_)) => level.entities.get(cow),
                _ => None,
            };
            let ours = level
                .entities
                .get(fish_id)
                .map(|entity| fish_state(&entity, cow.as_ref()));
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} fish: official {:?}, bamboo {ours:?}",
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
