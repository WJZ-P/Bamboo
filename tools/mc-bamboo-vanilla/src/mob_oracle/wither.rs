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

/// 无敌期结束爆炸场景的逐刻帧：16 个凋灵字段 + 事件表。
///
/// A frame of the explosion scene: 16 wither fields plus the event table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherExplodeFrame {
    w: [i64; 16],
    ev: Vec<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherExplodeCase {
    name: String,
    first_id: i32,
    invul: i32,
    mob_seed: i64,
    level_seed: i64,
    frames: Vec<WitherExplodeFrame>,
}

impl WitherExplodeCase {
    fn level(&self) -> MobLevel {
        // 基岩地板是 `#wither_immune`，爆炸不破坏方块，场景只验证触发与事件。
        // The bedrock floor is `#wither_immune`, so the blast destroys nothing.
        let mut blocks = Vec::new();
        for x in -15..=15 {
            for z in -15..=15 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::BEDROCK),
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

/// 把这一刻的 `Effect` 规范化成和 Java 侧一致的事件表（按字典序排序）。
///
/// Normalize this tick's `Effect`s into the same event table as the Java side.
fn normalize_effects(effects: Vec<mc_bamboo_blocks::Effect>) -> Vec<Vec<i64>> {
    use mc_bamboo_blocks::Effect;
    let mut out = Vec::new();
    for effect in effects {
        match effect {
            Effect::Explode { radius, .. } => {
                out.push(vec![1, i64::from(radius.to_bits())]);
            }
            Effect::GlobalLevelEvent { event, pos, data } => {
                out.push(vec![
                    2,
                    i64::from(event),
                    i64::from(pos.x),
                    i64::from(pos.y),
                    i64::from(pos.z),
                    i64::from(data),
                ]);
            }
            Effect::LevelEvent {
                event, pos, data, ..
            } => {
                out.push(vec![
                    3,
                    i64::from(event),
                    i64::from(pos.x),
                    i64::from(pos.y),
                    i64::from(pos.z),
                    i64::from(data),
                ]);
            }
            _ => {}
        }
    }
    out.sort();
    out
}

impl Report {
    /// 逐刻重放无敌期结束：Invul 递减、爆炸、1023 全服事件、两种随机源。
    ///
    /// Replay the end of the invulnerability phase: Invul decay, explosion,
    /// the global 1023 event, and both random sources.
    pub(super) fn replay_wither_explode(&mut self, case: &WitherExplodeCase) {
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
        let broken = self.break_test.as_deref() == Some("wither_explode");
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut ev = Vec::new();
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                ev = normalize_effects(world.drain_effects());
            }
            let Some(entity) = level.entities.get(id) else {
                self.mismatch(format!("{} tick {tick}: wither missing", case.name));
                self.steps += 1;
                continue;
            };
            if broken {
                // 改坏确认：把爆炸事件的半径位清零，只有爆炸那一刻会报差异。
                // Break check: zero the explosion radius bits; only the
                // explosion tick reports a mismatch.
                for event in &mut ev {
                    if event.first() == Some(&1) {
                        event[1] = 0;
                    }
                }
            }
            let mob = entity.kind.mob().expect("wither is a mob");
            let ours = [
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
            if ours != expected.w || ev != expected.ev {
                self.mismatch(format!(
                    "{} tick {tick}: official w {:?} ev {:?}, bamboo w {:?} ev {:?}",
                    case.name, expected.w, expected.ev, ours, ev
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}

/// 凋灵之首命中牛的逐刻帧：`[x,y,z,dx,dy,dz,yRot,xRot,removed,cowHealth,
/// cowDx,cowDy,cowDz,levelSeed,witherDuration,witherAmplifier]`。
///
/// Frame of the wither-skull-hits-cow scene (raw float/double bits).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherSkullCase {
    name: String,
    first_id: i32,
    start: [f64; 3],
    direction: [f64; 3],
    frames: Vec<[i64; 16]>,
}

impl WitherSkullCase {
    fn level(&self) -> MobLevel {
        let mut blocks = Vec::new();
        for x in -4..=4 {
            for z in -4..=12 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::BEDROCK),
                ));
            }
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
}

impl Report {
    /// 逐刻重放一枚凋灵之首命中牛：射弹轨迹、牛血量 / 击退、凋零效果、世界随机源。
    ///
    /// Replay a wither skull hitting a cow: projectile track, cow health /
    /// knockback, wither effect and the world random source.
    pub(super) fn replay_wither_skull(&mut self, case: &WitherSkullCase) {
        use mc_bamboo_blocks::{
            BlockTicks, World,
            entity::{Entity, EntityKind, mob::MobType, wither_skull::WitherSkullShot},
        };
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("wither_skull");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let direction = Vec3::new(case.direction[0], case.direction[1], case.direction[2]);
        let (skull_id, cow_id) = {
            let mut world = World::new(&mut level, &mut ticks);
            let skull_id = world.spawn_wither_skull_shot(&WitherSkullShot {
                position: start,
                direction,
                dangerous: false,
                owner: Some(case.first_id + 1),
                owner_uuid: None,
                // Java 夹具里 owner 凋灵 `snapTo(..., 0, 0)`，初始朝向是 (0, 0)。
                // The Java fixture's owner wither is spawned facing (0, 0).
                owner_y_rot: 0.0,
                owner_x_rot: 0.0,
            });
            let cow_id =
                world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(0.5, 64.0, 8.5)));
            (skull_id, cow_id)
        };
        let _ = level.entities.take_spawn_snapshots();
        let mut last_skull: Option<Entity> = None;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                // 只 tick 射弹，牛作为命中目标留在名单里（不跑物理）。
                // Tick only the skull; the cow stays as a hit target.
                if let Some(kept) = world.tick_one_entity_kept(skull_id) {
                    last_skull = Some(kept);
                }
                let _ = world.drain_effects();
            }
            let entity = level.entities.get(skull_id).or(last_skull.clone());
            let (position, delta, yaw, pitch, removed) = match &entity {
                Some(entity) => {
                    let pitch = match &entity.kind {
                        EntityKind::WitherSkull(skull) => skull.pitch,
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
            let (cow_health, cow_delta, wither_duration, wither_amplifier) =
                match level.entities.get(cow_id) {
                    Some(cow) => {
                        let mob = cow.kind.mob().expect("cow is a mob");
                        let effect = mob
                            .effects
                            .get(mc_bamboo_data::registries::mob_effect::WITHER);
                        (
                            mob.health.to_bits() as i32 as i64,
                            cow.delta,
                            effect.map_or(0, |e| i64::from(e.duration)),
                            effect.map_or(0, |e| i64::from(e.amplifier)),
                        )
                    }
                    None => (0, Vec3::default(), 0, 0),
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
                cow_delta.x.to_bits() as i64,
                cow_delta.y.to_bits() as i64,
                cow_delta.z.to_bits() as i64,
                level.random.source().seed(),
                wither_duration,
                wither_amplifier,
            ];
            if broken {
                // 改坏确认：把射弹 x 改一位，逐刻都报错。
                // Break check: shift the projectile x by one bit.
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
