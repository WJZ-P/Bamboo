//! 凋灵存档往返：官方 `WitherBoss.addAdditionalSaveData` / `readAdditionalSaveData`
//! 的 `Invul` 键（无敌刻数）以及通用键逐键对拍。
//!
//! Wither save parity: round-trip the `Invul` key (invulnerability ticks) of
//! vanilla `WitherBoss.addAdditionalSaveData` / `readAdditionalSaveData` plus
//! the common keys.

use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherSaveCase {
    invul: i32,
    nbt: String,
    reloaded: String,
}

const WITHER_SAVE_KEYS: &[&str] = &["Invul", "Health", "Pos", "Motion", "Rotation", "UUID"];

impl Report {
    pub(super) fn replay_wither_save(&mut self, case: &WitherSaveCase) {
        use mc_bamboo_blocks::entity::{
            Entity,
            mob::{MobType, nbt},
        };
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        let label = format!("wither save Invul {}", case.invul);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
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
        let mut entity = Entity::by_mob_type(MobType::Wither, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        // 四处改坏确认：分别把生产代码算出的 `Invul` / `Health` / `Pos` / `Rotation`
        // 写坏，逐键比较都要报差异。
        // Four break checks: corrupt each of the production values `Invul`,
        // `Health`, `Pos` and `Rotation`; every key comparison must report a mismatch.
        let broken = self.break_test.clone();
        for key in WITHER_SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let mut actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            match (broken.as_deref(), *key) {
                (Some("wither_save_invul"), "Invul") => {
                    actual = Some(mc_bamboo_nbt::Tag::Int(case.invul.wrapping_add(1)));
                }
                (Some("wither_save_health"), "Health") => {
                    actual = Some(mc_bamboo_nbt::Tag::Float(0.0));
                }
                (Some("wither_save_pos"), "Pos") => actual = None,
                (Some("wither_save_rotation"), "Rotation") => actual = None,
                _ => {}
            }
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
        println!("{label}: checked");
    }
}

/// 无敌期逐刻帧：`[invul, health, x, y, z, dx, dy, dz, yRot, xRot,
/// headX0, headX1, headY0, headY1, mobSeed, levelSeed]`（浮点是原始位）。
///
/// Invulnerability-phase frame: floats are raw bits.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherInvulCase {
    name: String,
    first_id: i32,
    invul: i32,
    mob_seed: i64,
    level_seed: i64,
    frames: Vec<[i64; 16]>,
}

impl WitherInvulCase {
    fn level(&self) -> MobLevel {
        let mut blocks = Vec::new();
        for x in -15..=15 {
            for z in -15..=15 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                ));
            }
        }
        MobLevel {
            raid: None,
            blocks: blocks.into_iter().collect(),
            random: LevelRandom::new(self.level_seed),
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
}

impl Report {
    /// 逐刻重放官方无敌期：Invul 递减、每 10 刻回血、头朝向和两种随机源。
    ///
    /// Replay the official invulnerability phase tick by tick.
    pub(super) fn replay_wither_invul(&mut self, case: &WitherInvulCase) {
        use mc_bamboo_blocks::{
            BlockTicks, World,
            entity::{Entity, JavaRandom, mob::MobType},
        };
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let id = {
            let mut world = World::new(&mut level, &mut ticks);
            let entity = Entity::by_mob_type(MobType::Wither, Vec3::new(0.5, 64.0, 0.5));
            let id = world.spawn_entity(entity);
            world.with_entity_mut(id, &mut |entity| {
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(0.5, 64.0, 0.5);
                entity.random = JavaRandom::from_state(case.mob_seed);
                entity.with_mob(|_, mob| {
                    mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
                    mob.health = 100.0;
                    mob.x_rot = 0.0;
                    mob.wither.invulnerable_ticks = case.invul;
                });
            });
            id
        };
        let _ = level.entities.take_spawn_snapshots();
        let broken = self.break_test.as_deref() == Some("wither_invul");
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                let _ = world.drain_effects();
            }
            let Some(entity) = level.entities.get(id) else {
                self.mismatch(format!("{} tick {tick}: wither missing", case.name));
                self.steps += 1;
                continue;
            };
            let mob = entity.kind.mob().expect("wither is a mob");
            let mut ours = [
                i64::from(mob.wither.invulnerable_ticks),
                i64::from(mob.health.to_bits() as i32),
                entity.position.x.to_bits() as i64,
                entity.position.y.to_bits() as i64,
                entity.position.z.to_bits() as i64,
                entity.delta.x.to_bits() as i64,
                entity.delta.y.to_bits() as i64,
                entity.delta.z.to_bits() as i64,
                i64::from(entity.y_rot.to_bits() as i32),
                i64::from(mob.x_rot.to_bits() as i32),
                i64::from(mob.wither.x_rot_heads[0].to_bits() as i32),
                i64::from(mob.wither.x_rot_heads[1].to_bits() as i32),
                i64::from(mob.wither.y_rot_heads[0].to_bits() as i32),
                i64::from(mob.wither.y_rot_heads[1].to_bits() as i32),
                entity.random.state(),
                level.random.source().seed(),
            ];
            if broken {
                ours[0] += 1;
            }
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
