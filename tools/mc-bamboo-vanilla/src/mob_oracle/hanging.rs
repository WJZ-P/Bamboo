//! 悬挂实体（画、物品展示框、荧光物品展示框）逐刻与事件对拍：
//!
//! 覆盖：
//! - 4×4 墙与 2×1 窄墙放画：随机最大面积 `#placeable` 变体、精确位置/包围盒/配对包，
//!   拆掉背后方块后第 101 刻掉落及掉落坐标与音效；
//! - 物品展示框放物品、旋转三下、隔墙驱动真实比较器（输出 1→2→3→4）、第一下打掉物品
//!   （比较器归 0）、第二下打掉框，逐刻比状态、增量元数据与音效；
//! - 荧光物品展示框顶面放置、五种专属音效与掉落，以及 `Fixed`/`Invisible` 展示框免疫拆除/交互/受击。
//!
//! Tick-by-tick and event parity for hanging entities (paintings, item frames,
//! and glow item frames) against vanilla 26.3.

use mc_bamboo_blocks::{
    BlockState, BlockTicks, Effect, ItemExchange, LevelRandom, World,
    context::{Actor, BlockHit},
    entity::{
        Entities, Entity, EntityKind, KindPublic,
        hanging::{HangingHurt, recalculate_item_frame},
    },
    interact::{InteractionResult, run_due_ticks, use_item_on_with_stack},
    state::{ComparatorMode, HORIZONTAL_FACING, MODE_COMPARATOR, POWERED},
};
use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_game::entity_tracking::{EntityTracking, Snapshot};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HangingCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) frames: Vec<HangingFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HangingFrame {
    /// `[alive, fixedAlive, actionResult, variantId, dirId, itemId, rotation, analogOut,
    ///   compOut, compPowered, handCount, dropCount, dropItem, yRotBits, xRotBits]`
    pub(super) state: [i64; 15],
    /// `[xBits, yBits, zBits]`
    pub(super) pos: [i64; 3],
    /// `[minX, minY, minZ, maxX, maxY, maxZ]` raw f64 bits
    pub(super) aabb: [i64; 6],
    /// `[dropX, dropY, dropZ]` raw f64 bits
    pub(super) drop_pos: [i64; 3],
    /// 这一刻播放的音效注册表 ID 列表。
    pub(super) sounds: Vec<i64>,
    /// 这一刻发出的 `ServerEntity` 包（base64）。
    pub(super) packets: Vec<String>,
}

impl HangingCase {
    fn level(&self) -> MobLevel {
        let mut blocks_map = HashMap::new();
        if self.name == "hanging_painting_4x4" {
            for x in 0..=3 {
                for y in 64..=67 {
                    blocks_map.insert(BlockPos::new(x, y, 0), BlockState(blocks::STONE));
                }
            }
        } else if self.name == "hanging_painting_2x1" {
            for x in 0..=1 {
                blocks_map.insert(BlockPos::new(x, 64, 0), BlockState(blocks::STONE));
            }
        } else if self.name == "hanging_item_frame_comparator" {
            for x in -2..=2 {
                for z in -2..=2 {
                    blocks_map.insert(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
                }
            }
            blocks_map.insert(BlockPos::new(0, 64, 0), BlockState(blocks::STONE));
            let comp = BlockState(blocks::COMPARATOR)
                .with(HORIZONTAL_FACING, Direction::South)
                .with(MODE_COMPARATOR, ComparatorMode::Compare)
                .with(POWERED, false);
            blocks_map.insert(BlockPos::new(0, 64, -1), comp);
        } else if self.name == "hanging_glow_frame_and_fixed" {
            blocks_map.insert(BlockPos::new(0, 64, 0), BlockState(blocks::STONE));
            blocks_map.insert(BlockPos::new(2, 64, 0), BlockState(blocks::STONE));
        }

        MobLevel {
            raid: None,
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

fn actor_for(item: i32) -> Actor {
    Actor {
        uid: 100,
        uuid: ((1_u128 << 64) | 9).to_be_bytes(),
        position: Vec3::new(0.5, 64.0, 3.0),
        yaw: 180.0,
        head_yaw: 180.0,
        pitch: 0.0,
        secondary_use: false,
        main_hand: item,
        off_hand: 0,
        eye_height: 1.62,
        block_interaction_range: 4.5,
        may_build: true,
        prevents_block_drops: false,
        can_use_game_master_blocks: false,
    }
}

fn decode_packets(encoded: &[String]) -> Vec<Vec<u8>> {
    encoded
        .iter()
        .map(|s| base64_stub::decode(s).expect("valid base64 packet"))
        .collect()
}

fn run_use_on(
    level: &mut MobLevel,
    ticks: &mut BlockTicks,
    hand: &mut ItemStack,
    clicked: BlockPos,
    face: Direction,
    effects: &mut Vec<Effect>,
) -> i64 {
    let hit = BlockHit {
        pos: clicked,
        face,
        location: Vec3::new(
            f64::from(clicked.x) + 0.5,
            f64::from(clicked.y) + 0.5,
            f64::from(clicked.z) + 0.5,
        ),
        inside: false,
    };
    let actor = actor_for(hand.item);
    let outcome = use_item_on_with_stack(level, ticks, &actor, Hand::Main, &hit, hand);
    effects.extend(outcome.effects);
    if matches!(outcome.item_exchange, Some(ItemExchange::Shrink)) {
        hand.count = (hand.count - 1).max(0);
        if hand.count == 0 {
            *hand = ItemStack::empty();
        }
    }
    i64::from(matches!(
        outcome.result,
        InteractionResult::Success | InteractionResult::Consume
    ))
}

impl Report {
    pub(super) fn replay_hanging(&mut self, case: &HangingCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let comparator_pos =
            (case.name == "hanging_item_frame_comparator").then_some(BlockPos::new(0, 64, -1));

        let mut tracking = EntityTracking::default();
        let mut main_uid: Option<i32> = None;
        let mut fixed_uid: Option<i32> = None;
        let mut hand = ItemStack::empty();

        // 保留最后一帧（哪怕已移除）的快照，与官方保持一致。
        let mut last_main: Option<Entity> = None;

        for (tick, expected) in case.frames.iter().enumerate() {
            let mut effects: Vec<Effect> = Vec::new();
            let mut tick_packets: Vec<Vec<u8>> = Vec::new();
            let mut action_result = -1_i64;
            let _ = level.entities.take_spawn_snapshots();

            if tick > 0 {
                let now = 1000 + tick as i64;
                ticks.set_time(now);
                let due = run_due_ticks(&mut level, &mut ticks, now);
                effects.extend(due.effects);
            }

            if case.name == "hanging_painting_4x4" {
                if tick == 0 {
                    hand = ItemStack::new(items::PAINTING, 2);
                    action_result = run_use_on(
                        &mut level,
                        &mut ticks,
                        &mut hand,
                        BlockPos::new(1, 64, 0),
                        Direction::South,
                        &mut effects,
                    );
                    for snap in level.entities.take_spawn_snapshots() {
                        if matches!(snap.kind, KindPublic::Painting(_)) {
                            main_uid = Some(snap.uid);
                            let s = Snapshot::of(&snap);
                            let tracked = tracking.tracked_for_oracle(&s);
                            tick_packets.extend(tracked.pairing_packets(&s));
                        }
                    }
                } else if tick == 10 {
                    level.blocks.remove(&BlockPos::new(1, 64, 0));
                }
            } else if case.name == "hanging_painting_2x1" {
                if tick == 0 {
                    hand = ItemStack::new(items::PAINTING, 2);
                    action_result = run_use_on(
                        &mut level,
                        &mut ticks,
                        &mut hand,
                        BlockPos::new(0, 64, 0),
                        Direction::South,
                        &mut effects,
                    );
                    for snap in level.entities.take_spawn_snapshots() {
                        if matches!(snap.kind, KindPublic::Painting(_)) {
                            main_uid = Some(snap.uid);
                            let s = Snapshot::of(&snap);
                            let tracked = tracking.tracked_for_oracle(&s);
                            tick_packets.extend(tracked.pairing_packets(&s));
                        }
                    }
                } else if tick == 5 {
                    level.blocks.remove(&BlockPos::new(1, 64, 0));
                }
            } else if case.name == "hanging_item_frame_comparator" {
                if tick == 1 {
                    hand = ItemStack::new(items::ITEM_FRAME, 2);
                    action_result = run_use_on(
                        &mut level,
                        &mut ticks,
                        &mut hand,
                        BlockPos::new(0, 64, 0),
                        Direction::South,
                        &mut effects,
                    );
                    for snap in level.entities.take_spawn_snapshots() {
                        if matches!(snap.kind, KindPublic::ItemFrame(_)) {
                            main_uid = Some(snap.uid);
                            let s = Snapshot::of(&snap);
                            let tracked = tracking.tracked_for_oracle(&s);
                            tick_packets.extend(tracked.pairing_packets(&s));
                        }
                    }
                } else if tick == 5
                    && let Some(uid) = main_uid
                {
                    hand = ItemStack::new(items::DIAMOND, 4);
                    let mut world = World::new(&mut level, &mut ticks);
                    let out = world.interact_item_frame(uid, &hand, false);
                    effects.extend(world.drain_effects());
                    if out.consume_held {
                        hand.count = (hand.count - 1).max(0);
                    }
                    action_result = i64::from(matches!(
                        out.result,
                        InteractionResult::Success | InteractionResult::Consume
                    ));
                } else if matches!(tick, 10 | 15 | 20)
                    && let Some(uid) = main_uid
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    let out = world.interact_item_frame(uid, &hand, false);
                    effects.extend(world.drain_effects());
                    action_result = i64::from(matches!(
                        out.result,
                        InteractionResult::Success | InteractionResult::Consume
                    ));
                } else if matches!(tick, 25 | 30)
                    && let Some(uid) = main_uid
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    let hurt = world.hurt_hanging(uid, HangingHurt::player(false));
                    effects.extend(world.drain_effects());
                    action_result = i64::from(hurt);
                }
            } else if case.name == "hanging_glow_frame_and_fixed" {
                if tick == 1 {
                    hand = ItemStack::new(items::GLOW_ITEM_FRAME, 2);
                    action_result = run_use_on(
                        &mut level,
                        &mut ticks,
                        &mut hand,
                        BlockPos::new(0, 64, 0),
                        Direction::Up,
                        &mut effects,
                    );
                    for snap in level.entities.take_spawn_snapshots() {
                        if matches!(snap.kind, KindPublic::ItemFrame(_)) {
                            main_uid = Some(snap.uid);
                            let s = Snapshot::of(&snap);
                            let tracked = tracking.tracked_for_oracle(&s);
                            tick_packets.extend(tracked.pairing_packets(&s));
                        }
                    }
                    let mut fx =
                        Entity::item_frame(BlockPos::new(2, 64, 1), Direction::South, false);
                    if let EntityKind::ItemFrame(f) = &mut fx.kind {
                        f.fixed = true;
                        f.invisible = true;
                    }
                    recalculate_item_frame(&mut fx);
                    let uid = {
                        let mut world = World::new(&mut level, &mut ticks);
                        world.spawn_entity(fx)
                    };
                    fixed_uid = Some(uid);
                    for snap in level.entities.take_spawn_snapshots() {
                        if snap.uid == uid {
                            let s = Snapshot::of(&snap);
                            let tracked = tracking.tracked_for_oracle(&s);
                            tick_packets.extend(tracked.pairing_packets(&s));
                        }
                    }
                } else if tick == 2 {
                    level.blocks.remove(&BlockPos::new(2, 64, 0));
                } else if tick == 3
                    && let Some(uid) = main_uid
                {
                    hand = ItemStack::new(items::GOLD_INGOT, 2);
                    let mut world = World::new(&mut level, &mut ticks);
                    let out = world.interact_item_frame(uid, &hand, false);
                    effects.extend(world.drain_effects());
                    if out.consume_held {
                        hand.count = (hand.count - 1).max(0);
                    }
                    action_result = i64::from(matches!(
                        out.result,
                        InteractionResult::Success | InteractionResult::Consume
                    ));
                } else if tick == 4
                    && let Some(uid) = fixed_uid
                {
                    hand = ItemStack::new(items::GOLD_INGOT, 2);
                    let mut world = World::new(&mut level, &mut ticks);
                    let out = world.interact_item_frame(uid, &hand, false);
                    effects.extend(world.drain_effects());
                    action_result = i64::from(matches!(
                        out.result,
                        InteractionResult::Success | InteractionResult::Consume
                    ));
                } else if tick == 5
                    && let Some(uid) = main_uid
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    let out = world.interact_item_frame(uid, &hand, false);
                    effects.extend(world.drain_effects());
                    action_result = i64::from(matches!(
                        out.result,
                        InteractionResult::Success | InteractionResult::Consume
                    ));
                } else if tick == 6
                    && let Some(uid) = fixed_uid
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    let hurt = world.hurt_hanging(uid, HangingHurt::player(false));
                    effects.extend(world.drain_effects());
                    action_result = i64::from(hurt);
                } else if matches!(tick, 7 | 9)
                    && let Some(uid) = main_uid
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    let hurt = world.hurt_hanging(uid, HangingHurt::player(false));
                    effects.extend(world.drain_effects());
                    action_result = i64::from(hurt);
                }
            }

            if tick > 0 {
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                effects.extend(world.drain_effects());
                for uid_opt in [main_uid, fixed_uid] {
                    if let Some(uid) = uid_opt
                        && let Some(entity) = level.entities.get(uid)
                        && !entity.removed
                    {
                        let pub_snap = entity.split().public;
                        let s = Snapshot::of(&pub_snap);
                        let tracked = tracking.tracked_for_oracle(&s);
                        tick_packets.extend(tracked.step(&s, s.needs_sync, s.sync_velocity));
                    }
                }
            }

            let current_main = main_uid.and_then(|uid| level.entities.get(uid));
            if let Some(ent) = &current_main {
                last_main = Some(ent.clone());
            }
            let alive = i64::from(current_main.as_ref().is_some_and(|e| !e.removed));
            let fixed_alive = i64::from(
                fixed_uid
                    .and_then(|uid| level.entities.get(uid))
                    .is_some_and(|e| !e.removed),
            );
            let mut variant_id = -1_i64;
            let mut dir_id = -1_i64;
            let mut item_id = 0_i64;
            let mut rotation = 0_i64;
            let mut analog_out = 0_i64;
            let mut pos_bits = [0_i64; 3];
            let mut aabb_bits = [0_i64; 6];
            let mut y_rot_bits = 0_i64;
            let mut x_rot_bits = 0_i64;
            if let Some(ent) = current_main.as_ref().or(last_main.as_ref()) {
                pos_bits = [
                    ent.position.x.to_bits() as i64,
                    ent.position.y.to_bits() as i64,
                    ent.position.z.to_bits() as i64,
                ];
                let bb = ent.bounding_box();
                aabb_bits = [
                    bb.min_x.to_bits() as i64,
                    bb.min_y.to_bits() as i64,
                    bb.min_z.to_bits() as i64,
                    bb.max_x.to_bits() as i64,
                    bb.max_y.to_bits() as i64,
                    bb.max_z.to_bits() as i64,
                ];
                y_rot_bits = i64::from(ent.y_rot.to_bits() as i32);
                match &ent.kind {
                    EntityKind::Painting(p) => {
                        variant_id = i64::from(p.variant);
                        dir_id = i64::from(p.direction.id());
                    }
                    EntityKind::ItemFrame(f) => {
                        dir_id = i64::from(f.direction.id());
                        item_id = i64::from(f.item.item);
                        rotation = i64::from(f.rotation);
                        analog_out = i64::from(f.analog_output());
                        x_rot_bits = i64::from(f.x_rot.to_bits() as i32);
                    }
                    _ => {}
                }
            }
            let (comp_out, comp_powered) = match comparator_pos {
                Some(cp) => (
                    i64::from(level.redstone.comparator_output(cp)),
                    i64::from(
                        level
                            .blocks
                            .get(&cp)
                            .copied()
                            .unwrap_or(BlockState::AIR)
                            .get(POWERED),
                    ),
                ),
                None => (0, 0),
            };
            let mut drop_count = 0_i64;
            let mut drop_item = 0_i64;
            let mut drop_pos = [0_i64; 3];
            for snap in level.entities.take_spawn_snapshots() {
                if let KindPublic::Item { stack, .. } = &snap.kind {
                    drop_count += 1;
                    drop_item = i64::from(stack.item);
                    drop_pos = [
                        snap.position.x.to_bits() as i64,
                        snap.position.y.to_bits() as i64,
                        snap.position.z.to_bits() as i64,
                    ];
                }
            }
            let sounds: Vec<i64> = effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Sound { sound, .. } => Some(i64::from(*sound)),
                    _ => None,
                })
                .collect();

            // `alive == 0` 时官方在 `dropItem` 中清了框内物品；Bamboo 已移除该实体，
            // 对齐 `last_main` 中的 item/analog_out 为 0，其余（坐标/包围盒/方向）保持最后值。
            if alive == 0
                && last_main
                    .as_ref()
                    .is_some_and(|e| matches!(e.kind, EntityKind::ItemFrame(_)))
            {
                item_id = 0;
                analog_out = 0;
            }

            let our_state = [
                alive,
                fixed_alive,
                action_result,
                variant_id,
                dir_id,
                item_id,
                rotation,
                analog_out,
                comp_out,
                comp_powered,
                i64::from(hand.count),
                drop_count,
                drop_item,
                y_rot_bits,
                x_rot_bits,
            ];
            if our_state != expected.state {
                self.mismatch(format!(
                    "{} tick {tick} state: official {:?}, bamboo {:?}",
                    case.name, expected.state, our_state
                ));
            }
            if pos_bits != expected.pos {
                self.mismatch(format!(
                    "{} tick {tick} pos: official {:?}, bamboo {:?}",
                    case.name, expected.pos, pos_bits
                ));
            }
            if aabb_bits != expected.aabb {
                self.mismatch(format!(
                    "{} tick {tick} aabb: official {:?}, bamboo {:?}",
                    case.name, expected.aabb, aabb_bits
                ));
            }
            if drop_pos != expected.drop_pos {
                self.mismatch(format!(
                    "{} tick {tick} drop_pos: official {:?}, bamboo {:?}",
                    case.name, expected.drop_pos, drop_pos
                ));
            }
            if sounds != expected.sounds {
                self.mismatch(format!(
                    "{} tick {tick} sounds: official {:?}, bamboo {:?}",
                    case.name, expected.sounds, sounds
                ));
            }
            let expected_packets = decode_packets(&expected.packets);
            if tick_packets != expected_packets {
                self.mismatch(format!(
                    "{} tick {tick} packets: official {} pkts {:?}, bamboo {} pkts {:?}",
                    case.name,
                    expected_packets.len(),
                    expected_packets,
                    tick_packets.len(),
                    tick_packets
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
