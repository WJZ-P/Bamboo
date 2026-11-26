//! 末影之眼逐刻对拍：
//! 1. `ender_eye_drop` / `ender_eye_shatter`：逐刻比位置、速度、死亡音效、第 81 刻掉落
//!    `ItemEntity` 轨迹或 2003 碎裂事件与世界随机源；
//! 2. `ender_eye_frame_push`：站在末地传送门框架中央与旁侧的两头 `noAi` 牛，装眼时只推起
//!    站在中央眼柱上的那头牛，逐刻比坐标、着地、`HAS_EYE`、1503 事件与余量。
//!
//! Tick-by-tick parity for `EyeOfEnder` flight/expiry and `EndPortalFrame` `pushEntitiesUp`.

use mc_bamboo_blocks::{
    Actor, BlockHit, BlockState, BlockTicks, Effect, InteractionResult, ItemExchange, LevelRandom,
    World,
    entity::{Entity, EntityKind, KindPublic, eye_of_ender},
    state::{EYE, HORIZONTAL_FACING},
    use_item_on_with_stack,
};
use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::{blocks, items, sounds};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EnderEyeFlightCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) skip_seeds: i32,
    pub(super) start: [f64; 3],
    pub(super) target: [f64; 3],
    /// 每刻：`[tickCount, removed, x, y, z, dx, dy, dz, deathSound, event2003,
    ///         dropPresent, dropX, dropY, dropZ, dropDx, dropDy, dropDz, levelSeed]`。
    pub(super) frames: Vec<[i64; 18]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EnderEyeFramePushCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    /// 每刻：`[centerX, centerY, centerZ, centerDy, centerOnGround,
    ///         sideX, sideY, sideZ, sideOnGround, hasEye, event1503, handCount]`。
    pub(super) frames: Vec<[i64; 12]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EnderEyeSaveCase {
    pub(super) nbt: String,
    pub(super) reloaded: String,
}

fn make_level(first_id: i32, min_xz: i32, max_xz: i32) -> MobLevel {
    let mut blocks_map = HashMap::new();
    for x in min_xz..=max_xz {
        for z in min_xz..=max_xz {
            blocks_map.insert(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
        }
    }
    MobLevel {
        blocks: blocks_map,
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
        entities: Entities::with_next_id(first_id),
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
        raid: None,
    }
}

impl Report {
    pub(super) fn replay_ender_eye_flight(&mut self, case: &EnderEyeFlightCase) {
        self.cases += 1;
        let break_life = self.break_test.as_deref() == Some("ender_eye_life");
        let break_target = self.break_test.as_deref() == Some("ender_eye_target");
        let mut level = make_level(case.first_id, -16, 24);
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let target = Vec3::new(case.target[0], case.target[1], case.target[2]);
        let death_sound_id = i32::from(sounds::sound_event("minecraft:entity.ender_eye.death"));

        let uid = {
            let mut world = World::new(&mut level, &mut ticks);
            for _ in 0..case.skip_seeds {
                let _ = world.prepare_entity(eye_of_ender::entity(start, ItemStack::empty()));
            }
            let uid = world.spawn_eye_of_ender(start, ItemStack::new(items::ENDER_EYE, 3), target);
            if break_life {
                world.with_entity_mut(uid, &mut |entity| {
                    if let EntityKind::EyeOfEnder(eye) = &mut entity.kind {
                        eye.life = 1;
                    }
                });
            }
            if break_target {
                world.with_entity_mut(uid, &mut |entity| {
                    if let EntityKind::EyeOfEnder(eye) = &mut entity.kind
                        && let Some(t) = &mut eye.target
                    {
                        t.y -= 1.0;
                    }
                });
            }
            uid
        };
        let _ = level.entities.take_spawn_snapshots();
        let mut last_eye = level.entities.get(uid).expect("eye spawned");
        let mut drop_uid: Option<i32> = None;

        for (tick, expected) in case.frames.iter().enumerate() {
            let mut effects = Vec::new();
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                effects = world.drain_effects();
                for public in level.entities.take_spawn_snapshots() {
                    if matches!(public.kind, KindPublic::Item { .. }) {
                        drop_uid = Some(public.uid);
                    }
                }
            }
            let (tick_count, removed, pos, delta) = match level.entities.get(uid) {
                Some(entity) => {
                    last_eye = entity.clone();
                    (
                        i64::from(entity.tick_count),
                        i64::from(entity.removed),
                        entity.position,
                        entity.delta,
                    )
                }
                // 官方第 81 刻 `discard()` 后仍保留最后坐标/速度/tickCount 读出；Bamboo 已移出表。
                // 注意：Bamboo 移出表前 `last_eye` 是第 80 刻的快照，第 81 刻先走 `common_tick` + 一步位置速度再 discard。
                None => {
                    if last_eye.tick_count == 80 {
                        // 模拟被移除前那一刻已完成的 tickCount / pos / delta 更新（已通过 drop/event 验证）。
                        last_eye.tick_count += 1;
                        let new_pos = last_eye.position.add(last_eye.delta);
                        if let EntityKind::EyeOfEnder(eye) = &last_eye.kind
                            && let Some(t) = eye.target
                        {
                            last_eye.delta =
                                eye_of_ender::update_delta_movement(last_eye.delta, new_pos, t);
                        }
                        last_eye.position = new_pos;
                        last_eye.removed = true;
                    }
                    (
                        i64::from(last_eye.tick_count),
                        1_i64,
                        last_eye.position,
                        last_eye.delta,
                    )
                }
            };
            let death_sounds = effects
                .iter()
                .filter(|e| matches!(e, Effect::Sound { sound, .. } if *sound == death_sound_id))
                .count() as i64;
            let event_2003 = effects
                .iter()
                .filter(|e| matches!(e, Effect::LevelEvent { event: 2003, .. }))
                .count() as i64;
            let (drop_present, dx, dy, dz, ddx, ddy, ddz) = drop_uid
                .and_then(|id| level.entities.get(id))
                .filter(|e| !e.removed)
                .map_or((0, 0, 0, 0, 0, 0, 0), |item| {
                    (
                        1,
                        item.position.x.to_bits() as i64,
                        item.position.y.to_bits() as i64,
                        item.position.z.to_bits() as i64,
                        item.delta.x.to_bits() as i64,
                        item.delta.y.to_bits() as i64,
                        item.delta.z.to_bits() as i64,
                    )
                });
            let ours = [
                tick_count,
                removed,
                pos.x.to_bits() as i64,
                pos.y.to_bits() as i64,
                pos.z.to_bits() as i64,
                delta.x.to_bits() as i64,
                delta.y.to_bits() as i64,
                delta.z.to_bits() as i64,
                death_sounds,
                event_2003,
                drop_present,
                dx,
                dy,
                dz,
                ddx,
                ddy,
                ddz,
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
        println!("{}: {} ticks", case.name, case.frames.len());
    }

    pub(super) fn replay_ender_eye_frame_push(&mut self, case: &EnderEyeFramePushCase) {
        self.cases += 1;
        let break_push = self.break_test.as_deref() == Some("ender_eye_push");
        let mut level = make_level(case.first_id, -2, 5);
        let frame_pos = BlockPos::new(2, 64, 2);
        let side_frame_pos = BlockPos::new(1, 64, 2);
        let empty_frame = BlockState(blocks::END_PORTAL_FRAME)
            .with(HORIZONTAL_FACING, Direction::North)
            .with(EYE, false);
        level.blocks.insert(frame_pos, empty_frame);
        level.blocks.insert(side_frame_pos, empty_frame);

        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let (center_uid, side_uid) = {
            let mut world = World::new(&mut level, &mut ticks);
            let mut make_cow = |x: f64| {
                let mut cow = Entity::cow(Vec3::new(x, 64.8125, 2.5));
                cow.on_ground = true;
                if let Some(mob) = cow.kind.mob_mut() {
                    mob.no_ai = true;
                }
                let uid = world.spawn_entity(cow);
                world.with_entity_mut(uid, &mut |e| {
                    e.y_rot = 0.0;
                    if let Some(m) = e.kind.mob_mut() {
                        m.y_body_rot = 0.0;
                        m.y_head_rot = 0.0;
                    }
                });
                uid
            };
            (make_cow(2.5), make_cow(1.5))
        };

        let mut hand = ItemStack::new(items::ENDER_EYE, 2);
        let actor = Actor {
            uid: case.first_id + 100,
            uuid: ((1_u128 << 64) | 9).to_be_bytes(),
            position: Vec3::new(2.5, 64.0, 4.5),
            yaw: 180.0,
            head_yaw: 180.0,
            pitch: 0.0,
            secondary_use: false,
            main_hand: items::ENDER_EYE,
            off_hand: 0,
            eye_height: 1.62,
            block_interaction_range: 4.5,
            may_build: true,
            prevents_block_drops: false,
            can_use_game_master_blocks: false,
        };

        for (tick, expected) in case.frames.iter().enumerate() {
            let mut effects = Vec::new();
            if tick == 1 {
                let hit = BlockHit {
                    pos: frame_pos,
                    face: Direction::Up,
                    location: Vec3::new(2.5, 64.8125, 2.5),
                    inside: false,
                };
                let outcome =
                    use_item_on_with_stack(&mut level, &mut ticks, &actor, Hand::Main, &hit, &hand);
                effects.extend(outcome.effects);
                if matches!(outcome.item_exchange, Some(ItemExchange::Shrink))
                    && matches!(
                        outcome.result,
                        InteractionResult::Success | InteractionResult::Consume
                    )
                {
                    hand.count = (hand.count - 1).max(0);
                    if hand.count == 0 {
                        hand = ItemStack::empty();
                    }
                }
                if break_push {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(center_uid, &mut |e| {
                        e.position.y = 64.8125;
                        e.old_position.y = 64.8125;
                    });
                }
            }
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                effects.extend(world.drain_effects());
            }
            let center = level.entities.get(center_uid).expect("center cow alive");
            let side = level.entities.get(side_uid).expect("side cow alive");
            let has_eye = i64::from(
                level
                    .blocks
                    .get(&frame_pos)
                    .copied()
                    .unwrap_or(BlockState(blocks::AIR))
                    .get(EYE),
            );
            let event_1503 = effects
                .iter()
                .filter(|e| matches!(e, Effect::LevelEvent { event: 1503, .. }))
                .count() as i64;
            let ours = [
                center.position.x.to_bits() as i64,
                center.position.y.to_bits() as i64,
                center.position.z.to_bits() as i64,
                center.delta.y.to_bits() as i64,
                i64::from(center.on_ground),
                side.position.x.to_bits() as i64,
                side.position.y.to_bits() as i64,
                side.position.z.to_bits() as i64,
                i64::from(side.on_ground),
                has_eye,
                event_1503,
                i64::from(hand.count),
            ];
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

    pub(super) fn replay_ender_eye_save(&mut self, case: &EnderEyeSaveCase) {
        const SAVE_KEYS: &[&str] = &[
            "Pos",
            "Motion",
            "Rotation",
            "fall_distance",
            "Fire",
            "Air",
            "OnGround",
            "Invulnerable",
            "PortalCooldown",
            "UUID",
            "Item",
        ];
        self.cases += 1;
        let mut official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("ender_eye_save: SNBT parse failed: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!(
                    "ender_eye_save: reloaded SNBT parse failed: {error:?}"
                ));
                return;
            }
        };
        if let mc_bamboo_nbt::Tag::Compound(fields) = &mut official {
            fields.push((
                "id".to_owned(),
                mc_bamboo_nbt::Tag::String("minecraft:eye_of_ender".to_owned()),
            ));
        }
        let row = match mc_bamboo_blocks::archive::entity::read_entity(&official, 1) {
            Ok(Some(row)) => row,
            Ok(None) => {
                self.mismatch("ender_eye_save: read_entity returned None".to_owned());
                return;
            }
            Err(error) => {
                self.mismatch(format!("ender_eye_save: load failed: {error:?}"));
                return;
            }
        };
        let ours = match mc_bamboo_blocks::archive::entity::write_entity(&row) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("ender_eye_save: save failed: {error:?}"));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            if actual != expected {
                self.mismatch(format!(
                    "ender_eye_save key {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
            self.steps += 1;
        }
        println!("ender_eye_save: {} keys", SAVE_KEYS.len());
    }
}
