//! 扔出的鸡蛋对拍：逐刻比蛋、孵出的小鸡、被打的牛和蛋的出站包；另比玩家扔蛋那一刻。
//!
//! Thrown egg comparison: each tick compares the egg, hatched chicks, the struck cow
//! and the egg's packets; a separate case compares a player's throw.

use std::collections::BTreeSet;

use mc_bamboo_blocks::entity::{
    FIRST_ENTITY_ID as FIRST_ID, JavaRandom, KindPublic, mob::MobType, thrown_egg::EggShot,
};
use mc_bamboo_protocol::{frame::packet_body, item::ItemStack, packet::play::RemoveEntities};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EggCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    dir: [f64; 3],
    item: i32,
    forced: i64,
    cow: bool,
    fire_ticks: i32,
    /// 蛋的玩家主人的实体号；没有主人时是 -1。有主人的场景里牛开着 AI。
    ///
    /// The entity id of the egg's player owner, -1 without one. With an owner the
    /// cow keeps its AI.
    #[serde(default = "no_owner")]
    owner: i32,
    frames: Vec<EggFrame>,
}

fn no_owner() -> i32 {
    -1
}

/// 和 `MobOracle.EGG_OWNER` 一样的主人位置。
///
/// The owner's position, as in `MobOracle.EGG_OWNER`.
const EGG_OWNER: Vec3 = Vec3::new(-6.5, 64.0, 0.5);

/// 和官方一侧一样，主人的 UUID 是 (1, 2)。
///
/// The owner's UUID is (1, 2), like on the official side.
const EGG_OWNER_UUID: u128 = (1 << 64) | 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EggFrame {
    eggs: Vec<EggState>,
    chicks: Vec<ChickState>,
    cow: Option<[i64; 8]>,
    /// 生命、`hurtTime`、随机源、`lastHurtByPlayer` 的实体号（没有是 -1）和它的记忆刻数。
    ///
    /// Health, `hurtTime`, random, the `lastHurtByPlayer` id (-1 if none) and its memory time.
    cow_state: Option<[i64; 5]>,
    packets: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct EggState {
    id: i32,
    motion: [i64; 8],
    seed: i64,
    on_fire: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ChickState {
    id: i32,
    motion: [i64; 8],
    seed: i64,
    uuid: [i64; 2],
    age: i32,
    egg: i32,
    head: i32,
    variant: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EggThrowCase {
    name: String,
    yaw: i32,
    pitch: i32,
    moving: bool,
    creative: bool,
    motion: [i64; 8],
    seed: i64,
    item: [i32; 2],
    left: i32,
    level_seed: i64,
    item_used: Vec<(i32, i32)>,
}

/// 发射器扔蛋那一刻：蛋的运动、随机源、蛋里的物品、剩下的物品数和世界随机源。
///
/// A dispensed egg right after the shot: its motion, random, carried item, the stack
/// left behind and the level random.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EggDispenseCase {
    name: String,
    up: bool,
    motion: [i64; 8],
    seed: i64,
    item: [i32; 2],
    left: i32,
    level_seed: i64,
}

impl EggCase {
    fn level(&self) -> MobLevel {
        let mut level = Self::empty_level(self.first_id);
        level.blocks = self
            .blocks
            .iter()
            .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
            .collect();
        if self.owner >= 0 {
            level.players.push(PlayerView {
                uuid: Some(EGG_OWNER_UUID.to_be_bytes()),
                uid: self.owner,
                position: EGG_OWNER,
                eye_height: 1.62,
                alive: true,
                spectator: false,
                creative: false,
                sneaking: false,
                invisible: false,
                armor_cover: 0.0,
                main_hand: 0,
                off_hand: 0,
                combat: Default::default(),
            });
        }
        level
    }

    /// 没有方块、世界随机源种子为 1 的普通难度假世界。
    ///
    /// A blockless normal-difficulty fake level whose level random is seeded with 1.
    fn empty_level(first_id: i32) -> MobLevel {
        MobLevel {
            blocks: HashMap::new(),
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
            entities: Entities::with_next_id(first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            natural_spawn: None,
        }
    }

    /// 和官方一侧同样的顺序：先蛋（种子、UUID、散布），再按需换种子，再牛。
    ///
    /// Same order as the official side: the egg (seed, UUID, spread), the forced
    /// seed if any, then the cow.
    fn spawn(&self, world: &mut World<'_>) -> i32 {
        let [x, y, z] = self.start;
        let [dx, dy, dz] = self.dir;
        let shot = EggShot {
            position: Vec3::new(x, y, z),
            direction: Vec3::new(dx, dy, dz),
            power: 1.5,
            uncertainty: 1.0,
            source_movement: None,
            owner: (self.owner >= 0).then_some(self.owner),
            owner_uuid: (self.owner >= 0).then_some(EGG_OWNER_UUID.to_be_bytes()),
            item: ItemStack::new(self.item, 16),
        };
        let mut egg = world.prepare_entity(Entity::thrown_egg(&shot));
        egg.shoot_egg(&shot);
        egg.remaining_fire_ticks = self.fire_ticks;
        if self.name != "egg_floor" {
            egg.random = JavaRandom::new(self.forced);
        }
        let id = world.add_prepared_entity(egg);
        if self.cow {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(4.5, 64.0, 0.5));
            let no_ai = self.owner < 0;
            cow.with_mob(|_, mob| mob.no_ai = no_ai);
            world.spawn_entity(cow);
        }
        id
    }
}

impl Report {
    pub(super) fn replay_egg(&mut self, case: &EggCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let egg_id = case.spawn(&mut World::new(&mut level, &mut ticks));
        let cow_id = case.cow.then_some(egg_id + 1);
        let mut tracking = EntityTracking::default();
        let mut previous = BTreeSet::new();
        let mut chick_ids = Vec::new();
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                World::new(&mut level, &mut ticks).tick_entities();
            }
            let events: Vec<_> = level
                .entities
                .take_mob_events()
                .into_iter()
                .filter(|(uid, _)| *uid == egg_id)
                .collect();
            let mut packets = event_packets(&events, || 0);
            let active: BTreeSet<_> = level
                .entities
                .ids()
                .into_iter()
                .filter(|&id| {
                    level
                        .entities
                        .public_at(id)
                        .is_some_and(|public| matches!(public.kind, KindPublic::Egg(_)))
                })
                .collect();
            for &uid in previous.difference(&active) {
                packets.push(packet_body(&RemoveEntities { entity_ids: &[uid] }));
            }
            for public in level.entities.take_spawn_snapshots() {
                match public.kind {
                    KindPublic::Egg(_) => {
                        let snapshot = Snapshot::of(&public);
                        packets.extend(
                            tracking
                                .tracked_for_oracle(&snapshot)
                                .pairing_packets(&snapshot),
                        );
                    }
                    KindPublic::Mob(ref mob) if mob.mob_type == MobType::Chicken => {
                        chick_ids.push(public.uid);
                    }
                    _ => {}
                }
            }
            if tick != 0 {
                for &uid in &active {
                    let snapshot = Snapshot::of(level.entities.public_at(uid).unwrap());
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        snapshot.sync_velocity,
                    ));
                }
            }
            let label = format!("{} tick {tick}", case.name);
            let eggs: Vec<_> = active
                .iter()
                .map(|&id| {
                    let entity = level.entities.get(id).unwrap();
                    let EntityKind::Egg(egg) = &entity.kind else {
                        unreachable!()
                    };
                    EggState {
                        id,
                        motion: projectile_motion(&entity, egg.x_rot),
                        seed: entity.random.state(),
                        on_fire: entity.remaining_fire_ticks > 0,
                    }
                })
                .collect();
            if eggs != expected.eggs {
                self.mismatch(format!(
                    "{label} eggs: official {:?}, bamboo {eggs:?}",
                    expected.eggs
                ));
            }
            let chicks: Vec<_> = chick_ids
                .iter()
                .map(|&id| chick_state(&level.entities.get(id).unwrap(), id))
                .collect();
            if chicks != expected.chicks {
                self.mismatch(format!(
                    "{label} chicks: official {:?}, bamboo {chicks:?}",
                    expected.chicks
                ));
            }
            if let (Some(theirs), Some(id)) = (expected.cow, cow_id) {
                let cow = level.entities.get(id).unwrap();
                let mob = cow.kind.mob().unwrap();
                let ours = projectile_motion(&cow, mob.x_rot);
                if ours != theirs {
                    self.mismatch(format!(
                        "{label} cow motion: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
                let state = [
                    i64::from(mob.health.to_bits() as i32),
                    i64::from(mob.hurt_time),
                    cow.random.state(),
                    i64::from(mob.last_hurt_by_player.unwrap_or(-1)),
                    i64::from(mob.last_hurt_by_player_memory_time),
                ];
                if Some(state) != expected.cow_state {
                    self.mismatch(format!(
                        "{label} cow health/hurtTime/seed/hurtByPlayer/memory: official {:?}, bamboo {state:?}",
                        expected.cow_state
                    ));
                }
            }
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                &packets,
                &expected.packets,
            );
            previous = active;
            self.steps += 1;
        }
        println!(
            "{}: {} ticks, {} chicks",
            case.name,
            case.frames.len(),
            chick_ids.len()
        );
    }

    pub(super) fn replay_egg_throw(&mut self, case: &EggThrowCase) {
        self.cases += 1;
        let movement = case.moving.then(|| Vec3::new(0.125, -0.25, 0.0625));
        let Some(ours) = mc_bamboo_game::player_egg::replay_throw(
            f32::from_bits(case.yaw as u32),
            f32::from_bits(case.pitch as u32),
            movement,
            case.creative,
        ) else {
            self.mismatch(format!("{}: bamboo threw no egg", case.name));
            return;
        };
        let theirs = mc_bamboo_game::player_egg::ReplayEggThrow {
            motion: case.motion,
            seed: case.seed,
            item: case.item,
            left: case.left,
            level_seed: case.level_seed,
            item_used: case.item_used.clone(),
        };
        if ours != theirs {
            self.mismatch(format!(
                "{}: official {theirs:?}, bamboo {ours:?}",
                case.name
            ));
        }
        self.steps += 1;
        println!("{}: egg thrown, {} left", case.name, ours.left);
    }

    pub(super) fn replay_egg_dispense(&mut self, case: &EggDispenseCase) {
        self.cases += 1;
        self.steps += 1;
        let mut level = EggCase::empty_level(FIRST_ID);
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let facing = if case.up {
            mc_bamboo_core::Direction::Up
        } else {
            mc_bamboo_core::Direction::East
        };
        let left = mc_bamboo_blocks::dispense::dispense_arrow_at(
            &mut World::new(&mut level, &mut ticks),
            BlockPos::new(0, 64, 0),
            facing,
            ItemStack::new(mc_bamboo_data::items::BLUE_EGG, 3),
        );
        let eggs: Vec<_> = level
            .entities
            .iter()
            .filter_map(|entity| match &entity.kind {
                EntityKind::Egg(egg) => Some((
                    projectile_motion(&entity, egg.x_rot),
                    entity.random.state(),
                    [egg.item.item, egg.item.count],
                )),
                _ => None,
            })
            .collect();
        let ours = (eggs, left.count, level.random.source().seed());
        let theirs = (
            vec![(case.motion, case.seed, case.item)],
            case.left,
            case.level_seed,
        );
        if ours != theirs {
            self.mismatch(format!(
                "{}: official {theirs:?}, bamboo {ours:?}",
                case.name
            ));
        }
        println!("{}: egg dispensed, {} left", case.name, left.count);
    }
}

fn chick_state(entity: &Entity, id: i32) -> ChickState {
    let mob = entity.kind.mob().unwrap();
    let uuid = u128::from_be_bytes(mob.uuid);
    ChickState {
        id,
        motion: projectile_motion(entity, mob.x_rot),
        seed: entity.random.state(),
        uuid: [(uuid >> 64) as i64, uuid as i64],
        age: mob.age,
        egg: mob.egg_time,
        head: mob.y_head_rot.to_bits() as i32,
        variant: mob.variant.name().to_owned(),
    }
}

fn projectile_motion(entity: &Entity, pitch: f32) -> [i64; 8] {
    let (p, v) = (entity.position, entity.delta);
    [
        p.x.to_bits() as i64,
        p.y.to_bits() as i64,
        p.z.to_bits() as i64,
        v.x.to_bits() as i64,
        v.y.to_bits() as i64,
        v.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
        i64::from(pitch.to_bits() as i32),
    ]
}
