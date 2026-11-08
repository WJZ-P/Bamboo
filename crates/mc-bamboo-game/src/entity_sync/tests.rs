//! `entities.md` 的官方字节和多人场景。
//!
//! 字节例子用固定的实体 7，由 [`Stage`] 直接驱动 [`send_changes`]。
//! 多人场景走 [`Harness`]，经过完整的 tick，实体 ID 从 1 开始。

use std::sync::{Arc, Mutex};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    packet::play::PlayRequest,
};
use uuid::Uuid;

use super::*;
use crate::{
    inbound::PlayerSink,
    world::tests::{
        Client, Frame, Harness, SPAWN_X, SPAWN_Y, SPAWN_Z, client_requesting, frames_with_id,
        received_chunks,
    },
};

const STEVE: PlayerId = PlayerId(7);
const STEVE_UUID: Uuid = Uuid::from_u128(0x069a79f4_44e9_4726_a5be_fca90e38aaf5);
const WATCHER: PlayerId = PlayerId(9);

/// 规格里的例子用的位置，和移动之后的位置。
const POSITION: Vec3 = Vec3::new(8.5, 65.0, -3.25);
const MOVED: Vec3 = Vec3::new(8.75, 65.0, -3.375);
const POSITION_HEX: &str = "40 21 00 00 00 00 00 00
                            40 50 40 00 00 00 00 00
                            c0 0a 00 00 00 00 00 00";

/// 菜单槽位，`blocks-inventory.md` 8.1。
const HEAD_SLOT: usize = 5;
const HOTBAR_SLOT: usize = 36;
const OFF_HAND_SLOT: usize = 45;

const STONE: i32 = 1;
const DIAMOND_HELMET: i32 = 1084;

const SHIFT: u8 = 0x20;
const FLYING: u8 = 0x02;

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    digits
        .chunks(2)
        .map(|pair| {
            let pair = str::from_utf8(pair).expect("十六进制文本应该是 ASCII");
            u8::from_str_radix(pair, 16).expect("十六进制文本写错了")
        })
        .collect()
}

fn frame(id: i32, body: &str) -> Frame {
    Frame {
        id,
        body: hex(body),
    }
}

fn parse_frames(bytes: &[u8]) -> Vec<Frame> {
    let mut rest = bytes;
    let mut frames = Vec::new();
    while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
        let end = bounds.header_len + bounds.payload_len;
        let mut reader = Reader::new(&rest[bounds.header_len..end]);
        let id = reader.varint().unwrap();
        frames.push(Frame {
            id,
            body: reader.rest().to_vec(),
        });
        rest = &rest[end..];
    }
    frames
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);

impl PlayerSink for Recorder {
    fn send(&mut self, frames: Vec<u8>) -> bool {
        self.0.lock().unwrap().extend(frames);
        true
    }

    fn close(&mut self) {}
}

impl Recorder {
    fn take_bytes(&self) -> Vec<u8> {
        mem::take(&mut *self.0.lock().unwrap())
    }

    fn take_frames(&self) -> Vec<Frame> {
        parse_frames(&self.take_bytes())
    }
}

/// 实体 7 和一名观察者。观察者只有链路，不在世界里，所以不会被 `visible` 解除。
struct Stage {
    region: Region,
    /// 玩家自己收到的字节。
    own: Recorder,
    /// 观察者收到的字节。
    watcher: Recorder,
    tick: u32,
    /// [`run`](Self::run) 期间观察者收到的包和当时的 tick。
    received: Vec<(u32, Frame)>,
}

impl Stage {
    fn at(position: Vec3) -> Self {
        Self::with_client(position, ClientInformation::default())
    }

    fn with_client(position: Vec3, client: ClientInformation) -> Self {
        let mut region = Region::new(8, 20, false);
        let own = Recorder::default();
        let watcher = Recorder::default();
        region.links.add(STEVE, Box::new(own.clone()));
        region.links.add(WATCHER, Box::new(watcher.clone()));

        let mut player = Player::new(
            STEVE,
            STEVE_UUID,
            "Steve".to_owned(),
            GameMode::Creative,
            client,
            position,
        );
        player.sync.seen_by.insert(WATCHER);
        region.insert_player(player);

        Self {
            region,
            own,
            watcher,
            tick: 0,
            received: Vec::new(),
        }
    }

    fn player(&mut self) -> &mut Player {
        self.region.players.get_mut(&STEVE).expect("玩家在世界里")
    }

    fn shift(&mut self, dx: f64, dy: f64, dz: f64) {
        let Vec3 { x, y, z } = self.player().position();
        self.region
            .move_player(STEVE, Vec3::new(x + dx, y + dy, z + dz));
    }

    /// 只跑 `send_changes`，和官方跑 `sendChanges` 的 trace 一样。
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            let Region { players, links, .. } = &mut self.region;
            let player = players.get_mut(&STEVE).expect("玩家在世界里");
            send_changes(player, links);
            self.collect();
        }
    }

    /// 派生状态加 `send_changes`，和世界的 tick 里实体相关的部分一样。
    fn step(&mut self) {
        update_derived_state(&mut self.region);
        self.run(1);
    }

    fn collect(&mut self) {
        self.region.links.flush();
        let tick = self.tick;
        let frames = self.watcher.take_frames();
        self.received
            .extend(frames.into_iter().map(|frame| (tick, frame)));
        self.tick += 1;
    }

    /// 观察者这段时间收到的包，不带 tick。
    fn take_received(&mut self) -> Vec<Frame> {
        mem::take(&mut self.received)
            .into_iter()
            .map(|(_, frame)| frame)
            .collect()
    }

    /// 第一个周期是特殊的，大部分例子都要跳过它。跑完之后下一 tick 就是一个周期。
    fn skip_first_cycle(&mut self) {
        self.run(2);
        self.received.clear();
    }

    fn flush_watcher(&mut self) -> Vec<Frame> {
        self.region.links.flush();
        self.watcher.take_frames()
    }

    fn flush_own(&mut self) -> Vec<Frame> {
        self.region.links.flush();
        self.own.take_frames()
    }
}

fn stepped_sync(position: Vec3, ticks: u8, yaw: f32, on_ground: bool) -> Frame {
    let mut body = hex("07 01 01");
    for coordinate in [position.x, position.y, position.z] {
        body.extend(coordinate.to_be_bytes());
    }
    body.push(ticks);
    body.extend(yaw.to_be_bytes());
    body.extend(0f32.to_be_bytes());
    body.push(u8::from(on_ground));
    Frame {
        id: clientbound::ENTITY_POSITION_SYNC,
        body,
    }
}

/// `entities.md` 7.3 的 `[跑]`。脚本和 `Trace.java` 相同，期望值是 `trace.out` 的全部输出。
#[test]
fn official_trace_replays_byte_for_byte() {
    let mut stage = Stage::at(Vec3::new(0.0, 64.0, 0.0));

    stage.run(125);
    for _ in 0..6 {
        stage.shift(0.1, 0.0, 0.0);
        stage.run(1);
    }

    stage.player().yaw = 90.0;
    stage.run(2);

    for yaw in [100.0, 110.0] {
        stage.shift(0.1, 0.0, 0.0);
        stage.player().yaw = yaw;
        stage.run(1);
    }

    stage.shift(0.002, 0.0, 0.0);
    stage.run(2);

    stage.player().on_ground = true;
    stage.run(2);

    stage.player().on_ground = false;
    stage.shift(0.0, 0.4, 0.0);
    stage.run(2);

    stage.shift(10.0, 0.0, 0.0);
    stage.run(2);

    assert_eq!(stage.tick, 143);
    stage.shift(0.1, 0.0, 0.0);
    stage.player().sync.shared_flags = ENTITY_FLAG_SHIFT_KEY_DOWN;
    stage.player().sync.metadata_dirty = true;
    stage.run(1);
    assert_eq!(
        stage.flush_own(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 02 ff")]
    );

    stage.shift(0.1, 0.0, 0.0);
    stage.run(1);

    stage.run(1000);

    let pos = |body| frame(clientbound::MOVE_ENTITY_POS, body);
    let idle = || pos("07 02 02 0000 0000 0000");
    let mut expected = vec![
        (0, pos("07 00 0000 0000 0000")),
        (60, idle()),
        (120, idle()),
        (126, pos("07 02 02 0333 0000 0000")),
        (128, pos("07 02 02 0333 0000 0000")),
        (130, pos("07 02 02 0334 0000 0000")),
        (132, frame(clientbound::MOVE_ENTITY_ROT, "07 00 40 00")),
        (
            134,
            frame(
                clientbound::MOVE_ENTITY_POS_ROT,
                "07 02 02 0333 0000 0000 4e 00",
            ),
        ),
        (
            138,
            stepped_sync(
                Vec3::new(0.801_999_999_999_999_9, 64.0, 0.0),
                2,
                110.0,
                true,
            ),
        ),
        (
            140,
            stepped_sync(
                Vec3::new(0.801_999_999_999_999_9, 64.4, 0.0),
                2,
                110.0,
                false,
            ),
        ),
        (
            142,
            stepped_sync(Vec3::new(10.802, 64.4, 0.0), 2, 110.0, false),
        ),
        (143, pos("07 02 01 019a 0000 0000")),
        (143, frame(clientbound::SET_ENTITY_DATA, "07 00 00 02 ff")),
        (144, pos("07 02 01 0199 0000 0000")),
    ];
    expected.extend((180..=900).step_by(60).map(|tick| (tick, idle())));
    expected.push((
        940,
        stepped_sync(
            Vec3::new(11.001_999_999_999_999, 64.4, 0.0),
            2,
            110.0,
            false,
        ),
    ));
    expected.extend((960..=1140).step_by(60).map(|tick| (tick, idle())));

    assert_eq!(stage.received, expected);
}

/// `entities.md` 1.2、7.4 的 `[跑]`：(8.5, 65, -3.25) 移到 (8.75, 65, -3.375)。
#[test]
fn first_cycle_uses_a_linear_delta() {
    let mut stage = Stage::at(POSITION);
    stage.player().on_ground = true;
    stage.player().sync.was_on_ground = true;

    stage.region.move_player(STEVE, MOVED);
    stage.run(1);

    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::MOVE_ENTITY_POS,
            "07 01 04 00 00 00 fe 00"
        )]
    );
}

#[test]
fn later_cycles_use_one_step_with_the_ticks_since_the_last_cycle() {
    let mut stage = Stage::at(POSITION);
    stage.player().on_ground = true;
    stage.player().sync.was_on_ground = true;
    stage.skip_first_cycle();

    stage.region.move_player(STEVE, MOVED);
    stage.run(1);

    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::MOVE_ENTITY_POS,
            "07 03 02 04 00 00 00 fe 00"
        )]
    );
}

#[test]
fn moving_and_turning_matches_official_bytes() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.region.move_player(STEVE, MOVED);
    stage.player().yaw = 90.0;
    stage.player().pitch = -45.0;
    stage.run(1);

    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::MOVE_ENTITY_POS_ROT,
            "07 02 02 04 00 00 00 fe 00 40 e0"
        )]
    );
}

#[test]
fn turning_on_the_ground_matches_official_bytes() {
    let mut stage = Stage::at(POSITION);
    stage.player().on_ground = true;
    stage.player().sync.was_on_ground = true;
    stage.skip_first_cycle();

    stage.player().yaw = 90.0;
    stage.player().pitch = -45.0;
    stage.run(1);

    assert_eq!(
        stage.take_received(),
        [frame(clientbound::MOVE_ENTITY_ROT, "07 01 40 e0")]
    );
}

#[test]
fn sync_in_the_first_cycle_is_linear() {
    let mut stage = Stage::at(POSITION);
    stage.player().yaw = 90.0;

    stage.player().on_ground = true;
    stage.run(1);

    let expected = format!("07 00 {POSITION_HEX} 42 b4 00 00 00 00 00 00 01");
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::ENTITY_POSITION_SYNC, &expected)]
    );
}

#[test]
fn sync_in_later_cycles_is_one_step() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();
    stage.player().yaw = 90.0;

    stage.player().on_ground = true;
    stage.run(1);

    let expected = format!("07 01 01 {POSITION_HEX} 02 42 b4 00 00 00 00 00 00 01");
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::ENTITY_POSITION_SYNC, &expected)]
    );
}

#[test]
fn landing_on_an_odd_tick_waits_for_the_next_cycle() {
    let mut stage = Stage::at(POSITION);
    stage.run(5);
    stage.received.clear();

    // 官方这里有 `needsSync`，马上发 `set_entity_motion` 和 ticks = 1 的全量同步。
    // Bamboo 没有服务端物理，不设这个标记，下一个周期才发。
    stage.player().on_ground = true;
    stage.run(1);
    assert!(stage.take_received().is_empty());

    stage.run(1);
    assert_eq!(
        stage.take_received(),
        [stepped_sync(POSITION, 2, 0.0, true)]
    );
}

#[test]
fn sync_carries_the_exact_angles_and_also_updates_the_last_sent_ones() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.player().yaw = 1.4;
    stage.player().pitch = 45.0;
    stage.player().on_ground = true;
    stage.run(1);

    assert_eq!(stage.player().sync.last_yaw, 0x00);
    assert_eq!(stage.player().sync.last_pitch, 0x20);
    let frames = stage.take_received();
    let body = &frames[0].body;
    assert_eq!(body[body.len() - 9..body.len() - 5], 1.4f32.to_be_bytes());
    assert_eq!(body[body.len() - 5..body.len() - 1], 45f32.to_be_bytes());

    // 这两个角度已经发过了，下一个周期没有东西要发。
    stage.run(2);
    assert!(stage.take_received().is_empty());
}

#[test]
fn angles_below_the_resolution_of_a_byte_are_not_sent() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.player().yaw = 1.4;
    stage.run(2);

    assert!(stage.take_received().is_empty());

    stage.player().yaw = 1.41;
    stage.run(2);

    assert_eq!(
        stage.take_received(),
        [frame(clientbound::MOVE_ENTITY_ROT, "07 00 01 00")]
    );
}

#[test]
fn base_stays_put_below_the_threshold_and_later_deltas_start_from_it() {
    let mut stage = Stage::at(Vec3::new(0.0, 64.0, 0.0));
    stage.skip_first_cycle();

    // 0.002² 小于 2⁻¹⁷。
    stage.shift(0.002, 0.0, 0.0);
    stage.run(2);
    assert!(stage.take_received().is_empty());
    assert_eq!(stage.player().sync.base, Vec3::new(0.0, 64.0, 0.0));

    // 第二步单独看也低于阈值，累积起来超过了，位移从原来的 `base` 算：round(0.004 × 4096) = 16。
    stage.shift(0.002, 0.0, 0.0);
    stage.run(2);
    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::MOVE_ENTITY_POS,
            "07 02 02 0010 0000 0000"
        )]
    );
    assert_eq!(stage.player().sync.base.x, 0.004);
}

#[test]
fn rotation_alone_does_not_move_the_base() {
    let mut stage = Stage::at(Vec3::new(0.0, 64.0, 0.0));
    stage.skip_first_cycle();

    stage.shift(0.002, 0.0, 0.0);
    stage.player().yaw = 90.0;
    stage.run(2);

    assert_eq!(
        stage.take_received(),
        [frame(clientbound::MOVE_ENTITY_ROT, "07 00 40 00")]
    );
    assert_eq!(stage.player().sync.base, Vec3::new(0.0, 64.0, 0.0));
}

#[test]
fn delta_overflow_falls_back_to_a_sync_without_resetting_the_teleport_timer() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();
    let delay = stage.player().sync.teleport_delay;

    // 32767 / 4096 不到 8 格。
    stage.shift(8.0, 0.0, 0.0);
    stage.run(1);

    let target = Vec3::new(POSITION.x + 8.0, POSITION.y, POSITION.z);
    assert_eq!(stage.take_received(), [stepped_sync(target, 2, 0.0, false)]);
    assert_eq!(stage.player().sync.base, target);
    assert_eq!(stage.player().sync.teleport_delay, delay + 1);
}

#[test]
fn largest_delta_still_fits_in_a_short() {
    let mut stage = Stage::at(Vec3::new(0.0, 64.0, 0.0));
    stage.skip_first_cycle();

    stage.shift(32767.0 / 4096.0, 0.0, -8.0);
    stage.run(1);

    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::MOVE_ENTITY_POS,
            "07 02 02 7fff 0000 8000"
        )]
    );
}

#[test]
fn one_cycle_sends_movement_then_metadata_then_the_head() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.region.move_player(STEVE, MOVED);
    stage.player().yaw = 90.0;
    stage.player().sync.shared_flags = ENTITY_FLAG_SPRINTING;
    stage.step();

    assert_eq!(
        stage.take_received(),
        [
            frame(
                clientbound::MOVE_ENTITY_POS_ROT,
                "07 02 02 04 00 00 00 fe 00 40 00"
            ),
            frame(clientbound::SET_ENTITY_DATA, "07 00 00 08 ff"),
            frame(clientbound::ROTATE_HEAD, "07 40"),
        ]
    );
    // 玩家自己只收到元数据。
    assert_eq!(
        stage.flush_own(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 08 ff")]
    );
}

#[test]
fn head_follows_the_yaw_and_is_only_sent_when_it_changes() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.player().yaw = 90.0;
    stage.step();
    assert_eq!(stage.player().head_yaw, 90.0);
    assert_eq!(
        stage.take_received(),
        [
            frame(clientbound::MOVE_ENTITY_ROT, "07 00 40 00"),
            frame(clientbound::ROTATE_HEAD, "07 40"),
        ]
    );

    stage.step();
    stage.step();
    assert!(stage.take_received().is_empty());
}

#[test]
fn head_waits_for_the_next_cycle_on_an_odd_tick() {
    let mut stage = Stage::at(POSITION);
    stage.run(3);
    stage.received.clear();

    stage.player().yaw = 90.0;
    stage.step();
    assert!(stage.take_received().is_empty());

    stage.step();
    assert_eq!(stage.take_received().len(), 2);
}

#[test]
fn sync_state_moves_on_while_nobody_watches() {
    let mut stage = Stage::at(POSITION);
    stage.player().sync.seen_by.clear();

    stage.region.move_player(STEVE, MOVED);
    stage.player().yaw = 90.0;
    stage.run(4);

    // 没有观察者时状态照样往前走，之后的新观察者从这里开始。
    assert!(stage.take_received().is_empty());
    assert!(stage.flush_own().is_empty());
    assert_eq!(stage.player().sync.base, MOVED);
    assert_eq!(stage.player().sync.last_yaw, 0x40);
}

fn left_handed_client() -> ClientInformation {
    ClientInformation {
        model_customisation: 0x7f,
        main_hand: HumanoidArm::Left,
        ..ClientInformation::default()
    }
}

/// `entities.md` 第 5 节和第 8 节的 `[跑]`，两个例子放在同一个 bundle 里。
#[test]
fn pairing_bundle_matches_official_bytes() {
    let mut stage = Stage::with_client(POSITION, left_handed_client());
    stage.player().yaw = 90.0;
    on_player_input(&mut stage.region, STEVE, SHIFT);
    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StartSprinting);
    stage.step();

    let bytes = pairing_frames(stage.player());

    let add_entity = format!(
        "07
         06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5
         9f 01
         {POSITION_HEX}
         00
         00 40 40
         00"
    );
    let delimiter = frame(clientbound::BUNDLE_DELIMITER, "");
    assert_eq!(
        parse_frames(&bytes),
        [
            delimiter.clone(),
            frame(clientbound::ADD_ENTITY, &add_entity),
            frame(
                clientbound::SET_ENTITY_DATA,
                "07
                 00 00 0a
                 06 14 05
                 09 03 41 a0 00 00
                 0f 2a 00
                 10 00 7f
                 ff"
            ),
            delimiter,
        ]
    );
    assert_eq!(bytes[..2], hex("01 00"));
    assert_eq!(bytes[bytes.len() - 2..], hex("01 00"));
}

#[test]
fn pairing_sends_only_the_values_that_are_not_default() {
    let mut stage = Stage::at(POSITION);

    let frames = parse_frames(&pairing_frames(stage.player()));

    assert_eq!(
        frames[2],
        frame(clientbound::SET_ENTITY_DATA, "07 09 03 41 a0 00 00 ff")
    );
}

#[test]
fn pairing_lists_the_non_empty_equipment_in_ordinal_order() {
    let mut stage = Stage::at(POSITION);
    let inventory = &mut stage.player().inventory;
    inventory.slots[HEAD_SLOT] = ItemStack::new(DIAMOND_HELMET, 1);
    inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 64);

    let frames = parse_frames(&pairing_frames(stage.player()));

    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::BUNDLE_DELIMITER,
            clientbound::ADD_ENTITY,
            clientbound::SET_ENTITY_DATA,
            clientbound::SET_EQUIPMENT,
            clientbound::BUNDLE_DELIMITER,
        ]
    );
    assert_eq!(frames[3].body, hex("07 80 40 01 00 00 05 01 bc 08 00 00"));
}

#[test]
fn pairing_starts_from_what_was_last_sent_not_from_the_current_state() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    // 这些变化还没进入周期。
    stage.region.move_player(STEVE, MOVED);
    stage.player().yaw = 90.0;
    stage.player().pitch = 45.0;
    stage.player().head_yaw = 90.0;
    stage.player().sync.shared_flags = ENTITY_FLAG_SPRINTING;

    let frames = parse_frames(&pairing_frames(stage.player()));

    let add_entity = format!(
        "07 06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5 9f 01 {POSITION_HEX} 00 00 00 00 00"
    );
    assert_eq!(frames[1], frame(clientbound::ADD_ENTITY, &add_entity));
    assert_eq!(
        frames[2],
        frame(clientbound::SET_ENTITY_DATA, "07 09 03 41 a0 00 00 ff")
    );
}

#[test]
fn metadata_dirty_starts_a_cycle_on_an_odd_tick_even_without_movement() {
    let mut stage = Stage::at(POSITION);
    stage.run(1);
    stage.received.clear();
    assert_eq!(stage.tick % 2, 1);

    stage.player().sync.shared_flags = ENTITY_FLAG_SPRINTING;
    stage.step();

    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 08 ff")]
    );
    assert!(!stage.player().sync.metadata_dirty);
}

#[test]
fn metadata_that_went_back_to_the_sent_value_sends_nothing() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    stage.player().sync.metadata_dirty = true;
    stage.run(1);

    assert!(stage.take_received().is_empty());
    assert!(stage.flush_own().is_empty());
    assert!(!stage.player().sync.metadata_dirty);
}

#[test]
fn join_values_count_as_already_sent() {
    let mut stage = Stage::with_client(POSITION, left_handed_client());

    stage.step();

    assert!(stage.flush_own().is_empty());
    assert!(!stage.player().sync.metadata_dirty);
}

#[test]
fn skin_parts_and_main_hand_only_send_the_index_that_changed() {
    let mut stage = Stage::with_client(POSITION, left_handed_client());
    stage.skip_first_cycle();

    stage.player().client.model_customisation = 0x3f;
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 10 00 3f ff")]
    );

    stage.player().client.main_hand = HumanoidArm::Right;
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 0f 2a 01 ff")]
    );
    assert_eq!(stage.flush_own().len(), 2);
}

/// `entities.md` 第 10 节的 `[跑]`：前进 + 潜行 + 冲刺键。冲刺键不代表正在冲刺。
#[test]
fn sneaking_changes_the_flag_and_the_pose_in_one_packet() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    on_player_input(&mut stage.region, STEVE, 0x61);
    stage.step();

    let sneaking = frame(clientbound::SET_ENTITY_DATA, "07 00 00 02 06 14 05 ff");
    assert_eq!(stage.take_received(), std::slice::from_ref(&sneaking));
    assert_eq!(stage.flush_own(), [sneaking]);
    assert!(stage.player().sync.shift_key_down);
    assert!(stage.player().sync.is_crouching());

    on_player_input(&mut stage.region, STEVE, 0x01);
    stage.step();

    let standing = frame(clientbound::SET_ENTITY_DATA, "07 00 00 00 06 14 00 ff");
    assert_eq!(stage.take_received(), std::slice::from_ref(&standing));
    assert_eq!(stage.flush_own(), [standing]);
    assert!(!stage.player().sync.is_crouching());
}

#[test]
fn shift_while_flying_keeps_the_pose_until_the_player_lands() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    on_player_abilities(&mut stage.region, STEVE, FLYING);
    on_player_input(&mut stage.region, STEVE, SHIFT);
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 02 ff")]
    );
    assert!(!stage.player().sync.is_crouching());

    on_player_abilities(&mut stage.region, STEVE, 0);
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 06 14 05 ff")]
    );
    assert!(stage.player().sync.is_crouching());
}

#[test]
fn only_players_that_may_fly_can_be_flying() {
    let mut stage = Stage::at(POSITION);

    stage.player().game_mode = GameMode::Survival;
    on_player_abilities(&mut stage.region, STEVE, FLYING);
    assert!(!stage.player().sync.flying);

    stage.player().game_mode = GameMode::Creative;
    on_player_abilities(&mut stage.region, STEVE, FLYING);
    assert!(stage.player().sync.flying);
}

#[test]
fn sprinting_sets_and_clears_its_flag_next_to_sneaking() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StartSprinting);
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 08 ff")]
    );

    on_player_input(&mut stage.region, STEVE, SHIFT);
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(
            clientbound::SET_ENTITY_DATA,
            "07 00 00 0a 06 14 05 ff"
        )]
    );

    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StopSprinting);
    stage.step();
    assert_eq!(
        stage.take_received(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 02 ff")]
    );
}

#[test]
fn other_player_commands_change_nothing() {
    let mut stage = Stage::at(POSITION);
    stage.skip_first_cycle();

    for action in [
        PlayerCommandKind::StopSleeping,
        PlayerCommandKind::StartRidingJump,
        PlayerCommandKind::StopRidingJump,
        PlayerCommandKind::OpenInventory,
    ] {
        on_player_command(&mut stage.region, STEVE, action);
    }
    stage.step();

    assert!(stage.take_received().is_empty());
    assert!(stage.flush_own().is_empty());
}

/// `entities.md` 第 10 节的 `[跑]`。
#[test]
fn fall_flying_refusal_uses_the_current_flags_and_only_reaches_the_player() {
    let mut stage = Stage::at(POSITION);

    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StartFallFlying);
    stage.region.links.flush();
    assert_eq!(stage.own.take_bytes(), hex("06 65 07 00 00 00 ff"));

    // 冲刺还没进入周期，回应也要写上它。
    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StartSprinting);
    on_player_command(&mut stage.region, STEVE, PlayerCommandKind::StartFallFlying);
    assert_eq!(
        stage.flush_own(),
        [frame(clientbound::SET_ENTITY_DATA, "07 00 00 08 ff")]
    );
    assert!(stage.flush_watcher().is_empty());
}

/// `entities.md` 第 9 节的 `[跑]`。
#[test]
fn punch_swings_the_main_hand_for_viewers_only() {
    let mut stage = Stage::at(POSITION);

    on_punch(&mut stage.region, STEVE);
    stage.region.links.flush();

    assert_eq!(stage.watcher.take_bytes(), hex("05 7b 07 00 01 06"));
    assert!(stage.flush_own().is_empty());
}

#[test]
fn placing_with_the_off_hand_swings_that_hand() {
    let mut stage = Stage::at(POSITION);

    broadcast_swing(&mut stage.region, STEVE, Hand::Off);

    assert_eq!(
        stage.flush_watcher(),
        [frame(clientbound::SWING_ANIMATION, "07 01 01 06")]
    );
    assert!(stage.flush_own().is_empty());
}

/// `entities.md` 第 6 节。
#[test]
fn leaving_tells_the_viewers_to_remove_the_entity() {
    let mut stage = Stage::at(POSITION);

    on_leave(&mut stage.region, STEVE);

    assert_eq!(
        stage.flush_watcher(),
        [frame(clientbound::REMOVE_ENTITIES, "01 07")]
    );
    assert!(stage.flush_own().is_empty());
}

#[test]
fn requests_for_a_player_that_is_gone_are_ignored() {
    let mut stage = Stage::at(POSITION);
    let gone = PlayerId(99);

    on_punch(&mut stage.region, gone);
    on_player_input(&mut stage.region, gone, SHIFT);
    on_player_abilities(&mut stage.region, gone, FLYING);
    on_player_command(&mut stage.region, gone, PlayerCommandKind::StartSprinting);
    on_player_command(&mut stage.region, gone, PlayerCommandKind::StartFallFlying);
    on_leave(&mut stage.region, gone);

    assert!(stage.flush_watcher().is_empty());
    assert!(stage.flush_own().is_empty());
}

/// `entities.md` 第 11 节的 `[跑]`。
#[test]
fn equipment_change_matches_official_bytes() {
    let mut stage = Stage::at(POSITION);
    stage.player().inventory.slots[OFF_HAND_SLOT] = ItemStack::new(STONE, 1);
    send_equipment_changes(&mut stage.region);
    stage.flush_watcher();

    let inventory = &mut stage.player().inventory;
    inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 64);
    inventory.slots[OFF_HAND_SLOT] = ItemStack::empty();
    inventory.slots[HEAD_SLOT] = ItemStack::new(DIAMOND_HELMET, 1);
    send_equipment_changes(&mut stage.region);

    assert_eq!(
        stage.flush_watcher(),
        [frame(
            clientbound::SET_EQUIPMENT,
            "07
             80 40 01 00 00
             81 00
             05 01 bc 08 00 00"
        )]
    );
    assert!(stage.flush_own().is_empty());
}

#[test]
fn equipment_is_only_sent_for_the_slots_that_changed() {
    let mut stage = Stage::at(POSITION);
    stage.player().inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 64);
    stage.player().inventory.slots[HEAD_SLOT] = ItemStack::new(DIAMOND_HELMET, 1);
    send_equipment_changes(&mut stage.region);
    stage.flush_watcher();

    stage.player().inventory.slots[HOTBAR_SLOT].count = 63;
    send_equipment_changes(&mut stage.region);
    assert_eq!(
        stage.flush_watcher(),
        [frame(clientbound::SET_EQUIPMENT, "07 00 3f 01 00 00")]
    );

    send_equipment_changes(&mut stage.region);
    assert!(stage.flush_watcher().is_empty());
}

#[test]
fn components_reach_the_viewers_and_count_as_a_change() {
    let mut stage = Stage::at(POSITION);
    stage.player().inventory.slots[HOTBAR_SLOT] = ItemStack {
        item: STONE,
        count: 2,
        added: vec![(1, vec![0x10])],
        removed: vec![12],
    };
    send_equipment_changes(&mut stage.region);
    assert_eq!(
        stage.flush_watcher(),
        [frame(
            clientbound::SET_EQUIPMENT,
            "07 00 02 01 01 01 01 10 0c"
        )]
    );

    // 官方 `equipmentHasChanged` 用 `ItemStack.matches` 比，组件变了也要重发。
    stage.player().inventory.slots[HOTBAR_SLOT].added.clear();
    send_equipment_changes(&mut stage.region);
    assert_eq!(
        stage.flush_watcher(),
        [frame(clientbound::SET_EQUIPMENT, "07 00 02 01 00 01 0c")]
    );

    let frames = parse_frames(&pairing_frames(stage.player()));
    assert_eq!(frames[3].body, hex("07 00 02 01 00 01 0c"));
}

#[test]
fn a_stack_with_a_zero_count_is_the_same_as_an_empty_slot() {
    let mut stage = Stage::at(POSITION);

    stage.player().inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 0);
    send_equipment_changes(&mut stage.region);

    assert!(stage.flush_watcher().is_empty());
}

/// 数量不是零的空气照官方 `ItemStack.matches` 算变了一次，发出去的还是一个空槽位。
///
/// 客户端发不出这种物品：`set_creative_mode_slot` 里空气一律变成空物品堆。
#[test]
fn air_with_a_count_counts_as_a_change_once() {
    let mut stage = Stage::at(POSITION);

    stage.player().inventory.slots[OFF_HAND_SLOT] = ItemStack::new(0, 5);
    send_equipment_changes(&mut stage.region);
    assert_eq!(
        stage.flush_watcher(),
        [frame(clientbound::SET_EQUIPMENT, "07 01 00")]
    );

    send_equipment_changes(&mut stage.region);
    assert!(stage.flush_watcher().is_empty());
}

#[test]
fn equipment_changes_without_viewers_are_remembered() {
    let mut stage = Stage::at(POSITION);
    stage.player().sync.seen_by.clear();
    stage.player().inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 1);
    send_equipment_changes(&mut stage.region);

    stage.player().sync.seen_by.insert(WATCHER);
    send_equipment_changes(&mut stage.region);

    assert!(stage.flush_watcher().is_empty());
}

#[test]
fn range_is_sixteen_blocks_per_chunk_up_to_the_tracking_limit() {
    assert_eq!(visible_range(2), 32.0);
    assert_eq!(visible_range(10), 160.0);
    assert_eq!(visible_range(32), 512.0);
    assert_eq!(visible_range(33), 512.0);
}

// 下面是走完整 tick 的多人场景。

fn entity_id(frame: &Frame) -> i32 {
    Reader::new(&frame.body).varint().unwrap()
}

fn added(frames: &[Frame]) -> Vec<i32> {
    frames_with_id(frames, clientbound::ADD_ENTITY)
        .into_iter()
        .map(entity_id)
        .collect()
}

fn removed(frames: &[Frame]) -> Vec<i32> {
    frames_with_id(frames, clientbound::REMOVE_ENTITIES)
        .into_iter()
        .map(|frame| {
            let mut reader = Reader::new(&frame.body);
            assert_eq!(reader.varint(), Ok(1));
            reader.varint().unwrap()
        })
        .collect()
}

/// `add_entity` 里的 x。
fn added_x(frame: &Frame) -> f64 {
    let mut reader = Reader::new(&frame.body);
    reader.varint().unwrap();
    reader.uuid().unwrap();
    reader.varint().unwrap();
    reader.f64().unwrap()
}

/// 一步的 `entity_position_sync` 里的 x。
fn synced_x(frame: &Frame) -> f64 {
    let mut reader = Reader::new(&frame.body);
    for _ in 0..3 {
        reader.varint().unwrap();
    }
    reader.f64().unwrap()
}

fn sees(server: &Harness, viewer: &Client, target: &Client) -> bool {
    server.player(target).sync.seen_by.contains(&viewer.id)
}

fn settle_with(server: &mut Harness, name: &str, view_distance: i8) -> Client {
    let client = server.join_with(name, client_requesting(view_distance));
    server.confirm_teleport(&client);
    server.receive_all_chunks(&client);
    client
}

/// 跑几个 tick 并回确认，留下收到的所有包。`receive_all_chunks` 只留区块。
fn receive_frames(server: &mut Harness, client: &Client, ticks: usize) -> Vec<Frame> {
    let mut received = Vec::new();
    for _ in 0..ticks {
        server.tick();
        let frames = client.take_frames();
        for _ in frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
            server.acknowledge_batch(client, 64.0);
        }
        received.extend(frames);
    }
    received
}

/// 两名站在出生点的玩家，已经互相看见，`onGround` 也发过了。
fn two_players() -> (Harness, Client, Client) {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    server.move_to(&alice, SPAWN_X, SPAWN_Z);
    server.move_to(&bob, SPAWN_X, SPAWN_Z);
    server.tick();
    server.tick();
    assert!(sees(&server, &alice, &bob) && sees(&server, &bob, &alice));
    alice.take_frames();
    bob.take_frames();
    (server, alice, bob)
}

fn assert_info_comes_first(frames: &[Frame], target: &Client) {
    let info = frames
        .iter()
        .position(|frame| {
            frame.id == clientbound::PLAYER_INFO_UPDATE
                && frame.body[0] == 0xff
                && frame
                    .body
                    .windows(16)
                    .any(|part| part == target.uuid.as_bytes())
        })
        .expect("进服时发了 ADD_PLAYER");
    let add = frames
        .iter()
        .position(|frame| frame.id == clientbound::ADD_ENTITY)
        .expect("这一段里配对了");
    assert!(info < add);
    assert_eq!(frames[add].body[1..17], *target.uuid.as_bytes());
    assert_eq!(frames[add - 1].id, clientbound::BUNDLE_DELIMITER);
}

#[test]
fn player_info_comes_before_add_entity_across_two_joins() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    alice.take_frames();

    let bob = server.join("Bob");
    server.confirm_teleport(&bob);
    server.tick();

    // Alice 早就有出生点的区块了，Bob 这时候一个区块也没有。
    let seen_by_alice = alice.take_frames();
    assert_eq!(added(&seen_by_alice), [bob.id.0]);
    assert_info_comes_first(&seen_by_alice, &bob);
    let mut seen_by_bob = bob.take_frames();
    assert!(added(&seen_by_bob).is_empty());

    server.tick();

    seen_by_bob.extend(bob.take_frames());
    assert_eq!(added(&seen_by_bob), [alice.id.0]);
    assert_info_comes_first(&seen_by_bob, &alice);
    assert!(added(&alice.take_frames()).is_empty());
}

#[test]
fn add_entity_uses_the_same_id_as_the_login_of_that_player() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    alice.take_frames();
    let bob = server.join("Bob");

    server.tick();

    let login = bob.take_frames().remove(0);
    assert_eq!(login.id, clientbound::LOGIN);
    let frames = alice.take_frames();
    let add_entity = frames_with_id(&frames, clientbound::ADD_ENTITY)[0];
    assert_eq!(login.body[..4], bob.id.0.to_be_bytes());
    assert_eq!(entity_id(add_entity), bob.id.0);
}

#[test]
fn nobody_is_paired_with_itself() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");

    for _ in 0..5 {
        server.tick();
    }

    assert!(added(&alice.take_frames()).is_empty());
    assert!(server.player(&alice).sync.seen_by.is_empty());
}

#[test]
fn range_depends_on_the_view_distance_of_the_viewer() {
    let mut server = Harness::new(10);
    let alice = settle_with(&mut server, "Alice", 12);
    let bob = settle_with(&mut server, "Bob", 2);
    server.tick();

    let walk = |server: &mut Harness, distance: f64| {
        server.move_to(&bob, SPAWN_X + distance, SPAWN_Z);
        server.receive_all_chunks(&bob);
        (sees(server, &alice, &bob), sees(server, &bob, &alice))
    };

    // Bob 请求 2，范围是 32 格。Alice 请求 12，被服务端限制到 10，范围是 160 格。
    assert_eq!(walk(&mut server, 32.0), (true, true));
    // Alice 的区块还在 Bob 的跟踪范围里，挡住它的只有距离。
    assert_eq!(walk(&mut server, 32.5), (true, false));
    assert_eq!(walk(&mut server, 160.0), (true, false));
    assert_eq!(walk(&mut server, 160.5), (false, false));
}

#[test]
fn distance_ignores_height() {
    let (mut server, alice, bob) = two_players();

    server.request(
        &bob,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(SPAWN_X, 300.0, SPAWN_Z)),
            rotation: None,
            on_ground: false,
        },
    );
    server.tick();

    assert!(sees(&server, &alice, &bob));
    assert!(removed(&alice.take_frames()).is_empty());
}

#[test]
fn pairing_waits_until_the_chunk_of_the_target_was_sent() {
    let mut server = Harness::new(8);
    let alice = settle_with(&mut server, "Alice", 8);
    // 第一批只有出生点周围 3×3，Alice 站在它外面的区块里。
    server.move_to(&alice, SPAWN_X + 48.0, SPAWN_Z);
    server.receive_all_chunks(&alice);
    alice.take_frames();
    let chunk_of_alice = ChunkPos::new(3, 0);

    let bob = server.join_with("Bob", client_requesting(8));
    server.confirm_teleport(&bob);
    for _ in 0..10 {
        server.tick();
    }

    let waiting = bob.take_frames();
    assert!(!received_chunks(&waiting).contains(&chunk_of_alice));
    assert!(added(&waiting).is_empty());
    assert!(!sees(&server, &bob, &alice));
    // 反方向不受影响，Alice 早就有出生点的区块了。
    assert_eq!(added(&alice.take_frames()), [bob.id.0]);

    server.acknowledge_batch(&bob, 64.0);
    server.tick();

    // 区块批次排在 tick 的最后，这一 tick 的配对已经过去了。
    let arrived = bob.take_frames();
    assert!(received_chunks(&arrived).contains(&chunk_of_alice));
    assert!(added(&arrived).is_empty());

    server.tick();

    assert_eq!(added(&bob.take_frames()), [alice.id.0]);
}

#[test]
fn walking_into_a_chunk_the_viewer_is_still_waiting_for_removes_the_entity() {
    let mut server = Harness::new(8);
    let alice = settle_with(&mut server, "Alice", 8);
    let bob = server.join_with("Bob", client_requesting(8));
    server.confirm_teleport(&bob);
    server.tick();
    server.tick();
    assert!(sees(&server, &bob, &alice));
    bob.take_frames();

    // Bob 只收到了第一批 3×3，这个区块还在 `pending`。
    server.move_to(&alice, SPAWN_X + 48.0, SPAWN_Z);
    server.tick();

    assert_eq!(removed(&bob.take_frames()), [alice.id.0]);
    assert!(!sees(&server, &bob, &alice));
}

#[test]
fn walking_out_of_range_and_back_reintroduces_the_entity() {
    let (mut server, alice, bob) = two_players();

    server.move_to(&bob, SPAWN_X + 100.0, SPAWN_Z);
    server.tick();

    assert_eq!(removed(&alice.take_frames()), [bob.id.0]);
    assert_eq!(removed(&bob.take_frames()), [alice.id.0]);
    assert!(!sees(&server, &alice, &bob) && !sees(&server, &bob, &alice));

    // 没有人看着的时候同步也在继续，这一段走完 `base` 已经在远处了。
    let far_away = receive_frames(&mut server, &bob, 4);
    assert!(added(&far_away).is_empty());
    assert!(alice.take_frames().is_empty());

    server.move_to(&bob, SPAWN_X, SPAWN_Z);
    let back = receive_frames(&mut server, &bob, 4);

    let frames = alice.take_frames();
    assert_eq!(added(&frames), [bob.id.0]);
    let add_entity = frames_with_id(&frames, clientbound::ADD_ENTITY)[0];
    assert_eq!(added_x(add_entity), SPAWN_X + 100.0);
    // 新观察者从上次发出去的位置开始，全量同步把它带回来。
    let sync = frames_with_id(&frames, clientbound::ENTITY_POSITION_SYNC)[0];
    assert_eq!(entity_id(sync), bob.id.0);
    assert_eq!(synced_x(sync), SPAWN_X);
    assert_eq!(added(&back), [alice.id.0]);
    assert!(removed(&frames).is_empty());
}

#[test]
fn leaving_while_watched_removes_the_entity_before_the_tab_entry() {
    let (mut server, alice, bob) = two_players();

    server.leave(&bob);
    server.tick();

    let frames = alice.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::SYSTEM_CHAT,
            clientbound::REMOVE_ENTITIES,
            clientbound::PLAYER_INFO_REMOVE,
        ]
    );
    assert_eq!(removed(&frames), [bob.id.0]);
    // Bob 也是 Alice 的观察者，走了之后不能留在她的 `seen_by` 里。
    assert!(server.player(&alice).sync.seen_by.is_empty());
}

#[test]
fn leaving_unseen_tells_nobody_to_remove_an_entity() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    server.move_to(&bob, SPAWN_X + 100.0, SPAWN_Z);
    server.tick();
    alice.take_frames();

    server.leave(&bob);
    server.tick();

    assert!(removed(&alice.take_frames()).is_empty());
}

#[test]
fn rejoining_gets_a_new_entity_for_everyone() {
    let (mut server, alice, bob) = two_players();

    server.leave(&bob);
    let carol = server.join("Carol");
    server.tick();

    let frames = alice.take_frames();
    assert_eq!(removed(&frames), [bob.id.0]);
    assert_eq!(added(&frames), [carol.id.0]);
}

#[test]
fn metadata_reaches_the_viewers_and_the_player_itself() {
    let (mut server, alice, bob) = two_players();

    server.request(&bob, PlayRequest::PlayerInput { flags: SHIFT });
    server.tick();

    let id = bob.id.0 as u8;
    for receiver in [&alice, &bob] {
        let frames = receiver.take_frames();
        let data = frames_with_id(&frames, clientbound::SET_ENTITY_DATA);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].body, [id, 0x00, 0x00, 0x02, 0x06, 0x14, 0x05, 0xff]);
    }
}

#[test]
fn changed_client_information_reaches_the_viewers_and_the_player_itself() {
    let (mut server, alice, bob) = two_players();

    server.request(
        &bob,
        PlayRequest::ClientInformation(ClientInformation {
            main_hand: HumanoidArm::Right,
            ..client_requesting(2)
        }),
    );
    server.tick();

    let id = bob.id.0 as u8;
    for receiver in [&alice, &bob] {
        let frames = receiver.take_frames();
        let data = frames_with_id(&frames, clientbound::SET_ENTITY_DATA);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].body, [id, 0x0f, 0x2a, 0x01, 0xff]);
    }
    assert!(!server.player(&bob).sync.metadata_dirty);
}

#[test]
fn new_viewer_gets_the_metadata_set_before_it_arrived() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    server.request(&alice, PlayRequest::PlayerInput { flags: SHIFT });
    server.tick();

    let bob = server.join("Bob");
    server.confirm_teleport(&bob);
    let frames = receive_frames(&mut server, &bob, 2);

    let index = frames
        .iter()
        .position(|frame| frame.id == clientbound::ADD_ENTITY)
        .expect("Bob 看得见 Alice");
    let id = alice.id.0 as u8;
    assert_eq!(frames[index + 1].id, clientbound::SET_ENTITY_DATA);
    assert_eq!(
        frames[index + 1].body,
        [
            &[id, 0x00, 0x00, 0x02, 0x06, 0x14, 0x05][..],
            &[0x09, 0x03, 0x41, 0xa0, 0x00, 0x00],
            &[0x0f, 0x2a, 0x00, 0x10, 0x00, 0x7f, 0xff],
        ]
        .concat()
    );
}

#[test]
fn delta_overflow_falls_back_to_entity_position_sync() {
    let (mut server, alice, bob) = two_players();

    server.move_to(&bob, SPAWN_X + 10.0, SPAWN_Z);
    server.tick();
    server.tick();

    let frames = alice.take_frames();
    let syncs = frames_with_id(&frames, clientbound::ENTITY_POSITION_SYNC);
    assert_eq!(syncs.len(), 1);
    assert_eq!(entity_id(syncs[0]), bob.id.0);
    assert_eq!(synced_x(syncs[0]), SPAWN_X + 10.0);
    assert!(frames_with_id(&frames, clientbound::MOVE_ENTITY_POS).is_empty());
}

#[test]
fn walking_sends_small_deltas() {
    let (mut server, alice, bob) = two_players();

    server.move_to(&bob, SPAWN_X + 0.25, SPAWN_Z);
    server.tick();
    server.tick();

    let frames = alice.take_frames();
    let moves = frames_with_id(&frames, clientbound::MOVE_ENTITY_POS);
    assert_eq!(moves.len(), 1);
    let id = bob.id.0 as u8;
    assert_eq!(moves[0].body[..2], [id, 0x03]);
    assert_eq!(moves[0].body[3..], [0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn moves_below_the_threshold_send_nothing_and_keep_the_base() {
    let (mut server, alice, bob) = two_players();
    let base = server.player(&bob).sync.base;

    server.move_to(&bob, SPAWN_X + 0.002, SPAWN_Z);
    server.tick();
    server.tick();

    let frames = alice.take_frames();
    assert!(frames_with_id(&frames, clientbound::MOVE_ENTITY_POS).is_empty());
    assert_eq!(server.player(&bob).sync.base, base);
}

#[test]
fn turning_reaches_the_viewers_as_rotation_and_head() {
    let (mut server, alice, bob) = two_players();

    server.request(
        &bob,
        PlayRequest::MovePlayer {
            position: None,
            rotation: Some((90.0, 0.0)),
            on_ground: true,
        },
    );
    server.tick();
    server.tick();

    let frames = alice.take_frames();
    let id = bob.id.0 as u8;
    let turn = frames_with_id(&frames, clientbound::MOVE_ENTITY_ROT);
    assert_eq!(turn[0].body, [id, 0x01, 0x40, 0x00]);
    let head = frames_with_id(&frames, clientbound::ROTATE_HEAD);
    assert_eq!(head[0].body, [id, 0x40]);
    assert!(frames_with_id(&bob.take_frames(), clientbound::MOVE_ENTITY_ROT).is_empty());
}

#[test]
fn every_viewer_gets_the_same_bytes() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    let carol = server.join_and_settle("Carol");
    server.tick();
    server.tick();
    alice.take_frames();
    carol.take_frames();

    server.request(
        &bob,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(SPAWN_X + 0.25, SPAWN_Y, SPAWN_Z)),
            rotation: Some((45.0, 10.0)),
            on_ground: true,
        },
    );
    server.request(&bob, PlayRequest::Punch);
    server.tick();
    server.tick();

    let about_bob = |client: &Client| -> Vec<Frame> {
        client
            .take_frames()
            .into_iter()
            .filter(|frame| Reader::new(&frame.body).varint() == Ok(bob.id.0))
            .collect()
    };
    let seen_by_alice = about_bob(&alice);
    assert!(seen_by_alice.len() >= 3);
    assert_eq!(seen_by_alice, about_bob(&carol));
}

#[test]
fn punching_reaches_viewers_but_not_the_puncher() {
    let (mut server, alice, bob) = two_players();

    server.request(&bob, PlayRequest::Punch);
    server.tick();

    let frames = alice.take_frames();
    let swings = frames_with_id(&frames, clientbound::SWING_ANIMATION);
    assert_eq!(swings[0].body, [bob.id.0 as u8, 0x00, 0x01, 0x06]);
    assert!(frames_with_id(&bob.take_frames(), clientbound::SWING_ANIMATION).is_empty());
}

#[test]
fn switching_the_held_item_updates_the_main_hand_for_viewers_only() {
    let (mut server, alice, bob) = two_players();
    let player = server.region().players.get_mut(&bob.id).unwrap();
    player.inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 64);
    player.inventory.slots[HOTBAR_SLOT + 1] = ItemStack::new(DIAMOND_HELMET, 1);
    server.tick();
    let id = bob.id.0 as u8;
    let frames = alice.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::SET_EQUIPMENT)[0].body,
        [id, 0x00, 0x40, 0x01, 0x00, 0x00]
    );

    server.request(&bob, PlayRequest::SetCarriedItem { slot: 1 });
    server.tick();

    let frames = alice.take_frames();
    let equipment = frames_with_id(&frames, clientbound::SET_EQUIPMENT);
    assert_eq!(equipment.len(), 1);
    assert_eq!(equipment[0].body, [id, 0x00, 0x01, 0xbc, 0x08, 0x00, 0x00]);
    assert!(frames_with_id(&bob.take_frames(), clientbound::SET_EQUIPMENT).is_empty());
}

#[test]
fn new_viewer_gets_the_equipment_in_the_pairing_bundle() {
    let mut server = Harness::new(8);
    let alice = server.join_and_settle("Alice");
    let player = server.region().players.get_mut(&alice.id).unwrap();
    player.inventory.slots[HOTBAR_SLOT] = ItemStack::new(STONE, 64);
    server.tick();

    let bob = server.join("Bob");
    server.confirm_teleport(&bob);
    let frames = receive_frames(&mut server, &bob, 2);

    let index = frames
        .iter()
        .position(|frame| frame.id == clientbound::ADD_ENTITY)
        .expect("Bob 看得见 Alice");
    assert_eq!(frames[index + 2].id, clientbound::SET_EQUIPMENT);
    assert_eq!(
        frames[index + 2].body,
        [alice.id.0 as u8, 0x00, 0x40, 0x01, 0x00, 0x00]
    );
    assert_eq!(frames[index + 3].id, clientbound::BUNDLE_DELIMITER);
}

#[test]
fn kicked_viewer_stays_harmless_until_it_leaves() {
    let (mut server, alice, bob) = two_players();

    server.request(
        &alice,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(f64::NAN, 0.0, 0.0)),
            rotation: None,
            on_ground: false,
        },
    );
    server.request(&bob, PlayRequest::Punch);
    server.move_to(&bob, SPAWN_X + 0.5, SPAWN_Z);
    server.tick();
    server.tick();
    assert!(alice.is_closed());

    server.leave(&alice);
    server.tick();

    assert!(server.player(&bob).sync.seen_by.is_empty());
}

#[test]
fn world_borders_do_not_break_the_sync() {
    let (mut server, alice, bob) = two_players();

    for (x, z) in [
        (3.0e7, 3.0e7),
        (-3.0e7, -3.0e7),
        (f64::INFINITY, f64::NEG_INFINITY),
        (SPAWN_X, SPAWN_Z),
    ] {
        server.move_to(&bob, x, z);
        server.tick();
        server.tick();
    }

    assert!(!alice.is_closed() && !bob.is_closed());
}
