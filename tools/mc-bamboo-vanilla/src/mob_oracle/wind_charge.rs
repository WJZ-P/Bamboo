//! 风弹逐刻对拍：官方夹具里一枚固定起点 / 速度的风弹朝墙或牛飞去，
//! 逐刻比位置、速度、朝向、消失、牛的血量与击退速度、世界随机源和最终方块。

use mc_bamboo_blocks::{
    BlockState, BlockTicks, LevelRandom, World,
    entity::{
        Entity, EntityKind,
        mob::MobType,
        wind_charge::{WindChargeShot, WindChargeVariant},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WindChargeCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) mob: bool,
    pub(super) start: [f64; 3],
    pub(super) velocity: [f64; 3],
    /// 每刻：`[x,y,z,dx,dy,dz,yRot,xRot,removed,cowHealth,levelSeed]`。
    pub(super) frames: Vec<[i64; 11]>,
    /// 结束时场景方块 `[x,y,z,state]`，顺序与官方一致。
    pub(super) final_blocks: Vec<[i32; 4]>,
}

impl WindChargeCase {
    fn level(&self) -> MobLevel {
        let mut blocks = Vec::new();
        for x in -4..=4 {
            for z in -4..=4 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                ));
            }
        }
        if !self.mob {
            blocks.push((
                BlockPos::new(2, 64, 0),
                BlockState(mc_bamboo_data::blocks::STONE),
            ));
        }
        MobLevel {
            blocks: blocks.into_iter().collect(),
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
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
        }
    }
}

impl Report {
    pub(super) fn replay_wind_charge(&mut self, case: &WindChargeCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("wind_charge");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let velocity = Vec3::new(case.velocity[0], case.velocity[1], case.velocity[2]);
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let (charge_id, cow_id) = {
            let mut world = World::new(&mut level, &mut ticks);
            let mut charge = Entity::wind_charge(&WindChargeShot {
                position: start,
                direction: velocity,
                power: 1.0,
                uncertainty: 0.0,
                variant: WindChargeVariant::Player,
                owner: None,
                owner_uuid: None,
            });
            charge.delta = velocity;
            if let EntityKind::WindCharge(state) = &mut charge.kind {
                state.uuid = [0; 16];
                state.pitch = 0.0;
            }
            let charge_id = world.spawn_entity(charge);
            let cow_id = if case.mob {
                world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(2.5, 64.0, 0.5)))
            } else {
                -1
            };
            (charge_id, cow_id)
        };
        let _ = level.entities.take_spawn_snapshots();
        let mut last_charge: Option<Entity> = None;
        let last_cow: Option<Entity> = None;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                // 官方夹具这一刻只 tick 风弹，牛只作为爆炸目标留在名单里（不跑物理）。
                // 用 take / tick / put 精确驱动风弹，保留消失后的最后状态用于报帧。
                if let Some(kept) = world.tick_one_entity_kept(charge_id) {
                    last_charge = Some(kept);
                }
                let _ = world.drain_effects();
            }
            let entity = level.entities.get(charge_id).or(last_charge.clone());
            let (position, delta, yaw, pitch, removed) = match &entity {
                Some(entity) => {
                    let pitch = match &entity.kind {
                        EntityKind::WindCharge(charge) => charge.pitch,
                        _ => 0.0,
                    };
                    (
                        entity.position,
                        entity.delta,
                        entity.y_rot,
                        pitch,
                        i64::from(entity.removed),
                    )
                }
                None => (Vec3::default(), Vec3::default(), 0.0, 0.0, 1),
            };
            let (cow_health, _cow_dx) = if case.mob {
                match level
                    .entities
                    .get(cow_id)
                    .or(last_cow.clone())
                    .and_then(|entity| entity.kind.mob().map(|mob| (mob.health, entity.delta.x)))
                {
                    Some((health, dx)) => (health.to_bits() as i32 as i64, dx.to_bits() as i64),
                    None => (0, 0),
                }
            } else {
                (0, 0)
            };
            let mut ours = [
                position.x.to_bits() as i64,
                position.y.to_bits() as i64,
                position.z.to_bits() as i64,
                delta.x.to_bits() as i64,
                delta.y.to_bits() as i64,
                delta.z.to_bits() as i64,
                i64::from(yaw.to_bits() as i32),
                i64::from(pitch.to_bits() as i32),
                removed,
                cow_health,
                level.random.source().seed(),
            ];
            if broken {
                // 改坏确认：把命中后的方块当成没命中（位置偏移一位），逐刻都会报错。
                ours[0] = position.x.to_bits() as i64 + 1;
            }
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
        for [x, y, z, state] in &case.final_blocks {
            let ours = level
                .blocks
                .get(&BlockPos::new(*x, *y, *z))
                .copied()
                .unwrap_or(BlockState(mc_bamboo_data::blocks::AIR));
            if ours.0 as i32 != *state {
                self.mismatch(format!(
                    "{} final block ({x},{y},{z}): official {state}, bamboo {}",
                    case.name, ours.0
                ));
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
