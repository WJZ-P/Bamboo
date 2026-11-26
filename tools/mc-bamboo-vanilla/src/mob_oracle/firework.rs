//! 烟花火箭逐刻对拍：官方夹具里一枚烟花（竖直无星 / 带星炸牛 / 弩斜射撞墙），
//! 逐刻比位置、速度、朝向、是否消失、`life` / `lifetime`、消失那一刻的实体事件 17、
//! 牛的血量与击退速度、世界随机源和最终方块。

use mc_bamboo_blocks::{
    BlockState, BlockTicks, LevelRandom, World,
    entity::{
        Entity, EntityKind,
        firework_rocket::{FireworkShoot, FireworkShot},
        mob::MobType,
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, value::FireworkExplosion},
    item::ItemStack,
};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FireworkCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) star: bool,
    pub(super) crossbow: bool,
    pub(super) start: [f64; 3],
    pub(super) direction: [f64; 3],
    /// 每刻：`[x,y,z,dx,dy,dz,yRot,xRot,removed,life,lifetime,event17,cowHealth,cowDx,cowDy,cowDz,levelSeed]`。
    pub(super) frames: Vec<[i64; 17]>,
    /// 结束时场景方块 `[x,y,z,state]`，顺序与官方一致。
    pub(super) final_blocks: Vec<[i32; 4]>,
}

impl FireworkCase {
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
        if self.star {
            blocks.push((
                BlockPos::new(0, 66, 0),
                BlockState(mc_bamboo_data::blocks::STONE),
            ));
        }
        if self.crossbow {
            blocks.push((
                BlockPos::new(2, 64, 0),
                BlockState(mc_bamboo_data::blocks::STONE),
            ));
        }
        MobLevel {
            raid: None,
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
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: mc_bamboo_blocks::entity::shoulder::ShoulderState::empty(),
        }
    }

    /// 带星场景的物品堆：`SMALL_BALL` 形状、白色、飞行时长 1。
    fn stack(&self) -> ItemStack {
        if !self.star && !self.crossbow {
            return ItemStack::new(items::FIREWORK_ROCKET, 1);
        }
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::FIREWORKS,
            PatchEntry::Set(ComponentValue::Fireworks {
                flight_duration: 1,
                explosions: vec![FireworkExplosion {
                    shape: 0,
                    colors: vec![0xFFFFFF],
                    fade_colors: Vec::new(),
                    has_trail: false,
                    has_twinkle: false,
                }],
            }),
        );
        let (added, removed) = patch.to_opaque();
        ItemStack {
            added,
            removed,
            ..ItemStack::new(items::FIREWORK_ROCKET, 1)
        }
    }
}

impl Report {
    pub(super) fn replay_firework(&mut self, case: &FireworkCase) {
        self.cases += 1;
        mc_bamboo_blocks::entity::firework_rocket::set_oracle_break(
            self.break_test.as_deref().unwrap_or(""),
        );
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let direction = Vec3::new(case.direction[0], case.direction[1], case.direction[2]);
        let shot = FireworkShot {
            position: start,
            owner: None,
            owner_uuid: None,
            item: case.stack(),
            shot_at_angle: case.crossbow,
            shoot: case.crossbow.then_some(FireworkShoot {
                direction,
                power: 1.6,
                uncertainty: 1.0,
            }),
        };
        let (firework_id, cow_id) = {
            let mut world = World::new(&mut level, &mut ticks);
            let firework_id = world.spawn_firework(&shot);
            let cow_id = if case.star {
                world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(1.5, 64.0, 0.5)))
            } else {
                -1
            };
            (firework_id, cow_id)
        };
        let _ = level.entities.take_spawn_snapshots();
        let mut last_firework: Option<Entity> = None;
        let last_cow: Option<Entity> = None;
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut event17 = false;
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                // 官方夹具这一刻只 tick 烟花，牛只作为爆炸目标留在名单里（不跑物理）。
                if let Some(kept) = world.tick_one_entity_kept(firework_id) {
                    event17 = matches!(&kept.kind, EntityKind::FireworkRocket(fw) if fw.exploded());
                    last_firework = Some(kept);
                }
                let _ = world.drain_effects();
            }
            let entity = level.entities.get(firework_id).or(last_firework.clone());
            let (position, delta, yaw, pitch, removed, life, lifetime) = match &entity {
                Some(entity) => {
                    let (pitch, life, lifetime) = match &entity.kind {
                        EntityKind::FireworkRocket(firework) => {
                            (firework.pitch, firework.life, firework.lifetime)
                        }
                        _ => (0.0, 0, 0),
                    };
                    (
                        entity.position,
                        entity.delta,
                        entity.y_rot,
                        pitch,
                        i64::from(entity.removed),
                        i64::from(life),
                        i64::from(lifetime),
                    )
                }
                None => (Vec3::default(), Vec3::default(), 0.0, 0.0, 1, 0, 0),
            };
            let (cow_health, cow_delta) = if case.star {
                match level
                    .entities
                    .get(cow_id)
                    .or(last_cow.clone())
                    .and_then(|entity| entity.kind.mob().map(|mob| (mob.health, entity.delta)))
                {
                    Some((health, delta)) => (health.to_bits() as i32 as i64, delta),
                    None => (0, Vec3::default()),
                }
            } else {
                (0, Vec3::default())
            };
            let ours = [
                position.x.to_bits() as i64,
                position.y.to_bits() as i64,
                position.z.to_bits() as i64,
                delta.x.to_bits() as i64,
                delta.y.to_bits() as i64,
                delta.z.to_bits() as i64,
                i64::from(yaw.to_bits() as i32),
                i64::from(pitch.to_bits() as i32),
                removed,
                life,
                lifetime,
                i64::from(event17),
                cow_health,
                cow_delta.x.to_bits() as i64,
                cow_delta.y.to_bits() as i64,
                cow_delta.z.to_bits() as i64,
                level.random.source().seed(),
            ];
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

    /// 烟花火箭存档往返：通过 `archive::entity::{read_entity, write_entity}` 读官方
    /// SNBT 再写回，逐键比对官方读回再写出的结果。
    pub(super) fn replay_firework_save(&mut self, case: &FireworkSaveCase) {
        self.cases += 1;
        let label = format!("firework save filled {}", case.filled);
        let mut official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        if let mc_bamboo_nbt::Tag::Compound(fields) = &mut official {
            fields.push((
                "id".to_owned(),
                mc_bamboo_nbt::Tag::String("minecraft:firework_rocket".to_owned()),
            ));
        }
        let row = match mc_bamboo_blocks::archive::entity::read_entity(&official, 1) {
            Ok(Some(row)) => row,
            Ok(None) => {
                self.mismatch(format!("{label}: read_entity returned None"));
                return;
            }
            Err(error) => {
                self.mismatch(format!("{label}: load failed: {error:?}"));
                return;
            }
        };
        let ours = match mc_bamboo_blocks::archive::entity::write_entity(&row) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
        println!("{label}: save round trip");
    }
}

/// 烟花火箭存档往返一题。
///
/// One firework rocket save round-trip case.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FireworkSaveCase {
    pub(super) filled: bool,
    pub(super) nbt: String,
    pub(super) reloaded: String,
}

const SAVE_KEYS: &[&str] = &[
    "Pos",
    "Motion",
    "Rotation",
    "UUID",
    "Owner",
    "LeftOwner",
    "HasBeenShot",
    "Life",
    "LifeTime",
    "FireworksItem",
    "ShotAtAngle",
];
